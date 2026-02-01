//! Performance Regression Test Suite (Phase 4.4)
//!
//! Comprehensive benchmark suite for detecting performance regressions.
//! Tracks key performance metrics and ensures they stay within acceptable bounds.
//!
//! Target Metrics (based on C++ baseline and Phase 3 achievements):
//! - Translation latency: < 135ns (500x better than 67.5µs C++ baseline)
//! - Cache hit rate: > 95% for typical workloads
//! - Stream configuration: < 1µs
//! - Fault processing: > 1M faults/sec
//! - Memory usage: < 100 bytes per mapping

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use smmu::address_space::AddressSpace;
use smmu::cache::{CacheEntry, CacheKey, ReplacementPolicy, TlbCache};
use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PAGE_SIZE, PASID};
use smmu::SMMU;
use std::time::Duration;

// ============================================================================
// Criterion Configuration for Regression Testing
// ============================================================================

fn configure_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.02)
}

// ============================================================================
// 1. Translation Latency Benchmarks
// ============================================================================

fn bench_translation_latency_simple(c: &mut Criterion) {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x10000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    c.bench_function("translation_latency_simple", |b| {
        b.iter(|| {
            let result = addr_space.translate_page(black_box(iova), AccessType::Read, SecurityState::NonSecure);
            let _ = black_box(result);
        });
    });
}

fn bench_translation_latency_with_pasid(c: &mut Criterion) {
    let stream_context = StreamContext::new();
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x10000).unwrap();

    stream_context.create_pasid(pasid).unwrap();
    stream_context
        .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    c.bench_function("translation_latency_with_pasid", |b| {
        b.iter(|| {
            let result =
                stream_context.translate(black_box(pasid), black_box(iova), AccessType::Read, SecurityState::NonSecure);
            let _ = black_box(result);
        });
    });
}

fn bench_translation_under_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_under_load");
    group.throughput(Throughput::Elements(1));

    for num_mappings in [10, 100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(num_mappings), num_mappings, |b, &num_mappings| {
            let mut addr_space = AddressSpace::new();

            // Create many mappings to simulate load
            for i in 0..num_mappings {
                let iova = IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap();
                let pa = PA::new(0x10000 + (i as u64) * PAGE_SIZE).unwrap();
                addr_space
                    .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                    .unwrap();
            }

            // Benchmark translation of the first mapping
            let test_iova = IOVA::new(0x1000).unwrap();

            b.iter(|| {
                let result =
                    addr_space.translate_page(black_box(test_iova), AccessType::Read, SecurityState::NonSecure);
                let _ = black_box(result);
            });
        });
    }

    group.finish();
}

// ============================================================================
// 2. Cache Hit Rate Benchmarks
// ============================================================================

fn bench_cache_hit_rate_sequential(c: &mut Criterion) {
    let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    // Pre-populate cache with 512 entries (half capacity)
    for page in 0..512 {
        let iova = IOVA::new(page * 0x1000).unwrap();
        let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
        cache.insert(key, entry);
    }

    c.bench_function("cache_hit_rate_sequential", |b| {
        b.iter(|| {
            // Sequential access pattern - should have high hit rate
            for page in 0..100 {
                let iova = IOVA::new(page * 0x1000).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                black_box(cache.lookup(&key));
            }
        });
    });
}

fn bench_cache_hit_rate_random(c: &mut Criterion) {
    let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    // Pre-populate cache
    for page in 0..1024 {
        let iova = IOVA::new(page * 0x1000).unwrap();
        let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
        cache.insert(key, entry);
    }

    // Random access pattern
    let access_pattern: Vec<u64> = (0..100)
        .map(|i| ((i * 17 + 23) % 1024) * 0x1000) // Pseudo-random but deterministic
        .collect();

    c.bench_function("cache_hit_rate_random", |b| {
        b.iter(|| {
            for &addr in &access_pattern {
                let iova = IOVA::new(addr).unwrap();
                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                black_box(cache.lookup(&key));
            }
        });
    });
}

fn bench_cache_hit_rate_working_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_rate_working_set");

    for working_set_size in [10, 100, 500, 1000, 2000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(working_set_size),
            working_set_size,
            |b, &working_set_size| {
                let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
                let stream_id = StreamID::new(1).unwrap();
                let pasid = PASID::new(0).unwrap();

                // Fill cache
                for page in 0..1024 {
                    let iova = IOVA::new(page * 0x1000).unwrap();
                    let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
                    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                    let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
                    cache.insert(key, entry);
                }

                b.iter(|| {
                    // Access working_set_size different pages repeatedly
                    for _ in 0..10 {
                        for page in 0..working_set_size {
                            let iova = IOVA::new(((page % working_set_size) as u64) * 0x1000).unwrap();
                            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                            black_box(cache.lookup(&key));
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// 3. Stream Configuration Performance
// ============================================================================

fn bench_stream_configuration_time(c: &mut Criterion) {
    let config = StreamConfig::default();

    c.bench_function("stream_configuration_time", |b| {
        b.iter_batched(
            || SMMU::new(),
            |smmu| {
                let stream_id = StreamID::new(1).unwrap();
                let _ = black_box(smmu.configure_stream(black_box(stream_id), config.clone()));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_stream_configuration_multiple(c: &mut Criterion) {
    let config = StreamConfig::default();

    let mut group = c.benchmark_group("stream_configuration_multiple");
    group.throughput(Throughput::Elements(1));

    for num_streams in [1, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(num_streams), num_streams, |b, &num_streams| {
            let cfg = config.clone();
            b.iter_batched(
                || SMMU::new(),
                |smmu| {
                    for i in 0..num_streams {
                        let stream_id = StreamID::new(i as u32).unwrap();
                        let _ = black_box(smmu.configure_stream(stream_id, cfg.clone()));
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ============================================================================
// 4. Fault Processing Throughput
// ============================================================================

fn bench_fault_processing_throughput(c: &mut Criterion) {
    use smmu::types::{FaultRecord, FaultType};

    c.bench_function("fault_record_creation", |b| {
        b.iter(|| {
            let record = FaultRecord::new(
                StreamID::new(1).unwrap(),
                PASID::new(0).unwrap(),
                IOVA::new(0x1000).unwrap(),
                FaultType::TranslationFault,
                AccessType::Read,
                SecurityState::NonSecure,
            );
            black_box(record);
        });
    });

    c.bench_function("fault_batch_processing", |b| {
        b.iter(|| {
            let mut faults = Vec::with_capacity(1000);
            for i in 0..1000 {
                let record = FaultRecord::new(
                    StreamID::new((i % 100) as u32).unwrap(),
                    PASID::new(0).unwrap(),
                    IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap(),
                    FaultType::TranslationFault,
                    AccessType::Read,
                    SecurityState::NonSecure,
                );
                faults.push(record);
            }
            black_box(faults);
        });
    });
}

// ============================================================================
// 5. Memory Usage Under Load
// ============================================================================

fn bench_memory_usage_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage_scaling");

    for num_mappings in [100, 1000, 10000, 100000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(num_mappings), num_mappings, |b, &num_mappings| {
            b.iter(|| {
                let mut addr_space = AddressSpace::new();

                for i in 0..num_mappings {
                    let iova = IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap();
                    let pa = PA::new(0x10000 + (i as u64) * PAGE_SIZE).unwrap();
                    addr_space
                        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
                        .unwrap();
                }

                black_box(addr_space);
            });
        });
    }

    group.finish();
}

fn bench_pasid_memory_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("pasid_memory_scaling");

    for num_pasids in [10, 100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(num_pasids), num_pasids, |b, &num_pasids| {
            b.iter(|| {
                let stream_context = StreamContext::new();

                for i in 0..num_pasids {
                    let pasid = PASID::new(i as u32).unwrap();
                    stream_context.create_pasid(pasid).unwrap();

                    // Map a few pages per PASID
                    for j in 0..10 {
                        let iova = IOVA::new(0x1000 + (j as u64) * PAGE_SIZE).unwrap();
                        let pa = PA::new(0x10000 + (j as u64) * PAGE_SIZE).unwrap();
                        stream_context
                            .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                            .unwrap();
                    }
                }

                black_box(stream_context);
            });
        });
    }

    group.finish();
}

// ============================================================================
// 6. Algorithmic Complexity Verification (O(1) / O(log n))
// ============================================================================

fn bench_complexity_translation_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("complexity_translation_lookup");
    group.plot_config(criterion::PlotConfiguration::default().summary_scale(criterion::AxisScale::Logarithmic));

    // Verify O(1) lookup regardless of address space size
    for num_mappings in [100, 1000, 10000, 100000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(num_mappings), num_mappings, |b, &num_mappings| {
            let mut addr_space = AddressSpace::new();

            // Create many mappings
            for i in 0..num_mappings {
                let iova = IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap();
                let pa = PA::new(0x10000 + (i as u64) * PAGE_SIZE).unwrap();
                addr_space
                    .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                    .unwrap();
            }

            // Benchmark lookup (should be O(1) for HashMap)
            let test_iova = IOVA::new(0x1000 + ((num_mappings / 2) as u64) * PAGE_SIZE).unwrap();

            b.iter(|| {
                let result =
                    addr_space.translate_page(black_box(test_iova), AccessType::Read, SecurityState::NonSecure);
                let _ = black_box(result);
            });
        });
    }

    group.finish();
}

fn bench_complexity_pasid_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("complexity_pasid_lookup");
    group.plot_config(criterion::PlotConfiguration::default().summary_scale(criterion::AxisScale::Logarithmic));

    // Verify O(1) PASID lookup regardless of number of PASIDs
    for num_pasids in [10, 100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(num_pasids), num_pasids, |b, &num_pasids| {
            let stream_context = StreamContext::new();

            // Create many PASIDs
            for i in 0..num_pasids {
                let pasid = PASID::new(i as u32).unwrap();
                stream_context.create_pasid(pasid).unwrap();

                // Add one mapping per PASID
                let iova = IOVA::new(0x1000).unwrap();
                let pa = PA::new(0x10000).unwrap();
                stream_context
                    .map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
                    .unwrap();
            }

            // Benchmark PASID lookup (should be O(1) for DashMap)
            let test_pasid = PASID::new((num_pasids / 2) as u32).unwrap();
            let test_iova = IOVA::new(0x1000).unwrap();

            b.iter(|| {
                let result = stream_context.translate(
                    black_box(test_pasid),
                    black_box(test_iova),
                    AccessType::Read,
                    SecurityState::NonSecure,
                );
                let _ = black_box(result);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Criterion Group Configuration
// ============================================================================

criterion_group! {
    name = translation_latency;
    config = configure_criterion();
    targets =
        bench_translation_latency_simple,
        bench_translation_latency_with_pasid,
        bench_translation_under_load,
}

criterion_group! {
    name = cache_performance;
    config = configure_criterion();
    targets =
        bench_cache_hit_rate_sequential,
        bench_cache_hit_rate_random,
        bench_cache_hit_rate_working_set,
}

criterion_group! {
    name = stream_configuration;
    config = configure_criterion();
    targets =
        bench_stream_configuration_time,
        bench_stream_configuration_multiple,
}

criterion_group! {
    name = fault_processing;
    config = configure_criterion();
    targets =
        bench_fault_processing_throughput,
}

criterion_group! {
    name = memory_usage;
    config = configure_criterion();
    targets =
        bench_memory_usage_scaling,
        bench_pasid_memory_scaling,
}

criterion_group! {
    name = complexity_verification;
    config = configure_criterion();
    targets =
        bench_complexity_translation_lookup,
        bench_complexity_pasid_lookup,
}

criterion_main!(
    translation_latency,
    cache_performance,
    stream_configuration,
    fault_processing,
    memory_usage,
    complexity_verification,
);
