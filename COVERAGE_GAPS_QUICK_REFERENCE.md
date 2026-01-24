# Coverage Gaps - Quick Reference

## Current Status: 86% (2,045/2,376 lines)

### Immediate Priorities

#### 1. stream_context.cpp - 27% Coverage (CRITICAL - 317 lines missing)
**Most Critical Gaps:**
- Lines 492-543: Two-stage translation coordination (52 lines)
- Lines 602-644: Fault recording logic (43 lines)  
- Lines 660-730: Permission validation (71 lines)
- Lines 734-805: Security state management (72 lines)
- Lines 809-872: Stream statistics (64 lines)

**Quick Wins (40 tests, Week 1):**
1. PASID validation errors (lines 56, 61, 90, 96) - 8 tests
2. Address space operation errors (lines 197-236) - 10 tests
3. Configuration errors (lines 285-335) - 8 tests
4. Security validation (lines 339-384) - 8 tests
5. Resource limits (lines 114-133) - 6 tests

**Expected: 27% → 55% coverage**

#### 2. smmu.cpp - 71% Coverage (CRITICAL - 286 lines missing)
**Most Critical Gaps:**
- Lines 636-740: performTwoStageTranslation errors (105 lines)
- Lines 796-838: Dead code? lookupTranslationCache (43 lines)
- Lines 551-557: Unused recordCacheHit/Miss (7 lines)
- Lines 853-892: Address size validation (40 lines)

**Quick Wins (15 tests, Week 1):**
1. Constructor invalid config fallback (line 51) - 1 test
2. Two-stage null StreamContext (lines 654-665) - 1 test
3. Translation bypass mode (lines 674-678) - 1 test
4. Stage configuration errors (lines 692-724) - 4 tests
5. Permission failures (lines 729-740) - 3 tests
6. Cache operations (lines 102, 135, 636-639) - 3 tests
7. Event handler errors (lines 418, 431) - 2 tests

**Expected: 71% → 78% coverage**

### Week 1 Goal: 86% → 90% Overall Coverage

**Test Breakdown:**
- stream_context.cpp: 40 tests → +28% coverage
- smmu.cpp: 15 tests → +7% coverage
- Total: 55 tests, ~2-3 days effort

---

## Component Details

### stream_context.cpp Gap Categories

| Category | Lines | Tests | Priority |
|----------|-------|-------|----------|
| Error paths | 56-206 | 18 | CRITICAL |
| Configuration | 285-335 | 8 | CRITICAL |
| Security | 339-384 | 8 | CRITICAL |
| Two-stage | 492-543 | 10 | CRITICAL |
| Permissions | 660-730 | 7 | HIGH |
| Statistics | 809-872 | 8 | MEDIUM |
| Edge cases | Various | 6 | MEDIUM |

### smmu.cpp Gap Categories

| Category | Lines | Tests | Priority |
|----------|-------|-------|----------|
| Two-stage errors | 636-740 | 8 | CRITICAL |
| Address validation | 853-892 | 3 | HIGH |
| Permission/Security | 938-956 | 6 | HIGH |
| Dead code | 551-557, 796-838 | 5 | LOW |
| Config errors | 51, 203, 285, 306 | 4 | MEDIUM |
| Cache ops | 102, 135, 636-639 | 3 | MEDIUM |

### Other Components (Lower Priority)

| Component | Coverage | Gap | Tests | Time |
|-----------|----------|-----|-------|------|
| address_space.cpp | 94% | 14 | 10 | 4h |
| configuration.cpp | 97% | 7 | 7 | 2h |
| tlb_cache.cpp | 98% | 5 | 2 | 1h |
| fault_handler.cpp | 98% | 2 | 1 | 30m |

---

## Specific Line Numbers to Target

### stream_context.cpp - Top 20 Priority Lines
1. Lines 492-543: Two-stage translation (52 lines) - HIGHEST PRIORITY
2. Lines 660-730: Permission validation (71 lines)
3. Lines 734-805: Security management (72 lines)
4. Lines 602-644: Fault recording (43 lines)
5. Lines 809-872: Statistics (64 lines)

### smmu.cpp - Top 20 Priority Lines
1. Lines 636-740: performTwoStageTranslation (105 lines) - HIGHEST PRIORITY
2. Lines 853-892: Address size validation (40 lines)
3. Lines 938-956: Permission/security checks (19 lines)
4. Lines 654-665: Null StreamContext check (12 lines)
5. Lines 692-724: Stage configuration errors (33 lines)

---

## Test Templates

### Template 1: Error Path Test
```cpp
TEST(StreamContextTest, CreatePASID_ExceedsMaxPASID_ReturnsError) {
    StreamContext ctx;
    PASID invalidPASID = MAX_PASID + 1;
    
    VoidResult result = ctx.createPASID(invalidPASID);
    
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPASID);
}
```

### Template 2: Two-Stage Translation Test
```cpp
TEST(SMMUTest, PerformTwoStage_NullStreamContext_ReturnsError) {
    SMMU smmu;
    
    TranslationResult result = smmu.performTwoStageTranslation(
        STREAM1, PASID1, TEST_IOVA, AccessType::Read, 
        SecurityState::NonSecure, nullptr);
    
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::StreamNotConfigured);
}
```

### Template 3: Security Validation Test
```cpp
TEST(StreamContextTest, Translate_SecurityStateMismatch_ReturnsFault) {
    StreamContext ctx;
    ctx.createPASID(PASID1);
    
    // Map page with Secure state
    ctx.mapPageForPASID(PASID1, IOVA1, PA1, perms, SecurityState::Secure);
    
    // Try to access with NonSecure state
    TranslationResult result = ctx.translate(
        PASID1, IOVA1, AccessType::Read, SecurityState::NonSecure);
    
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getFaultType(), FaultType::SecurityFault);
}
```

---

## Action Plan

### Day 1-2: stream_context.cpp Error Paths (20 tests)
- PASID validation (8 tests)
- Address space operations (10 tests)  
- Configuration errors (2 tests)

### Day 3: stream_context.cpp Security & Resources (20 tests)
- Security validation (8 tests)
- Configuration errors (6 tests)
- Resource limits (6 tests)

### Day 4-5: smmu.cpp Critical Paths (15 tests)
- Constructor/config (4 tests)
- Two-stage errors (8 tests)
- Cache operations (3 tests)

### Week 2: Advanced Features & Remaining Components
- stream_context.cpp two-stage (25 tests)
- smmu.cpp advanced (12 tests)
- address_space.cpp (10 tests)
- configuration.cpp (7 tests)

### Week 3: Final Polish
- stream_context.cpp state/edge cases (35 tests)
- smmu.cpp dead code (5 tests)
- tlb_cache.cpp (2 tests)
- fault_handler.cpp (1 test)

---

## Success Metrics

**Week 1 Target:**
- Overall: 86% → 90%
- stream_context.cpp: 27% → 55%
- smmu.cpp: 71% → 78%
- Tests added: 55
- All tests passing

**Week 2 Target:**
- Overall: 90% → 95%
- stream_context.cpp: 55% → 75%
- smmu.cpp: 78% → 85%
- address_space.cpp: 94% → 99%
- Tests added: 54
- All tests passing

**Week 3 Target:**
- Overall: 95% → 98%+
- stream_context.cpp: 75% → 98%
- smmu.cpp: 85% → 90%
- All components: 98%+
- Tests added: 44
- All tests passing

**Total: ~153 new tests, 98%+ overall coverage**

---

**Quick Start:**
1. Read: `/home/jpgreninger/Work/smmu/COVERAGE_GAP_ANALYSIS.md`
2. Start with: stream_context.cpp error paths (8 PASID tests)
3. Build incrementally, verify coverage after each batch
4. Use templates above for consistency

**Report Generated**: 2026-01-23
