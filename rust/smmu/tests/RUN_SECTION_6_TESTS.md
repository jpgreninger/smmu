# How to Run Section 6 Tests

## Quick Start

### Run All Section 6 Tests
```bash
cd /home/jpgreninger/Work/smmu/rust/smmu

# Run both Section 6.1 and 6.2 (89 tests)
cargo test --test test_fault_detection --test test_fault_processing --lib fault::
```

**Expected**: 89 tests passing in <100ms

## Individual Test Suites

### Section 6.1: Fault Detection (50 tests)
```bash
# Integration tests (30 tests)
cargo test --test test_fault_detection

# Unit tests (20 tests)
cargo test --lib fault::detection
cargo test --lib fault::validator
```

### Section 6.2: Fault Processing (39 tests)
```bash
# Integration tests (27 tests)
cargo test --test test_fault_processing

# Unit tests (12 tests)
cargo test --lib fault::processing
cargo test --lib fault::queue
cargo test --lib fault::recovery
```

## Run Specific Tests

### By Name
```bash
# Run single test
cargo test --test test_fault_processing test_terminate_mode_immediate_reporting

# Run tests matching pattern
cargo test --test test_fault_processing terminate_mode
cargo test --test test_fault_processing stall_mode
cargo test --test test_fault_processing recovery
```

### By Module
```bash
# Run all processing tests
cargo test --lib fault::processing

# Run all queue tests
cargo test --lib fault::queue

# Run all recovery tests
cargo test --lib fault::recovery
```

## Test Output Options

### Show Test Output
```bash
# Show println! output from tests
cargo test --test test_fault_processing -- --nocapture

# Show test names as they run
cargo test --test test_fault_processing -- --nocapture --test-threads=1
```

### Verbose Mode
```bash
# Show detailed compilation
cargo test --test test_fault_processing --verbose

# Show backtraces on failure
RUST_BACKTRACE=1 cargo test --test test_fault_processing

# Full backtrace
RUST_BACKTRACE=full cargo test --test test_fault_processing
```

## Performance Measurement

### Measure Execution Time
```bash
# Time Section 6.2 tests
time cargo test --test test_fault_processing

# Time all Section 6 tests
time cargo test --test test_fault_detection --test test_fault_processing --lib fault::

# Time individual test
time cargo test --test test_fault_processing test_terminate_mode_immediate_reporting -- --nocapture
```

### Benchmark Mode
```bash
# Run tests in release mode (faster)
cargo test --test test_fault_processing --release

# Show timing for each test
cargo test --test test_fault_processing -- --nocapture --test-threads=1 --show-output
```

## Continuous Integration

### CI/CD Commands
```bash
# Run all tests (full regression)
cargo test

# Run tests for specific targets
cargo test --all-targets

# Run tests for entire workspace
cargo test --workspace

# Run with warnings as errors
RUSTFLAGS="-D warnings" cargo test --test test_fault_processing
```

### Pre-Commit Hook
```bash
# Add to .git/hooks/pre-commit
#!/bin/bash
cargo test --test test_fault_detection --test test_fault_processing --lib fault::
if [ $? -ne 0 ]; then
    echo "Section 6 tests failed"
    exit 1
fi
```

## Debugging Failed Tests

### Show Failure Details
```bash
# Run with backtrace
RUST_BACKTRACE=1 cargo test --test test_fault_processing

# Run single test with output
cargo test --test test_fault_processing test_name -- --nocapture --test-threads=1
```

### Run in Debug Mode
```bash
# Build with debug symbols
cargo test --test test_fault_processing -- --nocapture

# Use debugger (lldb/gdb)
rust-lldb target/debug/deps/test_fault_processing-*
```

## Test Coverage

### Generate Coverage Report
```bash
# Using tarpaulin (install: cargo install cargo-tarpaulin)
cargo tarpaulin --test test_fault_processing --out Html

# Using llvm-cov (nightly Rust)
cargo +nightly llvm-cov --test test_fault_processing --html
```

## Expected Results

### Section 6.1 Tests
```
test result: ok. 30 passed; 0 failed; 0 ignored
```

### Section 6.2 Tests
```
test result: ok. 27 passed; 0 failed; 0 ignored
```

### Section 6 Unit Tests
```
test result: ok. 32 passed; 0 failed; 0 ignored
```

### Combined Section 6
```
Section 6.1 Integration: 30 tests ✅
Section 6.2 Integration: 27 tests ✅
Section 6 Unit Tests:    32 tests ✅
Total:                   89 tests ✅
Execution Time:          <100ms
```

## Common Issues

### Issue: Tests Not Found
```bash
# Make sure you're in the correct directory
cd /home/jpgreninger/Work/smmu/rust/smmu

# Check test files exist
ls tests/test_fault_*.rs
```

### Issue: Compilation Errors
```bash
# Clean build artifacts
cargo clean

# Rebuild from scratch
cargo test --test test_fault_processing
```

### Issue: Tests Timeout
```bash
# Increase timeout (default: 60s)
cargo test --test test_fault_processing -- --test-threads=1
```

## Test Organization

```
tests/
├── test_fault_detection.rs       # Section 6.1 Integration (30 tests)
├── test_fault_processing.rs      # Section 6.2 Integration (27 tests)
├── README_SECTION_6_1.md         # Section 6.1 Documentation
├── README_SECTION_6_2.md         # Section 6.2 Documentation
└── RUN_SECTION_6_TESTS.md        # This file

src/fault/
├── detection.rs                  # Section 6.1 (9 unit tests)
├── validator.rs                  # Section 6.1 (11 unit tests)
├── processing.rs                 # Section 6.2 (4 unit tests)
├── queue.rs                      # Section 6.2 (4 unit tests)
└── recovery.rs                   # Section 6.2 (4 unit tests)
```

## References

- **Section 6.1 Docs**: `tests/README_SECTION_6_1.md`
- **Section 6.2 Docs**: `tests/README_SECTION_6_2.md`
- **Test Summary**: `/home/jpgreninger/Work/smmu/SECTION_6_2_TEST_SUITE_SUMMARY.md`
- **Regression Report**: `/home/jpgreninger/Work/smmu/SECTION_6_2_REGRESSION_VERIFICATION.md`

## Quick Test Commands Cheat Sheet

```bash
# Run everything
cargo test

# Section 6 only
cargo test --test test_fault_detection --test test_fault_processing --lib fault::

# Section 6.1 only
cargo test --test test_fault_detection

# Section 6.2 only
cargo test --test test_fault_processing

# With output
cargo test --test test_fault_processing -- --nocapture

# Single test
cargo test --test test_fault_processing test_name

# With timing
time cargo test --test test_fault_processing
```
