// ARM SMMU v3 Minimal Integration Test - API Compatible Version
// Copyright (c) 2024 John Greninger
// Task 8.2: Integration Testing Validation

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace integration {

class MinimalIntegrationTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Use default configuration
        smmu = std::make_unique<SMMU>();
        
        // Test parameters
        testStreamID = 100;
        testPASID = 1;
        page_size = 4096;
        base_iova = 0x100000;
        base_pa = 0x200000;
    }

    void TearDown() override {
        smmu.reset();
    }

    std::unique_ptr<SMMU> smmu;
    StreamID testStreamID;
    PASID testPASID;
    size_t page_size;
    IOVA base_iova;
    PA base_pa;
};

// Test 1: Basic Stream Configuration
TEST_F(MinimalIntegrationTest, BasicStreamConfiguration) {
    // Configure a simple stream
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    auto result = smmu->configureStream(testStreamID, config);
    EXPECT_TRUE(result.isOk()) << "Stream configuration should succeed";
    
    // Enable the stream
    result = smmu->enableStream(testStreamID);
    EXPECT_TRUE(result.isOk()) << "Stream enable should succeed";
    
    // Verify stream status
    auto status_result = smmu->isStreamEnabled(testStreamID);
    EXPECT_TRUE(status_result.isOk()) << "Stream status check should succeed";
    EXPECT_TRUE(status_result.getValue()) << "Stream should be enabled";
}

// Test 2: PASID Creation and Basic Translation
TEST_F(MinimalIntegrationTest, BasicPASIDAndTranslation) {
    // Configure stream first
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    auto result = smmu->configureStream(testStreamID, config);
    ASSERT_TRUE(result.isOk());
    
    result = smmu->enableStream(testStreamID);
    ASSERT_TRUE(result.isOk());
    
    // Create PASID
    result = smmu->createStreamPASID(testStreamID, testPASID);
    EXPECT_TRUE(result.isOk()) << "PASID creation should succeed";
    
    // Map a page
    PagePermissions perms;
    perms.read = true;
    perms.write = true;
    perms.execute = false;
    
    IOVA test_iova = base_iova;
    PA test_pa = base_pa;
    
    result = smmu->mapPage(testStreamID, testPASID, test_iova, test_pa, perms);
    EXPECT_TRUE(result.isOk()) << "Page mapping should succeed";
    
    // Perform translation
    auto trans_result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read);
    EXPECT_TRUE(trans_result.isOk()) << "Translation should succeed";
    
    if (trans_result.isOk()) {
        EXPECT_EQ(trans_result.getValue().physicalAddress, test_pa) << "Translation should return correct PA";
    }
}

// Test 3: Stream Isolation Verification
TEST_F(MinimalIntegrationTest, BasicStreamIsolation) {
    const StreamID stream1 = 100;
    const StreamID stream2 = 200;
    
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    // Configure both streams
    auto result = smmu->configureStream(stream1, config);
    ASSERT_TRUE(result.isOk());
    result = smmu->enableStream(stream1);
    ASSERT_TRUE(result.isOk());
    
    result = smmu->configureStream(stream2, config);
    ASSERT_TRUE(result.isOk());
    result = smmu->enableStream(stream2);
    ASSERT_TRUE(result.isOk());
    
    // Create PASIDs for both streams
    result = smmu->createStreamPASID(stream1, testPASID);
    ASSERT_TRUE(result.isOk());
    result = smmu->createStreamPASID(stream2, testPASID);
    ASSERT_TRUE(result.isOk());
    
    // Map same IOVA to different PAs for each stream
    IOVA shared_iova = base_iova;
    PA pa1 = base_pa;
    PA pa2 = base_pa + page_size;
    
    PagePermissions perms;
    perms.read = true;
    perms.write = true;
    perms.execute = false;
    
    result = smmu->mapPage(stream1, testPASID, shared_iova, pa1, perms);
    ASSERT_TRUE(result.isOk());
    result = smmu->mapPage(stream2, testPASID, shared_iova, pa2, perms);
    ASSERT_TRUE(result.isOk());
    
    // Verify different streams get different physical addresses
    auto trans1 = smmu->translate(stream1, testPASID, shared_iova, AccessType::Read);
    auto trans2 = smmu->translate(stream2, testPASID, shared_iova, AccessType::Read);
    
    ASSERT_TRUE(trans1.isOk());
    ASSERT_TRUE(trans2.isOk());
    
    EXPECT_EQ(trans1.getValue().physicalAddress, pa1);
    EXPECT_EQ(trans2.getValue().physicalAddress, pa2);
    EXPECT_NE(trans1.getValue().physicalAddress, trans2.getValue().physicalAddress) 
        << "Stream isolation should ensure different PAs";
}

// Test 4: Fault Handling Integration
TEST_F(MinimalIntegrationTest, BasicFaultHandling) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    auto result = smmu->configureStream(testStreamID, config);
    ASSERT_TRUE(result.isOk());
    result = smmu->enableStream(testStreamID);
    ASSERT_TRUE(result.isOk());
    result = smmu->createStreamPASID(testStreamID, testPASID);
    ASSERT_TRUE(result.isOk());
    
    // Clear any existing events
    smmu->clearEvents();
    
    // Try to translate unmapped address (should fault)
    IOVA unmapped_iova = base_iova + 0x1000;
    auto trans_result = smmu->translate(testStreamID, testPASID, unmapped_iova, AccessType::Read);
    
    EXPECT_FALSE(trans_result.isOk()) << "Translation of unmapped address should fail";
    
    // Check that fault was recorded
    auto events = smmu->getEvents();
    EXPECT_TRUE(events.isOk()) << "Should be able to retrieve events";
    
    if (events.isOk()) {
        EXPECT_GT(events.getValue().size(), 0) << "Should have fault events recorded";
        
        if (events.getValue().size() > 0) {
            const auto& fault = events.getValue()[0];
            EXPECT_EQ(fault.streamID, testStreamID);
            EXPECT_EQ(fault.pasid, testPASID);
            EXPECT_EQ(fault.address, unmapped_iova);
        }
    }
}

// Test 5: Cache Statistics Integration
TEST_F(MinimalIntegrationTest, BasicCacheStatistics) {
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    
    auto result = smmu->configureStream(testStreamID, config);
    ASSERT_TRUE(result.isOk());
    result = smmu->enableStream(testStreamID);
    ASSERT_TRUE(result.isOk());
    result = smmu->createStreamPASID(testStreamID, testPASID);
    ASSERT_TRUE(result.isOk());
    
    // Map a page
    PagePermissions perms;
    perms.read = true;
    perms.write = true;
    perms.execute = false;
    
    IOVA test_iova = base_iova;
    PA test_pa = base_pa;
    
    result = smmu->mapPage(testStreamID, testPASID, test_iova, test_pa, perms);
    ASSERT_TRUE(result.isOk());
    
    // Reset statistics for clean measurement
    smmu->resetStatistics();
    
    // Perform translation
    auto trans_result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read);
    ASSERT_TRUE(trans_result.isOk());
    
    // Check cache statistics
    auto stats = smmu->getCacheStatistics();
    EXPECT_GT(stats.totalLookups, 0) << "Should have performed cache lookups";
    
    // Perform same translation again
    trans_result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read);
    ASSERT_TRUE(trans_result.isOk());
    
    auto stats2 = smmu->getCacheStatistics();
    EXPECT_GT(stats2.totalLookups, stats.totalLookups) << "Should have additional lookups";
}

} // namespace integration
} // namespace smmu