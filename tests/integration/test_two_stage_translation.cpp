// ARM SMMU v3 Two-Stage Translation Integration Tests
// Copyright (c) 2024 John Greninger
// Task 8.2.1: Two-Stage Translation Integration Tests (6 hours)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <vector>
#include <thread>
#include <chrono>

namespace smmu {
namespace integration {

class TwoStageTranslationTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Create SMMU with enhanced configuration for two-stage translation
        SMMUConfiguration config;
        config.maxStreams = 1024;
        config.maxPASIDsPerStream = 256;
        config.cacheConfig.maxEntries = 1024;
        config.cacheConfig.replacementPolicy = CacheReplacementPolicy::LRU;
        config.queueConfig.maxEventQueueSize = 512;
        config.queueConfig.maxCommandQueueSize = 256;
        config.addressConfig.addressSpaceBits = 48;
        config.addressConfig.granuleSize = 4096;
        
        smmu = std::make_unique<SMMU>(config);
        
        // Common test parameters
        testStreamID = 100;
        testPASID = 1;
        testSecurityState = SecurityState::NonSecure;
        
        // Setup test address space parameters
        stage1_iova_base = 0x1000000;
        stage1_ipa_base = 0x2000000;
        stage2_pa_base = 0x3000000;
        page_size = 4096;
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper to configure a stream for two-stage translation
    void setupTwoStageStream() {
        StreamConfig streamConfig;
        streamConfig.translationStage = TranslationStage::BothStages;
        streamConfig.faultMode = FaultMode::Terminate;
        streamConfig.securityState = testSecurityState;
        streamConfig.stage1Enabled = true;
        streamConfig.stage2Enabled = true;
        
        // Configure translation table bases and settings
        streamConfig.stage1TTBRs[0] = stage1_ipa_base;
        streamConfig.stage1TCR.granuleSize = 4096;
        streamConfig.stage1TCR.addressSpaceBits = 48;
        streamConfig.stage1TCR.walkCacheDisable = false;
        
        streamConfig.stage2TTBR = stage2_pa_base;
        streamConfig.stage2TCR.granuleSize = 4096;
        streamConfig.stage2TCR.addressSpaceBits = 48;
        streamConfig.stage2TCR.walkCacheDisable = false;
        
        auto result = smmu->configureStream(testStreamID, streamConfig);
        ASSERT_TRUE(result.isSuccess()) << "Failed to configure stream: " << static_cast<int>(result.getError());
        
        // Enable the stream
        result = smmu->enableStream(testStreamID);
        ASSERT_TRUE(result.isSuccess()) << "Failed to enable stream: " << static_cast<int>(result.getError());
        
        // Create PASID
        result = smmu->createStreamPASID(testStreamID, testPASID);
        ASSERT_TRUE(result.isSuccess()) << "Failed to create PASID: " << static_cast<int>(result.getError());
    }

    // Helper to map pages for two-stage translation
    void setupTwoStageMapping(IOVA iova, PA final_pa) {
        // Map Stage-1: IOVA -> IPA
        IPA ipa = stage1_ipa_base + (iova - stage1_iova_base);
        PagePermissions stage1_perms;
        stage1_perms.read = true;
        stage1_perms.write = true;
        stage1_perms.execute = false;
        stage1_perms.user = true;
        stage1_perms.global = false;
        
        auto result = smmu->mapPage(testStreamID, testPASID, iova, ipa, stage1_perms, testSecurityState);
        ASSERT_TRUE(result.isSuccess()) << "Failed to map Stage-1 page: " << static_cast<int>(result.getError());
        
        // Map Stage-2: IPA -> PA  
        // Note: For Stage-2, we need to map at PASID 0 (hypervisor context)
        PagePermissions stage2_perms;
        stage2_perms.read = true;
        stage2_perms.write = true;
        stage2_perms.execute = false;
        stage2_perms.user = false;  // Stage-2 is hypervisor managed
        stage2_perms.global = true;
        
        result = smmu->mapPage(testStreamID, 0, ipa, final_pa, stage2_perms, testSecurityState);
        ASSERT_TRUE(result.isSuccess()) << "Failed to map Stage-2 page: " << static_cast<int>(result.getError());
    }

    std::unique_ptr<SMMU> smmu;
    StreamID testStreamID;
    PASID testPASID;
    SecurityState testSecurityState;
    
    IOVA stage1_iova_base;
    IPA stage1_ipa_base;
    PA stage2_pa_base;
    size_t page_size;
};

// Test 1: Basic Two-Stage Translation Success Path
TEST_F(TwoStageTranslationTest, BasicTwoStageTranslationSuccess) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x1000;
    PA expected_pa = stage2_pa_base + 0x1000;
    
    setupTwoStageMapping(test_iova, expected_pa);
    
    // Perform translation
    auto result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, testSecurityState);
    
    ASSERT_TRUE(result.isSuccess()) << "Two-stage translation failed: " << static_cast<int>(result.getError());
    EXPECT_EQ(result.getValue().physicalAddress, expected_pa);
    EXPECT_EQ(result.getValue().translationStage, TranslationStage::BothStages);
    EXPECT_TRUE(result.getValue().permissions.read);
    EXPECT_EQ(result.getValue().securityState, testSecurityState);
}

// Test 2: Two-Stage Translation with Multiple Pages
TEST_F(TwoStageTranslationTest, MultiplePagesTranslation) {
    setupTwoStageStream();
    
    const size_t num_pages = 10;
    std::vector<IOVA> test_iovas;
    std::vector<PA> expected_pas;
    
    // Setup multiple page mappings
    for (size_t i = 0; i < num_pages; ++i) {
        IOVA iova = stage1_iova_base + (i * page_size);
        PA pa = stage2_pa_base + (i * page_size);
        
        test_iovas.push_back(iova);
        expected_pas.push_back(pa);
        
        setupTwoStageMapping(iova, pa);
    }
    
    // Test all translations
    for (size_t i = 0; i < num_pages; ++i) {
        auto result = smmu->translate(testStreamID, testPASID, test_iovas[i], AccessType::Read, testSecurityState);
        
        ASSERT_TRUE(result.isSuccess()) << "Translation failed for page " << i << ": " << static_cast<int>(result.getError());
        EXPECT_EQ(result.getValue().physicalAddress, expected_pas[i]) << "PA mismatch for page " << i;
        EXPECT_EQ(result.getValue().translationStage, TranslationStage::BothStages);
    }
}

// Test 3: Stage-1 Translation Fault Detection
TEST_F(TwoStageTranslationTest, Stage1TranslationFault) {
    setupTwoStageStream();
    
    IOVA unmapped_iova = stage1_iova_base + 0x5000;  // Not mapped in Stage-1
    
    // Attempt translation of unmapped IOVA
    auto result = smmu->translate(testStreamID, testPASID, unmapped_iova, AccessType::Read, testSecurityState);
    
    EXPECT_FALSE(result.isSuccess());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
    
    // Check that a fault was recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isSuccess());
    EXPECT_GT(events.getValue().size(), 0);
    
    auto& fault = events.getValue()[0];
    EXPECT_EQ(fault.streamID, testStreamID);
    EXPECT_EQ(fault.pasid, testPASID);
    EXPECT_EQ(fault.address, unmapped_iova);
    EXPECT_EQ(fault.faultType, FaultType::Level1Translation);
}

// Test 4: Stage-2 Translation Fault Detection
TEST_F(TwoStageTranslationTest, Stage2TranslationFault) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x2000;
    IPA intermediate_ipa = stage1_ipa_base + 0x2000;
    
    // Map only Stage-1 (IOVA -> IPA), but not Stage-2 (IPA -> PA)
    PagePermissions stage1_perms;
    stage1_perms.read = true;
    stage1_perms.write = true;
    stage1_perms.execute = false;
    stage1_perms.user = true;
    stage1_perms.global = false;
    
    auto map_result = smmu->mapPage(testStreamID, testPASID, test_iova, intermediate_ipa, stage1_perms, testSecurityState);
    ASSERT_TRUE(map_result.isSuccess());
    
    // Attempt translation - should fail at Stage-2
    auto trans_result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, testSecurityState);
    
    EXPECT_FALSE(trans_result.isSuccess());
    EXPECT_EQ(trans_result.getError(), SMMUError::PageNotMapped);
    
    // Check fault details
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isSuccess());
    EXPECT_GT(events.getValue().size(), 0);
    
    auto& fault = events.getValue()[0];
    EXPECT_EQ(fault.streamID, testStreamID);
    EXPECT_EQ(fault.pasid, 0);  // Stage-2 faults use PASID 0
    EXPECT_EQ(fault.address, intermediate_ipa);  // Fault address is the IPA
    EXPECT_EQ(fault.faultType, FaultType::Level1Translation);  // Stage-2 level-1 fault
}

// Test 5: Permission Intersection Between Stages
TEST_F(TwoStageTranslationTest, PermissionIntersection) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x3000;
    PA final_pa = stage2_pa_base + 0x3000;
    IPA intermediate_ipa = stage1_ipa_base + 0x3000;
    
    // Stage-1: Read + Write permissions
    PagePermissions stage1_perms;
    stage1_perms.read = true;
    stage1_perms.write = true;
    stage1_perms.execute = false;
    stage1_perms.user = true;
    stage1_perms.global = false;
    
    auto result = smmu->mapPage(testStreamID, testPASID, test_iova, intermediate_ipa, stage1_perms, testSecurityState);
    ASSERT_TRUE(result.isSuccess());
    
    // Stage-2: Read-only permissions (more restrictive)
    PagePermissions stage2_perms;
    stage2_perms.read = true;
    stage2_perms.write = false;  // No write permission at Stage-2
    stage2_perms.execute = false;
    stage2_perms.user = false;
    stage2_perms.global = true;
    
    result = smmu->mapPage(testStreamID, 0, intermediate_ipa, final_pa, stage2_perms, testSecurityState);
    ASSERT_TRUE(result.isSuccess());
    
    // Test read access - should succeed
    auto trans_result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, testSecurityState);
    EXPECT_TRUE(trans_result.isSuccess());
    EXPECT_EQ(trans_result.getValue().physicalAddress, final_pa);
    EXPECT_TRUE(trans_result.getValue().permissions.read);
    EXPECT_FALSE(trans_result.getValue().permissions.write);  // Intersection result
    
    // Test write access - should fail due to Stage-2 restrictions
    trans_result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Write, testSecurityState);
    EXPECT_FALSE(trans_result.isSuccess());
    EXPECT_EQ(trans_result.getError(), SMMUError::PagePermissionViolation);
}

// Test 6: Security State Validation Across Stages
TEST_F(TwoStageTranslationTest, SecurityStateValidation) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x4000;
    PA final_pa = stage2_pa_base + 0x4000;
    
    // Setup mapping with NonSecure state
    setupTwoStageMapping(test_iova, final_pa);
    
    // Test translation with matching security state
    auto result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isSuccess());
    EXPECT_EQ(result.getValue().securityState, SecurityState::NonSecure);
    
    // Test translation with mismatched security state
    result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, SecurityState::Secure);
    EXPECT_FALSE(result.isSuccess());
    EXPECT_EQ(result.getError(), SMMUError::InvalidSecurityState);
}

// Test 7: Concurrent Two-Stage Translations
TEST_F(TwoStageTranslationTest, ConcurrentTwoStageTranslations) {
    setupTwoStageStream();
    
    const size_t num_threads = 4;
    const size_t translations_per_thread = 100;
    
    // Setup mappings for concurrent access
    for (size_t i = 0; i < num_threads * translations_per_thread; ++i) {
        IOVA iova = stage1_iova_base + (i * page_size);
        PA pa = stage2_pa_base + (i * page_size);
        setupTwoStageMapping(iova, pa);
    }
    
    std::vector<std::thread> threads;
    std::atomic<size_t> successful_translations(0);
    std::atomic<size_t> failed_translations(0);
    
    auto worker = [&](size_t thread_id) {
        for (size_t i = 0; i < translations_per_thread; ++i) {
            size_t index = thread_id * translations_per_thread + i;
            IOVA iova = stage1_iova_base + (index * page_size);
            PA expected_pa = stage2_pa_base + (index * page_size);
            
            auto result = smmu->translate(testStreamID, testPASID, iova, AccessType::Read, testSecurityState);
            
            if (result.isSuccess() && result.getValue().physicalAddress == expected_pa) {
                successful_translations.fetch_add(1);
            } else {
                failed_translations.fetch_add(1);
            }
        }
    };
    
    // Start all threads
    for (size_t i = 0; i < num_threads; ++i) {
        threads.emplace_back(worker, i);
    }
    
    // Wait for completion
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Verify results
    EXPECT_EQ(successful_translations.load(), num_threads * translations_per_thread);
    EXPECT_EQ(failed_translations.load(), 0);
}

// Test 8: Cache Integration with Two-Stage Translation
TEST_F(TwoStageTranslationTest, CacheIntegrationTwoStage) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x6000;
    PA expected_pa = stage2_pa_base + 0x6000;
    
    setupTwoStageMapping(test_iova, expected_pa);
    
    // Clear cache statistics
    smmu->resetStatistics();
    
    // First translation - should be a cache miss
    auto result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, testSecurityState);
    ASSERT_TRUE(result.isSuccess());
    EXPECT_EQ(result.getValue().physicalAddress, expected_pa);
    
    auto stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.misses, 1);
    EXPECT_EQ(stats.hits, 0);
    
    // Second translation - should be a cache hit
    result = smmu->translate(testStreamID, testPASID, test_iova, AccessType::Read, testSecurityState);
    ASSERT_TRUE(result.isSuccess());
    EXPECT_EQ(result.getValue().physicalAddress, expected_pa);
    
    stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.misses, 1);
    EXPECT_EQ(stats.hits, 1);
}

// Test 9: Performance Validation for Two-Stage Translation
TEST_F(TwoStageTranslationTest, TwoStageTranslationPerformance) {
    setupTwoStageStream();
    
    const size_t num_translations = 1000;
    std::vector<IOVA> test_iovas;
    std::vector<PA> expected_pas;
    
    // Setup mappings
    for (size_t i = 0; i < num_translations; ++i) {
        IOVA iova = stage1_iova_base + (i * page_size);
        PA pa = stage2_pa_base + (i * page_size);
        
        test_iovas.push_back(iova);
        expected_pas.push_back(pa);
        setupTwoStageMapping(iova, pa);
    }
    
    // Measure translation performance
    auto start = std::chrono::high_resolution_clock::now();
    
    for (size_t i = 0; i < num_translations; ++i) {
        auto result = smmu->translate(testStreamID, testPASID, test_iovas[i], AccessType::Read, testSecurityState);
        EXPECT_TRUE(result.isSuccess());
        EXPECT_EQ(result.getValue().physicalAddress, expected_pas[i]);
    }
    
    auto end = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
    
    // Performance target: Each translation should complete in under 10 microseconds on average
    double avg_time_per_translation = static_cast<double>(duration.count()) / num_translations;
    EXPECT_LT(avg_time_per_translation, 10.0) << "Two-stage translation performance too slow: " 
                                              << avg_time_per_translation << " microseconds per translation";
    
    std::cout << "Two-stage translation performance: " << avg_time_per_translation 
              << " microseconds per translation" << std::endl;
}

// Test 10: Complex Address Range Two-Stage Translation
TEST_F(TwoStageTranslationTest, ComplexAddressRangeTwoStage) {
    setupTwoStageStream();
    
    // Test various address ranges and alignments
    struct TestCase {
        IOVA iova_offset;
        PA pa_offset;
        size_t size;
        const char* description;
    };
    
    std::vector<TestCase> test_cases = {
        {0x0000, 0x0000, page_size, "Page-aligned base addresses"},
        {0x1000, 0x2000, page_size, "Single page offset"},
        {0x10000, 0x20000, page_size * 4, "Multiple contiguous pages"},
        {0x100000, 0x200000, page_size, "Large offset single page"},
        {0x1000000, 0x2000000, page_size * 16, "Large address range mapping"}
    };
    
    for (const auto& test_case : test_cases) {
        SCOPED_TRACE(test_case.description);
        
        size_t num_pages = test_case.size / page_size;
        
        // Setup mappings for this test case
        for (size_t i = 0; i < num_pages; ++i) {
            IOVA iova = stage1_iova_base + test_case.iova_offset + (i * page_size);
            PA pa = stage2_pa_base + test_case.pa_offset + (i * page_size);
            setupTwoStageMapping(iova, pa);
        }
        
        // Test all pages in the range
        for (size_t i = 0; i < num_pages; ++i) {
            IOVA iova = stage1_iova_base + test_case.iova_offset + (i * page_size);
            PA expected_pa = stage2_pa_base + test_case.pa_offset + (i * page_size);
            
            auto result = smmu->translate(testStreamID, testPASID, iova, AccessType::Read, testSecurityState);
            
            ASSERT_TRUE(result.isSuccess()) << "Translation failed for " << test_case.description 
                                           << ", page " << i;
            EXPECT_EQ(result.getValue().physicalAddress, expected_pa) 
                << "PA mismatch for " << test_case.description << ", page " << i;
        }
    }
}

} // namespace integration
} // namespace smmu