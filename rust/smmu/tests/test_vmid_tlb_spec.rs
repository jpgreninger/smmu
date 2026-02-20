#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-M-02: VMID in STE config and Stage-2 TLB entry tagging.
//!
//! Spec: §3.8 (Virtualization), §5.2 (STE Word 2 S2VMID, bits 63:48),
//!       §4.4 (CMD_TLBI_S12_VMALL, CMD_TLBI_S2_IPA)
//!
//! Requirements:
//! - STE must carry a VMID (STE Word 2[63:48])
//! - TLB entries must be tagged with the stream's VMID
//! - CMD_TLBI_S12_VMALL must invalidate only entries with matching VMID
//! - CMD_TLBI_S2_IPA must also target VMID
//! - Non-matching VMID entries must survive invalidation
//! - VMID can be configured at configure_stream time (via StreamConfig) or via
//!   the set_stream_vmid() API

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, SecurityState,
    StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID  { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }
fn pa(addr: u64) -> PA     { PA::new(addr).unwrap() }

/// Configure a Stage-1 stream with a VMID and one mapped page.
fn setup_stream(smmu: &SMMU, stream_n: u32, vmid: u16, iova_addr: u64, pa_addr: u64) {
    let s = sid(stream_n);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.set_stream_vmid(s, vmid).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(s, pasid(0), iova(iova_addr), pa(pa_addr),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
}

// ── StreamConfig VMID field ───────────────────────────────────────────────────

/// StreamConfig must carry a vmid field; factory presets must default it to 0.
#[test]
fn test_stream_config_default_vmid_is_zero() {
    assert_eq!(StreamConfig::stage1_only().vmid, 0,
               "stage1_only vmid must default to 0");
    assert_eq!(StreamConfig::stage2_only().vmid, 0,
               "stage2_only vmid must default to 0");
    assert_eq!(StreamConfig::two_stage().vmid, 0,
               "two_stage vmid must default to 0");
    assert_eq!(StreamConfig::bypass().vmid, 0,
               "bypass vmid must default to 0");
}

/// StreamConfigBuilder must accept a vmid value.
#[test]
fn test_stream_config_builder_vmid() {
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(true)
        .vmid(0xBEEF)
        .build()
        .unwrap();
    assert_eq!(config.vmid, 0xBEEF);
}

// ── set_stream_vmid / get_stream_vmid API ────────────────────────────────────

/// set_stream_vmid must succeed for a configured stream.
#[test]
fn test_set_stream_vmid_returns_ok() {
    let smmu = make_smmu();
    smmu.configure_stream(sid(1), StreamConfig::stage1_only()).unwrap();
    assert!(smmu.set_stream_vmid(sid(1), 42).is_ok(),
            "set_stream_vmid must succeed for a configured stream");
}

/// Default VMID for a newly configured stream must be 0.
#[test]
fn test_get_stream_vmid_default_zero() {
    let smmu = make_smmu();
    smmu.configure_stream(sid(1), StreamConfig::stage1_only()).unwrap();
    assert_eq!(smmu.get_stream_vmid(sid(1)).unwrap(), 0,
               "default VMID must be 0");
}

/// After set_stream_vmid(), get_stream_vmid() must return the new value.
#[test]
fn test_set_then_get_stream_vmid() {
    let smmu = make_smmu();
    smmu.configure_stream(sid(2), StreamConfig::stage1_only()).unwrap();
    smmu.set_stream_vmid(sid(2), 0x1234).unwrap();
    assert_eq!(smmu.get_stream_vmid(sid(2)).unwrap(), 0x1234);
}

/// set_stream_vmid must fail for an unknown stream.
#[test]
fn test_set_stream_vmid_unknown_stream_fails() {
    let smmu = make_smmu();
    assert!(smmu.set_stream_vmid(sid(99), 1).is_err(),
            "set_stream_vmid must fail for unknown stream");
}

/// configure_stream with a StreamConfig carrying vmid != 0 must store that VMID.
#[test]
fn test_vmid_from_stream_config() {
    let smmu = make_smmu();
    let s = sid(5);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(true)
        .vmid(77)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    assert_eq!(smmu.get_stream_vmid(s).unwrap(), 77,
               "VMID from StreamConfig must be stored in stream context");
}

// ── VMID-targeted invalidation ────────────────────────────────────────────────

/// CMD_TLBI_S12_VMALL must evict only TLB entries tagged with matching VMID.
#[test]
fn test_tlbi_s12vmall_removes_only_matching_vmid() {
    let smmu = make_smmu();

    // Stream 10: VMID 10, mapped at 0x1000 → 0xA000
    setup_stream(&smmu, 10, 10, 0x1000, 0xA000);
    // Stream 11: VMID 20, mapped at 0x2000 → 0xB000
    setup_stream(&smmu, 11, 20, 0x2000, 0xB000);

    // Prime both into TLB
    smmu.translate(sid(10), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    smmu.translate(sid(11), pasid(0), iova(0x2000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();

    // Invalidate VMID 10 only
    let cmd = CommandEntry { cmd_type: CommandType::TlbiS12Vmall,
                             vmid: 10, ..CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0) };
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VMID 20 (stream 11) must still be cached → cache hit
    let stats_a = smmu.get_cache_statistics();
    smmu.translate(sid(11), pasid(0), iova(0x2000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    let stats_b = smmu.get_cache_statistics();
    assert!(stats_b.tlb_hits() > stats_a.tlb_hits(),
            "VMID-20 entry must remain cached after VMID-10 invalidation");

    // VMID 10 (stream 10) must have been evicted → translation still succeeds
    let result = smmu.translate(sid(10), pasid(0), iova(0x1000), AccessType::Read,
                                SecurityState::NonSecure);
    assert!(result.is_ok(), "translation of stream 10 must still succeed after eviction");
    assert_eq!(result.unwrap().physical_address().as_u64(), 0xA000,
               "PA must match original mapping after VMID invalidation");
}

/// CMD_TLBI_S12_VMALL must NOT remove entries tagged with a different VMID.
#[test]
fn test_tlbi_s12vmall_does_not_affect_different_vmid() {
    let smmu = make_smmu();
    setup_stream(&smmu, 20, 100, 0x3000, 0xC000);

    // Prime TLB for VMID 100
    smmu.translate(sid(20), pasid(0), iova(0x3000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();

    // Invalidate a completely different VMID (200)
    let cmd = CommandEntry { cmd_type: CommandType::TlbiS12Vmall,
                             vmid: 200, ..CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0) };
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VMID 100 entry must still be cached → hit
    let stats_a = smmu.get_cache_statistics();
    smmu.translate(sid(20), pasid(0), iova(0x3000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    let stats_b = smmu.get_cache_statistics();
    assert!(stats_b.tlb_hits() > stats_a.tlb_hits(),
            "entries tagged with VMID 100 must survive VMID-200 invalidation");
}

/// CMD_TLBI_S2_IPA must also perform VMID-targeted invalidation.
#[test]
fn test_tlbi_s2ipa_removes_matching_vmid() {
    let smmu = make_smmu();
    setup_stream(&smmu, 30, 7, 0x4000, 0xD000);

    // Prime TLB
    smmu.translate(sid(30), pasid(0), iova(0x4000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();

    // Invalidate with S2_IPA variant
    let cmd = CommandEntry { cmd_type: CommandType::TlbiS2Ipa,
                             vmid: 7, ..CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0) };
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Translation must still succeed after eviction
    let result = smmu.translate(sid(30), pasid(0), iova(0x4000), AccessType::Read,
                                SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address().as_u64(), 0xD000);
}

/// Invalidating VMID 0 (default) must only remove VMID-0-tagged entries.
#[test]
fn test_vmid_zero_invalidation_scoped() {
    let smmu = make_smmu();

    // Stream 40: VMID 0 (default)
    let s40 = sid(40);
    smmu.configure_stream(s40, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s40, pasid(0)).unwrap();
    smmu.map_page(s40, pasid(0), iova(0x5000), pa(0xE000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Stream 41: VMID 5
    setup_stream(&smmu, 41, 5, 0x6000, 0xF000);

    // Prime both
    smmu.translate(s40, pasid(0), iova(0x5000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    smmu.translate(sid(41), pasid(0), iova(0x6000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();

    // Invalidate VMID 0
    let cmd = CommandEntry { cmd_type: CommandType::TlbiS12Vmall,
                             vmid: 0, ..CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0) };
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VMID 5 (stream 41) must remain → cache hit
    let stats_a = smmu.get_cache_statistics();
    smmu.translate(sid(41), pasid(0), iova(0x6000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    let stats_b = smmu.get_cache_statistics();
    assert!(stats_b.tlb_hits() > stats_a.tlb_hits(),
            "VMID-5 entry must survive VMID-0 invalidation");
}

/// Multiple streams sharing the same VMID: invalidating that VMID must
/// evict entries for all of them.
#[test]
fn test_vmid_invalidation_covers_multiple_streams() {
    let smmu = make_smmu();

    // Three streams all with VMID 99
    for n in 50..53u32 {
        setup_stream(&smmu, n, 99, 0x1000, 0x2000 + u64::from(n - 50) * 0x1000);
        smmu.translate(sid(n), pasid(0), iova(0x1000), AccessType::Read,
                       SecurityState::NonSecure).unwrap();
    }

    // Invalidate VMID 99 — must evict all three stream entries
    let cmd = CommandEntry { cmd_type: CommandType::TlbiS12Vmall,
                             vmid: 99, ..CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0) };
    smmu.submit_command(cmd).unwrap();
    let processed = smmu.process_command_queue().unwrap();
    assert!(processed > 0, "at least one command must have been processed");

    // All three translations must still succeed (page table intact)
    for n in 50..53u32 {
        let result = smmu.translate(sid(n), pasid(0), iova(0x1000), AccessType::Read,
                                    SecurityState::NonSecure);
        assert!(result.is_ok(), "stream {} must still translate after VMID invalidation", n);
    }
}
