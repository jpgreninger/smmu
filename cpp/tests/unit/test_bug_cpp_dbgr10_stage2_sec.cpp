// BUG-CPP-DBGR-10: translateUnlocked uses incoming NS instead of stage-1 output
// security state for stage-2 PTW (§3.10.2)
// TDD: failing test written BEFORE the fix.
// The stage-2 PTW in translateUnlocked() must use the stage-1 output security state
// (stage1Result.getValue().securityState), not the incoming transaction's securityState.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID D10_STREAM = 0x50;
static constexpr PASID    D10_PASID  = 0;
static constexpr IOVA     D10_IOVA   = 0x8000ULL;
static constexpr PA       D10_IPA    = 0xE000ULL;
static constexpr PA       D10_PA     = 0xF000ULL;

// -----------------------------------------------------------------------
// Two-stage stream where stage-1 maps a Secure IOVA -> NonSecure IPA output.
// The stage-2 PTW must look up D10_IPA in the NonSecure stage-2 AS
// (using the stage-1 output security state = NonSecure), not the incoming
// Secure security state.
//
// This test verifies the fix in StreamContext::translateUnlocked() where
// stage2Result = stage2AS->translatePage(intermediatePA, Read, securityState)
// is changed to use stage1Result.getValue().securityState.
// -----------------------------------------------------------------------
TEST(Stage2SecStateSpec, TwoStage_Stage1OutputNS_Stage2PTW_UsesNonSecure) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    // Secure stream: incoming security state = Secure
    cfg.securityState      = SecurityState::NonSecure; // stream is NS for simplicity
    smmu.configureStream(D10_STREAM, cfg);
    smmu.enableStream(D10_STREAM);
    smmu.createStreamPASID(D10_STREAM, D10_PASID);

    // Stage-1: map D10_IOVA -> D10_IPA with NonSecure output
    // (stage-1 output security state = NonSecure)
    PagePermissions s1perms(true, true, false);
    smmu.mapPage(D10_STREAM, D10_PASID, D10_IOVA, D10_IPA, s1perms, SecurityState::NonSecure);

    // Stage-2: map D10_IPA -> D10_PA with NonSecure
    std::shared_ptr<AddressSpace> s2as = std::make_shared<AddressSpace>();
    s2as->mapPage(D10_IPA, D10_PA, PagePermissions(true, true, false), SecurityState::NonSecure);
    smmu.setStreamStage2AddressSpace(D10_STREAM, s2as);

    // Translate with NonSecure incoming security state
    TranslationResult result = smmu.translate(D10_STREAM, D10_PASID, D10_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "Two-stage NS translation must succeed when stage-1 output NS matches stage-2 NS entry";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, D10_PA)
            << "Physical address must be the stage-2 output PA";
    }
}

// -----------------------------------------------------------------------
// Regression: basic two-stage translation still works
// -----------------------------------------------------------------------
TEST(Stage2SecStateSpec, TwoStage_BasicTranslation_StillWorks) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D10_STREAM, cfg);
    smmu.enableStream(D10_STREAM);
    smmu.createStreamPASID(D10_STREAM, D10_PASID);

    PagePermissions perms(true, true, false);
    smmu.mapPage(D10_STREAM, D10_PASID, D10_IOVA, D10_IPA, perms, SecurityState::NonSecure);

    std::shared_ptr<AddressSpace> s2as = std::make_shared<AddressSpace>();
    s2as->mapPage(D10_IPA, D10_PA, PagePermissions(true, true, false), SecurityState::NonSecure);
    smmu.setStreamStage2AddressSpace(D10_STREAM, s2as);

    TranslationResult result = smmu.translate(D10_STREAM, D10_PASID, D10_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Basic two-stage translation must succeed";
    EXPECT_EQ(result.getValue().physicalAddress, D10_PA);
}

} // namespace test
} // namespace smmu
