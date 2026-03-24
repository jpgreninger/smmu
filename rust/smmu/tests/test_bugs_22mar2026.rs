//! Failing tests (TDD step 1) for two confirmed ARM SMMU v3 Rust bugs.
//!
//! Each test is designed to FAIL with the current buggy code and PASS only
//! after the corresponding fix is applied.
//!
//! # Bug Descriptions
//!
//! ## RUST-1: CMD_SYNC CS=3 CONS.RD / VecDeque Desync After Error Restart
//!
//! Location: `smmu/mod.rs` — `process_command_queue()` around line 5033.
//!
//! When `process_command_queue()` encounters a CS=3 CMD_SYNC (→ CERROR_ILL):
//!   1. `queue.pop_front()` removes the erroneous command from the VecDeque.
//!   2. `process_single_command()` returns `Err(CERROR_ILL)`.
//!   3. `cmdq_cons` is NOT advanced (correct per ARM §7.1).
//!   4. GERROR.CMDQ_ERR is set.
//!
//! The desync: after (1) the VecDeque no longer contains the CS=3 CMD_SYNC,
//! but `cmdq_cons` still points at that slot.  When software clears GERROR
//! and calls `process_command_queue()` again, the queue processes whatever
//! comes NEXT in the VecDeque (e.g. a CMD_NOP), while `cmdq_cons` was still
//! frozen at the index of the erroneous command.  The erroneous command has
//! been silently consumed — it is gone from the VecDeque but `cmdq_cons`
//! will advance from its frozen position as if it processed the NOP from
//! the erroneous slot.
//!
//! ARM §7.1: "SMMU_CMDQ_CONS.RD remains pointing to the erroneous command
//! in the Command queue."  The re-fetch model assumes the erroneous command
//! is still present at CONS.RD after GERROR is cleared; the VecDeque model
//! violates this because the command was popped before the error was known.
//!
//! Observable symptom: after clearing GERROR and reprocessing a CMD_NOP that
//! was submitted AFTER the CS=3 CMD_SYNC, `cmdq_cons` advances from 0 → 1
//! (as though the NOP was at slot 0), even though PROD advanced to 2 for two
//! commands (slot 0 = CS=3 CMD_SYNC, slot 1 = NOP).  After full processing
//! `cmdq_cons` should equal PROD (2), but the desync leaves it at 1 — meaning
//! the final PROD==CONS empty condition is never reached and the NOP was
//! misattributed to slot 0.
//!
//! ## RUST-3: C_BAD_STE Inline Push Missing OVFLG Toggle on Overflow
//!
//! Location: `smmu/mod.rs` — `configure_stream()` lines 1951–1958,
//! 1990–1997, 2033–2040.
//!
//! Three inline event-push blocks in `configure_stream()` enqueue a
//! `C_BAD_STE` event when the queue has capacity, but silently drop the
//! event when the queue is full WITHOUT calling `toggle_ovflg_once()`.
//!
//! ARM §7.4 requires that EVENTQ_PROD.OVFLG (bit 31) is toggled whenever
//! a non-stall event record is discarded due to a full queue.
//!
//! Compare with `enqueue_event()` which correctly calls `toggle_ovflg_once()`
//! in its `else` branch.  The three inline blocks are missing that else branch.
//!
//! Affected paths:
//!   Block A: reserved STE.Config (`translation_enabled && !stage1 && !stage2`)
//!   Block B: STRW=EL3 on a NonSecure stream
//!   Block C: S2TTB address exceeds OAS (stage2_enabled + s2_ttb >= 2^oas_bits)
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, CommandEntry, CommandType, FaultMode, QueueConfig, SMMUConfig, SecurityState,
    StreamConfig, StreamID, StreamWorld, IOVA, PASID,
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

/// Build an SMMU with SMMUEN + CMDQEN + EVENTQEN enabled (no helper-method
/// wrapping so we can control CR0 bits explicitly).
fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

/// Build an SMMU with a minimal event queue (16 entries) and all queue bits set.
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

/// Fill the event queue to capacity by translating unmapped IOVAs through a
/// stage-1-only terminate-mode stream (each translation → F_TRANSLATION fault).
fn fill_event_queue_to_capacity(smmu: &SMMU, stream: StreamID, capacity: usize) {
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
// RUST-1: CMD_SYNC CS=3 VecDeque / CONS.RD Desync
// ============================================================================

/// RUST-1 — After a CERROR_ILL halt, the erroneous command has been silently
/// consumed from the internal VecDeque (via `pop_front()`).  When software
/// clears GERROR and reprocesses, `cmdq_cons` is advanced from the frozen
/// (erroneous) position rather than from the *next* position after the error.
///
/// Concretely: submit CS=3 CMD_SYNC (slot 0), then CMD_NOP (slot 1).
/// PROD = 2 after both submits.
///
/// Expected (ARM §7.1 re-fetch model):
///   - After CERROR_ILL: CONS = 0 (frozen at erroneous slot 0), queue halted.
///   - After clear + restart: queue re-processes from slot 0 (the CMD_SYNC is
///     still there). But since this is a VecDeque model, the CMD_SYNC is gone.
///   - The symptom is that after a clean restart CONS ends at 1, not 2, and
///     PROD != CONS (the queue never becomes empty).
///
/// BEFORE FIX: cmdq_cons advances from 0 → 1 after the NOP is processed on
/// restart, but PROD=2, so PROD != CONS — the queue can never drain correctly.
///
/// AFTER FIX: the erroneous command is NOT popped until it has been confirmed
/// to process without error, so the VecDeque still contains it at CONS=0 after
/// the halt; the re-fetch on restart returns CERROR_ILL again and CONS stays
/// at 0 (a no-op restart), or the fix preserves the command for re-processing.
///
/// This test asserts the invariant that after a CERROR_ILL + clear-GERROR +
/// re-process cycle, CONS is still frozen at 0 and PROD remains at 2 (the
/// SMMU never writes CMDQ_PROD — ARM §6.3.25).
///
/// Updated with BUG-NEW-6 fix: the erroneous `cmdq_prod.store(cons_at_err)`
/// that used to reset PROD to CONS on the first error has been removed.
/// CMDQ_PROD is a software-driven register; only the OS/driver writes it.
/// With BUG-NEW-6 fixed:
///   - PROD stays at 2 (set by software, never touched by the SMMU).
///   - CONS stays at 0 (frozen at erroneous slot per ARM §7.1).
///   - Re-process re-encounters the same bad CMD_SYNC → CERROR_ILL again.
///   - The queue can only advance when software submits a new command
///     (which triggers the CERROR_ILL pop in submit_command()).
#[test]
fn rust1_cmdq_cons_desync_after_cerror_ill_restart() {
    let smmu = make_smmu();

    // Verify PROD == CONS == 0 at reset.
    assert_eq!(smmu.cmdq_prod_index(), 0, "precondition: PROD=0");
    assert_eq!(smmu.cmdq_cons_index(), 0, "precondition: CONS=0");

    // Submit CMD_SYNC with CS=3 (Reserved → CERROR_ILL).
    let mut bad_sync = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_sync.cs = 3;
    smmu.submit_command(bad_sync).unwrap();

    // Submit a no-op PrefetchConfig after the bad CMD_SYNC (PrefetchConfig is
    // a documented no-op that always succeeds — see process_single_command).
    let nop = CommandEntry::new(CommandType::PrefetchConfig, 0, 0);
    smmu.submit_command(nop).unwrap();

    // PROD must be 2 (two commands submitted).
    assert_eq!(smmu.cmdq_prod_index(), 2, "precondition: PROD=2 after two submits");
    assert_eq!(smmu.cmdq_cons_index(), 0, "precondition: CONS=0 before processing");

    // Step 1: process — hits CERROR_ILL on slot 0, halts.
    let _ = smmu.process_command_queue();

    // GERROR.CMDQ_ERR must be active.
    let active_bits = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_ne!(active_bits, 0, "precondition: GERROR.CMDQ_ERR must be active");

    // CONS must be frozen at 0 (the erroneous slot).
    assert_eq!(smmu.cmdq_cons_index(), 0, "precondition: CONS frozen at 0 after CERROR_ILL");

    // BUG-NEW-6 fix: PROD must NOT be reset by process_command_queue().
    // CMDQ_PROD is software-driven; only the OS writes it (ARM §6.3.25).
    assert_eq!(
        smmu.cmdq_prod_index(),
        2,
        "BUG-NEW-6: PROD must remain at 2 after CERROR_ILL — the SMMU must not write CMDQ_PROD"
    );

    // Step 2: clear GERROR (software acknowledges the error).
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);
    let active_after_clear = (smmu.get_gerror() ^ smmu.get_gerrorn()) & SMMU::GERROR_CMDQ_ERR;
    assert_eq!(active_after_clear, 0, "precondition: GERROR.CMDQ_ERR cleared");

    // Step 3: process again — the bad CMD_SYNC is still at the front of the
    // VecDeque (peek-not-pop), so CERROR_ILL fires again; CONS stays at 0.
    let _ = smmu.process_command_queue();

    // After the re-process the SMMU is still stuck at slot 0 (the bad command).
    // CONS is still 0; PROD is still 2.  The queue has not made progress because
    // software has not overwritten the erroneous slot (ARM §7.1 re-fetch model).
    let cons_final = smmu.cmdq_cons_index();
    let prod_final = smmu.cmdq_prod_index();
    assert_eq!(
        cons_final, 0,
        "RUST-1: CONS must stay frozen at 0 after re-process of bad CMD_SYNC; \
         got CONS={cons_final}"
    );
    assert_eq!(
        prod_final, 2,
        "BUG-NEW-6: PROD must still be 2 after re-process (SMMU must not write CMDQ_PROD); \
         got PROD={prod_final}"
    );

    // Verify the queue still has 2 entries (bad_sync + nop both present).
    assert_eq!(
        smmu.get_command_queue_size(),
        2,
        "RUST-1: both commands must remain in the VecDeque after error + re-process"
    );
}

/// RUST-1 — Companion check: after a CERROR_ILL + restart, the CMD_NOP that
/// follows the erroneous command must be processed exactly ONCE.
///
/// With the desync bug, the NOP IS processed (it's at the front of the
/// VecDeque after the CS=3 CMD_SYNC was popped), but `process_command_queue`
/// returns 1 (processed=1) from the restart, which is incorrect — the NOP
/// was at slot 1, so the restart should have yielded 0 (the CMD_SYNC at
/// slot 0 triggers CERROR_ILL again) or the erroneous command must be re-
/// presented.
///
/// This test confirms the desync by checking the queue is empty after restart
/// via `get_command_queue_size()`.  After a correct restart the VecDeque must
/// still hold the CMD_SYNC (queue size = 2 or CERROR_ILL again), whereas
/// with the bug the VecDeque holds 0 entries after the NOP is processed.
#[test]
fn rust1_vecdeque_empty_after_desync_restart() {
    let smmu = make_smmu();

    let mut bad_sync = CommandEntry::new(CommandType::Sync, 0, 0);
    bad_sync.cs = 3;
    smmu.submit_command(bad_sync).unwrap();

    let nop = CommandEntry::new(CommandType::PrefetchConfig, 0, 0);
    smmu.submit_command(nop).unwrap();

    // First process: halts at CERROR_ILL.
    let _ = smmu.process_command_queue();

    // The VecDeque size BEFORE the restart:
    // - Bug: 1 (only the NOP remains — CMD_SYNC was popped)
    // - Fix: 2 (CMD_SYNC still present at slot 0)
    // We do not assert this directly; we use the restart result.

    // Clear GERROR.
    smmu.clear_gerror(SMMU::GERROR_CMDQ_ERR);

    // Restart processing.
    let processed = smmu.process_command_queue();

    // After the restart with the BUG:
    //   - VecDeque had 1 entry (NOP). NOP is processed → processed=Ok(1).
    //   - VecDeque is now empty → get_command_queue_size()=0.
    //
    // After the restart with the FIX:
    //   - VecDeque had 2 entries. CMD_SYNC at front triggers CERROR_ILL again
    //     → processed=Err(CERROR_ILL), VecDeque still has CMD_SYNC + NOP.
    //   OR the fix implements "put-back" so CMD_SYNC is re-presented → same.
    //
    // Assert: the queue must NOT be drained to zero after one restart because
    // the CMD_SYNC was never successfully processed.
    let queue_size_after_restart = smmu.get_command_queue_size();
    assert_ne!(
        queue_size_after_restart,
        0,
        "RUST-1 desync: command queue must NOT be empty after one restart — \
         the CS=3 CMD_SYNC at slot 0 was popped from VecDeque before the error \
         was known, so the NOP was silently processed in its place; \
         queue size = {queue_size_after_restart}, processed = {processed:?}"
    );
}

// ============================================================================
// RUST-3: C_BAD_STE Inline Push Missing OVFLG Toggle (3 paths)
// ============================================================================

// ── Shared helper: set up a stream that generates F_TRANSLATION events ───────

fn setup_fault_stream(smmu: &SMMU, stream: StreamID) {
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(stream, cfg).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();
    // No map_page — every translate produces F_TRANSLATION.
}

// ── Block A: reserved STE.Config ─────────────────────────────────────────────

/// RUST-3 / Block A — When the event queue is full and `configure_stream()` is
/// called with a reserved STE.Config (translation_enabled=true but neither
/// stage1 nor stage2 enabled), the inline C_BAD_STE push block drops the event
/// silently without calling `toggle_ovflg_once()`.
///
/// ARM §7.4: OVFLG must be toggled when any non-stall event is discarded due
/// to a full queue.
///
/// BEFORE FIX: OVFLG stays at 0 after the overflow → assertion fails.
/// AFTER FIX: OVFLG is toggled to 1.
#[test]
fn rust3_block_a_reserved_ste_config_ovflg_not_toggled_on_overflow() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Set up a fault-generating stream for filling the queue.
    setup_fault_stream(&smmu, sid(1));

    // Fill the event queue to capacity.
    fill_event_queue_to_capacity(&smmu, sid(1), q);

    // Precondition: OVFLG must be 0 (no overflow yet).
    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Trigger Block A: translation_enabled=true, stage1_enabled=false, stage2_enabled=false.
    // This is a reserved STE.Config — configure_stream must return Err and
    // attempt to enqueue C_BAD_STE.  The queue is full, so the event is
    // discarded.  OVFLG must be toggled.
    let bad_cfg = StreamConfig {
        translation_enabled: true,
        stage1_enabled: false,
        stage2_enabled: false,
        ..StreamConfig::default()
    };
    let result = smmu.configure_stream(sid(99), bad_cfg);
    assert!(result.is_err(), "precondition: reserved STE.Config must be rejected");

    // ARM §7.4 invariant: OVFLG must be 1 because the C_BAD_STE event was
    // discarded due to a full queue.
    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-3 / Block A: OVFLG must be toggled to 1 when C_BAD_STE is discarded \
         on a full queue (reserved STE.Config path) — inline push block missing \
         toggle_ovflg_once() in else branch"
    );
}

// ── Block B: STRW=EL3 + NonSecure stream ─────────────────────────────────────

/// RUST-3 / Block B — Same as Block A but for the STRW=EL3 NonSecure stream
/// validation path (second inline push block).
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX: OVFLG is toggled to 1.
#[test]
fn rust3_block_b_strw_el3_ns_ovflg_not_toggled_on_overflow() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Set up a fault-generating stream for filling the queue.
    setup_fault_stream(&smmu, sid(2));

    // Fill the event queue to capacity.
    fill_event_queue_to_capacity(&smmu, sid(2), q);

    // Precondition: OVFLG must be 0.
    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Trigger Block B: STRW=EL3 on a NonSecure stream — must be rejected.
    let mut bad_cfg = StreamConfig::stage1_only();
    bad_cfg.security_state = SecurityState::NonSecure;
    bad_cfg.strw = StreamWorld::El3;

    let result = smmu.configure_stream(sid(98), bad_cfg);
    assert!(result.is_err(), "precondition: STRW=EL3 on NS stream must be rejected");

    // ARM §7.4 invariant.
    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-3 / Block B: OVFLG must be toggled to 1 when C_BAD_STE is discarded \
         on a full queue (STRW=EL3 + NonSecure path) — inline push block missing \
         toggle_ovflg_once() in else branch"
    );
}

// ── Block C: S2TTB exceeds OAS ────────────────────────────────────────────────

/// RUST-3 / Block C — Same as Block A/B but for the S2TTB out-of-OAS-bounds
/// validation path (third inline push block).
///
/// BEFORE FIX: OVFLG stays at 0 → assertion fails.
/// AFTER FIX: OVFLG is toggled to 1.
#[test]
fn rust3_block_c_s2ttb_oas_overflow_ovflg_not_toggled() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_small_smmu();

    // Set up a fault-generating stream for filling the queue.
    setup_fault_stream(&smmu, sid(3));

    // Fill the event queue to capacity.
    fill_event_queue_to_capacity(&smmu, sid(3), q);

    // Precondition: OVFLG must be 0.
    assert_eq!(ovflg(&smmu), 0, "precondition: OVFLG must be 0 before overflow test");

    // Trigger Block C: stage2_enabled=true, s2_ps=0 (32-bit OAS), S2TTB >= 2^32.
    let mut bad_cfg = StreamConfig::two_stage();
    bad_cfg.s2_ps = 0;           // OAS = 32 bits, limit = 0x1_0000_0000
    bad_cfg.s2_ttb = 0x1_0000_0000; // exactly 2^32 — out of range

    let result = smmu.configure_stream(sid(97), bad_cfg);
    assert!(result.is_err(), "precondition: S2TTB exceeding OAS must be rejected");

    // ARM §7.4 invariant.
    assert_eq!(
        ovflg(&smmu),
        1,
        "RUST-3 / Block C: OVFLG must be toggled to 1 when C_BAD_STE is discarded \
         on a full queue (S2TTB out-of-OAS-bounds path) — inline push block missing \
         toggle_ovflg_once() in else branch"
    );
}
