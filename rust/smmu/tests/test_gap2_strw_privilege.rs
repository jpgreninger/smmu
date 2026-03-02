#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

//! TDD tests for GAP-2: STE.STRW privilege check suppression
//! These tests MUST FAIL before implementation.

use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamWorld, IOVA, PA, PASID};

fn make_pasid(v: u32) -> PASID {
    PASID::new(v).unwrap()
}

fn make_iova(v: u64) -> IOVA {
    IOVA::new(v).unwrap()
}

fn make_pa(v: u64) -> PA {
    PA::new(v).unwrap()
}

// Helper: build a stage1-only StreamContext with the given StreamConfig and page perms
fn make_ctx_with_perms(cfg: StreamConfig, perms: PagePermissions) -> StreamContext {
    let mut ctx = StreamContext::new();
    ctx.update_configuration(cfg);
    ctx.enable();

    let as1 = AddressSpace::new();
    as1.map_page(make_iova(0x1000), make_pa(0x2000), perms, SecurityState::NonSecure)
        .unwrap();
    ctx.add_pasid(make_pasid(0), std::sync::Arc::new(as1)).unwrap();
    ctx.set_stage1_enabled(true);
    ctx
}

// ===== GAP-2: PagePermissions.privileged_only =====

#[test]
fn test_page_permissions_default_not_privileged_only() {
    let perms = PagePermissions::read_only();
    assert!(!perms.privileged_only());
}

#[test]
fn test_page_permissions_privileged_only_constructor() {
    let perms = PagePermissions::read_only_privileged();
    assert!(perms.read());
    assert!(!perms.write());
    assert!(perms.privileged_only());
}

#[test]
fn test_with_privileged_only_sets_bit() {
    let perms = PagePermissions::read_write().with_privileged_only(true);
    assert!(perms.read());
    assert!(perms.write());
    assert!(perms.privileged_only());
}

#[test]
fn test_with_privileged_only_clears_bit() {
    let perms = PagePermissions::read_only_privileged().with_privileged_only(false);
    assert!(perms.read());
    assert!(!perms.privileged_only());
}

#[test]
fn test_privileged_only_not_included_in_allows_check() {
    // allows() must NOT deny access simply because privileged_only is set
    let perms = PagePermissions::read_only_privileged();
    assert!(perms.allows(AccessType::Read), "allows() must not check privileged_only");
}

// ===== GAP-2: STRW=EL1/EL0 enforces privileged_only =====

#[test]
fn test_el1el0_unpriv_access_denied_on_priv_page() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El1El0)
        .build()
        .unwrap();
    let priv_perms = PagePermissions::read_only_privileged();
    let ctx = make_ctx_with_perms(cfg, priv_perms);

    // Unprivileged read on a privileged-only page should fail
    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Expected error for unprivileged access to privileged-only page");
}

// ===== GAP-2: STRW=EL2 suppresses privileged_only =====

#[test]
fn test_el2_suppresses_privileged_only() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El2)
        .build()
        .unwrap();
    let priv_perms = PagePermissions::read_only_privileged();
    let ctx = make_ctx_with_perms(cfg, priv_perms);

    // EL2 suppresses privilege check — read should succeed
    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Expected success: EL2 suppresses privileged_only check, got: {result:?}");
}

#[test]
fn test_el3_suppresses_privileged_only() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El3)
        .build()
        .unwrap();
    let priv_perms = PagePermissions::read_only_privileged();
    let ctx = make_ctx_with_perms(cfg, priv_perms);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Expected success: EL3 suppresses privileged_only check, got: {result:?}");
}

#[test]
fn test_el2e2h_does_not_suppress_privileged_only() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El2E2h)
        .build()
        .unwrap();
    let priv_perms = PagePermissions::read_only_privileged();
    let ctx = make_ctx_with_perms(cfg, priv_perms);

    // El2E2h does NOT suppress privilege checks
    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Expected error: El2E2h does not suppress privileged_only check");
}

#[test]
fn test_normal_page_allows_unprivileged_access_el1el0() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El1El0)
        .build()
        .unwrap();
    let normal_perms = PagePermissions::read_only(); // not privileged_only
    let ctx = make_ctx_with_perms(cfg, normal_perms);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Normal page should allow unprivileged read: {result:?}");
}

#[test]
fn test_intersection_preserves_privileged_only_with_or() {
    // If either permission set has privileged_only, the intersection should too
    let priv_perms = PagePermissions::read_only_privileged();
    let normal_perms = PagePermissions::read_only();
    let intersected = priv_perms.intersection(normal_perms);
    assert!(intersected.privileged_only(), "intersection should OR the privileged_only bit");

    let both_priv = PagePermissions::read_only_privileged();
    let intersected2 = both_priv.intersection(PagePermissions::read_only_privileged());
    assert!(intersected2.privileged_only(), "intersection of two priv-only should remain priv-only");

    let neither_priv = PagePermissions::read_only().intersection(PagePermissions::read_only());
    assert!(!neither_priv.privileged_only(), "intersection of two non-priv should not be priv-only");
}
