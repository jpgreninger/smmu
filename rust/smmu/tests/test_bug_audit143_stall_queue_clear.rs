#![allow(missing_docs)]
//! BUG-AUDIT-143: When SMMUEN transitions 1→0, stall queue must be cleared.
//!
//! Spec §3.12.2: When SMMUEN transitions 1→0, all stalled transactions are
//! implicitly terminated and STAG values may be re-used. The stall queue must
//! be cleared.

use smmu::types::{AccessType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID};
use smmu::types::TranslationError;
use smmu::SMMU;

/// Helper: configure a stall-mode stream, enable the SMMU, trigger a fault
/// so that the stall queue becomes non-empty, then return the SMMU.
fn setup_with_stalled_transaction() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x01).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    // Trigger a translation fault on an unmapped IOVA — this should stall.
    let result = smmu.translate(
        stream_id,
        PASID::new(0).unwrap(),
        IOVA::new(0x9000_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Verify the stall was actually created.
    assert!(
        matches!(result, Err(TranslationError::Stalled { .. })),
        "Expected Stalled error to set up test, got: {result:?}",
    );
    assert!(
        !smmu.get_stalled_transactions().is_empty(),
        "Stall queue must be non-empty before testing disable",
    );

    smmu
}

/// BUG-AUDIT-143: `disable()` must clear the stall queue (§3.12.2).
///
/// When SMMUEN transitions 1→0, all stalled transactions are implicitly
/// terminated and STAG values may be re-used.
#[test]
fn bug_audit_143_disable_clears_stall_queue() {
    let smmu = setup_with_stalled_transaction();

    // Call disable() — this is the 1→0 SMMUEN transition.
    smmu.disable().unwrap();

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "BUG-AUDIT-143: disable() must clear stall_queue on SMMUEN=0 (§3.12.2)",
    );
}

/// BUG-AUDIT-143: `set_cr0(0)` must clear the stall queue on SMMUEN 1→0 (§3.12.2).
///
/// Writing CR0 with SMMUEN cleared while it was set must clear stalled transactions.
#[test]
fn bug_audit_143_set_cr0_smmuen_0_clears_stall() {
    let smmu = setup_with_stalled_transaction();

    // set_cr0(0) — clears SMMUEN bit (1→0 transition).
    smmu.set_cr0(0);

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "BUG-AUDIT-143: set_cr0(0) must clear stall_queue on SMMUEN 1→0 transition (§3.12.2)",
    );
}

/// BUG-AUDIT-143: `set_cr0` with SMMUEN staying 0 must NOT clear the stall queue spuriously.
///
/// Only a 1→0 transition should clear; 0→0 is a no-op for stall state.
/// (SMMU starts disabled so there cannot be stalled transactions while disabled,
/// but we verify the code path doesn't panic.)
#[test]
fn bug_audit_143_set_cr0_smmuen_already_0_no_panic() {
    let smmu = SMMU::new();
    // SMMU starts with SMMUEN=0; setting CR0=0 again is a 0→0 non-transition.
    smmu.set_cr0(0);
    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "Stall queue should be empty on a freshly created SMMU",
    );
}

/// BUG-AUDIT-143: `set_cr0` with SMMUEN staying 1 must NOT clear the stall queue.
///
/// A 1→1 write to CR0 must preserve existing stall state.
#[test]
fn bug_audit_143_set_cr0_smmuen_stays_1_preserves_stall() {
    let smmu = setup_with_stalled_transaction();

    let stall_count_before = smmu.get_stalled_transactions().len();

    // set_cr0 with SMMUEN still set — 1→1, no transition.
    smmu.set_cr0(SMMU::CR0_SMMUEN);

    let stall_count_after = smmu.get_stalled_transactions().len();
    assert_eq!(
        stall_count_before, stall_count_after,
        "BUG-AUDIT-143: set_cr0 with SMMUEN=1→1 must not clear stall queue",
    );
}
