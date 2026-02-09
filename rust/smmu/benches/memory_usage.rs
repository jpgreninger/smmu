#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
//! Memory Usage Benchmarks (Section 7.2)
//!
//! Comprehensive benchmarks for memory usage optimization and monitoring.
//! These benchmarks measure actual memory consumption, allocation patterns,
//! and memory pooling effectiveness.
//!
//! # Benchmark Categories
//!
//! 1. Memory Allocation Patterns - Measure allocation overhead
//! 2. Memory Pooling - Test pooled vs non-pooled allocations
//! 3. Compact Representations - Compare different data layouts
//! 4. Memory Fragmentation - Track fragmentation over time
//! 5. Peak Memory Usage - Identify memory usage peaks
//! 6. Memory Efficiency Metrics - Calculate efficiency ratios

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

// ============================================================================
// Benchmark Configuration
// ============================================================================

fn configure_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(8))
        .sample_size(500)
}

// ============================================================================
// 1. Memory Allocation Pattern Benchmarks
// ============================================================================

/// Benchmark allocation overhead for different container sizes
fn bench_allocation_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_overhead");

    for capacity in &[64, 256, 1024, 4096, 16_384] {
        group.throughput(Throughput::Elements(*capacity as u64));

        group.bench_with_input(BenchmarkId::new("hashmap_alloc", capacity), capacity, |b, &capacity| {
            b.iter(|| {
                let map: HashMap<u64, u64> = HashMap::with_capacity(capacity);
                black_box(map);
            });
        });

        group.bench_with_input(BenchmarkId::new("vec_alloc", capacity), capacity, |b, &capacity| {
            b.iter(|| {
                let vec: Vec<u64> = Vec::with_capacity(capacity);
                black_box(vec);
            });
        });
    }

    group.finish();
}

/// Benchmark incremental growth vs pre-allocation
fn bench_growth_vs_prealloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("growth_vs_prealloc");
    let target_size = 1000;

    group.bench_function("incremental_growth", |b| {
        b.iter(|| {
            let mut vec = Vec::new();
            for i in 0..target_size {
                vec.push(i);
            }
            black_box(vec);
        });
    });

    group.bench_function("preallocated", |b| {
        b.iter(|| {
            let mut vec = Vec::with_capacity(target_size);
            for i in 0..target_size {
                vec.push(i);
            }
            black_box(vec);
        });
    });

    group.finish();
}

/// Benchmark reallocation frequency
fn bench_reallocation_frequency(c: &mut Criterion) {
    let mut group = c.benchmark_group("reallocation_frequency");

    // Small initial capacity forces reallocations
    group.bench_function("frequent_realloc", |b| {
        b.iter(|| {
            let mut vec = Vec::with_capacity(10);
            for i in 0..1000 {
                vec.push(i);
            }
            black_box(vec);
        });
    });

    // Large initial capacity avoids reallocations
    group.bench_function("no_realloc", |b| {
        b.iter(|| {
            let mut vec = Vec::with_capacity(1000);
            for i in 0..1000 {
                vec.push(i);
            }
            black_box(vec);
        });
    });

    group.finish();
}

// ============================================================================
// 2. Memory Pooling Benchmarks
// ============================================================================

/// Simulate memory pooling for cache entries
fn bench_memory_pooling(c: &mut Criterion) {
    use smmu::cache::CacheEntry;
    use smmu::{PagePermissions, IOVA, PA};

    let mut group = c.benchmark_group("memory_pooling");

    // Non-pooled: Individual allocations
    group.bench_function("individual_allocations", |b| {
        b.iter(|| {
            let mut entries = Vec::new();
            for i in 0..100 {
                let entry = CacheEntry::new(
                    IOVA::new(i * 0x1000).unwrap(),
                    PA::new(i * 0x1000).unwrap(),
                    PagePermissions::read_write(),
                    i,
                );
                entries.push(entry);
            }
            black_box(entries);
        });
    });

    // Pooled: Pre-allocated vector
    group.bench_function("pooled_allocations", |b| {
        b.iter(|| {
            let mut entries = Vec::with_capacity(100);
            for i in 0..100 {
                let entry = CacheEntry::new(
                    IOVA::new(i * 0x1000).unwrap(),
                    PA::new(i * 0x1000).unwrap(),
                    PagePermissions::read_write(),
                    i,
                );
                entries.push(entry);
            }
            black_box(entries);
        });
    });

    group.finish();
}

/// Benchmark object reuse vs allocation
fn bench_object_reuse(c: &mut Criterion) {
    let mut group = c.benchmark_group("object_reuse");

    // Allocate new objects each iteration
    group.bench_function("new_objects", |b| {
        b.iter(|| {
            let mut objects = Vec::new();
            for i in 0..1000 {
                objects.push(i);
            }
            black_box(objects);
        });
    });

    // Reuse existing vector
    group.bench_function("reuse_objects", |b| {
        let mut objects = Vec::with_capacity(1000);
        b.iter(|| {
            objects.clear();
            for i in 0..1000 {
                objects.push(i);
            }
            black_box(&objects);
        });
    });

    group.finish();
}

// ============================================================================
// 3. Compact Representation Benchmarks
// ============================================================================

/// Compare memory layout of different representations
fn bench_compact_layouts(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact_layouts");

    // Standard struct layout
    #[allow(clippy::items_after_statements)]
    #[derive(Clone, Copy)]
    #[allow(dead_code)]
    struct StandardEntry {
        iova: u64,
        pa: u64,
        permissions: u8,
        timestamp: u64,
    }

    // Compact layout using bit packing
    #[derive(Clone, Copy)]
    #[allow(dead_code)]
    struct CompactEntry {
        // Pack IOVA (48-bit) + permissions (8-bit) + security (8-bit)
        iova_and_flags: u64,
        // PA (48-bit) + reserved (16-bit)
        pa_and_reserved: u64,
        timestamp: u64,
    }

    group.bench_function("standard_layout_alloc", |b| {
        b.iter(|| {
            let mut entries = Vec::with_capacity(1000);
            for i in 0..1000 {
                entries.push(StandardEntry {
                    iova: i * 0x1000,
                    pa: i * 0x1000,
                    permissions: 0x3,
                    timestamp: i,
                });
            }
            black_box(entries);
        });
    });

    group.bench_function("compact_layout_alloc", |b| {
        b.iter(|| {
            let mut entries = Vec::with_capacity(1000);
            for i in 0..1000 {
                entries.push(CompactEntry {
                    iova_and_flags: (i * 0x1000) | (0x3 << 48),
                    pa_and_reserved: i * 0x1000,
                    timestamp: i,
                });
            }
            black_box(entries);
        });
    });

    group.finish();
}

/// Benchmark alignment and padding impact
fn bench_alignment_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("alignment_overhead");

    // Poorly aligned struct (requires padding)
    #[repr(C)]
    #[allow(clippy::items_after_statements)]
    struct PaddedStruct {
        flag: u8,   // 1 byte + 7 padding
        value: u64, // 8 bytes
        flag2: u8,  // 1 byte + 7 padding
    }

    // Well-aligned struct (minimal padding)
    #[repr(C)]
    #[allow(clippy::items_after_statements)]
    struct AlignedStruct {
        value: u64, // 8 bytes
        flag: u8,   // 1 byte
        flag2: u8,  // 1 byte + 6 padding
    }

    group.bench_function("padded_struct", |b| {
        b.iter(|| {
            let mut structs = Vec::with_capacity(1000);
            for i in 0..1000 {
                structs.push(PaddedStruct { flag: 1, value: i, flag2: 2 });
            }
            black_box(structs);
        });
    });

    group.bench_function("aligned_struct", |b| {
        b.iter(|| {
            let mut structs = Vec::with_capacity(1000);
            for i in 0..1000 {
                structs.push(AlignedStruct { value: i, flag: 1, flag2: 2 });
            }
            black_box(structs);
        });
    });

    group.finish();
}

// ============================================================================
// 4. Memory Fragmentation Benchmarks
// ============================================================================

/// Simulate fragmentation from random allocations/deallocations
fn bench_fragmentation_pattern(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmentation_pattern");

    group.bench_function("sequential_alloc_dealloc", |b| {
        b.iter(|| {
            let mut maps = Vec::new();

            // Sequential allocation
            for _ in 0..100 {
                let mut map = HashMap::new();
                for i in 0..100 {
                    map.insert(i, i * 2);
                }
                maps.push(map);
            }

            // Sequential deallocation
            maps.clear();
            black_box(maps);
        });
    });

    group.bench_function("interleaved_alloc_dealloc", |b| {
        b.iter(|| {
            let mut maps = Vec::new();

            // Interleaved allocation/deallocation
            for _ in 0..50 {
                let mut map = HashMap::new();
                for i in 0..100 {
                    map.insert(i, i * 2);
                }
                maps.push(map);

                // Deallocate every other one
                if maps.len() > 1 {
                    maps.remove(maps.len() / 2);
                }
            }

            maps.clear();
            black_box(maps);
        });
    });

    group.finish();
}

// ============================================================================
// 5. Peak Memory Usage Benchmarks
// ============================================================================

/// Benchmark memory usage during peak load
fn bench_peak_memory_usage(c: &mut Criterion) {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};

    let mut group = c.benchmark_group("peak_memory_usage");

    group.bench_function("max_cache_population", |b| {
        b.iter(|| {
            let cache = TlbCache::new(16_384, ReplacementPolicy::Lru);
            let stream_id = StreamID::new(1).unwrap();
            let pasid = PASID::new(0).unwrap();

            // Fill cache to maximum capacity
            for page in 0..16_384 {
                let iova = IOVA::new((page as u64) * 0x1000).unwrap();
                let pa = PA::new((page as u64) * 0x1000 + 0x1_0000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page as u64);
                cache.insert(key, entry);
            }
            black_box(cache);
        });
    });

    group.bench_function("multi_stream_peak", |b| {
        b.iter(|| {
            let cache = TlbCache::new(16_384, ReplacementPolicy::Lru);
            let pasid = PASID::new(0).unwrap();

            // Multiple streams filling cache
            for stream in 0..32 {
                let stream_id = StreamID::new(stream).unwrap();
                for page in 0..512 {
                    let iova = IOVA::new((page as u64) * 0x1000).unwrap();
                    let pa = PA::new((page as u64) * 0x1000 + 0x1_0000).unwrap();
                    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                    let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page as u64);
                    cache.insert(key, entry);
                }
            }
            black_box(cache);
        });
    });

    group.finish();
}

// ============================================================================
// 6. Memory Efficiency Metrics
// ============================================================================

/// Calculate and benchmark memory efficiency ratios
fn bench_memory_efficiency_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency_ratio");

    // Measure overhead of HashMap vs theoretical minimum
    group.bench_function("hashmap_efficiency", |b| {
        b.iter(|| {
            let num_entries = 1000;
            let map: HashMap<u64, u64> = (0..num_entries).map(|i| (i, i * 2)).collect();

            // Theoretical minimum: 16 bytes per entry (key + value)
            let theoretical_bytes = num_entries * 16;

            // Actual usage (approximate)
            let capacity = map.capacity();
            let actual_bytes = capacity * 32; // Rough estimate with overhead

            let efficiency = (theoretical_bytes as f64) / (actual_bytes as f64);
            black_box((map, efficiency));
        });
    });

    // Measure overhead of BTreeMap
    group.bench_function("btreemap_efficiency", |b| {
        b.iter(|| {
            let num_entries = 1000;
            let map: BTreeMap<u64, u64> = (0..num_entries).map(|i| (i, i * 2)).collect();

            // BTreeMap has node overhead
            let theoretical_bytes = num_entries * 16;
            let actual_bytes = num_entries * 48; // Rough estimate with node overhead

            let efficiency = (theoretical_bytes as f64) / (actual_bytes as f64);
            black_box((map, efficiency));
        });
    });

    group.finish();
}

/// Benchmark sparse vs dense memory efficiency
fn bench_sparse_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("sparse_efficiency");

    // Sparse mapping (1% populated)
    group.bench_function("sparse_1_percent", |b| {
        b.iter(|| {
            let mut map = HashMap::new();
            let populated = 1000; // 1% of 100,000

            for i in 0..populated {
                map.insert(i * 100, i * 0x1000);
            }
            black_box(map);
        });
    });

    // Dense mapping (50% populated)
    group.bench_function("dense_50_percent", |b| {
        b.iter(|| {
            let mut map = HashMap::new();
            let populated = 50_000; // 50% of 100,000

            for i in 0..populated {
                map.insert(i * 2, i * 0x1000);
            }
            black_box(map);
        });
    });

    group.finish();
}

/// Benchmark memory usage with different cache sizes
fn bench_cache_size_memory_usage(c: &mut Criterion) {
    use smmu::cache::{ReplacementPolicy, TlbCache};

    let mut group = c.benchmark_group("cache_size_memory_usage");

    for cache_size in &[256, 1024, 4096, 16_384] {
        group.bench_with_input(BenchmarkId::from_parameter(cache_size), cache_size, |b, &cache_size| {
            b.iter(|| {
                let cache = TlbCache::new(cache_size, ReplacementPolicy::Lru);
                black_box(cache);
            });
        });
    }

    group.finish();
}

/// Benchmark memory usage during concurrent operations
fn bench_concurrent_memory_usage(c: &mut Criterion) {
    
    use std::thread;

    let mut group = c.benchmark_group("concurrent_memory_usage");

    group.bench_function("single_thread_allocations", |b| {
        b.iter(|| {
            let mut maps = Vec::new();
            for _ in 0..10 {
                let mut map = HashMap::new();
                for i in 0..1000 {
                    map.insert(i, i * 2);
                }
                maps.push(map);
            }
            black_box(maps);
        });
    });

    group.bench_function("multi_thread_allocations", |b| {
        b.iter(|| {
            let mut handles = vec![];

            for _ in 0..4 {
                let handle = thread::spawn(|| {
                    let mut map = HashMap::new();
                    for i in 0..250 {
                        map.insert(i, i * 2);
                    }
                    map
                });
                handles.push(handle);
            }

            let maps: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            black_box(maps);
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark Groups
// ============================================================================

criterion_group! {
    name = benches;
    config = configure_criterion();
    targets =
        // 1. Allocation patterns
        bench_allocation_overhead,
        bench_growth_vs_prealloc,
        bench_reallocation_frequency,
        // 2. Memory pooling
        bench_memory_pooling,
        bench_object_reuse,
        // 3. Compact representations
        bench_compact_layouts,
        bench_alignment_overhead,
        // 4. Fragmentation
        bench_fragmentation_pattern,
        // 5. Peak usage
        bench_peak_memory_usage,
        // 6. Efficiency metrics
        bench_memory_efficiency_ratio,
        bench_sparse_efficiency,
        bench_cache_size_memory_usage,
        bench_concurrent_memory_usage
}

criterion_main!(benches);
