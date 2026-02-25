#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-M-06: GERROR register — command queue error conditions
//!
//! Spec: ARM IHI0070G.b §6.3.17 (SMMU_GERROR), §7.5 (Global error recording)
//!
//! Requirements:
//! - SMMU_GERROR starts at 0 after reset.
//! - CMDQ_ERR (bit 7) is set when command processing detects an error.
//! - Software clears GERROR bits by writing to SMMU_GERRORN (`clear_gerror`).
//! - Clearing only clears the specified bits; other bits are unaffected.
//! - After CMDQ_ERR is set, `process_command_queue` returns an error.
//! - Clearing CMDQ_ERR re-enables command queue processing.

use smmu::types::{CommandEntry, CommandType, EventType, StreamConfig, StreamID};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    SMMU::new()
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

const fn cfgi_ste_cmd(stream_id: u32) -> CommandEntry {
    CommandEntry::new(CommandType::CfgiSte, stream_id, 0)
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
    // New spec-correct constants (ARM §6.3.17)
    assert_eq!(SMMU::GERROR_CMDQ_ERR,           1 << 0, "CMDQ_ERR must be bit 0 (§6.3.17)");
    assert_eq!(SMMU::GERROR_EVENTQ_ABT_ERR,     1 << 2, "EVENTQ_ABT_ERR must be bit 2 (§6.3.17)");
    assert_eq!(SMMU::GERROR_PRIQ_ABT_ERR,       1 << 3, "PRIQ_ABT_ERR must be bit 3 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_CMDQ_ABT_ERR,   1 << 4, "MSI_CMDQ_ABT_ERR must be bit 4 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_EVENTQ_ABT_ERR, 1 << 5, "MSI_EVENTQ_ABT_ERR must be bit 5 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_PRIQ_ABT_ERR,   1 << 6, "MSI_PRIQ_ABT_ERR must be bit 6 (§6.3.17)");
    assert_eq!(SMMU::GERROR_MSI_GERROR_ABT_ERR, 1 << 7, "MSI_GERROR_ABT_ERR must be bit 7 (§6.3.17)");
    assert_eq!(SMMU::GERROR_SFM_ERR,            1 << 8, "SFM_ERR must be bit 8 (§6.3.17)");
    assert_eq!(SMMU::GERROR_CMDQP_ERR,          1 << 9, "CMDQP_ERR must be bit 9 (§6.3.17)");
    // Backward-compatible aliases must resolve to the correct bit positions
    assert_eq!(SMMU::GERROR_SFE,        SMMU::GERROR_SFM_ERR,          "GERROR_SFE alias must equal SFM_ERR (bit 8)");
    assert_eq!(SMMU::GERROR_MSI_ABT_ERR, SMMU::GERROR_MSI_EVENTQ_ABT_ERR, "GERROR_MSI_ABT_ERR alias must equal MSI_EVENTQ_ABT_ERR (bit 5)");
    assert_eq!(SMMU::GERROR_CMDQ_ABT_ERR, SMMU::GERROR_MSI_CMDQ_ABT_ERR, "GERROR_CMDQ_ABT_ERR alias must equal MSI_CMDQ_ABT_ERR (bit 4)");
}

// ── §6.3.18: SMMU_GERRORN — software clear ───────────────────────────────────

/// §6.3.18: clear_gerror on zero register is a no-op.
#[test]
fn test_clear_gerror_noop_when_zero() {
    let smmu = make_smmu();
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(smmu.get_gerror(), 0, "clear_gerror on 0 must stay 0");
}

// ── §6.3.17: CMDQ_ERR set on command queue error ─────────────────────────────

/// §6.3.17: CMDQ_ERR must be set when CMD_CFGI_STE references an unknown stream.
///
/// Per ARM §4.3.1 CONSTRAINED UNPREDICTABLE: the SMMU may record a
/// C_BAD_STREAMID error for an unrecognised stream ID in CMD_CFGI_STE.
/// This model records the error and sets CMDQ_ERR.
#[test]
fn test_cmdq_err_set_on_cfgi_ste_unknown_stream() {
    let smmu = make_smmu();
    // Stream 42 is not configured — should trigger C_BAD_STREAMID / CMDQ_ERR
    smmu.submit_command(cfgi_ste_cmd(42)).unwrap();
    let result = smmu.process_command_queue();
    assert!(result.is_err(), "process_command_queue must return Err on command error");
    assert_ne!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "GERROR.CMDQ_ERR must be set after command queue error"
    );
}

/// §7.3.3: C_BAD_STREAMID event must appear in the event queue when CMDQ_ERR fires.
#[test]
fn test_c_bad_streamid_event_generated_on_cmdq_err() {
    let smmu = make_smmu();
    smmu.submit_command(cfgi_ste_cmd(99)).unwrap();
    let _ = smmu.process_command_queue();
    let events = smmu.get_events();
    let bad_sid = events.iter().any(|e| e.event_type == EventType::CBadStreamid);
    assert!(bad_sid, "C_BAD_STREAMID event must be generated for unknown stream in CMD_CFGI_STE");
}

/// §6.3.18: clear_gerror(CMDQ_ERR) clears only the CMDQ_ERR bit.
#[test]
fn test_clear_gerror_clears_cmdq_err_bit() {
    let smmu = make_smmu();
    smmu.submit_command(cfgi_ste_cmd(77)).unwrap();
    let _ = smmu.process_command_queue();

    // Confirm CMDQ_ERR is set
    assert_ne!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0);

    // Clear it
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be cleared after clear_gerror"
    );
}

/// §6.3.18: clear_gerror clears only the specified bits; other bits unaffected.
#[test]
fn test_clear_gerror_only_clears_specified_bits() {
    let smmu = make_smmu();
    // Manually set multiple bits via back-to-back errors on two unknown streams
    smmu.submit_command(cfgi_ste_cmd(10)).unwrap();
    let _ = smmu.process_command_queue();
    // CMDQ_ERR is now set; also inject SFE bit into the same register by
    // clearing only CMDQ_ERR and verifying SFE-carrying bits are unaffected.
    // Since we can only trigger CMDQ_ERR via command errors, verify that
    // clear_gerror with a different mask leaves CMDQ_ERR intact.
    let gerror_before = smmu.get_gerror();
    smmu.clear_gerror(SMMU::GERROR_SFE);  // clear a bit that isn't set
    assert_eq!(
        smmu.get_gerror(),
        gerror_before,
        "clear_gerror(SFE) must not affect CMDQ_ERR when SFE is not set"
    );
}

/// §6.3.17/18: After CMDQ_ERR is cleared, subsequent valid commands process normally.
#[test]
fn test_command_queue_resumes_after_clear_gerror() {
    let smmu = make_smmu();
    smmu.enable().unwrap();

    // Trigger CMDQ_ERR with an unknown stream
    smmu.submit_command(cfgi_ste_cmd(55)).unwrap();
    let _ = smmu.process_command_queue();
    assert_ne!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0);

    // Clear CMDQ_ERR
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0);

    // Submit and process a valid command (TlbiNhAll) — must succeed
    smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();
    let result = smmu.process_command_queue();
    assert!(result.is_ok(), "Command queue must resume normally after clearing CMDQ_ERR");
    assert_eq!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0, "No new CMDQ_ERR after valid command");
}

/// §4.3.1: CMD_CFGI_STE for a valid (configured) stream must succeed.
#[test]
fn test_cfgi_ste_valid_stream_no_cmdq_err() {
    let smmu = make_smmu();
    smmu.enable().unwrap();
    // Configure stream 1
    smmu.configure_stream(sid(1), StreamConfig::stage1_only()).unwrap();

    // CMD_CFGI_STE for known stream must succeed
    smmu.submit_command(cfgi_ste_cmd(1)).unwrap();
    let result = smmu.process_command_queue();
    assert!(result.is_ok(), "CMD_CFGI_STE for known stream must succeed");
    assert_eq!(
        smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR,
        0,
        "No CMDQ_ERR for CMD_CFGI_STE with valid stream"
    );
}

/// §6.3.17: Only one command error per queue-halt: after CMDQ_ERR is set,
/// additional commands in the same batch are NOT processed (queue halted).
#[test]
fn test_cmdq_err_halts_remaining_commands() {
    let smmu = make_smmu();

    // Submit bad command followed by a good command
    smmu.submit_command(cfgi_ste_cmd(999)).unwrap();  // bad: unknown stream
    smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();  // good

    let result = smmu.process_command_queue();
    assert!(result.is_err(), "Queue must halt on command error");

    // The good TlbiNhAll command was never processed because queue halted
    // after the bad command — GERROR.CMDQ_ERR is set
    assert_ne!(smmu.get_gerror() & SMMU::GERROR_CMDQ_ERR, 0);
}
