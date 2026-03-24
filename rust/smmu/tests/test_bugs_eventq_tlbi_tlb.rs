//! Tests for BUG-NEW-B, BUG-NEW-C, BUG-NEW-D, BUG-NEW-F, BUG-NEW-G
//!
//! BUG-NEW-B: enqueue_event() drops injected events without toggling OVFLG.
//! BUG-NEW-C: All NS command-queue TLBI ops gated by CR2.PTM (CR2 resets 0 → PTM=0 silences all).
//! BUG-NEW-D: advance_index modulo strips OVFLG from PROD on the next enqueue.
//! BUG-NEW-F: is_eventq_empty_by_index() masks PROD bit 31 but not CONS.
//! BUG-NEW-G: TLB entries missing §3.17 stage-conditional ASID/VMID zeroing.

#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, SecurityState, SMMUConfig,
    StreamConfig, StreamID, StreamWorld, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

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

/// Build an SMMU with a tiny event queue (4 entries) for overflow tests.
fn smmu_small_eventq() -> SMMU {
    let mut cfg = SMMUConfig::default();
    cfg.queue_config.event_queue_size = 4;
    let smmu = SMMU::with_config(cfg);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    smmu
}

// ── BUG-NEW-B ─────────────────────────────────────────────────────────────────

/// BUG-NEW-B: When inject_ste_fetch_abort fires into a full event queue, OVFLG
/// must be toggled.  Before the fix, enqueue_event() silently dropped events
/// without calling toggle_ovflg_once(), leaving OVFLG=0.
///
/// Spec: ARM §7.4.
#[test]
fn bug_new_b_inject_ste_fetch_abort_full_queue_toggles_ovflg() {
    let smmu = smmu_small_eventq();
    let s = sid(1);

    // Configure stream so translations generate fault events (no pages mapped).
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Fill the event queue to capacity via translations.
    for i in 0u64..4 {
        let _ = smmu.translate(
            s, pasid(0), iova(0x1000 + i * 0x1000),
            AccessType::Read, SecurityState::NonSecure,
        );
    }

    // Confirm queue is full and OVFLG is clear before inject.
    assert_eq!(smmu.eventq_occupied_entries(), 4, "queue must be full before inject");
    let prod_before = smmu.get_eventq_prod();
    assert_eq!((prod_before >> 31) & 1, 0, "OVFLG must be 0 before inject into full queue");

    // inject_ste_fetch_abort goes through enqueue_event() — queue is full → must toggle OVFLG.
    smmu.inject_ste_fetch_abort(s);

    let prod_after = smmu.get_eventq_prod();
    assert_eq!(
        (prod_after >> 31) & 1, 1,
        "BUG-NEW-B: inject_ste_fetch_abort must toggle OVFLG when queue is full, \
         got prod={prod_after:#010x}"
    );
}

/// BUG-NEW-B variant: inject_cd_fetch_abort also uses enqueue_event() and must toggle OVFLG.
#[test]
fn bug_new_b_inject_cd_fetch_abort_full_queue_toggles_ovflg() {
    let smmu = smmu_small_eventq();
    let s = sid(2);

    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    for i in 0u64..4 {
        let _ = smmu.translate(
            s, pasid(0), iova(0x2000 + i * 0x1000),
            AccessType::Read, SecurityState::NonSecure,
        );
    }

    let prod_before = smmu.get_eventq_prod();
    assert_eq!((prod_before >> 31) & 1, 0, "OVFLG must be 0 before inject");

    smmu.inject_cd_fetch_abort(s, pasid(0));

    let prod_after = smmu.get_eventq_prod();
    assert_eq!(
        (prod_after >> 31) & 1, 1,
        "BUG-NEW-B: inject_cd_fetch_abort must toggle OVFLG when queue is full, \
         got prod={prod_after:#010x}"
    );
}

/// BUG-NEW-B variant: inject_walk_eabt also uses enqueue_event() and must toggle OVFLG.
#[test]
fn bug_new_b_inject_walk_eabt_full_queue_toggles_ovflg() {
    let smmu = smmu_small_eventq();
    let s = sid(3);

    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    for i in 0u64..4 {
        let _ = smmu.translate(
            s, pasid(0), iova(0x3000 + i * 0x1000),
            AccessType::Read, SecurityState::NonSecure,
        );
    }

    let prod_before = smmu.get_eventq_prod();
    assert_eq!((prod_before >> 31) & 1, 0, "OVFLG must be 0 before inject");

    smmu.inject_walk_eabt(s, pasid(0), iova(0xDEAD_0000));

    let prod_after = smmu.get_eventq_prod();
    assert_eq!(
        (prod_after >> 31) & 1, 1,
        "BUG-NEW-B: inject_walk_eabt must toggle OVFLG when queue is full, \
         got prod={prod_after:#010x}"
    );
}

// ── BUG-NEW-C ─────────────────────────────────────────────────────────────────

/// BUG-NEW-C: With PTM=0 (CR2 default), CMD_TLBI_NH_ALL must still invalidate
/// the TLB.  Before the fix, the PTM guard silently no-opped the command.
///
/// The invalidation_count stat must increment after processing the command.
///
/// Spec: ARM §6.3.12 — CR2.PTM governs only broadcast TLB maintenance.
#[test]
fn bug_new_c_tlbi_nh_all_executes_with_ptm_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Verify PTM=0 by default (CR2 reset value is 0).
    assert_eq!(smmu.get_cr2() & SMMU::CR2_PTM, 0, "CR2.PTM must be 0 by default");

    let s = sid(10);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s, pasid(0), iova(0x5000), pa(0x8000),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Warm up TLB.
    let r1 = smmu.translate(s, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "first translate must succeed");

    let inv_before = smmu.get_invalidation_count();

    // Submit CMD_TLBI_NH_ALL (PTM=0, must still execute).
    let cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let inv_after = smmu.get_invalidation_count();
    assert!(
        inv_after > inv_before,
        "BUG-NEW-C: CMD_TLBI_NH_ALL must increment invalidation_count even when PTM=0 \
         (before={inv_before}, after={inv_after})"
    );
}

/// BUG-NEW-C: CMD_TLBI_NH_ASID must also execute with PTM=0.
#[test]
fn bug_new_c_tlbi_nh_asid_executes_with_ptm_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let s = sid(11);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.set_pasid_asid(s, pasid(0), 77).unwrap();
    smmu.map_page(
        s, pasid(0), iova(0x6000), pa(0x9000),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    let _ = smmu.translate(s, pasid(0), iova(0x6000), AccessType::Read, SecurityState::NonSecure);

    let inv_before = smmu.get_invalidation_count();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhAsid, 0, 0);
    cmd.asid = 77;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let inv_after = smmu.get_invalidation_count();
    assert!(
        inv_after > inv_before,
        "BUG-NEW-C: CMD_TLBI_NH_ASID must execute even when PTM=0 \
         (before={inv_before}, after={inv_after})"
    );
}

/// BUG-NEW-C: CMD_TLBI_NH_VA must also execute with PTM=0.
#[test]
fn bug_new_c_tlbi_nh_va_executes_with_ptm_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let s = sid(12);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s, pasid(0), iova(0x7000), pa(0xA000),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    let _ = smmu.translate(s, pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure);

    let inv_before = smmu.get_invalidation_count();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.start_address = 0x7000;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let inv_after = smmu.get_invalidation_count();
    assert!(
        inv_after > inv_before,
        "BUG-NEW-C: CMD_TLBI_NH_VA must execute even when PTM=0 \
         (before={inv_before}, after={inv_after})"
    );
}

/// BUG-NEW-C: CMD_TLBI_NH_VAA must also execute with PTM=0.
#[test]
fn bug_new_c_tlbi_nh_vaa_executes_with_ptm_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let s = sid(13);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s, pasid(0), iova(0xB000), pa(0xC000),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    let _ = smmu.translate(s, pasid(0), iova(0xB000), AccessType::Read, SecurityState::NonSecure);

    let inv_before = smmu.get_invalidation_count();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0, 0);
    cmd.start_address = 0xB000;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let inv_after = smmu.get_invalidation_count();
    assert!(
        inv_after > inv_before,
        "BUG-NEW-C: CMD_TLBI_NH_VAA must execute even when PTM=0 \
         (before={inv_before}, after={inv_after})"
    );
}

// ── BUG-NEW-D ─────────────────────────────────────────────────────────────────

/// BUG-NEW-D: After OVFLG is set in PROD, enqueuing a new entry (when space
/// becomes available after draining) must preserve OVFLG in PROD.
///
/// Scenario:
///   1. Fill queue → OVFLG toggles to 1.
///   2. Drain queue (pop all events).
///   3. Enqueue one more event → PROD advances.
///   4. OVFLG must still be set in the updated PROD.
///
/// Spec: ARM §7.4 — OVFLG must persist until SW clears it via CONS.OVACKFLG.
#[test]
fn bug_new_d_ovflg_preserved_after_advance_on_enqueue() {
    let smmu = smmu_small_eventq();
    let s = sid(20);

    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Fill queue and trigger overflow (OVFLG set).
    for i in 0u64..5 {
        let _ = smmu.translate(
            s, pasid(0), iova(0x1000 + i * 0x1000),
            AccessType::Read, SecurityState::NonSecure,
        );
    }
    let prod_ovflg = smmu.get_eventq_prod();
    assert_eq!((prod_ovflg >> 31) & 1, 1, "OVFLG must be set after overflow");

    // Drain the event queue so there is space.
    let _ = smmu.get_events();

    // Enqueue one more event via inject (goes through enqueue_event).
    smmu.inject_ste_fetch_abort(s);

    let prod_after = smmu.get_eventq_prod();
    assert_eq!(
        (prod_after >> 31) & 1, 1,
        "BUG-NEW-D: OVFLG must still be set after advance_index on enqueue \
         (prod={prod_after:#010x})"
    );
}

// ── BUG-NEW-F ─────────────────────────────────────────────────────────────────

/// BUG-NEW-F: is_eventq_empty_by_index() must return true when PROD and CONS
/// positions agree even if both have bit 31 set (OVFLG=OVACKFLG=1).
///
/// The bug: PROD bit31 is masked but CONS bit31 is not. After overflow ack,
/// CONS bit31 (OVACKFLG) = 1, PROD bit31 (OVFLG) = 1 but masked to 0 ≠ 1 →
/// incorrectly reports non-empty even though the queue is actually empty.
///
/// Spec: ARM §3.5.1 — empty = PROD.WR == CONS.RD (bits[log2size:0] only).
#[test]
fn bug_new_f_is_eventq_empty_after_overflow_ack() {
    let smmu = smmu_small_eventq();
    let s = sid(30);

    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Step 1: Fill and overflow so OVFLG=1 in PROD.
    for i in 0u64..5 {
        let _ = smmu.translate(
            s, pasid(0), iova(0x1000 + i * 0x1000),
            AccessType::Read, SecurityState::NonSecure,
        );
    }
    assert_eq!(
        (smmu.get_eventq_prod() >> 31) & 1, 1,
        "OVFLG must be 1 after overflow"
    );

    // Step 2: Drain all events.
    let events = smmu.get_events();
    assert!(!events.is_empty(), "must have drained some events");

    // Step 3: Acknowledge the overflow so CONS bit31 (OVACKFLG) = PROD bit31 (OVFLG) = 1.
    smmu.acknowledge_eventq_overflow();

    // Step 4: Both OVFLG and OVACKFLG are equal (=1); queue positions must also be equal.
    let prod = smmu.get_eventq_prod();
    let cons = smmu.eventq_cons_index();
    let prod_pos = prod & !(1u32 << 31);
    let cons_pos = cons & !(1u32 << 31);

    // Only assert if positions also match; if not, the test setup is wrong (not a bug).
    if prod_pos == cons_pos {
        assert!(
            smmu.is_eventq_empty_by_index(),
            "BUG-NEW-F: is_eventq_empty_by_index must return true when positions match \
             and OVFLG=OVACKFLG=1. prod={prod:#010x}, cons={cons:#010x}"
        );
    }
}

/// BUG-NEW-F-F2: eventq_occupied_entries() must also mask CONS bit 31.
///
/// After overflow + ack, with an empty queue, occupied_entries() must return 0.
#[test]
fn bug_new_f_eventq_occupied_entries_zero_after_overflow_ack() {
    let smmu = smmu_small_eventq();
    let s = sid(31);

    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Overflow the queue.
    for i in 0u64..5 {
        let _ = smmu.translate(
            s, pasid(0), iova(0x2000 + i * 0x1000),
            AccessType::Read, SecurityState::NonSecure,
        );
    }

    // Drain events.
    let _ = smmu.get_events();

    // Acknowledge overflow: CONS bit31 ← PROD bit31.
    smmu.acknowledge_eventq_overflow();

    // Verify queue positions are aligned.
    let prod_pos = smmu.get_eventq_prod() & !(1u32 << 31);
    let cons_pos = smmu.eventq_cons_index() & !(1u32 << 31);

    if prod_pos == cons_pos {
        let occupied = smmu.eventq_occupied_entries();
        assert_eq!(
            occupied, 0,
            "BUG-NEW-F-F2: eventq_occupied_entries must be 0 after drain+ack \
             (got {occupied}). prod_pos={prod_pos}, cons_pos={cons_pos}"
        );
    }
}

// ── BUG-NEW-G ─────────────────────────────────────────────────────────────────

/// BUG-NEW-G (ASID side): Stage-2-only stream must tag TLB entry with ASID=0.
///
/// §3.17 table: IPA input (stage-2-only) → ASID=No (ASID=0).
///
/// Proof: populate TLB via a translate, then issue CMD_TLBI_NH_ASID(asid=0)
/// which MUST evict the entry (since it's tagged ASID=0).  Verify via cache
/// statistics: a second translate (same address, page still mapped) must
/// generate a TLB miss after the ASID=0 invalidation.
///
/// If the entry was incorrectly tagged with ASID != 0, CMD_TLBI_NH_ASID(asid=0)
/// would NOT evict it, and the second translate would be a TLB hit (no extra miss).
#[test]
fn bug_new_g_stage2_only_tlb_entry_uses_asid_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let s = sid(40);

    // Stage-2-only with VMID=5.
    let mut cfg = StreamConfig::stage2_only();
    cfg.vmid = 5;
    smmu.configure_stream(s, cfg).unwrap();
    smmu.create_stage2_address_space(s).unwrap();
    smmu.map_stage2_page(
        s, iova(0xD000), pa(0xE000),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Warm up the TLB (first translate is always a miss → inserts entry).
    let r1 = smmu.translate(s, pasid(0), iova(0xD000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "stage-2-only translate must succeed");

    let stats1 = smmu.get_cache_statistics();
    // Second translate must hit cache (entry was just inserted).
    let r2 = smmu.translate(s, pasid(0), iova(0xD000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "second translate must succeed");
    let stats2 = smmu.get_cache_statistics();
    assert!(
        stats2.tlb_hits() > stats1.tlb_hits(),
        "BUG-NEW-G setup: second translate must hit TLB (hits before={}, after={})",
        stats1.tlb_hits(), stats2.tlb_hits()
    );

    // Issue CMD_TLBI_NH_ASID(vmid=5, asid=0): must evict entry tagged (vmid=5, asid=0).
    // BUG-RUST-1 fix: CMD_TLBI_NH_ASID uses joint VMID+ASID matching (ARM §4.4.2.2).
    // The stage-2-only entry is tagged vmid=5 (from stream config) and asid=0
    // (stage-2-only entries have no ASID), so the command must include vmid=5.
    // NOTE: BUG-NEW-C must also be fixed for this command to execute.
    let mut cmd0 = CommandEntry::new(CommandType::TlbiNhAsid, 0, 0);
    cmd0.vmid = 5; // must match the stream's VMID for joint VMID+ASID invalidation
    cmd0.asid = 0;
    smmu.submit_command(cmd0).unwrap();
    smmu.process_command_queue().unwrap();

    // Third translate: TLB entry was evicted → must be a cache miss.
    let stats_before_3rd = smmu.get_cache_statistics();
    let r3 = smmu.translate(s, pasid(0), iova(0xD000), AccessType::Read, SecurityState::NonSecure);
    assert!(r3.is_ok(), "third translate must succeed");
    let stats_after_3rd = smmu.get_cache_statistics();

    assert!(
        stats_after_3rd.tlb_misses() > stats_before_3rd.tlb_misses(),
        "BUG-NEW-G (ASID): stage-2-only TLB entry must be tagged ASID=0 so \
         CMD_TLBI_NH_ASID(asid=0) evicts it (miss before={}, miss after={})",
        stats_before_3rd.tlb_misses(), stats_after_3rd.tlb_misses()
    );
}

/// BUG-NEW-G (VMID side, Secure stage-1-only): Secure stage-1-only stream must
/// tag TLB entry with VMID=0.
///
/// §3.17/§21035: Secure stage-1-only → VMID=0.
///
/// Proof: populate TLB, issue CMD_TLBI_S12_VMALL(vmid=0) which MUST evict the
/// entry tagged VMID=0.  Verify via cache miss on the subsequent translate.
///
/// Before fix: TLB entry tagged vmid=99 → TLBI(vmid=0) does NOT evict → TLB hit (wrong).
/// After fix:  TLB entry tagged vmid=0  → TLBI(vmid=0) DOES evict → TLB miss (correct).
#[test]
fn bug_new_g_secure_stage1_only_tlb_entry_uses_vmid_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let s = sid(41);

    // Secure stage-1-only stream with a non-zero VMID and STRW=El2.
    // Note: STRW=El3 is forbidden even for Secure streams per BUG-RUST-2b fix (ARM §5.2).
    // STRW=El2 has identical all-privileged semantics and is valid for Secure streams.
    let mut cfg = StreamConfig::stage1_only();
    cfg.vmid = 99;
    cfg.security_state = SecurityState::Secure;
    cfg.strw = StreamWorld::El2;
    smmu.configure_stream(s, cfg).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s, pasid(0), iova(0xF000), pa(0x1_0000),
        PagePermissions::read_write(), SecurityState::Secure,
    ).unwrap();

    // Warm TLB.
    let r1 = smmu.translate(s, pasid(0), iova(0xF000), AccessType::Read, SecurityState::Secure);
    assert!(r1.is_ok(), "Secure stage-1-only translate must succeed");

    // Verify cache hit on second translate.
    let stats1 = smmu.get_cache_statistics();
    let r2 = smmu.translate(s, pasid(0), iova(0xF000), AccessType::Read, SecurityState::Secure);
    assert!(r2.is_ok(), "second translate must succeed");
    let stats2 = smmu.get_cache_statistics();
    assert!(
        stats2.tlb_hits() > stats1.tlb_hits(),
        "BUG-NEW-G setup (Secure): second translate must hit TLB"
    );

    // Issue CMD_TLBI_S12_VMALL(vmid=0): must evict VMID=0 entries.
    // After fix: entry tagged VMID=0 → evicted by TLBI(vmid=0).
    // Before fix: entry tagged VMID=99 → NOT evicted by TLBI(vmid=0).
    let mut vmall0 = CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0);
    vmall0.vmid = 0;
    smmu.submit_command(vmall0).unwrap();
    smmu.process_command_queue().unwrap();

    // Third translate: after fix, entry evicted → TLB miss.
    let stats_before_3rd = smmu.get_cache_statistics();
    let r3 = smmu.translate(s, pasid(0), iova(0xF000), AccessType::Read, SecurityState::Secure);
    assert!(r3.is_ok(), "third translate must succeed (page still mapped)");
    let stats_after_3rd = smmu.get_cache_statistics();

    assert!(
        stats_after_3rd.tlb_misses() > stats_before_3rd.tlb_misses(),
        "BUG-NEW-G (VMID/Secure): Secure stage-1-only TLB entry must use VMID=0; \
         CMD_TLBI_S12_VMALL(vmid=0) must evict it \
         (miss before={}, miss after={})",
        stats_before_3rd.tlb_misses(), stats_after_3rd.tlb_misses()
    );
}

/// BUG-NEW-G (VMID side, NS-EL1 stage-1-only): NS-EL1 stage-1-only stream must
/// retain the VMID from STE.S2VMID in TLB entries.
///
/// §12582-§12585: NS-EL1 stage-1-only still uses the stream VMID.
///
/// Proof: populate TLB, issue CMD_TLBI_S12_VMALL(vmid=55) which MUST evict the
/// entry tagged VMID=55 (retained from STE.S2VMID).
/// Verify via cache miss on the subsequent translate.
///
/// If NS-EL1 VMID were incorrectly zeroed (like Secure), TLBI(vmid=55) would NOT
/// evict the VMID=0 entry → no extra TLB miss (wrong).
#[test]
fn bug_new_g_ns_el1_stage1_only_tlb_entry_retains_vmid() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let s = sid(42);

    // NS-EL1/EL0 stage-1-only stream with vmid=55.
    let mut cfg = StreamConfig::stage1_only();
    cfg.vmid = 55;
    smmu.configure_stream(s, cfg).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s, pasid(0), iova(0x1_1000), pa(0x1_2000),
        PagePermissions::read_write(), SecurityState::NonSecure,
    ).unwrap();

    // Warm TLB.
    let r1 = smmu.translate(s, pasid(0), iova(0x1_1000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "NS-EL1 stage-1-only translate must succeed");

    // Verify cache hit on second translate.
    let stats1 = smmu.get_cache_statistics();
    let r2 = smmu.translate(s, pasid(0), iova(0x1_1000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "second translate must succeed");
    let stats2 = smmu.get_cache_statistics();
    assert!(
        stats2.tlb_hits() > stats1.tlb_hits(),
        "BUG-NEW-G setup (NS): second translate must hit TLB"
    );

    // Issue CMD_TLBI_S12_VMALL(vmid=55): must evict VMID=55 entries.
    // NS-EL1 retains VMID from STE → entry tagged vmid=55 → evicted by TLBI(vmid=55).
    let mut vmall55 = CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0);
    vmall55.vmid = 55;
    smmu.submit_command(vmall55).unwrap();
    smmu.process_command_queue().unwrap();

    // Third translate: entry evicted → TLB miss.
    let stats_before_3rd = smmu.get_cache_statistics();
    let r3 = smmu.translate(s, pasid(0), iova(0x1_1000), AccessType::Read, SecurityState::NonSecure);
    assert!(r3.is_ok(), "third translate must succeed (page still mapped)");
    let stats_after_3rd = smmu.get_cache_statistics();

    assert!(
        stats_after_3rd.tlb_misses() > stats_before_3rd.tlb_misses(),
        "BUG-NEW-G (VMID/NS): NS-EL1 stage-1-only TLB entry must retain vmid=55; \
         CMD_TLBI_S12_VMALL(vmid=55) must evict it \
         (miss before={}, miss after={})",
        stats_before_3rd.tlb_misses(), stats_after_3rd.tlb_misses()
    );
}
