//! TDD failing tests for BUG-NEW-18, BUG-NEW-22, BUG-NEW-24, BUG-NEW-26 (Rust implementation).
//!
//! Each test is written to FAIL with the current code and PASS only after the
//! corresponding fix is applied.
//!
//! # Bug Summary
//!
//! ## BUG-NEW-18 — §4.4.2.7 CMD_TLBI_EL2_ALL over-invalidates
//!
//! `CommandType::TlbiEl2All` currently calls `self.tlb_cache.invalidate_all()`,
//! which evicts ALL entries — including El1El0 entries that should be preserved.
//! ARM §4.4.2.7 specifies that CMD_TLBI_EL2_ALL invalidates only entries tagged
//! `StreamWorld::El2` or `StreamWorld::El2E2h`.
//!
//! BEFORE FIX: El1El0 stream's TLB entry is evicted (TLB miss on second translate).
//! AFTER FIX:  El1El0 stream's TLB entry survives (TLB hit on second translate).
//!
//! ## BUG-NEW-22 — §3.12.4/§8.1 submit_page_request() does not emit E_PAGE_REQUEST
//!
//! `submit_page_request()` enqueues the PRI entry but does NOT emit the
//! E_PAGE_REQUEST event to the event queue.  The C++ implementation emits
//! immediately at submit time.
//!
//! BEFORE FIX: no E_PAGE_REQUEST in event queue after submit (no process_pri_queue call).
//! AFTER FIX:  E_PAGE_REQUEST is present immediately after submit.
//!
//! ## BUG-NEW-24 — §4.4.2.7 CMD_TLBI_EL2_ALL missing IDR0.Hyp guard
//!
//! ARM §4.4.2.7: if IDR0.Hyp==0, CMD_TLBI_EL2_ALL must raise CERROR_ILL.
//! The current implementation has no such guard.
//!
//! Positive test (Hyp=1 default): CMD_TLBI_EL2_ALL executes without CERROR_ILL.
//! Negative test (`#[ignore]`): requires IDR0.Hyp=0 override capability (not yet exposed).
//!
//! ## BUG-NEW-26 — §4.8 CMD_SYNC completion event security state
//!
//! CMD_SYNC has no StreamID operand; the completion event security state must
//! always be `SecurityState::NonSecure`.  Current code looks up the stream by
//! `command.stream_id`, which can yield Secure if a Secure stream exists with
//! that ID.
//!
//! BEFORE FIX: event.security_state == Secure (from stream lookup).
//! AFTER FIX:  event.security_state == NonSecure (always).
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, PRIEntry, PagePermissions, SecurityState,
    StreamConfig, StreamWorld, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN + PRIQEN enabled.
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    smmu
}

/// Returns true when GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
fn is_gerror_cmdq_err_active(smmu: &SMMU) -> bool {
    (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR != 0
}

/// Build a minimal CMD_TLBI_EL2_ALL command.
const fn cmd_tlbi_el2_all() -> CommandEntry {
    CommandEntry::new(CommandType::TlbiEl2All, 0, 0)
}

/// Build a stage-1 stream config with the given StreamWorld (STRW).
fn stage1_with_strw(strw: StreamWorld) -> StreamConfig {
    StreamConfig {
        strw,
        ..StreamConfig::stage1_only()
    }
}

// ============================================================================
// BUG-NEW-18: CMD_TLBI_EL2_ALL must not evict El1El0 entries (ARM §4.4.2.7)
// ============================================================================

/// BUG-NEW-18 (primary): CMD_TLBI_EL2_ALL must preserve TLB entries tagged
/// with `StreamWorld::El1El0` and evict only entries tagged `El2` / `El2E2h`.
///
/// ARM §4.4.2.7: CMD_TLBI_EL2_ALL invalidates EL2 translations only.
///
/// Stream A uses NonSecure + El1El0 (the only valid STRW for NonSecure besides
/// El2E2h).  Stream B uses Secure + El2 (valid for Secure streams per §5.2).
///
/// BEFORE FIX: `invalidate_all()` removes ALL entries, including El1El0 →
///   the second translate for stream A is a TLB miss → hit count unchanged.
/// AFTER FIX: `invalidate_el2_all()` preserves El1El0 entries →
///   the second translate for stream A is a TLB hit → hit count increases.
#[test]
fn bug_new18_tlbi_el2_all_preserves_el1el0_entries() {
    let smmu = make_smmu();

    // Stream A: NonSecure + El1El0 (must NOT be evicted by CMD_TLBI_EL2_ALL)
    let sid_a = StreamID::new(0x10).unwrap();
    let config_a = stage1_with_strw(StreamWorld::El1El0); // NonSecure default
    smmu.configure_stream(sid_a, config_a).unwrap();

    // Stream B: Secure + El2 (MUST be evicted by CMD_TLBI_EL2_ALL)
    // NonSecure+El2 is ILLEGAL per §5.2 (STRW bit[0]=1 forbidden for NS);
    // Secure+El2 (0b01) is valid per §5.2.
    let sid_b = StreamID::new(0x11).unwrap();
    let config_b = StreamConfig {
        security_state: SecurityState::Secure,
        ..stage1_with_strw(StreamWorld::El2)
    };
    smmu.configure_stream(sid_b, config_b).unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova_a = IOVA::new(0x1000).unwrap();
    let iova_b = IOVA::new(0x2000).unwrap();
    let pa_a = PA::new(0x0100_1000).unwrap();
    let pa_b = PA::new(0x0100_2000).unwrap();
    let perms = PagePermissions::read_write();

    // Create PASIDs and map pages for both streams
    smmu.create_pasid(sid_a, pasid).unwrap();
    smmu.create_pasid(sid_b, pasid).unwrap();
    smmu.map_page(sid_a, pasid, iova_a, pa_a, perms, SecurityState::NonSecure).unwrap();
    smmu.map_page(sid_b, pasid, iova_b, pa_b, perms, SecurityState::Secure).unwrap();

    // First translation for both streams — populates TLB (miss → insert)
    smmu.translate(sid_a, pasid, iova_a, AccessType::Read, SecurityState::NonSecure)
        .expect("stream A first translate must succeed");
    smmu.translate(sid_b, pasid, iova_b, AccessType::Read, SecurityState::Secure)
        .expect("stream B first translate must succeed");

    // Record TLB hit count before invalidation
    let stats_before = smmu.get_cache_statistics();
    let hits_before = stats_before.tlb_hits();

    // Issue CMD_TLBI_EL2_ALL via command queue
    let cmd = cmd_tlbi_el2_all();
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Second translation for stream A (El1El0).
    // BEFORE FIX: TLB was fully flushed → miss → hits_after == hits_before.
    // AFTER FIX:  El1El0 entry survives → hit → hits_after > hits_before.
    smmu.translate(sid_a, pasid, iova_a, AccessType::Read, SecurityState::NonSecure)
        .expect("stream A second translate must succeed");

    let stats_after = smmu.get_cache_statistics();
    let hits_after = stats_after.tlb_hits();

    assert!(
        hits_after > hits_before,
        "BUG-NEW-18: CMD_TLBI_EL2_ALL must preserve El1El0 TLB entries. \
         Expected TLB hit for stream A after El2-only invalidation. \
         hits_before={}, hits_after={} (ARM §4.4.2.7)",
        hits_before, hits_after
    );
}

/// BUG-NEW-18 (eviction): CMD_TLBI_EL2_ALL must evict TLB entries tagged
/// with `StreamWorld::El2`.
///
/// Uses Secure + El2 (valid: §5.2 permits El2 for Secure streams).
///
/// BEFORE FIX: `invalidate_all()` also evicts El2 (trivially passes the
///             miss-count check but the preservation test above catches the bug).
/// AFTER FIX:  `invalidate_el2_all()` specifically evicts El2 — passes.
#[test]
fn bug_new18_tlbi_el2_all_evicts_el2_entries() {
    let smmu = make_smmu();

    // Secure + El2 is the only valid combination for STRW=El2 (§5.2)
    let sid_b = StreamID::new(0x21).unwrap();
    let config_b = StreamConfig {
        security_state: SecurityState::Secure,
        ..stage1_with_strw(StreamWorld::El2)
    };
    smmu.configure_stream(sid_b, config_b).unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova_b = IOVA::new(0x3000).unwrap();
    let pa_b = PA::new(0x0100_3000).unwrap();
    let perms = PagePermissions::read_write();

    smmu.create_pasid(sid_b, pasid).unwrap();
    smmu.map_page(sid_b, pasid, iova_b, pa_b, perms, SecurityState::Secure).unwrap();

    // First translate — populates TLB
    smmu.translate(sid_b, pasid, iova_b, AccessType::Read, SecurityState::Secure)
        .expect("stream B first translate must succeed");

    // Capture stats to measure TLB misses
    let stats_before = smmu.get_cache_statistics();
    let misses_before = stats_before.tlb_misses();

    // Issue CMD_TLBI_EL2_ALL — should evict El2 entry
    smmu.submit_command(cmd_tlbi_el2_all()).unwrap();
    smmu.process_command_queue().unwrap();

    // Second translate — El2 entry was evicted → should be a TLB miss
    smmu.translate(sid_b, pasid, iova_b, AccessType::Read, SecurityState::Secure)
        .expect("stream B second translate must succeed");

    let stats_after = smmu.get_cache_statistics();
    let misses_after = stats_after.tlb_misses();

    assert!(
        misses_after > misses_before,
        "BUG-NEW-18: CMD_TLBI_EL2_ALL must evict El2 TLB entries. \
         Expected TLB miss for stream B after El2 invalidation. \
         misses_before={}, misses_after={} (ARM §4.4.2.7)",
        misses_before, misses_after
    );
}

/// BUG-NEW-18 (El2E2h): CMD_TLBI_EL2_ALL must also evict El2E2h entries.
///
/// ARM §4.4.2.7 covers both NS-EL2 and NS-EL2-E2H configurations.
/// El2E2h requires CR2.E2H=1 (§6.3.12) and is valid for NonSecure streams.
#[test]
fn bug_new18_tlbi_el2_all_evicts_el2e2h_entries() {
    let smmu = make_smmu();

    // Enable CR2.E2H so STRW=El2E2h (0b10) is honoured rather than treated as El2.
    smmu.set_cr2(SMMU::CR2_E2H);

    let sid_c = StreamID::new(0x31).unwrap();
    // NonSecure + El2E2h is valid per §5.2 (bit[0]=0 → not forbidden).
    let config_c = stage1_with_strw(StreamWorld::El2E2h);
    smmu.configure_stream(sid_c, config_c).unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova_c = IOVA::new(0x4000).unwrap();
    let pa_c = PA::new(0x0100_4000).unwrap();
    let perms = PagePermissions::read_write();

    smmu.create_pasid(sid_c, pasid).unwrap();
    smmu.map_page(sid_c, pasid, iova_c, pa_c, perms, SecurityState::NonSecure).unwrap();

    // First translate — populates TLB
    smmu.translate(sid_c, pasid, iova_c, AccessType::Read, SecurityState::NonSecure)
        .expect("stream C first translate must succeed");

    let stats_before = smmu.get_cache_statistics();
    let misses_before = stats_before.tlb_misses();

    // Issue CMD_TLBI_EL2_ALL — should evict El2E2h entry
    smmu.submit_command(cmd_tlbi_el2_all()).unwrap();
    smmu.process_command_queue().unwrap();

    // Second translate — El2E2h entry was evicted → TLB miss
    smmu.translate(sid_c, pasid, iova_c, AccessType::Read, SecurityState::NonSecure)
        .expect("stream C second translate must succeed");

    let stats_after = smmu.get_cache_statistics();
    let misses_after = stats_after.tlb_misses();

    assert!(
        misses_after > misses_before,
        "BUG-NEW-18: CMD_TLBI_EL2_ALL must evict El2E2h TLB entries. \
         Expected TLB miss for stream C after El2-only invalidation. \
         misses_before={}, misses_after={} (ARM §4.4.2.7)",
        misses_before, misses_after
    );
}

// ============================================================================
// BUG-NEW-22: submit_page_request() must emit E_PAGE_REQUEST immediately
// ============================================================================

/// BUG-NEW-22 (primary): calling `submit_page_request()` must immediately
/// enqueue an `E_PAGE_REQUEST` event without requiring a separate call to
/// `process_pri_queue()`.
///
/// ARM §3.12.4/§8.1: the SMMU reports a page request to software by writing
/// a Page Request event to the event queue when the request is received.
///
/// BEFORE FIX: no E_PAGE_REQUEST in event queue after submit — test fails.
/// AFTER FIX:  E_PAGE_REQUEST is present after submit — test passes.
#[test]
fn bug_new22_submit_page_request_emits_event_immediately() {
    let smmu = make_smmu();

    // Build a valid PRIEntry
    let request = PRIEntry {
        stream_id: 1,
        pasid: 0,
        requested_address: 0xDEAD_0000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    };

    // Submit the page request — this should emit E_PAGE_REQUEST immediately.
    smmu.submit_page_request(request).expect("submit_page_request must succeed");

    // Do NOT call process_pri_queue() — event must already be in the queue.

    let events = smmu.get_events();
    let page_req_event = events
        .iter()
        .find(|e| e.event_type == EventType::EPageRequest);

    assert!(
        page_req_event.is_some(),
        "BUG-NEW-22: submit_page_request() must emit E_PAGE_REQUEST to the \
         event queue immediately (without requiring process_pri_queue()). \
         Got {} events, none of type EPageRequest (ARM §3.12.4/§8.1)",
        events.len()
    );
}

/// BUG-NEW-22 (no-double-emit): calling `process_pri_queue()` after
/// `submit_page_request()` must NOT double-emit the E_PAGE_REQUEST event.
///
/// The priq_emitted cursor must prevent re-emission of already-emitted entries.
///
/// BEFORE FIX: if submit emits and process_pri_queue also emits → 2 events.
///   (Before submit is fixed, there's 0 events after submit and 1 after
///    process_pri_queue, so only 1 total — this test also fails because
///    the first assertion in bug_new22_submit_page_request_emits_event_immediately
///    already catches the bug.  But we keep this test to catch regression.)
/// AFTER FIX: exactly 1 E_PAGE_REQUEST event after both submit + process.
#[test]
fn bug_new22_process_pri_queue_does_not_double_emit() {
    let smmu = make_smmu();

    let request = PRIEntry {
        stream_id: 2,
        pasid: 0,
        requested_address: 0xBEEF_0000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
    };

    // Submit — emits once (after fix)
    smmu.submit_page_request(request).expect("submit_page_request must succeed");

    // Also call process_pri_queue() — must NOT emit a second time
    smmu.process_pri_queue().expect("process_pri_queue must succeed");

    let events = smmu.get_events();
    let count = events
        .iter()
        .filter(|e| e.event_type == EventType::EPageRequest)
        .count();

    assert_eq!(
        count, 1,
        "BUG-NEW-22: E_PAGE_REQUEST must be emitted exactly once — once at submit \
         time, and process_pri_queue() must not re-emit it. \
         Found {} E_PAGE_REQUEST events (ARM §3.12.4/§8.1)",
        count
    );
}

// ============================================================================
// BUG-NEW-24: CMD_TLBI_EL2_ALL must check IDR0.Hyp before executing (§4.4.2.7)
// ============================================================================

/// BUG-NEW-24 (positive): with the default IDR0.Hyp=1 (bit 9 set), CMD_TLBI_EL2_ALL
/// must execute normally — no CERROR_ILL, no GERROR.CMDQ_ERR.
///
/// This test verifies that the IDR0.Hyp guard added by BUG-NEW-24 does NOT
/// incorrectly fire when Hyp is supported (the common/default case).
///
/// BEFORE FIX: no guard → passes trivially.
/// AFTER FIX:  guard present but Hyp=1 → no CERROR_ILL → still passes.
#[test]
fn bug_new24_tlbi_el2_all_hyp1_executes_normally() {
    let smmu = make_smmu();

    // IDR0.Hyp is bit 9; get_idr0() always returns it as 1 (hardcoded).
    let idr0 = smmu.get_idr0();
    assert!(
        (idr0 & (1 << 9)) != 0,
        "BUG-NEW-24 precondition: IDR0.Hyp (bit 9) must be 1 in this model. \
         got IDR0=0x{:08x}",
        idr0
    );

    // Submit and process CMD_TLBI_EL2_ALL
    smmu.submit_command(cmd_tlbi_el2_all()).unwrap();
    let result = smmu.process_command_queue();

    // Must succeed (no CERROR_ILL)
    assert!(
        result.is_ok(),
        "BUG-NEW-24: CMD_TLBI_EL2_ALL with IDR0.Hyp=1 must succeed. Got {:?}",
        result
    );

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_NONE,
        "BUG-NEW-24: CMD_TLBI_EL2_ALL with IDR0.Hyp=1 must not set CERROR_ILL \
         in CMDQ_CONS.ERR (ARM §4.4.2.7)"
    );

    assert!(
        !is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-24: CMD_TLBI_EL2_ALL with IDR0.Hyp=1 must not assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.7)"
    );
}

/// BUG-NEW-24 (negative, `#[ignore]`): with IDR0.Hyp=0, CMD_TLBI_EL2_ALL must
/// raise CERROR_ILL and assert GERROR.CMDQ_ERR.
///
/// ARM §4.4.2.7: if SMMU_IDR0.HYP==0, this command is illegal.
///
/// This test is `#[ignore]` because there is currently no API to set
/// IDR0.Hyp=0.  The `get_idr0()` implementation hardcodes Hyp=1 (bit 9).
/// Once an IDR0 override API (e.g. `set_idr0_hyp_for_test(false)`) is added,
/// this test can be un-ignored to fully verify the negative path.
#[test]
#[ignore = "Requires IDR0.Hyp=0 override API — not yet exposed (see BUG-NEW-24)"]
fn bug_new24_tlbi_el2_all_hyp0_raises_cerror_ill() {
    let smmu = make_smmu();
    // IDR0.Hyp override would go here once the API exists.
    // smmu.set_idr0_hyp_for_test(false);

    smmu.submit_command(cmd_tlbi_el2_all()).unwrap();
    let _ = smmu.process_command_queue(); // expected to return Err(InvalidCommandParameters)

    assert_eq!(
        smmu.get_cmdq_cons_err(),
        SMMU::CERROR_ILL,
        "BUG-NEW-24: CMD_TLBI_EL2_ALL with IDR0.Hyp=0 must set CERROR_ILL \
         in CMDQ_CONS.ERR (ARM §4.4.2.7)"
    );

    assert!(
        is_gerror_cmdq_err_active(&smmu),
        "BUG-NEW-24: CMD_TLBI_EL2_ALL with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR \
         (ARM §4.4.2.7)"
    );
}

// ============================================================================
// BUG-NEW-26: CMD_SYNC completion event must always use SecurityState::NonSecure
// ============================================================================

/// BUG-NEW-26 (primary): CMD_SYNC has no StreamID operand — the
/// `CommandSyncCompletion` event must always carry `SecurityState::NonSecure`,
/// regardless of whether a stream with the same ID exists and is configured Secure.
///
/// ARM §4.8: CMD_SYNC carries no StreamID; the security state must not be
/// derived from a stream lookup.
///
/// Setup:
///   - Configure stream 0x50 with `SecurityState::Secure`.
///   - Issue CMD_SYNC with cs=1 and stream_id=0x50.
///   - Retrieve the CommandSyncCompletion event.
///
/// BEFORE FIX: stream lookup finds the Secure stream → event.security_state=Secure
///             → assertion on NonSecure fails → test fails (red).
/// AFTER FIX:  stream_sec_state = NonSecure unconditionally → event.security_state=NonSecure
///             → assertion passes (green).
#[test]
fn bug_new26_cmd_sync_completion_always_nonsecure() {
    let smmu = make_smmu();

    let stream_id = StreamID::new(0x50).unwrap();
    let secure_config = StreamConfig {
        security_state: SecurityState::Secure,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, secure_config).unwrap();

    // Issue CMD_SYNC with CS=1 (SIG_IRQ) to trigger a CommandSyncCompletion event.
    // stream_id=0x50 points to a Secure stream — the completion event must ignore this.
    let mut cmd = CommandEntry::new(CommandType::Sync, 0x50, 0);
    cmd.cs = 0x01; // SIG_IRQ

    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let events = smmu.get_events();
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::CommandSyncCompletion)
        .expect("CommandSyncCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::NonSecure,
        "BUG-NEW-26: CommandSyncCompletion.security_state must always be NonSecure \
         (CMD_SYNC has no StreamID operand, ARM §4.8). \
         Got: {:?}",
        completion.security_state
    );
}

/// BUG-NEW-26 (negative): a stream configured with `SecurityState::NonSecure`
/// still produces a `NonSecure` completion event (regression check).
///
/// This test passes both before and after the fix, confirming the common case
/// is unaffected.
#[test]
fn bug_new26_cmd_sync_nonsecure_stream_stays_nonsecure() {
    let smmu = make_smmu();

    let stream_id = StreamID::new(0x51).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();

    let mut cmd = CommandEntry::new(CommandType::Sync, 0x51, 0);
    cmd.cs = 0x01;

    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let events = smmu.get_events();
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::CommandSyncCompletion)
        .expect("CommandSyncCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::NonSecure,
        "BUG-NEW-26 (negative): NonSecure stream must still produce NonSecure \
         CommandSyncCompletion event. Got: {:?}",
        completion.security_state
    );
}

/// BUG-NEW-26 (no-stream): CMD_SYNC with a stream_id that has no configured stream
/// must also produce a `NonSecure` completion event (the fallback `map_or` default
/// was already NonSecure, but after the fix stream lookup is removed entirely).
#[test]
fn bug_new26_cmd_sync_unknown_stream_id_nonsecure() {
    let smmu = make_smmu();

    // No stream configured — stream_id 0x99 does not exist.
    let mut cmd = CommandEntry::new(CommandType::Sync, 0x99, 0);
    cmd.cs = 0x01;

    smmu.submit_command(cmd).expect("submit_command must succeed");
    smmu.process_command_queue().expect("process_command_queue must succeed");

    let events = smmu.get_events();
    let completion = events
        .iter()
        .find(|e| e.event_type == EventType::CommandSyncCompletion)
        .expect("CommandSyncCompletion event must be present");

    assert_eq!(
        completion.security_state,
        SecurityState::NonSecure,
        "BUG-NEW-26 (no-stream): CommandSyncCompletion with unknown stream_id \
         must be NonSecure. Got: {:?}",
        completion.security_state
    );
}
