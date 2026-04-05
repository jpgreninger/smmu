//! TDD tests for BUG-AUDIT-111 (Rust): bypass stream + SSV=1 must abort with
//! `C_BAD_SUBSTREAMID`.
//!
//! ARM IHI0070G.b §3.3.2 line 1354: "Transactions provided with a `SubstreamID`
//! are terminated when stage 1 translation is not enabled."  This applies to
//! ALL `SSV=1` transactions regardless of `SubstreamID` value, including
//! `SubstreamID=0` (i.e. `PASID=0` + `SSV=1`).
//!
//! The bug: `translate_with_type_ssv()` had `if !is_bypass && stream_s1cd_max
//! == 0 && ssv` — the `!is_bypass` guard caused the check to be skipped for
//! full-bypass streams, letting an identity-mapped translation succeed instead
//! of aborting with `C_BAD_SUBSTREAMID`.
//!
//! Fix: remove the `!is_bypass` guard; `stream_s1cd_max == 0 && ssv` is the
//! correct and sufficient condition for all streams that lack substream support.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID, TranslationError,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

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

// ============================================================================
// BUG-AUDIT-111: bypass + SSV=1 + PASID=0 must abort
// ============================================================================

/// BUG-AUDIT-111 primary case: bypass stream + SSV=1 + PASID=0 must return
/// `Err(BadSubstreamId)`.
///
/// ARM §3.3.2: "Transactions provided with a `SubstreamID` are terminated when
/// stage 1 translation is not enabled."  A bypass stream has stage 1 disabled;
/// presenting SSV=1 (even with SubstreamID=0) must abort.
///
/// BEFORE FIX: returns `Ok` (identity mapping bypasses the check).
/// AFTER FIX:  returns `Err(TranslationError::BadSubstreamId)`.
#[test]
fn bypass_ssv1_pasid0_fails() {
    let smmu = make_smmu();
    let stream_id = sid(200);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let result = smmu.translate_with_ssv(
        stream_id,
        pasid(0),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
        true, // SSV=1: SubstreamID was presented
    );

    assert!(
        matches!(result, Err(TranslationError::BadSubstreamId)),
        "BUG-AUDIT-111: bypass + SSV=1 + PASID=0 must return Err(BadSubstreamId); got: {result:?}"
    );
}

/// BUG-AUDIT-111 event check: bypass stream + SSV=1 + PASID=0 must record a
/// `C_BAD_SUBSTREAMID` event.
///
/// ARM §3.3.2 / §7.3.9: the abort must be accompanied by a
/// `C_BAD_SUBSTREAMID` (event code 0x08) event in the event queue.
///
/// BEFORE FIX: no `C_BAD_SUBSTREAMID` event is recorded.
/// AFTER FIX:  event queue contains `C_BAD_SUBSTREAMID` with matching `stream_id`.
#[test]
fn bypass_ssv1_pasid0_generates_c_bad_substreamid_event() {
    let smmu = make_smmu();
    let stream_id = sid(201);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let _ = smmu.translate_with_ssv(
        stream_id,
        pasid(0),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
        true,
    );

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        !events.is_empty(),
        "BUG-AUDIT-111: expected C_BAD_SUBSTREAMID event for bypass + SSV=1 + PASID=0; got: {:?}",
        smmu.get_events().iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
    assert_eq!(
        events[0].stream_id,
        stream_id.as_u32(),
        "BUG-AUDIT-111: C_BAD_SUBSTREAMID event must carry the faulting stream_id"
    );
}

// ============================================================================
// Regression: bypass + SSV=0 + PASID=0 must still succeed
// ============================================================================

/// Regression: bypass stream + SSV=0 + PASID=0 must succeed with identity PA.
///
/// `SSV=0` means no `SubstreamID` was presented — the §3.3.2 rule does not apply.
/// The existing bypass behaviour (identity mapping) must be preserved.
#[test]
fn bypass_ssv0_pasid0_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(202);

    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let result = smmu.translate_with_ssv(
        stream_id,
        pasid(0),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
        false, // SSV=0: no SubstreamID presented
    );

    assert!(
        result.is_ok(),
        "regression: bypass + SSV=0 + PASID=0 must succeed (identity PA); got: {result:?}"
    );

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        events.is_empty(),
        "regression: bypass + SSV=0 + PASID=0 must NOT generate C_BAD_SUBSTREAMID; got: {events:?}"
    );
}

// ============================================================================
// Regression: stage-1-enabled stream with valid PASID is unaffected
// ============================================================================

/// Regression: stage-1-enabled stream with `s1cd_max=2`, `SSV=1`, `PASID=1`
/// (within range) must NOT be affected by the fix.
///
/// The fix only removes the `!is_bypass` guard; the condition
/// `stream_s1cd_max == 0` is false for a substream-capable stream, so the
/// abort path must not trigger.
#[test]
fn stage1_enabled_valid_pasid_ssv1_unaffected() {
    use smmu::types::StreamConfigBuilder;

    let smmu = make_smmu();
    let stream_id = sid(203);

    // s1cd_max=2: valid PASIDs are 0..(2^2) = 0..3; PASID=1 is in-range.
    let config = StreamConfigBuilder::new()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .pasid_enabled(true)
        .max_pasid(16)
        .s1cd_max(2)
        .s1dss(0)
        .build()
        .unwrap();

    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();

    let pa = PA::new(0x3000).unwrap();
    smmu.map_page(
        stream_id,
        pasid(1),
        iova_1000(),
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate_with_ssv(
        stream_id,
        pasid(1),
        iova_1000(),
        AccessType::Read,
        SecurityState::NonSecure,
        true, // SSV=1, PASID=1 — valid substream, s1cd_max=2 > 0
    );

    // Must succeed: s1cd_max > 0 means the stream is substream-capable.
    assert!(
        result.is_ok(),
        "regression: stage-1-enabled s1cd_max=2 + SSV=1 + PASID=1 (valid) must succeed; got: {result:?}"
    );

    let events = smmu.get_events_by_type(EventType::CBadSubstreamid);
    assert!(
        events.is_empty(),
        "regression: valid substream must NOT generate C_BAD_SUBSTREAMID; got: {events:?}"
    );
}
