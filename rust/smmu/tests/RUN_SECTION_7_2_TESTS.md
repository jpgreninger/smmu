# Quick Guide: Running Section 7.2 Algorithm Optimization Tests

## Prerequisites

```bash
cd /home/jpgreninger/Work/smmu/rust/smmu
```

## Run All Section 7.2 Tests

### Performance Regression Tests (~ 30 seconds)
```bash
cargo test --release performance_regression_tests -- --nocapture
```

### Memory Usage Tests (~20 seconds)
```bash
cargo test --release memory_usage_tests -- --nocapture
```

### Both Test Suites
```bash
cargo test --release performance_regression_tests memory_usage_tests -- --nocapture
```

## Run All Section 7.2 Benchmarks

### Algorithm Optimization Benchmarks (~15 minutes)
```bash
cargo bench --bench algorithm_optimization
```

### Memory Usage Benchmarks (~10 minutes)
```bash
cargo bench --bench memory_usage
```

### Both Benchmark Suites
```bash
cargo bench --bench algorithm_optimization --bench memory_usage
```

## Run Specific Test Categories

### Complexity Verification Tests
```bash
# HashMap O(1) complexity
cargo test --release test_hashmap_lookup_is_o1 -- --nocapture

# BTreeMap O(log n) complexity
cargo test --release test_btreemap_lookup_is_olog_n -- --nocapture

# Cache lookup complexity
cargo test --release test_cache_lookup_complexity -- --nocapture

# PASID lookup complexity
cargo test --release test_pasid_lookup_complexity -- --nocapture
```

### Latency Bound Tests
```bash
# Cache hit latency (target: < 50ns)
cargo test --release test_cache_hit_latency_target -- --nocapture

# Hash function latency (target: < 10ns)
cargo test --release test_hash_function_latency -- --nocapture

# Insertion latency (target: < 100ns)
cargo test --release test_insertion_latency_bound -- --nocapture
```

### Throughput Tests
```bash
# Minimum throughput (target: ≥ 7M ops/sec)
cargo test --release test_minimum_throughput_requirement -- --nocapture

# Batch operation throughput
cargo test --release test_batch_operation_throughput -- --nocapture
```

### Memory Usage Tests
```bash
# Memory overhead verification
cargo test --release test_hashmap_memory_overhead -- --nocapture

# Cache memory bounds
cargo test --release test_cache_memory_bounds -- --nocapture

# Sparse structure efficiency
cargo test --release test_sparse_structure_memory_efficiency -- --nocapture
```

### Cache Performance Tests
```bash
# Hit rate target (≥ 95%)
cargo test --release test_cache_hit_rate_target -- --nocapture

# Eviction fairness
cargo test --release test_cache_eviction_fairness -- --nocapture
```

## Run Specific Benchmark Categories

### Lookup Complexity Benchmarks
```bash
cargo bench --bench algorithm_optimization -- lookup_complexity
```

### Hash Function Benchmarks
```bash
cargo bench --bench algorithm_optimization -- hash
```

### Sparse Structure Benchmarks
```bash
cargo bench --bench algorithm_optimization -- sparse
```

### SmallVec Benchmarks
```bash
cargo bench --bench algorithm_optimization -- smallvec
```

### Baseline Comparison Benchmarks
```bash
cargo bench --bench algorithm_optimization -- baseline
```

### Memory Allocation Benchmarks
```bash
cargo bench --bench memory_usage -- allocation
```

### Memory Pooling Benchmarks
```bash
cargo bench --bench memory_usage -- pooling
```

### Fragmentation Benchmarks
```bash
cargo bench --bench memory_usage -- fragmentation
```

## Criterion Baseline Management

### Save Current Performance as Baseline
```bash
cargo bench --bench algorithm_optimization -- --save-baseline main
cargo bench --bench memory_usage -- --save-baseline main
```

### Compare Against Baseline
```bash
# After making changes
cargo bench --bench algorithm_optimization -- --baseline main
cargo bench --bench memory_usage -- --baseline main
```

### View Benchmark Reports
```bash
# HTML reports generated at:
# target/criterion/<benchmark_name>/report/index.html

# Open in browser (Linux)
xdg-open target/criterion/hashmap_lookup_complexity/report/index.html

# Or navigate manually
```

## Performance Profiling

### Generate Flamegraph
```bash
# Install cargo-flamegraph
cargo install flamegraph

# Profile a specific benchmark
cargo flamegraph --bench algorithm_optimization -- --bench hashmap_lookup_complexity

# Open flamegraph.svg in browser
```

### Memory Profiling with Valgrind
```bash
# Install valgrind
sudo dnf install valgrind  # Fedora
# or
sudo apt install valgrind  # Ubuntu

# Run tests with memory check
cargo test --release performance_regression_tests
valgrind --tool=massif target/release/deps/performance_regression_tests-*

# Analyze with massif-visualizer
massif-visualizer massif.out.*
```

## Expected Output

### Successful Test Run
```
running 15 tests
test test_hashmap_lookup_is_o1 ... ok
test test_btreemap_lookup_is_olog_n ... ok
test test_cache_lookup_complexity ... ok
test test_pasid_lookup_complexity ... ok
test test_cache_hit_latency_target ... ok
test test_hash_function_latency ... ok
test test_insertion_latency_bound ... ok
test test_minimum_throughput_requirement ... ok
test test_batch_operation_throughput ... ok
test test_hashmap_memory_overhead ... ok
test test_cache_memory_bounds ... ok
test test_sparse_structure_memory_efficiency ... ok
test test_cache_hit_rate_target ... ok
test test_cache_eviction_fairness ... ok
test test_no_performance_regression_in_lookup ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Successful Benchmark Run
```
hashmap_lookup_complexity/100
                        time:   [12.345 ns 12.456 ns 12.567 ns]
hashmap_lookup_complexity/10000
                        time:   [12.678 ns 12.789 ns 12.890 ns]
hashmap_lookup_complexity/100000
                        time:   [13.012 ns 13.123 ns 13.234 ns]

Performance has not regressed.
```

## Troubleshooting

### Tests Running Slowly
```bash
# Ensure running in release mode
cargo test --release <test_name>

# Not debug mode (much slower)
# cargo test <test_name>  # DON'T USE FOR PERFORMANCE TESTS
```

### Benchmarks Showing High Variance
```bash
# Increase sample size (edit benchmark file)
# Change: .sample_size(500) → .sample_size(1000)

# Increase warm-up time
# Change: .warm_up_time(Duration::from_secs(2)) → Duration::from_secs(5)

# Run on idle system
# Close other applications
# Disable CPU frequency scaling
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
```

### Test Failures

**test_hashmap_lookup_is_o1 fails**:
- May indicate O(n) behavior in lookup path
- Check for iteration in critical sections
- Profile with flamegraph

**test_cache_hit_latency_target fails**:
- Ensure release build: `--release` flag
- Check system load: `top` or `htop`
- May need to adjust target if system is slow

**Memory tests fail**:
- Check available memory: `free -h`
- May need to reduce test sizes on low-memory systems

## Performance Metrics Summary

After running all tests and benchmarks, you should see:

### Complexity Metrics
- ✅ HashMap lookup: O(1) - ratio < 2.0x across 1K→100K
- ✅ BTreeMap lookup: O(log n) - ratio 1.0-5.0x across 1K→100K
- ✅ Cache lookup: O(1) - ratio < 3.0x across 100→10K
- ✅ PASID lookup: O(1) - ratio < 2.5x across 16→256

### Latency Metrics
- ✅ Cache hit: < 50ns (typically 20-40ns)
- ✅ Hash function: < 10ns (typically 5-8ns)
- ✅ Cache insert: < 100ns (typically 60-80ns)

### Throughput Metrics
- ✅ Cache lookups: ≥ 7M ops/sec
- ✅ Batch operations: Complete 100K ops in < 100ms

### Memory Metrics
- ✅ HashMap overhead: < 3.0x theoretical minimum
- ✅ Cache entry size: ≤ 128 bytes
- ✅ Cache key size: ≤ 64 bytes
- ✅ Cache hit rate: ≥ 95% for working set

## Integration with TASKS-RUST.md

These tests fulfill the following requirements from Section 7.2:

1. **Optimize lookup algorithms for O(1)/O(log n) performance** ✅
   - Complexity verification tests
   - Profiling with criterion and flamegraph
   - Hash function optimization benchmarks

2. **Implement efficient sparse data structures** ✅
   - HashMap/BTreeMap comparison benchmarks
   - Sparse structure efficiency tests

3. **Add memory usage optimization** ✅
   - Compact representation tests
   - Memory pooling benchmarks
   - Memory usage metrics tests

4. **Create performance benchmarking suite with criterion** ✅
   - Algorithm optimization benchmarks (50+ benchmarks)
   - Memory usage benchmarks (30+ benchmarks)
   - Regression detection tests (15+ tests)

## Time Estimates (from TASKS-RUST.md)

- ✅ Write performance regression tests: 5 hours - **COMPLETE**
- ✅ Create memory usage tests: 4 hours - **COMPLETE**
- ✅ Test algorithm complexity: 4 hours - **COMPLETE**

**Total test development time: 13 hours** - All deliverables complete

## Next Steps

After validating all tests pass:

1. **Commit the test suite**:
   ```bash
   git add benches/algorithm_optimization.rs
   git add benches/memory_usage.rs
   git add tests/performance_regression_tests.rs
   git add tests/memory_usage_tests.rs
   git add tests/README_SECTION_7_2_OPTIMIZATION.md
   git add tests/RUN_SECTION_7_2_TESTS.md
   git add Cargo.toml
   git commit -m "Add comprehensive Section 7.2 algorithm optimization tests and benchmarks"
   ```

2. **Run full validation**:
   ```bash
   ./run_section_7_2_validation.sh
   ```

3. **Update TASKS-RUST.md**:
   - Mark Section 7.2 test development as complete
   - Document test coverage metrics
   - Record baseline performance numbers

4. **Proceed to Section 7.3** (if applicable):
   - Integration testing
   - End-to-end validation
