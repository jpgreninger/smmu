//! TDD failing tests for BUG-NEW-9, BUG-NEW-10, BUG-NEW-11, BUG-NEW-12, BUG-NEW-14
//! (Rust implementation).
//!
//! Each test is written to FAIL with the current code and PASS only after the
//! corresponding fix is applied.  No fixes are included here.
//!
//! # Bug Summary
//!
//! ## BUG-NEW-9 — E_PAGE_REQUEST missing permission fields (§7.3.19)
//!
//! When a page request is processed, the resulting `E_PAGE_REQUEST` event must
//! carry per-access-type permission bits:
//!   - `ur`, `uw`, `ux` — unprivileged read/write/execute
//!   - `pr`, `pw`, `px` — privileged read/write/execute
//!   - `span`           — 8-bit access span in units of 4096 bytes
//!
//! Currently `EventEntry` has none of these fields.  Tests referencing them
//! will not compile, which is exactly the expected "red" state.
//!
//! ## BUG-NEW-10 — CERROR_ILL auto-cleared in submit_command() (§7.1, §6.3.28)
//!
//! ARM §7.1/§6.3.28: when `CMDQ_CONS.ERR` is set (e.g. CERROR_ILL), it must
//! persist until software explicitly acknowledges it via `SMMU_GERRORN`.
//! Submitting a new command must NOT clear the error.
//!
//! Currently `submit_command()` detects `CERROR_ILL` and unconditionally clears
//! it (pops the stuck command, advances CONS, and zeroes the error field) as if
//! the new command overrides the stale error state.  This violates the spec.
//!
//! ## BUG-NEW-11 — CMD_RESUME/STALL_TERM missing STALL_MODEL check (§4.7.1)
//!
//! When `IDR0.STALL_MODEL == 0b01` (terminate-only), `CMD_RESUME` and
//! `CMD_STALL_TERM` must raise `CERROR_ILL`.  No such check exists.
//! Because there is no API to set `IDR0.STALL_MODEL=0b01`, these tests are
//! written against a hypothetical `set_stall_model(u8)` API.  They will not
//! compile until that API (and the validation) is added.
//!
//! ## BUG-NEW-12 — PRI overflow auto-response for Last=0 PPRs (§8.1)
//!
//! ARM §8.1: a `PRG_RESPONSE(FAILURE)` must be auto-generated on PRIQ overflow
//! only when the overflowing PPR has `is_last_request=true`.  A PPR with
//! `is_last_request=false` must be silently discarded with no auto-response.
//!
//! Currently `submit_page_request()` unconditionally appends to the
//! auto-failure list regardless of `is_last_request`.
//!
//! ## BUG-NEW-14 — process_pri_queue() advances PRIQ_CONS (§3.5.1, §8.1)
//!
//! ARM §8.1/§3.5.1: `PRIQ_CONS` is software-written; the SMMU must never
//! advance it.  Only `CMD_PRI_RESP` processing may advance `PRIQ_CONS`.
//! `process_pri_queue()` (the event emitter for `E_PAGE_REQUEST` records)
//! must NOT touch `PRIQ_CONS`.
//!
//! Currently `process_pri_queue()` advances `PRIQ_CONS` on each iteration
//! of its drain loop, which is architecturally incorrect.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PRIEntry, SMMUConfig, SecurityState,
    StreamID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

#[allow(dead_code)]
fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

#[allow(dead_code)]
fn pasid(id: u32) -> smmu::types::PASID {
    smmu::types::PASID::new(id).unwrap()
}

/// Build an SMMU with all queues enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Build an SMMU with a very small PRIQ (4 entries) so overflow is easy.
fn make_smmu_small_priq() -> SMMU {
    let mut cfg = SMMUConfig::default();
    cfg.queue_config.pri_queue_size = 4;
    let smmu = SMMU::with_config(cfg);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

const SMALL_PRIQ: usize = 4;

/// Fill the PRIQ to `count` entries, all with `is_last_request=false`.
fn fill_priq(smmu: &SMMU, count: usize, stream: u32, p: u32, start_prg: u16) {
    for i in 0..count {
        let i_u16 = u16::try_from(i).unwrap();
        let req = PRIEntry {
            stream_id: stream,
            pasid: p,
            requested_address: (u64::from(start_prg) + i as u64) << 12,
            access_type: AccessType::Read,
            is_last_request: false,
            timestamp: 0,
            prg_index: start_prg + i_u16,
            security_state: SecurityState::NonSecure,
        };
        let _ = smmu.submit_page_request(req);
    }
}

// ============================================================================
// BUG-NEW-9: E_PAGE_REQUEST event must carry permission fields (§7.3.19)
// ============================================================================
//
// NOTE: The fields `ur`, `uw`, `ux`, `pr`, `pw`, `px`, `span` do NOT yet
// exist on `EventEntry`.  These tests WILL NOT COMPILE until those fields are
// added to the struct.  A compile error here is the expected "red" state.
//
// Once the fields are added, tests will compile but fail at runtime because
// `process_pri_queue()` does not yet populate them.

/// BUG-NEW-9: Read access must produce `ur=true`, `uw=false`, `ux=false`.
///
/// ARM §7.3.19 requires the permission bits to reflect the access type of the
/// page request.
///
/// BEFORE FIX: `ur`/`uw`/`ux` do not exist on `EventEntry` → compile error.
/// AFTER API ADD: fields exist but are zero → test fails at runtime.
/// AFTER FULL FIX: `ur=true` for a Read request → test passes.
#[test]
fn bug_new9_page_request_read_sets_ur() {
    let smmu = make_smmu();

    let req = PRIEntry {
        stream_id: 0xF0,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_page_request(req).unwrap();
    smmu.process_pri_queue().unwrap();

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::EPageRequest)
        .expect("BUG-NEW-9: E_PAGE_REQUEST event must be present in the event queue");

    // BUG-NEW-9: `ur` does not exist on EventEntry yet → will not compile.
    assert!(
        ev.ur,
        "BUG-NEW-9: E_PAGE_REQUEST for Read access must have ur=true (§7.3.19)"
    );
    assert!(
        !ev.uw,
        "BUG-NEW-9: E_PAGE_REQUEST for Read access must have uw=false (§7.3.19)"
    );
    assert!(
        !ev.ux,
        "BUG-NEW-9: E_PAGE_REQUEST for Read access must have ux=false (§7.3.19)"
    );
}

/// BUG-NEW-9: Write access must produce `ur=false`, `uw=true`, `ux=false`.
#[test]
fn bug_new9_page_request_write_sets_uw() {
    let smmu = make_smmu();

    let req = PRIEntry {
        stream_id: 0xF1,
        pasid: 0,
        requested_address: 0x2000,
        access_type: AccessType::Write,
        is_last_request: true,
        timestamp: 0,
        prg_index: 1,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_page_request(req).unwrap();
    smmu.process_pri_queue().unwrap();

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::EPageRequest)
        .expect("BUG-NEW-9: E_PAGE_REQUEST event must be present");

    // BUG-NEW-9: `uw` does not exist on EventEntry yet → will not compile.
    assert!(
        ev.uw,
        "BUG-NEW-9: E_PAGE_REQUEST for Write access must have uw=true (§7.3.19)"
    );
    assert!(
        !ev.ur,
        "BUG-NEW-9: E_PAGE_REQUEST for Write access must have ur=false (§7.3.19)"
    );
    assert!(
        !ev.ux,
        "BUG-NEW-9: E_PAGE_REQUEST for Write access must have ux=false (§7.3.19)"
    );
}

/// BUG-NEW-9: `span` field must be present and zero for a single-page request.
///
/// ARM §7.3.19: `span` is an 8-bit field, units of 4096 bytes.  A single-page
/// request covers 1 × 4096 bytes = span 0 (meaning "1 page").
#[test]
fn bug_new9_page_request_span_field_exists() {
    let smmu = make_smmu();

    let req = PRIEntry {
        stream_id: 0xF2,
        pasid: 0,
        requested_address: 0x3000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 2,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_page_request(req).unwrap();
    smmu.process_pri_queue().unwrap();

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| e.event_type == EventType::EPageRequest)
        .expect("BUG-NEW-9: E_PAGE_REQUEST event must be present");

    // BUG-NEW-9: `span` does not exist on EventEntry yet → will not compile.
    assert_eq!(
        ev.span, 0u8,
        "BUG-NEW-9: E_PAGE_REQUEST for a single-page request must have span=0 (§7.3.19)"
    );
}

// ============================================================================
// BUG-NEW-10: CERROR_ILL must NOT be cleared by submit_command() (§7.1)
// ============================================================================
//
// ARM §7.1/§6.3.28: CMDQ_CONS.ERR persists until software writes SMMU_GERRORN.
// Submitting a new command after a CERROR_ILL must NOT clear the error.

/// BUG-NEW-10 (primary): after CERROR_ILL is set, submitting a new command
/// must NOT clear CMDQ_CONS.ERR.
///
/// BEFORE FIX: `submit_command()` detects CERROR_ILL and clears it before
/// enqueueing the new command → `get_cmdq_cons_err()` returns `CERROR_NONE`.
///
/// AFTER FIX:  CERROR_ILL persists; `get_cmdq_cons_err()` still returns
/// `CERROR_ILL`.
#[test]
fn bug_new10_cerror_ill_persists_after_new_submit() {
    let smmu = make_smmu();

    // Force CERROR_ILL by submitting CMD_SYNC CS=3 (reserved encoding).
    let mut bad_sync = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_sync.cs = 3; // 0b11 = reserved → CERROR_ILL
    smmu.submit_command(bad_sync).unwrap();
    smmu.process_command_queue().unwrap();

    // Verify CERROR_ILL is set.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "precondition: CERROR_ILL must be set after CMD_SYNC CS=3"
    );

    // Now submit a harmless new command (CFGI_ALL).
    let cfgi = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    // submit_command() should accept the new command but MUST NOT clear CERROR_ILL.
    // Note: it may return Ok or Err depending on whether the impl rejects
    // submissions while an error is pending — either is acceptable, as long as
    // the error is not cleared.
    let _ = smmu.submit_command(cfgi);

    // BEFORE FIX: CERROR_ILL is cleared by submit_command() → get_cmdq_cons_err()
    //   returns CERROR_NONE.
    // AFTER FIX:  CERROR_ILL persists → get_cmdq_cons_err() returns CERROR_ILL.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-10: CMDQ_CONS.ERR must remain CERROR_ILL after submitting a new \
         command while an error is pending (ARM §7.1/§6.3.28). Before fix, \
         submit_command() unconditionally clears the error."
    );
}

/// BUG-NEW-10 (extended): CERROR_ILL must also persist across a full
/// submit+process cycle for the new command.
///
/// Even if submit_command() preserves the error, we also verify that calling
/// process_command_queue() after the new submit does not inadvertently clear it.
#[test]
fn bug_new10_cerror_ill_persists_after_process_cycle() {
    let smmu = make_smmu();

    // Trigger CERROR_ILL.
    let mut bad_sync = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_sync.cs = 3;
    smmu.submit_command(bad_sync).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "precondition: CERROR_ILL must be set"
    );

    // Submit and process a new command.
    let cfgi = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    let _ = smmu.submit_command(cfgi);
    let _ = smmu.process_command_queue();

    // CERROR_ILL must still be set — it is not cleared by hardware; only
    // software can clear it via SMMU_GERRORN.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-10: CMDQ_CONS.ERR must persist through subsequent submit+process \
         cycles until software explicitly clears it via SMMU_GERRORN (ARM §6.3.28). \
         Before fix, it is cleared by submit_command()."
    );
}

/// BUG-NEW-10 (guard): verifying that the fix does not prevent legitimate
/// processing once the error has been acknowledged via SMMU_GERRORN.
///
/// After software clears the error, a fresh command must execute normally and
/// CERROR_NONE must be visible.
///
/// This test should pass both before and after the fix.
#[test]
fn bug_new10_after_gerror_ack_queue_resumes() {
    let smmu = make_smmu();

    // Trigger CERROR_ILL.
    let mut bad_sync = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_sync.cs = 3;
    smmu.submit_command(bad_sync).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "precondition: CERROR_ILL must be set"
    );

    // Acknowledge the error via SMMU_GERRORN (clear GERROR_CMDQ_ERR).
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // Error must now be cleared.
    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "after software acknowledges GERROR.CMDQ_ERR via GERRORN the error must clear"
    );

    // Fresh command must execute cleanly.
    let cfgi = CommandEntry::new(CommandType::CfgiAll, 0, 0);
    smmu.submit_command(cfgi).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "after error acknowledgement a fresh command must execute with CERROR_NONE"
    );
}

// ============================================================================
// BUG-NEW-11: CMD_RESUME/STALL_TERM must raise CERROR_ILL when
//             IDR0.STALL_MODEL == 0b01 (terminate-only) (§4.7.1, §4.7.2)
// ============================================================================
//
// NOTE: There is currently no API to configure IDR0.STALL_MODEL.  The tests
// below call a hypothetical `set_stall_model(u8)` method that does not yet
// exist.  They WILL NOT COMPILE until both the API and the validation are added.
//
// This compile failure is the intended "red" state.

/// BUG-NEW-11: CMD_RESUME must raise CERROR_ILL when STALL_MODEL=0b01.
///
/// ARM §4.7.1: when `IDR0.STALL_MODEL == 0b01` (terminate-only model), the
/// SMMU does not support stall and `CMD_RESUME` is therefore illegal.  Processing
/// it must set `CMDQ_CONS.ERR = CERROR_ILL`.
///
/// BEFORE FIX: no such check exists; CMD_RESUME processes silently.
/// AFTER FIX:  CERROR_ILL is set.
#[test]
fn bug_new11_resume_cerror_ill_when_terminate_only() {
    let smmu = make_smmu();

    // BUG-NEW-11: set_stall_model() does not yet exist — will not compile.
    smmu.set_stall_model(0x01u8);

    let mut resume = CommandEntry::new(CommandType::Resume, 0, 0);
    resume.stag = 0;
    resume.action = true;
    smmu.submit_command(resume).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-11: CMD_RESUME must raise CERROR_ILL when IDR0.STALL_MODEL=0b01 \
         (terminate-only; ARM §4.7.1)"
    );
}

/// BUG-NEW-11: CMD_STALL_TERM must raise CERROR_ILL when STALL_MODEL=0b01.
///
/// ARM §4.7.2: same constraint applies to `CMD_STALL_TERM`.
///
/// BEFORE FIX: no check; command processes silently.
/// AFTER FIX:  CERROR_ILL is set.
#[test]
fn bug_new11_stall_term_cerror_ill_when_terminate_only() {
    let smmu = make_smmu();

    // BUG-NEW-11: set_stall_model() does not yet exist — will not compile.
    smmu.set_stall_model(0x01u8);

    let stall_term = CommandEntry::new(CommandType::StallTerm, 0x10, 0);
    smmu.submit_command(stall_term).unwrap();
    smmu.process_command_queue().unwrap();

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-11: CMD_STALL_TERM must raise CERROR_ILL when IDR0.STALL_MODEL=0b01 \
         (terminate-only; ARM §4.7.2)"
    );
}

// ============================================================================
// BUG-NEW-12: PRI overflow auto-response must obey is_last_request flag (§8.1)
// ============================================================================
//
// ARM §8.1: auto-failure PRG_RESPONSE is required ONLY when the overflowing
// PPR has `is_last_request=true`.  If `is_last_request=false` it must be
// silently discarded.

/// BUG-NEW-12 (primary): overflow with `is_last_request=false` must produce
/// no auto-failure response.
///
/// BEFORE FIX: `get_pri_auto_failure_responses()` returns 1 entry (bug).
/// AFTER FIX:  returns empty.
#[test]
fn bug_new12_overflow_last_false_no_auto_failure() {
    let smmu = make_smmu_small_priq();

    // Fill PRIQ to capacity.
    fill_priq(&smmu, SMALL_PRIQ, 0x20, 0, 0);
    assert_eq!(smmu.get_pri_queue_size(), SMALL_PRIQ as u64,
               "precondition: PRIQ must be at capacity");
    assert!(smmu.get_pri_auto_failure_responses().is_empty(),
            "precondition: no auto-failures before overflow");

    // Submit an overflow PPR with is_last_request=false.
    let overflow = PRIEntry {
        stream_id: 0x20,
        pasid: 0,
        requested_address: 0x9_9000,
        access_type: AccessType::Read,
        is_last_request: false,   // not the last PPR — device is NOT waiting
        timestamp: 0,
        prg_index: 0xAA,
        security_state: SecurityState::NonSecure,
    };
    let _ = smmu.submit_page_request(overflow);

    // ARM §8.1: silence discard — no auto-failure.
    // BEFORE FIX: 1 failure unconditionally pushed.
    // AFTER FIX:  0 failures.
    let failures = smmu.get_pri_auto_failure_responses();
    assert!(
        failures.is_empty(),
        "BUG-NEW-12: overflow with is_last_request=false must NOT generate an \
         auto-failure PRG_RESPONSE (ARM §8.1). Before fix, a failure is \
         unconditionally appended regardless of the Last bit. Got: {failures:?}"
    );
}

/// BUG-NEW-12 (positive): overflow with `is_last_request=true` MUST produce
/// an auto-failure response (existing correct behaviour — regression guard).
#[test]
fn bug_new12_overflow_last_true_generates_auto_failure() {
    let smmu = make_smmu_small_priq();

    // Fill PRIQ to capacity.
    fill_priq(&smmu, SMALL_PRIQ, 0x21, 0, 0);
    assert_eq!(smmu.get_pri_queue_size(), SMALL_PRIQ as u64,
               "precondition: PRIQ must be at capacity");
    assert!(smmu.get_pri_auto_failure_responses().is_empty(),
            "precondition: no auto-failures before overflow");

    // Submit an overflow PPR with is_last_request=true.
    let overflow = PRIEntry {
        stream_id: 0x21,
        pasid: 0,
        requested_address: 0x8_8000,
        access_type: AccessType::Write,
        is_last_request: true,    // last PPR — device awaits a response
        timestamp: 0,
        prg_index: 0xBB,
        security_state: SecurityState::NonSecure,
    };
    let _ = smmu.submit_page_request(overflow);

    let failures = smmu.get_pri_auto_failure_responses();
    assert_eq!(
        failures.len(),
        1,
        "BUG-NEW-12: overflow with is_last_request=true must generate exactly one \
         auto-failure PRG_RESPONSE (ARM §8.1). Got {failures:?}"
    );
    assert_eq!(failures[0].prg_index, 0xBBu16,
               "auto-failure prg_index must match the overflow PPR");
}

/// BUG-NEW-12 (mixed): two overflow PPRs — Last=false then Last=true.
/// Only the Last=true one should generate an auto-failure.
#[test]
fn bug_new12_overflow_mixed_only_last_true_generates_failure() {
    let smmu = make_smmu_small_priq();

    // Fill PRIQ to capacity.
    fill_priq(&smmu, SMALL_PRIQ, 0x22, 0, 0);
    assert_eq!(smmu.get_pri_queue_size(), SMALL_PRIQ as u64);
    assert!(smmu.get_pri_auto_failure_responses().is_empty());

    // First overflow: Last=false.
    let ovf1 = PRIEntry {
        stream_id: 0x22,
        pasid: 0,
        requested_address: 0x7_7000,
        access_type: AccessType::Read,
        is_last_request: false,
        timestamp: 0,
        prg_index: 0xCC,
        security_state: SecurityState::NonSecure,
    };
    let _ = smmu.submit_page_request(ovf1);

    // Second overflow: Last=true.
    let ovf2 = PRIEntry {
        stream_id: 0x22,
        pasid: 0,
        requested_address: 0x6_6000,
        access_type: AccessType::Write,
        is_last_request: true,
        timestamp: 0,
        prg_index: 0xDD,
        security_state: SecurityState::NonSecure,
    };
    let _ = smmu.submit_page_request(ovf2);

    let failures = smmu.get_pri_auto_failure_responses();

    // BEFORE FIX: 2 failures (one for each overflow).
    // AFTER FIX:  1 failure (only for Last=true).
    assert_eq!(
        failures.len(),
        1,
        "BUG-NEW-12: of two overflow PPRs (Last=false, Last=true), only the \
         Last=true one must generate an auto-failure (ARM §8.1). Before fix, both \
         unconditionally generate failures. Got: {failures:?}"
    );
    if !failures.is_empty() {
        assert_eq!(
            failures[0].prg_index, 0xDDu16,
            "BUG-NEW-12: the auto-failure must correspond to the Last=true PPR \
             (prg_index=0xDD)"
        );
    }
}

// ============================================================================
// BUG-NEW-14: process_pri_queue() must NOT advance PRIQ_CONS (§3.5.1, §8.1)
// ============================================================================
//
// ARM §8.1/§3.5.1: PRIQ_CONS is software-written.  Only CMD_PRI_RESP processing
// may advance it.  process_pri_queue() (which emits E_PAGE_REQUEST events) must
// leave PRIQ_CONS unchanged.

/// BUG-NEW-14 (primary): PRIQ_CONS must be unchanged after process_pri_queue().
///
/// BEFORE FIX: each iteration of the drain loop calls
/// `self.priq_cons.store(advance_index(...))`, incrementing PRIQ_CONS with each
/// processed entry.
///
/// AFTER FIX: `process_pri_queue()` never touches `priq_cons`.
#[test]
fn bug_new14_process_pri_queue_does_not_advance_priq_cons() {
    let smmu = make_smmu();

    let stream = 0x40u32;

    // Submit 3 page requests.
    for i in 0u16..3u16 {
        let req = PRIEntry {
            stream_id: stream,
            pasid: 0,
            requested_address: u64::from(i) << 12,
            access_type: AccessType::Read,
            is_last_request: i == 2, // last one
            timestamp: 0,
            prg_index: i,
            security_state: SecurityState::NonSecure,
        };
        smmu.submit_page_request(req).unwrap();
    }

    // Snapshot PRIQ_CONS before processing.
    let cons_before = smmu.priq_cons_index();

    // Process the PRI queue (emits E_PAGE_REQUEST events).
    smmu.process_pri_queue().unwrap();

    // PRIQ_CONS must be unchanged — only CMD_PRI_RESP may advance it.
    let cons_after = smmu.priq_cons_index();

    // BEFORE FIX: cons_after > cons_before (advanced by 3).
    // AFTER FIX:  cons_after == cons_before.
    assert_eq!(
        cons_after & !(1u32 << 31),
        cons_before & !(1u32 << 31),
        "BUG-NEW-14: process_pri_queue() must NOT advance PRIQ_CONS (ARM §8.1 / \
         §3.5.1). Only CMD_PRI_RESP processing may increment PRIQ_CONS. Before fix, \
         the drain loop unconditionally advances cons on each pop. \
         cons_before={}, cons_after={}",
        cons_before,
        cons_after
    );
}

/// BUG-NEW-14 (secondary): PRIQ_CONS must only advance when CMD_PRI_RESP is
/// processed, not when process_pri_queue() is called.
///
/// Verify that after a CMD_PRI_RESP PRIQ_CONS DOES advance (positive control).
#[test]
fn bug_new14_cmd_pri_resp_advances_priq_cons() {
    let smmu = make_smmu();

    let stream = 0x41u32;

    // Submit 1 page request.
    let req = PRIEntry {
        stream_id: stream,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 42,
        security_state: SecurityState::NonSecure,
    };
    smmu.submit_page_request(req).unwrap();

    // process_pri_queue() must not advance cons.
    let cons_before = smmu.priq_cons_index();
    smmu.process_pri_queue().unwrap();
    let cons_after_process = smmu.priq_cons_index();

    // Guard: process_pri_queue() must not advance cons (BUG-NEW-14 assertion).
    assert_eq!(
        cons_after_process & !(1u32 << 31),
        cons_before & !(1u32 << 31),
        "BUG-NEW-14 guard: process_pri_queue() must not advance PRIQ_CONS \
         (see also bug_new14_process_pri_queue_does_not_advance_priq_cons)"
    );

    // Now issue CMD_PRI_RESP — PRIQ_CONS must advance.
    let mut pri_resp = CommandEntry::new(CommandType::PriResp, stream, 0);
    pri_resp.prg_index = 42;
    smmu.submit_command(pri_resp).unwrap();
    smmu.process_command_queue().unwrap();

    let cons_after_resp = smmu.priq_cons_index();

    assert!(
        (cons_after_resp & !(1u32 << 31)) > (cons_after_process & !(1u32 << 31)),
        "CMD_PRI_RESP must advance PRIQ_CONS (ARM §8.1). \
         cons before resp={}, cons after resp={}",
        cons_after_process,
        cons_after_resp
    );
}
