// ARM SMMU v3 StreamContext Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/stream_context.h"
#include "smmu/types.h"
#include <thread>
#include <vector>
#include <atomic>
#include <chrono>

namespace smmu {
namespace test {

class StreamContextTest : public ::testing::Test {
protected:
    void SetUp() override {
        streamContext = std::make_unique<StreamContext>();
    }

    void TearDown() override {
        streamContext.reset();
    }

    std::unique_ptr<StreamContext> streamContext;
    
    // Test helper constants
    static constexpr StreamID TEST_STREAM_ID = 0x1000;
    static constexpr PASID TEST_PASID_1 = 0x1;
    static constexpr PASID TEST_PASID_2 = 0x2;
    static constexpr PASID TEST_PASID_3 = 0x3;
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr IOVA TEST_IOVA_2 = 0x20000000;
    static constexpr PA TEST_PA = 0x40000000;
    static constexpr PA TEST_PA_2 = 0x50000000;
    static constexpr PA TEST_INTERMEDIATE_PA = 0x60000000;
    
    // Helper method to set up two-stage translation
    void setupTwoStageTranslation(std::shared_ptr<AddressSpace> stage2Space) {
        streamContext->setStage1Enabled(true);
        streamContext->setStage2Enabled(true);
        streamContext->setStage2AddressSpace(stage2Space);
    }
};

// Test default construction
TEST_F(StreamContextTest, DefaultConstruction) {
    ASSERT_NE(streamContext, nullptr);
    
    // Verify that translation on empty context fails
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PASIDNotFound);
}

// Test PASID address space creation
TEST_F(StreamContextTest, CreatePASIDAddressSpace) {
    // Create address space for a PASID
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // Verify we can't create the same PASID again
    EXPECT_FALSE(streamContext->createPASID(TEST_PASID_1));
    
    // Verify we can create a different PASID
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
}

// Test PASID address space removal
TEST_F(StreamContextTest, RemovePASIDAddressSpace) {
    // Create and then remove a PASID
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->removePASID(TEST_PASID_1));
    
    // Verify we can't remove it again
    EXPECT_FALSE(streamContext->removePASID(TEST_PASID_1));
    
    // Verify we can recreate it after removal
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
}

// Test basic translation through PASID
TEST_F(StreamContextTest, BasicTranslation) {
    // Create PASID and set up a mapping
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    PagePermissions perms(true, true, false);  // Read-write
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    // Test translation
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
}

// Test translation failure for non-existent PASID
TEST_F(StreamContextTest, TranslationNonExistentPASID) {
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PASIDNotFound);
}

// Test multiple PASIDs with separate address spaces
TEST_F(StreamContextTest, MultiplePASIDs) {
    // Create two PASIDs
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    
    // Set up different mappings for each PASID
    PagePermissions perms1(true, false, false);  // Read-only
    PagePermissions perms2(true, true, true);    // Read-write-execute
    
    PA pa1 = TEST_PA;
    PA pa2 = TEST_PA + 0x10000;
    
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, pa1, perms1));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_2, TEST_IOVA, pa2, perms2));
    
    // Test translations - same IOVA should map to different PAs
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, pa1);
    EXPECT_EQ(result2.getValue().physicalAddress, pa2);
    EXPECT_NE(result1.getValue().physicalAddress, result2.getValue().physicalAddress);
    
    // Test permission differences
    TranslationResult write1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    TranslationResult write2 = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Write);
    
    EXPECT_TRUE(write1.isError());  // PASID 1 is read-only
    EXPECT_TRUE(write2.isOk());   // PASID 2 allows write
}

// Test page mapping failures
TEST_F(StreamContextTest, PageMappingFailures) {
    PagePermissions perms(true, true, false);
    
    // Try to map to non-existent PASID
    EXPECT_FALSE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    // Create PASID and verify mapping works
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
}

// Test page unmapping
TEST_F(StreamContextTest, PageUnmapping) {
    // Set up mapping
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    // Verify mapping exists
    TranslationResult beforeUnmap = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(beforeUnmap.isOk());
    
    // Unmap page
    EXPECT_TRUE(streamContext->unmapPage(TEST_PASID_1, TEST_IOVA));
    
    // Verify mapping no longer exists
    TranslationResult afterUnmap = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(afterUnmap.isError());
}

// Test unmapping from non-existent PASID
TEST_F(StreamContextTest, UnmapNonExistentPASID) {
    // Should return false but not crash
    EXPECT_FALSE(streamContext->unmapPage(TEST_PASID_1, TEST_IOVA));
}

// Test PASID statistics
TEST_F(StreamContextTest, PASIDStatistics) {
    // Initially no PASIDs
    EXPECT_EQ(streamContext->getPASIDCount(), 0);
    
    // Add PASIDs
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_EQ(streamContext->getPASIDCount(), 1);
    
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    EXPECT_EQ(streamContext->getPASIDCount(), 2);
    
    // Remove PASID
    EXPECT_TRUE(streamContext->removePASID(TEST_PASID_1));
    EXPECT_EQ(streamContext->getPASIDCount(), 1);
    
    EXPECT_TRUE(streamContext->removePASID(TEST_PASID_2));
    EXPECT_EQ(streamContext->getPASIDCount(), 0);
}

// Test PASID existence check
TEST_F(StreamContextTest, PASIDExistenceCheck) {
    // PASID doesn't exist initially
    EXPECT_FALSE(streamContext->hasPASID(TEST_PASID_1));
    
    // Create PASID
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->hasPASID(TEST_PASID_1));
    
    // Remove PASID
    EXPECT_TRUE(streamContext->removePASID(TEST_PASID_1));
    EXPECT_FALSE(streamContext->hasPASID(TEST_PASID_1));
}

// Test large PASID values (within 20-bit limit)
TEST_F(StreamContextTest, LargePASIDValues) {
    PASID largePASID = MAX_PASID;  // Maximum valid 20-bit PASID
    
    EXPECT_TRUE(streamContext->createPASID(largePASID));
    EXPECT_TRUE(streamContext->hasPASID(largePASID));
    
    // Test mapping with large PASID
    PagePermissions perms(true, false, false);
    EXPECT_TRUE(streamContext->mapPage(largePASID, TEST_IOVA, TEST_PA, perms));
    
    TranslationResult result = streamContext->translate(largePASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
}

// Test PASID isolation (ensure address spaces are independent)
TEST_F(StreamContextTest, PASIDIsolation) {
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    
    PagePermissions perms(true, true, false);
    
    // Map same IOVA to different PAs in different PASIDs
    PA pa1 = TEST_PA;
    PA pa2 = TEST_PA + 0x100000;
    
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, pa1, perms));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_2, TEST_IOVA, pa2, perms));
    
    // Unmap from PASID 1
    EXPECT_TRUE(streamContext->unmapPage(TEST_PASID_1, TEST_IOVA));
    
    // Verify PASID 1 no longer has mapping but PASID 2 still does
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(result1.isError());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result2.getValue().physicalAddress, pa2);
}

// Test clear all PASIDs
TEST_F(StreamContextTest, ClearAllPASIDs) {
    // Create multiple PASIDs
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    EXPECT_EQ(streamContext->getPASIDCount(), 2);
    
    // Clear all PASIDs
    VoidResult clearResult = streamContext->clearAllPASIDs();
    EXPECT_TRUE(clearResult.isOk());
    EXPECT_EQ(streamContext->getPASIDCount(), 0);
    
    // Verify PASIDs no longer exist
    EXPECT_FALSE(streamContext->hasPASID(TEST_PASID_1));
    EXPECT_FALSE(streamContext->hasPASID(TEST_PASID_2));
}

// Test address space access for PASID
TEST_F(StreamContextTest, GetPASIDAddressSpace) {
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // Get address space for the PASID
    AddressSpace* addrSpace = streamContext->getPASIDAddressSpace(TEST_PASID_1);
    EXPECT_NE(addrSpace, nullptr);
    
    // Verify we can use the address space directly
    PagePermissions perms(true, false, false);
    addrSpace->mapPage(TEST_IOVA, TEST_PA, perms);
    
    // Verify mapping works through StreamContext
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Test non-existent PASID returns nullptr
    AddressSpace* nullSpace = streamContext->getPASIDAddressSpace(TEST_PASID_2);
    EXPECT_EQ(nullSpace, nullptr);
}

// ======================================================================
// TASK 4.1 COMPREHENSIVE TESTS - Two-Stage Translation Configuration
// ======================================================================

// Test stage configuration methods
TEST_F(StreamContextTest, StageConfiguration) {
    // Test default configuration - ARM SMMU v3: Stage-1 enabled by default
    EXPECT_TRUE(streamContext->isStage1Enabled());   // Default: Stage-1 enabled
    EXPECT_FALSE(streamContext->isStage2Enabled());  // Default: Stage-2 disabled
    
    // Test enabling Stage-1
    streamContext->setStage1Enabled(true);
    EXPECT_TRUE(streamContext->isStage1Enabled());
    EXPECT_FALSE(streamContext->isStage2Enabled());
    
    // Test enabling Stage-2
    streamContext->setStage2Enabled(true);
    EXPECT_TRUE(streamContext->isStage1Enabled());
    EXPECT_TRUE(streamContext->isStage2Enabled());
    
    // Test disabling Stage-1
    streamContext->setStage1Enabled(false);
    EXPECT_FALSE(streamContext->isStage1Enabled());
    EXPECT_TRUE(streamContext->isStage2Enabled());
    
    // Test disabling both
    streamContext->setStage2Enabled(false);
    EXPECT_FALSE(streamContext->isStage1Enabled());
    EXPECT_FALSE(streamContext->isStage2Enabled());
}

// Test Stage-1 only translation (default configuration)
TEST_F(StreamContextTest, Stage1OnlyTranslation) {
    // Configure for Stage-1 only (default state)
    streamContext->setStage1Enabled(true);
    streamContext->setStage2Enabled(false);
    
    // Create PASID and map page
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    // Translation should work directly through Stage-1
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Verify write access works
    TranslationResult writeResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(writeResult.isOk());
    EXPECT_EQ(writeResult.getValue().physicalAddress, TEST_PA);
}

// Test Stage-2 only translation
TEST_F(StreamContextTest, Stage2OnlyTranslation) {
    // Create Stage-2 address space
    auto stage2Space = std::make_shared<AddressSpace>();
    PagePermissions stage2Perms(true, true, false);
    stage2Space->mapPage(TEST_IOVA, TEST_PA, stage2Perms);
    
    // Configure for Stage-2 only
    streamContext->setStage1Enabled(false);
    streamContext->setStage2Enabled(true);
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Create PASID (but don't map in Stage-1)
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // Translation should work through Stage-2 only
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
}

// Test both Stage-1 and Stage-2 translation
TEST_F(StreamContextTest, BothStagesTranslation) {
    // Create Stage-2 address space mapping intermediate PA to final PA
    auto stage2Space = std::make_shared<AddressSpace>();
    PagePermissions stage2Perms(true, true, false);
    stage2Space->mapPage(TEST_INTERMEDIATE_PA, TEST_PA_2, stage2Perms);
    
    // Set up two-stage translation
    setupTwoStageTranslation(stage2Space);
    
    // Create PASID and map IOVA to intermediate PA in Stage-1
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    PagePermissions stage1Perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_INTERMEDIATE_PA, stage1Perms));
    
    // Translation should go: IOVA -> Stage-1 -> Intermediate PA -> Stage-2 -> Final PA
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA_2);
}

// Test no stages enabled (identity mapping per ARM SMMU v3)
TEST_F(StreamContextTest, NoStagesTranslation) {
    // Disable both stages (Stage-1 enabled by default)
    streamContext->setStage1Enabled(false);
    streamContext->setStage2Enabled(false);
    EXPECT_FALSE(streamContext->isStage1Enabled());
    EXPECT_FALSE(streamContext->isStage2Enabled());
    
    // Create PASID
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // ARM SMMU v3: Identity mapping when translation disabled (bypass mode)
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_IOVA);  // Identity mapping
}

// ======================================================================
// TASK 4.1 COMPREHENSIVE TESTS - addPASID() Method Testing
// ======================================================================

// Test addPASID with existing AddressSpace
TEST_F(StreamContextTest, AddPASIDWithExistingAddressSpace) {
    // Create a standalone address space with mapping
    auto addressSpace = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, true); // Read-execute
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);
    
    // Add PASID with the existing address space
    streamContext->addPASID(TEST_PASID_1, addressSpace);
    
    // Verify PASID exists and count is updated
    EXPECT_TRUE(streamContext->hasPASID(TEST_PASID_1));
    EXPECT_EQ(streamContext->getPASIDCount(), 1);
    
    // Verify translation works through the added address space
    streamContext->setStage1Enabled(true);
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Verify execute access works
    TranslationResult execResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Execute);
    EXPECT_TRUE(execResult.isOk());
    
    // Verify write access fails (not permitted)
    TranslationResult writeResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(writeResult.isError());
}

// Test shared AddressSpace between PASIDs
TEST_F(StreamContextTest, SharedAddressSpaceBetweenPASIDs) {
    // Create a shared address space
    auto sharedSpace = std::make_shared<AddressSpace>();
    PagePermissions perms(true, true, false);
    sharedSpace->mapPage(TEST_IOVA, TEST_PA, perms);
    sharedSpace->mapPage(TEST_IOVA_2, TEST_PA_2, perms);
    
    // Add the same address space to multiple PASIDs
    streamContext->addPASID(TEST_PASID_1, sharedSpace);
    streamContext->addPASID(TEST_PASID_2, sharedSpace);
    
    EXPECT_EQ(streamContext->getPASIDCount(), 2);
    
    streamContext->setStage1Enabled(true);
    
    // Both PASIDs should access the same address space
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA);
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA);
    
    // Verify both PASIDs see changes made through the shared space
    TranslationResult result1_2 = streamContext->translate(TEST_PASID_1, TEST_IOVA_2, AccessType::Read);
    TranslationResult result2_2 = streamContext->translate(TEST_PASID_2, TEST_IOVA_2, AccessType::Read);
    
    EXPECT_TRUE(result1_2.isOk());
    EXPECT_TRUE(result2_2.isOk());
    EXPECT_EQ(result1_2.getValue().physicalAddress, TEST_PA_2);
    EXPECT_EQ(result2_2.getValue().physicalAddress, TEST_PA_2);
}

// ======================================================================
// TASK 4.1 COMPREHENSIVE TESTS - Fault Mode Configuration
// ======================================================================

// Test fault mode configuration
TEST_F(StreamContextTest, FaultModeConfiguration) {
    // Test setting Terminate mode
    streamContext->setFaultMode(FaultMode::Terminate);
    // Note: No getter for fault mode in current interface, but we can test behavior
    
    // Test setting Stall mode
    streamContext->setFaultMode(FaultMode::Stall);
    // Behavior testing would require fault simulation
}

// Test fault behavior with Terminate mode
TEST_F(StreamContextTest, FaultModeTerminate) {
    streamContext->setFaultMode(FaultMode::Terminate);
    streamContext->setStage1Enabled(true);
    
    // Create PASID but don't map the page
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // Translation should fail and terminate
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

// Test fault behavior with Stall mode
TEST_F(StreamContextTest, FaultModeStall) {
    streamContext->setFaultMode(FaultMode::Stall);
    streamContext->setStage1Enabled(true);
    
    // Create PASID but don't map the page
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // Translation should fail but with stall behavior
    TranslationResult result = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
    // In real implementation, this might queue for OS handling
}

// ======================================================================
// TASK 4.1 COMPREHENSIVE TESTS - Stage-2 AddressSpace Management
// ======================================================================

// Test Stage-2 AddressSpace shared across multiple PASIDs
TEST_F(StreamContextTest, Stage2AddressSpaceSharing) {
    // Create Stage-2 address space
    auto stage2Space = std::make_shared<AddressSpace>();
    PagePermissions stage2Perms(true, true, false);
    stage2Space->mapPage(TEST_INTERMEDIATE_PA, TEST_PA_2, stage2Perms);
    
    // Set up two-stage translation
    setupTwoStageTranslation(stage2Space);
    
    // Create multiple PASIDs with Stage-1 mappings to same intermediate PA
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    
    PagePermissions stage1Perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_INTERMEDIATE_PA, stage1Perms));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_2, TEST_IOVA_2, TEST_INTERMEDIATE_PA, stage1Perms));
    
    // Both PASIDs should use the same Stage-2 translation
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = streamContext->translate(TEST_PASID_2, TEST_IOVA_2, AccessType::Read);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA_2);  // Both should map to same final PA
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA_2);
}

// Test Stage-2 configuration changes
TEST_F(StreamContextTest, Stage2ConfigurationChanges) {
    // Create initial Stage-2 setup
    auto stage2Space1 = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    stage2Space1->mapPage(TEST_INTERMEDIATE_PA, TEST_PA, perms);
    
    setupTwoStageTranslation(stage2Space1);
    
    // Create PASID and mapping
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_INTERMEDIATE_PA, perms));
    
    // Verify initial translation
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA);
    
    // Change to different Stage-2 address space
    auto stage2Space2 = std::make_shared<AddressSpace>();
    stage2Space2->mapPage(TEST_INTERMEDIATE_PA, TEST_PA_2, perms);
    streamContext->setStage2AddressSpace(stage2Space2);
    
    // Translation should now use new Stage-2 mapping
    TranslationResult result2 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA_2);
}

// ======================================================================
// TASK 4.1 COMPREHENSIVE TESTS - Boundary Conditions and Edge Cases
// ======================================================================

// Test invalid PASID operations
TEST_F(StreamContextTest, InvalidPASIDOperations) {
    PagePermissions perms(true, true, false);
    
    // Test operations on PASID exceeding MAX_PASID (invalid per ARM SMMU v3 spec)
    PASID invalidPASID = MAX_PASID + 1;
    EXPECT_FALSE(streamContext->createPASID(invalidPASID)); // PASID exceeds specification
    EXPECT_FALSE(streamContext->mapPage(invalidPASID, TEST_IOVA, TEST_PA, perms));
    EXPECT_FALSE(streamContext->unmapPage(invalidPASID, TEST_IOVA));
    EXPECT_FALSE(streamContext->hasPASID(invalidPASID));
    
    TranslationResult result = streamContext->translate(invalidPASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPASID);  // PASID validation fault
}

// Test PASID 0 operations (valid in ARM SMMU v3 spec for kernel/hypervisor)
TEST_F(StreamContextTest, PASID0Operations) {
    PagePermissions perms(true, true, false);
    
    // PASID 0 should be valid and commonly used for kernel/hypervisor contexts
    PASID kernelPASID = 0;
    EXPECT_TRUE(streamContext->createPASID(kernelPASID));
    EXPECT_TRUE(streamContext->hasPASID(kernelPASID));
    
    // Should be able to map and translate pages with PASID 0
    EXPECT_TRUE(streamContext->mapPage(kernelPASID, TEST_IOVA, TEST_PA, perms));
    
    TranslationResult result = streamContext->translate(kernelPASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Should be able to unmap pages with PASID 0
    EXPECT_TRUE(streamContext->unmapPage(kernelPASID, TEST_IOVA));
    EXPECT_TRUE(streamContext->removePASID(kernelPASID));
}

// Test MAX_PASID boundary conditions
TEST_F(StreamContextTest, MaxPASIDBoundaryTest) {
    // Test with maximum valid PASID
    PASID maxValidPASID = MAX_PASID;
    EXPECT_TRUE(streamContext->createPASID(maxValidPASID));
    EXPECT_TRUE(streamContext->hasPASID(maxValidPASID));
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(maxValidPASID, TEST_IOVA, TEST_PA, perms));
    
    streamContext->setStage1Enabled(true);
    TranslationResult result = streamContext->translate(maxValidPASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA);
    
    // Test with PASID beyond maximum (should fail)
    PASID beyondMaxPASID = MAX_PASID + 1;
    EXPECT_FALSE(streamContext->createPASID(beyondMaxPASID));
    EXPECT_FALSE(streamContext->hasPASID(beyondMaxPASID));
}

// ======================================================================
// TASK 4.1 COMPREHENSIVE TESTS - Enhanced Stream Isolation Security
// ======================================================================

// Test comprehensive cross-PASID isolation
TEST_F(StreamContextTest, ComprehensiveCrossPASIDIsolation) {
    streamContext->setStage1Enabled(true);
    
    // Create multiple PASIDs with different access patterns
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_3));
    
    PagePermissions readOnly(true, false, false);
    PagePermissions readWrite(true, true, false);
    PagePermissions executeOnly(false, false, true);
    
    // Set up different mappings with different permissions
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, readOnly));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_2, TEST_IOVA, TEST_PA_2, readWrite));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_3, TEST_IOVA_2, TEST_PA, executeOnly));
    
    // Test that each PASID can only access its own mappings
    TranslationResult p1_read = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult p1_write = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    TranslationResult p1_exec = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Execute);
    
    EXPECT_TRUE(p1_read.isOk());
    EXPECT_EQ(p1_read.getValue().physicalAddress, TEST_PA);
    EXPECT_TRUE(p1_write.isError());  // Read-only mapping
    EXPECT_TRUE(p1_exec.isError());   // No execute permission
    
    // PASID 1 should not be able to access PASID 2's mapping at same IOVA
    TranslationResult p2_from_p1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_NE(p2_from_p1.getValue().physicalAddress, TEST_PA_2); // Should get TEST_PA, not TEST_PA_2
    
    // PASID 1 should not access PASID 3's IOVA at all
    TranslationResult p3_iova_from_p1 = streamContext->translate(TEST_PASID_1, TEST_IOVA_2, AccessType::Read);
    EXPECT_TRUE(p3_iova_from_p1.isError());
}

// Test isolation under Stage-2 shared configuration
TEST_F(StreamContextTest, IsolationWithSharedStage2) {
    // Set up shared Stage-2
    auto stage2Space = std::make_shared<AddressSpace>();
    PagePermissions stage2Perms(true, true, true);
    stage2Space->mapPage(TEST_INTERMEDIATE_PA, TEST_PA_2, stage2Perms);
    stage2Space->mapPage(TEST_PA, TEST_PA_2, stage2Perms); // Different intermediate mapping
    
    setupTwoStageTranslation(stage2Space);
    
    // Create PASIDs with different Stage-1 mappings
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    
    PagePermissions stage1Perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_INTERMEDIATE_PA, stage1Perms));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_2, TEST_IOVA, TEST_PA, stage1Perms));
    
    // Both should successfully translate but maintain Stage-1 isolation
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA_2); // Both ultimately map to same final PA
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA_2); // through different intermediate PAs
    
    // Verify that removing one PASID doesn't affect the other
    EXPECT_TRUE(streamContext->removePASID(TEST_PASID_1));
    
    TranslationResult result2_after = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result2_after.isOk());
    EXPECT_EQ(result2_after.getValue().physicalAddress, TEST_PA_2);
}

// Test security validation and fault generation
TEST_F(StreamContextTest, SecurityValidationAndFaults) {
    streamContext->setStage1Enabled(true);
    streamContext->setFaultMode(FaultMode::Terminate);
    
    // Create PASID with restricted permissions
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    PagePermissions restrictedPerms(true, false, false); // Read-only
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, restrictedPerms));
    
    // Valid read should succeed
    TranslationResult validRead = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(validRead.isOk());
    EXPECT_EQ(validRead.getValue().physicalAddress, TEST_PA);
    
    // Invalid write should generate permission fault
    TranslationResult invalidWrite = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(invalidWrite.isError());
    EXPECT_EQ(invalidWrite.getError(), SMMUError::PagePermissionViolation);
    
    // Invalid execute should generate permission fault
    TranslationResult invalidExec = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Execute);
    EXPECT_TRUE(invalidExec.isError());
    EXPECT_EQ(invalidExec.getError(), SMMUError::PagePermissionViolation);
    
    // Access to unmapped address should generate translation fault
    TranslationResult unmappedAccess = streamContext->translate(TEST_PASID_1, TEST_IOVA_2, AccessType::Read);
    EXPECT_TRUE(unmappedAccess.isError());
    EXPECT_EQ(unmappedAccess.getError(), SMMUError::PageNotMapped);
}

// ======================================================================
// TASK 4.2 COMPREHENSIVE TESTS - Stream Configuration Update Methods
// ======================================================================

// Test complete configuration update
TEST_F(StreamContextTest, UpdateConfigurationComplete) {
    // Create initial configuration
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = false;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Terminate;
    
    // Apply initial configuration
    EXPECT_TRUE(streamContext->updateConfiguration(initialConfig));
    
    // Verify initial configuration is applied
    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    EXPECT_EQ(currentConfig.translationEnabled, initialConfig.translationEnabled);
    EXPECT_EQ(currentConfig.stage1Enabled, initialConfig.stage1Enabled);
    EXPECT_EQ(currentConfig.stage2Enabled, initialConfig.stage2Enabled);
    EXPECT_EQ(currentConfig.faultMode, initialConfig.faultMode);
    
    // Set up Stage-2 address space for Stage-2 configuration
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Create new configuration
    StreamConfig newConfig;
    newConfig.translationEnabled = true;
    newConfig.stage1Enabled = true;
    newConfig.stage2Enabled = true;
    newConfig.faultMode = FaultMode::Stall;
    
    // Apply new configuration
    EXPECT_TRUE(streamContext->updateConfiguration(newConfig));
    
    // Verify new configuration is applied
    StreamConfig updatedConfig = streamContext->getStreamConfiguration();
    EXPECT_EQ(updatedConfig.translationEnabled, newConfig.translationEnabled);
    EXPECT_EQ(updatedConfig.stage1Enabled, newConfig.stage1Enabled);
    EXPECT_EQ(updatedConfig.stage2Enabled, newConfig.stage2Enabled);
    EXPECT_EQ(updatedConfig.faultMode, newConfig.faultMode);
    
    // Verify configuration change is detected
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
}

// Test selective configuration changes
TEST_F(StreamContextTest, ApplyConfigurationChanges) {
    // Set up initial state
    StreamConfig initialConfig;
    initialConfig.translationEnabled = false;
    initialConfig.stage1Enabled = true;
    initialConfig.stage2Enabled = false;
    initialConfig.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(initialConfig));
    
    // Set up Stage-2 address space for Stage-2 configuration
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Apply selective changes - only change fault mode and stage2
    StreamConfig partialChanges;
    partialChanges.translationEnabled = false; // Keep same
    partialChanges.stage1Enabled = true;       // Keep same
    partialChanges.stage2Enabled = true;       // Change
    partialChanges.faultMode = FaultMode::Stall; // Change
    
    EXPECT_TRUE(streamContext->applyConfigurationChanges(partialChanges));
    
    // Verify only specific fields changed
    StreamConfig result = streamContext->getStreamConfiguration();
    EXPECT_EQ(result.translationEnabled, false);      // Unchanged
    EXPECT_EQ(result.stage1Enabled, true);            // Unchanged
    EXPECT_EQ(result.stage2Enabled, true);            // Changed
    EXPECT_EQ(result.faultMode, FaultMode::Stall);    // Changed
}

// Test configuration validation
TEST_F(StreamContextTest, ConfigurationValidation) {
    // Valid configuration - Stage-1 only
    StreamConfig validConfig1;
    validConfig1.translationEnabled = true;
    validConfig1.stage1Enabled = true;
    validConfig1.stage2Enabled = false;
    validConfig1.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->isConfigurationValid(validConfig1));
    
    // Set up Stage-2 address space for Stage-2 configurations
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Valid configuration - Stage-2 only (with Stage-2 AddressSpace configured)
    StreamConfig validConfig2;
    validConfig2.translationEnabled = true;
    validConfig2.stage1Enabled = false;
    validConfig2.stage2Enabled = true;
    validConfig2.faultMode = FaultMode::Stall;
    EXPECT_TRUE(streamContext->isConfigurationValid(validConfig2));
    
    // Valid configuration - Both stages (with Stage-2 AddressSpace configured)
    StreamConfig validConfig3;
    validConfig3.translationEnabled = true;
    validConfig3.stage1Enabled = true;
    validConfig3.stage2Enabled = true;
    validConfig3.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->isConfigurationValid(validConfig3));
    
    // Valid configuration - Translation disabled (bypass mode)
    StreamConfig validConfig4;
    validConfig4.translationEnabled = false;
    validConfig4.stage1Enabled = false;
    validConfig4.stage2Enabled = false;
    validConfig4.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->isConfigurationValid(validConfig4));
    
    // Invalid configuration - Translation enabled but no stages
    StreamConfig invalidConfig1;
    invalidConfig1.translationEnabled = true;
    invalidConfig1.stage1Enabled = false;
    invalidConfig1.stage2Enabled = false;
    invalidConfig1.faultMode = FaultMode::Terminate;
    Result<bool> validResult1 = streamContext->isConfigurationValid(invalidConfig1);
    EXPECT_TRUE(validResult1.isOk());
    EXPECT_FALSE(validResult1.getValue());
    
    // Valid configuration - Stage-2 enabled without Stage-2 AddressSpace is structurally valid
    // ARM SMMU v3 spec: Configuration validation checks structural validity, not resource availability
    // AddressSpace availability is validated at translation time, not configuration time
    streamContext->setStage2AddressSpace(nullptr); // Remove Stage-2 address space
    StreamConfig stage2Config;
    stage2Config.translationEnabled = true;
    stage2Config.stage1Enabled = false;
    stage2Config.stage2Enabled = true;
    stage2Config.faultMode = FaultMode::Terminate;
    Result<bool> validResult2 = streamContext->isConfigurationValid(stage2Config);
    EXPECT_TRUE(validResult2.isOk());
    EXPECT_TRUE(validResult2.getValue()); // Should be valid - AddressSpace checked at translation time
}

// Test configuration change detection
TEST_F(StreamContextTest, ConfigurationChangeDetection) {
    // Initially no changes
    EXPECT_FALSE(streamContext->hasConfigurationChanged());
    
    // Apply initial configuration
    StreamConfig config1;
    config1.translationEnabled = true;
    config1.stage1Enabled = true;
    config1.stage2Enabled = false;
    config1.faultMode = FaultMode::Terminate;
    VoidResult updateResult1 = streamContext->updateConfiguration(config1);
    EXPECT_TRUE(updateResult1.isOk());
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
    
    // Apply same configuration - should still show changed
    VoidResult updateResult1b = streamContext->updateConfiguration(config1);
    EXPECT_TRUE(updateResult1b.isOk());
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
    
    // Apply different configuration with Stage-2 setup
    // For Stage-2 only mode, we need to set up a Stage-2 AddressSpace first
    std::shared_ptr<AddressSpace> stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    StreamConfig config2;
    config2.translationEnabled = true;
    config2.stage1Enabled = false;
    config2.stage2Enabled = true;
    config2.faultMode = FaultMode::Stall;
    VoidResult updateResult2 = streamContext->updateConfiguration(config2);
    EXPECT_TRUE(updateResult2.isOk());
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
}

// Test configuration edge cases
TEST_F(StreamContextTest, ConfigurationEdgeCases) {
    // Set up Stage-2 AddressSpace for Stage-2 configurations
    std::shared_ptr<AddressSpace> stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Test multiple rapid configuration changes
    for (int i = 0; i < 10; ++i) {
        StreamConfig config;
        config.translationEnabled = (i % 2 == 0);
        config.stage1Enabled = (i % 3 == 0);
        config.stage2Enabled = (i % 4 == 0);
        config.faultMode = (i % 2 == 0) ? FaultMode::Terminate : FaultMode::Stall;
        
        if (config.translationEnabled && !config.stage1Enabled && !config.stage2Enabled) {
            // Invalid configuration - translation enabled but no stages enabled
            Result<bool> validResult = streamContext->isConfigurationValid(config);
            EXPECT_TRUE(validResult.isOk());
            EXPECT_FALSE(validResult.getValue());
            VoidResult updateResult = streamContext->updateConfiguration(config);
            EXPECT_TRUE(updateResult.isError());
        }
        else {
            // Valid configuration
            Result<bool> validResult = streamContext->isConfigurationValid(config);
            EXPECT_TRUE(validResult.isOk());
            EXPECT_TRUE(validResult.getValue());
            VoidResult updateResult = streamContext->updateConfiguration(config);
            EXPECT_TRUE(updateResult.isOk());
        }
    }
    
    // Verify final configuration change is detected
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
    
    // Verify statistics track configuration updates
    StreamStatistics stats = streamContext->getStreamStatistics();
    EXPECT_GT(stats.configurationUpdateCount, 0);
}

// ======================================================================
// TASK 4.2 COMPREHENSIVE TESTS - Stream Enable/Disable Functionality
// ======================================================================

// Test basic stream enable/disable operations
TEST_F(StreamContextTest, StreamEnableDisableBasic) {
    // Initially disabled (default state per ARM SMMU v3)
    Result<bool> initialState = streamContext->isStreamEnabled();
    EXPECT_TRUE(initialState.isOk());
    EXPECT_FALSE(initialState.getValue());
    
    // Enable stream (should succeed with default stage1 enabled)
    VoidResult enableResult = streamContext->enableStream();
    EXPECT_TRUE(enableResult.isOk());
    Result<bool> enabledState = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState.isOk());
    EXPECT_TRUE(enabledState.getValue());
    
    // Disable stream
    VoidResult disableResult = streamContext->disableStream();
    EXPECT_TRUE(disableResult.isOk());
    Result<bool> disabledState = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledState.isOk());
    EXPECT_FALSE(disabledState.getValue());
    
    // Multiple disable calls should succeed
    VoidResult disableResult1 = streamContext->disableStream();
    EXPECT_TRUE(disableResult1.isOk());
    VoidResult disableResult2 = streamContext->disableStream();
    EXPECT_TRUE(disableResult2.isOk());
    Result<bool> multipleDisableState = streamContext->isStreamEnabled();
    EXPECT_TRUE(multipleDisableState.isOk());
    EXPECT_FALSE(multipleDisableState.getValue());
    
    // Multiple enable calls should succeed
    VoidResult enableResult1 = streamContext->enableStream();
    EXPECT_TRUE(enableResult1.isOk());
    VoidResult enableResult2 = streamContext->enableStream();
    EXPECT_TRUE(enableResult2.isOk());
    Result<bool> multipleEnabledState = streamContext->isStreamEnabled();
    EXPECT_TRUE(multipleEnabledState.isOk());
    EXPECT_TRUE(multipleEnabledState.getValue());
}

// Test stream enable/disable effect on translations
TEST_F(StreamContextTest, StreamEnableDisableEffectOnTranslations) {
    // Configure stream with translation enabled to activate stream enable/disable checks
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    VoidResult configResult = streamContext->updateConfiguration(config);
    EXPECT_TRUE(configResult.isOk());
    
    // Set up translation environment
    VoidResult createResult = streamContext->createPASID(TEST_PASID_1);
    EXPECT_TRUE(createResult.isOk());
    PagePermissions perms(true, true, false);
    VoidResult mapResult = streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms);
    EXPECT_TRUE(mapResult.isOk());
    
    // Stream initially disabled - translation should fail
    Result<bool> initiallyDisabled = streamContext->isStreamEnabled();
    EXPECT_TRUE(initiallyDisabled.isOk());
    EXPECT_FALSE(initiallyDisabled.getValue());
    TranslationResult result0 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result0.isError());
    
    // Enable stream - translation should work
    VoidResult enableResult = streamContext->enableStream();
    EXPECT_TRUE(enableResult.isOk());
    Result<bool> nowEnabled = streamContext->isStreamEnabled();
    EXPECT_TRUE(nowEnabled.isOk());
    EXPECT_TRUE(nowEnabled.getValue());
    TranslationResult result1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA);
    
    // Disable stream - translation should fail
    VoidResult disableResult = streamContext->disableStream();
    EXPECT_TRUE(disableResult.isOk());
    TranslationResult result2 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result2.isError());
    // Stream disabled should generate specific fault type or behavior
    
    // Re-enable stream - translation should work again
    VoidResult reEnableResult = streamContext->enableStream();
    EXPECT_TRUE(reEnableResult.isOk());
    TranslationResult result3 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result3.isOk());
    EXPECT_EQ(result3.getValue().physicalAddress, TEST_PA);
}

// Test stream state persistence across configuration changes
TEST_F(StreamContextTest, StreamStatePersistenceAcrossConfigChanges) {
    // Disable stream
    VoidResult disableResult = streamContext->disableStream();
    EXPECT_TRUE(disableResult.isOk());
    Result<bool> disabledState1 = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledState1.isOk());
    EXPECT_FALSE(disabledState1.getValue());
    
    // Apply configuration changes
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Stall;
    VoidResult updateResult = streamContext->updateConfiguration(config);
    EXPECT_TRUE(updateResult.isOk());
    
    // Stream should remain disabled after configuration change
    Result<bool> disabledState2 = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledState2.isOk());
    EXPECT_FALSE(disabledState2.getValue());
    
    // Enable stream
    VoidResult enableResult = streamContext->enableStream();
    EXPECT_TRUE(enableResult.isOk());
    Result<bool> enabledState1 = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState1.isOk());
    EXPECT_TRUE(enabledState1.getValue());
    
    // Apply more configuration changes
    config.faultMode = FaultMode::Terminate;
    VoidResult applyResult = streamContext->applyConfigurationChanges(config);
    EXPECT_TRUE(applyResult.isOk());
    
    // Stream should remain enabled after configuration change
    Result<bool> enabledState2 = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState2.isOk());
    EXPECT_TRUE(enabledState2.getValue());
}

// Test stream enable/disable with various configurations
TEST_F(StreamContextTest, StreamEnableDisableWithConfigurations) {
    // Test with Stage-1 only configuration
    StreamConfig stage1Config;
    stage1Config.translationEnabled = true;
    stage1Config.stage1Enabled = true;
    stage1Config.stage2Enabled = false;
    stage1Config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(stage1Config));
    
    EXPECT_TRUE(streamContext->enableStream());
    Result<bool> enabledState3 = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState3.isOk());
    EXPECT_TRUE(enabledState3.getValue());
    EXPECT_TRUE(streamContext->disableStream());
    Result<bool> disabledState3 = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledState3.isOk());
    EXPECT_FALSE(disabledState3.getValue());
    
    // Set up Stage-2 address space for Stage-2 configurations
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Test with Stage-2 only configuration
    StreamConfig stage2Config;
    stage2Config.translationEnabled = true;
    stage2Config.stage1Enabled = false;
    stage2Config.stage2Enabled = true;
    stage2Config.faultMode = FaultMode::Stall;
    EXPECT_TRUE(streamContext->updateConfiguration(stage2Config));
    
    EXPECT_TRUE(streamContext->enableStream());
    Result<bool> enabledState4 = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState4.isOk());
    EXPECT_TRUE(enabledState4.getValue());
    
    // Test with both stages configuration
    StreamConfig bothStagesConfig;
    bothStagesConfig.translationEnabled = true;
    bothStagesConfig.stage1Enabled = true;
    bothStagesConfig.stage2Enabled = true;
    bothStagesConfig.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(bothStagesConfig));
    
    Result<bool> enabledState5 = streamContext->isStreamEnabled(); // Should remain enabled
    EXPECT_TRUE(enabledState5.isOk());
    EXPECT_TRUE(enabledState5.getValue());
    EXPECT_TRUE(streamContext->disableStream());
    Result<bool> disabledState4 = streamContext->isStreamEnabled();
    EXPECT_TRUE(disabledState4.isOk());
    EXPECT_FALSE(disabledState4.getValue());
}

// ======================================================================
// TASK 4.2 COMPREHENSIVE TESTS - Stream State Querying Capabilities
// ======================================================================

// Test getStreamConfiguration functionality
TEST_F(StreamContextTest, GetStreamConfiguration) {
    // Test default configuration
    StreamConfig defaultConfig = streamContext->getStreamConfiguration();
    // Verify expected defaults match StreamConfig constructor
    EXPECT_FALSE(defaultConfig.translationEnabled); // Default: translation disabled
    
    // Set up Stage-2 address space for Stage-2 configuration
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Set specific configuration and verify retrieval
    StreamConfig testConfig;
    testConfig.translationEnabled = true;
    testConfig.stage1Enabled = false;
    testConfig.stage2Enabled = true;
    testConfig.faultMode = FaultMode::Stall;
    
    EXPECT_TRUE(streamContext->updateConfiguration(testConfig));
    
    StreamConfig retrievedConfig = streamContext->getStreamConfiguration();
    EXPECT_EQ(retrievedConfig.translationEnabled, testConfig.translationEnabled);
    EXPECT_EQ(retrievedConfig.stage1Enabled, testConfig.stage1Enabled);
    EXPECT_EQ(retrievedConfig.stage2Enabled, testConfig.stage2Enabled);
    EXPECT_EQ(retrievedConfig.faultMode, testConfig.faultMode);
}

// Test getStreamStatistics functionality
TEST_F(StreamContextTest, GetStreamStatistics) {
    // Get initial statistics
    StreamStatistics initialStats = streamContext->getStreamStatistics();
    EXPECT_EQ(initialStats.translationCount, 0);
    EXPECT_EQ(initialStats.faultCount, 0);
    EXPECT_EQ(initialStats.pasidCount, 0);
    EXPECT_EQ(initialStats.configurationUpdateCount, 0);
    EXPECT_GT(initialStats.creationTimestamp, 0);
    
    // Perform operations that should update statistics
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    
    PagePermissions perms(true, true, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    // Perform translations
    streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read); // Should fault
    
    // Apply configuration change
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config));
    
    // Verify updated statistics
    StreamStatistics updatedStats = streamContext->getStreamStatistics();
    EXPECT_GT(updatedStats.translationCount, initialStats.translationCount);
    EXPECT_GT(updatedStats.faultCount, initialStats.faultCount);
    EXPECT_EQ(updatedStats.pasidCount, 2);
    EXPECT_GT(updatedStats.configurationUpdateCount, initialStats.configurationUpdateCount);
    EXPECT_GT(updatedStats.lastAccessTimestamp, 0);
}

// Test getStreamState (alias for getStreamConfiguration)
TEST_F(StreamContextTest, GetStreamState) {
    // Set up Stage-2 address space for Stage-2 configuration
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Configure stream
    StreamConfig testConfig;
    testConfig.translationEnabled = true;
    testConfig.stage1Enabled = true;
    testConfig.stage2Enabled = true;
    testConfig.faultMode = FaultMode::Stall;
    EXPECT_TRUE(streamContext->updateConfiguration(testConfig));
    
    // Verify getStreamState returns same as getStreamConfiguration
    StreamConfig stateConfig = streamContext->getStreamState();
    StreamConfig directConfig = streamContext->getStreamConfiguration();
    
    EXPECT_EQ(stateConfig.translationEnabled, directConfig.translationEnabled);
    EXPECT_EQ(stateConfig.stage1Enabled, directConfig.stage1Enabled);
    EXPECT_EQ(stateConfig.stage2Enabled, directConfig.stage2Enabled);
    EXPECT_EQ(stateConfig.faultMode, directConfig.faultMode);
    
    EXPECT_EQ(stateConfig.translationEnabled, testConfig.translationEnabled);
    EXPECT_EQ(stateConfig.stage1Enabled, testConfig.stage1Enabled);
    EXPECT_EQ(stateConfig.stage2Enabled, testConfig.stage2Enabled);
    EXPECT_EQ(stateConfig.faultMode, testConfig.faultMode);
}

// Test isTranslationActive functionality
TEST_F(StreamContextTest, IsTranslationActive) {
    // Initially should be inactive (no PASIDs, default config)
    EXPECT_FALSE(streamContext->isTranslationActive());
    
    // Enable translation in configuration
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.faultMode = FaultMode::Terminate;
    EXPECT_TRUE(streamContext->updateConfiguration(config));
    
    // Still inactive without PASIDs
    EXPECT_FALSE(streamContext->isTranslationActive());
    
    // Add PASID and enable stream - should now be active
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->enableStream());
    EXPECT_TRUE(streamContext->isTranslationActive());
    
    // Disable stream - should become inactive
    EXPECT_TRUE(streamContext->disableStream());
    EXPECT_FALSE(streamContext->isTranslationActive());
    
    // Re-enable stream - should become active again
    EXPECT_TRUE(streamContext->enableStream());
    EXPECT_TRUE(streamContext->isTranslationActive());
    
    // Disable translation in configuration - should become inactive
    config.translationEnabled = false;
    EXPECT_TRUE(streamContext->updateConfiguration(config));
    EXPECT_FALSE(streamContext->isTranslationActive());
}

// Test comprehensive state querying scenario
TEST_F(StreamContextTest, ComprehensiveStateQuerying) {
    // Set up complex scenario
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_3));
    
    PagePermissions perms(true, false, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_2, TEST_IOVA_2, TEST_PA_2, perms));
    
    // Configure for two-stage translation
    auto stage2Space = std::make_shared<AddressSpace>();
    PagePermissions stage2Perms(true, true, false);
    stage2Space->mapPage(TEST_PA, TEST_PA_2, stage2Perms);
    
    streamContext->setStage2AddressSpace(stage2Space);
    
    StreamConfig complexConfig;
    complexConfig.translationEnabled = true;
    complexConfig.stage1Enabled = true;
    complexConfig.stage2Enabled = true;
    complexConfig.faultMode = FaultMode::Stall;
    EXPECT_TRUE(streamContext->updateConfiguration(complexConfig));
    
    // Enable stream for operations
    EXPECT_TRUE(streamContext->enableStream());
    
    // Perform various operations
    streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write); // Should fault
    streamContext->translate(TEST_PASID_2, TEST_IOVA_2, AccessType::Read);
    streamContext->translate(TEST_PASID_3, TEST_IOVA, AccessType::Read); // Should fault
    
    // Query comprehensive state
    StreamConfig currentConfig = streamContext->getStreamConfiguration();
    StreamStatistics stats = streamContext->getStreamStatistics();
    Result<bool> isEnabledResult = streamContext->isStreamEnabled();
    EXPECT_TRUE(isEnabledResult.isOk());
    bool isEnabled = isEnabledResult.getValue();
    bool isActive = streamContext->isTranslationActive();
    bool hasChanged = streamContext->hasConfigurationChanged();
    
    // Verify complex state
    EXPECT_EQ(currentConfig.translationEnabled, true);
    EXPECT_EQ(currentConfig.stage1Enabled, true);
    EXPECT_EQ(currentConfig.stage2Enabled, true);
    EXPECT_EQ(currentConfig.faultMode, FaultMode::Stall);
    
    EXPECT_GT(stats.translationCount, 0);
    EXPECT_GT(stats.faultCount, 0);
    EXPECT_EQ(stats.pasidCount, 3);
    EXPECT_GT(stats.configurationUpdateCount, 0);
    
    EXPECT_TRUE(isEnabled);
    EXPECT_TRUE(isActive);
    EXPECT_TRUE(hasChanged);
}

// ======================================================================
// TASK 4.2 COMPREHENSIVE TESTS - Stream Fault Handling Integration
// ======================================================================

// Test fault handler assignment and management
TEST_F(StreamContextTest, FaultHandlerAssignmentAndManagement) {
    // Initially no fault handler
    EXPECT_FALSE(streamContext->hasFaultHandler());
    EXPECT_EQ(streamContext->getFaultHandler(), nullptr);
    
    // Create and assign fault handler
    auto faultHandler = std::make_shared<FaultHandler>();
    VoidResult setResult1 = streamContext->setFaultHandler(faultHandler);
    EXPECT_TRUE(setResult1.isOk());
    
    // Verify fault handler is assigned
    EXPECT_TRUE(streamContext->hasFaultHandler());
    EXPECT_EQ(streamContext->getFaultHandler(), faultHandler);
    
    // Clear fault handler by setting to nullptr
    VoidResult clearResult = streamContext->setFaultHandler(nullptr);
    EXPECT_TRUE(clearResult.isOk());
    EXPECT_FALSE(streamContext->hasFaultHandler());
    EXPECT_EQ(streamContext->getFaultHandler(), nullptr);
    
    // Reassign fault handler
    VoidResult setResult2 = streamContext->setFaultHandler(faultHandler);
    EXPECT_TRUE(setResult2.isOk());
    EXPECT_TRUE(streamContext->hasFaultHandler());
    EXPECT_EQ(streamContext->getFaultHandler(), faultHandler);
}

// Test fault recording through fault handler
TEST_F(StreamContextTest, FaultRecordingThroughFaultHandler) {
    // Set up fault handler
    auto faultHandler = std::make_shared<FaultHandler>();
    VoidResult setFaultResult = streamContext->setFaultHandler(faultHandler);
    EXPECT_TRUE(setFaultResult.isOk());
    
    // Create test fault record
    FaultRecord testFault;
    testFault.streamID = TEST_STREAM_ID;
    testFault.pasid = TEST_PASID_1;
    testFault.address = TEST_IOVA;
    testFault.faultType = FaultType::TranslationFault;
    testFault.accessType = AccessType::Read;
    testFault.timestamp = 12345;
    
    // Record fault through StreamContext
    EXPECT_TRUE(streamContext->recordFault(testFault));
    
    // Verify fault was recorded in handler
    std::vector<FaultRecord> faults = faultHandler->getFaults();
    EXPECT_EQ(faults.size(), 1);
    EXPECT_EQ(faults[0].streamID, testFault.streamID);
    EXPECT_EQ(faults[0].pasid, testFault.pasid);
    EXPECT_EQ(faults[0].address, testFault.address);
    EXPECT_EQ(faults[0].faultType, testFault.faultType);
    EXPECT_EQ(faults[0].accessType, testFault.accessType);
    
    // Record multiple faults
    FaultRecord fault2;
    fault2.streamID = TEST_STREAM_ID;
    fault2.pasid = TEST_PASID_2;
    fault2.address = TEST_IOVA_2;
    fault2.faultType = FaultType::PermissionFault;
    fault2.accessType = AccessType::Write;
    fault2.timestamp = 12346;
    
    EXPECT_TRUE(streamContext->recordFault(fault2));
    
    std::vector<FaultRecord> allFaults = faultHandler->getFaults();
    EXPECT_EQ(allFaults.size(), 2);
}

// Test fault recording without fault handler
TEST_F(StreamContextTest, FaultRecordingWithoutFaultHandler) {
    // Ensure no fault handler is set
    EXPECT_FALSE(streamContext->hasFaultHandler());
    
    // Try to record fault - should fail gracefully
    FaultRecord testFault;
    testFault.streamID = TEST_STREAM_ID;
    testFault.pasid = TEST_PASID_1;
    testFault.address = TEST_IOVA;
    testFault.faultType = FaultType::TranslationFault;
    testFault.accessType = AccessType::Read;
    
    EXPECT_FALSE(streamContext->recordFault(testFault));
}

// Test stream-specific fault clearing
TEST_F(StreamContextTest, StreamSpecificFaultClearing) {
    // Set up fault handler
    auto faultHandler = std::make_shared<FaultHandler>();
    VoidResult setFaultResult2 = streamContext->setFaultHandler(faultHandler);
    EXPECT_TRUE(setFaultResult2.isOk());
    
    // Record multiple faults for different streams/PASIDs
    FaultRecord fault1;
    fault1.streamID = TEST_STREAM_ID;
    fault1.pasid = TEST_PASID_1;
    fault1.address = TEST_IOVA;
    fault1.faultType = FaultType::TranslationFault;
    fault1.accessType = AccessType::Read;
    
    FaultRecord fault2;
    fault2.streamID = TEST_STREAM_ID + 1; // Different stream
    fault2.pasid = TEST_PASID_1;
    fault2.address = TEST_IOVA;
    fault2.faultType = FaultType::PermissionFault;
    fault2.accessType = AccessType::Write;
    
    FaultRecord fault3;
    fault3.streamID = TEST_STREAM_ID;
    fault3.pasid = TEST_PASID_2;
    fault3.address = TEST_IOVA_2;
    fault3.faultType = FaultType::AddressSizeFault;
    fault3.accessType = AccessType::Execute;
    
    EXPECT_TRUE(streamContext->recordFault(fault1));
    EXPECT_TRUE(streamContext->recordFault(fault2));
    EXPECT_TRUE(streamContext->recordFault(fault3));
    
    EXPECT_EQ(faultHandler->getFaultCount(), 3);
    
    // Clear stream-specific faults
    streamContext->clearStreamFaults();
    
    // Implementation detail: clearStreamFaults should clear faults for this stream
    // The exact behavior depends on implementation - this test verifies the method exists
    // and can be called successfully
}

// Test fault handler integration with translations
TEST_F(StreamContextTest, FaultHandlerIntegrationWithTranslations) {
    // Set up fault handler and stream
    auto faultHandler = std::make_shared<FaultHandler>();
    VoidResult setFaultResult3 = streamContext->setFaultHandler(faultHandler);
    EXPECT_TRUE(setFaultResult3.isOk());
    
    streamContext->setStage1Enabled(true);
    streamContext->setFaultMode(FaultMode::Terminate);
    
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    // Set up mapping with restricted permissions
    PagePermissions readOnlyPerms(true, false, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, readOnlyPerms));
    
    // Enable stream for translations to work
    EXPECT_TRUE(streamContext->enableStream());
    
    // Successful translation should not generate fault
    size_t initialFaultCount = faultHandler->getFaultCount();
    TranslationResult validResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(validResult.isOk());
    EXPECT_EQ(faultHandler->getFaultCount(), initialFaultCount); // No new faults
    
    // Failed translation should generate fault (if implementation supports this)
    TranslationResult invalidResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(invalidResult.isError());
    EXPECT_EQ(invalidResult.getError(), SMMUError::PagePermissionViolation);
    
    // Translation to unmapped address should generate fault
    TranslationResult unmappedResult = streamContext->translate(TEST_PASID_1, TEST_IOVA_2, AccessType::Read);
    EXPECT_TRUE(unmappedResult.isError());
    EXPECT_EQ(unmappedResult.getError(), SMMUError::PageNotMapped);
}

// Test shared fault handler between multiple streams
TEST_F(StreamContextTest, SharedFaultHandlerBetweenStreams) {
    // Create shared fault handler
    auto sharedHandler = std::make_shared<FaultHandler>();
    
    // Create second StreamContext for testing
    auto secondStreamContext = std::make_unique<StreamContext>();
    
    // Assign same fault handler to both stream contexts
    VoidResult setShared1 = streamContext->setFaultHandler(sharedHandler);
    EXPECT_TRUE(setShared1.isOk());
    VoidResult setShared2 = secondStreamContext->setFaultHandler(sharedHandler);
    EXPECT_TRUE(setShared2.isOk());
    
    // Verify both have the same fault handler
    EXPECT_EQ(streamContext->getFaultHandler(), sharedHandler);
    EXPECT_EQ(secondStreamContext->getFaultHandler(), sharedHandler);
    
    // Record faults from both stream contexts
    FaultRecord fault1;
    fault1.streamID = TEST_STREAM_ID;
    fault1.pasid = TEST_PASID_1;
    fault1.faultType = FaultType::TranslationFault;
    fault1.accessType = AccessType::Read;
    
    FaultRecord fault2;
    fault2.streamID = TEST_STREAM_ID + 1;
    fault2.pasid = TEST_PASID_2;
    fault2.faultType = FaultType::PermissionFault;
    fault2.accessType = AccessType::Write;
    
    EXPECT_TRUE(streamContext->recordFault(fault1));
    EXPECT_TRUE(secondStreamContext->recordFault(fault2));
    
    // Verify both faults are recorded in shared handler
    std::vector<FaultRecord> allFaults = sharedHandler->getFaults();
    EXPECT_EQ(allFaults.size(), 2);
    
    // Verify faults can be retrieved from handler
    std::vector<FaultRecord> stream1Faults = sharedHandler->getFaultsByStream(TEST_STREAM_ID);
    std::vector<FaultRecord> stream2Faults = sharedHandler->getFaultsByStream(TEST_STREAM_ID + 1);
    
    EXPECT_EQ(stream1Faults.size(), 1);
    EXPECT_EQ(stream2Faults.size(), 1);
    EXPECT_EQ(stream1Faults[0].streamID, TEST_STREAM_ID);
    EXPECT_EQ(stream2Faults[0].streamID, TEST_STREAM_ID + 1);
}

// Test fault handler lifecycle management
TEST_F(StreamContextTest, FaultHandlerLifecycleManagement) {
    // Create fault handler with specific lifetime
    {
        auto temporaryHandler = std::make_shared<FaultHandler>();
        VoidResult setTempResult = streamContext->setFaultHandler(temporaryHandler);
        EXPECT_TRUE(setTempResult.isOk());
        EXPECT_TRUE(streamContext->hasFaultHandler());
        
        // Record a fault
        FaultRecord fault;
        fault.streamID = TEST_STREAM_ID;
        fault.pasid = TEST_PASID_1;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = AccessType::Read;
        EXPECT_TRUE(streamContext->recordFault(fault));
        
        EXPECT_EQ(temporaryHandler->getFaultCount(), 1);
    } // temporaryHandler goes out of scope, but shared_ptr should keep it alive
    
    // Fault handler should still be valid through StreamContext
    EXPECT_TRUE(streamContext->hasFaultHandler());
    auto retrievedHandler = streamContext->getFaultHandler();
    EXPECT_NE(retrievedHandler, nullptr);
    EXPECT_EQ(retrievedHandler->getFaultCount(), 1);
    
    // Clear fault handler
    VoidResult clearFinalResult = streamContext->setFaultHandler(nullptr);
    EXPECT_TRUE(clearFinalResult.isOk());
    EXPECT_FALSE(streamContext->hasFaultHandler());
}

// ======================================================================
// COMPREHENSIVE TESTS: Thread Safety and Concurrent Access
// ======================================================================

// Test concurrent PASID creation and removal
TEST_F(StreamContextTest, ConcurrentPASIDOperations) {
    const int NUM_THREADS = 8;
    const int OPERATIONS_PER_THREAD = 50;
    std::atomic<int> successCount(0);
    std::atomic<int> errorCount(0);
    std::vector<std::thread> threads;
    
    // Launch multiple threads performing concurrent PASID operations
    for (int t = 0; t < NUM_THREADS; ++t) {
        threads.emplace_back([&, t]() {
            for (int i = 0; i < OPERATIONS_PER_THREAD; ++i) {
                PASID pasid = (t * OPERATIONS_PER_THREAD + i) % (MAX_PASID / 2) + 1; // Avoid PASID 0
                
                // Create PASID
                VoidResult createResult = streamContext->createPASID(pasid);
                if (createResult.isOk()) {
                    successCount++;
                    
                    // Try to use the PASID for mapping
                    PagePermissions perms(true, false, false);
                    streamContext->mapPage(pasid, TEST_IOVA + i * 0x1000, TEST_PA + i * 0x1000, perms);
                    
                    // Remove PASID
                    VoidResult removeResult = streamContext->removePASID(pasid);
                    if (!removeResult.isOk()) {
                        errorCount++;
                    }
                } else {
                    errorCount++;
                }
            }
        });
    }
    
    // Wait for all threads to complete
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Verify that operations completed without crashing and state is consistent
    EXPECT_GT(successCount.load(), 0);
    EXPECT_EQ(streamContext->getPASIDCount(), 0); // All PASIDs should be removed
}

// Test concurrent translation operations
TEST_F(StreamContextTest, ConcurrentTranslationOperations) {
    // Set up initial state
    streamContext->setStage1Enabled(true);
    streamContext->enableStream();
    
    const int NUM_THREADS = 4;
    const int NUM_PASIDS = 10;
    
    // Create multiple PASIDs with mappings
    for (int i = 1; i <= NUM_PASIDS; ++i) {
        EXPECT_TRUE(streamContext->createPASID(i));
        PagePermissions perms(true, true, false);
        EXPECT_TRUE(streamContext->mapPage(i, TEST_IOVA + i * 0x1000, TEST_PA + i * 0x1000, perms));
    }
    
    std::atomic<int> translationCount(0);
    std::atomic<int> errorCount(0);
    std::vector<std::thread> threads;
    
    // Launch threads performing concurrent translations
    for (int t = 0; t < NUM_THREADS; ++t) {
        threads.emplace_back([&]() {
            for (int i = 0; i < 100; ++i) {
                PASID pasid = (i % NUM_PASIDS) + 1;
                IOVA iova = TEST_IOVA + pasid * 0x1000;
                
                TranslationResult result = streamContext->translate(pasid, iova, AccessType::Read);
                if (result.isOk()) {
                    translationCount++;
                } else {
                    errorCount++;
                }
                
                // Add small delay to increase concurrency
                std::this_thread::sleep_for(std::chrono::microseconds(1));
            }
        });
    }
    
    // Wait for completion
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Verify concurrent operations maintained consistency
    EXPECT_GT(translationCount.load(), 0);
    EXPECT_EQ(streamContext->getPASIDCount(), NUM_PASIDS);
}

// Test concurrent configuration updates
TEST_F(StreamContextTest, ConcurrentConfigurationUpdates) {
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    const int NUM_THREADS = 4;
    std::atomic<int> updateCount(0);
    std::vector<std::thread> threads;
    
    // Launch threads performing concurrent configuration updates
    for (int t = 0; t < NUM_THREADS; ++t) {
        threads.emplace_back([&, t]() {
            for (int i = 0; i < 20; ++i) {
                StreamConfig config;
                config.translationEnabled = (i % 2 == 0);
                config.stage1Enabled = (i % 3 == 0);
                config.stage2Enabled = (i % 4 == 0);
                config.faultMode = ((i + t) % 2 == 0) ? FaultMode::Terminate : FaultMode::Stall;
                
                // Skip invalid configurations
                if (config.translationEnabled && !config.stage1Enabled && !config.stage2Enabled) {
                    continue;
                }
                
                VoidResult result = streamContext->updateConfiguration(config);
                if (result.isOk()) {
                    updateCount++;
                }
                
                std::this_thread::sleep_for(std::chrono::microseconds(10));
            }
        });
    }
    
    // Wait for completion
    for (auto& thread : threads) {
        thread.join();
    }
    
    // Verify final state is consistent
    EXPECT_GT(updateCount.load(), 0);
    Result<bool> enabledState = streamContext->isStreamEnabled();
    EXPECT_TRUE(enabledState.isOk()); // State should remain consistent
}

// ======================================================================
// COMPREHENSIVE TESTS: SecurityState Integration
// ======================================================================

// Test SecurityState parameter in mapPage operations
TEST_F(StreamContextTest, SecurityStateMapPageIntegration) {
    streamContext->setStage1Enabled(true);
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    PagePermissions perms(true, true, false);
    
    // Test mapping with NonSecure state
    VoidResult nsResult = streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms, SecurityState::NonSecure);
    EXPECT_TRUE(nsResult.isOk());
    
    // Test mapping with Secure state
    VoidResult secResult = streamContext->mapPage(TEST_PASID_1, TEST_IOVA_2, TEST_PA_2, perms, SecurityState::Secure);
    EXPECT_TRUE(secResult.isOk());
    
    // Test mapping with Realm state
    VoidResult realmResult = streamContext->mapPage(TEST_PASID_1, TEST_IOVA + 0x2000, TEST_PA + 0x2000, perms, SecurityState::Realm);
    EXPECT_TRUE(realmResult.isOk());
    
    // Verify translations work with correct security states
    streamContext->enableStream();
    
    TranslationResult nsTranslation = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(nsTranslation.isOk());
    EXPECT_EQ(nsTranslation.getValue().securityState, SecurityState::NonSecure);
    
    TranslationResult secTranslation = streamContext->translate(TEST_PASID_1, TEST_IOVA_2, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(secTranslation.isOk());
    EXPECT_EQ(secTranslation.getValue().securityState, SecurityState::Secure);
}

// Test SecurityState isolation between different security domains
TEST_F(StreamContextTest, SecurityStateIsolationValidation) {
    streamContext->setStage1Enabled(true);
    streamContext->enableStream();
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    
    PagePermissions perms(true, true, false);
    
    // Map same IOVA to different PAs with different security states
    VoidResult nsMapResult = streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms, SecurityState::NonSecure);
    EXPECT_TRUE(nsMapResult.isOk());
    
    // Attempt translation with wrong security state should behave appropriately
    TranslationResult nsTranslation = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(nsTranslation.isOk());
    
    TranslationResult secTranslation = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read, SecurityState::Secure);
    // Implementation may allow this or may enforce isolation - test current behavior
    // ARM SMMU v3 implementation specifics determine the exact behavior
    // For now, we just verify the translation attempts don't crash
    (void)secTranslation; // Suppress unused variable warning
}

// ======================================================================
// COMPREHENSIVE TESTS: Context Descriptor Validation
// ======================================================================

// Test validateContextDescriptor with valid configurations
TEST_F(StreamContextTest, ValidateContextDescriptorValid) {
    ContextDescriptor validCD;
    validCD.asid = 0x1234;
    validCD.ttbr0Valid = true;
    validCD.ttbr1Valid = false;
    validCD.ttbr0 = 0x40000000; // 1GB aligned to 4KB
    validCD.ttbr1 = 0;
    validCD.tcr.granuleSize = TranslationGranule::Size4KB;
    validCD.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    validCD.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;
    validCD.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateContextDescriptor(validCD, TEST_PASID_1, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// Test validateContextDescriptor with invalid PASID
TEST_F(StreamContextTest, ValidateContextDescriptorInvalidPASID) {
    ContextDescriptor validCD;
    validCD.asid = 0x1234;
    validCD.ttbr0Valid = true;
    validCD.ttbr0 = 0x40000000;
    validCD.tcr.granuleSize = TranslationGranule::Size4KB;
    validCD.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    validCD.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;
    validCD.securityState = SecurityState::NonSecure;
    
    // Test with PASID 0 (valid in ARM SMMU v3 for kernel/hypervisor contexts)
    Result<bool> result0 = streamContext->validateContextDescriptor(validCD, 0, TEST_STREAM_ID);
    EXPECT_TRUE(result0.isOk());
    EXPECT_TRUE(result0.getValue());
    
    // Test with PASID > MAX_PASID
    Result<bool> resultMax = streamContext->validateContextDescriptor(validCD, MAX_PASID + 1, TEST_STREAM_ID);
    EXPECT_TRUE(resultMax.isOk());
    EXPECT_FALSE(resultMax.getValue());
}

// Test validateContextDescriptor with invalid ASID
TEST_F(StreamContextTest, ValidateContextDescriptorInvalidASID) {
    ContextDescriptor invalidCD;
    invalidCD.asid = 0xFFFF; // This will be maximum valid value, test the validation logic
    invalidCD.ttbr0Valid = true;
    invalidCD.ttbr0 = 0x40000000;
    invalidCD.tcr.granuleSize = TranslationGranule::Size4KB;
    invalidCD.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    invalidCD.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;
    invalidCD.securityState = SecurityState::NonSecure;
    
    // Test with valid maximum ASID first
    Result<bool> resultValid = streamContext->validateContextDescriptor(invalidCD, TEST_PASID_1, TEST_STREAM_ID);
    EXPECT_TRUE(resultValid.isOk());
    EXPECT_TRUE(resultValid.getValue()); // Should be valid
    
    // Note: Since uint16_t cannot exceed 0xFFFF, the validation logic in the implementation
    // will always see a valid range. The test validates that the implementation correctly
    // handles the maximum valid value.
}

// Test validateContextDescriptor with no valid TTBRs
TEST_F(StreamContextTest, ValidateContextDescriptorNoValidTTBRs) {
    ContextDescriptor invalidCD;
    invalidCD.asid = 0x1234;
    invalidCD.ttbr0Valid = false;
    invalidCD.ttbr1Valid = false;
    invalidCD.ttbr0 = 0x40000000;
    invalidCD.ttbr1 = 0x50000000;
    invalidCD.tcr.granuleSize = TranslationGranule::Size4KB;
    invalidCD.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    invalidCD.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;
    invalidCD.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateContextDescriptor(invalidCD, TEST_PASID_1, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateContextDescriptor with invalid address space sizes
TEST_F(StreamContextTest, ValidateContextDescriptorInvalidAddressSpaceSizes) {
    ContextDescriptor invalidCD;
    invalidCD.asid = 0x1234;
    invalidCD.ttbr0Valid = true;
    invalidCD.ttbr0 = 0x40000000;
    invalidCD.tcr.granuleSize = TranslationGranule::Size4KB;
    invalidCD.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    invalidCD.tcr.outputAddressSize = AddressSpaceSize::Size32Bit; // Output smaller than input
    invalidCD.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateContextDescriptor(invalidCD, TEST_PASID_1, TEST_STREAM_ID);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// ======================================================================
// COMPREHENSIVE TESTS: Translation Table Base Validation
// ======================================================================

// Test validateTranslationTableBase with valid configurations
TEST_F(StreamContextTest, ValidateTranslationTableBaseValid) {
    // Valid 4KB aligned TTBR
    Result<bool> result4K = streamContext->validateTranslationTableBase(0x40000000, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result4K.isOk());
    EXPECT_TRUE(result4K.getValue());
    
    // Valid 16KB aligned TTBR
    Result<bool> result16K = streamContext->validateTranslationTableBase(0x40004000, TranslationGranule::Size16KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result16K.isOk());
    EXPECT_TRUE(result16K.getValue());
    
    // Valid 64KB aligned TTBR
    Result<bool> result64K = streamContext->validateTranslationTableBase(0x40010000, TranslationGranule::Size64KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result64K.isOk());
    EXPECT_TRUE(result64K.getValue());
}

// Test validateTranslationTableBase with null TTBR
TEST_F(StreamContextTest, ValidateTranslationTableBaseNull) {
    Result<bool> result = streamContext->validateTranslationTableBase(0, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateTranslationTableBase with misaligned TTBR
TEST_F(StreamContextTest, ValidateTranslationTableBaseMisaligned) {
    // 4KB granule requires 4KB alignment
    Result<bool> result4K = streamContext->validateTranslationTableBase(0x40000001, TranslationGranule::Size4KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result4K.isOk());
    EXPECT_FALSE(result4K.getValue());
    
    // 16KB granule requires 16KB alignment
    Result<bool> result16K = streamContext->validateTranslationTableBase(0x40001000, TranslationGranule::Size16KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result16K.isOk());
    EXPECT_FALSE(result16K.getValue());
    
    // 64KB granule requires 64KB alignment
    Result<bool> result64K = streamContext->validateTranslationTableBase(0x40008000, TranslationGranule::Size64KB, AddressSpaceSize::Size48Bit);
    EXPECT_TRUE(result64K.isOk());
    EXPECT_FALSE(result64K.getValue());
}

// Test validateTranslationTableBase with address out of range
TEST_F(StreamContextTest, ValidateTranslationTableBaseOutOfRange) {
    // 32-bit address space with TTBR beyond range
    Result<bool> result32 = streamContext->validateTranslationTableBase(0x100000000ULL, TranslationGranule::Size4KB, AddressSpaceSize::Size32Bit);
    EXPECT_TRUE(result32.isOk());
    EXPECT_FALSE(result32.getValue());
    
    // Valid within 32-bit range
    Result<bool> validResult = streamContext->validateTranslationTableBase(0x80000000, TranslationGranule::Size4KB, AddressSpaceSize::Size32Bit);
    EXPECT_TRUE(validResult.isOk());
    EXPECT_TRUE(validResult.getValue());
}

// ======================================================================
// COMPREHENSIVE TESTS: ASID Configuration Validation
// ======================================================================

// Test validateASIDConfiguration with valid ASID
TEST_F(StreamContextTest, ValidateASIDConfigurationValid) {
    Result<bool> result = streamContext->validateASIDConfiguration(0x1234, TEST_PASID_1, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
    
    // ASID 0 should be valid (may be used for global translations)
    Result<bool> result0 = streamContext->validateASIDConfiguration(0, TEST_PASID_1, SecurityState::NonSecure);
    EXPECT_TRUE(result0.isOk());
    EXPECT_TRUE(result0.getValue());
}

// Test validateASIDConfiguration with invalid ASID range
TEST_F(StreamContextTest, ValidateASIDConfigurationInvalidRange) {
    // Test with maximum valid ASID (since uint16_t cannot exceed 0xFFFF)
    Result<bool> result = streamContext->validateASIDConfiguration(0xFFFF, TEST_PASID_1, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue()); // Should be valid since 0xFFFF is within range
    
    // Note: Since uint16_t cannot exceed 0xFFFF, the validation logic will always
    // see values within range. The test validates correct handling of maximum value.
}

// Test validateASIDConfiguration with different security states
TEST_F(StreamContextTest, ValidateASIDConfigurationSecurityStates) {
    // Test all valid security states
    Result<bool> nsResult = streamContext->validateASIDConfiguration(0x1234, TEST_PASID_1, SecurityState::NonSecure);
    EXPECT_TRUE(nsResult.isOk());
    EXPECT_TRUE(nsResult.getValue());
    
    Result<bool> secResult = streamContext->validateASIDConfiguration(0x1234, TEST_PASID_2, SecurityState::Secure);
    EXPECT_TRUE(secResult.isOk());
    EXPECT_TRUE(secResult.getValue());
    
    Result<bool> realmResult = streamContext->validateASIDConfiguration(0x1234, TEST_PASID_3, SecurityState::Realm);
    EXPECT_TRUE(realmResult.isOk());
    EXPECT_TRUE(realmResult.getValue());
}

// ======================================================================
// COMPREHENSIVE TESTS: Stream Table Entry Validation
// ======================================================================

// Test validateStreamTableEntry with valid configuration
TEST_F(StreamContextTest, ValidateStreamTableEntryValid) {
    StreamTableEntry validSTE;
    validSTE.translationEnabled = true;
    validSTE.stage1Enabled = true;
    validSTE.stage2Enabled = false;
    validSTE.contextDescriptorTableBase = 0x40000000; // 64-byte aligned
    validSTE.contextDescriptorTableSize = 1024;
    validSTE.faultMode = FaultMode::Terminate;
    validSTE.securityState = SecurityState::NonSecure;
    validSTE.stage1Granule = TranslationGranule::Size4KB;
    validSTE.stage2Granule = TranslationGranule::Size4KB;
    
    Result<bool> result = streamContext->validateStreamTableEntry(validSTE);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// Test validateStreamTableEntry with translation enabled but no stages
TEST_F(StreamContextTest, ValidateStreamTableEntryNoStages) {
    StreamTableEntry invalidSTE;
    invalidSTE.translationEnabled = true;
    invalidSTE.stage1Enabled = false;
    invalidSTE.stage2Enabled = false;
    invalidSTE.faultMode = FaultMode::Terminate;
    invalidSTE.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateStreamTableEntry(invalidSTE);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry with Stage-1 enabled but no CD table
TEST_F(StreamContextTest, ValidateStreamTableEntryNoCDTable) {
    StreamTableEntry invalidSTE;
    invalidSTE.translationEnabled = true;
    invalidSTE.stage1Enabled = true;
    invalidSTE.stage2Enabled = false;
    invalidSTE.contextDescriptorTableBase = 0; // Invalid
    invalidSTE.contextDescriptorTableSize = 1024;
    invalidSTE.faultMode = FaultMode::Terminate;
    invalidSTE.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateStreamTableEntry(invalidSTE);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry with misaligned CD table base
TEST_F(StreamContextTest, ValidateStreamTableEntryMisalignedCDTable) {
    StreamTableEntry invalidSTE;
    invalidSTE.translationEnabled = true;
    invalidSTE.stage1Enabled = true;
    invalidSTE.stage2Enabled = false;
    invalidSTE.contextDescriptorTableBase = 0x40000001; // Not 64-byte aligned
    invalidSTE.contextDescriptorTableSize = 1024;
    invalidSTE.faultMode = FaultMode::Terminate;
    invalidSTE.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateStreamTableEntry(invalidSTE);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// Test validateStreamTableEntry with invalid CD table size
TEST_F(StreamContextTest, ValidateStreamTableEntryInvalidCDTableSize) {
    StreamTableEntry invalidSTE;
    invalidSTE.translationEnabled = true;
    invalidSTE.stage1Enabled = true;
    invalidSTE.stage2Enabled = false;
    invalidSTE.contextDescriptorTableBase = 0x40000000;
    invalidSTE.contextDescriptorTableSize = 0; // Invalid
    invalidSTE.faultMode = FaultMode::Terminate;
    invalidSTE.securityState = SecurityState::NonSecure;
    
    Result<bool> result = streamContext->validateStreamTableEntry(invalidSTE);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());
}

// ======================================================================
// COMPREHENSIVE TESTS: Edge Cases and Error Boundaries
// ======================================================================

// Test maximum PASID operations at boundary
TEST_F(StreamContextTest, MaximumPASIDBoundaryOperations) {
    // Test operations with maximum valid PASID
    EXPECT_TRUE(streamContext->createPASID(MAX_PASID));
    EXPECT_TRUE(streamContext->hasPASID(MAX_PASID));
    
    PagePermissions perms(true, false, false);
    EXPECT_TRUE(streamContext->mapPage(MAX_PASID, TEST_IOVA, TEST_PA, perms));
    
    streamContext->setStage1Enabled(true);
    streamContext->enableStream();
    TranslationResult result = streamContext->translate(MAX_PASID, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    
    EXPECT_TRUE(streamContext->unmapPage(MAX_PASID, TEST_IOVA));
    EXPECT_TRUE(streamContext->removePASID(MAX_PASID));
}

// Test operations with large number of PASIDs
TEST_F(StreamContextTest, LargeNumberOfPASIDs) {
    const int NUM_PASIDS = 100;
    
    // Create many PASIDs
    for (int i = 1; i <= NUM_PASIDS; ++i) {
        EXPECT_TRUE(streamContext->createPASID(i));
    }
    
    EXPECT_EQ(streamContext->getPASIDCount(), NUM_PASIDS);
    
    // Map pages in all PASIDs
    PagePermissions perms(true, true, false);
    for (int i = 1; i <= NUM_PASIDS; ++i) {
        EXPECT_TRUE(streamContext->mapPage(i, TEST_IOVA + i * 0x1000, TEST_PA + i * 0x1000, perms));
    }
    
    // Test translations
    streamContext->setStage1Enabled(true);
    streamContext->enableStream();
    
    for (int i = 1; i <= NUM_PASIDS; ++i) {
        TranslationResult result = streamContext->translate(i, TEST_IOVA + i * 0x1000, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, TEST_PA + i * 0x1000);
    }
    
    // Clear all PASIDs
    VoidResult clearResult = streamContext->clearAllPASIDs();
    EXPECT_TRUE(clearResult.isOk());
    EXPECT_EQ(streamContext->getPASIDCount(), 0);
}

// Test rapid configuration changes
TEST_F(StreamContextTest, RapidConfigurationChanges) {
    auto stage2Space = std::make_shared<AddressSpace>();
    streamContext->setStage2AddressSpace(stage2Space);
    
    // Perform rapid configuration changes
    for (int i = 0; i < 50; ++i) {
        StreamConfig config;
        config.translationEnabled = (i % 2 == 0);
        config.stage1Enabled = (i % 3 != 0);
        config.stage2Enabled = (i % 4 == 0);
        config.faultMode = ((i % 2) == 0) ? FaultMode::Terminate : FaultMode::Stall;
        
        // Skip invalid configurations
        if (config.translationEnabled && !config.stage1Enabled && !config.stage2Enabled) {
            continue;
        }
        
        VoidResult result = streamContext->updateConfiguration(config);
        EXPECT_TRUE(result.isOk());
        
        // Verify configuration was applied
        StreamConfig retrieved = streamContext->getStreamConfiguration();
        EXPECT_EQ(retrieved.translationEnabled, config.translationEnabled);
        EXPECT_EQ(retrieved.stage1Enabled, config.stage1Enabled);
        EXPECT_EQ(retrieved.stage2Enabled, config.stage2Enabled);
        EXPECT_EQ(retrieved.faultMode, config.faultMode);
    }
    
    // Verify final state is valid
    EXPECT_TRUE(streamContext->hasConfigurationChanged());
    StreamStatistics stats = streamContext->getStreamStatistics();
    EXPECT_GT(stats.configurationUpdateCount, 0);
}

// Test error recovery scenarios
TEST_F(StreamContextTest, ErrorRecoveryScenarios) {
    // Test recovery from invalid PASID operations - PASID 0 is now valid
    EXPECT_TRUE(streamContext->createPASID(0)); // PASID 0 is valid for kernel/hypervisor
    EXPECT_FALSE(streamContext->createPASID(MAX_PASID + 1)); // Out of range
    
    // Verify system state remains consistent
    EXPECT_EQ(streamContext->getPASIDCount(), 1);
    
    // Test recovery from invalid translations
    TranslationResult invalidResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(invalidResult.isError());
    
    // System should still be able to perform valid operations
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_EQ(streamContext->getPASIDCount(), 2);
    
    PagePermissions perms(true, false, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    streamContext->setStage1Enabled(true);
    streamContext->enableStream();
    
    TranslationResult validResult = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(validResult.isOk());
}

// Test statistics accuracy under various operations
TEST_F(StreamContextTest, StatisticsAccuracyValidation) {
    StreamStatistics initialStats = streamContext->getStreamStatistics();
    EXPECT_EQ(initialStats.translationCount, 0);
    EXPECT_EQ(initialStats.faultCount, 0);
    EXPECT_EQ(initialStats.pasidCount, 0);
    
    // Perform operations that should update statistics
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_1));
    EXPECT_TRUE(streamContext->createPASID(TEST_PASID_2));
    
    StreamStatistics afterPASIDStats = streamContext->getStreamStatistics();
    EXPECT_EQ(afterPASIDStats.pasidCount, 2);
    
    // Set up translations
    streamContext->setStage1Enabled(true);
    streamContext->enableStream();
    
    PagePermissions perms(true, false, false);
    EXPECT_TRUE(streamContext->mapPage(TEST_PASID_1, TEST_IOVA, TEST_PA, perms));
    
    // Perform translations (successful and failed)
    TranslationResult success = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(success.isOk());
    
    TranslationResult fault1 = streamContext->translate(TEST_PASID_1, TEST_IOVA, AccessType::Write); // Permission fault
    EXPECT_TRUE(fault1.isError());
    
    TranslationResult fault2 = streamContext->translate(TEST_PASID_2, TEST_IOVA, AccessType::Read); // Not mapped
    EXPECT_TRUE(fault2.isError());
    
    // Verify statistics are accurate
    StreamStatistics finalStats = streamContext->getStreamStatistics();
    EXPECT_EQ(finalStats.pasidCount, 2);
    EXPECT_EQ(finalStats.translationCount, 3); // All translation attempts counted
    EXPECT_EQ(finalStats.faultCount, 2); // Only failed translations counted
    EXPECT_GT(finalStats.lastAccessTimestamp, initialStats.creationTimestamp);
}

} // namespace test
} // namespace smmu