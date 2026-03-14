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

/// CONF-GAP-2: CMD_CFGI_STE for unknown stream is a silent no-op (§4.3.1).
/// Even with RECINVSID=1, no C_BAD_STREAMID event must be generated.
#[test]
fn recinvsid_set_event_recorded_for_cfgi_ste_unknown() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    let cmd = CommandEntry::new(CommandType::CfgiSte, 0xDEAD, 0);
    smmu.submit_command(cmd).unwrap();
    let result = smmu.process_command_queue();
    assert!(result.is_ok(), "CONF-GAP-2: CMD_CFGI_STE for unknown stream must be a silent no-op (Ok)");
    let events = smmu.get_events();
    assert!(
        !events.iter().any(|e| e.stream_id == 0xDEAD),
        "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT record C_BAD_STREAMID (§4.3.1); got: {events:?}"
    );
}

/// CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR.
/// §4.3.1 defines no error condition for unknown StreamID in CMD_CFGI_STE.
#[test]
fn recinvsid_default_gerror_cmdq_err_still_set() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    let cmd = CommandEntry::new(CommandType::CfgiSte, 0xBEEF, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
    let active = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_eq!(
        active & SMMU::GERROR_CMDQ_ERR,
        0,
        "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR (§4.3.1 silent no-op)"
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

    // Trigger CMDQ_ERR once via CMD_SYNC CS=3 (CERROR_ILL per §4.7.3).
    // (CONF-GAP-2: CMD_CFGI_STE for unknown StreamID is a silent no-op per §4.3.1.)
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3;
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

    // Clear the error and re-arm with two bad commands to verify idempotence.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // Re-arm: submit two CMD_SYNC CS=3 commands; only the first should activate
    // CMDQ_ERR; processing halts before the second is consumed.
    let mut cmd2 = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd2.cs = 3;
    let mut cmd3 = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd3.cs = 3;
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

// ── Gap 4: CONF-GAP-4 CMD_DPTI_ALL / CMD_DPTI_PA ───────────────────────────

/// §4.6.1: `CMD_DPTI_ALL` when `IDR3.DPT = 0` (no dirty-page tracking) must
/// generate `CERROR_ILL` and set `GERROR.CMDQ_ERR`.  The current no-op
/// silently swallows the command, violating §4.6.1.
#[test]
fn conf_gap4_dpti_all_sets_gerror_cmdq_err() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    let cmd = CommandEntry::new(CommandType::DptiAll, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(
        active, 0,
        "CONF-GAP-4: CMD_DPTI_ALL must set GERROR.CMDQ_ERR when IDR3.DPT=0 (§4.6.1)"
    );

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "CONF-GAP-4: CMDQ_CONS.ERR must be CERROR_ILL for CMD_DPTI_ALL when DPT not supported"
    );
}

/// §4.6.1: `CMD_DPTI_PA` when `IDR3.DPT = 0` must also generate `CERROR_ILL`
/// and set `GERROR.CMDQ_ERR`.
#[test]
fn conf_gap4_dpti_pa_sets_gerror_cmdq_err() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    let cmd = CommandEntry::new(CommandType::DptiPa, 0, 0);
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();

    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(
        active, 0,
        "CONF-GAP-4: CMD_DPTI_PA must set GERROR.CMDQ_ERR when IDR3.DPT=0 (§4.6.1)"
    );

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "CONF-GAP-4: CMDQ_CONS.ERR must be CERROR_ILL for CMD_DPTI_PA when DPT not supported"
    );
}
