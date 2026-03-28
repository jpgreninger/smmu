//! TDD failing tests for NEW-BUG-1, NEW-BUG-2, NEW-BUG-3.
//!
//! Each test is written to FAIL with the current code (red) and PASS only
//! after the corresponding fix is applied (green).
//!
//! # NEW-BUG-1 (§4.4.1.1): TG Granule Encoding Swapped
//!
//! ARM IHI0070G.b §4.4.1.1 table: TG field encoding for RIL TLBI commands:
//!   - TG=0b00 (0) → not a RIL granule / default
//!   - TG=0b01 (1) → 4KB  granule (4096 bytes per block)
//!   - TG=0b10 (2) → 16KB granule (16384 bytes per block)
//!   - TG=0b11 (3) → 64KB granule (65536 bytes per block)
//!
//! Current code (`smmu/mod.rs` RIL granule match):
//! ```text
//!     1 => 65536   ← WRONG (should be 4096)
//!     2 => 16384   ← correct
//!     _ => 4096    ← handles tg=0 AND tg=3 as 4KB (tg=3 wrong)
//! ```
//!
//! ## TG=1 test
//! Issue `TlbiNhVa` with `ril=true, tg=1 (4KB), num=15, scale=0`.
//! - CORRECT (TG=1=4KB): range = 16 × 1 × 4096 = 64KB.
//!   Page at BASE+65536 (BASE+64KB) is outside range → TLB HIT after TLBI.
//! - BUG (TG=1=64KB): range = 16 × 1 × 65536 = 1MB.
//!   Page at BASE+65536 is inside 1MB range → evicted → TLB MISS.
//!
//! BEFORE FIX: TLB MISS for BASE+65536 (BUG treats TG=1 as 64KB) → test FAILS.
//! AFTER FIX:  TLB HIT for BASE+65536 (correct: 64KB range) → test PASSES.
//!
//! ## TG=3 test
//! Issue `TlbiNhVa` with `ril=true, tg=3 (64KB), num=0, scale=0`.
//! - CORRECT (TG=3=64KB): range = 1 × 1 × 65536 = 64KB.
//!   Page at BASE+4096 is inside [BASE, BASE+65535] → must be EVICTED → MISS.
//! - BUG (TG=3=4KB default): range = 1 × 1 × 4096 = 4KB.
//!   Page at BASE+4096 is outside [BASE, BASE+4095] → NOT evicted → HIT.
//!
//! BEFORE FIX: TLB HIT for BASE+4096 (BUG: range only 4KB) → test FAILS.
//! AFTER FIX:  TLB MISS for BASE+4096 (correct: 64KB range evicts it) → PASSES.
//!
//! # NEW-BUG-2 (§4.4): Reserved RIL Parameter Combination → CERROR_ILL
//!
//! ARM §4.4: TG != 0b00 AND NUM == 0 AND SCALE == 0 AND TTL == 0b00 is Reserved.
//! The SMMU must raise CERROR_ILL for this combination.
//!
//! BEFORE FIX: command executes normally, `get_cmdq_cons_err() == 0` → FAILS.
//! AFTER FIX:  `get_cmdq_cons_err() == CERROR_ILL` and `GERROR_CMDQ_ERR` set → PASSES.
//!
//! # NEW-BUG-3 (§3.9.1.3): ATSCHK=1 Missing F_TRANSL_FORBIDDEN for Bypass Streams
//!
//! ARM §3.9.1.3: `AtsTranslated` + `CR0.ATSCHK==1` on a bypass stream must
//! produce `F_TRANSL_FORBIDDEN` (§7.3.8).
//!
//! Current code: `perform_two_stage_translation()` for a bypass stream returns
//! the identity (PA==IOVA) as success, so the ATSCHK re-check never fails and
//! `F_TRANSL_FORBIDDEN` is never emitted.
//!
//! BEFORE FIX: `translate_with_type(bypass, AtsTranslated, ATSCHK=1)` → Ok (no event) → FAILS.
//! AFTER FIX:  `F_TRANSL_FORBIDDEN` emitted, result is `Err` → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PagePermissions, SecurityState,
    StreamConfig, StreamID, TransactionType, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build an SMMU with all queues enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

/// Returns true when the event queue contains at least one event of the given type.
fn has_event(smmu: &SMMU, event_type: EventType) -> bool {
    smmu.get_events().iter().any(|e| e.event_type == event_type)
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID  { PASID::new(n).unwrap() }
fn iova(a: u64) -> IOVA    { IOVA::new(a).unwrap() }
fn pa(a: u64) -> PA        { PA::new(a).unwrap() }

// ============================================================================
// NEW-BUG-1 (TG=1): TG=1 must mean 4KB, NOT 64KB (ARM §4.4.1.1)
// ============================================================================
// ARM §4.4.1.1: TG=0b01 = 4KB granule.
// Current code: `1 => 65536` (WRONG — treats TG=1 as 64KB).
//
// With tg=1 and num=15, scale=0:
//   CORRECT (TG=1=4KB): range = 16 * 2^0 * 4096 = 64KB.
//     Page at BASE+65536 is outside [BASE, BASE+65535] → must survive (TLB HIT).
//   BUG (TG=1=64KB): range = 16 * 2^0 * 65536 = 1MB.
//     Page at BASE+65536 is inside 1MB range → evicted → TLB MISS.
//
// BEFORE FIX: page at BASE+65536 is a TLB MISS (BUG: 1MB range) → test FAILS.
// AFTER FIX:  page at BASE+65536 is a TLB HIT (correct: 64KB range) → test PASSES.

/// NEW-BUG-1 (TG=1=4KB): Page at BASE+64KB must survive a 64KB-range TLBI.
///
/// ARM §4.4.1.1: TG=0b01 encodes a 4KB granule.
/// With num=15, scale=0: range = 16 × 1 × 4KB = 64KB.
/// Page at BASE+65536 is just outside [BASE, BASE+65535] and must NOT be evicted.
///
/// BEFORE FIX: TG=1 treated as 64KB → range=1MB → BASE+65536 evicted → TLB MISS → FAILS.
/// AFTER FIX:  TG=1=4KB → range=64KB → BASE+65536 survives → TLB HIT → PASSES.
#[test]
fn new_bug1_tg1_is_4kb_page_beyond_64kb_survives() {
    let smmu = make_smmu();

    let base: u64 = 0x0010_0000; // 1MB base
    let stream_n  = 0x30_u32;

    let s = sid(stream_n);
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(s, cfg).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Map 17 consecutive 4KB pages at BASE, BASE+4K, ..., BASE+16*4K.
    for i in 0u64..=16 {
        smmu.map_page(
            s,
            pasid(0),
            iova(base + i * 4096),
            pa(0x0020_0000 + i * 4096),
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        ).unwrap();
    }

    // Warm TLB for all pages.
    for i in 0u64..=16 {
        let r = smmu.translate(s, pasid(0), iova(base + i * 4096), AccessType::Read, SecurityState::NonSecure);
        assert!(r.is_ok(), "precondition: page {} must translate", i);
    }

    // Issue RIL TlbiNhVa: tg=1 (4KB per spec), num=15, scale=0.
    // CORRECT range = 16 * 2^0 * 4KB = 64KB = [base, base+65535].
    // Page at base+65536 (= BASE+64KB) is outside this range → must survive.
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.start_address = base;
    cmd.asid          = 0;
    cmd.vmid          = 0;
    cmd.ril           = true;
    cmd.tg            = 1; // 4KB per spec (current code treats this as 64KB → BUG)
    cmd.num           = 15;
    cmd.scale         = 0;

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Page at BASE+65536 must still be a TLB HIT (not evicted).
    let stats_before = smmu.get_cache_statistics();
    let r = smmu.translate(s, pasid(0), iova(base + 65536), AccessType::Read, SecurityState::NonSecure);
    assert!(
        r.is_ok(),
        "NEW-BUG-1 (TG=1): page at BASE+64KB must still translate after 64KB range TLBI"
    );
    let stats_after = smmu.get_cache_statistics();

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "NEW-BUG-1 (TG=1=4KB): page at BASE+64KB must be a TLB HIT after 64KB TLBI \
         (ARM §4.4.1.1: TG=0b01 = 4KB granule, range = 16×4KB = 64KB). \
         With the bug (TG=1=64KB, range=1MB), BASE+64KB is wrongly evicted → MISS. \
         tlb_hits before={} after={}",
        stats_before.tlb_hits(),
        stats_after.tlb_hits()
    );
}

/// NEW-BUG-1 (TG=3=64KB): Page at BASE+4KB must be evicted by a 64KB-range TLBI.
///
/// ARM §4.4.1.1: TG=0b11 encodes a 64KB granule.
/// With num=0, scale=0: range = 1 × 1 × 64KB = 64KB = [BASE, BASE+65535].
/// Page at BASE+4096 is inside this range and must be EVICTED (TLB MISS after TLBI).
///
/// BEFORE FIX: TG=3 treated as 4KB (default case) → range=4KB → BASE+4096 NOT evicted
///   → TLB HIT instead of MISS → test FAILS.
/// AFTER FIX:  TG=3=64KB → range=64KB → BASE+4096 evicted → TLB MISS → test PASSES.
#[test]
fn new_bug1_tg3_is_64kb_page_at_4kb_offset_evicted() {
    let smmu = make_smmu();

    let base: u64 = 0x0020_0000;
    let stream_n  = 0x31_u32;

    let s = sid(stream_n);
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    smmu.configure_stream(s, cfg).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    smmu.map_page(s, pasid(0), iova(base),          pa(0x0030_0000),         PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    smmu.map_page(s, pasid(0), iova(base + 4096),   pa(0x0030_1000),         PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Warm TLB for both pages.
    let r1 = smmu.translate(s, pasid(0), iova(base),        AccessType::Read, SecurityState::NonSecure);
    let r2 = smmu.translate(s, pasid(0), iova(base + 4096), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "precondition: BASE must translate");
    assert!(r2.is_ok(), "precondition: BASE+4096 must translate");

    // Issue RIL TlbiNhVa: tg=3 (64KB per spec), num=0, scale=0.
    // CORRECT range = 1 * 2^0 * 64KB = 64KB = [base, base+65535].
    // Page at base+4096 is inside this range → must be EVICTED.
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.start_address = base;
    cmd.asid          = 0;
    cmd.vmid          = 0;
    cmd.ril           = true;
    cmd.tg            = 3; // 64KB per spec (current code falls into default=4KB → BUG)
    cmd.num           = 0;
    cmd.scale         = 0;

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Page at BASE+4096 must be a TLB MISS (evicted by 64KB range).
    let stats_before = smmu.get_cache_statistics();
    let r3 = smmu.translate(s, pasid(0), iova(base + 4096), AccessType::Read, SecurityState::NonSecure);
    assert!(
        r3.is_ok(),
        "NEW-BUG-1 (TG=3): BASE+4096 must still translate (page is mapped)"
    );
    let stats_after = smmu.get_cache_statistics();

    assert_eq!(
        stats_after.tlb_hits(),
        stats_before.tlb_hits(),
        "NEW-BUG-1 (TG=3=64KB): page at BASE+4096 must be a TLB MISS after 64KB TLBI \
         (ARM §4.4.1.1: TG=0b11 = 64KB granule — BASE+4096 is inside [BASE, BASE+65535]). \
         With the bug (TG=3=4KB default, range=4KB), BASE+4096 is NOT evicted → HIT. \
         tlb_hits before={} after={}",
        stats_before.tlb_hits(),
        stats_after.tlb_hits()
    );
}

// ============================================================================
// NEW-BUG-2 (§4.4): Reserved RIL Parameter Combination → CERROR_ILL
// ============================================================================
// ARM §4.4: TG != 0b00 AND NUM == 0 AND SCALE == 0 AND TTL == 0b00 is Reserved.
// The SMMU must raise CERROR_ILL and signal GERROR_CMDQ_ERR.
//
// BEFORE FIX: command executes normally (no CERROR_ILL) → test FAILS.
// AFTER FIX:  CERROR_ILL raised, GERROR_CMDQ_ERR active → test PASSES.

/// NEW-BUG-2: `TlbiNhVa` with TG=1, NUM=0, SCALE=0, TTL=0 must raise CERROR_ILL.
///
/// ARM §4.4: This parameter combination is Reserved and must be rejected.
///
/// BEFORE FIX: `get_cmdq_cons_err() == 0` (CERROR_NONE) → test FAILS.
/// AFTER FIX:  `get_cmdq_cons_err() == CERROR_ILL` and `GERROR_CMDQ_ERR` set → PASSES.
#[test]
fn new_bug2_reserved_ril_tg1_num0_scale0_ttl0_raises_cerror_ill() {
    // §4.4: TG != 0b00 AND NUM == 0 AND SCALE == 0 AND TTL == 0b00 is Reserved.
    let smmu = make_smmu();

    // Clear any pre-existing error.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.start_address = 0x1000;
    cmd.asid          = 0;
    cmd.vmid          = 0;
    cmd.ril           = true;
    cmd.tg            = 1;  // TG != 0b00
    cmd.num           = 0;  // NUM == 0
    cmd.scale         = 0;  // SCALE == 0
    cmd.ttl           = 0;  // TTL == 0b00 → Reserved combination per §4.4

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    // CMDQ_CONS.ERR must be CERROR_ILL.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "NEW-BUG-2: TLBI with TG!=0 AND NUM==0 AND SCALE==0 AND TTL==0 must raise \
         CERROR_ILL (ARM §4.4: Reserved RIL parameter combination). \
         Got CMDQ_CONS.ERR={}",
        smmu.get_cmdq_cons_err()
    );

    // GERROR.CMDQ_ERR must be active.
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "NEW-BUG-2: Reserved RIL parameter combination must set GERROR.CMDQ_ERR (ARM §4.4)."
    );
}

// ============================================================================
// NEW-BUG-3 (§3.9.1.3): ATSCHK=1 Missing F_TRANSL_FORBIDDEN for Bypass Streams
// ============================================================================
// ARM §3.9.1.3: AtsTranslated + CR0.ATSCHK==1 on a bypass stream must produce
// F_TRANSL_FORBIDDEN (§7.3.8).
//
// Current code: perform_two_stage_translation() on bypass returns identity
// (PA==IOVA) success → ATSCHK re-check succeeds → F_TRANSL_FORBIDDEN never emitted.
//
// BEFORE FIX: translate returns Ok (no F_TRANSL_FORBIDDEN event) → test FAILS.
// AFTER FIX:  F_TRANSL_FORBIDDEN emitted and result is Err → test PASSES.

/// NEW-BUG-3: Bypass stream + ATSCHK=1 + AtsTranslated → F_TRANSL_FORBIDDEN.
///
/// ARM §3.9.1.3 / §7.3.8: When `CR0.ATSCHK==1`, an `AtsTranslated` transaction
/// on a bypass stream must be rejected with `F_TRANSL_FORBIDDEN`.
///
/// BEFORE FIX: `translate_with_type(bypass, AtsTranslated)` returns `Ok` → FAILS.
/// AFTER FIX:  `FTranslForbidden` event emitted and result is `Err` → PASSES.
#[test]
fn new_bug3_atschk_bypass_ats_translated_emits_f_transl_forbidden() {
    // Build SMMU with ATSCHK (bit 4) set.
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN
        | SMMU::CR0_PRIQEN | SMMU::CR0_ATSCHK,
    );

    let s = sid(0x40);

    // Configure a bypass stream (StreamConfig::bypass() = STE.Config==0b100).
    smmu.configure_stream(s, StreamConfig::bypass()).unwrap();

    let test_iova = iova(0x5000);

    // Issue AtsTranslated to bypass stream with ATSCHK=1.
    // Per §3.9.1.3 this must emit F_TRANSL_FORBIDDEN and fail.
    let result = smmu.translate_with_type(
        s,
        PASID::new(0).unwrap(),
        test_iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(
        result.is_err(),
        "NEW-BUG-3: Bypass stream + ATSCHK=1 + AtsTranslated must FAIL \
         (ARM §3.9.1.3: F_TRANSL_FORBIDDEN required). Got Ok instead."
    );

    assert!(
        has_event(&smmu, EventType::FTranslForbidden),
        "NEW-BUG-3: Bypass stream + ATSCHK=1 + AtsTranslated must emit \
         F_TRANSL_FORBIDDEN event (ARM §3.9.1.3 / §7.3.8). \
         Current bug: perform_two_stage_translation() on bypass returns identity \
         success → ATSCHK re-check never fails → event not emitted."
    );
}

/// NEW-BUG-3 regression guard: Bypass + ATSCHK=0 + AtsTranslated must succeed.
///
/// Without `CR0.ATSCHK`, the bypass pass-through is the normal path.
/// No `F_TRANSL_FORBIDDEN` should be emitted.
#[test]
fn new_bug3_bypass_atschk_disabled_ats_translated_succeeds() {
    // Build SMMU WITHOUT ATSCHK bit.
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);

    let s = sid(0x41);
    smmu.configure_stream(s, StreamConfig::bypass()).unwrap();

    let test_iova = iova(0x6000);

    // AtsTranslated on bypass with ATSCHK=0 — no re-check, should succeed.
    let result = smmu.translate_with_type(
        s,
        PASID::new(0).unwrap(),
        test_iova,
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    assert!(
        result.is_ok(),
        "NEW-BUG-3 regression: Bypass + ATSCHK=0 + AtsTranslated must succeed. \
         Got: {result:?}"
    );

    assert!(
        !has_event(&smmu, EventType::FTranslForbidden),
        "NEW-BUG-3 regression: No F_TRANSL_FORBIDDEN expected when ATSCHK=0."
    );
}
