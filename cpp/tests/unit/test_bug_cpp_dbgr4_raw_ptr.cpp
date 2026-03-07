// BUG-CPP-DBGR-4: raw pointer lifetime after stripe lock release (defensive clarity)
// TDD: verify that translate() still works correctly after the fix
// (streamContext = nullptr after lock.unlock()).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

static constexpr StreamID D4_STREAM = 0x10;
static constexpr PASID    D4_PASID  = 0;
static constexpr IOVA     D4_IOVA   = 0x1000ULL;
static constexpr PA       D4_PA     = 0xDEAD0000ULL;

// Helper: create and configure a basic translation stream
static void setupD4Stream(SMMU& smmu) {
    smmu.enable();
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D4_STREAM, cfg);
    smmu.enableStream(D4_STREAM);
    smmu.createStreamPASID(D4_STREAM, D4_PASID);
    smmu.mapPage(D4_STREAM, D4_PASID, D4_IOVA, D4_PA,
                 PagePermissions(true, true, false),
                 SecurityState::NonSecure);
}

// -----------------------------------------------------------------------
// Basic translate() still returns correct PA after the null-ptr fix
// -----------------------------------------------------------------------
TEST(RawPtrLifetimeSpec, Translate_SucceedsAfterFix) {
    SMMU smmu;
    setupD4Stream(smmu);

    TranslationResult result = smmu.translate(D4_STREAM, D4_PASID, D4_IOVA,
                                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "translate() must succeed for a mapped page";
    EXPECT_EQ(result.getValue().physicalAddress, D4_PA)
        << "Physical address must match the mapped PA";
}

// -----------------------------------------------------------------------
// translate() on an error path (unknown stream) must still return error
// -----------------------------------------------------------------------
TEST(RawPtrLifetimeSpec, Translate_UnknownStream_ReturnsError) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(10);

    TranslationResult result = smmu.translate(0x999, D4_PASID, D4_IOVA,
                                              AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "translate() must return an error for an unknown stream";
}

// -----------------------------------------------------------------------
// translate() on a stall-mode stream with an unmapped page (exercises the
// lock.unlock() → streamContext = nullptr path before stall handling)
// -----------------------------------------------------------------------
TEST(RawPtrLifetimeSpec, Translate_StallMode_UnmappedPage_ReturnsStalled) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall; // stall mode
    cfg.aa64               = true;
    smmu.configureStream(D4_STREAM, cfg);
    smmu.enableStream(D4_STREAM);
    smmu.createStreamPASID(D4_STREAM, D4_PASID);
    // Do NOT map any page — translation will fail

    TranslationResult result = smmu.translate(D4_STREAM, D4_PASID, D4_IOVA,
                                              AccessType::Read, SecurityState::NonSecure);
    // Should be Stalled (not a crash or UB from dangling pointer)
    EXPECT_TRUE(result.isError()) << "translate() must return an error for unmapped page";
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "Stall-mode stream with unmapped page must return SMMUError::Stalled";
}

// -----------------------------------------------------------------------
// translate() on a terminate-mode stream with unmapped page (exercises the
// lock.unlock() → streamContext = nullptr path before handleTranslationFailure)
// -----------------------------------------------------------------------
TEST(RawPtrLifetimeSpec, Translate_TerminateMode_UnmappedPage_ReturnsError) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D4_STREAM, cfg);
    smmu.enableStream(D4_STREAM);
    smmu.createStreamPASID(D4_STREAM, D4_PASID);
    // Do NOT map any page

    TranslationResult result = smmu.translate(D4_STREAM, D4_PASID, D4_IOVA,
                                              AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "translate() must return error for unmapped page";
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

} // namespace test
} // namespace smmu
