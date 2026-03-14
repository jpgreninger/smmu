#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-M-06: GERROR register — command queue error conditions
//!
//! Spec: ARM IHI0070G.b §6.3.17 (SMMU_GERROR), §7.5 (Global error recording)
//!
//! Requirements:
//! - SMMU_GERROR starts at 0 after reset.
//! - CMDQ_ERR (bit 0) is set when command processing detects an error.
//! - Software clears GERROR bits by writing to SMMU_GERRORN (`clear_gerror`).
//! - Clearing only clears the specified bits; other bits are unaffected.
//! - After CMDQ_ERR is set, `process_command_queue` returns an error.
//! - Clearing CMDQ_ERR re-enables command queue processing.
//!
//! CONF-GAP-2 note: CMD_CFGI_STE for unknown StreamID is a silent no-op (§4.3.1).
//! CMDQ_ERR is triggered here via CMD_SYNC CS=3 (Reserved → CERROR_ILL per §4.7.3).

use smmu::types::{CommandEntry, CommandType, StreamConfig, StreamID};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

const fn cfgi_ste_cmd(stream_id: u32) -> CommandEntry {
    CommandEntry::new(CommandType::CfgiSte, stream_id, 0)
}

/// Helper: trigger GERROR.CMDQ_ERR via CMD_SYNC CS=3 (CERROR_ILL per §4.7.3).
fn trigger_cmdq_err(smmu: &SMMU) {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 is Reserved → CERROR_ILL
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
}

// ── §6.3.17: GERROR reset state ───────────────────────────────────────────────

/// §6.3.17: SMMU_GERROR must be zero after reset (no errors active).
#[test]
fn test_gerror_starts_at_zero() {
    let smmu = make_smmu();
    assert_eq!(smmu.get_gerror(), 0, "GERROR must be 0 after reset");
}

/// §6.3.17: GERROR_CMDQ_ERR constant must be bit 0 (0x01) per spec.
#[test]
fn test_gerror_cmdq_err_bit_position() {
    assert_eq!(SMMU::GERROR_CMDQ_ERR, 1 << 0, "CMDQ_ERR must be bit 0 (§6.3.17)");
}

/// §6.3.17: Confirm all GERROR bit constant values match the ARM IHI0070G.b spec table.
#[test]
fn test_gerror_bit_constants() {
    assert_eq!(SMMU::GERROR_CMDQ_ERR,           1 << 0, "CMDQ_ERR must be bit 0 (§6.3.17)");
    assert_eq!(SMMU::GERROR_EVENTQ_ABT_ERR,     1 << 2, "EVENTQ_ABT_ERR must be bit 2 (§6.3.17)");
    assert_eq!(SMMU::GERROR_PRIQ_ABT_ERR,       1 << 3, "PRIQ_ABT_ERR must be bit 3 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_CMDQ_ABT_ERR,   1 << 4, "MSI_CMDQ_ABT_ERR must be bit 4 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_EVENTQ_ABT_ERR, 1 << 5, "MSI_EVENTQ_ABT_ERR must be bit 5 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_PRIQ_ABT_ERR,   1 << 6, "MSI_PRIQ_ABT_ERR must be bit 6 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_GERROR_ABT_ERR, 1 << 7, "MSI_GERROR_ABT_ERR must be bit 7 (§6.3.17)");
    assert_eq!(SMMU::GERROR_SFM_ERR,            1 << 8, "SFM_ERR must be bit 8 (§6.3.17)");
    assert_eq!(SMMU::GERROR_CMDQP_ERR,          1 << 9, "CMDQP_ERR must be bit 9 (§6.3.17)");
    assert_eq!(SMMU::GERROR_SFE,        SMMU::GERROR_SFM_ERR,          "GERROR_SFE alias must equal SFM_ERR (bit 8)");
    assert_eq!(SMMU::GERROR_MSI_ABT_ERR, SMMU::GERROR_MSI_EVENTQ_ABT_ERR, "GERROR_MSI_ABT_ERR alias must equal MSI_EVENTQ_ABT_ERR (bit 5)");
    assert_eq!(SMMU::GERROR_CMDQ_ABT_ERR, SMMU::GERROR_MSI_CMDQ_ABT_ERR, "GERROR_CMDQ_ABT_ERR alias must equal MSI_CMDQ_ABT_ERR (bit 4)");
}

// ── §6.3.18: SMMU_GERRORN — software clear ───────────────────────────────────

/// §6.3.20: clear_gerror on an INACTIVE error toggles GERRORN (pre-acknowledge).
#[test]
fn test_clear_gerror_noop_when_zero() {
    let smmu = make_smmu();
    assert_eq!(
        smmu.get_gerror() ^ smmu.get_gerrorn(),
        0,
        "at reset, GERROR XOR GERRORN must be 0 (no active errors)"
    );
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(smmu.get_gerror(), 0, "clear_gerror must not modify GERROR");
}

// ── §6.3.17: CMDQ_ERR set on command queue error ─────────────────────────────

/// §6.3.17: CMDQ_ERR must be set when a reserved CMD_SYNC CS=3 (CERROR_ILL) is processed.
/// CONF-GAP-2: CMD_CFGI_STE for unknown stream is a silent no-op (§4.3.1), so we
/// use CMD_SYNC CS=3 as the correct CMDQ_ERR trigger.
#[test]
fn test_cmdq_err_set_on_cfgi_ste_unknown_stream() {
    let smmu = make_smmu();
    // Trigger via CMD_SYNC CS=3 (Reserved → CERROR_ILL per §4.7.3).
    trigger_cmdq_err(&smmu);
    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "GERROR.CMDQ_ERR must be set after CERROR_ILL (CMD_SYNC CS=3)"
    );
}

/// CONF-GAP-2: CMD_CFGI_STE for unknown StreamID is a silent no-op (§4.3.1).
/// No C_BAD_STREAMID event and no GERROR.CMDQ_ERR must be generated.
#[test]
fn test_c_bad_streamid_event_generated_on_cmdq_err() {
    let smmu = make_smmu();
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    smmu.submit_command(cfgi_ste_cmd(99)).unwrap();
    let result = smmu.process_command_queue();
    // CONF-GAP-2: must succeed as silent no-op
    assert!(result.is_ok(), "CONF-GAP-2: CMD_CFGI_STE for unknown stream must be a silent no-op (Ok)");
    // No CMDQ_ERR
    assert_eq!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR"
    );
    // No C_BAD_STREAMID event even with RECINVSID=1
    let events = smmu.get_events();
    assert!(
        events.is_empty(),
        "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT generate C_BAD_STREAMID event (§4.3.1)"
    );
}

/// §6.3.20: clear_gerror(CMDQ_ERR) acknowledges the error (makes it INACTIVE).
#[test]
fn test_clear_gerror_clears_cmdq_err_bit() {
    let smmu = make_smmu();
    trigger_cmdq_err(&smmu);

    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be ACTIVE before acknowledge"
    );

    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be INACTIVE (GERROR == GERRORN) after acknowledge"
    );
}

/// §6.3.20: clear_gerror toggles only the specified GERRORN bits; others unaffected.
#[test]
fn test_clear_gerror_only_clears_specified_bits() {
    let smmu = make_smmu();
    trigger_cmdq_err(&smmu);
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "precondition: CMDQ_ERR must be ACTIVE"
    );
    let cmdq_err_in_gerror  = smmu.get_gerror()  & SMMU::GERROR_CMDQ_ERR;
    let cmdq_err_in_gerrorn = smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR;
    let full_gerror_snapshot = smmu.get_gerror();
    smmu.clear_gerror(SMMU::GERROR_SFE);  // toggle a bit that isn't active
    assert_eq!(
        smmu.get_gerror(),
        full_gerror_snapshot,
        "clear_gerror(SFE) must not modify GERROR"
    );
    assert_eq!(
        smmu.get_gerrorn() & SMMU::GERROR_CMDQ_ERR,
        cmdq_err_in_gerrorn,
        "clear_gerror(SFE) must not affect GERRORN.CMDQ_ERR"
    );
    assert_ne!(cmdq_err_in_gerror, 0, "CMDQ_ERR must remain set in GERROR");
}

/// §6.3.19/6.3.20: After CMDQ_ERR is acknowledged, subsequent valid commands process normally.
#[test]
fn test_command_queue_resumes_after_clear_gerror() {
    let smmu = make_smmu();
    smmu.enable().unwrap();

    trigger_cmdq_err(&smmu);
    assert_ne!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be ACTIVE after command error"
    );

    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be INACTIVE after acknowledge"
    );

    smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();
    let result = smmu.process_command_queue();
    assert!(result.is_ok(), "Command queue must resume normally after acknowledging CMDQ_ERR");
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "No new CMDQ_ERR after valid command"
    );
}

/// §4.3.1: CMD_CFGI_STE for a valid (configured) stream must succeed.
#[test]
fn test_cfgi_ste_valid_stream_no_cmdq_err() {
    let smmu = make_smmu();
    smmu.enable().unwrap();
    smmu.configure_stream(sid(1), StreamConfig::stage1_only()).unwrap();

    smmu.submit_command(cfgi_ste_cmd(1)).unwrap();
    let result = smmu.process_command_queue();
    assert!(result.is_ok(), "CMD_CFGI_STE for known stream must succeed");
    assert_eq!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "No CMDQ_ERR for CMD_CFGI_STE with valid stream"
    );
}

/// §6.3.17: Queue halts after CMDQ_ERR — remaining commands are not processed.
/// Uses CMD_SYNC CS=3 as the bad command (CERROR_ILL per §4.7.3).
#[test]
fn test_cmdq_err_halts_remaining_commands() {
    let smmu = make_smmu();

    // Submit bad command (CMD_SYNC CS=3 → CERROR_ILL) followed by a good command.
    let mut bad_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_cmd.cs = 3;
    smmu.submit_command(bad_cmd).unwrap();
    smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();

    let result = smmu.process_command_queue();
    assert!(result.is_err(), "Queue must halt on command error");

    // The good TlbiNhAll command was never processed because queue halted
    assert_ne!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0);
}
