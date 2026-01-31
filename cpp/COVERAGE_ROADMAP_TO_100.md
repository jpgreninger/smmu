# Coverage Improvement Roadmap to 100%

**Document Version**: 1.0
**Created**: 2026-01-24
**Current Coverage**: 88.51% (2,103/2,376 lines)
**Target Coverage**: 100%
**Remaining Lines**: 273

---

## Executive Summary

This roadmap provides a strategic plan to improve test coverage from the current 88.51% to 100%. The primary focus is on **smmu.cpp** which contains 225 of the remaining 273 uncovered lines (82% of the gap).

### Key Metrics

| Metric | Current | Target | Gap |
|--------|---------|--------|-----|
| **Overall Coverage** | 88.51% | 100% | 11.49% |
| **Lines Covered** | 2,103 | 2,376 | 273 |
| **Test Pass Rate** | 100% | 100% | ✅ |
| **Test Count** | 190+ | 250+ | 60+ tests |

### Timeline

- **Phase 1** (Week 1): 88.51% → 93% (smmu.cpp focus)
- **Phase 2** (Week 2): 93% → 97% (smmu.cpp completion)
- **Phase 3** (Week 3): 97% → 99% (polish all components)
- **Phase 4** (Week 4): 99% → 100% (unreachable code analysis)

---

## Current Status Analysis

### Component Coverage Breakdown

| Component | Coverage | Lines | Missing | Priority | Effort |
|-----------|----------|-------|---------|----------|--------|
| **smmu.cpp** | 77.52% | 776/1,001 | **225** | CRITICAL | 5 days |
| **stream_context.cpp** | 95.43% | 418/438 | 20 | LOW | 1 day |
| **address_space.cpp** | 94.29% | 231/245 | 14 | LOW | 4 hours |
| **configuration.cpp** | 97.60% | 285/292 | 7 | LOW | 2 hours |
| **tlb_cache.cpp** | 98.11% | 259/264 | 5 | LOW | 1 hour |
| **fault_handler.cpp** | 98.53% | 134/136 | 2 | LOW | 30 min |

### Gap Analysis

**Total Remaining**: 273 lines (11.49%)

**Distribution**:
- smmu.cpp: 225 lines (82.4% of gap) - **PRIMARY FOCUS**
- stream_context.cpp: 20 lines (7.3% of gap)
- address_space.cpp: 14 lines (5.1% of gap)
- configuration.cpp: 7 lines (2.6% of gap)
- tlb_cache.cpp: 5 lines (1.8% of gap)
- fault_handler.cpp: 2 lines (0.7% of gap)

---

## Phase 1: SMMU Core Translation Paths (Week 1)

**Goal**: 88.51% → 93%
**Focus**: smmu.cpp lines 636-892
**Estimated Effort**: 5 days

### 1.1 Two-Stage Translation Error Paths (Lines 636-740)

**Uncovered**: ~105 lines
**Tests Needed**: 25 tests
**Effort**: 2 days

#### Test Categories

**A. performTwoStageTranslation Error Handling (10 tests)**
- Null StreamContext pointer handling
- Translation bypass mode validation
- Stage configuration error cases:
  - Both stages disabled
  - Stage-1 only with no S1 address space
  - Stage-2 only with no S2 address space
  - Both enabled with missing address spaces
- Null address translation detection
- Stage-1 translation failure handling
- Stage-2 translation failure handling
- Permission validation failure in two-stage flow

**B. Stage Coordination Logic (8 tests)**
- Stage-1 to IPA translation
- IPA to Stage-2 translation
- Permission intersection between stages
- Address size propagation through stages
- Security state validation across stages
- Fault attribution (which stage failed)
- Translation result aggregation
- Cache interaction in two-stage flow

**C. Edge Cases (7 tests)**
- Maximum address size in two-stage
- Minimum address size in two-stage
- Mismatched address sizes between stages
- Permission conflicts between stages
- Security state transitions
- Concurrent translations
- Cache invalidation during two-stage translation

#### Expected Impact

- smmu.cpp: 77.52% → 87% (+9.5%)
- Overall: 88.51% → 91.5% (+3%)

---

### 1.2 Address Size Validation (Lines 853-892)

**Uncovered**: ~40 lines
**Tests Needed**: 15 tests
**Effort**: 1.5 days

#### Test Categories

**A. Input Address Size Validation (6 tests)**
- Valid sizes: 32, 36, 40, 44, 48, 52 bits
- Invalid sizes: 31, 33, 53, 64 bits
- Address overflow detection for each valid size
- Maximum address validation for each size
- IOVA truncation behavior
- Sign extension handling (if applicable)

**B. Output Address Size Validation (5 tests)**
- PA size validation for each supported size
- Output overflow detection
- PA truncation behavior
- Address range validation
- Physical address alignment

**C. Size Mismatch Handling (4 tests)**
- Input larger than output
- Output larger than input
- Stage-1 vs Stage-2 size mismatches
- Configuration validation on size change

#### Expected Impact

- smmu.cpp: 87% → 91% (+4%)
- Overall: 91.5% → 93% (+1.5%)

---

## Phase 2: SMMU Advanced Features (Week 2)

**Goal**: 93% → 97%
**Focus**: smmu.cpp remaining gaps + stream_context.cpp
**Estimated Effort**: 5 days

### 2.1 Permission and Security Validation (Lines 938-956, Others)

**Uncovered**: ~20 lines in smmu.cpp
**Tests Needed**: 10 tests
**Effort**: 1 day

#### Test Categories

**A. Permission Fault Handling (5 tests)**
- Stage-1 permission fault detection
- Stage-2 permission fault detection
- Permission intersection violations
- Execute permission validation
- Privilege level validation

**B. Security Fault Handling (5 tests)**
- Security state mismatch detection
- Secure to non-secure access attempts
- Realm state validation
- Cross-domain access control
- Security fault event generation

#### Expected Impact

- smmu.cpp: 91% → 93% (+2%)

---

### 2.2 Cache Logic and Dead Code (Lines 551-557, 796-838)

**Uncovered**: ~50 lines
**Tests Needed**: 8 tests
**Effort**: 1 day

#### Analysis Required

**Potentially Unreachable Code**:
- Lines 551-557: recordCacheHit/recordCacheMiss - may be unused
- Lines 796-838: lookupTranslationCache - may be dead code

**Actions**:
1. Review code to determine if reachable
2. If unreachable: Mark with `// LCOV_EXCL_START` ... `// LCOV_EXCL_STOP`
3. If reachable: Create tests to trigger these paths
4. Document findings in code comments

#### Test Categories (If Reachable)

**A. Cache Statistics (3 tests)**
- Cache hit recording
- Cache miss recording
- Statistics aggregation

**B. Cache Lookup Edge Cases (5 tests)**
- Empty cache lookup
- Expired entry lookup
- Concurrent cache access
- Cache entry eviction
- Cache invalidation race conditions

#### Expected Impact

- If unreachable (excluded): smmu.cpp coverage increases artificially
- If reachable (tested): smmu.cpp: 93% → 95% (+2%)
- Overall: 93% → 95% (+2%)

---

### 2.3 StreamContext Polish (20 Lines)

**Uncovered**: 20 lines
**Tests Needed**: 10-12 tests
**Effort**: 1 day

#### Areas to Target

**A. Error Path Coverage (from .gcov analysis)**
- Specific uncovered error conditions
- Boundary conditions in PASID management
- Edge cases in two-stage coordination
- Rare fault scenarios

**B. State Management Edge Cases**
- Concurrent configuration changes
- Rapid enable/disable cycles
- Resource exhaustion scenarios
- Recovery from error states

#### Expected Impact

- stream_context.cpp: 95.43% → 99% (+3.57%)
- Overall: 95% → 95.5% (+0.5%)

---

### 2.4 Configuration Error Paths (7 Lines)

**Uncovered**: 7 lines
**Tests Needed**: 5 tests
**Effort**: 2 hours

#### Test Categories

**A. Invalid Configuration Detection (3 tests)**
- Out-of-range parameter values
- Conflicting configuration options
- Hardware constraint violations

**B. Configuration Change Validation (2 tests)**
- Invalid transitions
- Change during active operation

#### Expected Impact

- configuration.cpp: 97.60% → 100% (+2.4%)
- Overall: 95.5% → 95.7% (+0.2%)

---

### 2.5 TLB Cache Edge Cases (5 Lines)

**Uncovered**: 5 lines
**Tests Needed**: 3 tests
**Effort**: 1 hour

#### Test Categories

**A. Cache Edge Cases (3 tests)**
- Cache entry expiration edge cases
- Eviction policy corner cases
- Invalidation race conditions

#### Expected Impact

- tlb_cache.cpp: 98.11% → 100% (+1.89%)
- Overall: 95.7% → 95.8% (+0.1%)

---

### 2.6 Fault Handler Completion (2 Lines)

**Uncovered**: 2 lines
**Tests Needed**: 1-2 tests
**Effort**: 30 minutes

#### Test Categories

**A. Rare Fault Scenarios (2 tests)**
- Specific fault type combinations
- Fault handler error conditions

#### Expected Impact

- fault_handler.cpp: 98.53% → 100% (+1.47%)
- Overall: 95.8% → 96% (+0.2%)

---

### 2.7 Address Space Edge Cases (14 Lines)

**Uncovered**: 14 lines
**Tests Needed**: 7 tests
**Effort**: 4 hours

#### Test Categories

**A. Exception Handling (3 tests)**
- Memory allocation failures
- Invalid page table operations
- Resource exhaustion

**B. Boundary Conditions (4 tests)**
- Maximum/minimum address values
- Page size boundary cases
- Alignment violations
- Overflow detection

#### Expected Impact

- address_space.cpp: 94.29% → 100% (+5.71%)
- Overall: 96% → 97% (+1%)

---

## Phase 3: Comprehensive Polish (Week 3)

**Goal**: 97% → 99%
**Focus**: Remaining gaps in smmu.cpp
**Estimated Effort**: 5 days

### 3.1 SMMU Remaining Lines (~60 Lines)

**Uncovered**: ~60 lines
**Tests Needed**: 25-30 tests
**Effort**: 3 days

#### Systematic Line-by-Line Coverage

**Process**:
1. Generate detailed .gcov report for smmu.cpp
2. Identify each uncovered line
3. Categorize by functionality
4. Create targeted tests for each category
5. Verify coverage incrementally

#### Expected Test Categories

- Stream lifecycle management
- Event queue operations
- Command queue processing
- Error recovery mechanisms
- Concurrent operation handling
- Performance optimization paths
- Rare configuration combinations

#### Expected Impact

- smmu.cpp: 95% → 98% (+3%)
- Overall: 97% → 98.5% (+1.5%)

---

### 3.2 Final Stream Context Lines (~10 Lines)

**Uncovered**: ~10 lines
**Tests Needed**: 5-8 tests
**Effort**: 1 day

#### Expected Impact

- stream_context.cpp: 99% → 100% (+1%)
- Overall: 98.5% → 99% (+0.5%)

---

## Phase 4: Unreachable Code Analysis (Week 4)

**Goal**: 99% → 100%
**Focus**: Identify and handle unreachable code
**Estimated Effort**: 2-3 days

### 4.1 Dead Code Analysis

**Process**:
1. Review all uncovered lines (expected: ~20 lines remaining)
2. Perform static analysis to determine reachability
3. For each uncovered block:
   - **If reachable**: Create test to cover it
   - **If unreachable**: Document reason and exclude from coverage
   - **If defensive**: Mark as defensive code and exclude

### 4.2 Coverage Exclusion Markers

**Use LCOV exclusion markers**:

```cpp
// LCOV_EXCL_START - Defensive code: Impossible condition
if (ptr == nullptr && ptr != nullptr) {
    // This can never happen but guards against future bugs
}
// LCOV_EXCL_STOP

// LCOV_EXCL_LINE - Debug-only code
assert(invariant_holds()); // LCOV_EXCL_LINE
```

### 4.3 Documentation

**Required Documentation**:
- Document each excluded block with justification
- Update COVERAGE_REPORT.txt with exclusion explanations
- Create UNREACHABLE_CODE_ANALYSIS.md documenting:
  - Each unreachable code block
  - Why it's unreachable
  - Whether it should be removed or kept

### Expected Impact

- Overall: 99% → 100% (+1%)
- Adjusted coverage with exclusions: 100%

---

## Test Development Guidelines

### Test Quality Standards

**All new tests must meet**:
- ✅ Follow GoogleTest framework conventions
- ✅ Use clear, descriptive test names
- ✅ Include setup and teardown in fixtures
- ✅ Test one concept per test case
- ✅ Use ASSERT for prerequisites, EXPECT for verifications
- ✅ Include comments explaining complex scenarios
- ✅ Align with ARM SMMU v3 specification
- ✅ Execute in <10ms per test
- ✅ Be deterministic (no flaky tests)
- ✅ Be independent (no test interdependencies)

### Test Naming Convention

```
ComponentName_Scenario_ExpectedBehavior

Examples:
- SMMU_TwoStageTranslation_BothStagesDisabled_ReturnsError
- StreamContext_PASIDExceedsMax_ReturnsInvalidPASID
- AddressSpace_PageNotMapped_ReturnsTranslationFault
```

### Coverage Verification Process

After each test batch:

```bash
# Rebuild with coverage
cd build
make clean
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
make -j$(nproc)

# Run tests
ctest -j4

# Generate coverage
gcov CMakeFiles/smmu_lib.dir/src/smmu/smmu.cpp.gcda

# Verify improvement
# Compare with previous COVERAGE_REPORT.txt
```

---

## Estimated Effort Summary

### Time Breakdown

| Phase | Duration | Tests | Lines | Coverage Gain |
|-------|----------|-------|-------|---------------|
| **Phase 1** | 5 days | 40 | 145 | 88.51% → 93% |
| **Phase 2** | 5 days | 50 | 108 | 93% → 97% |
| **Phase 3** | 5 days | 35 | 70 | 97% → 99% |
| **Phase 4** | 3 days | 5-10 | 20-30 | 99% → 100% |
| **TOTAL** | **18 days** | **130-145** | **273** | **88.51% → 100%** |

### Resource Requirements

- **Developer Time**: 18 days (3.6 weeks at 5 days/week)
- **QA Review**: 2 days
- **Documentation**: 1 day
- **Total Calendar Time**: 4 weeks

### Milestones

| Date | Milestone | Coverage Target |
|------|-----------|-----------------|
| Week 1 End | Phase 1 Complete | 93% |
| Week 2 End | Phase 2 Complete | 97% |
| Week 3 End | Phase 3 Complete | 99% |
| Week 4 End | Phase 4 Complete | 100% |

---

## Risk Assessment

### High Risks

**1. Unreachable Code Estimation**
- **Risk**: May have more unreachable code than estimated
- **Impact**: Could inflate coverage % if too much code is excluded
- **Mitigation**:
  - Strict review process for exclusions
  - Document all exclusions thoroughly
  - Limit exclusions to <1% of codebase

**2. SMMU Complexity**
- **Risk**: Two-stage translation paths are complex, may take longer
- **Impact**: Phase 1 could extend beyond 5 days
- **Mitigation**:
  - Break down into smaller sub-tasks
  - Use debugger agent proactively
  - Leverage existing test patterns

**3. Test Maintenance**
- **Risk**: Adding 130+ tests increases maintenance burden
- **Impact**: Long-term CI/CD runtime increase
- **Mitigation**:
  - Keep tests focused and fast (<10ms each)
  - Use parameterized tests where applicable
  - Organize tests into logical suites

### Medium Risks

**4. Coverage Tool Accuracy**
- **Risk**: gcov may report inaccurate coverage in template-heavy code
- **Impact**: May not reach true 100% even with all reachable code tested
- **Mitigation**:
  - Use multiple coverage tools (gcov, lcov, gcovr)
  - Manual code review of uncovered lines
  - Cross-reference with static analysis

**5. ARM Spec Compliance**
- **Risk**: Tests may not align with ARM SMMU v3 specification
- **Impact**: False confidence in implementation correctness
- **Mitigation**:
  - Use qa-engineer agent for spec review
  - Reference specification for each test category
  - Document spec sections in test comments

---

## Success Criteria

### Phase Completion Criteria

**Phase 1**:
- ✅ smmu.cpp coverage ≥ 87%
- ✅ Overall coverage ≥ 93%
- ✅ 40 new tests added
- ✅ All tests passing (100% pass rate)
- ✅ Zero compilation warnings

**Phase 2**:
- ✅ smmu.cpp coverage ≥ 95%
- ✅ All components except smmu.cpp at 100%
- ✅ Overall coverage ≥ 97%
- ✅ 50 new tests added
- ✅ Dead code analysis complete

**Phase 3**:
- ✅ smmu.cpp coverage ≥ 98%
- ✅ Overall coverage ≥ 99%
- ✅ 35 new tests added
- ✅ All reachable code tested

**Phase 4**:
- ✅ Overall coverage = 100% (with justified exclusions)
- ✅ Unreachable code documented
- ✅ All exclusions reviewed and approved
- ✅ Final documentation complete

### Final Success Criteria

**Code Coverage**:
- ✅ Overall coverage: 100% (or 99%+ with <1% justified exclusions)
- ✅ All components at 95%+ coverage
- ✅ No untested error paths in critical code

**Test Quality**:
- ✅ 100% test pass rate maintained throughout
- ✅ All tests execute in <1 second total per suite
- ✅ Zero flaky tests
- ✅ ARM SMMU v3 specification compliance verified

**Documentation**:
- ✅ COVERAGE_REPORT.txt updated
- ✅ All coverage gaps documented
- ✅ Unreachable code analysis complete
- ✅ Test documentation up to date

**Process**:
- ✅ CI/CD integration maintained
- ✅ Zero compilation warnings
- ✅ Zero compilation errors
- ✅ Code review completed for all new tests

---

## Tools and Automation

### Coverage Generation

```bash
#!/bin/bash
# generate_coverage.sh - Automated coverage report generation

set -e

# Clean and rebuild
cd build
make clean
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON \
    -DCMAKE_CXX_FLAGS="--coverage" \
    -DCMAKE_EXE_LINKER_FLAGS="--coverage"
make -j$(nproc)

# Run all tests
ctest -j4 --output-on-failure

# Generate coverage data
lcov --capture --directory . --output-file coverage.info
lcov --remove coverage.info '/usr/*' '*/test/*' --output-file coverage_filtered.info

# Generate HTML report
genhtml coverage_filtered.info --output-directory coverage_html

# Generate text report
gcov ../src/**/*.cpp > /dev/null 2>&1
python3 ../scripts/generate_coverage_report.py > ../COVERAGE_REPORT.txt

# Display summary
echo "Coverage Report Generated:"
cat ../COVERAGE_REPORT.txt

# Open HTML report
xdg-open coverage_html/index.html 2>/dev/null || \
    open coverage_html/index.html 2>/dev/null || \
    echo "HTML report available at: coverage_html/index.html"
```

### Line-by-Line Analysis

```bash
#!/bin/bash
# analyze_uncovered_lines.sh - Identify specific uncovered lines

COMPONENT=$1  # e.g., "smmu.cpp"

if [ -z "$COMPONENT" ]; then
    echo "Usage: $0 <component.cpp>"
    exit 1
fi

cd build

# Generate gcov for specific component
gcov CMakeFiles/smmu_lib.dir/src/*/$COMPONENT.gcda

# Extract uncovered lines
GCOV_FILE="../$COMPONENT.gcov"
if [ -f "$GCOV_FILE" ]; then
    echo "Uncovered lines in $COMPONENT:"
    grep -E "^\s+#####:" "$GCOV_FILE" | \
        awk '{print "Line " $1 ": " substr($0, index($0,$3))}'
else
    echo "Error: $GCOV_FILE not found"
    exit 1
fi
```

### Progress Tracking

```bash
#!/bin/bash
# track_coverage_progress.sh - Track coverage improvements over time

LOGFILE="coverage_progress.log"

# Get current coverage
CURRENT=$(grep "TOTAL" ../COVERAGE_REPORT.txt | awk '{print $NF}' | tr -d '%')

# Log progress
echo "$(date '+%Y-%m-%d %H:%M:%S'),$CURRENT" >> $LOGFILE

# Display trend
echo "Coverage Progress:"
tail -10 $LOGFILE | column -t -s','

# Calculate trend
if [ $(wc -l < $LOGFILE) -gt 1 ]; then
    PREV=$(tail -2 $LOGFILE | head -1 | cut -d',' -f2)
    DIFF=$(echo "$CURRENT - $PREV" | bc)
    echo "Change since last measurement: ${DIFF}%"
fi
```

---

## Maintenance Plan

### Ongoing Coverage Maintenance

**Daily**:
- Run full test suite before commits
- Check coverage hasn't decreased

**Weekly**:
- Review coverage report
- Identify new gaps from code changes
- Create tests for new code

**Monthly**:
- Audit test quality
- Remove redundant tests
- Optimize slow tests
- Update coverage goals

**Quarterly**:
- Review unreachable code exclusions
- Update coverage documentation
- Assess test coverage effectiveness
- Plan coverage improvements for new features

### CI/CD Integration

**Pre-Commit Hook**:
```bash
# .git/hooks/pre-commit
#!/bin/bash
cd build
make test
if [ $? -ne 0 ]; then
    echo "Tests failed. Commit aborted."
    exit 1
fi
```

**Coverage Gate**:
- Fail build if coverage decreases
- Require 95%+ coverage for new files
- Require coverage improvement plan for <95% files

---

## Contact and Support

### Responsible Parties

- **Coverage Lead**: [Assign owner]
- **Test Development**: [Assign developers]
- **QA Review**: [Assign QA engineer]
- **Specification Compliance**: [Assign architect]

### Review Schedule

- **Weekly Progress Reviews**: Every Friday 2pm
- **Phase Gate Reviews**: End of each phase
- **Final Review**: End of Phase 4

---

## Appendices

### A. Detailed Line Analysis Commands

```bash
# Generate detailed coverage for specific component
cd build
gcov CMakeFiles/smmu_lib.dir/src/smmu/smmu.cpp.gcda

# View uncovered lines
grep "####" ../smmu.cpp.gcov | less

# Count uncovered lines
grep -c "####" ../smmu.cpp.gcov
```

### B. Test Template

```cpp
// Template for new coverage tests
TEST_F(ComponentTest, Scenario_Condition_ExpectedResult) {
    // Arrange - Set up test fixtures and inputs
    ComponentUnderTest component;
    InputType input = createTestInput();

    // Act - Execute the code under test
    ResultType result = component.methodUnderTest(input);

    // Assert - Verify expected behavior
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue(), expectedValue);
}
```

### C. Coverage Report Interpretation

**Line Markers**:
- `-`: Line not executable (whitespace, comments, declarations)
- `####`: Line not covered (NEEDS TEST)
- `N`: Line executed N times (COVERED)

**Branch Coverage**:
- `taken N%`: Branch taken N% of the time
- `never executed`: Branch never taken (NEEDS TEST)

### D. Related Documents

- `COVERAGE_REPORT.txt` - Current coverage statistics
- `PHASE5_COVERAGE_IMPROVEMENT_REPORT.md` - Phase 5 results
- `TEST_AUTOMATION_REPORT.md` - Test automation analysis
- `COVERAGE_GAPS_QUICK_REFERENCE.md` - Quick reference
- `ARM_SMMU_v3_PRD.md` - Product requirements
- `TASKS.md` - Implementation task tracker

---

**Roadmap Version**: 1.0
**Last Updated**: 2026-01-24
**Next Review**: Start of Phase 1
**Status**: READY FOR EXECUTION

---

*This roadmap provides a comprehensive, achievable plan to reach 100% code coverage for the ARM SMMU v3 implementation. Execute phases sequentially, verify progress incrementally, and adjust timeline as needed based on actual progress.*
