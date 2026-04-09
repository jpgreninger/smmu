#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! §3.17 — When S2P==0, broadcast TLBI with vmid≠0 must match vmid=0 entries.
//!
//! ARM spec §3.17: "When SMMU_IDR0.S2P==0, the SMMU matches VMID 0 for incoming
//! broadcast TLB invalidation messages for a regime that uses VMIDs."
//!
//! BUG-AUDIT-131: `receive_broadcast_tlbi()` must substitute `vmid=0` for all
//! VMID-targeted broadcast invalidations when S2P==0.

use smmu::types::{
    AccessType, CommandType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA,
    PASID,
};
use smmu::SMMU;

fn make_smmu_s2p_false() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_s2p_supported(false);
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

/// §3.17 BUG-AUDIT-131 — When S2P==0, `TlbiNhAll` with vmid=5 (non-zero) must
/// substitute vmid=0 and thus invalidate TLB entries that are tagged vmid=0.
///
/// With S2P==0 every stream has vmid=0 (stage-2 not supported).
/// BEFORE FIX: `invalidate_nh_by_vmid(5)` finds nothing → TLB entry survives → cache hit.
/// AFTER FIX:  `invalidate_nh_by_vmid(0)` evicts the vmid=0 entry → cache miss on next translate.
#[test]
fn test_broadcast_tlbi_s2p0_substitutes_vmid0() {
    let smmu = make_smmu_s2p_false();

    // Configure a simple S1-only stream (vmid defaults to 0).
    let s = sid(1);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x1000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Prime the TLB by translating once.
    let result = smmu.translate(s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "initial translate must succeed");

    // Snapshot hits before issuing the broadcast TLBI.
    let stats_before = smmu.get_cache_statistics();
    let hits_before = stats_before.tlb_hits();

    // Verify the entry is actually cached: a second translate should be a hit.
    smmu.translate(s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure).unwrap();
    let stats_after_prime = smmu.get_cache_statistics();
    assert!(
        stats_after_prime.tlb_hits() > hits_before,
        "TLB entry must be cached before broadcast TLBI"
    );

    // Issue broadcast TlbiNhAll with vmid=5 (non-zero) while S2P==0.
    // With the fix, vmid is substituted with 0, evicting the vmid=0 entry.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAll, 0, 5, iova(0));

    // After the broadcast TLBI the TLB entry for vmid=0 should be gone.
    // A translate now must re-walk the page table (cache miss), which still
    // returns the correct PA.
    let lookups_before_remiss = smmu.get_cache_statistics().tlb_lookups();
    let result2 = smmu.translate(s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result2.is_ok(), "translate after broadcast TLBI must still succeed");
    assert_eq!(
        result2.unwrap().physical_address().as_u64(),
        0xA000,
        "PA must match original mapping after TLBI"
    );

    // The post-TLBI translate must NOT be a cache hit (the entry was evicted).
    let stats_final = smmu.get_cache_statistics();
    // Total hits should not have increased relative to lookups: we expect a miss.
    // More precisely: the hit count must be the same as it was right after the TLBI
    // (i.e., no new hit was recorded for the re-translate).
    let hits_at_tlbi = stats_after_prime.tlb_hits();
    assert_eq!(
        stats_final.tlb_hits(),
        hits_at_tlbi,
        "§3.17 BUG-AUDIT-131: S2P==0 broadcast TLBI with vmid=5 must \
         substitute vmid=0 and evict the cached entry (no new hit expected); \
         before fix the vmid-5 invalidation misses and the entry stays cached"
    );
    let _ = lookups_before_remiss; // used above for clarity
}

/// §3.17 BUG-AUDIT-131 — TlbiNhAsid with vmid=7 (non-zero) while S2P==0
/// must also substitute vmid=0.
#[test]
fn test_broadcast_tlbi_nh_asid_s2p0_substitutes_vmid0() {
    let smmu = make_smmu_s2p_false();

    let s = sid(2);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    // Set ASID=42 so the ASID-targeted invalidation has a chance to match.
    smmu.set_pasid_asid(s, pasid(0), 42).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x2000),
        pa(0xB000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Prime the TLB.
    smmu.translate(s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure).unwrap();

    // Confirm the entry is cached.
    let hits_before = smmu.get_cache_statistics().tlb_hits();
    smmu.translate(s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert!(
        smmu.get_cache_statistics().tlb_hits() > hits_before,
        "TLB entry must be cached before TLBI"
    );
    let hits_after_prime = smmu.get_cache_statistics().tlb_hits();

    // TlbiNhAsid with vmid=7, asid=42: with fix → vmid→0, evicts the entry.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAsid, 42, 7, iova(0));

    // Must be a miss (no new hit).
    smmu.translate(s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert_eq!(
        smmu.get_cache_statistics().tlb_hits(),
        hits_after_prime,
        "§3.17 BUG-AUDIT-131: TlbiNhAsid with vmid=7 while S2P==0 must \
         substitute vmid=0 and evict the cached entry"
    );
}

/// §3.17 BUG-AUDIT-131 — TlbiNhVa with vmid=3 (non-zero) while S2P==0
/// must substitute vmid=0.
#[test]
fn test_broadcast_tlbi_nh_va_s2p0_substitutes_vmid0() {
    let smmu = make_smmu_s2p_false();

    let s = sid(3);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.set_pasid_asid(s, pasid(0), 10).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x3000),
        pa(0xC000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.translate(s, pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure).unwrap();

    let hits_before = smmu.get_cache_statistics().tlb_hits();
    smmu.translate(s, pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert!(
        smmu.get_cache_statistics().tlb_hits() > hits_before,
        "TLB entry must be cached before TLBI"
    );
    let hits_after_prime = smmu.get_cache_statistics().tlb_hits();

    // TlbiNhVa with vmid=3, asid=10, va=0x3000: fix → vmid→0, evicts the entry.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhVa, 10, 3, iova(0x3000));

    smmu.translate(s, pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert_eq!(
        smmu.get_cache_statistics().tlb_hits(),
        hits_after_prime,
        "§3.17 BUG-AUDIT-131: TlbiNhVa with vmid=3 while S2P==0 must \
         substitute vmid=0 and evict the cached entry"
    );
}

/// §3.17 BUG-AUDIT-131 — TlbiNhVaa with vmid=9 (non-zero) while S2P==0
/// must substitute vmid=0.
#[test]
fn test_broadcast_tlbi_nh_vaa_s2p0_substitutes_vmid0() {
    let smmu = make_smmu_s2p_false();

    let s = sid(4);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x4000),
        pa(0xD000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.translate(s, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure).unwrap();

    let hits_before = smmu.get_cache_statistics().tlb_hits();
    smmu.translate(s, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert!(
        smmu.get_cache_statistics().tlb_hits() > hits_before,
        "TLB entry must be cached before TLBI"
    );
    let hits_after_prime = smmu.get_cache_statistics().tlb_hits();

    // TlbiNhVaa with vmid=9, va=0x4000: fix → vmid→0, evicts the entry.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhVaa, 0, 9, iova(0x4000));

    smmu.translate(s, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert_eq!(
        smmu.get_cache_statistics().tlb_hits(),
        hits_after_prime,
        "§3.17 BUG-AUDIT-131: TlbiNhVaa with vmid=9 while S2P==0 must \
         substitute vmid=0 and evict the cached entry"
    );
}

/// §3.17 — Sanity: when S2P==1, the original vmid must be used (no substitution).
/// Issuing TlbiNhAll with vmid=5 while S2P==1 must NOT evict a vmid=0 entry.
#[test]
fn test_broadcast_tlbi_s2p1_preserves_original_vmid() {
    let smmu = SMMU::new();
    // S2P defaults to true — leave it as-is.
    smmu.enable().unwrap();

    let s = sid(5);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(0x5000),
        pa(0xE000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Prime the TLB for vmid=0 (default).
    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure).unwrap();

    let hits_before = smmu.get_cache_statistics().tlb_hits();
    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure).unwrap();
    let hits_after_prime = smmu.get_cache_statistics().tlb_hits();
    assert!(hits_after_prime > hits_before, "entry must be cached");

    // Issue TlbiNhAll with vmid=5 while S2P==1: only vmid=5 entries are evicted.
    // The vmid=0 entry must survive.
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAll, 0, 5, iova(0));

    // The vmid=0 entry must still be a cache hit.
    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure).unwrap();
    assert!(
        smmu.get_cache_statistics().tlb_hits() > hits_after_prime,
        "§3.17: when S2P==1, TlbiNhAll with vmid=5 must NOT evict vmid=0 entries"
    );
}
