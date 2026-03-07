// ARM SMMU v3 BUG-CPP-NEW-5: Stage-2 PTW uses transaction NS-bit instead of stage-1 output NS
// Copyright (c) 2024 John Greninger
//
// TDD test for the fix in performBothStagesTranslation():
// stage2AddressSpace->translatePage(intermediatePA, AccessType::Read, securityState)
// must use stage1Data.securityState, not the incoming securityState parameter.
//
// ARM IHI0070G.b §3.3.2 / §3.10: The IPA security state is determined by the
// stage-1 output, not the incoming transaction's NS bit. STE.NSCFG applies only
// when stage-1 is bypassed; when stage-1 completes a full translation, its output
// carries the IPA security state.
//
// Model note: In the current simplified address-space model, translatePage() enforces
// strict entry-securityState == requestedSecurityState equality. This means that in
// normal operation stage1Data.securityState always equals the transaction securityState
// (otherwise stage-1 itself would fail). The fix is therefore a spec-compliance code
// correction that prevents future divergence if the model is extended to support
// NS-output-override at stage-1. The test below verifies:
//   (a) Two-stage translation works end-to-end with NonSecure transactions.
//   (b) Two-stage translation works end-to-end with Secure transactions.
//   (c) A cross-security mismatch between stage-1 IPA and stage-2 AS is rejected.
//
// After the fix, (c) uses stage1Data.securityState for the stage-2 PTW lookup,
// which is the correct architectural behavior per ARM §3.3.2.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================================
// Test fixture: two-stage stream helper
// ============================================================================
class Stage2NsTest : public ::testing::Test {
protected:
    static constexpr StreamID STREAM_NS   = 0xC0;
    static constexpr StreamID STREAM_SEC  = 0xC1;
    static constexpr PASID    PASID0      = 0;
    static constexpr IOVA     TEST_IOVA   = 0x10000ULL;
    static constexpr PA       TEST_IPA    = 0x50000ULL;   // Stage-1 output (IPA)
    static constexpr PA       TEST_PA     = 0xA0000000ULL; // Stage-2 output (PA)

    void SetUp() override {
        smmu = std::make_unique<SMMU>();
        smmu->enable();
        stage2NS  = std::make_shared<AddressSpace>();
        stage2Sec = std::make_shared<AddressSpace>();
    }

    // Build a minimal two-stage StreamConfig.
    static StreamConfig makeTwoStageConfig() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.aa64               = true;
        return cfg;
    }

    std::unique_ptr<SMMU> smmu;
    std::shared_ptr<AddressSpace> stage2NS;
    std::shared_ptr<AddressSpace> stage2Sec;
};

// -----------------------------------------------------------------------
// Regression: two-stage translation with NonSecure must succeed.
// Before fix: securityState passed to stage-2 (NonSecure == stage1Data.securityState).
// After fix:  stage1Data.securityState passed (same value). No regression.
// -----------------------------------------------------------------------
TEST_F(Stage2NsTest, TwoStage_NonSecure_SuccessfulTranslation) {
    // Configure stream.
    ASSERT_TRUE(smmu->configureStream(STREAM_NS, makeTwoStageConfig()).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_NS).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_NS, PASID0).isOk());

    // Stage-1: IOVA -> IPA (NonSecure mapping).
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(STREAM_NS, PASID0, TEST_IOVA, TEST_IPA,
                              perms, SecurityState::NonSecure).isOk());

    // Stage-2: IPA -> PA (NonSecure mapping).
    stage2NS->mapPage(TEST_IPA, TEST_PA, perms, SecurityState::NonSecure);
    ASSERT_TRUE(smmu->setStreamStage2AddressSpace(STREAM_NS, stage2NS).isOk());

    // Translate with NonSecure.
    auto result = smmu->translate(STREAM_NS, PASID0, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk())
        << "Two-stage NonSecure translation must succeed";
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA)
        << "Final PA must come from stage-2 output";
}

// -----------------------------------------------------------------------
// Regression: two-stage translation with Secure must succeed when both
// stage-1 and stage-2 map as Secure.
// -----------------------------------------------------------------------
TEST_F(Stage2NsTest, TwoStage_Secure_BothStagesSecure_SuccessfulTranslation) {
    ASSERT_TRUE(smmu->configureStream(STREAM_SEC, makeTwoStageConfig()).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_SEC).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_SEC, PASID0).isOk());

    PagePermissions perms(true, false, false);
    // Stage-1 maps IOVA -> IPA with Secure security state.
    ASSERT_TRUE(smmu->mapPage(STREAM_SEC, PASID0, TEST_IOVA, TEST_IPA,
                              perms, SecurityState::Secure).isOk());

    // Stage-2 maps IPA -> PA with Secure security state.
    // After the fix, the stage-2 PTW uses stage1Data.securityState (Secure)
    // for the lookup, which matches the mapping — must succeed.
    stage2Sec->mapPage(TEST_IPA, TEST_PA, perms, SecurityState::Secure);
    ASSERT_TRUE(smmu->setStreamStage2AddressSpace(STREAM_SEC, stage2Sec).isOk());

    auto result = smmu->translate(STREAM_SEC, PASID0, TEST_IOVA,
                                  AccessType::Read, SecurityState::Secure);
    ASSERT_TRUE(result.isOk())
        << "Two-stage Secure translation must succeed when both stages are Secure";
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
}

// -----------------------------------------------------------------------
// Spec correctness: When stage-2 address space maps IPA as NonSecure but
// stage-1 maps IOVA as Secure, the security mismatch must produce a fault.
// This verifies that the stage-2 PTW uses the correct (stage-1 output)
// security state, not an incorrect override.
//
// NOTE: In the current model, stage-1 translatePage() enforces strict
// securityState equality, so a Secure transaction cannot reach stage-2 with
// a NonSecure IPA. This test documents the model's intended behavior:
// mismatched IPA security states must fault.
// -----------------------------------------------------------------------
TEST_F(Stage2NsTest, TwoStage_Stage2SecurityMismatch_MustFault) {
    static constexpr StreamID STREAM_MIS = 0xC2;
    ASSERT_TRUE(smmu->configureStream(STREAM_MIS, makeTwoStageConfig()).isOk());
    ASSERT_TRUE(smmu->enableStream(STREAM_MIS).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(STREAM_MIS, PASID0).isOk());

    PagePermissions perms(true, false, false);
    // Stage-1 maps IOVA -> IPA with NonSecure (transaction must be NonSecure too).
    ASSERT_TRUE(smmu->mapPage(STREAM_MIS, PASID0, TEST_IOVA, TEST_IPA,
                              perms, SecurityState::NonSecure).isOk());

    // Stage-2 maps the SAME IPA as Secure — mismatch with stage-1 output (NonSecure).
    // After the fix, stage-2 PTW uses stage1Data.securityState (NonSecure),
    // but stage-2 entry is Secure → fault (correct behavior).
    auto stage2Mis = std::make_shared<AddressSpace>();
    stage2Mis->mapPage(TEST_IPA, TEST_PA, perms, SecurityState::Secure);
    ASSERT_TRUE(smmu->setStreamStage2AddressSpace(STREAM_MIS, stage2Mis).isOk());

    auto result = smmu->translate(STREAM_MIS, PASID0, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "Stage-2 security mismatch (NonSecure IPA vs Secure stage-2 entry) must fault "
           "(BUG-CPP-NEW-5: stage-2 PTW uses stage1Data.securityState, not transaction NS)";
}

} // namespace test
} // namespace smmu
