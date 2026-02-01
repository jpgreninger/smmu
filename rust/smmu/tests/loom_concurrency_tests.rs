//! Comprehensive Loom-based Concurrency Testing
//!
//! This test suite uses Loom to exhaustively test concurrent access patterns
//! and detect race conditions, deadlocks, and memory ordering issues in the
//! SMMU implementation.
//!
//! ## About Loom
//!
//! Loom is a testing tool that exhaustively explores all possible thread
//! interleavings to find concurrency bugs. Unlike traditional stress testing
//! that relies on random scheduling, Loom systematically checks all possible
//! execution orderings.
//!
//! ## Running Loom Tests
//!
//! ```bash
//! # Run all loom tests (WARNING: Can be slow for complex tests)
//! RUSTFLAGS="--cfg loom" cargo test --test loom_concurrency_tests --release
//!
//! # Run specific test
//! RUSTFLAGS="--cfg loom" cargo test --test loom_concurrency_tests loom_cache_concurrent_insert
//! ```
//!
//! ## Test Categories
//!
//! 1. **AddressSpace Concurrency** - Concurrent mapping/unmapping operations
//! 2. **StreamContext Concurrency** - Concurrent PASID management
//! 3. **TLB Cache Concurrency** - Concurrent cache operations
//! 4. **Fault Queue Concurrency** - Concurrent fault recording
//! 5. **SMMU Concurrency** - Full SMMU concurrent operations
//! 6. **Memory Ordering** - Happens-before relationship validation
//!
//! ## ARM SMMU v3 Concurrency Requirements
//!
//! - Thread-safe concurrent translations
//! - Safe concurrent PASID creation/deletion
//! - Lock-free TLB cache lookups (when possible)
//! - Correct fault ordering and attribution

#![cfg(loom)]

use loom::sync::{Arc, Mutex, RwLock};
use loom::thread;

// ============================================================================
// Section 4.1.1: AddressSpace Concurrency Tests
// ============================================================================

#[test]
fn loom_address_space_concurrent_map_different_pages() {
    loom::model(|| {
        use smmu::address_space::AddressSpace;
        use smmu::types::{PagePermissions, SecurityState, IOVA, PA};

        let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap();
        let pa1 = PA::new(0x3000).unwrap();
        let pa2 = PA::new(0x4000).unwrap();

        let addr_space1 = Arc::clone(&addr_space);
        let addr_space2 = Arc::clone(&addr_space);

        let t1 = thread::spawn(move || {
            let mut guard = addr_space1.lock().unwrap();
            guard
                .map_page(iova1, pa1, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        });

        let t2 = thread::spawn(move || {
            let mut guard = addr_space2.lock().unwrap();
            guard
                .map_page(iova2, pa2, PagePermissions::read_write(), SecurityState::NonSecure)
                .unwrap();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Both mappings should succeed
        let guard = addr_space.lock().unwrap();
        assert_eq!(guard.get_page_count().unwrap(), 2);
    });
}

#[test]
fn loom_address_space_concurrent_map_same_page() {
    loom::model(|| {
        use smmu::address_space::AddressSpace;
        use smmu::types::{PagePermissions, SecurityState, IOVA, PA};

        let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
        let iova = IOVA::new(0x1000).unwrap();
        let pa1 = PA::new(0x3000).unwrap();
        let pa2 = PA::new(0x4000).unwrap();

        let addr_space1 = Arc::clone(&addr_space);
        let addr_space2 = Arc::clone(&addr_space);

        let t1 = thread::spawn(move || {
            let mut guard = addr_space1.lock().unwrap();
            let _ = guard.map_page(iova, pa1, PagePermissions::read_only(), SecurityState::NonSecure);
        });

        let t2 = thread::spawn(move || {
            let mut guard = addr_space2.lock().unwrap();
            let _ = guard.map_page(iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // One mapping should win (last writer)
        let guard = addr_space.lock().unwrap();
        assert_eq!(guard.get_page_count().unwrap(), 1);
    });
}

#[test]
fn loom_address_space_map_and_translate() {
    loom::model(|| {
        use smmu::address_space::AddressSpace;
        use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA};

        let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let addr_space1 = Arc::clone(&addr_space);
        let addr_space2 = Arc::clone(&addr_space);

        // Thread 1: Maps the page
        let t1 = thread::spawn(move || {
            let mut guard = addr_space1.lock().unwrap();
            guard
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        });

        // Thread 2: Attempts translation
        let t2 = thread::spawn(move || {
            let guard = addr_space2.lock().unwrap();
            let _ = guard.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Final state should have mapping
        let guard = addr_space.lock().unwrap();
        let result = guard.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
    });
}

#[test]
fn loom_address_space_concurrent_unmap() {
    loom::model(|| {
        use smmu::address_space::AddressSpace;
        use smmu::types::{PagePermissions, SecurityState, IOVA, PA};

        let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        // Initial mapping
        {
            let mut guard = addr_space.lock().unwrap();
            guard
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        }

        let addr_space1 = Arc::clone(&addr_space);
        let addr_space2 = Arc::clone(&addr_space);

        // Both threads try to unmap
        let t1 = thread::spawn(move || {
            let mut guard = addr_space1.lock().unwrap();
            let _ = guard.unmap_page(iova);
        });

        let t2 = thread::spawn(move || {
            let mut guard = addr_space2.lock().unwrap();
            let _ = guard.unmap_page(iova);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Page should be unmapped
        let guard = addr_space.lock().unwrap();
        assert_eq!(guard.get_page_count().unwrap(), 0);
    });
}

// ============================================================================
// Section 4.1.2: StreamContext Concurrency Tests
// ============================================================================

#[test]
fn loom_stream_context_concurrent_pasid_creation() {
    loom::model(|| {
        use smmu::stream_context::StreamContext;
        use smmu::types::PASID;

        let stream_context = Arc::new(StreamContext::new());
        let pasid1 = PASID::new(1).unwrap();
        let pasid2 = PASID::new(2).unwrap();

        let sc1 = Arc::clone(&stream_context);
        let sc2 = Arc::clone(&stream_context);

        let t1 = thread::spawn(move || {
            sc1.create_pasid(pasid1).unwrap();
        });

        let t2 = thread::spawn(move || {
            sc2.create_pasid(pasid2).unwrap();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        assert_eq!(stream_context.pasid_count(), 2);
        assert!(stream_context.has_pasid(pasid1));
        assert!(stream_context.has_pasid(pasid2));
    });
}

#[test]
fn loom_stream_context_duplicate_pasid_race() {
    loom::model(|| {
        use smmu::stream_context::StreamContext;
        use smmu::types::PASID;

        let stream_context = Arc::new(StreamContext::new());
        let pasid = PASID::new(1).unwrap();

        let sc1 = Arc::clone(&stream_context);
        let sc2 = Arc::clone(&stream_context);

        // Both threads try to create the same PASID
        let t1 = thread::spawn(move || {
            let _ = sc1.create_pasid(pasid);
        });

        let t2 = thread::spawn(move || {
            let _ = sc2.create_pasid(pasid);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Exactly one PASID should exist
        assert_eq!(stream_context.pasid_count(), 1);
        assert!(stream_context.has_pasid(pasid));
    });
}

#[test]
fn loom_stream_context_concurrent_translation() {
    loom::model(|| {
        use smmu::stream_context::StreamContext;
        use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA, PASID};

        let stream_context = Arc::new(StreamContext::new());
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        // Setup: Create PASID and map page
        stream_context.create_pasid(pasid).unwrap();
        stream_context
            .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();

        let sc1 = Arc::clone(&stream_context);
        let sc2 = Arc::clone(&stream_context);

        // Concurrent reads
        let t1 = thread::spawn(move || {
            let result = sc1.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            assert!(result.is_ok());
        });

        let t2 = thread::spawn(move || {
            let result = sc2.translate(pasid, iova, AccessType::Write, SecurityState::NonSecure);
            assert!(result.is_ok());
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

#[test]
fn loom_stream_context_create_and_remove_pasid() {
    loom::model(|| {
        use smmu::stream_context::StreamContext;
        use smmu::types::PASID;

        let stream_context = Arc::new(StreamContext::new());
        let pasid = PASID::new(1).unwrap();

        let sc1 = Arc::clone(&stream_context);
        let sc2 = Arc::clone(&stream_context);

        // Thread 1: Creates PASID
        let t1 = thread::spawn(move || {
            let _ = sc1.create_pasid(pasid);
        });

        // Thread 2: Tries to remove PASID
        let t2 = thread::spawn(move || {
            let _ = sc2.remove_pasid(pasid);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // PASID may or may not exist depending on interleaving
        // But count should be 0 or 1
        assert!(stream_context.pasid_count() <= 1);
    });
}

// ============================================================================
// Section 4.1.3: TLB Cache Concurrency Tests
// ============================================================================

#[test]
fn loom_cache_concurrent_insert() {
    loom::model(|| {
        use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
        use smmu::types::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};

        let cache = Arc::new(TlbCache::new(10, ReplacementPolicy::Lru));
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(1).unwrap();
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap();
        let pa1 = PA::new(0x3000).unwrap();
        let pa2 = PA::new(0x4000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova1, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova2, SecurityState::NonSecure);
        let entry1 = CacheEntry::new(iova1, pa1, PagePermissions::read_only(), 0);
        let entry2 = CacheEntry::new(iova2, pa2, PagePermissions::read_write(), 0);

        let cache1 = Arc::clone(&cache);
        let cache2 = Arc::clone(&cache);

        let t1 = thread::spawn(move || {
            cache1.insert(key1, entry1);
        });

        let t2 = thread::spawn(move || {
            cache2.insert(key2, entry2);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Both entries should be in cache
        assert_eq!(cache.len(), 2);
    });
}

#[test]
fn loom_cache_concurrent_lookup() {
    loom::model(|| {
        use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
        use smmu::types::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};

        let cache = Arc::new(TlbCache::new(10, ReplacementPolicy::Lru));
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        // Insert entry
        cache.insert(key, entry);

        let cache1 = Arc::clone(&cache);
        let cache2 = Arc::clone(&cache);

        // Concurrent lookups
        let t1 = thread::spawn(move || {
            let result = cache1.lookup(&key);
            assert!(result.is_some());
        });

        let t2 = thread::spawn(move || {
            let result = cache2.lookup(&key);
            assert!(result.is_some());
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

#[test]
fn loom_cache_insert_and_invalidate() {
    loom::model(|| {
        use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
        use smmu::types::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};

        let cache = Arc::new(TlbCache::new(10, ReplacementPolicy::Lru));
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        let cache1 = Arc::clone(&cache);
        let cache2 = Arc::clone(&cache);

        // Thread 1: Inserts entry
        let t1 = thread::spawn(move || {
            cache1.insert(key, entry);
        });

        // Thread 2: Invalidates
        let t2 = thread::spawn(move || {
            cache2.invalidate_all();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Cache should be empty or have one entry depending on interleaving
        assert!(cache.len() <= 1);
    });
}

// ============================================================================
// Section 4.1.4: SMMU Concurrency Tests
// ============================================================================

#[test]
fn loom_smmu_concurrent_stream_configuration() {
    loom::model(|| {
        use smmu::types::{StreamConfig, StreamID};
        use smmu::SMMU;

        let smmu = Arc::new(SMMU::new());
        let stream_id1 = StreamID::new(1).unwrap();
        let stream_id2 = StreamID::new(2).unwrap();

        let smmu1 = Arc::clone(&smmu);
        let smmu2 = Arc::clone(&smmu);

        let t1 = thread::spawn(move || {
            let _ = smmu1.configure_stream(stream_id1, StreamConfig::default());
        });

        let t2 = thread::spawn(move || {
            let _ = smmu2.configure_stream(stream_id2, StreamConfig::default());
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Both streams should be configured
        assert_eq!(smmu.get_stream_count(), 2);
    });
}

#[test]
fn loom_smmu_concurrent_pasid_creation() {
    loom::model(|| {
        use smmu::types::{StreamConfig, StreamID, PASID};
        use smmu::SMMU;

        let smmu = Arc::new(SMMU::new());
        let stream_id = StreamID::new(1).unwrap();
        let pasid1 = PASID::new(1).unwrap();
        let pasid2 = PASID::new(2).unwrap();

        // Configure stream first
        smmu.configure_stream(stream_id, StreamConfig::default()).unwrap();

        let smmu1 = Arc::clone(&smmu);
        let smmu2 = Arc::clone(&smmu);

        let t1 = thread::spawn(move || {
            smmu1.create_pasid(stream_id, pasid1).unwrap();
        });

        let t2 = thread::spawn(move || {
            smmu2.create_pasid(stream_id, pasid2).unwrap();
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

#[test]
fn loom_smmu_concurrent_translation() {
    loom::model(|| {
        use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
        use smmu::SMMU;

        let smmu = Arc::new(SMMU::new());
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        // Setup
        smmu.configure_stream(stream_id, StreamConfig::default()).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();
        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        let smmu1 = Arc::clone(&smmu);
        let smmu2 = Arc::clone(&smmu);

        // Concurrent translations
        let t1 = thread::spawn(move || {
            let result = smmu1.translate(stream_id, pasid, iova, AccessType::Read);
            assert!(result.is_ok());
        });

        let t2 = thread::spawn(move || {
            let result = smmu2.translate(stream_id, pasid, iova, AccessType::Write);
            assert!(result.is_ok());
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// ============================================================================
// Section 4.1.5: Memory Ordering Tests
// ============================================================================

#[test]
fn loom_memory_ordering_map_before_translate() {
    loom::model(|| {
        use loom::sync::atomic::{AtomicBool, Ordering};
        use smmu::address_space::AddressSpace;
        use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA};

        let addr_space = Arc::new(Mutex::new(AddressSpace::new()));
        let mapped = Arc::new(AtomicBool::new(false));
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let addr_space1 = Arc::clone(&addr_space);
        let mapped1 = Arc::clone(&mapped);

        let addr_space2 = Arc::clone(&addr_space);
        let mapped2 = Arc::clone(&mapped);

        // Thread 1: Maps then signals
        let t1 = thread::spawn(move || {
            let mut guard = addr_space1.lock().unwrap();
            guard
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
            drop(guard);
            mapped1.store(true, Ordering::Release);
        });

        // Thread 2: Waits for signal then translates
        let t2 = thread::spawn(move || {
            while !mapped2.load(Ordering::Acquire) {
                thread::yield_now();
            }
            let guard = addr_space2.lock().unwrap();
            let result = guard.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
            assert!(result.is_ok());
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

#[test]
fn loom_memory_ordering_pasid_creation() {
    loom::model(|| {
        use loom::sync::atomic::{AtomicBool, Ordering};
        use smmu::stream_context::StreamContext;
        use smmu::types::PASID;

        let stream_context = Arc::new(StreamContext::new());
        let created = Arc::new(AtomicBool::new(false));
        let pasid = PASID::new(1).unwrap();

        let sc1 = Arc::clone(&stream_context);
        let created1 = Arc::clone(&created);

        let sc2 = Arc::clone(&stream_context);
        let created2 = Arc::clone(&created);

        // Thread 1: Creates PASID then signals
        let t1 = thread::spawn(move || {
            sc1.create_pasid(pasid).unwrap();
            created1.store(true, Ordering::Release);
        });

        // Thread 2: Waits then checks
        let t2 = thread::spawn(move || {
            while !created2.load(Ordering::Acquire) {
                thread::yield_now();
            }
            assert!(sc2.has_pasid(pasid));
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}
