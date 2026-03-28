//! TDD failing tests for BUG-NEW-A, BUG-NEW-C, BUG-NEW-D, BUG-NEW-E (Rust).
//!
//! Each test is written to FAIL with the current code (red) and PASS only
//! after the corresponding fix is applied (green).
//!
//! # BUG-NEW-A (§4.5.2): CMD_PRI_RESP Missing `resp` Field + No Resp==0b11 CERROR_ILL
//!
//! ARM IHI0070G.b §4.5.2: CMD_PRI_RESP has a 2-bit Resp field.
//! Resp==0b11 is Reserved/ILLEGAL → must raise CERROR_ILL + GERROR_CMDQ_ERR.
//!
//! Current code: `CommandEntry` has no `resp` field; `PriResp` handler does not
//! validate the Resp encoding.  Resp==0b11 is silently accepted (no error raised).
//!
//! BEFORE FIX: `get_cmdq_cons_err() == 0` (CERROR_NONE) → test FAILS.
//! AFTER FIX:  `get_cmdq_cons_err() == CERROR_ILL`, GERROR.CMDQ_ERR active → PASSES.
//!
//! # BUG-NEW-C (§4.4): Rust RIL Reserved Check Exempts TG=3 Without Spec Basis
//!
//! ARM IHI0070G.b §4.4: The Reserved combination (TG!=0 AND NUM==0 AND SCALE==0
//! AND TTL==0) applies to TG=3 (64KB granule) as well — no exemption for TG=3.
//!
//! Current code: `if command.tg != 0 && command.tg != 3 && ...` — explicitly
//! exempts TG=3 from the Reserved check.  TG=3 with NUM=0, SCALE=0, TTL=0 slips
//! through without raising CERROR_ILL.
//!
//! BEFORE FIX: `get_cmdq_cons_err() == 0` for TG=3 → test FAILS.
//! AFTER FIX:  `get_cmdq_cons_err() == CERROR_ILL` → test PASSES.
//!
//! # BUG-NEW-D (§4.4.2.10): TLBI_EL2_ASID Over-Invalidates EL1_EL0 Entries
//!
//! ARM §4.4.2.10: Only NS-EL2-E2H entries with matching ASID should be evicted.
//! EL1_EL0 entries with the same ASID must be preserved.
//!
//! Current code: `TlbiEl2Asid` calls `invalidate_by_asid()` which evicts ALL
//! entries with matching ASID regardless of `StreamWorld` tag.
//!
//! BEFORE FIX: EL1_EL0 entry with same ASID evicted → TLB MISS → FAILS.
//! AFTER FIX:  EL1_EL0 entry preserved → TLB HIT → PASSES.
//!
//! # BUG-NEW-E (§4.4.2.8/9): TLBI_EL2_VA/VAA Over-Invalidate EL1_EL0 Entries
//!
//! ARM §4.4.2.8/9: Only NS-EL2/EL2_E2H entries should be invalidated by VA.
//! EL1_EL0 entries at the same VA (and ASID) must be preserved.
//!
//! Current code: `TlbiEl2Va` uses `invalidate_by_va_and_asid()` and `TlbiEl2Vaa`
//! uses `invalidate_by_va()` — both evict entries regardless of StreamWorld.
//!
//! BEFORE FIX: EL1_EL0 entry at same VA evicted → TLB MISS → FAILS.
//! AFTER FIX:  EL1_EL0 entry preserved → TLB HIT → PASSES.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, SecurityState, StreamConfig,
    StreamID, StreamWorld, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid() -> PASID {
    PASID::new(0).unwrap()
}

fn iova(a: u64) -> IOVA {
    IOVA::new(a).unwrap()
}

fn pa(a: u64) -> PA {
    PA::new(a).unwrap()
}

/// Build an SMMU with all queues enabled and Hyp=1 (for EL2 TLBI commands).
fn make_smmu_with_hyp() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_hyp_supported(true);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Build a minimal SMMU (all queues enabled, no Hyp override).
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

/// Set up a stage-1 stream with ASID and STRW, map one page, warm the TLB.
/// Returns `true` if the warm-up translate succeeds.
///
/// ASID is set via `set_pasid_asid()` (the per-PASID CD.ASID API).
/// STRW is set via `StreamConfig::stage1_only().strw(strw)` builder.
fn setup_stream_and_warm(
    smmu: &SMMU,
    stream: StreamID,
    asid: u16,
    strw: StreamWorld,
    test_iova: IOVA,
    test_pa: PA,
) -> bool {
    // Build stage1-only config with the requested STRW.
    // Note: STRW validation runs only for NS streams; EL2/EL2_E2H require S2P=1 OR
    // stage1_enabled=true in an NS security state to pass the configureStream check.
    // We use a plain stage1_only() config (NS, stage1_enabled=true) which allows
    // EL1_EL0 and EL2_E2H for NS security state.
    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = strw;

    if smmu.configure_stream(stream, cfg).is_err() {
        return false;
    }
    let p = pasid();
    if smmu.create_pasid(stream, p).is_err() {
        return false;
    }
    // Set the CD.ASID for PASID 0 so TLB entries are tagged with the correct ASID.
    if smmu.set_pasid_asid(stream, p, asid).is_err() {
        return false;
    }
    if smmu
        .map_page(
            stream,
            p,
            test_iova,
            test_pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .is_err()
    {
        return false;
    }

    // Warm TLB.
    smmu.translate(stream, p, test_iova, AccessType::Read, SecurityState::NonSecure)
        .is_ok()
}

// ============================================================================
// BUG-NEW-A: CMD_PRI_RESP Resp==0b11 must raise CERROR_ILL (ARM §4.5.2)
// ============================================================================
//
// ARM §4.5.2: The 2-bit Resp field encodes the response type.
// Resp==0b11 is Reserved/ILLEGAL → must raise CERROR_ILL + GERROR_CMDQ_ERR.
//
// Current code: CommandEntry has no `resp` field; PriResp handler does not
// check the Resp encoding.  Resp==0b11 is silently accepted.
//
// The test encodes Resp==0b11 in the low 2 bits of the `flags` field of
// CommandEntry (as a carrier until the dedicated `resp` field is added).
// The fix must extract bits[1:0] of `flags` as the Resp value and reject 0b11.
//
// BEFORE FIX: `get_cmdq_cons_err() == 0` (CERROR_NONE) → test FAILS.
// AFTER FIX:  `get_cmdq_cons_err() == CERROR_ILL`, GERROR.CMDQ_ERR → PASSES.

/// BUG-NEW-A: CMD_PRI_RESP with Resp==0b11 must raise CERROR_ILL (§4.5.2).
///
/// ARM §4.5.2: The Resp field encodes the Page Request response type.
/// Encoding 0b11 is Reserved → CERROR_ILL and GERROR.CMDQ_ERR must be signalled.
///
/// BEFORE FIX: command silently accepted, `get_cmdq_cons_err() == 0` → FAILS.
/// AFTER FIX:  `get_cmdq_cons_err() == CERROR_ILL`, `GERROR_CMDQ_ERR` active → PASSES.
#[test]
fn bug_new_a_pri_resp_resp_0b11_raises_cerror_ill() {
    // §4.5.2: CMD_PRI_RESP Resp==0b11 is Reserved → CERROR_ILL.
    let smmu = make_smmu();

    // Clear any pre-existing error state.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    // Build a CMD_PRI_RESP with Resp==0b11 encoded in flags bits[1:0].
    // The fix must: (a) add a `resp` field to CommandEntry, OR
    //              (b) extract bits[1:0] of `flags` as the Resp encoding.
    // Either way: Resp==0b11 must be rejected with CERROR_ILL.
    let mut cmd = CommandEntry::new(CommandType::PriResp, 0, 0);
    // Encode Resp==0b11 in the low 2 bits of flags.
    cmd.flags = 0x03;   // bits[1:0] = 0b11 (Reserved Resp per §4.5.2)

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-A: CMD_PRI_RESP with Resp==0b11 must raise CERROR_ILL \
         (ARM §4.5.2: Reserved Resp encoding). \
         Got CMDQ_CONS.ERR={}",
        smmu.get_cmdq_cons_err()
    );

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-A: CMD_PRI_RESP Resp==0b11 must set GERROR.CMDQ_ERR (ARM §4.5.2)."
    );
}

// ============================================================================
// BUG-NEW-C: Rust RIL Reserved Check Exempts TG=3 Without Spec Basis (§4.4)
// ============================================================================
//
// ARM §4.4: The Reserved combination TG!=0 AND NUM==0 AND SCALE==0 AND TTL==0
// must raise CERROR_ILL for ALL non-zero TG values including TG=3 (64KB).
//
// Current code: `if command.tg != 0 && command.tg != 3 && ...` — TG=3 is
// explicitly exempt, so the Reserved check never fires for TG=3.
//
// BEFORE FIX: get_cmdq_cons_err() == 0 (CERROR_NONE) for TG=3 → FAILS.
// AFTER FIX:  get_cmdq_cons_err() == CERROR_ILL for TG=3 → PASSES.

/// BUG-NEW-C: `TlbiNhVa` with TG=3 (64KB), NUM=0, SCALE=0, TTL=0 must raise CERROR_ILL.
///
/// ARM §4.4: This combination is Reserved for ALL non-zero TG values.
/// The Rust code wrongly exempts TG=3 with `command.tg != 3` in the guard.
///
/// BEFORE FIX: TG=3 passes without error → `get_cmdq_cons_err() == 0` → FAILS.
/// AFTER FIX:  CERROR_ILL raised, GERROR.CMDQ_ERR active → PASSES.
#[test]
fn bug_new_c_reserved_ril_tg3_num0_scale0_ttl0_raises_cerror_ill() {
    // §4.4: TG=3 (64KB), NUM=0, SCALE=0, TTL=0 with ril=true is Reserved → CERROR_ILL.
    let smmu = make_smmu();

    // Clear any pre-existing error state.
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0, 0);
    cmd.start_address = 0x1000;
    cmd.asid          = 0;
    cmd.vmid          = 0;
    cmd.ril           = true;
    cmd.tg            = 3;  // TG=3 (64KB) — Reserved when NUM=0, SCALE=0, TTL=0
    cmd.num           = 0;  // NUM == 0
    cmd.scale         = 0;  // SCALE == 0
    cmd.ttl           = 0;  // TTL == 0b00 → Reserved combination per §4.4

    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-C: TlbiNhVa with TG=3 (64KB) AND NUM==0 AND SCALE==0 AND TTL==0 \
         must raise CERROR_ILL (ARM §4.4: Reserved RIL parameter combination). \
         Current Rust code exempts TG=3 via `command.tg != 3` in the guard. \
         Got CMDQ_CONS.ERR={}",
        smmu.get_cmdq_cons_err()
    );

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-C: TlbiNhVa TG=3 Reserved RIL combination must set GERROR.CMDQ_ERR (ARM §4.4)."
    );
}

// ============================================================================
// BUG-NEW-D (1 of 2): TlbiEl2Asid must NOT invalidate EL1_EL0 entries (§4.4.2.10)
// ============================================================================
//
// ARM §4.4.2.10: CMD_TLBI_EL2_ASID invalidates NS-EL2-E2H entries matching ASID.
// EL1_EL0 entries with the same ASID must NOT be evicted.
//
// Current code: `TlbiEl2Asid` calls `invalidate_by_asid(command.asid)` which
// evicts ALL entries with that ASID regardless of StreamWorld tag.
//
// Setup:
//   Stream A: STRW=El2E2h, ASID=5 — page at 0x5000. TLB warmed.
//   Stream B: STRW=El1El0, ASID=5 — page at 0x6000. TLB warmed.
// Issue TlbiEl2Asid(ASID=5).
//   Stream A evicted (correct). Stream B preserved (BUG: currently also evicted).
//
// BEFORE FIX: Stream B TLB MISS after invalidation → FAILS.
// AFTER FIX:  Stream B TLB HIT → PASSES.

/// BUG-NEW-D: `TlbiEl2Asid` must preserve EL1_EL0 entries with same ASID (§4.4.2.10).
///
/// ARM §4.4.2.10: Only NS-EL2-E2H entries should be evicted.
/// Current code: `invalidate_by_asid()` over-invalidates across StreamWorld.
///
/// BEFORE FIX: EL1_EL0 entry at same ASID evicted → TLB MISS → FAILS.
/// AFTER FIX:  EL1_EL0 entry preserved → TLB HIT → PASSES.
#[test]
fn bug_new_d_tlbi_el2_asid_preserves_el1_el0_entries() {
    // §4.4.2.10: TlbiEl2Asid must preserve El1El0 entries with same ASID.
    let smmu = make_smmu_with_hyp();

    let stream_a = sid(50); // STRW=El2E2h
    let stream_b = sid(51); // STRW=El1El0
    let k_asid: u16 = 5;
    let iova_a = iova(0x5000);
    let iova_b = iova(0x6000);

    assert!(
        setup_stream_and_warm(&smmu, stream_a, k_asid, StreamWorld::El2E2h, iova_a, pa(0x15000)),
        "precondition: Stream A (El2E2h) page must translate"
    );
    assert!(
        setup_stream_and_warm(&smmu, stream_b, k_asid, StreamWorld::El1El0, iova_b, pa(0x16000)),
        "precondition: Stream B (El1El0) page must translate"
    );

    // Issue TlbiEl2Asid with ASID=5.
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Asid, 0, 0);
    cmd.asid = k_asid;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream B's El1El0 entry must still be in the TLB (TLB HIT).
    let stats_before = smmu.get_cache_statistics();
    let r_b = smmu.translate(stream_b, pasid(), iova_b, AccessType::Read, SecurityState::NonSecure);
    let stats_after = smmu.get_cache_statistics();

    assert!(
        r_b.is_ok(),
        "BUG-NEW-D: Stream B (El1El0) page must still translate after TlbiEl2Asid"
    );

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "BUG-NEW-D: TlbiEl2Asid must NOT invalidate El1El0 entries with same ASID \
         (ARM §4.4.2.10: only NS-EL2-E2H entries should be evicted). \
         Current code uses invalidate_by_asid() which over-invalidates across StreamWorld. \
         tlb_hits before={} after={}",
        stats_before.tlb_hits(),
        stats_after.tlb_hits()
    );
}

/// BUG-NEW-D regression: `TlbiEl2Asid` must NOT evict El1El0 entries with a DIFFERENT ASID.
///
/// Regression guard: an EL1_EL0 stream with a different ASID must be completely unaffected.
/// This test should pass both before and after the fix.
#[test]
fn bug_new_d_tlbi_el2_asid_preserves_el1_el0_different_asid() {
    // Regression: El1El0 entry with a different ASID must NOT be evicted.
    let smmu = make_smmu_with_hyp();

    let stream_c = sid(53); // STRW=El1El0, ASID=99 (different ASID)
    let k_asid_other: u16 = 99;
    let iova_c = iova(0x8000);

    assert!(
        setup_stream_and_warm(&smmu, stream_c, k_asid_other, StreamWorld::El1El0, iova_c, pa(0x18000)),
        "precondition: Stream C (El1El0, ASID=99) page must translate"
    );

    // Issue TlbiEl2Asid with a DIFFERENT ASID (6).
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Asid, 0, 0);
    cmd.asid = 6; // different from k_asid_other=99
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream C's El1El0 entry with ASID=99 must NOT be evicted (different ASID).
    let stats_before = smmu.get_cache_statistics();
    let r_c = smmu.translate(stream_c, pasid(), iova_c, AccessType::Read, SecurityState::NonSecure);
    let stats_after = smmu.get_cache_statistics();

    assert!(
        r_c.is_ok(),
        "Regression: Stream C (El1El0, ASID=99) page must still translate"
    );

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "Regression: El1El0 entry with a DIFFERENT ASID (99) must NOT be evicted \
         by TlbiEl2Asid(ASID=6). \
         tlb_hits before={} after={}",
        stats_before.tlb_hits(),
        stats_after.tlb_hits()
    );
}

// ============================================================================
// BUG-NEW-E (TlbiEl2Va): must NOT invalidate EL1_EL0 entries (§4.4.2.8)
// ============================================================================
//
// ARM §4.4.2.8: CMD_TLBI_EL2_VA invalidates NS-EL2/EL2_E2H entries by VA+ASID.
// EL1_EL0 entries with the same VA+ASID must be preserved.
//
// Current code: `TlbiEl2Va` calls `invalidate_by_va_and_asid()` which evicts
// ALL entries with that VA+ASID regardless of StreamWorld.
//
// Setup:
//   Stream A: STRW=El2E2h, ASID=7 — page at 0x1000. TLB warmed.
//   Stream B: STRW=El1El0, ASID=7 — page at 0x1000. TLB warmed.
// Issue TlbiEl2Va(VA=0x1000, ASID=7).
//   Stream A evicted (correct). Stream B preserved (BUG: also evicted).
//
// BEFORE FIX: Stream B also evicted → TLB MISS → FAILS.
// AFTER FIX:  Stream B preserved → TLB HIT → PASSES.

/// BUG-NEW-E (TlbiEl2Va): EL1_EL0 entries with same VA+ASID must be preserved.
///
/// ARM §4.4.2.8: Only NS-EL2/EL2_E2H entries should be evicted by VA+ASID.
/// Current code: `invalidate_by_va_and_asid()` evicts across StreamWorld.
///
/// BEFORE FIX: El1El0 entry evicted → TLB MISS → FAILS.
/// AFTER FIX:  El1El0 entry preserved → TLB HIT → PASSES.
#[test]
fn bug_new_e_tlbi_el2_va_preserves_el1_el0_entries() {
    // §4.4.2.8: TlbiEl2Va must preserve El1El0 entries with same VA+ASID.
    let smmu = make_smmu_with_hyp();

    let stream_a = sid(60); // STRW=El2E2h
    let stream_b = sid(61); // STRW=El1El0
    let k_asid: u16 = 7;
    let k_iova = iova(0x1000);

    assert!(
        setup_stream_and_warm(&smmu, stream_a, k_asid, StreamWorld::El2E2h, k_iova, pa(0x20000)),
        "precondition: Stream A (El2E2h) page must translate"
    );
    assert!(
        setup_stream_and_warm(&smmu, stream_b, k_asid, StreamWorld::El1El0, k_iova, pa(0x30000)),
        "precondition: Stream B (El1El0) page must translate"
    );

    // Issue TlbiEl2Va targeting the shared IOVA and ASID.
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Va, 0, 0);
    cmd.start_address = 0x1000;
    cmd.asid          = k_asid;
    cmd.ril           = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream B's El1El0 entry must still be in the TLB (TLB HIT).
    let stats_before = smmu.get_cache_statistics();
    let r_b = smmu.translate(stream_b, pasid(), k_iova, AccessType::Read, SecurityState::NonSecure);
    let stats_after = smmu.get_cache_statistics();

    assert!(
        r_b.is_ok(),
        "BUG-NEW-E (TlbiEl2Va): Stream B (El1El0) page must still translate"
    );

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "BUG-NEW-E: TlbiEl2Va must NOT invalidate El1El0 entries \
         (ARM §4.4.2.8: only NS-EL2/El2E2h entries should be evicted by VA+ASID). \
         Current code uses invalidate_by_va_and_asid() which evicts across StreamWorld. \
         tlb_hits before={} after={}",
        stats_before.tlb_hits(),
        stats_after.tlb_hits()
    );
}

// ============================================================================
// BUG-NEW-E (TlbiEl2Vaa): must NOT invalidate EL1_EL0 entries (§4.4.2.9)
// ============================================================================
//
// ARM §4.4.2.9: CMD_TLBI_EL2_VAA invalidates NS-EL2/EL2_E2H entries by VA
// (any ASID).  EL1_EL0 entries at the same VA must be preserved.
//
// Current code: `TlbiEl2Vaa` calls `invalidate_by_va()` which evicts ALL
// entries at that VA regardless of StreamWorld.
//
// BEFORE FIX: El1El0 entry at same VA evicted → TLB MISS → FAILS.
// AFTER FIX:  El1El0 entry preserved → TLB HIT → PASSES.

/// BUG-NEW-E (TlbiEl2Vaa): EL1_EL0 entries with same VA must be preserved.
///
/// ARM §4.4.2.9: Only NS-EL2/EL2_E2H entries should be evicted by VAA (any ASID).
/// Current code: `invalidate_by_va()` evicts entries regardless of StreamWorld.
///
/// BEFORE FIX: El1El0 entry at same VA evicted → TLB MISS → FAILS.
/// AFTER FIX:  El1El0 entry preserved → TLB HIT → PASSES.
#[test]
fn bug_new_e_tlbi_el2_vaa_preserves_el1_el0_entries() {
    // §4.4.2.9: TlbiEl2Vaa must preserve El1El0 entries with same VA (any ASID).
    let smmu = make_smmu_with_hyp();

    let stream_a = sid(70); // STRW=El2E2h
    let stream_b = sid(71); // STRW=El1El0
    let k_asid_a: u16 = 10;
    let k_asid_b: u16 = 11;
    let k_iova = iova(0x2000);

    assert!(
        setup_stream_and_warm(&smmu, stream_a, k_asid_a, StreamWorld::El2E2h, k_iova, pa(0x40000)),
        "precondition: Stream A (El2E2h) page must translate"
    );
    assert!(
        setup_stream_and_warm(&smmu, stream_b, k_asid_b, StreamWorld::El1El0, k_iova, pa(0x50000)),
        "precondition: Stream B (El1El0) page must translate"
    );

    // Issue TlbiEl2Vaa targeting the shared IOVA (no ASID operand — VAA = all ASIDs).
    let mut cmd = CommandEntry::new(CommandType::TlbiEl2Vaa, 0, 0);
    cmd.start_address = 0x2000;
    cmd.ril           = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Stream B's El1El0 entry must still be in the TLB (TLB HIT).
    let stats_before = smmu.get_cache_statistics();
    let r_b = smmu.translate(stream_b, pasid(), k_iova, AccessType::Read, SecurityState::NonSecure);
    let stats_after = smmu.get_cache_statistics();

    assert!(
        r_b.is_ok(),
        "BUG-NEW-E (TlbiEl2Vaa): Stream B (El1El0) page must still translate"
    );

    assert!(
        stats_after.tlb_hits() > stats_before.tlb_hits(),
        "BUG-NEW-E: TlbiEl2Vaa must NOT invalidate El1El0 entries \
         (ARM §4.4.2.9: only NS-EL2/El2E2h entries should be evicted by VAA). \
         Current code uses invalidate_by_va() which evicts across StreamWorld. \
         tlb_hits before={} after={}",
        stats_before.tlb_hits(),
        stats_after.tlb_hits()
    );
}
