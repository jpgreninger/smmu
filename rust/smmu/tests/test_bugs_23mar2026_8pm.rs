//! Failing tests (TDD) for two confirmed ARM SMMU v3 Rust bugs (2026-03-23 8pm batch).
//!
//! Each test is designed to FAIL with the current buggy code and PASS only
//! after the corresponding fix is applied.
//!
//! # Bug Descriptions
//!
//! ## BUG-NEW-6: Erroneous `cmdq_prod.store()` in `process_command_queue()`
//!
//! Location: `smmu/mod.rs` — `process_command_queue()` error-handling branch.
//!
//! When a command causes `CERROR_ILL`, the error path resets `CMDQ_PROD` to the
//! current `CMDQ_CONS` value.  In the ARM SMMU v3 architecture, `CMDQ_PROD` is a
//! **software-driven** register: only the driver/OS writes it; the SMMU hardware
//! only advances `CMDQ_CONS`.  The SMMU writing back to `CMDQ_PROD` corrupts the
//! externally visible register state.
//!
//! `is_command_queue_full()` already uses `queue.len() >= capacity` (not
//! `cmdq_prod`), so removing the store has no internal side effects — it only
//! stops poisoning the register.
//!
//! Observable symptom: after submitting N valid commands followed by 1 illegal
//! command, `cmdq_prod_index()` is corrupted (reset to CONS) rather than
//! remaining at N+1 as software set it.
//!
//! ## BUG-NEW-7: pop-before-emit in `process_pri_queue()`
//!
//! Location: `smmu/mod.rs` — `process_pri_queue()`.
//!
//! The PRIEntry is popped from the queue *before* `submit_event()` is called.
//! If the event queue is full, `submit_event()` returns `Err(EventQueueFull)`
//! (silently ignored with `let _ = ...`), but the PRIEntry has already been
//! removed.  The `E_PAGE_REQUEST` event is never emitted — a silent loss.
//!
//! Fix: peek the front entry, emit the event, and only pop on success (same
//! pattern as the `process_command_queue()` peek-then-pop fix).
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventEntry, EventType, PRIEntry, QueueConfig, SMMUConfig,
    StreamConfig, StreamID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu_default() -> SMMU {
    SMMU::new()
}

/// Create an SMMU with a very small event queue so we can fill it cheaply.
fn make_smmu_tiny_eventq(event_queue_size: usize) -> SMMU {
    let cfg = SMMUConfig {
        queue_config: QueueConfig {
            command_queue_size: 256,
            event_queue_size,
            pri_queue_size: 32,
        },
        ..SMMUConfig::default()
    };
    SMMU::with_config(cfg)
}

const fn make_illegal_sync_cmd() -> CommandEntry {
    // CMD_SYNC with CS=3 is Reserved → CERROR_ILL per ARM §4.7.3.
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3;
    cmd
}

// ============================================================================
// BUG-NEW-6: cmdq_prod must NOT be written back by process_command_queue()
// ============================================================================

/// BUG-NEW-6 — failing test.
///
/// After submitting 3 valid commands + 1 illegal command, `CMDQ_PROD` must
/// remain at 4 (reflecting all 4 submissions by software).  The buggy code
/// resets PROD to CONS (= 3, after the 3 good commands are consumed) when the
/// error fires, corrupting the register.
#[test]
fn bug_new_6_cmdq_prod_not_corrupted_by_error_path() {
    let smmu = make_smmu_default();
    smmu.enable().unwrap();
    let sid = StreamID::new(1).unwrap();
    smmu.configure_stream(sid, StreamConfig::stage1_only()).unwrap();

    // Submit 3 valid commands.
    for _ in 0..3 {
        smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0))
            .unwrap();
    }
    // Submit 1 illegal command (CS=3 → CERROR_ILL).
    smmu.submit_command(make_illegal_sync_cmd()).unwrap();

    // Software-written PROD must be 4 (all 4 submissions).
    assert_eq!(
        smmu.cmdq_prod_index(),
        4,
        "BUG-NEW-6 pre-condition: PROD must reflect 4 software submissions"
    );

    // Processing: the first 3 commands succeed (CONS advances to 3).
    // The 4th command triggers CERROR_ILL.
    let _ = smmu.process_command_queue();

    // After the error, PROD must still be 4 — the SMMU must never write PROD.
    // The buggy code resets PROD = CONS = 3, so this assertion will FAIL before the fix.
    assert_eq!(
        smmu.cmdq_prod_index(),
        4,
        "BUG-NEW-6: CMDQ_PROD was corrupted by process_command_queue() error path \
         (reset to CONS instead of staying at 4)"
    );
}

/// BUG-NEW-6 — edge case: single illegal command as the first submission.
///
/// PROD=1 before processing.  After the error, PROD must still be 1.
#[test]
fn bug_new_6_cmdq_prod_not_corrupted_first_command_error() {
    let smmu = make_smmu_default();
    smmu.enable().unwrap();

    smmu.submit_command(make_illegal_sync_cmd()).unwrap();
    assert_eq!(smmu.cmdq_prod_index(), 1, "PROD must be 1 after one submission");

    let _ = smmu.process_command_queue();

    // Buggy code resets PROD=CONS=0 here; fixed code leaves it at 1.
    assert_eq!(
        smmu.cmdq_prod_index(),
        1,
        "BUG-NEW-6: CMDQ_PROD must not be reset to 0 (CONS) after first-command error"
    );
}

// ============================================================================
// BUG-NEW-7: process_pri_queue() must emit event BEFORE popping the entry
// ============================================================================

/// BUG-NEW-7 — failing test.
///
/// Fill the event queue to capacity, then call `process_pri_queue()`.
/// With the buggy pop-before-emit order, the PRIEntry is dequeued but no
/// `E_PAGE_REQUEST` event is ever recorded (submit_event fails silently).
/// The PRI queue becomes empty but the event queue still has no new events.
///
/// After the fix (emit-before-pop), when `submit_event()` fails the entry
/// stays in the PRI queue so the caller can retry after draining the event
/// queue.  The PRI queue must NOT shrink when emission fails.
#[test]
fn bug_new_7_pri_entry_not_lost_when_event_queue_full() {
    // Event queue of size 4 so we can fill it quickly.
    let smmu = make_smmu_tiny_eventq(4);
    smmu.enable().unwrap();

    // Fill the event queue to capacity.
    for i in 0..4_u32 {
        let ev = EventEntry::new(EventType::FTranslation, i, 0, 0);
        smmu.submit_event(ev).unwrap();
    }
    assert_eq!(smmu.get_events().len(), 4, "event queue must be full");

    // Submit one PRI request.
    let req = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    smmu.submit_page_request(req).unwrap();
    assert_eq!(smmu.get_pri_queue_size(), 1, "PRI queue must have 1 entry");

    // Process: event queue is full, so E_PAGE_REQUEST cannot be emitted.
    // BUG: pop-before-emit silently discards the entry.
    // FIX: emit-before-pop keeps the entry in the queue on failure.
    let _ = smmu.process_pri_queue();

    // After the fix, the PRI entry must still be in the queue because
    // the event submission failed.  With the bug, the queue is empty.
    assert_eq!(
        smmu.get_pri_queue_size(),
        1,
        "BUG-NEW-7: PRI entry was silently dropped when event queue was full \
         (pop happened before emit, discarding the entry on submit_event failure)"
    );
}

/// BUG-NEW-7 — success path regression: when the event queue has space,
/// `process_pri_queue()` must still consume the entry (normal operation
/// must not be broken by the fix).
#[test]
fn bug_new_7_pri_entry_consumed_on_success() {
    let smmu = make_smmu_default();
    smmu.enable().unwrap();

    let req = PRIEntry::with_address(2, 0, 0x2000, AccessType::Read);
    smmu.submit_page_request(req).unwrap();
    assert_eq!(smmu.get_pri_queue_size(), 1);

    smmu.process_pri_queue().unwrap();

    // On success the entry must be consumed and an event recorded.
    assert_eq!(
        smmu.get_pri_queue_size(),
        0,
        "BUG-NEW-7 regression: PRI entry must be consumed after successful emit"
    );
    let events = smmu.get_events();
    assert_eq!(
        events.len(),
        1,
        "BUG-NEW-7 regression: one E_PAGE_REQUEST event must be in the event queue"
    );
    assert_eq!(
        events[0].event_type,
        EventType::EPageRequest,
        "BUG-NEW-7 regression: event type must be EPageRequest"
    );
}
