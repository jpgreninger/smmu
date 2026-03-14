#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for two bugs in SMMU queue management:
//!
//! - BUG-RUST-Q2: `reset_queues()` does not clear `stall_pending`.
//!   Per ARM SMMU v3 §3.5.1, initialization must produce a consistent empty
//!   state. `reset_queues()` calls `clear_event_queue()`, `clear_command_queue()`,
//!   and `clear_pri_queue()`, but `clear_event_queue()` does NOT drain
//!   `stall_pending`. Stall events that overflowed into the internal buffer
//!   therefore survive a reset, violating the spec invariant.
//!
//! - BUG-RUST-Q4: `process_single_command()` for `CommandType::PriResp` searches
//!   the entire PRI queue for a matching `(stream_id, prg_index)` entry and
//!   advances `priq_cons` only when `pos == 0`. Per ARM §3.5.1 and §6.3.98 the
//!   PRI queue is STRICT FIFO: `PriResp` MUST only consume the HEAD entry.
//!   Removing an interior entry while leaving the head intact is incorrect, and
//!   not advancing CONS when a non-head entry is removed is the lesser symptom
//!   of the deeper FIFO violation.

use smmu::types::{AccessType, CommandEntry, CommandType, EventEntry, EventType, PRIEntry, QueueConfig, SMMUConfig};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Create an SMMU with all queue-enable bits set so PRI/command operations work.
fn smmu_with_all_queues_enabled() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );
    smmu
}

/// Create an SMMU with a tiny event queue (size 4, allowed by the overflow-test
/// exception in QueueConfig::validate) so that stall events quickly overflow into
/// `stall_pending`.
fn smmu_with_tiny_event_queue() -> SMMU {
    let queue_cfg = QueueConfig::with_event_queue_size(QueueConfig::default(), 4);
    let smmu_cfg = SMMUConfig::from(queue_cfg);
    let smmu = SMMU::with_config(smmu_cfg);
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );
    smmu
}

/// Build a stall `EventEntry` for stream `sid`.
const fn stall_event(sid: u32) -> EventEntry {
    EventEntry {
        event_type: EventType::FTranslation,
        stream_id: sid,
        pasid: 0,
        address: 0x1000,
        security_state: smmu::types::SecurityState::NonSecure,
        error_code: 0,
        timestamp: 0,
        stall: true,
        stag: 1,
        ..EventEntry::zeroed()
    }
}

/// Build a `PRIEntry` for `(stream_id, prg_index)`.
const fn pri_entry(stream_id: u32, prg_index: u16) -> PRIEntry {
    PRIEntry::with_address(stream_id, 0, 0x1000, AccessType::Read).with_prg_index(prg_index)
}

/// Build a `PriResp` command for `(stream_id, prg_index)`.
const fn pri_resp_cmd(stream_id: u32, prg_index: u16) -> CommandEntry {
    let mut cmd = CommandEntry::new(CommandType::PriResp, stream_id, 0);
    cmd.prg_index = prg_index;
    cmd
}

// ============================================================================
// BUG-RUST-Q2: reset_queues() must clear stall_pending
// ============================================================================

/// Fill the (tiny) event queue to capacity with non-stall events, then submit
/// one more stall event — which must go to `stall_pending`. Verify the buffer
/// is non-empty, call `reset_queues()`, and assert the buffer is now empty.
///
/// FAILS before the fix because `reset_queues()` → `clear_event_queue()` does
/// not drain `stall_pending`.
#[test]
fn bug_q2_reset_queues_clears_stall_pending() {
    let smmu = smmu_with_tiny_event_queue();

    // Fill the event queue with non-stall events to capacity (4 slots).
    for sid in 0..4_u32 {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: sid,
            pasid: 0,
            address: 0x2000,
            security_state: smmu::types::SecurityState::NonSecure,
            error_code: 0,
            timestamp: 0,
            stall: false,
            stag: 0,
            ..EventEntry::default()
        };
        // Ignore errors: the queue may be full, which is fine — we just want it full.
        let _ = smmu.submit_event(ev);
    }

    // Queue is now full. Submit a stall event — it must overflow to stall_pending.
    let _ = smmu.submit_event(stall_event(99));

    // Confirm stall_pending has at least one entry before reset.
    let pending_before = smmu.get_pending_stall_count();
    assert!(
        pending_before > 0,
        "BUG-Q2 precondition: stall_pending must be non-empty after overflow (got {pending_before})",
    );

    // Now reset all queues.
    smmu.reset_queues();

    // After reset, stall_pending must be empty per ARM §3.5.1.
    let pending_after = smmu.get_pending_stall_count();
    assert_eq!(
        pending_after,
        0,
        "BUG-Q2: reset_queues() must clear stall_pending (spec §3.5.1 empty state), \
         but {pending_after} entries remain",
    );
}

/// Regression: after a reset the main event queue itself must also be empty.
#[test]
fn bug_q2_reset_queues_clears_event_queue_too() {
    let smmu = smmu_with_tiny_event_queue();

    // Fill with non-stall events.
    for sid in 0..4_u32 {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: sid,
            pasid: 0,
            address: 0x3000,
            security_state: smmu::types::SecurityState::NonSecure,
            error_code: 0,
            timestamp: 0,
            stall: false,
            stag: 0,
            ..EventEntry::default()
        };
        let _ = smmu.submit_event(ev);
    }
    let _ = smmu.submit_event(stall_event(42));

    smmu.reset_queues();

    assert_eq!(
        smmu.get_event_queue_size(),
        0,
        "reset_queues() must also clear the main event queue"
    );
    assert_eq!(
        smmu.get_pending_stall_count(),
        0,
        "BUG-Q2: stall_pending must be 0 after reset_queues()"
    );
}

/// Multiple stall events overflowing into stall_pending — all must be cleared.
#[test]
fn bug_q2_reset_clears_multiple_stall_pending_entries() {
    let smmu = smmu_with_tiny_event_queue();

    // Fill the main queue to capacity.
    for sid in 0..4_u32 {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: sid,
            pasid: 0,
            address: 0x4000,
            security_state: smmu::types::SecurityState::NonSecure,
            error_code: 0,
            timestamp: 0,
            stall: false,
            stag: 0,
            ..EventEntry::default()
        };
        let _ = smmu.submit_event(ev);
    }

    // Overflow 3 stall events into stall_pending.
    for stag in 1..=3_u16 {
        let mut ev = stall_event(100 + u32::from(stag));
        ev.stag = stag;
        let _ = smmu.submit_event(ev);
    }

    let pending_before = smmu.get_pending_stall_count();
    assert!(
        pending_before >= 3,
        "BUG-Q2 precondition: expected >=3 stall_pending entries, got {pending_before}",
    );

    smmu.reset_queues();

    let pending_after = smmu.get_pending_stall_count();
    assert_eq!(
        pending_after,
        0,
        "BUG-Q2: reset_queues() must clear ALL stall_pending entries, {pending_after} remain",
    );
}

// ============================================================================
// BUG-RUST-Q4: PriResp must be strict FIFO — only the HEAD entry may be consumed
// ============================================================================

/// Issue a PriResp for an INTERIOR entry (not the head). Per strict FIFO semantics
/// the head must NOT be removed and CONS must NOT advance.
///
/// FAILS before the fix because the buggy code removes any matching entry and
/// (when pos > 0) leaves CONS unchanged — but still removes the wrong entry.
#[test]
fn bug_q4_priresp_for_interior_entry_does_not_advance_cons() {
    let smmu = smmu_with_all_queues_enabled();

    // Enqueue three PRI entries: head=(sid=1,prg=0), mid=(sid=2,prg=1), tail=(sid=3,prg=2).
    smmu.submit_page_request(pri_entry(1, 0)).unwrap();
    smmu.submit_page_request(pri_entry(2, 1)).unwrap();
    smmu.submit_page_request(pri_entry(3, 2)).unwrap();

    let cons_before = smmu.priq_cons_index();

    // Submit a PriResp for the MIDDLE entry (not the head).
    smmu.submit_command(pri_resp_cmd(2, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after = smmu.priq_cons_index();

    // CONS must NOT advance when the head was not consumed.
    assert_eq!(
        cons_after,
        cons_before,
        "BUG-Q4: PriResp for interior entry must not advance CONS \
         (before={cons_before}, after={cons_after})",
    );

    // The queue must still contain 3 entries — the interior match must NOT be removed.
    let queue_size = smmu.get_pri_queue_size();
    assert_eq!(
        queue_size,
        3,
        "BUG-Q4: PriResp for interior entry must not remove any entry \
         (strict FIFO); queue size is {queue_size} (expected 3)",
    );
}

/// Issue a PriResp for the HEAD entry. CONS must advance by exactly 1 and the
/// head entry must be removed from the queue.
#[test]
fn bug_q4_priresp_for_head_advances_cons() {
    let smmu = smmu_with_all_queues_enabled();

    // Enqueue three PRI entries.
    smmu.submit_page_request(pri_entry(1, 0)).unwrap();
    smmu.submit_page_request(pri_entry(2, 1)).unwrap();
    smmu.submit_page_request(pri_entry(3, 2)).unwrap();

    let cons_before = smmu.priq_cons_index();

    // PriResp for the HEAD entry (sid=1, prg=0).
    smmu.submit_command(pri_resp_cmd(1, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after = smmu.priq_cons_index();

    // CONS must advance by exactly 1.
    assert_ne!(
        cons_after,
        cons_before,
        "BUG-Q4: PriResp for head entry must advance CONS"
    );

    // Two entries remain.
    assert_eq!(
        smmu.get_pri_queue_size(),
        2,
        "BUG-Q4: after consuming head, queue must have 2 entries left"
    );
}

/// Full strict-FIFO sequence: interior PriResp is a no-op, then the head
/// PriResp succeeds. Verifies combined behaviour.
#[test]
fn bug_q4_interior_noop_then_head_succeeds() {
    let smmu = smmu_with_all_queues_enabled();

    smmu.submit_page_request(pri_entry(10, 0)).unwrap();
    smmu.submit_page_request(pri_entry(20, 1)).unwrap();
    smmu.submit_page_request(pri_entry(30, 2)).unwrap();

    let cons_initial = smmu.priq_cons_index();

    // Step 1: PriResp for interior (sid=20, prg=1) — must be a no-op.
    smmu.submit_command(pri_resp_cmd(20, 1)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.priq_cons_index(),
        cons_initial,
        "BUG-Q4: interior PriResp must not change CONS"
    );
    assert_eq!(
        smmu.get_pri_queue_size(),
        3,
        "BUG-Q4: interior PriResp must not remove any entry"
    );

    // Step 2: PriResp for the actual head (sid=10, prg=0) — must consume it.
    smmu.submit_command(pri_resp_cmd(10, 0)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_ne!(
        smmu.priq_cons_index(),
        cons_initial,
        "BUG-Q4: head PriResp must advance CONS"
    );
    assert_eq!(
        smmu.get_pri_queue_size(),
        2,
        "BUG-Q4: after head consumed, 2 entries must remain"
    );
}

/// PriResp for an entry that does not match even the head is a no-op (software error).
#[test]
fn bug_q4_priresp_for_unknown_entry_is_noop() {
    let smmu = smmu_with_all_queues_enabled();

    smmu.submit_page_request(pri_entry(5, 0)).unwrap();
    smmu.submit_page_request(pri_entry(6, 1)).unwrap();

    let cons_before = smmu.priq_cons_index();

    // Unknown (stream_id=99, prg=7) — no entry exists at all.
    smmu.submit_command(pri_resp_cmd(99, 7)).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.priq_cons_index(),
        cons_before,
        "BUG-Q4: PriResp for unknown entry must not advance CONS"
    );
    assert_eq!(
        smmu.get_pri_queue_size(),
        2,
        "BUG-Q4: PriResp for unknown entry must not alter queue"
    );
}
