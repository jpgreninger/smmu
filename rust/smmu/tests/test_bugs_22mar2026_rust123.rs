//! Failing tests (TDD step 1) for three confirmed bugs in the Rust SMMU
//! implementation.  Each test is designed to FAIL with the current buggy code
//! and PASS only after the corresponding fix is applied.
//!
//! # Bug Descriptions
//!
//! ## RUST-1: ATS ATSCHK rollback does not decrement `eventq_prod`
//!
//! Location: `smmu/mod.rs` lines ~3214–3227 (rollback path).
//!
//! When `CR0.ATSCHK=1` and an `AtsTranslated` transaction fails the re-check:
//!   1. The snapshot captures the VecDeque length (`snapshot_len`).
//!   2. The inner `translate()` enqueues an F_TRANSLATION event AND advances
//!      `eventq_prod` by one.
//!   3. The rollback pops the entry from the VecDeque and decrements
//!      `event_count` — but NEVER decrements `eventq_prod`.
//!   4. F_TRANSL_FORBIDDEN is then enqueued, advancing `eventq_prod` again.
//!
//! Result: `eventq_prod` ends up 1 higher than it should be.  After rollback
//! + F_TRANSL_FORBIDDEN, prod has advanced twice but the queue holds one entry.
//!
//! ## RUST-2: `eventq_prod` snapshot is never taken (TOCTOU)
//!
//! Same root as RUST-1: the rollback snapshots the VecDeque length but takes
//! NO snapshot of `eventq_prod`.  The inner `translate()` advances the atomic
//! `eventq_prod` independently, and the rollback never restores it.
//!
//! Observable symptom: `get_eventq_prod() - eventq_cons_index()` does not
//! equal `get_event_queue_size()` after an ATSCHK rollback.
//!
//! ## RUST-3: Inline F_TRANSL_FORBIDDEN enqueue never calls `toggle_ovflg_once()`
//!
//! Location: `smmu/mod.rs` lines ~3245–3255 (ATSCHK forbidden event inline path).
//!
//! `enqueue_event()` correctly calls `toggle_ovflg_once()` in its else branch.
//! The ATSCHK inline F_TRANSL_FORBIDDEN block uses a bare
//! `if queue.len() < capacity { push + advance prod }` with NO else branch.
//! When the queue is at capacity the event is silently discarded and
//! `toggle_ovflg_once()` is never called, violating ARM §7.4.
//!
//! Observable: after a pre-filled-queue ATSCHK failure, PROD has advanced once
//! more than the queue actually contains (RUST-1 leak) and the PROD/CONS
//! register difference is larger than `get_event_queue_size()`.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, EventEntry, EventType, QueueConfig, SMMUConfig, SecurityState, StreamConfig,
    StreamID, TransactionType, IOVA, PA, PASID,
};
use smmu::{PagePermissions, SMMU};

// ============================================================================
// Helpers
// ============================================================================

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

/// Build an SMMU with SMMUEN + EVENTQEN + ATSCHK enabled.
fn make_atschk_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_ATSCHK);
    smmu
}

/// Configure a stage-1-only ATS stream (eats=2) and map IOVA 0x1000.
/// IOVA 0x9000 is intentionally left unmapped so the ATSCHK recheck fails
/// when the caller uses that address.
fn setup_ats_stream(smmu: &SMMU, stream_id: StreamID) {
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.eats = 2; // full ATS support required for AtsTranslated
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    // IOVA 0x9000 intentionally unmapped — ATSCHK inner recheck will fail there.
}

/// Build an SMMU with a small event queue (4 slots), ATSCHK enabled.
fn make_atschk_smmu_small(queue_cap: usize) -> SMMU {
    let config = SMMUConfig::builder()
        .queue_config(
            QueueConfig::builder()
                .event_queue_size(queue_cap)
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    let smmu = SMMU::with_config(config);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_ATSCHK);
    smmu
}

// ============================================================================
// RUST-1: ATS ATSCHK rollback — eventq_prod advances by 2 instead of 1
// ============================================================================
//
// When CR0.ATSCHK=1, an AtsTranslated to an unmapped IOVA:
//   inner translate() → enqueues F_TRANSLATION, advances prod by 1.
//   rollback       → pops F_TRANSLATION from queue, BUT prod stays advanced.
//   F_TRANSL_FORBIDDEN enqueue → advances prod by 1 more.
//
// After the call: queue holds exactly 1 entry (F_TRANSL_FORBIDDEN),
//                 but eventq_prod has advanced TWICE.
//
// Expected correct state:  prod delta = 1 (one valid entry enqueued).
// Observed buggy state:    prod delta = 2 (inner advance leaked through rollback).

/// RUST-1: After ATSCHK rollback + F_TRANSL_FORBIDDEN, prod must advance by
/// exactly ONE step from its initial value.
///
/// BEFORE FIX: delta == 2 → assertion fails.
/// AFTER FIX:  delta == 1 → assertion passes.
#[test]
fn rust1_atschk_rollback_eventq_prod_advances_by_one_not_two() {
    let smmu = make_atschk_smmu();
    let stream_id = sid(0xAC_10);
    setup_ats_stream(&smmu, stream_id);

    let prod_before = smmu.get_eventq_prod() & 0x7FFF_FFFF;

    // AtsTranslated to unmapped IOVA → inner recheck fails → rollback → F_TRANSL_FORBIDDEN.
    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x9000), // unmapped
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "AtsTranslated to unmapped IOVA must fail");

    // Queue must hold exactly 1 event (F_TRANSL_FORBIDDEN only — inner rolled back).
    let queue_size = smmu.get_event_queue_size();
    assert_eq!(
        queue_size, 1,
        "After ATSCHK rollback, queue must hold exactly 1 event (F_TRANSL_FORBIDDEN). \
         got {queue_size}"
    );

    // PROD must have advanced by exactly 1 step from initial.
    let prod_after = smmu.get_eventq_prod() & 0x7FFF_FFFF;
    let prod_delta = prod_after.wrapping_sub(prod_before);

    // BUG-RUST-1 assertion: delta must be 1, not 2.
    assert_eq!(
        prod_delta,
        1,
        "RUST-1: eventq_prod must advance by exactly 1 after ATSCHK rollback + \
         F_TRANSL_FORBIDDEN. prod_before={prod_before} prod_after={prod_after} \
         delta={prod_delta}. \
         BUG: inner translate() advances prod but rollback never decrements it."
    );
}

/// RUST-2: After an ATSCHK rollback, PROD - CONS must equal the number of
/// entries physically in the queue.
///
/// BEFORE FIX: PROD-CONS == 2, queue_entries == 1 → assertion fails.
/// AFTER FIX:  PROD-CONS == 1, queue_entries == 1 → assertion passes.
#[test]
fn rust2_atschk_rollback_prod_minus_cons_equals_queue_entries() {
    let smmu = make_atschk_smmu();
    let stream_id = sid(0xAC_11);
    setup_ats_stream(&smmu, stream_id);

    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x9000), // unmapped
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "AtsTranslated to unmapped IOVA must fail");

    let queue_entries = smmu.get_event_queue_size();
    let prod_after   = smmu.get_eventq_prod()     & 0x7FFF_FFFF;
    let cons_after   = smmu.eventq_cons_index()   & 0x7FFF_FFFF;
    let prod_cons_diff = prod_after.wrapping_sub(cons_after);

    // BUG-RUST-2 assertion: PROD - CONS must equal the number of valid entries.
    assert_eq!(
        u64::from(prod_cons_diff),
        queue_entries,
        "RUST-2: PROD - CONS must equal the number of valid entries in the queue. \
         prod_after={prod_after}, cons_after={cons_after}, diff={prod_cons_diff}, \
         queue_entries={queue_entries}. \
         BUG: ATSCHK rollback never decrements prod, leaving it 1 ahead of reality."
    );
}

/// RUST-1 regression guard: successful AtsTranslated with ATSCHK=1 must not
/// advance prod (no event emitted).  Passes before and after the fix.
#[test]
fn rust1_atschk1_successful_translated_no_advance() {
    let smmu = make_atschk_smmu();
    let stream_id = sid(0xAC_13);
    setup_ats_stream(&smmu, stream_id);

    let prod_before = smmu.get_eventq_prod() & 0x7FFF_FFFF;

    // IOVA 0x1000 IS mapped — inner recheck succeeds → no event, no prod advance.
    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x1000), // mapped
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_ok(), "AtsTranslated to mapped IOVA must succeed");

    let prod_after = smmu.get_eventq_prod() & 0x7FFF_FFFF;
    let delta = prod_after.wrapping_sub(prod_before);
    assert_eq!(
        delta, 0,
        "Regression guard: successful AtsTranslated must emit no event. delta={delta}"
    );
}

/// RUST-1 regression guard: AtsTranslated with ATSCHK=0 advances prod by
/// exactly 1 (one F_TRANSLATION for the unmapped IOVA).  Passes before and
/// after the fix.
#[test]
fn rust1_atschk0_translated_advances_by_one() {
    // Build SMMU WITHOUT ATSCHK bit.
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN); // no ATSCHK
    let stream_id = sid(0xAC_14);
    let mut cfg = StreamConfig::stage1_only();
    cfg.t0sz = 0;
    cfg.eats = 2;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    // IOVA 0x9000 unmapped.

    let prod_before = smmu.get_eventq_prod() & 0x7FFF_FFFF;

    let result = smmu.translate_with_type(
        stream_id,
        pasid(0),
        iova(0x9000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "Unmapped AtsTranslated must fail");

    let prod_after = smmu.get_eventq_prod() & 0x7FFF_FFFF;
    let delta = prod_after.wrapping_sub(prod_before);
    assert_eq!(
        delta, 1,
        "ATSCHK=0: AtsTranslated must advance prod by exactly 1 (F_TRANSLATION). \
         delta={delta}"
    );
}

// ============================================================================
// RUST-3: PROD/CONS consistency after ATSCHK failure — inline forbidden path
// ============================================================================
//
// The inline F_TRANSL_FORBIDDEN enqueue block (lines ~3245–3255) has no else
// branch calling `toggle_ovflg_once()` when the queue is full.  Combined with
// the RUST-1 prod desync, the PROD/CONS consistency invariant is violated.
//
// Scenario (4-slot queue, pre-fill 3 entries):
//   prod_before = 3 (three enqueues via submit_event).
//   ATSCHK failure on unmapped IOVA:
//     inner translate() → queue 3/4, 1 free → enqueues F_TRANSLATION → prod=4.
//     rollback → pop 1 entry → queue=3. event_count decremented.
//                              prod NOT decremented (RUST-1 bug).
//     F_TRANSL_FORBIDDEN → queue 3/4, 1 free → enqueues → prod=5.
//   Final state: queue holds 4 entries, prod=5.
//   prod - cons = 5 - 0 = 5 ≠ 4 (queue_entries).
//
// Expected correct state after fix: prod=4, prod-cons=4=queue_entries.

/// RUST-1+RUST-3: PROD-CONS must equal queue_size after an ATSCHK failure
/// against a (capacity-1)-full queue.
///
/// BEFORE FIX: prod-cons = capacity+1, queue_size = capacity → fails.
/// AFTER FIX:  prod-cons = capacity,   queue_size = capacity → passes.
#[test]
fn rust3_full_queue_atschk_failure_prod_must_not_advance() {
    // 4-slot queue.
    let smmu = make_atschk_smmu_small(4);

    // Register ATS stream.
    let ats_sid = sid(0xAC_40);
    let mut ats_cfg = StreamConfig::stage1_only();
    ats_cfg.t0sz = 0;
    ats_cfg.eats = 2;
    smmu.configure_stream(ats_sid, ats_cfg).unwrap();
    smmu.create_pasid(ats_sid, pasid(0)).unwrap();
    // IOVA 0x9000 unmapped.

    // Pre-fill 3 of 4 slots.
    for i in 0u32..3 {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: 0xFF_10 + i,
            ..EventEntry::zeroed()
        };
        smmu.submit_event(ev).unwrap();
    }

    let prod_before_atschk = smmu.get_eventq_prod() & 0x7FFF_FFFF;
    let queue_before = smmu.get_event_queue_size();
    assert_eq!(queue_before, 3, "Queue must hold 3 entries before ATSCHK test");

    // ATSCHK failure.  Expected result:
    //   inner translate() fills slot 4, advances prod (+1 → 4).
    //   rollback pops slot 4, queue=3 again.  prod NOT decremented (bug).
    //   F_TRANSL_FORBIDDEN fills slot 4, advances prod (+1 → 5).
    //   Final: queue=4, prod=5.  prod delta from prod_before = 2.
    let result = smmu.translate_with_type(
        ats_sid,
        pasid(0),
        iova(0x9000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "ATSCHK failure must return Err");

    let queue_after = smmu.get_event_queue_size();
    let prod_after  = smmu.get_eventq_prod() & 0x7FFF_FFFF;
    let cons_after  = smmu.eventq_cons_index()  & 0x7FFF_FFFF;
    let prod_delta  = prod_after.wrapping_sub(prod_before_atschk);

    // Queue must hold exactly 4 entries (3 pre-filled + 1 F_TRANSL_FORBIDDEN).
    assert_eq!(
        queue_after, 4,
        "After ATSCHK failure: queue must hold 4 entries (3 original + 1 F_TRANSL_FORBIDDEN)"
    );

    // PROD must advance by exactly 1 (only for F_TRANSL_FORBIDDEN; rollback
    // must undo the inner translate's advance).
    //
    // BEFORE FIX: prod_delta == 2 → assertion fails.
    // AFTER FIX:  prod_delta == 1 → assertion passes.
    assert_eq!(
        prod_delta,
        1,
        "RUST-3/RUST-1: prod must advance by exactly 1 after ATSCHK rollback + \
         F_TRANSL_FORBIDDEN. Got delta={prod_delta}. \
         prod_before_atschk={prod_before_atschk} prod_after={prod_after} \
         cons_after={cons_after}. \
         BUG: inner translate() advances prod (+1) but rollback never decrements it; \
         forbidden then advances prod (+1 more) → total delta=2."
    );
}

/// RUST-1+RUST-3 combined: PROD-CONS must equal queue_size after an ATSCHK
/// failure on a (capacity-1)-full queue.
///
/// BEFORE FIX: PROD-CONS = capacity+1, queue_size = capacity → fails.
/// AFTER FIX:  PROD-CONS = capacity,   queue_size = capacity → passes.
#[test]
fn rust1_rust3_prod_cons_consistency_after_atschk_failure() {
    // 4-slot queue.
    let smmu = make_atschk_smmu_small(4);

    // Register ATS stream.
    let ats_sid = sid(0xAC_50);
    let mut ats_cfg = StreamConfig::stage1_only();
    ats_cfg.t0sz = 0;
    ats_cfg.eats = 2;
    smmu.configure_stream(ats_sid, ats_cfg).unwrap();
    smmu.create_pasid(ats_sid, pasid(0)).unwrap();
    // IOVA 0x9000 unmapped.

    // Pre-fill 3 of 4 slots.
    for i in 0u32..3 {
        let ev = EventEntry {
            event_type: EventType::FTranslation,
            stream_id: 0xFF_20 + i,
            ..EventEntry::zeroed()
        };
        smmu.submit_event(ev).unwrap();
    }

    let cons_before = smmu.eventq_cons_index() & 0x7FFF_FFFF;

    let result = smmu.translate_with_type(
        ats_sid,
        pasid(0),
        iova(0x9000),
        AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated,
    );
    assert!(result.is_err(), "ATSCHK failure must return Err");

    let queue_after = smmu.get_event_queue_size();
    let prod_after  = smmu.get_eventq_prod() & 0x7FFF_FFFF;
    let cons_after  = smmu.eventq_cons_index()  & 0x7FFF_FFFF;

    // CONS must not have moved.
    assert_eq!(
        cons_before, cons_after,
        "CONS must not advance during ATSCHK failure. \
         cons_before={cons_before}, cons_after={cons_after}"
    );

    let prod_cons_diff = prod_after.wrapping_sub(cons_after);

    // BEFORE FIX: queue_after=4, prod_after=5, diff=5 ≠ 4 → FAILS.
    // AFTER FIX:  queue_after=4, prod_after=4, diff=4 == 4 → PASSES.
    assert_eq!(
        u64::from(prod_cons_diff),
        queue_after,
        "RUST-1+RUST-3: PROD-CONS must equal the number of entries in the queue. \
         prod_after={prod_after}, cons_after={cons_after}, diff={prod_cons_diff}, \
         queue_after={queue_after}. \
         BUG: ATSCHK rollback advances prod for the inner event but never decrements it."
    );
}
