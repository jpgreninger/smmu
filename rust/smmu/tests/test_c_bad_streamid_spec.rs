//! Spec-compliance tests for `C_BAD_STREAMID` and `C_BAD_STE` event generation.
//!
//! ARM IHI0070G.b §7.3.3 `C_BAD_STREAMID`: fired when a StreamID is OUTSIDE the
//! configured stream table range (StreamID >= 2^LOG2SIZE).  Gated by CR2.RECINVSID.
//!
//! ARM IHI0070G.b §7.3.5 `C_BAD_STE`: fired when a StreamID is WITHIN the configured
//! table bounds but STE.V=0 (stream not configured).  NOT gated by RECINVSID.
//!
//! SPEC-20 fix: previous tests incorrectly expected `C_BAD_STREAMID` for in-range
//! unconfigured streams.  All tests below now match the corrected spec behavior.
//!
//! FINDING-NEW-02 — tests for §7.3.3 `C_BAD_STREAMID` (out-of-range StreamID).

use smmu::types::{AccessType, EventType, SecurityState, StreamID, IOVA, PASID};
use smmu::SMMU;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// SMMU with LOG2SIZE=8 (valid range 0..=255) and RECINVSID=1 so that
/// out-of-range `C_BAD_STREAMID` events are recorded.
fn make_smmu_out_of_range() -> SMMU {
    let smmu = SMMU::new();
    // BUG-AUDIT-60 fix: CR2 is RO when SMMUEN=1 (ARM §6.3.12); set CR2 before enable().
    // §6.3.12 / BUG-NEW-RUST-2: RECINVSID=1 is required for C_BAD_STREAMID events
    // to be written to the event queue on out-of-range StreamID translate() calls.
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    smmu.enable().unwrap();
    // LOG2SIZE=8: StreamIDs 0..=255 are in-range; >= 256 are out-of-range.
    smmu.set_strtab_log2size(8);
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

// ─── C_BAD_STREAMID tests (out-of-range StreamID) ───────────────────────────

/// §7.3.3: Translation on an out-of-range StreamID must queue `C_BAD_STREAMID` (0x02).
///
/// SPEC-20 correction: "unknown stream" means OUT-OF-RANGE (>= 2^LOG2SIZE), not
/// "in-range but unconfigured".  An in-range unconfigured stream produces `C_BAD_STE`.
#[test]
fn test_unknown_stream_generates_c_bad_streamid_event() {
    let smmu = make_smmu_out_of_range();
    // StreamID 300 is out-of-range for LOG2SIZE=8 (limit = 256).
    let stream_id = sid(300);

    let result = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "expected error for out-of-range stream");

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(
        !events.is_empty(),
        "expected C_BAD_STREAMID event (0x02) for out-of-range StreamID; got: {:?}",
        smmu.get_events().iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §7.3.3: The event for an out-of-range StreamID must NOT be `F_TRANSLATION`.
#[test]
fn test_unknown_stream_not_f_translation_event() {
    let smmu = make_smmu_out_of_range();
    // StreamID 500 is out-of-range for LOG2SIZE=8 (limit = 256).
    let stream_id = sid(500);

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Write, SecurityState::NonSecure);

    let f_translation = smmu.get_events_by_type(EventType::FTranslation);
    let c_bad = smmu.get_events_by_type(EventType::CBadStreamid);

    assert!(!c_bad.is_empty(), "must have C_BAD_STREAMID event for out-of-range StreamID");
    assert!(
        f_translation.is_empty(),
        "must NOT have F_TRANSLATION for out-of-range StreamID fault; got: {f_translation:?}"
    );
}

/// §7.3.3: `C_BAD_STREAMID` event must record the correct `stream_id` field.
#[test]
fn test_c_bad_streamid_event_carries_stream_id() {
    let smmu = make_smmu_out_of_range();
    // StreamID 400 is out-of-range for LOG2SIZE=8 (limit = 256).
    let stream_id = sid(400);

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(!events.is_empty(), "expected C_BAD_STREAMID event for out-of-range StreamID");
    assert_eq!(events[0].stream_id, 400, "stream_id must match the requested StreamID");
}

/// §7.3.3: Multiple out-of-range StreamID translations each produce a `C_BAD_STREAMID` event.
#[test]
fn test_multiple_unknown_streams_each_generate_c_bad_streamid() {
    let smmu = make_smmu_out_of_range();

    // All StreamIDs below are out-of-range for LOG2SIZE=8 (limit = 256).
    for n in [300u32, 400, 500] {
        let _ = smmu.translate(sid(n), pasid_zero(), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    }

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert_eq!(events.len(), 3, "expected one C_BAD_STREAMID per out-of-range StreamID");
}

/// §7.3.3: Security state must be preserved in the `C_BAD_STREAMID` event.
#[test]
fn test_c_bad_streamid_event_carries_security_state() {
    let smmu = make_smmu_out_of_range();
    // StreamID 600 is out-of-range for LOG2SIZE=8 (limit = 256).
    let stream_id = sid(600);

    let _ = smmu.translate(stream_id, pasid_zero(), iova_1000(), AccessType::Read, SecurityState::Secure);

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(!events.is_empty(), "expected C_BAD_STREAMID event for out-of-range StreamID");
    assert_eq!(
        events[0].security_state,
        SecurityState::Secure,
        "security_state must be preserved in the C_BAD_STREAMID event"
    );
}
