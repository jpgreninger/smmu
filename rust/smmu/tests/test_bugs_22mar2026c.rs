//! TDD failing tests for two confirmed spec-compliance bugs in mod.rs.
//!
//! Tests are written to FAIL with the current buggy code and PASS only after
//! the corresponding fix is applied.
//!
//! # Bugs Covered
//!
//! ## BUG-RUST-1 — ATS-path C_BAD_STREAMID sets non-zero InputAddr
//!
//! The `CBadStreamid` EventEntry built in the ATS TranslationRequest / `None`
//! arm of the stream lookup sets `address: iova.as_u64()` and
//! `pasid: pasid.as_u32()`.  ARM §7.3.3 wire format: C_BAD_STREAMID has no
//! InputAddr field — bits[127:64] are RES0 and MUST be zero.
//!
//! ## BUG-RUST-2a — C_BAD_CD bypasses MEV deduplication
//!
//! The inline C_BAD_CD push block (around line 3550) does not check MEV before
//! pushing the event.  ARM §5.2 / §7.3.1: MEV=true must suppress duplicate
//! events of the same (event_type, stream_id) pair.
//!
//! ## BUG-RUST-2b — T0SZ F_TRANSLATION bypasses MEV deduplication
//!
//! The inline T0SZ F_TRANSLATION push block (around line 3636) does not check
//! MEV before pushing the event.  ARM §5.2 / §7.3.1: MEV=true must suppress
//! duplicate events of the same (event_type, stream_id) pair.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventType, SecurityState, StreamConfig, StreamID, TransactionType, IOVA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
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
// BUG-RUST-1 — ATS-path C_BAD_STREAMID must have address=0 and pasid=0
// ============================================================================
//
// The ATS TranslationRequest `None` arm at ~line 3144 builds an EventEntry with
// `address: iova.as_u64()` and `pasid: pasid.as_u32()`.
//
// ARM §7.3.3 wire format: C_BAD_STREAMID has no InputAddr field.
// Bits[127:64] are RES0.  Both `address` and `pasid` MUST be 0.
//
// BEFORE FIX: address == iova (non-zero) → assertion fails.
// AFTER FIX:  address == 0, pasid == 0  → assertions pass.

/// BUG-RUST-1: ATS-path C_BAD_STREAMID must carry address=0 and pasid=0.
///
/// ARM §7.3.3: no InputAddr field — bits[127:64] are RES0.
///
/// BEFORE FIX: address == iova (e.g. 0x1000) → FAILS.
/// AFTER FIX:  address == 0, pasid == 0     → PASSES.
#[test]
fn rust1_ats_bad_streamid_address_must_be_zero() {
    // BUG-AUDIT-60 fix: CR2 is RO when SMMUEN=1 (ARM §6.3.12); set CR2 before enable().
    let smmu = SMMU::new();
    // Enable ATS recording: CR2.RECINVSID=1 AND CR2.REC_CFG_ATS=1.
    // Must be set BEFORE enable() since CR2 is RO once SMMUEN=1.
    smmu.set_cr2(SMMU::CR2_RECINVSID | SMMU::CR2_REC_CFG_ATS);
    smmu.enable().unwrap();
    smmu.clear_event_queue();

    // Use a StreamID that is NOT configured → None arm → C_BAD_STREAMID.
    // StreamID::new() must be within the default strtab range (default log2size allows
    // small StreamIDs; pick one that is "unconfigured" rather than "out-of-range").
    // The ATS path hits the None arm when streams.get() returns None — i.e. the
    // StreamID is within range but no configure_stream() call was made for it.
    let unknown_sid = sid(0x55);
    let test_pasid = pasid(7);
    let test_iova = iova(0x1000);

    let _ = smmu.translate_with_type(
        unknown_sid,
        test_pasid,
        test_iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    let events = smmu.get_events();
    let bad_sid_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::CBadStreamid)
        .collect();

    assert_eq!(
        bad_sid_events.len(),
        1,
        "BUG-RUST-1: expected exactly one C_BAD_STREAMID event; got {}: {:?}",
        bad_sid_events.len(),
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );

    let ev = bad_sid_events[0];
    assert_eq!(
        ev.address, 0,
        "BUG-RUST-1: C_BAD_STREAMID address must be 0 (RES0 per ARM §7.3.3); got 0x{:x}",
        ev.address
    );
    assert_eq!(
        ev.pasid, 0,
        "BUG-RUST-1: C_BAD_STREAMID pasid must be 0 (RES0 per ARM §7.3.3); got {}",
        ev.pasid
    );
}

// ============================================================================
// BUG-RUST-2a — C_BAD_CD must respect MEV deduplication
// ============================================================================
//
// The inline C_BAD_CD push block (after `drop(stream_ref)` at ~line 3516) pushes
// the event without a MEV guard.
//
// ARM §5.2 / §7.3.1: when STE.MEV=1, duplicate events with the same
// (event_type, stream_id) must be suppressed.
//
// Trigger: configure stream with aa64=false (unsupported AArch32 → C_BAD_CD)
// AND mev=true.  Translate twice.  Without fix, two C_BAD_CD events appear.
// With fix, only one survives (second is MEV-suppressed).
//
// BEFORE FIX: two C_BAD_CD events in queue → assertion fails.
// AFTER FIX:  one C_BAD_CD event in queue  → assertion passes.

/// BUG-RUST-2a: C_BAD_CD must be MEV-deduplicated when stream.mev=true.
///
/// ARM §5.2 / §7.3.1: STE.MEV=1 suppresses duplicate events per stream.
///
/// BEFORE FIX: two C_BAD_CD events recorded → FAILS.
/// AFTER FIX:  one C_BAD_CD event recorded  → PASSES.
#[test]
fn rust2a_cbadcd_duplicate_suppressed_when_mev_set() {
    let smmu = make_smmu();
    let stream_id = sid(0xC2_A1);

    // aa64=false triggers C_BAD_CD (AArch32 LPAE unsupported per §5.4/CT-14).
    // mev=true enables MEV deduplication.
    let cfg = StreamConfig {
        aa64: false,
        mev: true,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // First translation — emits C_BAD_CD.
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Second translation — same fault, same stream.  MEV must suppress duplicate.
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let cbadcd_count = events
        .iter()
        .filter(|e| {
            e.event_type == EventType::CBadCd && e.stream_id == stream_id.as_u32()
        })
        .count();

    assert_eq!(
        cbadcd_count, 1,
        "BUG-RUST-2a: MEV=true must suppress duplicate C_BAD_CD events. \
         Expected 1 event, got {cbadcd_count}. MEV deduplication missing from C_BAD_CD inline push."
    );
}

// ============================================================================
// BUG-RUST-2b — T0SZ F_TRANSLATION must respect MEV deduplication
// ============================================================================
//
// The inline T0SZ F_TRANSLATION push block (after `drop(stream_ref)` at ~line 3597)
// pushes the event without a MEV guard.
//
// ARM §5.2 / §7.3.1: when STE.MEV=1, duplicate events with the same
// (event_type, stream_id) must be suppressed.
//
// Trigger: configure stream with t0sz=16 (VA limit = 2^48) AND mev=true.
// Translate twice with IOVA >= 2^48.  Without fix, two F_TRANSLATION events appear.
// With fix, only one survives (second is MEV-suppressed).
//
// BEFORE FIX: two F_TRANSLATION events in queue → assertion fails.
// AFTER FIX:  one F_TRANSLATION event in queue  → assertion passes.

/// BUG-RUST-2b: T0SZ F_TRANSLATION must be MEV-deduplicated when stream.mev=true.
///
/// ARM §5.2 / §7.3.1: STE.MEV=1 suppresses duplicate events per stream.
///
/// BEFORE FIX: two F_TRANSLATION events recorded → FAILS.
/// AFTER FIX:  one F_TRANSLATION event recorded  → PASSES.
#[test]
fn rust2b_t0sz_translation_duplicate_suppressed_when_mev_set() {
    let smmu = make_smmu();
    let stream_id = sid(0xC2_B1);

    // t0sz=16 → VA limit = 2^(64-16) = 2^48.  IOVA >= 2^48 → F_TRANSLATION.
    // mev=true enables MEV deduplication.
    let cfg = StreamConfig {
        t0sz: 16,
        mev: true,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    // IOVA above VA limit (2^48 = 0x0001_0000_0000_0000).
    let out_of_range = iova(0x0001_0000_0000_0000u64);

    // First translation — emits F_TRANSLATION (T0SZ out-of-range).
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        out_of_range,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Second translation — same fault type, same stream.  MEV must suppress duplicate.
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        out_of_range,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let f_translation_count = events
        .iter()
        .filter(|e| {
            e.event_type == EventType::FTranslation && e.stream_id == stream_id.as_u32()
        })
        .count();

    assert_eq!(
        f_translation_count, 1,
        "BUG-RUST-2b: MEV=true must suppress duplicate T0SZ F_TRANSLATION events. \
         Expected 1 event, got {f_translation_count}. MEV deduplication missing from T0SZ inline push."
    );
}

// ============================================================================
// Sanity: MEV=false must still record all duplicate events (non-regression)
// ============================================================================

/// Sanity: with MEV=false, C_BAD_CD duplicates must NOT be suppressed.
///
/// Verifies that the MEV guard is conditional — when mev=false both events
/// are recorded.
#[test]
fn rust2a_cbadcd_not_suppressed_when_mev_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xC2_A2);

    let cfg = StreamConfig {
        aa64: false,
        mev: false, // MEV disabled — no deduplication
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let cbadcd_count = events
        .iter()
        .filter(|e| {
            e.event_type == EventType::CBadCd && e.stream_id == stream_id.as_u32()
        })
        .count();

    assert_eq!(
        cbadcd_count, 2,
        "Sanity: MEV=false must record both C_BAD_CD events; got {cbadcd_count}"
    );
}

/// Sanity: with MEV=false, T0SZ F_TRANSLATION duplicates must NOT be suppressed.
#[test]
fn rust2b_t0sz_not_suppressed_when_mev_false() {
    let smmu = make_smmu();
    let stream_id = sid(0xC2_B2);

    let cfg = StreamConfig {
        t0sz: 16,
        mev: false,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let out_of_range = iova(0x0001_0000_0000_0000u64);

    let _ = smmu.translate(
        stream_id,
        pasid(0),
        out_of_range,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        out_of_range,
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let f_translation_count = events
        .iter()
        .filter(|e| {
            e.event_type == EventType::FTranslation && e.stream_id == stream_id.as_u32()
        })
        .count();

    assert_eq!(
        f_translation_count, 2,
        "Sanity: MEV=false must record both F_TRANSLATION events; got {f_translation_count}"
    );
}
