#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]

//! Concurrency Stress Testing (Task 5.2)
//!
//! This module provides extensive stress testing for concurrent operations
//! with random workloads. While Loom tests exhaustively check all interleavings
//! for small tests, these stress tests use real threading with randomized
//! patterns to find issues in realistic high-load scenarios.
//!
//! Test Categories:
//! - Random workload stress tests
//! - High-contention scenarios
//! - Mixed read/write patterns
//! - Burst load handling
//! - Long-running concurrent operations
//!
//! Running with `ThreadSanitizer` (if available):
//! ```bash
//! RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test --tests -Z build-std --target x86_64-unknown-linux-gnu
//! ```

use rand::Rng;
use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, CacheConfig, PagePermissions, SecurityState, SMMUConfigBuilder, StreamConfig,
    StreamID, IOVA, PA, PAGE_SIZE, PASID,
};
use smmu::SMMU;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

// ============================================================================
// Utility Functions
// ============================================================================

fn random_page_aligned_iova() -> IOVA {
    let mut rng = rand::thread_rng();
    let page_num = rng.gen_range(1..0x10_0000);
    IOVA::new(page_num * PAGE_SIZE).unwrap()
}

fn random_pa() -> PA {
    let mut rng = rand::thread_rng();
    let page_num = rng.gen_range(1..0x10_0000);
    PA::new(page_num * PAGE_SIZE).unwrap()
}

fn random_access_type() -> AccessType {
    let mut rng = rand::thread_rng();
    match rng.gen_range(0..3) {
        0 => AccessType::Read,
        1 => AccessType::Write,
        _ => AccessType::Execute,
    }
}

fn random_permissions() -> PagePermissions {
    let mut rng = rand::thread_rng();
    match rng.gen_range(0..8) {
        0 => PagePermissions::none(),
        1 => PagePermissions::read_only(),
        2 => PagePermissions::write_only(),
        3 => PagePermissions::execute_only(),
        4 => PagePermissions::read_write(),
        5 => PagePermissions::read_execute(),
        6 => PagePermissions::new(false, true, true), // write + execute
        _ => PagePermissions::all(),
    }
}

fn random_security_state() -> SecurityState {
    let mut rng = rand::thread_rng();
    match rng.gen_range(0..3) {
        0 => SecurityState::NonSecure,
        1 => SecurityState::Secure,
        _ => SecurityState::Realm,
    }
}

// ============================================================================
// AddressSpace Stress Tests
// ============================================================================

#[test]
fn stress_address_space_concurrent_random_maps() {
    const NUM_THREADS: usize = 8;
    const OPS_PER_THREAD: usize = 1000;

    let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let addr_space_clone = Arc::clone(&addr_space);

        let handle = thread::spawn(move || {
            for i in 0..OPS_PER_THREAD {
                let iova = IOVA::new((thread_id * 0x10_0000 + i * 0x1000) as u64).unwrap();
                let pa = random_pa();
                let perms = random_permissions();
                let security = random_security_state();

                let mut guard = addr_space_clone.lock().unwrap();
                let _ = guard.map_page(iova, pa, perms, security);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify final state
    let page_count = {
        let guard = addr_space.lock().unwrap();
        guard.get_page_count().unwrap_or(0)
    };
    assert!(
        page_count > 0,
        "Address space should have mappings after concurrent operations"
    );
    assert!(
        page_count <= NUM_THREADS * OPS_PER_THREAD,
        "Page count should not exceed total operations"
    );
}

#[test]
fn stress_address_space_mixed_read_write() {
    const NUM_READERS: usize = 4;
    const NUM_WRITERS: usize = 2;
    const DURATION_MS: u64 = 100;

    let addr_space = Arc::new(AddressSpace::new());

    // Pre-populate with some mappings (no lock needed - AddressSpace is lock-free)
    {
        for i in 0..100 {
            let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
            let pa = PA::new(0x10_0000 + i * PAGE_SIZE).unwrap();
            let _ = addr_space.map_page(iova, pa, PagePermissions::all(), SecurityState::NonSecure);
        }
    }

    let mut handles = vec![];

    // Spawn reader threads
    for _ in 0..NUM_READERS {
        let addr_space_clone = Arc::clone(&addr_space);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let start = std::time::Instant::now();

            while start.elapsed() < Duration::from_millis(DURATION_MS) {
                let i = rng.gen_range(0..100);
                let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();

                // No lock needed - AddressSpace is lock-free
                let _ = addr_space_clone.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
            }
        });

        handles.push(handle);
    }

    // Spawn writer threads
    for _ in 0..NUM_WRITERS {
        let addr_space_clone = Arc::clone(&addr_space);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let start = std::time::Instant::now();

            while start.elapsed() < Duration::from_millis(DURATION_MS) {
                let i = rng.gen_range(0..100);
                let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
                let pa = random_pa();

                // No lock needed - AddressSpace is lock-free
                let _ = addr_space_clone.map_page(iova, pa, PagePermissions::all(), SecurityState::NonSecure);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify consistency (no lock needed - AddressSpace is lock-free)
    assert!(addr_space.get_page_count().unwrap() > 0);
}

#[test]
fn stress_address_space_high_contention() {
    const NUM_THREADS: usize = 16;
    const SHARED_PAGES: usize = 10;
    const OPS_PER_THREAD: usize = 500;

    let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
    let mut handles = vec![];

    for _ in 0..NUM_THREADS {
        let addr_space_clone = Arc::clone(&addr_space);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();

            for _ in 0..OPS_PER_THREAD {
                // All threads compete for same small set of pages
                let page_idx = rng.gen_range(0..SHARED_PAGES);
                let iova = IOVA::new((page_idx * 0x1000) as u64).unwrap();
                let pa = random_pa();

                let mut guard = addr_space_clone.lock().unwrap();

                // Random operation
                match rng.gen_range(0..3) {
                    0 => {
                        // Map
                        let _ =
                            guard.map_page(iova, pa, PagePermissions::all(), SecurityState::NonSecure);
                    }
                    1 => {
                        // Translate
                        let _ = guard.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
                    }
                    _ => {
                        // Unmap
                        let _ = guard.unmap_page(iova);
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should complete without deadlock or panic
    let guard = addr_space.lock().unwrap();
    let _ = guard.get_page_count();
}

// ============================================================================
// StreamContext Stress Tests
// ============================================================================

#[test]
fn stress_stream_context_concurrent_pasid_management() {
    const NUM_THREADS: usize = 8;
    const PASIDS_PER_THREAD: usize = 100;

    let stream_context = Arc::new(StreamContext::new());
    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let stream_context_clone = Arc::clone(&stream_context);

        let handle = thread::spawn(move || {
            for i in 0..PASIDS_PER_THREAD {
                let pasid_val = (thread_id * PASIDS_PER_THREAD + i) as u32;
                if let Ok(pasid) = PASID::new(pasid_val) {
                    let _ = stream_context_clone.create_pasid(pasid);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify final state
    let expected_count = NUM_THREADS * PASIDS_PER_THREAD;
    let actual_count = stream_context.pasid_count();

    assert_eq!(
        actual_count, expected_count,
        "All PASIDs should be created without duplicates or loss"
    );
}

#[test]
fn stress_stream_context_mixed_operations() {
    const NUM_THREADS: usize = 8;
    const DURATION_MS: u64 = 100;
    const MAX_PASIDS: u32 = 100;

    let stream_context = Arc::new(StreamContext::new());
    let mut handles = vec![];

    for _ in 0..NUM_THREADS {
        let stream_context_clone = Arc::clone(&stream_context);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let start = std::time::Instant::now();

            while start.elapsed() < Duration::from_millis(DURATION_MS) {
                let pasid_val = rng.gen_range(0..MAX_PASIDS);
                if let Ok(pasid) = PASID::new(pasid_val) {
                    match rng.gen_range(0..4) {
                        0 => {
                            // Create PASID
                            let _ = stream_context_clone.create_pasid(pasid);
                        }
                        1 => {
                            // Check PASID
                            let _ = stream_context_clone.has_pasid(pasid);
                        }
                        2 => {
                            // Map page
                            let iova = random_page_aligned_iova();
                            let pa = random_pa();
                            let _ = stream_context_clone.map_page(
                                pasid,
                                iova,
                                pa,
                                PagePermissions::all(),
                                SecurityState::NonSecure,
                            );
                        }
                        _ => {
                            // Translate
                            let iova = random_page_aligned_iova();
                            let _ = stream_context_clone.translate(
                                pasid,
                                iova,
                                random_access_type(),
                                SecurityState::NonSecure,
                            );
                        }
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify consistency
    let pasid_count = stream_context.pasid_count();
    assert!(
        pasid_count <= MAX_PASIDS as usize,
        "PASID count should not exceed maximum"
    );
}

#[test]
fn stress_stream_context_translation_bursts() {
    const NUM_THREADS: usize = 16;
    const BURST_SIZE: usize = 100;
    const NUM_BURSTS: usize = 10;

    let stream_context = Arc::new(StreamContext::new());

    // Setup: Create PASIDs and mappings
    for i in 0..NUM_THREADS {
        let pasid = PASID::new(i as u32).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        for j in 0..10 {
            let iova = IOVA::new(0x1000 + (j * 0x1000) as u64).unwrap();
            let pa = PA::new(0x10_0000 + (j * 0x1000) as u64).unwrap();
            let _ = stream_context.map_page(
                pasid,
                iova,
                pa,
                PagePermissions::all(),
                SecurityState::NonSecure,
            );
        }
    }

    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let stream_context_clone = Arc::clone(&stream_context);

        let handle = thread::spawn(move || {
            let pasid = PASID::new(thread_id as u32).unwrap();
            let mut rng = rand::thread_rng();

            for _ in 0..NUM_BURSTS {
                // Burst of translations
                for _ in 0..BURST_SIZE {
                    let page_idx = rng.gen_range(0..10);
                    let iova = IOVA::new(0x1000 + (page_idx * 0x1000) as u64).unwrap();

                    let _ = stream_context_clone.translate(
                        pasid,
                        iova,
                        random_access_type(),
                        SecurityState::NonSecure,
                    );
                }

                // Small delay between bursts
                thread::sleep(Duration::from_micros(10));
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // All threads should complete successfully
}

// ============================================================================
// SMMU Integration Stress Tests
// ============================================================================

#[test]
fn stress_smmu_concurrent_stream_operations() {
    const NUM_THREADS: usize = 8;
    const OPS_PER_THREAD: usize = 100;

    let smmu = Arc::new(Mutex::new(SMMU::new()));
    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);

        let handle = thread::spawn(move || {
            let stream_id = StreamID::new(thread_id as u32).unwrap();
            let pasid = PASID::new(0).unwrap();

            // Configure stream
            {
                let guard = smmu_clone.lock().unwrap();
                let config = StreamConfig::stage1_only();
                let _ = guard.configure_stream(stream_id, config);
                let _ = guard.create_pasid(stream_id, pasid);
            }

            // Perform operations
            for i in 0..OPS_PER_THREAD {
                let iova = IOVA::new(0x1000 + (i * 0x1000) as u64).unwrap();
                let pa = random_pa();

                let guard = smmu_clone.lock().unwrap();

                // Map
                let _ = guard.map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::all(),
                    SecurityState::NonSecure,
                );

                // Translate
                let _ = guard.translate(
                    stream_id,
                    pasid,
                    iova,
                    AccessType::Read,
                );
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn stress_smmu_mixed_workload() {
    const NUM_THREADS: usize = 12;
    const DURATION_MS: u64 = 100;
    const NUM_STREAMS: u32 = 10;

    let smmu = Arc::new(Mutex::new(SMMU::new()));

    // Setup: Configure streams and PASIDs
    {
        let guard = smmu.lock().unwrap();
        for i in 0..NUM_STREAMS {
            let stream_id = StreamID::new(i).unwrap();
            let config = StreamConfig::stage1_only();
            let _ = guard.configure_stream(stream_id, config);
            let pasid = PASID::new(0).unwrap();
            let _ = guard.create_pasid(stream_id, pasid);
        }
    }

    let mut handles = vec![];

    for _ in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let start = std::time::Instant::now();

            while start.elapsed() < Duration::from_millis(DURATION_MS) {
                let stream_val = rng.gen_range(0..NUM_STREAMS);
                let stream_id = StreamID::new(stream_val).unwrap();
                let pasid = PASID::new(0).unwrap();
                let iova = random_page_aligned_iova();

                let guard = smmu_clone.lock().unwrap();

                match rng.gen_range(0..3) {
                    0 => {
                        // Map
                        let pa = random_pa();
                        let _ = guard.map_page(
                            stream_id,
                            pasid,
                            iova,
                            pa,
                            PagePermissions::all(),
                            SecurityState::NonSecure,
                        );
                    }
                    1 => {
                        // Translate
                        let _ = guard.translate(
                            stream_id,
                            pasid,
                            iova,
                            random_access_type(),
                        );
                    }
                    _ => {
                        // Remap with different permissions
                        let pa = random_pa();
                        let _ = guard.map_page(
                            stream_id,
                            pasid,
                            iova,
                            pa,
                            PagePermissions::none(),
                            SecurityState::NonSecure,
                        );
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn stress_smmu_cache_thrashing() {
    const NUM_THREADS: usize = 16;
    const CACHE_SIZE: usize = 64;
    const OPS_PER_THREAD: usize = 1000;

    let cache_config = CacheConfig {
        tlb_cache_size: CACHE_SIZE,
        cache_max_age_ms: 5000,
        enable_caching: true,
    };
    let config = SMMUConfigBuilder::new()
        .cache_config(cache_config)
        .build()
        .unwrap();
    let smmu = Arc::new(Mutex::new(SMMU::with_config(config)));
    let stream_id = StreamID::new(0).unwrap();
    let pasid = PASID::new(0).unwrap();

    // Setup
    {
        let guard = smmu.lock().unwrap();
        let config = StreamConfig::stage1_only();
        guard.configure_stream(stream_id, config).unwrap();
        guard.create_pasid(stream_id, pasid).unwrap();

        // Pre-populate cache with mappings
        for i in 0..CACHE_SIZE * 4 {
            let iova = IOVA::new(0x1000 + (i * 0x1000) as u64).unwrap();
            let pa = PA::new(0x10_0000 + (i * 0x1000) as u64).unwrap();
            let _ = guard.map_page(
                stream_id,
                pasid,
                iova,
                pa,
                PagePermissions::all(),
                SecurityState::NonSecure,
            );
        }
    }

    let mut handles = vec![];

    for _ in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();

            for _ in 0..OPS_PER_THREAD {
                // Access random pages to thrash cache
                let page_idx = rng.gen_range(0..CACHE_SIZE * 4);
                let iova = IOVA::new(0x1000 + (page_idx * 0x1000) as u64).unwrap();

                let guard = smmu_clone.lock().unwrap();
                let _ = guard.translate(
                    stream_id,
                    pasid,
                    iova,
                    AccessType::Read,
                );
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify that operations completed successfully
    let (total, successful, _failed) = {
        let guard = smmu.lock().unwrap();
        guard.get_translation_stats()
    };
    assert!(total > 0, "Should have recorded translations");
    assert!(successful > 0, "Should have successful translations");
}

// ============================================================================
// Long-Running Stress Tests
// ============================================================================

#[test]
#[ignore = "Run with --ignored for extended testing"]
fn stress_long_running_concurrent_operations() {
    const NUM_THREADS: usize = 8;
    const DURATION_SECONDS: u64 = 10;

    let smmu = Arc::new(Mutex::new(SMMU::new()));
    let mut handles = vec![];

    for thread_id in 0..NUM_THREADS {
        let smmu_clone = Arc::clone(&smmu);

        let handle = thread::spawn(move || {
            let stream_id = StreamID::new(thread_id as u32).unwrap();
            let pasid = PASID::new(0).unwrap();
            let mut rng = rand::thread_rng();

            // Configure stream
            {
                let guard = smmu_clone.lock().unwrap();
                let config = StreamConfig::stage1_only();
                let _ = guard.configure_stream(stream_id, config);
                let _ = guard.create_pasid(stream_id, pasid);
            }

            let start = std::time::Instant::now();
            let mut op_count = 0;

            while start.elapsed() < Duration::from_secs(DURATION_SECONDS) {
                let iova = random_page_aligned_iova();
                let pa = random_pa();

                {
                    let guard = smmu_clone.lock().unwrap();

                    match rng.gen_range(0..10) {
                        0..=3 => {
                            // Map (40%)
                            let _ = guard.map_page(
                                stream_id,
                                pasid,
                                iova,
                                pa,
                                random_permissions(),
                                random_security_state(),
                            );
                        }
                        4..=8 => {
                            // Translate (50%)
                            let _ = guard.translate(
                                stream_id,
                                pasid,
                                iova,
                                random_access_type(),
                            );
                        }
                        _ => {
                            // Invalidate/Remap (10%)
                            let _ = guard.map_page(
                                stream_id,
                                pasid,
                                iova,
                                pa,
                                PagePermissions::none(),
                                random_security_state(),
                            );
                        }
                    }
                }

                op_count += 1;
            }

            println!("Thread {thread_id} completed {op_count} operations in {DURATION_SECONDS} seconds");
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
