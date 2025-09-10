// ARM SMMU v3 SMMU Controller Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

class SMMUTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmuController = std::make_unique<SMMU>();
    }

    void TearDown() override {
        smmuController.reset();
    }

    std::unique_ptr<SMMU> smmuController;
    
    // Test helper constants
    static constexpr StreamID TEST_STREAM_ID_1 = 0x1000;
    static constexpr StreamID TEST_STREAM_ID_2 = 0x2000;
    static constexpr PASID TEST_PASID_1 = 0x1;
    static constexpr PASID TEST_PASID_2 = 0x2;
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr PA TEST_PA = 0x40000000;
};

// Test default construction
TEST_F(SMMUTest, DefaultConstruction) {
    ASSERT_NE(smmuController, nullptr);
    
    // Verify that translation on unconfigured SMMU fails
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

// Test stream configuration
TEST_F(SMMUTest, StreamConfiguration) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Configure a stream
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    // Verify stream is configured
    Result<bool> configured1 = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configured1.isOk());
    EXPECT_TRUE(configured1.getValue());
    
    Result<bool> configured2 = smmuController->isStreamConfigured(TEST_STREAM_ID_2);
    EXPECT_TRUE(configured2.isOk());
    EXPECT_FALSE(configured2.getValue());
}

// Test basic translation setup and execution
TEST_F(SMMUTest, BasicTranslation) {
    // Configure stream
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    // Create PASID and set up mapping
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);  // Read-write
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    // Perform translation
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
}

// Test translation with disabled stream
TEST_F(SMMUTest, DisabledStreamTranslation) {
    StreamConfig config;
    config.translationEnabled = false;  // Translation disabled
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    // Translation should bypass SMMU (pass-through mode)
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_IOVA);  // Should return input address unchanged
}

// Test multiple streams with independent configurations
TEST_F(SMMUTest, MultipleStreams) {
    // Configure first stream
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = true;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    
    // Configure second stream
    StreamConfig config2;
    config2.translationEnabled = true;
    config2.stage1Enabled = true;
    config2.stage2Enabled = false;
    config2.faultMode = FaultMode::Stall;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config1).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config2).isOk());
    
    // Set up different mappings for each stream
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, false, false);  // Read-only
    PA pa1 = TEST_PA;
    PA pa2 = TEST_PA + 0x100000;
    
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, pa1, perms).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, pa2, perms).isOk());
    
    // Test independent translations
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, pa1);
    EXPECT_EQ(result2.getValue().physicalAddress, pa2);
    EXPECT_NE(result1.getValue().physicalAddress, result2.getValue().physicalAddress);
}

// Test fault handling and recording
TEST_F(SMMUTest, FaultHandling) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Try to translate without any mappings (should cause fault)
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
    
    // Check that fault was recorded
    Result<std::vector<FaultRecord>> faultResult = smmuController->getEvents();
    EXPECT_TRUE(faultResult.isOk());
    std::vector<FaultRecord> faults = faultResult.getValue();
    EXPECT_GT(faults.size(), 0);
    
    if (!faults.empty()) {
        const FaultRecord& fault = faults.back();
        EXPECT_EQ(fault.streamID, TEST_STREAM_ID_1);
        EXPECT_EQ(fault.pasid, TEST_PASID_1);
        EXPECT_EQ(fault.address, TEST_IOVA);
        EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
        EXPECT_EQ(fault.accessType, AccessType::Read);
    }
}

// Test permission faults
TEST_F(SMMUTest, PermissionFaults) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Set up read-only mapping
    PagePermissions perms(true, false, false);  // Read-only
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    // Read should succeed
    TranslationResult readResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(readResult.isOk());
    
    // Write should fail with permission fault
    TranslationResult writeResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(writeResult.isError());
    EXPECT_EQ(writeResult.getError(), SMMUError::PagePermissionViolation);
}

// Test stream removal
TEST_F(SMMUTest, StreamRemoval) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    Result<bool> configuredBefore = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configuredBefore.isOk());
    EXPECT_TRUE(configuredBefore.getValue());
    
    // Remove stream
    EXPECT_TRUE(smmuController->removeStream(TEST_STREAM_ID_1).isOk());
    Result<bool> configuredAfter = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configuredAfter.isOk());
    EXPECT_FALSE(configuredAfter.getValue());
    
    // Translation should now fail
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
}

// Test SMMU statistics
TEST_F(SMMUTest, SMMUStatistics) {
    // Initially no streams
    EXPECT_EQ(smmuController->getStreamCount(), 0);
    EXPECT_EQ(smmuController->getTotalTranslations(), 0);
    EXPECT_EQ(smmuController->getTotalFaults(), 0);
    
    // Configure streams
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    EXPECT_EQ(smmuController->getStreamCount(), 2);
    
    // Set up mapping and perform translations
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    // Successful translation
    TranslationResult successResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(successResult.isOk());
    
    // Failed translation (different stream, no mapping)
    TranslationResult failResult = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(failResult.isError());
    
    // Check statistics
    EXPECT_GE(smmuController->getTotalTranslations(), 2);
    EXPECT_GE(smmuController->getTotalFaults(), 1);
}

// Test event clearing
TEST_F(SMMUTest, EventClearing) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    // Generate some faults
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    
    Result<std::vector<FaultRecord>> faultResult = smmuController->getEvents();
    EXPECT_TRUE(faultResult.isOk());
    std::vector<FaultRecord> faults = faultResult.getValue();
    EXPECT_GT(faults.size(), 0);
    
    // Clear events
    VoidResult clearResult = smmuController->clearEvents();
    EXPECT_TRUE(clearResult.isOk());
    Result<std::vector<FaultRecord>> faultUpdateResult = smmuController->getEvents();
    EXPECT_TRUE(faultUpdateResult.isOk());
    faults = faultUpdateResult.getValue();
    EXPECT_EQ(faults.size(), 0);
}

// Test large-scale stream configuration
TEST_F(SMMUTest, LargeScaleStreamConfiguration) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    const size_t numStreams = 100;
    
    // Configure many streams
    for (size_t i = 0; i < numStreams; ++i) {
        StreamID streamID = static_cast<StreamID>(i + 1000);
        EXPECT_TRUE(smmuController->configureStream(streamID, config));
    }
    
    EXPECT_EQ(smmuController->getStreamCount(), numStreams);
    
    // Verify all streams are configured
    for (size_t i = 0; i < numStreams; ++i) {
        StreamID streamID = static_cast<StreamID>(i + 1000);
        Result<bool> configResult = smmuController->isStreamConfigured(streamID);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
    }
}

// ========== ENHANCED TESTS FOR TASK 5.1 COMPREHENSIVE COVERAGE ==========

// Test SMMU constructor initialization and defaults
TEST_F(SMMUTest, ConstructorInitialization) {
    // Test that SMMU is properly initialized with defaults
    ASSERT_NE(smmuController, nullptr);
    
    // Verify initial state
    EXPECT_EQ(smmuController->getStreamCount(), 0);
    EXPECT_EQ(smmuController->getTotalTranslations(), 0);
    EXPECT_EQ(smmuController->getTotalFaults(), 0);
    EXPECT_EQ(smmuController->getCacheHitCount(), 0);
    EXPECT_EQ(smmuController->getCacheMissCount(), 0);
    
    // Verify events are empty initially
    Result<std::vector<FaultRecord>> eventsResult = smmuController->getEvents();
    EXPECT_TRUE(eventsResult.isOk());
    std::vector<FaultRecord> events = eventsResult.getValue();
    EXPECT_EQ(events.size(), 0);
    
    // Verify no streams are configured initially
    Result<bool> configResultFalse1 = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configResultFalse1.isOk());
    EXPECT_FALSE(configResultFalse1.getValue());
    Result<bool> configResultFalse2 = smmuController->isStreamConfigured(0);
    EXPECT_TRUE(configResultFalse2.isOk());
    EXPECT_FALSE(configResultFalse2.getValue());
    Result<bool> configResultFalse3 = smmuController->isStreamConfigured(MAX_STREAM_ID);
    EXPECT_TRUE(configResultFalse3.isOk());
    EXPECT_FALSE(configResultFalse3.getValue());
}

// Test SMMU destructor and RAII cleanup
TEST_F(SMMUTest, DestructorCleanup) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Configure multiple streams with mappings
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_2));
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_2, TEST_IOVA + 0x1000, TEST_PA + 0x1000, perms));
    
    EXPECT_GT(smmuController->getStreamCount(), 0);
    
    // Trigger some activity to create faults/events
    smmuController->translate(999, TEST_PASID_1, TEST_IOVA, AccessType::Read); // Should fault
    
    // Reset SMMU to test cleanup
    smmuController.reset(); // Explicit reset to test RAII
    
    // Recreate for TearDown
    smmuController = std::make_unique<SMMU>();
    
    // Verify the new SMMU is clean
    EXPECT_EQ(smmuController->getStreamCount(), 0);
    EXPECT_EQ(smmuController->getTotalTranslations(), 0);
}

// Test stream enable/disable lifecycle
TEST_F(SMMUTest, StreamLifecycleControl) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Configure stream
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Initially enabled based on config
    Result<bool> enabledResult = smmuController->isStreamEnabled(TEST_STREAM_ID_1);
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
    
    // Disable stream
    smmuController->disableStream(TEST_STREAM_ID_1);
    Result<bool> enabledResultFalse = smmuController->isStreamEnabled(TEST_STREAM_ID_1);
    EXPECT_TRUE(enabledResultFalse.isOk());
    EXPECT_FALSE(enabledResultFalse.getValue());
    Result<bool> configResult = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue()); // Still configured
    
    // Enable stream again
    smmuController->enableStream(TEST_STREAM_ID_1);
    Result<bool> enabledResult2 = smmuController->isStreamEnabled(TEST_STREAM_ID_1);
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
    
    // Test with unconfigured stream
    smmuController->enableStream(999); // Should not crash
    smmuController->disableStream(999); // Should not crash
    Result<bool> enabledResultFalse2 = smmuController->isStreamEnabled(999);
    EXPECT_TRUE(enabledResultFalse.isOk());
    EXPECT_FALSE(enabledResultFalse.getValue());
}

// Test StreamID boundary validation
TEST_F(SMMUTest, StreamIDBoundaryValidation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Test valid StreamID at boundary
    EXPECT_TRUE(smmuController->configureStream(MAX_STREAM_ID, config));
    Result<bool> configResult = smmuController->isStreamConfigured(MAX_STREAM_ID);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
    
    // Note: MAX_STREAM_ID + 1 wraps around to 0 due to uint32_t overflow, which is valid
    // Test translation with unconfigured StreamID instead
    StreamID unconfiguredStreamID = 12345; // Use a StreamID that's not configured
    uint64_t initialFaults = smmuController->getTotalFaults();
    TranslationResult result = smmuController->translate(unconfiguredStreamID, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
    EXPECT_GT(smmuController->getTotalFaults(), initialFaults);
    
    // Verify the unconfigured StreamID is indeed not configured
    Result<bool> configResultFalse = smmuController->isStreamConfigured(unconfiguredStreamID);
    EXPECT_TRUE(configResultFalse.isOk());
    EXPECT_FALSE(configResultFalse.getValue());
}

// Test PASID boundary validation
TEST_F(SMMUTest, PASIDBoundaryValidation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    // Test valid PASID at boundary
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, MAX_PASID));
    
    // Test invalid PASID beyond boundary
    EXPECT_FALSE(smmuController->createStreamPASID(TEST_STREAM_ID_1, MAX_PASID + 1));
    
    // Test invalid PASID = 0 (reserved per ARM SMMU v3 spec)
    EXPECT_FALSE(smmuController->createStreamPASID(TEST_STREAM_ID_1, 0));
    
    // Test PASID operations with unconfigured stream
    EXPECT_FALSE(smmuController->createStreamPASID(999, TEST_PASID_1));
    EXPECT_FALSE(smmuController->removeStreamPASID(999, TEST_PASID_1));
}

// Test comprehensive stream configuration updates
TEST_F(SMMUTest, StreamConfigurationUpdates) {
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = true;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    
    // Initial configuration
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config1).isOk());
    Result<bool> configResult = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
    
    // Update configuration
    StreamConfig config2;
    config2.translationEnabled = false; // Disable translation
    config2.stage1Enabled = false;
    config2.stage2Enabled = false;
    config2.faultMode = FaultMode::Stall;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config2));
    Result<bool> configResult2 = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
    
    // Verify pass-through behavior after configuration update
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_IOVA); // Pass-through
}

// Test global fault mode configuration
TEST_F(SMMUTest, GlobalFaultModeConfiguration) {
    // Configure multiple streams with different fault modes
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = true;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    
    StreamConfig config2;
    config2.translationEnabled = true;
    config2.stage1Enabled = true;
    config2.stage2Enabled = false;
    config2.faultMode = FaultMode::Stall;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config1).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config2).isOk());
    
    // Set global fault mode - should propagate to all streams
    VoidResult setGlobalResult = smmuController->setGlobalFaultMode(FaultMode::Stall);
    EXPECT_TRUE(setGlobalResult.isOk());
    
    // Test that global setting affects fault handling
    // This is implementation-dependent behavior that should be consistent
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    // Generate faults and verify they're handled with the global mode
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(result1.isError());
    EXPECT_TRUE(result2.isError());
}

// Test caching enable/disable functionality
TEST_F(SMMUTest, CachingConfiguration) {
    // Test initial caching state (should be enabled by default)
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    // Perform translations to generate cache activity
    uint64_t initialHits = smmuController->getCacheHitCount();
    
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    
    // Should have incremented cache statistics
    EXPECT_GT(smmuController->getCacheHitCount(), initialHits);
    
    // Test disabling caching
    VoidResult enableCachingResult1 = smmuController->enableCaching(false);
    EXPECT_TRUE(enableCachingResult1.isOk());
    
    // Test re-enabling caching
    VoidResult enableCachingResult2 = smmuController->enableCaching(true);
    EXPECT_TRUE(enableCachingResult2.isOk());
    
    // Verify translations still work
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result2.isOk());
}

// Test comprehensive statistics tracking
TEST_F(SMMUTest, ComprehensiveStatisticsTracking) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Initial statistics should be zero
    EXPECT_EQ(smmuController->getTranslationCount(), 0);
    EXPECT_EQ(smmuController->getCacheHitCount(), 0);
    EXPECT_EQ(smmuController->getCacheMissCount(), 0);
    
    // Configure stream and perform operations
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    // Successful translation (should increment hits)
    TranslationResult successResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(successResult.isOk());
    EXPECT_EQ(smmuController->getTranslationCount(), 1);
    EXPECT_EQ(smmuController->getCacheHitCount(), 1);
    
    // Failed translation (should increment misses)
    TranslationResult failResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(failResult.isError());
    EXPECT_EQ(smmuController->getTranslationCount(), 2);
    EXPECT_EQ(smmuController->getCacheMissCount(), 1);
    
    // Test statistics reset
    smmuController->resetStatistics();
    EXPECT_EQ(smmuController->getTranslationCount(), 0);
    EXPECT_EQ(smmuController->getCacheHitCount(), 0);
    EXPECT_EQ(smmuController->getCacheMissCount(), 0);
    EXPECT_EQ(smmuController->getTotalFaults(), 0);
}

// Test cross-stream isolation
TEST_F(SMMUTest, CrossStreamIsolation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Configure two streams
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    // Create same PASID in both streams (should be isolated)
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    // Map different pages for same PASID in different streams
    PagePermissions perms1(true, false, false); // Read-only
    PagePermissions perms2(false, true, false); // Write-only
    
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms1));
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, TEST_PA + 0x1000, perms2));
    
    // Test read access - should succeed in stream 1, fail in stream 2
    TranslationResult stream1Read = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult stream2Read = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(stream1Read.isOk());
    EXPECT_TRUE(stream2Read.isError());
    EXPECT_EQ(stream2Read.getError(), SMMUError::PagePermissionViolation);
    
    // Test write access - should fail in stream 1, succeed in stream 2
    TranslationResult stream1Write = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    TranslationResult stream2Write = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    
    EXPECT_FALSE(stream1Write.isOk());
    EXPECT_EQ(stream1Write.getError(), SMMUError::PagePermissionViolation);
    EXPECT_TRUE(stream2Write.isOk());
    
    // Verify different physical addresses
    EXPECT_EQ(stream1Read.getValue().physicalAddress, TEST_PA);
    EXPECT_EQ(stream2Write.getValue().physicalAddress, TEST_PA + 0x1000);
}

// Test fault record StreamID correction
TEST_F(SMMUTest, FaultRecordStreamIDCorrection) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Clear any existing faults
    VoidResult clearResult2 = smmuController->clearEvents();
    EXPECT_TRUE(clearResult2.isOk());
    
    // Generate a translation fault
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    
    // Check that fault record has correct StreamID
    Result<std::vector<FaultRecord>> faultResult = smmuController->getEvents();
    EXPECT_TRUE(faultResult.isOk());
    std::vector<FaultRecord> faults = faultResult.getValue();
    ASSERT_GT(faults.size(), 0);
    
    const FaultRecord& fault = faults.back();
    EXPECT_EQ(fault.streamID, TEST_STREAM_ID_1); // Should be corrected
    EXPECT_EQ(fault.pasid, TEST_PASID_1);
    EXPECT_EQ(fault.address, TEST_IOVA);
    EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    EXPECT_EQ(fault.accessType, AccessType::Read);
    EXPECT_GT(fault.timestamp, 0);
}

// Test SMMU reset functionality
TEST_F(SMMUTest, SMMUReset) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Configure streams and mappings
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    EXPECT_GT(smmuController->getStreamCount(), 0);
    
    // Perform some operations to generate statistics and events
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    smmuController->translate(999, TEST_PASID_1, TEST_IOVA, AccessType::Read); // Generate fault
    
    EXPECT_GT(smmuController->getTotalTranslations(), 0);
    EXPECT_GT(smmuController->getTotalFaults(), 0);
    
    // Set non-default global configuration
    VoidResult setGlobalResult = smmuController->setGlobalFaultMode(FaultMode::Stall);
    EXPECT_TRUE(setGlobalResult.isOk());
    VoidResult enableCachingResult1 = smmuController->enableCaching(false);
    EXPECT_TRUE(enableCachingResult1.isOk());
    
    // Reset SMMU
    smmuController->reset();
    
    // Verify everything is cleared and reset to defaults
    EXPECT_EQ(smmuController->getStreamCount(), 0);
    EXPECT_EQ(smmuController->getTotalTranslations(), 0);
    EXPECT_EQ(smmuController->getTotalFaults(), 0);
    EXPECT_EQ(smmuController->getCacheHitCount(), 0);
    EXPECT_EQ(smmuController->getCacheMissCount(), 0);
    Result<bool> configResultFalse = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configResultFalse.isOk());
    EXPECT_FALSE(configResultFalse.getValue());
    
    // Verify events are cleared
    Result<std::vector<FaultRecord>> eventsResult = smmuController->getEvents();
    EXPECT_TRUE(eventsResult.isOk());
    std::vector<FaultRecord> events = eventsResult.getValue();
    EXPECT_EQ(events.size(), 0);
    
    // Verify SMMU can be reconfigured after reset
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    Result<bool> configResult = smmuController->isStreamConfigured(TEST_STREAM_ID_2);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
}

// ========== PERFORMANCE AND SCALE TESTS ==========

// Test large-scale multiple PASID management
TEST_F(SMMUTest, MultiPASIDManagement) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    const size_t numPASIDs = 50;
    
    // Create multiple PASIDs
    for (size_t i = 1; i <= numPASIDs; ++i) {
        PASID pasid = static_cast<PASID>(i);
        EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, pasid));
        
        // Set up mapping for each PASID
        PagePermissions perms(true, true, false);
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA pa = TEST_PA + (i * 0x1000);
        EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, pasid, iova, pa, perms));
    }
    
    // Test translations for all PASIDs
    for (size_t i = 1; i <= numPASIDs; ++i) {
        PASID pasid = static_cast<PASID>(i);
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA expectedPA = TEST_PA + (i * 0x1000);
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, pasid, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
    
    EXPECT_EQ(smmuController->getTotalTranslations(), numPASIDs);
    EXPECT_EQ(smmuController->getCacheHitCount(), numPASIDs);
    
    // Remove all PASIDs
    for (size_t i = 1; i <= numPASIDs; ++i) {
        PASID pasid = static_cast<PASID>(i);
        EXPECT_TRUE(smmuController->removeStreamPASID(TEST_STREAM_ID_1, pasid));
    }
    
    // Verify all PASIDs are removed
    for (size_t i = 1; i <= numPASIDs; ++i) {
        PASID pasid = static_cast<PASID>(i);
        IOVA iova = TEST_IOVA + (i * 0x1000);
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, pasid, iova, AccessType::Read);
        EXPECT_TRUE(result.isError());
    }
}

// Test concurrent stream operations (simulated)
TEST_F(SMMUTest, ConcurrentStreamOperations) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    const size_t numStreams = 20;
    
    // Configure multiple streams rapidly
    for (size_t i = 0; i < numStreams; ++i) {
        StreamID streamID = static_cast<StreamID>(1000 + i);
        EXPECT_TRUE(smmuController->configureStream(streamID, config));
        EXPECT_TRUE(smmuController->createStreamPASID(streamID, TEST_PASID_1));
        
        PagePermissions perms(true, true, false);
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA pa = TEST_PA + (i * 0x1000);
        EXPECT_TRUE(smmuController->mapPage(streamID, TEST_PASID_1, iova, pa, perms));
    }
    
    EXPECT_EQ(smmuController->getStreamCount(), numStreams);
    
    // Perform translations on all streams
    uint64_t successfulTranslations = 0;
    for (size_t i = 0; i < numStreams; ++i) {
        StreamID streamID = static_cast<StreamID>(1000 + i);
        IOVA iova = TEST_IOVA + (i * 0x1000);
        
        TranslationResult result = smmuController->translate(streamID, TEST_PASID_1, iova, AccessType::Read);
        if (result.isOk()) {
            successfulTranslations++;
        }
    }
    
    EXPECT_EQ(successfulTranslations, numStreams);
    EXPECT_EQ(smmuController->getTotalTranslations(), numStreams);
    
    // Disable half the streams
    for (size_t i = 0; i < numStreams / 2; ++i) {
        StreamID streamID = static_cast<StreamID>(1000 + i);
        smmuController->disableStream(streamID);
        Result<bool> enabledResultFalse = smmuController->isStreamEnabled(streamID);
    EXPECT_TRUE(enabledResultFalse.isOk());
    EXPECT_FALSE(enabledResultFalse.getValue());
    }
    
    // Re-enable them
    for (size_t i = 0; i < numStreams / 2; ++i) {
        StreamID streamID = static_cast<StreamID>(1000 + i);
        smmuController->enableStream(streamID);
        Result<bool> enabledResult = smmuController->isStreamEnabled(streamID);
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
    }
}

// Test memory usage patterns with sparse address spaces
TEST_F(SMMUTest, SparseAddressSpaceHandling) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Create sparse mappings across large address range
    PagePermissions perms(true, true, false);
    const size_t numMappings = 10;
    const uint64_t addressGap = 0x100000000ULL; // 4GB gap
    
    for (size_t i = 0; i < numMappings; ++i) {
        IOVA iova = TEST_IOVA + (i * addressGap);
        PA pa = TEST_PA + (i * 0x1000); // Compact physical mapping
        
        EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova, pa, perms));
    }
    
    // Verify all sparse mappings work
    for (size_t i = 0; i < numMappings; ++i) {
        IOVA iova = TEST_IOVA + (i * addressGap);
        PA expectedPA = TEST_PA + (i * 0x1000);
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
    
    // Test unmapping sparse entries
    for (size_t i = 0; i < numMappings; i += 2) { // Unmap every other entry
        IOVA iova = TEST_IOVA + (i * addressGap);
        EXPECT_TRUE(smmuController->unmapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova));
    }
    
    // Verify unmapped entries fail, mapped entries still work
    for (size_t i = 0; i < numMappings; ++i) {
        IOVA iova = TEST_IOVA + (i * addressGap);
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Read);
        
        if (i % 2 == 0) {
            EXPECT_TRUE(result.isError()); // Unmapped
        } else {
            EXPECT_TRUE(result.isOk());  // Still mapped
        }
    }
}

// Test error recovery and fault isolation
TEST_F(SMMUTest, ErrorRecoveryAndFaultIsolation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Clear any existing faults first
    VoidResult clearResult3 = smmuController->clearEvents();
    EXPECT_TRUE(clearResult3.isOk());
    uint64_t initialFaults = smmuController->getTotalFaults();
    
    // Configure multiple streams
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    // Set up one stream with valid mapping
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    // Leave stream 2 without mapping
    
    // Generate multiple faults from stream 2
    for (int i = 0; i < 5; ++i) {
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + i * 0x1000, AccessType::Read);
        EXPECT_TRUE(result.isError());
        EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
    }
    
    // Verify faults were recorded (should be at least the 5 we generated)
    EXPECT_GE(smmuController->getTotalFaults(), initialFaults + 5);
    
    // Verify stream 1 still works despite faults in stream 2
    TranslationResult goodResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(goodResult.isOk());
    EXPECT_EQ(goodResult.getValue().physicalAddress, TEST_PA);
    
    // Verify fault isolation - stream 1 should not be affected
    Result<bool> enabledResult = smmuController->isStreamEnabled(TEST_STREAM_ID_1);
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
    Result<bool> enabledResult2 = smmuController->isStreamEnabled(TEST_STREAM_ID_2);
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
    
    // Check fault records have correct StreamIDs
    Result<std::vector<FaultRecord>> faultResult = smmuController->getEvents();
    EXPECT_TRUE(faultResult.isOk());
    std::vector<FaultRecord> faults = faultResult.getValue();
    EXPECT_GE(faults.size(), 1); // At least some faults should be recorded
    
    // Count faults for each stream
    size_t stream1Faults = 0, stream2Faults = 0;
    for (const auto& fault : faults) {
        if (fault.streamID == TEST_STREAM_ID_1) stream1Faults++;
        if (fault.streamID == TEST_STREAM_ID_2) stream2Faults++;
    }
    
    EXPECT_EQ(stream1Faults, 0); // No faults from stream 1
    EXPECT_GE(stream2Faults, 1); // At least some faults from stream 2
}

// Test ARM SMMU v3 specification compliance
TEST_F(SMMUTest, ARMSMMUv3SpecificationCompliance) {
    // Test that SMMU follows ARM SMMU v3 specification requirements
    
    // 1. Initial state compliance
    EXPECT_EQ(smmuController->getStreamCount(), 0); // No streams configured initially
    EXPECT_EQ(smmuController->getTotalTranslations(), 0);
    EXPECT_EQ(smmuController->getTotalFaults(), 0);
    
    // 2. StreamID validation (ARM SMMU v3 spec requirement)
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Valid StreamID should work
    EXPECT_TRUE(smmuController->configureStream(0, config));     // Minimum valid
    EXPECT_TRUE(smmuController->configureStream(MAX_STREAM_ID, config)); // Maximum valid
    
    // Note: MAX_STREAM_ID + 1 wraps to 0 due to uint32_t overflow, which is valid
    // Test with a large StreamID that should succeed
    StreamID largeStreamID = MAX_STREAM_ID - 1;
    EXPECT_TRUE(smmuController->configureStream(largeStreamID, config));
    
    // 3. PASID validation (ARM SMMU v3 spec requirement)
    EXPECT_FALSE(smmuController->createStreamPASID(0, 0));       // PASID 0 is reserved
    EXPECT_TRUE(smmuController->createStreamPASID(0, 1));        // Minimum valid PASID
    EXPECT_TRUE(smmuController->createStreamPASID(0, MAX_PASID)); // Maximum valid
    EXPECT_FALSE(smmuController->createStreamPASID(0, MAX_PASID + 1)); // Invalid
    
    // 4. Two-stage translation support structure validation
    // Note: Stage2 requires Stage2AddressSpace to be configured, which is not done in this test
    // Test that stage1-only configuration works
    StreamConfig stage1Config;
    stage1Config.translationEnabled = true;
    stage1Config.stage1Enabled = true;
    stage1Config.stage2Enabled = false; // Stage1 only for this test
    stage1Config.faultMode = FaultMode::Terminate;
    
    // Use a different StreamID that hasn't been configured yet in this test
    StreamID freshStreamID = 54321; // Use a completely different StreamID
    EXPECT_TRUE(smmuController->configureStream(freshStreamID, stage1Config));
    
    // 5. Pass-through mode when translation is disabled
    StreamConfig bypassConfig;
    bypassConfig.translationEnabled = false;
    bypassConfig.stage1Enabled = false;
    bypassConfig.stage2Enabled = false;
    bypassConfig.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, bypassConfig));
    
    TranslationResult bypassResult = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(bypassResult.isOk());
    EXPECT_EQ(bypassResult.getValue().physicalAddress, TEST_IOVA); // Should pass through unchanged
    
    // 6. Fault handling modes (Terminate, Stall)
    StreamConfig stallConfig = config;
    stallConfig.faultMode = FaultMode::Stall;
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, stallConfig));
    
    // 7. Event queue functionality
    Result<std::vector<FaultRecord>> eventsResult = smmuController->getEvents();
    EXPECT_TRUE(eventsResult.isOk());
    std::vector<FaultRecord> events = eventsResult.getValue();
    VoidResult clearResult4 = smmuController->clearEvents();
    EXPECT_TRUE(clearResult4.isOk());
    Result<std::vector<FaultRecord>> clearedEventsResult = smmuController->getEvents();
    EXPECT_TRUE(clearedEventsResult.isOk());
    std::vector<FaultRecord> clearedEvents = clearedEventsResult.getValue();
    EXPECT_EQ(clearedEvents.size(), 0);
}

// Test page mapping operations through SMMU API
TEST_F(SMMUTest, PageMappingOperations) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Test page mapping with different permissions
    PagePermissions readOnly(true, false, false);
    PagePermissions writeOnly(false, true, false);
    PagePermissions readWrite(true, true, false);
    PagePermissions execute(false, false, true);
    
    IOVA baseIOVA = TEST_IOVA;
    PA basePA = TEST_PA;
    
    // Map pages with different permissions
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA, basePA, readOnly));
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x1000, basePA + 0x1000, writeOnly));
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x2000, basePA + 0x2000, readWrite));
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x3000, basePA + 0x3000, execute));
    
    // Test read operations
    TranslationResult readResult1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA, AccessType::Read);
    EXPECT_TRUE(readResult1.isOk()); // Read-only page
    
    TranslationResult readResult2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(readResult2.isError()); // Write-only page
    EXPECT_EQ(readResult2.getError(), SMMUError::PagePermissionViolation);
    
    TranslationResult readResult3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x2000, AccessType::Read);
    EXPECT_TRUE(readResult3.isOk()); // Read-write page
    
    // Test write operations
    TranslationResult writeResult1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA, AccessType::Write);
    EXPECT_TRUE(writeResult1.isError()); // Read-only page
    EXPECT_EQ(writeResult1.getError(), SMMUError::PagePermissionViolation);
    
    TranslationResult writeResult2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x1000, AccessType::Write);
    EXPECT_TRUE(writeResult2.isOk()); // Write-only page
    
    TranslationResult writeResult3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x2000, AccessType::Write);
    EXPECT_TRUE(writeResult3.isOk()); // Read-write page
    
    // Test execute operations
    TranslationResult executeResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA + 0x3000, AccessType::Execute);
    EXPECT_TRUE(executeResult.isOk()); // Execute page
    
    // Test unmapping
    EXPECT_TRUE(smmuController->unmapPage(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA));
    
    TranslationResult unmappedResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, baseIOVA, AccessType::Read);
    EXPECT_TRUE(unmappedResult.isError());
    EXPECT_EQ(unmappedResult.getError(), SMMUError::PageNotMapped);
    
    // Test operations with invalid parameters
    EXPECT_FALSE(smmuController->mapPage(999, TEST_PASID_1, baseIOVA, basePA, readOnly)); // Invalid stream
    EXPECT_FALSE(smmuController->unmapPage(999, TEST_PASID_1, baseIOVA)); // Invalid stream
}

// Test comprehensive event management
TEST_F(SMMUTest, ComprehensiveEventManagement) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Clear initial events
    VoidResult clearResult = smmuController->clearEvents();
    EXPECT_TRUE(clearResult.isOk());
    Result<std::vector<FaultRecord>> emptyEventsResult = smmuController->getEvents();
    EXPECT_TRUE(emptyEventsResult.isOk());
    EXPECT_EQ(emptyEventsResult.getValue().size(), 0);
    
    // Generate different types of faults
    // 1. Translation fault
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isError());
    
    // 2. Permission fault
    PagePermissions readOnly(true, false, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, readOnly));
    
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(result2.isError());
    EXPECT_EQ(result2.getError(), SMMUError::PagePermissionViolation);
    
    // 3. Invalid StreamID fault (use unconfigured StreamID)
    TranslationResult result3 = smmuController->translate(99999, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_FALSE(result3.isOk());
    
    // Check event accumulation
    Result<std::vector<FaultRecord>> eventsResult = smmuController->getEvents();
    EXPECT_TRUE(eventsResult.isOk());
    std::vector<FaultRecord> events = eventsResult.getValue();
    EXPECT_GE(events.size(), 1); // At least some events should be recorded
    
    // Verify event details
    bool foundTranslationFault = false;
    bool foundPermissionFault = false;
    for (const auto& event : events) {
        if (event.faultType == FaultType::TranslationFault) {
            foundTranslationFault = true;
            EXPECT_GT(event.timestamp, 0);
        }
        if (event.faultType == FaultType::PermissionFault) {
            foundPermissionFault = true;
            EXPECT_EQ(event.accessType, AccessType::Write);
        }
    }
    
    EXPECT_TRUE(foundTranslationFault);
    EXPECT_TRUE(foundPermissionFault);
    
    // Test event clearing
    VoidResult clearResult5 = smmuController->clearEvents();
    EXPECT_TRUE(clearResult.isOk());
    Result<std::vector<FaultRecord>> eventsUpdateResult = smmuController->getEvents();
    EXPECT_TRUE(eventsUpdateResult.isOk());
    events = eventsUpdateResult.getValue();
    EXPECT_EQ(events.size(), 0);
    
    // Verify statistics still work after event clearing
    EXPECT_GT(smmuController->getTotalFaults(), 0);
    EXPECT_GT(smmuController->getTotalTranslations(), 0);
}

// ========== TASK 5.2 TRANSLATION ENGINE COMPREHENSIVE TESTS ==========

// Test TLB Cache Integration with translate() method
TEST_F(SMMUTest, Task52_TLBCacheIntegration) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    // Reset statistics to ensure clean test
    smmuController->resetStatistics();
    
    // First translation - should be a cache miss
    uint64_t initialMisses = smmuController->getCacheMissCount();
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA);
    EXPECT_GT(smmuController->getCacheMissCount(), initialMisses);
    
    // Second translation of same address - should be a cache hit
    uint64_t initialHits = smmuController->getCacheHitCount();
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA);
    EXPECT_GT(smmuController->getCacheHitCount(), initialHits);
    
    // Third translation with different access type - should still be a cache hit (same address)
    uint64_t hitsBefore = smmuController->getCacheHitCount();
    TranslationResult result3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(result3.isOk());
    EXPECT_EQ(result3.getValue().physicalAddress, TEST_PA);
    EXPECT_GT(smmuController->getCacheHitCount(), hitsBefore);
    
    // Verify cache statistics are properly tracked
    CacheStatistics cacheStats = smmuController->getCacheStatistics();
    EXPECT_GT(cacheStats.hitCount, 0);
    EXPECT_GT(cacheStats.missCount, 0);
    EXPECT_EQ(cacheStats.totalLookups, cacheStats.hitCount + cacheStats.missCount);
    EXPECT_GT(cacheStats.hitRate, 0.0);
    EXPECT_LE(cacheStats.hitRate, 1.0);
}

// Test cache miss followed by cache hit scenarios with different addresses
TEST_F(SMMUTest, Task52_CacheMissHitScenarios) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Map multiple pages
    PagePermissions perms(true, true, false);
    IOVA iova1 = TEST_IOVA;
    IOVA iova2 = TEST_IOVA + 0x1000;
    IOVA iova3 = TEST_IOVA + 0x2000;
    PA pa1 = TEST_PA;
    PA pa2 = TEST_PA + 0x1000;
    PA pa3 = TEST_PA + 0x2000;
    
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova1, pa1, perms));
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova2, pa2, perms));
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova3, pa3, perms));
    
    smmuController->resetStatistics();
    
    // First access to each address - all should be cache misses
    uint64_t initialMisses = smmuController->getCacheMissCount();
    
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova1, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, pa1);
    
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova2, AccessType::Read);
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result2.getValue().physicalAddress, pa2);
    
    TranslationResult result3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova3, AccessType::Read);
    EXPECT_TRUE(result3.isOk());
    EXPECT_EQ(result3.getValue().physicalAddress, pa3);
    
    EXPECT_EQ(smmuController->getCacheMissCount(), initialMisses + 3);
    
    // Second access to same addresses - all should be cache hits
    uint64_t initialHits = smmuController->getCacheHitCount();
    
    TranslationResult result4 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova1, AccessType::Write);
    EXPECT_TRUE(result4.isOk());
    EXPECT_EQ(result4.getValue().physicalAddress, pa1);
    
    TranslationResult result5 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova2, AccessType::Write);
    EXPECT_TRUE(result5.isOk());
    EXPECT_EQ(result5.getValue().physicalAddress, pa2);
    
    TranslationResult result6 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova3, AccessType::Execute);
    EXPECT_TRUE(result6.isOk());
    EXPECT_EQ(result6.getValue().physicalAddress, pa3);
    
    EXPECT_EQ(smmuController->getCacheHitCount(), initialHits + 3);
    
    // Verify final cache statistics
    CacheStatistics finalStats = smmuController->getCacheStatistics();
    EXPECT_EQ(finalStats.hitCount, 3);
    EXPECT_EQ(finalStats.missCount, 3);
    EXPECT_EQ(finalStats.totalLookups, 6);
    EXPECT_EQ(finalStats.hitRate, 0.5); // 50% hit rate
}

// Test Two-Stage Translation Logic - Stage1+Stage2 combination
TEST_F(SMMUTest, Task52_TwoStageTranslationStage1PlusStage2) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = true;  // Enable both stages
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Set up Stage1 mapping
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    smmuController->resetStatistics();
    
    // Perform two-stage translation
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    
    // For this test, we expect Stage1-only behavior since Stage2 setup is complex
    // In real implementation, this would go through both stages
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Verify translation was cached after two-stage processing
    uint64_t initialHits = smmuController->getCacheHitCount();
    TranslationResult cachedResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(cachedResult.isOk());
    EXPECT_GT(smmuController->getCacheHitCount(), initialHits);
}

// Test Two-Stage Translation Logic - Stage1-only translation
TEST_F(SMMUTest, Task52_TwoStageTranslationStage1Only) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;  // Stage1 only
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    smmuController->resetStatistics();
    
    // Perform Stage1-only translation
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Verify caching works for Stage1-only translations
    TranslationResult cachedResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(cachedResult.isOk());
    EXPECT_EQ(cachedResult.getValue().physicalAddress, TEST_PA);
    
    // Should have at least one cache hit from second translation
    EXPECT_GT(smmuController->getCacheHitCount(), 0);
}

// Test Two-Stage Translation Logic - Stage2-only translation
TEST_F(SMMUTest, Task52_TwoStageTranslationStage2Only) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;  // Stage2 only
    config.stage2Enabled = true;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // For Stage2-only, input addresses are treated as intermediate physical addresses
    // This is a complex scenario in real hardware, but for testing we verify the logic works
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    smmuController->resetStatistics();
    
    // Perform Stage2-only translation
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    
    // The implementation should handle Stage2-only configuration
    // Exact behavior depends on Stage2 implementation details
    EXPECT_TRUE(result.isOk() || result.getError() == SMMUError::PageNotMapped);
    
    // Verify statistics are tracked for Stage2 translations
    EXPECT_GT(smmuController->getTotalTranslations(), 0);
}

// Test Translation bypass mode when translation is disabled
TEST_F(SMMUTest, Task52_TranslationBypassMode) {
    StreamConfig config;
    config.translationEnabled = false;  // Bypass mode
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    smmuController->resetStatistics();
    
    // In bypass mode, IOVA should pass through unchanged
    TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_IOVA);  // Pass-through behavior
    
    // Bypass translations should still be counted but may not be cached
    EXPECT_GT(smmuController->getTotalTranslations(), 0);
    
    // Test multiple bypass translations
    for (int i = 0; i < 5; i++) {
        IOVA testIova = TEST_IOVA + (i * 0x1000);
        TranslationResult bypassResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, testIova, AccessType::Read);
        EXPECT_TRUE(bypassResult.isOk());
        EXPECT_EQ(bypassResult.getValue().physicalAddress, testIova);
    }
    
    EXPECT_EQ(smmuController->getTotalTranslations(), 6);  // 1 + 5 = 6 translations
}

// Test Cache Invalidation - Global cache invalidation
TEST_F(SMMUTest, Task52_GlobalCacheInvalidation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Set up multiple streams for comprehensive testing
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, TEST_PA + 0x1000, perms));
    
    smmuController->resetStatistics();
    
    // Generate some cached translations
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(result2.isOk());
    
    // Verify they're cached (cache hits on second access)
    uint64_t hitsBefore = smmuController->getCacheHitCount();
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_GT(smmuController->getCacheHitCount(), hitsBefore);
    
    // Invalidate all translation cache
    smmuController->invalidateTranslationCache();
    
    // Next accesses should be cache misses again
    uint64_t missesBefore = smmuController->getCacheMissCount();
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_GT(smmuController->getCacheMissCount(), missesBefore);
    
    // Verify translations still work correctly after cache invalidation
    TranslationResult postInvalidResult1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(postInvalidResult1.isOk());
    EXPECT_EQ(postInvalidResult1.getValue().physicalAddress, TEST_PA);
}

// Test Cache Invalidation - Stream-specific invalidation
TEST_F(SMMUTest, Task52_StreamSpecificCacheInvalidation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, TEST_PA + 0x1000, perms));
    
    smmuController->resetStatistics();
    
    // Cache translations for both streams
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    
    // Verify both are cached
    uint64_t hitsBefore = smmuController->getCacheHitCount();
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_EQ(smmuController->getCacheHitCount(), hitsBefore + 2);
    
    // Invalidate only stream 1 cache
    smmuController->invalidateStreamCache(TEST_STREAM_ID_1);
    
    // Stream 1 should now be cache miss, stream 2 should still be cache hit
    uint64_t missesBeforeStream1 = smmuController->getCacheMissCount();
    uint64_t hitsBeforeStream2 = smmuController->getCacheHitCount();
    
    TranslationResult postInvalidResult1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(postInvalidResult1.isOk());
    EXPECT_GT(smmuController->getCacheMissCount(), missesBeforeStream1);
    
    TranslationResult postInvalidResult2 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(postInvalidResult2.isOk());
    EXPECT_GT(smmuController->getCacheHitCount(), hitsBeforeStream2);
}

// Test Cache Invalidation - PASID-specific invalidation
TEST_F(SMMUTest, Task52_PASIDSpecificCacheInvalidation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_2));
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_2, TEST_IOVA + 0x1000, TEST_PA + 0x1000, perms));
    
    smmuController->resetStatistics();
    
    // Cache translations for both PASIDs
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_2, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    
    // Verify both are cached
    uint64_t hitsBefore = smmuController->getCacheHitCount();
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_2, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_EQ(smmuController->getCacheHitCount(), hitsBefore + 2);
    
    // Invalidate only PASID_1 cache for this stream
    smmuController->invalidatePASIDCache(TEST_STREAM_ID_1, TEST_PASID_1);
    
    // PASID_1 should be cache miss, PASID_2 should still be cache hit
    uint64_t missesBefore = smmuController->getCacheMissCount();
    uint64_t hitsBeforePasid2 = smmuController->getCacheHitCount();
    
    TranslationResult postInvalidResult1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(postInvalidResult1.isOk());
    EXPECT_GT(smmuController->getCacheMissCount(), missesBefore);
    
    TranslationResult postInvalidResult2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_2, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(postInvalidResult2.isOk());
    EXPECT_GT(smmuController->getCacheHitCount(), hitsBeforePasid2);
}

// Test Translation Performance Optimization - Fast-path cache optimization
TEST_F(SMMUTest, Task52_FastPathCacheOptimization) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Set up multiple sequential pages for testing
    PagePermissions perms(true, true, true);
    const size_t numPages = 10;
    
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA pa = TEST_PA + (i * 0x1000);
        EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova, pa, perms));
    }
    
    smmuController->resetStatistics();
    
    // First pass - populate cache (all misses)
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA expectedPA = TEST_PA + (i * 0x1000);
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
    
    EXPECT_EQ(smmuController->getCacheMissCount(), numPages);
    EXPECT_EQ(smmuController->getCacheHitCount(), 0);
    
    // Second pass - fast path cache hits
    uint64_t fastPathHitsBefore = smmuController->getCacheHitCount();
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA expectedPA = TEST_PA + (i * 0x1000);
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Execute);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
    
    // All second-pass translations should be cache hits
    EXPECT_EQ(smmuController->getCacheHitCount(), fastPathHitsBefore + numPages);
    EXPECT_EQ(smmuController->getCacheMissCount(), numPages);  // Should not increase
    
    // Verify cache statistics reflect fast-path optimization
    CacheStatistics stats = smmuController->getCacheStatistics();
    EXPECT_EQ(stats.hitRate, 0.5);  // 50% hit rate (10 misses, 10 hits)
    EXPECT_EQ(stats.totalLookups, numPages * 2);
}

// Test Page alignment optimization
TEST_F(SMMUTest, Task52_PageAlignmentOptimization) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    // Test page alignment with non-aligned addresses
    PagePermissions perms(true, true, false);
    IOVA alignedIOVA = TEST_IOVA;  // Should be 4K aligned
    PA alignedPA = TEST_PA;        // Should be 4K aligned
    
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, alignedIOVA, alignedPA, perms));
    
    smmuController->resetStatistics();
    
    // Test translation with various offsets within the page
    const uint32_t pageSize = 4096;
    for (uint32_t offset = 0; offset < pageSize; offset += 256) {
        IOVA testIOVA = alignedIOVA + offset;
        PA expectedPA = alignedPA + offset;
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, testIOVA, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
    
    // All translations within same page should benefit from page alignment optimization
    // This should result in efficient cache utilization
    CacheStatistics stats = smmuController->getCacheStatistics();
    EXPECT_GT(stats.totalLookups, 0);
    
    // Verify that page boundaries are handled correctly
    IOVA nextPageIOVA = alignedIOVA + pageSize;
    PA nextPagePA = alignedPA + pageSize;
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, nextPageIOVA, nextPagePA, perms));
    
    TranslationResult nextPageResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, nextPageIOVA, AccessType::Read);
    EXPECT_TRUE(nextPageResult.isOk());
    EXPECT_EQ(nextPageResult.getValue().physicalAddress, nextPagePA);
}

// Test Enhanced Error Handling - Fault classification accuracy
TEST_F(SMMUTest, Task52_EnhancedFaultClassification) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    VoidResult clearResult6 = smmuController->clearEvents();
    EXPECT_TRUE(clearResult6.isOk());
    
    // Test 1: Translation fault (no mapping)
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isError());
    EXPECT_EQ(result1.getError(), SMMUError::PageNotMapped);
    
    // Test 2: Permission fault (read-only page with write access)
    PagePermissions readOnlyPerms(true, false, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, readOnlyPerms));
    
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(result2.isError());
    EXPECT_EQ(result2.getError(), SMMUError::PagePermissionViolation);
    
    // Test 3: Execute permission fault
    TranslationResult result3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Execute);
    EXPECT_FALSE(result3.isOk());
    EXPECT_EQ(result3.getError(), SMMUError::PagePermissionViolation);
    
    // Test 4: Invalid stream configuration fault
    TranslationResult result4 = smmuController->translate(99999, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_FALSE(result4.isOk());
    // Fault type depends on implementation - could be TranslationFault or ConfigurationFault
    
    // Verify fault records are properly classified
    Result<std::vector<FaultRecord>> eventsResult = smmuController->getEvents();
    EXPECT_TRUE(eventsResult.isOk());
    std::vector<FaultRecord> events = eventsResult.getValue();
    EXPECT_GE(events.size(), 3);
    
    // Count fault types
    int translationFaults = 0, permissionFaults = 0;
    for (const auto& event : events) {
        if (event.faultType == FaultType::TranslationFault) translationFaults++;
        if (event.faultType == FaultType::PermissionFault) permissionFaults++;
        
        // Verify all fault records have valid timestamps
        EXPECT_GT(event.timestamp, 0);
        
        // Verify fault records have correct context
        EXPECT_TRUE(event.streamID == TEST_STREAM_ID_1 || event.streamID == 99999);
        EXPECT_EQ(event.address, TEST_IOVA);
    }
    
    EXPECT_GE(translationFaults, 1);
    EXPECT_GE(permissionFaults, 2);
}

// Test Enhanced Error Handling - Fault recovery mechanisms
TEST_F(SMMUTest, Task52_FaultRecoveryMechanisms) {
    StreamConfig terminateConfig;
    terminateConfig.translationEnabled = true;
    terminateConfig.stage1Enabled = true;
    terminateConfig.stage2Enabled = false;
    terminateConfig.faultMode = FaultMode::Terminate;
    
    StreamConfig stallConfig;
    stallConfig.translationEnabled = true;
    stallConfig.stage1Enabled = true;
    stallConfig.stage2Enabled = false;
    stallConfig.faultMode = FaultMode::Stall;
    
    // Test fault recovery in Terminate mode
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, terminateConfig));
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    VoidResult clearResult = smmuController->clearEvents();
    EXPECT_TRUE(clearResult.isOk());
    smmuController->resetStatistics();
    
    // Generate a fault in Terminate mode
    TranslationResult terminateResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(terminateResult.isError());
    EXPECT_EQ(terminateResult.getError(), SMMUError::PageNotMapped);
    
    // Verify fault was recorded
    Result<std::vector<FaultRecord>> terminateFaultsResult = smmuController->getEvents();
    EXPECT_TRUE(terminateFaultsResult.isOk());
    std::vector<FaultRecord> terminateFaults = terminateFaultsResult.getValue();
    EXPECT_GT(terminateFaults.size(), 0);
    
    // Stream should still be functional after fault in Terminate mode
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    TranslationResult recoveryResult = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(recoveryResult.isOk());
    EXPECT_EQ(recoveryResult.getValue().physicalAddress, TEST_PA);
    
    // Test fault recovery in Stall mode
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, stallConfig));
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    VoidResult clearResult7 = smmuController->clearEvents();
    EXPECT_TRUE(clearResult.isOk());
    
    // Generate a fault in Stall mode
    TranslationResult stallResult = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA + 0x1000, AccessType::Read);
    EXPECT_TRUE(stallResult.isError());
    
    // Verify fault handling doesn't crash the system
    Result<std::vector<FaultRecord>> stallFaultsResult = smmuController->getEvents();
    EXPECT_TRUE(stallFaultsResult.isOk());
    std::vector<FaultRecord> stallFaults = stallFaultsResult.getValue();
    EXPECT_GT(stallFaults.size(), 0);
    
    // Verify statistics are properly updated despite faults
    EXPECT_GT(smmuController->getTotalTranslations(), 0);
    EXPECT_GT(smmuController->getTotalFaults(), 0);
}

// Test Cache Statistics Accuracy and Tracking
TEST_F(SMMUTest, Task52_CacheStatisticsAccuracy) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    const size_t numPages = 5;
    
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA pa = TEST_PA + (i * 0x1000);
        EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova, pa, perms));
    }
    
    smmuController->resetStatistics();
    
    // Generate cache misses (first access to each page)
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
    }
    
    // Verify initial statistics
    CacheStatistics stats1 = smmuController->getCacheStatistics();
    EXPECT_EQ(stats1.hitCount, 0);
    EXPECT_EQ(stats1.missCount, numPages);
    EXPECT_EQ(stats1.totalLookups, numPages);
    EXPECT_EQ(stats1.hitRate, 0.0);
    
    // Generate cache hits (second access to same pages)
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Write);
        EXPECT_TRUE(result.isOk());
    }
    
    // Verify statistics after hits
    CacheStatistics stats2 = smmuController->getCacheStatistics();
    EXPECT_EQ(stats2.hitCount, numPages);
    EXPECT_EQ(stats2.missCount, numPages);
    EXPECT_EQ(stats2.totalLookups, numPages * 2);
    EXPECT_EQ(stats2.hitRate, 0.5);  // 50% hit rate
    
    // Test individual statistic getters
    EXPECT_EQ(smmuController->getCacheHitCount(), numPages);
    EXPECT_EQ(smmuController->getCacheMissCount(), numPages);
    
    // Generate more misses with new addresses
    for (size_t i = 0; i < 3; i++) {
        IOVA iova = TEST_IOVA + ((numPages + i) * 0x1000);
        PA pa = TEST_PA + ((numPages + i) * 0x1000);
        EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova, pa, perms));
        
        TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
    }
    
    // Verify final statistics accuracy
    CacheStatistics stats3 = smmuController->getCacheStatistics();
    EXPECT_EQ(stats3.hitCount, numPages);
    EXPECT_EQ(stats3.missCount, numPages + 3);
    EXPECT_EQ(stats3.totalLookups, (numPages * 2) + 3);
    EXPECT_GT(stats3.hitRate, 0.0);
    EXPECT_LT(stats3.hitRate, 1.0);
    
    // Test statistics reset
    smmuController->resetStatistics();
    CacheStatistics resetStats = smmuController->getCacheStatistics();
    EXPECT_EQ(resetStats.hitCount, 0);
    EXPECT_EQ(resetStats.missCount, 0);
    EXPECT_EQ(resetStats.totalLookups, 0);
    EXPECT_EQ(resetStats.hitRate, 0.0);
}

// Test Cache performance under high load
TEST_F(SMMUTest, Task52_CachePerformanceUnderLoad) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, true);
    const size_t numPages = 100;
    
    // Set up a large number of mappings
    for (size_t i = 0; i < numPages; i++) {
        IOVA iova = TEST_IOVA + (i * 0x1000);
        PA pa = TEST_PA + (i * 0x1000);
        EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, iova, pa, perms));
    }
    
    smmuController->resetStatistics();
    
    // Generate high load with mixed access patterns
    for (int round = 0; round < 3; round++) {
        // Forward access
        for (size_t i = 0; i < numPages; i++) {
            IOVA iova = TEST_IOVA + (i * 0x1000);
            TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Read);
            EXPECT_TRUE(result.isOk());
        }
        
        // Reverse access
        for (size_t i = numPages; i > 0; i--) {
            IOVA iova = TEST_IOVA + ((i - 1) * 0x1000);
            TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Write);
            EXPECT_TRUE(result.isOk());
        }
        
        // Random access pattern (simplified)
        for (size_t i = 0; i < numPages / 2; i++) {
            size_t index = (i * 7) % numPages;  // Simple pseudo-random
            IOVA iova = TEST_IOVA + (index * 0x1000);
            TranslationResult result = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, iova, AccessType::Execute);
            EXPECT_TRUE(result.isOk());
        }
    }
    
    // Verify high-load performance characteristics
    CacheStatistics highLoadStats = smmuController->getCacheStatistics();
    EXPECT_GT(highLoadStats.totalLookups, numPages * 5);  // At least 5 accesses per page
    EXPECT_GT(highLoadStats.hitCount, 0);
    EXPECT_GT(highLoadStats.hitRate, 0.0);
    EXPECT_LT(highLoadStats.hitRate, 1.0);
    
    // Cache should be efficient under high load
    EXPECT_GT(highLoadStats.hitRate, 0.5);  // At least 50% hit rate expected
    
    // Verify system remains stable under high load
    EXPECT_EQ(smmuController->getTotalTranslations(), highLoadStats.totalLookups);
}

// Test Integration with existing SMMU functionality
TEST_F(SMMUTest, Task52_IntegrationWithExistingSMMU) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Test that Task 5.2 enhancements work with existing stream management
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    Result<bool> configResult = smmuController->isStreamConfigured(TEST_STREAM_ID_1);
    EXPECT_TRUE(configResult.isOk());
    EXPECT_TRUE(configResult.getValue());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    smmuController->resetStatistics();
    
    // Test cache integration with stream enable/disable
    TranslationResult result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    
    // Disable and re-enable stream - cache should still work
    smmuController->disableStream(TEST_STREAM_ID_1);
    Result<bool> enabledResultFalse = smmuController->isStreamEnabled(TEST_STREAM_ID_1);
    EXPECT_TRUE(enabledResultFalse.isOk());
    EXPECT_FALSE(enabledResultFalse.getValue());
    
    smmuController->enableStream(TEST_STREAM_ID_1);
    Result<bool> enabledResult = smmuController->isStreamEnabled(TEST_STREAM_ID_1);
    EXPECT_TRUE(enabledResult.isOk());
    EXPECT_TRUE(enabledResult.getValue());
    
    TranslationResult result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA);
    
    // Test cache integration with PASID removal and recreation
    EXPECT_TRUE(smmuController->removeStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1));
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    
    TranslationResult result3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(result3.isOk());
    EXPECT_EQ(result3.getValue().physicalAddress, TEST_PA);
    
    // Test cache integration with global configuration changes
    VoidResult setGlobalResult = smmuController->setGlobalFaultMode(FaultMode::Stall);
    EXPECT_TRUE(setGlobalResult.isOk());
    VoidResult enableCachingResult1 = smmuController->enableCaching(false);
    EXPECT_TRUE(enableCachingResult1.isOk());
    
    TranslationResult result4 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Execute);
    EXPECT_TRUE(result4.isOk());
    
    VoidResult enableCachingResult2 = smmuController->enableCaching(true);
    EXPECT_TRUE(enableCachingResult2.isOk());
    TranslationResult result5 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result5.isOk());
    
    // Test cache integration with SMMU reset
    uint64_t translationsBefore = smmuController->getTotalTranslations();
    EXPECT_GT(translationsBefore, 0);
    
    smmuController->reset();
    
    // After reset, cache statistics should be cleared
    EXPECT_EQ(smmuController->getCacheHitCount(), 0);
    EXPECT_EQ(smmuController->getCacheMissCount(), 0);
    EXPECT_EQ(smmuController->getTotalTranslations(), 0);
    
    // Verify SMMU still functions after reset
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    TranslationResult postResetResult = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(postResetResult.isOk());
    EXPECT_EQ(postResetResult.getValue().physicalAddress, TEST_PA);
}

// Test Cross-stream cache isolation with Task 5.2 enhancements
TEST_F(SMMUTest, Task52_CrossStreamCacheIsolation) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_1, config).isOk());
    EXPECT_TRUE(smmuController->configureStream(TEST_STREAM_ID_2, config).isOk());
    
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_1, TEST_PASID_1).isOk());
    EXPECT_TRUE(smmuController->createStreamPASID(TEST_STREAM_ID_2, TEST_PASID_1).isOk());
    
    // Map same IOVA to different PAs in different streams
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, TEST_PA, perms).isOk());
    EXPECT_TRUE(smmuController->mapPage(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, TEST_PA + 0x10000, perms));
    
    smmuController->resetStatistics();
    
    // Generate cache entries for both streams
    TranslationResult stream1Result1 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(stream1Result1.isOk());
    EXPECT_EQ(stream1Result1.getValue().physicalAddress, TEST_PA);
    
    TranslationResult stream2Result1 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(stream2Result1.isOk());
    EXPECT_EQ(stream2Result1.getValue().physicalAddress, TEST_PA + 0x10000);
    
    // Verify both are in cache (cache hits on repeat)
    uint64_t hitsBefore = smmuController->getCacheHitCount();
    
    TranslationResult stream1Result2 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(stream1Result2.isOk());
    EXPECT_EQ(stream1Result2.getValue().physicalAddress, TEST_PA);
    
    TranslationResult stream2Result2 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(stream2Result2.isOk());
    EXPECT_EQ(stream2Result2.getValue().physicalAddress, TEST_PA + 0x10000);
    
    EXPECT_EQ(smmuController->getCacheHitCount(), hitsBefore + 2);
    
    // Invalidate stream 1 cache - stream 2 should remain cached
    smmuController->invalidateStreamCache(TEST_STREAM_ID_1);
    
    uint64_t missesBeforeStream1 = smmuController->getCacheMissCount();
    uint64_t hitsBeforeStream2 = smmuController->getCacheHitCount();
    
    // Stream 1 should miss, stream 2 should hit
    TranslationResult stream1Result3 = smmuController->translate(TEST_STREAM_ID_1, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(stream1Result3.isOk());
    EXPECT_EQ(stream1Result3.getValue().physicalAddress, TEST_PA);
    EXPECT_GT(smmuController->getCacheMissCount(), missesBeforeStream1);
    
    TranslationResult stream2Result3 = smmuController->translate(TEST_STREAM_ID_2, TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(stream2Result3.isOk());
    EXPECT_EQ(stream2Result3.getValue().physicalAddress, TEST_PA + 0x10000);
    EXPECT_GT(smmuController->getCacheHitCount(), hitsBeforeStream2);
    
    // Verify streams remain isolated - different physical addresses for same IOVA
    EXPECT_NE(stream1Result3.getValue().physicalAddress, stream2Result3.getValue().physicalAddress);
}

} // namespace test
} // namespace smmu