//! Failing tests for two confirmed bugs in the Rust SMMU implementation.
//!
//! ## BUG-RUST-5: MEV deduplication missing PASID check
//!
//! In `smmu/mod.rs` ~line 4197, the MEV duplicate-suppression check compares only
//! `(event_type, stream_id)`.  ARM §7.3.1 states that events are identical only
//! when ALL defined fields match; SubstreamID (PASID) is a defined field.
//!
//! Observable failure: a fault on PASID 0 silently suppresses a *distinct* fault
//! on PASID 1 for the same stream and event type, so software never sees the
//! second fault.
//!
//! ## BUG-RUST-6: CMD_PRI_RESP PASID match missing
//!
//! In `smmu/mod.rs` ~line 5177, the `CommandType::PriResp` handler retires the
//! PRI queue head when `(stream_id, prg_index)` matches the command.  ARM §4.5.2
//! specifies that `CMD_PRI_RESP` addresses the `(StreamID, SubstreamID, PRGIndex)`
//! tuple.  A response targeting PASID=0 can therefore wrongly retire a head entry
//! whose PASID is 5, violating the per-substream response requirement.
#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, PRIEntry, SecurityState, StreamConfig,
    StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

/// Build a `CMD_PRI_RESP` command for `(stream_id, pasid, prg_index)`.
const fn pri_resp_cmd(stream_id: u32, pasid_val: u32, prg_index: u16) -> CommandEntry {
    let mut cmd = CommandEntry::new(CommandType::PriResp, stream_id, pasid_val);
    cmd.prg_index = prg_index;
    cmd
}

// ============================================================================
// BUG-RUST-5: MEV deduplication missing PASID check
//
// smmu/mod.rs ~line 4197-4202:
//   BEFORE FIX: queue.iter().any(|e| e.event_type == event.event_type
//                                 && e.stream_id == event.stream_id)
//   AFTER FIX:  queue.iter().any(|e| e.event_type == event.event_type
//                                 && e.stream_id == event.stream_id
//                                 && e.pasid    == event.pasid)
//
// ARM §7.3.1: "Two events are identical if all defined fields … match."
// SubstreamID (pasid) is a defined field; it must be included in the
// duplicate check.
// ============================================================================

/// BUG-RUST-5a: Two Write-access faults on *different* PASIDs of the same
/// MEV=true stream must BOTH be recorded.
///
/// SETUP:
///   - Stream configured with MEV=true, Stage1-only.
///   - PASID 0 maps IOVA 0x1000 → PA 0x2000, read-only.
///   - PASID 1 maps IOVA 0x1000 → PA 0x3000, read-only.
///   - Translate PASID 0 with Write → F_PERMISSION (event 1 recorded, PASID=0).
///   - Translate PASID 1 with Write → F_PERMISSION (should be event 2, PASID=1).
///
/// BEFORE FIX: the second event is suppressed because the MEV check only
///   compares (event_type=F_PERMISSION, stream_id) — PASID is ignored.
///   Result: only 1 event in the queue.
///
/// AFTER FIX: PASID is included in the duplicate check; PASID 1 ≠ PASID 0 so
///   the second event is NOT a duplicate and is recorded.
///   Result: 2 events in the queue.
///
/// This test FAILS before the fix (asserts 2 events but only 1 present).
#[test]
fn bug_rust_5a_mev_different_pasid_both_events_recorded() {
    let smmu = make_smmu();
    let stream_id = sid(0x50);

    let mut cfg = StreamConfig::stage1_only();
    cfg.mev = true;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();

    // Create two distinct PASIDs for the stream.
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();

    // Map IOVA 0x1000 as read-only in both PASIDs so that a Write triggers F_PERMISSION.
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.map_page(
        stream_id,
        pasid(1),
        iova(0x1000),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First fault: PASID 0, Write → F_PERMISSION.
    let r0 = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        r0.is_err(),
        "BUG-RUST-5a setup: PASID 0 Write on read-only page must fail; got {r0:?}"
    );

    // Second fault: PASID 1, Write → F_PERMISSION (different PASID — must NOT be suppressed).
    let r1 = smmu.translate(
        stream_id,
        pasid(1),
        iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        r1.is_err(),
        "BUG-RUST-5a setup: PASID 1 Write on read-only page must fail; got {r1:?}"
    );

    // Both faults are on different PASIDs; MEV must NOT suppress the second one.
    // ARM §7.3.1: events are identical only when ALL defined fields match.
    // SubstreamID (PASID) is a defined field — PASID 0 ≠ PASID 1.
    let events = smmu.get_events();
    let f_perm_count = events
        .iter()
        .filter(|e| e.stream_id == stream_id.as_u32()
            && e.event_type as u8 == 0x13 /* F_PERMISSION */)
        .count();

    assert_eq!(
        f_perm_count,
        2,
        "BUG-RUST-5a: MEV=true must NOT suppress faults on different PASIDs — \
         expected 2 F_PERMISSION events (one per PASID), got {f_perm_count}. \
         Root cause: MEV check at smmu/mod.rs ~line 4198 compares only \
         (event_type, stream_id) and ignores PASID; PASID 1 fault is silently \
         dropped because a PASID 0 fault of the same type is already queued."
    );
}

/// BUG-RUST-5b: Two Write faults on the *same* PASID with MEV=true must produce
/// only 1 event (MEV correctly deduplicates identical events).
///
/// This is a regression guard — it must PASS both before and after the fix.
#[test]
fn bug_rust_5b_mev_same_pasid_deduplicated() {
    let smmu = make_smmu();
    let stream_id = sid(0x51);

    let mut cfg = StreamConfig::stage1_only();
    cfg.mev = true;
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x2000),
        pa(0x4000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Two identical Write faults on PASID 0 — same stream, same type, same PASID.
    // MEV must suppress the second one.
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x2000),
        AccessType::Write,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let f_perm_count = events
        .iter()
        .filter(|e| e.stream_id == stream_id.as_u32()
            && e.event_type as u8 == 0x13 /* F_PERMISSION */)
        .count();

    assert_eq!(
        f_perm_count,
        1,
        "BUG-RUST-5b: MEV=true must deduplicate identical faults (same stream + same PASID) \
         to exactly 1 event; got {f_perm_count} events"
    );
}

/// BUG-RUST-5c: With MEV=false, Write faults on two different PASIDs must
/// each produce their own event (no suppression at all).
///
/// This is a regression guard — it must PASS both before and after the fix.
#[test]
fn bug_rust_5c_mev_false_all_events_recorded() {
    let smmu = make_smmu();
    let stream_id = sid(0x52);

    let mut cfg = StreamConfig::stage1_only();
    cfg.mev = false; // MEV disabled — no deduplication
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();

    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x3000),
        pa(0x5000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu.map_page(
        stream_id,
        pasid(1),
        iova(0x3000),
        pa(0x6000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let _ = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x3000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    let _ = smmu.translate(
        stream_id,
        pasid(1),
        iova(0x3000),
        AccessType::Write,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let f_perm_count = events
        .iter()
        .filter(|e| e.stream_id == stream_id.as_u32()
            && e.event_type as u8 == 0x13 /* F_PERMISSION */)
        .count();

    assert_eq!(
        f_perm_count,
        2,
        "BUG-RUST-5c: MEV=false must record all events without suppression; \
         expected 2 F_PERMISSION events, got {f_perm_count}"
    );
}

// ============================================================================
// BUG-RUST-6: CMD_PRI_RESP PASID match missing
//
// smmu/mod.rs ~line 5177:
//   BEFORE FIX: head.stream_id == command.stream_id
//               && head.prg_index == command.prg_index
//   AFTER FIX:  head.stream_id == command.stream_id
//               && head.prg_index == command.prg_index
//               && head.pasid    == command.pasid
//
// ARM §4.5.2: CMD_PRI_RESP addresses the (StreamID, SubstreamID, PRGIndex) tuple.
// A response for the wrong PASID must be treated as a no-op.
//
// Note on SSV (ARM §4.5.2): SSV=0 in CMD_PRI_RESP means "SubstreamID not valid",
// in which case the PASID field is ignored and any PASID is considered matching.
// The CommandEntry.pasid field carries the SubstreamID; a non-zero pasid in
// `CommandEntry::new(cmd_type, stream_id, pasid_val)` naturally sets SSV=1
// semantics.  These tests use a non-zero PASID in the PRI entry (PASID=5) and
// a mismatched PASID in the command (PASID=0) to expose the bug.
// ============================================================================

/// BUG-RUST-6a: A `CMD_PRI_RESP` for the correct stream and PRG_INDEX but the
/// *wrong* PASID must NOT retire the PRI queue head entry.
///
/// SETUP:
///   - Enqueue a PRI entry for stream_id=0x60, PASID=5, PRG_INDEX=0.
///   - Issue CMD_PRI_RESP with stream_id=0x60, PASID=0 (wrong!), PRG_INDEX=0.
///
/// BEFORE FIX: the head IS retired because PASID is not checked — the match
///   only requires (stream_id=0x60, prg_index=0), which matches.
///   Result: queue size = 0 and CONS advances (WRONG).
///
/// AFTER FIX: PASID=0 ≠ head.pasid=5 → no match → head is NOT retired.
///   Result: queue size = 1 and CONS does not advance (CORRECT).
///
/// This test FAILS before the fix (queue_size is 0 but should be 1).
#[test]
fn bug_rust_6a_cmd_pri_resp_wrong_pasid_does_not_retire() {
    // Use set_cr0 directly so the PRI queue is accessible without needing
    // full stream configuration.  This mirrors the pattern used in test_bugs_q2_q4.rs.
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );

    // Enqueue a PRI entry: stream_id=0x60, PASID=5, PRG_INDEX=0.
    let entry = PRIEntry::with_address(0x60, 5, 0x1000, AccessType::Read).with_prg_index(0);
    smmu.submit_page_request(entry).unwrap();

    let cons_before = smmu.priq_cons_index();

    // Issue CMD_PRI_RESP with the same stream_id and prg_index, but PASID=0 (wrong).
    // ARM §4.5.2: the response must match (StreamID, SubstreamID, PRGIndex).
    // PASID=0 ≠ head.pasid=5 → this is a mismatched response → no-op.
    let cmd = pri_resp_cmd(0x60, /*wrong_pasid=*/ 0, /*prg_index=*/ 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after = smmu.priq_cons_index();
    let queue_size = smmu.get_pri_queue_size();

    // CONS must not advance when there is no valid match.
    assert_eq!(
        cons_after,
        cons_before,
        "BUG-RUST-6a: CMD_PRI_RESP with wrong PASID must not advance PRIQ_CONS \
         (before={cons_before}, after={cons_after}). \
         Root cause: match at smmu/mod.rs ~line 5177 checks only \
         (stream_id, prg_index) — PASID 0 wrongly retired the head entry for PASID 5."
    );

    // The head entry must still be in the queue — it was not correctly retired.
    assert_eq!(
        queue_size,
        1,
        "BUG-RUST-6a: CMD_PRI_RESP with wrong PASID must not remove the head entry; \
         expected queue size=1 (entry for PASID 5 still present), got {queue_size}. \
         Root cause: PASID is not included in the CMD_PRI_RESP match condition."
    );
}

/// BUG-RUST-6b: A `CMD_PRI_RESP` with the *correct* PASID retires the head entry.
///
/// This is a regression guard — it must PASS both before and after the fix.
#[test]
fn bug_rust_6b_cmd_pri_resp_correct_pasid_retires() {
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );

    // Enqueue a PRI entry: stream_id=0x61, PASID=5, PRG_INDEX=0.
    let entry = PRIEntry::with_address(0x61, 5, 0x1000, AccessType::Read).with_prg_index(0);
    smmu.submit_page_request(entry).unwrap();

    let cons_before = smmu.priq_cons_index();

    // Issue CMD_PRI_RESP with the correct stream_id, PASID=5, and prg_index=0.
    let cmd = pri_resp_cmd(0x61, /*correct_pasid=*/ 5, /*prg_index=*/ 0);
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after = smmu.priq_cons_index();
    let queue_size = smmu.get_pri_queue_size();

    // CONS must advance — the head was correctly retired.
    assert_ne!(
        cons_after,
        cons_before,
        "BUG-RUST-6b: CMD_PRI_RESP with correct PASID must advance PRIQ_CONS"
    );

    // The queue must now be empty.
    assert_eq!(
        queue_size,
        0,
        "BUG-RUST-6b: after CMD_PRI_RESP with correct PASID, queue must be empty; \
         got {queue_size}"
    );
}
