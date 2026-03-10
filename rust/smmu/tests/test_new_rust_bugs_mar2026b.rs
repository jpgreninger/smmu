#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! Tests for five new bugs in the Rust ARM SMMU v3 implementation (March 2026, set B).
//!
//! BUG-NEW-RUST-1: S1DSS TOCTOU — stale s1dss_will_override snapshot allows
//!   caching a CD[0] result that should not be cached when s1dss is concurrently
//!   changed from 0x02 to 0x01.
//!
//! BUG-NEW-RUST-2: process_command_queue() SeqCst fence does not close the
//!   gerror/gerrorn TOCTOU window — a double-load seqlock is required.
//!
//! BUG-NEW-RUST-3: PriResp unconditionally advances priq_cons even when the
//!   matched entry is not at the front of the VecDeque (pos != 0).
//!
//! BUG-NEW-RUST-4: submit_event() bypasses §7.4 overflow and stall semantics —
//!   OVFLG is not toggled when a non-stall event is dropped from a full queue.
//!
//! BUG-NEW-RUST-5: shutdown() clears streams before zeroing stream_count —
//!   get_stream_count() must return 0 after shutdown() returns.

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventEntry, EventType, PagePermissions, PRIEntry,
    SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ─── helpers ──────────────────────────────────────────────────────────────────

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

// ─── BUG-NEW-RUST-1 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-1: S1DSS TOCTOU — stale `s1dss_will_override` snapshot.
///
/// Scenario:
/// 1. Configure stream with s1dss=0x02 (use CD[0]) and s1cd_max=4.
/// 2. Map a page at IOVA 0x1000 → PA 0x8000_1000 for PASID=0.
/// 3. Translate PASID=0 → s1dss==0x02, so CD[0] result is cached (PA=0x8000_1000).
/// 4. Reconfigure the stream with s1dss=0x01 (identity bypass for PASID=0).
/// 5. Translate PASID=0 again.
///    - Correct: s1dss=0x01 → identity bypass → PA == IOVA == 0x1000.
///    - Buggy:   stale TLB entry hit → PA == 0x8000_1000 (wrong).
#[test]
fn rust1_s1dss_toctou_stale_tlb_invalidated_on_reconfigure() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream = sid(0xA0);
    let p0 = pasid(0);
    let test_iova = iova(0x1000);
    let mapped_pa = 0x8000_1000u64;

    // Configure with s1dss=0x02 (use CD[0])
    let cfg_v2 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .s1dss(0x02)
        .s1cd_max(4)
        .build()
        .unwrap();
    smmu.configure_stream(stream, cfg_v2).unwrap();
    smmu.create_pasid(stream, p0).unwrap();

    // Map a real page so CD[0] translate succeeds
    smmu.map_page(
        stream,
        p0,
        test_iova,
        pa(mapped_pa),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate: s1dss=0x02 → uses CD[0] → result may be cached
    let r1 = smmu.translate(stream, p0, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "First translate (s1dss=0x02) must succeed: {r1:?}");
    assert_eq!(
        r1.unwrap().physical_address().as_u64(),
        mapped_pa,
        "First translate must return the mapped PA"
    );

    // Reconfigure with s1dss=0x01 (identity bypass for PASID=0)
    let cfg_v1 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .s1dss(0x01)
        .s1cd_max(4)
        .build()
        .unwrap();
    smmu.reconfigure_stream(stream, cfg_v1).unwrap();

    // Second translate: with s1dss=0x01, PA must equal IOVA (identity bypass)
    let r2 = smmu.translate(stream, p0, test_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(r2.is_ok(), "Second translate (s1dss=0x01) must succeed: {r2:?}");
    assert_eq!(
        r2.unwrap().physical_address().as_u64(),
        test_iova.as_u64(),
        "BUG-NEW-RUST-1: after reconfigure to s1dss=0x01, PA must equal IOVA (identity), \
         not the stale CD[0] PA"
    );
}

// ─── BUG-NEW-RUST-2 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-2: process_command_queue() must use a double-load seqlock instead
/// of a SeqCst fence to correctly detect GERROR_CMDQ_ERR.
///
/// A SeqCst fence between two separate loads does NOT prevent a concurrent
/// signal_gerror() write from landing between them.  A seqlock (re-read gerror
/// after gerrorn and retry if changed) is the correct fix.
///
/// Strategy: activate CMDQ_ERR by processing an invalid CfgiSte command (unknown
/// stream_id), then verify:
///   (a) A subsequent call to process_command_queue() with CMDQ_ERR active returns
///       Ok(0) — no commands are consumed.
///   (b) After clear_gerror(), process_command_queue() resumes and processes the
///       pending command.
///
/// This test validates the observable invariant.  The seqlock correctness cannot
/// be forced deterministically in a single-threaded test, but the invariant it
/// protects must hold in all single-threaded cases.
#[test]
fn rust2_process_command_queue_seqlock_halts_on_cmdq_err() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Activate CMDQ_ERR via a command for an unknown stream_id
    let bad_stream = StreamID::new(0xBEEF).unwrap();
    smmu.submit_command(CommandEntry::new(CommandType::CfgiSte, bad_stream.as_u32(), 0))
        .unwrap();
    let _ = smmu.process_command_queue(); // activates CMDQ_ERR

    let active = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(active, 0, "CMDQ_ERR must be active after processing bad command");

    // Submit a valid sync command; it must NOT be processed while CMDQ_ERR is active
    let good_cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    smmu.submit_command(good_cmd).unwrap();
    assert_eq!(smmu.get_command_queue_size(), 1, "one valid command pending");

    // process_command_queue() must return Ok(0) — CMDQ_ERR is active
    let r1 = smmu.process_command_queue();
    assert!(r1.is_ok(), "process_command_queue() must return Ok when CMDQ_ERR is active");
    assert_eq!(
        r1.unwrap(),
        0,
        "BUG-NEW-RUST-2: must NOT process commands while CMDQ_ERR is active; \
         double-load seqlock required instead of SeqCst fence"
    );
    assert_eq!(
        smmu.get_command_queue_size(),
        1,
        "valid command must remain in queue while CMDQ_ERR is active"
    );

    // Clear the error — command queue processing must resume
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    assert_eq!(
        (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be inactive after clear_gerror"
    );

    let r2 = smmu.process_command_queue();
    assert!(r2.is_ok(), "process_command_queue() must succeed after clearing CMDQ_ERR");
    assert_eq!(
        r2.unwrap(),
        1,
        "BUG-NEW-RUST-2: exactly one command must be processed after clearing CMDQ_ERR"
    );
}

// ─── BUG-NEW-RUST-3 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-3: PriResp must NOT advance priq_cons when the matched entry is
/// not the front entry of the queue (pos != 0).
///
/// Scenario:
/// 1. Enable PRI queue (CR0.PRIQEN).
/// 2. Submit three PRI entries for three different stream IDs (stream 1, 2, 3)
///    with distinct prg_index values (1, 2, 3).
/// 3. Record priq_cons BEFORE issuing PriResp.
/// 4. Issue CMD_PRI_RESP for the MIDDLE entry (stream_id=2, prg_index=2).
///    After removal, two entries remain (positions 0 and 1 in VecDeque).
/// 5. priq_cons must NOT have advanced (the front entry was NOT consumed).
///    Only consuming the front entry should advance CONS.
#[test]
fn rust3_pri_resp_middle_entry_does_not_advance_priq_cons() {
    let smmu = SMMU::new();
    smmu.set_cr0(
        SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN,
    );

    // Submit three PRI entries in order: stream 1 (prg=1), stream 2 (prg=2), stream 3 (prg=3)
    let entry1 = PRIEntry::with_address(1, 0, 0x1000, AccessType::Read).with_prg_index(1);
    let entry2 = PRIEntry::with_address(2, 0, 0x2000, AccessType::Read).with_prg_index(2);
    let entry3 = PRIEntry::with_address(3, 0, 0x3000, AccessType::Read).with_prg_index(3);

    smmu.submit_page_request(entry1).unwrap();
    smmu.submit_page_request(entry2).unwrap();
    smmu.submit_page_request(entry3).unwrap();
    assert_eq!(smmu.get_pri_queue_size(), 3, "three PRI entries must be in the queue");

    // Record CONS before PriResp
    let cons_before = smmu.priq_cons_index();

    // Issue CMD_PRI_RESP for the middle entry (stream_id=2, prg_index=2)
    let mut cmd = CommandEntry::new(CommandType::PriResp, 2, 0);
    cmd.prg_index = 2;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // The middle entry must be removed
    assert_eq!(
        smmu.get_pri_queue_size(),
        2,
        "one PRI entry must be removed by PriResp"
    );

    // CONS must NOT have advanced — stream_id=1 is still at the front
    let cons_after = smmu.priq_cons_index();
    assert_eq!(
        cons_after,
        cons_before,
        "BUG-NEW-RUST-3: priq_cons must NOT advance when a non-front PRI entry is retired"
    );
}

// ─── BUG-NEW-RUST-4 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-4: submit_event() must toggle OVFLG when the queue is full and
/// the event is dropped (ARM §7.4).
///
/// The public `submit_event()` currently returns `EventQueueFull` without
/// toggling OVFLG.  The internal `record_translation_fault()` path correctly
/// toggles OVFLG.  After this fix, `submit_event()` must also toggle OVFLG.
///
/// Strategy:
/// 1. Create an SMMU with a small event queue (16 entries).
/// 2. Enable EVENTQEN.
/// 3. Fill the queue to capacity via submit_event().
/// 4. Call submit_event() once more with a non-stall event.
/// 5. OVFLG (bit 31 of EVENTQ_PROD) must differ from OVACKFLG (bit 31 of EVENTQ_CONS).
#[test]
fn rust4_submit_event_full_queue_toggles_ovflg() {
    use smmu::types::QueueConfig;
    let capacity: usize = 16;
    let smmu = SMMU::with_config(smmu::types::SMMUConfig::from(QueueConfig {
        event_queue_size: capacity,
        command_queue_size: 32,
        pri_queue_size: 32,
    }));
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    let make_event = |n: u32| EventEntry {
        event_type: EventType::FTranslation,
        stream_id: n,
        pasid: 0,
        address: 0,
        security_state: SecurityState::NonSecure,
        error_code: 0,
        timestamp: 0,
        stall: false,
        stag: 0,
    };

    // Fill the event queue to capacity
    for i in 0..u32::try_from(capacity).expect("capacity fits in u32") {
        smmu.submit_event(make_event(i)).unwrap();
    }
    assert_eq!(smmu.get_event_queue_size(), capacity as u64, "queue must be full");

    // Verify no overflow yet
    let prod_before = smmu.get_eventq_prod();
    let cons_before = smmu.eventq_cons_index();
    assert_eq!(
        (prod_before >> 31) & 1,
        (cons_before >> 31) & 1,
        "OVFLG must not be active before overflow"
    );

    // Submit one more non-stall event — queue is full, must be dropped and OVFLG toggled
    let cap_u32 = u32::try_from(capacity).expect("capacity fits in u32");
    let overflow_event = make_event(cap_u32 + 1);
    let _ = smmu.submit_event(overflow_event);

    // OVFLG must now be active
    let prod_after = smmu.get_eventq_prod();
    let cons_after = smmu.eventq_cons_index();
    assert_ne!(
        (prod_after >> 31) & 1,
        (cons_after >> 31) & 1,
        "BUG-NEW-RUST-4: OVFLG must be toggled when submit_event() drops a non-stall \
         event due to full queue (prod bit-31 must differ from cons bit-31)"
    );
}

// ─── BUG-NEW-RUST-5 ───────────────────────────────────────────────────────────

/// BUG-NEW-RUST-5: get_stream_count() must return 0 after shutdown() returns.
///
/// The bug: shutdown() calls `self.streams.clear()` BEFORE
/// `self.stream_count.store(0)`.  Any observer reading stream_count between
/// those two operations sees count > 0 with an empty streams map.
///
/// The fix: zero stream_count BEFORE clearing streams.
///
/// This test verifies the externally-observable postcondition:
/// after shutdown() returns, get_stream_count() == 0.
#[test]
fn rust5_get_stream_count_is_zero_after_shutdown() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    // Register two streams
    smmu.configure_stream(sid(1), StreamConfig::bypass()).unwrap();
    smmu.configure_stream(sid(2), StreamConfig::bypass()).unwrap();
    assert_eq!(smmu.get_stream_count(), 2, "two streams must be registered before shutdown");

    // Shutdown
    smmu.shutdown().unwrap();

    // After shutdown() returns, stream_count must be 0
    assert_eq!(
        smmu.get_stream_count(),
        0,
        "BUG-NEW-RUST-5: get_stream_count() must return 0 immediately after shutdown() returns"
    );
}
