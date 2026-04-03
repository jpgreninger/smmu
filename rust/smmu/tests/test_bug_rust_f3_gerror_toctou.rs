#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::similar_names)]

//! TDD regression tests for BUG-RUST-F3:
//! TOCTOU in GERROR check inside `process_command_queue()`.
//!
//! Problem (§6.3.19, §7.5):
//!   "Commands are not consumed from the Command queue while CMDQ_ERR is active."
//!
//!   The guard reads gerror and gerrorn with two SEPARATE `Acquire` loads:
//!   ```
//!   let gerror_active = self.gerror.load(Ordering::Acquire)
//!       ^ self.gerrorn.load(Ordering::Acquire);
//!   ```
//!   A concurrent `clear_gerror()` between these two loads can make
//!   `gerror_active` compute as 0 even though CMDQ_ERR was logically active
//!   when the first load was taken, allowing commands to be processed while
//!   the error is still in progress.
//!
//! Fix: Add `fence(SeqCst)` between the two loads so both loads are
//! ordered with respect to any concurrent clear_gerror() operation, OR
//! use a single combined atomic for gerror/gerrorn.
//!
//! These tests verify the OBSERVABLE INVARIANT:
//! - While CMDQ_ERR is active, `process_command_queue()` must return Ok(0).
//! - Only after `clear_gerror()` may commands be processed again.

use smmu::types::{CommandEntry, CommandType, StreamConfig, StreamID};
use smmu::SMMU;

fn good_sid() -> StreamID {
    StreamID::new(1).unwrap()
}

fn setup_smmu_with_cmdq_err() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR via CMD_SYNC CS=3 (Reserved → CERROR_ILL per §4.7.3).
    // (CONF-GAP-2: CMD_CFGI_STE for unknown StreamID is a silent no-op per §4.3.1.)
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3;
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue(); // This activates CMDQ_ERR.
    smmu
}

/// When CMDQ_ERR is active, process_command_queue must process ZERO commands.
///
/// This verifies the §6.3.19 / §7.5 requirement that the command queue
/// halts while CMDQ_ERR is active.
#[test]
fn f3_command_queue_halts_while_cmdq_err_active() {
    let smmu = setup_smmu_with_cmdq_err();

    // Confirm CMDQ_ERR is active.
    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(active, 0, "F3: CMDQ_ERR must be active after bad command");

    // Configure a stream so a subsequent CfgiSte command would be valid.
    smmu.configure_stream(good_sid(), StreamConfig::stage1_only()).unwrap();

    // Submit a valid command while CMDQ_ERR is active.
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, good_sid().as_u32(), 0))
        .unwrap();

    // The queue MUST halt — zero commands processed while CMDQ_ERR is active.
    let processed = smmu.process_command_queue().unwrap_or(0);
    assert_eq!(
        processed, 0,
        "F3 BUG: process_command_queue processed {processed} commands while CMDQ_ERR was active; \
         §6.3.19 / §7.5 prohibit command consumption while error is active"
    );
}

/// After `clear_gerror()` acknowledges CMDQ_ERR, commands resume processing.
///
/// This verifies the recovery path: once the error is cleared, the queue
/// should become operational again.
#[test]
fn f3_commands_resume_after_clear_gerror() {
    let smmu = setup_smmu_with_cmdq_err();

    // Confirm CMDQ_ERR is active.
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "F3: CMDQ_ERR must be active"
    );

    // ARM §7.1 two-step recovery: advance CONS past the erroneous command first,
    // then acknowledge GERROR.CMDQ_ERR.  (BUG-AUDIT-93: clear_gerror alone no
    // longer pops the bad command — that is advance_cmdq_cons's responsibility.)
    smmu.advance_cmdq_cons();
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // CMDQ_ERR must now be inactive.
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "F3: CMDQ_ERR must be inactive after clear_gerror"
    );

    // Submit a valid command (configure stream first so CfgiSte succeeds).
    smmu.configure_stream(good_sid(), StreamConfig::stage1_only()).unwrap();
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, good_sid().as_u32(), 0))
        .unwrap();

    // Commands must now be processed.
    let processed = smmu.process_command_queue().unwrap_or(0);
    assert_eq!(
        processed, 1,
        "F3: exactly one command should be processed after clearing CMDQ_ERR, got {processed}"
    );
}

/// The GERROR active check is atomic — both loads of gerror and gerrorn must
/// be ordered relative to each other and to concurrent clear_gerror calls.
///
/// This test verifies the single-threaded ordering invariant:
/// if we set CMDQ_ERR and then call process_command_queue in the same thread,
/// the error check must always see the active state (never spuriously pass).
#[test]
fn f3_gerror_check_never_spuriously_passes_in_single_thread() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Activate CMDQ_ERR via CMD_SYNC CS=3 (CERROR_ILL per §4.7.3).
    let mut bad_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_cmd.cs = 3;
    smmu.submit_command(bad_cmd).unwrap();
    let _ = smmu.process_command_queue();

    // Verify CMDQ_ERR is active.
    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(active, 0, "F3: setup — CMDQ_ERR must be active");

    // Queue more commands.
    for _ in 0..5 {
        smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, good_sid().as_u32(), 0))
            .unwrap();
    }

    // Call process_command_queue 10 times in a row — must always return 0.
    for i in 0..10 {
        let processed = smmu.process_command_queue().unwrap_or(0);
        assert_eq!(
            processed, 0,
            "F3 BUG: iteration {i}: processed {processed} commands while CMDQ_ERR active; \
             the two-load TOCTOU window must be closed with a SeqCst fence"
        );
    }
}

/// Verify that the XOR-active test is the correct check (not raw GERROR bit).
///
/// If GERROR[bit]==1 and GERRORN[bit]==1, the error is INACTIVE — XOR is 0.
/// process_command_queue must therefore process commands in that state.
#[test]
fn f3_gerror_xor_active_test_is_correct() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Activate CMDQ_ERR via CMD_SYNC CS=3 (CERROR_ILL per §4.7.3).
    let mut bad_cmd2 = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_cmd2.cs = 3;
    smmu.submit_command(bad_cmd2).unwrap();
    let _ = smmu.process_command_queue();

    // ARM §7.1 two-step recovery: advance CONS past the erroneous command first,
    // then acknowledge GERROR.CMDQ_ERR.  (BUG-AUDIT-93: clear_gerror alone no
    // longer pops the bad command — that is advance_cmdq_cons's responsibility.)
    smmu.advance_cmdq_cons();
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    let gerror_xor_gerrorn = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_eq!(
        gerror_xor_gerrorn & SMMU::GERROR_CMDQ_ERR,
        0,
        "F3: after clear_gerror the XOR-active test must show INACTIVE"
    );

    // Now a valid command must be processed (queue is unblocked).
    smmu.configure_stream(good_sid(), StreamConfig::stage1_only()).unwrap();
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, good_sid().as_u32(), 0))
        .unwrap();

    let processed = smmu.process_command_queue().unwrap_or(0);
    assert!(processed > 0, "F3: at least one command must be processed when CMDQ_ERR is inactive");
}
