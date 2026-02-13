#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::cast_precision_loss)]

//! Unit tests for performance optimizations
//!
//! Tests algorithmic complexity, hash function performance, memory access
//! patterns, and scalability per ARM SMMU v3 Section 7.2.

use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA, PAGE_SIZE, PASID};
use smmu::SMMU;
use std::time::Instant;

// ============================================================================
// Hash Function Performance Tests
// ============================================================================

#[test]
fn test_hash_function_distribution() {
    let addr_space = AddressSpace::new();

    // Map 1000 pages with sequential addresses
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 1000);

    // All translations should be O(1) average case
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
    }
}

#[test]
fn test_hash_function_sparse_addresses() {
    let addr_space = AddressSpace::new();

    // Map pages at very sparse addresses (good distribution test)
    let sparse_addrs = [0x1000, 0x1000_0000, 0x1_0000_0000, 0x10_0000_0000, 0x100_0000_0000];

    for &addr in &sparse_addrs {
        let iova = IOVA::new(addr).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), sparse_addrs.len());
}

// ============================================================================
// Algorithm Complexity Tests
// ============================================================================

#[test]
fn test_o1_lookup_complexity() {
    let addr_space = AddressSpace::new();

    // Map 10,000 pages
    for i in 0..10_000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Measure lookup time for first and last entries
    let iova_first = IOVA::new(0x1000).unwrap();
    let iova_last = IOVA::new(0x1000 + 9999 * PAGE_SIZE).unwrap();

    let start = Instant::now();
    let _ = addr_space.translate_page(iova_first, AccessType::Read, SecurityState::NonSecure);
    let time_first = start.elapsed();

    let start = Instant::now();
    let _ = addr_space.translate_page(iova_last, AccessType::Read, SecurityState::NonSecure);
    let time_last = start.elapsed();

    // Times should be similar (within 10x) for O(1) operations
    let ratio = time_last.as_nanos() as f64 / time_first.as_nanos().max(1) as f64;
    assert!(ratio < 10.0, "Lookup time ratio too large: {ratio}");
}

#[test]
fn test_scalability_10_to_10000() {
    // Test at different scales
    for count in &[10, 100, 1000, 10_000] {
        let addr_space = AddressSpace::new();

        let start = Instant::now();
        for i in 0..*count {
            let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
            let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
            addr_space
                .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                .unwrap();
        }
        let map_time = start.elapsed();

        let start = Instant::now();
        for i in 0..*count {
            let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
            let _ = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
        }
        let translate_time = start.elapsed();

        println!("Scale {count}: map={map_time:?}, translate={translate_time:?}");

        assert_eq!(addr_space.get_page_count().unwrap(), *count as usize);
    }
}

// ============================================================================
// Memory Access Pattern Tests
// ============================================================================

#[test]
fn test_sequential_access_pattern() {
    let addr_space = AddressSpace::new();

    // Map 1000 sequential pages
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Access sequentially (cache-friendly)
    let start = Instant::now();
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let _ = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    }
    let sequential_time = start.elapsed();

    println!("Sequential access time: {sequential_time:?}");
}

#[test]
fn test_random_access_pattern() {
    let addr_space = AddressSpace::new();

    // Map 1000 pages
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Access in pseudo-random order
    let start = Instant::now();
    for i in 0..1000 {
        let index = (i * 317) % 1000; // Simple pseudo-random
        let iova = IOVA::new(0x1000 + index * PAGE_SIZE).unwrap();
        let _ = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    }
    let random_time = start.elapsed();

    println!("Random access time: {random_time:?}");
}

// ============================================================================
// StreamContext Performance Tests
// ============================================================================

#[test]
fn test_pasid_lookup_performance() {
    let stream_context = StreamContext::new();

    // Create 100 PASIDs
    for i in 0..100 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();
    }

    // Measure lookup time
    let pasid = PASID::new(50).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        assert!(stream_context.has_pasid(pasid));
    }
    let lookup_time = start.elapsed();

    println!("PASID lookup time (1000 ops): {lookup_time:?}");
}

#[test]
fn test_multi_pasid_translation_performance() {
    let stream_context = StreamContext::new();

    // Create 10 PASIDs with mappings
    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000 + u64::from(i) * 0x1000).unwrap();
        stream_context
            .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Measure translation time across different PASIDs
    let start = Instant::now();
    for i in 0..1000 {
        let pasid = PASID::new((i % 10) as u32).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let _ = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }
    let translate_time = start.elapsed();

    println!("Multi-PASID translation time (1000 ops): {translate_time:?}");
}

// ============================================================================
// SMMU Controller Performance Tests
// ============================================================================

#[test]
fn test_multi_stream_performance() {
    let smmu = SMMU::new();

    // Configure 10 streams with PASIDs
    for i in 0..10 {
        let stream_id = smmu::types::StreamID::new(i).unwrap();
        smmu.configure_stream(stream_id, smmu::types::StreamConfig::default()).unwrap();

        let pasid = PASID::new(1).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000 + u64::from(i) * 0x1000).unwrap();
        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    // Measure translation time across different streams
    let start = Instant::now();
    for i in 0..1000 {
        let stream_id = smmu::types::StreamID::new((i % 10) as u32).unwrap();
        let pasid = PASID::new(1).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    }
    let translate_time = start.elapsed();

    println!("Multi-stream translation time (1000 ops): {translate_time:?}");
}

// ============================================================================
// Performance Regression Prevention Tests
// ============================================================================

#[test]
fn test_translation_latency_target() {
    let addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Measure average translation time
    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    }
    let total_time = start.elapsed();
    let avg_time_ns = total_time.as_nanos() / iterations;

    println!("Average translation time: {avg_time_ns} ns");

    // Target: sub-microsecond (< 1000 ns)
    // Relaxed for debug builds
    #[cfg(not(debug_assertions))]
    assert!(avg_time_ns < 1000, "Translation too slow: {} ns", avg_time_ns);
}

#[test]
fn test_mapping_throughput() {
    let addr_space = AddressSpace::new();

    let iterations = 10_000;
    let start = Instant::now();
    for i in 0..iterations {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }
    let total_time = start.elapsed();
    let throughput = iterations as f64 / total_time.as_secs_f64();

    println!("Mapping throughput: {throughput:.0} ops/sec");

    // Should handle at least 1M mappings/sec
    #[cfg(not(debug_assertions))]
    assert!(throughput > 1_000_000.0, "Mapping too slow: {:.0} ops/sec", throughput);
}

#[test]
fn test_batch_operation_efficiency() {
    let addr_space = AddressSpace::new();

    // Create batch of mappings
    let count = 1000;
    let mappings: Vec<(IOVA, PA)> = (0..count)
        .map(|i| {
            let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
            let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
            (iova, pa)
        })
        .collect();

    // Measure batch operation time
    let start = Instant::now();
    addr_space.map_pages(&mappings, PagePermissions::read_write()).unwrap();
    let batch_time = start.elapsed();

    println!("Batch mapping time (1000 pages): {batch_time:?}");

    assert_eq!(addr_space.get_page_count().unwrap(), count as usize);
}
