#![allow(missing_docs)]
//! BUG-AUDIT-141: CD.R gates event recording for stage-1 terminate-mode faults.
//!
//! Spec §3.12.1: If CD.R=0, the transaction is terminated but NO event is
//! written to the event queue.

use smmu::types::{AccessType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID};
use smmu::SMMU;

/// BUG-AUDIT-141: CD.R=true (default) records the event in the queue for a terminate fault.
///
/// A stage-1 translation fault on a terminate-mode stream with CD.R=true MUST
/// write an event to the event queue.
#[test]
fn bug_audit_141_cd_r_true_records_event() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x10).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .cd_r(true)
        .build()
        .unwrap();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, PASID::new(0).unwrap()).unwrap();

    // Trigger a translation fault on an unmapped address.
    let result = smmu.translate(
        stream_id,
        PASID::new(0).unwrap(),
        IOVA::new(0x9000_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "Expected translation error, got Ok");
    // CD.R=true: event MUST be recorded.
    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-AUDIT-141: CD.R=true must record event in queue (got {} events)",
        events.len()
    );
}

/// BUG-AUDIT-141: CD.R=false suppresses event recording for terminate-mode faults.
///
/// A stage-1 translation fault on a terminate-mode stream with CD.R=false MUST
/// NOT write an event to the event queue, but `translate()` MUST still return Err.
#[test]
fn bug_audit_141_cd_r_false_suppresses_event() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x11).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .cd_r(false)
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

    // Still returns an error (transaction is terminated).
    assert!(result.is_err(), "Expected translation error with CD.R=false, got Ok");
    // But NO event must be recorded.
    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "BUG-AUDIT-141: CD.R=false must suppress event recording (got {} events)",
        events.len()
    );
}

/// BUG-AUDIT-141: CD.R=false with stall mode still records the event.
///
/// CD.R only suppresses event recording for terminate-mode faults.
/// When `fault_mode` is Stall, the event is always written regardless of CD.R.
#[test]
fn bug_audit_141_cd_r_false_stall_still_records() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(0x12).unwrap();
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .cd_r(false)
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

    // Should be stalled, not terminated.
    assert!(
        matches!(result, Err(smmu::types::TranslationError::Stalled { .. })),
        "Expected Stalled error for stall-mode stream, got: {result:?}",
    );
    // Stall events are always recorded regardless of CD.R.
    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-AUDIT-141: CD.R=false must NOT suppress stall-mode event recording (got {} events)",
        events.len()
    );
}
