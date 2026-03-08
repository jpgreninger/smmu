#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD test for BUG-6: TOCTOU race on stream-count limit check in `configure_stream`.
//!
//! The `streams.len() >= max_streams` check is performed outside the DashMap shard
//! lock.  Two concurrent `configure_stream` calls can both pass the limit check
//! before either inserts, allowing the stream table to exceed `max_streams` by the
//! number of concurrent callers.
//!
//! Fix: use an `AtomicUsize` counter that is atomically incremented BEFORE the
//! DashMap insert (fetch_add), and decremented on failure or on duplicate.
//!
//! # Test Strategy
//!
//! 1. Create an SMMU with `max_streams = 2`.
//! 2. Spawn `N` threads (N >> 2), each attempting `configure_stream` with a unique
//!    `StreamID`.
//! 3. After all threads complete, assert `smmu.get_stream_count() <= 2`.
//!
//! BEFORE FIX: Multiple threads can race past the `streams.len()` check and all
//! insert, producing a stream count > 2.
//!
//! AFTER FIX: The AtomicUsize gate ensures that at most `max_streams` insertions
//! succeed.

use smmu::types::{SMMUConfig, StreamConfig, StreamID};
use smmu::SMMU;
use std::sync::Arc;
use std::thread;

// ── Test 1: Single-threaded limit is enforced (sanity check) ─────────────────

/// Single-threaded limit check: configuring more than `max_streams` streams
/// must fail starting at stream N+1.
///
/// This test passes both before and after the fix.
#[test]
fn stream_limit_enforced_single_thread() {
    let config = SMMUConfig::default().with_max_streams(2);
    let smmu = SMMU::with_config(config);

    let s0 = StreamID::new(0).unwrap();
    let s1 = StreamID::new(1).unwrap();
    let s2 = StreamID::new(2).unwrap();

    assert!(smmu.configure_stream(s0, StreamConfig::bypass()).is_ok());
    assert!(smmu.configure_stream(s1, StreamConfig::bypass()).is_ok());

    let result = smmu.configure_stream(s2, StreamConfig::bypass());
    assert!(
        result.is_err(),
        "Expected Err when exceeding max_streams=2, got Ok"
    );

    assert_eq!(
        smmu.get_stream_count(),
        2,
        "Stream count must be exactly 2 after one rejected insertion"
    );
}

// ── Test 2: THE CRITICAL FAILING TEST — concurrent configure_stream race ──────

/// Concurrent stream configuration must never exceed `max_streams`.
///
/// BEFORE FIX: Two (or more) threads simultaneously pass the `streams.len() >=
/// max_streams` check and both insert, producing a stream count > `max_streams`.
///
/// AFTER FIX: An `AtomicUsize` counter is incremented before the DashMap insert;
/// threads that observe `count >= max_streams` after increment roll back and
/// return `Err`.  The final stream count is exactly `<= max_streams`.
#[test]
fn stream_limit_not_exceeded_under_concurrent_configure() {
    let max_streams: usize = 2;
    let num_threads: usize = 32;

    let config = SMMUConfig::default().with_max_streams(max_streams);
    let smmu = Arc::new(SMMU::with_config(config));

    let mut handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            // Each thread uses a unique StreamID so duplicate-stream rejection
            // does not confound the count: we are purely testing the limit check.
            let stream_id = StreamID::new(u32::try_from(t).unwrap()).unwrap();
            // Ignore Ok/Err — what matters is the final count.
            let _ = smmu_clone.configure_stream(stream_id, StreamConfig::bypass());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let count = smmu.get_stream_count();

    assert!(
        count <= max_streams,
        "BUG-6: TOCTOU race on stream-count check — \
         max_streams={max_streams}, actual count={count}. \
         Fix by using AtomicUsize counter that is incremented before DashMap insert."
    );
}

// ── Test 3: Exact-limit stress with many threads ──────────────────────────────

/// Repeated concurrent configuration bursts must never push the stream count
/// over the configured limit.
///
/// Uses `max_streams = 4` and 64 concurrent threads to maximise collision
/// probability across all DashMap shards.
#[test]
fn stream_limit_stress_many_threads() {
    let max_streams: usize = 4;
    let num_threads: usize = 64;

    let config = SMMUConfig::default().with_max_streams(max_streams);
    let smmu = Arc::new(SMMU::with_config(config));

    let mut handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            let stream_id = StreamID::new(u32::try_from(t).unwrap()).unwrap();
            let _ = smmu_clone.configure_stream(stream_id, StreamConfig::bypass());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let count = smmu.get_stream_count();

    assert!(
        count <= max_streams,
        "BUG-6 stress: max_streams={max_streams}, actual count={count} after {num_threads} concurrent threads."
    );
}

// ── Test 4: Atomic counter rolls back on duplicate insert ─────────────────────

/// When two threads race to configure the SAME stream, the losing thread must
/// observe an `Err` (stream already configured) AND the atomic counter must be
/// decremented so the slot is not permanently consumed.
///
/// After the race, a third unique stream must still be configurable if
/// `max_streams` allows it.
#[test]
fn stream_count_rolls_back_on_duplicate() {
    let max_streams: usize = 2;
    let config = SMMUConfig::default().with_max_streams(max_streams);
    let smmu = Arc::new(SMMU::with_config(config));

    // Both threads attempt to configure stream 0 — exactly one should succeed.
    let smmu1 = Arc::clone(&smmu);
    let smmu2 = Arc::clone(&smmu);

    let s0 = StreamID::new(0).unwrap();

    let h1 = thread::spawn(move || smmu1.configure_stream(s0, StreamConfig::bypass()));
    let h2 = thread::spawn(move || smmu2.configure_stream(s0, StreamConfig::bypass()));

    let r1 = h1.join().expect("Thread 1 panicked");
    let r2 = h2.join().expect("Thread 2 panicked");

    // Exactly one must succeed.
    let successes = [r1.is_ok(), r2.is_ok()].iter().filter(|&&b| b).count();
    assert_eq!(
        successes, 1,
        "Exactly one configure_stream(stream=0) must succeed; both succeeded or both failed."
    );

    // Count must be 1 (the single successful insert for stream 0).
    assert_eq!(
        smmu.get_stream_count(),
        1,
        "After one successful and one duplicate configure_stream, count must be 1"
    );

    // A second unique stream (stream 1) must still be configurable because max_streams=2.
    let s1 = StreamID::new(1).unwrap();
    assert!(
        smmu.configure_stream(s1, StreamConfig::bypass()).is_ok(),
        "Stream 1 must be configurable: only 1 slot consumed so far out of max_streams=2. \
         If AtomicUsize did not roll back on duplicate, this will fail."
    );

    assert_eq!(
        smmu.get_stream_count(),
        2,
        "After inserting stream 0 and stream 1, count must be 2"
    );
}
