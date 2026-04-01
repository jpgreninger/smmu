//! TDD failing tests for BUG-QA-7 through BUG-QA-10 (Rust implementation).
//!
//! Each test is written to FAIL with the current buggy code and PASS only
//! after the corresponding fix is applied.  No fixes are included here.
//!
//! # Bug Summary
//!
//! ## BUG-QA-7 — CMD_SYNC CS=0b10 labeled as MSI instead of SEV (§4.7.3)
//!
//! The spec defines:
//! - CS=0b00 = SIG_NONE
//! - CS=0b01 = SIG_IRQ
//! - CS=0b10 = SIG_SEV (PE-level wakeup event)
//! - CS=0b11 = Reserved → CERROR_ILL
//!
//! The comment on `get_cmd_sync_last_signal_type()` says "2=MSI" which is wrong.
//! The doc comment and inline comment at the storage site both say "MSI".
//! Fix: rename the label to "SEV" in all three locations.  Behavioral logic
//! is correct — only naming/documentation needs updating.
//!
//! The test verifies that `get_cmd_sync_last_signal_type()` returns 2 after
//! CMD_SYNC CS=2 (SEV), confirming the API works correctly.  A compile-time
//! check is not possible for comments alone, so this test also serves as a
//! baseline regression check.
//!
//! ## BUG-QA-8 — inject_walk_eabt() event_class must be 1 (TT) (§7.3.12)
//!
//! ARM §7.3.12: F_WALK_EABT CLASS field must be 0b01 (TT = translation-table
//! descriptor fetch).  Currently `inject_walk_eabt()` uses
//! `..EventEntry::zeroed()` which leaves `event_class: 0` (CD).
//!
//! ## BUG-QA-9 — CMD_TLBI_EL3_ALL and CMD_TLBI_EL3_VA must raise CERROR_ILL (§4.4.2.5/§4.4.2.6)
//!
//! The spec states these commands are valid only on the Secure Command queue.
//! On the Non-secure queue (the only queue this model has), they must raise
//! CERROR_ILL.  Currently CMD_TLBI_EL3_ALL calls invalidate_all() and
//! CMD_TLBI_EL3_VA calls VA-based invalidation — both are wrong.
//!
//! ## BUG-QA-10 — inject_walk_eabt() SSV must be true when PASID != 0 (§7.3)
//!
//! ARM §7.3: SSV (SubstreamID Valid) must be set to true when the transaction
//! carried a non-zero PASID.  Currently `..EventEntry::zeroed()` forces
//! `ssv: false` regardless of the pasid parameter.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{CommandEntry, CommandType, EventType, StreamConfig, StreamID, IOVA, PASID};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

// ============================================================================
// BUG-QA-7: CMD_SYNC CS=2 must be labeled SEV, not MSI (§4.7.3)
// ============================================================================
//
// The behavioral logic (storing CS=2) is correct; only the label in comments
// is wrong.  These tests verify the actual runtime behavior is correct and
// serves as a regression baseline for the renaming fix.

/// BUG-QA-7 (primary): after CMD_SYNC CS=2 (SIG_SEV), the stored signal type
/// must equal 2.
///
/// BEFORE FIX: comment says "2=MSI" — mislabeled, but runtime returns 2
/// correctly.  The test passes before and after.  The fix is the label rename.
///
/// AFTER FIX: same runtime behavior; comment now says "2=SEV".
/// Note: BUG-AUDIT-65 fix gates CS=2 on IDR0.SEV=1, so set_sev_supported(true)
/// is required for the SIG_SEV path to store signal type 2 (ARM §4.7.3).
#[test]
fn bug_qa7_cmd_sync_cs2_stored_as_sev() {
    let smmu = make_smmu();

    // BUG-AUDIT-65: CS=2 (SIG_SEV) only stores signal type 2 when IDR0.SEV=1.
    smmu.set_sev_supported(true);

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0x02; // SIG_SEV
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let sig = smmu.get_cmd_sync_last_signal_type();
    assert_eq!(
        sig, 2,
        "BUG-QA-7: CMD_SYNC CS=0b10 (SIG_SEV) must store signal type 2. \
         Got {}. ARM §4.7.3: CS=2 is SIG_SEV (PE wakeup), NOT MSI.",
        sig
    );
}

/// BUG-QA-7 (companion): CS=1 (SIG_IRQ) stores signal type 1.
/// This guards against the fix accidentally breaking SIG_IRQ.
#[test]
fn bug_qa7_cmd_sync_cs1_stored_as_irq() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0x01; // SIG_IRQ
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let sig = smmu.get_cmd_sync_last_signal_type();
    assert_eq!(
        sig, 1,
        "CMD_SYNC CS=0b01 (SIG_IRQ) must store signal type 1. Got {}.",
        sig
    );
}

/// BUG-QA-7 (companion): CS=0 (SIG_NONE) leaves signal type at previous value.
/// get_cmd_sync_last_signal_type() returns 0 when no non-NONE sync has been
/// processed yet (initial state).
#[test]
fn bug_qa7_cmd_sync_cs0_does_not_update_signal_type() {
    let smmu = make_smmu();

    // Initial state: no sync processed yet → signal type = 0.
    assert_eq!(
        smmu.get_cmd_sync_last_signal_type(),
        0,
        "Initial signal type must be 0 (SIG_NONE)."
    );

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0x00; // SIG_NONE — must NOT update last signal type.
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    assert_eq!(
        smmu.get_cmd_sync_last_signal_type(),
        0,
        "CMD_SYNC CS=0b00 (SIG_NONE) must not update the stored signal type."
    );
}

// ============================================================================
// BUG-QA-8: inject_walk_eabt() must set event_class = 1 (TT) (§7.3.12)
// ============================================================================
//
// ARM §7.3.12 F_WALK_EABT: CLASS field must be 0b01 (TT = translation-table
// descriptor fetch).  The current implementation uses ..EventEntry::zeroed()
// which leaves event_class=0 (CD).
//
// BEFORE FIX: inject_walk_eabt() produces event with event_class=0 → test fails.
// AFTER FIX:  inject_walk_eabt() produces event with event_class=1 → test passes.

/// BUG-QA-8 (primary): F_WALK_EABT event must have event_class == 1 (TT).
#[test]
fn bug_qa8_walk_eabt_event_class_is_tt() {
    let smmu = make_smmu();

    let stream_id = sid(1);
    let p = pasid(0);
    let addr = iova(0x1000);
    smmu.inject_walk_eabt(stream_id, p, addr, false, 1);

    let events = smmu.get_events();
    assert_eq!(events.len(), 1, "Expected exactly one event after inject_walk_eabt");

    let ev = &events[0];
    assert_eq!(
        ev.event_class, 1,
        "BUG-QA-8: F_WALK_EABT must have event_class=1 (TT per ARM §7.3.12). \
         Got event_class={}. The current code leaves ..EventEntry::zeroed() \
         which defaults event_class to 0 (CD).",
        ev.event_class
    );
}

/// BUG-QA-8 (companion): event_type must be FWalkEabt.
/// Guards against the fix accidentally changing the event type.
#[test]
fn bug_qa8_walk_eabt_event_type_correct() {
    let smmu = make_smmu();

    smmu.inject_walk_eabt(sid(2), pasid(0), iova(0x2000), false, 1);

    let events = smmu.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].event_type,
        EventType::FWalkEabt,
        "inject_walk_eabt must produce EventType::FWalkEabt"
    );
}

// ============================================================================
// BUG-QA-9: CMD_TLBI_EL3_ALL and CMD_TLBI_EL3_VA must raise CERROR_ILL (§4.4.2.5/§4.4.2.6)
// ============================================================================
//
// These commands are only valid on the Secure Command queue.  This model only
// has a Non-secure Command queue, so both must ALWAYS raise CERROR_ILL.
//
// BEFORE FIX:
//   - CMD_TLBI_EL3_ALL calls invalidate_all() — no CERROR_ILL → test fails.
//   - CMD_TLBI_EL3_VA calls VA-based invalidation — no CERROR_ILL → test fails.
// AFTER FIX:
//   - Both commands return CERROR_ILL → tests pass.

/// BUG-QA-9 (primary): CMD_TLBI_EL3_ALL on Non-secure queue must raise CERROR_ILL.
#[test]
fn bug_qa9_tlbi_el3_all_raises_cerror_ill() {
    let smmu = make_smmu();

    // Configure a stream so translate() works, but the test is about
    // the command queue error path.
    smmu.configure_stream(sid(1), StreamConfig::bypass()).unwrap();

    let cmd = CommandEntry::new(CommandType::TlbiEl3All, 0, 0);
    smmu.submit_command(cmd).expect("submit_command must not fail (enqueueing only)");
    // process_command_queue() may return Ok or Err — the important check
    // is the CMDQ_CONS.ERR field after processing.
    let _ = smmu.process_command_queue();

    let err = smmu.get_cmdq_cons_err();
    assert_eq!(
        err,
        SMMU::CERROR_ILL,
        "BUG-QA-9: CMD_TLBI_EL3_ALL on Non-secure queue must set CMDQ_CONS.ERR = \
         CERROR_ILL (ARM §4.4.2.5). Got err={}. Currently the command calls \
         invalidate_all() instead of raising CERROR_ILL.",
        err
    );
}

/// BUG-QA-9 (secondary): CMD_TLBI_EL3_VA on Non-secure queue must raise CERROR_ILL.
#[test]
fn bug_qa9_tlbi_el3_va_raises_cerror_ill() {
    let smmu = make_smmu();

    smmu.configure_stream(sid(1), StreamConfig::bypass()).unwrap();

    let cmd = CommandEntry::new(CommandType::TlbiEl3Va, 0, 0);
    smmu.submit_command(cmd).expect("submit_command must not fail");
    let _ = smmu.process_command_queue();

    let err = smmu.get_cmdq_cons_err();
    assert_eq!(
        err,
        SMMU::CERROR_ILL,
        "BUG-QA-9: CMD_TLBI_EL3_VA on Non-secure queue must set CMDQ_CONS.ERR = \
         CERROR_ILL (ARM §4.4.2.6). Got err={}. Currently the command calls \
         VA-based invalidation instead of raising CERROR_ILL.",
        err
    );
}

/// BUG-QA-9 (companion): CMD_TLBI_EL3_ALL must NOT advance the invalidation
/// counter (no TLB operation should occur on an illegal command).
#[test]
fn bug_qa9_tlbi_el3_all_no_invalidation_side_effect() {
    let smmu = make_smmu();

    // Clear any prior error so we start from CERROR_NONE.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    let before = smmu.get_invalidation_count();

    let cmd = CommandEntry::new(CommandType::TlbiEl3All, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    let after = smmu.get_invalidation_count();
    // The invalidation count must not increase on an illegal command.
    // (It may have gone to CERROR_ILL, which is the correct behavior.)
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "CMD_TLBI_EL3_ALL must produce CERROR_ILL"
    );
    assert_eq!(
        before, after,
        "BUG-QA-9: CMD_TLBI_EL3_ALL must not increment invalidation_count; \
         no TLB operation should occur for an illegal command. \
         before={}, after={}",
        before, after
    );
}

// ============================================================================
// BUG-QA-10: inject_walk_eabt() SSV must be true when pasid != 0 (§7.3)
// ============================================================================
//
// ARM §7.3: SSV (SubstreamID Valid) is set when the transaction carried a
// non-zero PASID.  inject_walk_eabt() accepts a pasid parameter but uses
// ..EventEntry::zeroed() which forces ssv=false.
//
// BEFORE FIX: inject_walk_eabt(pasid=5) produces ssv=false → test fails.
// AFTER FIX:  inject_walk_eabt(pasid=5) produces ssv=true  → test passes.

/// BUG-QA-10 (primary): inject_walk_eabt() with non-zero PASID must set ssv=true.
#[test]
fn bug_qa10_walk_eabt_ssv_true_when_pasid_nonzero() {
    let smmu = make_smmu();

    smmu.inject_walk_eabt(sid(3), pasid(5), iova(0x3000), false, 1);

    let events = smmu.get_events();
    assert_eq!(events.len(), 1, "Expected exactly one event");

    let ev = &events[0];
    assert!(
        ev.ssv,
        "BUG-QA-10: inject_walk_eabt() with non-zero PASID (5) must set ssv=true. \
         ARM §7.3: SSV indicates the transaction carried a SubstreamID (PASID). \
         Currently ..EventEntry::zeroed() forces ssv=false."
    );
}

/// BUG-QA-10 (companion): inject_walk_eabt() with PASID=0 must leave ssv=false.
/// Verifies that the fix does not accidentally set ssv=true for PASID=0.
#[test]
fn bug_qa10_walk_eabt_ssv_false_when_pasid_zero() {
    let smmu = make_smmu();

    smmu.inject_walk_eabt(sid(4), pasid(0), iova(0x4000), false, 1);

    let events = smmu.get_events();
    assert_eq!(events.len(), 1, "Expected exactly one event");

    let ev = &events[0];
    assert!(
        !ev.ssv,
        "inject_walk_eabt() with PASID=0 must leave ssv=false. \
         ARM §7.3: SSV is only set when a non-zero PASID is present. Got ssv=true."
    );
}

/// BUG-QA-8 + BUG-QA-10 combined: inject_walk_eabt() with non-zero PASID must
/// simultaneously produce event_class=1 (TT) AND ssv=true.
#[test]
fn bug_qa8_and_qa10_walk_eabt_combined_fix() {
    let smmu = make_smmu();

    smmu.inject_walk_eabt(sid(5), pasid(42), iova(0x5000), false, 1);

    let events = smmu.get_events();
    assert_eq!(events.len(), 1, "Expected exactly one event");

    let ev = &events[0];
    assert_eq!(
        ev.event_class, 1,
        "BUG-QA-8: event_class must be 1 (TT). Got {}.",
        ev.event_class
    );
    assert!(
        ev.ssv,
        "BUG-QA-10: ssv must be true for non-zero PASID. Got ssv=false."
    );
    assert_eq!(
        ev.pasid, 42,
        "PASID must be preserved in the event. Got {}.",
        ev.pasid
    );
    assert_eq!(
        ev.stream_id, 5,
        "StreamID must be preserved. Got {}.",
        ev.stream_id
    );
}
