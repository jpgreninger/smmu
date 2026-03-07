#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]

//! TDD regression test for BUG-RUST-DBGR-2:
//! Invalid CD returns `BadSubstreamId` (event 0x08) instead of `BadCD` (event 0x0A).
//!
//! Problem: When T0SZ, T1SZ, or AA64 are invalid (bad context descriptor),
//! the code returns `Err(TranslationError::BadSubstreamId)`. This produces
//! event code 0x08 (C_BAD_SUBSTREAMID) through the fault mapping path, while
//! the event queue correctly records 0x0A (C_BAD_CD). The two are inconsistent.
//!
//! Fix: Add a `BadCD` variant to `TranslationError`. Return `Err(TranslationError::BadCD)`
//! in the invalid-CD branch. Map `BadCD` → `FaultType::BadCd` (event code 0x0A).
//! Ensure `BadCD` is non-stall-eligible (configuration faults always abort).

use smmu::types::{AccessType, EventType, FaultMode, SecurityState, StreamConfig, StreamID, TranslationError, IOVA, PASID};
use smmu::SMMU;

fn smmu_enabled() -> SMMU {
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

/// When T0SZ > 39 (invalid), the translation must return `TranslationError::BadCD`.
///
/// Before the fix: returns `TranslationError::BadSubstreamId` (event 0x08).
/// After the fix: returns `TranslationError::BadCD` (event 0x0A).
#[test]
fn dbgr2_invalid_t0sz_returns_bad_cd_error() {
    let smmu = smmu_enabled();

    // t0sz=40 is out-of-range (valid: 0–39), triggers C_BAD_CD.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .t0sz(40)
        .t1sz(16)
        .aa64(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid(20), cfg).unwrap();
    smmu.create_pasid(sid(20), pasid(0)).unwrap();

    let result = smmu.translate(
        sid(20),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "DBGR-2: invalid CD must return an error");

    // Must NOT be BadSubstreamId.
    assert!(
        !matches!(result, Err(TranslationError::BadSubstreamId)),
        "DBGR-2 BUG: invalid T0SZ returned BadSubstreamId (event 0x08) instead of BadCD (event 0x0A); \
         got: {result:?}"
    );

    // Must be BadCD.
    assert!(
        matches!(result, Err(TranslationError::BadCD)),
        "DBGR-2: invalid T0SZ must return TranslationError::BadCD; got: {result:?}"
    );
}

/// When T1SZ > 39 (invalid), the translation must return `TranslationError::BadCD`.
#[test]
fn dbgr2_invalid_t1sz_returns_bad_cd_error() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .t0sz(16)
        .t1sz(50) // out-of-range
        .aa64(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid(21), cfg).unwrap();
    smmu.create_pasid(sid(21), pasid(0)).unwrap();

    let result = smmu.translate(
        sid(21),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "DBGR-2: invalid T1SZ must return an error");
    assert!(
        !matches!(result, Err(TranslationError::BadSubstreamId)),
        "DBGR-2 BUG: invalid T1SZ returned BadSubstreamId instead of BadCD; got: {result:?}"
    );
    assert!(
        matches!(result, Err(TranslationError::BadCD)),
        "DBGR-2: invalid T1SZ must return TranslationError::BadCD; got: {result:?}"
    );
}

/// When AA64=false (AArch32 LPAE — unsupported), must return `TranslationError::BadCD`.
#[test]
fn dbgr2_aa64_false_returns_bad_cd_error() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .t0sz(16)
        .t1sz(16)
        .aa64(false) // AArch32 LPAE unsupported → C_BAD_CD
        .build()
        .unwrap();

    smmu.configure_stream(sid(22), cfg).unwrap();
    smmu.create_pasid(sid(22), pasid(0)).unwrap();

    let result = smmu.translate(
        sid(22),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(result.is_err(), "DBGR-2: AA64=false must return an error");
    assert!(
        !matches!(result, Err(TranslationError::BadSubstreamId)),
        "DBGR-2 BUG: AA64=false returned BadSubstreamId instead of BadCD; got: {result:?}"
    );
    assert!(
        matches!(result, Err(TranslationError::BadCD)),
        "DBGR-2: AA64=false must return TranslationError::BadCD; got: {result:?}"
    );
}

/// BadCD must never stall, even on a stall-mode stream (ARM §3.12.2).
/// Configuration faults always abort.
#[test]
fn dbgr2_bad_cd_from_invalid_t0sz_must_not_stall() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .t0sz(40) // invalid
        .t1sz(16)
        .aa64(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid(23), cfg).unwrap();
    smmu.create_pasid(sid(23), pasid(0)).unwrap();

    let result = smmu.translate(
        sid(23),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Must never stall — configuration faults abort.
    assert!(
        !matches!(result, Err(TranslationError::Stalled { .. })),
        "DBGR-2: BadCD (invalid T0SZ) must never stall on a stall-mode stream; got: {result:?}"
    );
    assert!(result.is_err(), "DBGR-2: must return error (abort)");
}

/// Event type recorded in the event queue must be CBadCd (0x0A), not CBadSubstreamid (0x08).
#[test]
fn dbgr2_invalid_cd_records_c_bad_cd_event_not_substreamid() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .t0sz(40) // invalid
        .t1sz(16)
        .aa64(true)
        .build()
        .unwrap();

    smmu.configure_stream(sid(24), cfg).unwrap();
    smmu.create_pasid(sid(24), pasid(0)).unwrap();

    let _ = smmu.translate(
        sid(24),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();

    // Must have a CBadCd event.
    let has_bad_cd = events.iter().any(|e| e.event_type == EventType::CBadCd);
    assert!(
        has_bad_cd,
        "DBGR-2: invalid CD must record CBadCd event (0x0A); got events: {events:?}"
    );

    // Must NOT record CBadSubstreamid.
    let has_bad_substreamid = events.iter().any(|e| e.event_type == EventType::CBadSubstreamid);
    assert!(
        !has_bad_substreamid,
        "DBGR-2 BUG: invalid CD must NOT record CBadSubstreamid (0x08); got events: {events:?}"
    );
}
