# ARM SMMU v3 Test Suite

This directory contains the comprehensive test suite for the ARM SMMU v3 Rust implementation.

## Test Organization

### Unit Tests
Unit tests are co-located with the source code in each module:
- `src/types/mod.rs` - Type system tests
- `src/address_space/mod.rs` - Address space tests
- `src/stream_context/mod.rs` - Stream context tests
- `src/smmu/mod.rs` - SMMU controller tests
- `src/fault/mod.rs` - Fault handling tests
- `src/cache/mod.rs` - Cache/TLB tests

### Integration Tests
Located in `tests/`:
- `integration_test.rs` - Cross-component interaction tests
- `compliance_test.rs` - ARM SMMU v3 specification compliance tests

### Test Utilities
Located in `tests/common/`:
- `builders.rs` - Test data builders with fluent APIs
- `assertions.rs` - Custom assertion helpers
- `fixtures.rs` - Pre-defined test scenarios and data
- `perf.rs` - Performance measurement utilities
- `mocks.rs` - Mock objects and test doubles

### Test Fixtures
Located in `tests/fixtures/`:
- `translation_scenarios.json` - Translation test cases
- `fault_scenarios.json` - Fault handling test cases

## Running Tests

### All Tests
```bash
cd rust
cargo test --workspace --all-features
```

### Unit Tests Only
```bash
cargo test --lib
```

### Integration Tests Only
```bash
cargo test --test integration_test
cargo test --test compliance_test
```

### Specific Test
```bash
cargo test test_name
```

### With Output
```bash
cargo test -- --nocapture
```

### With Test Threads
```bash
cargo test -- --test-threads=1  # Single-threaded
cargo test -- --test-threads=4  # 4 threads
```

## Code Coverage

### Generate Coverage Report
```bash
# HTML report
../scripts/coverage.sh --html

# LCOV report for CI
../scripts/coverage.sh --lcov

# Check coverage threshold (95%)
../scripts/coverage.sh --check

# Summary only
../scripts/coverage.sh --summary
```

### Coverage Requirements
- **Minimum Line Coverage**: 95%
- **Branch Coverage**: Best effort
- All new code must include tests
- Critical paths require 100% coverage

## Benchmarks

Benchmarks are located in `benches/`:
- `translation.rs` - Translation performance (target: 135ns)
- `address_space.rs` - Page table operations
- `cache.rs` - TLB/cache performance

### Running Benchmarks
```bash
# All benchmarks
cargo bench --workspace

# Specific benchmark
cargo bench --bench translation
cargo bench --bench address_space
cargo bench --bench cache

# With custom configuration
cargo bench -- --warm-up-time 5 --measurement-time 10
```

### Performance Targets
- **Translation Latency**: < 135ns average (matching C++ baseline)
- **TLB Hit**: < 10ns
- **Page Table Walk**: O(log n) or better
- **Memory Efficiency**: Sparse representation proportional to mapped pages

## Test-Driven Development (TDD)

We follow strict TDD practices:

1. **Write Failing Test**: Create test that captures requirement
2. **Run Test**: Verify it fails for the right reason
3. **Implement**: Write minimal code to pass test
4. **Verify**: Run test and ensure it passes
5. **Refactor**: Improve code while maintaining test pass
6. **Coverage**: Ensure >95% coverage maintained

## Test Categories

### Functional Tests
- Basic translation flows
- Multi-stream isolation
- Multi-PASID support
- Address space management
- Fault handling
- Permission checking

### Compliance Tests
- ARM SMMU v3 specification adherence
- Stream ID range validation
- PASID range validation (20-bit)
- Address width compliance (48-bit VA/PA)
- Page size support (4KB, 2MB, 1GB)
- Translation stage support

### Performance Tests
- Translation latency
- Throughput measurements
- Cache hit rates
- Memory efficiency
- Scalability (streams, PASIDs, pages)

### Security Tests
- Stream isolation
- PASID isolation
- Permission enforcement
- Fault containment

### Stress Tests
- Large number of streams (>1000)
- Large number of PASIDs (>256)
- Large address spaces (>100K pages)
- Concurrent access patterns

## Test Utilities Usage

### Using Builders
```rust
use common::builders::*;

let config = SMMUConfigBuilder::new()
    .with_stream_count(128)
    .with_pasid_bits(16)
    .build();
```

### Using Assertions
```rust
use common::assertions::*;

assert_page_aligned(0x1000);
assert_valid_pasid(42);
assert_canonical_va(0x0000_7FFF_FFFF_FFFF);
```

### Using Fixtures
```rust
use common::fixtures::*;

let fixture = StandardTestFixture::new();
let mappings = fixture.generate_identity_mappings(16);
```

### Using Performance Utilities
```rust
use common::perf::*;

let result = measure_average(1000, || {
    // Operation to measure
});

assert!(result.meets_target(135, 10.0)); // 135ns ±10%
```

## Continuous Integration

Tests run automatically on:
- Every push to main/develop branches
- Every pull request
- Manual workflow dispatch

CI checks:
- Format (rustfmt)
- Lints (clippy)
- Build (debug and release)
- Tests (unit and integration)
- Coverage (with threshold check)
- Benchmarks (with regression detection)
- Documentation build
- MSRV compatibility
- Security audit

## Contributing Tests

When adding new features:
1. Add tests to appropriate test file
2. Use test utilities for consistency
3. Follow naming conventions (test_*)
4. Add documentation comments
5. Ensure coverage >95%
6. Run full test suite before committing

## Test Naming Conventions

- `test_<feature>_<scenario>` - Standard test
- `test_<feature>_<scenario>_success` - Success case
- `test_<feature>_<scenario>_failure` - Failure case
- `test_<feature>_<scenario>_edge_case` - Edge case
- `bench_<feature>` - Benchmark

## Debugging Tests

### Run with backtrace
```bash
RUST_BACKTRACE=1 cargo test
RUST_BACKTRACE=full cargo test
```

### Run with logging
```bash
RUST_LOG=debug cargo test
```

### Run specific test with output
```bash
cargo test test_name -- --nocapture --test-threads=1
```

## Resources

- ARM SMMU v3 Specification: IHI0070G_b
- Project Requirements: `../ARM_SMMU_v3_PRD.md`
- Implementation Guide: `../DEVELOPMENT.md`
- Coverage Reports: `../target/coverage/html/index.html`
