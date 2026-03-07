//! Tests for BUG-RUST-DBGR-3 and BUG-RUST-DBGR-6 — S1DSS bypass/abort in translate()
#![allow(clippy::doc_markdown)]
//!
//! ARM §5.2 §3.9: When a stream is substream-capable (s1cd_max > 0) and PASID == 0,
//! the STE.S1DSS field controls behaviour:
//!   0b00 (0) = abort with F_STREAM_DISABLED
//!   0b01 (1) = bypass stage-1 (identity mapping)
//!   0b10 (2) = use CD[0] for translation (default)

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, TranslationError, IOVA, PA, PASID};

fn make_substream_ctx(s1dss: u8) -> StreamContext {
    let ctx = StreamContext::new();
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1cd_max(8) // substream-capable
        .s1dss(s1dss)
        .build()
        .expect("valid substream config");
    ctx.update_configuration(cfg);
    ctx
}

// ─── BUG-RUST-DBGR-3: S1DSS=0b01 bypass (identity mapping) ──────────────────

/// When S1DSS=1 and PASID=0 on a substream-capable stream, translate()
/// must return an identity mapping (IOVA == PA) even when no PASID 0
/// entry exists in the pasid_map.
#[test]
fn dbgr3_s1dss_bypass_returns_identity_mapping() {
    let ctx = make_substream_ctx(1); // s1dss = bypass
    let iova = IOVA::new(0x1000).unwrap();
    let pasid = PASID::new(0).unwrap();

    // No PASID 0 entry — with the old code this would return PASIDNotFound or
    // fall through to translate_stage1_only which returns PASIDNotFound.
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "S1DSS=1 with PASID=0 must succeed with identity mapping, got: {result:?}"
    );
    let data = result.unwrap();
    assert_eq!(
        data.physical_address().as_u64(),
        iova.as_u64(),
        "S1DSS=1 (bypass) must produce identity mapping: PA == IOVA"
    );
}

/// S1DSS=1 bypass must not require a PASID 0 entry in pasid_map.
#[test]
fn dbgr3_s1dss_bypass_no_pasid_entry_needed() {
    let ctx = make_substream_ctx(1);
    let pasid = PASID::new(0).unwrap();
    assert!(
        !ctx.has_pasid(pasid),
        "setup invariant: no PASID 0 entry should exist"
    );
    let iova = IOVA::new(0x5000).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "bypass must not require pasid_map entry; got: {result:?}");
}

/// Non-zero PASID must not be affected by S1DSS (S1DSS only applies when PASID == 0).
#[test]
fn dbgr3_s1dss_bypass_only_for_pasid0() {
    let ctx = make_substream_ctx(1);
    // Create PASID 1 with a mapped page
    let pasid1 = PASID::new(1).unwrap();
    ctx.create_pasid(pasid1).unwrap();
    let iova = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x3000).unwrap();
    ctx.map_page(pasid1, iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    let result = ctx.translate(pasid1, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x3000);
}

// ─── BUG-RUST-DBGR-6: S1DSS=0b00 abort (F_STREAM_DISABLED) ─────────────────

/// When S1DSS=0 and PASID=0 on a substream-capable stream, translate()
/// must return StreamDisabled (F_STREAM_DISABLED), NOT PASIDNotFound.
#[test]
fn dbgr6_s1dss_abort_returns_stream_disabled() {
    let ctx = make_substream_ctx(0); // s1dss = abort
    let iova = IOVA::new(0x1000).unwrap();
    let pasid = PASID::new(0).unwrap();

    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::StreamDisabled)),
        "S1DSS=0 with PASID=0 must return StreamDisabled, got: {result:?}"
    );
}

/// S1DSS=0 abort must not return PASIDNotFound.
#[test]
fn dbgr6_s1dss_abort_not_pasid_not_found() {
    let ctx = make_substream_ctx(0);
    let iova = IOVA::new(0x2000).unwrap();
    let pasid = PASID::new(0).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        !matches!(result, Err(TranslationError::PASIDNotFound)),
        "S1DSS=0 must return StreamDisabled, not PASIDNotFound; got: {result:?}"
    );
}

/// When s1cd_max == 0 (not substream-capable), S1DSS is ignored and
/// PASID 0 must use the normal CD[0] path (or PASIDNotFound if no entry).
#[test]
fn dbgr6_s1dss_ignored_when_not_substream_capable() {
    let ctx = StreamContext::new(); // default: s1cd_max=0, stage1 enabled
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1cd_max(0) // NOT substream-capable
        .s1dss(0)   // abort — but should be ignored
        .build()
        .expect("valid config");
    ctx.update_configuration(cfg);
    let iova = IOVA::new(0x1000).unwrap();
    let pasid = PASID::new(0).unwrap();
    // Without a PASID 0 entry, we should get PASIDNotFound (not StreamDisabled)
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::PASIDNotFound)),
        "When s1cd_max=0, S1DSS is ignored and PASID 0 miss → PASIDNotFound; got: {result:?}"
    );
}

/// S1DSS=2 (default): PASID=0 should use CD[0] — normal lookup.
#[test]
fn dbgr3_s1dss_cd0_fallthrough_uses_normal_lookup() {
    let ctx = make_substream_ctx(2); // s1dss = use CD[0]
    let pasid0 = PASID::new(0).unwrap();
    ctx.create_pasid(pasid0).unwrap();
    let iova = IOVA::new(0x4000).unwrap();
    let pa = PA::new(0x8000).unwrap();
    ctx.map_page(pasid0, iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    let result = ctx.translate(pasid0, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "S1DSS=2 should look up PASID 0; got: {result:?}");
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x8000);
}
