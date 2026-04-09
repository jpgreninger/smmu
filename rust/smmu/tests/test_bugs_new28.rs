//! TDD failing tests for BUG-AUDIT-65 through BUG-AUDIT-68.
//!
//! **BUG-AUDIT-65** (Both §4.7.3): CMD_SYNC CS=0b10 (SIG_SEV) not gated on IDR0.SEV.
//!   Spec §4.7.3: "Use of SIG_SEV only sends an event when SMMU_IDR0.SEV is set,
//!   otherwise, no event is available to a PE and SIG_SEV is equivalent to SIG_NONE."
//!   BEFORE FIX: `sev_supported=false` is ignored; `cmd_sync_last_signal_type` is stored
//!               as 2 (SEV) and `CommandSyncCompletion` event is generated unconditionally.
//!   AFTER FIX:  when `sev_supported=false`, CS=2 is treated as SIG_NONE — no signal
//!               stored, no completion event generated.
//!
//! **BUG-AUDIT-66** (Both §6.3.11): `set_cr1()` uses a single combined guard
//!   (`SMMUEN | CMDQEN | EVENTQEN | PRIQEN`) for both TABLE_* and QUEUE_* fields.
//!   Spec §6.3.11:
//!     TABLE_* (bits\[11:6\]): RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
//!     QUEUE_* (bits\[5:0\]):  RO when ANY of CMDQEN/EVENTQEN/PRIQEN is 1. Not gated by SMMUEN.
//!   BEFORE FIX:
//!     (a) SMMUEN=1 + all queues disabled → QUEUE_* writes blocked (wrong).
//!     (b) SMMUEN=0 + queues enabled → TABLE_* writes blocked (wrong).
//!   AFTER FIX:
//!     (a) QUEUE_* only blocked by queue-enable bits; SMMUEN alone does not block them.
//!     (b) TABLE_* only blocked by SMMUEN; queue enables alone do not block them.
//!
//! **BUG-AUDIT-67** (Both §6.3.12/6.3.24/6.3.25): `set_cr2`/`set_strtab_*` check only
//!   CR0.SMMUEN; spec requires also checking CR0ACK.SMMUEN. Functionally benign in the
//!   synchronous model where CR0ACK always mirrors CR0. Tests verify the existing guard
//!   and document spec intent.
//!
//! **BUG-AUDIT-68** (Both §6.3.11): `set_cr1()` TABLE_* guard missing CR0ACK.SMMUEN.
//!   Spec: TABLE_* is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
//!   Current code only checks CR0.SMMUEN. Functionally benign in sync model.
//!   Tests verify the existing CR0.SMMUEN guard and document the missing CR0ACK check.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{CommandEntry, CommandType, EventType};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build a fresh SMMU with SMMUEN=1, CMDQEN=1, EVENTQEN=1, PRIQEN=1.
fn make_enabled_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Submit CMD_SYNC with the given CS value and process the queue.
fn submit_sync(smmu: &SMMU, cs: u8) {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = cs;
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");
}

/// Count COMMAND_SYNC_COMPLETION events in the event queue.
fn count_sync_completion_events(smmu: &SMMU) -> usize {
    smmu.get_events()
        .into_iter()
        .filter(|e| e.event_type == EventType::CommandSyncCompletion)
        .count()
}

// ============================================================================
// BUG-AUDIT-65: CMD_SYNC SIG_SEV must be gated on IDR0.SEV
// ============================================================================

/// BUG-AUDIT-65 test 1: CS=2 (SIG_SEV) with sev_supported=false must behave as SIG_NONE.
///
/// ARM §4.7.3: "Use of SIG_SEV only sends an event when SMMU_IDR0.SEV is set,
/// otherwise, no event is available to a PE and SIG_SEV is equivalent to SIG_NONE."
///
/// When IDR0.SEV==0 (set_sev_supported(false)), CMD_SYNC with CS=2 must:
///   - NOT store 2 in `cmd_sync_last_signal_type`.
///   - NOT generate a `CommandSyncCompletion` event.
///
/// BEFORE FIX: `cmd_sync_last_signal_type` is set to 2 and completion event generated → FAILS.
/// AFTER FIX:  CS=2 treated as SIG_NONE → signal type unchanged (0), no event → PASSES.
#[test]
fn bug_audit_65_sev_cs2_with_sev_unsupported_is_noop() {
    let smmu = make_enabled_smmu();

    // IDR0.SEV=0 (default — confirm it).
    smmu.set_sev_supported(false);
    assert_eq!(
        (smmu.get_idr0() >> 14) & 1,
        0,
        "BUG-AUDIT-65 precondition: IDR0.SEV must be 0 when set_sev_supported(false)"
    );

    // Clear any pre-existing events.
    smmu.clear_event_queue();

    // Submit CMD_SYNC with CS=2 (SIG_SEV).
    submit_sync(&smmu, 2);

    // Signal type must NOT have been updated to 2 (SEV).
    let sig = smmu.get_cmd_sync_last_signal_type();
    assert_ne!(
        sig, 2,
        "BUG-AUDIT-65: CS=2 with IDR0.SEV==0 must NOT set signal type to 2 (SEV). \
         ARM §4.7.3: SIG_SEV is equivalent to SIG_NONE when IDR0.SEV=0. \
         Current code stores 2 unconditionally (ignores sev_supported). \
         got={}",
        sig
    );

    // No CommandSyncCompletion event must have been generated.
    let completions = count_sync_completion_events(&smmu);
    assert_eq!(
        completions, 0,
        "BUG-AUDIT-65: CS=2 with IDR0.SEV==0 must NOT generate CommandSyncCompletion. \
         ARM §4.7.3: SIG_SEV == SIG_NONE when IDR0.SEV=0; no event should be written. \
         Current code generates the completion event unconditionally. \
         completions={}",
        completions
    );
}

/// BUG-AUDIT-65 test 2: CS=2 (SIG_SEV) with sev_supported=true must behave normally.
///
/// When IDR0.SEV==1 (set_sev_supported(true)), CMD_SYNC with CS=2 must:
///   - Store 2 in `cmd_sync_last_signal_type`.
///   - Generate a `CommandSyncCompletion` event.
///
/// BEFORE FIX: this test PASSES (2 is always stored regardless of sev_supported).
/// AFTER FIX:  this test still PASSES (now gated correctly on sev_supported=true).
#[test]
fn bug_audit_65_sev_cs2_with_sev_supported_generates_event() {
    let smmu = make_enabled_smmu();

    // Enable IDR0.SEV.
    smmu.set_sev_supported(true);
    assert_ne!(
        (smmu.get_idr0() >> 14) & 1,
        0,
        "BUG-AUDIT-65 precondition: IDR0.SEV must be 1 when set_sev_supported(true)"
    );

    smmu.clear_event_queue();

    // Submit CMD_SYNC with CS=2 (SIG_SEV).
    submit_sync(&smmu, 2);

    // Signal type must be 2 (SEV).
    let sig = smmu.get_cmd_sync_last_signal_type();
    assert_eq!(
        sig, 2,
        "BUG-AUDIT-65: CS=2 with IDR0.SEV==1 must store signal type 2 (SEV). \
         ARM §4.7.3: SIG_SEV is operational when IDR0.SEV=1. \
         got={}",
        sig
    );

    // Exactly one CommandSyncCompletion event must have been generated.
    let completions = count_sync_completion_events(&smmu);
    assert_eq!(
        completions, 1,
        "BUG-AUDIT-65: CS=2 with IDR0.SEV==1 must generate exactly one \
         CommandSyncCompletion event. ARM §4.7.3. \
         completions={}",
        completions
    );
}

/// BUG-AUDIT-65 test 3: CS=1 (SIG_IRQ) must always work regardless of sev_supported.
///
/// SIG_IRQ is not affected by IDR0.SEV — it must always generate a completion event.
/// This test verifies no regression from the BUG-AUDIT-65 fix.
///
/// BEFORE / AFTER FIX: CS=1 always works → PASSES.
#[test]
fn bug_audit_65_irq_cs1_always_works_regardless_of_sev_supported() {
    let smmu = make_enabled_smmu();

    // IDR0.SEV=0.
    smmu.set_sev_supported(false);
    smmu.clear_event_queue();

    // Submit CMD_SYNC with CS=1 (SIG_IRQ).
    submit_sync(&smmu, 1);

    // Signal type must be 1 (IRQ).
    let sig = smmu.get_cmd_sync_last_signal_type();
    assert_eq!(
        sig, 1,
        "BUG-AUDIT-65 regression: CS=1 (SIG_IRQ) must always store signal type 1 (IRQ) \
         regardless of IDR0.SEV. ARM §4.7.3: SIG_IRQ is independent of SEV. \
         got={}",
        sig
    );

    // Exactly one completion event must be generated.
    let completions = count_sync_completion_events(&smmu);
    assert_eq!(
        completions, 1,
        "BUG-AUDIT-65 regression: CS=1 (SIG_IRQ) must generate exactly one \
         CommandSyncCompletion event regardless of IDR0.SEV. \
         completions={}",
        completions
    );
}

// ============================================================================
// BUG-AUDIT-66: set_cr1() split guard for TABLE_* vs QUEUE_* fields
// ============================================================================

/// BUG-AUDIT-66 test 4: QUEUE_* fields must be writable when SMMUEN=1 but all
/// queue-enables are cleared.
///
/// ARM §6.3.11: QUEUE_* (bits\[5:0\]) are RO only when CMDQEN=1 OR EVENTQEN=1 OR PRIQEN=1.
/// SMMUEN alone must NOT prevent QUEUE_* writes.
///
/// Scenario (updated for BUG-AUDIT-124): ARM §6.3.9.3/§6.3.9.4 makes queue-enable
/// bits RO-while-set, so once CMDQEN/EVENTQEN/PRIQEN are set to 1 they cannot be
/// cleared by set_cr0().  The correct way to obtain SMMUEN=1 / all-queue-enables=0
/// is to start from reset (where all bits are 0) and write only SMMUEN=1.
///
/// BEFORE FIX: combined guard (`SMMUEN | CMDQEN | EVENTQEN | PRIQEN`) blocks the
///             entire write when SMMUEN=1 even though no queues are enabled → FAILS.
/// AFTER FIX:  QUEUE_* written when all queue-enables are cleared, even if SMMUEN=1
///             → PASSES.
#[test]
fn bug_audit_66_queue_fields_writable_when_smmuen_set_but_queues_disabled() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);

    // Set CR1 to 0 before enabling (all bits are 0 at reset anyway; explicit for clarity).
    smmu.set_cr1(0);
    assert_eq!(
        smmu.get_cr1(),
        0,
        "BUG-AUDIT-66 precondition: set_cr1(0) must succeed before enable"
    );

    // From reset (all queue-enables=0), set only SMMUEN=1.
    // Per BUG-AUDIT-124/§6.3.9.3, queue-enable bits are only RO-while-set; since
    // they start at 0 (reset value), omitting them here leaves them at 0.
    smmu.set_cr0(SMMU::CR0_SMMUEN);
    assert_eq!(
        smmu.get_cr0() & (SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN),
        0,
        "BUG-AUDIT-66 precondition: all queue-enables must be 0 (never set from reset)"
    );
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-66 precondition: SMMUEN must be 1"
    );

    // Attempt to write QUEUE_SH (bits[5:4]).
    let queue_sh_val = SMMU::CR1_QUEUE_SH; // 0x30
    smmu.set_cr1(queue_sh_val);

    // QUEUE_* must have been written because no queue is enabled.
    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & SMMU::CR1_QUEUE_SH,
        queue_sh_val,
        "BUG-AUDIT-66: QUEUE_* bits must be writable when SMMUEN=1 but all queue-enables \
         are cleared. ARM §6.3.11: QUEUE_* is RO only when a queue-enable bit is set. \
         SMMUEN alone must not block QUEUE_* writes. \
         cr1=0x{:08X}",
        cr1
    );
}

/// BUG-AUDIT-66 test 5: TABLE_* fields must be writable when queues are enabled
/// but SMMUEN=0.
///
/// ARM §6.3.11: TABLE_* (bits\[11:6\]) are RO only when SMMUEN=1 OR CR0ACK.SMMUEN=1.
/// Queue-enables alone (with SMMUEN=0) must NOT prevent TABLE_* writes.
///
/// Scenario: Set CR1=0 with no enables. Enable CMDQEN+EVENTQEN (not SMMUEN).
/// Attempt to write TABLE_SH. The write must succeed.
///
/// BEFORE FIX: combined guard blocks write because CMDQEN+EVENTQEN are set → FAILS.
/// AFTER FIX:  TABLE_* written because SMMUEN=0 → PASSES.
#[test]
fn bug_audit_66_table_fields_writable_when_queues_enabled_but_smmuen_cleared() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);

    // Set CR1 to 0 before any enables.
    smmu.set_cr1(0);
    assert_eq!(
        smmu.get_cr1(),
        0,
        "BUG-AUDIT-66 precondition: set_cr1(0) must succeed before enabling"
    );

    // Enable CMDQEN and EVENTQEN but NOT SMMUEN.
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-66 precondition: SMMUEN must be 0"
    );
    assert_ne!(
        smmu.get_cr0() & (SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN),
        0,
        "BUG-AUDIT-66 precondition: CMDQEN+EVENTQEN must be set"
    );

    // Attempt to write TABLE_SH (bits[11:10]).
    let table_sh_val = SMMU::CR1_TABLE_SH; // 0xC00
    smmu.set_cr1(table_sh_val);

    // TABLE_* must have been written because SMMUEN=0.
    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & SMMU::CR1_TABLE_SH,
        table_sh_val,
        "BUG-AUDIT-66: TABLE_* bits must be writable when SMMUEN=0, even if queue-enable \
         bits are set. ARM §6.3.11: TABLE_* is RO only when SMMUEN=1. \
         Queue enables alone must not block TABLE_* writes. \
         Current code uses a combined guard that incorrectly blocks TABLE_* when CMDQEN \
         or EVENTQEN is set regardless of SMMUEN. \
         cr1=0x{:08X}",
        cr1
    );
}

// ============================================================================
// BUG-AUDIT-67: setCR2/STRTAB guards — CR0ACK.SMMUEN check
// ============================================================================

/// BUG-AUDIT-67 test 6: `set_cr2()` guard — verify CR0.SMMUEN blocks the write.
///
/// ARM §6.3.12: SMMU_CR2 is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
/// In the synchronous model, CR0ACK always mirrors CR0, so this tests the
/// existing CR0.SMMUEN guard (which fires CR0ACK guard implicitly).
///
/// BEFORE / AFTER FIX: `set_cr2()` while SMMUEN=1 is blocked → PASSES.
#[test]
fn bug_audit_67_set_cr2_blocked_by_smmuen() {
    let smmu = SMMU::new();

    // Set CR2_PTM before enabling.
    smmu.set_cr2(SMMU::CR2_PTM);
    assert_eq!(
        smmu.get_cr2() & SMMU::CR2_PTM,
        SMMU::CR2_PTM,
        "BUG-AUDIT-67 precondition: set_cr2(CR2_PTM) must succeed before enable"
    );

    // Enable SMMU (sets CR0.SMMUEN=1, which also sets CR0ACK.SMMUEN=1 in sync model).
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);

    // Attempt to clear CR2 while SMMUEN=1 — must be silently ignored.
    smmu.set_cr2(0);

    let cr2 = smmu.get_cr2();
    assert_eq!(
        cr2 & SMMU::CR2_PTM,
        SMMU::CR2_PTM,
        "BUG-AUDIT-67: set_cr2() must be silently ignored when CR0.SMMUEN=1. \
         ARM §6.3.12: SMMU_CR2 is RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. \
         cr2=0x{:08X}",
        cr2
    );
}

/// BUG-AUDIT-67 test 7: `set_cr2()` is writable when SMMUEN=0 and CR0ACK.SMMUEN=0.
///
/// When both CR0.SMMUEN and CR0ACK.SMMUEN are 0, set_cr2() must succeed.
///
/// BEFORE / AFTER FIX: set_cr2() while SMMUEN=0 succeeds → PASSES.
#[test]
fn bug_audit_67_set_cr2_writable_when_smmuen_cleared() {
    let smmu = SMMU::new();

    // Enable then disable to confirm SMMUEN=0.
    smmu.set_cr0(SMMU::CR0_SMMUEN);
    smmu.set_cr0(0);
    assert_eq!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-67 precondition: SMMUEN must be 0"
    );
    assert_eq!(
        smmu.get_cr0ack() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-67 precondition: CR0ACK.SMMUEN must be 0 (sync model mirrors CR0)"
    );

    // Write CR2 — must succeed.
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    let cr2 = smmu.get_cr2();
    assert_eq!(
        cr2 & SMMU::CR2_RECINVSID,
        SMMU::CR2_RECINVSID,
        "BUG-AUDIT-67: set_cr2() must succeed when CR0.SMMUEN=0 and CR0ACK.SMMUEN=0. \
         ARM §6.3.12: SMMU_CR2 is only RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. \
         cr2=0x{:08X}",
        cr2
    );
}

/// BUG-AUDIT-67 test 8: CR0ACK mirrors CR0 after set_cr0() in the synchronous model.
///
/// Verifies that get_cr0ack() reflects what set_cr0() wrote. Also verifies
/// that reset_cr0ack() independently clears CR0ACK (for future async testing).
///
/// BEFORE / AFTER FIX: CR0ACK mirrors CR0 after set_cr0() → PASSES.
#[test]
fn bug_audit_67_cr0ack_mirrors_cr0_after_set_cr0() {
    let smmu = SMMU::new();

    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    let cr0 = smmu.get_cr0();
    let cr0ack = smmu.get_cr0ack();

    assert_eq!(
        cr0 & SMMU::CR0_SMMUEN,
        cr0ack & SMMU::CR0_SMMUEN,
        "BUG-AUDIT-67: CR0ACK.SMMUEN must mirror CR0.SMMUEN in the synchronous model. \
         ARM §6.3.10: SMMU_CR0ACK is the hardware acknowledgement of SMMU_CR0. \
         cr0=0x{:08X} cr0ack=0x{:08X}",
        cr0,
        cr0ack
    );

    // reset_cr0ack() must independently clear CR0ACK.
    smmu.reset_cr0ack();
    assert_eq!(
        smmu.get_cr0ack() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-67: reset_cr0ack() must clear CR0ACK.SMMUEN. \
         This API enables future testing of async CR0ACK update paths."
    );
}

// ============================================================================
// BUG-AUDIT-68: set_cr1() TABLE_* guard missing CR0ACK.SMMUEN check
// ============================================================================

/// BUG-AUDIT-68 test 9: TABLE_* write guard checks CR0.SMMUEN.
///
/// ARM §6.3.11: TABLE_* (bits\[11:6\]) are RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
/// Current code only checks CR0.SMMUEN. In the synchronous model CR0ACK always
/// mirrors CR0, so this is functionally benign.
///
/// This test verifies that set_cr1() TABLE_* write is blocked when CR0.SMMUEN=1.
///
/// BEFORE / AFTER FIX: TABLE_* write blocked when SMMUEN=1 → PASSES.
#[test]
fn bug_audit_68_table_fields_blocked_by_smmuen() {
    let smmu = SMMU::new();

    // Set CR1 to 0 before enabling.
    smmu.set_cr1(0);
    assert_eq!(
        smmu.get_cr1(),
        0,
        "BUG-AUDIT-68 precondition: set_cr1(0) must succeed before enable"
    );

    // Enable SMMU.
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);

    // Attempt to write TABLE_SH while SMMUEN=1 — must be blocked.
    smmu.set_cr1(SMMU::CR1_TABLE_SH);

    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & SMMU::CR1_TABLE_SH,
        0,
        "BUG-AUDIT-68: TABLE_* bits must be RO when CR0.SMMUEN=1. \
         ARM §6.3.11: TABLE_* is RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. \
         cr1=0x{:08X}",
        cr1
    );
}

/// BUG-AUDIT-68 test 10: TABLE_* guard behavior with CR0ACK.SMMUEN cleared via
/// `reset_cr0ack()` — exposing the missing CR0ACK check.
///
/// ARM §6.3.11: TABLE_* is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
/// Current code checks only CR0.SMMUEN, missing the CR0ACK.SMMUEN check.
///
/// Scenario: Enable SMMU (CR0.SMMUEN=1, CR0ACK.SMMUEN=1). Reset CR0ACK to 0
/// via reset_cr0ack() while CR0.SMMUEN remains 1. Attempt TABLE_* write.
/// The spec says the write must STILL be blocked because CR0.SMMUEN=1.
///
/// This test verifies that CR0.SMMUEN alone is sufficient to block TABLE_* writes
/// (the missing CR0ACK check cannot make things LESS restrictive in this scenario).
///
/// BEFORE / AFTER FIX: TABLE_* write blocked when CR0.SMMUEN=1 regardless of CR0ACK
/// → PASSES.
///
/// Note: The inverse scenario (CR0.SMMUEN=0 but CR0ACK.SMMUEN=1 should still block)
/// cannot be tested in Rust because there is no `set_cr0ack()` API to set CR0ACK to
/// a specific non-zero value independently; only `reset_cr0ack()` is available.
/// The C++ side exposes `setCR0ACK()` which enables full testing of this path.
#[test]
fn bug_audit_68_table_fields_blocked_when_cr0_smmuen_set_regardless_of_cr0ack() {
    let smmu = SMMU::new();

    smmu.set_cr1(0);
    assert_eq!(
        smmu.get_cr1(),
        0,
        "BUG-AUDIT-68 precondition: set_cr1(0) must succeed"
    );

    // Enable SMMU (CR0.SMMUEN=1, CR0ACK.SMMUEN=1).
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-68 precondition: CR0.SMMUEN must be 1"
    );
    assert_ne!(
        smmu.get_cr0ack() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-68 precondition: CR0ACK.SMMUEN must be 1"
    );

    // Reset CR0ACK to 0 (simulates async hardware delay where CR0ACK not yet updated).
    // Note: CR0.SMMUEN is still 1.
    smmu.reset_cr0ack();
    assert_eq!(
        smmu.get_cr0ack() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-68: after reset_cr0ack(), CR0ACK.SMMUEN must be 0"
    );
    assert_ne!(
        smmu.get_cr0() & SMMU::CR0_SMMUEN,
        0,
        "BUG-AUDIT-68: CR0.SMMUEN must still be 1 after reset_cr0ack()"
    );

    // Attempt TABLE_* write — must be blocked because CR0.SMMUEN=1.
    smmu.set_cr1(SMMU::CR1_TABLE_SH);

    let cr1 = smmu.get_cr1();
    assert_eq!(
        cr1 & SMMU::CR1_TABLE_SH,
        0,
        "BUG-AUDIT-68: TABLE_* bits must be RO when CR0.SMMUEN=1. \
         Current code correctly checks CR0.SMMUEN (but is missing the CR0ACK check). \
         The missing CR0ACK check (CR0ACK.SMMUEN=1 with CR0.SMMUEN=0) cannot be tested \
         in Rust due to absence of set_cr0ack() API; see C++ test for full coverage. \
         cr1=0x{:08X}",
        cr1
    );
}
