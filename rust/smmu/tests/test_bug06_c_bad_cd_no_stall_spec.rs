#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! BUG-06: C_BAD_CD Must Not Stall (ARM IHI0070G.b §7.3.11 / §3.12.2)
//!
//! Spec: C_BAD_CD is a configuration fault, not a translation fault.
//! It must ALWAYS abort — never stall, regardless of stream's FaultMode.
//!
//! Faults that must NEVER stall (ARM §11.639):
//! C_BAD_STREAMID, C_BAD_STE, C_BAD_SUBSTREAMID, C_BAD_CD,
//! F_STE_FETCH, F_CD_FETCH, F_BAD_ATS_TREQ, F_TRANSL_FORBIDDEN
//!
//! Only F_TRANSLATION, F_ADDR_SIZE, F_ACCESS, F_PERMISSION may stall.

use smmu::types::{AccessType, EventType, FaultMode, SecurityState, StreamConfig, StreamID, IOVA, PASID};
use smmu::SMMU;

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

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

/// §7.3.11 / §3.12.2 / BUG-06: C_BAD_CD must never stall, even on a
/// stall-mode stream, when PASID is not found (PASIDNotFound → BadCD).
///
/// C_BAD_CD occurs when PASID != 0 but there is no CD for that PASID.
/// The result must be an error (abort), NOT Stalled.
///
/// This test FAILS before the fix because PASIDNotFound is incorrectly
/// included in the stall-eligible set and returns Stalled { stag }.
#[test]
fn bug06_c_bad_cd_pasid_not_found_must_not_stall() {
    let smmu = make_smmu();

    // Configure a stall-mode Stage-1 stream.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(1), cfg).unwrap();
    // Create PASID 0 but NOT PASID 5 — PASID 5 will be not-found → C_BAD_CD.
    smmu.create_pasid(sid(1), pasid(0)).unwrap();

    // Translate with PASID 5 (not configured) on a stall-mode stream.
    let result = smmu.translate(
        sid(1),
        pasid(5),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Must NOT return Stalled — C_BAD_CD is a configuration fault (§7.3.11).
    assert!(
        !matches!(result, Err(smmu::types::TranslationError::Stalled { .. })),
        "BUG-06/§7.3.11: C_BAD_CD (PASIDNotFound) must never stall on a stall-mode stream; \
         got: {result:?}"
    );
    // Must return an error (abort).
    assert!(
        result.is_err(),
        "BUG-06: C_BAD_CD must return an error (abort); got Ok"
    );
}

/// §7.3.11 / §3.12.2 / BUG-06: C_BAD_CD event (CBadCd) must be recorded
/// when PASIDNotFound occurs, NOT CBadSubstreamId.
///
/// The fault classification must correctly identify this as C_BAD_CD.
#[test]
fn bug06_c_bad_cd_pasid_not_found_records_correct_event_type() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(2), cfg).unwrap();
    smmu.create_pasid(sid(2), pasid(0)).unwrap();

    // Translate with PASID 7 (not configured) — should generate C_BAD_CD.
    let _ = smmu.translate(
        sid(2),
        pasid(7),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    // Must have at least one C_BAD_CD event.
    let has_bad_cd = events.iter().any(|e| e.event_type == EventType::CBadCd);
    assert!(
        has_bad_cd,
        "BUG-06: C_BAD_CD event (CBadCd) must be recorded for PASIDNotFound; \
         got events: {events:?}"
    );
    // Must NOT misclassify as C_BAD_SUBSTREAMID.
    let has_bad_substreamid = events.iter().any(|e| e.event_type == EventType::CBadSubstreamid);
    assert!(
        !has_bad_substreamid,
        "BUG-06: PASIDNotFound must NOT generate CBadSubstreamid; got events: {events:?}"
    );
}

/// §7.3.11 / §3.12.2 / BUG-06: C_BAD_CD event must have stall==false.
///
/// Even on a stall-mode stream, C_BAD_CD events must not be marked as stall
/// events since the fault must abort, not stall.
#[test]
fn bug06_c_bad_cd_event_stall_field_must_be_false() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(3), cfg).unwrap();
    smmu.create_pasid(sid(3), pasid(0)).unwrap();

    let _ = smmu.translate(
        sid(3),
        pasid(9),
        iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    for event in &events {
        assert!(
            !event.stall,
            "BUG-06: C_BAD_CD events must have stall==false; got event: {event:?}"
        );
    }
}

/// §3.12.2 / BUG-06: Confirm stall-eligible faults (F_TRANSLATION) still stall
/// normally — the fix must not break legitimate stall-mode operation.
#[test]
fn bug06_regression_normal_stall_still_works_after_fix() {
    use smmu::types::{PagePermissions, PA};

    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(4), cfg).unwrap();
    smmu.create_pasid(sid(4), pasid(0)).unwrap();
    // Map one page so the stream is valid.
    smmu.map_page(
        sid(4),
        pasid(0),
        iova(0x1000),
        PA::new(0x8000_0000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translate unmapped address — F_TRANSLATION — must stall.
    let result = smmu.translate(
        sid(4),
        pasid(0),
        iova(0x9000), // unmapped
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(smmu::types::TranslationError::Stalled { .. })),
        "BUG-06 regression: normal stall-mode F_TRANSLATION must still stall; got: {result:?}"
    );
}
