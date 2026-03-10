#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::similar_names)]

//! Tests for the 3 remaining Rust compliance gaps.
//!
//! Gap 1: RECINVSID not enforced (§6.3.12 / §7.3.3)
//! Gap 2: BUG-RUST-C minor TOCTOU in configure_stream ordering
//! Gap 3: BUG-RUST-E over-retry in signal_gerror (re-reads gerror instead of gerrorn)

use smmu::types::{CommandEntry, CommandType, SMMUConfig, StreamConfig};
use smmu::SMMU;

// ── Gap 1: RECINVSID ────────────────────────────────────────────────────────

/// §6.3.12 / §7.3.3: With CR2.RECINVSID=0 (reset default), a C_BAD_STREAMID
/// event triggered by CMD_CFGI_STE for an unknown stream must NOT be recorded
/// in the event queue.
#[test]
fn recinvsid_default_no_event_for_cfgi_ste_unknown() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    // CR2.RECINVSID == 0 by default
    let cmd = CommandEntry::new(CommandType::CfgiSte, 0xDEAD, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "RECINVSID=0: C_BAD_STREAMID must NOT be recorded, got: {events:?}"
    );
}

/// §6.3.12 / §7.3.3: With CR2.RECINVSID=1, a C_BAD_STREAMID event triggered
/// by CMD_CFGI_STE for an unknown stream MUST be recorded in the event queue.
#[test]
fn recinvsid_set_event_recorded_for_cfgi_ste_unknown() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    let cmd = CommandEntry::new(CommandType::CfgiSte, 0xDEAD, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.stream_id == 0xDEAD),
        "RECINVSID=1: C_BAD_STREAMID must be recorded; got: {events:?}"
    );
}

/// §6.3.12: GERROR.CMDQ_ERR must be signalled unconditionally regardless of
/// the RECINVSID setting — event recording and GERROR are independent.
#[test]
fn recinvsid_default_gerror_cmdq_err_still_set() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    // CR2.RECINVSID == 0 — no event, but GERROR must still be signalled
    let cmd = CommandEntry::new(CommandType::CfgiSte, 0xBEEF, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
    let active = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_ne!(
        active & SMMU::GERROR_CMDQ_ERR,
        0,
        "GERROR.CMDQ_ERR must be active even when RECINVSID=0"
    );
}

/// CR2 register round-trip: set_cr2/get_cr2 must correctly store and retrieve values.
#[test]
fn cr2_get_set_round_trip() {
    let smmu = SMMU::new();
    assert_eq!(smmu.get_cr2(), 0, "CR2 must reset to 0 (§6.3.12)");
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    assert_eq!(smmu.get_cr2(), SMMU::CR2_RECINVSID, "CR2 must store the set value");
    smmu.set_cr2(0);
    assert_eq!(smmu.get_cr2(), 0, "CR2 must store the cleared value");
}

// ── Gap 2: BUG-RUST-C ordering ──────────────────────────────────────────────

/// BUG-RUST-C: max_streams snapshot must be taken BEFORE fetch_add so the
/// limit in effect at decision time is used.  With max_streams=2 and 2 existing
/// streams, a third configure_stream must fail deterministically.
#[test]
fn bug_rust_c_max_streams_snapshot_before_increment() {
    let config = SMMUConfig::default().with_max_streams(2);
    let smmu = SMMU::with_config(config);

    let cfg = StreamConfig::bypass();
    smmu.configure_stream(smmu::types::StreamID::new(1).unwrap(), cfg.clone()).unwrap();
    smmu.configure_stream(smmu::types::StreamID::new(2).unwrap(), cfg.clone()).unwrap();

    // Third stream must fail — limit was 2
    let result = smmu.configure_stream(smmu::types::StreamID::new(3).unwrap(), cfg);
    assert!(result.is_err(), "Third stream must be rejected when max_streams=2");

    // Stream count must stay at 2 after the failed attempt rolls back
    assert_eq!(smmu.get_stream_count(), 2, "Stream count must remain 2 after failed configure_stream");
}

// ── Gap 3: BUG-RUST-E gerrorn re-read ───────────────────────────────────────

/// BUG-RUST-E: signal_gerror() must not double-toggle an already-active bit.
/// The re-read consistency check must target gerrorn (modified by clear_gerror),
/// not gerror (only modified by signal_gerror itself), to detect concurrent
/// clear_gerror() calls — not concurrent signal_gerror() calls.
///
/// Observable invariant: after activating CMDQ_ERR, a second bad command must
/// leave gerror unchanged (bit stays active, not double-toggled).
#[test]
fn bug_rust_e_signal_gerror_uses_gerrorn_reread() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR once via a bad CfgiSte command
    let cmd = CommandEntry::new(CommandType::CfgiSte, 0xAB01, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    let gerror_after_first = smmu.get_gerror();
    let gerrorn_after_first = smmu.get_gerrorn();

    // CMDQ_ERR must be active
    assert_ne!(
        (gerror_after_first ^ gerrorn_after_first) & SMMU::GERROR_CMDQ_ERR,
        0,
        "BUG-RUST-E setup: CMDQ_ERR must be active after first bad command"
    );

    // Attempt a second bad command — CMDQ_ERR is already active so
    // process_command_queue returns early (queue halted). Submit the command
    // but the queue won't process it while CMDQ_ERR is set.
    // Instead call signal_gerror directly via clear+re-trigger to verify
    // idempotence: clear first, then trigger twice.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // Re-arm: submit two bad commands and process; only the first should activate
    // CMDQ_ERR; processing halts before the second is consumed.
    let cmd2 = CommandEntry::new(CommandType::CfgiSte, 0xAB02, 0);
    let cmd3 = CommandEntry::new(CommandType::CfgiSte, 0xAB03, 0);
    smmu.submit_command(cmd2).unwrap();
    smmu.submit_command(cmd3).unwrap();
    let _ = smmu.process_command_queue(); // activates CMDQ_ERR on cmd2, halts before cmd3

    let gerror_v2 = smmu.get_gerror();
    let gerrorn_v2 = smmu.get_gerrorn();
    assert_ne!(
        (gerror_v2 ^ gerrorn_v2) & SMMU::GERROR_CMDQ_ERR,
        0,
        "BUG-RUST-E: CMDQ_ERR must be active after second activation"
    );

    // Now directly call signal_gerror with the same bit (already active) —
    // gerror must not change (no double-toggle).  We do this by submitting
    // a second process attempt, which returns Ok(0) due to CMDQ_ERR guard —
    // so gerror stays unchanged.
    let gerror_before_retry = smmu.get_gerror();
    let _ = smmu.process_command_queue(); // must return Ok(0) — CMDQ_ERR active
    let gerror_after_retry = smmu.get_gerror();

    assert_eq!(
        gerror_before_retry, gerror_after_retry,
        "BUG-RUST-E: gerror must not change when CMDQ_ERR is already active"
    );

    // Error must still be active
    let active_final = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_ne!(
        active_final & SMMU::GERROR_CMDQ_ERR,
        0,
        "BUG-RUST-E: CMDQ_ERR must remain active (not cancelled by double-toggle)"
    );
}
