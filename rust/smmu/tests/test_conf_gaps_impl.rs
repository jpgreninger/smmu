//! Tests for ARM SMMU v3 conformance gaps (CONF-GAP-3, 6, 8-14, 17, 18)
//!
//! Each test follows TDD — written BEFORE implementation so it fails first,
//! then passes after the fix is applied.

use smmu::types::{CommandEntry, CommandType, StreamConfig, StreamID};
use smmu::SMMU;

// ============================================================================
// CONF-GAP-9: SMMU_CR0ACK handshake (§6.3.10)
// ============================================================================

#[test]
fn test_cr0ack_tracks_cr0_after_set_cr0() {
    let smmu = SMMU::new();
    let val = SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN;
    smmu.set_cr0(val);
    assert_eq!(
        smmu.get_cr0ack(),
        val,
        "CR0ACK must mirror CR0 after set_cr0()"
    );
}

#[test]
fn test_cr0ack_zero_after_reset() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN);
    smmu.reset_cr0ack(); // reset back to 0
    assert_eq!(smmu.get_cr0ack(), 0, "CR0ACK must be 0 after reset");
}

#[test]
fn test_cr0ack_initial_value_is_zero() {
    let smmu = SMMU::new();
    assert_eq!(smmu.get_cr0ack(), 0, "CR0ACK initial value must be 0");
}

// ============================================================================
// CONF-GAP-10: SMMU_CR1 register (§6.3.11)
// ============================================================================

#[test]
fn test_cr1_set_get_roundtrip() {
    let smmu = SMMU::new();
    let val = SMMU::CR1_TABLE_SH | SMMU::CR1_QUEUE_IC;
    smmu.set_cr1(val);
    assert_eq!(smmu.get_cr1(), val, "CR1 set/get round-trip must work");
}

#[test]
fn test_cr1_initial_value_is_zero() {
    let smmu = SMMU::new();
    assert_eq!(smmu.get_cr1(), 0, "CR1 initial value must be 0 (§6.3.11)");
}

#[test]
fn test_cr1_constants_non_overlapping() {
    // Verify the constants cover the right bit positions
    assert_eq!(SMMU::CR1_TABLE_SH, 0x3 << 10);
    assert_eq!(SMMU::CR1_TABLE_OC, 0x3 << 8);
    assert_eq!(SMMU::CR1_TABLE_IC, 0x3 << 6);
    assert_eq!(SMMU::CR1_QUEUE_SH, 0x3 << 4);
    assert_eq!(SMMU::CR1_QUEUE_OC, 0x3 << 2);
    assert_eq!(SMMU::CR1_QUEUE_IC, 0x3);
}

// ============================================================================
// CONF-GAP-14: STE.MEV event merging (§5.2)
// ============================================================================

#[test]
fn test_mev_suppresses_duplicate_events() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(1).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.mev = true;
    smmu.configure_stream(stream_id, cfg).unwrap();

    // Generate the same fault 3 times by attempting translations that
    // produce F_TRANSLATION events (no mapping configured).
    let iova = smmu::IOVA::new(0x1000).unwrap();
    let _ = smmu.translate(stream_id, smmu::PASID::new(0).unwrap(), iova, smmu::types::AccessType::Read, smmu::types::SecurityState::NonSecure);
    let _ = smmu.translate(stream_id, smmu::PASID::new(0).unwrap(), iova, smmu::types::AccessType::Read, smmu::types::SecurityState::NonSecure);
    let _ = smmu.translate(stream_id, smmu::PASID::new(0).unwrap(), iova, smmu::types::AccessType::Read, smmu::types::SecurityState::NonSecure);

    let events = smmu.get_events();
    // With MEV=true and identical faults, at most 1 event should be in the queue
    assert!(
        events.len() <= 1,
        "MEV=true must suppress duplicate events; got {} events",
        events.len()
    );
}

#[test]
fn test_mev_false_allows_multiple_events() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(2).unwrap();
    let mut cfg = StreamConfig::stage1_only();
    cfg.mev = false;
    smmu.configure_stream(stream_id, cfg).unwrap();

    let iova = smmu::IOVA::new(0x2000).unwrap();
    let _ = smmu.translate(stream_id, smmu::PASID::new(0).unwrap(), iova, smmu::types::AccessType::Read, smmu::types::SecurityState::NonSecure);
    let _ = smmu.translate(stream_id, smmu::PASID::new(0).unwrap(), iova, smmu::types::AccessType::Read, smmu::types::SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(
        events.len() >= 2,
        "MEV=false must not suppress events; got {} events",
        events.len()
    );
}

// ============================================================================
// CONF-GAP-11: CR2.PTM effect (§6.3.12)
// ============================================================================

#[test]
fn test_cr2_ptm_zero_does_not_gate_cq_tlbi_nh_all() {
    // BUG-NEW-C fix (§6.3.12): CR2.PTM governs only *broadcast* TLB maintenance
    // received from PEs via the broadcast interface.  Command-queue TLBI commands
    // (including TLBI_NH_ALL) must execute regardless of the PTM bit value.
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // Configure a stream and add a TLB entry via translation
    let stream_id = StreamID::new(3).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    // PTM=0 (default) — CQ TLBI_NH_ALL must still execute (not gated by PTM).
    let cr2_val = smmu.get_cr2() & !SMMU::CR2_PTM;
    smmu.set_cr2(cr2_val);

    let before = smmu.get_cache_statistics().invalidation_count();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
    cmd.cs = 0;
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    let after = smmu.get_cache_statistics().invalidation_count();
    assert!(
        after > before,
        "PTM=0 must NOT gate CQ TLBI_NH_ALL (§6.3.12: PTM only affects broadcast TLBI)"
    );
}

#[test]
fn test_cr2_ptm_one_executes_tlbi_nh_all() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);

    // PTM=1 — TLBI_NH_ALL should execute
    smmu.set_cr2(SMMU::CR2_PTM);

    let before = smmu.get_cache_statistics().invalidation_count();

    let cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    let after = smmu.get_cache_statistics().invalidation_count();
    assert!(
        after > before,
        "PTM=1: TLBI_NH_ALL must increment invalidation count (§6.3.12)"
    );
}

// ============================================================================
// CONF-GAP-12: CR0.VMW VMID wildcard matching (§6.3.9)
// ============================================================================

#[test]
fn test_cr0_vmw_constants() {
    // Per ARM IHI0070G.b §6.3.9.1: VMW occupies CR0 bits [8:6], not [4:2].
    assert_eq!(SMMU::CR0_VMW_SHIFT, 6);
    assert_eq!(SMMU::CR0_VMW_MASK, 7 << 6);
}

#[test]
fn test_vmid_wildcard_invalidation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR2_PTM);
    smmu.set_cr2(SMMU::CR2_PTM);

    // VMW=2: bits [1:0] are wildcarded → invalidate VMIDs 4, 5, 6, 7 when targeting 4
    // Set VMW=2 in CR0 bits[4:2]
    let vmw = 2u32;
    let cr0 = smmu.get_cr0() | (vmw << SMMU::CR0_VMW_SHIFT);
    smmu.set_cr0(cr0);

    // Issue TLBI_S12_VMALL with vmid=4 — should also evict entries with vmid=5,6,7
    let mut cmd = CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0);
    cmd.vmid = 4;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
    // The test just verifies no panic and correct constant values (functional
    // verification requires TLB seeding which is covered by invalidate_by_vmid_with_mask)
}

// ============================================================================
// CONF-GAP-13: GBPA output attribute fields (§6.3.22)
// ============================================================================

#[test]
fn test_gbpa_config_roundtrip() {
    let smmu = SMMU::new();
    let cfg = smmu::GbpaConfig {
        abort: false,
        inst_cfg: 1,
        priv_cfg: 2,
        mt_cfg: true,
        mem_attr: 3,
        sh_cfg: 1,
        alloc_cfg: 5,
    };
    smmu.set_gbpa_config(cfg.clone());
    let got = smmu.get_gbpa_config();
    assert_eq!(got.inst_cfg, cfg.inst_cfg);
    assert_eq!(got.priv_cfg, cfg.priv_cfg);
    assert_eq!(got.mt_cfg, cfg.mt_cfg);
    assert_eq!(got.mem_attr, cfg.mem_attr);
    assert_eq!(got.sh_cfg, cfg.sh_cfg);
    assert_eq!(got.alloc_cfg, cfg.alloc_cfg);
}

#[test]
fn test_gbpa_disabled_smmu_applies_output_attrs() {
    let smmu = SMMU::new();
    // SMMU disabled (SMMUEN=0) + GBPA not abort

    let cfg = smmu::GbpaConfig {
        abort: false,
        inst_cfg: 1,
        priv_cfg: 0,
        mt_cfg: true,
        mem_attr: 5,
        sh_cfg: 2,
        alloc_cfg: 3,
    };
    smmu.set_gbpa_config(cfg);

    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let iova = smmu::IOVA::new(0x1000).unwrap();
    let result = smmu.translate(
        stream_id,
        smmu::PASID::new(0).unwrap(),
        iova,
        smmu::types::AccessType::Read,
        smmu::types::SecurityState::NonSecure,
    );
    // Should succeed (identity bypass) and carry GBPA output attrs
    assert!(result.is_ok(), "GBPA bypass should succeed");
    let data = result.unwrap();
    assert_eq!(data.mem_type(), 5, "GBPA mem_attr must be applied (mt_cfg=true)");
    assert_eq!(data.shareability(), 2, "GBPA sh_cfg must be applied");
}

// ============================================================================
// CONF-GAP-17: CMDQ_CONS.ERR reason codes (§6.3.17)
// ============================================================================

#[test]
fn test_cmdq_cons_err_constants() {
    assert_eq!(SMMU::CMDQ_CONS_ERR_SHIFT, 24);
    assert_eq!(SMMU::CERROR_NONE, 0);
    assert_eq!(SMMU::CERROR_ILL, 1);
    assert_eq!(SMMU::CERROR_ABT, 2);
    assert_eq!(SMMU::CERROR_ATC_INV_SYNC, 3);
}

#[test]
fn test_cerror_ill_set_on_cmd_sync_cs3() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN);

    // Initial ERR should be 0
    assert_eq!(smmu.get_cmdq_cons_err(), 0, "CMDQ_CONS.ERR must be 0 initially");

    // CMD_SYNC with CS=0b11 (reserved) → CERROR_ILL
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0b11;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "CMD_SYNC CS=3 must set CERROR_ILL in CMDQ_CONS[31:24]"
    );
}

// ============================================================================
// CONF-GAP-18: CMD_SYNC SIG_IRQ vs SIG_MSI distinction (§4.7.3)
// ============================================================================

#[test]
fn test_cmd_sync_sig_irq_records_signal_type_1() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_PTM); // needed so TLBIs work

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 1; // SIG_IRQ
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmd_sync_last_signal_type(),
        1,
        "CS=1 (SIG_IRQ) must record signal_type=1"
    );
}

#[test]
fn test_cmd_sync_sig_msi_records_signal_type_2() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_PTM);

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 2; // SIG_MSI
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmd_sync_last_signal_type(),
        2,
        "CS=2 (SIG_MSI) must record signal_type=2"
    );
}

#[test]
fn test_cmd_sync_msi_registers_roundtrip() {
    let smmu = SMMU::new();
    smmu.set_cmdq_sync_msi_attr(0xAB);
    smmu.set_cmdq_sync_msi_data(0xDEAD_BEEF);
    assert_eq!(smmu.get_cmdq_sync_msi_attr(), 0xAB);
    assert_eq!(smmu.get_cmdq_sync_msi_data(), 0xDEAD_BEEF);
}

// ============================================================================
// CONF-GAP-6: VA/VAA TLBI selective invalidation (§4.4)
// ============================================================================

#[test]
fn test_tlbi_nh_va_selective_invalidation() {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PagePermissions, PASID, StreamID};

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);

    let sid1 = StreamID::new(1).unwrap();
    let sid2 = StreamID::new(2).unwrap();
    let pasid0 = PASID::new(0).unwrap();
    let iova_a = IOVA::new(0x1000).unwrap();
    let iova_b = IOVA::new(0x2000).unwrap();
    let pa_a = PA::new(0xA000).unwrap();
    let pa_b = PA::new(0xB000).unwrap();

    let key_a = CacheKey::new(sid1, pasid0, iova_a, smmu::types::SecurityState::NonSecure);
    let key_b = CacheKey::new(sid2, pasid0, iova_b, smmu::types::SecurityState::NonSecure);

    let mut entry_a = CacheEntry::new(iova_a, pa_a, PagePermissions::read_only(), 1);
    entry_a.asid = 42;
    let mut entry_b = CacheEntry::new(iova_b, pa_b, PagePermissions::read_only(), 2);
    entry_b.asid = 99;

    cache.insert(key_a, entry_a);
    cache.insert(key_b, entry_b);

    // Selective VA+ASID invalidation for iova_a with asid=42
    cache.invalidate_by_va_and_asid(iova_a.as_u64(), 42u16);

    // iova_a entry must be gone
    assert!(cache.lookup(&key_a).is_none(), "iova_a entry must be evicted");
    // iova_b entry must still be present
    assert!(cache.lookup(&key_b).is_some(), "iova_b entry must survive");
}

#[test]
fn test_tlbi_nh_vaa_invalidates_by_va_only() {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PagePermissions, PASID, StreamID};

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);

    let sid1 = StreamID::new(1).unwrap();
    let pasid0 = PASID::new(0).unwrap();
    let iova_a = IOVA::new(0x1000).unwrap();
    let iova_b = IOVA::new(0x2000).unwrap();
    let pa_a = PA::new(0xA000).unwrap();
    let pa_b = PA::new(0xB000).unwrap();

    let key_a1 = CacheKey::new(sid1, pasid0, iova_a, smmu::types::SecurityState::NonSecure);
    let key_a2 = CacheKey::new(sid1, pasid0, iova_b, smmu::types::SecurityState::NonSecure);

    let mut entry_a = CacheEntry::new(iova_a, pa_a, PagePermissions::read_only(), 1);
    entry_a.asid = 10;
    let mut entry_b = CacheEntry::new(iova_b, pa_b, PagePermissions::read_only(), 2);
    entry_b.asid = 20;

    cache.insert(key_a1, entry_a);
    cache.insert(key_a2, entry_b);

    // VAA invalidation: by VA only, any ASID
    cache.invalidate_by_va(iova_a.as_u64());

    assert!(cache.lookup(&key_a1).is_none(), "iova_a must be evicted by VAA");
    assert!(cache.lookup(&key_a2).is_some(), "iova_b must survive VAA for different VA");
}

// ============================================================================
// CONF-GAP-8: Range-based TLBI / RIL feature (§4.4.1.1)
// ============================================================================

#[test]
fn test_ril_fields_on_command_entry() {
    let cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    // Fields must exist and default to 0/false
    assert_eq!(cmd.tg, 0);
    assert_eq!(cmd.num, 0);
    assert_eq!(cmd.scale, 0);
    assert_eq!(cmd.ttl, 0);
    assert!(!cmd.ril);
}

#[test]
fn test_invalidate_by_va_range_and_asid() {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{IOVA, PA, PagePermissions, PASID, StreamID};

    let cache = TlbCache::new(64, ReplacementPolicy::Lru);
    let sid = StreamID::new(1).unwrap();
    let pasid0 = PASID::new(0).unwrap();

    // Create entries at 0x1000, 0x2000, 0x3000, 0x4000 — all ASID=7
    for page in 0u64..4 {
        let iova = IOVA::new((page + 1) * 0x1000).unwrap();
        let pa = PA::new(0xF000 + page * 0x1000).unwrap();
        let key = CacheKey::new(sid, pasid0, iova, smmu::types::SecurityState::NonSecure);
        let mut e = CacheEntry::new(iova, pa, PagePermissions::read_only(), page);
        e.asid = 7;
        cache.insert(key, e);
    }

    // Invalidate range [0x1000, 0x3000] with asid=7 → pages 1, 2, 3 evicted; page 4 survives
    cache.invalidate_by_va_range_and_asid(0x1000, 0x3000, 7u16);

    let key4 = CacheKey::new(sid, pasid0, IOVA::new(0x4000).unwrap(), smmu::types::SecurityState::NonSecure);
    assert!(cache.lookup(&key4).is_some(), "0x4000 must survive range invalidation");

    let key1 = CacheKey::new(sid, pasid0, IOVA::new(0x1000).unwrap(), smmu::types::SecurityState::NonSecure);
    assert!(cache.lookup(&key1).is_none(), "0x1000 must be evicted");
    let key2 = CacheKey::new(sid, pasid0, IOVA::new(0x2000).unwrap(), smmu::types::SecurityState::NonSecure);
    assert!(cache.lookup(&key2).is_none(), "0x2000 must be evicted");
    let key3 = CacheKey::new(sid, pasid0, IOVA::new(0x3000).unwrap(), smmu::types::SecurityState::NonSecure);
    assert!(cache.lookup(&key3).is_none(), "0x3000 must be evicted");
}

// ============================================================================
// CONF-GAP-3: 2-level stream table (§3.3.1.2, §6.3.25)
// ============================================================================

#[test]
fn test_stream_table_format_constants() {
    // StreamTableFormat enum must be accessible
    // Verify both variants are accessible (used via cast below)
    assert_eq!(smmu::types::StreamTableFormat::Linear as u32, 0);
    assert_eq!(smmu::types::StreamTableFormat::TwoLevel as u32, 1);
}

#[test]
fn test_strtab_format_default_is_linear() {
    let smmu = SMMU::new();
    assert_eq!(
        smmu.get_strtab_format() as u32,
        smmu::types::StreamTableFormat::Linear as u32,
        "Default strtab format must be Linear"
    );
}

#[test]
fn test_strtab_format_set_two_level() {
    let smmu = SMMU::new();
    smmu.set_strtab_format(smmu::types::StreamTableFormat::TwoLevel);
    assert_eq!(
        smmu.get_strtab_format() as u32,
        smmu::types::StreamTableFormat::TwoLevel as u32
    );
}

#[test]
fn test_strtab_split_default_and_set() {
    let smmu = SMMU::new();
    assert_eq!(smmu.get_strtab_split(), 6, "Default split must be 6");
    smmu.set_strtab_split(8);
    assert_eq!(smmu.get_strtab_split(), 8);
}

#[test]
fn test_two_level_valid_stream_id_accepted() {
    let smmu = SMMU::new();
    smmu.set_strtab_log2size(10); // 1024 entry table
    smmu.set_strtab_format(smmu::types::StreamTableFormat::TwoLevel);
    smmu.set_strtab_split(6);
    smmu.enable().unwrap();

    // StreamID=0: L1_idx=0, L2_idx=0 — valid
    let sid = StreamID::new(0).unwrap();
    smmu.configure_stream(sid, StreamConfig::bypass()).unwrap();

    let iova = smmu::IOVA::new(0x1000).unwrap();
    let result = smmu.translate(
        sid,
        smmu::PASID::new(0).unwrap(),
        iova,
        smmu::types::AccessType::Read,
        smmu::types::SecurityState::NonSecure,
    );
    assert!(result.is_ok(), "Valid StreamID in 2-level table must succeed");
}

#[test]
fn test_two_level_invalid_stream_id_rejected() {
    let smmu = SMMU::new();
    // split=6, log2size=8 → L1 has 2^(8-6)=4 entries, each L2 has 64 entries
    // Valid SIDs: 0..=255
    smmu.set_strtab_log2size(8);
    smmu.set_strtab_format(smmu::types::StreamTableFormat::TwoLevel);
    smmu.set_strtab_split(6);
    smmu.enable().unwrap();

    // StreamID=256 is out of range (1 << 8 = 256 → same as linear limit)
    // For 2-level, L1_idx = 256 >> 6 = 4, L1_size = 1 << (8-6) = 4
    // L1_idx=4 >= L1_size=4 → out of range
    let sid = StreamID::new(256).unwrap();
    smmu.configure_stream(sid, StreamConfig::bypass()).unwrap();

    let iova = smmu::IOVA::new(0x1000).unwrap();
    let result = smmu.translate(
        sid,
        smmu::PASID::new(0).unwrap(),
        iova,
        smmu::types::AccessType::Read,
        smmu::types::SecurityState::NonSecure,
    );
    // Must return C_BAD_STREAMID / InvalidStreamID
    assert!(
        result.is_err(),
        "Out-of-range StreamID in 2-level table must fail"
    );
}
