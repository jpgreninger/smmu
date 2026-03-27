//! TDD failing tests for BUG-NEW-28, BUG-NEW-29, BUG-NEW-30, BUG-NEW-32
//! (Rust side).
//!
//! Each test is written to FAIL with the current code and PASS only after the
//! corresponding fix is applied.  Positive/regression tests that should
//! already PASS are explicitly noted.
//!
//! Note: BUG-NEW-31 is C++ only (Rust already uses the correct AND logic).
//!
//! # Bug Summary
//!
//! ## BUG-NEW-28 (§4.4.4.2)
//!
//! `TlbiSnhAll` is matched alongside `TlbiSEl2All` in a single arm that calls
//! `self.tlb_cache.invalidate_all()` with no CERROR_ILL guard.
//!
//! ARM §4.4.4.2: "This command causes CERROR_ILL when used on the Non-secure
//! Command queue."
//!
//! BEFORE FIX: `TlbiSnhAll` executes normally → CMDQ_ERR not set → test FAILS.
//! AFTER FIX:  CERROR_ILL raised → CMDQ_ERR toggled → passes.
//!
//! ## BUG-NEW-29 (§4.4.2.11/12/13/14)
//!
//! `TlbiSEl2Va` is grouped with `TlbiNhVa` (line ~5680 of smmu/mod.rs) with
//! no CERROR_ILL guard.  `TlbiSEl2Vaa` is grouped with `TlbiNhVaa` (line ~5736)
//! with no guard.  `TlbiSEl2All` and `TlbiSEl2Asid` are in the same arm as
//! `TlbiSnhAll` / their own arm — all execute as real TLB ops without error.
//!
//! ARM §4.4.2.11-14: each of these four variants "is valid only on the Secure
//! Command queue; on the Non-secure Command queue it causes CERROR_ILL."
//!
//! BEFORE FIX: all four execute as real invalidations → no CERROR → test FAILS.
//! AFTER FIX:  CERROR_ILL raised for each → passes.
//!
//! ## BUG-NEW-30 (§4.4.3.3/§4.4.3.4)
//!
//! `TlbiSS12Vmall` is matched alongside `TlbiS12Vmall` (line ~5741) and
//! `TlbiSS2Ipa` is matched alongside `TlbiS2Ipa` (line ~5752) — both execute
//! as real TLB operations without CERROR_ILL.
//!
//! ARM §4.4.3.3/4.4.3.4: "This command is valid only on the Secure Command
//! queue; on the Non-secure Command queue it causes CERROR_ILL."
//!
//! BEFORE FIX: both execute normally → no CERROR → tests FAIL.
//! AFTER FIX:  CERROR_ILL raised → tests pass.
//!
//! ## BUG-NEW-32 (§4.3.5/§4.1.6)
//!
//! `CfgiVmsPidm` is a silent no-op (`CommandType::CfgiVmsPidm => {}` at line
//! ~6130) with no guards.  Two guards are missing:
//! 1. SSec=1 on NS queue → CERROR_ILL (§4.1.6)
//! 2. IDR3.MPAM==0 → CERROR_ILL (§4.3.5; this model reports IDR3.MPAM=0)
//!
//! BEFORE FIX: silent no-op for ssec=0; no CERROR_ILL for either guard →
//!   tests FAIL.
//! AFTER FIX:  CERROR_ILL raised (both guards) → tests pass.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{CommandEntry, CommandType};
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

// ============================================================================
// BUG-NEW-28: TlbiSnhAll on NS queue must raise CERROR_ILL (§4.4.4.2)
// ============================================================================
// ARM §4.4.4.2: "This command causes CERROR_ILL when used on the Non-secure
// Command queue."
//
// Current code: `CommandType::TlbiSEl2All | CommandType::TlbiSnhAll` at line
// ~6120 calls `self.tlb_cache.invalidate_all()` with no error check.
//
// BEFORE FIX: TlbiSnhAll executes as a real invalidation → CMDQ_ERR not set
//   → `is_gerror_cmdq_err_active` returns false → test FAILS (red).
// AFTER FIX:  TlbiSnhAll gets its own arm that raises CERROR_ILL → passes.

/// BUG-NEW-28 (primary): submitting `TlbiSnhAll` on the NS queue must set
/// CMDQ_ERR and write CERROR_ILL.
///
/// ARM §4.4.4.2: "This command causes CERROR_ILL when used on the Non-secure
/// Command queue."
///
/// BEFORE FIX: grouped with `TlbiSEl2All` — calls `invalidate_all()` with no
///   error → `is_gerror_cmdq_err_active` returns false → test FAILS (red).
/// AFTER FIX:  dedicated arm raises CERROR_ILL → passes (green).
#[test]
fn bug_new28_snhall_ns_queue_cerror_ill() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::TlbiSnhAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-28: TlbiSnhAll on NS queue must write CERROR_ILL \
         (ARM §4.4.4.2). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-28: TlbiSnhAll on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.4.2)"
    );
}

// ============================================================================
// BUG-NEW-29: TlbiSEl2All/Va/Vaa/Asid on NS queue must raise CERROR_ILL
// (§4.4.2.11/12/13/14)
// ============================================================================
// ARM §4.4.2.11: "CMD_TLBI_S_EL2_ALL is valid only on the Secure Command
//   queue; on the Non-secure Command queue it causes CERROR_ILL."
// ARM §4.4.2.12: same for CMD_TLBI_S_EL2_VA.
// ARM §4.4.2.13: same for CMD_TLBI_S_EL2_VAA.
// ARM §4.4.2.14: same for CMD_TLBI_S_EL2_ASID.
//
// BEFORE FIX:
//   TlbiSEl2All: grouped with TlbiSnhAll → invalidate_all(), no error.
//   TlbiSEl2Va:  grouped with TlbiNhVa → VA invalidation, no error.
//   TlbiSEl2Vaa: grouped with TlbiNhVaa → VA invalidation, no error.
//   TlbiSEl2Asid: own arm → ASID invalidation, no error.
//   All four → CMDQ_ERR not set → tests FAIL (red).
// AFTER FIX:  all four raise CERROR_ILL → tests pass (green).

/// BUG-NEW-29: `TlbiSEl2All` on NS queue must raise CERROR_ILL.
///
/// BEFORE FIX: grouped with TlbiSnhAll — calls invalidate_all() → FAILS.
/// AFTER FIX:  dedicated arm raises CERROR_ILL → passes.
#[test]
fn bug_new29_s_el2_all_ns_cerror_ill() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::TlbiSEl2All, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-29: TlbiSEl2All on NS queue must write CERROR_ILL \
         (ARM §4.4.2.11). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-29: TlbiSEl2All on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.11)"
    );
}

/// BUG-NEW-29: `TlbiSEl2Va` on NS queue must raise CERROR_ILL.
///
/// BEFORE FIX: grouped with TlbiNhVa — VA invalidation, no error → FAILS.
/// AFTER FIX:  dedicated arm raises CERROR_ILL → passes.
#[test]
fn bug_new29_s_el2_va_ns_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiSEl2Va, 0, 0);
    cmd.start_address = 0x0001_0000u64;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-29: TlbiSEl2Va on NS queue must write CERROR_ILL \
         (ARM §4.4.2.12). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-29: TlbiSEl2Va on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.12)"
    );
}

/// BUG-NEW-29: `TlbiSEl2Vaa` on NS queue must raise CERROR_ILL.
///
/// BEFORE FIX: grouped with TlbiNhVaa — VA invalidation, no error → FAILS.
/// AFTER FIX:  dedicated arm raises CERROR_ILL → passes.
#[test]
fn bug_new29_s_el2_vaa_ns_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiSEl2Vaa, 0, 0);
    cmd.start_address = 0x0002_0000u64;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-29: TlbiSEl2Vaa on NS queue must write CERROR_ILL \
         (ARM §4.4.2.13). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-29: TlbiSEl2Vaa on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.13)"
    );
}

/// BUG-NEW-29: `TlbiSEl2Asid` on NS queue must raise CERROR_ILL.
///
/// BEFORE FIX: own arm calls invalidate_by_asid(), no error → FAILS.
/// AFTER FIX:  arm raises CERROR_ILL → passes.
#[test]
fn bug_new29_s_el2_asid_ns_cerror_ill() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::TlbiSEl2Asid, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-29: TlbiSEl2Asid on NS queue must write CERROR_ILL \
         (ARM §4.4.2.14). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-29: TlbiSEl2Asid on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.14)"
    );
}

// ============================================================================
// BUG-NEW-30: TlbiSS2Ipa and TlbiSS12Vmall on NS queue must raise CERROR_ILL
// (§4.4.3.3/§4.4.3.4)
// ============================================================================
// ARM §4.4.3.3: "CMD_TLBI_S_S2_IPA is valid only on the Secure Command queue;
//   on the Non-secure Command queue it causes CERROR_ILL."
// ARM §4.4.3.4: "CMD_TLBI_S_S12_VMALL is valid only on the Secure Command
//   queue; on the Non-secure Command queue it causes CERROR_ILL."
//
// BEFORE FIX:
//   TlbiSS2Ipa:    grouped with TlbiS2Ipa → executes IPA invalidation, no error.
//   TlbiSS12Vmall: grouped with TlbiS12Vmall → executes VMID invalidation, no error.
//   Both → CMDQ_ERR not set → tests FAIL (red).
// AFTER FIX:  both raise CERROR_ILL → tests pass (green).

/// BUG-NEW-30: `TlbiSS2Ipa` on NS queue must raise CERROR_ILL.
///
/// BEFORE FIX: grouped with TlbiS2Ipa — calls IPA invalidation, no error → FAILS.
/// AFTER FIX:  dedicated arm raises CERROR_ILL → passes.
#[test]
fn bug_new30_s_s2_ipa_ns_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiSS2Ipa, 0, 0);
    cmd.start_address = 0x0004_0000u64;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-30: TlbiSS2Ipa on NS queue must write CERROR_ILL \
         (ARM §4.4.3.3). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-30: TlbiSS2Ipa on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.3.3)"
    );
}

/// BUG-NEW-30: `TlbiSS12Vmall` on NS queue must raise CERROR_ILL.
///
/// BEFORE FIX: grouped with TlbiS12Vmall — calls VMID invalidation, no error → FAILS.
/// AFTER FIX:  dedicated arm raises CERROR_ILL → passes.
#[test]
fn bug_new30_s_s12_vmall_ns_cerror_ill() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::TlbiSS12Vmall, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-30: TlbiSS12Vmall on NS queue must write CERROR_ILL \
         (ARM §4.4.3.4). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-30: TlbiSS12Vmall on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.3.4)"
    );
}

// ============================================================================
// BUG-NEW-32: CfgiVmsPidm missing guards (§4.3.5/§4.1.6)
// ============================================================================
// ARM §4.3.5: CMD_CFGI_VMS_PIDM requires IDR3.MPAM==1.  This model reports
// IDR3.MPAM=0, so the command must unconditionally raise CERROR_ILL.
// ARM §4.1.6: Any NS-queue command with SSec=1 is illegal → CERROR_ILL.
//
// Current code: `CommandType::CfgiVmsPidm => {}` is a silent no-op with no
// guards whatsoever.
//
// BEFORE FIX: CfgiVmsPidm is a no-op → CMDQ_ERR not set → tests FAIL (red).
// AFTER FIX:  CERROR_ILL raised for both guards → tests pass (green).

/// BUG-NEW-32: `CfgiVmsPidm` with `ssec=true` must raise CERROR_ILL.
///
/// ARM §4.1.6: "Any NS-queue command with SSec=1 is illegal — CERROR_ILL."
///
/// BEFORE FIX: silent no-op → no CERROR → test FAILS (red).
/// AFTER FIX:  ssec guard added → CERROR_ILL raised → passes (green).
#[test]
fn bug_new32_cfgi_vms_pidm_ssec1_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::CfgiVmsPidm, 0, 0);
    cmd.ssec = true;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-32: CfgiVmsPidm with ssec=1 must write CERROR_ILL \
         (ARM §4.1.6). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-32: CfgiVmsPidm with ssec=1 must assert GERROR.CMDQ_ERR \
         (ARM §4.1.6)"
    );
}

/// BUG-NEW-32: `CfgiVmsPidm` with `ssec=false` must still raise CERROR_ILL
/// because IDR3.MPAM==0 makes the command unconditionally illegal per §4.3.5.
///
/// BEFORE FIX: silent no-op → no CERROR → test FAILS (red).
/// AFTER FIX:  IDR3.MPAM==0 guard raises CERROR_ILL → passes (green).
#[test]
fn bug_new32_cfgi_vms_pidm_mpam0_cerror_ill() {
    let smmu = make_smmu();

    // ssec=false — the SSec guard is NOT the reason for failure here.
    // The IDR3.MPAM==0 guard must fire regardless.
    let mut cmd = CommandEntry::new(CommandType::CfgiVmsPidm, 0, 0);
    cmd.ssec = false;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-32: CfgiVmsPidm with ssec=0 must still write CERROR_ILL \
         because IDR3.MPAM==0 and MPAM is required for this command \
         (ARM §4.3.5). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-32: CfgiVmsPidm with ssec=0 must assert GERROR.CMDQ_ERR \
         because IDR3.MPAM==0 (ARM §4.3.5)"
    );
}
