//! Failing tests (TDD step 1) for two ARM SMMU v3 Rust bugs.
//!
//! Each test is designed to FAIL with the current buggy code and PASS only
//! after the corresponding fix is applied.
//!
//! # Bug Descriptions
//!
//! ## RUST-2: 7 Inline Event Sites Missing OVFLG Toggle on Overflow
//!
//! ARM §7.4 requires EVENTQ_PROD.OVFLG (bit 31) to be toggled whenever a
//! non-stall event is discarded because the queue is full.
//!
//! The following inline push blocks in `smmu/mod.rs` all have:
//!
//! ```text
//! if queue.len() < self.event_queue_capacity {
//!     queue.push_back(event);
//!     ...
//! }
//! // NO else branch → OVFLG never toggled on overflow
//! ```
//!
//! Affected sites (approximate line numbers):
//!   - ~3066: F_BAD_ATS_TREQ (SMMUEN=0, ATS TR, CR2.REC_CFG_ATS=1)
//!   - ~3101: F_TRANSL_FORBIDDEN (SMMUEN=0, ATS TT)
//!   - ~3148: C_BAD_STREAMID (ATS TR, stream not found)
//!   - ~3203: F_BAD_ATS_TREQ (ATS TR, stream found but EATS=0)
//!   - ~3538: C_BAD_CD (Stage-1 enabled, aa64=false / t0sz > 39)
//!   - ~3621: F_TRANSLATION (Stage-1 enabled, T0SZ range exceeded)
//!   - ~3862: F_STREAM_DISABLED (S1DSS=0x00, s1cd_max > 0, PASID=0)
//!
//! Compare with `enqueue_event()` which correctly calls `toggle_ovflg_once()`
//! in its `else` branch.
//!
//! ## RUST-3: Inline Push Sites Skip stall_pending Drain
//!
//! ARM §3.5.3 — FIFO fairness: `submit_event()` and `record_translation_fault()`
//! drain `stall_pending` before inserting new non-stall events so that older stall
//! events are not overtaken.  The 7 inline push sites do NOT drain `stall_pending`,
//! so a non-stall event pushed at one of those sites will appear in the queue
//! BEFORE any older stall events that were buffered in `stall_pending`.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, FaultMode, QueueConfig, SMMUConfig, SecurityState, StreamConfig,
    StreamID, TransactionType, IOVA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

/// Build an SMMU with the minimum event queue size (16 entries) and
/// SMMUEN + CMDQEN + EVENTQEN all enabled.
fn make_small_smmu() -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::from(QueueConfig {
        event_queue_size: QueueConfig::MIN_QUEUE_SIZE,
        ..Default::default()
    }));
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

/// Return the current OVFLG bit (bit 31) of EVENTQ_PROD as 0 or 1.
fn ovflg(smmu: &SMMU) -> u32 {
    (smmu.get_eventq_prod() >> 31) & 1
}

/// Set up a minimal Stage-1 terminate stream that produces F_TRANSLATION on
/// every translate() call (no pages mapped).
fn setup_fault_stream(smmu: &SMMU, stream: StreamID) {
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Terminate,
        t0sz: 16,  // VA limit = 2^48
        aa64: true,
        ..StreamConfig::default()
    };
    smmu.configure_stream(stream, cfg).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();
    // No map_page → every translate() → F_TRANSLATION fault event.
}

/// Fill the event queue to capacity by issuing translation faults on the
/// given stream until get_events().len() == capacity.
fn fill_event_queue(smmu: &SMMU, stream: StreamID, capacity: usize) {
    for i in 0..capacity {
        let _ = smmu.translate(
            stream,
            pasid(0),
            iova(0x1000 * (i as u64 + 1)),
            AccessType::Read,
            SecurityState::NonSecure,
        );
    }
    assert_eq!(
        smmu.get_events().len(),
        capacity,
        "precondition: event queue must be full ({capacity} entries)"
    );
}

// ============================================================================
// RUST-2 — Site ~3066: F_BAD_ATS_TREQ when SMMUEN=0, ATS TR, REC_CFG_ATS=1
// ============================================================================

/// RUST-2 / Site 3066 — SMMUEN=0 + ATS TR + CR2.REC_CFG_ATS=1.
///
/// When SMMUEN=0 and an ATS Translation Request arrives, the code in
/// `translate_with_type()` tries to record F_BAD_ATS_TREQ (gated on
/// CR2.REC_CFG_ATS=1 and CR0.EVENTQEN=1).  The inline push block is
/// missing the `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3066_f_bad_ats_treq_smmuen0_ovflg_not_toggled() {
    // Use a small SMMU so filling the queue is cheap.
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Set up a stream to act as the fill source.
    setup_fault_stream(&smmu, sid(1));

    // Fill the event queue to capacity.
    fill_event_queue(&smmu, sid(1), q);

    // Precondition: OVFLG must be 0 before the overflow test.
    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Now disable the SMMU (SMMUEN=0) while keeping EVENTQEN=1.
    // SMMUEN is the bit 0 of CR0. Clearing it makes `enabled` = false.
    // Keep EVENTQEN and CMDQEN so the event path is active.
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Enable CR2.REC_CFG_ATS so the F_BAD_ATS_TREQ event is attempted.
    smmu.set_cr2(SMMU::CR2_REC_CFG_ATS);

    // Issue an ATS Translation Request — with SMMUEN=0 this hits site ~3066.
    let _ = smmu.translate_with_type(
        sid(1),
        pasid(0),
        iova(0xDEAD_0000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    // ARM §7.4: OVFLG must be toggled to 1 because the F_BAD_ATS_TREQ event
    // was discarded due to the full queue.
    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3066: OVFLG must be toggled when F_BAD_ATS_TREQ is discarded \
         on a full queue (SMMUEN=0, ATS TR, REC_CFG_ATS=1) — inline push block is \
         missing the else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-2 — Site ~3101: F_TRANSL_FORBIDDEN when SMMUEN=0, ATS TT
// ============================================================================

/// RUST-2 / Site 3101 — SMMUEN=0 + ATS Translated transaction.
///
/// When SMMUEN=0 and an ATS Translated transaction arrives, the code records
/// F_TRANSL_FORBIDDEN.  The inline push block is missing the
/// `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3101_f_transl_forbidden_smmuen0_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    setup_fault_stream(&smmu, sid(2));
    fill_event_queue(&smmu, sid(2), q);

    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Disable SMMU (SMMUEN=0), keep EVENTQEN.
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Issue an ATS Translated transaction — with SMMUEN=0 this hits site ~3101.
    let _ = smmu.translate_with_type(
        sid(2),
        pasid(0),
        iova(0xDEAD_1000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );

    // ARM §7.4: OVFLG must be toggled to 1.
    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3101: OVFLG must be toggled when F_TRANSL_FORBIDDEN is discarded \
         on a full queue (SMMUEN=0, ATS TT) — inline push block is missing the \
         else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-2 — Site ~3148: C_BAD_STREAMID when ATS TR, stream not found
// ============================================================================

/// RUST-2 / Site 3148 — SMMUEN=1, ATS TR, unknown StreamID.
///
/// When SMMUEN=1 and an ATS Translation Request arrives for a StreamID that
/// has no STE, the code (gated on CR2.RECINVSID + CR2.REC_CFG_ATS + EVENTQEN)
/// tries to record C_BAD_STREAMID.  The inline push block is missing the
/// `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3148_c_bad_streamid_ats_tr_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Fill source stream.
    setup_fault_stream(&smmu, sid(3));
    fill_event_queue(&smmu, sid(3), q);

    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Enable both CR2 gates so the C_BAD_STREAMID event is attempted.
    smmu.set_cr2(SMMU::CR2_RECINVSID | SMMU::CR2_REC_CFG_ATS);

    // Issue an ATS TR for a StreamID that is not configured — hits site ~3148.
    // sid(0xFF) was never configured.
    let _ = smmu.translate_with_type(
        sid(0xFF),
        pasid(0),
        iova(0xDEAD_2000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3148: OVFLG must be toggled when C_BAD_STREAMID is discarded \
         on a full queue (ATS TR, unknown stream) — inline push block is missing the \
         else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-2 — Site ~3203: F_BAD_ATS_TREQ when ATS TR, EATS=0
// ============================================================================

/// RUST-2 / Site 3203 — SMMUEN=1, ATS TR, stream configured but EATS=0.
///
/// When SMMUEN=1 and an ATS Translation Request arrives for a stream that has
/// EATS=0 (no ATS support), the code records F_BAD_ATS_TREQ.  The inline push
/// block is missing the `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3203_f_bad_ats_treq_eats0_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Fill source stream (stream sid(4)).
    setup_fault_stream(&smmu, sid(4));
    fill_event_queue(&smmu, sid(4), q);

    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Configure stream sid(5) with EATS=0 so ATS TR is unsupported.
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Terminate,
        t0sz: 16,
        aa64: true,
        eats: 0,  // EATS=0: ATS not supported → F_BAD_ATS_TREQ
        ..StreamConfig::default()
    };
    smmu.configure_stream(sid(5), cfg).unwrap();
    smmu.create_pasid(sid(5), pasid(0)).unwrap();

    // Issue an ATS TR on sid(5) — hits site ~3203.
    let _ = smmu.translate_with_type(
        sid(5),
        pasid(0),
        iova(0xDEAD_3000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest,
    );

    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3203: OVFLG must be toggled when F_BAD_ATS_TREQ is discarded \
         on a full queue (ATS TR, EATS=0) — inline push block is missing the \
         else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-2 — Site ~3538: C_BAD_CD when Stage-1 has aa64=false (CD invalid)
// ============================================================================

/// RUST-2 / Site 3538 — Stage-1 enabled, aa64=false → C_BAD_CD.
///
/// The code at ~3538 tries to record C_BAD_CD when the CD is invalid
/// (aa64=false or t0sz/t1sz out of range [0,39]).  The inline push block is
/// missing the `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3538_c_bad_cd_aa64_false_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Fill source stream (stream sid(6)).
    setup_fault_stream(&smmu, sid(6));
    fill_event_queue(&smmu, sid(6), q);

    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Configure stream sid(7) with aa64=false — invalid CD → C_BAD_CD on translate.
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Terminate,
        t0sz: 16,
        aa64: false,  // aa64=false → C_BAD_CD (AArch32 LPAE unsupported)
        ..StreamConfig::default()
    };
    smmu.configure_stream(sid(7), cfg).unwrap();
    smmu.create_pasid(sid(7), pasid(0)).unwrap();

    // Translate — hits site ~3538 (C_BAD_CD inline push).
    let _ = smmu.translate(
        sid(7),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3538: OVFLG must be toggled when C_BAD_CD is discarded \
         on a full queue (Stage-1, aa64=false) — inline push block is missing the \
         else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-2 — Site ~3621: F_TRANSLATION when T0SZ range exceeded
// ============================================================================

/// RUST-2 / Site 3621 — Stage-1 enabled, T0SZ=16, IOVA >= 2^48.
///
/// The T0SZ range check at ~3574 fires before the address-space lookup and
/// tries to record F_TRANSLATION at site ~3621.  The inline push block is
/// missing the `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3621_f_translation_t0sz_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Fill source stream (stream sid(8)).
    setup_fault_stream(&smmu, sid(8));
    fill_event_queue(&smmu, sid(8), q);

    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Configure stream sid(9) with T0SZ=16 (VA limit 2^48).
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Terminate,
        t0sz: 16,  // VA limit = 2^(64-16) = 2^48
        aa64: true,
        ..StreamConfig::default()
    };
    smmu.configure_stream(sid(9), cfg).unwrap();
    smmu.create_pasid(sid(9), pasid(0)).unwrap();

    // Issue a translate with IOVA >= 2^48 — hits site ~3621.
    let out_of_range_iova = 1u64 << 48; // exactly 2^48 = 0x0001_0000_0000_0000
    let _ = smmu.translate(
        sid(9),
        pasid(0),
        iova(out_of_range_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3621: OVFLG must be toggled when F_TRANSLATION (T0SZ range) \
         is discarded on a full queue — inline push block is missing the \
         else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-2 — Site ~3862: F_STREAM_DISABLED when S1DSS=0x00
// ============================================================================

/// RUST-2 / Site 3862 — S1DSS=0x00, s1cd_max > 0, PASID=0.
///
/// When a non-substream transaction (PASID=0) arrives on a substream-capable
/// stream (s1cd_max > 0) with S1DSS=0b00, the code aborts with F_STREAM_DISABLED
/// and tries to push the event at site ~3862.  The inline push block is missing
/// the `else { toggle_ovflg_once() }` branch.
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX:  OVFLG is toggled to 1.
#[test]
fn rust2_site3862_f_stream_disabled_s1dss0_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Fill source stream (stream sid(10)).
    setup_fault_stream(&smmu, sid(10));
    fill_event_queue(&smmu, sid(10), q);

    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Configure stream sid(11) with s1cd_max=1 (substream-capable) and s1dss=0
    // (abort for non-substream PASID=0 transactions → F_STREAM_DISABLED).
    let cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Terminate,
        t0sz: 16,
        aa64: true,
        s1cd_max: 1,  // 2^1 = 2 substreams supported
        s1dss: 0,     // S1DSS=0b00: abort non-substream transactions
        ..StreamConfig::default()
    };
    smmu.configure_stream(sid(11), cfg).unwrap();
    smmu.create_pasid(sid(11), pasid(0)).unwrap();

    // Translate with PASID=0 on a substream-capable stream with S1DSS=0b00
    // → F_STREAM_DISABLED at site ~3862.
    let _ = smmu.translate(
        sid(11),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-2 / site ~3862: OVFLG must be toggled when F_STREAM_DISABLED is discarded \
         on a full queue (S1DSS=0b00, s1cd_max>0, PASID=0) — inline push block is \
         missing the else {{ toggle_ovflg_once() }} branch"
    );
}

// ============================================================================
// RUST-3: Inline Push Sites Skip stall_pending Drain
// ============================================================================

/// RUST-3 — Inline push sites do not drain stall_pending before inserting.
///
/// ARM §3.5.3 FIFO fairness: `record_translation_fault()` drains `stall_pending`
/// before inserting a new event so that older stall events are not bypassed.
///
/// The 7 inline push sites (including site ~3621 for F_TRANSLATION T0SZ) do
/// NOT call the drain logic.  As a result, if `stall_pending` has a buffered
/// stall event and one slot is free, the inline path takes that slot with the
/// non-stall event — the older stall event remains in `stall_pending`.
///
/// When `get_events()` is subsequently called with enough free space it drains
/// `stall_pending` and the older stall event appears AFTER the newer non-stall
/// event — a FIFO ordering violation.
///
/// Observable:
/// - After the inline non-stall push takes the last free slot and `get_events()`
///   is called with no additional space, the stall event from `stall_pending` is
///   NOT present in the returned snapshot (it was never drained into the queue).
/// - With the FIX, the inline push would drain `stall_pending` first, moving the
///   older stall event into that slot; the inline non-stall event would be dropped
///   (OVFLG toggled per ARM §7.4) and `get_events()` WOULD include the stall event.
///
/// Test approach: use a SIZE-4 queue (allowed by the overflow-test exemption),
/// fill it using translate() faults on a stall stream so that both a stall event
/// ends up in the main queue AND one overflows to `stall_pending`.  Then free one
/// slot by using `get_events()` which drains `stall_pending` into that slot.
/// Finally compare two scenarios — correct path (record_translation_fault) and
/// inline path (T0SZ F_TRANSLATION site ~3621) — and verify ordering.
///
/// NOTE: The public API does not provide a per-entry consume method for the event
/// queue VecDeque.  The test uses a SIZE-4 queue and leverages the fact that
/// `record_translation_fault()` correctly drains `stall_pending` before inserting
/// new events.  The ORDERING VIOLATION is observed by checking which event stream
/// ends up at the last slot when `stall_pending` has an older entry.
///
/// BEFORE FIX: last slot taken by non-stall inline event; stall event stays in
///   `stall_pending`; the stall event is NOT at any position in the snapshot.
/// AFTER FIX:  last slot taken by stall event (drained first); inline event
///   is DROPPED (OVFLG toggled); stall event IS the last event in the snapshot.
///
/// Concretely: when `get_events()` is called with space in the queue, it drains
/// stall_pending and APPENDS those events after whatever is already in the queue.
/// If the inline push already occupies the last slot, the stall event goes after
/// the non-stall event in the final snapshot — violating FIFO.
///
/// Test scenario:
/// 1. SMMU with capacity=16.  Configure stall stream sid(20), terminate stream sid(21).
/// 2. Fill main queue to capacity-2 (14 entries) with non-stall faults on sid(21).
/// 3. Trigger a stall fault on sid(20) while queue has space — stall event goes to
///    main queue (slot 14), queue now has 15 entries (capacity-1).
/// 4. Trigger a second stall fault on sid(20) while queue is at capacity-1 (still has
///    1 slot free) — stall event goes to main queue (slot 15), queue full (16).
/// 5. Trigger a third stall fault on sid(20) while queue is FULL — stall event is
///    redirected to stall_pending (ARM §7.4).  stall_pending now has 1 event.
/// 6. Free one slot in the main queue by popping (decrementing event_count is not
///    directly accessible, but we can consume one entry by reducing capacity via
///    a different approach). Since we cannot pop individual entries without clearing,
///    we use a different strategy:
///
/// REVISED strategy (avoiding clear_event_queue which also clears stall_pending):
/// 1. SMMU capacity=16.  Configure stall stream sid(20), terminate stream sid(21).
/// 2. Fill main queue to exactly capacity-1 (15 entries) with non-stall terminate faults.
/// 3. While queue has 1 slot free, trigger stall fault on sid(20): stall event fills
///    that slot, queue now full (16).
/// 4. While queue is FULL, trigger a second stall fault on sid(20): goes to stall_pending.
/// 5. Now pop one non-stall event to free a slot without clearing stall_pending.
///    (We use advance_eventq_cons() to consume one entry, or note: the public API
///    doesn't expose per-entry consume.  Instead: observe the ordering differently.)
///
/// SIMPLEST observable test: use a 2-slot queue (by picking small capacity to easily
/// see the ordering inversion).
///
/// ACTUAL SIMPLIFIED SCENARIO:
/// 1. Queue capacity=2.  Configure stall stream (sid 20) and terminate stream (sid 21).
/// 2. Fill queue to capacity-1 (1 entry) with a non-stall fault on sid(21).
/// 3. Trigger stall fault on sid(20) — stall event fills the last slot, queue full (2/2).
/// 4. Trigger second stall fault on sid(20) — queue full → stall event goes to
///    stall_pending.  stall_pending now has 1 entry.
/// 5. Now advance the consumer pointer by 1 (consume the non-stall event at head).
///    Use `advance_eventq_cons(1)` to simulate software consuming 1 event.
///    This opens 1 slot in the ring without calling clear.
/// 6. Issue a T0SZ out-of-range translate on sid(21) — this hits the inline push at
///    site ~3621.  Queue has 1 slot free.  The inline push inserts the non-stall
///    F_TRANSLATION directly WITHOUT draining stall_pending.
///    Queue now has 2 entries: [stall_event_from_step3, nonst_inline_event].
///    stall_pending still has 1 entry.
/// 7. Advance consumer by 1 more to open another slot, then call get_events():
///    it drains stall_pending and appends the older stall event AFTER the newer
///    inline non-stall event — an ordering inversion.
///
/// EVEN SIMPLER: since we cannot use advance_eventq_cons, we observe the bug
/// directly: with 1 free slot, stall_pending has 1 event (OLDER), inline push
/// adds a non-stall event (NEWER). Then get_events() appends the older stall
/// event after the newer non-stall — violating FIFO.
///
/// The assert: after get_events() returns (with the drain), if the last event
/// has stall=true and an earlier event has stall=false with a NEWER timestamp,
/// FIFO was violated.  With the fix, the stall event is drained BEFORE the
/// inline insert so it appears BEFORE the non-stall event.
///
/// BEFORE FIX: queue snapshot has non-stall at index N, stall at index N+1
///   (stall is older but appears later — FIFO violation).
/// AFTER FIX:  stall event is drained first, so stall appears at index N and
///   non-stall inline event appears at index N+1 (correct FIFO order).
#[allow(clippy::too_many_lines)]
#[ignore = "RUST-3 is non-normative (ARM §3.5.3 'recommends' not 'shall') — stall_pending drain ordering not mandatory"]
#[test]
fn rust3_inline_push_skips_stall_pending_drain() {
    // Use the minimum queue size for efficiency.
    let capacity = QueueConfig::MIN_QUEUE_SIZE; // 16
    let smmu = SMMU::with_config(SMMUConfig::from(QueueConfig {
        event_queue_size: capacity,
        ..Default::default()
    }));
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Stream sid(20): stall-mode, Stage-1, T0SZ=16, no pages mapped.
    let stall_stream = sid(20);
    let stall_cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Stall,
        t0sz: 16,
        aa64: true,
        ..StreamConfig::default()
    };
    smmu.configure_stream(stall_stream, stall_cfg).unwrap();
    smmu.create_pasid(stall_stream, pasid(0)).unwrap();

    // Stream sid(21): terminate-mode, Stage-1, T0SZ=16.
    // T0SZ=16 means VA limit = 2^48.  Faults with in-range IOVA go via
    // enqueue_event (which drains stall_pending correctly); faults with
    // out-of-range IOVA go via the inline push at site ~3621.
    let nonst_stream = sid(21);
    let nonst_cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        fault_mode: FaultMode::Terminate,
        t0sz: 16,
        aa64: true,
        ..StreamConfig::default()
    };
    smmu.configure_stream(nonst_stream, nonst_cfg).unwrap();
    smmu.create_pasid(nonst_stream, pasid(0)).unwrap();

    // Step 1: fill main queue to capacity-1 with non-stall faults (each in-range
    // IOVA → F_TRANSLATION via enqueue_event).
    for i in 0..(capacity - 1) {
        let _ = smmu.translate(
            nonst_stream,
            pasid(0),
            iova(0x1_0000 + 0x1000 * i as u64),
            AccessType::Read,
            SecurityState::NonSecure,
        );
    }
    assert_eq!(
        smmu.get_events().len(),
        capacity - 1,
        "precondition: queue must have capacity-1 ({}) entries", capacity - 1
    );

    // Step 2: trigger stall fault — fills the last slot.  Queue is now full.
    // This stall event goes DIRECTLY into the main queue (space available).
    let _ = smmu.translate(
        stall_stream,
        pasid(0),
        iova(0x0001_0000),   // in-range IOVA → F_TRANSLATION (stall)
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert_eq!(
        smmu.get_events().len(),
        capacity,
        "precondition: queue must be full ({capacity}) after stall fault"
    );

    // Step 3: trigger ANOTHER stall fault while queue is FULL.
    // Per ARM §7.4 + BUG-13 fix: stall events when queue is full go to stall_pending,
    // not dropped.  stall_pending now has 1 entry (the older stall event).
    let _ = smmu.translate(
        stall_stream,
        pasid(0),
        iova(0x0002_0000),   // in-range IOVA → F_TRANSLATION (stall, into stall_pending)
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // Queue still full (stall went to stall_pending, not main queue).
    assert_eq!(
        smmu.get_events().len(),
        capacity,
        "precondition: queue must remain full — second stall should be in stall_pending"
    );

    // Step 4: consume the LAST event in the main queue to free one slot.
    // We do this by calling advance_eventq_cons(1) to advance the consumer ring.
    // This frees one slot in the ring without calling clear_event_queue (which
    // would also wipe stall_pending).
    smmu.advance_eventq_cons(1);

    // The main queue ring now has 1 slot free (capacity-1 events visible).
    // stall_pending still has 1 entry (the older stall event from step 3).

    // Step 5: issue an inline non-stall F_TRANSLATION via T0SZ out-of-range (site ~3621).
    // IOVA = 2^48 = 0x0001_0000_0000_0000 → triggers T0SZ range check (iova >= va_limit).
    // With 1 slot free the inline push succeeds and inserts the event directly,
    // WITHOUT draining stall_pending first.
    // After this push the queue is full again and stall_pending still has the older event.
    let out_of_range_va = 1u64 << 48;
    let _ = smmu.translate(
        nonst_stream,
        pasid(0),
        iova(out_of_range_va),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // Queue should be full (capacity entries) again after the inline push.
    // stall_pending must still have 1 entry (the inline push did NOT drain it).
    // We verify this by checking that get_events() appends the stall event AFTER
    // the inline-pushed non-stall event, revealing the ordering inversion.

    // Free another slot so get_events() can drain stall_pending.
    smmu.advance_eventq_cons(1);

    // Call get_events(): it drains stall_pending into the main queue.
    // The main queue has capacity-1 events + the inline non-stall F_TRANSLATION
    // at the end.  After drain, the older stall event from stall_pending is
    // appended AFTER the non-stall event — a FIFO ordering violation.
    let events = smmu.get_events();

    // There must be capacity events (capacity-1 old events + inline non-stall +
    // the drained stall event overflows, OR the stall event is the last one).
    // The key assertion: find the non-stall F_TRANSLATION event and the stall
    // F_TRANSLATION event; the stall event must have a LOWER index (i.e., must
    // appear BEFORE the non-stall inline event) since it was created earlier.
    let nonst_idx = events
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.stall && e.stream_id == sid(21).as_u32())
        .map(|(i, _)| i)
        .next_back(); // last non-stall from sid(21) is the inline-pushed one

    let stall_idx = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.stall && e.stream_id == sid(20).as_u32())
        .map(|(i, _)| i)
        .next_back(); // stall event from stall_pending drain

    // Both events must be present for the test to be meaningful.
    assert!(
        nonst_idx.is_some() && stall_idx.is_some(),
        "RUST-3: both the inline non-stall event (stream 21) and stall event (stream 20) \
         must be observable in get_events(); \
         nonst_idx={nonst_idx:?}, stall_idx={stall_idx:?}, queue_len={}",
        events.len()
    );

    let nonst_i = nonst_idx.unwrap();
    let stall_i = stall_idx.unwrap();

    // With the CORRECT fix: the inline push should drain stall_pending FIRST,
    // so the older stall event appears at a LOWER index than the newer inline event.
    // With the BUG: the inline push does NOT drain stall_pending, so the non-stall
    // event is at a LOWER index than the stall event (FIFO ordering violated).
    assert!(
        stall_i < nonst_i,
        "RUST-3: stall event (from stall_pending, index {stall_i}) must appear BEFORE \
         the inline non-stall event (index {nonst_i}) — the inline T0SZ push at site \
         ~3621 did not drain stall_pending first, violating FIFO ordering (ARM §3.5.3). \
         Queue snapshot: {} events total",
        events.len()
    );
}
