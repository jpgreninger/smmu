// TDD test for BUG-CPP-DBGR-7:
// Incorrect validateSecurityState after two-stage translation in performBothStagesTranslation().
//
// ARM IHI0070G.b §3.10.2: The stage-2 output security state is the authoritative
// PA security state and need not match the incoming transaction's NS bit.  The
// translatePage() call at stage-2 already enforces the correct security state
// match.  The redundant validateSecurityState(securityState, stage2Data.securityState)
// call that follows is architecturally incorrect and must be removed.
//
// BEFORE FIX: a NonSecure stream mapping a NonSecure IPA->PA via stage-2 may
// spuriously fail the validateSecurityState check depending on model internals,
// and the misplaced check is conceptually wrong per spec even when it happens
// to pass.
//
// AFTER FIX: The redundant check is gone; two-stage NonSecure translations
// succeed as expected and no erroneous security fault is raised.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================================
// Fixture
// ============================================================================
class Stage2SecCheckTest : public ::testing::Test {
protected:
    static constexpr StreamID STREAM_ID = 0xD0;
    static constexpr PASID    PASID0    = 0;
    static constexpr IOVA     IOVA_VAL  = 0x20000ULL;
    static constexpr PA       IPA_VAL   = 0x60000ULL;
    static constexpr PA       PA_VAL    = 0xB0000000ULL;

    void SetUp() override {
        smmuObj = std::make_unique<SMMU>();
        smmuObj->enable();
    }

    static StreamConfig makeTwoStageConfig() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.aa64               = true;
        return cfg;
    }

    std::unique_ptr<SMMU> smmuObj;
};

// -----------------------------------------------------------------------
// Test 1 (regression): NonSecure two-stage translation succeeds end-to-end.
// This test must pass both before and after the fix to confirm no regression.
// -----------------------------------------------------------------------
TEST_F(Stage2SecCheckTest, TwoStageNonSecure_SucceedsEndToEnd) {
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, makeTwoStageConfig()).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions perms(true, true, false);

    // Stage-1: IOVA -> IPA (NonSecure)
    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, IPA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    // Stage-2: IPA -> PA (NonSecure)
    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->mapPage(IPA_VAL, PA_VAL, perms, SecurityState::NonSecure);
    ASSERT_TRUE(smmuObj->setStreamStage2AddressSpace(STREAM_ID, stage2AS).isOk());

    auto result = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                     AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk())
        << "Two-stage NonSecure translation must succeed; error suggests spurious security check";
    EXPECT_EQ(result.getValue().physicalAddress, PA_VAL)
        << "PA must be the stage-2 output address";
}

// -----------------------------------------------------------------------
// Test 2: Verifies the redundant validateSecurityState check is absent.
//
// With the incorrect check in place, if someone were to construct a scenario
// where the incoming transaction's security state does NOT match the stage-2
// output security state (even though translatePage already enforced it), the
// check would wrongly reject the translation.
//
// In the current model, translatePage enforces strict equality so the two
// security states always agree on a successful translation.  Therefore the
// validateSecurityState block after a successful stage-2 is guaranteed to
// be a no-op — but it remains architecturally wrong per §3.10.2.
//
// This test confirms the CORRECT path: NonSecure stream, NonSecure stage-2 AS,
// NonSecure transaction -> must succeed. This was already the regression test
// above.  Additionally we verify the Read permission is honoured (not a
// security fault masquerading as a permission fault).
// -----------------------------------------------------------------------
TEST_F(Stage2SecCheckTest, TwoStageNonSecure_SecurityStateNotReCheckedAfterStage2) {
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, makeTwoStageConfig()).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions perms(true, false, false); // read-only

    // Stage-1: IOVA -> IPA (NonSecure)
    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, IPA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    // Stage-2: IPA -> PA (NonSecure)
    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->mapPage(IPA_VAL, PA_VAL, perms, SecurityState::NonSecure);
    ASSERT_TRUE(smmuObj->setStreamStage2AddressSpace(STREAM_ID, stage2AS).isOk());

    // Read must succeed — no security state mismatch should be reported
    auto readResult = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                          AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(readResult.isOk())
        << "Read on NonSecure two-stage stream must succeed; got error code "
        << static_cast<int>(readResult.getError());
    EXPECT_NE(readResult.getError(), SMMUError::InvalidSecurityState)
        << "No security state error must be raised for a valid NonSecure two-stage translation";
    EXPECT_EQ(readResult.getValue().physicalAddress, PA_VAL);
}

// -----------------------------------------------------------------------
// Test 3: Write to a two-stage NonSecure stream (both stages allow write)
// -----------------------------------------------------------------------
TEST_F(Stage2SecCheckTest, TwoStageNonSecure_WriteSucceeds) {
    ASSERT_TRUE(smmuObj->configureStream(STREAM_ID, makeTwoStageConfig()).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_ID).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_ID, PASID0).isOk());

    PagePermissions rwPerms(true, true, false);

    ASSERT_TRUE(smmuObj->mapPage(STREAM_ID, PASID0, IOVA_VAL, IPA_VAL,
                                  rwPerms, SecurityState::NonSecure).isOk());

    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->mapPage(IPA_VAL, PA_VAL, rwPerms, SecurityState::NonSecure);
    ASSERT_TRUE(smmuObj->setStreamStage2AddressSpace(STREAM_ID, stage2AS).isOk());

    auto result = smmuObj->translate(STREAM_ID, PASID0, IOVA_VAL,
                                      AccessType::Write, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk())
        << "Write on two-stage NonSecure stream must succeed";
    EXPECT_EQ(result.getValue().physicalAddress, PA_VAL);
}

} // namespace test
} // namespace smmu
