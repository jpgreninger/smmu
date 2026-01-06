// ARM SMMU v3 FaultHandler Coverage Tests
// Copyright (c) 2024 John Greninger
//
// Comprehensive coverage tests for FaultHandler to achieve 90%+ coverage
// Focus: recordTranslationFault(), recordPermissionFault(), and fault type coverage

#include <gtest/gtest.h>
#include "smmu/fault_handler.h"
#include "smmu/types.h"
#include <thread>
#include <chrono>
#include <vector>

namespace smmu {
namespace test {

class FaultHandlerCoverageTest : public ::testing::Test {
protected:
    void SetUp() override {
        faultHandler = std::unique_ptr<FaultHandler>(new FaultHandler());
    }

    void TearDown() override {
        faultHandler.reset();
    }

    std::unique_ptr<FaultHandler> faultHandler;

    // Test helper constants
    static constexpr StreamID TEST_STREAM_ID = 0x2000;
    static constexpr StreamID TEST_STREAM_ID_2 = 0x3000;
    static constexpr PASID TEST_PASID = 0x10;
    static constexpr PASID TEST_PASID_2 = 0x20;
    static constexpr IOVA TEST_IOVA = 0x40000000;
    static constexpr IOVA TEST_IOVA_2 = 0x50000000;
};

// ============================================================================
// TC-FAULT-001: Translation Fault Recording
// Test recordTranslationFault() method directly (lines 32-41)
// ============================================================================

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_BasicRead) {
    // Test translation fault with Read access
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    // Verify fault was recorded
    EXPECT_EQ(faultHandler->getFaultCount(), 1);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.streamID, TEST_STREAM_ID);
    EXPECT_EQ(fault.pasid, TEST_PASID);
    EXPECT_EQ(fault.address, TEST_IOVA);
    EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    EXPECT_EQ(fault.accessType, AccessType::Read);
    EXPECT_GT(fault.timestamp, 0);  // Timestamp should be set
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_BasicWrite) {
    // Test translation fault with Write access
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    EXPECT_EQ(fault.accessType, AccessType::Write);
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_BasicExecute) {
    // Test translation fault with Execute access
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Execute);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    EXPECT_EQ(fault.accessType, AccessType::Execute);
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_AllFieldsPopulated) {
    // Test that all fault record fields are properly populated
    StreamID testStreamID = 0x12345678;
    PASID testPASID = 0x54321;
    IOVA testIOVA = 0xDEADBEEFCAFEBABE;

    faultHandler->recordTranslationFault(testStreamID, testPASID, testIOVA, AccessType::Read);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    // Verify all fields are correctly set
    EXPECT_EQ(fault.streamID, testStreamID);
    EXPECT_EQ(fault.pasid, testPASID);
    EXPECT_EQ(fault.address, testIOVA);
    EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    EXPECT_EQ(fault.accessType, AccessType::Read);
    EXPECT_GT(fault.timestamp, 0);
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_TimestampGeneration) {
    // Test that timestamps are generated and increase
    uint64_t timestamp1 = 0;
    uint64_t timestamp2 = 0;

    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    timestamp1 = faults[0].timestamp;

    // Small delay to ensure different timestamp
    std::this_thread::sleep_for(std::chrono::microseconds(10));

    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);
    faults = faultHandler->getFaults();
    timestamp2 = faults[1].timestamp;

    // Second timestamp should be greater than or equal to first
    EXPECT_GE(timestamp2, timestamp1);
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_QueueIntegration) {
    // Test that translation faults are properly added to fault queue
    const size_t numFaults = 5;

    for (size_t i = 0; i < numFaults; ++i) {
        faultHandler->recordTranslationFault(
            TEST_STREAM_ID + i,
            TEST_PASID + i,
            TEST_IOVA + (i * PAGE_SIZE),
            AccessType::Read
        );
    }

    EXPECT_EQ(faultHandler->getFaultCount(), numFaults);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), numFaults);

    // Verify all faults are translation faults
    for (size_t i = 0; i < numFaults; ++i) {
        EXPECT_EQ(faults[i].faultType, FaultType::TranslationFault);
        EXPECT_EQ(faults[i].streamID, TEST_STREAM_ID + i);
    }
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_MultipleConcurrent) {
    // Test multiple concurrent translation faults
    const int numThreads = 4;
    const int faultsPerThread = 10;
    std::vector<std::thread> threads;

    for (int t = 0; t < numThreads; ++t) {
        threads.emplace_back([this, t, faultsPerThread]() {
            for (int i = 0; i < faultsPerThread; ++i) {
                faultHandler->recordTranslationFault(
                    TEST_STREAM_ID + (t * 100),
                    TEST_PASID + i,
                    TEST_IOVA + (i * PAGE_SIZE),
                    AccessType::Read
                );
            }
        });
    }

    for (auto& thread : threads) {
        thread.join();
    }

    // Verify total fault count
    EXPECT_EQ(faultHandler->getFaultCount(), numThreads * faultsPerThread);

    // Verify all are translation faults
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    for (const auto& fault : faults) {
        EXPECT_EQ(fault.faultType, FaultType::TranslationFault);
    }
}

TEST_F(FaultHandlerCoverageTest, RecordTranslationFault_StatisticsTracking) {
    // Test that translation fault statistics are properly tracked
    EXPECT_EQ(faultHandler->getTotalFaultCount(), 0);
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), 0);

    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_EQ(faultHandler->getTotalFaultCount(), 1);
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), 1);
    EXPECT_EQ(faultHandler->getPermissionFaultCount(), 0);

    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    EXPECT_EQ(faultHandler->getTotalFaultCount(), 2);
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), 2);
}

// ============================================================================
// TC-FAULT-002: Permission Fault Recording
// Test recordPermissionFault() method directly (lines 43-52)
// ============================================================================

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_BasicRead) {
    // Test permission fault with Read access (no read permission)
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_EQ(faultHandler->getFaultCount(), 1);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.streamID, TEST_STREAM_ID);
    EXPECT_EQ(fault.pasid, TEST_PASID);
    EXPECT_EQ(fault.address, TEST_IOVA);
    EXPECT_EQ(fault.faultType, FaultType::PermissionFault);
    EXPECT_EQ(fault.accessType, AccessType::Read);
    EXPECT_GT(fault.timestamp, 0);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_BasicWrite) {
    // Test permission fault with Write access (no write permission)
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.faultType, FaultType::PermissionFault);
    EXPECT_EQ(fault.accessType, AccessType::Write);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_BasicExecute) {
    // Test permission fault with Execute access (no execute permission)
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Execute);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.faultType, FaultType::PermissionFault);
    EXPECT_EQ(fault.accessType, AccessType::Execute);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_AllFieldsPopulated) {
    // Test that all fault record fields are properly populated
    StreamID testStreamID = 0xABCDEF12;
    PASID testPASID = 0x98765;
    IOVA testIOVA = 0xFEEDFACEDEADC0DE;

    faultHandler->recordPermissionFault(testStreamID, testPASID, testIOVA, AccessType::Write);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];
    EXPECT_EQ(fault.streamID, testStreamID);
    EXPECT_EQ(fault.pasid, testPASID);
    EXPECT_EQ(fault.address, testIOVA);
    EXPECT_EQ(fault.faultType, FaultType::PermissionFault);
    EXPECT_EQ(fault.accessType, AccessType::Write);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_TypeDistinction) {
    // Test that permission faults are distinct from translation faults
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    faultHandler->recordPermissionFault(TEST_STREAM_ID_2, TEST_PASID, TEST_IOVA_2, AccessType::Write);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 2);

    EXPECT_EQ(faults[0].faultType, FaultType::TranslationFault);
    EXPECT_EQ(faults[1].faultType, FaultType::PermissionFault);

    // Verify they can be distinguished
    EXPECT_NE(faults[0].faultType, faults[1].faultType);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_EventQueuePropagation) {
    // Test that permission faults propagate to event queue properly
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(faultHandler->hasEvents());
    EXPECT_EQ(faultHandler->getEventCount(), 1);

    std::vector<FaultRecord> events = faultHandler->getEvents();
    ASSERT_EQ(events.size(), 1);
    EXPECT_EQ(events[0].faultType, FaultType::PermissionFault);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_PermissionScenarios) {
    // Test different permission violation scenarios

    // Scenario 1: Read access with no read permission
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x1000, AccessType::Read);

    // Scenario 2: Write access with no write permission
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x2000, AccessType::Write);

    // Scenario 3: Execute access with no execute permission
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x3000, AccessType::Execute);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 3);

    // All should be permission faults
    for (const auto& fault : faults) {
        EXPECT_EQ(fault.faultType, FaultType::PermissionFault);
    }

    // Access types should be different
    EXPECT_EQ(faults[0].accessType, AccessType::Read);
    EXPECT_EQ(faults[1].accessType, AccessType::Write);
    EXPECT_EQ(faults[2].accessType, AccessType::Execute);
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_MultipleConcurrent) {
    // Test multiple concurrent permission faults
    const int numThreads = 3;
    const int faultsPerThread = 8;
    std::vector<std::thread> threads;

    for (int t = 0; t < numThreads; ++t) {
        threads.emplace_back([this, t, faultsPerThread]() {
            for (int i = 0; i < faultsPerThread; ++i) {
                faultHandler->recordPermissionFault(
                    TEST_STREAM_ID + (t * 50),
                    TEST_PASID + i,
                    TEST_IOVA + (i * PAGE_SIZE),
                    AccessType::Write
                );
            }
        });
    }

    for (auto& thread : threads) {
        thread.join();
    }

    EXPECT_EQ(faultHandler->getFaultCount(), numThreads * faultsPerThread);

    // Verify all are permission faults
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    for (const auto& fault : faults) {
        EXPECT_EQ(fault.faultType, FaultType::PermissionFault);
    }
}

TEST_F(FaultHandlerCoverageTest, RecordPermissionFault_StatisticsTracking) {
    // Test that permission fault statistics are properly tracked
    EXPECT_EQ(faultHandler->getTotalFaultCount(), 0);
    EXPECT_EQ(faultHandler->getPermissionFaultCount(), 0);

    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    EXPECT_EQ(faultHandler->getTotalFaultCount(), 1);
    EXPECT_EQ(faultHandler->getPermissionFaultCount(), 1);
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), 0);

    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_EQ(faultHandler->getTotalFaultCount(), 2);
    EXPECT_EQ(faultHandler->getPermissionFaultCount(), 2);
}

// ============================================================================
// TC-FAULT-003: Fault Type Coverage
// Test all FaultType enum values and comprehensive fault scenarios
// ============================================================================

TEST_F(FaultHandlerCoverageTest, FaultType_TranslationFaultClassification) {
    // Test translation fault type classification
    FaultRecord fault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.address = TEST_IOVA;
    fault.faultType = FaultType::TranslationFault;
    fault.accessType = AccessType::Read;
    fault.timestamp = 1000;

    faultHandler->recordFault(fault);

    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::TranslationFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_PermissionFaultClassification) {
    // Test permission fault type classification
    FaultRecord fault;
    fault.faultType = FaultType::PermissionFault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Write;

    faultHandler->recordFault(fault);

    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::PermissionFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_AddressSizeFaultClassification) {
    // Test address size fault type
    FaultRecord fault;
    fault.faultType = FaultType::AddressSizeFault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.address = 0xFFFFFFFFFFFFFFFF;  // Address too large
    fault.accessType = AccessType::Read;

    faultHandler->recordFault(fault);

    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::AddressSizeFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_AccessFaultClassification) {
    // Test general access fault type
    FaultRecord fault;
    fault.faultType = FaultType::AccessFault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;

    faultHandler->recordFault(fault);

    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::AccessFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_SecurityFaultClassification) {
    // Test security fault type
    FaultRecord fault;
    fault.faultType = FaultType::SecurityFault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.address = TEST_IOVA;
    fault.accessType = AccessType::Read;
    fault.securityState = SecurityState::Secure;

    faultHandler->recordFault(fault);

    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::SecurityFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_AllEnumValues) {
    // Test all FaultType enum values can be recorded
    std::vector<FaultType> allFaultTypes = {
        FaultType::TranslationFault,
        FaultType::PermissionFault,
        FaultType::AddressSizeFault,
        FaultType::AccessFault,
        FaultType::SecurityFault,
        FaultType::ContextDescriptorFormatFault,
        FaultType::TranslationTableFormatFault,
        FaultType::Level0TranslationFault,
        FaultType::Level1TranslationFault,
        FaultType::Level2TranslationFault,
        FaultType::Level3TranslationFault,
        FaultType::AccessFlagFault,
        FaultType::DirtyBitFault,
        FaultType::TLBConflictFault,
        FaultType::ExternalAbort,
        FaultType::SynchronousExternalAbort,
        FaultType::AsynchronousExternalAbort,
        FaultType::StreamTableFormatFault,
        FaultType::ConfigurationCacheFault,
        FaultType::Stage2TranslationFault,
        FaultType::Stage2PermissionFault
    };

    // Record one fault of each type
    for (size_t i = 0; i < allFaultTypes.size(); ++i) {
        FaultRecord fault;
        fault.streamID = TEST_STREAM_ID;
        fault.pasid = TEST_PASID;
        fault.address = TEST_IOVA + (i * PAGE_SIZE);
        fault.faultType = allFaultTypes[i];
        fault.accessType = AccessType::Read;
        fault.timestamp = 1000 + i;

        faultHandler->recordFault(fault);
    }

    EXPECT_EQ(faultHandler->getFaultCount(), allFaultTypes.size());

    // Verify each fault type was recorded
    for (const auto& faultType : allFaultTypes) {
        EXPECT_GE(faultHandler->getFaultCountByType(faultType), 1);
    }
}

TEST_F(FaultHandlerCoverageTest, FaultType_RecoveryScenarios) {
    // Test fault recovery by clearing and re-recording
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    EXPECT_EQ(faultHandler->getFaultCount(), 1);

    // Clear faults (simulating recovery)
    faultHandler->clearFaults();
    EXPECT_EQ(faultHandler->getFaultCount(), 0);

    // Record new fault after recovery
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);
    EXPECT_EQ(faultHandler->getFaultCount(), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_LoggingAndReporting) {
    // Test fault logging and reporting capabilities
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, 0x1000, AccessType::Read);
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x2000, AccessType::Write);

    FaultRecord fault;
    fault.faultType = FaultType::AddressSizeFault;
    fault.streamID = TEST_STREAM_ID;
    fault.pasid = TEST_PASID;
    fault.address = 0x3000;
    fault.accessType = AccessType::Execute;
    faultHandler->recordFault(fault);

    // Verify faults can be retrieved and reported
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 3);

    // Verify each fault can be identified
    EXPECT_EQ(faults[0].faultType, FaultType::TranslationFault);
    EXPECT_EQ(faults[1].faultType, FaultType::PermissionFault);
    EXPECT_EQ(faults[2].faultType, FaultType::AddressSizeFault);
}

TEST_F(FaultHandlerCoverageTest, FaultType_StatisticsTracking) {
    // Test fault statistics tracking for different fault types
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Execute);

    EXPECT_EQ(faultHandler->getTotalFaultCount(), 3);
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), 2);
    EXPECT_EQ(faultHandler->getPermissionFaultCount(), 1);

    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::TranslationFault), 2);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::PermissionFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultType_RecordRetrievalQuerying) {
    // Test fault record retrieval and querying
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);
    faultHandler->recordPermissionFault(TEST_STREAM_ID_2, TEST_PASID_2, TEST_IOVA_2, AccessType::Write);

    // Query by stream ID
    std::vector<FaultRecord> stream1Faults = faultHandler->getFaultsByStream(TEST_STREAM_ID);
    ASSERT_EQ(stream1Faults.size(), 1);
    EXPECT_EQ(stream1Faults[0].faultType, FaultType::TranslationFault);

    std::vector<FaultRecord> stream2Faults = faultHandler->getFaultsByStream(TEST_STREAM_ID_2);
    ASSERT_EQ(stream2Faults.size(), 1);
    EXPECT_EQ(stream2Faults[0].faultType, FaultType::PermissionFault);

    // Query by PASID
    std::vector<FaultRecord> pasid1Faults = faultHandler->getFaultsByPASID(TEST_PASID);
    ASSERT_EQ(pasid1Faults.size(), 1);
    EXPECT_EQ(pasid1Faults[0].streamID, TEST_STREAM_ID);
}

TEST_F(FaultHandlerCoverageTest, FaultType_QueueOverflowBehavior) {
    // Test fault queue overflow behavior with different fault types
    const size_t maxFaults = 10;
    faultHandler->setMaxFaults(maxFaults);

    // Fill queue with translation faults
    for (size_t i = 0; i < maxFaults / 2; ++i) {
        faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA + i, AccessType::Read);
    }

    // Fill remaining with permission faults
    for (size_t i = 0; i < maxFaults / 2; ++i) {
        faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA + i, AccessType::Write);
    }

    EXPECT_LE(faultHandler->getFaultCount(), maxFaults);

    // Add more faults to trigger overflow
    for (size_t i = 0; i < 5; ++i) {
        FaultRecord fault;
        fault.faultType = FaultType::AddressSizeFault;
        fault.streamID = TEST_STREAM_ID;
        fault.pasid = TEST_PASID;
        fault.address = TEST_IOVA + i;
        fault.accessType = AccessType::Execute;
        faultHandler->recordFault(fault);
    }

    // Queue should not exceed max size
    EXPECT_LE(faultHandler->getFaultCount(), maxFaults);

    // Most recent faults should be in the queue
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    bool hasAddressSizeFault = false;
    for (const auto& fault : faults) {
        if (fault.faultType == FaultType::AddressSizeFault) {
            hasAddressSizeFault = true;
            break;
        }
    }
    EXPECT_TRUE(hasAddressSizeFault);  // Recent faults should be retained
}

// ============================================================================
// Additional Coverage Tests
// ============================================================================

TEST_F(FaultHandlerCoverageTest, MixedFaultTypes_CompleteScenario) {
    // Test a complete scenario with mixed fault types

    // Translation faults
    faultHandler->recordTranslationFault(0x1000, 1, 0x10000, AccessType::Read);
    faultHandler->recordTranslationFault(0x1001, 2, 0x20000, AccessType::Write);

    // Permission faults
    faultHandler->recordPermissionFault(0x2000, 3, 0x30000, AccessType::Execute);
    faultHandler->recordPermissionFault(0x2001, 4, 0x40000, AccessType::Read);

    // Other fault types
    FaultRecord addrFault;
    addrFault.faultType = FaultType::AddressSizeFault;
    addrFault.streamID = 0x3000;
    addrFault.pasid = 5;
    addrFault.address = 0x50000;
    addrFault.accessType = AccessType::Write;
    faultHandler->recordFault(addrFault);

    // Verify total counts
    EXPECT_EQ(faultHandler->getFaultCount(), 5);
    EXPECT_EQ(faultHandler->getTotalFaultCount(), 5);
    EXPECT_EQ(faultHandler->getTranslationFaultCount(), 2);
    EXPECT_EQ(faultHandler->getPermissionFaultCount(), 2);

    // Verify by type
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::TranslationFault), 2);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::PermissionFault), 2);
    EXPECT_EQ(faultHandler->getFaultCountByType(FaultType::AddressSizeFault), 1);
}

TEST_F(FaultHandlerCoverageTest, FaultRecordStructure_ARMSMMUv3Compliance) {
    // Test that FaultRecord structure matches ARM SMMU v3 spec
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    std::vector<FaultRecord> faults = faultHandler->getFaults();
    ASSERT_EQ(faults.size(), 1);

    const FaultRecord& fault = faults[0];

    // Verify ARM SMMU v3 required fields are present
    EXPECT_TRUE(fault.streamID <= MAX_STREAM_ID);
    EXPECT_TRUE(fault.pasid <= MAX_PASID);
    EXPECT_NE(fault.address, 0);  // IOVA should be set
    EXPECT_GE(fault.timestamp, 0);  // Timestamp should be valid

    // Verify fault type is a valid ARM SMMU v3 fault type
    EXPECT_EQ(fault.faultType, FaultType::TranslationFault);

    // Verify access type is valid
    EXPECT_TRUE(fault.accessType == AccessType::Read ||
                fault.accessType == AccessType::Write ||
                fault.accessType == AccessType::Execute);
}

TEST_F(FaultHandlerCoverageTest, AccessType_AllTypesCovered) {
    // Test all access types with both translation and permission faults

    // Translation faults
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, 0x1000, AccessType::Read);
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, 0x2000, AccessType::Write);
    faultHandler->recordTranslationFault(TEST_STREAM_ID, TEST_PASID, 0x3000, AccessType::Execute);

    // Permission faults
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x4000, AccessType::Read);
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x5000, AccessType::Write);
    faultHandler->recordPermissionFault(TEST_STREAM_ID, TEST_PASID, 0x6000, AccessType::Execute);

    EXPECT_EQ(faultHandler->getFaultCount(), 6);

    // Verify access type counting
    EXPECT_EQ(faultHandler->getFaultCountByAccessType(AccessType::Read), 2);
    EXPECT_EQ(faultHandler->getFaultCountByAccessType(AccessType::Write), 2);
    EXPECT_EQ(faultHandler->getFaultCountByAccessType(AccessType::Execute), 2);
}

} // namespace test
} // namespace smmu
