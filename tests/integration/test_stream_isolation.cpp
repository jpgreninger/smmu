// ARM SMMU v3 Stream Isolation Validation Integration Tests
// Copyright (c) 2024 John Greninger
// Task 8.2.2: Stream Isolation Validation Tests (4 hours)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <vector>
#include <thread>
#include <random>
#include <chrono>
#include <set>

namespace smmu {
namespace integration {

class StreamIsolationTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Create SMMU with configuration optimized for multi-stream testing
        SMMUConfiguration config;
        config.maxStreams = 2048;  // Large number of streams for isolation testing
        config.maxPASIDsPerStream = 256;
        config.cacheConfig.maxEntries = 2048;
        config.cacheConfig.replacementPolicy = CacheReplacementPolicy::LRU;
        config.queueConfig.maxEventQueueSize = 1024;
        config.queueConfig.maxCommandQueueSize = 512;
        config.addressConfig.addressSpaceBits = 48;
        config.addressConfig.granuleSize = 4096;
        
        smmu = std::make_unique<SMMU>(config);
        
        // Test parameters
        page_size = 4096;
        base_iova = 0x1000000;
        base_pa = 0x10000000;
        
        // Initialize random number generator for stress testing
        rng.seed(std::chrono::steady_clock::now().time_since_epoch().count());
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper to configure a stream with specific settings
    void configureTestStream(StreamID streamID, SecurityState securityState = SecurityState::NonSecure,
                           TranslationStage stage = TranslationStage::Stage1Only) {
        StreamConfig streamConfig;
        streamConfig.translationStage = stage;
        streamConfig.faultMode = FaultMode::Terminate;
        streamConfig.securityState = securityState;
        streamConfig.stage1Enabled = (stage == TranslationStage::Stage1Only || stage == TranslationStage::BothStages);
        streamConfig.stage2Enabled = (stage == TranslationStage::Stage2Only || stage == TranslationStage::BothStages);
        
        // Configure translation table bases
        streamConfig.stage1TTBRs[0] = base_pa + (streamID * 0x100000);  // Unique per stream
        streamConfig.stage1TCR.granuleSize = 4096;
        streamConfig.stage1TCR.addressSpaceBits = 48;
        streamConfig.stage1TCR.walkCacheDisable = false;
        
        if (stage == TranslationStage::BothStages || stage == TranslationStage::Stage2Only) {
            streamConfig.stage2TTBR = base_pa + (streamID * 0x100000) + 0x50000;
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
    void createStreamPASID(StreamID streamID, PASID pasid) {
        auto result = smmu->createStreamPASID(streamID, pasid);
        ASSERT_TRUE(result.isSuccess()) << "Failed to create PASID " << pasid 
                                       << " for stream " << streamID;
    }

    // Helper to map a page for a specific stream/PASID
    void mapStreamPage(StreamID streamID, PASID pasid, IOVA iova, PA pa, 
                      const PagePermissions& perms, SecurityState securityState = SecurityState::NonSecure) {
        auto result = smmu->mapPage(streamID, pasid, iova, pa, perms, securityState);
        ASSERT_TRUE(result.isSuccess()) << "Failed to map page for stream " << streamID 
                                       << ", PASID " << pasid;
    }

    // Helper to create default page permissions
    PagePermissions createDefaultPermissions(bool write = true, bool execute = false) {
        PagePermissions perms;
        perms.read = true;
        perms.write = write;
        perms.execute = execute;
        perms.user = true;
        perms.global = false;
        return perms;
    }

    std::unique_ptr<SMMU> smmu;
    size_t page_size;
    IOVA base_iova;
    PA base_pa;
    std::mt19937 rng;
};

// Test 1: Basic Stream Isolation - Different Streams Cannot Access Each Other's Memory
TEST_F(StreamIsolationTest, BasicStreamIsolation) {
    const StreamID stream1 = 100;
    const StreamID stream2 = 200;
    const PASID pasid = 1;
    
    // Configure both streams
    configureTestStream(stream1);
    configureTestStream(stream2);
    
    createStreamPASID(stream1, pasid);
    createStreamPASID(stream2, pasid);
    
    // Map same IOVA to different physical addresses for each stream
    IOVA shared_iova = base_iova + 0x1000;
    PA stream1_pa = base_pa + 0x1000;
    PA stream2_pa = base_pa + 0x2000;
    
    auto perms = createDefaultPermissions();
    mapStreamPage(stream1, pasid, shared_iova, stream1_pa, perms);
    mapStreamPage(stream2, pasid, shared_iova, stream2_pa, perms);
    
    // Test translation for stream1
    auto result1 = smmu->translate(stream1, pasid, shared_iova, AccessType::Read);
    ASSERT_TRUE(result1.isSuccess());
    EXPECT_EQ(result1.getValue().physicalAddress, stream1_pa);
    
    // Test translation for stream2 - should get different PA
    auto result2 = smmu->translate(stream2, pasid, shared_iova, AccessType::Read);
    ASSERT_TRUE(result2.isSuccess());
    EXPECT_EQ(result2.getValue().physicalAddress, stream2_pa);
    
    // Verify that streams see different physical addresses
    EXPECT_NE(result1.getValue().physicalAddress, result2.getValue().physicalAddress);
}

// Test 2: Security State Isolation Between Streams
TEST_F(StreamIsolationTest, SecurityStateIsolation) {
    const StreamID secure_stream = 300;
    const StreamID nonsecure_stream = 400;
    const PASID pasid = 1;
    
    // Configure streams with different security states
    configureTestStream(secure_stream, SecurityState::Secure);
    configureTestStream(nonsecure_stream, SecurityState::NonSecure);
    
    createStreamPASID(secure_stream, pasid);
    createStreamPASID(nonsecure_stream, pasid);
    
    IOVA test_iova = base_iova + 0x3000;
    PA secure_pa = base_pa + 0x3000;
    PA nonsecure_pa = base_pa + 0x4000;
    
    auto perms = createDefaultPermissions();
    mapStreamPage(secure_stream, pasid, test_iova, secure_pa, perms, SecurityState::Secure);
    mapStreamPage(nonsecure_stream, pasid, test_iova, nonsecure_pa, perms, SecurityState::NonSecure);
    
    // Test secure stream with secure access
    auto secure_result = smmu->translate(secure_stream, pasid, test_iova, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(secure_result.isSuccess());
    EXPECT_EQ(secure_result.getValue().physicalAddress, secure_pa);
    EXPECT_EQ(secure_result.getValue().securityState, SecurityState::Secure);
    
    // Test non-secure stream with non-secure access
    auto nonsecure_result = smmu->translate(nonsecure_stream, pasid, test_iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(nonsecure_result.isSuccess());
    EXPECT_EQ(nonsecure_result.getValue().physicalAddress, nonsecure_pa);
    EXPECT_EQ(nonsecure_result.getValue().securityState, SecurityState::NonSecure);
    
    // Test security violation: non-secure stream trying secure access
    auto violation_result = smmu->translate(nonsecure_stream, pasid, test_iova, AccessType::Read, SecurityState::Secure);
    EXPECT_FALSE(violation_result.isSuccess());
    EXPECT_EQ(violation_result.getError(), SMMUError::InvalidSecurityState);
}

// Test 3: Fault Isolation Between Streams
TEST_F(StreamIsolationTest, FaultIsolationBetweenStreams) {
    const StreamID stream1 = 500;
    const StreamID stream2 = 600;
    const PASID pasid = 1;
    
    configureTestStream(stream1);
    configureTestStream(stream2);
    
    createStreamPASID(stream1, pasid);
    createStreamPASID(stream2, pasid);
    
    // Map page only for stream1
    IOVA test_iova = base_iova + 0x5000;
    PA test_pa = base_pa + 0x5000;
    auto perms = createDefaultPermissions();
    mapStreamPage(stream1, pasid, test_iova, test_pa, perms);
    
    // Clear any existing events
    smmu->clearEvents();
    
    // Test valid translation for stream1
    auto result1 = smmu->translate(stream1, pasid, test_iova, AccessType::Read);
    EXPECT_TRUE(result1.isSuccess());
    
    // Test invalid translation for stream2 (should fault)
    auto result2 = smmu->translate(stream2, pasid, test_iova, AccessType::Read);
    EXPECT_FALSE(result2.isSuccess());
    EXPECT_EQ(result2.getError(), SMMUError::PageNotMapped);
    
    // Check that fault is recorded for stream2 only
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isSuccess());
    EXPECT_GT(events.getValue().size(), 0);
    
    // Find the fault record for stream2
    bool found_stream2_fault = false;
    for (const auto& fault : events.getValue()) {
        if (fault.streamID == stream2) {
            found_stream2_fault = true;
            EXPECT_EQ(fault.pasid, pasid);
            EXPECT_EQ(fault.address, test_iova);
            EXPECT_EQ(fault.faultType, FaultType::Level1Translation);
        }
        // Should not find any faults for stream1
        EXPECT_NE(fault.streamID, stream1) << "Stream1 should not have faults";
    }
    
    EXPECT_TRUE(found_stream2_fault) << "Should find fault record for stream2";
}

// Test 4: Cache Isolation Between Streams
TEST_F(StreamIsolationTest, CacheIsolationBetweenStreams) {
    const StreamID stream1 = 700;
    const StreamID stream2 = 800;
    const PASID pasid = 1;
    
    configureTestStream(stream1);
    configureTestStream(stream2);
    
    createStreamPASID(stream1, pasid);
    createStreamPASID(stream2, pasid);
    
    // Map same IOVA to different PAs for each stream
    IOVA shared_iova = base_iova + 0x6000;
    PA stream1_pa = base_pa + 0x6000;
    PA stream2_pa = base_pa + 0x7000;
    
    auto perms = createDefaultPermissions();
    mapStreamPage(stream1, pasid, shared_iova, stream1_pa, perms);
    mapStreamPage(stream2, pasid, shared_iova, stream2_pa, perms);
    
    // Reset cache statistics
    smmu->resetStatistics();
    
    // First translation for stream1 - cache miss
    auto result1 = smmu->translate(stream1, pasid, shared_iova, AccessType::Read);
    ASSERT_TRUE(result1.isSuccess());
    EXPECT_EQ(result1.getValue().physicalAddress, stream1_pa);
    
    // First translation for stream2 - should also be cache miss (different stream)
    auto result2 = smmu->translate(stream2, pasid, shared_iova, AccessType::Read);
    ASSERT_TRUE(result2.isSuccess());
    EXPECT_EQ(result2.getValue().physicalAddress, stream2_pa);
    
    auto stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.misses, 2) << "Both streams should have cache misses";
    
    // Repeat translations - should be cache hits but still return different PAs
    result1 = smmu->translate(stream1, pasid, shared_iova, AccessType::Read);
    ASSERT_TRUE(result1.isSuccess());
    EXPECT_EQ(result1.getValue().physicalAddress, stream1_pa);
    
    result2 = smmu->translate(stream2, pasid, shared_iova, AccessType::Read);
    ASSERT_TRUE(result2.isSuccess());
    EXPECT_EQ(result2.getValue().physicalAddress, stream2_pa);
    
    // Verify cache hits occurred but isolation maintained
    stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.hits, 2) << "Both streams should have cache hits";
    EXPECT_NE(result1.getValue().physicalAddress, result2.getValue().physicalAddress) 
        << "Cache hits should still maintain stream isolation";
}

// Test 5: Permission Isolation Between Streams
TEST_F(StreamIsolationTest, PermissionIsolationBetweenStreams) {
    const StreamID readonly_stream = 900;
    const StreamID readwrite_stream = 1000;
    const PASID pasid = 1;
    
    configureTestStream(readonly_stream);
    configureTestStream(readwrite_stream);
    
    createStreamPASID(readonly_stream, pasid);
    createStreamPASID(readwrite_stream, pasid);
    
    IOVA test_iova = base_iova + 0x8000;
    PA readonly_pa = base_pa + 0x8000;
    PA readwrite_pa = base_pa + 0x9000;
    
    // Map with read-only permissions for readonly_stream
    PagePermissions readonly_perms = createDefaultPermissions(false, false);  // No write
    mapStreamPage(readonly_stream, pasid, test_iova, readonly_pa, readonly_perms);
    
    // Map with read-write permissions for readwrite_stream
    PagePermissions readwrite_perms = createDefaultPermissions(true, false);  // Read+Write
    mapStreamPage(readwrite_stream, pasid, test_iova, readwrite_pa, readwrite_perms);
    
    // Test read access for both streams
    auto read_result1 = smmu->translate(readonly_stream, pasid, test_iova, AccessType::Read);
    EXPECT_TRUE(read_result1.isSuccess());
    EXPECT_FALSE(read_result1.getValue().permissions.write);
    
    auto read_result2 = smmu->translate(readwrite_stream, pasid, test_iova, AccessType::Read);
    EXPECT_TRUE(read_result2.isSuccess());
    EXPECT_TRUE(read_result2.getValue().permissions.write);
    
    // Test write access - should fail for readonly_stream, succeed for readwrite_stream
    auto write_result1 = smmu->translate(readonly_stream, pasid, test_iova, AccessType::Write);
    EXPECT_FALSE(write_result1.isSuccess());
    EXPECT_EQ(write_result1.getError(), SMMUError::PagePermissionViolation);
    
    auto write_result2 = smmu->translate(readwrite_stream, pasid, test_iova, AccessType::Write);
    EXPECT_TRUE(write_result2.isSuccess());
}

// Test 6: Concurrent Multi-Stream Access
TEST_F(StreamIsolationTest, ConcurrentMultiStreamAccess) {
    const size_t num_streams = 10;
    const size_t translations_per_stream = 100;
    const PASID pasid = 1;
    
    std::vector<StreamID> stream_ids;
    
    // Setup multiple streams
    for (size_t i = 0; i < num_streams; ++i) {
        StreamID streamID = 1100 + i;
        stream_ids.push_back(streamID);
        
        configureTestStream(streamID);
        createStreamPASID(streamID, pasid);
        
        // Map unique pages for each stream
        for (size_t j = 0; j < translations_per_stream; ++j) {
            IOVA iova = base_iova + (i * translations_per_stream + j) * page_size;
            PA pa = base_pa + (i * translations_per_stream + j) * page_size;
            auto perms = createDefaultPermissions();
            mapStreamPage(streamID, pasid, iova, pa, perms);
        }
    }
    
    std::vector<std::thread> threads;
    std::atomic<size_t> successful_translations(0);
    std::atomic<size_t> failed_translations(0);
    
    auto worker = [&](size_t stream_index) {
        StreamID streamID = stream_ids[stream_index];
        
        for (size_t i = 0; i < translations_per_stream; ++i) {
            IOVA iova = base_iova + (stream_index * translations_per_stream + i) * page_size;
            PA expected_pa = base_pa + (stream_index * translations_per_stream + i) * page_size;
            
            auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
            
            if (result.isSuccess() && result.getValue().physicalAddress == expected_pa) {
                successful_translations.fetch_add(1);
            } else {
                failed_translations.fetch_add(1);
            }
        }
    };
    
    // Start concurrent access from all streams
    for (size_t i = 0; i < num_streams; ++i) {
        threads.emplace_back(worker, i);
    }
    
    // Wait for completion
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Verify isolation maintained under concurrent access
    size_t expected_total = num_streams * translations_per_stream;
    EXPECT_EQ(successful_translations.load(), expected_total);
    EXPECT_EQ(failed_translations.load(), 0);
}

// Test 7: Stream Invalidation Isolation
TEST_F(StreamIsolationTest, StreamInvalidationIsolation) {
    const StreamID stream1 = 1200;
    const StreamID stream2 = 1300;
    const PASID pasid = 1;
    
    configureTestStream(stream1);
    configureTestStream(stream2);
    
    createStreamPASID(stream1, pasid);
    createStreamPASID(stream2, pasid);
    
    // Map pages for both streams
    IOVA test_iova = base_iova + 0xa000;
    PA stream1_pa = base_pa + 0xa000;
    PA stream2_pa = base_pa + 0xb000;
    
    auto perms = createDefaultPermissions();
    mapStreamPage(stream1, pasid, test_iova, stream1_pa, perms);
    mapStreamPage(stream2, pasid, test_iova, stream2_pa, perms);
    
    // Perform initial translations to populate cache
    auto result1 = smmu->translate(stream1, pasid, test_iova, AccessType::Read);
    auto result2 = smmu->translate(stream2, pasid, test_iova, AccessType::Read);
    ASSERT_TRUE(result1.isSuccess());
    ASSERT_TRUE(result2.isSuccess());
    
    smmu->resetStatistics();
    
    // Invalidate cache for stream1 only
    smmu->invalidateStreamCache(stream1);
    
    // Next translation for stream1 should be cache miss
    result1 = smmu->translate(stream1, pasid, test_iova, AccessType::Read);
    ASSERT_TRUE(result1.isSuccess());
    
    // Next translation for stream2 should still be cache hit (not invalidated)
    result2 = smmu->translate(stream2, pasid, test_iova, AccessType::Read);
    ASSERT_TRUE(result2.isSuccess());
    
    auto stats = smmu->getCacheStatistics();
    EXPECT_EQ(stats.misses, 1) << "Only stream1 should have cache miss after invalidation";
    EXPECT_EQ(stats.hits, 1) << "Stream2 should still have cache hit";
}

// Test 8: Large-Scale Stream Isolation Stress Test
TEST_F(StreamIsolationTest, LargeScaleStreamIsolationStress) {
    const size_t num_streams = 100;
    const size_t pages_per_stream = 50;
    const PASID pasid = 1;
    
    std::vector<StreamID> stream_ids;
    std::uniform_int_distribution<IOVA> iova_dist(base_iova, base_iova + 0x1000000);
    std::uniform_int_distribution<size_t> stream_dist(0, num_streams - 1);
    
    // Setup streams with random mappings
    for (size_t i = 0; i < num_streams; ++i) {
        StreamID streamID = 1400 + i;
        stream_ids.push_back(streamID);
        
        configureTestStream(streamID);
        createStreamPASID(streamID, pasid);
        
        // Create sparse, random mappings for each stream
        std::set<IOVA> used_iovas;
        for (size_t j = 0; j < pages_per_stream; ++j) {
            IOVA iova;
            do {
                iova = (iova_dist(rng) / page_size) * page_size;  // Page-align
            } while (used_iovas.count(iova) > 0);
            used_iovas.insert(iova);
            
            PA pa = base_pa + (i * pages_per_stream + j) * page_size;
            auto perms = createDefaultPermissions();
            mapStreamPage(streamID, pasid, iova, pa, perms);
        }
    }
    
    // Perform random access pattern stress test
    const size_t num_random_accesses = 10000;
    std::atomic<size_t> isolation_violations(0);
    std::atomic<size_t> successful_accesses(0);
    
    auto stress_worker = [&]() {
        for (size_t i = 0; i < num_random_accesses / 4; ++i) {  // 4 threads
            size_t stream_idx = stream_dist(rng);
            StreamID streamID = stream_ids[stream_idx];
            
            // Generate random IOVA that might be mapped
            IOVA random_iova = (iova_dist(rng) / page_size) * page_size;
            
            auto result = smmu->translate(streamID, pasid, random_iova, AccessType::Read);
            
            if (result.isSuccess()) {
                // Verify PA is in expected range for this stream
                PA expected_pa_min = base_pa + (stream_idx * pages_per_stream) * page_size;
                PA expected_pa_max = base_pa + ((stream_idx + 1) * pages_per_stream) * page_size;
                
                if (result.getValue().physicalAddress >= expected_pa_min && 
                    result.getValue().physicalAddress < expected_pa_max) {
                    successful_accesses.fetch_add(1);
                } else {
                    isolation_violations.fetch_add(1);
                }
            }
        }
    };
    
    // Run stress test with multiple threads
    std::vector<std::thread> stress_threads;
    for (int i = 0; i < 4; ++i) {
        stress_threads.emplace_back(stress_worker);
    }
    
    for (auto& thread : stress_threads) {
        thread.join();
    }
    
    // Verify no isolation violations occurred
    EXPECT_EQ(isolation_violations.load(), 0) 
        << "Stream isolation violations detected under stress test";
    EXPECT_GT(successful_accesses.load(), 0) 
        << "Should have some successful accesses in stress test";
    
    std::cout << "Stress test completed: " << successful_accesses.load() 
              << " successful accesses, " << isolation_violations.load() 
              << " isolation violations" << std::endl;
}

// Test 9: Cross-Stream PASID Isolation
TEST_F(StreamIsolationTest, CrossStreamPASIDIsolation) {
    const StreamID stream1 = 1500;
    const StreamID stream2 = 1600;
    const PASID pasid1 = 1;
    const PASID pasid2 = 2;
    
    configureTestStream(stream1);
    configureTestStream(stream2);
    
    createStreamPASID(stream1, pasid1);
    createStreamPASID(stream1, pasid2);
    createStreamPASID(stream2, pasid1);
    createStreamPASID(stream2, pasid2);
    
    // Create mapping matrix: each stream+PASID combination gets unique PA
    IOVA test_iova = base_iova + 0xc000;
    PA pa_stream1_pasid1 = base_pa + 0xc000;
    PA pa_stream1_pasid2 = base_pa + 0xd000;
    PA pa_stream2_pasid1 = base_pa + 0xe000;
    PA pa_stream2_pasid2 = base_pa + 0xf000;
    
    auto perms = createDefaultPermissions();
    mapStreamPage(stream1, pasid1, test_iova, pa_stream1_pasid1, perms);
    mapStreamPage(stream1, pasid2, test_iova, pa_stream1_pasid2, perms);
    mapStreamPage(stream2, pasid1, test_iova, pa_stream2_pasid1, perms);
    mapStreamPage(stream2, pasid2, test_iova, pa_stream2_pasid2, perms);
    
    // Test all combinations
    auto result_s1_p1 = smmu->translate(stream1, pasid1, test_iova, AccessType::Read);
    auto result_s1_p2 = smmu->translate(stream1, pasid2, test_iova, AccessType::Read);
    auto result_s2_p1 = smmu->translate(stream2, pasid1, test_iova, AccessType::Read);
    auto result_s2_p2 = smmu->translate(stream2, pasid2, test_iova, AccessType::Read);
    
    ASSERT_TRUE(result_s1_p1.isSuccess());
    ASSERT_TRUE(result_s1_p2.isSuccess());
    ASSERT_TRUE(result_s2_p1.isSuccess());
    ASSERT_TRUE(result_s2_p2.isSuccess());
    
    // Verify each combination gets unique PA
    EXPECT_EQ(result_s1_p1.getValue().physicalAddress, pa_stream1_pasid1);
    EXPECT_EQ(result_s1_p2.getValue().physicalAddress, pa_stream1_pasid2);
    EXPECT_EQ(result_s2_p1.getValue().physicalAddress, pa_stream2_pasid1);
    EXPECT_EQ(result_s2_p2.getValue().physicalAddress, pa_stream2_pasid2);
    
    // Verify all are different
    std::set<PA> unique_pas = {
        result_s1_p1.getValue().physicalAddress,
        result_s1_p2.getValue().physicalAddress,
        result_s2_p1.getValue().physicalAddress,
        result_s2_p2.getValue().physicalAddress
    };
    EXPECT_EQ(unique_pas.size(), 4) << "All stream+PASID combinations should have unique PAs";
}

// Test 10: Stream Configuration Isolation
TEST_F(StreamIsolationTest, StreamConfigurationIsolation) {
    const StreamID stage1_stream = 1700;
    const StreamID stage2_stream = 1800;
    const StreamID both_stages_stream = 1900;
    const PASID pasid = 1;
    
    // Configure streams with different translation stages
    configureTestStream(stage1_stream, SecurityState::NonSecure, TranslationStage::Stage1Only);
    configureTestStream(stage2_stream, SecurityState::NonSecure, TranslationStage::Stage2Only);
    configureTestStream(both_stages_stream, SecurityState::NonSecure, TranslationStage::BothStages);
    
    createStreamPASID(stage1_stream, pasid);
    createStreamPASID(stage2_stream, pasid);
    createStreamPASID(both_stages_stream, pasid);
    
    IOVA test_iova = base_iova + 0x10000;
    
    // Map pages according to each stream's translation stage requirements
    auto perms = createDefaultPermissions();
    
    // Stage1Only: Direct IOVA -> PA mapping
    PA stage1_pa = base_pa + 0x10000;
    mapStreamPage(stage1_stream, pasid, test_iova, stage1_pa, perms);
    
    // Stage2Only: Need to map IPA -> PA (treating IOVA as IPA)
    PA stage2_pa = base_pa + 0x11000;
    mapStreamPage(stage2_stream, 0, test_iova, stage2_pa, perms);  // PASID 0 for Stage-2
    
    // BothStages: Need both IOVA -> IPA and IPA -> PA
    IPA intermediate_ipa = base_iova + 0x20000;
    PA both_stages_pa = base_pa + 0x12000;
    mapStreamPage(both_stages_stream, pasid, test_iova, intermediate_ipa, perms);  // Stage-1
    mapStreamPage(both_stages_stream, 0, intermediate_ipa, both_stages_pa, perms);  // Stage-2
    
    // Test translations - each should behave according to its configuration
    auto result_stage1 = smmu->translate(stage1_stream, pasid, test_iova, AccessType::Read);
    auto result_stage2 = smmu->translate(stage2_stream, pasid, test_iova, AccessType::Read);
    auto result_both = smmu->translate(both_stages_stream, pasid, test_iova, AccessType::Read);
    
    ASSERT_TRUE(result_stage1.isSuccess());
    ASSERT_TRUE(result_stage2.isSuccess());
    ASSERT_TRUE(result_both.isSuccess());
    
    // Verify different translation behaviors
    EXPECT_EQ(result_stage1.getValue().physicalAddress, stage1_pa);
    EXPECT_EQ(result_stage2.getValue().physicalAddress, stage2_pa);
    EXPECT_EQ(result_both.getValue().physicalAddress, both_stages_pa);
    
    EXPECT_EQ(result_stage1.getValue().translationStage, TranslationStage::Stage1Only);
    EXPECT_EQ(result_stage2.getValue().translationStage, TranslationStage::Stage2Only);
    EXPECT_EQ(result_both.getValue().translationStage, TranslationStage::BothStages);
}

} // namespace integration
} // namespace smmu