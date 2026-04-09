//! TDD tests for §3.13.4 HTTU behavior summary — BUG-AUDIT-129 and BUG-AUDIT-130.
//!
//! ## BUG-AUDIT-129: HD=1 requires HA=1 (§3.13 / §3.13.4)
//!
//! ARM spec §3.13 states that HD=1 implies HA=1.  The combinations
//! `{HA=0, HD=1}` and `{S2HA=0, S2HD=1}` are architecturally illegal.
//! `StreamConfig::validate()` must reject them with `ValidationError::InvalidConfiguration`.
//!
//! ## BUG-AUDIT-130: `translate_two_stage()` missing S2 HTTU update (§3.13.2 / §3.13.4)
//!
//! The non-GATOS two-stage path (`translate_two_stage`) omits the stage-2
//! `update_access_flags` call after a successful stage-2 translation.
//! After this fix, a `translate()` call on a two-stage stream with S2HA=true
//! and an AF=0 stage-2 page must atomically set AF=1 in the stage-2 descriptor,
//! mirroring the existing fix in `translate_two_stage_with_ipa` (BUG-AUDIT-128).
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]

use smmu::types::{
    AccessType, FaultMode, PagePermissions, SecurityState,
    StreamConfig, StreamID, IOVA, PA, PASID,
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

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(n: u64) -> IOVA {
    IOVA::new(n).unwrap()
}

fn pa(n: u64) -> PA {
    PA::new(n).unwrap()
}

// ============================================================================
// BUG-AUDIT-129: HD=1 requires HA=1 (stage-1 constraint)
// ============================================================================

/// §3.13.4 — {HA=0, HD=1} is architecturally illegal.
/// `StreamConfig::validate()` must return `Err` for this combination.
///
/// BEFORE FIX: `validate()` returns `Ok` → test FAILS.
/// AFTER FIX:  `validate()` returns `Err` → test PASSES.
#[test]
fn test_hd_requires_ha_stage1() {
    // Build a config with ha=false, hd=true — architecturally illegal per §3.13.4.
    let cfg = StreamConfig::builder()
        .stage1_enabled(false) // Bypass the HTTU/stage-enabled check in check_ste_illegal.
        .stage2_enabled(false) // Use bypass mode so configure_stream won't reject for other reasons.
        .translation_enabled(false)
        .build()
        .map(|mut c| {
            c.ha = false;
            c.hd = true;
            c
        })
        .expect("builder itself should not fail");

    // The validate() call must reject {HA=0, HD=1} as architecturally illegal.
    let result = cfg.validate();
    assert!(
        result.is_err(),
        "BUG-AUDIT-129: StreamConfig::validate() must return Err for {{HA=0, HD=1}} \
         (§3.13.4: dirty-state HTTU implies access-flag HTTU)"
    );
}

/// §3.13.4 — {S2HA=0, S2HD=1} is architecturally illegal.
/// `StreamConfig::validate()` must return `Err` for this combination.
///
/// BEFORE FIX: `validate()` returns `Ok` → test FAILS.
/// AFTER FIX:  `validate()` returns `Err` → test PASSES.
#[test]
fn test_s2hd_requires_s2ha() {
    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .translation_enabled(false)
        .build()
        .map(|mut c| {
            c.s2ha = false;
            c.s2hd = true;
            c
        })
        .expect("builder itself should not fail");

    let result = cfg.validate();
    assert!(
        result.is_err(),
        "BUG-AUDIT-129: StreamConfig::validate() must return Err for {{S2HA=0, S2HD=1}} \
         (§3.13.4: stage-2 dirty-state HTTU implies stage-2 access-flag HTTU)"
    );
}

/// §3.13.4 regression: {HA=1, HD=1} is the valid combination — must be accepted.
#[test]
fn test_ha_hd_both_true_is_valid() {
    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .translation_enabled(false)
        .build()
        .map(|mut c| {
            c.ha = true;
            c.hd = true;
            c
        })
        .expect("builder itself should not fail");

    let result = cfg.validate();
    assert!(
        result.is_ok(),
        "§3.13.4 regression: {{HA=1, HD=1}} must be accepted, got {result:?}"
    );
}

/// §3.13.4 regression: {S2HA=1, S2HD=1} is valid — must be accepted.
#[test]
fn test_s2ha_s2hd_both_true_is_valid() {
    let cfg = StreamConfig::builder()
        .stage1_enabled(false)
        .stage2_enabled(false)
        .translation_enabled(false)
        .build()
        .map(|mut c| {
            c.s2ha = true;
            c.s2hd = true;
            c
        })
        .expect("builder itself should not fail");

    let result = cfg.validate();
    assert!(
        result.is_ok(),
        "§3.13.4 regression: {{S2HA=1, S2HD=1}} must be accepted, got {result:?}"
    );
}

// ============================================================================
// BUG-AUDIT-130: translate_two_stage() missing S2 HTTU update
// ============================================================================

/// §3.13.2/§3.13.4 — non-GATOS `translate()` on a two-stage stream with
/// S2HA=true and a stage-2 page that has AF=0 must atomically set AF=1
/// in the stage-2 descriptor after a successful translation.
///
/// This exercises the `translate_two_stage()` (non-GATOS) path, NOT the
/// GATOS `translate_two_stage_with_ipa()` path already fixed by BUG-AUDIT-128.
///
/// BEFORE FIX: stage-2 AF remains 0 after `translate()` → test FAILS.
/// AFTER FIX:  stage-2 AF is set to 1 by the hardware update → test PASSES.
#[test]
fn test_translate_two_stage_s2ha_updates_af() {
    let smmu = make_smmu();
    let stream_id = sid(129);

    // Two-stage stream with S2HA=true.
    // S2HD is left false (no dirty-state update needed for this test).
    // S1: ha=false (so no stage-1 AF update complicates the picture).
    // S1: affd=true (so AF=0 on stage-1 side does not fault).
    let mut cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2ha = true;   // S2HA=1: stage-2 hardware AF update enabled.
    cfg.s2hd = false;  // S2HD=0 — access-flag update only.
    cfg.ha = false;    // Stage-1 HA disabled to isolate stage-2 behavior.
    cfg.affd = true;   // Stage-1 AFFD=1 suppresses stage-1 AF faults.
    cfg.s2affd = false; // S2AFFD=0: AF faults enabled (overridden by S2HA).

    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1: map IOVA 0x1000 → IPA 0x2000 (AF=true so no S1 AF fault).
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-2: map IPA 0x2000 → PA 0x3000 with AF=0 (unaccessed).
    smmu.map_stage2_page_unaccessed(
        stream_id,
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Verify stage-2 AF starts at 0.
    let af_before = smmu.get_stage2_page_access_flag(stream_id, iova(0x2000));
    assert_eq!(
        af_before,
        Some(false),
        "Pre-condition: stage-2 page must start with AF=0"
    );

    // translate() uses the non-GATOS path (translate_two_stage) — NOT translate_with_ipa.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-AUDIT-130: S2HA=true must allow translation through AF=0 stage-2 page \
         without fault, got {result:?}"
    );

    // After translate(), the stage-2 AF must be set to 1 by hardware update.
    let af_after = smmu.get_stage2_page_access_flag(stream_id, iova(0x2000));
    assert_eq!(
        af_after,
        Some(true),
        "BUG-AUDIT-130: translate_two_stage() must set AF=1 in stage-2 descriptor \
         when S2HA=true (§3.13.2 / §3.13.4)"
    );

    // No spurious fault events.
    let events = smmu.get_events();
    let any_event = events.iter().find(|e| e.stream_id == stream_id.as_u32());
    assert!(
        any_event.is_none(),
        "BUG-AUDIT-130: No fault events expected after successful translate() with S2HA=true, \
         got {events:?}"
    );
}
