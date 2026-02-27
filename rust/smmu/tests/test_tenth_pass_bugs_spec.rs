#![allow(clippy::doc_markdown)]
//! Tenth-pass bug tests — BUG-09/15, BUG-10, BUG-11, BUG-13 (Rust)
//!
//! Spec: ARM IHI 0070 G.b
//!
//! BUG-09/15: Rust CMD_SYNC CS=0b11 performs spurious fetch_xor(GERROR_CMDQ_ERR)
//!   before process_command_queue's authoritative fetch_or. (§6.3.19, §7.5)
//! BUG-10: record_translation_fault toggles EVENTQ_PROD.OVFLG unconditionally
//!   on queue-full instead of only when ovflg==ovackflg and !event.stall. (§7.4)
//! BUG-11: s1cd_max >= 32 causes panic (debug) / UB (release) on shift; no
//!   validation that s1cd_max <= 20 (SSIDSIZE max). (§5.2, SMMU_IDR1.SSIDSIZE)
//! BUG-13: Stall queue exhaustion (iters >= 65535) returns TlbConflict without
//!   recording a fault event; should record and return original error. (§3.12.2)

use smmu::types::{
    AccessType, CommandEntry, CommandType, FaultMode, QueueConfig, SMMUConfig,
    SecurityState, StreamConfig, StreamID, IOVA, PASID,
};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn make_smmu_with_event_queue(size: usize) -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::from(QueueConfig {
        event_queue_size: size,
        ..Default::default()
    }));
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }

fn stage1_terminate_config() -> StreamConfig {
    StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap()
}

// ── BUG-09/15: GERROR fetch_xor removal ───────────────────────────────────────
//
// The spec (§6.3.19 / §7.5) requires that GERROR bits use "activate-only-if-
// inactive" (OR) semantics — the SMMU must NOT toggle a bit when the error is
// already active.  Before the fix, process_single_command performed a
// fetch_xor(GERROR_CMDQ_ERR) then returned Err; process_command_queue then
// performed a fetch_or(GERROR_CMDQ_ERR).  The XOR was spurious: if CMDQ_ERR
// was already 1, XOR toggled it to 0 before OR set it back — creating a brief
// race window where GERROR.CMDQ_ERR appeared cleared.
//
// The single-threaded observable invariant: GERROR.CMDQ_ERR must be set (not 0)
// after process_command_queue returns Err for CMD_SYNC CS=0b11.

/// §6.3.19 / §7.5 / BUG-09/15: GERROR.CMDQ_ERR must be set after CMD_SYNC CS=0b11.
#[test]
fn bug09_cmd_sync_cs3_sets_gerror_cmdq_err() {
    let smmu = make_smmu();
    assert_eq!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0, "GERROR starts at 0");

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0b11;
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    assert!(result.is_err(), "§4.7.3: process_command_queue must return Err for CS=0b11");
    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "§6.3.19: GERROR.CMDQ_ERR must be set after CERROR_ILL"
    );
}

/// §7.5 / BUG-09/15: No EventEntry is written to the event queue for CERROR_ILL.
/// CMD_SYNC CS=0b11 must NOT produce a completion event — command errors go to
/// GERROR / CMDQ_CONS.ERR only (§7.1 / §4.1.3).
#[test]
fn bug09_cmd_sync_cs3_does_not_produce_event_entry() {
    let smmu = make_smmu();
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0b11;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().ok();

    assert_eq!(
        smmu.get_events().len(),
        0,
        "§4.1.3 / §7.1: no EventEntry must be recorded for CERROR_ILL"
    );
}

// ── BUG-10: OVFLG must not re-toggle when overflow already active (§7.4) ──────
//
// §7.4: "When an overflow occurs, the SMMU toggles the SMMU_EVENTQ_PROD.OVFLG
// flag if the overflow condition is not already present."
// Overflow is already present when OVFLG != OVACKFLG.
//
// Before the fix, record_translation_fault called fetch_xor(OVFLG) unconditionally.
// The second consecutive overflow would toggle OVFLG back to 0 — WRONG.
//
// After the fix: the toggle is guarded by (ovflg == ovackflg) AND (!event.stall).

/// §7.4 / BUG-10: OVFLG must toggle 0→1 on first overflow.
#[test]
fn bug10_ovflg_set_on_first_overflow() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    let cfg = stage1_terminate_config();
    smmu.configure_stream(sid(1), cfg).unwrap();
    smmu.create_pasid(sid(1), pasid(0)).unwrap();

    // Fill the event queue to capacity (all unmapped → F_TRANSLATION events).
    for i in 0..q {
        let _ = smmu.translate(sid(1), pasid(0), iova(0x1000 * (i as u64 + 1)),
                               AccessType::Read, SecurityState::NonSecure);
    }
    assert_eq!(smmu.get_events().len(), q, "queue must be full");
    assert_eq!(
        (smmu.get_eventq_prod() >> 31) & 1, 0,
        "OVFLG must be 0 before any overflow"
    );

    // First overflow — OVFLG must toggle 0→1.
    let _ = smmu.translate(sid(1), pasid(0), iova(0x1000 * (q as u64 + 1)),
                           AccessType::Read, SecurityState::NonSecure);
    assert_eq!(
        (smmu.get_eventq_prod() >> 31) & 1, 1,
        "§7.4: OVFLG must be 1 after first overflow"
    );
}

/// §7.4 / BUG-10: OVFLG must NOT re-toggle when overflow is already active.
///
/// This test FAILS before the fix: the unconditional fetch_xor in
/// record_translation_fault toggles OVFLG 1→0 on the second discard.
#[test]
fn bug10_ovflg_not_retriggered_on_second_overflow() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    let cfg = stage1_terminate_config();
    smmu.configure_stream(sid(2), cfg).unwrap();
    smmu.create_pasid(sid(2), pasid(0)).unwrap();

    // Fill queue to capacity.
    for i in 0..q {
        let _ = smmu.translate(sid(2), pasid(0), iova(0x2000 * (i as u64 + 1)),
                               AccessType::Read, SecurityState::NonSecure);
    }

    // First overflow — sets OVFLG to 1.
    let _ = smmu.translate(sid(2), pasid(0), iova(0x2000 * (q as u64 + 1)),
                           AccessType::Read, SecurityState::NonSecure);
    assert_eq!((smmu.get_eventq_prod() >> 31) & 1, 1, "precondition: OVFLG=1 after first overflow");

    // Second overflow — BUG-10: unconditional XOR toggles 1→0.
    // After fix: ovflg(1) != ovackflg(0) → no toggle → OVFLG stays 1.
    let _ = smmu.translate(sid(2), pasid(0), iova(0x2000 * (q as u64 + 2)),
                           AccessType::Read, SecurityState::NonSecure);
    assert_eq!(
        (smmu.get_eventq_prod() >> 31) & 1, 1,
        "§7.4 / BUG-10: OVFLG must remain 1 on second overflow (must not re-toggle)"
    );
}

/// §7.4 / BUG-10: A stall event that hits the extended-capacity boundary must NOT
/// toggle OVFLG (stall events are never discarded per §7.4).
///
/// The extended stall capacity is 2× the normal capacity.  Fill the normal
/// capacity with non-stall events (which were already there), then a stall event
/// arriving at the boundary should be discarded silently — not toggle OVFLG.
///
/// NOTE: Because stall-mode events use the 2× extended limit, a stall event
/// only hits the discard path when queue.len() >= stall_capacity (2×q).
/// We verify that if somehow a stall event is discarded, OVFLG is NOT toggled.
/// We test the softer property: OVFLG remains unchanged after a stall-mode fault
/// on a queue that was not yet full from non-stall perspective.
#[test]
fn bug10_stall_fault_does_not_toggle_ovflg() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    // Stream with stall mode.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(3), cfg).unwrap();
    smmu.create_pasid(sid(3), pasid(0)).unwrap();

    let ovflg_before = (smmu.get_eventq_prod() >> 31) & 1;

    // A stall fault — queue is not yet full, so this goes into the event queue.
    let result = smmu.translate(sid(3), pasid(0), iova(0x3000),
                                AccessType::Read, SecurityState::NonSecure);
    // Stall mode: result should be Stalled, not an error.
    assert!(
        matches!(result, Err(smmu::types::TranslationError::Stalled { .. })),
        "stall mode must return Stalled"
    );

    let ovflg_after = (smmu.get_eventq_prod() >> 31) & 1;
    assert_eq!(
        ovflg_before, ovflg_after,
        "§7.4 / BUG-10: stall event must not toggle OVFLG"
    );
}

// ── BUG-11: s1cd_max must be validated against SSIDSIZE max of 20 ─────────────
//
// ARM §5.2 STE.S1CDMax: "The allowable range is 0 to SMMU_IDR1.SSIDSIZE inclusive."
// ARM SMMU_IDR1.SSIDSIZE: "Valid range 0 to 20 inclusive."
//
// StreamConfig::validate() must reject s1cd_max > 20.  Additionally, the shift
// `1u32 << s1cd_max` at the C_BAD_SUBSTREAMID check panics in debug mode / has
// UB in release mode for s1cd_max >= 32.
//
// Test FAILS before fix: build() currently returns Ok for s1cd_max=21.

/// §5.2 / BUG-11: s1cd_max=21 must fail validation (exceeds SSIDSIZE max of 20).
#[test]
fn bug11_s1cd_max_21_fails_validation() {
    let result = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .s1cd_max(21)
        .build();
    assert!(
        result.is_err(),
        "§5.2 / BUG-11: s1cd_max=21 must fail validation (SSIDSIZE max is 20)"
    );
}

/// §5.2 / BUG-11: s1cd_max=20 must succeed (exactly at the SSIDSIZE maximum).
#[test]
fn bug11_s1cd_max_20_is_valid() {
    let result = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .s1cd_max(20)
        .build();
    assert!(result.is_ok(), "§5.2: s1cd_max=20 must succeed (SSIDSIZE max is 20)");
}

/// §5.2 / BUG-11: s1cd_max=0 (not substream-capable) must always be valid.
#[test]
fn bug11_s1cd_max_0_is_valid() {
    let result = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .s1cd_max(0)
        .build();
    assert!(result.is_ok(), "s1cd_max=0 (not substream-capable) must be valid");
}

/// §5.2 / BUG-11: s1cd_max=31 must fail validation (> 20, also panic risk on shift).
#[test]
fn bug11_s1cd_max_31_fails_validation() {
    let result = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .s1cd_max(31)
        .build();
    assert!(
        result.is_err(),
        "§5.2 / BUG-11: s1cd_max=31 must fail validation"
    );
}

// ── BUG-13: Stall queue exhaustion must record fault and return original error ─
//
// ARM §3.12.2: "The SMMU always records the details of the access into the Event
// queue." When the stall queue is exhausted (iters >= 65535), the implementation
// previously returned TlbConflict without calling record_translation_fault.
// After the fix: record_translation_fault is called with is_stall=false before
// returning, and the original translation error is returned (not TlbConflict).
//
// Full exhaustion (65535 STAGs) is impractical in a unit test; the test below
// verifies the NORMAL stall path records fault events correctly as a regression
// guard for the related fault-recording logic.

/// §3.12.2 / BUG-13: Normal stall faults must produce an event entry.
/// (Regression guard ensuring fault recording works on the stall path.)
#[test]
fn bug13_stall_fault_records_event() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(5), cfg).unwrap();
    smmu.create_pasid(sid(5), pasid(0)).unwrap();

    // Trigger a stall fault (unmapped address).
    let result = smmu.translate(sid(5), pasid(0), iova(0xDEAD_0000),
                                AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(smmu::types::TranslationError::Stalled { .. })),
        "stall mode must return Stalled"
    );

    // An event must be recorded in the event queue for the stalled transaction.
    let events = smmu.get_events();
    assert!(!events.is_empty(), "§3.12.2 / BUG-13: stall fault must produce an event entry");
}

/// §3.12.2 / BUG-13: TlbConflict must never be returned for a straightforward
/// stall-mode translation fault (it is IMPDEF for actual TLB geometry conflicts).
#[test]
fn bug13_stall_fault_does_not_return_tlb_conflict() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(6), cfg).unwrap();
    smmu.create_pasid(sid(6), pasid(0)).unwrap();

    let result = smmu.translate(sid(6), pasid(0), iova(0xBEEF_0000),
                                AccessType::Read, SecurityState::NonSecure);
    assert!(
        !matches!(result, Err(smmu::types::TranslationError::TlbConflict)),
        "§3.12.2 / BUG-13: TlbConflict must not be returned for a normal stall-mode fault"
    );
}
