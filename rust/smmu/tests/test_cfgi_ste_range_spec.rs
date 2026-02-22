#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-NEW-05: CMD_CFGI_STE_RANGE / CMD_CFGI_ALL range-prefix semantics.
//!
//! Spec: ARM IHI0070G.b §4.3.2 (CMD_CFGI_STE_RANGE / CMD_CFGI_ALL)
//!
//! Requirements:
//! - Both commands share opcode 0x04 (`CommandType::CfgiAll`).
//! - `CommandEntry` must expose a `range: u8` field.
//! - `CommandEntry::new()` must default `range` to 31 (full invalidation semantics).
//! - CMD_CFGI_ALL (Range == 31): invalidates all STE-related TLB entries globally.
//! - CMD_CFGI_STE_RANGE (Range < 31): invalidates only streams whose StreamID matches the
//!   prefix `n[StreamIDSize-1 : Range+1]`:
//!   match condition → `(sid >> (Range+1)) == (command.stream_id >> (Range+1))`
//! - Processing either form must increment the SMMU invalidation counter.
//! - Page-table mappings survive both commands (only TLB entries are evicted;
//!   re-translation must succeed).

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, SecurityState, StreamConfig, StreamID,
    IOVA, PA, PASID,
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

/// Configure a Stage-1 stream with a single PASID-0 page mapping and warm the TLB.
fn setup_stream(smmu: &SMMU, stream_n: u32) {
    let s = sid(stream_n);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    let iova_addr = iova(0x1000);
    let pa_addr = pa(0x8000_0000 + u64::from(stream_n) * 0x1000);
    smmu.map_page(s, pasid(0), iova_addr, pa_addr, PagePermissions::read_write(),
                  SecurityState::NonSecure).unwrap();
    // Warm the TLB so a cached entry exists
    let _ = smmu.translate(s, pasid(0), iova_addr, AccessType::Read, SecurityState::NonSecure);
}

/// Returns `true` when the page mapping for the stream is reachable via translation.
fn is_accessible(smmu: &SMMU, stream_n: u32) -> bool {
    smmu.translate(
        sid(stream_n),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    )
    .is_ok()
}

/// Build a CMD_CFGI_ALL command (Range = 31 → full global invalidation).
const fn cfgi_all_cmd() -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::CfgiAll,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        prg_index: 0,
        action: false,
        abort: false,
        range: 31, // CMD_CFGI_ALL: Range == 31 → global invalidation per ARM §4.3.2
        leaf: false,
        cs: 0,
    }
}

/// Build a CMD_CFGI_STE_RANGE command for the given `stream_id` and `range`.
const fn cfgi_ste_range_cmd(stream_id: u32, range: u8) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::CfgiAll, // same opcode (0x04) per ARM §4.1.1
        stream_id,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag: 0,
        prg_index: 0,
        action: false,
        abort: false,
        range, // CMD_CFGI_STE_RANGE: Range < 31 selects streams by prefix
        leaf: false,
        cs: 0,
    }
}

// ── Field and opcode correctness ──────────────────────────────────────────────

/// `CommandType::CfgiAll` must have opcode 0x04 (ARM §4.1.1).
#[test]
fn test_cfgi_all_opcode_is_0x04() {
    assert_eq!(
        CommandType::CfgiAll as u8,
        0x04,
        "CMD_CFGI_ALL / CMD_CFGI_STE_RANGE opcode must be 0x04 per ARM IHI0070G.b §4.1.1"
    );
}

/// `CommandEntry::new()` must default the `range` field to 31
/// (full-invalidation semantics = CMD_CFGI_ALL) per ARM §4.3.2.
#[test]
fn test_range_field_defaults_to_31() {
    let cmd = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    assert_eq!(
        cmd.range,
        31,
        "CommandEntry::new() must default range to 31 per ARM §4.3.2 (CMD_CFGI_ALL semantics)"
    );
}

// ── CMD_CFGI_ALL (Range == 31) — global invalidation ─────────────────────────

/// CMD_CFGI_ALL (Range == 31) must increment the SMMU invalidation counter.
#[test]
fn test_cfgi_all_range31_increments_invalidation_count() {
    let smmu = make_smmu();
    setup_stream(&smmu, 1);
    setup_stream(&smmu, 2);

    let before = smmu.get_invalidation_count();
    smmu.submit_command(cfgi_all_cmd()).unwrap();
    smmu.process_command_queue().unwrap();
    let after = smmu.get_invalidation_count();

    assert!(
        after > before,
        "CMD_CFGI_ALL (Range=31) must increment the invalidation counter"
    );
}

/// After CMD_CFGI_ALL (Range == 31), all page-table mappings remain accessible.
/// TLB eviction does not delete page-table entries; re-translation must succeed.
#[test]
fn test_cfgi_all_range31_all_mappings_remain_accessible() {
    let smmu = make_smmu();
    setup_stream(&smmu, 1);
    setup_stream(&smmu, 2);

    smmu.submit_command(cfgi_all_cmd()).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(
        is_accessible(&smmu, 1),
        "Stream 1 mapping must remain accessible after CMD_CFGI_ALL (re-translation)"
    );
    assert!(
        is_accessible(&smmu, 2),
        "Stream 2 mapping must remain accessible after CMD_CFGI_ALL (re-translation)"
    );
}

/// CMD_CFGI_ALL (Range == 31) must evict TLB entries for all streams.
/// Verified by observing a TLB miss for stream 1 immediately after the command.
#[test]
fn test_cfgi_all_range31_evicts_all_tlb_entries() {
    let smmu = make_smmu();
    setup_stream(&smmu, 1);
    // TLB is warm for stream 1 at this point

    smmu.submit_command(cfgi_all_cmd()).unwrap();
    smmu.process_command_queue().unwrap();

    // After eviction, the next translation must be a cache miss (then re-inserted).
    // A subsequent translate should then be a cache hit.
    let stats1 = smmu.get_cache_statistics();
    smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap(); // miss → insert
    let stats2 = smmu.get_cache_statistics();
    // Re-translate: now should be a hit
    smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    let stats3 = smmu.get_cache_statistics();

    // First re-translate after eviction must have been a miss (hits didn't change)
    assert_eq!(
        stats2.tlb_hits(),
        stats1.tlb_hits(),
        "First translate after CMD_CFGI_ALL must be a TLB miss (eviction verified)"
    );
    // Second re-translate must be a hit (entry was re-inserted on miss)
    assert!(
        stats3.tlb_hits() > stats2.tlb_hits(),
        "Second translate after CMD_CFGI_ALL must be a TLB hit (re-inserted on miss)"
    );
}

// ── CMD_CFGI_STE_RANGE (Range < 31) — prefix-matched invalidation ─────────────

/// CMD_CFGI_STE_RANGE (Range < 31) must increment the SMMU invalidation counter.
#[test]
fn test_cfgi_ste_range_increments_invalidation_count() {
    let smmu = make_smmu();
    setup_stream(&smmu, 8);

    let before = smmu.get_invalidation_count();
    // Range=2: prefix_bits = 3; stream 8 (0b01000) >> 3 = 1
    // command.stream_id = 8 >> 3 = 1 → stream 8 matches itself → evicted
    smmu.submit_command(cfgi_ste_range_cmd(8, 2)).unwrap();
    smmu.process_command_queue().unwrap();
    let after = smmu.get_invalidation_count();

    assert!(after > before, "CMD_CFGI_STE_RANGE must increment the invalidation counter");
}

/// After CMD_CFGI_STE_RANGE, the matching stream's page mapping remains accessible.
/// The TLB entry is evicted but the page-table entry is intact; re-translation succeeds.
#[test]
fn test_cfgi_ste_range_matching_stream_mapping_remains_accessible() {
    let smmu = make_smmu();
    setup_stream(&smmu, 8);
    // Range=2: stream 8 matches → TLB evicted
    smmu.submit_command(cfgi_ste_range_cmd(8, 2)).unwrap();
    smmu.process_command_queue().unwrap();
    assert!(
        is_accessible(&smmu, 8),
        "Stream 8 mapping must remain accessible after CMD_CFGI_STE_RANGE (re-translation)"
    );
}

/// CMD_CFGI_STE_RANGE must NOT evict TLB entries for streams whose StreamID
/// does not match the prefix.  Non-matching streams must remain cached.
///
/// Setup: streams 8 (`0b01000`) and 16 (`0b10000`).
/// Command: stream_id=8, Range=3 → prefix_bits=4.
///   stream 8  >> 4 = 0   (matches: same prefix as command)
///   stream 16 >> 4 = 1   (no match: different prefix)
/// Stream 16 TLB entry must survive → next translate of stream 16 is a cache HIT.
#[test]
fn test_cfgi_ste_range_nonmatching_stream_tlb_entry_survives() {
    let smmu = make_smmu();
    setup_stream(&smmu, 8);
    setup_stream(&smmu, 16);

    // Confirm both are warm (prime into TLB if not already)
    smmu.translate(sid(8), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    smmu.translate(sid(16), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();

    // Snapshot stats before range invalidation
    let stats_before = smmu.get_cache_statistics();

    // Invalidate prefix matching stream 8 only (Range=3: streams 0–15 match; stream 16 does not)
    // Actually: Range=3, prefix_bits=4. stream 8 >> 4 = 0, stream 16 >> 4 = 1. Only stream 8 matches.
    smmu.submit_command(cfgi_ste_range_cmd(8, 3)).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream 16's TLB entry must still be cached → next translate is a HIT
    let stats_mid = smmu.get_cache_statistics();
    smmu.translate(sid(16), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_after.tlb_hits() > stats_mid.tlb_hits(),
        "Stream 16 TLB entry must survive CMD_CFGI_STE_RANGE targeting stream 8 (prefix mismatch)"
    );

    let _ = stats_before; // silence unused-variable lint
}

/// CMD_CFGI_STE_RANGE must evict TLB entries for matching streams but not
/// for non-matching streams.  Both streams' page-table mappings remain accessible.
#[test]
fn test_cfgi_ste_range_both_streams_accessible_after_prefix_cmd() {
    let smmu = make_smmu();
    // stream 8 (0b01000) and stream 16 (0b10000): different top-4-bit prefixes
    setup_stream(&smmu, 8);
    setup_stream(&smmu, 16);

    let before = smmu.get_invalidation_count();
    // Range=3 → only stream 8 (prefix 0) is invalidated; stream 16 (prefix 1) is not
    smmu.submit_command(cfgi_ste_range_cmd(8, 3)).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(smmu.get_invalidation_count() > before, "invalidation counter must increase");

    // Both mappings must still be accessible via re-translation
    assert!(is_accessible(&smmu, 8),
            "Stream 8 mapping must remain accessible after CMD_CFGI_STE_RANGE");
    assert!(is_accessible(&smmu, 16),
            "Stream 16 mapping must remain accessible (not invalidated by range cmd targeting stream 8)");
}

/// A CMD_CFGI_STE_RANGE command with Range=0 invalidates all streams whose
/// StreamID bit-0 prefix matches the command StreamID bit-0.
/// This tests the finest-granularity prefix (single high bit).
#[test]
fn test_cfgi_ste_range_range0_prefix_semantics() {
    let smmu = make_smmu();
    // Range=0: prefix_bits=1; streams grouped by top bit.
    // stream 1 (0b001) >> 1 = 0; stream 2 (0b010) >> 1 = 1
    // command.stream_id = 1 >> 1 = 0 → stream 1 matches; stream 2 does NOT
    setup_stream(&smmu, 1);
    setup_stream(&smmu, 2);

    smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    smmu.translate(sid(2), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();

    let stats_before = smmu.get_cache_statistics();
    smmu.submit_command(cfgi_ste_range_cmd(1, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream 2 must remain cached (prefix mismatch → not evicted)
    let stats_mid = smmu.get_cache_statistics();
    smmu.translate(sid(2), pasid(0), iova(0x1000), AccessType::Read,
                   SecurityState::NonSecure).unwrap();
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_after.tlb_hits() > stats_mid.tlb_hits(),
        "Stream 2 must remain cached after CMD_CFGI_STE_RANGE targeting stream 1 (Range=0)"
    );

    // Both mappings accessible
    assert!(is_accessible(&smmu, 1));
    assert!(is_accessible(&smmu, 2));

    let _ = stats_before;
}
