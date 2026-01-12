// ARM SMMU v3 Large-Scale Scalability Integration Tests
// Copyright (c) 2024 John Greninger
// Task 8.2.4: Large-Scale Scalability Tests (6 hours)

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
#include <atomic>
#include <future>

namespace smmu {
namespace integration {

class LargeScaleScalabilityTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Use default SMMU configuration for scalability testing
        smmu = std::make_unique<SMMU>();
        
        // Test parameters
        page_size = 4096;
        base_iova = 0x100000000ULL;  // Use 64-bit address space
        base_pa = 0x200000000ULL;
        
        // Initialize high-quality random number generator
        rng.seed(std::chrono::steady_clock::now().time_since_epoch().count());
        
        // Performance thresholds
        max_translation_time_us = 10.0;      // 10 microseconds per translation
        max_context_switch_time_us = 1.0;    // 1 microsecond per context switch
        min_throughput_ops_per_second = 100000; // 100K operations per second
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper to configure a stream efficiently
    void configureStream(StreamID streamID, bool enableStage2 = false,
                        SecurityState securityState = SecurityState::NonSecure) {
        StreamConfig streamConfig;
        streamConfig.translationEnabled = true;
        streamConfig.stage1Enabled = true;
        streamConfig.stage2Enabled = enableStage2;
        streamConfig.faultMode = FaultMode::Terminate;

        auto result = smmu->configureStream(streamID, streamConfig);
        ASSERT_TRUE(result.isOk()) << "Failed to configure stream " << streamID;

        result = smmu->enableStream(streamID);
        ASSERT_TRUE(result.isOk()) << "Failed to enable stream " << streamID;
    }

    // Helper to create PASID and basic mappings
    void createPASIDWithMappings(StreamID streamID, PASID pasid, size_t num_pages = 100) {
        auto result = smmu->createStreamPASID(streamID, pasid);
        ASSERT_TRUE(result.isOk()) << "Failed to create PASID " << pasid
                                       << " for stream " << streamID;

        PagePermissions perms;
        perms.read = true;
        perms.write = true;
        perms.execute = false;
        
        uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
        uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
        
        for (size_t i = 0; i < num_pages; ++i) {
            IOVA iova = base_iova + stream_base + pasid_base + (i * page_size);
            PA pa = base_pa + stream_base + pasid_base + (i * page_size);
            
            result = smmu->mapPage(streamID, pasid, iova, pa, perms);
            ASSERT_TRUE(result.isOk()) << "Failed to map page " << i 
                                           << " for stream " << streamID << ", PASID " << pasid;
        }
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

    std::unique_ptr<SMMU> smmu;
    size_t page_size;
    IOVA base_iova;
    PA base_pa;
    std::mt19937 rng;
    
    // Performance thresholds
    double max_translation_time_us;
    double max_context_switch_time_us;
    double min_throughput_ops_per_second;
};

// Test 1: Large-Scale Stream Configuration
TEST_F(LargeScaleScalabilityTest, LargeScaleStreamConfiguration) {
    const size_t num_streams = 1000;  // Test with 1000 streams
    const size_t pasids_per_stream = 50;
    
    std::vector<StreamID> stream_ids;
    
    // Measure stream configuration performance
    auto start = std::chrono::high_resolution_clock::now();
    
    // Configure many streams
    for (size_t i = 1; i <= num_streams; ++i) {
        StreamID streamID = i;
        stream_ids.push_back(streamID);
        configureStream(streamID);
        
        // Create multiple PASIDs per stream
        for (size_t j = 1; j <= pasids_per_stream; ++j) {
            PASID pasid = j;
            auto result = smmu->createStreamPASID(streamID, pasid);
            ASSERT_TRUE(result.isOk()) << "Failed to create PASID " << pasid 
                                           << " for stream " << streamID;
        }
    }
    
    auto end = std::chrono::high_resolution_clock::now();
    auto setup_duration = std::chrono::duration_cast<std::chrono::milliseconds>(end - start);
    
    // Verify all streams are properly configured
    for (StreamID streamID : stream_ids) {
        auto result = smmu->isStreamConfigured(streamID);
        EXPECT_TRUE(result.isOk());
        EXPECT_TRUE(result.getValue());
        
        result = smmu->isStreamEnabled(streamID);
        EXPECT_TRUE(result.isOk());
        EXPECT_TRUE(result.getValue());
    }
    
    // Verify total counts
    size_t stream_count = smmu->getStreamCount();
    EXPECT_EQ(stream_count, num_streams);
    
    std::cout << "Large-scale configuration: " << num_streams << " streams with " 
              << pasids_per_stream << " PASIDs each in " << setup_duration.count() 
              << " ms" << std::endl;
    
    // Performance target: Should complete within reasonable time
    EXPECT_LT(setup_duration.count(), 30000) << "Stream configuration took too long";
}

// Test 2: Massive Translation Load Testing
TEST_F(LargeScaleScalabilityTest, MassiveTranslationLoad) {
    const size_t num_streams = 100;
    const size_t pasids_per_stream = 20;
    const size_t pages_per_pasid = 100;
    const size_t translations_per_combo = 50;
    
    // Setup large-scale mapping infrastructure
    std::vector<StreamID> stream_ids;
    
    for (size_t i = 1; i <= num_streams; ++i) {
        StreamID streamID = i;
        stream_ids.push_back(streamID);
        configureStream(streamID);
        
        for (size_t j = 1; j <= pasids_per_stream; ++j) {
            PASID pasid = j;
            createPASIDWithMappings(streamID, pasid, pages_per_pasid);
        }
    }
    
    // Calculate total expected operations
    size_t total_combinations = num_streams * pasids_per_stream * pages_per_pasid;
    size_t total_translations = total_combinations * translations_per_combo;
    
    std::cout << "Massive load test: " << total_combinations << " unique mappings, " 
              << total_translations << " total translations" << std::endl;
    
    // Reset statistics for accurate measurement
    smmu->resetStatistics();
    
    // Perform massive translation load
    std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
    std::uniform_int_distribution<PASID> pasid_dist(1, pasids_per_stream);
    std::uniform_int_distribution<size_t> page_dist(0, pages_per_pasid - 1);
    
    auto start = std::chrono::high_resolution_clock::now();
    
    size_t successful_translations = 0;
    size_t failed_translations = 0;
    
    for (size_t i = 0; i < total_translations; ++i) {
        StreamID streamID = stream_ids[stream_dist(rng)];
        PASID pasid = pasid_dist(rng);
        size_t page_index = page_dist(rng);
        
        uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
        uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
        IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
        
        auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
        
        if (result.isOk()) {
            successful_translations++;
        } else {
            failed_translations++;
        }
    }
    
    auto end = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
    
    // Calculate performance metrics
    double total_time_seconds = static_cast<double>(duration.count()) / 1000000.0;
    double throughput = static_cast<double>(total_translations) / total_time_seconds;
    double avg_translation_time = static_cast<double>(duration.count()) / total_translations;
    
    // Verify correctness
    EXPECT_EQ(successful_translations, total_translations) 
        << "All translations should succeed in massive load test";
    EXPECT_EQ(failed_translations, 0);
    
    // Performance validation
    EXPECT_LT(avg_translation_time, max_translation_time_us) 
        << "Average translation time too slow: " << avg_translation_time << " microseconds";
    
    EXPECT_GT(throughput, min_throughput_ops_per_second) 
        << "Throughput too low: " << throughput << " ops/sec";
    
    // Check cache efficiency
    auto stats = smmu->getCacheStatistics();
    double cache_hit_rate = static_cast<double>(stats.hitCount) / (stats.hitCount + stats.missCount);
    EXPECT_GT(cache_hit_rate, 0.8) << "Cache hit rate should be >80% for good performance";
    
    std::cout << "Massive load results:" << std::endl;
    std::cout << "  Total translations: " << total_translations << std::endl;
    std::cout << "  Total time: " << total_time_seconds << " seconds" << std::endl;
    std::cout << "  Throughput: " << throughput << " translations/sec" << std::endl;
    std::cout << "  Avg time per translation: " << avg_translation_time << " microseconds" << std::endl;
    std::cout << "  Cache hit rate: " << (cache_hit_rate * 100.0) << "%" << std::endl;
}

// Test 3: Concurrent High-Load Scalability
TEST_F(LargeScaleScalabilityTest, ConcurrentHighLoadScalability) {
    const size_t num_threads = 16;
    const size_t streams_per_thread = 50;
    const size_t operations_per_thread = 10000;
    
    // Setup concurrent infrastructure
    std::vector<std::vector<StreamID>> thread_streams(num_threads);
    
    for (size_t thread_id = 0; thread_id < num_threads; ++thread_id) {
        for (size_t i = 0; i < streams_per_thread; ++i) {
            StreamID streamID = (thread_id * streams_per_thread) + i + 1;
            thread_streams[thread_id].push_back(streamID);
            
            configureStream(streamID);
            createPASIDWithMappings(streamID, 1, 50);  // Single PASID with 50 pages
        }
    }
    
    std::vector<std::thread> threads;
    std::atomic<size_t> total_successful(0);
    std::atomic<size_t> total_failed(0);
    std::atomic<size_t> total_operations(0);
    
    auto worker = [&](size_t thread_id) {
        std::mt19937 local_rng(thread_id * 12345);
        std::uniform_int_distribution<size_t> stream_dist(0, streams_per_thread - 1);
        std::uniform_int_distribution<size_t> page_dist(0, 49);
        
        size_t local_successful = 0;
        size_t local_failed = 0;
        
        auto thread_start = std::chrono::high_resolution_clock::now();
        
        for (size_t i = 0; i < operations_per_thread; ++i) {
            StreamID streamID = thread_streams[thread_id][stream_dist(local_rng)];
            size_t page_index = page_dist(local_rng);
            
            uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
            uint64_t pasid_base = static_cast<uint64_t>(1) * 0x1000000ULL;
            IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
            
            auto result = smmu->translate(streamID, 1, iova, AccessType::Read);
            
            if (result.isOk()) {
                local_successful++;
            } else {
                local_failed++;
            }
        }
        
        auto thread_end = std::chrono::high_resolution_clock::now();
        auto thread_duration = std::chrono::duration_cast<std::chrono::microseconds>(thread_end - thread_start);
        
        total_successful.fetch_add(local_successful);
        total_failed.fetch_add(local_failed);
        total_operations.fetch_add(operations_per_thread);
        
        double thread_throughput = static_cast<double>(operations_per_thread) / 
                                 (static_cast<double>(thread_duration.count()) / 1000000.0);
        
        std::cout << "Thread " << thread_id << " throughput: " << thread_throughput 
                  << " ops/sec" << std::endl;
    };
    
    // Start concurrent load test
    auto start = std::chrono::high_resolution_clock::now();
    
    for (size_t i = 0; i < num_threads; ++i) {
        threads.emplace_back(worker, i);
    }
    
    for (auto& thread : threads) {
        thread.join();
    }
    
    auto end = std::chrono::high_resolution_clock::now();
    auto total_duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
    
    // Calculate overall performance
    double total_time_seconds = static_cast<double>(total_duration.count()) / 1000000.0;
    double overall_throughput = static_cast<double>(total_operations.load()) / total_time_seconds;
    
    // Verify correctness under high concurrency
    size_t expected_operations = num_threads * operations_per_thread;
    EXPECT_EQ(total_operations.load(), expected_operations);
    EXPECT_EQ(total_successful.load(), expected_operations) 
        << "All operations should succeed under concurrent load";
    EXPECT_EQ(total_failed.load(), 0);
    
    // Performance validation
    EXPECT_GT(overall_throughput, min_throughput_ops_per_second * num_threads * 0.7) 
        << "Concurrent throughput should scale reasonably with thread count";
    
    std::cout << "Concurrent high-load results:" << std::endl;
    std::cout << "  Threads: " << num_threads << std::endl;
    std::cout << "  Total operations: " << total_operations.load() << std::endl;
    std::cout << "  Total time: " << total_time_seconds << " seconds" << std::endl;
    std::cout << "  Overall throughput: " << overall_throughput << " ops/sec" << std::endl;
    std::cout << "  Successful: " << total_successful.load() << std::endl;
    std::cout << "  Failed: " << total_failed.load() << std::endl;
}

// Test 4: Memory Scalability Under Load
TEST_F(LargeScaleScalabilityTest, MemoryScalabilityUnderLoad) {
    const size_t num_streams = 200;
    const size_t pasids_per_stream = 100;
    const size_t pages_per_pasid = 1000;  // Large memory footprint
    
    // Track memory usage pattern (simplified)
    std::vector<StreamID> stream_ids;
    
    // Phase 1: Gradual buildup to test memory scaling
    for (size_t batch = 1; batch <= 10; ++batch) {
        size_t streams_in_batch = num_streams / 10;
        
        auto batch_start = std::chrono::high_resolution_clock::now();
        
        for (size_t i = 0; i < streams_in_batch; ++i) {
            StreamID streamID = ((batch - 1) * streams_in_batch) + i + 1;
            stream_ids.push_back(streamID);
            
            configureStream(streamID);
            
            for (size_t j = 1; j <= pasids_per_stream; ++j) {
                PASID pasid = j;
                createPASIDWithMappings(streamID, pasid, pages_per_pasid);
            }
        }
        
        auto batch_end = std::chrono::high_resolution_clock::now();
        auto batch_duration = std::chrono::duration_cast<std::chrono::milliseconds>(batch_end - batch_start);
        
        // Test performance after each batch
        const size_t test_operations = 1000;
        std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
        std::uniform_int_distribution<PASID> pasid_dist(1, pasids_per_stream);
        std::uniform_int_distribution<size_t> page_dist(0, pages_per_pasid - 1);
        
        auto perf_start = std::chrono::high_resolution_clock::now();
        
        for (size_t k = 0; k < test_operations; ++k) {
            StreamID streamID = stream_ids[stream_dist(rng)];
            PASID pasid = pasid_dist(rng);
            size_t page_index = page_dist(rng);
            
            uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
            uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
            IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
            
            auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
            EXPECT_TRUE(result.isOk()) << "Translation should succeed in batch " << batch;
        }
        
        auto perf_end = std::chrono::high_resolution_clock::now();
        auto perf_duration = std::chrono::duration_cast<std::chrono::microseconds>(perf_end - perf_start);
        
        double avg_translation_time = static_cast<double>(perf_duration.count()) / test_operations;
        
        std::cout << "Batch " << batch << ": " << stream_ids.size() << " streams, "
                  << "setup " << batch_duration.count() << " ms, "
                  << "avg translation " << avg_translation_time << " μs" << std::endl;
        
        // Performance should not degrade significantly with scale
        EXPECT_LT(avg_translation_time, max_translation_time_us * 2.0) 
            << "Translation performance degraded too much at batch " << batch;
    }
    
    // Final verification: Large-scale random access
    const size_t final_test_size = 50000;
    std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
    std::uniform_int_distribution<PASID> pasid_dist(1, pasids_per_stream);
    std::uniform_int_distribution<size_t> page_dist(0, pages_per_pasid - 1);
    
    auto final_start = std::chrono::high_resolution_clock::now();
    
    size_t final_successful = 0;
    for (size_t i = 0; i < final_test_size; ++i) {
        StreamID streamID = stream_ids[stream_dist(rng)];
        PASID pasid = pasid_dist(rng);
        size_t page_index = page_dist(rng);
        
        uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
        uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
        IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
        
        auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
        if (result.isOk()) {
            final_successful++;
        }
    }
    
    auto final_end = std::chrono::high_resolution_clock::now();
    auto final_duration = std::chrono::duration_cast<std::chrono::microseconds>(final_end - final_start);
    
    double final_avg_time = static_cast<double>(final_duration.count()) / final_test_size;
    
    EXPECT_EQ(final_successful, final_test_size) 
        << "All translations should succeed in final memory scalability test";
    
    EXPECT_LT(final_avg_time, max_translation_time_us * 1.5) 
        << "Final performance should remain reasonable";
    
    std::cout << "Memory scalability final test: " << final_successful << "/" << final_test_size 
              << " successful, avg time " << final_avg_time << " μs" << std::endl;
}

// Test 5: Cache Scalability and Efficiency
TEST_F(LargeScaleScalabilityTest, CacheScalabilityAndEfficiency) {
    const size_t num_streams = 100;
    const size_t pasids_per_stream = 50;
    const size_t pages_per_pasid = 200;
    
    // Setup large working set
    std::vector<StreamID> stream_ids;
    
    for (size_t i = 1; i <= num_streams; ++i) {
        StreamID streamID = i;
        stream_ids.push_back(streamID);
        configureStream(streamID);
        
        for (size_t j = 1; j <= pasids_per_stream; ++j) {
            PASID pasid = j;
            createPASIDWithMappings(streamID, pasid, pages_per_pasid);
        }
    }
    
    // Phase 1: Sequential access pattern (high cache hit rate expected)
    smmu->resetStatistics();
    
    const size_t sequential_operations = 20000;
    for (size_t i = 0; i < sequential_operations; ++i) {
        StreamID streamID = stream_ids[i % stream_ids.size()];
        PASID pasid = (i % pasids_per_stream) + 1;
        size_t page_index = i % pages_per_pasid;
        
        uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
        uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
        IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
        
        auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
    }
    
    auto sequential_stats = smmu->getCacheStatistics();
    double sequential_hit_rate = static_cast<double>(sequential_stats.hitCount) / 
                                (sequential_stats.hitCount + sequential_stats.missCount);
    
    std::cout << "Sequential access: " << sequential_stats.hitCount << " hits, " 
              << sequential_stats.missCount << " misses, " 
              << (sequential_hit_rate * 100.0) << "% hit rate" << std::endl;
    
    // Phase 2: Random access pattern (lower cache hit rate expected)
    smmu->invalidateTranslationCache();  // Clear cache
    smmu->resetStatistics();
    
    std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
    std::uniform_int_distribution<PASID> pasid_dist(1, pasids_per_stream);
    std::uniform_int_distribution<size_t> page_dist(0, pages_per_pasid - 1);
    
    const size_t random_operations = 20000;
    for (size_t i = 0; i < random_operations; ++i) {
        StreamID streamID = stream_ids[stream_dist(rng)];
        PASID pasid = pasid_dist(rng);
        size_t page_index = page_dist(rng);
        
        uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
        uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
        IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
        
        auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
    }
    
    auto random_stats = smmu->getCacheStatistics();
    double random_hit_rate = static_cast<double>(random_stats.hitCount) / 
                            (random_stats.hitCount + random_stats.missCount);
    
    std::cout << "Random access: " << random_stats.hitCount << " hits, " 
              << random_stats.missCount << " misses, " 
              << (random_hit_rate * 100.0) << "% hit rate" << std::endl;
    
    // Phase 3: Locality-based access pattern (medium cache hit rate)
    smmu->invalidateTranslationCache();
    smmu->resetStatistics();
    
    const size_t locality_operations = 20000;
    const size_t locality_window = 10;  // Access within small window
    
    for (size_t i = 0; i < locality_operations; ++i) {
        size_t base_index = i / locality_window;
        StreamID streamID = stream_ids[base_index % stream_ids.size()];
        PASID pasid = (base_index % pasids_per_stream) + 1;
        size_t page_index = (base_index + (i % locality_window)) % pages_per_pasid;
        
        uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
        uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
        IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
        
        auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
    }
    
    auto locality_stats = smmu->getCacheStatistics();
    double locality_hit_rate = static_cast<double>(locality_stats.hitCount) / 
                              (locality_stats.hitCount + locality_stats.missCount);
    
    std::cout << "Locality access: " << locality_stats.hitCount << " hits, " 
              << locality_stats.missCount << " misses, " 
              << (locality_hit_rate * 100.0) << "% hit rate" << std::endl;
    
    // Verify cache behavior is reasonable
    EXPECT_GT(sequential_hit_rate, 0.9) << "Sequential access should have >90% hit rate";
    EXPECT_GT(locality_hit_rate, 0.6) << "Locality access should have >60% hit rate";
    EXPECT_GT(locality_hit_rate, random_hit_rate) << "Locality should outperform random access";
}

// Test 6: Stress Testing with Mixed Workloads
TEST_F(LargeScaleScalabilityTest, MixedWorkloadStressTesting) {
    const size_t num_streams = 200;
    const size_t test_duration_seconds = 30;
    
    // Setup mixed workload infrastructure
    std::vector<StreamID> stream_ids;
    
    for (size_t i = 1; i <= num_streams; ++i) {
        StreamID streamID = i;
        stream_ids.push_back(streamID);
        configureStream(streamID);
        
        // Create varied PASID counts per stream
        size_t pasids_for_stream = 10 + (i % 50);  // 10-59 PASIDs per stream
        for (size_t j = 1; j <= pasids_for_stream; ++j) {
            PASID pasid = j;
            createPASIDWithMappings(streamID, pasid, 50 + (j % 100));  // 50-149 pages per PASID
        }
    }
    
    // Mixed workload: concurrent readers, writers, and management operations
    std::atomic<bool> stop_test(false);
    std::atomic<size_t> read_operations(0);
    std::atomic<size_t> write_operations(0);
    std::atomic<size_t> mgmt_operations(0);
    std::atomic<size_t> total_errors(0);
    
    auto reader_worker = [&]() {
        std::hash<std::thread::id> hasher;
        std::mt19937 local_rng(hasher(std::this_thread::get_id()));
        std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
        
        while (!stop_test.load()) {
            StreamID streamID = stream_ids[stream_dist(local_rng)];
            PASID pasid = 1 + (local_rng() % 50);  // Random PASID
            size_t page_index = local_rng() % 100;
            
            uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
            uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
            IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
            
            auto result = smmu->translate(streamID, pasid, iova, AccessType::Read);
            if (result.isOk()) {
                read_operations.fetch_add(1);
            } else {
                total_errors.fetch_add(1);
            }
        }
    };
    
    auto writer_worker = [&]() {
        std::hash<std::thread::id> hasher;
        std::mt19937 local_rng(hasher(std::this_thread::get_id()));
        std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
        
        while (!stop_test.load()) {
            StreamID streamID = stream_ids[stream_dist(local_rng)];
            PASID pasid = 1 + (local_rng() % 50);
            size_t page_index = local_rng() % 100;
            
            uint64_t stream_base = static_cast<uint64_t>(streamID) * 0x10000000ULL;
            uint64_t pasid_base = static_cast<uint64_t>(pasid) * 0x1000000ULL;
            IOVA iova = base_iova + stream_base + pasid_base + (page_index * page_size);
            
            auto result = smmu->translate(streamID, pasid, iova, AccessType::Write);
            if (result.isOk()) {
                write_operations.fetch_add(1);
            } else {
                total_errors.fetch_add(1);
            }
        }
    };
    
    auto mgmt_worker = [&]() {
        std::hash<std::thread::id> hasher;
        std::mt19937 local_rng(hasher(std::this_thread::get_id()));
        std::uniform_int_distribution<size_t> stream_dist(0, stream_ids.size() - 1);
        
        while (!stop_test.load()) {
            StreamID streamID = stream_ids[stream_dist(local_rng)];
            
            // Perform cache management operations
            if (local_rng() % 10 == 0) {
                smmu->invalidateStreamCache(streamID);
                mgmt_operations.fetch_add(1);
            }
            
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    };
    
    // Start mixed workload
    std::vector<std::thread> workers;
    
    // Start multiple reader and writer threads
    for (int i = 0; i < 8; ++i) {
        workers.emplace_back(reader_worker);
    }
    for (int i = 0; i < 4; ++i) {
        workers.emplace_back(writer_worker);
    }
    workers.emplace_back(mgmt_worker);
    
    // Run stress test for specified duration
    std::this_thread::sleep_for(std::chrono::seconds(test_duration_seconds));
    
    // Stop all workers
    stop_test.store(true);
    
    for (auto& worker : workers) {
        worker.join();
    }
    
    // Calculate results
    size_t total_operations = read_operations.load() + write_operations.load();
    double operations_per_second = static_cast<double>(total_operations) / test_duration_seconds;
    double error_rate = static_cast<double>(total_errors.load()) / total_operations;
    
    std::cout << "Mixed workload stress test results:" << std::endl;
    std::cout << "  Duration: " << test_duration_seconds << " seconds" << std::endl;
    std::cout << "  Read operations: " << read_operations.load() << std::endl;
    std::cout << "  Write operations: " << write_operations.load() << std::endl;
    std::cout << "  Management operations: " << mgmt_operations.load() << std::endl;
    std::cout << "  Total errors: " << total_errors.load() << std::endl;
    std::cout << "  Operations per second: " << operations_per_second << std::endl;
    std::cout << "  Error rate: " << (error_rate * 100.0) << "%" << std::endl;
    
    // Verify stress test results
    EXPECT_GT(total_operations, 0) << "Should have performed operations during stress test";
    EXPECT_LT(error_rate, 0.01) << "Error rate should be <1% during stress test";
    EXPECT_GT(operations_per_second, min_throughput_ops_per_second * 0.5) 
        << "Should maintain reasonable throughput under stress";
}

} // namespace integration
} // namespace smmu