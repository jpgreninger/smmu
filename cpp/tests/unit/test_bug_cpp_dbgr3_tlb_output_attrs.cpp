// TDD test for BUG-CPP-DBGR-3:
// TLB cache hit returns TranslationData with STE output-attribute fields zeroed.
//
// The fast-path TLB hit in SMMU::translate() constructs TranslationData with the
// 3-argument constructor (PA, permissions, securityState) and returns immediately.
// This zeroes memType, shareability, allocHint, instCfg, privCfg, and nsCfgOut
// even when the stream has non-zero STE output-attribute overrides configured.
//
// BEFORE FIX (expected failure):
//   - First translate() (cache miss, slow path): returns correct output attrs.
//   - Second translate() same IOVA (cache hit, fast path): returns all zeros.
//
// AFTER FIX:
//   - Both translates return the correct output attributes from the stream config.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================================
// Fixture
// ============================================================================
class TlbOutputAttrsTest : public ::testing::Test {
protected:
    static constexpr StreamID STREAM_ID = 0xE0;
    static constexpr PASID    PASID0    = 0;
    static constexpr IOVA     IOVA_VAL  = 0x30000ULL;
    static constexpr PA       PA_VAL    = 0xC0000000ULL;

    void SetUp() override {
        smmuObj = std::make_unique<SMMU>();
        smmuObj->enable();
        smmuObj->enableCaching(true);
    }

    // Build a stream config with non-zero STE output-attribute overrides.
    static StreamConfig makeConfigWithOutputAttrs() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.aa64               = true;
        // Non-zero output-attribute overrides under test
        cfg.mtCfg    = true;
        cfg.memAttr  = 7;    // will show up as memType=7
        cfg.shCfg    = 2;    // shareability=2 (OSH)
        cfg.allocCfg = 0xA;  // allocHint=0xA
        cfg.instCfg  = 1;    // instCfg=1
        cfg.privCfg  = 2;    // privCfg=2
        cfg.nsCfg    = 3;    // nsCfgOut=3
        return cfg;
    }

    std::unique_ptr<SMMU> smmuObj;
};

// -----------------------------------------------------------------------
// Test 1: Slow-path (cache miss) returns correct output attributes.
// This is a sanity check — it should pass both before and after the fix.
// -----------------------------------------------------------------------
TEST_F(TlbOutputAttrsTest, SlowPath_CacheMiss_OutputAttrsCorrect) {
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, makeConfigWithOutputAttrs()).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    // Invalidate TLB to guarantee a cache miss on this translate.
    smmuObj->invalidateStreamCache(STREAM_ID);

    auto result = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                      AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Cache-miss translate must succeed";

    const TranslationData& td = result.getValue();
    EXPECT_EQ(td.memType,      7u)    << "memType must be 7 (mtCfg=true, memAttr=7) on slow path";
    EXPECT_EQ(td.shareability, 2u)    << "shareability must be 2 (shCfg=2) on slow path";
    EXPECT_EQ(td.allocHint,    0xAu)  << "allocHint must be 0xA (allocCfg=0xA) on slow path";
    EXPECT_EQ(td.instCfg,      1u)    << "instCfg must be 1 on slow path";
    EXPECT_EQ(td.privCfg,      2u)    << "privCfg must be 2 on slow path";
    EXPECT_EQ(td.nsCfgOut,     3u)    << "nsCfgOut must be 3 (nsCfg=3) on slow path";
}

// -----------------------------------------------------------------------
// Test 2: Fast-path (cache hit) MUST return correct output attributes.
// BEFORE FIX this test FAILS because the TLB hit path returns all zeros.
// AFTER FIX this test PASSES.
// -----------------------------------------------------------------------
TEST_F(TlbOutputAttrsTest, FastPath_CacheHit_OutputAttrsCorrect) {
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, makeConfigWithOutputAttrs()).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    // Invalidate TLB first so the first translate is a guaranteed miss.
    smmuObj->invalidateStreamCache(STREAM_ID);

    // First translate: cache miss, slow path populates TLB.
    auto firstResult = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                           AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(firstResult.isOk()) << "First (cache-miss) translate must succeed";

    // Second translate of the SAME IOVA: must be a TLB cache hit.
    auto secondResult = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                            AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(secondResult.isOk()) << "Second (cache-hit) translate must succeed";

    const TranslationData& td = secondResult.getValue();
    // These assertions expose the bug: before the fix all fields are 0.
    EXPECT_EQ(td.memType,      7u)
        << "memType must be 7 on TLB cache hit (BUG: returns 0 before fix)";
    EXPECT_EQ(td.shareability, 2u)
        << "shareability must be 2 on TLB cache hit (BUG: returns 0 before fix)";
    EXPECT_EQ(td.allocHint,    0xAu)
        << "allocHint must be 0xA on TLB cache hit (BUG: returns 0 before fix)";
    EXPECT_EQ(td.instCfg,      1u)
        << "instCfg must be 1 on TLB cache hit (BUG: returns 0 before fix)";
    EXPECT_EQ(td.privCfg,      2u)
        << "privCfg must be 2 on TLB cache hit (BUG: returns 0 before fix)";
    EXPECT_EQ(td.nsCfgOut,     3u)
        << "nsCfgOut must be 3 on TLB cache hit (BUG: returns 0 before fix)";
}

// -----------------------------------------------------------------------
// Test 3: When mtCfg=false, memType must be 0 even on a cache hit.
// -----------------------------------------------------------------------
TEST_F(TlbOutputAttrsTest, FastPath_CacheHit_MtCfgFalse_MemTypeIsZero) {
    StreamConfig cfg = makeConfigWithOutputAttrs();
    cfg.mtCfg   = false;
    cfg.memAttr = 7; // memAttr is irrelevant when mtCfg=false
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    smmuObj->invalidateStreamCache(STREAM_ID);

    // Warm up the TLB.
    auto first = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                     AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(first.isOk());
    EXPECT_EQ(first.getValue().memType, 0u)
        << "memType must be 0 when mtCfg=false (slow path)";

    // Cache-hit path.
    auto second = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                      AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(second.isOk());
    EXPECT_EQ(second.getValue().memType, 0u)
        << "memType must be 0 when mtCfg=false (cache-hit path)";
}

// -----------------------------------------------------------------------
// Test 4: Physical address offset within page is preserved on cache hit.
// -----------------------------------------------------------------------
TEST_F(TlbOutputAttrsTest, FastPath_CacheHit_PageOffsetPreserved) {
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, makeConfigWithOutputAttrs()).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions perms(true, true, false);
    // Map the page-aligned base address.
    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    smmuObj->invalidateStreamCache(STREAM_ID);

    const IOVA iovaWithOffset = IOVA_VAL + 0x123ULL;

    // First translate (slow path, populates cache).
    auto first = smmuObj->translate(STREAM_ID, PASID0, iovaWithOffset,
                                     AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(first.isOk());
    EXPECT_EQ(first.getValue().physicalAddress, PA_VAL + 0x123ULL);

    // Second translate (cache hit).
    auto second = smmuObj->translate(STREAM_ID, PASID0, iovaWithOffset,
                                      AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(second.isOk());
    EXPECT_EQ(second.getValue().physicalAddress, PA_VAL + 0x123ULL)
        << "Page offset must be preserved on TLB cache hit";
    // Output attrs must also be correct.
    EXPECT_EQ(second.getValue().memType,      7u);
    EXPECT_EQ(second.getValue().shareability, 2u);
}

} // namespace test
} // namespace smmu
