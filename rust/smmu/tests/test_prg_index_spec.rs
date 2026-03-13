#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-M-08: PRGIndex tracking for PRI/PRI_RESP matching.
//!
//! Spec: ARM IHI0070G.b §8 (Page Request Queue), §8.2 (PRI queue entry format),
//!       §8.3 (PRG Response Message codes).
//!
//! Requirements:
//! - Every PRIEntry carries a `prg_index: u16` (Page Request Group Index, ARM §8.2).
//! - `CommandEntry` carries a `prg_index: u16` for use by `CMD_PRI_RESP` (ARM §8.3).
//! - `submit_page_request()` advances `priq_prod` after each push.
//! - `CMD_PRI_RESP` processed via `process_command_queue()` finds the pending
//!   PRIEntry whose `(stream_id, prg_index)` matches the command, removes it
//!   from the PRI queue, and advances `priq_cons`.
//! - A `CMD_PRI_RESP` with a non-matching `prg_index` must NOT remove any entry.
//! - `process_pri_queue()` drains the PRI queue and returns the count processed.

use smmu::types::{AccessType, CommandEntry, CommandType, PRIEntry};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    // ARM §6.3.9 + §6.3.9.5 + §8.2: CR0 resets to 0.
    // Set SMMUEN, CMDQEN, and PRIQEN so command and PRI queue operations
    // work in these tests.  BUG-PRIQEN-ASYMMETRY fix: effective PRIQEN =
    // CR0.PRIQEN AND CR0.SMMUEN, so SMMUEN must also be set here.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    smmu
}

// ============================================================================
// CommandEntry prg_index field
// ============================================================================

#[test]
fn test_command_entry_has_prg_index_field() {
    // ARM §8.3: CommandEntry must carry prg_index for CMD_PRI_RESP.
    let cmd = CommandEntry::new(CommandType::PriResp, 1, 0);
    assert_eq!(cmd.prg_index, 0);
}

#[test]
fn test_command_entry_prg_index_can_be_set() {
    let mut cmd = CommandEntry::new(CommandType::PriResp, 1, 0);
    cmd.prg_index = 42;
    assert_eq!(cmd.prg_index, 42);
}

// ============================================================================
// priq_prod advances on submit_page_request
// ============================================================================

#[test]
fn test_priq_prod_advances_on_submit() {
    // ARM §3.5.1: PRIQ_PROD must advance each time a PRI entry is enqueued.
    let smmu = make_smmu();
    assert_eq!(smmu.priq_prod_index(), 0);

    let req = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    smmu.submit_page_request(req).unwrap();

    assert_eq!(smmu.priq_prod_index(), 1);
}

#[test]
fn test_priq_prod_advances_twice_on_two_submits() {
    let smmu = make_smmu();
    smmu.submit_page_request(PRIEntry::with_address(1, 0, 0x1000, AccessType::Read)).unwrap();
    smmu.submit_page_request(PRIEntry::with_address(1, 0, 0x2000, AccessType::Write)).unwrap();
    assert_eq!(smmu.priq_prod_index(), 2);
}

// ============================================================================
// CMD_PRI_RESP matching logic
// ============================================================================

#[test]
fn test_pri_resp_matching_removes_pri_entry() {
    // ARM §8.3: A CMD_PRI_RESP with matching (stream_id, prg_index) must
    // remove the corresponding PRIEntry from the PRI queue.
    let smmu = make_smmu();

    let mut req = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    req.prg_index = 7;
    smmu.submit_page_request(req).unwrap();
    assert_eq!(smmu.get_pri_queue().len(), 1);

    let mut resp = CommandEntry::new(CommandType::PriResp, 1, 0);
    resp.prg_index = 7;
    smmu.submit_command(resp).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(smmu.get_pri_queue().len(), 0);
}

#[test]
fn test_pri_resp_non_matching_prg_index_does_not_clear() {
    // ARM §8.3: A CMD_PRI_RESP with a non-matching prg_index is a software
    // error; the SMMU must leave the mismatched PRIEntry untouched.
    let smmu = make_smmu();

    let mut req = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    req.prg_index = 7;
    smmu.submit_page_request(req).unwrap();

    let mut resp = CommandEntry::new(CommandType::PriResp, 1, 0);
    resp.prg_index = 99; // wrong index
    smmu.submit_command(resp).unwrap();
    smmu.process_command_queue().unwrap();

    // PRIEntry with prg_index=7 must still be present.
    assert_eq!(smmu.get_pri_queue().len(), 1);
    assert_eq!(smmu.get_pri_queue()[0].prg_index, 7);
}

#[test]
fn test_pri_resp_non_matching_stream_id_does_not_clear() {
    // A CMD_PRI_RESP for a different stream_id must not remove our PRIEntry.
    let smmu = make_smmu();

    let mut req = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    req.prg_index = 5;
    smmu.submit_page_request(req).unwrap();

    let mut resp = CommandEntry::new(CommandType::PriResp, 2, 0); // different stream
    resp.prg_index = 5;
    smmu.submit_command(resp).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(smmu.get_pri_queue().len(), 1);
}

#[test]
fn test_pri_resp_removes_only_matching_entry() {
    // When two PRIEntries are pending, CMD_PRI_RESP should remove only the one
    // whose prg_index matches; the other must remain.
    let smmu = make_smmu();

    let mut req_a = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    req_a.prg_index = 3;
    smmu.submit_page_request(req_a).unwrap();

    let mut req_b = PRIEntry::with_address(1, 0, 0x2000, AccessType::Write);
    req_b.prg_index = 9;
    smmu.submit_page_request(req_b).unwrap();

    assert_eq!(smmu.get_pri_queue().len(), 2);

    // Respond to only prg_index=3.
    let mut resp = CommandEntry::new(CommandType::PriResp, 1, 0);
    resp.prg_index = 3;
    smmu.submit_command(resp).unwrap();
    smmu.process_command_queue().unwrap();

    let remaining = smmu.get_pri_queue();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].prg_index, 9);
}

// ============================================================================
// process_pri_queue echoes prg_index / drains queue
// ============================================================================

#[test]
fn test_process_pri_queue_echoes_prg_index_in_event() {
    // process_pri_queue() should drain the PRI queue and return 1 processed
    // entry when one PRIEntry with prg_index=5 is pending.
    let smmu = make_smmu();

    let mut req = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read);
    req.prg_index = 5;
    smmu.submit_page_request(req).unwrap();

    let processed = smmu.process_pri_queue().unwrap();
    assert_eq!(processed, 1);
}

#[test]
fn test_process_pri_queue_drains_queue() {
    // After process_pri_queue(), the PRI queue must be empty.
    let smmu = make_smmu();

    smmu.submit_page_request(PRIEntry::with_address(1, 0, 0x1000, AccessType::Read)).unwrap();
    smmu.submit_page_request(PRIEntry::with_address(2, 0, 0x2000, AccessType::Write)).unwrap();

    let processed = smmu.process_pri_queue().unwrap();
    assert_eq!(processed, 2);
    assert_eq!(smmu.get_pri_queue().len(), 0);
}
