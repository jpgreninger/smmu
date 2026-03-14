//! Tests for BUG-RUST-DBGR-7 — GERROR bit toggle without CAS (§7.5)
#![allow(clippy::doc_markdown)]
#![allow(clippy::similar_names)]
//!
//! The spec says "SMMU does not toggle bit[x] if error already active".
//! This means if GERROR[x] != GERRORN[x] (error active), a second signal
//! must NOT toggle GERROR[x] again.
//!
//! CONF-GAP-2 note: CMD_CFGI_STE for unknown StreamID is a silent no-op (§4.3.1).
//! CMDQ_ERR is triggered here via CMD_SYNC CS=3 (Reserved → CERROR_ILL per §4.7.3).

use smmu::SMMU;
use smmu::types::{CommandEntry, CommandType};

/// Helper: trigger GERROR.CMDQ_ERR via CMD_SYNC CS=3 (CERROR_ILL per §4.7.3).
fn trigger_cmdq_err(smmu: &SMMU) {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 is Reserved → CERROR_ILL
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
}

/// Verify that signalling GERROR when the error bit is already active is a no-op.
///
/// Protocol: signal once (activates GERROR.CMDQ_ERR), then signal again.
/// The second signal must leave GERROR unchanged (idempotent).
#[test]
fn dbgr7_gerror_toggle_idempotent_when_already_active() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR via CMD_SYNC CS=3 (CERROR_ILL).
    trigger_cmdq_err(&smmu);

    // Error should now be ACTIVE (GERROR[x] != GERRORN[x]).
    let gerror1 = smmu.get_gerror();
    let gerrorn1 = smmu.get_gerrorn();
    assert_ne!(
        (gerror1 ^ gerrorn1) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be ACTIVE after first signal"
    );

    // Clear the error first so we can re-arm:
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    // Now signal once more to activate it again.
    trigger_cmdq_err(&smmu);

    let gerror2 = smmu.get_gerror();
    let gerrorn2 = smmu.get_gerrorn();
    assert_ne!(
        (gerror2 ^ gerrorn2) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be ACTIVE after re-arm"
    );

    // Now signal AGAIN without acknowledging — must NOT toggle the bit again.
    // We can't directly call signal_gerror (private), but we can trigger another
    // bad command while CMDQ_ERR is already active; the queue halts on active error
    // so process_command_queue is a no-op.  The GERROR bits must remain unchanged.
    let gerror_before = smmu.get_gerror();
    let gerrorn_before = smmu.get_gerrorn();

    // Verify the SMMU halts processing (CMDQ_ERR active → no commands processed).
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3;
    smmu.submit_command(cmd).unwrap();
    let processed = smmu.process_command_queue().unwrap_or(0);
    assert_eq!(processed, 0, "Queue must halt while CMDQ_ERR is active");

    let gerror_after = smmu.get_gerror();
    let gerrorn_after = smmu.get_gerrorn();
    assert_eq!(
        gerror_before, gerror_after,
        "GERROR must not change when error already active"
    );
    assert_eq!(
        gerrorn_before, gerrorn_after,
        "GERRORN must not change when error already active"
    );
}

/// Verify that GERROR bit can be re-activated after software acknowledges it.
#[test]
fn dbgr7_gerror_can_be_reactivated_after_ack() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // First activation via CMD_SYNC CS=3.
    trigger_cmdq_err(&smmu);
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be active after first bad command"
    );

    // Acknowledge
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be inactive after acknowledge"
    );

    // Second activation.
    trigger_cmdq_err(&smmu);
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be re-activatable after acknowledge"
    );
}
