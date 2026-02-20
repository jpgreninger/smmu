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

/// Build a CMD_RESUME command for a given STAG.
const fn resume_cmd(stag: u16) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::Resume,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag,
        prg_index: 0,
    }
}

/// Build a CMD_STALL_TERM command for a given STAG.
const fn stall_term_cmd(stag: u16) -> CommandEntry {
    CommandEntry {
        cmd_type: CommandType::StallTerm,
        stream_id: 0,
        pasid: 0,
        start_address: 0,
        end_address: 0,
        flags: 0,
        timestamp: 0,
        asid: 0,
        vmid: 0,
        stag,
        prg_index: 0,
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

    smmu.submit_command(resume_cmd(stag)).unwrap();
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
    smmu.submit_command(resume_cmd(wrong_stag)).unwrap();
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

    smmu.submit_command(stall_term_cmd(stag)).unwrap();
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
