#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_lossless)]
//! Translation performance benchmarks
//!
//! Benchmarks for measuring translation latency and throughput.
//! Target: 135ns average translation time (matching C++ baseline)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ============================================================================
// Benchmark Configuration
// ============================================================================

fn configure_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .sample_size(1000)
        .significance_level(0.05)
        .noise_threshold(0.02)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Setup a basic SMMU with a configured stream and mapped page
fn setup_basic_smmu() -> (SMMU, StreamID, PASID, IOVA) {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    // Configure stream with Stage-1 translation
    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    smmu.configure_stream(stream_id, config).unwrap();

    // Create PASID
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map a page
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    (smmu, stream_id, pasid, iova)
}

/// Setup SMMU with multiple mapped pages for large address space testing
fn setup_large_address_space(num_pages: usize) -> (SMMU, StreamID, PASID, Vec<IOVA>) {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let perms = PagePermissions::read_write();
    let mut iovas = Vec::with_capacity(num_pages);

    for i in 0..num_pages {
        let iova = IOVA::new(0x1000 + (i as u64 * 0x1000)).unwrap();
        let pa = PA::new(0x10000 + (i as u64 * 0x1000)).unwrap();
        smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
        iovas.push(iova);
    }

    (smmu, stream_id, pasid, iovas)
}

/// Setup SMMU with multiple PASIDs
fn setup_multi_pasid(num_pasids: u32) -> (SMMU, StreamID, Vec<PASID>, Vec<IOVA>) {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();

    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.pasid_enabled = true;
    config.max_pasid = num_pasids;
    smmu.configure_stream(stream_id, config).unwrap();

    let perms = PagePermissions::read_write();
    let mut pasids = Vec::with_capacity(num_pasids as usize);
    let mut iovas = Vec::with_capacity(num_pasids as usize);

    for i in 0..num_pasids {
        let pasid = PASID::new(i).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        let iova = IOVA::new(0x1000 + (i as u64 * 0x1000)).unwrap();
        let pa = PA::new(0x10000 + (i as u64 * 0x1000)).unwrap();
        smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

        pasids.push(pasid);
        iovas.push(iova);
    }

    (smmu, stream_id, pasids, iovas)
}

// ============================================================================
// Simple Translation Benchmarks
// ============================================================================

fn bench_translation_simple(c: &mut Criterion) {
    let (smmu, stream_id, pasid, iova) = setup_basic_smmu();

    c.bench_function("translation_simple", |b| {
        b.iter(|| {
            // Measure single translation with PASID 0, single stream
            // This should be a cache hit after the first iteration
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });
}

fn bench_translation_with_pasid(c: &mut Criterion) {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(5).unwrap(); // Non-zero PASID

    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    config.pasid_enabled = true;
    config.max_pasid = 10;
    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();
    smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    c.bench_function("translation_with_pasid", |b| {
        b.iter(|| {
            // Measure translation with non-zero PASID lookup
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });
}

// ============================================================================
// Cached vs Uncached Translation
// ============================================================================

fn bench_translation_cached(c: &mut Criterion) {
    let (smmu, stream_id, pasid, iova) = setup_basic_smmu();

    // Warm up cache with one translation
    let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);

    c.bench_function("translation_cached_hit", |b| {
        b.iter(|| {
            // Measure cached translation (TLB hit)
            // Target: <50ns per translation
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });
}

fn bench_translation_uncached(c: &mut Criterion) {
    let smmu = SMMU::new();
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    let mut config = StreamConfig::default();
    config.translation_enabled = true;
    config.stage1_enabled = true;
    smmu.configure_stream(stream_id, config).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map many pages to ensure we cycle through enough addresses to evict from cache
    // With default cache size of 1024 entries, using 2000 pages ensures cache misses
    let perms = PagePermissions::read_write();
    let num_pages = 2000;
    let mut iovas = Vec::with_capacity(num_pages);

    for i in 0..num_pages {
        let iova = IOVA::new(0x1000 + (i as u64 * 0x1000)).unwrap();
        let pa = PA::new(0x10000 + (i as u64 * 0x1000)).unwrap();
        smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
        iovas.push(iova);
    }

    let mut page_index = 0;

    c.bench_function("translation_cache_miss", |b| {
        b.iter(|| {
            // Cycle through addresses to force cache misses
            // Since we have 2000 pages and cache holds 1024, we get natural evictions
            let iova = iovas[page_index];
            page_index = (page_index + 1) % num_pages;

            // Measure uncached translation (TLB miss due to cache eviction)
            // Expected: 200-350ns per translation
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });
}

// ============================================================================
// Multi-PASID Benchmarks
// ============================================================================

fn bench_translation_multi_pasid(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_multi_pasid");

    for num_pasids in &[1, 2, 4, 8, 16, 32] {
        group.bench_with_input(BenchmarkId::from_parameter(num_pasids), num_pasids, |b, &num_pasids| {
            let (smmu, stream_id, pasids, iovas) = setup_multi_pasid(num_pasids);

            // Warm up cache
            for (pasid, iova) in pasids.iter().zip(iovas.iter()) {
                let _ = smmu.translate(stream_id, *pasid, *iova, AccessType::Read);
            }

            b.iter(|| {
                // Measure translation rotating through all PASIDs
                // Tests PASID lookup scalability - should be O(1) with HashMap
                for (pasid, iova) in pasids.iter().zip(iovas.iter()) {
                    let result = smmu.translate(
                        black_box(stream_id),
                        black_box(*pasid),
                        black_box(*iova),
                        black_box(AccessType::Read),
                    );
                    black_box(result.unwrap());
                }
            });
        });
    }

    group.finish();
}

// ============================================================================
// Large Address Space Benchmarks
// ============================================================================

fn bench_translation_large_address_space(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_large_address_space");

    // Test with different numbers of mapped pages
    for num_pages in &[10, 100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::from_parameter(num_pages), num_pages, |b, &num_pages| {
            let (smmu, stream_id, pasid, iovas) = setup_large_address_space(num_pages);

            // Warm up cache with all pages
            for iova in &iovas {
                let _ = smmu.translate(stream_id, pasid, *iova, AccessType::Read);
            }

            let mut page_index = 0;

            b.iter(|| {
                // Measure translation from large address space
                // Verify O(1) lookup performance regardless of address space size
                let iova = iovas[page_index];
                page_index = (page_index + 1) % num_pages;

                let result = smmu.translate(
                    black_box(stream_id),
                    black_box(pasid),
                    black_box(iova),
                    black_box(AccessType::Read),
                );
                black_box(result.unwrap());
            });
        });
    }

    group.finish();
}

// ============================================================================
// Mixed Workload Benchmark
// ============================================================================

fn bench_translation_mixed_workload(c: &mut Criterion) {
    let (smmu, stream_id, pasid, iovas) = setup_large_address_space(100);

    // Warm up cache with 90% of pages (simulating 90% hit rate)
    for iova in &iovas[0..90] {
        let _ = smmu.translate(stream_id, pasid, *iova, AccessType::Read);
    }

    let mut page_index = 0;

    c.bench_function("translation_mixed_90pct_hit", |b| {
        b.iter(|| {
            // 90% cache hits (pages 0-89), 10% cache misses (pages 90-99 not cached)
            // Target: <135ns average
            let iova = iovas[page_index];
            page_index = (page_index + 1) % 100;

            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });
}

// ============================================================================
// Throughput Benchmarks
// ============================================================================

fn bench_translation_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_throughput");

    // Measure throughput with batches of translations
    for batch_size in [10, 100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(batch_size), &batch_size, |b, &batch_size| {
            let (smmu, stream_id, pasid, iovas) = setup_large_address_space(batch_size);

            // Warm up cache
            for iova in &iovas {
                let _ = smmu.translate(stream_id, pasid, *iova, AccessType::Read);
            }

            b.iter(|| {
                // Measure throughput (ops/sec) for batch translations
                for iova in &iovas {
                    let result = smmu.translate(
                        black_box(stream_id),
                        black_box(pasid),
                        black_box(*iova),
                        black_box(AccessType::Read),
                    );
                    black_box(result.unwrap());
                }
            });
        });
    }

    group.finish();
}

// ============================================================================
// Concurrent Translation Benchmarks
// ============================================================================

fn bench_translation_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_concurrent");

    for num_threads in &[1, 2, 4, 8] {
        group.bench_with_input(BenchmarkId::from_parameter(num_threads), num_threads, |b, &num_threads| {
            let (smmu, stream_id, pasid, iovas) = setup_large_address_space(1000);
            let smmu = Arc::new(smmu);

            // Warm up cache
            for iova in &iovas {
                let _ = smmu.translate(stream_id, pasid, *iova, AccessType::Read);
            }

            b.iter(|| {
                // Spawn threads to perform concurrent translations
                let mut handles = Vec::new();

                for thread_id in 0..num_threads {
                    let smmu_clone = Arc::clone(&smmu);
                    let iovas_clone = iovas.clone();

                    let handle = thread::spawn(move || {
                        // Each thread translates a subset of pages
                        let start_idx = thread_id * (1000 / num_threads);
                        let end_idx = (thread_id + 1) * (1000 / num_threads);

                        for iova in &iovas_clone[start_idx..end_idx] {
                            let result = smmu_clone.translate(stream_id, pasid, *iova, AccessType::Read);
                            black_box(result.unwrap());
                        }
                    });

                    handles.push(handle);
                }

                // Wait for all threads to complete
                for handle in handles {
                    handle.join().unwrap();
                }
            });
        });
    }

    group.finish();
}

// ============================================================================
// Stage Configuration Comparison Benchmarks
// ============================================================================

fn bench_translation_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_comparison");

    // Stage-1 only translation
    group.bench_function("stage1_only", |b| {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();

        let mut config = StreamConfig::default();
        config.translation_enabled = true;
        config.stage1_enabled = true;
        config.stage2_enabled = false;
        smmu.configure_stream(stream_id, config).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        smmu.map_page(stream_id, pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Warm cache
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);

        b.iter(|| {
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });

    // Stage-2 only translation
    group.bench_function("stage2_only", |b| {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(2).unwrap();
        let pasid = PASID::new(0).unwrap();

        let mut config = StreamConfig::default();
        config.translation_enabled = true;
        config.stage1_enabled = false;
        config.stage2_enabled = true;
        smmu.configure_stream(stream_id, config).unwrap();
        smmu.create_stage2_address_space(stream_id).unwrap();

        let ipa = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        smmu.map_stage2_page(stream_id, ipa, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Warm cache
        let _ = smmu.translate(stream_id, pasid, ipa, AccessType::Read);

        b.iter(|| {
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(ipa),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });

    // Two-stage translation
    group.bench_function("two_stage", |b| {
        let smmu = SMMU::new();
        let stream_id = StreamID::new(3).unwrap();
        let pasid = PASID::new(0).unwrap();

        let mut config = StreamConfig::default();
        config.translation_enabled = true;
        config.stage1_enabled = true;
        config.stage2_enabled = true;
        smmu.configure_stream(stream_id, config).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();
        smmu.create_stage2_address_space(stream_id).unwrap();

        let iova = IOVA::new(0x1000).unwrap();
        let ipa = PA::new(0x2000).unwrap();
        let final_pa = PA::new(0x3000).unwrap();

        // Map Stage-1: IOVA -> IPA
        smmu.map_page(stream_id, pasid, iova, ipa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Map Stage-2: IPA -> PA
        let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
        smmu.map_stage2_page(stream_id, ipa_as_iova, final_pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Warm cache
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read);

        b.iter(|| {
            let result = smmu.translate(
                black_box(stream_id),
                black_box(pasid),
                black_box(iova),
                black_box(AccessType::Read),
            );
            black_box(result.unwrap());
        });
    });

    group.finish();
}

// ============================================================================
// Access Type Variation Benchmarks
// ============================================================================

fn bench_translation_access_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_access_types");

    for access_type in &[AccessType::Read, AccessType::Write, AccessType::Execute] {
        let access_name = match access_type {
            AccessType::Read => "read",
            AccessType::Write => "write",
            AccessType::Execute => "execute",
            _ => "unknown",
        };

        group.bench_function(access_name, |b| {
            // Setup SMMU with appropriate permissions for each access type
            let smmu = SMMU::new();
            let stream_id = StreamID::new(1).unwrap();
            let pasid = PASID::new(0).unwrap();

            let mut config = StreamConfig::default();
            config.translation_enabled = true;
            config.stage1_enabled = true;
            smmu.configure_stream(stream_id, config).unwrap();
            smmu.create_pasid(stream_id, pasid).unwrap();

            let iova = IOVA::new(0x1000).unwrap();
            let pa = PA::new(0x2000).unwrap();

            // Map page with permissions appropriate for the access type
            let perms = match access_type {
                AccessType::Read => PagePermissions::read_only(),
                AccessType::Write => PagePermissions::read_write(),
                AccessType::Execute => PagePermissions::read_execute(),
                _ => PagePermissions::all(),
            };
            smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

            // Warm cache
            let _ = smmu.translate(stream_id, pasid, iova, *access_type);

            b.iter(|| {
                let result = smmu.translate(
                    black_box(stream_id),
                    black_box(pasid),
                    black_box(iova),
                    black_box(*access_type),
                );
                black_box(result.unwrap());
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark Groups
// ============================================================================

criterion_group! {
    name = benches;
    config = configure_criterion();
    targets =
        bench_translation_simple,
        bench_translation_with_pasid,
        bench_translation_cached,
        bench_translation_uncached,
        bench_translation_mixed_workload,
        bench_translation_multi_pasid,
        bench_translation_large_address_space,
        bench_translation_throughput,
        bench_translation_concurrent,
        bench_translation_comparison,
        bench_translation_access_types
}

criterion_main!(benches);
