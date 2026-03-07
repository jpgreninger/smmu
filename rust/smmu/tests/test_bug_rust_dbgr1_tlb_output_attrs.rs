#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]

//! TDD regression test for BUG-RUST-DBGR-1:
//! TLB cache hit discards all six STE output-attribute fields.
//!
//! Problem: The TLB fast-path constructs `TranslationData::new(pa, perms, sec)`
//! which zero-initialises all six output-attribute fields. The slow path
//! correctly calls `apply_output_attrs()`. On a cache hit the fields are always
//! zero regardless of STE configuration.
//!
//! Fix: After constructing TranslationData on a cache hit, look up the stream
//! context and call `apply_output_attrs()` to populate the fields from the
//! live stream configuration before returning.

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
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

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Configure a stream with mt_cfg=true, mem_attr=7, sh_cfg=2.
/// Translate the SAME iova twice:
///   - First translate: slow path (TLB miss) — output attrs must be populated.
///   - Second translate: fast path (TLB hit) — output attrs must still be populated.
///
/// Before the fix: second translate returns mem_type=0, shareability=0.
/// After the fix: second translate returns mem_type=7, shareability=2.
#[test]
fn dbgr1_tlb_cache_hit_preserves_mem_type_and_shareability() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .mt_cfg(true)
        .mem_attr(7)
        .sh_cfg(2)
        .build()
        .unwrap();

    smmu.configure_stream(sid(10), cfg).unwrap();
    smmu.create_pasid(sid(10), pasid(0)).unwrap();
    smmu.map_page(
        sid(10),
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate — slow path (TLB miss).
    let result1 = smmu
        .translate(sid(10), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .expect("DBGR-1: first translate must succeed");

    assert_eq!(
        result1.mem_type(),
        7,
        "DBGR-1: slow path must return mem_type=7 (STE.MTCFG/MemAttr)"
    );
    assert_eq!(
        result1.shareability(),
        2,
        "DBGR-1: slow path must return shareability=2 (STE.SHCFG)"
    );

    // Second translate of the SAME IOVA — fast path (TLB hit).
    let result2 = smmu
        .translate(sid(10), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure)
        .expect("DBGR-1: second translate must succeed (cache hit)");

    assert_eq!(
        result2.mem_type(),
        7,
        "DBGR-1 BUG: TLB cache hit path discards mem_type; \
         expected 7 but got {} (STE.MTCFG/MemAttr must survive cache hit)",
        result2.mem_type()
    );
    assert_eq!(
        result2.shareability(),
        2,
        "DBGR-1 BUG: TLB cache hit path discards shareability; \
         expected 2 but got {} (STE.SHCFG must survive cache hit)",
        result2.shareability()
    );
}

/// Physical address from cache hit must still be correct (regression guard).
#[test]
fn dbgr1_tlb_cache_hit_physical_address_unchanged() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .mt_cfg(true)
        .mem_attr(5)
        .sh_cfg(3)
        .build()
        .unwrap();

    smmu.configure_stream(sid(11), cfg).unwrap();
    smmu.create_pasid(sid(11), pasid(0)).unwrap();
    smmu.map_page(
        sid(11),
        pasid(0),
        iova(0x2000),
        pa(0x9000_2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let r1 = smmu
        .translate(sid(11), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let r2 = smmu
        .translate(sid(11), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // PA must match on both paths.
    assert_eq!(
        r1.physical_address(),
        r2.physical_address(),
        "DBGR-1: physical address must be identical on slow and fast path"
    );
    // Output attributes must also match.
    assert_eq!(r1.mem_type(), r2.mem_type(), "DBGR-1: mem_type mismatch slow vs fast path");
    assert_eq!(
        r1.shareability(),
        r2.shareability(),
        "DBGR-1: shareability mismatch slow vs fast path"
    );
}

/// When mt_cfg=false, mem_type must be 0 on both slow and fast path.
#[test]
fn dbgr1_tlb_cache_hit_mem_type_zero_when_mt_cfg_false() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .mt_cfg(false)
        .mem_attr(7) // ignored because mt_cfg=false
        .sh_cfg(0)
        .build()
        .unwrap();

    smmu.configure_stream(sid(12), cfg).unwrap();
    smmu.create_pasid(sid(12), pasid(0)).unwrap();
    smmu.map_page(
        sid(12),
        pasid(0),
        iova(0x3000),
        pa(0xA000_3000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let r1 = smmu
        .translate(sid(12), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let r2 = smmu
        .translate(sid(12), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // mt_cfg=false → mem_type=0 on both paths.
    assert_eq!(r1.mem_type(), 0, "DBGR-1: slow path mem_type must be 0 when mt_cfg=false");
    assert_eq!(
        r2.mem_type(),
        0,
        "DBGR-1: cache hit mem_type must be 0 when mt_cfg=false"
    );
}

/// All six output-attribute fields are preserved across a cache hit.
#[test]
fn dbgr1_tlb_cache_hit_all_six_output_attrs_preserved() {
    let smmu = smmu_enabled();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .mt_cfg(true)
        .mem_attr(6)
        .sh_cfg(3)
        .alloc_cfg(0xF)
        .inst_cfg(2)
        .priv_cfg(1)
        .ns_cfg(1)
        .build()
        .unwrap();

    smmu.configure_stream(sid(13), cfg).unwrap();
    smmu.create_pasid(sid(13), pasid(0)).unwrap();
    smmu.map_page(
        sid(13),
        pasid(0),
        iova(0x4000),
        pa(0xB000_4000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Slow path
    let r1 = smmu
        .translate(sid(13), pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    // Fast path (cache hit)
    let r2 = smmu
        .translate(sid(13), pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(
        r1.mem_type(),
        r2.mem_type(),
        "DBGR-1: mem_type must match across slow/fast paths"
    );
    assert_eq!(
        r1.shareability(),
        r2.shareability(),
        "DBGR-1: shareability must match across slow/fast paths"
    );
    assert_eq!(
        r1.alloc_hint(),
        r2.alloc_hint(),
        "DBGR-1: alloc_hint must match across slow/fast paths"
    );
    assert_eq!(
        r1.inst_cfg(),
        r2.inst_cfg(),
        "DBGR-1: inst_cfg must match across slow/fast paths"
    );
    assert_eq!(
        r1.priv_cfg(),
        r2.priv_cfg(),
        "DBGR-1: priv_cfg must match across slow/fast paths"
    );
    assert_eq!(
        r1.ns_cfg_out(),
        r2.ns_cfg_out(),
        "DBGR-1: ns_cfg_out must match across slow/fast paths"
    );
    // Verify non-zero values were populated correctly on both paths
    assert_eq!(r2.mem_type(), 6, "DBGR-1: cache hit mem_type must be 6");
    assert_eq!(r2.shareability(), 3, "DBGR-1: cache hit shareability must be 3");
}
