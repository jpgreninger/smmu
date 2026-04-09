#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD test for BUG-1: `clear_pri_queue()` does not reset `priq_prod` and
//! `priq_cons` atomics to 0.
//!
//! Per ARM SMMU v3 spec §3.5.1, the empty condition for all SMMU queues is
//! defined as PROD == CONS.  After `clear_pri_queue()` removes every entry
//! from the underlying `VecDeque` but leaves the circular index registers
//! unchanged, any caller reading `priq_prod_index()` / `priq_cons_index()`
//! observes a false non-empty state.
//!
//! By contrast, `clear_event_queue()` and `clear_command_queue()` both reset
//! their respective PROD/CONS pairs to 0 after clearing the VecDeque.
//! `clear_pri_queue()` must be fixed to do the same.
//!
//! # Test Strategy (TDD)
//!
//! 1. Enable the PRI queue via CR0.PRIQEN.
//! 2. Enqueue several `PRIEntry` items via `submit_page_request()` to advance
//!    `priq_prod`.
//! 3. Dequeue them via `process_pri_queue()` to advance `priq_cons`.
//! 4. Call `clear_pri_queue()`.
//! 5. Assert `priq_prod_index() == 0` — FAILS before the fix.
//! 6. Assert `priq_cons_index() == 0` — FAILS before the fix.
//! 7. Assert `get_pri_queue_size() == 0` (sanity: VecDeque is empty).
//! 8. Assert `is_priq_empty_by_index()` — FAILS before the fix when PROD !=
//!    CONS after clear.

use smmu::types::{AccessType, CommandEntry, CommandType, PRIEntry};
use smmu::SMMU;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an SMMU with all queue-enable bits set so `submit_page_request`
/// succeeds.
fn smmu_with_priq_enabled() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );
    smmu
}

/// Construct a simple `PRIEntry` for stream `sid`, PASID 0 at the given IOVA.
const fn make_req(sid: u32, iova: u64) -> PRIEntry {
    PRIEntry::with_address(sid, 0, iova, AccessType::Read)
}

// ---------------------------------------------------------------------------
// BUG-1 tests
// ---------------------------------------------------------------------------

/// After enqueueing entries (advancing priq_prod) and then calling
/// `clear_pri_queue()`, `priq_prod_index()` must return 0.
///
/// Before the fix, `priq_prod` retains its non-zero advanced value, so this
/// assertion FAILS.
#[test]
fn bug1_clear_pri_queue_resets_priq_prod_to_zero() {
    let smmu = smmu_with_priq_enabled();

    // Advance priq_prod by submitting three entries.
    smmu.submit_page_request(make_req(1, 0x1000)).unwrap();
    smmu.submit_page_request(make_req(2, 0x2000)).unwrap();
    smmu.submit_page_request(make_req(3, 0x3000)).unwrap();

    // Confirm prod advanced past 0.
    let prod_before = smmu.priq_prod_index();
    assert!(prod_before > 0, "priq_prod must be > 0 after enqueuing");

    smmu.clear_pri_queue();

    assert_eq!(
        smmu.priq_prod_index(),
        0,
        "BUG-1: clear_pri_queue() must reset priq_prod to 0 (spec §3.5.1 empty = PROD == CONS)"
    );
}

/// After dequeueing entries (advancing priq_cons via CMD_PRI_RESP) and then
/// calling `clear_pri_queue()`, `priq_cons_index()` must return 0.
///
/// BUG-NEW-14 update: `process_pri_queue()` no longer advances `priq_cons`
/// (ARM §8.1/§3.5.1 — only CMD_PRI_RESP may advance PRIQ_CONS).  The test
/// uses CMD_PRI_RESP to advance `priq_cons` before calling `clear_pri_queue`.
#[test]
fn bug1_clear_pri_queue_resets_priq_cons_to_zero() {
    let smmu = smmu_with_priq_enabled();

    // Enqueue a page request.
    let req1 = PRIEntry { stream_id: 1, pasid: 0, requested_address: 0x1000,
        access_type: AccessType::Read, is_last_request: true, timestamp: 0,
        prg_index: 0, security_state: smmu::types::SecurityState::NonSecure, span: 0 };
    smmu.submit_page_request(req1).unwrap();

    // Process the PRI queue to emit E_PAGE_REQUEST events (does NOT advance priq_cons
    // per BUG-NEW-14 / ARM §8.1).
    smmu.process_pri_queue().unwrap();

    // Use CMD_PRI_RESP to advance priq_cons (the architectural mechanism, §8.1).
    let mut resp = CommandEntry::new(CommandType::PriResp, 1, 0);
    resp.prg_index = 0;
    smmu.submit_command(resp).unwrap();
    smmu.process_command_queue().unwrap();

    // Confirm cons advanced past 0 (CMD_PRI_RESP advanced it).
    let cons_before = smmu.priq_cons_index();
    assert!(cons_before > 0, "priq_cons must be > 0 after CMD_PRI_RESP");

    smmu.clear_pri_queue();

    assert_eq!(
        smmu.priq_cons_index(),
        0,
        "BUG-1: clear_pri_queue() must reset priq_cons to 0 (spec §3.5.1 empty = PROD == CONS)"
    );
}

/// After `clear_pri_queue()`, both `get_pri_queue_size()` and the index-based
/// empty check (PROD == CONS) must agree that the queue is empty.
///
/// `get_pri_queue_size()` passes even before the fix (VecDeque.clear() works).
/// The PROD == CONS check FAILS before the fix because the indices are not reset.
#[test]
fn bug1_clear_pri_queue_queue_is_empty_by_index() {
    let smmu = smmu_with_priq_enabled();

    smmu.submit_page_request(make_req(10, 0xA000)).unwrap();
    smmu.submit_page_request(make_req(11, 0xB000)).unwrap();
    smmu.submit_page_request(make_req(12, 0xC000)).unwrap();
    smmu.process_pri_queue().unwrap();

    smmu.clear_pri_queue();

    // VecDeque must be empty.
    assert_eq!(smmu.get_pri_queue_size(), 0, "VecDeque must be empty after clear");

    // Index-based empty check: PROD == CONS per ARM §3.5.1.
    let prod = smmu.priq_prod_index();
    let cons = smmu.priq_cons_index();
    assert_eq!(
        prod,
        cons,
        "BUG-1: after clear_pri_queue(), PROD ({prod}) must equal CONS ({cons}) — spec §3.5.1 empty condition"
    );
}

/// Verify PROD == CONS after clear so that the spec-defined empty condition
/// holds numerically.
#[test]
fn bug1_clear_pri_queue_prod_equals_cons() {
    let smmu = smmu_with_priq_enabled();

    smmu.submit_page_request(make_req(5, 0x5000)).unwrap();
    smmu.submit_page_request(make_req(6, 0x6000)).unwrap();
    smmu.process_pri_queue().unwrap();

    smmu.clear_pri_queue();

    let prod = smmu.priq_prod_index();
    let cons = smmu.priq_cons_index();
    assert_eq!(
        prod,
        cons,
        "BUG-1: after clear_pri_queue(), PROD ({prod}) must equal CONS ({cons}) per ARM §3.5.1"
    );
}

/// Regression: `reset_queues()` calls `clear_pri_queue()` internally; verify
/// that reset_queues() also correctly zeros the PRI index registers.
#[test]
fn bug1_reset_queues_resets_pri_indices() {
    let smmu = smmu_with_priq_enabled();

    smmu.submit_page_request(make_req(7, 0x7000)).unwrap();
    smmu.submit_page_request(make_req(8, 0x8000)).unwrap();
    smmu.process_pri_queue().unwrap();

    smmu.reset_queues();

    assert_eq!(
        smmu.priq_prod_index(),
        0,
        "BUG-1: reset_queues() must result in priq_prod == 0"
    );
    assert_eq!(
        smmu.priq_cons_index(),
        0,
        "BUG-1: reset_queues() must result in priq_cons == 0"
    );
}
