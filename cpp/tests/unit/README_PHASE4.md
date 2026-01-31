# Phase 4 Coverage Test Suite - Quick Reference

## Overview

The Phase 4 test suite (`test_smmu_phase4_coverage.cpp`) contains **70 comprehensive test cases** designed to improve coverage of `smmu.cpp` from 71% to 80%+.

## Quick Start

### Build and Run

```bash
# From repository root
cd build

# Build Phase 4 tests
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
make test_smmu_phase4_coverage

# Run Phase 4 tests
./tests/unit/test_smmu_phase4_coverage

# Run with verbose output
./tests/unit/test_smmu_phase4_coverage --gtest_verbose

# Run specific test
./tests/unit/test_smmu_phase4_coverage --gtest_filter="*TwoStageTranslation*"
```

### Coverage Analysis

```bash
# Build with coverage flags
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON \
  -DCMAKE_CXX_FLAGS="--coverage -fprofile-arcs -ftest-coverage"
make test_smmu_phase4_coverage

# Run all SMMU tests
./tests/unit/test_smmu
./tests/unit/test_smmu_coverage
./tests/unit/test_smmu_advanced_coverage
./tests/unit/test_smmu_priority2_coverage
./tests/unit/test_smmu_priority2_phase2
./tests/unit/test_smmu_phase4_coverage

# Generate coverage report
gcov ../src/smmu/smmu.cpp
```

## Test Categories

### 1. Constructor & Configuration (6 tests)
- Invalid configuration fallback
- Security state mismatches
- Expired cache entries
- Configuration error paths

### 2. Two-Stage Translation (9 tests)
- Unconfigured streams
- No stages enabled
- Stage-2 null address space
- Permission validation failures

### 3. Cache Invalidation (8 tests)
- Event handling errors
- Cache clearing operations
- Statistics management
- Global fault mode

### 4. Event Handling (7 tests)
- Configuration errors
- Internal errors
- Event queue overflow
- Event type validation

### 5. Command Processing (6 tests)
- Command queue full checks
- CFGI commands
- TLBI variants
- ATC invalidation

### 6. Event Queue Errors (3 tests)
- Overflow handling
- Error code validation
- Queue management

### 7. Security States (6 tests)
- Secure state validation
- Realm state transitions
- Context determination

### 8. Fault Syndrome (11 tests)
- All fault type encoding
- Stage-2 fault bits
- Privilege level determination
- Detailed classification

### 9. Access Flags & Dirty Bits (6 tests)
- Cache hit recording
- Access flag faults
- Dirty bit updates
- Write permission validation

### 10. Address Size (3 tests)
- Address size fault detection
- Input/output validation
- Large address handling

### 11. Comprehensive (5 tests)
- Multi-stream operations
- Cache coherency
- Queue lifecycle
- Configuration updates

## Test Execution Options

### Run All Tests
```bash
./tests/unit/test_smmu_phase4_coverage
```

### Run by Category
```bash
# Two-stage translation tests
./tests/unit/test_smmu_phase4_coverage --gtest_filter="*TwoStageTranslation*"

# Event handling tests
./tests/unit/test_smmu_phase4_coverage --gtest_filter="*EventHandling*"

# Security state tests
./tests/unit/test_smmu_phase4_coverage --gtest_filter="*SecurityState*"

# Fault syndrome tests
./tests/unit/test_smmu_phase4_coverage --gtest_filter="*FaultSyndrome*"
```

### List All Tests
```bash
./tests/unit/test_smmu_phase4_coverage --gtest_list_tests
```

### Run Specific Test
```bash
./tests/unit/test_smmu_phase4_coverage \
  --gtest_filter="SMMUPhase4CoverageTest.TwoStageTranslation_PermissionIntersection"
```

## Test Results

Expected output:
```
[==========] Running 70 tests from 1 test suite.
[----------] Global test environment set-up.
[----------] 70 tests from SMMUPhase4CoverageTest
...
[----------] 70 tests from SMMUPhase4CoverageTest (17 ms total)

[----------] Global test environment tear-down
[==========] 70 tests from 1 test suite ran. (17 ms total)
[  PASSED  ] 70 tests.
```

## Integration with CI/CD

### CMake Integration
The test suite is automatically built with:
```bash
make unit_tests
```

### CTest Integration
Run via CTest:
```bash
ctest -R test_smmu_phase4_coverage
ctest -R test_smmu_phase4_coverage --verbose
```

## Debugging Failed Tests

### Verbose Output
```bash
./tests/unit/test_smmu_phase4_coverage --gtest_verbose
```

### Run with Debugger
```bash
gdb ./tests/unit/test_smmu_phase4_coverage
(gdb) run
(gdb) bt  # backtrace on failure
```

### Enable Debug Logging
Rebuild with debug symbols:
```bash
cmake .. -DCMAKE_BUILD_TYPE=Debug
make test_smmu_phase4_coverage
```

## Coverage Targets

| Category | Lines Targeted | Test Count |
|----------|----------------|------------|
| Constructor/Config | 51, 102, 135, 203, 285, 306 | 6 |
| Two-Stage Translation | 636-740 | 9 |
| Cache Invalidation | 418-512 | 8 |
| Event Handling | 1272-1308 | 7 |
| Command Processing | 1363-1539 | 6 |
| Event Queue Errors | 1588-1620 | 3 |
| Security States | 1672-1698 | 6 |
| Fault Syndrome | 1737-1891 | 11 |
| Access Flags | 551-838 | 6 |
| Address Size | 853-892 | 3 |
| Comprehensive | Multiple | 5 |

## Test Constants

The test suite uses the following constants:
- `STREAM1 = 0x1000`
- `STREAM2 = 0x2000`
- `STREAM3 = 0x3000`
- `PASID1 = 0x1`
- `PASID2 = 0x2`
- `PASID_ZERO = 0x0`
- `TEST_IOVA1 = 0x10000000`
- `TEST_IOVA2 = 0x20000000`
- `TEST_PA1 = 0x40000000`
- `TEST_PA2 = 0x50000000`

## Helper Functions

The test suite provides:
- `setupBasicStream()` - Configure basic single-stage stream
- `setupTwoStageStream()` - Configure two-stage translation stream with options

## ARM SMMU v3 Compliance

All tests validate compliance with:
- ARM SMMU v3 Architecture Specification (IHI0070G)
- Two-stage address translation
- Security state management (NonSecure, Secure, Realm)
- Fault syndrome register encoding
- Event and command queue processing
- TLB and cache invalidation

## Troubleshooting

### Build Failures
```bash
# Clean rebuild
cd build
rm -rf *
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
make test_smmu_phase4_coverage
```

### Test Failures
1. Check that all existing tests still pass
2. Verify configuration is valid
3. Check for resource limits (queue sizes, cache sizes)
4. Review test output for specific failure reasons

### Coverage Issues
```bash
# Ensure coverage flags are set
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON \
  -DCMAKE_CXX_FLAGS="--coverage"

# Run all tests to accumulate coverage
for test in tests/unit/test_smmu*; do
    ./$test > /dev/null 2>&1
done

# Generate report
gcov ../src/smmu/smmu.cpp
```

## Performance

- **Execution Time**: ~17ms for all 70 tests
- **Memory Usage**: Minimal (test fixtures cleaned up after each test)
- **Build Time**: ~3-5 seconds (incremental)

## Contact & Support

For issues or questions about the Phase 4 test suite:
- Review: `PHASE4_TEST_SUITE_REPORT.md`
- Source: `tests/unit/test_smmu_phase4_coverage.cpp`
- Coverage: See `COVERAGE_REPORT.txt` for detailed line coverage

---

**Last Updated**: 2026-01-14
**Test Count**: 70
**Status**: ✅ All tests passing
