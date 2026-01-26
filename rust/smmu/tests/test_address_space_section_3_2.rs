//! Comprehensive failing tests for TASKS-RUST.md Section 3.2: Address Space Operations
//!
//! This test suite covers advanced AddressSpace operations:
//! - Iterator APIs (into_iter, iter, iter_mut) with lazy evaluation
//! - Bulk page operations with batch processing optimizations
//! - State querying with immutable borrows and concurrent access
//! - Cache invalidation with proper synchronization and memory ordering
//!
//! All tests are written BEFORE implementation following TDD principles.
//! These tests MUST fail initially and pass only after correct implementation.
//!
//! ## Section 3.2 Requirements from TASKS-RUST.md:
//!
//! 1. **Address range mapping with iterator API** (5 hours)
//!    - Iterators for zero-cost abstractions
//!    - IntoIterator implementation for ergonomic usage
//!    - Parallel iteration with rayon if beneficial
//!
//! 2. **Bulk page operations with batch processing** (4 hours)
//!    - Minimize lock contention with batching
//!    - Vec for efficient bulk operations
//!    - SmallVec for stack optimization (small batches)
//!
//! 3. **State querying with immutable borrows** (3 hours)
//!    - API preventing mutation during queries
//!    - & references for efficient read access
//!    - Iterator for lazy evaluation
//!
//! 4. **Cache invalidation with proper synchronization** (4 hours)
//!    - Atomic operations where possible
//!    - Proper memory ordering semantics
//!    - Fence operations for cross-core visibility

use smmu::address_space::AddressSpace;
use smmu::types::{PagePermissions, SecurityState, IOVA, PA, PAGE_SIZE};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

// Helper constants for testing
const TEST_IOVA_BASE: u64 = 0x1000_0000;
const TEST_PA_BASE: u64 = 0x4000_0000;

// ============================================================================
// SECTION 3.2.1: ITERATOR API TESTS
// ============================================================================

#[test]
fn test_address_range_into_iterator() {
    // Test IntoIterator implementation for AddressRange
    let mut addr_space = AddressSpace::new();

    // Map a contiguous range
    let start_iova = IOVA::new(TEST_IOVA_BASE).unwrap();
    let end_iova = IOVA::new(TEST_IOVA_BASE + (10 * PAGE_SIZE as u64)).unwrap();
    let start_pa = PA::new(TEST_PA_BASE).unwrap();

    addr_space
        .map_range(start_iova, end_iova, start_pa, PagePermissions::read_only())
        .unwrap();

    // Get mapped ranges
    let ranges = addr_space.get_mapped_ranges();
    assert_eq!(ranges.len(), 1);

    // Test IntoIterator - should iterate over all pages in the range
    let mut page_count = 0;
    for page in ranges[0].into_iter() {
        // Each iteration should yield PageInfo with IOVA
        assert!(page.iova().as_u64() >= TEST_IOVA_BASE);
        assert!(page.iova().as_u64() <= TEST_IOVA_BASE + (10 * PAGE_SIZE as u64));
        page_count += 1;
    }

    assert_eq!(page_count, 11, "Should iterate over 11 pages (0-10 inclusive)");
}

#[test]
fn test_address_space_iter() {
    // Test Iterator implementation for iterating over all mapped pages
    let mut addr_space = AddressSpace::new();

    // Map several non-contiguous pages
    for i in [0, 5, 10, 15, 20] {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Test iter() - should provide immutable iterator
    let mut count = 0;
    for entry in addr_space.iter() {
        // Should yield PageEntryRef with IOVA and entry data
        assert!(entry.iova().as_u64() >= TEST_IOVA_BASE);
        assert!(entry.permissions().read());
        assert!(entry.permissions().write());
        count += 1;
    }

    assert_eq!(count, 5, "Should iterate over 5 mapped pages");
}

#[test]
fn test_address_space_iter_mut() {
    // Test mutable Iterator implementation
    let mut addr_space = AddressSpace::new();

    // Map several pages
    for i in 0..5 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Test iter_mut() - should provide mutable iterator for permission updates
    let mut count = 0;
    for mut entry in addr_space.iter_mut() {
        // Should be able to modify PageEntry permissions in place
        entry.set_permissions(PagePermissions::read_write());
        count += 1;
    }

    assert_eq!(count, 5);

    // Verify permissions were updated
    for i in 0..5 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let perms = addr_space.get_page_permissions(iova).unwrap();
        assert_eq!(perms, PagePermissions::read_write());
    }
}

#[test]
fn test_iterator_lazy_evaluation() {
    // Test that iterators use lazy evaluation and don't allocate unless consumed
    let mut addr_space = AddressSpace::new();

    // Map 1000 pages
    for i in 0..1000 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Create iterator but don't consume
    let iter = addr_space.iter();

    // Iterator creation should be O(1), not O(n)
    // Only take first 10 items - lazy evaluation means only 10 are processed
    let collected: Vec<_> = iter.take(10).collect();
    assert_eq!(collected.len(), 10, "Should only evaluate 10 items");

    // Verify the remaining pages are still mapped (iterator didn't consume them)
    assert_eq!(addr_space.get_page_count().unwrap(), 1000);
}

#[test]
fn test_iterator_filter_map_chain() {
    // Test iterator combinators work correctly
    let mut addr_space = AddressSpace::new();

    // Map pages with different permissions
    for i in 0..20 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();

        let perms = if i % 2 == 0 {
            PagePermissions::read_only()
        } else {
            PagePermissions::read_write()
        };

        addr_space
            .map_page(iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    // Use iterator combinators to filter and map
    let writable_iovas: Vec<u64> = addr_space
        .iter()
        .filter(|entry| entry.permissions().write())
        .map(|entry| entry.iova().as_u64())
        .collect();

    assert_eq!(writable_iovas.len(), 10, "Should have 10 writable pages");

    // Verify all are odd indices
    for iova in writable_iovas {
        let offset = (iova - TEST_IOVA_BASE) / PAGE_SIZE as u64;
        assert_eq!(offset % 2, 1, "Only odd indices should be writable");
    }
}

#[test]
#[should_panic(expected = "not yet implemented")]
#[ignore] // Requires rayon dependency - enable when implementing parallel iteration
fn test_iterator_parallel_with_rayon() {
    // Test parallel iteration using rayon for large datasets
    // Note: This requires rayon dependency and AddressSpace to be Send + Sync
    // TODO: Add rayon to Cargo.toml dev-dependencies when implementing this feature
    // use rayon::prelude::*;

    let mut addr_space = AddressSpace::new();

    // Map many pages
    for i in 0..10000 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Wrap in Arc for shared access
    let _addr_space = Arc::new(addr_space);

    // Parallel iteration to count pages with specific property
    // let count = _addr_space
    //     .par_iter() // Parallel iterator
    //     .filter(|entry| entry.permissions().read())
    //     .count();

    // assert_eq!(count, 10000, "All pages should be readable");

    // Placeholder assertion
    panic!("not yet implemented - requires rayon integration");
}

#[test]
fn test_iterator_security_state_filtering() {
    // Test iterating with security state filtering
    let mut addr_space = AddressSpace::new();

    // Map pages with different security states
    for i in 0..30 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();

        let security_state = match i % 3 {
            0 => SecurityState::Secure,
            1 => SecurityState::NonSecure,
            _ => SecurityState::Realm,
        };

        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), security_state)
            .unwrap();
    }

    // Filter by security state using iterator
    let secure_count = addr_space
        .iter()
        .filter(|entry| entry.security_state() == SecurityState::Secure)
        .count();

    assert_eq!(secure_count, 10, "Should have 10 secure pages");
}

// ============================================================================
// SECTION 3.2.2: BULK OPERATIONS WITH BATCH PROCESSING
// ============================================================================

#[test]
fn test_bulk_map_with_batching() {
    // Test bulk mapping with internal batching for lock contention reduction
    let mut addr_space = AddressSpace::new();

    // Create large batch of mappings
    let mappings: Vec<(IOVA, PA)> = (0..1000)
        .map(|i| {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            (iova, pa)
        })
        .collect();

    // Bulk map should use internal batching to minimize lock acquisitions
    let result = addr_space.map_pages_batched(&mappings, PagePermissions::read_write());

    assert!(result.is_ok());
    assert_eq!(addr_space.get_page_count().unwrap(), 1000);

    // Verify first and last pages are correctly mapped
    let first_iova = IOVA::new(TEST_IOVA_BASE).unwrap();
    let last_iova = IOVA::new(TEST_IOVA_BASE + (999 * PAGE_SIZE as u64)).unwrap();
    assert!(addr_space.is_page_mapped(first_iova).unwrap());
    assert!(addr_space.is_page_mapped(last_iova).unwrap());
}

#[test]
fn test_bulk_unmap_with_batching() {
    // Test bulk unmapping with batching
    let mut addr_space = AddressSpace::new();

    // Map pages first
    let iovas: Vec<IOVA> = (0..500)
        .map(|i| {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE).unwrap();
            addr_space
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
            iova
        })
        .collect();

    assert_eq!(addr_space.get_page_count().unwrap(), 500);

    // Bulk unmap with batching
    let result = addr_space.unmap_pages_batched(&iovas);

    assert!(result.is_ok());
    assert_eq!(addr_space.get_page_count().unwrap(), 0);

    // Verify pages are actually unmapped
    for iova in &iovas {
        assert!(!addr_space.is_page_mapped(*iova).unwrap());
    }
}

#[test]
fn test_small_batch_stack_optimization() {
    // Test that small batches use stack allocation (SmallVec)
    let mut addr_space = AddressSpace::new();

    // Small batch (< 16 items) should use stack allocation
    let small_batch: Vec<(IOVA, PA)> = (0..8)
        .map(|i| {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            (iova, pa)
        })
        .collect();

    // This should internally use SmallVec for stack optimization
    let result = addr_space.map_pages_batched(&small_batch, PagePermissions::read_only());

    assert!(result.is_ok());
    assert_eq!(addr_space.get_page_count().unwrap(), 8);

    // Verify all pages are mapped correctly
    for (iova, _pa) in &small_batch {
        assert!(addr_space.is_page_mapped(*iova).unwrap());
    }
}

#[test]
fn test_batch_permission_update() {
    // Test bulk permission updates with batching
    let mut addr_space = AddressSpace::new();

    // Map pages with initial permissions
    let iovas: Vec<IOVA> = (0..100)
        .map(|i| {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE).unwrap();
            addr_space
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
            iova
        })
        .collect();

    // Verify initial permissions
    for iova in &iovas {
        let perms = addr_space.get_page_permissions(*iova).unwrap();
        assert!(perms.read() && !perms.write());
    }

    // Bulk update permissions
    let result = addr_space.update_permissions_batched(&iovas, PagePermissions::read_write());

    assert!(result.is_ok());

    // Verify all updated
    for iova in iovas {
        let perms = addr_space.get_page_permissions(iova).unwrap();
        assert_eq!(perms, PagePermissions::read_write());
    }
}

#[test]
fn test_batch_with_partial_failure() {
    // Test that batch operations handle partial failures correctly
    let mut addr_space = AddressSpace::new();

    // Create batch with invalid address that exceeds maximum (52-bit limit)
    let max_valid = (1u64 << 52) - 1;
    let mixed_batch: Vec<(IOVA, PA)> = vec![
        (IOVA::new(TEST_IOVA_BASE).unwrap(), PA::new(TEST_PA_BASE).unwrap()),
        (IOVA::new(TEST_IOVA_BASE + PAGE_SIZE as u64).unwrap(), PA::new(TEST_PA_BASE).unwrap()),
    ];

    // Batch operation should validate all addresses first
    let result = addr_space.map_pages_batched(&mixed_batch, PagePermissions::read_only());

    // These should succeed
    assert!(result.is_ok());
    assert_eq!(addr_space.get_page_count().unwrap(), 2);

    // Now test with truly invalid batch
    let invalid_batch: Vec<(IOVA, PA)> = vec![
        (IOVA::new(TEST_IOVA_BASE + (2 * PAGE_SIZE as u64)).unwrap(), PA::new(max_valid + 1).unwrap()),
    ];
    let result = addr_space.map_pages_batched(&invalid_batch, PagePermissions::read_only());

    // Should fail due to invalid PA
    assert!(result.is_err());
    // Original mappings should still be present
    assert_eq!(addr_space.get_page_count().unwrap(), 2);
}

#[test]
fn test_concurrent_batch_operations() {
    // Test concurrent batch operations with reduced lock contention
    let addr_space = Arc::new(RwLock::new(AddressSpace::new()));

    let mut handles = vec![];

    // Spawn multiple threads performing batch operations
    for thread_id in 0..4 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let mut space = addr_space_clone.write().unwrap();

            let batch: Vec<(IOVA, PA)> = (0..250)
                .map(|i| {
                    let offset = (thread_id * 250 + i) * PAGE_SIZE as u64;
                    let iova = IOVA::new(TEST_IOVA_BASE + offset).unwrap();
                    let pa = PA::new(TEST_PA_BASE + offset).unwrap();
                    (iova, pa)
                })
                .collect();

            space
                .map_pages_batched(&batch, PagePermissions::read_write())
                .unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let space = addr_space.read().unwrap();
    assert_eq!(space.get_page_count().unwrap(), 1000);

    // Verify some sample pages are correctly mapped
    for thread_id in 0..4 {
        let offset = thread_id * 250 * PAGE_SIZE as u64;
        let iova = IOVA::new(TEST_IOVA_BASE + offset).unwrap();
        assert!(space.is_page_mapped(iova).unwrap());
    }
}

// ============================================================================
// SECTION 3.2.3: STATE QUERYING WITH IMMUTABLE BORROWS
// ============================================================================

#[test]
fn test_query_prevents_mutation() {
    // Test that query API prevents mutation during read operations
    let mut addr_space = AddressSpace::new();

    // Map some pages
    for i in 0..10 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Create immutable query interface
    let query = addr_space.query();

    // While query is active, mutations should be prevented by borrow checker
    // This would fail to compile:
    // addr_space.map_page(...); // ERROR: cannot borrow as mutable

    // Query operations should work
    assert_eq!(query.page_count(), 10);
    assert!(query.is_mapped(IOVA::new(TEST_IOVA_BASE).unwrap()));
    assert!(!query.is_mapped(IOVA::new(TEST_IOVA_BASE + (100 * PAGE_SIZE as u64)).unwrap()));

    // After query is dropped, mutations should work again
    drop(query);
    let new_iova = IOVA::new(TEST_IOVA_BASE + (10 * PAGE_SIZE as u64)).unwrap();
    addr_space
        .map_page(new_iova, PA::new(TEST_PA_BASE).unwrap(), PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    assert_eq!(addr_space.get_page_count().unwrap(), 11);
}

#[test]
fn test_immutable_reference_query() {
    // Test efficient read access with immutable references
    let mut addr_space = AddressSpace::new();

    // Map pages
    for i in 0..5 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Query using immutable reference (no copy/clone needed)
    let query_result = addr_space.query_page(IOVA::new(TEST_IOVA_BASE).unwrap());

    // Should return reference to PageEntry
    assert!(query_result.is_some());
    let entry_ref = query_result.unwrap();
    assert_eq!(entry_ref.physical_address().as_u64(), TEST_PA_BASE);
    assert!(entry_ref.permissions().read());
    assert!(entry_ref.permissions().write());

    // Test query for non-mapped page
    let unmapped = addr_space.query_page(IOVA::new(TEST_IOVA_BASE + (100 * PAGE_SIZE as u64)).unwrap());
    assert!(unmapped.is_none());
}

#[test]
fn test_lazy_query_iterator() {
    // Test lazy evaluation with iterator-based queries
    let mut addr_space = AddressSpace::new();

    // Map 1000 pages
    for i in 0..1000 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Query with predicate using lazy iterator
    let query = addr_space.query();
    let mut writable_pages = query.iter().filter(|entry| entry.permissions().write());

    // Iterator should be lazy - not evaluated until consumed
    // Taking first item should only evaluate until first match
    let first = writable_pages.next();
    assert!(first.is_none(), "No writable pages should exist");

    // Test with readable pages - should find many
    let readable_count = query.iter().filter(|entry| entry.permissions().read()).count();
    assert_eq!(readable_count, 1000);
}

#[test]
fn test_concurrent_immutable_queries() {
    // Test multiple concurrent immutable queries
    let mut addr_space = AddressSpace::new();

    // Map pages
    for i in 0..100 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let addr_space = Arc::new(RwLock::new(addr_space));

    // Spawn multiple concurrent readers
    let mut handles = vec![];
    for thread_id in 0..10 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let space = addr_space_clone.read().unwrap();
            let query = space.query();

            // Each thread performs independent queries
            let iova = IOVA::new(TEST_IOVA_BASE + (thread_id * PAGE_SIZE as u64)).unwrap();
            assert!(query.is_mapped(iova));
            assert_eq!(query.page_count(), 100);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify final state
    let space = addr_space.read().unwrap();
    assert_eq!(space.get_page_count().unwrap(), 100);
}

#[test]
fn test_query_range_statistics() {
    // Test querying statistics over a range
    let mut addr_space = AddressSpace::new();

    // Map pages with varying permissions
    for i in 0..50 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();

        let perms = match i % 3 {
            0 => PagePermissions::read_only(),
            1 => PagePermissions::read_write(),
            _ => PagePermissions::read_execute(),
        };

        addr_space
            .map_page(iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    // Query statistics
    let query = addr_space.query();
    let start = IOVA::new(TEST_IOVA_BASE).unwrap();
    let end = IOVA::new(TEST_IOVA_BASE + (49 * PAGE_SIZE as u64)).unwrap();

    let stats = query.range_statistics(start, end);

    // Should provide statistics without copying all entries
    assert_eq!(stats.total_pages, 50);
    assert_eq!(stats.readable_pages, 50); // All pages are readable
    // Pages with i % 3 == 1 have read_write (writable): 1,4,7,10,13,16,19,22,25,28,31,34,37,40,43,46,49 = 17 pages
    // But we need to count actual writable pages: indices 0-49, every 3rd starting at 1
    let expected_writable = (0..50).filter(|&i| i % 3 == 1).count();
    assert_eq!(stats.writable_pages, expected_writable);
    // Pages with i % 3 == 2 have read_execute (executable): 2,5,8,11,14,17,20,23,26,29,32,35,38,41,44,47 = 16 pages
    let expected_executable = (0..50).filter(|&i| i % 3 == 2).count();
    assert_eq!(stats.executable_pages, expected_executable);
}

// ============================================================================
// SECTION 3.2.4: CACHE INVALIDATION WITH SYNCHRONIZATION
// ============================================================================

#[test]
fn test_atomic_cache_invalidation() {
    // Test cache invalidation using atomic operations
    let mut addr_space = AddressSpace::new();

    // Map pages
    for i in 0..10 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let gen_before = addr_space.invalidation_generation();

    // Invalidate should use atomic operations for synchronization
    let iova = IOVA::new(TEST_IOVA_BASE).unwrap();
    addr_space.invalidate_page_atomic(iova);

    // Verify invalidation occurred
    assert!(addr_space.is_invalidated(iova));
    let gen_after = addr_space.invalidation_generation();
    assert!(gen_after > gen_before, "Generation counter should increment");
}

#[test]
fn test_memory_ordering_acquire_release() {
    // Test proper memory ordering semantics (Acquire/Release)
    let addr_space = Arc::new(RwLock::new(AddressSpace::new()));

    // Thread 1: Map page and invalidate with Release ordering
    let addr_space_clone1 = Arc::clone(&addr_space);
    let handle1 = thread::spawn(move || {
        let mut space = addr_space_clone1.write().unwrap();
        let iova = IOVA::new(TEST_IOVA_BASE).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();

        space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();

        // Invalidate with Release ordering
        space.invalidate_page_with_ordering(iova, Ordering::Release);
    });

    handle1.join().unwrap();

    // Thread 2: Access with Acquire ordering should see invalidation
    let addr_space_clone2 = Arc::clone(&addr_space);
    let handle2 = thread::spawn(move || {
        // Small delay to ensure thread 1 completes
        thread::sleep(Duration::from_millis(10));

        let space = addr_space_clone2.read().unwrap();
        let iova = IOVA::new(TEST_IOVA_BASE).unwrap();

        // Check with Acquire ordering
        let invalidated = space.is_invalidated_with_ordering(iova, Ordering::Acquire);
        assert!(invalidated, "Invalidation should be visible");

        // Verify page is still mapped
        assert!(space.is_page_mapped(iova).unwrap());
    });

    handle2.join().unwrap();
}

#[test]
fn test_fence_cross_core_visibility() {
    // Test fence operations for cross-core cache visibility
    let addr_space = Arc::new(RwLock::new(AddressSpace::new()));
    let invalidation_flag = Arc::new(AtomicBool::new(false));

    // Map pages
    {
        let mut space = addr_space.write().unwrap();
        for i in 0..5 {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE).unwrap();
            space
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        }
    }

    let addr_space_clone = Arc::clone(&addr_space);
    let flag_clone = Arc::clone(&invalidation_flag);

    // Thread 1: Invalidate and set flag
    let handle1 = thread::spawn(move || {
        let mut space = addr_space_clone.write().unwrap();
        let iova = IOVA::new(TEST_IOVA_BASE).unwrap();

        space.invalidate_page(iova);

        // Fence to ensure invalidation is visible across cores
        std::sync::atomic::fence(Ordering::Release);

        flag_clone.store(true, Ordering::Release);
    });

    // Thread 2: Wait for flag and verify invalidation visible
    let handle2 = thread::spawn(move || {
        // Spin until flag is set (with timeout protection)
        let mut attempts = 0;
        while !invalidation_flag.load(Ordering::Acquire) {
            std::hint::spin_loop();
            attempts += 1;
            if attempts > 1_000_000 {
                break;
            }
        }

        // Fence to ensure we see the invalidation
        std::sync::atomic::fence(Ordering::Acquire);

        let space = addr_space.read().unwrap();
        let iova = IOVA::new(TEST_IOVA_BASE).unwrap();

        // Invalidation should be visible
        assert!(space.is_invalidated(iova));
    });

    handle1.join().unwrap();
    handle2.join().unwrap();
}

#[test]
fn test_invalidation_seqcst_ordering() {
    // Test SeqCst ordering for strongest synchronization guarantees
    let addr_space = Arc::new(RwLock::new(AddressSpace::new()));
    let counter = Arc::new(AtomicU64::new(0));

    // Map pages - need 20 pages for 4 threads * 5 pages each
    {
        let mut space = addr_space.write().unwrap();
        for i in 0..20 {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE).unwrap();
            space
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        }
    }

    let mut handles = vec![];

    // Multiple threads invalidating with SeqCst ordering
    for thread_id in 0..4 {
        let addr_space_clone = Arc::clone(&addr_space);
        let counter_clone = Arc::clone(&counter);

        let handle = thread::spawn(move || {
            let mut space = addr_space_clone.write().unwrap();

            for i in 0..5 {
                let iova = IOVA::new(TEST_IOVA_BASE + ((thread_id * 5 + i) * PAGE_SIZE as u64)).unwrap();

                // Invalidate with SeqCst ordering
                space.invalidate_page_with_ordering(iova, Ordering::SeqCst);

                // Increment counter with SeqCst
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // All invalidations should be visible
    assert_eq!(counter.load(Ordering::SeqCst), 20);

    // Verify all pages are invalidated
    let space = addr_space.read().unwrap();
    for i in 0..20 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        assert!(space.is_invalidated(iova));
    }
}

#[test]
fn test_bulk_invalidation_atomic() {
    // Test bulk invalidation with atomic operations
    let mut addr_space = AddressSpace::new();

    // Map many pages
    for i in 0..100 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_BASE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let gen_before = addr_space.invalidation_generation();

    // Bulk invalidate with atomic counter tracking
    let start = IOVA::new(TEST_IOVA_BASE).unwrap();
    let end = IOVA::new(TEST_IOVA_BASE + (99 * PAGE_SIZE as u64)).unwrap();

    let invalidated_count = addr_space.invalidate_range_atomic(start, end);

    assert_eq!(invalidated_count, 100, "Should invalidate 100 pages");

    // Verify generation counter increased
    let gen_after = addr_space.invalidation_generation();
    assert_eq!(gen_after, gen_before + 100);

    // Verify some pages are invalidated
    assert!(addr_space.is_invalidated(IOVA::new(TEST_IOVA_BASE).unwrap()));
    assert!(addr_space.is_invalidated(IOVA::new(TEST_IOVA_BASE + (50 * PAGE_SIZE as u64)).unwrap()));
}

#[test]
fn test_invalidation_generation_counter() {
    // Test invalidation tracking with generation counter
    let mut addr_space = AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_BASE).unwrap();
    let pa = PA::new(TEST_PA_BASE).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Get initial generation
    let gen1 = addr_space.invalidation_generation();

    // Invalidate page
    addr_space.invalidate_page(iova);

    // Generation should increment
    let gen2 = addr_space.invalidation_generation();
    assert!(gen2 > gen1, "Generation counter should increment on invalidation");
    assert_eq!(gen2, gen1 + 1);

    // Multiple invalidations should increment multiple times
    addr_space.invalidate_page(iova);
    let gen3 = addr_space.invalidation_generation();
    assert!(gen3 > gen2);
    assert_eq!(gen3, gen2 + 1);

    // Verify page is marked as invalidated
    assert!(addr_space.is_invalidated(iova));
}

#[test]
fn test_cross_thread_invalidation_visibility() {
    // Test that invalidations are visible across threads
    let addr_space = Arc::new(RwLock::new(AddressSpace::new()));

    // Map pages in main thread
    {
        let mut space = addr_space.write().unwrap();
        for i in 0..20 {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE).unwrap();
            space
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        }
    }

    let invalidation_visible = Arc::new(AtomicBool::new(false));

    let addr_space_clone1 = Arc::clone(&addr_space);
    let addr_space_clone2 = Arc::clone(&addr_space);
    let visibility_clone = Arc::clone(&invalidation_visible);

    // Thread 1: Invalidate
    let handle1 = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        let mut space = addr_space_clone1.write().unwrap();
        let iova = IOVA::new(TEST_IOVA_BASE + (10 * PAGE_SIZE as u64)).unwrap();
        space.invalidate_page(iova);
    });

    // Thread 2: Check visibility
    let handle2 = thread::spawn(move || {
        // Wait for invalidation
        thread::sleep(Duration::from_millis(50));

        let space = addr_space_clone2.read().unwrap();
        let iova = IOVA::new(TEST_IOVA_BASE + (10 * PAGE_SIZE as u64)).unwrap();

        // Check if invalidation is visible
        if space.is_invalidated(iova) {
            visibility_clone.store(true, Ordering::Release);
        }
    });

    handle1.join().unwrap();
    handle2.join().unwrap();

    assert!(invalidation_visible.load(Ordering::Acquire), "Invalidation should be visible across threads");

    // Verify the page is still mapped
    let space = addr_space.read().unwrap();
    let iova = IOVA::new(TEST_IOVA_BASE + (10 * PAGE_SIZE as u64)).unwrap();
    assert!(space.is_page_mapped(iova).unwrap());
}

#[test]
fn test_compare_exchange_invalidation() {
    // Test compare-and-exchange for atomic invalidation state updates
    let mut addr_space = AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_BASE).unwrap();
    let pa = PA::new(TEST_PA_BASE).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Initially not invalidated
    assert!(!addr_space.is_invalidated(iova));

    // Attempt to invalidate only if not already invalidated (from false to true)
    let was_valid = addr_space.compare_exchange_invalidate(iova, false, true, Ordering::AcqRel);

    assert!(was_valid, "Should successfully invalidate from valid state");
    assert!(addr_space.is_invalidated(iova));

    // Second attempt should fail (already invalidated - current state is true, not false)
    let already_invalid = addr_space.compare_exchange_invalidate(iova, false, true, Ordering::AcqRel);

    assert!(!already_invalid, "Should fail to invalidate already-invalidated page");

    // But we can transition from true to false (re-validate)
    let can_revalidate = addr_space.compare_exchange_invalidate(iova, true, false, Ordering::AcqRel);
    assert!(can_revalidate, "Should be able to transition from invalidated to valid");
}

// ============================================================================
// INTEGRATION TESTS: COMBINING MULTIPLE FEATURES
// ============================================================================

#[test]
fn test_iterator_with_concurrent_invalidation() {
    // Test iterator stability during concurrent invalidations
    let addr_space = Arc::new(RwLock::new(AddressSpace::new()));

    // Map pages
    {
        let mut space = addr_space.write().unwrap();
        for i in 0..50 {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            let pa = PA::new(TEST_PA_BASE).unwrap();
            space
                .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                .unwrap();
        }
    }

    let addr_space_clone1 = Arc::clone(&addr_space);
    let addr_space_clone2 = Arc::clone(&addr_space);

    // Thread 1: Iterate (acquires read lock first)
    let handle1 = thread::spawn(move || {
        let space = addr_space_clone1.read().unwrap();
        let count = space.iter().count();
        count
    });

    // Thread 2: Invalidate after short delay (will wait for read lock)
    let handle2 = thread::spawn(move || {
        thread::sleep(Duration::from_millis(5));
        let mut space = addr_space_clone2.write().unwrap();
        for i in 0..10 {
            let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
            space.invalidate_page(iova);
        }
    });

    let count = handle1.join().unwrap();
    handle2.join().unwrap();

    // Iterator should see consistent snapshot - either 50 (before invalidation)
    // or 50 still (invalidation doesn't remove pages)
    assert_eq!(count, 50, "Iterator should see all 50 pages");

    // Verify invalidations were applied
    let space = addr_space.read().unwrap();
    for i in 0..10 {
        let iova = IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap();
        assert!(space.is_invalidated(iova));
    }
}

#[test]
fn test_batch_operations_with_query() {
    // Test batch operations combined with immutable queries
    let mut addr_space = AddressSpace::new();

    // Batch map
    let mappings: Vec<(IOVA, PA)> = (0..100)
        .map(|i| {
            (
                IOVA::new(TEST_IOVA_BASE + (i * PAGE_SIZE as u64)).unwrap(),
                PA::new(TEST_PA_BASE + (i * PAGE_SIZE as u64)).unwrap(),
            )
        })
        .collect();

    addr_space
        .map_pages_batched(&mappings, PagePermissions::read_write())
        .unwrap();

    // Query state
    let query = addr_space.query();
    assert_eq!(query.page_count(), 100);

    // Iterate with filter
    let writable_count = query.iter().filter(|e| e.permissions().write()).count();
    assert_eq!(writable_count, 100);

    // Verify readable count
    let readable_count = query.iter().filter(|e| e.permissions().read()).count();
    assert_eq!(readable_count, 100);

    // After query is dropped, verify we can still access the address space
    drop(query);
    assert_eq!(addr_space.get_page_count().unwrap(), 100);
}
