// ARM SMMU v3 BUG-CPP-NEW-3: S1DSS bypass result incorrectly cached in TLB
// Copyright (c) 2024 John Greninger
//
// TDD failing test — written BEFORE the fix.
// ARM §3.9/§14.5 prohibits caching S1DSS bypass results in the TLB.
// The bug: smmu.cpp lines ~314-316 unconditionally cache the result of
// performTwoStageTranslation(), including the S1DSS==0x01 bypass early return.
//
// Test strategy (two approaches):
//
// APPROACH A — cache-hit counting:
//   After a bypass translate (s1dss=0x01, pasid=0), a second translate of the
//   SAME IOVA must NOT be a TLB hit.  If the bypass result was incorrectly
//   cached, the second translate will show as a TLB hit (hit count increases).
//   After the fix, both translates must be TLB misses (bypass skips TLB insert).
//
// APPROACH B — removeStream + reconfigure path:
//   After bypass + removeStream (which clears TLB) + reconfigure to s1dss=0x00:
//   The third translate must fail.  This also verifies the removeStream cleanup.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID STREAM_A  = 0xAB;
static constexpr PASID    PASID0    = 0;
static constexpr IOVA     IOVA_A    = 0x5000ULL;

// Helper: build a stage-1-only StreamConfig with given s1dss, s1cdMax.
static StreamConfig makeS1dssConfig(uint8_t s1dss, uint8_t s1cdMax = 4) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.s1dss              = s1dss;
    cfg.s1cdMax            = s1cdMax;
    return cfg;
}

// -----------------------------------------------------------------------
// APPROACH A: Verify bypass result is NOT cached by checking TLB hit count.
//
// Procedure:
//   1. Configure s1dss=0x01, reset statistics.
//   2. Translate PASID=0 twice.
//   3. After first translate: TLB miss expected (first-time lookup).
//   4. After second translate: if bypass was cached → TLB HIT (bug).
//                              if bypass not cached → TLB MISS (correct).
//
// Before fix: second translate is a TLB hit (bypass entry was cached).
// After fix:  second translate is a TLB miss (bypass is never cached).
// -----------------------------------------------------------------------
TEST(S1dssTlbCacheSpec, BypassResult_NeverCachedInTLB_HitCountVerification) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    ASSERT_TRUE(smmu->configureStream(STREAM_A, makeS1dssConfig(0x01)).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_A).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_A, PASID0).isOk());

    // Reset statistics so we start with clean counters.
    smmu->resetStatistics();

    uint64_t hitsAfterFirst, hitsAfterSecond;

    // First translate (bypass).
    auto r1 = smmu->translate(STREAM_A, PASID0, IOVA_A,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "S1DSS=0x01 bypass translate must succeed";
    EXPECT_EQ(r1.getValue().physicalAddress, IOVA_A) << "Bypass must return identity PA";
    hitsAfterFirst = smmu->getCacheHitCount();

    // Second translate of SAME iova/pasid.
    auto r2 = smmu->translate(STREAM_A, PASID0, IOVA_A,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r2.isOk()) << "Second bypass translate must succeed";
    hitsAfterSecond = smmu->getCacheHitCount();

    // KEY ASSERTION: no new TLB hit between first and second translate.
    // If bypass was cached after r1, r2 would hit the TLB (hitsAfterSecond > hitsAfterFirst).
    // After the fix, bypass is never cached, so hit count stays the same.
    EXPECT_EQ(hitsAfterSecond, hitsAfterFirst)
        << "S1DSS=0x01 bypass result must NOT be cached in TLB (BUG-CPP-NEW-3). "
           "Hit count increased between first and second bypass translate, meaning "
           "the bypass result was incorrectly inserted into the TLB.";
}

// -----------------------------------------------------------------------
// APPROACH B: removeStream + reconfigure path verifies cleanup.
// After bypass translate + removeStream (TLB cleared) + reconfigure to
// s1dss=0x00 (abort), third translate must return error.
// -----------------------------------------------------------------------
TEST(S1dssTlbCacheSpec, BypassResult_NotCachedInTLB_ReconfigDetected) {
    static constexpr StreamID STREAM_B = 0xAC;

    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    // Step 1: configure with s1dss=0x01 (bypass for PASID=0).
    ASSERT_TRUE(smmu->configureStream(STREAM_B, makeS1dssConfig(0x01)).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_B).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_B, PASID0).isOk());

    // Step 2: translate twice — both must return Ok (identity bypass).
    auto r1 = smmu->translate(STREAM_B, PASID0, IOVA_A,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "First bypass translate must succeed";
    EXPECT_EQ(r1.getValue().physicalAddress, IOVA_A) << "Bypass must return identity PA";

    auto r2 = smmu->translate(STREAM_B, PASID0, IOVA_A,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r2.isOk()) << "Second bypass translate must succeed";

    // Step 3: remove stream (calls invalidateStreamCache) + reconfigure with s1dss=0x00.
    ASSERT_TRUE(smmu->removeStream(STREAM_B).isOk());
    ASSERT_TRUE(smmu->configureStream(STREAM_B, makeS1dssConfig(0x00)).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_B).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_B, PASID0).isOk());

    // Step 4: translate again — must return an error (s1dss=0x00 aborts PASID=0).
    auto r3 = smmu->translate(STREAM_B, PASID0, IOVA_A,
                              AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r3.isError())
        << "After reconfigure to s1dss=0x00, translate must fail.";
}

// -----------------------------------------------------------------------
// Sanity: s1dss=0x02 (use CD[0]) result IS cacheable — second translate
// must be a TLB hit.  This confirms the fix guards ONLY s1dss==0x01.
// -----------------------------------------------------------------------
TEST(S1dssTlbCacheSpec, S1dss02_ResultIsCacheable_HitOnSecondTranslate) {
    static constexpr StreamID STREAM_C = 0xAD;
    static constexpr IOVA     IOVA_C   = 0x7000ULL;
    static constexpr PA       PA_C     = 0x91000000ULL;

    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg = makeS1dssConfig(0x02);  // use CD[0]
    ASSERT_TRUE(smmu->configureStream(STREAM_C, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_C).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_C, 0).isOk());
    ASSERT_TRUE(smmu->mapPage(STREAM_C, 0, IOVA_C, PA_C,
                              PagePermissions(true, false, false)).isOk());

    smmu->resetStatistics();

    // First translate — TLB miss, result cached.
    auto r1 = smmu->translate(STREAM_C, 0, IOVA_C, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "First translate (s1dss=0x02) must succeed";
    uint64_t hitsAfterFirst = smmu->getCacheHitCount();

    // Second translate — must be a TLB hit.
    auto r2 = smmu->translate(STREAM_C, 0, IOVA_C, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r2.isOk()) << "Second translate (s1dss=0x02) must succeed";
    uint64_t hitsAfterSecond = smmu->getCacheHitCount();

    EXPECT_GT(hitsAfterSecond, hitsAfterFirst)
        << "s1dss=0x02 translation result MUST be cached (second translate should be TLB hit)";
    if (r2.isOk()) {
        EXPECT_EQ(r2.getValue().physicalAddress, PA_C);
    }
}

} // namespace test
} // namespace smmu
