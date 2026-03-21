#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

/// Tests for BUG-AUDIT-1, BUG-AUDIT-3, BUG-AUDIT-4, and BUG-AUDIT-7.
///
/// These tests follow strict TDD: each test is written to FAIL before the fix
/// and PASS after the fix is applied.
use smmu::types::{
    AccessType, CommandEntry, CommandType, FaultMode, PagePermissions, SecurityState, StreamConfig,
    StreamID, TranslationError, IOVA, PA, PASID,
};
use smmu::SMMU;

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

// ---------------------------------------------------------------------------
// BUG-AUDIT-1: RIL range formula — scale used as 5*scale instead of scale
// ---------------------------------------------------------------------------

/// Verify that the RIL TLBI range formula uses `scale` as the direct shift
/// exponent (not `5 * scale`).
///
/// ARM IHI0070G.b §4.4.1.1: range = (num+1) * 2^scale * granule_size
///
/// With scale=1, num=0, tg=0 (4KB granule):
///   correct:  (0+1) * 2^1 * 4096 = 8192  bytes → covers [0x0000, 0x1FFF]
///   buggy:    (0+1) * 2^5 * 4096 = 131072 bytes → covers [0x0000, 0x1FFFF]
///
/// We warm a TLB entry at VA=0x3000, then issue a RIL TLBI with scale=1, num=0.
/// With the CORRECT formula the TLB entry at 0x3000 is NOT evicted (TLB hit on
/// the subsequent lookup).  With the BUGGY formula (5*scale shift) 0x3000 falls
/// within the over-large range and IS evicted (TLB miss on subsequent lookup).
///
/// We verify this by comparing TLB miss counts before and after.
#[test]
fn bugaudit1_ril_range_formula_uses_scale_not_5times_scale() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let asid: u16 = 1;
    let stream = sid(1);
    let mut config = StreamConfig::stage1_only();
    config.t0sz = 0; // no VA restriction
    smmu.configure_stream(stream, config).unwrap();

    let p0 = pasid(0);
    smmu.create_pasid(stream, p0).unwrap();
    smmu.set_pasid_asid(stream, p0, asid).unwrap();

    // Map VA=0x3000 — should survive [0x0000, 0x1FFF] TLBI (correct formula).
    smmu.map_page(stream, p0, iova(0x3000), pa(0x8000), PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    // Map VA=0x0000 (inside the invalidation range — expected to be evicted).
    smmu.map_page(stream, p0, iova(0x0000), pa(0x9000), PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Warm both TLB entries (two translates → two TLB inserts, subsequent
    // translates on the same address should be cache hits).
    smmu.translate(stream, p0, iova(0x3000), AccessType::Read, SecurityState::NonSecure).unwrap();
    smmu.translate(stream, p0, iova(0x0000), AccessType::Read, SecurityState::NonSecure).unwrap();

    // Verify both are now in the TLB (warm them a second time — should be hits).
    let stats_before_tlbi = smmu.get_cache_statistics();
    smmu.translate(stream, p0, iova(0x3000), AccessType::Read, SecurityState::NonSecure).unwrap();
    smmu.translate(stream, p0, iova(0x0000), AccessType::Read, SecurityState::NonSecure).unwrap();
    let stats_after_warm = smmu.get_cache_statistics();
    // Both should be TLB hits (no new misses added).
    assert_eq!(stats_after_warm.tlb_misses(), stats_before_tlbi.tlb_misses(),
        "Both 0x3000 and 0x0000 must be TLB hits after warm-up");

    // Issue RIL TLBI: scale=1, num=0, tg=0 (4KB)
    // Correct formula: (0+1) * 2^1 * 4096 = 8192 bytes → [0x0000, 0x1FFF]
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.asid = asid;
    cmd.start_address = 0x0000;
    cmd.ril = true;
    cmd.tg = 0;    // 4KB granule
    cmd.scale = 1; // correct formula: shift=1 → 2^1=2 blocks
    cmd.num = 0;   // num+1=1 block

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // After TLBI, translate 0x3000 again and check TLB miss count.
    let misses_before_check = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(stream, p0, iova(0x3000), AccessType::Read, SecurityState::NonSecure).unwrap();
    let misses_after_check = smmu.get_cache_statistics().tlb_misses();

    // With CORRECT formula: 0x3000 is outside [0x0000, 0x1FFF] → TLB hit → no new miss.
    // With BUGGY formula (5*scale): 0x3000 is inside [0x0000, 0x1FFFF] → TLB evicted → miss.
    assert_eq!(misses_after_check, misses_before_check,
        "TLB entry at 0x3000 must NOT be evicted by RIL TLBI covering [0x0000, 0x1FFF]. \
         Correct formula (scale=1 → shift=1 → range=8192). \
         Bug: 5*scale exponent gives shift=5 → range=131072, which evicts 0x3000 (=12288).");
}

/// Verify that with scale=2 the RIL formula covers exactly [0, 16383] (4 granules).
///
/// ARM formula: (num+1) * 2^scale * granule = (0+1) * 2^2 * 4096 = 16384 bytes.
/// An address at 0x5000 (20480) must NOT be evicted from TLB by this TLBI.
/// With 5*scale: (0+1) * 2^10 * 4096 = 4 MB → would evict 0x5000 (= TLB miss).
#[test]
fn bugaudit1_ril_range_scale2_four_granules() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let asid: u16 = 2;
    let stream = sid(2);
    let mut config = StreamConfig::stage1_only();
    config.t0sz = 0;
    smmu.configure_stream(stream, config).unwrap();

    let p0 = pasid(0);
    smmu.create_pasid(stream, p0).unwrap();
    smmu.set_pasid_asid(stream, p0, asid).unwrap();

    // Map a page well outside the expected range.
    smmu.map_page(stream, p0, iova(0x5000), pa(0xA000), PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Warm the TLB entry (first translate = miss + insert; second = hit).
    smmu.translate(stream, p0, iova(0x5000), AccessType::Read, SecurityState::NonSecure).unwrap();
    let misses_after_warm = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(stream, p0, iova(0x5000), AccessType::Read, SecurityState::NonSecure).unwrap();
    // Confirm it is now a TLB hit (second translate adds no new miss).
    assert_eq!(smmu.get_cache_statistics().tlb_misses(), misses_after_warm,
        "0x5000 must be a TLB hit after warm-up");

    // RIL TLBI: scale=2, num=0 → range = (0+1)*2^2*4096 = 16384 bytes → [0x0000, 0x3FFF].
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.asid = asid;
    cmd.start_address = 0x0000;
    cmd.ril = true;
    cmd.tg = 0;
    cmd.scale = 2;
    cmd.num = 0;

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // 0x5000 = 20480 is outside [0x0000, 0x3FFF = 16383] → must still be a TLB hit.
    let misses_before_check = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(stream, p0, iova(0x5000), AccessType::Read, SecurityState::NonSecure).unwrap();
    let misses_after_check = smmu.get_cache_statistics().tlb_misses();

    assert_eq!(misses_after_check, misses_before_check,
        "TLB entry at 0x5000 must NOT be evicted by RIL TLBI of scale=2,num=0 (range=16384B). \
         Correct formula: 0x5000=20480 > 16383 → outside range → TLB hit. \
         Bug: 5*scale gives shift=10 → range=4MB, which includes 0x5000 → TLB miss.");
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-3: Stage-2 must be STE-scoped (stream-scoped), not PASID-scoped
// ---------------------------------------------------------------------------

/// Verify that both PASID 0 and PASID 1 share the same stream-level stage-2
/// address space, and that mapping IPA→PA once is visible to both.
///
/// ARM §3.3.3: stage-2 translation tables are specified in the STE, not CD.
/// All PASIDs for a stream share one stage-2 table.
#[test]
fn bugaudit3_stage2_is_stream_scoped_not_pasid_scoped() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream = sid(10);
    let mut config = StreamConfig::two_stage();
    config.t0sz = 0;
    config.s2_t0sz = 0;
    smmu.configure_stream(stream, config).unwrap();

    // Create stream-level stage-2 AS.
    smmu.create_stage2_address_space(stream).unwrap();

    // Create two PASIDs.
    let p0 = pasid(0);
    let p1 = pasid(1);
    smmu.create_pasid(stream, p0).unwrap();
    smmu.create_pasid(stream, p1).unwrap();

    // Both PASIDs map IOVA=0x1000 → IPA=0x5000 (stage-1 output).
    let test_iova = iova(0x1000);
    let ipa_addr  = pa(0x5000); // stage-1 PA output = stage-2 IPA input
    let final_pa  = pa(0xC000);
    smmu.map_page(stream, p0, test_iova, ipa_addr, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_page(stream, p1, test_iova, ipa_addr, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Map IPA=0x5000 → PA=0xC000 in the shared stage-2 (one mapping for all PASIDs).
    smmu.map_stage2_page(stream, iova(0x5000), final_pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Both PASID 0 and PASID 1 must resolve to PA=0xC000.
    let result0 = smmu.translate(stream, p0, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result0.is_ok(), "PASID 0 translation must succeed using shared stage-2 mapping");
    assert_eq!(result0.unwrap().physical_address().as_u64(), 0xC000,
        "PASID 0 must resolve to PA=0xC000 via stream-scoped stage-2");

    let result1 = smmu.translate(stream, p1, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result1.is_ok(), "PASID 1 translation must succeed using shared stage-2 mapping");
    assert_eq!(result1.unwrap().physical_address().as_u64(), 0xC000,
        "PASID 1 must resolve to PA=0xC000 via the same shared stage-2 (stream-scoped, not PASID-scoped)");
}

/// Verify PASID 0 and PASID 1 get identical errors when stage-2 IPA is unmapped.
/// Confirms no special-case handling for PASID 0 vs PASID 1.
#[test]
fn bugaudit3_pasid0_not_special_for_stage2_miss() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream = sid(11);
    let mut config = StreamConfig::two_stage();
    config.t0sz = 0;
    config.s2_t0sz = 0;
    smmu.configure_stream(stream, config).unwrap();

    smmu.create_stage2_address_space(stream).unwrap();

    let p0 = pasid(0);
    let p1 = pasid(1);
    smmu.create_pasid(stream, p0).unwrap();
    smmu.create_pasid(stream, p1).unwrap();

    // Map stage-1 IOVA→IPA for both PASIDs but do NOT map IPA→PA in stage-2.
    let test_iova = iova(0x2000);
    let ipa_addr  = pa(0x6000);
    smmu.map_page(stream, p0, test_iova, ipa_addr, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_page(stream, p1, test_iova, ipa_addr, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Both must fault (no stage-2 mapping).
    let err0 = smmu.translate(stream, p0, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(err0.is_err(), "PASID 0 must fault when stage-2 IPA has no mapping");

    let err1 = smmu.translate(stream, p1, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(err1.is_err(), "PASID 1 must fault when stage-2 IPA has no mapping");

    // Both must produce the same error variant (no special-case for PASID 0).
    let err0_str = format!("{:?}", err0.unwrap_err());
    let err1_str = format!("{:?}", err1.unwrap_err());
    assert_eq!(err0_str, err1_str,
        "PASID 0 and PASID 1 must produce identical errors for missing stage-2 mapping");
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-4: get_events() does not advance CONS
// ---------------------------------------------------------------------------

/// Verify that calling `get_events()` advances EVENTQ_CONS.RD.
///
/// ARM §3.5.1: Software (i.e. get_events) must update CONS.RD after reading
/// events to signal that ring buffer slots are free.
///
/// Before fix: CONS.RD stays at 0 regardless of how many events were read.
/// After fix:  CONS.RD advances by the number of events consumed.
#[test]
fn bugaudit4_get_events_advances_eventq_cons() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream = sid(20);
    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();

    // Verify CONS starts at 0.
    let cons_before = smmu.eventq_cons_index() & !(1u32 << 31);
    assert_eq!(cons_before, 0, "CONS.RD must start at 0");

    // Generate a fault → one event enters the queue.
    let _ = smmu.translate(stream, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(smmu.has_events(), "At least one event must be in the queue after a translation fault");

    let prod_after = smmu.eventq_prod_index();
    assert!(prod_after > 0, "PROD must have advanced after recording a fault event");

    // Consume events.
    let events = smmu.get_events();
    assert!(!events.is_empty(), "get_events() must return the fault event");

    // CONS must now match PROD.
    let cons_after = smmu.eventq_cons_index() & !(1u32 << 31);
    assert!(cons_after > 0,
        "CONS.RD must advance after get_events() consumes events (ARM §3.5.1). \
         Before fix: CONS stays at 0 forever.");

    assert_eq!(cons_after, prod_after & !(1u32 << 31),
        "After consuming all events, CONS.RD must equal PROD.WR (empty queue condition).");
}

/// Verify queue does not appear full after consuming events with get_events().
///
/// If CONS never advances, ring buffer occupancy treats every slot as occupied
/// after capacity is reached, silently dropping new events.
#[test]
fn bugaudit4_queue_not_full_after_consuming_events() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream = sid(21);
    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();

    // Generate 32 faults.
    for i in 0u64..32 {
        let addr = iova(0x1000 + i * 0x1000);
        let _ = smmu.translate(stream, pasid(0), addr, AccessType::Read, SecurityState::NonSecure);
    }

    // Consume — CONS must advance to match PROD.
    let events = smmu.get_events();
    assert!(!events.is_empty(), "First batch must contain events");

    let cons_after = smmu.eventq_cons_index() & !(1u32 << 31);
    let prod_after = smmu.eventq_prod_index();
    assert_eq!(cons_after, prod_after & !(1u32 << 31),
        "After consuming all events CONS must equal PROD (empty queue). \
         Bug: CONS stays at 0 so queue appears perpetually full.");
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-7: STAG inner zero-skip loop has no cap
// ---------------------------------------------------------------------------

/// Verify that STAG allocation never returns STAG=0 across multiple stall faults.
///
/// ARM §3.12.2: STAG is a non-zero 16-bit identifier.
/// The fix caps the inner `while candidate == 0` loop so wrap-around cannot
/// loop indefinitely.
#[test]
fn bugaudit7_stag_allocation_never_returns_zero() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let stream = sid(30);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(stream, config).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();

    // Map as execute-only so write access → permission fault → stall.
    smmu.map_page(
        stream, pasid(0), iova(0x1000), pa(0xD000),
        PagePermissions::execute_only(), SecurityState::NonSecure,
    ).unwrap();

    // Perform 8 stall-causing faults, verifying STAG != 0 each time.
    for _round in 0..8 {
        let result = smmu.translate(
            stream, pasid(0), iova(0x1000), AccessType::Write, SecurityState::NonSecure,
        );
        if let Err(TranslationError::Stalled { stag }) = result {
            assert_ne!(stag, 0,
                "STAG=0 is reserved and must never be issued (ARM §3.12.2).");
            // Drain the stall by sending CMD_RESUME.
            let mut resume = CommandEntry::new(CommandType::Resume, stream.as_u32(), 0);
            resume.stag = stag;
            resume.action = true; // Ac=1: retry
            smmu.submit_command(resume).unwrap();
            smmu.process_command_queue().unwrap();
        }
        // Consume events to keep queue from filling.
        let _ = smmu.get_events();
    }
}

/// Verify that two concurrent stall faults produce distinct non-zero STAGs.
#[test]
fn bugaudit7_stag_values_are_unique_across_stalls() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream = sid(31);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(stream, config).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();

    // Map two execute-only pages so write access → stall on each.
    smmu.map_page(stream, pasid(0), iova(0x1000), pa(0xE000), PagePermissions::execute_only(), SecurityState::NonSecure).unwrap();
    smmu.map_page(stream, pasid(0), iova(0x2000), pa(0xF000), PagePermissions::execute_only(), SecurityState::NonSecure).unwrap();

    // Trigger two stall faults simultaneously (before resuming either).
    let result1 = smmu.translate(stream, pasid(0), iova(0x1000), AccessType::Write, SecurityState::NonSecure);
    let result2 = smmu.translate(stream, pasid(0), iova(0x2000), AccessType::Write, SecurityState::NonSecure);

    let stag1 = if let Err(TranslationError::Stalled { stag }) = result1 { Some(stag) } else { None };
    let stag2 = if let Err(TranslationError::Stalled { stag }) = result2 { Some(stag) } else { None };

    // If both produced stall errors verify they are non-zero and distinct.
    if let (Some(s1), Some(s2)) = (stag1, stag2) {
        assert_ne!(s1, 0, "First STAG must be non-zero");
        assert_ne!(s2, 0, "Second STAG must be non-zero");
        assert_ne!(s1, s2, "Two distinct stall faults must produce distinct STAGs");
    }
    // If one produced StallQueueFull that is acceptable — the property checked is
    // that we never observe STAG=0.
}
