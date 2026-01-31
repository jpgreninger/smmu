// ARM SMMU v3 Two-Stage Translation Integration Tests - Corrected Version
// Copyright (c) 2024 John Greninger
// Task 8.2.1: Two-Stage Translation Integration Tests (6 hours)

#include "test_integration_base.h"
#include <vector>
#include <thread>
#include <chrono>
#include <random>

namespace smmu {
namespace integration {

class TwoStageTranslationTest : public IntegrationTestBase {
protected:
    void SetUp() override {
        IntegrationTestBase::SetUp();
        
        // Common test parameters
        testStreamID = 100;
        testPASID = 1;
        testSecurityState = SecurityState::NonSecure;
        
        // Setup test address space parameters
        stage1_iova_base = base_iova;
        stage1_ipa_base = base_iova + 0x1000000;
        stage2_pa_base = base_pa;
    }

    // Helper to configure a stream for two-stage translation
    void setupTwoStageStream() {
        configureBasicStream(testStreamID, testSecurityState, TranslationStage::BothStages);
        createPASID(testStreamID, testPASID);
    }

    // Helper to map pages for two-stage translation
    void setupTwoStageMapping(IOVA iova, PA final_pa) {
        // Map Stage-1: IOVA -> IPA
        IPA ipa = stage1_ipa_base + (iova - stage1_iova_base);
        mapPage(testStreamID, testPASID, iova, ipa, testSecurityState);
        
        // Map Stage-2: IPA -> PA (PASID 0 for hypervisor context)
        mapPage(testStreamID, 0, ipa, final_pa, testSecurityState);
    }

    StreamID testStreamID;
    PASID testPASID;
    SecurityState testSecurityState;
    
    IOVA stage1_iova_base;
    IPA stage1_ipa_base;
    PA stage2_pa_base;
};

// Test 1: Basic Two-Stage Translation Success Path
TEST_F(TwoStageTranslationTest, BasicTwoStageTranslationSuccess) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x1000;
    PA expected_pa = stage2_pa_base + 0x1000;
    
    setupTwoStageMapping(test_iova, expected_pa);
    
    // Perform translation
    verifyTranslation(testStreamID, testPASID, test_iova, expected_pa);
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
        verifyTranslation(testStreamID, testPASID, test_iovas[i], expected_pas[i]);
    }
}

// Test 3: Stage-1 Translation Fault Detection
TEST_F(TwoStageTranslationTest, Stage1TranslationFault) {
    setupTwoStageStream();
    
    IOVA unmapped_iova = stage1_iova_base + 0x5000;  // Not mapped in Stage-1
    
    // Attempt translation of unmapped IOVA
    auto result = smmu->translate(testStreamID, testPASID, unmapped_iova, AccessType::Read, testSecurityState);
    
    EXPECT_FALSE(result.isSuccess());
    
    // Check that a fault was recorded
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isSuccess());
    EXPECT_GT(events.getValue().size(), 0);
    
    auto& fault = events.getValue()[0];
    EXPECT_EQ(fault.streamID, testStreamID);
    EXPECT_EQ(fault.pasid, testPASID);
    EXPECT_EQ(fault.address, unmapped_iova);
}

// Test 4: Performance Validation for Two-Stage Translation
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

// Test 5: Cache Integration with Two-Stage Translation
TEST_F(TwoStageTranslationTest, CacheIntegrationTwoStage) {
    setupTwoStageStream();
    
    IOVA test_iova = stage1_iova_base + 0x6000;
    PA expected_pa = stage2_pa_base + 0x6000;
    
    setupTwoStageMapping(test_iova, expected_pa);
    
    // Clear cache statistics
    smmu->resetStatistics();
    
    // First translation - should be a cache miss
    verifyTranslation(testStreamID, testPASID, test_iova, expected_pa);
    
    auto stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.lookups, 1);
    EXPECT_EQ(stats.evictions, 0);
    
    // Second translation - should be a cache hit
    verifyTranslation(testStreamID, testPASID, test_iova, expected_pa);
    
    stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.lookups, 2);
}

} // namespace integration
} // namespace smmu