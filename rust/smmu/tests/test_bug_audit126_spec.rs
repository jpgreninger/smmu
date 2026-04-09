//! TDD tests for BUG-AUDIT-126.
//!
//! ARM SMMU v3 spec §3.9.1.2:
//!
//! "A Translation Request that encounters an Address Size, Access or Translation
//!  fault arising from the translation process for a page, at either stage,
//!  results in an ATS Translation Completion with Success status and R==W==0 ...
//!  and NO fault is recorded in the SMMU."
//!
//! Table §3.9.1.2:
//! | F_ADDR_SIZE / F_ACCESS / F_TRANSLATION | Success: R==W==0 (access denied) — no event |
//! | F_PERMISSION                           | Success with subset permissions — no event   |
//!
//! BUG: When an ATS Translation Request passes the EATS/abort pre-checks and reaches
//! the ordinary translate path, encountering an unmapped page (→ F_TRANSLATION) or a
//! permission fault (→ F_PERMISSION) causes record_translation_fault() to be called,
//! recording an event in the event queue. The spec forbids any event for these faults
//! on ATS TRs.
//!
//! BEFORE FIX: F_TRANSLATION / F_PERMISSION / F_ADDR_SIZE events ARE recorded for
//!             ATS Translation Requests that reach the translate path.
//! AFTER FIX:  No fault events are recorded for ATS TRs that encounter these faults.
//!             The result still returns Err (the device gets "denied"), but the SMMU
//!             event queue stays empty.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use smmu::{
    types::{AccessType, EventType, PagePermissions, SecurityState, StreamConfig, TransactionType, IOVA, PA, PASID},
    StreamID, SMMU,
};

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid0() -> PASID { PASID::new(0).unwrap() }

fn make_smmu_s1_ats() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    // Stage-1 only stream with EATS=1 (ATS enabled).
    let cfg = StreamConfig { eats: 1, ..StreamConfig::stage1_only() };
    smmu.configure_stream(sid(1), cfg).unwrap();
    smmu.create_pasid(sid(1), pasid0()).unwrap();
    smmu
}

fn make_smmu_s1_ats_with_ro_page() -> SMMU {
    let smmu = make_smmu_s1_ats();
    // Map page as read-only.
    smmu.map_page(
        sid(1), pasid0(),
        IOVA::new(0x1000).unwrap(),
        PA::new(0x8000_1000).unwrap(),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    ).unwrap();
    smmu
}

// ============================================================================
// BUG-AUDIT-126 Test 1: F_TRANSLATION (unmapped page) → no event on ATS TR
// ============================================================================

/// §3.9.1.2: ATS TR on unmapped IOVA → no F_TRANSLATION event recorded.
///
/// An ATS TR that encounters an unmapped page (F_TRANSLATION) must NOT record
/// any event. The device gets R==W==0 (denied), but the SMMU event queue is empty.
///
/// This test FAILS before the fix because the translate path records F_TRANSLATION.
#[test]
fn test_ats_tr_unmapped_page_no_event() {
    let smmu = make_smmu_s1_ats();
    smmu.clear_event_queue();

    // Translate an unmapped address — will produce F_TRANSLATION in ordinary path.
    let result = smmu.translate_with_type(
        sid(1), pasid0(),
        IOVA::new(0x9999_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    assert!(result.is_err(), "ATS TR on unmapped page must fail (R==W==0 denial)");

    let events = smmu.get_events();
    let translation_events: Vec<_> = events.iter()
        .filter(|e| matches!(e.event_type, EventType::FTranslation | EventType::FAddrSize | EventType::FAccess | EventType::FPermission))
        .collect();
    assert!(
        translation_events.is_empty(),
        "ATS TR on unmapped page must NOT record any fault event per §3.9.1.2; \
         got: {:?}",
        translation_events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// BUG-AUDIT-126 Test 2: F_PERMISSION (write on RO page) → no event on ATS TR
// ============================================================================

/// §3.9.1.2: ATS TR that hits a permission fault → no F_PERMISSION event recorded.
///
/// An ATS TR for write access on a read-only page must NOT record F_PERMISSION.
/// The device gets R=1, W=0 (partial permissions), no SMMU event.
///
/// This test FAILS before the fix because the translate path records F_PERMISSION.
#[test]
fn test_ats_tr_permission_fault_no_event() {
    let smmu = make_smmu_s1_ats_with_ro_page();
    smmu.clear_event_queue();

    // Request write access on read-only page → F_PERMISSION in ordinary path.
    let result = smmu.translate_with_type(
        sid(1), pasid0(),
        IOVA::new(0x1000).unwrap(),
        AccessType::Write,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    assert!(result.is_err(), "ATS TR write on RO page must fail (permission denied)");

    let events = smmu.get_events();
    let perm_events: Vec<_> = events.iter()
        .filter(|e| matches!(e.event_type, EventType::FTranslation | EventType::FAddrSize | EventType::FAccess | EventType::FPermission))
        .collect();
    assert!(
        perm_events.is_empty(),
        "ATS TR permission fault must NOT record any fault event per §3.9.1.2; \
         got: {:?}",
        perm_events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// Regression: Ordinary translate still records events as before
// ============================================================================

/// Regression: Ordinary translate on unmapped page still records F_TRANSLATION.
#[test]
fn test_ordinary_unmapped_page_still_records_event() {
    let smmu = make_smmu_s1_ats();
    smmu.clear_event_queue();

    let result = smmu.translate(
        sid(1), pasid0(),
        IOVA::new(0x9999_0000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "Ordinary translate on unmapped page must fail");

    let events = smmu.get_events();
    let translation_count = events.iter()
        .filter(|e| e.event_type == EventType::FTranslation)
        .count();
    assert_eq!(
        translation_count, 1,
        "Ordinary translate on unmapped page must still record F_TRANSLATION; \
         got: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// Regression: ATS TR on valid mapped+accessible page still returns Ok
// ============================================================================

/// Regression: ATS TR on a valid RW mapping returns Ok (no error, no event).
#[test]
fn test_ats_tr_valid_mapping_ok_no_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    let cfg = StreamConfig { eats: 1, ..StreamConfig::stage1_only() };
    smmu.configure_stream(sid(2), cfg).unwrap();
    smmu.create_pasid(sid(2), pasid0()).unwrap();
    smmu.map_page(
        sid(2), pasid0(),
        IOVA::new(0x2000).unwrap(),
        PA::new(0x8000_2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();
    smmu.clear_event_queue();

    let result = smmu.translate_with_type(
        sid(2), pasid0(),
        IOVA::new(0x2000).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    assert!(result.is_ok(), "ATS TR on valid RW mapping must succeed; got {result:?}");
    let events = smmu.get_events();
    assert!(events.is_empty(), "ATS TR success must not generate events; got {events:?}");
}
