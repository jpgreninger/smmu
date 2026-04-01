#![allow(clippy::doc_markdown)]
//! ARM SMMU v3 FINDING-NEW-26 and FINDING-NEW-27 spec tests
//!
//! Tests for:
//! - FINDING-NEW-26: Stall `EventEntry` missing `stag` field (§3.12.2, §7.3)
//! - FINDING-NEW-27: CMD_SYNC CS field not modeled; SIG_NONE generates spurious event (§4.8)

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, SecurityState, StreamConfig,
    TranslationError, StreamID, PASID, IOVA,
};
use smmu::SMMU;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

fn sid(n: u32) -> StreamID { StreamID::new(n).unwrap() }
fn pasid(n: u32) -> PASID { PASID::new(n).unwrap() }
fn iova(addr: u64) -> IOVA { IOVA::new(addr).unwrap() }

fn stall_stream(smmu: &SMMU, stream_n: u32) {
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(sid(stream_n), cfg).unwrap();
    smmu.create_pasid(sid(stream_n), pasid(0)).unwrap();
}

fn terminate_stream(smmu: &SMMU, stream_n: u32) {
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(stream_n), cfg).unwrap();
    smmu.create_pasid(sid(stream_n), pasid(0)).unwrap();
}

fn count_sync_completion_events(smmu: &SMMU) -> usize {
    smmu.get_events()
        .iter()
        .filter(|e| e.event_type == EventType::CommandSyncCompletion)
        .count()
}

// ── FINDING-NEW-26: Stall EventEntry must carry the STAG field (§3.12.2) ────

/// When a translation fault stalls, the resulting `EventEntry` must have a non-zero `stag`.
#[test]
fn stall_event_entry_has_nonzero_stag() {
    let smmu = make_smmu();
    stall_stream(&smmu, 0xC0);

    // Trigger a stall fault with an unmapped address
    let result = smmu.translate(sid(0xC0), pasid(0), iova(0x5000),
                                AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::Stalled { .. })),
        "translation must stall, got: {result:?}"
    );

    // Verify the EventEntry in the event queue has stag != 0
    let events = smmu.get_events();
    let stall_event = events.iter().find(|e| e.stall);
    assert!(stall_event.is_some(), "a stall event must be present in the event queue");
    assert_ne!(
        stall_event.unwrap().stag,
        0,
        "§3.12.2: EventEntry.stag must be non-zero for stall events"
    );
}

/// The `stag` in the `EventEntry` must match the `STAG` from the `Stalled` error.
#[test]
fn stall_event_stag_matches_error_stag() {
    let smmu = make_smmu();
    stall_stream(&smmu, 0xC1);

    let result = smmu.translate(sid(0xC1), pasid(0), iova(0x6000),
                                AccessType::Read, SecurityState::NonSecure);
    let stag_from_err = match result {
        Err(TranslationError::Stalled { stag }) => stag,
        other => panic!("expected Stalled error, got: {other:?}"),
    };

    // STAG from error must match the stag in the event queue
    let events = smmu.get_events();
    let stall_event = events.iter().find(|e| e.stall).expect("stall event must exist");
    assert_eq!(
        stall_event.stag, stag_from_err,
        "§3.12.2: EventEntry.stag must match the STAG returned in the Stalled error"
    );
}

/// Non-stall events must have `stag==0` (no stall tag applies).
#[test]
fn non_stall_event_has_zero_stag() {
    let smmu = make_smmu();
    terminate_stream(&smmu, 0xC2);

    // Unmapped address — F_TRANSLATION (no stall)
    let _ = smmu.translate(sid(0xC2), pasid(0), iova(0x7000),
                           AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    assert!(!events.is_empty(), "at least one event must be recorded");
    for ev in &events {
        if !ev.stall {
            assert_eq!(ev.stag, 0, "non-stall EventEntry must have stag==0");
        }
    }
}

// ── FINDING-NEW-27: CMD_SYNC CS field must control completion signal (§4.8) ──

/// CMD_SYNC with CS=0 (SIG_NONE) must NOT generate a `COMMAND_SYNC_COMPLETION` event.
#[test]
fn sync_cs_sig_none_produces_no_completion_event() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0x00; // SIG_NONE
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    assert_eq!(
        count_sync_completion_events(&smmu),
        0,
        "§4.8: CMD_SYNC with CS=SIG_NONE must not generate COMMAND_SYNC_COMPLETION"
    );
}

/// CMD_SYNC with CS=1 (SIG_IRQ) must generate exactly one `COMMAND_SYNC_COMPLETION` event.
#[test]
fn sync_cs_sig_irq_produces_one_completion_event() {
    let smmu = make_smmu();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0x01; // SIG_IRQ
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    assert_eq!(
        count_sync_completion_events(&smmu),
        1,
        "§4.8: CMD_SYNC with CS=SIG_IRQ must generate exactly one COMMAND_SYNC_COMPLETION"
    );
}

/// CMD_SYNC with CS=2 (SIG_SEV) must also generate a completion event when IDR0.SEV=1.
#[test]
fn sync_cs_sig_msi_produces_one_completion_event() {
    let smmu = make_smmu();
    // BUG-AUDIT-65: CS=2 (SIG_SEV) only generates a completion event when IDR0.SEV=1 (ARM §4.7.3).
    smmu.set_sev_supported(true);

    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 0x02; // SIG_SEV
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    assert_eq!(
        count_sync_completion_events(&smmu),
        1,
        "§4.8: CMD_SYNC with CS=SIG_SEV must generate exactly one COMMAND_SYNC_COMPLETION"
    );
}

/// Mixed CS values: only syncs with CS!=0 contribute completion events.
#[test]
fn sync_mixed_cs_values_correct_event_count() {
    let smmu = make_smmu();

    // SIG_NONE, SIG_IRQ, SIG_NONE → expect exactly 1 completion event
    for cs in [0x00_u8, 0x01, 0x00] {
        let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
        cmd.cs = cs;
        smmu.submit_command(cmd).expect("submit_command must succeed");
        smmu.process_command_queue().expect("process_command_queue must succeed");
    }

    assert_eq!(
        count_sync_completion_events(&smmu),
        1,
        "§4.8: only CMD_SYNC with CS!=SIG_NONE must generate completion events"
    );
}

/// Default `CommandEntry.cs` is 0 (SIG_NONE) so a plain Sync produces no completion event.
#[test]
fn sync_default_cs_zero_produces_no_completion_event() {
    let smmu = make_smmu();

    let cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    assert_eq!(cmd.cs, 0, "default CommandEntry.cs must be 0 (SIG_NONE)");
    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    assert_eq!(
        count_sync_completion_events(&smmu),
        0,
        "§4.8: default CMD_SYNC (cs=0) must not generate COMMAND_SYNC_COMPLETION"
    );
}
