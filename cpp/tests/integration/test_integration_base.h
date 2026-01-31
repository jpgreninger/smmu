// ARM SMMU v3 Integration Test Base Helper
// Copyright (c) 2024 John Greninger
// Common utilities for integration tests

#ifndef SMMU_TEST_INTEGRATION_BASE_H
#define SMMU_TEST_INTEGRATION_BASE_H

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <thread>
#include <chrono>
#include <functional>

namespace smmu {
namespace integration {

// Helper class for common integration test functionality
class IntegrationTestBase : public ::testing::Test {
protected:
    void SetUp() override {
        // Create SMMU with high-performance configuration for integration testing
        config = SMMUConfiguration::createHighPerformance();
        
        // Update configuration for integration testing needs
        QueueConfiguration queueConfig;
        queueConfig.eventQueueSize = 2048;
        queueConfig.commandQueueSize = 1024;
        queueConfig.priQueueSize = 512;
        config.setQueueConfiguration(queueConfig);
        
        CacheConfiguration cacheConfig;
        cacheConfig.tlbCacheSize = 4096;
        cacheConfig.cacheMaxAge = 10000;
        cacheConfig.enableCaching = true;
        config.setCacheConfiguration(cacheConfig);
        
        AddressConfiguration addressConfig;
        addressConfig.maxIOVASize = (1ULL << 48);  // 48-bit IOVA
        addressConfig.maxPASize = (1ULL << 52);    // 52-bit PA
        addressConfig.maxStreamCount = 4096;
        addressConfig.maxPASIDCount = 2048;
        config.setAddressConfiguration(addressConfig);
        
        smmu = std::make_unique<SMMU>(config);
        
        // Common test parameters
        page_size = 4096;
        base_iova = 0x100000000ULL;  // 4GB base
        base_pa = 0x200000000ULL;    // 8GB base
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper to configure a basic stream
    void configureBasicStream(StreamID streamID, SecurityState securityState = SecurityState::NonSecure,
                             TranslationStage stage = TranslationStage::Stage1Only) {
        StreamConfig streamConfig;
        streamConfig.translationStage = stage;
        streamConfig.faultMode = FaultMode::Terminate;
        streamConfig.securityState = securityState;
        streamConfig.stage1Enabled = (stage == TranslationStage::Stage1Only || stage == TranslationStage::BothStages);
        streamConfig.stage2Enabled = (stage == TranslationStage::Stage2Only || stage == TranslationStage::BothStages);
        
        // Configure translation table bases
        streamConfig.stage1TTBRs[0] = base_pa + (static_cast<uint64_t>(streamID) * 0x1000000ULL);
        streamConfig.stage1TCR.granuleSize = 4096;
        streamConfig.stage1TCR.addressSpaceBits = 48;
        streamConfig.stage1TCR.walkCacheDisable = false;
        
        if (stage == TranslationStage::BothStages || stage == TranslationStage::Stage2Only) {
            streamConfig.stage2TTBR = base_pa + (static_cast<uint64_t>(streamID) * 0x1000000ULL) + 0x800000;
            streamConfig.stage2TCR.granuleSize = 4096;
            streamConfig.stage2TCR.addressSpaceBits = 48;
            streamConfig.stage2TCR.walkCacheDisable = false;
        }
        
        auto result = smmu->configureStream(streamID, streamConfig);
        ASSERT_TRUE(result.isSuccess()) << "Failed to configure stream " << streamID 
                                       << ": " << static_cast<int>(result.getError());
        
        result = smmu->enableStream(streamID);
        ASSERT_TRUE(result.isSuccess()) << "Failed to enable stream " << streamID;
    }

    // Helper to create PASID for a stream
    void createPASID(StreamID streamID, PASID pasid) {
        auto result = smmu->createStreamPASID(streamID, pasid);
        ASSERT_TRUE(result.isSuccess()) << "Failed to create PASID " << pasid 
                                       << " for stream " << streamID;
    }

    // Helper to map a page with default permissions
    void mapPage(StreamID streamID, PASID pasid, IOVA iova, PA pa, 
                SecurityState securityState = SecurityState::NonSecure) {
        PagePermissions perms;
        perms.read = true;
        perms.write = true;
        perms.execute = false;
        perms.user = true;
        perms.global = false;
        
        auto result = smmu->mapPage(streamID, pasid, iova, pa, perms, securityState);
        ASSERT_TRUE(result.isSuccess()) << "Failed to map page for stream " << streamID 
                                       << ", PASID " << pasid;
    }

    // Helper to verify translation result
    void verifyTranslation(StreamID streamID, PASID pasid, IOVA iova, PA expected_pa,
                          AccessType accessType = AccessType::Read,
                          SecurityState securityState = SecurityState::NonSecure) {
        auto result = smmu->translate(streamID, pasid, iova, accessType, securityState);
        ASSERT_TRUE(result.isSuccess()) << "Translation failed for stream " << streamID 
                                       << ", PASID " << pasid << ", IOVA 0x" << std::hex << iova;
        EXPECT_EQ(result.getValue().physicalAddress, expected_pa) 
            << "PA mismatch for stream " << streamID << ", PASID " << pasid;
    }

    // Performance measurement helper
    template<typename Func>
    double measureExecutionTime(Func&& func, size_t iterations = 1) {
        auto start = std::chrono::high_resolution_clock::now();
        
        for (size_t i = 0; i < iterations; ++i) {
            func();
        }
        
        auto end = std::chrono::high_resolution_clock::now();
        auto duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
        
        return static_cast<double>(duration.count()) / iterations;
    }

    // Helper to get thread-safe random number generator
    uint32_t getRandomNumber() {
        thread_local std::mt19937 rng(std::hash<std::thread::id>{}(std::this_thread::get_id()));
        return rng();
    }

protected:
    std::unique_ptr<SMMU> smmu;
    SMMUConfiguration config;
    size_t page_size;
    IOVA base_iova;
    PA base_pa;
};

} // namespace integration
} // namespace smmu

#endif // SMMU_TEST_INTEGRATION_BASE_H