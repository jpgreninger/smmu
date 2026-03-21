#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! TDD tests for BUG-RUST-5 (CMDQ CONS.RD advanced before command is processed)
//!
//! ARM §7.1: "SMMU_(*_)CMDQ_CONS.RD remains pointing to the erroneous command
//! in the Command queue."
//!
//! ARM §7.1 also states: "Older commands (prior to the erroneous command in
//! the queue) have been consumed."  CONS.RD therefore MUST only advance for
//! commands that complete without error.  On the first error it must freeze at
//! the index of the erroneous command.
//!
//! # Root cause (before fix)
//!
//! In `process_command_queue()` the internal deque pop and `cmdq_cons` advance
//! happen inside the same write-lock block, BEFORE `process_single_command()`
//! is called.  When `process_single_command()` returns `Err(_)` the index has
//! already been bumped — violating the spec.
//!
//! # Fix contract
//!
//! Advance `cmdq_cons` ONLY after `process_single_command()` returns `Ok(())`.
//! On error, do NOT advance `cmdq_cons`; leave it pointing at the erroneous
//! command so that software can inspect and recover.
//!
//! # Test trigger
//!
//! CMD_SYNC with CS=3 is a Reserved encoding → CERROR_ILL (ARM §4.7.3).
//! `process_single_command()` returns `Err(...)` for this command.

use smmu::types::{CommandEntry, CommandType};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    // Gate: CMDQEN must be set for process_command_queue to do anything.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

/// Build a CMD_SYNC command with CS=3, which is Reserved and causes CERROR_ILL.
const fn bad_sync_cmd() -> CommandEntry {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 → CERROR_ILL per ARM §4.7.3
    cmd
}

/// Return only the queue-index bits from a raw CONS value.
/// cmdq_cons stores the index in bits [log2size:0].  For the default log2size=7
/// the index occupies the low 8 bits (wrap bit at bit 7, position at bits 6:0).
/// We mask away everything above bit 30 (the ERR field lives at [30:24] in the
/// hardware register; our model stores ERR separately, so the raw atomic value
/// IS the index).  We compare against the raw value directly.
fn cons_index(smmu: &SMMU) -> u32 {
    smmu.cmdq_cons_index()
}

// ── Test 1: THE CRITICAL FAILING TEST ─────────────────────────────────────────

/// BUG-RUST-5 core: after a CERROR_ILL, CONS.RD must remain at the erroneous
/// command (index 0) — it must NOT have advanced to index 1.
///
/// BEFORE FIX: cmdq_cons is advanced unconditionally inside the write-lock block
/// before `process_single_command()` runs, so this test sees index=1.
///
/// AFTER FIX: cmdq_cons is advanced only on `Ok(())` from
/// `process_single_command()`, so the index stays at 0 on error.
#[test]
fn cmdq_cons_rd_stays_at_erroneous_command_on_cerror_ill() {
    let smmu = make_smmu();

    // Verify precondition: CONS starts at 0 (queue empty, no wrap).
    assert_eq!(cons_index(&smmu), 0, "precondition: CONS.RD must be 0 before any commands");

    // Submit one CMD_SYNC with CS=3 (will cause CERROR_ILL).
    smmu.submit_command(bad_sync_cmd()).unwrap();

    // PROD must now be 1 (submit_command advanced it).
    let prod_after_submit = smmu.cmdq_prod_index();
    assert_eq!(prod_after_submit, 1, "precondition: PROD must be 1 after one submit");

    // Process — this must trigger CERROR_ILL and leave CONS at 0.
    let _ = smmu.process_command_queue();

    let cons_after = cons_index(&smmu);
    assert_eq!(
        cons_after,
        0,
        "BUG-RUST-5: CONS.RD must remain at 0 (erroneous command) after CERROR_ILL; \
         got {cons_after} (was advanced past the bad command before it was processed)"
    );
}

// ── Test 2: CERROR_ILL is reported correctly ──────────────────────────────────

/// After a CERROR_ILL the ERR field must be CERROR_ILL (1).
///
/// This test verifies the companion invariant: not only must CONS.RD freeze,
/// but the error code must be correctly recorded.
#[test]
fn cmdq_cons_err_is_cerror_ill_after_bad_sync() {
    let smmu = make_smmu();

    smmu.submit_command(bad_sync_cmd()).unwrap();
    let _ = smmu.process_command_queue();

    let err = smmu.get_cmdq_cons_err();
    assert_eq!(
        err,
        SMMU::CERROR_ILL,
        "BUG-RUST-5: CMDQ_CONS.ERR must be CERROR_ILL ({}) after CS=3; got {err}",
        SMMU::CERROR_ILL
    );
}

// ── Test 3: GERROR.CMDQ_ERR is raised ────────────────────────────────────────

/// After CERROR_ILL, GERROR.CMDQ_ERR must be active (GERROR != GERRORN for that
/// bit).  This is a pre-existing correct behaviour that must not regress.
#[test]
fn gerror_cmdq_err_set_after_cerror_ill() {
    let smmu = make_smmu();

    smmu.submit_command(bad_sync_cmd()).unwrap();
    let _ = smmu.process_command_queue();

    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(
        active, 0,
        "GERROR.CMDQ_ERR must be ACTIVE (GERROR != GERRORN) after CERROR_ILL"
    );
}

// ── Test 4: Successful commands DO advance CONS.RD ───────────────────────────

/// Successful commands must still advance CONS.RD by 1 per command.
///
/// This guards against an over-eager fix that stops advancing CONS.RD entirely.
/// A CMD_SYNC with CS=0 (SIG_NONE) succeeds → CONS must advance to 1.
#[test]
fn cmdq_cons_rd_advances_on_successful_command() {
    let smmu = make_smmu();

    assert_eq!(cons_index(&smmu), 0, "precondition: CONS=0 at reset");

    // CS=0 = SIG_NONE, valid → should succeed.
    let mut ok_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    ok_cmd.cs = 0;
    smmu.submit_command(ok_cmd).unwrap();

    let result = smmu.process_command_queue();
    assert!(result.is_ok(), "CS=0 CMD_SYNC must succeed, got: {result:?}");
    assert_eq!(result.unwrap(), 1, "one command must have been processed");

    let cons_after = cons_index(&smmu);
    assert_eq!(
        cons_after, 1,
        "CONS.RD must advance to 1 after a successful command; got {cons_after}"
    );
}

// ── Test 5: Preceding successful commands advance; erroneous stops ────────────

/// ARM §7.1: "Older commands (prior to the erroneous command) have been
/// consumed."  Two good commands followed by one bad command: CONS must
/// advance to 2 (past the two good commands) and freeze there.
#[test]
fn cmdq_cons_rd_advances_past_good_cmds_then_freezes_at_bad() {
    let smmu = make_smmu();

    // Submit 2 good commands (CS=0) then 1 bad command (CS=3).
    let mut ok_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    ok_cmd.cs = 0;
    smmu.submit_command(ok_cmd).unwrap();
    smmu.submit_command(ok_cmd).unwrap();
    smmu.submit_command(bad_sync_cmd()).unwrap();

    // PROD must be 3 after 3 submits.
    assert_eq!(smmu.cmdq_prod_index(), 3, "precondition: PROD=3");
    assert_eq!(cons_index(&smmu), 0, "precondition: CONS=0");

    // Process — should process 2 good, then hit the bad one and halt.
    let _ = smmu.process_command_queue();

    let cons_after = cons_index(&smmu);
    assert_eq!(
        cons_after,
        2,
        "BUG-RUST-5: CONS.RD must be 2 (advanced past 2 good cmds, frozen at bad cmd index 2); \
         got {cons_after}"
    );

    // ERR must still be CERROR_ILL.
    assert_eq!(smmu.get_cmdq_cons_err(), SMMU::CERROR_ILL);
}

// ── Test 6: CONS.RD is stable; no double-advance on error ────────────────────

/// Calling process_command_queue again after a CERROR_ILL (while GERROR is
/// still active) must be a complete no-op: CONS.RD must not change.
#[test]
fn cmdq_cons_rd_stable_after_gerror_blocks_further_processing() {
    let smmu = make_smmu();

    smmu.submit_command(bad_sync_cmd()).unwrap();
    let _ = smmu.process_command_queue();

    let cons_after_first = cons_index(&smmu);

    // Call again — GERROR.CMDQ_ERR is active, so this is a no-op.
    let result = smmu.process_command_queue();
    assert_eq!(result.unwrap(), 0, "second call must be a no-op (GERROR active)");

    let cons_after_second = cons_index(&smmu);
    assert_eq!(
        cons_after_second, cons_after_first,
        "CONS.RD must not change when GERROR is blocking queue processing"
    );
}
