// ARM SMMU v3 FaultHandler Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/fault_handler.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

class FaultHandlerTest : public ::testing::Test {
protected:
    void SetUp() override {
        faultHandler = std::make_unique<FaultHandler>();
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
    
    // Should get faults from timestamps: 1000, 900, 800, 700, 600
    EXPECT_EQ(recentFaults.size(), 5);
    
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

} // namespace test
} // namespace smmu