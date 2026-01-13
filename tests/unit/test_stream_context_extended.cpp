// ARM SMMU v3 StreamContext Extended Coverage Tests
// Copyright (c) 2024 John Greninger
//
// This test suite targets specific coverage gaps to improve StreamContext coverage from 27% to 70%+
//
// Test Coverage Priorities:
// - Priority 1: Configuration and State Management (Lines 602-700)
// - Priority 2: Fault Handler Integration (Lines 700-800)
// - Priority 3: Validation Methods (Lines 800-1000)
//
// All tests follow ARM SMMU v3 specification compliance requirements.

#include <gtest/gtest.h>
#include "smmu/stream_context.h"
#include "smmu/types.h"
#include "smmu/fault_handler.h"
#include <memory>
#include <thread>
#include <chrono>

namespace smmu {
namespace test {

class StreamContextExtendedTest : public ::testing::Test {
protected:
    void SetUp() override {
        streamContext = std::make_unique<StreamContext>();
        faultHandler = std::make_shared<FaultHandler>();
    }

    void TearDown() override {
        streamContext.reset();
        faultHandler.reset();
    }

    std::unique_ptr<StreamContext> streamContext;
    std::shared_ptr<FaultHandler> faultHandler;

    // Test constants
    static constexpr StreamID TEST_STREAM_ID = 0x1000;
    static constexpr PASID TEST_PASID = 0x1;
    static constexpr PASID TEST_PASID_2 = 0x2;
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr PA TEST_PA = 0x40000000;

    // Helper to create valid context descriptor
    ContextDescriptor createValidContextDescriptor() {
        ContextDescriptor cd;
        cd.ttbr0 = 0x1000;
        cd.ttbr1 = 0x2000;
        cd.ttbr0Valid = true;
        cd.ttbr1Valid = true;
        cd.asid = 1;
        cd.contextDescriptorIndex = 0;
        cd.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
        cd.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;
        cd.tcr.granuleSize = TranslationGranule::Size4KB;
        cd.securityState = SecurityState::NonSecure;
        return cd;
    }

    // Helper to create valid stream table entry
    StreamTableEntry createValidStreamTableEntry() {
        StreamTableEntry ste;
        ste.translationEnabled = true;
        ste.stage1Enabled = true;
        ste.stage2Enabled = false;
        ste.contextDescriptorTableBase = 0x1000;
        ste.contextDescriptorTableSize = 1;
        ste.faultMode = FaultMode::Terminate;
        ste.securityState = SecurityState::NonSecure;
        ste.stage1Granule = TranslationGranule::Size4KB;
        ste.stage2Granule = TranslationGranule::Size4KB;
        return ste;
    }
};

// ============================================================================
// Priority 1: Configuration and State Management (Lines 602-700)
// ============================================================================

// Test updateConfiguration() with valid configuration
TEST_F(StreamContextExtendedTest, UpdateConfigurationValid) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;

    VoidResult result = streamContext->updateConfiguration(config);
    EXPECT_TRUE(result.isOk());

    // Verify configuration was applied
    StreamConfig retrievedConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(retrievedConfig.translationEnabled);
    EXPECT_TRUE(retrievedConfig.stage1Enabled);
    EXPECT_FALSE(retrievedConfig.stage2Enabled);
    EXPECT_EQ(retrievedConfig.faultMode, FaultMode::Terminate);

    // Verify configuration changed flag is set
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
}

// Test updateConfiguration() with invalid configuration
TEST_F(StreamContextExtendedTest, UpdateConfigurationInvalid) {
    StreamConfig config;
    config.translationEnabled = true;  // Translation enabled
    config.stage1Enabled = false;      // But no stages enabled
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;

    VoidResult result = streamContext->updateConfiguration(config);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidConfiguration);

    // Verify configuration was NOT changed
    StreamConfig retrievedConfig = streamContext->getStreamConfiguration();
    EXPECT_FALSE(retrievedConfig.translationEnabled);  // Should remain at default
}

// Test updateConfiguration() statistics tracking
TEST_F(StreamContextExtendedTest, UpdateConfigurationStatistics) {
    StreamStatistics initialStats = streamContext->getStreamStatistics();
    uint64_t initialUpdateCount = initialStats.configurationUpdateCount;

    // Sleep to ensure timestamp advances (needed when tests run in quick succession)
    std::this_thread::sleep_for(std::chrono::microseconds(10));

    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;

    EXPECT_TRUE(streamContext->updateConfiguration(config));

    StreamStatistics updatedStats = streamContext->getStreamStatistics();
    EXPECT_EQ(updatedStats.configurationUpdateCount, initialUpdateCount + 1);
    EXPECT_GT(updatedStats.lastAccessTimestamp, initialStats.lastAccessTimestamp);
}

// Test applyConfigurationChanges() with translation enabled change
TEST_F(StreamContextExtendedTest, ApplyConfigChangesTranslationEnabled) {
    // Set initial configuration
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(initialConfig));

    // Apply change to only translation enabled
    StreamConfig newConfig = initialConfig;
    newConfig.translationEnabled = true;

    VoidResult result = streamContext->applyConfigurationChanges(newConfig);
    EXPECT_TRUE(result.isOk());

    // Verify only translation enabled changed
    StreamConfig updatedConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(updatedConfig.translationEnabled);
    EXPECT_TRUE(updatedConfig.stage1Enabled);
    EXPECT_FALSE(updatedConfig.stage2Enabled);
}

// Test applyConfigurationChanges() with stage2 enabled change
TEST_F(StreamContextExtendedTest, ApplyConfigChangesStage2Enabled) {
    // Set initial configuration
    StreamConfig initialConfig;
    initialConfig.translationEnabled = true;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(initialConfig));

    // Apply change to enable Stage 2
    StreamConfig newConfig = initialConfig;
    newConfig.stage2Enabled = true;

    VoidResult result = streamContext->applyConfigurationChanges(newConfig);
    EXPECT_TRUE(result.isOk());

    // Verify Stage 2 was enabled
    EXPECT_TRUE(streamContext->isStage2Enabled());
}

// Test applyConfigurationChanges() with fault mode change
TEST_F(StreamContextExtendedTest, ApplyConfigChangesFaultMode) {
    // Set initial configuration with Terminate mode
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(initialConfig));

    // Apply change to Stall mode
    StreamConfig newConfig = initialConfig;
    newConfig.faultMode = FaultMode::Stall;

    VoidResult result = streamContext->applyConfigurationChanges(newConfig);
    EXPECT_TRUE(result.isOk());

    // Verify fault mode changed
    StreamConfig updatedConfig = streamContext->getStreamConfiguration();
    EXPECT_EQ(updatedConfig.faultMode, FaultMode::Stall);
}

// Test applyConfigurationChanges() with no changes
TEST_F(StreamContextExtendedTest, ApplyConfigChangesNoChanges) {
    // Set initial configuration
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(initialConfig));

    StreamStatistics beforeStats = streamContext->getStreamStatistics();
    uint64_t beforeUpdateCount = beforeStats.configurationUpdateCount;

    // Apply same configuration (no changes)
    VoidResult result = streamContext->applyConfigurationChanges(initialConfig);
    EXPECT_TRUE(result.isOk());

    // Verify update count did NOT increment (optimization path)
    StreamStatistics afterStats = streamContext->getStreamStatistics();
    EXPECT_EQ(afterStats.configurationUpdateCount, beforeUpdateCount);
}

// Test isConfigurationValid() with valid configurations
TEST_F(StreamContextExtendedTest, IsConfigurationValidPositiveCases) {
    // Valid: Stage 1 only with translation enabled
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = true;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    Result<bool> result1 = streamContext->isConfigurationValid(config1);
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result1.getValue());

    // Valid: Stage 2 only with translation enabled
    StreamConfig config2;
    config2.translationEnabled = true;
    config2.stage1Enabled = false;
    config2.stage2Enabled = true;
    config2.faultMode = FaultMode::Terminate;
    Result<bool> result2 = streamContext->isConfigurationValid(config2);
    EXPECT_TRUE(result2.isOk());
    EXPECT_TRUE(result2.getValue());

    // Valid: Both stages with translation enabled
    StreamConfig config3;
    config3.translationEnabled = true;
    config3.stage1Enabled = true;
    config3.stage2Enabled = true;
    config3.faultMode = FaultMode::Stall;
    Result<bool> result3 = streamContext->isConfigurationValid(config3);
    EXPECT_TRUE(result3.isOk());
    EXPECT_TRUE(result3.getValue());

    // Valid: Translation disabled (stages don't matter)
    StreamConfig config4;
    config4.translationEnabled = false;
    config4.stage1Enabled = false;
    config4.stage2Enabled = false;
    config4.faultMode = FaultMode::Terminate;
    Result<bool> result4 = streamContext->isConfigurationValid(config4);
    EXPECT_TRUE(result4.isOk());
    EXPECT_TRUE(result4.getValue());
}

// Test isConfigurationValid() with invalid configurations
TEST_F(StreamContextExtendedTest, IsConfigurationValidNegativeCases) {
    // Invalid: Translation enabled but no stages
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = false;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    Result<bool> result1 = streamContext->isConfigurationValid(config1);
    EXPECT_TRUE(result1.isOk());
    EXPECT_FALSE(result1.getValue());

    // Invalid: Invalid fault mode
    StreamConfig config2;
    config2.translationEnabled = true;
    config2.stage1Enabled = true;
    config2.stage2Enabled = false;
    config2.faultMode = static_cast<FaultMode>(99);
    Result<bool> result2 = streamContext->isConfigurationValid(config2);
    EXPECT_TRUE(result2.isOk());
    EXPECT_FALSE(result2.getValue());
}

// Test enableStream() with valid configuration
TEST_F(StreamContextExtendedTest, EnableStreamValid) {
    // Set up valid configuration
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config));

    VoidResult result = streamContext->enableStream();
    EXPECT_TRUE(result.isOk());

    Result<bool> enabledResult = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
}

// Test enableStream() with no stages enabled
TEST_F(StreamContextExtendedTest, EnableStreamNoStages) {
    // Disable both stages
    streamContext->setStage1Enabled(false);
    streamContext->setStage2Enabled(false);

    VoidResult result = streamContext->enableStream();
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::ConfigurationError);

    Result<bool> enabledResult = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_FALSE(enabledResult.getValue());
}

// Test enableStream() with invalid configuration
TEST_F(StreamContextExtendedTest, EnableStreamInvalidConfig) {
    // Set invalid configuration
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;

    // This update should fail
    VoidResult updateResult = streamContext->updateConfiguration(config);
    EXPECT_TRUE(updateResult.isError());

    // Now try to enable stream
    streamContext->setStage1Enabled(false);
    streamContext->setStage2Enabled(false);
    VoidResult enableResult = streamContext->enableStream();
    EXPECT_TRUE(enableResult.isError());
}

// Test disableStream()
TEST_F(StreamContextExtendedTest, DisableStream) {
    // First enable stream
    streamContext->setStage1Enabled(true);
    VoidResult enableResult = streamContext->enableStream();
    EXPECT_TRUE(enableResult.isOk());

    // Verify enabled
    Result<bool> enabledCheck = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledCheck.isOk());
    EXPECT_TRUE(enabledCheck.getValue());

    // Now disable stream
    VoidResult disableResult = streamContext->disableStream();
    EXPECT_TRUE(disableResult.isOk());

    // Verify disabled
    Result<bool> disabledCheck = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledCheck.isOk());
    EXPECT_FALSE(disabledCheck.getValue());
}

// Test isStreamEnabled() state tracking
TEST_F(StreamContextExtendedTest, IsStreamEnabledTracking) {
    // Initial state should be disabled
    Result<bool> initialState = streamContext->isStreamEnabled();
    EXPECT_TRUE(initialState.isOk());
    EXPECT_FALSE(initialState.getValue());

    // Enable stream
    streamContext->setStage1Enabled(true);
    EXPECT_TRUE(streamContext->enableStream());

    Result<bool> enabledState = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState.isOk());
    EXPECT_TRUE(enabledState.getValue());

    // Disable stream
    EXPECT_TRUE(streamContext->disableStream());

    Result<bool> disabledState = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledState.isOk());
    EXPECT_FALSE(disabledState.getValue());
}

// Test getStreamConfiguration() returns correct state
TEST_F(StreamContextExtendedTest, GetStreamConfiguration) {
    // Set specific configuration
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;
    config.faultMode = FaultMode::Stall;
    EXPECT_TRUE(streamContext->updateConfiguration(config));

    // Retrieve and verify
    StreamConfig retrieved = streamContext->getStreamConfiguration();
    EXPECT_TRUE(retrieved.translationEnabled);
    EXPECT_TRUE(retrieved.stage1Enabled);
    EXPECT_TRUE(retrieved.stage2Enabled);
    EXPECT_EQ(retrieved.faultMode, FaultMode::Stall);
}

// Test getStreamStatistics() returns accurate data
TEST_F(StreamContextExtendedTest, GetStreamStatistics) {
    StreamStatistics stats = streamContext->getStreamStatistics();

    // Verify initial statistics
    EXPECT_GE(stats.creationTimestamp, 0);
    EXPECT_GE(stats.lastAccessTimestamp, 0);
    EXPECT_EQ(stats.pasidCount, 0);
    EXPECT_EQ(stats.translationCount, 0);
    EXPECT_EQ(stats.faultCount, 0);
    EXPECT_EQ(stats.configurationUpdateCount, 0);

    // Perform operations and check updates
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID));

    StreamStatistics updatedStats = streamContext->getStreamStatistics();
    EXPECT_EQ(updatedStats.pasidCount, 1);
}

// Test getStreamState() alias for getStreamConfiguration()
TEST_F(StreamContextExtendedTest, GetStreamState) {
    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config));

    // Both methods should return same state
    StreamConfig fromGetConfig = streamContext->getStreamConfiguration();
    StreamConfig fromGetState = streamContext->getStreamState();

    EXPECT_EQ(fromGetConfig.translationEnabled, fromGetState.translationEnabled);
    EXPECT_EQ(fromGetConfig.stage1Enabled, fromGetState.stage1Enabled);
    EXPECT_EQ(fromGetConfig.stage2Enabled, fromGetState.stage2Enabled);
    EXPECT_EQ(fromGetConfig.faultMode, fromGetState.faultMode);
}

// Test isTranslationActive() with various states
TEST_F(StreamContextExtendedTest, IsTranslationActiveStates) {
    // Initial state: not active (no PASIDs)
    EXPECT_FALSE(streamContext->isTranslationActive());

    // Create PASID but don't enable stream
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID));
    EXPECT_FALSE(streamContext->isTranslationActive());

    // Enable stream and translation
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config));
    EXPECT_TRUE(streamContext->enableStream());

    // Now translation should be active
    EXPECT_TRUE(streamContext->isTranslationActive());

    // Disable stream
    EXPECT_TRUE(streamContext->disableStream());
    EXPECT_FALSE(streamContext->isTranslationActive());
}

// Test hasConfigurationChanged() flag
TEST_F(StreamContextExtendedTest, HasConfigurationChangedFlag) {
    // Update configuration
    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config));

    // Configuration changed flag should be set
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
}

// ============================================================================
// Priority 2: Fault Handler Integration (Lines 700-800)
// ============================================================================

// Test setFaultHandler() with valid handler
TEST_F(StreamContextExtendedTest, SetFaultHandlerValid) {
    VoidResult result = streamContext->setFaultHandler(faultHandler);
    EXPECT_TRUE(result.isOk());

    // Verify handler was set
    EXPECT_TRUE(streamContext->hasFaultHandler());

    std::shared_ptr<FaultHandler> retrieved = streamContext->getFaultHandler();
    EXPECT_NE(retrieved, nullptr);
    EXPECT_EQ(retrieved.get(), faultHandler.get());
}

// Test setFaultHandler() with nullptr (clearing handler)
TEST_F(StreamContextExtendedTest, SetFaultHandlerNull) {
    // First set a handler
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));
    EXPECT_TRUE(streamContext->hasFaultHandler());

    // Clear handler with nullptr
    VoidResult result = streamContext->setFaultHandler(nullptr);
    EXPECT_TRUE(result.isOk());

    // Verify handler was cleared
    EXPECT_FALSE(streamContext->hasFaultHandler());
    EXPECT_EQ(streamContext->getFaultHandler(), nullptr);
}

// Test setFaultHandler() updates statistics
TEST_F(StreamContextExtendedTest, SetFaultHandlerStatistics) {
    StreamStatistics beforeStats = streamContext->getStreamStatistics();
    uint64_t beforeTimestamp = beforeStats.lastAccessTimestamp;

    // Small delay to ensure timestamp difference
    std::this_thread::sleep_for(std::chrono::milliseconds(10));

    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    StreamStatistics afterStats = streamContext->getStreamStatistics();
    EXPECT_GT(afterStats.lastAccessTimestamp, beforeTimestamp);
}

// Test getFaultHandler() when not set
TEST_F(StreamContextExtendedTest, GetFaultHandlerNotSet) {
    std::shared_ptr<FaultHandler> handler = streamContext->getFaultHandler();
    EXPECT_EQ(handler, nullptr);
}

// Test getFaultHandler() when set
TEST_F(StreamContextExtendedTest, GetFaultHandlerSet) {
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    std::shared_ptr<FaultHandler> retrieved = streamContext->getFaultHandler();
    EXPECT_NE(retrieved, nullptr);
    EXPECT_EQ(retrieved.get(), faultHandler.get());
}

// Test recordFault() with handler configured
TEST_F(StreamContextExtendedTest, RecordFaultWithHandler) {
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    StreamStatistics beforeStats = streamContext->getStreamStatistics();
    uint64_t beforeFaultCount = beforeStats.faultCount;

    FaultRecord fault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.faultType = FaultType::TranslationFault;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;

    VoidResult result = streamContext->recordFault(fault);
    EXPECT_TRUE(result.isOk());

    // Verify fault count incremented
    StreamStatistics afterStats = streamContext->getStreamStatistics();
    EXPECT_EQ(afterStats.faultCount, beforeFaultCount + 1);
}

// Test recordFault() without handler configured
TEST_F(StreamContextExtendedTest, RecordFaultWithoutHandler) {
    EXPECT_FALSE(streamContext->hasFaultHandler());

    FaultRecord fault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.faultType = FaultType::TranslationFault;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;

    VoidResult result = streamContext->recordFault(fault);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::FaultHandlingError);
}

// Test recordFault() with different fault types
TEST_F(StreamContextExtendedTest, RecordFaultDifferentTypes) {
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    StreamStatistics beforeStats = streamContext->getStreamStatistics();
    uint64_t beforeFaultCount = beforeStats.faultCount;

    // Translation fault
    FaultRecord fault1;
    fault1.streamID = TEST_STREAM_ID;
    fault1.pasid = TEST_PASID;
    fault1.faultType = FaultType::TranslationFault;
    fault1.address = TEST_IOVA;
    fault1.accessType = AccessType::Read;
    EXPECT_TRUE(streamContext->recordFault(fault1));

    // Permission fault
    FaultRecord fault2;
    fault2.streamID = TEST_STREAM_ID;
    fault2.pasid = TEST_PASID;
    fault2.faultType = FaultType::PermissionFault;
    fault2.address = TEST_IOVA + 0x1000;
    fault2.accessType = AccessType::Write;
    EXPECT_TRUE(streamContext->recordFault(fault2));

    // Access fault
    FaultRecord fault3;
    fault3.streamID = TEST_STREAM_ID;
    fault3.pasid = TEST_PASID;
    fault3.faultType = FaultType::AccessFault;
    fault3.address = TEST_IOVA + 0x2000;
    fault3.accessType = AccessType::Execute;
    EXPECT_TRUE(streamContext->recordFault(fault3));

    // Verify all faults were counted
    StreamStatistics afterStats = streamContext->getStreamStatistics();
    EXPECT_EQ(afterStats.faultCount, beforeFaultCount + 3);
}

// Test recordFault() updates statistics correctly
TEST_F(StreamContextExtendedTest, RecordFaultStatistics) {
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    StreamStatistics beforeStats = streamContext->getStreamStatistics();
    uint64_t beforeTimestamp = beforeStats.lastAccessTimestamp;
    uint64_t beforeFaultCount = beforeStats.faultCount;

    std::this_thread::sleep_for(std::chrono::milliseconds(10));

    FaultRecord fault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.faultType = FaultType::TranslationFault;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;
    EXPECT_TRUE(streamContext->recordFault(fault));

    StreamStatistics afterStats = streamContext->getStreamStatistics();
    EXPECT_EQ(afterStats.faultCount, beforeFaultCount + 1);
    EXPECT_GT(afterStats.lastAccessTimestamp, beforeTimestamp);
}

// Test hasFaultHandler() returns correct state
TEST_F(StreamContextExtendedTest, HasFaultHandlerState) {
    // Initially no handler
    EXPECT_FALSE(streamContext->hasFaultHandler());

    // Set handler
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));
    EXPECT_TRUE(streamContext->hasFaultHandler());

    // Clear handler
    EXPECT_TRUE(streamContext->setFaultHandler(nullptr));
    EXPECT_FALSE(streamContext->hasFaultHandler());
}

// Test clearStreamFaults() with handler configured
TEST_F(StreamContextExtendedTest, ClearStreamFaultsWithHandler) {
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    // Record some faults
    FaultRecord fault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.faultType = FaultType::TranslationFault;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;
    EXPECT_TRUE(streamContext->recordFault(fault));

    // Clear faults
    streamContext->clearStreamFaults();

    // Verify faults were cleared in handler
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_TRUE(faults.empty());
}

// Test clearStreamFaults() without handler configured
TEST_F(StreamContextExtendedTest, ClearStreamFaultsWithoutHandler) {
    EXPECT_FALSE(streamContext->hasFaultHandler());

    // Clear faults should not crash
    streamContext->clearStreamFaults();

    // Verify no handler
    EXPECT_FALSE(streamContext->hasFaultHandler());
}

// Test clearStreamFaults() updates statistics
TEST_F(StreamContextExtendedTest, ClearStreamFaultsStatistics) {
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    StreamStatistics beforeStats = streamContext->getStreamStatistics();
    uint64_t beforeTimestamp = beforeStats.lastAccessTimestamp;

    std::this_thread::sleep_for(std::chrono::milliseconds(10));

    streamContext->clearStreamFaults();

    StreamStatistics afterStats = streamContext->getStreamStatistics();
    EXPECT_GT(afterStats.lastAccessTimestamp, beforeTimestamp);
}

// ============================================================================
// Priority 3: Validation Methods (Lines 800-1000)
// ============================================================================

// Test validateContextDescriptor() with valid descriptor
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorValid) {
    ContextDescriptor cd = createValidContextDescriptor();

    Result<bool> result = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// Test validateContextDescriptor() with invalid PASID
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorInvalidPASID) {
    ContextDescriptor cd = createValidContextDescriptor();

    Result<bool> result = streamContext->validateContextDescriptor(cd, MAX_PASID + 1, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateContextDescriptor() with no valid TTBRs
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorNoValidTTBRs) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.ttbr0Valid = false;
    cd.ttbr1Valid = false;

    Result<bool> result = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateContextDescriptor() with invalid TTBR0
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorInvalidTTBR0) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.ttbr0 = 0;  // Null TTBR

    Result<bool> result = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateContextDescriptor() with invalid TTBR1
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorInvalidTTBR1) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.ttbr1 = 0;  // Null TTBR

    Result<bool> result = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateContextDescriptor() with mismatched address sizes
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorMismatchedAddressSizes) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    cd.tcr.outputAddressSize = AddressSpaceSize::Size32Bit;

    Result<bool> result = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateContextDescriptor() with invalid granule size
TEST_F(StreamContextExtendedTest, ValidateContextDescriptorInvalidGranuleSize) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.tcr.granuleSize = static_cast<TranslationGranule>(99);

    Result<bool> result = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with 4KB granule
TEST_F(StreamContextExtendedTest, ValidateTTBR4KBGranule) {
    // Valid 4KB aligned address
    uint64_t ttbr = 0x1000;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());

    // Invalid 4KB alignment
    ttbr = 0x1001;
    result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with 16KB granule
TEST_F(StreamContextExtendedTest, ValidateTTBR16KBGranule) {
    // Valid 16KB aligned address
    uint64_t ttbr = 0x4000;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size16KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());

    // Invalid 16KB alignment
    ttbr = 0x4001;
    result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size16KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with 64KB granule
TEST_F(StreamContextExtendedTest, ValidateTTBR64KBGranule) {
    // Valid 64KB aligned address
    uint64_t ttbr = 0x10000;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size64KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());

    // Invalid 64KB alignment
    ttbr = 0x10001;
    result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size64KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with null TTBR
TEST_F(StreamContextExtendedTest, ValidateTTBRNull) {
    uint64_t ttbr = 0;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with invalid granule size
TEST_F(StreamContextExtendedTest, ValidateTTBRInvalidGranuleSize) {
    uint64_t ttbr = 0x1000;
    TranslationGranule invalidGranule = static_cast<TranslationGranule>(99);

    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, invalidGranule, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with 32-bit address size
TEST_F(StreamContextExtendedTest, ValidateTTBR32BitAddressSize) {
    // Valid 32-bit address
    uint64_t ttbr = 0xFFFF0000;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size32Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());

    // Address exceeding 32-bit range
    ttbr = 0x100000000ULL;
    result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size32Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with 48-bit address size
TEST_F(StreamContextExtendedTest, ValidateTTBR48BitAddressSize) {
    // Valid 48-bit address
    uint64_t ttbr = 0xFFFFFFFF0000ULL;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());

    // Address exceeding 48-bit range
    ttbr = 0x1000000000000ULL;
    result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with 52-bit address size
TEST_F(StreamContextExtendedTest, ValidateTTBR52BitAddressSize) {
    // Valid 52-bit address
    uint64_t ttbr = 0xFFFFFFFFFF000ULL;
    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size52Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());

    // Address exceeding 52-bit range
    ttbr = 0x10000000000000ULL;
    result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, AddressSpaceSize::Size52Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase() with invalid address size
TEST_F(StreamContextExtendedTest, ValidateTTBRInvalidAddressSize) {
    uint64_t ttbr = 0x1000;
    AddressSpaceSize invalidSize = static_cast<AddressSpaceSize>(99);

    Result<bool> result = streamContext->validateTranslationTableBase(
        ttbr, TranslationGranule::Size4KB, invalidSize);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateASIDConfiguration() with valid ASID
TEST_F(StreamContextExtendedTest, ValidateASIDConfigurationValid) {
    uint16_t asid = 1;
    Result<bool> result = streamContext->validateASIDConfiguration(
        asid, TEST_PASID, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// Test validateASIDConfiguration() with ASID 0
TEST_F(StreamContextExtendedTest, ValidateASIDConfigurationZero) {
    uint16_t asid = 0;
    Result<bool> result = streamContext->validateASIDConfiguration(
        asid, TEST_PASID, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());  // ASID 0 is valid per ARM SMMU v3
}

// Test validateASIDConfiguration() with different security states
TEST_F(StreamContextExtendedTest, ValidateASIDConfigurationSecurityStates) {
    uint16_t asid = 1;

    // Non-secure
    Result<bool> result1 = streamContext->validateASIDConfiguration(
        asid, TEST_PASID, SecurityState::NonSecure);
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result1.getValue());

    // Secure
    Result<bool> result2 = streamContext->validateASIDConfiguration(
        asid, TEST_PASID, SecurityState::Secure);
    EXPECT_TRUE(result2.isOk());
    EXPECT_TRUE(result2.getValue());

    // Realm
    Result<bool> result3 = streamContext->validateASIDConfiguration(
        asid, TEST_PASID, SecurityState::Realm);
    EXPECT_TRUE(result3.isOk());
    EXPECT_TRUE(result3.getValue());
}

// Test validateASIDConfiguration() with invalid security state
TEST_F(StreamContextExtendedTest, ValidateASIDConfigurationInvalidSecurityState) {
    uint16_t asid = 1;
    SecurityState invalidState = static_cast<SecurityState>(99);

    Result<bool> result = streamContext->validateASIDConfiguration(
        asid, TEST_PASID, invalidState);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with valid STE
TEST_F(StreamContextExtendedTest, ValidateSTEValid) {
    StreamTableEntry ste = createValidStreamTableEntry();

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// Test validateStreamTableEntry() with translation but no stages
TEST_F(StreamContextExtendedTest, ValidateSTETranslationNoStages) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.translationEnabled = true;
    ste.stage1Enabled = false;
    ste.stage2Enabled = false;

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with Stage 1 but no CD table base
TEST_F(StreamContextExtendedTest, ValidateSTEStage1NoCDTableBase) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.contextDescriptorTableBase = 0;

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with CD table base misaligned
TEST_F(StreamContextExtendedTest, ValidateSTECDTableBaseMisaligned) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.contextDescriptorTableBase = 0x1001;  // Not 64-byte aligned

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with zero CD table size
TEST_F(StreamContextExtendedTest, ValidateSTEZeroCDTableSize) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.contextDescriptorTableSize = 0;

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with invalid fault mode
TEST_F(StreamContextExtendedTest, ValidateSTEInvalidFaultMode) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.faultMode = static_cast<FaultMode>(99);

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with invalid security state
TEST_F(StreamContextExtendedTest, ValidateSTEInvalidSecurityState) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.securityState = static_cast<SecurityState>(99);

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with invalid Stage 1 granule
TEST_F(StreamContextExtendedTest, ValidateSTEInvalidStage1Granule) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.stage1Granule = static_cast<TranslationGranule>(99);

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry() with invalid Stage 2 granule
TEST_F(StreamContextExtendedTest, ValidateSTEInvalidStage2Granule) {
    StreamTableEntry ste = createValidStreamTableEntry();
    ste.stage2Granule = static_cast<TranslationGranule>(99);

    Result<bool> result = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test generateContextDescriptorFaultSyndrome()
TEST_F(StreamContextExtendedTest, GenerateContextDescriptorFaultSyndrome) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.contextDescriptorIndex = 5;

    PASID pasid = 0x12345;
    uint32_t errorCode = 0xA;

    FaultSyndrome syndrome = streamContext->generateContextDescriptorFaultSyndrome(
        cd, pasid, errorCode);

    // Verify syndrome register encoding
    uint32_t syndromeValue = syndrome.syndromeRegister;

    // Verify fault type (bits 7:0)
    uint32_t faultType = syndromeValue & 0xFF;
    EXPECT_EQ(faultType, static_cast<uint32_t>(FaultType::ContextDescriptorFormatFault));

    // Verify PASID (bits 27:8)
    uint32_t extractedPASID = (syndromeValue >> 8) & 0xFFFFF;
    EXPECT_EQ(extractedPASID, pasid);

    // Verify error code (bits 31:28)
    uint32_t extractedErrorCode = (syndromeValue >> 28) & 0xF;
    EXPECT_EQ(extractedErrorCode, errorCode);

    // Verify fault stage
    EXPECT_EQ(syndrome.faultingStage, FaultStage::Stage1Only);

    // Verify context descriptor index
    EXPECT_EQ(syndrome.contextDescriptorIndex, cd.contextDescriptorIndex);
}

// Test generateContextDescriptorFaultSyndrome() with different parameters
TEST_F(StreamContextExtendedTest, GenerateContextDescriptorFaultSyndromeDifferentParams) {
    ContextDescriptor cd = createValidContextDescriptor();
    cd.contextDescriptorIndex = 15;

    PASID pasid = 0xFFFFF;  // Max PASID
    uint32_t errorCode = 0xF;  // Max error code

    FaultSyndrome syndrome = streamContext->generateContextDescriptorFaultSyndrome(
        cd, pasid, errorCode);

    uint32_t syndromeValue = syndrome.syndromeRegister;

    // Verify PASID
    uint32_t extractedPASID = (syndromeValue >> 8) & 0xFFFFF;
    EXPECT_EQ(extractedPASID, pasid);

    // Verify error code
    uint32_t extractedErrorCode = (syndromeValue >> 28) & 0xF;
    EXPECT_EQ(extractedErrorCode, errorCode);

    // Verify context descriptor index
    EXPECT_EQ(syndrome.contextDescriptorIndex, 15);
}

// ============================================================================
// Integration Tests - Complex Scenarios
// ============================================================================

// Test complete configuration lifecycle
TEST_F(StreamContextExtendedTest, CompleteConfigurationLifecycle) {
    // Start with default configuration
    StreamConfig initialConfig = streamContext->getStreamConfiguration();
    EXPECT_FALSE(initialConfig.translationEnabled);

    // Update to enable translation
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = true;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config1));

    // Apply incremental change to enable Stage 2
    StreamConfig config2 = config1;
    config2.stage2Enabled = true;
    EXPECT_TRUE(streamContext->applyConfigurationChanges(config2));

    // Verify final configuration
    StreamConfig finalConfig = streamContext->getStreamConfiguration();
    EXPECT_TRUE(finalConfig.translationEnabled);
    EXPECT_TRUE(finalConfig.stage1Enabled);
    EXPECT_TRUE(finalConfig.stage2Enabled);
    EXPECT_EQ(finalConfig.faultMode, FaultMode::Terminate);
}

// Test fault handling integration with configuration
TEST_F(StreamContextExtendedTest, FaultHandlingIntegrationWithConfig) {
    // Set up fault handler
    EXPECT_TRUE(streamContext->setFaultHandler(faultHandler));

    // Configure stream
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Stall;
    EXPECT_TRUE(streamContext->updateConfiguration(config));

    // Record faults
    FaultRecord fault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.faultType = FaultType::TranslationFault;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;
    EXPECT_TRUE(streamContext->recordFault(fault));

    // Verify statistics
    StreamStatistics stats = streamContext->getStreamStatistics();
    EXPECT_EQ(stats.faultCount, 1);
    EXPECT_EQ(stats.configurationUpdateCount, 1);

    // Clear faults
    streamContext->clearStreamFaults();
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_TRUE(faults.empty());
}

// Test validation with real configuration
TEST_F(StreamContextExtendedTest, ValidationWithRealConfiguration) {
    // Create valid context descriptor
    ContextDescriptor cd = createValidContextDescriptor();

    // Validate context descriptor
    Result<bool> cdResult = streamContext->validateContextDescriptor(cd, TEST_PASID, TEST_STREAM_ID);
    EXPECT_TRUE(cdResult.isOk());
    EXPECT_TRUE(cdResult.getValue());

    // Create and validate stream table entry
    StreamTableEntry ste = createValidStreamTableEntry();
    Result<bool> steResult = streamContext->validateStreamTableEntry(ste);
    EXPECT_TRUE(steResult.isOk());
    EXPECT_TRUE(steResult.getValue());

    // Apply configuration matching STE
    StreamConfig config;
    config.translationEnabled = ste.translationEnabled;
    config.stage1Enabled = ste.stage1Enabled;
    config.stage2Enabled = ste.stage2Enabled;
    config.faultMode = ste.faultMode;
    EXPECT_TRUE(streamContext->updateConfiguration(config));

    // Verify configuration is valid
    Result<bool> configResult = streamContext->isConfigurationValid(config);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
}

} // namespace test
} // namespace smmu
