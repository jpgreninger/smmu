//! Tests for BUG-RUST-DBGR-7 — GERROR bit toggle without CAS (§7.5)
#![allow(clippy::doc_markdown)]
#![allow(clippy::similar_names)]
//!
//! The spec says "SMMU does not toggle bit[x] if error already active".
//! This means if GERROR[x] != GERRORN[x] (error active), a second signal
//! must NOT toggle GERROR[x] again.

use smmu::SMMU;
use smmu::types::{CommandEntry, CommandType, StreamConfig, StreamID};

/// Verify that signalling GERROR when the error bit is already active is a no-op.
///
/// Protocol: signal once (activates GERROR.CMDQ_ERR), then signal again.
/// The second signal must leave GERROR unchanged (idempotent).
#[test]
fn dbgr7_gerror_toggle_idempotent_when_already_active() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR: submit a command for an unknown stream.
    let bad_sid = StreamID::new(0xDEAD).unwrap();
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, bad_sid.as_u32(), 0)).unwrap();
    let _ = smmu.process_command_queue();

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
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, bad_sid.as_u32(), 0)).unwrap();
    let _ = smmu.process_command_queue();

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
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, bad_sid.as_u32(), 0)).unwrap();
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

    let bad_sid = StreamID::new(0xBEEF).unwrap();

    // First activation
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, bad_sid.as_u32(), 0)).unwrap();
    let _ = smmu.process_command_queue();
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

    // Second activation (after configure stream so we can trigger another error)
    // Re-arm with another bad command.
    let _cfg = StreamConfig::stage1_only();
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, 0xCAFE, 0)).unwrap();
    let _ = smmu.process_command_queue();
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be re-activatable after acknowledge"
    );
}
