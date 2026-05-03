#![allow(missing_docs)]
//! BUG-AUDIT-142: CD.A selects abort vs RAZ/WI for stage-1 terminate-mode faults.
//!
//! Spec §3.12.1: CD.A=true (default) → abort the transaction.
//! CD.A=false (when `TERM_MODEL=0`) → complete with RAZ/WI data.

use smmu::types::{AccessType, FaultMode, SecurityState, StreamConfig, StreamID, TranslationError, IOVA, PASID};
use smmu::SMMU;

/// BUG-AUDIT-142: CD.A=true (default) returns an abort error on terminate fault.
#[test]
fn bug_audit_142_cd_a_true_returns_abort() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x20).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .cd_a(true)  // abort behavior
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    let result = smmu.translate(
        stream_id,
        PASID::new(0).unwrap(),
        IOVA::new(0x9000_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "Expected error on terminate fault with CD.A=true");
    assert!(
        !matches!(result, Err(TranslationError::RazWi)),
        "CD.A=true must NOT return RazWi — got: {result:?}"
    );
}

/// BUG-AUDIT-142: CD.A=false returns `RazWi` error on terminate fault.
///
/// Spec §3.12.1: When `TERM_MODEL=0` and `CD.A=0`, stage-1 terminate-mode fault
/// completes with RAZ/WI behavior instead of an abort.
#[test]
fn bug_audit_142_cd_a_false_returns_razwi() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x21).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .cd_a(false)  // RAZ/WI behavior
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    let result = smmu.translate(
        stream_id,
        PASID::new(0).unwrap(),
        IOVA::new(0x9000_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(TranslationError::RazWi)),
        "BUG-AUDIT-142: CD.A=false must return RazWi on terminate fault, got: {result:?}"
    );
}

/// BUG-AUDIT-142: CD.A=false with stall mode is still stalled, not RAZ/WI.
///
/// CD.A only applies to terminate mode. Stall mode overrides CD.A.
#[test]
fn bug_audit_142_cd_a_false_stall_mode_not_razwi() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x22).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .cd_a(false)  // irrelevant in stall mode
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    let result = smmu.translate(
        stream_id,
        PASID::new(0).unwrap(),
        IOVA::new(0x9000_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(TranslationError::Stalled { .. })),
        "BUG-AUDIT-142: stall mode with CD.A=false must still stall, got: {result:?}"
    );
}
