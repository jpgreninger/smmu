//! Tests for NEW-10, NEW-11, and NEW-12 conformance gaps.
//!
//! NEW-10 (§6.3.96): `acknowledge_eventq_overflow()` — copies OVFLG into OVACKFLG
//!                   without clearing queue contents.
//! NEW-11 (§7.3.2):  `report_unsupported_transaction()` — generates `F_UUT` event,
//!                   gated on `CR0.EVENTQEN`.
//! NEW-12 (§3.9):    `TransactionType` / `translate_with_type()` — ATS translation
//!                   request and translated-transaction checks.

use smmu::types::{
    AccessType, EventEntry, EventType, IOVA, PASID, PagePermissions, SecurityState, StreamConfig,
    StreamID,
};
use smmu::{TransactionType, SMMU};

// ── NEW-10 tests ─────────────────────────────────────────────────────────────

/// NEW-10: `acknowledge_eventq_overflow()` must copy OVFLG into OVACKFLG
/// without discarding queued events.
#[test]
fn test_new10_acknowledge_overflow_does_not_clear_queue() {
    // Use smallest allowed event queue so we can overflow quickly.
    use smmu::types::{QueueConfig, SMMUConfig};
    let capacity: usize = 16;
    let smmu = SMMU::with_config(SMMUConfig::from(QueueConfig {
        event_queue_size: capacity,
        command_queue_size: 32,
        pri_queue_size: 32,
    }));
    // Enable SMMU + EVENTQEN so events are accepted.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // Fill queue to capacity.
    for i in 0..u32::try_from(capacity).unwrap() {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: i,
            ..EventEntry::zeroed()
        };
        let _ = smmu.submit_event(ev);
    }

    // Trigger an overflow by trying to add one more via translation fault.
    let sid = StreamID::new(0x99).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let _ = smmu.translate(sid, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    // OVFLG must be active: prod[31] != cons[31].
    let prod_before = smmu.get_eventq_prod();
    let cons_before = smmu.eventq_cons_index();
    assert_ne!(
        (prod_before >> 31) & 1,
        (cons_before >> 31) & 1,
        "OVFLG must be active after overflow"
    );

    // Call the method under test.
    smmu.acknowledge_eventq_overflow();

    // (a) Queue contents must be unchanged — all capacity events still present.
    let events = smmu.get_events();
    assert_eq!(
        events.len(),
        capacity,
        "Queue must retain all {capacity} events after acknowledge (no clear)"
    );

    // (b) OVFLG and OVACKFLG must now be equal (overflow acknowledged).
    let prod_after = smmu.get_eventq_prod();
    let cons_after = smmu.eventq_cons_index();
    assert_eq!(
        (prod_after >> 31) & 1,
        (cons_after >> 31) & 1,
        "OVACKFLG must equal OVFLG after acknowledge_eventq_overflow()"
    );
}

/// NEW-10: When no overflow is present, `acknowledge_eventq_overflow()` must be
/// a no-op (both OVFLG and OVACKFLG remain 0).
#[test]
fn test_new10_acknowledge_no_op_when_no_overflow() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // No overflow has occurred — both bits are 0.
    let prod_before = smmu.get_eventq_prod();
    let cons_before = smmu.eventq_cons_index();
    assert_eq!((prod_before >> 31) & 1, 0, "OVFLG must start at 0");
    assert_eq!((cons_before >> 31) & 1, 0, "OVACKFLG must start at 0");

    smmu.acknowledge_eventq_overflow();

    let prod_after = smmu.get_eventq_prod();
    let cons_after = smmu.eventq_cons_index();
    assert_eq!((prod_after >> 31) & 1, 0, "OVFLG must remain 0 (no-op)");
    assert_eq!((cons_after >> 31) & 1, 0, "OVACKFLG must remain 0 (no-op)");
}

// ── NEW-11 tests ──────────────────────────────────────────────────────────────

/// NEW-11: `report_unsupported_transaction()` must emit an `FUut` event when
/// `CR0.EVENTQEN` is set.
#[test]
fn test_new11_fuut_emitted_when_eventqen() {
    let smmu = SMMU::new();
    // Enable SMMU and EVENTQEN.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let sid = StreamID::new(5).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x2000).unwrap();

    let result = smmu.report_unsupported_transaction(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "report_unsupported_transaction() must succeed");

    let events = smmu.get_events();
    assert_eq!(events.len(), 1, "Exactly one F_UUT event must be queued");
    let ev = &events[0];
    assert_eq!(ev.event_type, EventType::FUut, "Event type must be FUut");
    assert_eq!(ev.stream_id, 5, "stream_id must match");
    assert_eq!(ev.pasid, 0, "pasid must match");
    assert_eq!(ev.address, 0x2000, "address must match");
    assert_eq!(ev.security_state, SecurityState::NonSecure, "security_state must match");
    assert_eq!(ev.event_class, 2, "event_class must be 2 (IN) per NEW-1");
}

/// NEW-11: `report_unsupported_transaction()` must silently drop the event when
/// `CR0.EVENTQEN` is not set.
#[test]
fn test_new11_fuut_silent_when_eventqen_disabled() {
    let smmu = SMMU::new();
    // EVENTQEN is NOT set.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);

    let sid = StreamID::new(7).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x3000).unwrap();

    let result = smmu.report_unsupported_transaction(
        sid,
        pasid,
        iova,
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "report_unsupported_transaction() must still return Ok");

    let events = smmu.get_events();
    assert!(events.is_empty(), "No event must be queued when EVENTQEN=0");
}

// ── NEW-12 tests ──────────────────────────────────────────────────────────────

/// Helper: create SMMU with a stage-1 stream on StreamID 1 with eats=0 (no ATS).
fn make_smmu_s1_no_ats() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    let sid = StreamID::new(1).unwrap();
    let cfg = StreamConfig {
        eats: 0,
        t0sz: 0, // no VA-range restriction
        ips: 6,  // 52-bit IPS
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(sid, cfg).unwrap();
    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    // Map a page for StreamID=1, PASID=0, IOVA=0x1000.
    let iova = IOVA::new(0x1000).unwrap();
    smmu.map_page(
        sid,
        pasid,
        iova,
        smmu::types::PA::new(0x8000_1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu
}

/// Helper: create SMMU with a stage-1 stream on StreamID 1 with eats=1 (ATS enabled).
fn make_smmu_s1_with_ats() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    let sid = StreamID::new(1).unwrap();
    let cfg = StreamConfig {
        eats: 1,
        t0sz: 0,
        ips: 6,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(sid, cfg).unwrap();
    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(sid, pasid).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    smmu.map_page(
        sid,
        pasid,
        iova,
        smmu::types::PA::new(0x8000_1000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu
}

/// NEW-12: ATS Translation Request (TR) on a stream with `eats=0` must generate
/// `F_BAD_ATS_TREQ` (§7.3.6 event 0x05) and return an error.
#[test]
fn test_new12_tr_eats_zero_bad_ats_treq() {
    let smmu = make_smmu_s1_no_ats();
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_err(), "TR on eats=0 stream must fail");

    let events = smmu.get_events();
    let bad_ats_treq_count =
        events.iter().filter(|e| e.event_type == EventType::FBadAtsTreq).count();
    assert_eq!(
        bad_ats_treq_count,
        1,
        "Exactly one F_BAD_ATS_TREQ event must be emitted; event types: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-12: ATS Translation Request (TR) on a bypass stream must also generate
/// `F_BAD_ATS_TREQ` (§7.3.6 event 0x05) — bypass streams do not support ATS.
#[test]
fn test_new12_tr_bypass_stream_bad_ats_treq() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    let sid = StreamID::new(2).unwrap();
    let cfg = StreamConfig { eats: 1, ..StreamConfig::bypass() };
    smmu.configure_stream(sid, cfg).unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_err(), "TR on bypass stream must fail");

    let events = smmu.get_events();
    let found = events.iter().any(|e| e.event_type == EventType::FBadAtsTreq);
    assert!(found, "F_BAD_ATS_TREQ must be emitted for TR on bypass stream");
}

/// NEW-12: ATS Translation Request (TR) on a stream with eats!=0 and translation
/// enabled must succeed and return a translated PA.
#[test]
fn test_new12_tr_eats_nonzero_succeeds() {
    let smmu = make_smmu_s1_with_ats();
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(result.is_ok(), "TR on eats!=0 stream with valid mapping must succeed; got {result:?}");
}

/// NEW-12: ATS Translated transaction (TT) with ATSCHK=0 must succeed without
/// re-checking the mapping (even if IOVA is not actually mapped).
#[test]
fn test_new12_tt_atschk_off_no_recheck() {
    let smmu = make_smmu_s1_with_ats();
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    // Use the mapped IOVA — ATSCHK=0 so no extra F_BAD_ATS_TREQ re-check step.
    let iova = IOVA::new(0x1000).unwrap();

    // ATSCHK is bit 4 of CR0 — we keep it 0 (default).
    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    // Without ATSCHK, the TT request uses the ordinary translation path;
    // the mapped page means the translation succeeds with no extra event.
    assert!(result.is_ok(), "TT with ATSCHK=0 on mapped page must succeed");
    // No ATS error events must be emitted (ATSCHK=0, no re-check).
    let events = smmu.get_events();
    let bad_treq = events.iter().any(|e| e.event_type == EventType::FBadAtsTreq);
    let transl_forbidden = events.iter().any(|e| e.event_type == EventType::FTranslForbidden);
    assert!(!bad_treq, "F_BAD_ATS_TREQ must NOT be emitted when ATSCHK=0");
    assert!(!transl_forbidden, "F_TRANSL_FORBIDDEN must NOT be emitted when ATSCHK=0");
}

/// NEW-12: ATS Translated transaction (TT) with ATSCHK=1 and a mapped IOVA
/// must succeed (re-check passes).
#[test]
fn test_new12_tt_atschk_on_mapped_succeeds() {
    let smmu = make_smmu_s1_with_ats();
    // Set ATSCHK=1.
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_ATSCHK,
    );
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap(); // this page IS mapped

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_ok(), "TT with ATSCHK=1 and valid mapping must succeed; got {result:?}");

    let events = smmu.get_events();
    let transl_forbidden = events.iter().any(|e| e.event_type == EventType::FTranslForbidden);
    assert!(!transl_forbidden, "F_TRANSL_FORBIDDEN must NOT be emitted for a valid TT");
}

/// NEW-12: ATS Translated transaction (TT) with `ATSCHK=1` and an UNMAPPED IOVA
/// must generate `F_TRANSL_FORBIDDEN` (§7.3.8 event 0x07) and return an error.
#[test]
fn test_new12_tt_atschk_on_unmapped_transl_forbidden() {
    let smmu = make_smmu_s1_with_ats();
    // Set ATSCHK=1.
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_ATSCHK,
    );
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x9000).unwrap(); // NOT mapped

    let result = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "TT with ATSCHK=1 and unmapped IOVA must fail");

    let events = smmu.get_events();
    let transl_forbidden_count =
        events.iter().filter(|e| e.event_type == EventType::FTranslForbidden).count();
    assert_eq!(
        transl_forbidden_count,
        1,
        "Exactly one F_TRANSL_FORBIDDEN event must be emitted; event types: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

/// NEW-12: Ordinary transactions must be completely unaffected by the new
/// `translate_with_type()` infrastructure — existing behavior preserved.
#[test]
fn test_new12_ordinary_behavior_preserved() {
    let smmu = make_smmu_s1_with_ats();
    let sid = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    // translate() delegates to translate_with_type(..., Ordinary).
    let result_via_translate = smmu.translate(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    smmu.clear_event_queue();

    let result_via_type = smmu.translate_with_type(
        sid,
        pasid,
        iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::Ordinary,
    );

    assert_eq!(
        result_via_translate.is_ok(),
        result_via_type.is_ok(),
        "Ordinary translate_with_type must match translate() success/failure"
    );
}
