//! Spec-compliance tests for `C_BAD_STREAMID` event generation
//!
//! ARM IHI0070G.b §7.3.3: When a StreamID is not found in the stream table,
//! the SMMU must record a `C_BAD_STREAMID` event (event code 0x02) in the
//! event queue — NOT a generic `F_TRANSLATION` (0x10).
//!
//! FINDING-NEW-02 — tests must be RED before the fix.

use smmu::types::{AccessType, EventType, SecurityState, StreamID, IOVA, PASID};
use smmu::SMMU;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    // §6.3.12 / BUG-NEW-RUST-2: CR2.RECINVSID=1 is required for C_BAD_STREAMID events
    // to be written to the event queue on invalid-stream translate() calls.
    smmu.set_cr2(SMMU::CR2_RECINVSID);
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

// ─── Tests ────────────────────────────────────────────────────────────────────

/// §7.3.3: Translation on an unconfigured StreamID must queue `C_BAD_STREAMID` (0x02).
#[test]
fn test_unknown_stream_generates_c_bad_streamid_event() {
    let smmu = make_smmu();
    // StreamID 99 is never configured — stream table lookup will fail.
    let stream_id = sid(99);

    let result = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "expected error for unconfigured stream");

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(
        !events.is_empty(),
        "expected C_BAD_STREAMID event (0x02); got: {:?}",
        smmu.get_events().iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §7.3.3: The event must NOT be `F_TRANSLATION` for an unknown StreamID.
#[test]
fn test_unknown_stream_not_f_translation_event() {
    let smmu = make_smmu();
    let stream_id = sid(77);

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Write, SecurityState::NonSecure);

    let f_translation = smmu.get_events_by_type(EventType::FTranslation);
    let c_bad = smmu.get_events_by_type(EventType::CBadStreamid);

    assert!(!c_bad.is_empty(), "must have C_BAD_STREAMID event");
    assert!(
        f_translation.is_empty(),
        "must NOT have F_TRANSLATION for unknown-stream fault; got: {f_translation:?}"
    );
}

/// §7.3.3: `C_BAD_STREAMID` event must record the correct `stream_id` field.
#[test]
fn test_c_bad_streamid_event_carries_stream_id() {
    let smmu = make_smmu();
    let stream_id = sid(42);

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(!events.is_empty(), "expected C_BAD_STREAMID event");
    assert_eq!(events[0].stream_id, 42, "stream_id must match the requested StreamID");
}

/// §7.3.3: Multiple unknown-stream translations each produce a `C_BAD_STREAMID` event.
#[test]
fn test_multiple_unknown_streams_each_generate_c_bad_streamid() {
    let smmu = make_smmu();

    for n in [10u32, 20, 30] {
        let _ = smmu.translate(sid(n), pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    }

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert_eq!(events.len(), 3, "expected one C_BAD_STREAMID per unknown stream");
}

/// §7.3.3: Security state must be preserved in the `C_BAD_STREAMID` event.
#[test]
fn test_c_bad_streamid_event_carries_security_state() {
    let smmu = make_smmu();
    let stream_id = sid(55);

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::Secure);

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(!events.is_empty(), "expected C_BAD_STREAMID event");
    assert_eq!(
        events[0].security_state,
        SecurityState::Secure,
        "security_state must be preserved in the event"
    );
}
