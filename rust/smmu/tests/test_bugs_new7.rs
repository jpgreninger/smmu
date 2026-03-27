//! Regression tests for BUG-NEW-34 and BUG-NEW-35 (Rust).
//!
//! # BUG-NEW-34 (Rust, §4.4.2.9)
//!
//! In `process_single_command()`, the `TlbiEl3All` and `TlbiEl3Va` match arms
//! omit the inline `signal_gerror(GERROR_CMDQ_ERR)` call that every other
//! CERROR_ILL path includes.  `process_command_queue()` compensates by calling
//! `signal_gerror` in the caller after detecting a CERROR.  The observable
//! behaviour is identical before and after any refactor that moves the call
//! into the arm.  These tests confirm that `GERROR.CMDQ_ERR` is set after
//! submitting and processing each command.
//!
//! # BUG-NEW-35 (Rust, §4.1.1)
//!
//! Same pattern as BUG-NEW-34 for `DptiAll` and `DptiPa`.  Both commands
//! are Secure-only / unimplemented in this model and must raise CERROR_ILL.
//! The inline `signal_gerror` call is missing from their match arms;
//! `process_command_queue()` compensates.  Again, observable behaviour is
//! unchanged.
//!
//! # BUG-NEW-36 (Both)
//!
//! Comment-only fix — no test needed.
//!
//! # Test classification
//!
//! All tests are REGRESSION GUARDS.  They should PASS both before and after
//! any refactor that moves the `signal_gerror` call into the individual arms.

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
// BUG-NEW-34: TlbiEl3All and TlbiEl3Va — confirm GERROR.CMDQ_ERR is set
// ============================================================================
// ARM §4.4.2.9: CMD_TLBI_EL3_ALL and CMD_TLBI_EL3_VA are valid only on the
// Secure Command queue; on the Non-secure Command queue they cause CERROR_ILL.
//
// These tests are REGRESSION GUARDS — they must PASS both before and after
// any refactor that moves the signal_gerror call into the match arm.

/// BUG-NEW-34 (regression guard): `TlbiEl3All` on NS queue must set
/// GERROR.CMDQ_ERR after processing.
///
/// ARM §4.4.2.9: valid only on Secure queue; NS queue → CERROR_ILL.
/// The inline `signal_gerror` call is missing from this arm; the caller
/// compensates.  Observable behaviour is unchanged — this guard confirms it.
#[test]
fn bug_new34_tlbi_el3_all_gerror_cmdq_err_active() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::TlbiEl3All, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-34: TlbiEl3All on NS queue must write CERROR_ILL \
         (ARM §4.4.2.9). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-34: TlbiEl3All on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.9)"
    );
}

/// BUG-NEW-34 (regression guard): `TlbiEl3Va` on NS queue must set
/// GERROR.CMDQ_ERR after processing.
///
/// ARM §4.4.2.9: valid only on Secure queue; NS queue → CERROR_ILL.
#[test]
fn bug_new34_tlbi_el3_va_gerror_cmdq_err_active() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::TlbiEl3Va, 0, 0);
    cmd.start_address = 0x0000_1000u64;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-34: TlbiEl3Va on NS queue must write CERROR_ILL \
         (ARM §4.4.2.9). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-34: TlbiEl3Va on NS queue must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.9)"
    );
}

// ============================================================================
// BUG-NEW-35: DptiAll and DptiPa — confirm GERROR.CMDQ_ERR is set
// ============================================================================
// ARM §4.1.1: CMD_DPTI_ALL (opcode 0x70) and CMD_DPTI_PA (opcode 0x73) are
// dirty-page tracking invalidation commands.  This model does not implement
// dirty-page tracking, so both must raise CERROR_ILL when submitted.
//
// Same pattern as BUG-NEW-34: the inline `signal_gerror` call is missing from
// the match arms; the caller compensates.  Observable behaviour is unchanged.
//
// These tests are REGRESSION GUARDS — they must PASS both before and after
// any refactor that moves the signal_gerror call into the match arm.

/// BUG-NEW-35 (regression guard): `DptiAll` must set GERROR.CMDQ_ERR after
/// processing.
///
/// This model does not implement dirty-page tracking invalidation; the command
/// must raise CERROR_ILL on any queue submission.
#[test]
fn bug_new35_dpti_all_gerror_cmdq_err_active() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::DptiAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-35: DptiAll must write CERROR_ILL \
         (dirty-page tracking not implemented). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-35: DptiAll must assert GERROR.CMDQ_ERR \
         (dirty-page tracking not implemented)"
    );
}

/// BUG-NEW-35 (regression guard): `DptiPa` must set GERROR.CMDQ_ERR after
/// processing.
///
/// This model does not implement dirty-page tracking invalidation; the command
/// must raise CERROR_ILL on any queue submission.
#[test]
fn bug_new35_dpti_pa_gerror_cmdq_err_active() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::DptiPa, 0, 0);
    cmd.start_address = 0u64;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-35: DptiPa must write CERROR_ILL \
         (dirty-page tracking not implemented). Got CERROR={}",
        smmu.get_cmdq_cons_err()
    );
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-35: DptiPa must assert GERROR.CMDQ_ERR \
         (dirty-page tracking not implemented)"
    );
}
