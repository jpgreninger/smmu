# ARM SMMU v3 Integration Test Suite

## Quick Reference

### Build and Run Tests

```bash
# Build all integration tests
cd build
make integration_tests

# Run all integration tests
make run_integration_tests

# Run with CTest
ctest -L integration --output-on-failure

# Run specific test
./tests/integration/test_stream_isolation

# List all integration tests
ctest -N -L integration
```

### Test Suites

| Test Suite | Status | Tests | Pass Rate | File |
|------------|--------|-------|-----------|------|
| test_minimal_integration | ✅ PASS | 5/5 | 100% | test_minimal_integration.cpp |
| test_stream_isolation | ✅ PASS | 9/9 | 100% | test_stream_isolation.cpp |
| test_two_stage_translation | ❌ FAIL | 0/10 | 0% | test_two_stage_translation.cpp |
| test_pasid_context_switching | ⚠️ PARTIAL | 6/10 | 60% | test_pasid_context_switching.cpp |
| test_large_scale_scalability | ⚠️ TIMEOUT | ?/6 | ? | test_large_scale_scalability.cpp |

### Known Issues

#### 1. Two-Stage Translation (0/10 passing)
**Issue**: PASID 0 not created for Stage-2 hypervisor context
**Fix**: Add `smmu->createStreamPASID(testStreamID, 0)` in test setup
**Estimated Time**: 15 minutes

#### 2. Large-Scale Scalability (timeout)
**Issue**: Test execution exceeds 30-second timeout
**Fix**: Reduce test scale or increase timeout
**Estimated Time**: 1-2 hours

#### 3. PASID Switching Performance (test failing)
**Issue**: 3.64μs per switch vs 1.0μs target
**Fix**: Profile and optimize PASID switching code path
**Estimated Time**: 2-4 hours

### Documentation

- **INTEGRATION_TEST_SUMMARY.md** - Original implementation summary with detailed test descriptions
- **INTEGRATION_TEST_SUITE_STATUS.md** - Current status report with execution results and fixes
- **test_integration_base.h** - Common test utilities and helper functions

### Test Categories

#### Minimal Integration (5 tests)
Basic API validation and integration baseline

#### Stream Isolation (9 tests) ✅
- Stream address space isolation
- Security state isolation
- Fault isolation
- Cache isolation
- Large-scale testing (100 streams)

#### Two-Stage Translation (10 tests) ❌
- IOVA → IPA → PA pipeline
- Stage-1 and Stage-2 fault handling
- Permission intersection
- Security state validation
- Performance validation

#### PASID Context Switching (10 tests) ⚠️
- PASID lifecycle management
- Context isolation
- Cache behavior
- Performance validation
- Resource limits

#### Large-Scale Scalability (6 tests) ⚠️
- 1000 streams, 50 PASIDs per stream
- 200,000 translation operations
- 16-thread concurrent access
- Memory scalability
- Cache efficiency

### CI/CD Integration

#### Recommended Pipeline Configuration

```yaml
test_integration:
  stage: test
  script:
    - mkdir -p build && cd build
    - cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
    - make -j$(nproc)
    - ctest -L integration --output-on-failure --timeout 60
  artifacts:
    when: always
    reports:
      junit: build/test_results/*.xml
  allow_failure: true  # Until all tests pass
```

#### Fast Feedback Loop (CI)

```bash
# Run only passing tests for quick validation
ctest -R "test_minimal_integration|test_stream_isolation"
```

#### Comprehensive Testing (Nightly)

```bash
# Run all integration tests with extended timeout
ctest -L integration --timeout 300 --output-on-failure
```

### Performance Targets

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| Translation latency | <10μs | ~135ns | ✅ Excellent |
| PASID switch time | <1μs | 3.64μs | ❌ Needs optimization |
| Throughput | >100K ops/sec | ? | ⚠️ Needs measurement |
| Cache hit rate | >80% | ? | ⚠️ Needs measurement |

### Integration Test Matrix

```
Build System Integration:     ✅ COMPLETE
CTest Registration:           ✅ COMPLETE
Test Compilation:            ✅ COMPLETE
Custom Targets:              ✅ COMPLETE
Test Execution:              ⚠️ ~50% passing
Overall Status:              ⚠️ Needs fixes
```

### Next Actions

1. Fix PASID 0 creation in two-stage translation tests (15 min)
2. Address large-scale scalability timeout (1-2 hrs)
3. Optimize PASID switching performance (2-4 hrs)
4. Implement PASID resource limits (1-2 hrs)

**Estimated Time to 100% Pass Rate**: 4-8 hours

### Contact

For questions or issues with the integration test suite, refer to:
- INTEGRATION_TEST_SUITE_STATUS.md (current status)
- INTEGRATION_TEST_SUMMARY.md (implementation details)
- CLAUDE.md (project guidelines)
