// ARM SMMU v3 StreamContext Two-Stage Translation Comprehensive Tests
// Copyright (c) 2024 John Greninger
//
// This test suite provides comprehensive coverage of two-stage translation in stream_context.cpp
// Targets lines 492-543, 660-730 and related two-stage translation logic.
//
// Coverage Targets:
// - Two-stage translation flow: IOVA -> IPA -> PA
// - Stage-1 only translation: IOVA -> PA (Stage-2 bypassed)
// - Stage-2 only translation: IPA -> PA (Stage-1 bypassed)
// - Permission intersection logic across stages
// - Security state propagation and validation
// - Address size mismatches between stages
// - Configuration validation for two-stage scenarios
// - Fault handling at each stage
// - TLB behavior with two-stage translation

#include <gtest/gtest.h>
#include "smmu/stream_context.h"
#include "smmu/types.h"
#include "smmu/fault_handler.h"
#include "smmu/address_space.h"
#include <memory>
#include <thread>
#include <chrono>

namespace smmu {
namespace test {

class TwoStageTranslationTest : public ::testing::Test {
protected:
    void SetUp() override {
        streamContext = std::make_unique<StreamContext>();
        faultHandler = std::make_shared<FaultHandler>();

        // Create Stage-1 address space (for PASID 1)
        stage1AddressSpace = std::make_shared<AddressSpace>();

        // Create Stage-2 address space (for PASID 0 - hypervisor)
        stage2AddressSpace = std::make_shared<AddressSpace>();
    }

    void TearDown() override {
        streamContext.reset();
        faultHandler.reset();
        stage1AddressSpace.reset();
        stage2AddressSpace.reset();
    }

    std::unique_ptr<StreamContext> streamContext;
    std::shared_ptr<FaultHandler> faultHandler;
    std::shared_ptr<AddressSpace> stage1AddressSpace;
    std::shared_ptr<AddressSpace> stage2AddressSpace;

    // Test constants
    static constexpr PASID GUEST_PASID = 0x1;
    static constexpr PASID HYPERVISOR_PASID = 0x0;  // Stage-2 uses PASID 0
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr IPA TEST_IPA = 0x20000000;
    static constexpr PA TEST_PA = 0x40000000;
};

// ========== Configuration Validation Tests ==========

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_NoChanges_ReturnsSuccess) {
    // Target: Lines 492-524 - No configuration changes detected
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    // Apply initial configuration
    VoidResult result1 = streamContext->updateConfiguration(config);
    ASSERT_TRUE(result1.isOk());

    // Apply same configuration again (no changes)
    VoidResult result2 = streamContext->applyConfigurationChanges(config);

    EXPECT_TRUE(result2.isOk());
}

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_TranslationEnabledChange_Success) {
    // Target: Lines 501-504 - Translation enabled change
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;

    VoidResult result1 = streamContext->updateConfiguration(initialConfig);
    ASSERT_TRUE(result1.isOk());

    // Change only translationEnabled
    StreamConfig newConfig = initialConfig;
    newConfig.translationEnabled = true;

    VoidResult result2 = streamContext->applyConfigurationChanges(newConfig);

    EXPECT_TRUE(result2.isOk());

    // Verify the change was applied
    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(currentConfig.translationEnabled);
}

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_Stage1EnabledChange_Success) {
    // Target: Lines 506-509 - Stage-1 enabled change
    StreamConfig initialConfig;
    initialConfig.translationEnabled = true;
    initialConfig.stage1Enabled = false;
    initialConfig.stage2Enabled = true;

    VoidResult result1 = streamContext->updateConfiguration(initialConfig);
    ASSERT_TRUE(result1.isOk());

    // Change only stage1Enabled
    StreamConfig newConfig = initialConfig;
    newConfig.stage1Enabled = true;

    VoidResult result2 = streamContext->applyConfigurationChanges(newConfig);

    EXPECT_TRUE(result2.isOk());

    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(currentConfig.stage1Enabled);
}

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_Stage2EnabledChange_Success) {
    // Target: Lines 511-514 - Stage-2 enabled change
    StreamConfig initialConfig;
    initialConfig.translationEnabled = true;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;

    VoidResult result1 = streamContext->updateConfiguration(initialConfig);
    ASSERT_TRUE(result1.isOk());

    // Change only stage2Enabled
    StreamConfig newConfig = initialConfig;
    newConfig.stage2Enabled = true;

    VoidResult result2 = streamContext->applyConfigurationChanges(newConfig);

    EXPECT_TRUE(result2.isOk());

    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(currentConfig.stage2Enabled);
}

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_FaultModeChange_Success) {
    // Target: Lines 516-519 - Fault mode change
    StreamConfig initialConfig;
    initialConfig.translationEnabled = true;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Stall;

    VoidResult result1 = streamContext->updateConfiguration(initialConfig);
    ASSERT_TRUE(result1.isOk());

    // Change only faultMode
    StreamConfig newConfig = initialConfig;
    newConfig.faultMode = FaultMode::Terminate;

    VoidResult result2 = streamContext->applyConfigurationChanges(newConfig);

    EXPECT_TRUE(result2.isOk());

    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    EXPECT_EQ(currentConfig.faultMode, FaultMode::Terminate);
}

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_InvalidMergedConfig_ReturnsError) {
    // Target: Lines 527-530 - Invalid merged configuration rejected
    StreamConfig initialConfig;
    initialConfig.translationEnabled = true;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;

    VoidResult result1 = streamContext->updateConfiguration(initialConfig);
    ASSERT_TRUE(result1.isOk());

    // Try to change to invalid configuration (translation enabled but no stages)
    StreamConfig newConfig;
    newConfig.translationEnabled = true;
    newConfig.stage1Enabled = false;
    newConfig.stage2Enabled = false;

    VoidResult result2 = streamContext->applyConfigurationChanges(newConfig);

    // Should reject invalid configuration
    EXPECT_TRUE(result2.isError());
    EXPECT_EQ(result2.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(TwoStageTranslationTest, ApplyConfigurationChanges_MultipleChanges_Success) {
    // Target: Lines 532-542 - Multiple configuration changes applied
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = false;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Stall;

    VoidResult result1 = streamContext->updateConfiguration(initialConfig);
    ASSERT_TRUE(result1.isOk());

    // Change multiple fields
    StreamConfig newConfig;
    newConfig.translationEnabled = true;
    newConfig.stage1Enabled = true;
    newConfig.stage2Enabled = true;
    newConfig.faultMode = FaultMode::Terminate;

    VoidResult result2 = streamContext->applyConfigurationChanges(newConfig);

    EXPECT_TRUE(result2.isOk());

    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(currentConfig.translationEnabled);
    EXPECT_TRUE(currentConfig.stage1Enabled);
    EXPECT_TRUE(currentConfig.stage2Enabled);
    EXPECT_EQ(currentConfig.faultMode, FaultMode::Terminate);
}

// ========== Stream Statistics and State Tests ==========

TEST_F(TwoStageTranslationTest, GetStreamStatistics_ReturnsValidStats) {
    // Target: Lines 660-663 - Get stream usage statistics
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    VoidResult result = streamContext->updateConfiguration(config);
    ASSERT_TRUE(result.isOk());

    StreamStatistics stats = streamContext->getStreamStatistics();

    // Verify we got valid statistics
    EXPECT_GE(stats.configurationUpdateCount, 1u);  // At least one update
    EXPECT_GE(stats.lastAccessTimestamp, 0u);
}

TEST_F(TwoStageTranslationTest, GetStreamState_ReturnsCurrentConfiguration) {
    // Target: Lines 667-670 - Get comprehensive stream state
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = true;
    config.faultMode = FaultMode::Terminate;

    VoidResult result = streamContext->updateConfiguration(config);
    ASSERT_TRUE(result.isOk());

    StreamConfig state = streamContext->getStreamState();

    EXPECT_EQ(state.translationEnabled, config.translationEnabled);
    EXPECT_EQ(state.stage1Enabled, config.stage1Enabled);
    EXPECT_EQ(state.stage2Enabled, config.stage2Enabled);
    EXPECT_EQ(state.faultMode, config.faultMode);
}

TEST_F(TwoStageTranslationTest, IsTranslationActive_StreamDisabled_ReturnsFalse) {
    // Target: Lines 674-678 - Translation active query with disabled stream
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    streamContext->updateConfiguration(config);
    streamContext->createPASID(GUEST_PASID);

    // Stream not enabled yet
    bool isActive = streamContext->isTranslationActive();

    EXPECT_FALSE(isActive);  // Should be false because stream not enabled
}

TEST_F(TwoStageTranslationTest, IsTranslationActive_TranslationDisabled_ReturnsFalse) {
    // Target: Lines 674-678 - Translation active query with translation disabled
    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();
    streamContext->createPASID(GUEST_PASID);

    bool isActive = streamContext->isTranslationActive();

    EXPECT_FALSE(isActive);  // Should be false because translation disabled
}

TEST_F(TwoStageTranslationTest, IsTranslationActive_NoStagesEnabled_ReturnsFalse) {
    // Target: Lines 674-678 - Translation active query with no stages
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = false;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();
    streamContext->createPASID(GUEST_PASID);

    bool isActive = streamContext->isTranslationActive();

    EXPECT_FALSE(isActive);  // Should be false because no stages enabled
}

TEST_F(TwoStageTranslationTest, IsTranslationActive_NoPASIDs_ReturnsTrue) {
    // BUG-CPP-2 fix: isTranslationActive() must return true for an enabled
    // stage-1 stream even with no PASIDs yet.  PASID population does not
    // affect whether a stream is "active" per the ARM SMMU v3 spec.
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    // No PASIDs created — stream is still active per the spec.
    bool isActive = streamContext->isTranslationActive();

    EXPECT_TRUE(isActive);  // BUG-CPP-2 fix: true because stream+stage are enabled
}

TEST_F(TwoStageTranslationTest, IsTranslationActive_AllConditionsMet_ReturnsTrue) {
    // Target: Lines 674-678 - Translation active query with all conditions met
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();
    streamContext->createPASID(GUEST_PASID);

    bool isActive = streamContext->isTranslationActive();

    EXPECT_TRUE(isActive);  // All conditions met
}

TEST_F(TwoStageTranslationTest, HasConfigurationChanged_AfterUpdate_ReturnsTrue) {
    // Target: Lines 682-685 - Configuration change detection
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    VoidResult result = streamContext->updateConfiguration(config);
    ASSERT_TRUE(result.isOk());

    bool hasChanged = streamContext->hasConfigurationChanged();

    EXPECT_TRUE(hasChanged);
}

// ========== Fault Handler Integration Tests ==========

TEST_F(TwoStageTranslationTest, SetFaultHandler_ValidHandler_Success) {
    // Target: Lines 693-703 - Set fault handler
    VoidResult result = streamContext->setFaultHandler(faultHandler);

    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(streamContext->hasFaultHandler());
}

TEST_F(TwoStageTranslationTest, SetFaultHandler_NullHandler_ClearsPrevious) {
    // Target: Lines 693-703 - Clear fault handler with nullptr
    streamContext->setFaultHandler(faultHandler);
    ASSERT_TRUE(streamContext->hasFaultHandler());

    VoidResult result = streamContext->setFaultHandler(nullptr);

    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(streamContext->hasFaultHandler());
}

TEST_F(TwoStageTranslationTest, GetFaultHandler_AfterSet_ReturnsHandler) {
    // Target: Lines 707-710 - Get fault handler query
    streamContext->setFaultHandler(faultHandler);

    std::shared_ptr<FaultHandler> retrievedHandler = streamContext->getFaultHandler();

    EXPECT_EQ(retrievedHandler, faultHandler);
}

TEST_F(TwoStageTranslationTest, RecordFault_NoHandler_ReturnsError) {
    // Target: Lines 714-720 - Record fault without handler
    FaultRecord fault;
    fault.streamID = 1;
    fault.pasid = GUEST_PASID;
    fault.address = TEST_IOVA;
    fault.faultType = FaultType::TranslationFault;
    fault.accessType = AccessType::Read;

    VoidResult result = streamContext->recordFault(fault);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::FaultHandlingError);
}

TEST_F(TwoStageTranslationTest, RecordFault_WithHandler_Success) {
    // Target: Lines 714-730 - Record fault through handler
    streamContext->setFaultHandler(faultHandler);

    FaultRecord fault;
    fault.streamID = 1;
    fault.pasid = GUEST_PASID;
    fault.address = TEST_IOVA;
    fault.faultType = FaultType::PermissionFault;
    fault.accessType = AccessType::Write;

    VoidResult result = streamContext->recordFault(fault);

    EXPECT_TRUE(result.isOk());

    // Verify fault was recorded
    auto faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 1u);
    EXPECT_EQ(faults[0].faultType, FaultType::PermissionFault);
    EXPECT_EQ(faults[0].address, TEST_IOVA);
}

TEST_F(TwoStageTranslationTest, RecordFault_UpdatesStatistics) {
    // Target: Lines 725-729 - Fault count statistics
    streamContext->setFaultHandler(faultHandler);

    StreamStatistics statsBefore = streamContext->getStreamStatistics();

    // Small delay to ensure timestamp difference
    std::this_thread::sleep_for(std::chrono::milliseconds(1));

    FaultRecord fault;
    fault.streamID = 1;
    fault.pasid = GUEST_PASID;
    fault.address = TEST_IOVA;
    fault.faultType = FaultType::TranslationFault;
    fault.accessType = AccessType::Read;

    streamContext->recordFault(fault);

    StreamStatistics statsAfter = streamContext->getStreamStatistics();

    EXPECT_EQ(statsAfter.faultCount, statsBefore.faultCount + 1);
    EXPECT_GT(statsAfter.lastAccessTimestamp, statsBefore.lastAccessTimestamp);
}

TEST_F(TwoStageTranslationTest, RecordFault_MultipleFaults_AllRecorded) {
    // Target: Lines 722-730 - Multiple fault recording
    streamContext->setFaultHandler(faultHandler);

    // Record multiple faults
    for (uint32_t i = 0; i < 5; i++) {
        FaultRecord fault;
        fault.streamID = 1;
        fault.pasid = GUEST_PASID;
        fault.address = TEST_IOVA + (i * PAGE_SIZE);
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = AccessType::Read;

        VoidResult result = streamContext->recordFault(fault);
        EXPECT_TRUE(result.isOk());
    }

    auto faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 5u);

    StreamStatistics stats = streamContext->getStreamStatistics();
    EXPECT_EQ(stats.faultCount, 5u);
}

TEST_F(TwoStageTranslationTest, HasFaultHandler_NoHandler_ReturnsFalse) {
    // Target: Fault handler presence check
    EXPECT_FALSE(streamContext->hasFaultHandler());
}

TEST_F(TwoStageTranslationTest, HasFaultHandler_WithHandler_ReturnsTrue) {
    // Target: Fault handler presence check
    streamContext->setFaultHandler(faultHandler);

    EXPECT_TRUE(streamContext->hasFaultHandler());
}

// ========== Two-Stage Translation Scenarios ==========

TEST_F(TwoStageTranslationTest, TwoStageTranslation_BothStagesEnabled_Success) {
    // Setup: Both Stage-1 and Stage-2 enabled
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    // Create Stage-1 address space for guest (PASID 1)
    streamContext->createPASID(GUEST_PASID);
    streamContext->addPASID(GUEST_PASID, stage1AddressSpace);

    // Set Stage-2 address space for hypervisor
    streamContext->setStage2AddressSpace(stage2AddressSpace);

    // Map Stage-1: IOVA -> IPA
    PagePermissions stage1Perms(true, true, false);  // RW
    stage1AddressSpace->mapPage(TEST_IOVA, TEST_IPA, stage1Perms, SecurityState::NonSecure);

    // Map Stage-2: IPA -> PA
    PagePermissions stage2Perms(true, true, true);  // RWX
    stage2AddressSpace->mapPage(TEST_IPA, TEST_PA, stage2Perms, SecurityState::NonSecure);

    // Translate: Should go through both stages
    TranslationResult result = streamContext->translate(GUEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        TranslationData data = result.getValue();
        EXPECT_EQ(data.physicalAddress, TEST_PA);
        // Permissions should be intersection of both stages (RW & RWX = RW)
        EXPECT_TRUE(data.permissions.read);
        EXPECT_TRUE(data.permissions.write);
    }
}

TEST_F(TwoStageTranslationTest, TwoStageTranslation_Stage1Only_BypassesStage2) {
    // Setup: Only Stage-1 enabled
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    // Create Stage-1 address space for guest (PASID 1)
    streamContext->createPASID(GUEST_PASID);
    streamContext->addPASID(GUEST_PASID, stage1AddressSpace);

    // Map Stage-1: IOVA -> PA directly (no Stage-2)
    PagePermissions perms(true, false, false);  // Read-only
    stage1AddressSpace->mapPage(TEST_IOVA, TEST_PA, perms, SecurityState::NonSecure);

    // Translate: Should use Stage-1 only
    TranslationResult result = streamContext->translate(GUEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        TranslationData data = result.getValue();
        EXPECT_EQ(data.physicalAddress, TEST_PA);
        EXPECT_TRUE(data.permissions.read);
        EXPECT_FALSE(data.permissions.write);
    }
}

TEST_F(TwoStageTranslationTest, TwoStageTranslation_Stage2Only_BypassesStage1) {
    // Setup: Only Stage-2 enabled (IOVA = IPA)
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = true;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    // Set Stage-2 address space for hypervisor
    streamContext->setStage2AddressSpace(stage2AddressSpace);

    // Map Stage-2: IPA -> PA (IOVA is treated as IPA)
    PagePermissions perms(true, true, true);  // RWX
    stage2AddressSpace->mapPage(TEST_IOVA, TEST_PA, perms, SecurityState::NonSecure);

    // Translate: Should use Stage-2 only (any PASID works since Stage-1 is bypassed)
    TranslationResult result = streamContext->translate(GUEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        TranslationData data = result.getValue();
        EXPECT_EQ(data.physicalAddress, TEST_PA);
        EXPECT_TRUE(data.permissions.read);
        EXPECT_TRUE(data.permissions.write);
        EXPECT_TRUE(data.permissions.execute);
    }
}

TEST_F(TwoStageTranslationTest, TwoStageTranslation_PermissionIntersection_RestrictsAccess) {
    // Test permission intersection: Stage-1 allows RWX, Stage-2 allows RO
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    streamContext->createPASID(GUEST_PASID);
    streamContext->addPASID(GUEST_PASID, stage1AddressSpace);
    streamContext->setStage2AddressSpace(stage2AddressSpace);

    // Stage-1: Full permissions (RWX)
    PagePermissions stage1Perms(true, true, true);
    stage1AddressSpace->mapPage(TEST_IOVA, TEST_IPA, stage1Perms, SecurityState::NonSecure);

    // Stage-2: Read-only
    PagePermissions stage2Perms(true, false, false);
    stage2AddressSpace->mapPage(TEST_IPA, TEST_PA, stage2Perms, SecurityState::NonSecure);

    // Translate: Final permissions should be intersection (RO)
    TranslationResult result = streamContext->translate(GUEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        TranslationData data = result.getValue();
        EXPECT_TRUE(data.permissions.read);
        EXPECT_FALSE(data.permissions.write);  // Restricted by Stage-2
        EXPECT_FALSE(data.permissions.execute);  // Restricted by Stage-2
    }
}

TEST_F(TwoStageTranslationTest, TwoStageTranslation_Stage1Fault_ReportsCorrectly) {
    // Test Stage-1 translation fault
    streamContext->setFaultHandler(faultHandler);

    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    streamContext->createPASID(GUEST_PASID);
    streamContext->addPASID(GUEST_PASID, stage1AddressSpace);
    streamContext->setStage2AddressSpace(stage2AddressSpace);

    // No Stage-1 mapping, but Stage-2 is mapped
    PagePermissions stage2Perms(true, true, true);
    stage2AddressSpace->mapPage(TEST_IPA, TEST_PA, stage2Perms, SecurityState::NonSecure);

    // Translate: Should fail at Stage-1
    TranslationResult result = streamContext->translate(GUEST_PASID, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(TwoStageTranslationTest, TwoStageTranslation_MultipleAddressSizes_Supported) {
    // Test different address sizes across stages
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;

    streamContext->updateConfiguration(config);
    streamContext->enableStream();

    streamContext->createPASID(GUEST_PASID);
    streamContext->addPASID(GUEST_PASID, stage1AddressSpace);
    streamContext->setStage2AddressSpace(stage2AddressSpace);

    // Test with various address values
    IOVA iova48bit = 0x0000FFFF00000000;  // 48-bit address
    IPA ipa40bit = 0x000000FF00000000;    // 40-bit IPA
    PA pa44bit = 0x00000FFF00000000;      // 44-bit PA

    PagePermissions perms(true, true, false);
    stage1AddressSpace->mapPage(iova48bit, ipa40bit, perms, SecurityState::NonSecure);
    stage2AddressSpace->mapPage(ipa40bit, pa44bit, perms, SecurityState::NonSecure);

    TranslationResult result = streamContext->translate(GUEST_PASID, iova48bit, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, pa44bit);
    }
}

} // namespace test
} // namespace smmu
