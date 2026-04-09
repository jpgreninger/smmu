//! TDD tests for BUG-AUDIT-128.
//!
//! ARM SMMU v3 spec §3.13.2:
//! "When HTTU is supported and enabled for a stream, a translation that causes an SMMU
//!  fetch of a descriptor with AF==0 that would, without HTTU and with AFFD==0, have
//!  caused an Access fault performs an atomic update to set AF==1 in the descriptor."
//!
//! This applies to stage-2 descriptors in a two-stage translation stream.  When STE.S2HA==1,
//! a successful stage-2 translation with AF==0 must:
//! 1. Set AF=1 in the stage-2 descriptor (hardware update).
//! 2. NOT generate an `F_ACCESS` fault.
//!
//! BEFORE FIX: `translate_and_get_stage2_ipa()` calls `update_access_flags` only on the
//!             stage-1 address space (when ha/hd are set) but never on the stage-2 address
//!             space (even when s2ha is set). Stage-2 pages remain AF=0 after translation.
//!
//! AFTER FIX:  After a successful stage-2 translation, if s2ha==true or s2hd==true,
//!             `update_access_flags` is called on the stage-2 address space with the IPA.
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
// BUG-AUDIT-128: Stage-2 HA hardware AF update missing in two-stage translation
// ============================================================================

/// Two-stage stream: S2HA=true, stage-2 page has AF=0.
/// After translation, the stage-2 descriptor AF must be set to 1 (hardware update).
/// The translation must succeed (no `F_ACCESS`).
#[test]
fn bug_audit128_two_stage_s2ha_sets_af_on_stage2_descriptor() {
    let smmu = make_smmu();
    let stream_id = sid(50);

    // Two-stage stream: stage-1 + stage-2. S2HA=true so stage-2 AF updates are hardware-managed.
    let mut cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2ha = true;   // Stage-2 hardware AF update enabled
    cfg.s2affd = false;
    cfg.ha = false;    // Stage-1 HA disabled (to isolate stage-2 behavior)
    cfg.affd = true;   // Stage-1 AFFD=true so stage-1 AF=0 won't fault
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1: map IOVA 0x1000 → IPA 0x2000 (AF=true, so no S1 AF fault)
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-2: map IPA 0x2000 → PA 0x3000 with AF=0 (unaccessed)
    smmu.map_stage2_page_unaccessed(
        stream_id,
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Verify stage-2 page starts with AF=0
    let af_before = smmu.get_stage2_page_access_flag(stream_id, iova(0x2000));
    assert_eq!(af_before, Some(false), "Stage-2 page must start with AF=0 for this test");

    // Translate: should succeed (s2ha=true means no F_ACCESS on AF=0)
    let result = smmu.translate(
        stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-AUDIT-128: S2HA=true should allow translation through AF=0 stage-2 page, got {result:?}"
    );

    // After translation, stage-2 AF must have been set to 1 by hardware update
    let af_after = smmu.get_stage2_page_access_flag(stream_id, iova(0x2000));
    assert_eq!(
        af_after, Some(true),
        "BUG-AUDIT-128: S2HA=true must atomically set AF=1 in stage-2 descriptor after translation"
    );

    // No F_ACCESS event should have been emitted
    let events = smmu.get_events();
    let af_fault = events.iter().find(|e| e.stream_id == stream_id.as_u32());
    assert!(af_fault.is_none(), "BUG-AUDIT-128: No events expected, got {events:?}");
}

/// Two-stage stream: S2HA=false, S2AFFD=false, stage-2 page has AF=0.
/// Translation must fail with `F_ACCESS` (no hardware update, no suppression).
/// This validates the existing correct behavior is preserved after the fix.
#[test]
fn bug_audit128_two_stage_s2ha_false_generates_f_access_for_af_zero() {
    let smmu = make_smmu();
    let stream_id = sid(51);

    let mut cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2ha = false;
    cfg.s2affd = false;
    cfg.ha = false;
    cfg.affd = true;   // Stage-1 AFFD=true to isolate stage-2 behavior
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    smmu.map_stage2_page_unaccessed(
        stream_id,
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "S2HA=false + S2AFFD=false + AF=0 must generate F_ACCESS error, got Ok"
    );
}

/// Two-stage stream: S2HA=true, stage-2 page already has AF=1.
/// Translation must succeed and AF remains 1 (no spurious toggle).
#[test]
fn bug_audit128_two_stage_s2ha_true_af_already_set_succeeds() {
    let smmu = make_smmu();
    let stream_id = sid(52);

    let mut cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s2_t0sz(25)
        .s2_ps(5)
        .build()
        .unwrap();
    cfg.s2ha = true;
    cfg.s2affd = false;
    cfg.ha = false;
    cfg.affd = true;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-2 page with AF=1 (already accessed) — use regular map_stage2_page
    smmu.map_stage2_page(
        stream_id,
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    let result = smmu.translate(
        stream_id, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "S2HA=true + AF=1 must succeed, got {result:?}");

    let af_after = smmu.get_stage2_page_access_flag(stream_id, iova(0x2000));
    assert_eq!(af_after, Some(true), "AF must remain 1 after translation");
}
