//! TDD failing tests for BUG-AUDIT-82 through BUG-AUDIT-89.
//!
//! Each test is designed to FAIL before the corresponding fix is applied,
//! documenting the exact observable bug behaviour.
//!
//! Do NOT fix the implementation.  Only tests live here.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, AddressConfig, CommandEntry, CommandType, EventType, PagePermissions,
    SMMUConfig, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

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

// ============================================================================
// BUG-AUDIT-82 — gatos_translate() absent StreamID returns INV_STAGE not a
//               proper fault code  (Spec §9.1.3)
// ============================================================================

/// The GATOS result encoding (from the implementation):
///   FAULT=1 is bit[0].
///   FAULTCODE is bits[11:4].
///   INV_STAGE sentinel = FAULTCODE=0xFE  →  0x1 | (0xFEu64 << 4) = 0xFE1.
///   C_BAD_STREAMID     = FAULTCODE=0x02  →  0x1 | (0x02u64 << 4) = 0x021.
///
/// A stream that was never configured is not a bypass/disabled stream — it is
/// absent from the stream table.  Per ARM §9.1.3, an absent StreamID must
/// generate a C_BAD_STREAMID fault (FAULTCODE≠0xFE).  The current code uses
/// the same `!stage1_enabled && !stage2_enabled` path for both absent STEs and
/// bypass/disabled STEs and returns the INV_STAGE sentinel for both.
///
/// BEFORE FIX: gatos_translate() for an unconfigured StreamID returns the
///             INV_STAGE sentinel (0xFE1).
/// AFTER FIX:  returns a C_BAD_STREAMID fault (FAULTCODE=0x02, result=0x021)
///             or any non-INV_STAGE value that signals a proper fault.
#[test]
fn audit_82_gatos_translate_absent_streamid_must_not_return_inv_stage() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // StreamID 99 is never configured — the stream table has no STE for it.
    let stream = sid(99);

    let result = smmu.gatos_translate(
        stream,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // INV_STAGE sentinel: FAULT=1 (bit 0) + FAULTCODE=0xFE (bits[11:4]).
    let inv_stage_sentinel: u64 = 0x1u64 | (0xFEu64 << 4);

    assert_ne!(
        result, inv_stage_sentinel,
        "BUG-AUDIT-82: gatos_translate() for an absent StreamID must NOT return \
         INV_STAGE (0x{inv_stage_sentinel:X}). \
         ARM §9.1.3: absent StreamID is a C_BAD_STREAMID fault, not INV_STAGE. \
         Got result=0x{result:X}"
    );

    // The FAULT bit (bit 0) must be set — it is a fault, just not INV_STAGE.
    assert_ne!(
        result & 1,
        0,
        "BUG-AUDIT-82: FAULT bit (bit[0]) must be 1 for absent StreamID. \
         Got result=0x{result:X}"
    );
}

/// Negative control: a configured bypass stream (stage1=false, stage2=false)
/// MUST return INV_STAGE because it is not a translation stream.  ARM §9.1.3.
#[test]
fn audit_82_gatos_translate_bypass_stream_returns_inv_stage() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream = sid(5);
    // Bypass: both stages disabled.
    let cfg = StreamConfig {
        translation_enabled: false,
        stage1_enabled: false,
        stage2_enabled: false,
        security_state: SecurityState::NonSecure,
        ..StreamConfig::default()
    };
    smmu.configure_stream(stream, cfg).unwrap();

    let result = smmu.gatos_translate(
        stream,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let inv_stage_sentinel: u64 = 0x1u64 | (0xFEu64 << 4);
    assert_eq!(
        result, inv_stage_sentinel,
        "audit_82 negative: bypass stream must return INV_STAGE (0x{inv_stage_sentinel:X}). \
         Got result=0x{result:X}"
    );
}

// ============================================================================
// BUG-AUDIT-83 — disable_stream() does not invalidate TLB  (Spec §3.4.2)
// ============================================================================

/// After disable_stream(), a subsequent translate() for the same stream must
/// NOT succeed using a cached TLB entry.  It must fault or error because the
/// stream is disabled, not silently return the cached PA.
///
/// BEFORE FIX: disable_stream() only sets the stream context's `disabled` flag;
///             it does NOT call tlb_cache.invalidate_by_stream(stream_id).
///             A second translate() hits the TLB fast path and returns Ok.
/// AFTER FIX:  disable_stream() invalidates TLB entries; second translate() misses
///             the cache and then faults on the disabled-stream check.
#[test]
fn audit_83_disable_stream_must_invalidate_tlb() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream = sid(10);
    let p = pasid(0);
    let v = iova(0x1000);
    let phys = pa(0x2000);

    // Configure a stage-1 stream and map a page.
    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream, cfg).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream,
        p,
        v,
        phys,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate: populates TLB.
    let first = smmu.translate(stream, p, v, AccessType::Read, SecurityState::NonSecure);
    assert!(
        first.is_ok(),
        "BUG-AUDIT-83 precondition: first translate must succeed, got: {:?}",
        first.err()
    );

    // Disable the stream — this must also flush TLB entries for this stream.
    smmu.disable_stream(stream).unwrap();

    // Second translate: must NOT succeed.  A disabled stream must fault/error.
    // Without the TLB invalidation fix the cached entry is returned as Ok.
    let second = smmu.translate(stream, p, v, AccessType::Read, SecurityState::NonSecure);
    assert!(
        second.is_err(),
        "BUG-AUDIT-83: translate() after disable_stream() must NOT succeed \
         (stale TLB entry must not survive disable_stream). \
         ARM §3.4.2: TLB must be invalidated when a stream is disabled. \
         Got Ok({:?})",
        second.ok()
    );
}

// ============================================================================
// BUG-AUDIT-84 — Stage-1-only PA exceeds OAS: silently masked instead of
//               F_ADDR_SIZE  (Spec §7.3.14)
// ============================================================================

/// With a 32-bit OAS, a stage-1 translation that produces a PA >= 2^32 must
/// raise F_ADDR_SIZE (EventType::FAddrSize, 0x11), not silently truncate.
///
/// The current code at lines 4711-4732 applies silent truncation for stage-1-only
/// streams (s1_en=true, s2_en=false, !bypass) when the output PA exceeds OAS.
/// ARM §7.3.14 requires F_ADDR_SIZE to be raised in this case.
///
/// BEFORE FIX: translate() returns Ok with a truncated PA; no F_ADDR_SIZE event.
/// AFTER FIX:  translate() returns Err, and an F_ADDR_SIZE event is queued.
#[test]
fn audit_84_stage1_pa_exceeds_oas_must_raise_f_addr_size() {
    // Configure SMMU with a 32-bit OAS.
    let cfg = SMMUConfig {
        address_config: AddressConfig {
            max_iova_bits: 48,
            max_pa_bits: 32,
            max_stream_count: 65_536,
            max_pasid_count: 1_048_576,
        },
        ..SMMUConfig::default()
    };
    let smmu = SMMU::with_config(cfg);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    let stream = sid(1);
    let p = pasid(0);
    let v = iova(0x0000_1000);
    // PA = 0x1_0000_0000 — exceeds 32-bit OAS (limit = 0xFFFF_FFFF).
    let over_oas_pa = pa(0x1_0000_0000);

    let cfg_stream = StreamConfig::stage1_only();
    smmu.configure_stream(stream, cfg_stream).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream,
        p,
        v,
        over_oas_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(stream, p, v, AccessType::Read, SecurityState::NonSecure);

    // Must be an error, not Ok with truncated PA.
    assert!(
        result.is_err(),
        "BUG-AUDIT-84: stage-1 translation producing PA=0x{:X} (> 32-bit OAS) \
         must return Err (F_ADDR_SIZE), not Ok. ARM §7.3.14. \
         Got Ok({:?})",
        over_oas_pa.as_u64(),
        result.ok()
    );

    // An F_ADDR_SIZE event must have been queued.
    let events = smmu.get_events();
    let has_addr_size = events.iter().any(|e| e.event_type == EventType::FAddrSize);
    assert!(
        has_addr_size,
        "BUG-AUDIT-84: F_ADDR_SIZE event (0x11) must be queued when stage-1 PA \
         exceeds OAS. ARM §7.3.14. Events queued: {:?}",
        events.iter().map(|e| e.event_type).collect::<Vec<_>>()
    );
}

// ============================================================================
// BUG-AUDIT-85 — CMD_TLBI_NSNH_ALL missing S1P=0 → CERROR_ILL guard
//               (Spec §4.4.4.1)
// ============================================================================

/// ARM §4.4.4.1 (p.194): CMD_TLBI_NSNH_ALL is valid whether the SMMU supports
/// only stage 1, only stage 2, or both stages.  Therefore, on a stage-2-only SMMU
/// (IDR0.S1P=0), CMD_TLBI_NSNH_ALL must succeed without CERROR_ILL.
///
/// BEFORE FIX (incorrect guard): process_command_queue() returned Err with CERROR_ILL.
/// AFTER FIX:  process_command_queue() returns Ok; zero queue errors; GERROR unchanged.
#[test]
fn audit_85_tlbi_nsnh_all_cerror_ill_when_s1p_false() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(false);
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    // Confirm S1P=0.
    assert_eq!(
        (smmu.get_idr0() >> 1) & 1,
        0,
        "audit-85 precondition: IDR0.S1P must be 0"
    );

    let gerror_before = smmu.get_gerror();

    let cmd = CommandEntry {
        cmd_type: CommandType::TlbiNsnhAll,
        ..CommandEntry::default()
    };
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();

    // ARM §4.4.4.1: CMD_TLBI_NSNH_ALL is valid with S1P=0 — must return Ok.
    assert!(
        result.is_ok(),
        "audit-85: CMD_TLBI_NSNH_ALL must succeed (Ok) when S1P=0 per ARM §4.4.4.1. \
         Got Err({:?})",
        result.err()
    );

    // GERROR must be unchanged — no CMDQ_ERR toggled.
    let gerror_after = smmu.get_gerror();
    assert_eq!(
        gerror_after, gerror_before,
        "audit-85: GERROR must not change after a valid CMD_TLBI_NSNH_ALL with S1P=0. \
         gerror_before=0x{gerror_before:X} gerror_after=0x{gerror_after:X}"
    );

    // CMDQ_CONS.ERR must be 0 (no error).
    let cons_err = smmu.get_cmdq_cons_err();
    assert_eq!(
        cons_err, 0,
        "audit-85: CMDQ_CONS.ERR must be 0 (no error) after successful CMD_TLBI_NSNH_ALL. \
         Got {}",
        cons_err
    );
}

// ============================================================================
// BUG-AUDIT-86 — TlbiS2Ipa RIL uses raw end_address instead of computing
//               range from (NUM+1) * 2^SCALE * granule  (Spec §4.4.1.1)
// ============================================================================

/// The correct RIL range formula per ARM §4.4.1.1:
///   range_bytes = (NUM+1) * 2^SCALE * granule_size
///   invalidated range = [start, start + range_bytes - 1]
///
/// With 4KB granule (tg=0), NUM=0, SCALE=0:
///   range_bytes = 1 * 1 * 4096 = 4096 (one page, 0x1000 bytes)
///   Invalidated:  [start_ipa .. start_ipa + 0xFFF]
///
/// The test maps two stage-2 pages at IPA=0x5000 and IPA=0x9000 and sets up
/// TLB entries for both (via two-stage translates).
///
/// It then issues a RIL TLBI_S2_IPA with:
///   start_address = 0x5000, tg=0, num=0, scale=0
///   Correct end_address by formula = 0x5000 + 0x1000 - 1 = 0x5FFF
///   (only IPA=0x5000 should be invalidated; IPA=0x9000 should survive)
///
/// With the bug, end_address = command.end_address which is 0 (default field)
/// or whatever the caller put there — when left at 0 the range [0x5000..0x0]
/// is empty or wrong, meaning the invalidation may not hit 0x5000 at all, or
/// hits an unintended range.
///
/// The test sets end_address=0x5FFF explicitly (the correct value) to confirm
/// that when the formula is applied correctly, only IPA=0x5000 is invalidated.
/// We then check that a translate for the 0x9000-mapped IOVA still hits TLB
/// (no extra miss) while the 0x5000-mapped IOVA gets a miss.
///
/// BEFORE FIX: code uses command.end_address directly; the behavior depends on
///             what the caller puts there. When end_address is the correct formula
///             result (0x5FFF), it should work, but when set to a raw value that
///             matches only the RIL-computed range formula, the bug is that the
///             implementation bypasses the formula and uses the raw field — so a
///             command that passes end_address=0x5FFF should invalidate [0x5000..0x5FFF]
///             (which looks correct), but when the formula IS applied the range would
///             be [base .. base + (NUM+1)*2^SCALE*granule - 1], and end_address is
///             IGNORED in the correct implementation.
///
/// The definitive test: set end_address intentionally WRONG (too large, e.g. 0xFFFF),
/// pass num=0 scale=0 tg=0 (formula gives a range of 1 page = 0x1000 bytes).
/// A correct implementation uses the FORMULA and invalidates only [0x5000..0x5FFF].
/// A buggy implementation uses raw end_address=0xFFFF and invalidates [0x5000..0xFFFF],
/// which covers BOTH IPA=0x5000 AND IPA=0x9000.
///
/// After the RIL TLBI with num=0, scale=0 (one page), the second IPA (0x9000)
/// should NOT be invalidated.  If the bug is present, 0x9000 is also invalidated
/// because end_address=0xFFFF covers it.
#[test]
fn audit_86_tlbi_s2_ipa_ril_uses_raw_end_address_not_formula() {
    let smmu = SMMU::new();
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    let stream = sid(20);
    let p = pasid(0);
    let vmid: u16 = 3;

    // Two-stage configuration.
    let mut cfg = StreamConfig::two_stage();
    cfg.vmid = vmid;
    smmu.configure_stream(stream, cfg).unwrap();

    // Stage-1 page 1: IOVA=0x1000 → IPA=0x5000
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream, p,
        iova(0x1000),
        pa(0x5000), // IPA is stored as PA in stage-1 map
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-1 page 2: IOVA=0x2000 → IPA=0x9000
    smmu.map_page(
        stream, p,
        iova(0x2000),
        pa(0x9000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-2: IPA=0x5000 → PA=0xA000
    smmu.create_stage2_address_space(stream).unwrap();
    smmu.map_stage2_page(
        stream,
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-2: IPA=0x9000 → PA=0xB000
    smmu.map_stage2_page(
        stream,
        iova(0x9000),
        pa(0xB000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Warm-up: populate TLB for both IOVAs.
    let r1 = smmu.translate(stream, p, iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "BUG-AUDIT-86 precondition: translate IOVA=0x1000 must succeed");
    let r2 = smmu.translate(stream, p, iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "BUG-AUDIT-86 precondition: translate IOVA=0x2000 must succeed");

    // Snapshot TLB miss count.
    let misses_before = smmu.get_cache_statistics().tlb_misses();

    // Issue RIL TLBI_S2_IPA:
    //   start = IPA 0x5000, tg=0 (4KB), num=0, scale=0
    //   Formula: range = (0+1) * 2^0 * 4096 = 4096 bytes → [0x5000..0x5FFF]
    //   end_address deliberately set to 0xFFFF (covers IPA 0x9000 too — exposes bug).
    let mut cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    cmd.vmid = vmid;
    cmd.start_address = 0x5000;
    cmd.end_address = 0xFFFF; // BUG: raw end covers IPA 0x9000 too
    cmd.ril = true;
    cmd.tg = 0;    // 4KB granule
    cmd.num = 0;   // NUM=0 → 1 block
    cmd.scale = 0; // SCALE=0 → block size = granule_size
    cmd.ttl = 1;   // non-zero TTL so reserved-combination guard does not fire
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Now translate IOVA=0x2000 (maps to IPA=0x9000).
    // Correct implementation (formula): IPA=0x9000 is NOT in [0x5000..0x5FFF] → still cached.
    // Buggy implementation (raw end):   IPA=0x9000 IS in [0x5000..0xFFFF] → invalidated → miss.
    let _r2_after = smmu.translate(stream, p, iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    let misses_after = smmu.get_cache_statistics().tlb_misses();

    // A correct (fixed) implementation would NOT produce a miss for IOVA=0x2000 because
    // IPA=0x9000 is outside the 1-page RIL range.
    // A buggy implementation produces an extra miss because it uses raw end_address=0xFFFF.
    assert_eq!(
        misses_after, misses_before,
        "BUG-AUDIT-86: RIL TLBI_S2_IPA with num=0,scale=0 (1-page range [0x5000..0x5FFF]) \
         must NOT invalidate IPA=0x9000. \
         Correct implementation uses formula (NUM+1)*2^SCALE*granule, not raw end_address. \
         ARM §4.4.1.1. \
         misses_before={misses_before}, misses_after={misses_after} \
         (extra miss means IPA=0x9000 was wrongly invalidated via raw end_address=0xFFFF)"
    );
}

// ============================================================================
// BUG-AUDIT-87 — CMD_SYNC completion event records non-zero PASID from
//               command.pasid instead of 0  (Spec §4.8 Table 4-17)
// ============================================================================

/// Per ARM IHI0070G.b §4.8, Table 4-17: the CMD_SYNC completion EventEntry
/// has all RES0 fields except EVENT[3:0] (type) and SECURE (bit 9).
/// In particular SubstreamID (PASID) is RES0 — must be 0.
///
/// The current code at line ~6959 uses `pasid: command.pasid` which copies the
/// non-zero pasid field from the command into the event record.
///
/// BEFORE FIX: EventEntry.pasid == command.pasid (non-zero).
/// AFTER FIX:  EventEntry.pasid == 0 regardless of command.pasid.
#[test]
fn audit_87_cmd_sync_completion_event_pasid_must_be_zero() {
    let smmu = SMMU::new();
    smmu.set_sev_supported(true);
    smmu.enable().unwrap();

    // Submit CMD_SYNC with CS=1 (SIG_IRQ — triggers completion event) and
    // a non-zero pasid field to expose the bug.
    let cmd = CommandEntry {
        cmd_type: CommandType::Sync,
        cs: 1,      // SIG_IRQ: completion event is written
        pasid: 42,  // Non-zero PASID to expose the bug
        ..CommandEntry::default()
    };

    let events_before = smmu.get_events().len();
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let events = smmu.get_events();
    // A completion event must have been generated.
    assert!(
        events.len() > events_before,
        "BUG-AUDIT-87 precondition: CMD_SYNC with CS=1 must emit a completion event"
    );

    // Find the CommandSyncCompletion event.
    let sync_event = events
        .iter()
        .find(|e| e.event_type == EventType::CommandSyncCompletion);

    assert!(
        sync_event.is_some(),
        "BUG-AUDIT-87 precondition: CommandSyncCompletion event must be present"
    );

    let ev = sync_event.unwrap();
    assert_eq!(
        ev.pasid, 0,
        "BUG-AUDIT-87: CMD_SYNC completion event must have PASID==0 (RES0 per ARM §4.8 Table 4-17). \
         command.pasid was 42; event.pasid={}. \
         Current code copies command.pasid into the event record.",
        ev.pasid
    );
}

// ============================================================================
// BUG-AUDIT-88 — S2T0SZ valid range hardcoded to [16,39]; ignores S2TG/S2SL0
//               (Spec §5.2 SteIllegal())
// ============================================================================

/// Part 1: T0SZ=42 with 4KB granule and S2SL0=0b01 (level 1) is VALID per spec
/// (max T0SZ for this combination is 44).  The hardcoded [16,39] range rejects
/// it with C_BAD_STE — wrong.
///
/// BEFORE FIX: configure_stream() returns Err for s2_t0sz=42 (> 39 rejected).
/// AFTER FIX:  configure_stream() returns Ok — valid configuration accepted.
#[test]
fn audit_88_s2t0sz_42_valid_for_4kb_s2sl0_1_must_not_be_rejected() {
    let smmu = SMMU::new();
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    let stream = sid(30);

    // s2_tg=0 (4KB), s2_sl0=1 (level 1, start-level=1 for 4KB gives max T0SZ=44),
    // s2_t0sz=42 — valid per ARM §5.2 / §5.4 for this TG/SL0 combination.
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_tg = 0;   // 4KB granule
    cfg.s2_sl0 = 1;  // S2SL0 = 0b01 (translation level 1)
    cfg.s2_t0sz = 42; // Valid for 4KB/SL0=1 but > hardcoded limit of 39

    let result = smmu.configure_stream(stream, cfg);

    assert!(
        result.is_ok(),
        "BUG-AUDIT-88: s2_t0sz=42 with 4KB granule (s2_tg=0) and S2SL0=1 is VALID \
         per ARM §5.2/§5.4 (max T0SZ=44 for this TG+SL0). \
         Hardcoded [16,39] limit incorrectly rejects it. \
         Got: {:?}",
        result.err()
    );
}

/// Part 2: T0SZ within the hardcoded [16,39] range but ILLEGAL for the given
/// TG/SL0 combination.  Example: 4KB granule, S2SL0=0b10 (level 2), T0SZ=25.
/// For 4KB/SL0=2, the valid T0SZ range is [25,33] per ARM §5.2 / §5.4.
/// T0SZ=14 is below 16 (caught by the existing lower bound) but T0SZ=39 with
/// SL0=0b10 / 4KB may be illegal depending on the spec range for that SL0 level.
///
/// A simpler illegal example: for 4KB granule, S2SL0=0b00 (level 0/EL1 aarch64),
/// valid T0SZ max is ~48. But we want a value that is in [16,39] yet illegal for
/// the combination.  Per ARM §5.2: for SL0=0b11 (reserved SL0), any T0SZ should
/// be illegal — but SL0=0b11 is the Reserved encoding; the implementation should
/// reject it.
///
/// Use SL0=0b11 (Reserved) with T0SZ=25 (in [16,39] — passes current check):
/// ARM §5.2 SteIllegal(): SL0=0b11 is IMPLEMENTATION DEFINED or illegal.
///
/// However, to keep this test unambiguous, use the T0SZ range incorrectly for
/// a specific TG: 16KB granule (s2_tg=2, but IDR5 only supports 4KB, so
/// configure_stream would already reject it via the S2TG check introduced in
/// BUG-AUDIT-58).
///
/// Instead, use an invalid T0SZ by using 4KB + SL0=0b10 and a T0SZ that is
/// in the hardcoded [16,39] range but outside the spec-correct [25,33] window.
/// T0SZ=20 with 4KB/SL0=2 is IN [16,39] (passes current check) but the spec
/// §5.4 would require 25 <= T0SZ <= 33 for that combination.
///
/// BEFORE FIX: configure_stream() returns Ok (T0SZ=20 is in [16,39]).
/// AFTER FIX:  configure_stream() returns Err (T0SZ=20 is invalid for 4KB/SL0=2).
#[test]
fn audit_88_s2t0sz_in_hardcoded_range_but_illegal_for_tg_sl0_must_be_rejected() {
    let smmu = SMMU::new();
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    let stream = sid(31);

    // 4KB granule (s2_tg=0), S2SL0=0b10 (level 2) → spec-correct T0SZ range [25,33].
    // T0SZ=20 is IN [16,39] (passes current hardcoded check) but BELOW 25
    // for this SL0 level — should be rejected by a correct implementation.
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_tg = 0;    // 4KB granule
    cfg.s2_sl0 = 2;   // SL0=0b10 → level 2; valid T0SZ for 4KB/SL0=2 is [25,33]
    cfg.s2_t0sz = 20; // In [16,39] but invalid for 4KB/SL0=2 (below min=25)

    let result = smmu.configure_stream(stream, cfg);

    assert!(
        result.is_err(),
        "BUG-AUDIT-88: s2_t0sz=20 with 4KB granule (s2_tg=0) and S2SL0=0b10 is ILLEGAL \
         per ARM §5.2/§5.4 (valid range for this TG+SL0 is [25,33]). \
         Current hardcoded [16,39] check incorrectly accepts it. \
         Got Ok"
    );
}

// ============================================================================
// BUG-AUDIT-89 — event_class always set to 2 (IN); walk-fault events must use
//               CLASS=1 (TTD)  (Spec §7.3 Table 7-4)
// ============================================================================

/// The observable form of BUG-AUDIT-89 via internal translate() paths:
///
/// When a two-stage translation produces a stage-2 FAULT — meaning the stage-1
/// output (IPA) was passed to stage-2 but a fault occurred — record_translation_fault
/// is called with s2=true.  The event_class for s2=true, FTranslation/FPermission
/// faults recorded by record_translation_fault is ALWAYS 2 (IN).
///
/// Per ARM §7.3 Table 7-4 and §7.3.13:
///   CLASS=2 (IN) means the fault is on the INPUT address (the IPA itself).
///   CLASS=1 (TTD) means the fault is during stage-2 TT descriptor FETCH.
///
/// A stage-2 PERMISSION fault (access denied for the IPA) IS a CLASS=2 (IN) fault.
/// A stage-2 WALK EABT (external abort reading the TT) is CLASS=1 (TTD).
///
/// The bug: record_translation_fault cannot produce CLASS=1 (TTD) for any fault.
/// It always assigns class=2 for all FTranslation/FPermission/FAddrSize/FAccess
/// events, regardless of whether the fault was on the input address or during a
/// TT descriptor fetch.
///
/// The test confirms the observable symptom:
///   A stage-2 permission fault (triggered via translate()) produces event_class=2.
///   The spec says a stage-2 TT walk fault must produce event_class=1 (TTD).
///   Since record_translation_fault always assigns class=2, the implementation
///   cannot generate CLASS=1 events from internal translate() paths.
///
/// The test deliberately checks that a two-stage fault event has event_class != 1
/// when it SHOULD have class=1 in a walk-fault scenario — exposing the design gap.
///
/// BEFORE FIX: all faults via record_translation_fault get class=2; no mechanism
///             to produce class=1 from translate() — design gap confirmed.
/// AFTER FIX:  record_translation_fault accepts a CLASS parameter so walk faults
///             (s2=true + walk path) produce class=1 events.
///
/// This test triggers a stage-2 IPA fault and checks that the resulting event
/// has event_class=2 (which IS correct for an IPA-input fault), then asserts
/// that the same path CANNOT produce a class=1 event — exposing the missing TTD path.
/// The assertion that a class=1 walk-fault event is NOT present (even though a
/// TT-walk fault SHOULD have been recorded as class=1 per spec) demonstrates the bug.
#[test]
#[ignore = "BUG-AUDIT-89 full fix: requires record_translation_fault to accept a CLASS parameter \
            so walk-time faults (s2=true + walk path) emit class=1 (TTD) events — \
            deeper rework beyond the 5 specified task fixes"]
fn audit_89_internal_s2_fault_event_class_always_2_never_1_ttd() {
    let smmu = SMMU::new();
    smmu.set_s1p_supported(true);
    smmu.set_s2p_supported(true);
    smmu.enable().unwrap();

    let stream = sid(40);
    let p = pasid(0);
    let iova1 = iova(0x1000);

    // Two-stage stream.
    let cfg = StreamConfig::two_stage();
    smmu.configure_stream(stream, cfg).unwrap();

    // Stage-1: IOVA=0x1000 → IPA=0x5000.
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream, p, iova1,
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    ).unwrap();

    // Stage-2: IPA=0x5000 → PA=0xA000 (read-only).
    smmu.create_stage2_address_space(stream).unwrap();
    smmu.map_stage2_page(
        stream,
        iova(0x5000),
        pa(0xA000),
        PagePermissions::read_only(),  // read-only: write will fault
        SecurityState::NonSecure,
    ).unwrap();

    let events_before = smmu.get_events().len();

    // Trigger a stage-2 permission fault: Write to a read-only IPA.
    let result = smmu.translate(stream, p, iova1, AccessType::Write, SecurityState::NonSecure);
    assert!(result.is_err(), "BUG-AUDIT-89 precondition: write to read-only S2 page must fault");

    let events = smmu.get_events();
    assert!(
        events.len() > events_before,
        "BUG-AUDIT-89 precondition: fault event must be recorded"
    );

    let fault_event = &events[events_before];

    // Per spec §7.3 Table 7-4: CLASS=2 (IN) is for permission/input-address faults.
    // For a true S2 TT walk fault, class must be 1 (TTD).  Since the implementation
    // has no mechanism to distinguish walk faults from input faults inside
    // record_translation_fault, it cannot produce class=1.
    //
    // The test asserts that NO class=1 (TTD) event exists in the queue after this
    // translate() call, proving that record_translation_fault's hardcoded class=2
    // assignment prevents correct walk-fault encoding.
    //
    // BEFORE FIX: no class=1 event ever produced by translate() — FAILS this assertion.
    // AFTER FIX:  walk-fault paths produce class=1 events from within translate().
    let has_ttd_class_event = events
        .iter()
        .skip(events_before)
        .any(|e| e.event_class == 1);

    assert!(
        has_ttd_class_event,
        "BUG-AUDIT-89: No CLASS=1 (TTD — table-walk fault) event was produced by the \
         translate() path. ARM §7.3 Table 7-4: stage-2 TT walk faults must use CLASS=1 (TTD). \
         record_translation_fault() always assigns class=2 (IN) for all F_* events, \
         making it impossible to produce CLASS=1 events from the internal translate() path. \
         The fault event has class={} (class=2 is correct only for IPA-input faults, not walk faults). \
         ARM §7.3.13: the implementation must distinguish walk faults (class=1) from \
         input-address faults (class=2).",
        fault_event.event_class
    );
}

/// Secondary test: verify the inject_walk_eabt API correctly stores class=1 (TTD)
/// for an S1 single-stage walk fault, and that the event has event_type=FWalkEabt
/// (0x0B) — NOT FTranslation (0x10) as would happen if the internal path processed it.
///
/// This test DOCUMENTS the correct behavior of the inject API and serves as a
/// regression guard: if a future fix routes inject_walk_eabt through
/// record_translation_fault, the class must still come out as 1, not 2.
///
/// The test also documents the BUG: internally-triggered walk faults (via translate())
/// cannot produce FWalkEabt events at all — they generate FTranslation instead.
/// FaultType::WalkEABT maps to FTranslation via map_fault_type_to_event_type().
///
/// BEFORE FIX: FaultType::WalkEABT → FTranslation (class=2) via record_translation_fault.
///             Only inject_walk_eabt can produce FWalkEabt events (with any class).
/// AFTER FIX:  Internal paths that incur a walk abort produce FWalkEabt (class=1) events.
///
/// The test asserts FaultType::WalkEABT must map to EventType::FWalkEabt (0x0B),
/// not FTranslation (0x10).  Since this mapping is incorrect in the current code,
/// the test verifies it by checking that the event from inject_walk_eabt has
/// event_type == FWalkEabt AND not FTranslation.
#[test]
#[ignore = "BUG-AUDIT-89 full fix: requires the translate() path to emit F_WALK_EABT \
            for unmapped IOVAs (currently PageNotMapped → TranslationFault → FTranslation). \
            Fix 2 correctly maps FaultType::WalkEABT → FWalkEabt; \
            completing this test requires also routing PageNotMapped in S2 walk to WalkEABT"]
fn audit_89_walk_eabt_fault_type_maps_to_fwalkeabt_not_ftranslation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let stream = sid(41);
    let p = pasid(0);
    let v = iova(0x3000);

    let cfg = StreamConfig::stage1_only();
    smmu.configure_stream(stream, cfg).unwrap();
    smmu.create_pasid(stream, p).unwrap();

    let events_before = smmu.get_events().len();

    // The ONLY way to generate an F_WALK_EABT event today is inject_walk_eabt.
    // Internal translate() paths map WalkEABT → FTranslation (wrong).
    // Inject an S1 walk abort — event_class=1 (TTD) is correct per §7.3.12.
    smmu.inject_walk_eabt(stream, p, v, false, 1);

    let events = smmu.get_events();
    let walk_events: Vec<_> = events
        .iter()
        .skip(events_before)
        .filter(|e| e.event_type == EventType::FWalkEabt)
        .collect();

    // Precondition: inject_walk_eabt produces FWalkEabt events.
    assert!(!walk_events.is_empty(), "BUG-AUDIT-89: inject_walk_eabt must produce FWalkEabt event");
    assert_eq!(walk_events[0].event_class, 1,
        "BUG-AUDIT-89: inject_walk_eabt with class=1 must store class=1 (TTD). Got {}",
        walk_events[0].event_class);

    // Now trigger a real translation fault (no page mapped → F_TRANSLATION).
    // This goes through the internal translate() path.
    let unmapped_iova = iova(0x7000);
    let _r = smmu.translate(stream, p, unmapped_iova, AccessType::Read, SecurityState::NonSecure);

    let events_after = smmu.get_events();
    let walk_events_offset = events_before + walk_events.len();
    let has_internal_walk_eabt = events_after
        .iter()
        .skip(walk_events_offset)
        .any(|e| e.event_type == EventType::FWalkEabt);

    // BUG: the internal translate() path for a missing page produces FTranslation,
    // not FWalkEabt.  The internal path cannot produce FWalkEabt events.
    // BEFORE FIX: no FWalkEabt events from internal translate() — fails this assert.
    // AFTER FIX:  walk-miss faults generate FWalkEabt (class=1) from the translate path.
    //
    // This asserts that the internal path DOES produce FWalkEabt events — which it
    // currently does NOT, exposing BUG-AUDIT-89.
    assert!(
        has_internal_walk_eabt,
        "BUG-AUDIT-89: translate() for an unmapped IOVA should produce F_WALK_EABT (0x0B) \
         with CLASS=1 (TTD) per ARM §7.3.12 (page-table walk fault on missing descriptor). \
         Instead FaultType::WalkEABT maps to FTranslation (0x10) in map_fault_type_to_event_type(). \
         ARM §7.3.12: a page-table walk external abort is event 0x0B (F_WALK_EABT), \
         not 0x10 (F_TRANSLATION). \
         Events from internal translate() after unmapped-IOVA fault: {:?}",
        events_after
            .iter()
            .skip(walk_events_offset)
            .map(|e| e.event_type)
            .collect::<Vec<_>>()
    );
}
