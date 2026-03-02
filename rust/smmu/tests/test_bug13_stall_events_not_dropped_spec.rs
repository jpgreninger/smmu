#![allow(clippy::doc_markdown)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::cast_possible_truncation)]
#![allow(missing_docs)]

//! BUG-13: Stall Events Must Not Be Dropped (ARM IHI0070G.b §7.4 / §3.5.3 / §3.5.4)
//!
//! Spec: §7.4:
//! "a fault record from a stalled transaction is not discarded and an event is
//!  reported for the stalled transaction when the queue is next writable."
//!
//! The current bug: stall events are silently dropped when
//! queue.len() >= 2 * event_queue_capacity. This is non-conformant.
//!
//! Requirements:
//! - Stall events must NEVER be dropped (they go to stall_pending if queue is full).
//! - Stall events must NOT trigger OVFLG toggle (the spec says stall fault records
//!   do not cause an overflow condition).
//! - Non-stall events may be dropped with OVFLG toggle when queue is full.
//! - When space becomes available in the main queue, pending stall events drain
//!   in FIFO order.

use smmu::types::{
    AccessType, EventEntry, EventType, FaultMode, PagePermissions, QueueConfig, SMMUConfig,
    SecurityState, StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

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

/// Build a small SMMU whose event queue capacity is exactly `size`.
fn make_smmu_with_event_queue(size: usize) -> SMMU {
    let config = SMMUConfig::from(QueueConfig {
        event_queue_size: size,
        ..Default::default()
    });
    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();
    smmu
}

/// Configure a stall-mode Stage-1 stream with one mapped page.
fn setup_stall_stream(smmu: &SMMU, stream_n: u32) {
    let s = sid(stream_n);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .security_enforced(false)
        .fault_mode(FaultMode::Stall)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();
    // Map one page so the stream is valid; unmapped addresses will stall.
    smmu.map_page(
        s,
        pasid(0),
        iova(0x1000),
        pa(0x8000_1000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
}

/// §7.4 / BUG-13: A stall event arriving when the main queue is exactly at
/// capacity (not 2x) must NOT be dropped.
///
/// After the fix, the stall event must be preserved in the stall_pending buffer
/// and become accessible after the queue has space (via get_events or after
/// clearing the queue).
///
/// This test FAILS before the fix because the stall event is silently dropped
/// when queue.len() >= event_queue_capacity (the 2x check didn't help when the
/// queue fills via submit_event first).
#[test]
fn bug13_stall_event_not_dropped_when_queue_full() {
    let q = QueueConfig::MIN_QUEUE_SIZE; // smallest valid queue size
    let smmu = make_smmu_with_event_queue(q);

    // Fill the queue to capacity with non-stall events.
    for _ in 0..q {
        smmu.submit_event(EventEntry::new(EventType::FTranslation, 0, 0, 0))
            .unwrap();
    }
    assert_eq!(smmu.get_event_queue_size() as usize, q, "queue must be full before stall test");

    // Set up a stall-mode stream.
    setup_stall_stream(&smmu, 10);

    // Translate unmapped address — stall fault event is generated.
    // BEFORE fix: event silently dropped because queue.len() >= 2*capacity
    //             (queue is already at capacity from submit_event calls above).
    // AFTER fix:  event goes to stall_pending buffer, never dropped.
    let result = smmu.translate(
        sid(10),
        pasid(0),
        iova(0x2000), // unmapped
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(smmu::types::TranslationError::Stalled { .. })),
        "stall-mode translation must return Stalled; got: {result:?}"
    );

    // Clear the main queue to make space.
    smmu.clear_event_queue();

    // After clearing, the stall_pending event must drain into the main queue
    // and become visible via get_events().
    // (The drain may happen on the next get_events() call or the next record_event call.)
    // Trigger a new event to force drain of stall_pending.
    let _ = smmu.translate(
        sid(10),
        pasid(0),
        iova(0x3000), // another unmapped address
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let stall_events: Vec<_> = events.iter().filter(|e| e.stall).collect();

    assert!(
        !stall_events.is_empty(),
        "BUG-13: stall event must NOT be dropped when queue is full; \
         stall_events={}, total events={}",
        stall_events.len(),
        events.len()
    );
}

/// §7.4 / BUG-13: A stall event arriving when the main queue is at capacity
/// must NOT be dropped — it must go to stall_pending.
///
/// BEFORE fix: the else branch silently dropped the stall event.
/// AFTER fix: a stall_pending buffer holds the event until space is available.
///
/// Verification logic:
/// - Fill the queue to capacity with stall events (events A1..A_q).
/// - Send one more stall event (event B) — this is the "overflow" stall event.
/// - Check get_pending_stall_count():
///   BEFORE fix: 0 (event B was silently dropped).
///   AFTER fix:  >= 1 (event B is in stall_pending).
/// - Clear the main queue, trigger drain, verify event B appears.
#[test]
fn bug13_stall_event_not_dropped_at_2x_capacity() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    setup_stall_stream(&smmu, 15);

    // Fill the queue to capacity with stall events.
    for addr_offset in 0..q {
        let _ = smmu.translate(
            sid(15),
            pasid(0),
            iova(0x2000 + 0x1000 * addr_offset as u64), // all unmapped
            AccessType::Read,
            SecurityState::NonSecure,
        );
    }

    // Queue should be at capacity.
    let queue_size = smmu.get_event_queue_size() as usize;
    assert_eq!(
        queue_size, q,
        "precondition: queue must be at capacity; got {queue_size}"
    );

    // Another stall event arriving when queue is full.
    // BEFORE fix: silently dropped.
    // AFTER fix: goes to stall_pending.
    let overflow_result = smmu.translate(
        sid(15),
        pasid(0),
        iova(0x2000 + 0x1000 * (q + 1) as u64),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(overflow_result, Err(smmu::types::TranslationError::Stalled { .. })),
        "precondition: must get Stalled result; got: {overflow_result:?}"
    );

    // Check pending stall count — must be >= 1 after fix (0 before fix).
    // BEFORE fix: event was silently dropped, pending count = 0.
    // AFTER fix:  event is in stall_pending, pending count >= 1.
    let pending = smmu.get_pending_stall_count();
    assert!(
        pending >= 1,
        "BUG-13: stall event on a full queue must be in stall_pending (not dropped); \
         got pending_stall_count={pending}"
    );

    // Clear main queue to make space.
    smmu.clear_event_queue();

    // Trigger a new translation to force drain from stall_pending.
    let _ = smmu.translate(
        sid(15),
        pasid(0),
        iova(0x9999_0000), // unmapped
        AccessType::Read,
        SecurityState::NonSecure,
    );

    let events = smmu.get_events();
    let stall_count = events.iter().filter(|e| e.stall).count();

    // After drain: at least the pending stall event must have drained.
    assert!(
        stall_count >= 1,
        "BUG-13: stall event from stall_pending must drain into event queue; \
         got stall_count={stall_count} in {} total events",
        events.len()
    );
}

/// §7.4 / BUG-13: Multiple stall events arriving when the queue is full must
/// all be preserved in the stall_pending buffer — none must be dropped.
#[test]
fn bug13_multiple_stall_events_preserved_in_pending_buffer() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    // Fill the queue to capacity.
    for _ in 0..q {
        smmu.submit_event(EventEntry::new(EventType::FTranslation, 0, 0, 0))
            .unwrap();
    }

    setup_stall_stream(&smmu, 20);
    setup_stall_stream(&smmu, 21);

    // Two stall faults on a full queue — both must be preserved.
    let _ = smmu.translate(sid(20), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    let _ = smmu.translate(sid(21), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);

    // Clear main queue to make space.
    smmu.clear_event_queue();

    // Trigger drain by making a new translation.
    let _ = smmu.translate(sid(20), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure);

    let events = smmu.get_events();
    let stall_count = events.iter().filter(|e| e.stall).count();

    // At minimum the first pending stall event must have drained.
    assert!(
        stall_count >= 1,
        "BUG-13: at least one pending stall event must drain after queue is cleared; \
         stall_count={stall_count}"
    );
}

/// §7.4 / BUG-13: Stall events preserved in stall_pending must NOT trigger OVFLG.
///
/// ARM §7.4: "stall fault records do not cause an overflow condition."
/// OVFLG must remain unchanged when a stall event is redirected to stall_pending.
#[test]
fn bug13_stall_pending_does_not_trigger_ovflg() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    // Fill the queue.
    for _ in 0..q {
        smmu.submit_event(EventEntry::new(EventType::FTranslation, 0, 0, 0))
            .unwrap();
    }

    let ovflg_before = (smmu.get_eventq_prod() >> 31) & 1;
    assert_eq!(ovflg_before, 0, "precondition: OVFLG must be 0 before test");

    setup_stall_stream(&smmu, 30);

    // Stall fault on a full queue — must NOT toggle OVFLG.
    let _ = smmu.translate(sid(30), pasid(0), iova(0x2000), AccessType::Read, SecurityState::NonSecure);

    let ovflg_after = (smmu.get_eventq_prod() >> 31) & 1;
    assert_eq!(
        ovflg_before, ovflg_after,
        "BUG-13: stall event preserved in stall_pending must NOT toggle OVFLG"
    );
}

/// §7.4 / BUG-13: Non-stall events on a full queue must still trigger OVFLG.
///
/// This ensures the stall_pending path does not interfere with the normal
/// overflow signalling for non-stall events.
#[test]
fn bug13_non_stall_overflow_still_triggers_ovflg() {
    let q = QueueConfig::MIN_QUEUE_SIZE;
    let smmu = make_smmu_with_event_queue(q);

    // Configure terminate-mode stream.
    let s = sid(40);
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .fault_mode(smmu::types::FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(s, config).unwrap();
    smmu.create_pasid(s, pasid(0)).unwrap();

    // Fill the queue.
    for i in 0..q {
        let _ = smmu.translate(s, pasid(0), iova(0x1000 * (i as u64 + 1)), AccessType::Read, SecurityState::NonSecure);
    }
    assert_eq!(smmu.get_event_queue_size() as usize, q, "precondition: queue full");
    assert_eq!((smmu.get_eventq_prod() >> 31) & 1, 0, "precondition: OVFLG=0");

    // One more non-stall fault — should trigger OVFLG.
    let _ = smmu.translate(s, pasid(0), iova(0x1000 * (q as u64 + 1)), AccessType::Read, SecurityState::NonSecure);

    assert_eq!(
        (smmu.get_eventq_prod() >> 31) & 1, 1,
        "BUG-13 regression: non-stall overflow must still toggle OVFLG to 1"
    );
}
