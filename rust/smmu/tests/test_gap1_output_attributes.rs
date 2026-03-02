#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

//! TDD tests for GAP-1: STE output-attribute override fields
//! These tests MUST FAIL before implementation.

use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamConfig, TranslationData, IOVA, PA, PASID,
};

fn make_pasid(v: u32) -> PASID {
    PASID::new(v).unwrap()
}

fn make_iova(v: u64) -> IOVA {
    IOVA::new(v).unwrap()
}

fn make_pa(v: u64) -> PA {
    PA::new(v).unwrap()
}

// ===== GAP-1: TranslationData has output-attribute fields =====

#[test]
fn test_translation_data_has_mem_type_field() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(data.mem_type(), 0u8);
}

#[test]
fn test_translation_data_has_shareability_field() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(data.shareability(), 0u8);
}

#[test]
fn test_translation_data_has_alloc_hint_field() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(data.alloc_hint(), 0u8);
}

#[test]
fn test_translation_data_has_inst_cfg_field() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(data.inst_cfg(), 0u8);
}

#[test]
fn test_translation_data_has_priv_cfg_field() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(data.priv_cfg(), 0u8);
}

#[test]
fn test_translation_data_has_ns_cfg_out_field() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure);
    assert_eq!(data.ns_cfg_out(), 0u8);
}

// Helper: build a stage1-only StreamContext ready for translation
fn make_stage1_ctx(cfg: StreamConfig) -> StreamContext {
    let mut ctx = StreamContext::new();
    ctx.update_configuration(cfg);
    ctx.enable();

    let as1 = AddressSpace::new();
    let perms = PagePermissions::read_only();
    as1.map_page(make_iova(0x1000), make_pa(0x2000), perms, SecurityState::NonSecure)
        .unwrap();
    ctx.add_pasid(make_pasid(0), std::sync::Arc::new(as1)).unwrap();
    ctx.set_stage1_enabled(true);
    ctx
}

#[test]
fn test_mtcfg_true_applies_mem_attr() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .mt_cfg(true)
        .mem_attr(0x07)
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().mem_type(), 0x07u8);
}

#[test]
fn test_mtcfg_false_uses_default() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .mt_cfg(false)
        .mem_attr(0x07)
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().mem_type(), 0u8);
}

#[test]
fn test_sh_cfg_applied() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .sh_cfg(3) // ISH
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().shareability(), 3u8);
}

#[test]
fn test_inst_cfg_applied() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .inst_cfg(2)
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().inst_cfg(), 2u8);
}

#[test]
fn test_priv_cfg_applied() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .priv_cfg(2)
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().priv_cfg(), 2u8);
}

#[test]
fn test_ns_cfg_applied() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .ns_cfg(2)
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().ns_cfg_out(), 2u8);
}

#[test]
fn test_alloc_cfg_applied() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .alloc_cfg(0xA)
        .build()
        .unwrap();
    let ctx = make_stage1_ctx(cfg);

    let result = ctx.translate(make_pasid(0), make_iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed: {result:?}");
    assert_eq!(result.unwrap().alloc_hint(), 0xAu8);
}

#[test]
fn test_with_output_attrs_sets_all_fields() {
    let pa = make_pa(0x1000);
    let perms = PagePermissions::read_only();
    let data = TranslationData::new(pa, perms, SecurityState::NonSecure)
        .with_output_attrs(7, 3, 0xF, 2, 1, 2);
    assert_eq!(data.mem_type(), 7u8);
    assert_eq!(data.shareability(), 3u8);
    assert_eq!(data.alloc_hint(), 0xFu8);
    assert_eq!(data.inst_cfg(), 2u8);
    assert_eq!(data.priv_cfg(), 1u8);
    assert_eq!(data.ns_cfg_out(), 2u8);
}
