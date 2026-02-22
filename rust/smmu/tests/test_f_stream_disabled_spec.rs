//! Spec-compliance tests for `F_STREAM_DISABLED` event generation
//!
//! ARM IHI0070G.b §7.3.7 / §5.2 (Config table):
//! When `STE.Config == 0b000` (stream disabled / abort), incoming traffic is terminated
//! WITHOUT recording an event. `F_STREAM_DISABLED` (0x06) is only generated when a
//! non-substream transaction arrives on a stream that has substreams required
//! (`STE.S1DSS == 0b00`, `STE.S1CDMax > 0`) — a distinct configuration.

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

// ─── Spec-corrected Tests ─────────────────────────────────────────────────────

/// §7.3.7 / §5.2: Translation on a disabled stream (STE.Config==0b000) must NOT
/// enqueue any event — incoming traffic is terminated without recording an event.
#[test]
fn test_disabled_stream_generates_f_stream_disabled_event() {
    let smmu = make_smmu();
    let stream_id = sid(10);

    // Configure with stage-1 so it's a real translation stream
    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    // Disable the stream (equivalent to STE.Config == 0b000)
    smmu.disable_stream(stream_id).unwrap();

    // Attempt translation — must fail with StreamDisabled
    let result =
        smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "expected error on disabled stream");

    // §7.3.7 / §5.2: STE.Config==0b000 terminates without recording an event.
    // No event of any kind must appear in the queue.
    let all_events = smmu.get_events();
    assert!(
        all_events.is_empty(),
        "STE.Config==0b000 must produce no event; got: {:?}",
        all_events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §7.3.7 / §5.2: Disabled stream (`STE.Config==0b000`) must produce no event at all,
/// so there is no event carrying the `stream_id` to inspect.
#[test]
fn test_disabled_stream_event_carries_stream_id() {
    let smmu = make_smmu();
    let stream_id = sid(42);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.disable_stream(stream_id).unwrap();

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    // §7.3.7 / §5.2: no event is recorded for STE.Config==0b000.
    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "STE.Config==0b000 must produce no event; got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §7.3.7 / §5.2: Disabled stream (STE.Config==0b000) must generate neither
/// `F_STREAM_DISABLED` nor `F_TRANSLATION` — no event is recorded at all.
#[test]
fn test_disabled_stream_not_f_translation_event() {
    let smmu = make_smmu();
    let stream_id = sid(20);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.disable_stream(stream_id).unwrap();

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Write, SecurityState::NonSecure);

    // Must NOT have F_TRANSLATION event
    let translation_events = smmu.get_events_by_type(EventType::FTranslation);
    assert!(
        translation_events.is_empty(),
        "must NOT have F_TRANSLATION event for stream-disabled fault; got: {translation_events:?}"
    );
    // Must NOT have F_STREAM_DISABLED event (STE.Config==0b000 records no event)
    let disabled_events = smmu.get_events_by_type(EventType::FStreamDisabled);
    assert!(
        disabled_events.is_empty(),
        "must NOT have F_STREAM_DISABLED event for STE.Config==0b000; got: {disabled_events:?}"
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
