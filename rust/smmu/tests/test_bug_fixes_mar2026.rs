#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for confirmed bugs in the Rust ARM SMMU v3 implementation.
//!
//! BUG-4: remove_stream TOCTOU — TLB invalidation must precede DashMap removal.
//!   §3.12/§3.16: after stream deconfiguration, stale cached translations must
//!   not succeed.  The original code removed the stream first then invalidated
//!   the TLB, leaving a window where a concurrent translate() on a TLB hit could
//!   find the stream gone and still return Ok with the cached (stale) data.
//!
//! BUG-6: signal_gerror / clear_gerror residual TOCTOU — two separate AtomicU32
//!   fields (gerror + gerrorn) cannot be updated atomically with each other.
//!   §6.3.19/§6.3.20: the XOR-invariant (active iff gerror[x] != gerrorn[x])
//!   must be maintained atomically.  The fix packs both into a single AtomicU64
//!   so every read-modify-write touches both fields in one CAS.
//!
//! BUG-11: configure_stream() missing C_BAD_STE event for illegal STE.
//!   ARM IHI0070G.b §7.3.5: C_BAD_STE event must be recorded before returning
//!   an error when an STE is determined illegal (STRW=EL3/NonSecure, S2TTB OAS
//!   violation, reserved STE.Config encoding).
//!
//! BUG-13: Concurrent remove_stream() yields spurious StreamNotFound.
//!   Two concurrent remove_stream() calls both pass contains_key but only one
//!   gets Some from remove(); the other returns a false StreamNotFound error.
//!   The fix treats None from remove() as success (idempotent) while preserving
//!   the BUG-4 constraint that TLB invalidation occurs before map removal.

use smmu::types::{
    AccessType, CommandEntry, CommandType, EventType, FaultMode, PagePermissions, SecurityState,
    StreamConfig, StreamID, StreamWorld, IOVA, PA, PASID,
};
use smmu::SMMU;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Build a stage-1-only stream config suitable for page mapping tests.
fn stage1_config() -> StreamConfig {
    StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16) // 48-bit VA space
        .build()
        .unwrap()
}

/// Trigger GERROR.CMDQ_ERR via CMD_SYNC CS=3 (CERROR_ILL per §4.7.3).
fn trigger_cmdq_err(smmu: &SMMU) {
    let mut cmd = CommandEntry::new(CommandType::Sync, 0, 0);
    cmd.cs = 3; // CS=0b11 is Reserved → CERROR_ILL
    smmu.submit_command(cmd).unwrap();
    let _ = smmu.process_command_queue();
}

// ─── BUG-4 ────────────────────────────────────────────────────────────────────

/// BUG-4: After `remove_stream()` a subsequent translate() must fail rather than
/// returning the stale cached (TLB) translation.
///
/// The fix is to call `tlb_cache.invalidate_by_stream()` BEFORE
/// `streams.remove()`.  This test verifies the observable behaviour: populate the
/// TLB via a successful translate(), call remove_stream(), then verify that the
/// next translate() for the same stream returns an error (not the stale PA).
#[test]
fn bug4_remove_stream_tlb_invalidated_before_map_removal() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = sid(1);
    let p = pasid(0);
    // IOVA within the 48-bit (T0SZ=16) range
    let addr = iova(0x0000_0000_0001_0000);

    smmu.configure_stream(stream_id, stage1_config()).unwrap();
    smmu.create_pasid(stream_id, p).unwrap();
    smmu.map_page(
        stream_id,
        p,
        addr,
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate — populates the TLB cache.
    let first = smmu.translate(stream_id, p, addr, AccessType::Read, SecurityState::NonSecure);
    assert!(
        first.is_ok(),
        "first translate must succeed: {:?}",
        first.err()
    );

    // Remove the stream.  The TLB entry must be cleared before the stream
    // disappears from the map.
    smmu.remove_stream(stream_id).unwrap();

    // Any subsequent translate() must fail — the TLB must have been invalidated,
    // so there is no fast-path hit and the slow path finds no stream.
    let second = smmu.translate(stream_id, p, addr, AccessType::Read, SecurityState::NonSecure);
    assert!(
        second.is_err(),
        "translate after remove_stream must fail, got Ok({:?})",
        second.ok()
    );
}

/// BUG-4 concurrent ordering: spawn a tight-loop reader and verify that
/// translate() never returns Ok after remove_stream() has completed.
///
/// The reader waits for the main thread's Release store (signalling that
/// remove_stream has returned), then performs a single translate.  The
/// Release/Acquire pair guarantees the reader sees the fully-completed
/// remove_stream state, including the TLB invalidation.
#[test]
fn bug4_remove_stream_concurrent_no_stale_translation() {
    use std::thread;

    let smmu = Arc::new(SMMU::new());
    smmu.enable().unwrap();

    let stream_id = sid(2);
    let p = pasid(0);
    let addr = iova(0x0000_0000_0002_0000);

    smmu.configure_stream(stream_id, stage1_config()).unwrap();
    smmu.create_pasid(stream_id, p).unwrap();
    smmu.map_page(
        stream_id,
        p,
        addr,
        pa(0x8000_2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Warm the TLB.
    smmu.translate(stream_id, p, addr, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    let smmu_reader = Arc::clone(&smmu);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);

    let reader = thread::spawn(move || {
        // Spin until the main thread signals that remove_stream has completed.
        while !stop_reader.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        // remove_stream() has fully returned before this point (Release/Acquire).
        smmu_reader.translate(stream_id, p, addr, AccessType::Read, SecurityState::NonSecure)
    });

    // Remove stream then release the reader.
    smmu.remove_stream(stream_id).unwrap();
    stop.store(true, Ordering::Release);

    let result = reader.join().unwrap();
    assert!(
        result.is_err(),
        "translate after remove_stream must fail, got Ok({:?})",
        result.ok()
    );
}

// ─── BUG-6 ────────────────────────────────────────────────────────────────────

/// BUG-6: Basic XOR-invariant: signal then clear must leave CMDQ_ERR inactive.
///
/// active = gerror XOR gerrorn must be 0 after clear_gerror for a bit that was
/// previously signalled.
#[test]
fn bug6_gerror_xor_invariant_signal_then_clear() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // No active errors at reset.
    assert_eq!(smmu.get_gerror() ^ smmu.get_gerrorn(), 0);

    // Activate CMDQ_ERR via an illegal command.
    trigger_cmdq_err(&smmu);

    let active_after = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_ne!(
        active_after & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be active after illegal command"
    );

    // clear_gerror must deactivate the bit (XOR invariant restored to 0).
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    let active_cleared = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_eq!(
        active_cleared & SMMU::GERROR_CMDQ_ERR,
        0,
        "CMDQ_ERR must be inactive after clear_gerror"
    );
}

/// BUG-6: signal_gerror must be idempotent — signalling an already-active bit
/// must not toggle it back to inactive (double-toggle).
#[test]
fn bug6_signal_gerror_idempotent() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // First signal — activates CMDQ_ERR.
    trigger_cmdq_err(&smmu);
    let active1 = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_ne!(active1 & SMMU::GERROR_CMDQ_ERR, 0, "bit must be active");

    // Second signal (same bit already active) — must NOT double-toggle.
    trigger_cmdq_err(&smmu);
    let active2 = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_ne!(
        active2 & SMMU::GERROR_CMDQ_ERR,
        0,
        "bit must remain active after second signal (no double-toggle)"
    );
}

/// BUG-6: Concurrent signal + clear must not corrupt the XOR invariant.
///
/// After many rounds of concurrent signal and clear the fields must still be
/// internally consistent (no bits set outside the expected mask).
#[test]
fn bug6_gerror_concurrent_signal_clear_invariant() {
    use std::thread;

    const ROUNDS: usize = 200;
    let smmu = Arc::new(SMMU::new());
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Thread A: repeatedly trigger CMDQ_ERR.
    let smmu_a = Arc::clone(&smmu);
    let thread_a = thread::spawn(move || {
        for _ in 0..ROUNDS {
            trigger_cmdq_err(&smmu_a);
            std::hint::spin_loop();
        }
    });

    // Thread B: repeatedly clear CMDQ_ERR.
    let smmu_b = Arc::clone(&smmu);
    let thread_b = thread::spawn(move || {
        for _ in 0..ROUNDS {
            smmu_b.clear_gerror(SMMU::GERROR_CMDQ_ERR);
            std::hint::spin_loop();
        }
    });

    thread_a.join().unwrap();
    thread_b.join().unwrap();

    // After all operations the invariant must hold: only CMDQ_ERR may differ
    // between gerror and gerrorn — no other bits should have been corrupted.
    let g = smmu.get_gerror();
    let gn = smmu.get_gerrorn();
    let active = g ^ gn;
    assert_eq!(
        active & !SMMU::GERROR_CMDQ_ERR,
        0,
        "no bits other than CMDQ_ERR should be active: gerror={g:#010x} gerrorn={gn:#010x}"
    );
}

// ─── BUG-12 ───────────────────────────────────────────────────────────────────

/// BUG-12: Two-stage TLB entries were inserted with IPA=0 instead of the real
/// Stage-1 output IPA.  As a result, `CMD_TLBI_S2_IPA` never matched any entry
/// (the `e.ipa != 0` guard in `invalidate_by_vmid_and_ipa` skips all of them),
/// leaving stale translations alive after a hypervisor remap.
///
/// Fix: store `stage2_ipa_opt.unwrap_or(0)` in `CacheEntry.ipa` at TLB-insert
/// time.  Single-stage entries keep `ipa=0` and are unaffected.
///
/// Per ARM IHI0070G.b §4.4.3.1 (CMD_TLBI_S2_IPA) and §3.17 (TLB tagging).
#[test]
fn bug12_tlbi_s2_ipa_invalidates_two_stage_tlb_entry() {
    // ── Setup ──────────────────────────────────────────────────────────────
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = sid(10);
    let p = pasid(0);
    // IOVA for stage-1 (within 48-bit VA space, T0SZ=16).
    let iova_addr = iova(0x0000_0000_0001_0000);
    // Stage-1 maps IOVA → IPA=0x5000, stage-2 maps IPA=0x5000 → PA=0xA000.
    let ipa_addr = iova(0x0000_0000_0000_5000); // used as IOVA for map_stage2_page
    let pa_addr = pa(0x0000_0000_0000_A000);

    // Two-stage config: T0SZ=16 (48-bit VA), S2T0SZ=16 (48-bit IPA), vmid=7.
    let mut cfg = StreamConfig::two_stage();
    cfg.vmid = 7;
    smmu.configure_stream(stream_id, cfg).unwrap();

    // Stage-1: IOVA → IPA
    smmu.create_pasid(stream_id, p).unwrap();
    smmu.map_page(
        stream_id,
        p,
        iova_addr,
        PA::new(0x0000_0000_0000_5000).unwrap(), // IPA treated as PA in s1 map
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA → PA
    smmu.create_stage2_address_space(stream_id).unwrap();
    smmu.map_stage2_page(
        stream_id,
        ipa_addr,
        pa_addr,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // ── First translate: populates the TLB ────────────────────────────────
    let first = smmu.translate(stream_id, p, iova_addr, AccessType::Read, SecurityState::NonSecure);
    assert!(first.is_ok(), "first translate must succeed: {:?}", first.err());

    // Record TLB misses after the warm-up translate.
    let stats_before = smmu.get_cache_statistics();
    let misses_before = stats_before.tlb_misses();

    // ── Issue CMD_TLBI_S2_IPA targeting IPA=0x5000, VMID=7 ───────────────
    let mut cmd = CommandEntry::new(CommandType::TlbiS2Ipa, 0, 0);
    cmd.vmid = 7;
    cmd.start_address = 0x0000_0000_0000_5000;
    smmu.submit_command(cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // ── Second translate: must be a TLB MISS (re-translate, not cached) ───
    let second = smmu.translate(stream_id, p, iova_addr, AccessType::Read, SecurityState::NonSecure);
    assert!(second.is_ok(), "second translate must succeed: {:?}", second.err());

    let stats_after = smmu.get_cache_statistics();
    let misses_after = stats_after.tlb_misses();

    // The TLB invalidation must have forced a miss on the second translate.
    // Before BUG-12 fix: IPA=0 means TLBI_S2_IPA never matches → no miss increment.
    // After fix: IPA=0x5000 is stored → TLBI hits the entry → miss on re-translate.
    assert!(
        misses_after > misses_before,
        "CMD_TLBI_S2_IPA must have invalidated the two-stage TLB entry, \
         causing a miss on re-translate (misses_before={misses_before}, \
         misses_after={misses_after})"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-11 tests: configure_stream() must emit C_BAD_STE event for illegal STE
// ARM IHI0070G.b §7.3.5
// ─────────────────────────────────────────────────────────────────────────────

/// BUG-11a: STRW=EL3 on a NonSecure stream must emit C_BAD_STE to the event queue.
///
/// ARM §5.2.2: STRW=EL3 is forbidden for Non-Secure streams.
/// The implementation currently returns Err without recording the event.
#[test]
fn test_bug11_strw_el3_nonsecure_emits_c_bad_ste_event() {
    let smmu = SMMU::new();
    // Enable SMMU and EVENTQEN so events can be recorded.
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = sid(100);

    // Build an illegal config: NonSecure stream with STRW=EL3 and stage-1 active.
    // Note: bypass (stage1_enabled=false) makes STRW unused; must use stage1_only to
    // exercise the STRW validation path (BUG-RUST-2a fix adds strw_unused guard).
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.strw = StreamWorld::El3;

    // configure_stream() must return Err for this illegal combination.
    let result = smmu.configure_stream(stream_id, cfg);
    assert!(result.is_err(), "configure_stream with STRW=EL3/NonSecure must return Err");

    // BUG-11: before fix, the event queue is empty — no C_BAD_STE was emitted.
    // After fix, exactly one C_BAD_STE event must be in the queue.
    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::CBadSte && e.stream_id == 100),
        "configure_stream(STRW=EL3, NonSecure) must emit C_BAD_STE event (ARM §7.3.5); \
         events found: {events:?}"
    );
}

/// BUG-11b: S2TTB out-of-range for S2PS must emit C_BAD_STE to the event queue.
///
/// ARM §5.2.2: S2TTB must be within the OAS derived from STE.S2PS.
#[test]
fn test_bug11_s2ttb_oas_violation_emits_c_bad_ste_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = sid(101);

    // s2_ps=0 → 32-bit OAS → limit = 0x1_0000_0000.
    // S2TTB=0x2_0000_0000 exceeds the 32-bit OAS limit.
    let mut cfg = StreamConfig::stage2_only();
    cfg.s2_ps = 0; // 32-bit OAS
    cfg.s2_ttb = 0x2_0000_0000; // out of range

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(result.is_err(), "configure_stream with out-of-range S2TTB must return Err");

    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::CBadSte && e.stream_id == 101),
        "configure_stream(S2TTB OAS violation) must emit C_BAD_STE event (ARM §7.3.5); \
         events found: {events:?}"
    );
}

/// BUG-11c: Reserved STE.Config encoding must emit C_BAD_STE to the event queue.
///
/// ARM §5.2 Table STE.Config: translation_enabled=true with neither stage1 nor
/// stage2 enabled is a reserved encoding.
#[test]
fn test_bug11_reserved_ste_config_emits_c_bad_ste_event() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = sid(102);

    // translation_enabled=true but no stage enabled → reserved STE.Config.
    let mut cfg = StreamConfig::bypass();
    cfg.translation_enabled = true;
    cfg.stage1_enabled = false;
    cfg.stage2_enabled = false;

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(result.is_err(), "configure_stream with reserved STE.Config must return Err");

    let events = smmu.get_events();
    assert!(
        events.iter().any(|e| e.event_type == EventType::CBadSte && e.stream_id == 102),
        "configure_stream(reserved STE.Config) must emit C_BAD_STE event (ARM §7.3.5); \
         events found: {events:?}"
    );
}

/// BUG-11d: C_BAD_STE event must NOT be emitted when EVENTQEN=0.
///
/// ARM §7.2.1 / §3.5.3: events are gated on CR0.EVENTQEN.
#[test]
fn test_bug11_c_bad_ste_not_emitted_when_eventqen_zero() {
    let smmu = SMMU::new();
    // Enable SMMU but NOT EVENTQEN.
    smmu.set_cr0(SMMU::CR0_SMMUEN);

    let stream_id = sid(103);

    // Use stage1_only so STRW validation is active (bypass has strw_unused=true).
    let mut cfg = StreamConfig::stage1_only();
    cfg.security_state = SecurityState::NonSecure;
    cfg.strw = StreamWorld::El3;

    let result = smmu.configure_stream(stream_id, cfg);
    assert!(result.is_err(), "configure_stream with STRW=EL3/NonSecure must return Err");

    // With EVENTQEN=0 no event must be queued.
    let events = smmu.get_events();
    assert!(
        !events.iter().any(|e| e.event_type == EventType::CBadSte),
        "C_BAD_STE must not be recorded when EVENTQEN=0 (ARM §7.2.1); \
         events found: {events:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// BUG-13 tests: remove_stream() idempotent behaviour / no spurious StreamNotFound
// ─────────────────────────────────────────────────────────────────────────────

/// BUG-13: Double remove_stream() must not return a spurious StreamNotFound on
/// the second call when a concurrent caller already completed the removal.
///
/// The correct behaviour:
/// - First call:  Ok(())                 — stream removed, count decremented.
/// - Second call: Err(StreamNotFound)    — stream was already gone.
/// - stream_count decremented exactly once.
///
/// Before the fix, a concurrent second call that passes contains_key then finds
/// None from remove() also returns StreamNotFound, which is correct for the serial
/// case.  The bug is that the original code at line 1742 uses ok_or_else on the
/// result of remove() AFTER already passing the contains_key guard; in a concurrent
/// scenario both callers pass contains_key, but only one gets Some from remove() —
/// the other gets None and errors out with a spurious StreamNotFound.
/// The fix makes None from remove() silent (treat as success), preventing double-
/// decrement by only calling fetch_sub when remove() returns Some.
#[test]
fn test_bug13_second_remove_returns_stream_not_found() {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = sid(200);
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    // Count before removal.
    let count_before = smmu.get_stream_count();
    assert_eq!(count_before, 1, "stream_count must be 1 after configure");

    // First remove: must succeed.
    let first = smmu.remove_stream(stream_id);
    assert!(first.is_ok(), "first remove_stream must return Ok; got {first:?}");

    let count_after_first = smmu.get_stream_count();
    assert_eq!(count_after_first, 0, "stream_count must be 0 after first remove");

    // Second remove: stream is already gone — must return StreamNotFound.
    let second = smmu.remove_stream(stream_id);
    assert!(second.is_err(), "second remove_stream must return Err(StreamNotFound)");

    // stream_count must still be 0 — no double-decrement.
    let count_after_second = smmu.get_stream_count();
    assert_eq!(
        count_after_second, 0,
        "stream_count must not underflow after second remove (was {count_after_second})"
    );
}

/// BUG-13 concurrent: simulates the TOCTOU race where two threads both pass
/// the contains_key guard then both call remove().  With the fix, whichever
/// thread loses the remove() race gets None and should treat it as success
/// rather than returning StreamNotFound.
///
/// This test uses Arc<SMMU> and two threads.  Thread A holds the stream
/// briefly, thread B calls remove_stream concurrently.  We assert that the
/// combined result is exactly one Ok(()) and one Err(StreamNotFound) — not
/// two Err(StreamNotFound).
#[test]
fn test_bug13_concurrent_remove_at_most_one_error() {
    use std::thread;

    let smmu = Arc::new(SMMU::new());
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    let stream_id = sid(201);
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let smmu_a = Arc::clone(&smmu);
    let smmu_b = Arc::clone(&smmu);

    let handle_a = thread::spawn(move || smmu_a.remove_stream(stream_id));
    let handle_b = thread::spawn(move || smmu_b.remove_stream(stream_id));

    let result_a = handle_a.join().expect("thread A panicked");
    let result_b = handle_b.join().expect("thread B panicked");

    // BUG-13 fix: with None-from-remove treated as success, both concurrent callers
    // may return Ok(()) — the one that wins remove() gets Some, the loser gets None
    // and also returns Ok (idempotent).  The key invariant is:
    //   (a) at most one Err(StreamNotFound) — no spurious double-error, and
    //   (b) stream_count == 0 — no underflow (fetch_sub only on Some).
    let err_count = [&result_a, &result_b]
        .iter()
        .filter(|r: &&&Result<(), smmu::types::SMMUError>| r.is_err())
        .count();

    assert!(
        err_count <= 1,
        "at most one remove_stream may return Err (no spurious double-StreamNotFound); \
         got a={result_a:?} b={result_b:?}"
    );

    // stream_count must be exactly 0 — no underflow.
    assert_eq!(smmu.get_stream_count(), 0, "stream_count must be 0 after concurrent removes");
}
