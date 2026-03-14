#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-H-05: CMD_RESUME stall model with STAG tracking.
//!
//! Spec: ARM IHI0070G.b §3.12.2 (Stall fault model), §4.6 (CMD_RESUME),
//!       §4.7 (CMD_STALL_TERM), §7.3 (event types)
//!
//! Requirements:
//! - FaultMode::Stall in StreamConfig causes faulting translations to stall
//!   instead of immediately returning an error.
//! - A stalled translation returns Err(TranslationError::Stalled { stag }) where
//!   stag is a unique per-fault Stall TAG.
//! - The SMMU maintains a stall queue keyed by STAG.
//! - CMD_RESUME (opcode 0x44) with matching STAG completes the stall: the record
//!   is removed from the stall queue.
//! - CMD_STALL_TERM (opcode 0x45) with matching STAG aborts the stall: the record
//!   is removed from the stall queue.
//! - Streams with FaultMode::Terminate still return an immediate fault (not stalled).
//! - Two distinct faults on the same stall-mode stream produce different STAGs.
//! - get_stalled_transactions() returns all pending stall records.

use smmu::types::{
    AccessType, CommandEntry, CommandType, FaultMode, PagePermissions, SecurityState, StreamConfig,
    StreamID, TranslationError, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}
fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}
fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}
fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

/// Configure a stall-mode Stage-1 stream with one mapped page.
fn setup_stall_stream(smmu: &SMMU, stream_n: u32, iova_addr: u64, pa_addr: u64) {
    let s = sid(stream_n);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    smmu.map_page(
        s,
        pasid(0),
        iova(iova_addr),
        pa(pa_addr),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
}

/// Build a CMD_RESUME command (Ac=0, Ab=0: terminate successfully).
const fn resume_cmd(stag: u16, stream_id: u32) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::Resume,
        stream_id,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag,
        prg_index: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        tg: 0,
        num: 0,
        scale: 0,
        ttl: 0,
        ril: false,
        security_state: SecurityState::NonSecure,
    }
}

/// Build a CMD_RESUME command with Ac=1 (retry).
const fn resume_cmd_retry(stag: u16, stream_id: u32) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::Resume,
        stream_id,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag,
        prg_index: 0,
        action: true,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        tg: 0,
        num: 0,
        scale: 0,
        ttl: 0,
        ril: false,
        security_state: SecurityState::NonSecure,
    }
}

/// Build a CMD_RESUME command with Ac=0, Ab=1 (abort with bus error).
const fn resume_cmd_abort(stag: u16, stream_id: u32) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::Resume,
        stream_id,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag,
        prg_index: 0,
        action: false,
        abort: true,
        range: 31,
        leaf: false,
        cs: 0,
        tg: 0,
        num: 0,
        scale: 0,
        ttl: 0,
        ril: false,
        security_state: SecurityState::NonSecure,
    }
}

/// Build a CMD_STALL_TERM command for a given STAG.
const fn stall_term_cmd(stag: u16, stream_id: u32) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::StallTerm,
        stream_id,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag,
        prg_index: 0,
        action: false,
        abort: false,
        range: 31,
        leaf: false,
        cs: 0,
        tg: 0,
        num: 0,
        scale: 0,
        ttl: 0,
        ril: false,
        security_state: SecurityState::NonSecure,
    }
}

// ── StreamConfig stall_mode field ─────────────────────────────────────────────

/// StreamConfigBuilder must accept fault_mode(FaultMode::Stall).
#[test]
fn test_stream_config_builder_stall_mode() {
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    assert_eq!(config.fault_mode, FaultMode::Stall);
}

/// Factory presets must default fault_mode to Terminate.
#[test]
fn test_stream_config_factory_default_terminate_mode() {
    assert_eq!(
        StreamConfig::stage1_only().fault_mode,
        FaultMode::Terminate,
        "stage1_only must default to Terminate mode"
    );
    assert_eq!(
        StreamConfig::bypass().fault_mode,
        FaultMode::Terminate,
        "bypass must default to Terminate mode"
    );
}

// ── Stall mode: faulting translation returns Stalled ─────────────────────────

/// On a stall-mode stream, a faulting translation (unmapped page) must return
/// Err(TranslationError::Stalled { stag }) instead of an immediate fault.
#[test]
fn test_stall_mode_fault_returns_stalled_error() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    // No page mapped — translation must stall, not terminate.
    let unmapped_iova = iova(0xDEAD_0000);
    let result = smmu.translate(s, pasid(0), unmapped_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::Stalled { .. })),
        "Expected Stalled error, got: {:?}",
        result
    );
}

/// On a terminate-mode stream (default), a faulting translation must NOT stall.
#[test]
fn test_terminate_mode_fault_is_not_stalled() {
    let smmu = make_smmu();
    let s = sid(1);
    smmu.configure_stream(s, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    let unmapped_iova = iova(0xDEAD_0000);
    let result = smmu.translate(s, pasid(0), unmapped_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        !matches!(result, Err(TranslationError::Stalled { .. })),
        "Terminate-mode stream must not return Stalled, got: {:?}",
        result
    );
}

/// Successful translations on a stall-mode stream must still succeed.
#[test]
fn test_stall_mode_successful_translation_still_works() {
    let smmu = make_smmu();
    setup_stall_stream(&smmu, 1, 0x1000, 0x2000);
    let result = smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Successful translation on stall-mode stream must work: {:?}", result);
}

// ── Stall queue ───────────────────────────────────────────────────────────────

/// After a stalled fault, get_stalled_transactions() must return one entry.
#[test]
fn test_stall_queue_populated_after_stall_fault() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let _ = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stalled = smmu.get_stalled_transactions();
    assert_eq!(stalled.len(), 1, "Stall queue must contain exactly one record after a stall fault");
}

/// get_stalled_transactions() must be empty before any stall fault occurs.
#[test]
fn test_stall_queue_empty_initially() {
    let smmu = make_smmu();
    assert!(smmu.get_stalled_transactions().is_empty(), "Stall queue must be empty on new SMMU");
}

// ── STAG uniqueness ───────────────────────────────────────────────────────────

/// Two distinct faults on the same stall-mode stream must produce different STAGs.
#[test]
fn test_two_faults_produce_different_stags() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let r1 = smmu.translate(s, pasid(0), iova(0xAAAA_0000), AccessType::Read, SecurityState::NonSecure);
    let r2 = smmu.translate(s, pasid(0), iova(0xBBBB_0000), AccessType::Read, SecurityState::NonSecure);

    let stag1 = match r1 {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    let stag2 = match r2 {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    assert_ne!(stag1, stag2, "Two distinct faults must produce different STAGs");
    assert_eq!(smmu.get_stalled_transactions().len(), 2);
}

// ── CMD_RESUME ────────────────────────────────────────────────────────────────

/// CMD_RESUME with matching STAG must remove the stall record from the queue.
#[test]
fn test_resume_command_clears_stall_queue() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };

    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    smmu.submit_command(resume_cmd(stag, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "Stall queue must be empty after CMD_RESUME"
    );
}

/// CMD_RESUME with wrong STAG must leave the stall record in the queue.
#[test]
fn test_resume_with_wrong_stag_leaves_queue_intact() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };

    // Send Resume with a different STAG (wrong one)
    let wrong_stag = stag.wrapping_add(1);
    smmu.submit_command(resume_cmd(wrong_stag, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_stalled_transactions().len(),
        1,
        "Wrong STAG resume must not remove stall record"
    );
}

// ── CMD_STALL_TERM ────────────────────────────────────────────────────────────

/// CMD_STALL_TERM with matching STAG must remove the stall record (abort).
#[test]
fn test_stall_term_command_aborts_stalled_transaction() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };

    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    smmu.submit_command(stall_term_cmd(stag, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "Stall queue must be empty after CMD_STALL_TERM"
    );
}

// ── abort_stalled_transaction API ─────────────────────────────────────────────

/// abort_stalled_transaction(stag) public API must remove the matching record.
#[test]
fn test_abort_stalled_transaction_api() {
    let smmu = make_smmu();
    let s = sid(1);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };

    let removed = smmu.abort_stalled_transaction(stag);
    assert!(removed, "abort_stalled_transaction must return true for a known STAG");
    assert!(smmu.get_stalled_transactions().is_empty());
}

/// abort_stalled_transaction(unknown_stag) must return false (no-op).
#[test]
fn test_abort_unknown_stag_returns_false() {
    let smmu = make_smmu();
    let removed = smmu.abort_stalled_transaction(0xFFFF);
    assert!(!removed, "abort_stalled_transaction must return false for unknown STAG");
}

// ── FINDING-NEW-04: CMD_RESUME Action/Abort parameters (ARM §4.6, Table 4-10) ─

/// CMD_RESUME with Ac=1 (action=true, retry) must clear the stall record.
/// Spec: ARM §4.6 — Ac=1: transaction retried as if freshly arrived.
#[test]
fn test_resume_ac1_retry_clears_stall_record() {
    let smmu = make_smmu();
    let s = sid(10);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    smmu.submit_command(resume_cmd_retry(stag, 10)).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "Ac=1 (retry) CMD_RESUME must remove the stall record"
    );
}

/// CMD_RESUME with Ac=0, Ab=0 (terminate successfully) must clear the stall record.
/// Spec: ARM §4.6 — Ac=0, Ab=0: terminate successfully (RAZ/WI).
#[test]
fn test_resume_ac0_ab0_terminate_clears_stall_record() {
    let smmu = make_smmu();
    let s = sid(11);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    // resume_cmd uses action=false, abort=false (Ac=0, Ab=0)
    smmu.submit_command(resume_cmd(stag, 11)).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "Ac=0, Ab=0 CMD_RESUME must remove the stall record"
    );
}

/// CMD_RESUME with Ac=0, Ab=1 (abort with bus error) must clear the stall record.
/// Spec: ARM §4.6 — Ac=0, Ab=1: abort, transaction terminated with bus error.
#[test]
fn test_resume_ac0_ab1_abort_clears_stall_record() {
    let smmu = make_smmu();
    let s = sid(12);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    smmu.submit_command(resume_cmd_abort(stag, 12)).unwrap();
    smmu.process_command_queue().unwrap();

    assert!(
        smmu.get_stalled_transactions().is_empty(),
        "Ac=0, Ab=1 CMD_RESUME must remove the stall record"
    );
}

/// CommandEntry for CMD_RESUME must carry action and abort fields per ARM §4.6.
#[test]
fn test_resume_command_entry_has_action_abort_fields() {
    // action=true → Ac=1 (retry)
    let retry_cmd = resume_cmd_retry(1, 0);
    assert!(retry_cmd.action, "action must be true for Ac=1 retry");
    assert!(!retry_cmd.abort, "abort must be false for Ac=1 retry");

    // action=false, abort=false → Ac=0, Ab=0 (terminate success)
    let term_cmd = resume_cmd(1, 0);
    assert!(!term_cmd.action, "action must be false for Ac=0");
    assert!(!term_cmd.abort, "abort must be false for Ab=0 terminate success");

    // action=false, abort=true → Ac=0, Ab=1 (abort)
    let abort_cmd = resume_cmd_abort(1, 0);
    assert!(!abort_cmd.action, "action must be false for Ac=0");
    assert!(abort_cmd.abort, "abort must be true for Ab=1");
}

// ── FINDING-NEW-10: CMD_RESUME STAG/StreamID verification (ARM §4.6) ──────────

/// CMD_RESUME with correct STAG but wrong StreamID must be a no-op (ARM §4.6).
/// Spec §4.6: "If the transaction does not match the given StreamID, this command
/// has no effect."
#[test]
fn test_resume_wrong_stream_id_is_noop() {
    let smmu = make_smmu();
    let s = sid(20);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Stall a translation on stream 20
    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    // Send CMD_RESUME with correct STAG but wrong StreamID (21, not 20)
    smmu.submit_command(resume_cmd(stag, 21)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_stalled_transactions().len(),
        1,
        "CMD_RESUME with wrong StreamID must not retire the stall record (ARM §4.6)"
    );

    // Now confirm the correct StreamID does retire it
    smmu.submit_command(resume_cmd(stag, 20)).unwrap();
    smmu.process_command_queue().unwrap();
    assert!(smmu.get_stalled_transactions().is_empty(), "Correct StreamID must retire the record");
}

/// CMD_STALL_TERM with correct STAG but wrong StreamID must be a no-op (ARM §4.6 / §4.7).
#[test]
fn test_stall_term_wrong_stream_id_is_noop() {
    let smmu = make_smmu();
    let s = sid(21);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    let result = smmu.translate(s, pasid(0), iova(0xDEAD_0000), AccessType::Read, SecurityState::NonSecure);
    let stag = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("Expected Stalled, got {:?}", other),
    };
    assert_eq!(smmu.get_stalled_transactions().len(), 1);

    // Send CMD_STALL_TERM with wrong StreamID (99, not 21)
    smmu.submit_command(stall_term_cmd(stag, 99)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_stalled_transactions().len(),
        1,
        "CMD_STALL_TERM with wrong StreamID must not retire the stall record (ARM §4.6/§4.7)"
    );

    // Correct StreamID must retire it
    smmu.submit_command(stall_term_cmd(stag, 21)).unwrap();
    smmu.process_command_queue().unwrap();
    assert!(smmu.get_stalled_transactions().is_empty(), "Correct StreamID must retire the record");
}
