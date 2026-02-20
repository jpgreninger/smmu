//! Spec-compliance tests for `F_STREAM_DISABLED` event generation
//!
//! ARM IHI0070G.b §7.3.7: When `STE.Config` indicates a disabled/abort stream,
//! a non-substream transaction must generate an `F_STREAM_DISABLED` event record
//! (event code 0x06) in the event queue — NOT a generic `F_TRANSLATION` or `C_BAD_STE`.

use smmu::types::{AccessType, EventType, SecurityState, StreamConfig, StreamID, IOVA, PASID};
use smmu::SMMU;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    // SMMUEN must be set (§6.3.9) for per-stream disable to be reached.
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}
fn pasid_zero() -> PASID {
    PASID::new(0).unwrap()
}
fn iova_1000() -> IOVA {
    IOVA::new(0x1000).unwrap()
}

// ─── Failing Tests (RED state) ────────────────────────────────────────────────

/// §7.3.7: Translation on a disabled stream must queue `F_STREAM_DISABLED` (0x06)
#[test]
fn test_disabled_stream_generates_f_stream_disabled_event() {
    let smmu = make_smmu();
    let stream_id = sid(10);

    // Configure with stage-1 so it's a real translation stream
    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    // Disable the stream
    smmu.disable_stream(stream_id).unwrap();

    // Attempt translation — must fail with StreamDisabled
    let result =
        smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "expected error on disabled stream");

    // Event queue must contain F_STREAM_DISABLED
    let by_type = smmu.get_events_by_type(EventType::FStreamDisabled);
    assert!(
        !by_type.is_empty(),
        "expected FStreamDisabled event; got events: {:?}",
        smmu.get_events().iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §7.3.7: `F_STREAM_DISABLED` event must record the correct `stream_id`
#[test]
fn test_disabled_stream_event_carries_stream_id() {
    let smmu = make_smmu();
    let stream_id = sid(42);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.disable_stream(stream_id).unwrap();

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::FStreamDisabled);
    assert!(!events.is_empty(), "expected FStreamDisabled event");
    assert_eq!(events[0].stream_id, 42, "stream_id must match");
}

/// §7.3.7: The event generated must be `F_STREAM_DISABLED` (0x06), not `F_TRANSLATION`
#[test]
fn test_disabled_stream_not_f_translation_event() {
    let smmu = make_smmu();
    let stream_id = sid(20);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.disable_stream(stream_id).unwrap();

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Write, SecurityState::NonSecure);

    // Must NOT have generated a generic F_TRANSLATION event for this case
    let translation_events = smmu.get_events_by_type(EventType::FTranslation);
    let disabled_events = smmu.get_events_by_type(EventType::FStreamDisabled);
    assert!(
        !disabled_events.is_empty(),
        "must have F_STREAM_DISABLED event"
    );
    assert!(
        translation_events.is_empty(),
        "must NOT have F_TRANSLATION event for stream-disabled fault; got F_TRANSLATION events: {translation_events:?}"
    );
}

/// Disabled stream must not generate `C_BAD_STE` (wrong type)
#[test]
fn test_disabled_stream_not_c_bad_ste_event() {
    let smmu = make_smmu();
    let stream_id = sid(30);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.disable_stream(stream_id).unwrap();

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let bad_ste_events = smmu.get_events_by_type(EventType::CBadSte);
    assert!(
        bad_ste_events.is_empty(),
        "must NOT have C_BAD_STE event for stream-disabled fault; got: {bad_ste_events:?}"
    );
}

/// Disabling then re-enabling a stream must resume normal translation
#[test]
fn test_reenable_stream_resumes_translation() {
    let smmu = make_smmu();
    let stream_id = sid(50);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();
    smmu.disable_stream(stream_id).unwrap();

    // Translation must fail while disabled
    let r1 = smmu.translate(
        stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(r1.is_err(), "must fail while disabled");

    smmu.enable_stream(stream_id).unwrap();
    smmu.clear_event_queue();

    // Translation must succeed after re-enable (bypass mode: IOVA == PA)
    let r2 = smmu.translate(
        stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(r2.is_ok(), "must succeed after re-enable");
    // No disabled event after re-enable
    assert!(
        smmu.get_events_by_type(EventType::FStreamDisabled).is_empty(),
        "no F_STREAM_DISABLED after re-enable"
    );
}

/// `F_STREAM_DISABLED` event code must be 0x06 per §7.3.7
#[test]
fn test_f_stream_disabled_event_code_is_0x06() {
    assert_eq!(EventType::FStreamDisabled as u8, 0x06, "F_STREAM_DISABLED must be event code 0x06 per §7.3.7");
}
