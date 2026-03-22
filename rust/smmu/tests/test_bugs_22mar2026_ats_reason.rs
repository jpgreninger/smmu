//! TDD failing tests for Item 1 (F_BAD_ATS_TREQ ats_r/w/x/p fields) and
//! Item 2 (F_UUT reason field) from the 2026-03-22 spec-gap fixes.
//!
//! # Items Covered
//!
//! ## Item 1 (Moderate) — F_BAD_ATS_TREQ ATS-specific R/W/X/P fields
//!
//! ARM §7.3.6 specifies four permission bits at bits [95:92] of the event record
//! that capture the ATS Translation Request's requested permissions (pre-STE-override).
//! These fields are RES0 for all other event types.
//!
//! ## Item 2 (Low) — F_UUT Reason field
//!
//! ARM §7.3.2 specifies a 16-bit IMPLEMENTATION DEFINED Reason field at bits [79:64].
//! This software model always emits 0, which is a valid IMPDEF choice.
//!
//! # Test Strategy
//!
//! Tests are written to FAIL with the current code (fields don't exist yet) and
//! PASS only after the fields are added to EventEntry and populated at the two
//! FBadAtsTreq emit sites in smmu/mod.rs.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::{TransactionType, SMMU};

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

/// Make a fresh SMMU with SMMUEN=0 but EVENTQEN=1 and REC_CFG_ATS=1.
///
/// This triggers the SMMUEN=0 FBadAtsTreq path in translate_with_type().
fn make_smmu_disabled_rec_cfg_ats() -> SMMU {
    let s = SMMU::new();
    // EVENTQEN=1 and REC_CFG_ATS=1, but NOT SMMUEN — so the SMMUEN=0 path fires.
    s.set_cr0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    s.set_cr2(SMMU::CR2_REC_CFG_ATS);
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

// ============================================================================
// Item 1, Test 1 — FBadAtsTreq ats_r/w/x/p for AccessType::Read
// ============================================================================
//
// AccessType::Read: can_read=true, can_write=false, can_execute=false, is_privileged=false.
// Expected per §7.3.6:
//   R = not a pure write, so ats_r=true
//   W = write permission requested (!NW) = false, so ats_w=false
//   X = execute permission = false, so ats_x=false
//   P = privileged = false, so ats_p=false
//
// Trigger: SMMUEN=0 path (make_smmu_disabled_rec_cfg_ats), ATS TR.
//
// BEFORE FIX: fields don't exist → compile error.
// AFTER FIX:  ats_r=true, ats_w=false, ats_x=false, ats_p=false.

/// Item1-T1: F_BAD_ATS_TREQ from AccessType::Read must have ats_r=true, ats_w=false,
/// ats_x=false, ats_p=false.
///
/// ARM §7.3.6 bits [95:92]: R=!pure_write, W=write(!NW), X=execute, P=privileged.
#[test]
fn item1_ats_fields_read_access() {
    let smmu = make_smmu_disabled_rec_cfg_ats();
    let stream_id = sid(0xD1_01);

    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(
        result.is_err(),
        "Item1-T1: ATS TR with SMMUEN=0 must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FBadAtsTreq)
        })
        .expect("Item1-T1: Expected F_BAD_ATS_TREQ event");

    assert!(
        ev.ats_r,
        "Item1-T1: F_BAD_ATS_TREQ with Read access must have ats_r=true \
         (Read is not a pure write → R permission requested per §7.3.6)."
    );
    assert!(
        !ev.ats_w,
        "Item1-T1: F_BAD_ATS_TREQ with Read access must have ats_w=false \
         (Read does not request write permission per §7.3.6)."
    );
    assert!(
        !ev.ats_x,
        "Item1-T1: F_BAD_ATS_TREQ with Read access must have ats_x=false \
         (Read does not request execute permission per §7.3.6)."
    );
    assert!(
        !ev.ats_p,
        "Item1-T1: F_BAD_ATS_TREQ with Read access must have ats_p=false \
         (Read is unprivileged per §7.3.6)."
    );
}

// ============================================================================
// Item 1, Test 2 — FBadAtsTreq ats_r/w/x/p for AccessType::Write
// ============================================================================
//
// AccessType::Write: can_read=false, can_write=true, can_execute=false, is_privileged=false.
// Expected per §7.3.6:
//   R = pure write only, no read/execute → ats_r=false
//   W = write permission requested → ats_w=true
//   X = execute = false → ats_x=false
//   P = privileged = false → ats_p=false
//
// Trigger: ATSCHK/stream path (make_smmu, stream with eats=0).
//
// BEFORE FIX: fields don't exist → compile error.
// AFTER FIX:  ats_r=false, ats_w=true, ats_x=false, ats_p=false.

/// Item1-T2: F_BAD_ATS_TREQ from AccessType::Write must have ats_r=false, ats_w=true,
/// ats_x=false, ats_p=false.
///
/// ARM §7.3.6: Write is a pure write → R=false, W=true(!NW), X=false, P=false.
#[test]
fn item1_ats_fields_write_access() {
    let smmu = make_smmu();
    let stream_id = sid(0xD1_02);

    // eats=0 → ATS TR not supported → F_BAD_ATS_TREQ on stream path.
    let cfg = StreamConfig {
        eats: 0,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x2000),
        PA::new(0x8000_2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Write,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(
        result.is_err(),
        "Item1-T2: ATS TR on eats=0 stream must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FBadAtsTreq)
        })
        .expect("Item1-T2: Expected F_BAD_ATS_TREQ event");

    assert!(
        !ev.ats_r,
        "Item1-T2: F_BAD_ATS_TREQ with Write access must have ats_r=false \
         (Write is a pure write, no read permission requested per §7.3.6)."
    );
    assert!(
        ev.ats_w,
        "Item1-T2: F_BAD_ATS_TREQ with Write access must have ats_w=true \
         (Write requests write permission = !NW per §7.3.6)."
    );
    assert!(
        !ev.ats_x,
        "Item1-T2: F_BAD_ATS_TREQ with Write access must have ats_x=false \
         (Write does not request execute permission per §7.3.6)."
    );
    assert!(
        !ev.ats_p,
        "Item1-T2: F_BAD_ATS_TREQ with Write access must have ats_p=false \
         (Write is unprivileged per §7.3.6)."
    );
}

// ============================================================================
// Item 1, Test 3 — FBadAtsTreq ats_r/w/x/p for AccessType::ReadPrivileged
// ============================================================================
//
// AccessType::ReadPrivileged: can_read=true, can_write=false, can_execute=false,
// is_privileged=true.
// Expected per §7.3.6:
//   R = not a pure write → ats_r=true
//   W = no write → ats_w=false
//   X = no execute → ats_x=false
//   P = privileged → ats_p=true
//
// Trigger: SMMUEN=0 path.
//
// BEFORE FIX: fields don't exist → compile error.
// AFTER FIX:  ats_r=true, ats_w=false, ats_x=false, ats_p=true.

/// Item1-T3: F_BAD_ATS_TREQ from AccessType::ReadPrivileged must have ats_r=true,
/// ats_w=false, ats_x=false, ats_p=true.
///
/// ARM §7.3.6: ReadPrivileged → R=true (not pure write), W=false, X=false, P=true.
#[test]
fn item1_ats_fields_read_privileged_access() {
    let smmu = make_smmu_disabled_rec_cfg_ats();
    let stream_id = sid(0xD1_03);

    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x3000),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(
        result.is_err(),
        "Item1-T3: ATS TR with SMMUEN=0 must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FBadAtsTreq)
        })
        .expect("Item1-T3: Expected F_BAD_ATS_TREQ event");

    assert!(
        ev.ats_r,
        "Item1-T3: F_BAD_ATS_TREQ with ReadPrivileged must have ats_r=true \
         (ReadPrivileged requests read → not pure write per §7.3.6)."
    );
    assert!(
        !ev.ats_w,
        "Item1-T3: F_BAD_ATS_TREQ with ReadPrivileged must have ats_w=false \
         (ReadPrivileged does not request write per §7.3.6)."
    );
    assert!(
        !ev.ats_x,
        "Item1-T3: F_BAD_ATS_TREQ with ReadPrivileged must have ats_x=false \
         (ReadPrivileged does not request execute per §7.3.6)."
    );
    assert!(
        ev.ats_p,
        "Item1-T3: F_BAD_ATS_TREQ with ReadPrivileged must have ats_p=true \
         (ReadPrivileged is privileged access per §7.3.6)."
    );
}

// ============================================================================
// Item 1, Test 4 — FBadAtsTreq ats_r/w/x/p for AccessType::Execute
// ============================================================================
//
// AccessType::Execute: can_read=false, can_write=false, can_execute=true,
// is_privileged=false.
// Expected per §7.3.6:
//   R = has execute (instruction fetch implies read) → ats_r=true
//   W = no write → ats_w=false
//   X = execute = true → ats_x=true
//   P = not privileged → ats_p=false
//
// Trigger: ATSCHK/stream path with eats=0.
//
// BEFORE FIX: fields don't exist → compile error.
// AFTER FIX:  ats_r=true, ats_w=false, ats_x=true, ats_p=false.

/// Item1-T4: F_BAD_ATS_TREQ from AccessType::Execute must have ats_r=true, ats_w=false,
/// ats_x=true, ats_p=false.
///
/// ARM §7.3.6: Execute → R=true (instruction fetch is not a pure write),
/// W=false, X=true, P=false.
#[test]
fn item1_ats_fields_execute_access() {
    let smmu = make_smmu();
    let stream_id = sid(0xD1_04);

    // eats=0 → ATS TR not supported → F_BAD_ATS_TREQ on stream path.
    let cfg = StreamConfig {
        eats: 0,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x4000),
        PA::new(0x8000_4000).unwrap(),
        PagePermissions::all(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x4000),
        AccessType::Execute,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );
    assert!(
        result.is_err(),
        "Item1-T4: ATS TR on eats=0 stream must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FBadAtsTreq)
        })
        .expect("Item1-T4: Expected F_BAD_ATS_TREQ event");

    assert!(
        ev.ats_r,
        "Item1-T4: F_BAD_ATS_TREQ with Execute access must have ats_r=true \
         (Execute is an instruction fetch — not a pure write → R=true per §7.3.6)."
    );
    assert!(
        !ev.ats_w,
        "Item1-T4: F_BAD_ATS_TREQ with Execute access must have ats_w=false \
         (Execute does not request write permission per §7.3.6)."
    );
    assert!(
        ev.ats_x,
        "Item1-T4: F_BAD_ATS_TREQ with Execute access must have ats_x=true \
         (Execute requests execute permission per §7.3.6)."
    );
    assert!(
        !ev.ats_p,
        "Item1-T4: F_BAD_ATS_TREQ with Execute access must have ats_p=false \
         (Execute is unprivileged per §7.3.6)."
    );
}

// ============================================================================
// Item 1, Test 5 — ats_* fields are zero/false for non-ATS events (FTranslation)
// ============================================================================
//
// The ats_r/w/x/p fields are RES0 for all event types other than FBadAtsTreq.
// An FTranslation event generated from a normal translate() call must have all
// four ats_* fields as false.
//
// BEFORE FIX: fields don't exist → compile error.
// AFTER FIX:  all four ats_* fields false (via ..EventEntry::zeroed()).

/// Item1-T5: ats_r/w/x/p must all be false (RES0) for non-ATS events (e.g. FTranslation).
///
/// ARM §7.3.6: the four ATS permission bits are RES0 for all events other than
/// F_BAD_ATS_TREQ (0x05).
#[test]
fn item1_ats_fields_zero_for_non_ats_events() {
    let smmu = make_smmu();
    let stream_id = sid(0xD1_05);

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do NOT map any page → translate produces FTranslation.

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x5000),
        AccessType::ReadWrite,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Item1-T5: Unmapped IOVA must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslation)
        })
        .expect("Item1-T5: Expected F_TRANSLATION event for unmapped page");

    assert!(
        !ev.ats_r,
        "Item1-T5: FTranslation must have ats_r=false (RES0 for non-ATS events §7.3.6). \
         Got ats_r=true."
    );
    assert!(
        !ev.ats_w,
        "Item1-T5: FTranslation must have ats_w=false (RES0 for non-ATS events §7.3.6). \
         Got ats_w=true."
    );
    assert!(
        !ev.ats_x,
        "Item1-T5: FTranslation must have ats_x=false (RES0 for non-ATS events §7.3.6). \
         Got ats_x=true."
    );
    assert!(
        !ev.ats_p,
        "Item1-T5: FTranslation must have ats_p=false (RES0 for non-ATS events §7.3.6). \
         Got ats_p=true."
    );
}

// ============================================================================
// Item 2 — reason field is zero for any event
// ============================================================================
//
// ARM §7.3.2 F_UUT specifies a 16-bit IMPLEMENTATION DEFINED Reason field at
// bits [79:64].  This software model always emits 0 (valid IMPDEF per §7.3.2).
// Any event (FTranslation used here for simplicity) must have reason==0.
//
// BEFORE FIX: field doesn't exist → compile error.
// AFTER FIX:  reason==0.

/// Item2-T1: The `reason` field must be 0 on any event.
///
/// ARM §7.3.2: F_UUT Reason at bits [79:64] is IMPLEMENTATION DEFINED.
/// This SW model always emits 0 (a valid IMPDEF choice per §7.3.2).
#[test]
fn item2_reason_field_is_zero() {
    let smmu = make_smmu();
    let stream_id = sid(0xD2_01);

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // Do NOT map page → FTranslation event.

    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x6000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "Item2-T1: Unmapped IOVA must fail, got {result:?}"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FTranslation)
        })
        .expect("Item2-T1: Expected F_TRANSLATION event");

    assert_eq!(
        ev.reason, 0,
        "Item2-T1: EventEntry.reason must be 0 (IMPDEF=0 per §7.3.2). \
         Got reason={}.",
        ev.reason
    );
}
