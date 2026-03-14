#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-M-09: Range-based ATC invalidation for CMD_ATC_INV.
//!
//! Spec: ARM IHI0070G.b §4.5.1 (CMD_ATC_INV)
//!
//! Requirements:
//! - CMD_ATC_INV with G=0 must invalidate TLB entries whose IOVA falls within
//!   [start_address, end_address] for the specified (stream_id, PASID).
//! - Entries whose IOVA is OUTSIDE [start, end] must NOT be evicted.
//! - Entries belonging to a DIFFERENT stream must survive.
//! - Entries belonging to a DIFFERENT PASID on the same stream must survive.
//! - CMD_ATC_INV with G=1 (Global flag, CommandEntry.flags bit 0) must
//!   invalidate ALL entries for the specified (stream_id, PASID) regardless
//!   of address.
//! - The AtcInvalidateCompletion event must still be emitted.
//! - The SMMU-level invalidation counter must increment after each command.
//!
//! TLB hit/miss behaviour is verified via get_cache_statistics().tlb_hits()
//! and .tlb_misses(): after an AtcInv, a translation to an evicted IOVA
//! produces a TLB miss; a translation to a surviving IOVA produces a TLB hit.

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions, SecurityState,
    StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
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

/// Build an ATC invalidation command with explicit start/end and flags.
const fn atc_inv(stream_id: u32, pasid_val: u32, start: u64, end: u64, flags: u32) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::AtcInv,
        stream_id,
        pasid: pasid_val,
        start_address: start,
        end_address: end,
        flags,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        prg_index: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        tg: 0,
        num: 0,
        scale: 0,
        ttl: 0,
        ril: false,
        security_state: SecurityState::NonSecure,
    }
}

/// Set up a Stage-1 stream with `n` PASIDs.  PASID p gets a page mapped at
/// IOVA = `0x1000 + p * 0x1000` → PA = `0x8000_0000 + p * 0x1000`.
/// All pages are translated once to warm the TLB.
fn setup_and_warm(smmu: &SMMU, stream_n: u32, num_pasids: u32) {
    let s = sid(stream_n);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .pasid_enabled(true)
        .max_pasid(num_pasids)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();

    for p in 0..num_pasids {
        smmu.create_pasid(s, pasid(p)).unwrap();
        let iova_val = 0x1000 + u64::from(p) * 0x1000;
        let pa_val = 0x8000_0000 + u64::from(p) * 0x1000;
        smmu.map_page(s, pasid(p), iova(iova_val), pa(pa_val),
                      PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        // Warm TLB
        let _ = smmu.translate(s, pasid(p), iova(iova_val), AccessType::Read, SecurityState::NonSecure);
    }
}

/// Returns the delta in TLB hits after translating `iova_addr` for the given
/// (stream, pasid).  A delta of +1 means TLB hit; 0 means TLB miss.
fn hits_delta_after_translate(smmu: &SMMU, stream_n: u32, pasid_n: u32, iova_addr: u64) -> u64 {
    let before = smmu.get_cache_statistics().tlb_hits();
    let _ = smmu.translate(sid(stream_n), pasid(pasid_n), iova(iova_addr),
                           AccessType::Read, SecurityState::NonSecure);
    let after = smmu.get_cache_statistics().tlb_hits();
    after - before
}

/// Returns the delta in TLB misses after translating `iova_addr`.
fn misses_delta_after_translate(smmu: &SMMU, stream_n: u32, pasid_n: u32, iova_addr: u64) -> u64 {
    let before = smmu.get_cache_statistics().tlb_misses();
    let _ = smmu.translate(sid(stream_n), pasid(pasid_n), iova(iova_addr),
                           AccessType::Read, SecurityState::NonSecure);
    let after = smmu.get_cache_statistics().tlb_misses();
    after - before
}

// ── Range invalidation — targeted eviction ────────────────────────────────────

/// CMD_ATC_INV with G=0 must evict TLB entries whose IOVA falls in [start, end].
/// This is verified by observing a TLB miss on the next translate for those IOVAs.
#[test]
fn test_atc_inv_range_evicts_entries_within_range() {
    let smmu = make_smmu();
    // Map and warm: PASID 0 → IOVA 0x1000; PASID 0 uses page at 0x1000 and 0x3000
    let s = sid(1);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(s, pasid(0), iova(0x1000), pa(0xA000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_page(s, pasid(0), iova(0x3000), pa(0xB000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Warm both TLB entries
    let _ = smmu.translate(s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    let _ = smmu.translate(s, pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure);

    // Invalidate range [0x1000, 0x2000] — only 0x1000 should be evicted
    smmu.submit_command(atc_inv(1, 0, 0x1000, 0x2000, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    // 0x1000 must be a TLB MISS (was evicted)
    let miss_delta = misses_delta_after_translate(&smmu, 1, 0, 0x1000);
    assert_eq!(miss_delta, 1,
               "IOVA 0x1000 is within AtcInv range — must be a TLB miss after invalidation");

    // 0x3000 must be a TLB HIT (outside range, survived)
    let hit_delta = hits_delta_after_translate(&smmu, 1, 0, 0x3000);
    assert_eq!(hit_delta, 1,
               "IOVA 0x3000 is outside AtcInv range — must remain a TLB hit");
}

/// The boundary: an entry exactly at start_address must be evicted.
#[test]
fn test_atc_inv_range_boundary_start_is_inclusive() {
    let smmu = make_smmu();
    let s = sid(1);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(s, pasid(0), iova(0x2000), pa(0xC000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s, pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);

    // Invalidate [0x2000, 0x4000] — 0x2000 is the start boundary
    smmu.submit_command(atc_inv(1, 0, 0x2000, 0x4000, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    let miss_delta = misses_delta_after_translate(&smmu, 1, 0, 0x2000);
    assert_eq!(miss_delta, 1, "start_address is inclusive — must be a TLB miss");
}

/// The boundary: an entry exactly at end_address must be evicted.
#[test]
fn test_atc_inv_range_boundary_end_is_inclusive() {
    let smmu = make_smmu();
    let s = sid(1);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(s, pasid(0), iova(0x4000), pa(0xD000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure);

    // Invalidate [0x2000, 0x4000] — 0x4000 is the end boundary
    smmu.submit_command(atc_inv(1, 0, 0x2000, 0x4000, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    let miss_delta = misses_delta_after_translate(&smmu, 1, 0, 0x4000);
    assert_eq!(miss_delta, 1, "end_address is inclusive — must be a TLB miss");
}

// ── Range invalidation — non-matching entries survive ────────────────────────

/// Entries for a different stream must NOT be evicted by CMD_ATC_INV.
#[test]
fn test_atc_inv_range_does_not_affect_other_streams() {
    let smmu = make_smmu();

    // Stream 1: map 0x1000
    let s1 = sid(1);
    smmu.configure_stream(s1, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s1, pasid(0)).unwrap();
    smmu.map_page(s1, pasid(0), iova(0x1000), pa(0xA000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s1, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // Stream 2: map 0x1000 — same IOVA, different stream
    let s2 = sid(2);
    smmu.configure_stream(s2, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s2, pasid(0)).unwrap();
    smmu.map_page(s2, pasid(0), iova(0x1000), pa(0xB000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s2, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // Invalidate stream 1's [0x1000, 0x2000]
    smmu.submit_command(atc_inv(1, 0, 0x1000, 0x2000, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream 2's entry at 0x1000 must survive (TLB hit)
    let hit_delta = hits_delta_after_translate(&smmu, 2, 0, 0x1000);
    assert_eq!(hit_delta, 1,
               "Stream 2's TLB entry must survive CMD_ATC_INV targeting stream 1");
}

/// Entries for a different PASID on the same stream must NOT be evicted.
#[test]
fn test_atc_inv_range_does_not_affect_other_pasids() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .pasid_enabled(true)
        .max_pasid(2)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();

    // PASID 0: map 0x1000
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(s, pasid(0), iova(0x1000), pa(0xA000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // PASID 1: map 0x1000 — same IOVA, different PASID
    smmu.create_pasid(s, pasid(1)).unwrap();
    smmu.map_page(s, pasid(1), iova(0x1000), pa(0xB000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s, pasid(1), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // Invalidate stream 1 / PASID 0's [0x1000, 0x2000]
    smmu.submit_command(atc_inv(1, 0, 0x1000, 0x2000, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    // PASID 1's entry at 0x1000 must survive (TLB hit)
    let hit_delta = hits_delta_after_translate(&smmu, 1, 1, 0x1000);
    assert_eq!(hit_delta, 1,
               "PASID 1's TLB entry must survive CMD_ATC_INV targeting PASID 0");
}

// ── Global flag (G=1) — invalidate all entries for stream/PASID ──────────────

/// CMD_ATC_INV with flags bit 0 set (G=1) must invalidate ALL TLB entries for
/// the (stream, PASID) pair regardless of address.
#[test]
fn test_atc_inv_global_flag_invalidates_all_for_stream_pasid() {
    let smmu = make_smmu();
    let s = sid(1);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    // Map two well-separated pages
    smmu.map_page(s, pasid(0), iova(0x1000), pa(0xA000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_page(s, pasid(0), iova(0x9000), pa(0xB000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    // Warm TLB
    let _ = smmu.translate(s, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    let _ = smmu.translate(s, pasid(0), iova(0x9000), AccessType::Read, SecurityState::NonSecure);

    // G=1: flags bit 0 set, address range irrelevant
    smmu.submit_command(atc_inv(1, 0, 0x1000, 0x2000, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    // Both entries must be gone — TLB miss for each
    let miss_1000 = misses_delta_after_translate(&smmu, 1, 0, 0x1000);
    let miss_9000 = misses_delta_after_translate(&smmu, 1, 0, 0x9000);
    assert_eq!(miss_1000, 1,
               "G=1: 0x1000 must be a TLB miss (global invalidation)");
    assert_eq!(miss_9000, 1,
               "G=1: 0x9000 must be a TLB miss even though it's outside range — global flag overrides range");
}

/// CMD_ATC_INV with G=1 must NOT affect entries for a different stream.
#[test]
fn test_atc_inv_global_does_not_affect_other_streams() {
    let smmu = make_smmu();

    let s1 = sid(1);
    smmu.configure_stream(s1, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s1, pasid(0)).unwrap();
    smmu.map_page(s1, pasid(0), iova(0x1000), pa(0xA000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s1, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    let s2 = sid(2);
    smmu.configure_stream(s2, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s2, pasid(0)).unwrap();
    smmu.map_page(s2, pasid(0), iova(0x1000), pa(0xB000),
                  PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    let _ = smmu.translate(s2, pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);

    // Global invalidation targeting stream 1 only
    smmu.submit_command(atc_inv(1, 0, 0, u64::MAX, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    let hit_delta = hits_delta_after_translate(&smmu, 2, 0, 0x1000);
    assert_eq!(hit_delta, 1,
               "Stream 2's TLB entry must survive G=1 CMD_ATC_INV targeting stream 1");
}

// ── Completion event & counter ────────────────────────────────────────────────

/// CMD_ATC_INV must still emit an AtcInvalidateCompletion event.
#[test]
fn test_atc_inv_emits_completion_event() {
    let smmu = make_smmu();
    setup_and_warm(&smmu, 1, 1);

    smmu.submit_command(atc_inv(1, 0, 0x1000, 0x2000, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::AtcInvalidateCompletion),
        "CMD_ATC_INV must generate an AtcInvalidateCompletion event"
    );
}

/// CMD_ATC_INV must increment the SMMU-level invalidation counter.
#[test]
fn test_atc_inv_increments_invalidation_count() {
    let smmu = make_smmu();
    setup_and_warm(&smmu, 1, 1);

    let before = smmu.get_invalidation_count();
    smmu.submit_command(atc_inv(1, 0, 0x1000, 0x2000, 0)).unwrap();
    smmu.process_command_queue().unwrap();
    let after = smmu.get_invalidation_count();

    assert!(after > before, "CMD_ATC_INV must increment the invalidation counter");
}
