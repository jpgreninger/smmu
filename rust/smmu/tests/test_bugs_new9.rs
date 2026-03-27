//! Regression tests for BUG-NEW-41, BUG-NEW-42, and BUG-NEW-43.
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green).
//!
//! # BUG-NEW-41 (§4.4.2.4 / §4.4.1.1) — TlbiNhVaa ignores RIL range fields
//!
//! ARM §4.4.2.4: CMD_TLBI_NH_VAA is a VMID-scoped VAA (any-ASID) invalidation.
//! ARM §4.4.1.1: When RIL=1, the VA TLBI commands must invalidate a range of
//! `(num+1) * 2^scale * granule_size` bytes starting at `start_address`.
//!
//! Current code: `TlbiNhVaa` with `ril=true` calls
//! `invalidate_by_vmid_and_va(vmid, start_address)` unconditionally — a
//! single-page invalidation that ignores the `tg`, `num`, and `scale` fields.
//!
//! BEFORE FIX: with `ril=true, tg=0 (4KB), scale=0, num=1` (range = 2 pages =
//!   8192 bytes covering [0x1000, 0x2FFF]), only VA=0x1000 is evicted; VA=0x2000
//!   remains cached → translate(VA=0x2000) is a TLB hit → miss-count assertion
//!   FAILS (red): the test expects a miss but gets a hit.
//! AFTER FIX:  range invalidation evicts both VA=0x1000 and VA=0x2000 →
//!   translate(VA=0x2000) is a TLB miss → assertion PASSES (green).
//!
//! # BUG-NEW-42 (§4.4.2.9 / §4.4.1.1) — TlbiEl2Vaa ignores RIL range fields
//!
//! ARM §4.4.2.9: CMD_TLBI_EL2_VAA invalidates EL2 TLB entries by VA (any ASID).
//! ARM §4.4.1.1: When RIL=1, range-based invalidation must be applied.
//!
//! Current code: `TlbiEl2Vaa` calls `invalidate_by_va(start_address)`
//! unconditionally — a single-page invalidation that ignores `tg`, `num`, `scale`.
//!
//! BEFORE FIX: with `ril=true, tg=0 (4KB), scale=0, num=1` (range = 2 pages =
//!   8192 bytes covering [0x1000, 0x2FFF]), only VA=0x1000 is evicted; VA=0x2000
//!   remains cached → translate(VA=0x2000) is a TLB hit → miss-count assertion
//!   FAILS (red): the test expects a miss but gets a hit.
//! AFTER FIX:  range invalidation evicts both VA=0x1000 and VA=0x2000 →
//!   translate(VA=0x2000) is a TLB miss → assertion PASSES (green).
//!
//! # BUG-NEW-43 (comment-only) — Section numbers swapped in test_bugs_new8 header
//!
//! The doc comment header of `test_bugs_new8.rs` (and the C++ equivalent) had
//! §4.4.2.3 and §4.4.2.4 transposed in the BUG-NEW-37 description:
//!   ARM IHI0070G.b §4.4.2.3 = CMD_TLBI_NH_VAA (not NH_VA)
//!   ARM IHI0070G.b §4.4.2.4 = CMD_TLBI_NH_VA  (not NH_VAA)
//!
//! The fix is a comment-only change in test_bugs_new8.rs lines 8–9 and the
//! C++ equivalent in test_bugs_new8.cpp.  No new test is needed for a
//! comment fix; BUG-NEW-43 is documented here for traceability.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, SecurityState, StreamConfig,
    StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN + PRIQEN enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID  { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }
fn pa(addr: u64) -> PA     { PA::new(addr).unwrap() }

/// Set up a stage-1 stream with the given VMID and ASID, map a page, and warm
/// the TLB by performing an initial translate.  Returns without error on success.
fn setup_stream_and_warm_tlb(
    smmu: &SMMU,
    stream_n: u32,
    vmid: u16,
    asid: u16,
    iova_addr: u64,
    pa_addr: u64,
) {
    let s = sid(stream_n);
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0; // no VA-range restriction
    smmu.configure_stream(s, cfg).unwrap();
    smmu.set_stream_vmid(s, vmid).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.set_pasid_asid(s, pasid(0), asid).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(iova_addr),
        pa(pa_addr),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    ).unwrap();
    // Warm TLB by performing an initial translate.
    let result = smmu.translate(
        s, pasid(0), iova(iova_addr), AccessType::Read, SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "setup: initial translate for stream {} at VA=0x{:X} must succeed",
        stream_n, iova_addr
    );
}

// ============================================================================
// BUG-NEW-41 — TlbiNhVaa ignores RIL range fields (single-page only)
// ============================================================================
// ARM §4.4.2.4: CMD_TLBI_NH_VAA is a VMID-scoped VAA (any-ASID) invalidation.
// ARM §4.4.1.1: RIL=1 means range invalidation using tg, num, scale.
//
// With tg=0 (4KB), scale=0, num=1:
//   range = (num+1) * 2^scale * granule = (1+1) * 2^0 * 4096 = 8192 bytes
//   covers [start_address, start_address + 8191]
//   e.g. start=0x1000 → covers [0x1000, 0x2FFF] → evicts pages at 0x1000 AND 0x2000
//
// BEFORE FIX: only 0x1000 is evicted (single-page call), 0x2000 remains cached
//   → translate(0x2000) is a TLB hit → miss-count diff == 0 → ASSERTION FAILS.
// AFTER FIX:  range invalidation evicts both; translate(0x2000) is a miss
//   → miss-count diff == 1 → ASSERTION PASSES.

/// BUG-NEW-41 primary: `TlbiNhVaa` with `ril=true` must evict the second page
/// in the range (VA=0x2000), not just the start address (VA=0x1000).
///
/// ARM §4.4.2.4 + §4.4.1.1: RIL=1, tg=0 (4KB), scale=0, num=1 → 8192-byte
/// range covering [0x1000, 0x2FFF].  Both pages at 0x1000 and 0x2000 must be
/// evicted.
///
/// BEFORE FIX: `invalidate_by_vmid_and_va(vmid, 0x1000)` is called → only
///   0x1000 evicted → 0x2000 still a TLB hit → test FAILS (red).
/// AFTER FIX:  range invalidation evicts [0x1000, 0x2FFF] → 0x2000 is a TLB
///   miss → test PASSES (green).
#[test]
fn bug_new41_tlbi_nh_vaa_ril_range_evicts_second_page() {
    let smmu = make_smmu();

    // Stream 0x30: VMID=5, ASID=10, page at VA=0x1000.
    setup_stream_and_warm_tlb(&smmu, 0x30, 5, 10, 0x1000, 0xA000);

    // Stream 0x31: VMID=5, ASID=10, page at VA=0x2000.
    // Both streams share VMID=5 so TlbiNhVaa(VMID=5) targets both.
    setup_stream_and_warm_tlb(&smmu, 0x31, 5, 10, 0x2000, 0xB000);

    // Verify that VA=0x2000 is in the TLB (second translate is a hit).
    let misses_before_confirm = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x31), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_after_confirm = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_after_confirm, misses_before_confirm,
        "BUG-NEW-41 setup: VA=0x2000 (stream 0x31) must be a TLB hit before invalidation"
    );

    // Issue TlbiNhVaa with ril=true, tg=0 (4KB), scale=0, num=1.
    // Correct range: (1+1) * 2^0 * 4096 = 8192 bytes → [0x1000, 0x2FFF].
    // Both pages at 0x1000 and 0x2000 must be evicted.
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0, 0);
    cmd.vmid = 5;
    cmd.start_address = 0x1000;
    cmd.ril = true;
    cmd.tg = 0;    // 4 KB granule
    cmd.scale = 0; // 2^0 = 1 block per num+1 units
    cmd.num = 1;   // num+1 = 2 blocks → 2 * 1 * 4096 = 8192 bytes
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VA=0x2000 must now be a TLB miss (evicted by the range invalidation).
    // BEFORE FIX: only VA=0x1000 was evicted → VA=0x2000 is still a hit →
    //   misses_after - misses_before == 0 → assertion FAILS (red).
    // AFTER FIX: range covers [0x1000, 0x2FFF] → VA=0x2000 evicted →
    //   misses_after - misses_before == 1 → assertion PASSES (green).
    let misses_before_check = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x31), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_after_check = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_after_check - misses_before_check,
        1,
        "BUG-NEW-41: TlbiNhVaa with ril=true, tg=0, scale=0, num=1 must evict VA=0x2000 \
         (range covers [0x1000, 0x2FFF]).  Before fix: only VA=0x1000 is evicted \
         (single-page invalidate_by_vmid_and_va), VA=0x2000 remains a TLB hit \
         (miss delta=0).  After fix: delta=1."
    );
}

/// BUG-NEW-41 secondary: `TlbiNhVaa` with `ril=true` must NOT evict a page
/// that is OUTSIDE the computed range.
///
/// ARM §4.4.1.1: range = (num+1) * 2^scale * granule_size bytes from start.
/// With tg=0 (4KB), scale=0, num=1 → covers [0x1000, 0x2FFF].
/// VA=0x3000 is outside this range and must remain cached after the TLBI.
///
/// This test verifies the upper boundary: the range must not over-invalidate.
/// It PASSES before and after the fix (no regression expected), but confirms
/// the fix does not introduce over-invalidation.
///
/// BEFORE FIX: single-page call evicts only 0x1000; 0x3000 is still cached →
///   this sub-test passes vacuously (no over-invalidation from the bug).
/// AFTER FIX:  range [0x1000, 0x2FFF] is invalidated; 0x3000 is still outside →
///   still cached → this sub-test passes.
#[test]
fn bug_new41_tlbi_nh_vaa_ril_range_preserves_page_outside_range() {
    let smmu = make_smmu();

    // Stream 0x32: VMID=6, ASID=11, page at VA=0x1000 (inside range [0x1000, 0x2FFF]).
    setup_stream_and_warm_tlb(&smmu, 0x32, 6, 11, 0x1000, 0xC000);

    // Stream 0x33: VMID=6, ASID=11, page at VA=0x3000 (OUTSIDE range [0x1000, 0x2FFF]).
    setup_stream_and_warm_tlb(&smmu, 0x33, 6, 11, 0x3000, 0xD000);

    // Confirm VA=0x3000 is in the TLB before TLBI.
    let misses_pre_warm = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x33), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_post_warm = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_post_warm, misses_pre_warm,
        "BUG-NEW-41 setup: VA=0x3000 must be a TLB hit before invalidation"
    );

    // Issue TlbiNhVaa with ril=true, tg=0, scale=0, num=1 → range [0x1000, 0x2FFF].
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0, 0);
    cmd.vmid = 6;
    cmd.start_address = 0x1000;
    cmd.ril = true;
    cmd.tg = 0;
    cmd.scale = 0;
    cmd.num = 1;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VA=0x3000 must NOT be evicted — it is outside [0x1000, 0x2FFF].
    // Both before and after fix this should be a TLB hit (no over-invalidation).
    let misses_before_check = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x33), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_after_check = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_after_check, misses_before_check,
        "BUG-NEW-41: TlbiNhVaa range [0x1000, 0x2FFF] must NOT evict VA=0x3000 \
         (outside the range).  After fix: only [0x1000, 0x2FFF] is invalidated."
    );
}

// ============================================================================
// BUG-NEW-42 — TlbiEl2Vaa ignores RIL range fields (single-page only)
// ============================================================================
// ARM §4.4.2.9: CMD_TLBI_EL2_VAA invalidates EL2 TLB entries by VA (any ASID).
// ARM §4.4.1.1: RIL=1 means range invalidation using tg, num, scale.
//
// With tg=0 (4KB), scale=0, num=1:
//   range = (num+1) * 2^scale * granule = (1+1) * 2^0 * 4096 = 8192 bytes
//   covers [start_address, start_address + 8191]
//   e.g. start=0x1000 → covers [0x1000, 0x2FFF] → evicts pages at 0x1000 AND 0x2000
//
// BEFORE FIX: only 0x1000 is evicted (single-page call), 0x2000 remains cached
//   → translate(0x2000) is a TLB hit → miss-count diff == 0 → ASSERTION FAILS.
// AFTER FIX:  range invalidation evicts both; translate(0x2000) is a miss
//   → miss-count diff == 1 → ASSERTION PASSES.
//
// Note: TlbiEl2Vaa requires IDR0.Hyp=1. Use set_hyp_supported(true) to enable.
// For EL2 streams, TLB entries are tagged by VA only (no VMID on EL2 world).
// Use two separate streams (different stream IDs) both configured for EL2,
// each mapping a different VA page, to warm two distinct TLB entries.

/// BUG-NEW-42 primary: `TlbiEl2Vaa` with `ril=true` must evict the second page
/// in the range (VA=0x2000), not just the start address (VA=0x1000).
///
/// ARM §4.4.2.9 + §4.4.1.1: RIL=1, tg=0 (4KB), scale=0, num=1 → 8192-byte
/// range covering [0x1000, 0x2FFF].  Both EL2 TLB entries at 0x1000 and 0x2000
/// must be evicted.
///
/// BEFORE FIX: `invalidate_by_va(0x1000)` is called → only 0x1000 evicted →
///   0x2000 still a TLB hit → test FAILS (red).
/// AFTER FIX:  range invalidation evicts [0x1000, 0x2FFF] → 0x2000 is a TLB
///   miss → test PASSES (green).
#[test]
fn bug_new42_tlbi_el2_vaa_ril_range_evicts_second_page() {
    let smmu = make_smmu();

    // Enable EL2 (IDR0.Hyp=1): required for TlbiEl2Vaa to proceed past the guard.
    smmu.set_hyp_supported(true);

    // Stream 0x40: EL2-scoped stage-1 stream, page at VA=0x1000.
    // Use stage1_only config; EL2 streams use a per-stream address space.
    {
        let s = sid(0x40);
        let mut cfg = StreamConfig::stage1_only();
        cfg.t0sz = 0;
        smmu.configure_stream(s, cfg).unwrap();
        smmu.create_pasid(s, pasid(0)).unwrap();
        smmu.map_page(
            s, pasid(0), iova(0x1000), pa(0xE000),
            PagePermissions::read_only(), SecurityState::NonSecure,
        ).unwrap();
        smmu.translate(
            s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
        ).unwrap();
    }

    // Stream 0x41: EL2-scoped stage-1 stream, page at VA=0x2000.
    {
        let s = sid(0x41);
        let mut cfg = StreamConfig::stage1_only();
        cfg.t0sz = 0;
        smmu.configure_stream(s, cfg).unwrap();
        smmu.create_pasid(s, pasid(0)).unwrap();
        smmu.map_page(
            s, pasid(0), iova(0x2000), pa(0xF000),
            PagePermissions::read_only(), SecurityState::NonSecure,
        ).unwrap();
        smmu.translate(
            s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure,
        ).unwrap();
    }

    // Confirm VA=0x2000 is in the TLB (second translate is a hit).
    let misses_before_confirm = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x41), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_after_confirm = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_after_confirm, misses_before_confirm,
        "BUG-NEW-42 setup: VA=0x2000 (stream 0x41) must be a TLB hit before invalidation"
    );

    // Issue TlbiEl2Vaa with ril=true, tg=0 (4KB), scale=0, num=1.
    // Correct range: (1+1) * 2^0 * 4096 = 8192 bytes → [0x1000, 0x2FFF].
    // Both pages at 0x1000 and 0x2000 must be evicted.
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Vaa, 0, 0);
    cmd.start_address = 0x1000;
    cmd.ril = true;
    cmd.tg = 0;    // 4 KB granule
    cmd.scale = 0; // 2^0 = 1 block per num+1 units
    cmd.num = 1;   // num+1 = 2 blocks → 2 * 1 * 4096 = 8192 bytes
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VA=0x2000 must now be a TLB miss (evicted by the range invalidation).
    // BEFORE FIX: only VA=0x1000 was evicted → VA=0x2000 is still a hit →
    //   misses_after - misses_before == 0 → assertion FAILS (red).
    // AFTER FIX: range covers [0x1000, 0x2FFF] → VA=0x2000 evicted →
    //   misses_after - misses_before == 1 → assertion PASSES (green).
    let misses_before_check = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x41), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_after_check = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_after_check - misses_before_check,
        1,
        "BUG-NEW-42: TlbiEl2Vaa with ril=true, tg=0, scale=0, num=1 must evict VA=0x2000 \
         (range covers [0x1000, 0x2FFF]).  Before fix: only VA=0x1000 is evicted \
         (single-page invalidate_by_va), VA=0x2000 remains a TLB hit (miss delta=0). \
         After fix: delta=1."
    );
}

/// BUG-NEW-42 secondary: `TlbiEl2Vaa` with `ril=true` must NOT evict a page
/// that is OUTSIDE the computed range.
///
/// ARM §4.4.1.1: range = (num+1) * 2^scale * granule_size bytes from start.
/// With tg=0 (4KB), scale=0, num=1 → covers [0x1000, 0x2FFF].
/// VA=0x3000 is outside this range and must remain cached after the TLBI.
///
/// This test verifies the upper boundary: the range must not over-invalidate.
///
/// BEFORE FIX: single-page call evicts only 0x1000; 0x3000 is still cached →
///   this sub-test passes vacuously (no over-invalidation from the bug).
/// AFTER FIX:  range [0x1000, 0x2FFF] is invalidated; 0x3000 is still outside →
///   still cached → this sub-test passes.
#[test]
fn bug_new42_tlbi_el2_vaa_ril_range_preserves_page_outside_range() {
    let smmu = make_smmu();
    smmu.set_hyp_supported(true);

    // Stream 0x42: page at VA=0x1000 (inside range [0x1000, 0x2FFF]).
    {
        let s = sid(0x42);
        let mut cfg = StreamConfig::stage1_only();
        cfg.t0sz = 0;
        smmu.configure_stream(s, cfg).unwrap();
        smmu.create_pasid(s, pasid(0)).unwrap();
        smmu.map_page(
            s, pasid(0), iova(0x1000), pa(0x1_0000),
            PagePermissions::read_only(), SecurityState::NonSecure,
        ).unwrap();
        smmu.translate(
            s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure,
        ).unwrap();
    }

    // Stream 0x43: page at VA=0x3000 (OUTSIDE range [0x1000, 0x2FFF]).
    {
        let s = sid(0x43);
        let mut cfg = StreamConfig::stage1_only();
        cfg.t0sz = 0;
        smmu.configure_stream(s, cfg).unwrap();
        smmu.create_pasid(s, pasid(0)).unwrap();
        smmu.map_page(
            s, pasid(0), iova(0x3000), pa(0x1_1000),
            PagePermissions::read_only(), SecurityState::NonSecure,
        ).unwrap();
        smmu.translate(
            s, pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
        ).unwrap();
    }

    // Confirm VA=0x3000 is in the TLB before TLBI.
    let misses_pre_warm = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x43), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_post_warm = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_post_warm, misses_pre_warm,
        "BUG-NEW-42 setup: VA=0x3000 must be a TLB hit before invalidation"
    );

    // Issue TlbiEl2Vaa with ril=true, tg=0, scale=0, num=1 → range [0x1000, 0x2FFF].
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Vaa, 0, 0);
    cmd.start_address = 0x1000;
    cmd.ril = true;
    cmd.tg = 0;
    cmd.scale = 0;
    cmd.num = 1;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VA=0x3000 must NOT be evicted — it is outside [0x1000, 0x2FFF].
    // Both before and after fix this should be a TLB hit (no over-invalidation).
    let misses_before_check = smmu.get_cache_statistics().tlb_misses();
    smmu.translate(
        sid(0x43), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure,
    ).unwrap();
    let misses_after_check = smmu.get_cache_statistics().tlb_misses();
    assert_eq!(
        misses_after_check, misses_before_check,
        "BUG-NEW-42: TlbiEl2Vaa range [0x1000, 0x2FFF] must NOT evict VA=0x3000 \
         (outside the range).  After fix: only [0x1000, 0x2FFF] is invalidated."
    );
}
