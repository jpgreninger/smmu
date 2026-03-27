//! Regression tests for BUG-NEW-37, BUG-NEW-38, BUG-NEW-39.
//!
//! Each test is written to FAIL with the current code (red) and PASS only after
//! the corresponding fix is applied (green).
//!
//! # BUG-NEW-37 (§4.4.2.3/§4.4.2.4) — TlbiNhVa and TlbiNhVaa missing VMID scope
//!
//! ARM §4.4.2.3: CMD_TLBI_NH_VAA is a VMID-scoped VAA invalidation.
//! ARM §4.4.2.4: CMD_TLBI_NH_VA is a VMID-scoped VA+ASID invalidation.
//!
//! Current code: `TlbiNhVa` calls `invalidate_by_va_and_asid(va, asid)` and
//! `TlbiNhVaa` calls `invalidate_by_va(va)` — neither checks the VMID field,
//! so a TLBI for VMID-A incorrectly evicts VMID-B entries with the same VA+ASID.
//!
//! BEFORE FIX: TLBI(VMID-A) evicts VMID-B TLB entries → second translate is a
//!   page-table walk (miss) instead of a TLB hit → test FAILS (red).
//! AFTER FIX:  TLBI(VMID-A) only evicts VMID-A entries; VMID-B entries survive
//!   → second translate is still a TLB hit → test PASSES (green).
//!
//! # BUG-NEW-38 (§4.1.6) — SSec==1 guard missing for 8 NS-queue commands
//!
//! ARM §4.1.6: "Any command with SSec=1 submitted to the Non-secure Command queue
//! is illegal and must cause CERROR_ILL."
//!
//! Eight commands currently lack the SSec guard:
//!   TlbiNhAll, TlbiNhAsid, TlbiNhVa, TlbiNhVaa,
//!   TlbiS12Vmall, TlbiS2Ipa, TlbiNsnhAll, AtcInv.
//!
//! BEFORE FIX: commands execute normally → CMDQ_ERR not set → tests FAIL (red).
//! AFTER FIX:  CERROR_ILL raised → CMDQ_ERR toggled → tests PASS (green).
//!
//! # BUG-NEW-39 (§4.4.3.1/§4.4.3.2) — TlbiS12Vmall and TlbiS2Ipa missing IDR0.S2P==0 guard
//!
//! ARM §4.4.3.1/§4.4.3.2: CMD_TLBI_S12_VMALL and CMD_TLBI_S2_IPA are stage-2
//! invalidation commands.  When IDR0.S2P=0 (stage-2 translation not supported),
//! these commands must cause CERROR_ILL.
//!
//! BEFORE FIX: no S2P guard exists → commands execute normally → CMDQ_ERR not set
//!   → tests FAIL (red).
//! AFTER FIX:  set_s2p_supported(false) → S2P bit cleared in IDR0 → guard fires
//!   → CERROR_ILL raised → tests PASS (green).

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

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID  { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }
fn pa(addr: u64) -> PA     { PA::new(addr).unwrap() }

/// Set up a stage-1 stream with a given VMID, ASID, one mapped page, and warm the TLB.
fn setup_stream_and_warm_tlb(
    smmu: &SMMU,
    stream_n: u32,
    vmid: u16,
    asid: u16,
    iova_addr: u64,
    pa_addr: u64,
) {
    let s = sid(stream_n);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
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
    let result = smmu.translate(s, pasid(0), iova(iova_addr), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "setup: initial translate for stream {} must succeed", stream_n);
}

// ============================================================================
// BUG-NEW-37 — TlbiNhVa missing VMID scope
// ============================================================================
// ARM §4.4.2.3: CMD_TLBI_NH_VA invalidates EL1_EL0 stage-1 entries by
// VA+ASID scoped to the VMID field in the command.  Entries with a different
// VMID must NOT be evicted.
//
// BEFORE FIX: invalidate_by_va_and_asid(va, asid) evicts ALL entries with
//   that VA+ASID regardless of VMID → VMID-B TLB entry is wrongly evicted →
//   second translate is a miss → assert on tlb_hits FAILS (red).
// AFTER FIX: invalidate_by_vmid_and_va_and_asid(vmid, va, asid) only evicts
//   VMID-A entries; VMID-B entry survives → second translate is a hit → PASSES.

/// BUG-NEW-37 primary: `TlbiNhVa` scoped to VMID-A must NOT evict a TLB entry
/// belonging to VMID-B (same VA and ASID, different VMID).
///
/// ARM §4.4.2.3: TLBI_NH_VA is VMID-scoped VA+ASID invalidation.
#[test]
fn bug_new37_tlbi_nh_va_vmid_scope_preserves_other_vmid() {
    let smmu = make_smmu();

    // Stream 0x10: VMID=1, ASID=5, page at VA=0x1000.
    setup_stream_and_warm_tlb(&smmu, 0x10, 1, 5, 0x1000, 0x2000);

    // Stream 0x11: VMID=2, ASID=5, page at VA=0x1000 (same VA+ASID, different VMID).
    setup_stream_and_warm_tlb(&smmu, 0x11, 2, 5, 0x1000, 0x3000);

    // Confirm VMID=2 TLB entry is cached (second translate is a hit).
    let stats_before_inv = smmu.get_cache_statistics();
    let _ = smmu.translate(sid(0x11), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    let stats_after_second = smmu.get_cache_statistics();
    assert!(
        stats_after_second.tlb_hits() > stats_before_inv.tlb_hits(),
        "BUG-NEW-37 setup: VMID=2 entry must be cached before invalidation"
    );

    // Issue TlbiNhVa for VMID=1, VA=0x1000, ASID=5.
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0x1000, 0);
    cmd.vmid = 1;
    cmd.asid = 5;
    cmd.ril = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VMID=2 translation must still be a TLB hit (entry was NOT evicted).
    let stats_before_vmid2 = smmu.get_cache_statistics();
    let r = smmu.translate(sid(0x11), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "BUG-NEW-37: VMID=2 translate after TlbiNhVa(VMID=1) must succeed");
    let stats_after_vmid2 = smmu.get_cache_statistics();
    assert!(
        stats_after_vmid2.tlb_hits() > stats_before_vmid2.tlb_hits(),
        "BUG-NEW-37: TlbiNhVa(VMID=1) must NOT evict VMID=2 entries \
         (hits before={}, after={})",
        stats_before_vmid2.tlb_hits(),
        stats_after_vmid2.tlb_hits()
    );
}

// ============================================================================
// BUG-NEW-37 — TlbiNhVaa missing VMID scope
// ============================================================================
// ARM §4.4.2.4: CMD_TLBI_NH_VAA invalidates EL1_EL0 stage-1 entries by VA
// (any ASID) scoped to the VMID field in the command.  Entries with a
// different VMID must NOT be evicted.
//
// BEFORE FIX: invalidate_by_va(va) evicts ALL entries at that VA regardless
//   of VMID → VMID-B entry is evicted → second translate is a miss → FAILS.
// AFTER FIX: invalidate_by_vmid_and_va(vmid, va) only evicts VMID-A entries;
//   VMID-B entry survives → second translate is a TLB hit → PASSES.

/// BUG-NEW-37 secondary: `TlbiNhVaa` scoped to VMID-A must NOT evict a TLB
/// entry belonging to VMID-B at the same VA (any ASID).
///
/// ARM §4.4.2.4: TLBI_NH_VAA is VMID-scoped VAA (any ASID) invalidation.
#[test]
fn bug_new37_tlbi_nh_vaa_vmid_scope_preserves_other_vmid() {
    let smmu = make_smmu();

    // Stream 0x20: VMID=3, page at VA=0x4000.
    setup_stream_and_warm_tlb(&smmu, 0x20, 3, 7, 0x4000, 0x5000);

    // Stream 0x21: VMID=4, page at VA=0x4000 (same VA, different VMID).
    setup_stream_and_warm_tlb(&smmu, 0x21, 4, 7, 0x4000, 0x6000);

    // Confirm VMID=4 TLB entry is cached.
    let stats_before_inv = smmu.get_cache_statistics();
    let _ = smmu.translate(sid(0x21), pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure);
    let stats_after_second = smmu.get_cache_statistics();
    assert!(
        stats_after_second.tlb_hits() > stats_before_inv.tlb_hits(),
        "BUG-NEW-37 setup: VMID=4 entry must be cached before invalidation"
    );

    // Issue TlbiNhVaa for VMID=3, VA=0x4000.
    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0x4000, 0);
    cmd.vmid = 3;
    cmd.ril = false;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // VMID=4 translation must still be a TLB hit.
    let stats_before_vmid4 = smmu.get_cache_statistics();
    let r = smmu.translate(sid(0x21), pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure);
    assert!(r.is_ok(), "BUG-NEW-37: VMID=4 translate after TlbiNhVaa(VMID=3) must succeed");
    let stats_after_vmid4 = smmu.get_cache_statistics();
    assert!(
        stats_after_vmid4.tlb_hits() > stats_before_vmid4.tlb_hits(),
        "BUG-NEW-37: TlbiNhVaa(VMID=3) must NOT evict VMID=4 entries \
         (hits before={}, after={})",
        stats_before_vmid4.tlb_hits(),
        stats_after_vmid4.tlb_hits()
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiNhAll
// ============================================================================

/// BUG-NEW-38: `TlbiNhAll` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: "Any command with SSec=1 on the Non-secure Command queue causes CERROR_ILL."
///
/// BEFORE FIX: TlbiNhAll executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_nh_all_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhAll, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiNhAll ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiNhAll ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiNhAsid
// ============================================================================

/// BUG-NEW-38: `TlbiNhAsid` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: TlbiNhAsid executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_nh_asid_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhAsid, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiNhAsid ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiNhAsid ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiNhVa
// ============================================================================

/// BUG-NEW-38: `TlbiNhVa` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: TlbiNhVa executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_nh_va_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVa, 0x1000, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiNhVa ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiNhVa ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiNhVaa
// ============================================================================

/// BUG-NEW-38: `TlbiNhVaa` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: TlbiNhVaa executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_nh_vaa_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiNhVaa, 0x1000, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiNhVaa ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiNhVaa ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiS12Vmall
// ============================================================================

/// BUG-NEW-38: `TlbiS12Vmall` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: TlbiS12Vmall executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_s12_vmall_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiS12Vmall ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiS12Vmall ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiS2Ipa
// ============================================================================

/// BUG-NEW-38: `TlbiS2Ipa` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: TlbiS2Ipa executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_s2_ipa_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiS2Ipa ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiS2Ipa ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for TlbiNsnhAll
// ============================================================================

/// BUG-NEW-38: `TlbiNsnhAll` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: TlbiNsnhAll executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_tlbi_nsnh_all_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiNsnhAll, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: TlbiNsnhAll ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: TlbiNsnhAll ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-38 — SSec==1 guard for AtcInv
// ============================================================================

/// BUG-NEW-38: `AtcInv` with `ssec=true` on NS queue must raise CERROR_ILL.
///
/// ARM §4.1.6: SSec=1 on NS queue is always illegal.
///
/// BEFORE FIX: AtcInv executes normally → CMDQ_ERR not set → test FAILS.
/// AFTER FIX:  ssec guard fires → CERROR_ILL → CMDQ_ERR toggled → PASSES.
#[test]
fn bug_new38_atc_inv_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::AtcInv, 0, 0x1000);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-38: AtcInv ssec=1 must write CERROR_ILL (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-38: AtcInv ssec=1 must set GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

// ============================================================================
// BUG-NEW-39 — TlbiS12Vmall S2P guard
// ============================================================================
// ARM §4.4.3.1: CMD_TLBI_S12_VMALL is a stage-2 invalidation command.
// When IDR0.S2P=0, stage-2 translation is not supported; the command must
// cause CERROR_ILL.
//
// BEFORE FIX: no S2P guard → TlbiS12Vmall executes normally → CMDQ_ERR not
//   set → test FAILS (red).
// AFTER FIX: set_s2p_supported(false) clears IDR0.S2P; guard fires →
//   CERROR_ILL raised → CMDQ_ERR toggled → test PASSES (green).

/// BUG-NEW-39 primary: `TlbiS12Vmall` when IDR0.S2P=0 must raise CERROR_ILL.
///
/// ARM §4.4.3.1: CMD_TLBI_S12_VMALL causes CERROR_ILL when IDR0.S2P=0.
#[test]
fn bug_new39_tlbi_s12_vmall_s2p0_cerror_ill() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(false);

    let cmd = CommandEntry::new(CommandType::TlbiS12Vmall, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-39: TlbiS12Vmall with IDR0.S2P=0 must write CERROR_ILL \
         (ARM §4.4.3.1). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-39: TlbiS12Vmall with IDR0.S2P=0 must set GERROR.CMDQ_ERR \
         (ARM §4.4.3.1)"
    );
}

// ============================================================================
// BUG-NEW-39 — TlbiS2Ipa S2P guard
// ============================================================================
// ARM §4.4.3.2: CMD_TLBI_S2_IPA is a stage-2 invalidation command.
// When IDR0.S2P=0, stage-2 translation is not supported; the command must
// cause CERROR_ILL.
//
// BEFORE FIX: no S2P guard → TlbiS2Ipa executes normally → CMDQ_ERR not set
//   → test FAILS (red).
// AFTER FIX: set_s2p_supported(false) clears IDR0.S2P; guard fires →
//   CERROR_ILL raised → CMDQ_ERR toggled → test PASSES (green).

/// BUG-NEW-39 secondary: `TlbiS2Ipa` when IDR0.S2P=0 must raise CERROR_ILL.
///
/// ARM §4.4.3.2: CMD_TLBI_S2_IPA causes CERROR_ILL when IDR0.S2P=0.
#[test]
fn bug_new39_tlbi_s2_ipa_s2p0_cerror_ill() {
    let smmu = make_smmu();
    smmu.set_s2p_supported(false);

    let cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-39: TlbiS2Ipa with IDR0.S2P=0 must write CERROR_ILL \
         (ARM §4.4.3.2). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-39: TlbiS2Ipa with IDR0.S2P=0 must set GERROR.CMDQ_ERR \
         (ARM §4.4.3.2)"
    );
}
