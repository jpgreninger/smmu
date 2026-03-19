//! SPEC-20: In-bounds unconfigured stream must emit C_BAD_STE, not C_BAD_STREAMID.
//!
//! ARM IHI0070G.b §7.3.3 C_BAD_STREAMID: fires when StreamID is OUTSIDE the
//! configured table range (>= 2^LOG2SIZE).
//!
//! ARM IHI0070G.b §7.3.5 C_BAD_STE: fires when StreamID is WITHIN table bounds
//! but STE.V=0 (unconfigured / stream not found in the stream table).
//!
//! ARM IHI0070G.b §6.3.12 RECINVSID: gates recording of C_BAD_STREAMID ONLY.
//! C_BAD_STE must be recorded regardless of RECINVSID.
//!
//! Tests are RED before the fix.
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

use smmu::types::{AccessType, EventType, SecurityState, StreamID, IOVA, PASID};
use smmu::SMMU;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid_zero() -> PASID {
    PASID::new(0).unwrap()
}

fn iova_1000() -> IOVA {
    IOVA::new(0x1000).unwrap()
}

/// Build an SMMU with a stream table of LOG2SIZE=8 (valid StreamIDs 0..=255).
/// RECINVSID is intentionally NOT set, to verify C_BAD_STE is never gated by it.
fn make_smmu_with_log2size_8() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    // LOG2SIZE=8: valid StreamIDs are 0..=255; StreamID >= 256 is out-of-range.
    smmu.set_strtab_log2size(8);
    // RECINVSID deliberately left at 0 to prove C_BAD_STE is NOT gated by it.
    smmu
}

// ─── SPEC-20 Tests ───────────────────────────────────────────────────────────

/// SPEC-20-A: In-range unconfigured stream (STE.V=0) must emit C_BAD_STE (0x04),
/// NOT C_BAD_STREAMID (0x02).
///
/// StreamID=50 is within the LOG2SIZE=8 table (0..=255) but not configured.
#[test]
fn spec20_a_in_range_unconfigured_stream_emits_c_bad_ste() {
    let smmu = make_smmu_with_log2size_8();

    // StreamID=50 is within range (< 256) but not configured — STE.V=0.
    let result = smmu.translate(
        sid(50),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "expected error for unconfigured in-range stream");

    let events = smmu.get_events();
    let c_bad_ste = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .count();
    let c_bad_streamid = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadStreamid)
        .count();

    assert_eq!(
        c_bad_ste, 1,
        "SPEC-20: in-range unconfigured stream must emit C_BAD_STE (0x04); \
         got events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        c_bad_streamid, 0,
        "SPEC-20: in-range unconfigured stream must NOT emit C_BAD_STREAMID (0x02); \
         got events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// SPEC-20-B: Out-of-range StreamID (>= 2^LOG2SIZE) must still emit C_BAD_STREAMID (0x02).
///
/// StreamID=300 is outside LOG2SIZE=8 table (0..=255), so it is a genuine
/// C_BAD_STREAMID fault — not C_BAD_STE.
#[test]
fn spec20_b_out_of_range_stream_emits_c_bad_streamid() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_strtab_log2size(8);
    // Set RECINVSID so out-of-range C_BAD_STREAMID events are recorded.
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // StreamID=300 is out of range for LOG2SIZE=8 (limit is 256).
    let result = smmu.translate(
        sid(300),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "expected error for out-of-range stream");

    let events = smmu.get_events();
    let c_bad_streamid = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadStreamid)
        .count();
    let c_bad_ste = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .count();

    assert_eq!(
        c_bad_streamid, 1,
        "SPEC-20: out-of-range stream must emit C_BAD_STREAMID (0x02); \
         got events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        c_bad_ste, 0,
        "SPEC-20: out-of-range stream must NOT emit C_BAD_STE (0x04); \
         got events: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// SPEC-20-C: C_BAD_STE must NOT be gated by RECINVSID (§6.3.12).
///
/// RECINVSID only gates C_BAD_STREAMID. C_BAD_STE must always be recorded
/// when CR0.EVENTQEN=1, regardless of RECINVSID value.
#[test]
fn spec20_c_c_bad_ste_not_gated_by_recinvsid() {
    // Make two SMMUs: one with RECINVSID=0, one with RECINVSID=1.
    // Both must emit C_BAD_STE for an in-range unconfigured stream.

    let smmu_no_recinvsid = make_smmu_with_log2size_8();
    // RECINVSID remains 0 (not set)
    let _ = smmu_no_recinvsid.translate(
        sid(50),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let events_no_rec = smmu_no_recinvsid.get_events();
    let c_bad_ste_no_rec = events_no_rec
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .count();

    let smmu_with_recinvsid = make_smmu_with_log2size_8();
    smmu_with_recinvsid.set_cr2(SMMU::CR2_RECINVSID);
    let _ = smmu_with_recinvsid.translate(
        sid(50),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let events_with_rec = smmu_with_recinvsid.get_events();
    let c_bad_ste_with_rec = events_with_rec
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .count();

    assert_eq!(
        c_bad_ste_no_rec, 1,
        "SPEC-20-C: C_BAD_STE must be recorded even when RECINVSID=0; \
         got events: {:?}",
        events_no_rec.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        c_bad_ste_with_rec, 1,
        "SPEC-20-C: C_BAD_STE must be recorded when RECINVSID=1 too; \
         got events: {:?}",
        events_with_rec.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// SPEC-20-D: C_BAD_STREAMID (out-of-range) must still be gated by RECINVSID.
///
/// When RECINVSID=0, the C_BAD_STREAMID event for an out-of-range StreamID
/// must NOT be recorded. When RECINVSID=1, it must be recorded.
#[test]
fn spec20_d_c_bad_streamid_still_gated_by_recinvsid() {
    // With RECINVSID=0: out-of-range stream must NOT produce event.
    let smmu_no_rec = SMMU::new();
    smmu_no_rec.enable().unwrap();
    smmu_no_rec.set_strtab_log2size(8);
    // RECINVSID stays 0
    let _ = smmu_no_rec.translate(
        sid(300),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let events_no_rec = smmu_no_rec.get_events();
    assert!(
        events_no_rec.is_empty(),
        "SPEC-20-D: without RECINVSID, C_BAD_STREAMID event must be suppressed; \
         got: {:?}",
        events_no_rec.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );

    // With RECINVSID=1: out-of-range stream must produce C_BAD_STREAMID event.
    let smmu_with_rec = SMMU::new();
    smmu_with_rec.enable().unwrap();
    smmu_with_rec.set_strtab_log2size(8);
    smmu_with_rec.set_cr2(SMMU::CR2_RECINVSID);
    let _ = smmu_with_rec.translate(
        sid(300),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let events_with_rec = smmu_with_rec.get_events();
    let c_bad_streamid = events_with_rec
        .iter()
        .filter(|e| e.event_type == EventType::CBadStreamid)
        .count();
    assert_eq!(
        c_bad_streamid, 1,
        "SPEC-20-D: with RECINVSID=1, C_BAD_STREAMID must be recorded for out-of-range stream; \
         got: {:?}",
        events_with_rec.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// SPEC-20-E: C_BAD_STE event carries the correct stream_id field.
#[test]
fn spec20_e_c_bad_ste_event_carries_stream_id() {
    let smmu = make_smmu_with_log2size_8();

    let _ = smmu.translate(
        sid(77),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let bad_ste_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .collect();

    assert_eq!(bad_ste_events.len(), 1, "expected exactly one C_BAD_STE event");
    assert_eq!(
        bad_ste_events[0].stream_id, 77,
        "C_BAD_STE event must carry the StreamID that caused the fault"
    );
}

/// SPEC-20-F: No LOG2SIZE set (sentinel=32) — unconfigured stream must emit C_BAD_STE.
///
/// Even without a configured LOG2SIZE limit, an unconfigured stream (STE.V=0)
/// must produce C_BAD_STE, not C_BAD_STREAMID.
#[test]
fn spec20_f_no_log2size_unconfigured_stream_emits_c_bad_ste() {
    // No set_strtab_log2size call — sentinel 32 means no table-size limit.
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    // RECINVSID intentionally not set.

    let _ = smmu.translate(
        sid(42),
        pasid_zero(),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let c_bad_ste = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadSte)
        .count();
    let c_bad_streamid = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadStreamid)
        .count();

    assert_eq!(
        c_bad_ste, 1,
        "SPEC-20-F: unconfigured stream (no LOG2SIZE limit) must emit C_BAD_STE; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        c_bad_streamid, 0,
        "SPEC-20-F: unconfigured stream (no LOG2SIZE limit) must NOT emit C_BAD_STREAMID; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}
