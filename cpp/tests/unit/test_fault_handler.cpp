// ARM SMMU v3 FaultHandler Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/fault_handler.h"
#include "smmu/types.h"
#include <thread>
#include <vector>
#include <atomic>

namespace smmu {
namespace test {

class FaultHandlerTest : public ::testing::Test {
protected:
    void SetUp() override {
        faultHandler = std::unique_ptr<FaultHandler>(new FaultHandler());
    }

    void TearDown() override {
        faultHandler.reset();
    }

    std::unique_ptr<FaultHandler> faultHandler;
    
    // Test helper constants
    static constexpr StreamID TEST_STREAM_ID = 0x1000;
    static constexpr PASID TEST_PASID = 0x1;
    static constexpr IOVA TEST_IOVA = 0x10000000;
    
    // Helper function to create a fault record
    FaultRecord createTestFault(FaultType type, AccessType access, uint64_t timestamp = 12345) {
        FaultRecord fault;
        fault.streamID = TEST_STREAM_ID;
        fault.pasid = TEST_PASID;
        fault.address = TEST_IOVA;
        fault.faultType = type;
        fault.accessType = access;
        fault.timestamp = timestamp;
        return fault;
    }
};

// Test default construction
TEST_F(FaultHandlerTest, DefaultConstruction) {
    ASSERT_NE(faultHandler, nullptr);
    
    // Initially no faults
    EXPECT_EQ(faultHandler->getFaultCount(), 0);
    EXPECT_TRUE(faultHandler->getFaults().empty());
}

// Test single fault recording
TEST_F(FaultHandlerTest, SingleFaultRecording) {
    FaultRecord fault = createTestFault(FaultType::TranslationFault, AccessType::Read);
    
    faultHandler->recordFault(fault);
    
    EXPECT_EQ(faultHandler->getFaultCount(), 1);
    
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 1);
    
    const FaultRecord& recordedFault = faults[0];
    EXPECT_EQ(recordedFault.streamID, TEST_STREAM_ID);
    EXPECT_EQ(recordedFault.pasid, TEST_PASID);
    EXPECT_EQ(recordedFault.address, TEST_IOVA);
    EXPECT_EQ(recordedFault.faultType, FaultType::TranslationFault);
    EXPECT_EQ(recordedFault.accessType, AccessType::Read);
}

// Test multiple fault recording
TEST_F(FaultHandlerTest, MultipleFaultRecording) {
    FaultRecord fault1 = createTestFault(FaultType::TranslationFault, AccessType::Read, 100);
    FaultRecord fault2 = createTestFault(FaultType::PermissionFault, AccessType::Write, 200);
    FaultRecord fault3 = createTestFault(FaultType::AddressSizeFault, AccessType::Execute, 300);
    
    faultHandler->recordFault(fault1);
    faultHandler->recordFault(fault2);
    faultHandler->recordFault(fault3);
    
    EXPECT_EQ(faultHandler->getFaultCount(), 3);
    
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 3);
    
    // Verify faults are stored in order
    EXPECT_EQ(faults[0].faultType, FaultType::TranslationFault);
    EXPECT_EQ(faults[0].timestamp, 100);
    
    EXPECT_EQ(faults[1].faultType, FaultType::PermissionFault);
    EXPECT_EQ(faults[1].timestamp, 200);
    
    EXPECT_EQ(faults[2].faultType, FaultType::AddressSizeFault);
    EXPECT_EQ(faults[2].timestamp, 300);
}

// Test different fault types
TEST_F(FaultHandlerTest, DifferentFaultTypes) {
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Read));
    faultHandler->recordFault(createTestFault(FaultType::PermissionFault, AccessType::Write));
    faultHandler->recordFault(createTestFault(FaultType::AddressSizeFault, AccessType::Execute));
    faultHandler->recordFault(createTestFault(FaultType::AccessFault, AccessType::Read));
    
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 4);
    
    // Verify all fault types are recorded correctly
    std::set<FaultType> expectedTypes = {
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::AddressSizeFault,
        FaultType::AccessFault
    };
    
    std::set<FaultType> actualTypes;
    for (const auto& fault : faults) {
        actualTypes.insert(fault.faultType);
    }
    
    EXPECT_EQ(actualTypes, expectedTypes);
}

// Test different access types
TEST_F(FaultHandlerTest, DifferentAccessTypes) {
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Read));
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Write));
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Execute));
    
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 3);
    
    // Verify all access types are recorded correctly
    std::set<AccessType> expectedTypes = {
        AccessType::Read,
        AccessType::Write,
        AccessType::Execute
    };
    
    std::set<AccessType> actualTypes;
    for (const auto& fault : faults) {
        actualTypes.insert(fault.accessType);
    }
    
    EXPECT_EQ(actualTypes, expectedTypes);
}

// Test fault clearing
TEST_F(FaultHandlerTest, FaultClearing) {
    // Add some faults
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Read));
    faultHandler->recordFault(createTestFault(FaultType::PermissionFault, AccessType::Write));
    
    EXPECT_EQ(faultHandler->getFaultCount(), 2);
    
    // Clear faults
    faultHandler->clearFaults();
    
    EXPECT_EQ(faultHandler->getFaultCount(), 0);
    EXPECT_TRUE(faultHandler->getFaults().empty());
}

// Test fault statistics
TEST_F(FaultHandlerTest, FaultStatistics) {
    // Add various faults
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Read));
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Write));
    faultHandler->recordFault(createTestFault(FaultType::PermissionFault, AccessType::Read));
    faultHandler->recordFault(createTestFault(FaultType::AddressSizeFault, AccessType::Execute));
    
    // Test fault count by type
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::TranslationFault), 2);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::PermissionFault), 1);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::AddressSizeFault), 1);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::AccessFault), 0);
    
    // Test fault count by access type
    EXPECT_EQ(faultHandler->getFaultCountByAccessType(AccessType::Read), 2);
    EXPECT_EQ(faultHandler->getFaultCountByAccessType(AccessType::Write), 1);
    EXPECT_EQ(faultHandler->getFaultCountByAccessType(AccessType::Execute), 1);
}

// Test fault filtering by stream ID
TEST_F(FaultHandlerTest, FaultFilteringByStream) {
    FaultRecord fault1 = createTestFault(FaultType::TranslationFault, AccessType::Read);
    fault1.streamID = 0x1000;
    
    FaultRecord fault2 = createTestFault(FaultType::PermissionFault, AccessType::Write);
    fault2.streamID = 0x2000;
    
    FaultRecord fault3 = createTestFault(FaultType::AddressSizeFault, AccessType::Execute);
    fault3.streamID = 0x1000;
    
    faultHandler->recordFault(fault1);
    faultHandler->recordFault(fault2);
    faultHandler->recordFault(fault3);
    
    // Get faults for specific stream
    std::vector<FaultRecord> stream1Faults = faultHandler->getFaultsByStream(0x1000);
    std::vector<FaultRecord> stream2Faults = faultHandler->getFaultsByStream(0x2000);
    
    EXPECT_EQ(stream1Faults.size(), 2);
    EXPECT_EQ(stream2Faults.size(), 1);
    
    // Verify correct faults for stream 0x1000
    EXPECT_EQ(stream1Faults[0].faultType, FaultType::TranslationFault);
    EXPECT_EQ(stream1Faults[1].faultType, FaultType::AddressSizeFault);
    
    // Verify correct fault for stream 0x2000
    EXPECT_EQ(stream2Faults[0].faultType, FaultType::PermissionFault);
}

// Test fault filtering by PASID
TEST_F(FaultHandlerTest, FaultFilteringByPASID) {
    FaultRecord fault1 = createTestFault(FaultType::TranslationFault, AccessType::Read);
    fault1.pasid = 0x1;
    
    FaultRecord fault2 = createTestFault(FaultType::PermissionFault, AccessType::Write);
    fault2.pasid = 0x2;
    
    FaultRecord fault3 = createTestFault(FaultType::AddressSizeFault, AccessType::Execute);
    fault3.pasid = 0x1;
    
    faultHandler->recordFault(fault1);
    faultHandler->recordFault(fault2);
    faultHandler->recordFault(fault3);
    
    // Get faults for specific PASID
    std::vector<FaultRecord> pasid1Faults = faultHandler->getFaultsByPASID(0x1);
    std::vector<FaultRecord> pasid2Faults = faultHandler->getFaultsByPASID(0x2);
    
    EXPECT_EQ(pasid1Faults.size(), 2);
    EXPECT_EQ(pasid2Faults.size(), 1);
}

// Test fault limit and overflow handling
TEST_F(FaultHandlerTest, FaultLimitHandling) {
    // Set a small fault limit for testing
    const size_t maxFaults = 10;
    faultHandler->setMaxFaults(maxFaults);
    
    // Add more faults than the limit
    for (size_t i = 0; i < maxFaults + 5; ++i) {
        FaultRecord fault = createTestFault(FaultType::TranslationFault, AccessType::Read, i);
        fault.address = TEST_IOVA + i * PAGE_SIZE;
        faultHandler->recordFault(fault);
    }
    
    // Should not exceed the limit
    EXPECT_LE(faultHandler->getFaultCount(), maxFaults);
    
    // Verify we have the most recent faults
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_LE(faults.size(), maxFaults);
    
    if (!faults.empty()) {
        // The last fault should have the highest timestamp
        uint64_t maxTimestamp = 0;
        for (const auto& fault : faults) {
            maxTimestamp = std::max(maxTimestamp, fault.timestamp);
        }
        EXPECT_GE(maxTimestamp, maxFaults - 1);
    }
}

// Test fault rate tracking
TEST_F(FaultHandlerTest, FaultRateTracking) {
    uint64_t baseTime = 1000;
    
    // Add faults with specific timestamps
    for (int i = 0; i < 5; ++i) {
        FaultRecord fault = createTestFault(FaultType::TranslationFault, AccessType::Read, baseTime + i);
        faultHandler->recordFault(fault);
    }
    
    // Test fault rate calculation (faults per time unit)
    uint64_t timeWindow = 10;  // 10 time units
    uint64_t faultRate = faultHandler->getFaultRate(baseTime + 10, timeWindow);
    
    // Time window is (1000, 1010], so includes timestamps 1001, 1002, 1003, 1004 (4 faults)
    EXPECT_EQ(faultRate, 4);  // 4 faults in the time window (exclusive lower bound)
}

// Test recent faults retrieval
TEST_F(FaultHandlerTest, RecentFaultsRetrieval) {
    uint64_t currentTime = 1000;
    
    // Add faults at different times
    for (int i = 0; i < 10; ++i) {
        FaultRecord fault = createTestFault(FaultType::TranslationFault, AccessType::Read, currentTime - i * 100);
        faultHandler->recordFault(fault);
    }
    
    // Get recent faults within time window
    uint64_t timeWindow = 500;  // 500 time units back
    std::vector<FaultRecord> recentFaults = faultHandler->getRecentFaults(currentTime, timeWindow);
    
    // BUG-14 fix: getRecentFaults() uses a closed interval [earliestTime, currentTime].
    // earliestTime = 1000 - 500 = 500, so timestamps 1000, 900, 800, 700, 600, 500 are all included.
    EXPECT_EQ(recentFaults.size(), 6);
    
    // Verify all recent faults are within the time window
    for (const auto& fault : recentFaults) {
        EXPECT_GE(fault.timestamp, currentTime - timeWindow);
    }
}

// Test fault handler reset
TEST_F(FaultHandlerTest, FaultHandlerReset) {
    // Add some faults
    faultHandler->recordFault(createTestFault(FaultType::TranslationFault, AccessType::Read));
    faultHandler->recordFault(createTestFault(FaultType::PermissionFault, AccessType::Write));

    EXPECT_GT(faultHandler->getFaultCount(), 0);

    // Reset fault handler
    faultHandler->reset();

    EXPECT_EQ(faultHandler->getFaultCount(), 0);
    EXPECT_TRUE(faultHandler->getFaults().empty());

    // Verify all statistics are reset
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::TranslationFault), 0);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::PermissionFault), 0);
}

// BUG-CPP-H01: FaultHandler statistics counters data race.
// getTotalFaultCount(), getTranslationFaultCount(), getPermissionFaultCount()
// must read atomically — no lock is held during those calls, so the underlying
// counters must be std::atomic<uint64_t>.
//
// This test verifies two things:
//
//  1. No missed increments: the exact expected counts are observed after all
//     writers have joined (checks against lost-update under concurrent access).
//
//  2. No out-of-range torn reads: while writers are active, each individual
//     counter read must return a value in [0, expectedTotal].  A torn read of
//     a plain uint64_t on a 32-bit or store-forwarding architecture can produce
//     a value that is completely outside the valid range (e.g. 0xFFFFFFFF00000000
//     if the high word is written before the low word is visible).
//
// Note: The three counters (total, translation, permission) are updated by
// three separate atomic fetch_add calls.  It is therefore possible to
// transiently observe total < translation + permission (reader samples them
// in different moments).  That is NOT a bug; it is an expected property of
// three independent atomics.  Only torn reads or lost increments are bugs.
TEST_F(FaultHandlerTest, StatisticsCounters_ConcurrentReadWrite_NoDataRace) {
    static constexpr size_t NUM_WRITER_THREADS = 4;
    static constexpr size_t FAULTS_PER_THREAD  = 500;
    static constexpr uint64_t EXPECTED_TOTAL   = NUM_WRITER_THREADS * FAULTS_PER_THREAD;

    // Ensure the queue is large enough to hold every fault so that no eviction
    // occurs.  enforceQueueLimit() now correctly decrements the atomic counters
    // on eviction (BUG-NEW2-01 fix); without a sufficient queue size the counters
    // would reflect the bounded queue size rather than the lifetime total this
    // test checks for.
    faultHandler->setMaxQueueSize(static_cast<size_t>(EXPECTED_TOTAL));

    std::atomic<bool> go{false};
    std::atomic<bool> tornReadDetected{false};

    // Reader thread: continuously reads the statistics counters without holding
    // any external lock (mirrors the data-race scenario from the bug report).
    std::thread reader([&]() {
        while (!go.load(std::memory_order_acquire)) {}
        for (int i = 0; i < 10000; ++i) {
            uint64_t total       = faultHandler->getTotalFaultCount();
            uint64_t translation = faultHandler->getTranslationFaultCount();
            uint64_t permission  = faultHandler->getPermissionFaultCount();

            // Each counter must be in [0, EXPECTED_TOTAL].  A torn read of a
            // plain (non-atomic) uint64_t can produce a garbage value far
            // outside this range, demonstrating undefined behavior.
            if (total > EXPECTED_TOTAL ||
                translation > EXPECTED_TOTAL ||
                permission  > EXPECTED_TOTAL) {
                tornReadDetected.store(true, std::memory_order_release);
            }
        }
    });

    // Writer threads: each records an equal mix of TranslationFault and
    // PermissionFault records.
    std::vector<std::thread> writers;
    writers.reserve(NUM_WRITER_THREADS);
    for (size_t t = 0; t < NUM_WRITER_THREADS; ++t) {
        writers.emplace_back([&, t]() {
            while (!go.load(std::memory_order_acquire)) {}
            for (size_t i = 0; i < FAULTS_PER_THREAD; ++i) {
                FaultType ft = ((i % 2) == 0) ? FaultType::TranslationFault
                                               : FaultType::PermissionFault;
                FaultRecord fault;
                fault.streamID   = static_cast<StreamID>(t);
                fault.pasid      = static_cast<PASID>(i % 8);
                fault.address    = static_cast<IOVA>(i * 0x1000);
                fault.faultType  = ft;
                fault.accessType = AccessType::Read;
                fault.timestamp  = static_cast<uint64_t>(i);
                faultHandler->recordFault(fault);
            }
        });
    }

    go.store(true, std::memory_order_release);

    for (auto& w : writers) {
        w.join();
    }
    reader.join();

    EXPECT_FALSE(tornReadDetected.load())
        << "Torn read detected: a statistics counter returned a value outside "
           "[0, EXPECTED_TOTAL], indicating a non-atomic read of a plain uint64_t";

    // After all writers finish the counters must be exact (no lost increments).
    const uint64_t expectedTranslation = EXPECTED_TOTAL / 2;
    const uint64_t expectedPermission  = EXPECTED_TOTAL / 2;

    EXPECT_EQ(faultHandler->getTotalFaultCount(),       EXPECTED_TOTAL)
        << "Total fault count incorrect after concurrent writes";
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), expectedTranslation)
        << "Translation fault count incorrect after concurrent writes";
    EXPECT_EQ(faultHandler->getPermissionFaultCount(),  expectedPermission)
        << "Permission fault count incorrect after concurrent writes";
}

// BUG-CPP-H01 (b): resetStatistics() followed immediately by getTotalFaultCount()
// on another thread must return 0, not a stale or torn value.
TEST_F(FaultHandlerTest, StatisticsCounters_ResetIsVisible_AfterConcurrentReset) {
    // Populate the handler with some faults.
    for (int i = 0; i < 100; ++i) {
        faultHandler->recordFault(
            createTestFault(FaultType::TranslationFault, AccessType::Read, i));
    }
    ASSERT_EQ(faultHandler->getTotalFaultCount(), 100u);

    std::atomic<bool> resetDone{false};
    uint64_t observedAfterReset = 999u; // sentinel

    std::thread resetter([&]() {
        faultHandler->resetStatistics();
        resetDone.store(true, std::memory_order_release);
    });

    resetter.join();

    // Read must see the reset value once the store is visible.
    ASSERT_TRUE(resetDone.load(std::memory_order_acquire));
    observedAfterReset = faultHandler->getTotalFaultCount();

    EXPECT_EQ(observedAfterReset, 0u)
        << "getTotalFaultCount() returned non-zero after resetStatistics()";
}

} // namespace test
} // namespace smmu