//! Spec-compliance tests for `C_BAD_SUBSTREAMID` event generation
//!
//! ARM IHI0070G.b §3.9: "Supply of a PASID or `SubstreamID` to a configuration
//! without stage 1 translation causes the translation to fail. Such
//! transactions are terminated with an abort and `C_BAD_SUBSTREAMID` is
//! recorded."
//!
//! When a non-zero PASID is presented to a stream configured as:
//!   - Stage-2-only (`stage1_enabled=false, stage2_enabled=true`), OR
//!   - Bypass (`stage1_enabled=false, stage2_enabled=false`)
//!
//! the SMMU must abort with event `C_BAD_SUBSTREAMID` (`EventType::CBadSubstreamid`,
//! code 0x08).
//!
//! FINDING-NEW-11

use smmu::types::{
    AccessType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// --- Helpers ------------------------------------------------------------------

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova_1000() -> IOVA {
    IOVA::new(0x1000).unwrap()
}

// --- Tests -------------------------------------------------------------------

/// §7.3.9: `C_BAD_SUBSTREAMID` event opcode must be 0x08.
#[test]
fn event_opcode_is_0x08() {
    assert_eq!(
        EventType::CBadSubstreamid as u32,
        0x08,
        "C_BAD_SUBSTREAMID event code must be 0x08 per §7.3.9"
    );
}

/// §3.9: Stage-2-only stream + non-zero PASID must return an error.
#[test]
fn stage2_only_nonzero_pasid_fails() {
    let smmu = make_smmu();
    let stream_id = sid(1);

    smmu.configure_stream(stream_id, StreamConfig::stage2_only()).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    let ipa = iova_1000();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_stage2_page(stream_id, ipa, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "expected error for non-zero PASID on stage-2-only stream; got: {result:?}"
    );
}

/// §3.9: Stage-2-only stream + non-zero PASID must generate `C_BAD_SUBSTREAMID` event.
#[test]
fn stage2_only_nonzero_pasid_generates_event() {
    let smmu = make_smmu();
    let stream_id = sid(2);

    smmu.configure_stream(stream_id, StreamConfig::stage2_only()).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    let ipa = iova_1000();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_stage2_page(stream_id, ipa, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let _ = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        !events.is_empty(),
        "expected C_BAD_SUBSTREAMID event; got events: {:?}",
        smmu.get_events().iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §3.9: Stage-2-only stream + PASID=0 must succeed (no `C_BAD_SUBSTREAMID`).
#[test]
fn stage2_only_pasid_zero_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(3);

    smmu.configure_stream(stream_id, StreamConfig::stage2_only()).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    let ipa = iova_1000();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_stage2_page(stream_id, ipa, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "PASID=0 on stage-2-only stream must succeed; got: {result:?}"
    );

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        events.is_empty(),
        "PASID=0 must NOT generate C_BAD_SUBSTREAMID; got: {events:?}"
    );
}

/// §3.9: Bypass stream + non-zero PASID must return an error.
#[test]
fn bypass_nonzero_pasid_fails() {
    let smmu = make_smmu();
    let stream_id = sid(4);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let result = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "expected error for non-zero PASID on bypass stream; got: {result:?}"
    );
}

/// §3.9: Bypass stream + non-zero PASID must generate `C_BAD_SUBSTREAMID` event.
#[test]
fn bypass_nonzero_pasid_generates_event() {
    let smmu = make_smmu();
    let stream_id = sid(5);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let _ = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        !events.is_empty(),
        "expected C_BAD_SUBSTREAMID event for bypass + non-zero PASID; got events: {:?}",
        smmu.get_events().iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// §3.9: Bypass stream + PASID=0 must succeed (identity mapping, no error, no event).
#[test]
fn bypass_pasid_zero_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(6);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let result = smmu.translate(stream_id, pasid(0), iova_1000(), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "PASID=0 on bypass stream must succeed (identity PA); got: {result:?}"
    );

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        events.is_empty(),
        "PASID=0 bypass must NOT generate C_BAD_SUBSTREAMID; got: {events:?}"
    );
}

/// §3.9: Stage-1-only stream + non-zero PASID must NOT generate `C_BAD_SUBSTREAMID`.
#[test]
fn stage1_only_nonzero_pasid_is_ok() {
    let smmu = make_smmu();
    let stream_id = sid(7);

    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();
    let pa = PA::new(0x3000).unwrap();
    smmu.map_page(stream_id, pasid(1), iova_1000(), pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let _ = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        events.is_empty(),
        "stage-1-only + non-zero PASID must NOT generate C_BAD_SUBSTREAMID; got: {events:?}"
    );
}

/// §3.9: Two-stage stream + non-zero PASID must NOT generate `C_BAD_SUBSTREAMID`.
#[test]
fn both_stages_nonzero_pasid_is_ok() {
    let smmu = make_smmu();
    let stream_id = sid(8);

    smmu.configure_stream(stream_id, StreamConfig::two_stage()).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();

    // Attempt translation (may fail due to missing mappings — that is fine).
    // What matters is that no C_BAD_SUBSTREAMID event is generated.
    let _ = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        events.is_empty(),
        "two-stage + non-zero PASID must NOT generate C_BAD_SUBSTREAMID; got: {events:?}"
    );
}

/// §3.9 / §7.3.9: `C_BAD_SUBSTREAMID` event must record the correct `stream_id`.
#[test]
fn stage2_only_event_carries_stream_id() {
    let smmu = make_smmu();
    let stream_id = sid(42);

    smmu.configure_stream(stream_id, StreamConfig::stage2_only()).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();
    let pa = PA::new(0x5000).unwrap();
    smmu.map_stage2_page(stream_id, iova_1000(), pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let _ = smmu.translate(stream_id, pasid(1), iova_1000(), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(!events.is_empty(), "expected C_BAD_SUBSTREAMID event");
    assert_eq!(
        events[0].stream_id,
        42,
        "event stream_id must match the faulting StreamID"
    );
}
