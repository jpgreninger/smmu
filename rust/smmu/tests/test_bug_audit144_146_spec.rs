#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! BUG-AUDIT-144: §3.17 TLB tagging — EL2 and EL3 StreamWorlds must not tag
//! TLB entries with ASID or VMID.
//!
//! Per ARM IHI0070G.b §3.17 Table "TLB tagging for each StreamWorld":
//!   any-EL2 (STRW=0b01): ASID=No, VMID=No
//!   EL3     (STRW=0b11): ASID=No, VMID=No
//!
//! BUG-AUDIT-146: §3.17.6 VMW broadcast invalidation — TlbiNhAll via
//! receive_broadcast_tlbi() must apply CR0.VMW wildcard masking to VMID.
//!
//! Per ARM IHI0070G.b §3.17.6: "Both broadcast TLB invalidation and explicit
//! SMMU TLB invalidation commands...affect all Non-secure VMIDs that match
//! the group wildcard when SMMU_CR0.VMW!=0."

use smmu::types::{
    AccessType, CommandType, PagePermissions, SecurityState, StreamConfig, StreamID,
    StreamWorld, IOVA, PA, PASID,
};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }
fn pa(addr: u64) -> PA { PA::new(addr).unwrap() }

// ── BUG-AUDIT-144: EL2 STRW must not tag TLB entries with ASID ───────────────

/// §3.17 BUG-AUDIT-144 — a Secure stream with STRW=El2 (S-EL2) must store ASID=0
/// in TLB, even if the PASID has a non-zero ASID configured.
///
/// Per ARM §3.17 Table: any-EL2 (STRW=0b01) → ASID=No.
/// STRW=El2 is valid only for Secure STEs (NonSecure+El2 is C_BAD_STE per §5.2).
///
/// Verification: after setting ASID=42 on the PASID and priming the TLB,
/// TlbiNhAsid(42) should NOT evict the entry (it's tagged ASID=0).
///
/// BEFORE FIX: entry has raw ASID=42 → evicted by TlbiNhAsid(42) → miss.
/// AFTER FIX:  entry has ASID=0     → NOT evicted                  → hit.
#[test]
fn test_bug_audit144_el2_tlb_entry_uses_asid_zero() {
    let smmu = make_smmu();

    // S-EL2 stage-1-only stream (Secure security state, STRW=El2)
    let s = sid(50);
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .security_state(SecurityState::Secure)
        .security_enforced(true)
        .strw(StreamWorld::El2)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    // Set a non-zero ASID — must NOT appear in TLB tag for El2
    smmu.set_pasid_asid(s, pasid(0), 42).unwrap();
    smmu.map_page(s, pasid(0), iova(0x5000), pa(0xD000),
                  PagePermissions::read_only(), SecurityState::Secure).unwrap();

    // Prime the TLB
    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::Secure)
        .unwrap();

    // Invalidate ASID 42 — should NOT evict El2 entry (tagged with ASID=0)
    let cmd = smmu::types::CommandEntry {
        cmd_type: CommandType::TlbiNhAsid,
        asid: 42,
        ..smmu::types::CommandEntry::new(CommandType::TlbiNhAsid, 0, 0)
    };
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // After fix: El2 TLB entry (ASID=0) survives — next translate is a cache hit
    let stats_before = smmu.get_cache_statistics();
    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::Secure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "BUG-AUDIT-144: El2 TLB entry must be tagged ASID=0, not ASID=42; \
         TlbiNhAsid(42) must NOT evict it"
    );
}

/// §3.17 BUG-AUDIT-144 — STRW=El2E2h with CR2.E2H=1 must store raw ASID in TLB.
///
/// Per ARM §3.17 Table: any-EL2-E2H (STRW=0b10) → ASID=Yes (if TTD.nG==1).
/// When CR2.E2H=1 (set before enabling SMMU since CR2 is RO when SMMUEN=1),
/// El2E2h entries keep their ASID tag.
/// TlbiNhAsid(42) MUST evict the El2E2h entry → second translate is a TLB miss.
///
/// This is a regression guard: El2E2h must not be over-zeroed by the El2 fix.
#[test]
fn test_bug_audit144_el2e2h_tlb_entry_uses_nonzero_asid() {
    // CR2 is RO when SMMUEN=1 — set E2H before enabling
    let smmu = SMMU::new();
    smmu.set_cr2(SMMU::CR2_E2H); // set before enable
    smmu.enable().unwrap();

    let s = sid(52);
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .strw(StreamWorld::El2E2h)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.set_pasid_asid(s, pasid(0), 42).unwrap();
    smmu.map_page(s, pasid(0), iova(0x5000), pa(0xD000),
                  PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // With El2E2h and CR2.E2H=1, the entry is tagged ASID=42
    // TlbiNhAsid(42) MUST evict it
    let cmd = smmu::types::CommandEntry {
        cmd_type: CommandType::TlbiNhAsid,
        asid: 42,
        ..smmu::types::CommandEntry::new(CommandType::TlbiNhAsid, 0, 0)
    };
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let stats_before = smmu.get_cache_statistics();
    smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_after.tlb_misses() > stats_before.tlb_misses(),
        "BUG-AUDIT-144 regression: El2E2h with CR2.E2H=1 must keep ASID=42 tag; \
         TlbiNhAsid(42) must evict it"
    );
}

// ── BUG-AUDIT-146: receive_broadcast_tlbi TlbiNhAll must apply CR0.VMW mask ──

/// §3.17.6 BUG-AUDIT-146 — broadcast TlbiNhAll with vmid=4 and VMW=2 must
/// evict TLB entries tagged with VMID 4,5,6,7 (bits[1:0] wildcarded).
///
/// Setup: two stage-1 streams — one with VMID=4, one with VMID=6.
/// Set VMW=2 in CR0.  Issue receive_broadcast_tlbi(TlbiNhAll, vmid=4).
/// AFTER FIX: both entries are evicted (vmid & ~0x3 == 4 & ~0x3 == 4) → TLB misses.
/// BEFORE FIX: TlbiNhAll invokes invalidate_nh_by_vmid(4) with no mask, leaving vmid=6 alive.
#[test]
fn test_bug_audit146_broadcast_tlbi_nh_all_applies_vmw_mask() {
    let smmu = make_smmu();

    // Stream A: VMID=4, stage-1 only
    let sa = sid(60);
    smmu.configure_stream(sa, StreamConfig::stage1_only()).unwrap();
    smmu.set_stream_vmid(sa, 4).unwrap();
    smmu.create_pasid(sa, pasid(0)).unwrap();
    smmu.map_page(sa, pasid(0), iova(0x7000), pa(0xF000),
                  PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Stream B: VMID=6, stage-1 only
    let sb = sid(61);
    smmu.configure_stream(sb, StreamConfig::stage1_only()).unwrap();
    smmu.set_stream_vmid(sb, 6).unwrap();
    smmu.create_pasid(sb, pasid(0)).unwrap();
    smmu.map_page(sb, pasid(0), iova(0x7000), pa(0xF000),
                  PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Prime both into TLB
    smmu.translate(sa, pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    smmu.translate(sb, pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Set VMW=2 (wildcard bottom 2 VMID bits)
    let cr0 = smmu.get_cr0() | (2u32 << SMMU::CR0_VMW_SHIFT);
    smmu.set_cr0(cr0);

    // Broadcast TlbiNhAll with vmid=4 — with VMW=2, must match vmid=4,5,6,7
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAll, 0, 4, iova(0));

    // Both streams (vmid=4 and vmid=6) should now be TLB misses
    let stats_before = smmu.get_cache_statistics();
    smmu.translate(sa, pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_mid = smmu.get_cache_statistics();
    smmu.translate(sb, pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_mid.tlb_misses() > stats_before.tlb_misses(),
        "BUG-AUDIT-146: broadcast TlbiNhAll with VMW=2 must evict vmid=4 entry (miss on re-translate)"
    );
    assert!(
        stats_after.tlb_misses() > stats_mid.tlb_misses(),
        "BUG-AUDIT-146: broadcast TlbiNhAll with VMW=2 must evict vmid=6 entry (miss on re-translate)"
    );
}

/// §3.17.6 BUG-AUDIT-146 regression — when VMW=0, broadcast TlbiNhAll with vmid=4
/// must NOT evict entries with vmid=6 (exact VMID match only).
#[test]
fn test_bug_audit146_broadcast_tlbi_nh_all_vmw0_exact_match() {
    let smmu = make_smmu();

    let sa = sid(62);
    smmu.configure_stream(sa, StreamConfig::stage1_only()).unwrap();
    smmu.set_stream_vmid(sa, 4).unwrap();
    smmu.create_pasid(sa, pasid(0)).unwrap();
    smmu.map_page(sa, pasid(0), iova(0x8000), pa(0x10000),
                  PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    let sb = sid(63);
    smmu.configure_stream(sb, StreamConfig::stage1_only()).unwrap();
    smmu.set_stream_vmid(sb, 6).unwrap();
    smmu.create_pasid(sb, pasid(0)).unwrap();
    smmu.map_page(sb, pasid(0), iova(0x8000), pa(0x10000),
                  PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    smmu.translate(sa, pasid(0), iova(0x8000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    smmu.translate(sb, pasid(0), iova(0x8000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // VMW=0: exact VMID match only — ensure clear
    let cr0 = smmu.get_cr0() & !(SMMU::CR0_VMW_MASK);
    smmu.set_cr0(cr0);

    // Broadcast TlbiNhAll with vmid=4 — should only evict vmid=4, not vmid=6
    smmu.receive_broadcast_tlbi(CommandType::TlbiNhAll, 0, 4, iova(0));

    // vmid=6 entry must survive → hit on re-translate
    let stats_before = smmu.get_cache_statistics();
    smmu.translate(sb, pasid(0), iova(0x8000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "BUG-AUDIT-146 regression: with VMW=0, TlbiNhAll(vmid=4) must NOT evict vmid=6 entry"
    );
}
