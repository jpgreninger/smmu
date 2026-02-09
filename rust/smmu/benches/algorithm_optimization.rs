#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::items_after_statements)]
//! Algorithm Optimization Benchmarks (Section 7.2)
//!
//! Comprehensive benchmarks for algorithm complexity validation and optimization.
//! These benchmarks verify O(1)/O(log n) performance requirements and compare
//! against the C++ baseline target of 135ns average translation time.
//!
//! # Benchmark Categories
//!
//! 1. Lookup Algorithm Complexity - Verify O(1)/O(log n) scaling
//! 2. Hash Function Performance - Optimize cache key hashing
//! 3. Sparse Data Structure Performance - `HashMap` vs `BTreeMap`
//! 4. Memory Usage Optimization - Memory pooling and compact representations
//! 5. `SmallVec` Optimization - Stack allocation for batched operations
//! 6. Baseline Comparisons - Compare against 135ns C++ target

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

// ============================================================================
// Benchmark Configuration
// ============================================================================

fn configure_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .sample_size(1000)
        .significance_level(0.01)
        .noise_threshold(0.01)
}

// ============================================================================
// 1. Lookup Algorithm Complexity Benchmarks
// ============================================================================

/// Benchmark `HashMap` lookup performance vs. size to verify O(1) complexity
///
/// This benchmark tests that lookup time remains constant as the number of
/// entries increases, validating O(1) average case performance.
fn bench_hashmap_lookup_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("hashmap_lookup_complexity");

    // Test with exponentially increasing sizes to clearly show O(1) behavior
    for size in &[100, 500, 1000, 5000, 10_000, 50_000, 100_000] {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Setup: Create and populate HashMap
            let mut map = HashMap::with_capacity(size);
            for i in 0..size {
                map.insert(i as u64, i as u64 * 2);
            }

            // Benchmark: Lookup should be O(1) regardless of size
            b.iter(|| {
                // Access middle element to avoid edge cases
                let key = black_box((size / 2) as u64);
                let result = map.get(&key);
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark `BTreeMap` lookup performance vs. size to verify O(log n) complexity
///
/// This benchmark validates that lookup time grows logarithmically with size.
fn bench_btreemap_lookup_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("btreemap_lookup_complexity");

    for size in &[100, 500, 1000, 5000, 10_000, 50_000, 100_000] {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Setup: Create and populate BTreeMap
            let mut map = BTreeMap::new();
            for i in 0..size {
                map.insert(i as u64, i as u64 * 2);
            }

            // Benchmark: Lookup should be O(log n)
            b.iter(|| {
                let key = black_box((size / 2) as u64);
                let result = map.get(&key);
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark cache key lookup scaling with TLB size
fn bench_cache_lookup_scaling(c: &mut Criterion) {
    use smmu::cache::{CacheKey, CacheKeyHash};
    use smmu::{SecurityState, StreamID, IOVA, PASID};

    let mut group = c.benchmark_group("cache_lookup_scaling");

    // Test cache lookup performance at different cache sizes
    for cache_size in &[64, 256, 1024, 4096, 16_384] {
        group.throughput(Throughput::Elements(*cache_size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(cache_size), cache_size, |b, &cache_size| {
            // Setup: Populate cache with entries
            let mut cache_map: HashMap<u64, u64> = HashMap::with_capacity(cache_size);
            let stream_id = StreamID::new(1).unwrap();
            let pasid = PASID::new(0).unwrap();

            for page in 0..cache_size {
                let iova = IOVA::new((page as u64) * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let hash = CacheKeyHash::hash(&key);
                cache_map.insert(hash, page as u64);
            }

            // Benchmark: Lookup should be O(1)
            b.iter(|| {
                let page = black_box((cache_size / 2) as u64);
                let iova = IOVA::new(page * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let hash = CacheKeyHash::hash(&key);
                let result = cache_map.get(&hash);
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark PASID lookup complexity (should be O(1) or O(log n))
fn bench_pasid_lookup_complexity(c: &mut Criterion) {
    use smmu::PASID;

    let mut group = c.benchmark_group("pasid_lookup_complexity");

    // Test with increasing numbers of PASIDs per stream
    for num_pasids in &[1, 4, 16, 64, 256, 1024] {
        group.throughput(Throughput::Elements(*num_pasids as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_pasids), num_pasids, |b, &num_pasids| {
            // Setup: Create PASID map (simulating StreamContext)
            let mut pasid_map: HashMap<u32, u64> = HashMap::with_capacity(num_pasids);
            for i in 0..num_pasids {
                let pasid = PASID::new(i as u32).unwrap();
                pasid_map.insert(pasid.as_u32(), i as u64);
            }

            // Benchmark: PASID lookup should be O(1)
            b.iter(|| {
                let pasid = PASID::new(black_box((num_pasids / 2) as u32)).unwrap();
                let result = pasid_map.get(&pasid.as_u32());
                black_box(result);
            });
        });
    }

    group.finish();
}

// ============================================================================
// 2. Hash Function Performance Benchmarks
// ============================================================================

/// Benchmark custom FNV-1a hash function performance
fn bench_fnv1a_hash(c: &mut Criterion) {
    use smmu::cache::{CacheKey, CacheKeyHash};
    use smmu::{SecurityState, StreamID, IOVA, PASID};

    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

    c.bench_function("fnv1a_hash_function", |b| {
        b.iter(|| {
            let hash = CacheKeyHash::hash(black_box(&key));
            black_box(hash);
        });
    });
}

/// Compare hash function quality by distribution
fn bench_hash_distribution(c: &mut Criterion) {
    use smmu::cache::{CacheKey, CacheKeyHash};
    use smmu::{SecurityState, StreamID, IOVA, PASID};

    let mut group = c.benchmark_group("hash_distribution");

    // Benchmark hashing of sequential addresses (realistic pattern)
    group.bench_function("sequential_addresses", |b| {
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();

        b.iter(|| {
            let mut hashes = Vec::with_capacity(1000);
            for page in 0..1000 {
                let iova = IOVA::new(page * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                hashes.push(CacheKeyHash::hash(&key));
            }
            black_box(hashes);
        });
    });

    // Benchmark hashing with different StreamIDs
    group.bench_function("different_streams", |b| {
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        b.iter(|| {
            let mut hashes = Vec::with_capacity(100);
            for stream in 0..100 {
                let stream_id = StreamID::new(stream).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                hashes.push(CacheKeyHash::hash(&key));
            }
            black_box(hashes);
        });
    });

    group.finish();
}

/// Benchmark hash collision rate
fn bench_hash_collisions(c: &mut Criterion) {
    use smmu::cache::{CacheKey, CacheKeyHash};
    use smmu::{SecurityState, StreamID, IOVA, PASID};
    use std::collections::HashSet;

    c.bench_function("hash_collision_detection", |b| {
        b.iter(|| {
            let mut seen_hashes = HashSet::new();
            let mut collisions = 0;
            let stream_id = StreamID::new(1).unwrap();
            let pasid = PASID::new(0).unwrap();

            // Hash 10_000 sequential pages
            for page in 0..10_000 {
                let iova = IOVA::new(page * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let hash = CacheKeyHash::hash(&key);

                if !seen_hashes.insert(hash) {
                    collisions += 1;
                }
            }
            black_box(collisions);
        });
    });
}

// ============================================================================
// 3. Sparse Data Structure Performance Benchmarks
// ============================================================================

/// Compare `HashMap` vs `BTreeMap` for sparse page tables
fn bench_sparse_structure_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("sparse_structure_comparison");

    // Test with sparse mappings (only 1% of address space populated)
    let mapped_pages = 1000; // 1% sparsity

    group.bench_function("hashmap_sparse_lookup", |b| {
        let mut map = HashMap::with_capacity(mapped_pages);
        for i in 0..mapped_pages {
            // Sparse addresses - every 100th page
            map.insert(i * 100, i * 0x1000);
        }

        b.iter(|| {
            // Look up a mapped page
            let result = map.get(black_box(&(500 * 100)));
            black_box(result);
        });
    });

    group.bench_function("btreemap_sparse_lookup", |b| {
        let mut map = BTreeMap::new();
        for i in 0..mapped_pages {
            map.insert(i * 100, i * 0x1000);
        }

        b.iter(|| {
            let result = map.get(black_box(&(500 * 100)));
            black_box(result);
        });
    });

    group.bench_function("hashmap_sparse_insert", |b| {
        b.iter_batched(
            || HashMap::with_capacity(mapped_pages),
            |mut map| {
                for i in 0..100 {
                    map.insert(i * 100, i * 0x1000);
                }
                black_box(map);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("btreemap_sparse_insert", |b| {
        b.iter_batched(
            BTreeMap::new,
            |mut map| {
                for i in 0..100 {
                    map.insert(i * 100, i * 0x1000);
                }
                black_box(map);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark iteration over sparse structures
fn bench_sparse_iteration(c: &mut Criterion) {
    let mut group = c.benchmark_group("sparse_iteration");
    let mapped_pages = 1000;

    group.bench_function("hashmap_iteration", |b| {
        let mut map = HashMap::with_capacity(mapped_pages);
        for i in 0..mapped_pages {
            map.insert(i * 100, i * 0x1000);
        }

        b.iter(|| {
            let sum: u64 = map.values().map(|&v| v as u64).sum();
            black_box(sum);
        });
    });

    group.bench_function("btreemap_iteration", |b| {
        let mut map = BTreeMap::new();
        for i in 0..mapped_pages {
            map.insert(i * 100, i * 0x1000);
        }

        b.iter(|| {
            let sum: u64 = map.values().map(|&v| v as u64).sum();
            black_box(sum);
        });
    });

    group.finish();
}

// ============================================================================
// 4. Memory Usage Optimization Benchmarks
// ============================================================================

/// Benchmark memory overhead of different data structures
fn bench_memory_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_overhead");

    // Measure allocation overhead for different structure sizes
    for num_entries in &[100, 1000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("hashmap_allocation", num_entries),
            num_entries,
            |b, &num_entries| {
                b.iter(|| {
                    let mut map = HashMap::with_capacity(num_entries);
                    for i in 0..num_entries {
                        map.insert(i as u64, i as u64 * 2);
                    }
                    black_box(map);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("btreemap_allocation", num_entries),
            num_entries,
            |b, &num_entries| {
                b.iter(|| {
                    let mut map = BTreeMap::new();
                    for i in 0..num_entries {
                        map.insert(i as u64, i as u64 * 2);
                    }
                    black_box(map);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark compact vs standard representations
fn bench_compact_representations(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact_representations");

    // Compare u64 vs (u32, u32) for storing page entries
    group.bench_function("u64_storage", |b| {
        let mut entries = Vec::with_capacity(1000);
        b.iter(|| {
            for i in 0..1000 {
                entries.push(i as u64);
            }
            black_box(&entries);
            entries.clear();
        });
    });

    group.bench_function("tuple_storage", |b| {
        let mut entries = Vec::with_capacity(1000);
        b.iter(|| {
            for i in 0..1000 {
                entries.push((i as u32, i as u32));
            }
            black_box(&entries);
            entries.clear();
        });
    });

    group.finish();
}

// ============================================================================
// 5. SmallVec Optimization Benchmarks
// ============================================================================

/// Benchmark `SmallVec` vs Vec for batched operations
fn bench_smallvec_batched_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("smallvec_batched_operations");

    // Test with different batch sizes
    for batch_size in &[4, 8, 16, 32, 64] {
        group.bench_with_input(BenchmarkId::new("vec", batch_size), batch_size, |b, &batch_size| {
            b.iter(|| {
                let mut batch = Vec::with_capacity(batch_size);
                for i in 0..batch_size {
                    batch.push(i as u64);
                }
                black_box(batch);
            });
        });

        group.bench_with_input(BenchmarkId::new("smallvec", batch_size), batch_size, |b, &batch_size| {
            b.iter(|| {
                let mut batch: SmallVec<[u64; 32]> = SmallVec::new();
                for i in 0..batch_size {
                    batch.push(i as u64);
                }
                black_box(batch);
            });
        });
    }

    group.finish();
}

/// Benchmark `SmallVec` for TLB invalidation batches
fn bench_smallvec_invalidation_batches(c: &mut Criterion) {
    use smmu::cache::CacheKey;
    use smmu::{SecurityState, StreamID, IOVA, PASID};

    let mut group = c.benchmark_group("smallvec_invalidation_batches");

    group.bench_function("vec_invalidation_batch", |b| {
        b.iter(|| {
            let mut invalidations = Vec::with_capacity(16);
            let stream_id = StreamID::new(1).unwrap();
            let pasid = PASID::new(0).unwrap();

            for page in 0..16 {
                let iova = IOVA::new(page * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                invalidations.push(key);
            }
            black_box(invalidations);
        });
    });

    group.bench_function("smallvec_invalidation_batch", |b| {
        b.iter(|| {
            let mut invalidations: SmallVec<[CacheKey; 16]> = SmallVec::new();
            let stream_id = StreamID::new(1).unwrap();
            let pasid = PASID::new(0).unwrap();

            for page in 0..16 {
                let iova = IOVA::new(page * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                invalidations.push(key);
            }
            black_box(invalidations);
        });
    });

    group.finish();
}

// ============================================================================
// 6. Baseline Comparison Benchmarks (vs C++ 135ns)
// ============================================================================

/// Benchmark translation latency against C++ baseline
fn bench_translation_baseline_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("baseline_comparison");

    // Set explicit target for comparison: 135ns C++ baseline
    // Note: C++ baseline is 135ns

    group.bench_function("translation_latency", |b| {
        // TODO: Implement when full translation path is ready
        // Target: < 135ns average
        b.iter(|| {
            // Simulate minimal translation (will be replaced with actual)
            black_box(());
        });
    });

    group.finish();
}

/// Benchmark cache hit performance vs baseline
fn bench_cache_hit_baseline(c: &mut Criterion) {
    use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
    use smmu::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};

    let mut group = c.benchmark_group("cache_hit_baseline");

    group.bench_function("tlb_hit_latency", |b| {
        let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();

        // Pre-populate cache
        for page in 0..100 {
            let iova = IOVA::new(page * 0x1000).unwrap();
            let pa = PA::new(page * 0x1000 + 0x1_0000).unwrap();
            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
            cache.insert(key, entry);
        }

        let lookup_key = CacheKey::new(stream_id, pasid, IOVA::new(50 * 0x1000).unwrap(), SecurityState::NonSecure);

        // Benchmark: Should be much faster than 135ns baseline
        b.iter(|| {
            let result = cache.lookup(black_box(&lookup_key));
            black_box(result);
        });
    });

    group.finish();
}

/// Benchmark throughput (operations per second)
fn bench_throughput_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_comparison");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("batch_1000_lookups", |b| {
        // TODO: Implement when translation is ready
        // Measure sustained throughput for 1000 translations
        b.iter(|| {
            for _ in 0..1000 {
                black_box(());
            }
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
        // 1. Complexity benchmarks
        bench_hashmap_lookup_complexity,
        bench_btreemap_lookup_complexity,
        bench_cache_lookup_scaling,
        bench_pasid_lookup_complexity,
        // 2. Hash function benchmarks
        bench_fnv1a_hash,
        bench_hash_distribution,
        bench_hash_collisions,
        // 3. Sparse structure benchmarks
        bench_sparse_structure_comparison,
        bench_sparse_iteration,
        // 4. Memory optimization benchmarks
        bench_memory_overhead,
        bench_compact_representations,
        // 5. SmallVec benchmarks
        bench_smallvec_batched_ops,
        bench_smallvec_invalidation_batches,
        // 6. Baseline comparison benchmarks
        bench_translation_baseline_comparison,
        bench_cache_hit_baseline,
        bench_throughput_comparison
}

criterion_main!(benches);
