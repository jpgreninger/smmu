// ARM SMMU v3 Thread Safety Tests
// Copyright (c) 2024 John Greninger
//
// Comprehensive multi-threaded test scenarios to validate thread safety
// fixes made to TLBCache and StreamContext classes.

#include <gtest/gtest.h>
#include <thread>
#include <vector>
#include <chrono>
#include <atomic>
#include <random>
#include <memory>
#include <mutex>
#include <condition_variable>

#include "smmu/tlb_cache.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

class ThreadSafetyTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Create test objects
        tlbCache = std::unique_ptr<TLBCache>(new TLBCache(1024));
        streamContext = std::unique_ptr<StreamContext>(new StreamContext());
        
        // Set up basic configuration
        setupStreamContext();
        
        // Initialize test data
        initializeTestData();
        
        // Reset counters
        errorCount.store(0);
        totalOperations.store(0);
    }

    void TearDown() override {
        tlbCache.reset();
        streamContext.reset();
    }

    void setupStreamContext() {
        // Create address spaces for testing
        auto addressSpace1 = std::make_shared<AddressSpace>();
        auto addressSpace2 = std::make_shared<AddressSpace>();
        
        // Add some basic page mappings
        addressSpace1->mapPage(TEST_IOVA_BASE, TEST_PA_BASE, PagePermissions{true, true, false});
        addressSpace1->mapPage(TEST_IOVA_BASE + 0x1000, TEST_PA_BASE + 0x1000, PagePermissions{true, true, false});
        addressSpace1->mapPage(TEST_IOVA_BASE + 0x2000, TEST_PA_BASE + 0x2000, PagePermissions{true, false, false});
        
        addressSpace2->mapPage(TEST_IOVA_BASE, TEST_PA_BASE + 0x10000, PagePermissions{true, true, true});
        addressSpace2->mapPage(TEST_IOVA_BASE + 0x1000, TEST_PA_BASE + 0x11000, PagePermissions{true, true, true});
        
        // Add PASIDs to stream context
        streamContext->addPASID(TEST_PASID_1, addressSpace1);
        streamContext->addPASID(TEST_PASID_2, addressSpace2);
        
        // Enable stream
        streamContext->enableStream();
    }

    void initializeTestData() {
        // Initialize test entries for TLB cache
        testEntries.clear();
        for (size_t i = 0; i < NUM_TEST_ENTRIES; ++i) {
            TLBEntry entry;
            entry.streamID = TEST_STREAM_ID_BASE + (i % 4);
            entry.pasid = TEST_PASID_1 + (i % 2);
            entry.iova = TEST_IOVA_BASE + (i * 0x1000);
            entry.physicalAddress = TEST_PA_BASE + (i * 0x1000);
            entry.permissions = PagePermissions{true, (i % 2) == 0, (i % 3) == 0};
            entry.valid = true;
            entry.timestamp = i;
            testEntries.push_back(entry);
        }
    }

    // Test data
    std::unique_ptr<TLBCache> tlbCache;
    std::unique_ptr<StreamContext> streamContext;
    std::vector<TLBEntry> testEntries;
    
    // Test configuration
    static constexpr size_t NUM_THREADS = 16;
    static constexpr size_t NUM_TEST_ENTRIES = 100;
    static constexpr size_t OPERATIONS_PER_THREAD = 1000;
    static constexpr std::chrono::milliseconds TEST_DURATION{2000}; // 2 seconds
    
    // Test data constants
    static constexpr StreamID TEST_STREAM_ID_BASE = 0x1000;
    static constexpr PASID TEST_PASID_1 = 0x1;
    static constexpr PASID TEST_PASID_2 = 0x2;
    static constexpr IOVA TEST_IOVA_BASE = 0x10000000;
    static constexpr PA TEST_PA_BASE = 0x40000000;
    
    // Error tracking
    std::atomic<size_t> errorCount{0};
    std::atomic<size_t> totalOperations{0};
    
    // Synchronization primitives
    std::mutex startMutex;
    std::condition_variable startCondition;
    std::atomic<bool> startFlag{false};
    
    // Helper method to wait for all threads to be ready
    void waitForAllThreadsReady(size_t /*threadId*/, std::atomic<size_t>& readyCount) {
        readyCount.fetch_add(1);
        std::unique_lock<std::mutex> lock(startMutex);
        startCondition.wait(lock, [this] { return startFlag.load(); });
    }
    
    // Helper method to start all threads simultaneously
    void startAllThreads() {
        {
            std::lock_guard<std::mutex> lock(startMutex);
            startFlag.store(true);
        }
        startCondition.notify_all();
    }
};

// ============================================================================
// TLBCache Multi-threaded Tests
// ============================================================================

TEST_F(ThreadSafetyTest, TLBCache_ConcurrentLookupInsert) {
    const size_t numThreads = 8;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    
    // Test concurrent lookup/insert operations
    for (size_t i = 0; i < numThreads; ++i) {
        threads.emplace_back([this, i, &readyCount]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<size_t> dist(0, NUM_TEST_ENTRIES - 1);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                size_t idx = dist(rng);
                const auto& entry = testEntries[idx];
                
                // Alternate between lookup and insert operations
                if (totalOperations.load() % 2 == 0) {
                    // Lookup operation
                    TLBEntry* result = tlbCache->lookup(entry.streamID, entry.pasid, entry.iova);
                    // Result may be null if entry hasn't been inserted yet - this is expected
                    (void)result; // Suppress unused warning
                } else {
                    // Insert operation
                    tlbCache->insert(entry);
                }
                
                totalOperations.fetch_add(1);
                
                // Brief pause to allow other threads to interleave
                std::this_thread::sleep_for(std::chrono::microseconds(1));
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < numThreads) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate cache state integrity
    EXPECT_EQ(errorCount.load(), 0) << "Concurrent lookup/insert operations caused errors";
    EXPECT_GT(totalOperations.load(), 0) << "No operations were performed";
    
    // Validate that cache statistics are consistent using atomic snapshot
    auto stats = tlbCache->getAtomicStatistics();
    EXPECT_EQ(stats.hitCount + stats.missCount, stats.totalLookups) << "Cache statistics are inconsistent";
}

TEST_F(ThreadSafetyTest, TLBCache_ConcurrentInvalidation) {
    const size_t numReaderThreads = 6;
    const size_t numWriterThreads = 2;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    
    // Pre-populate cache with test entries
    for (const auto& entry : testEntries) {
        tlbCache->insert(entry);
    }
    
    // Reader threads performing lookups
    for (size_t i = 0; i < numReaderThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, numReaderThreads]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<size_t> dist(0, NUM_TEST_ENTRIES - 1);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                size_t idx = dist(rng);
                const auto& entry = testEntries[idx];
                
                TLBEntry* result = tlbCache->lookup(entry.streamID, entry.pasid, entry.iova);
                // Result may be null due to concurrent invalidation - this is expected
                (void)result; // Suppress unused warning
                
                totalOperations.fetch_add(1);
                std::this_thread::sleep_for(std::chrono::microseconds(1));
            }
        });
    }
    
    // Writer threads performing invalidations
    for (size_t i = 0; i < numWriterThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, numReaderThreads]() {
            waitForAllThreadsReady(i + numReaderThreads, readyCount);
            
            std::mt19937 rng(i + numReaderThreads + 1);
            std::uniform_int_distribution<size_t> dist(0, NUM_TEST_ENTRIES - 1);
            std::uniform_int_distribution<int> opDist(0, 3);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                size_t idx = dist(rng);
                const auto& entry = testEntries[idx];
                
                int op = opDist(rng);
                switch (op) {
                    case 0:
                        tlbCache->invalidate(entry.streamID, entry.pasid, entry.iova);
                        break;
                    case 1:
                        tlbCache->invalidateByStream(entry.streamID);
                        break;
                    case 2:
                        tlbCache->invalidateByPASID(entry.streamID, entry.pasid);
                        break;
                    case 3:
                        tlbCache->invalidateAll();
                        break;
                }
                
                totalOperations.fetch_add(1);
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < (numReaderThreads + numWriterThreads)) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate no errors occurred
    EXPECT_EQ(errorCount.load(), 0) << "Concurrent invalidation operations caused errors";
    EXPECT_GT(totalOperations.load(), 0) << "No operations were performed";
}

TEST_F(ThreadSafetyTest, TLBCache_StatisticsIntegrity) {
    const size_t numThreads = 10;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    std::atomic<size_t> insertCount{0};
    std::atomic<size_t> lookupCount{0};
    
    // Test statistics integrity under concurrent access
    for (size_t i = 0; i < numThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, &insertCount, &lookupCount]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<size_t> dist(0, NUM_TEST_ENTRIES - 1);
            
            for (size_t op = 0; op < OPERATIONS_PER_THREAD; ++op) {
                size_t idx = dist(rng);
                const auto& entry = testEntries[idx];
                
                if (op % 3 == 0) {
                    // Insert operation
                    tlbCache->insert(entry);
                    insertCount.fetch_add(1);
                } else {
                    // Lookup operation
                    TLBEntry* result = tlbCache->lookup(entry.streamID, entry.pasid, entry.iova);
                    (void)result; // Suppress unused warning
                    lookupCount.fetch_add(1);
                }
                
                totalOperations.fetch_add(1);
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < numThreads) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate statistics integrity using atomic snapshot
    auto stats = tlbCache->getAtomicStatistics();
    
    EXPECT_EQ(stats.hitCount + stats.missCount, stats.totalLookups) << "Hit + Miss count doesn't equal total lookups";
    EXPECT_EQ(stats.totalLookups, lookupCount.load()) << "Total lookups doesn't match expected lookup count";
    EXPECT_EQ(errorCount.load(), 0) << "Statistics integrity test caused errors";
}

// ============================================================================
// StreamContext Multi-threaded Tests
// ============================================================================

TEST_F(ThreadSafetyTest, StreamContext_ConcurrentTranslate) {
    const size_t numThreads = 12;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    std::atomic<size_t> successCount{0};
    std::atomic<size_t> faultCount{0};
    
    // Test concurrent translation operations
    for (size_t i = 0; i < numThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, &successCount, &faultCount]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<int> pasidDist(TEST_PASID_1, TEST_PASID_2);
            std::uniform_int_distribution<uint64_t> iovaDist(TEST_IOVA_BASE, TEST_IOVA_BASE + 0x10000);
            std::uniform_int_distribution<int> accessDist(0, 2);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                PASID pasid = pasidDist(rng);
                IOVA iova = iovaDist(rng) & ~0xFFFULL; // Align to page boundary
                AccessType access = static_cast<AccessType>(accessDist(rng));
                
                TranslationResult result = streamContext->translate(pasid, iova, access);
                
                if (result.isOk()) {
                    successCount.fetch_add(1);
                } else {
                    faultCount.fetch_add(1);
                }
                
                totalOperations.fetch_add(1);
                std::this_thread::sleep_for(std::chrono::microseconds(1));
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < numThreads) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate results
    EXPECT_EQ(errorCount.load(), 0) << "Concurrent translation operations caused errors";
    EXPECT_GT(totalOperations.load(), 0) << "No operations were performed";
    EXPECT_EQ(successCount.load() + faultCount.load(), totalOperations.load()) 
        << "Success + Fault count doesn't match total operations";
    EXPECT_GT(successCount.load(), 0) << "No successful translations occurred";
}

TEST_F(ThreadSafetyTest, StreamContext_ConcurrentPASIDManagement) {
    const size_t numReaderThreads = 8;
    const size_t numWriterThreads = 4;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    
    const PASID DYNAMIC_PASID_BASE = 0x100;
    const size_t NUM_DYNAMIC_PASIDS = 20;
    
    // Reader threads performing translations and queries
    for (size_t i = 0; i < numReaderThreads; ++i) {
        threads.emplace_back([this, i, &readyCount]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<PASID> pasidDist(DYNAMIC_PASID_BASE, DYNAMIC_PASID_BASE + NUM_DYNAMIC_PASIDS - 1);
            std::uniform_int_distribution<uint64_t> iovaDist(TEST_IOVA_BASE, TEST_IOVA_BASE + 0x5000);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                PASID pasid = pasidDist(rng);
                IOVA iova = iovaDist(rng) & ~0xFFFULL;
                
                // Try translation (may fail if PASID doesn't exist)
                TranslationResult result = streamContext->translate(pasid, iova, AccessType::Read);
                (void)result; // Suppress unused warning
                
                // Query PASID existence
                bool hasPasid = streamContext->hasPASID(pasid);
                (void)hasPasid; // Suppress unused warning
                
                totalOperations.fetch_add(2);
                std::this_thread::sleep_for(std::chrono::microseconds(1));
            }
        });
    }
    
    // Writer threads creating and removing PASIDs
    for (size_t i = 0; i < numWriterThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, numReaderThreads]() {
            waitForAllThreadsReady(i + numReaderThreads, readyCount);
            
            std::mt19937 rng(i + numReaderThreads + 1);
            std::uniform_int_distribution<PASID> pasidDist(DYNAMIC_PASID_BASE, DYNAMIC_PASID_BASE + NUM_DYNAMIC_PASIDS - 1);
            std::uniform_int_distribution<int> opDist(0, 1);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                PASID pasid = pasidDist(rng);
                
                if (opDist(rng) == 0) {
                    // Create PASID
                    streamContext->createPASID(pasid);
                } else {
                    // Remove PASID
                    streamContext->removePASID(pasid);
                }
                
                totalOperations.fetch_add(1);
                std::this_thread::sleep_for(std::chrono::milliseconds(5));
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < (numReaderThreads + numWriterThreads)) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate no errors occurred
    EXPECT_EQ(errorCount.load(), 0) << "Concurrent PASID management operations caused errors";
    EXPECT_GT(totalOperations.load(), 0) << "No operations were performed";
}

TEST_F(ThreadSafetyTest, StreamContext_ConfigurationUpdates) {
    const size_t numTranslationThreads = 6;
    const size_t numConfigThreads = 2;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    
    // Translation threads performing continuous translations
    for (size_t i = 0; i < numTranslationThreads; ++i) {
        threads.emplace_back([this, i, &readyCount]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<int> pasidDist(TEST_PASID_1, TEST_PASID_2);
            std::uniform_int_distribution<uint64_t> iovaDist(TEST_IOVA_BASE, TEST_IOVA_BASE + 0x3000);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                PASID pasid = pasidDist(rng);
                IOVA iova = iovaDist(rng) & ~0xFFFULL;
                
                TranslationResult result = streamContext->translate(pasid, iova, AccessType::Read);
                (void)result; // Suppress unused warning
                
                totalOperations.fetch_add(1);
                std::this_thread::sleep_for(std::chrono::microseconds(1));
            }
        });
    }
    
    // Configuration threads updating stream configuration
    for (size_t i = 0; i < numConfigThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, numTranslationThreads]() {
            waitForAllThreadsReady(i + numTranslationThreads, readyCount);
            
            std::mt19937 rng(i + numTranslationThreads + 1);
            std::uniform_int_distribution<int> opDist(0, 2);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                int op = opDist(rng);
                
                switch (op) {
                    case 0:
                        streamContext->setStage1Enabled(true);
                        break;
                    case 1:
                        streamContext->setStage2Enabled(false);
                        break;
                    case 2:
                        streamContext->setFaultMode(FaultMode::Stall);
                        break;
                }
                
                // Query stream state
                Result<bool> enabledResult = streamContext->isStreamEnabled();
                bool enabled = enabledResult.isOk() ? enabledResult.getValue() : false;
                bool stage1 = streamContext->isStage1Enabled();
                bool stage2 = streamContext->isStage2Enabled();
                (void)enabled; (void)stage1; (void)stage2; // Suppress unused warnings
                
                totalOperations.fetch_add(4);
                std::this_thread::sleep_for(std::chrono::milliseconds(20));
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < (numTranslationThreads + numConfigThreads)) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate no errors occurred
    EXPECT_EQ(errorCount.load(), 0) << "Concurrent configuration update operations caused errors";
    EXPECT_GT(totalOperations.load(), 0) << "No operations were performed";
}

// ============================================================================
// Combined Integration Tests
// ============================================================================

TEST_F(ThreadSafetyTest, Combined_SMSTranslationWithCaching) {
    const size_t numThreads = 12;
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    std::atomic<size_t> cacheHits{0};
    std::atomic<size_t> cacheMisses{0};
    std::atomic<size_t> translations{0};
    
    // Pre-populate some cache entries
    for (size_t i = 0; i < 10; ++i) {
        const auto& entry = testEntries[i];
        tlbCache->insert(entry);
    }
    
    // Test end-to-end multi-threaded scenarios
    for (size_t i = 0; i < numThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, &cacheHits, &cacheMisses, &translations]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<int> pasidDist(TEST_PASID_1, TEST_PASID_2);
            std::uniform_int_distribution<uint64_t> iovaDist(TEST_IOVA_BASE, TEST_IOVA_BASE + 0x5000);
            std::uniform_int_distribution<int> accessDist(0, 2);
            std::uniform_int_distribution<StreamID> streamDist(TEST_STREAM_ID_BASE, TEST_STREAM_ID_BASE + 3);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                PASID pasid = pasidDist(rng);
                IOVA iova = iovaDist(rng) & ~0xFFFULL;
                AccessType access = static_cast<AccessType>(accessDist(rng));
                StreamID streamID = streamDist(rng);
                
                // Check TLB cache first
                TLBEntry* cacheEntry = tlbCache->lookup(streamID, pasid, iova);
                if (cacheEntry) {
                    cacheHits.fetch_add(1);
                } else {
                    cacheMisses.fetch_add(1);
                    
                    // Perform translation through StreamContext
                    TranslationResult result = streamContext->translate(pasid, iova, access);
                    translations.fetch_add(1);
                    
                    // If translation successful, cache it
                    if (result.isOk()) {
                        TLBEntry newEntry;
                        newEntry.streamID = streamID;
                        newEntry.pasid = pasid;
                        newEntry.iova = iova;
                        newEntry.physicalAddress = result.getValue().physicalAddress;
                        newEntry.permissions = PagePermissions{true, true, false};
                        newEntry.valid = true;
                        newEntry.timestamp = i;
                        
                        tlbCache->insert(newEntry);
                    }
                }
                
                totalOperations.fetch_add(1);
                std::this_thread::sleep_for(std::chrono::microseconds(2));
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < numThreads) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate results
    EXPECT_EQ(errorCount.load(), 0) << "Combined integration test caused errors";
    EXPECT_GT(totalOperations.load(), 0) << "No operations were performed";
    EXPECT_GT(cacheHits.load() + cacheMisses.load(), 0) << "No cache operations occurred";
    EXPECT_GT(translations.load(), 0) << "No translations occurred";
    
    // Validate cache statistics consistency using atomic snapshot
    auto tlbStats = tlbCache->getAtomicStatistics();
    EXPECT_EQ(tlbStats.hitCount + tlbStats.missCount, tlbStats.totalLookups) << "TLB cache statistics inconsistent";
}

TEST_F(ThreadSafetyTest, StressTest_HighConcurrency) {
    const size_t numThreads = 16; // Maximum concurrency
    std::vector<std::thread> threads;
    std::atomic<size_t> readyCount{0};
    std::atomic<size_t> dataCorruptions{0};
    
    // High-concurrency stress test
    for (size_t i = 0; i < numThreads; ++i) {
        threads.emplace_back([this, i, &readyCount, &dataCorruptions]() {
            waitForAllThreadsReady(i, readyCount);
            
            std::mt19937 rng(i + 1);
            std::uniform_int_distribution<int> opDist(0, 9);
            std::uniform_int_distribution<int> pasidDist(TEST_PASID_1, TEST_PASID_2);
            std::uniform_int_distribution<uint64_t> iovaDist(TEST_IOVA_BASE, TEST_IOVA_BASE + 0x8000);
            std::uniform_int_distribution<size_t> entryDist(0, NUM_TEST_ENTRIES - 1);
            std::uniform_int_distribution<StreamID> streamDist(TEST_STREAM_ID_BASE, TEST_STREAM_ID_BASE + 7);
            
            auto endTime = std::chrono::steady_clock::now() + TEST_DURATION;
            
            while (std::chrono::steady_clock::now() < endTime) {
                int operation = opDist(rng);
                
                try {
                    switch (operation) {
                        case 0: case 1: case 2: {
                            // TLB Cache lookup (30%)
                            StreamID sid = streamDist(rng);
                            PASID pid = pasidDist(rng);
                            IOVA va = iovaDist(rng) & ~0xFFFULL;
                            TLBEntry* entry = tlbCache->lookup(sid, pid, va);
                            (void)entry; // Suppress unused warning
                            break;
                        }
                        case 3: case 4: {
                            // TLB Cache insert (20%)
                            size_t idx = entryDist(rng);
                            tlbCache->insert(testEntries[idx]);
                            break;
                        }
                        case 5: {
                            // TLB Cache invalidate (10%)
                            StreamID sid = streamDist(rng);
                            tlbCache->invalidateByStream(sid);
                            break;
                        }
                        case 6: case 7: {
                            // StreamContext translate (20%)
                            PASID pid = pasidDist(rng);
                            IOVA va = iovaDist(rng) & ~0xFFFULL;
                            TranslationResult result = streamContext->translate(pid, va, AccessType::Read);
                            (void)result; // Suppress unused warning
                            break;
                        }
                        case 8: {
                            // PASID management (10%)
                            PASID pid = pasidDist(rng) + 0x100;
                            if (rng() % 2 == 0) {
                                streamContext->createPASID(pid);
                            } else {
                                streamContext->removePASID(pid);
                            }
                            break;
                        }
                        case 9: {
                            // Statistics query (10%) - use atomic snapshot to avoid race conditions
                            auto stats = tlbCache->getAtomicStatistics();
                            
                            // Consistency check using atomic snapshot
                            if (stats.hitCount + stats.missCount != stats.totalLookups && stats.totalLookups > 0) {
                                dataCorruptions.fetch_add(1);
                            }
                            break;
                        }
                    }
                } catch (...) {
                    errorCount.fetch_add(1);
                }
                
                totalOperations.fetch_add(1);
            }
        });
    }
    
    // Wait for all threads to be ready
    while (readyCount.load() < numThreads) {
        std::this_thread::sleep_for(std::chrono::microseconds(10));
    }
    
    // Start all threads simultaneously
    startAllThreads();
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Validate no errors or data corruption occurred
    EXPECT_EQ(errorCount.load(), 0) << "High concurrency stress test caused exceptions";
    EXPECT_EQ(dataCorruptions.load(), 0) << "High concurrency stress test caused data corruption";
    EXPECT_GT(totalOperations.load(), numThreads * 100) << "Insufficient operations performed in stress test";
    
    // Final consistency check using atomic snapshot
    auto finalStats = tlbCache->getAtomicStatistics();
    EXPECT_EQ(finalStats.hitCount + finalStats.missCount, finalStats.totalLookups) 
        << "Final cache statistics are inconsistent";
}

} // namespace test
} // namespace smmu