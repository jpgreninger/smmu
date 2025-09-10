// ARM SMMU v3 PASID Context Switching Integration Tests
// Copyright (c) 2024 John Greninger
// Task 8.2.3: PASID Context Switching Tests (5 hours)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <vector>
#include <thread>
#include <random>
#include <chrono>
#include <map>
#include <set>
#include <algorithm>

namespace smmu {
namespace integration {

class PASIDContextSwitchingTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Create SMMU optimized for PASID management testing
        SMMUConfiguration config;
        config.maxStreams = 512;
        config.maxPASIDsPerStream = 1024;  // Large PASID space for testing
        config.cacheConfig.maxEntries = 4096;
        config.cacheConfig.replacementPolicy = CacheReplacementPolicy::LRU;
        config.queueConfig.maxEventQueueSize = 2048;
        config.queueConfig.maxCommandQueueSize = 1024;
        config.addressConfig.addressSpaceBits = 48;
        config.addressConfig.granuleSize = 4096;
        
        smmu = std::make_unique<SMMU>(config);
        
        // Test parameters
        testStreamID = 100;
        page_size = 4096;
        base_iova = 0x1000000;
        base_pa = 0x10000000;
        
        // Setup base stream configuration
        setupTestStream();
        
        // Initialize random number generator
        rng.seed(std::chrono::steady_clock::now().time_since_epoch().count());
    }

    void TearDown() override {
        smmu.reset();
    }

    void setupTestStream() {
        StreamConfig streamConfig;
        streamConfig.translationStage = TranslationStage::Stage1Only;
        streamConfig.faultMode = FaultMode::Terminate;
        streamConfig.securityState = SecurityState::NonSecure;
        streamConfig.stage1Enabled = true;
        streamConfig.stage2Enabled = false;
        
        streamConfig.stage1TTBRs[0] = base_pa;
        streamConfig.stage1TCR.granuleSize = 4096;
        streamConfig.stage1TCR.addressSpaceBits = 48;
        streamConfig.stage1TCR.walkCacheDisable = false;
        
        auto result = smmu->configureStream(testStreamID, streamConfig);
        ASSERT_TRUE(result.isSuccess()) << "Failed to configure test stream";
        
        result = smmu->enableStream(testStreamID);
        ASSERT_TRUE(result.isSuccess()) << "Failed to enable test stream";
    }

    void createAndMapPASID(PASID pasid, size_t num_pages = 10) {
        auto result = smmu->createStreamPASID(testStreamID, pasid);
        ASSERT_TRUE(result.isSuccess()) << "Failed to create PASID " << pasid;
        
        // Map pages for this PASID
        PagePermissions perms;
        perms.read = true;
        perms.write = true;
        perms.execute = false;
        perms.user = true;
        perms.global = false;
        
        for (size_t i = 0; i < num_pages; ++i) {
            IOVA iova = base_iova + (pasid * 0x100000) + (i * page_size);  // Unique per PASID
            PA pa = base_pa + (pasid * 0x100000) + (i * page_size);
            
            result = smmu->mapPage(testStreamID, pasid, iova, pa, perms);
            ASSERT_TRUE(result.isSuccess()) << "Failed to map page " << i << " for PASID " << pasid;
        }
    }

    // Helper to verify PASID context integrity
    void verifyPASIDContext(PASID pasid, size_t num_pages = 10) {
        for (size_t i = 0; i < num_pages; ++i) {
            IOVA iova = base_iova + (pasid * 0x100000) + (i * page_size);
            PA expected_pa = base_pa + (pasid * 0x100000) + (i * page_size);
            
            auto result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
            ASSERT_TRUE(result.isSuccess()) << "Translation failed for PASID " << pasid << ", page " << i;
            EXPECT_EQ(result.getValue().physicalAddress, expected_pa) 
                << "PA mismatch for PASID " << pasid << ", page " << i;
        }
    }

    std::unique_ptr<SMMU> smmu;
    StreamID testStreamID;
    size_t page_size;
    IOVA base_iova;
    PA base_pa;
    std::mt19937 rng;
};

// Test 1: Basic PASID Context Creation and Switching
TEST_F(PASIDContextSwitchingTest, BasicPASIDContextSwitching) {
    const std::vector<PASID> test_pasids = {1, 2, 3, 4, 5};
    
    // Create multiple PASID contexts
    for (PASID pasid : test_pasids) {
        createAndMapPASID(pasid);
    }
    
    // Test switching between PASID contexts
    for (PASID pasid : test_pasids) {
        SCOPED_TRACE("Testing PASID " + std::to_string(pasid));
        verifyPASIDContext(pasid);
    }
    
    // Test random access pattern across PASIDs
    std::uniform_int_distribution<size_t> pasid_dist(0, test_pasids.size() - 1);
    std::uniform_int_distribution<size_t> page_dist(0, 9);
    
    for (int i = 0; i < 100; ++i) {
        PASID pasid = test_pasids[pasid_dist(rng)];
        size_t page_index = page_dist(rng);
        
        IOVA iova = base_iova + (pasid * 0x100000) + (page_index * page_size);
        PA expected_pa = base_pa + (pasid * 0x100000) + (page_index * page_size);
        
        auto result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
        ASSERT_TRUE(result.isSuccess()) << "Random access failed for PASID " << pasid;
        EXPECT_EQ(result.getValue().physicalAddress, expected_pa);
    }
}

// Test 2: PASID Context Isolation
TEST_F(PASIDContextSwitchingTest, PASIDContextIsolation) {
    const PASID pasid1 = 10;
    const PASID pasid2 = 20;
    
    createAndMapPASID(pasid1);
    createAndMapPASID(pasid2);
    
    // Use same IOVA for both PASIDs but they should map to different PAs
    IOVA shared_iova = base_iova + 0x1000;
    
    auto result1 = smmu->translate(testStreamID, pasid1, shared_iova, AccessType::Read);
    auto result2 = smmu->translate(testStreamID, pasid2, shared_iova, AccessType::Read);
    
    ASSERT_TRUE(result1.isSuccess());
    ASSERT_TRUE(result2.isSuccess());
    
    // Should get different physical addresses due to PASID isolation
    EXPECT_NE(result1.getValue().physicalAddress, result2.getValue().physicalAddress);
    
    PA expected_pa1 = base_pa + (pasid1 * 0x100000) + 0x1000;
    PA expected_pa2 = base_pa + (pasid2 * 0x100000) + 0x1000;
    
    EXPECT_EQ(result1.getValue().physicalAddress, expected_pa1);
    EXPECT_EQ(result2.getValue().physicalAddress, expected_pa2);
}

// Test 3: PASID Creation and Removal Lifecycle
TEST_F(PASIDContextSwitchingTest, PASIDLifecycleManagement) {
    const PASID test_pasid = 30;
    
    // Verify PASID doesn't exist initially
    IOVA test_iova = base_iova + 0x2000;
    auto result = smmu->translate(testStreamID, test_pasid, test_iova, AccessType::Read);
    EXPECT_FALSE(result.isSuccess());
    EXPECT_EQ(result.getError(), SMMUError::PASIDNotFound);
    
    // Create PASID and verify it works
    createAndMapPASID(test_pasid, 5);
    verifyPASIDContext(test_pasid, 5);
    
    // Remove PASID
    auto remove_result = smmu->removeStreamPASID(testStreamID, test_pasid);
    EXPECT_TRUE(remove_result.isSuccess()) << "Failed to remove PASID";
    
    // Verify PASID no longer works
    result = smmu->translate(testStreamID, test_pasid, test_iova, AccessType::Read);
    EXPECT_FALSE(result.isSuccess());
    EXPECT_EQ(result.getError(), SMMUError::PASIDNotFound);
    
    // Recreate PASID and verify it works again
    createAndMapPASID(test_pasid, 3);
    verifyPASIDContext(test_pasid, 3);
}

// Test 4: Large-Scale PASID Context Switching
TEST_F(PASIDContextSwitchingTest, LargeScalePASIDSwitching) {
    const size_t num_pasids = 100;
    const size_t pages_per_pasid = 20;
    
    std::vector<PASID> pasids;
    
    // Create many PASID contexts
    for (size_t i = 1; i <= num_pasids; ++i) {
        PASID pasid = i;
        pasids.push_back(pasid);
        createAndMapPASID(pasid, pages_per_pasid);
    }
    
    // Verify all contexts work correctly
    for (PASID pasid : pasids) {
        SCOPED_TRACE("Large-scale test PASID " + std::to_string(pasid));
        verifyPASIDContext(pasid, pages_per_pasid);
    }
    
    // Perform intensive random switching
    std::uniform_int_distribution<size_t> pasid_dist(0, pasids.size() - 1);
    std::uniform_int_distribution<size_t> page_dist(0, pages_per_pasid - 1);
    
    const size_t num_random_accesses = 5000;
    size_t successful_accesses = 0;
    
    for (size_t i = 0; i < num_random_accesses; ++i) {
        PASID pasid = pasids[pasid_dist(rng)];
        size_t page_index = page_dist(rng);
        
        IOVA iova = base_iova + (pasid * 0x100000) + (page_index * page_size);
        PA expected_pa = base_pa + (pasid * 0x100000) + (page_index * page_size);
        
        auto result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
        
        if (result.isSuccess() && result.getValue().physicalAddress == expected_pa) {
            successful_accesses++;
        }
    }
    
    EXPECT_EQ(successful_accesses, num_random_accesses) 
        << "Large-scale PASID switching should be 100% successful";
}

// Test 5: Concurrent PASID Context Switching
TEST_F(PASIDContextSwitchingTest, ConcurrentPASIDSwitching) {
    const size_t num_threads = 8;
    const size_t pasids_per_thread = 10;
    const size_t accesses_per_thread = 500;
    
    // Create PASID contexts for each thread
    std::vector<std::vector<PASID>> thread_pasids(num_threads);
    
    for (size_t thread_id = 0; thread_id < num_threads; ++thread_id) {
        for (size_t i = 0; i < pasids_per_thread; ++i) {
            PASID pasid = (thread_id * pasids_per_thread) + i + 1;
            thread_pasids[thread_id].push_back(pasid);
            createAndMapPASID(pasid, 10);
        }
    }
    
    std::vector<std::thread> threads;
    std::atomic<size_t> total_successful(0);
    std::atomic<size_t> total_failed(0);
    
    auto worker = [&](size_t thread_id) {
        std::mt19937 local_rng(thread_id);
        std::uniform_int_distribution<size_t> pasid_dist(0, pasids_per_thread - 1);
        std::uniform_int_distribution<size_t> page_dist(0, 9);
        
        size_t local_successful = 0;
        size_t local_failed = 0;
        
        for (size_t i = 0; i < accesses_per_thread; ++i) {
            PASID pasid = thread_pasids[thread_id][pasid_dist(local_rng)];
            size_t page_index = page_dist(local_rng);
            
            IOVA iova = base_iova + (pasid * 0x100000) + (page_index * page_size);
            PA expected_pa = base_pa + (pasid * 0x100000) + (page_index * page_size);
            
            auto result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
            
            if (result.isSuccess() && result.getValue().physicalAddress == expected_pa) {
                local_successful++;
            } else {
                local_failed++;
            }
        }
        
        total_successful.fetch_add(local_successful);
        total_failed.fetch_add(local_failed);
    };
    
    // Start all worker threads
    for (size_t i = 0; i < num_threads; ++i) {
        threads.emplace_back(worker, i);
    }
    
    // Wait for completion
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Verify concurrent PASID switching worked correctly
    size_t expected_total = num_threads * accesses_per_thread;
    EXPECT_EQ(total_successful.load(), expected_total);
    EXPECT_EQ(total_failed.load(), 0);
    
    std::cout << "Concurrent PASID switching: " << total_successful.load() 
              << " successful, " << total_failed.load() << " failed" << std::endl;
}

// Test 6: PASID Cache Behavior During Context Switching
TEST_F(PASIDContextSwitchingTest, PASIDCacheBehavior) {
    const std::vector<PASID> test_pasids = {40, 41, 42};
    
    for (PASID pasid : test_pasids) {
        createAndMapPASID(pasid, 5);
    }
    
    IOVA test_iova = base_iova + 0x1000;
    
    // Reset statistics
    smmu->resetStatistics();
    
    // First access to each PASID should be cache miss
    for (PASID pasid : test_pasids) {
        auto result = smmu->translate(testStreamID, pasid, test_iova, AccessType::Read);
        ASSERT_TRUE(result.isSuccess());
    }
    
    auto stats_after_misses = smmu->getCacheStatistics();
    EXPECT_EQ(stats_after_misses.misses, test_pasids.size());
    EXPECT_EQ(stats_after_misses.hits, 0);
    
    // Second access to each PASID should be cache hit
    for (PASID pasid : test_pasids) {
        auto result = smmu->translate(testStreamID, pasid, test_iova, AccessType::Read);
        ASSERT_TRUE(result.isSuccess());
    }
    
    auto stats_after_hits = smmu->getCacheStatistics();
    EXPECT_EQ(stats_after_hits.misses, test_pasids.size());
    EXPECT_EQ(stats_after_hits.hits, test_pasids.size());
    
    // Test PASID-specific cache invalidation
    smmu->invalidatePASIDCache(testStreamID, test_pasids[0]);
    
    // Next access to invalidated PASID should be miss, others should be hits
    auto result_invalidated = smmu->translate(testStreamID, test_pasids[0], test_iova, AccessType::Read);
    auto result_cached1 = smmu->translate(testStreamID, test_pasids[1], test_iova, AccessType::Read);
    auto result_cached2 = smmu->translate(testStreamID, test_pasids[2], test_iova, AccessType::Read);
    
    ASSERT_TRUE(result_invalidated.isSuccess());
    ASSERT_TRUE(result_cached1.isSuccess());
    ASSERT_TRUE(result_cached2.isSuccess());
    
    auto stats_after_invalidation = smmu->getCacheStatistics();
    EXPECT_EQ(stats_after_invalidation.misses, test_pasids.size() + 1);  // One additional miss
    EXPECT_EQ(stats_after_invalidation.hits, test_pasids.size() + 2);    // Two additional hits
}

// Test 7: PASID Context Switching with Different Security States
TEST_F(PASIDContextSwitchingTest, PASIDSecurityStateContextSwitching) {
    // Reconfigure stream for both secure and non-secure operation
    StreamConfig streamConfig;
    streamConfig.translationStage = TranslationStage::Stage1Only;
    streamConfig.faultMode = FaultMode::Terminate;
    streamConfig.securityState = SecurityState::NonSecure;  // Base state
    streamConfig.stage1Enabled = true;
    streamConfig.stage2Enabled = false;
    
    streamConfig.stage1TTBRs[0] = base_pa;
    streamConfig.stage1TCR.granuleSize = 4096;
    streamConfig.stage1TCR.addressSpaceBits = 48;
    streamConfig.stage1TCR.walkCacheDisable = false;
    
    auto result = smmu->configureStream(testStreamID, streamConfig);
    ASSERT_TRUE(result.isSuccess());
    
    const PASID nonsecure_pasid = 50;
    const PASID secure_pasid = 51;
    
    // Create PASIDs for different security states
    auto create_result = smmu->createStreamPASID(testStreamID, nonsecure_pasid);
    ASSERT_TRUE(create_result.isSuccess());
    
    create_result = smmu->createStreamPASID(testStreamID, secure_pasid);
    ASSERT_TRUE(create_result.isSuccess());
    
    // Map pages with different security states
    IOVA test_iova = base_iova + 0x5000;
    PA nonsecure_pa = base_pa + 0x5000;
    PA secure_pa = base_pa + 0x6000;
    
    PagePermissions perms;
    perms.read = true;
    perms.write = true;
    perms.execute = false;
    perms.user = true;
    perms.global = false;
    
    result = smmu->mapPage(testStreamID, nonsecure_pasid, test_iova, nonsecure_pa, perms, SecurityState::NonSecure);
    ASSERT_TRUE(result.isSuccess());
    
    result = smmu->mapPage(testStreamID, secure_pasid, test_iova, secure_pa, perms, SecurityState::Secure);
    ASSERT_TRUE(result.isSuccess());
    
    // Test access with matching security states
    auto nonsecure_result = smmu->translate(testStreamID, nonsecure_pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(nonsecure_result.isSuccess());
    EXPECT_EQ(nonsecure_result.getValue().physicalAddress, nonsecure_pa);
    EXPECT_EQ(nonsecure_result.getValue().securityState, SecurityState::NonSecure);
    
    auto secure_result = smmu->translate(testStreamID, secure_pasid, test_iova, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(secure_result.isSuccess());
    EXPECT_EQ(secure_result.getValue().physicalAddress, secure_pa);
    EXPECT_EQ(secure_result.getValue().securityState, SecurityState::Secure);
    
    // Test security violations
    auto violation_result = smmu->translate(testStreamID, nonsecure_pasid, test_iova, AccessType::Read, SecurityState::Secure);
    EXPECT_FALSE(violation_result.isSuccess());
    EXPECT_EQ(violation_result.getError(), SMMUError::InvalidSecurityState);
}

// Test 8: PASID Context Switching Performance
TEST_F(PASIDContextSwitchingTest, PASIDSwitchingPerformance) {
    const size_t num_pasids = 50;
    const size_t switches_per_pasid = 100;
    
    std::vector<PASID> pasids;
    for (size_t i = 1; i <= num_pasids; ++i) {
        pasids.push_back(i);
        createAndMapPASID(i, 10);
    }
    
    // Pre-populate cache with all PASIDs
    for (PASID pasid : pasids) {
        IOVA iova = base_iova + (pasid * 0x100000);
        auto result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
        ASSERT_TRUE(result.isSuccess());
    }
    
    // Measure PASID switching performance
    std::uniform_int_distribution<size_t> pasid_dist(0, pasids.size() - 1);
    const size_t total_switches = num_pasids * switches_per_pasid;
    
    auto start = std::chrono::high_resolution_clock::now();
    
    for (size_t i = 0; i < total_switches; ++i) {
        PASID pasid = pasids[pasid_dist(rng)];
        IOVA iova = base_iova + (pasid * 0x100000);
        
        auto result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
        EXPECT_TRUE(result.isSuccess());
    }
    
    auto end = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
    
    double avg_switch_time = static_cast<double>(duration.count()) / total_switches;
    
    // Performance target: PASID switching should be very fast (sub-microsecond)
    EXPECT_LT(avg_switch_time, 1.0) << "PASID context switching too slow: " 
                                   << avg_switch_time << " microseconds per switch";
    
    std::cout << "PASID switching performance: " << avg_switch_time 
              << " microseconds per switch" << std::endl;
}

// Test 9: PASID Context Switching with Faults
TEST_F(PASIDContextSwitchingTest, PASIDFaultHandlingDuringSwitching) {
    const PASID valid_pasid = 60;
    const PASID fault_pasid = 61;
    const PASID unmapped_pasid = 62;
    
    // Create valid PASID with full mapping
    createAndMapPASID(valid_pasid, 10);
    
    // Create PASID with limited mapping (will cause faults)
    auto create_result = smmu->createStreamPASID(testStreamID, fault_pasid);
    ASSERT_TRUE(create_result.isSuccess());
    
    // Map only first page for fault_pasid
    PagePermissions perms;
    perms.read = true;
    perms.write = false;  // Write will fault
    perms.execute = false;
    perms.user = true;
    perms.global = false;
    
    IOVA fault_iova = base_iova + (fault_pasid * 0x100000);
    PA fault_pa = base_pa + (fault_pasid * 0x100000);
    
    auto map_result = smmu->mapPage(testStreamID, fault_pasid, fault_iova, fault_pa, perms);
    ASSERT_TRUE(map_result.isSuccess());
    
    // Don't create unmapped_pasid at all
    
    smmu->clearEvents();
    
    // Test switching between valid operations and faults
    struct TestCase {
        PASID pasid;
        IOVA iova_offset;
        AccessType access;
        bool should_succeed;
        SMMUError expected_error;
        const char* description;
    };
    
    std::vector<TestCase> test_cases = {
        {valid_pasid, 0x1000, AccessType::Read, true, SMMUError::Success, "Valid PASID read"},
        {valid_pasid, 0x2000, AccessType::Write, true, SMMUError::Success, "Valid PASID write"},
        {fault_pasid, 0, AccessType::Read, true, SMMUError::Success, "Fault PASID read mapped page"},
        {fault_pasid, 0, AccessType::Write, false, SMMUError::PagePermissionViolation, "Fault PASID write violation"},
        {fault_pasid, 0x1000, AccessType::Read, false, SMMUError::PageNotMapped, "Fault PASID unmapped page"},
        {unmapped_pasid, 0, AccessType::Read, false, SMMUError::PASIDNotFound, "Non-existent PASID"},
        {valid_pasid, 0x3000, AccessType::Read, true, SMMUError::Success, "Valid PASID after faults"}
    };
    
    for (const auto& test_case : test_cases) {
        SCOPED_TRACE(test_case.description);
        
        IOVA iova = base_iova + (test_case.pasid * 0x100000) + test_case.iova_offset;
        auto result = smmu->translate(testStreamID, test_case.pasid, iova, test_case.access);
        
        if (test_case.should_succeed) {
            EXPECT_TRUE(result.isSuccess()) << "Expected success for " << test_case.description;
        } else {
            EXPECT_FALSE(result.isSuccess()) << "Expected failure for " << test_case.description;
            EXPECT_EQ(result.getError(), test_case.expected_error);
        }
    }
    
    // Verify that valid PASID still works after fault conditions
    IOVA valid_iova = base_iova + (valid_pasid * 0x100000) + 0x4000;
    auto final_result = smmu->translate(testStreamID, valid_pasid, valid_iova, AccessType::Read);
    EXPECT_TRUE(final_result.isSuccess()) << "Valid PASID should still work after fault conditions";
}

// Test 10: PASID Resource Limit Testing
TEST_F(PASIDContextSwitchingTest, PASIDResourceLimits) {
    // Get current configuration limits
    const auto& config = smmu->getConfiguration();
    const size_t max_pasids = config.maxPASIDsPerStream;
    
    std::vector<PASID> created_pasids;
    
    // Create PASIDs up to the limit
    for (size_t i = 1; i <= max_pasids; ++i) {
        PASID pasid = i;
        auto result = smmu->createStreamPASID(testStreamID, pasid);
        
        if (result.isSuccess()) {
            created_pasids.push_back(pasid);
        } else {
            // Might hit limit before theoretical maximum due to implementation constraints
            EXPECT_EQ(result.getError(), SMMUError::PASIDLimitExceeded);
            break;
        }
    }
    
    EXPECT_GT(created_pasids.size(), 0) << "Should be able to create at least some PASIDs";
    
    // Try to create one more PASID beyond the limit (if we hit max)
    if (created_pasids.size() == max_pasids) {
        PASID over_limit_pasid = max_pasids + 1;
        auto result = smmu->createStreamPASID(testStreamID, over_limit_pasid);
        EXPECT_FALSE(result.isSuccess());
        EXPECT_EQ(result.getError(), SMMUError::PASIDLimitExceeded);
    }
    
    // Verify all created PASIDs work
    for (PASID pasid : created_pasids) {
        // Create minimal mapping
        PagePermissions perms;
        perms.read = true;
        perms.write = true;
        perms.execute = false;
        perms.user = true;
        perms.global = false;
        
        IOVA iova = base_iova + (pasid * 0x100000);
        PA pa = base_pa + (pasid * 0x100000);
        
        auto map_result = smmu->mapPage(testStreamID, pasid, iova, pa, perms);
        ASSERT_TRUE(map_result.isSuccess()) << "Failed to map for PASID " << pasid;
        
        auto trans_result = smmu->translate(testStreamID, pasid, iova, AccessType::Read);
        EXPECT_TRUE(trans_result.isSuccess()) << "Translation failed for PASID " << pasid;
    }
    
    // Remove some PASIDs and verify we can create new ones
    const size_t pasids_to_remove = std::min(created_pasids.size(), static_cast<size_t>(10));
    
    for (size_t i = 0; i < pasids_to_remove; ++i) {
        auto result = smmu->removeStreamPASID(testStreamID, created_pasids[i]);
        EXPECT_TRUE(result.isSuccess()) << "Failed to remove PASID " << created_pasids[i];
    }
    
    // Should be able to create new PASIDs now
    for (size_t i = 0; i < pasids_to_remove; ++i) {
        PASID new_pasid = max_pasids + 10 + i;  // Use different PASID numbers
        auto result = smmu->createStreamPASID(testStreamID, new_pasid);
        EXPECT_TRUE(result.isSuccess()) << "Should be able to create new PASID after removal";
    }
    
    std::cout << "PASID resource test: Created " << created_pasids.size() 
              << " PASIDs out of " << max_pasids << " maximum" << std::endl;
}

} // namespace integration
} // namespace smmu