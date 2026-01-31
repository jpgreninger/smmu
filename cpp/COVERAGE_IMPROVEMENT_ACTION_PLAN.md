# ARM SMMU v3 Coverage Improvement Action Plan
**Date:** 2026-01-24
**Current Coverage:** 73% (1,745/2,376 lines)
**Target Coverage:** 100%

## Quick Reference

### New Test Files Created
1. `/home/jpgreninger/Work/smmu/tests/unit/test_smmu_two_stage_comprehensive.cpp` (18 tests, 11 passing)
2. `/home/jpgreninger/Work/smmu/tests/unit/test_stream_context_two_stage_advanced.cpp` (31 tests, 22 passing)

### Coverage Targets Addressed
- **smmu.cpp:** 164 lines targeted (+16% projected improvement)
- **stream_context.cpp:** 302 lines targeted (+69% projected improvement)
- **Overall:** 466 lines targeted (+20% projected improvement to 93%)

## Immediate Actions (Priority 1)

### 1. Fix 16 Failing Tests
**Estimated Time:** 2-3 hours

#### Action Items:
1. **Enable streams before translation (9 tests in stream_context):**
   ```cpp
   // Add this after configuration
   ASSERT_TRUE(streamContext->enableStream().isOk());
   ```

   **Files to modify:**
   - test_stream_context_two_stage_advanced.cpp
   - Tests: PermissionValidation_*, Security_*, Statistics_*

2. **Fix PASID creation sequence (7 tests in smmu):**
   ```cpp
   // Ensure proper sequence
   ASSERT_TRUE(smmu->configureStream(streamID, config).isOk());
   ASSERT_TRUE(smmu->createStreamPASID(streamID, pasid).isOk());
   ```

   **Files to modify:**
   - test_smmu_two_stage_comprehensive.cpp
   - Tests: TwoStage_*, Permission_*, MultiStream_*, MultiPASID_*

### 2. Generate Coverage Report
**Estimated Time:** 30 minutes

```bash
cd /home/jpgreninger/Work/smmu/build

# Build with coverage flags
cmake .. -DCMAKE_BUILD_TYPE=Debug \
         -DCMAKE_CXX_FLAGS="--coverage -fprofile-arcs -ftest-coverage"
make clean
make -j$(nproc)

# Run tests
ctest --output-on-failure

# Generate coverage report
lcov --capture --directory . \
     --output-file coverage.info \
     --exclude '*/googletest/*' \
     --exclude '*/tests/*' \
     --exclude '/usr/*'

# Generate HTML report
genhtml coverage.info --output-directory coverage_html

# View summary
lcov --summary coverage.info
```

### 3. Validate Coverage Improvement
**Estimated Time:** 30 minutes

Compare coverage before/after:
- **Before:** 73% (1,745/2,376 lines)
- **Expected:** 93% (2,211/2,376 lines)
- **Verify:** Lines 492-543, 602-644, 660-872 in stream_context.cpp
- **Verify:** Lines 636-740, 853-892, 938-956 in smmu.cpp

## Short-Term Actions (Priority 2)

### 4. Create Edge Case Tests for Minor Gaps
**Estimated Time:** 2-3 hours

#### address_space.cpp (14 lines to cover)
Create: `test_address_space_final_coverage.cpp`
```cpp
// Target Lines: 127, 162-164, 184-186, 199, 204-206, 260, 450, 504
- Unmap non-existent page (Line 127)
- Page size validation (Lines 162-164)
- Security state validation (Lines 184-186)
- Translation statistics edge cases (Lines 204-206)
- Cache invalidation (Lines 260, 450, 504)
```

#### configuration.cpp (7 lines to cover)
Create: `test_configuration_final_coverage.cpp`
```cpp
// Target Lines: 35, 137, 140, 143, 147, 448, 459
- Invalid queue sizes (Line 35)
- Resource limit boundaries (Lines 137-147)
- Configuration parsing errors (Lines 448, 459)
```

#### tlb_cache.cpp (5 lines to cover)
Create: `test_tlb_cache_final_coverage.cpp`
```cpp
// Target Lines: 313-314, 380-382
- Cache eviction edge cases (Lines 313-314)
- Invalid cache operations (Lines 380-382)
```

#### fault_handler.cpp (2 lines to cover)
Create: `test_fault_handler_final_coverage.cpp`
```cpp
// Target Lines: 131-132
- Fault queue overflow (Lines 131-132)
```

### 5. Update CMakeLists.txt
**Estimated Time:** 15 minutes

```cmake
set(UNIT_TEST_SOURCES
    ...
    test_address_space_final_coverage.cpp
    test_configuration_final_coverage.cpp
    test_tlb_cache_final_coverage.cpp
    test_fault_handler_final_coverage.cpp
)
```

## Medium-Term Actions (Priority 3)

### 6. Fix test_smmu_phase5_errors.cpp (7 failing tests)
**Estimated Time:** 2-3 hours

**Current Issues:**
- Security state API misunderstanding
- Invalid stream ID validation
- Cache expiration timing
- Configuration error codes

**Recommended Approach:**
1. Review ARM SMMU v3 spec for security state handling
2. Validate error code expectations against implementation
3. Adjust tests to match actual API behavior
4. Add comments explaining spec requirements

### 7. Create Stress and Performance Tests
**Estimated Time:** 4-6 hours

```cpp
// test_smmu_stress.cpp
- 1000+ concurrent streams
- 10000+ PASIDs across streams
- 1M+ translations
- Memory pressure scenarios
- Cache thrashing scenarios

// test_smmu_performance.cpp
- Translation latency benchmarks
- Cache hit rate validation
- Scalability measurements
- Throughput testing
```

## Long-Term Actions (Priority 4)

### 8. Implement Test Infrastructure Improvements
**Estimated Time:** 1-2 days

1. **Coverage Automation Script:**
   ```bash
   #!/bin/bash
   # scripts/generate_coverage.sh
   # Automated coverage report generation
   ```

2. **Coverage Visualization:**
   - Line-by-line coverage view
   - Function coverage summary
   - Branch coverage analysis

3. **CI/CD Integration:**
   ```yaml
   # .github/workflows/coverage.yml
   - Run tests
   - Generate coverage
   - Upload to codecov/coveralls
   - Fail if coverage decreases
   ```

### 9. Create Traceability Matrix
**Estimated Time:** 1 day

Map tests to ARM SMMU v3 specification sections:
```markdown
| Spec Section | Requirement | Test Case | Coverage |
|--------------|-------------|-----------|----------|
| 6.3.1 | Two-stage translation | test_smmu_two_stage_comprehensive.cpp | 100% |
| 6.3.2 | Permission checking | test_stream_context_two_stage_advanced.cpp | 95% |
| ... | ... | ... | ... |
```

### 10. Documentation and Knowledge Transfer
**Estimated Time:** 2-3 days

1. **Test Automation Guide:**
   - Best practices for writing SMMU tests
   - Common patterns and anti-patterns
   - Debugging failing tests

2. **Coverage Analysis Guide:**
   - How to identify coverage gaps
   - Prioritizing test cases
   - Measuring test effectiveness

3. **Maintenance Guide:**
   - Keeping tests up-to-date
   - Refactoring test code
   - Managing test dependencies

## Success Metrics

### Immediate Success (Week 1)
- [ ] 16 failing tests fixed (100% test pass rate)
- [ ] Coverage report generated
- [ ] Coverage improvement validated (93%+)

### Short-Term Success (Week 2)
- [ ] Edge case tests created (26 lines covered)
- [ ] Coverage reaches 95%+
- [ ] All test suites passing

### Medium-Term Success (Month 1)
- [ ] Coverage reaches 98%+
- [ ] Stress tests implemented
- [ ] Performance benchmarks established

### Long-Term Success (Month 2-3)
- [ ] Coverage reaches 100%
- [ ] CI/CD integration complete
- [ ] Full traceability matrix
- [ ] Comprehensive documentation

## Risk Mitigation

### Risk 1: API Misunderstandings
**Impact:** High (16 tests failing)
**Mitigation:**
- Review API documentation carefully
- Consult ARM SMMU v3 specification
- Add API usage examples to test documentation

### Risk 2: Unreachable Code
**Impact:** Medium (some lines may be unreachable)
**Mitigation:**
- Analyze uncovered lines carefully
- Document why lines are unreachable
- Consider removing dead code

### Risk 3: Test Maintenance Burden
**Impact:** Medium (49+ tests to maintain)
**Mitigation:**
- Follow DRY principle
- Create test utilities/helpers
- Use fixtures for common setup
- Document test intentions clearly

## Resource Requirements

### Personnel
- **Developer Time:** 40-60 hours for Priority 1-2 actions
- **QA Time:** 20-30 hours for validation and documentation
- **DevOps Time:** 10-15 hours for CI/CD integration

### Infrastructure
- **Build Server:** For continuous integration
- **Coverage Server:** For hosting coverage reports
- **Documentation Server:** For test documentation

### Tools
- **lcov/gcov:** Coverage measurement (already available)
- **codecov/coveralls:** Coverage tracking (optional)
- **Doxygen:** Test documentation generation (optional)

## Timeline

### Week 1: Immediate Actions
- **Day 1-2:** Fix 16 failing tests
- **Day 3:** Generate coverage report
- **Day 4-5:** Validate coverage improvement, create edge case tests

### Week 2: Short-Term Actions
- **Day 1-3:** Create and test edge case tests
- **Day 4-5:** Fix test_smmu_phase5_errors.cpp, documentation

### Week 3-4: Medium-Term Actions
- **Day 1-10:** Stress tests, performance tests, infrastructure improvements

### Month 2-3: Long-Term Actions
- **Ongoing:** CI/CD integration, traceability matrix, documentation

## Conclusion

Clear path to 100% coverage with:
1. **Immediate wins:** Fix 16 failing tests (93% coverage)
2. **Short-term wins:** Edge case tests (95% coverage)
3. **Medium-term wins:** Comprehensive test suite (98% coverage)
4. **Long-term wins:** Perfect coverage with full automation (100% coverage)

**Recommendation:** Execute Priority 1 actions immediately to validate approach and demonstrate value. Then proceed with Priority 2-4 actions based on project priorities and resource availability.
