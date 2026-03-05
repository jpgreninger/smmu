#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::doc_lazy_continuation)]

//! TDD test for BUG-RUST-4: TOCTOU race in TlbCache::insert()
//!
//! In `src/cache/mod.rs` around lines 714-719, `TlbCache::insert()` checks
//! `self.entries.len() >= self.capacity` then separately calls
//! `self.entries.insert(key, entry)`.  Since `DashMap` shards locks by key
//! hash, two concurrent threads can both pass the capacity check and both
//! insert, causing the cache to exceed capacity.
//!
//! Fix: Add an `AtomicUsize` counter and use `fetch_add`/`compare_exchange`
//! for atomic capacity tracking, OR document the bounded overage.
//!
//! # Test Strategy
//!
//! 1. Create a TlbCache with capacity N.
//! 2. Spawn M threads (M >> N), each inserting a unique entry concurrently.
//! 3. After all threads complete, assert `cache.len() <= N + tolerance`.
//! 4. Before the fix, `cache.len()` may equal M (way over capacity).

use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
use smmu::types::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};
use std::sync::Arc;
use std::thread;

fn make_key(stream_n: u32, iova_n: u64) -> CacheKey {
    let stream_id = StreamID::new(stream_n % 1000).unwrap(); // keep within valid range
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(iova_n * 0x1000).unwrap();
    CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure)
}

fn make_entry(iova_n: u64, pa_n: u64) -> CacheEntry {
    let iova = IOVA::new(iova_n * 0x1000).unwrap();
    let pa = PA::new(pa_n * 0x1000).unwrap();
    CacheEntry::new(iova, pa, PagePermissions::read_write(), 0)
}

// ── Test 1: Single-threaded capacity enforcement (sanity check) ─────────────

/// Single-threaded capacity check: inserting N+extra entries must not exceed N.
///
/// This test should pass before AND after the fix (single-threaded eviction
/// works correctly today).
#[test]
fn tlb_cache_capacity_enforced_single_thread() {
    let capacity = 10usize;
    let cache = TlbCache::new(capacity, ReplacementPolicy::Lru);

    for i in 0u32..30 {
        let key = make_key(i, u64::from(i));
        let entry = make_entry(u64::from(i), u64::from(i) + 100);
        cache.insert(key, entry);
    }

    assert!(
        cache.len() <= capacity,
        "Single-threaded: cache len {} must not exceed capacity {}",
        cache.len(),
        capacity
    );
}

// ── Test 2: THE CRITICAL FAILING TEST — concurrent insert capacity violation ─

/// Concurrent insert: M threads each insert a unique entry into a cache with
/// capacity N (N << M).  After all threads complete, the cache must not exceed
/// capacity + a small tolerance (e.g., capacity + number of CPU cores * 2).
///
/// BEFORE FIX: The TOCTOU race allows multiple threads to pass the `len >=
/// capacity` check simultaneously and all insert, causing `cache.len() > N`.
/// In extreme cases (M = 10*N), the cache may hold M entries.
///
/// AFTER FIX: An atomic count tracks the pre-eviction size and prevents
/// simultaneous insertions from exceeding capacity.  The cache len must not
/// exceed `capacity + small_tolerance`.
///
/// `small_tolerance` accounts for:
/// - ABA races on the AtomicUsize counter itself (unavoidable without global lock)
/// - DashMap shard-level races during evict_one()
///
/// A tolerance of 0 (strict capacity) is achievable with a global mutex but
/// would hurt performance.  A tolerance of `num_cpus` is acceptable.
#[test]
fn tlb_cache_concurrent_inserts_respect_capacity() {
    let capacity = 16usize;
    let num_threads = 64usize;
    // Before the fix, violation can be up to num_threads entries over capacity;
    // after the fix, the overage is bounded to ~0 (a few due to inherent lock-free races).
    // We use capacity + 4 as the strict limit in the assertion below.

    let cache = Arc::new(TlbCache::new(capacity, ReplacementPolicy::Lru));

    let mut handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            // Each thread inserts a unique entry
            let key = make_key(u32::try_from(t % 1000).unwrap_or(u32::MAX), t as u64 + 1);
            let entry = make_entry(t as u64 + 1, t as u64 + 200);
            cache_clone.insert(key, entry);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let len = cache.len();

    // BEFORE FIX: len may equal num_threads (64) >> capacity (16).
    // AFTER FIX: len must be <= capacity + small_tolerance.
    //
    // A strict capacity check would be `len <= capacity`, but we allow
    // `capacity + 4` to account for inherent races in lock-free structures.
    let strict_limit = capacity + 4;

    assert!(
        len <= strict_limit,
        "BUG-RUST-4: TlbCache::insert() TOCTOU race — capacity={capacity}, \
         num_threads={num_threads}, actual len={len}, max allowed={strict_limit}. \
         Fix by using atomic capacity tracking to prevent concurrent over-insertion."
    );
}

// ── Test 3: Concurrent insert correctness — every inserted key is retrievable ─

/// When multiple threads insert entries concurrently, every entry that was
/// successfully inserted (within capacity) must be retrievable OR have been
/// evicted.  No entry should be partially written or corrupt.
///
/// This test verifies correctness, not just capacity.
#[test]
fn tlb_cache_concurrent_inserts_are_consistent() {
    let capacity = 32usize;
    let num_threads = 32usize;

    let cache = Arc::new(TlbCache::new(capacity, ReplacementPolicy::Lru));
    let mut handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            let key = make_key(u32::try_from(t % 1000).unwrap_or(u32::MAX), t as u64 + 1);
            let entry = make_entry(t as u64 + 1, t as u64 + 500);
            cache_clone.insert(key, entry);

            // Immediately look up the key — it may have been evicted by another thread
            // but if found, it must have the correct physical address.
            if let Some(hit) = cache_clone.lookup(&key) {
                let expected_pa = (t as u64 + 500) * 0x1000;
                assert_eq!(
                    hit.physical_address.as_u64(),
                    expected_pa,
                    "Thread {t}: cache hit must have correct PA"
                );
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // After all threads finish, cache must be in a consistent state.
    assert!(
        cache.len() <= capacity,
        "After concurrent inserts matching capacity: len {} must not exceed capacity {}",
        cache.len(),
        capacity
    );
}

// ── Test 4: Concurrent insert with LRU policy ───────────────────────────────

/// Verify that concurrent inserts with LRU policy do not panic or corrupt state.
#[test]
fn tlb_cache_concurrent_inserts_lru_no_panic() {
    let capacity = 8usize;
    let num_threads = 48usize;

    let cache = Arc::new(TlbCache::new(capacity, ReplacementPolicy::Lru));
    let mut handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            for j in 0..4usize {
                let idx = t * 4 + j;
                let key = make_key(u32::try_from(idx % 1000).unwrap_or(u32::MAX), idx as u64 + 1);
                let entry = make_entry(idx as u64 + 1, idx as u64 + 1000);
                cache_clone.insert(key, entry);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked (LRU concurrent insert)");
    }

    // Must not have grown unboundedly — allow 2x tolerance for LRU eviction races
    assert!(
        cache.len() <= capacity * 2,
        "BUG-RUST-4 LRU: cache len {} must not greatly exceed capacity {}",
        cache.len(),
        capacity
    );
}

// ── Test 5: Stress test — high contention on small cache ────────────────────

/// Stress test: many threads hammer a tiny cache (capacity=4) concurrently.
///
/// BEFORE FIX: cache may hold hundreds of entries (num_threads * iterations).
/// AFTER FIX: cache must stay near capacity at all times.
#[test]
fn tlb_cache_stress_high_contention_small_cache() {
    let capacity = 4usize;
    let num_threads = 32usize;
    let iterations_per_thread = 5usize;

    let cache = Arc::new(TlbCache::new(capacity, ReplacementPolicy::Fifo));
    let mut handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            for i in 0..iterations_per_thread {
                let idx = t * iterations_per_thread + i;
                let key = make_key(u32::try_from(idx % 1000).unwrap_or(u32::MAX), idx as u64 + 1);
                let entry = make_entry(idx as u64 + 1, idx as u64 + 2000);
                cache_clone.insert(key, entry);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked (stress test)");
    }

    let total_inserted = num_threads * iterations_per_thread; // 160 entries
    let len = cache.len();

    // BEFORE FIX: len may be up to total_inserted (160) >> capacity (4).
    // AFTER FIX: len must be bounded near capacity.
    //
    // We allow capacity * 8 as a conservative post-fix bound (still far below
    // total_inserted for the before-fix state).
    let allowed_max = capacity * 8; // 32 — well below 160 if fix works

    assert!(
        len <= allowed_max,
        "BUG-RUST-4 stress: capacity={capacity}, total_inserted={total_inserted}, \
         actual len={len}, max allowed={allowed_max}. \
         TOCTOU race allows all threads to insert simultaneously when unconstrained."
    );
}
