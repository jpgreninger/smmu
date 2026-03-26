//! TDD failing tests for BUG-NEW-16 and BUG-NEW-17 (Rust implementation).
//!
//! Each test is written to FAIL with the current code and PASS only after the
//! corresponding fix is applied.  No fixes are included here.
//!
//! NOTE: BUG-NEW-15 is C++ only (Rust `submit_page_request()` already checks
//! the effective PRIQEN = CR0.PRIQEN AND CR0.SMMUEN, so it correctly rejects
//! requests when SMMUEN=0 even if PRIQEN remains set after `disable()`).
//!
//! # Bug Summary
//!
//! ## BUG-NEW-16 — §4.7.1/§4.1.6 CMD_RESUME missing ssec validation
//!
//! ARM §4.1.6: a Non-Secure command queue entry with SSec=1 is illegal and
//! must raise `CERROR_ILL` / `GERROR.CMDQ_ERR`.  `CommandEntry` currently has
//! no `ssec` field, so validation of this condition is structurally impossible.
//!
//! These tests reference `cmd.ssec = true` which will NOT COMPILE until the
//! `ssec` field is added to `CommandEntry`.  That compile failure is the
//! intended "red" state demonstrating the missing field and missing validation.
//!
//! ## BUG-NEW-17 — §4.7.2/§4.1.6 CMD_STALL_TERM missing ssec validation
//!
//! Same gap as BUG-NEW-16 but for `CMD_STALL_TERM`.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{CommandEntry, CommandType, SecurityState};
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
// BUG-NEW-16: CMD_RESUME with ssec=1 must raise CERROR_ILL (§4.7.1/§4.1.6)
// ============================================================================
//
// NOTE: The `ssec` field does NOT yet exist on `CommandEntry`.
// The line `cmd.ssec = true` below WILL NOT COMPILE until the field is added.
// A compile error here is the expected "red" state that demonstrates the
// missing structural support for the ssec validation.
//
// Once `ssec` is added to `CommandEntry`, the test will compile but the
// assertion on `get_cmdq_cons_err()` will FAIL because `process_command_queue()`
// does not yet validate the `ssec` field.
// Only after the validation logic is also added will the test pass (green).

/// BUG-NEW-16 (primary): CMD_RESUME with ssec=1 on Non-Secure queue must
/// raise CERROR_ILL and assert GERROR.CMDQ_ERR.
///
/// ARM §4.1.6: SSec=1 on a Non-Secure command queue is illegal.
///
/// BEFORE FIX: `CommandEntry` has no `ssec` field → compile error.
/// AFTER API ADD: field exists but processCommandQueue() ignores it → runtime fail.
/// AFTER FULL FIX: ssec=1 → CERROR_ILL set + GERROR.CMDQ_ERR active → pass.
#[test]
fn bug_new16_resume_ssec1_raises_cerror_ill() {
    let smmu = make_smmu();

    // Issue CMD_RESUME with SSec=1 on a Non-Secure command queue.
    // ARM §4.1.6: Non-Secure queue + SSec=1 → CERROR_ILL + GERROR.CMDQ_ERR.
    //
    // BUG-NEW-16: `ssec` field does not exist on CommandEntry yet.
    // This line WILL NOT COMPILE — this is the expected "red" state.
    let mut cmd = CommandEntry::new(CommandType::Resume, 0, 0);
    cmd.security_state = SecurityState::NonSecure; // NS queue — makes ssec=1 illegal
    cmd.ssec = true; // BUG-NEW-16: ssec field does not exist yet → compile error

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // AFTER FIX: CERROR_ILL must be set in CMDQ_CONS.ERR.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-16: CMD_RESUME with ssec=1 on Non-Secure queue must raise \
         CERROR_ILL in CMDQ_CONS.ERR (ARM §4.7.1/§4.1.6)"
    );

    // GERROR.CMDQ_ERR must be active.
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-16: CMD_RESUME with ssec=1 must assert GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

/// BUG-NEW-16 (negative): CMD_RESUME with ssec=0 on Non-Secure queue must NOT
/// raise CERROR_ILL (normal/no-stall-match path is a silent no-op).
///
/// BEFORE FIX: `CommandEntry` has no `ssec` field → compile error.
/// AFTER FIX: ssec=0 → no CERROR_ILL → test passes.
#[test]
fn bug_new16_resume_ssec0_does_not_raise_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::Resume, 0, 0);
    cmd.security_state = SecurityState::NonSecure;
    cmd.ssec = false; // BUG-NEW-16: ssec field does not exist yet → compile error

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // ssec=0 is legal — CERROR_ILL must not be raised.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-16 neg: CMD_RESUME with ssec=0 must NOT raise CERROR_ILL (ARM §4.1.6)"
    );

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-16 neg: GERROR.CMDQ_ERR must NOT be asserted for ssec=0 resume"
    );
}

// ============================================================================
// BUG-NEW-17: CMD_STALL_TERM with ssec=1 must raise CERROR_ILL (§4.7.2/§4.1.6)
// ============================================================================
//
// Same structural gap as BUG-NEW-16 but for CMD_STALL_TERM.
//
// NOTE: References to `cmd.ssec` below WILL NOT COMPILE until the `ssec` field
// is added to `CommandEntry`.  That compile failure is the expected "red" state.

/// BUG-NEW-17 (primary): CMD_STALL_TERM with ssec=1 on Non-Secure queue must
/// raise CERROR_ILL and assert GERROR.CMDQ_ERR.
///
/// ARM §4.1.6: SSec=1 on a Non-Secure command queue is illegal.
///
/// BEFORE FIX: `CommandEntry` has no `ssec` field → compile error.
/// AFTER API ADD: field exists but processCommandQueue() ignores it → runtime fail.
/// AFTER FULL FIX: ssec=1 → CERROR_ILL set + GERROR.CMDQ_ERR active → pass.
#[test]
fn bug_new17_stall_term_ssec1_raises_cerror_ill() {
    let smmu = make_smmu();

    // Issue CMD_STALL_TERM with SSec=1 on a Non-Secure command queue.
    // ARM §4.1.6: Non-Secure queue + SSec=1 → CERROR_ILL + GERROR.CMDQ_ERR.
    //
    // BUG-NEW-17: `ssec` field does not exist on CommandEntry yet.
    // This line WILL NOT COMPILE — this is the expected "red" state.
    let mut cmd = CommandEntry::new(CommandType::StallTerm, 0x10, 0);
    cmd.security_state = SecurityState::NonSecure;
    cmd.ssec = true; // BUG-NEW-17: ssec field does not exist yet → compile error

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // AFTER FIX: CERROR_ILL must be set in CMDQ_CONS.ERR.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-17: CMD_STALL_TERM with ssec=1 on Non-Secure queue must raise \
         CERROR_ILL in CMDQ_CONS.ERR (ARM §4.7.2/§4.1.6)"
    );

    // GERROR.CMDQ_ERR must be active.
    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-17: CMD_STALL_TERM with ssec=1 must assert GERROR.CMDQ_ERR (ARM §4.1.6)"
    );
}

/// BUG-NEW-17 (negative): CMD_STALL_TERM with ssec=0 on Non-Secure queue must
/// NOT raise CERROR_ILL.
///
/// BEFORE FIX: `CommandEntry` has no `ssec` field → compile error.
/// AFTER FIX: ssec=0 → no CERROR_ILL → test passes.
#[test]
fn bug_new17_stall_term_ssec0_does_not_raise_cerror_ill() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::StallTerm, 0x11, 0);
    cmd.security_state = SecurityState::NonSecure;
    cmd.ssec = false; // BUG-NEW-17: ssec field does not exist yet → compile error

    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // ssec=0 is legal — CERROR_ILL must not be raised.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-17 neg: CMD_STALL_TERM with ssec=0 must NOT raise CERROR_ILL (ARM §4.1.6)"
    );

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-17 neg: GERROR.CMDQ_ERR must NOT be asserted for ssec=0 stall-term"
    );
}
