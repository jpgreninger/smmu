# TLB Cache Benchmark Implementation Plan

**Purpose**: Implement functional benchmarks to measure and track TLB cache performance
**Target**: Sub-microsecond translation latency with high cache hit rates
**Status**: Skeleton defined, implementations needed

---

## Benchmark Priority Matrix

| Priority | Benchmark | Target | Importance |
|----------|-----------|--------|------------|
| P0 | bench_tlb_hit | < 100ns | Critical - validates O(1) lookup |
| P0 | bench_tlb_miss | < 100ns | Critical - validates miss cost |
| P0 | bench_tlb_hit_rate | > 95% | Critical - validates effectiveness |
| P1 | bench_tlb_invalidate_all | < 10μs | High - common operation |
| P1 | bench_concurrent_tlb_access | Linear scaling | High - multi-core usage |
| P2 | bench_tlb_invalidate_by_stream | < 5μs | Medium - selective invalidation |
| P2 | bench_cache_comparison | 10-100x speedup | Medium - demonstrates value |
| P3 | All others | Baseline | Low - optimization insights |

---

## Implementation Templates

### Template 1: Basic Lookup Benchmark

```rust
fn bench_tlb_hit(c: &mut Criterion) {
    use smmu::cache::{TlbCache, CacheKey, CacheEntry, ReplacementPolicy};
    use smmu::{StreamID, PASID, IOVA, PA, PagePermissions, SecurityState};

    // Setup: Create cache and populate with entries
    let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    // Pre-populate cache
    for page in 0..100 {
        let iova = IOVA::new(page * 0x1000).unwrap();
        let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
        cache.insert(key, entry);
    }

    // Benchmark: Lookup existing entry (hit)
    let lookup_key = CacheKey::new(
        stream_id,
        pasid,
        IOVA::new(50 * 0x1000).unwrap(),
        SecurityState::NonSecure,
    );

    c.bench_function("tlb_hit", |b| {
        b.iter(|| {
            let result = cache.lookup(black_box(&lookup_key));
            black_box(result);
        });
    });
}
```

**Expected Result**: 50-100ns per lookup (O(1) hash table access)

### Template 2: Miss Benchmark

```rust
fn bench_tlb_miss(c: &mut Criterion) {
    let cache = TlbCache::new(1024, ReplacementPolicy::Lru);

    // Don't populate cache - force misses
    let lookup_key = CacheKey::new(
        StreamID::new(1).unwrap(),
        PASID::new(0).unwrap(),
        IOVA::new(0x1000).unwrap(),
        SecurityState::NonSecure,
    );

    c.bench_function("tlb_miss", |b| {
        b.iter(|| {
            let result = cache.lookup(black_box(&lookup_key));
            black_box(result);
        });
    });
}
```

**Expected Result**: 20-50ns per lookup (hash miss + miss counter)

### Template 3: Hit Rate with Varying Working Sets

```rust
fn bench_tlb_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlb_hit_rate");

    for num_pages in [10, 100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_pages),
            num_pages,
            |b, &num_pages| {
                let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
                let stream_id = StreamID::new(1).unwrap();
                let pasid = PASID::new(0).unwrap();

                // Populate cache with entries
                for page in 0..1024 {
                    let iova = IOVA::new(page * 0x1000).unwrap();
                    let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
                    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                    let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
                    cache.insert(key, entry);
                }

                // Access pattern: cycle through num_pages
                b.iter(|| {
                    for page in 0..num_pages {
                        let iova = IOVA::new((page % num_pages) * 0x1000).unwrap();
                        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                        black_box(cache.lookup(&key));
                    }
                });
            },
        );
    }

    group.finish();
}
```

**Expected Result**:
- 10 pages: >99% hit rate (all fit in cache)
- 100 pages: >99% hit rate (all fit in cache)
- 1000 pages: >95% hit rate (most fit in cache)
- 10000 pages: ~10% hit rate (cache thrashing)

### Template 4: Invalidation Benchmark

```rust
fn bench_tlb_invalidate_all(c: &mut Criterion) {
    c.bench_function("tlb_invalidate_all", |b| {
        b.iter_batched(
            || {
                // Setup: Create and populate cache
                let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
                let stream_id = StreamID::new(1).unwrap();
                let pasid = PASID::new(0).unwrap();

                for page in 0..1024 {
                    let iova = IOVA::new(page * 0x1000).unwrap();
                    let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
                    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                    let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
                    cache.insert(key, entry);
                }

                cache
            },
            |cache| {
                // Benchmark: Invalidate all entries
                cache.invalidate_all();
            },
            criterion::BatchSize::SmallInput,
        );
    });
}
```

**Expected Result**: < 10μs for 1024 entries (O(n) clear operation)

### Template 5: Concurrent Access

```rust
fn bench_concurrent_tlb_access(c: &mut Criterion) {
    use std::sync::Arc;
    use std::thread;

    c.bench_function("concurrent_tlb_access", |b| {
        let cache = Arc::new(TlbCache::new(1024, ReplacementPolicy::Lru));
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(0).unwrap();

        // Pre-populate cache
        for page in 0..100 {
            let iova = IOVA::new(page * 0x1000).unwrap();
            let pa = PA::new(page * 0x1000 + 0x10000).unwrap();
            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);
            cache.insert(key, entry);
        }

        b.iter(|| {
            let mut handles = vec![];

            // Spawn 4 threads performing lookups
            for thread_id in 0..4 {
                let cache_clone = Arc::clone(&cache);
                let handle = thread::spawn(move || {
                    for _ in 0..100 {
                        let page = (thread_id * 25) % 100;
                        let iova = IOVA::new(page * 0x1000).unwrap();
                        let key = CacheKey::new(
                            stream_id,
                            pasid,
                            iova,
                            SecurityState::NonSecure,
                        );
                        black_box(cache_clone.lookup(&key));
                    }
                });
                handles.push(handle);
            }

            // Wait for all threads
            for handle in handles {
                handle.join().unwrap();
            }
        });
    });
}
```

**Expected Result**: Near-linear scaling (4x threads = ~3-4x throughput)

---

## Implementation Checklist

### Phase 1: Critical Benchmarks (P0)

- [ ] **bench_tlb_hit**: Measure cache hit latency
  - Setup: 1024-entry cache, 100 entries populated
  - Measure: Single lookup of existing entry
  - Target: < 100ns
  - Validates: O(1) lookup, DashMap performance

- [ ] **bench_tlb_miss**: Measure cache miss latency
  - Setup: Empty cache
  - Measure: Single lookup of non-existent entry
  - Target: < 50ns
  - Validates: Fast-path miss detection

- [ ] **bench_tlb_hit_rate**: Measure hit rate vs working set
  - Setup: 1024-entry cache
  - Test: Working sets of 10, 100, 1000, 10000 pages
  - Target: >95% for working sets < cache size
  - Validates: Cache effectiveness, eviction policy

### Phase 2: High Priority (P1)

- [ ] **bench_tlb_invalidate_all**: Global invalidation cost
  - Setup: Fully populated cache (1024 entries)
  - Measure: Time to clear all entries
  - Target: < 10μs
  - Validates: Bulk invalidation efficiency

- [ ] **bench_tlb_invalidate_by_stream**: Stream-specific invalidation
  - Setup: Multiple streams, 100 entries per stream
  - Measure: Invalidate single stream
  - Target: < 5μs
  - Validates: Secondary index effectiveness

- [ ] **bench_concurrent_tlb_access**: Multi-threaded lookup
  - Setup: 4 threads, shared cache
  - Measure: Throughput with concurrent access
  - Target: 3-4x single-thread performance
  - Validates: Lock-free design

### Phase 3: Medium Priority (P2)

- [ ] **bench_tlb_invalidate_by_pasid**: PASID-specific invalidation
- [ ] **bench_tlb_invalidate_by_va**: VA range invalidation
- [ ] **bench_cache_comparison**: With vs without cache
- [ ] **bench_tlb_size_impact**: Cache size variation
- [ ] **bench_tlb_lru_replacement**: LRU policy performance
- [ ] **bench_tlb_fifo_replacement**: FIFO policy performance

### Phase 4: Optimization Insights (P3)

- [ ] **bench_sequential_access_pattern**: Sequential VA pattern
- [ ] **bench_random_access_pattern**: Random VA pattern
- [ ] **bench_strided_access_pattern**: Strided VA pattern
- [ ] **bench_cache_overhead**: Memory overhead measurement
- [ ] **bench_cache_insertion_cost**: Insert operation cost
- [ ] **bench_cache_eviction_cost**: Eviction operation cost
- [ ] **bench_concurrent_invalidation**: Invalidation during lookups

---

## Benchmark Execution Plan

### Step 1: Implement Priority Benchmarks

```bash
# Edit benches/cache.rs with implementations above
# Focus on P0 benchmarks first
```

### Step 2: Establish Baseline

```bash
cd /home/jpgreninger/Work/smmu/rust/smmu

# Run benchmarks and save baseline
cargo bench cache --bench cache > baseline_cache_bench.txt

# Review results
cat baseline_cache_bench.txt
```

### Step 3: Validate Against Targets

Compare results against targets:
- TLB hit: < 100ns ✓/✗
- TLB miss: < 50ns ✓/✗
- Hit rate (small working set): > 95% ✓/✗
- Invalidation: < 10μs ✓/✗
- Concurrent scaling: > 3x ✓/✗

### Step 4: Integrate into CI

```yaml
# .github/workflows/benchmarks.yml
name: Performance Benchmarks
on: [push, pull_request]

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run benchmarks
        run: cargo bench cache --bench cache
      - name: Compare with baseline
        run: |
          # Compare results with committed baseline
          # Fail if regression > 10%
```

---

## Performance Targets Summary

| Benchmark | Target | Rationale |
|-----------|--------|-----------|
| TLB hit | < 100ns | Sub-microsecond translation requirement |
| TLB miss | < 50ns | Minimize miss penalty |
| Hit rate (1K working set) | > 95% | High cache effectiveness |
| Invalidate all | < 10μs | Acceptable for configuration changes |
| Invalidate stream | < 5μs | Fast selective invalidation |
| Concurrent (4 threads) | > 3x throughput | Near-linear scaling |
| Insert | < 500ns | Fast cache population |
| Eviction | < 1μs | Minimize replacement overhead |

---

## Benchmark Output Format

Expected Criterion output:

```
tlb_hit                 time:   [87.234 ns 89.123 ns 91.456 ns]
                        thrpt:  [10.93M elem/s 11.22M elem/s 11.46M elem/s]

tlb_miss                time:   [42.156 ns 43.789 ns 45.234 ns]
                        thrpt:  [22.11M elem/s 22.84M elem/s 23.72M elem/s]

tlb_hit_rate/10         time:   [891.23 ns 912.34 ns 934.56 ns]
                        thrpt:  [10.70M elem/s 10.96M elem/s 11.22M elem/s]

tlb_hit_rate/100        time:   [8.912 μs 9.123 μs 9.345 μs]
                        thrpt:  [10.70M elem/s 10.96M elem/s 11.22M elem/s]

tlb_hit_rate/1000       time:   [89.12 μs 91.23 μs 93.45 μs]
                        thrpt:  [10.70M elem/s 10.96M elem/s 11.22M elem/s]

tlb_invalidate_all      time:   [7.234 μs 7.456 μs 7.678 μs]

concurrent_tlb_access   time:   [28.912 μs 29.456 μs 30.123 μs]
                        thrpt:  [3.32M ops/s 3.39M ops/s 3.46M ops/s]
```

---

## Success Criteria

### Functional Requirements
- ✅ All benchmarks compile without errors
- ✅ All benchmarks complete without panics
- ✅ Results are deterministic (< 5% variance)
- ✅ Baseline can be established

### Performance Requirements
- ✅ TLB hit < 100ns
- ✅ TLB miss < 50ns
- ✅ Hit rate > 95% for appropriate working sets
- ✅ Invalidation < 10μs
- ✅ Concurrent scaling > 3x with 4 threads

### Integration Requirements
- ✅ Benchmarks integrated into `cargo bench`
- ✅ Results saved to file for comparison
- ✅ CI/CD runs benchmarks on every PR
- ✅ Regression detection alerts maintainers

---

## Next Steps

1. **Implement P0 benchmarks** (bench_tlb_hit, bench_tlb_miss, bench_tlb_hit_rate)
2. **Run baseline** and verify targets are met
3. **Implement P1 benchmarks** (invalidation, concurrent)
4. **Document results** in this file
5. **Add CI integration** for continuous tracking
6. **Implement P2/P3** as time permits

---

## Appendix: Benchmark Helper Functions

```rust
/// Create a pre-populated cache for benchmarking
fn create_populated_cache(
    capacity: usize,
    num_entries: usize,
    policy: ReplacementPolicy,
) -> TlbCache {
    let cache = TlbCache::new(capacity, policy);
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    for page in 0..num_entries.min(capacity) {
        let iova = IOVA::new(page as u64 * 0x1000).unwrap();
        let pa = PA::new(page as u64 * 0x1000 + 0x10000).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page as u64);
        cache.insert(key, entry);
    }

    cache
}

/// Create a cache key for benchmarking
fn create_test_key(stream: u32, pasid: u32, page: u64) -> CacheKey {
    CacheKey::new(
        StreamID::new(stream).unwrap(),
        PASID::new(pasid).unwrap(),
        IOVA::new(page * 0x1000).unwrap(),
        SecurityState::NonSecure,
    )
}
```

---

**Document Version**: 1.0
**Last Updated**: 2026-01-28
**Status**: Planning complete, implementation required
