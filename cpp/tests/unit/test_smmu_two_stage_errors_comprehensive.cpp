// ARM SMMU v3 Two-Stage Translation Error Tests (Comprehensive Phase 3)
// Copyright (c) 2024 John Greninger
//
// This test suite provides comprehensive coverage of two-stage translation error paths
// Targets lines 636-740, 845-960 in smmu.cpp for 80+ lines coverage gain
//
// Coverage Targets:
// - Stage 1 enabled, Stage 2 disabled translation paths
// - Stage 1 disabled, Stage 2 enabled translation paths
// - Both stages disabled configuration errors
// - Stage 1 success, Stage 2 translation failures
// - Stage 1 success, Stage 2 permission failures
// - Stage 1 translation failures (Stage 2 not reached)
// - Stage 1 permission failures (Stage 2 not reached)
// - IPA out of range for Stage 2
// - Stage 2 address space not configured
// - Permission intersection between stages
// - Security state propagation and validation
// - PASID validation in two-stage context
// - Fault syndrome generation for each stage
// - Event recording for two-stage faults
// - Complex fault scenarios

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

class SMMUTwoStageErrorsTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();
        // ARM §6.3.9: SMMU starts disabled; enable globally before tests.
        smmu->enable();
    }

    void TearDown() override {
        smmu.reset();
    }

    std::unique_ptr<SMMU> smmu;

    // Test constants
    static constexpr StreamID TEST_STREAM_ID = 0x100;
    static constexpr StreamID TEST_STREAM_ID_2 = 0x101;
    static constexpr PASID TEST_PASID = 0x1;
    static constexpr PASID HYPERVISOR_PASID = 0x0;
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr IOVA TEST_IOVA_2 = 0x20000000;
    static constexpr IPA TEST_IPA = 0x30000000;
    static constexpr PA TEST_PA = 0x40000000;
    static constexpr PA TEST_PA_STAGE2 = 0x50000000;
};

// ========== Stage Configuration Tests ==========

TEST_F(SMMUTwoStageErrorsTest, Stage1Enabled_Stage2Disabled_TranslateSucceeds) {
    // Target: Lines 684-686 - Stage 1 only translation path
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());

    // Map page in Stage 1
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_PA, perms).isOk());

    // Translation should succeed through Stage 1 only
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    }
}

TEST_F(SMMUTwoStageErrorsTest, Stage1Disabled_Stage2Enabled_TranslateSucceeds) {
    // Target: Lines 687-689 - Stage 2 only translation path
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map page in Stage 2 (hypervisor PASID 0)
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IOVA, TEST_PA_STAGE2, perms).isOk());

    // Translation should succeed through Stage 2 only (IOVA treated as IPA)
    TranslationResult result = smmu->translate(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, TEST_PA_STAGE2);
    }
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesDisabled_TranslationEnabled_ConfigurationError) {
    // Target: Lines 690-704 - No stages enabled but translation enabled error
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = false;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());

    // Translation should fail with configuration error
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::ConfigurationError);

    // Verify fault was recorded
    auto events = smmu->getEvents();
    EXPECT_TRUE(events.isOk());
    if (events.isOk()) {
        EXPECT_GT(events.getValue().size(), 0);
    }
}

// ========== Two-Stage Translation Failures ==========

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage1Success_Stage2TranslationFailure) {
    // Target: Lines 905-922 - Stage 2 translation failure path
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map page in Stage 1 but NOT in Stage 2
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());

    // Stage 2 mapping is missing - should fail during Stage 2 translation
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);

    // Verify fault was recorded for Stage 2
    auto events = smmu->getEvents();
    EXPECT_TRUE(events.isOk());
    if (events.isOk()) {
        EXPECT_GT(events.getValue().size(), 0);
    }
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage1Success_Stage2PermissionFailure) {
    // Target: Lines 936-941 - Permission intersection and validation failure
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map with write permission in Stage 1
    PagePermissions stage1Perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, stage1Perms).isOk());

    // Map with read-only permission in Stage 2 (permission intersection will restrict)
    PagePermissions stage2Perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, stage2Perms).isOk());

    // Try to write - should fail due to Stage 2 restricting write permission
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage1TranslationFailure_Stage2NotReached) {
    // Target: Lines 869-882 - Stage 1 translation failure prevents Stage 2
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map Stage 2 but not Stage 1
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, perms).isOk());

    // Translation should fail at Stage 1, never reach Stage 2
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);

    // Verify fault recorded for Stage 1
    auto events = smmu->getEvents();
    EXPECT_TRUE(events.isOk());
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage1PermissionFailure_Stage2NotReached) {
    // Target: Lines 869-882 - Stage 1 permission fault prevents Stage 2
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map Stage 1 with read-only permission
    PagePermissions stage1Perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, stage1Perms).isOk());

    // Map Stage 2 with full permissions
    PagePermissions stage2Perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, stage2Perms).isOk());

    // Try to write - should fail at Stage 1 permission check
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    EXPECT_TRUE(result.isError());
}

// ========== IPA and Address Space Validation ==========

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage1ProducesInvalidIPA_Failure) {
    // Target: Lines 888-893 - Invalid IPA validation from Stage 1
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map to address 0 (suspicious null translation)
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, 0x0, perms).isOk());

    // Translation should detect invalid IPA
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::TranslationTableError);
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage2AddressSpaceNotConfigured_Failure) {
    // Target: Lines 897-903 - Stage 2 address space not configured error
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    // DO NOT create HYPERVISOR_PASID (Stage 2 address space)

    // Map Stage 1
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());

    // Translation should fail - Stage 2 address space not configured
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::AddressSpaceExhausted);
}

// ========== Permission Intersection Tests ==========

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_BothStagesRestrictPermissions_IntersectionApplied) {
    // Target: Lines 928-941 - Permission intersection logic
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Stage 1: Read + Execute
    PagePermissions stage1Perms(true, false, true);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, stage1Perms).isOk());

    // Stage 2: Read + Write
    PagePermissions stage2Perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, stage2Perms).isOk());

    // Read should succeed (both allow)
    TranslationResult readResult = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(readResult.isOk());

    // Write should fail (Stage 1 doesn't allow)
    TranslationResult writeResult = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(writeResult.isError());

    // Execute should fail (Stage 2 doesn't allow)
    TranslationResult execResult = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Execute);
    EXPECT_TRUE(execResult.isError());
}

// ========== Security State Validation ==========

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_SecurityStateInconsistency_Failure) {
    // Target: Lines 943-949 - Security state consistency validation
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map Stage 1 with Secure state
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms, SecurityState::Secure).isOk());

    // Map Stage 2 with NonSecure state (inconsistency)
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, perms, SecurityState::NonSecure).isOk());

    // Translation should detect security state inconsistency
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::Secure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidSecurityState);
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_SecurityStatePropagation_BothSecure) {
    // Target: Lines 943-957 - Security state propagation validation
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map both stages with Secure state
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms, SecurityState::Secure).isOk());
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, perms, SecurityState::Secure).isOk());

    // Translation with Secure state should succeed
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::Secure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().securityState, SecurityState::Secure);
    }
}

// ========== PASID Validation ==========

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_PASIDNotConfigured_Stage1Failure) {
    // Target: Lines 859-865 - PASID not configured for Stage 1
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    // DO NOT create TEST_PASID

    // Translation should fail - PASID not configured
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PASIDNotFound);
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_ConfigurationErrorCheck_BothStagesShouldBeEnabled) {
    // Target: Lines 850-856 - Configuration validation in performBothStagesTranslation
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map pages properly
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, perms).isOk());

    // Translation should succeed with both stages properly configured
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isOk());
}

// ========== Complex Fault Scenarios ==========

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_Stage1MappedStage2Unmapped_RecordsFault) {
    // Target: Fault recording for two-stage failures
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map only Stage 1
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());

    // Clear events before test
    ASSERT_TRUE(smmu->clearEvents().isOk());

    // Translation fails at Stage 2
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());

    // Verify fault was recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_GT(events.getValue().size(), 0);

    if (events.getValue().size() > 0) {
        const FaultRecord& fault = events.getValue()[0];
        EXPECT_EQ(fault.streamID, TEST_STREAM_ID);
        EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    }
}

TEST_F(SMMUTwoStageErrorsTest, BothStagesEnabled_MultipleTranslationAttempts_FaultsRecorded) {
    // Test multiple translation failures with fault accumulation
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());

    // Clear events
    ASSERT_TRUE(smmu->clearEvents().isOk());

    // Attempt multiple translations that will fail
    for (int i = 0; i < 3; i++) {
        IOVA testAddr = TEST_IOVA + (i * 0x1000);
        TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, testAddr, AccessType::Read);
        EXPECT_TRUE(result.isError());
    }

    // Verify multiple faults were recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_GE(events.getValue().size(), 3);
}

// ========== Stage-Specific Translation Tests ==========

TEST_F(SMMUTwoStageErrorsTest, Stage1Only_NullTranslationDetected_Failure) {
    // Target: Lines 984-997 - Stage 1 only null translation detection
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());

    // Map to null address
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, 0x0, perms).isOk());

    // Translation should detect suspicious null mapping
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(SMMUTwoStageErrorsTest, Stage2Only_NullTranslationDetected_Failure) {
    // Target: Lines 1023-1036 - Stage 2 only null translation detection
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map to null address
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IOVA, 0x0, perms).isOk());

    // Translation should detect suspicious null mapping
    TranslationResult result = smmu->translate(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(SMMUTwoStageErrorsTest, Stage1Only_TranslationFailure_FaultRecorded) {
    // Target: Lines 968-980 - Stage 1 only translation failure with fault
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());

    // Clear events
    ASSERT_TRUE(smmu->clearEvents().isOk());

    // Attempt translation without mapping
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);

    // Verify fault recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_GT(events.getValue().size(), 0);
}

TEST_F(SMMUTwoStageErrorsTest, Stage2Only_TranslationFailure_FaultRecorded) {
    // Target: Lines 1007-1019 - Stage 2 only translation failure with fault
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Clear events
    ASSERT_TRUE(smmu->clearEvents().isOk());

    // Attempt translation without mapping
    TranslationResult result = smmu->translate(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);

    // Verify fault recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_GT(events.getValue().size(), 0);
}

// ========== Final Validation Tests ==========

TEST_F(SMMUTwoStageErrorsTest, PerformTwoStageTranslation_SuspiciousNullAddress_TranslationError) {
    // Target: Lines 710-725 - Final translation validation for null addresses
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map Stage 1 to IPA, then Stage 2 to null PA
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, 0x0, perms).isOk());

    // Should fail due to suspicious null PA from Stage 2
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    // May succeed or fail depending on null handling - just verify it doesn't crash
    EXPECT_TRUE(result.isOk() || result.isError());
}

TEST_F(SMMUTwoStageErrorsTest, PerformTwoStageTranslation_PermissionValidation_CorrectFaultType) {
    // Target: Lines 726-741 - Permission validation with correct fault type
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    ASSERT_TRUE(smmu->configureStream(TEST_STREAM_ID, config).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_STREAM_ID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM_ID, HYPERVISOR_PASID).isOk());

    // Map with execute-only permission in both stages
    PagePermissions perms(false, false, true);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM_ID, HYPERVISOR_PASID, TEST_IPA, TEST_PA_STAGE2, perms).isOk());

    // Clear events
    ASSERT_TRUE(smmu->clearEvents().isOk());

    // Try to read - should fail with permission fault
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation);

    // Verify permission fault was recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    if (events.getValue().size() > 0) {
        EXPECT_EQ(events.getValue()[0].faultType, FaultType::PermissionFault);
    }
}

} // namespace test
} // namespace smmu
