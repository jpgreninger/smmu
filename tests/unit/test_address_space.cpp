// ARM SMMU v3 AddressSpace Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/address_space.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

class AddressSpaceTest : public ::testing::Test {
protected:
    void SetUp() override {
        addressSpace = std::make_unique<AddressSpace>();
    }

    void TearDown() override {
        addressSpace.reset();
    }

    std::unique_ptr<AddressSpace> addressSpace;
    
    // Test helper constants
    static constexpr IOVA TEST_IOVA_1 = 0x10000000;
    static constexpr IOVA TEST_IOVA_2 = 0x20000000;
    static constexpr PA TEST_PA_1 = 0x40000000;
    static constexpr PA TEST_PA_2 = 0x50000000;
};

// Test default construction
TEST_F(AddressSpaceTest, DefaultConstruction) {
    ASSERT_NE(addressSpace, nullptr);
    
    // Verify that a translation on empty address space fails
    TranslationResult result = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

// Test single page mapping
TEST_F(AddressSpaceTest, SinglePageMapping) {
    PagePermissions perms(true, true, false);  // Read-write, no execute
    
    // Map a single page
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    
    // Verify read access succeeds
    TranslationResult readResult = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(readResult.isOk());
    EXPECT_EQ(readResult.getValue().physicalAddress, TEST_PA_1);
    
    // Verify write access succeeds
    TranslationResult writeResult = addressSpace->translatePage(TEST_IOVA_1, AccessType::Write);
    EXPECT_TRUE(writeResult.isOk());
    EXPECT_EQ(writeResult.getValue().physicalAddress, TEST_PA_1);
    
    // Verify execute access fails (not permitted)
    TranslationResult executeResult = addressSpace->translatePage(TEST_IOVA_1, AccessType::Execute);
    EXPECT_TRUE(executeResult.isError());
    EXPECT_EQ(executeResult.getError(), SMMUError::PagePermissionViolation);
}

// Test multiple page mappings
TEST_F(AddressSpaceTest, MultiplePageMappings) {
    PagePermissions perms1(true, false, false);  // Read-only
    PagePermissions perms2(true, true, true);    // Read-write-execute
    
    // Map two pages with different permissions
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms1);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, perms2);
    
    // Test first page (read-only)
    TranslationResult read1 = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(read1.isOk());
    EXPECT_EQ(read1.getValue().physicalAddress, TEST_PA_1);
    
    TranslationResult write1 = addressSpace->translatePage(TEST_IOVA_1, AccessType::Write);
    EXPECT_TRUE(write1.isError());
    EXPECT_EQ(write1.getError(), SMMUError::PagePermissionViolation);
    
    // Test second page (read-write-execute)
    TranslationResult read2 = addressSpace->translatePage(TEST_IOVA_2, AccessType::Read);
    EXPECT_TRUE(read2.isOk());
    EXPECT_EQ(read2.getValue().physicalAddress, TEST_PA_2);
    
    TranslationResult write2 = addressSpace->translatePage(TEST_IOVA_2, AccessType::Write);
    EXPECT_TRUE(write2.isOk());
    EXPECT_EQ(write2.getValue().physicalAddress, TEST_PA_2);
    
    TranslationResult execute2 = addressSpace->translatePage(TEST_IOVA_2, AccessType::Execute);
    EXPECT_TRUE(execute2.isOk());
    EXPECT_EQ(execute2.getValue().physicalAddress, TEST_PA_2);
}

// Test page remapping (overwrite existing mapping)
TEST_F(AddressSpaceTest, PageRemapping) {
    PagePermissions oldPerms(true, false, false);  // Read-only
    PagePermissions newPerms(true, true, false);   // Read-write
    
    // Initial mapping
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, oldPerms);
    
    // Verify initial mapping
    TranslationResult initialResult = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(initialResult.isOk());
    EXPECT_EQ(initialResult.getValue().physicalAddress, TEST_PA_1);
    
    TranslationResult initialWrite = addressSpace->translatePage(TEST_IOVA_1, AccessType::Write);
    EXPECT_TRUE(initialWrite.isError());
    
    // Remap to different PA with different permissions
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_2, newPerms);
    
    // Verify new mapping
    TranslationResult newResult = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(newResult.isOk());
    EXPECT_EQ(newResult.getValue().physicalAddress, TEST_PA_2);  // Should be new PA
    
    TranslationResult newWrite = addressSpace->translatePage(TEST_IOVA_1, AccessType::Write);
    EXPECT_TRUE(newWrite.isOk());  // Should now be allowed
}

// Test page unmapping
TEST_F(AddressSpaceTest, PageUnmapping) {
    PagePermissions perms(true, true, false);
    
    // Map a page
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    
    // Verify mapping exists
    TranslationResult beforeUnmap = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(beforeUnmap.isOk());
    
    // Unmap the page
    addressSpace->unmapPage(TEST_IOVA_1);
    
    // Verify mapping no longer exists
    TranslationResult afterUnmap = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(afterUnmap.isError());
    EXPECT_EQ(afterUnmap.getError(), SMMUError::PageNotMapped);
}

// Test unmapping non-existent page (should not crash)
TEST_F(AddressSpaceTest, UnmapNonExistentPage) {
    // This should not crash or cause issues
    EXPECT_NO_THROW(addressSpace->unmapPage(TEST_IOVA_1));
    
    // Verify state is still consistent
    TranslationResult result = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(result.isError());
}

// Test address space statistics
TEST_F(AddressSpaceTest, AddressSpaceStatistics) {
    // Initially empty
    Result<size_t> pageCountResult = addressSpace->getPageCount();
    EXPECT_TRUE(pageCountResult.isOk());
    EXPECT_EQ(pageCountResult.getValue(), 0);
    
    // Add some pages
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, perms);
    
    Result<size_t> count1 = addressSpace->getPageCount();
    EXPECT_TRUE(count1.isOk());
    EXPECT_EQ(count1.getValue(), 2);
    
    // Remove one page
    addressSpace->unmapPage(TEST_IOVA_1);
    Result<size_t> count2 = addressSpace->getPageCount();
    EXPECT_TRUE(count2.isOk());
    EXPECT_EQ(count2.getValue(), 1);
    
    // Remove remaining page
    addressSpace->unmapPage(TEST_IOVA_2);
    Result<size_t> count3 = addressSpace->getPageCount();
    EXPECT_TRUE(count3.isOk());
    EXPECT_EQ(count3.getValue(), 0);
}

// Test sparse address space efficiency
TEST_F(AddressSpaceTest, SparseAddressSpace) {
    PagePermissions perms(true, false, false);
    
    // Map pages with large gaps between them (sparse)
    IOVA sparseAddresses[] = {
        0x1000000000,  // 64GB
        0x2000000000,  // 128GB  
        0x4000000000,  // 256GB
        0x8000000000   // 512GB
    };
    
    PA physicalBases[] = {
        0x40000000,
        0x50000000,
        0x60000000,
        0x70000000
    };
    
    // Map all sparse pages
    for (size_t i = 0; i < 4; ++i) {
        addressSpace->mapPage(sparseAddresses[i], physicalBases[i], perms);
    }
    
    Result<size_t> sparseCount = addressSpace->getPageCount();
    EXPECT_TRUE(sparseCount.isOk());
    EXPECT_EQ(sparseCount.getValue(), 4);
    
    // Verify all mappings work correctly
    for (size_t i = 0; i < 4; ++i) {
        TranslationResult result = addressSpace->translatePage(sparseAddresses[i], AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, physicalBases[i]);
    }
}

// Test page alignment requirements
TEST_F(AddressSpaceTest, PageAlignment) {
    PagePermissions perms(true, true, false);
    
    // Test that unaligned addresses are handled appropriately
    IOVA unalignedIOVA = 0x12345678;  // Not page-aligned
    PA unalignedPA = 0x87654321;      // Not page-aligned
    
    // The AddressSpace should handle alignment internally
    // This test verifies that the interface doesn't crash with unaligned inputs
    EXPECT_NO_THROW(addressSpace->mapPage(unalignedIOVA, unalignedPA, perms));
}

// Test clear all mappings
TEST_F(AddressSpaceTest, ClearAllMappings) {
    PagePermissions perms(true, true, false);
    
    // Add multiple mappings
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, perms);
    Result<size_t> countBefore = addressSpace->getPageCount();
    EXPECT_TRUE(countBefore.isOk());
    EXPECT_EQ(countBefore.getValue(), 2);
    
    // Clear all mappings
    addressSpace->clear();
    Result<size_t> countAfter = addressSpace->getPageCount();
    EXPECT_TRUE(countAfter.isOk());
    EXPECT_EQ(countAfter.getValue(), 0);
    
    // Verify no mappings exist
    TranslationResult result1 = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    TranslationResult result2 = addressSpace->translatePage(TEST_IOVA_2, AccessType::Read);
    
    EXPECT_TRUE(result1.isError());
    EXPECT_TRUE(result2.isError());
}

// Test copy constructor behavior
TEST_F(AddressSpaceTest, CopySemantics) {
    PagePermissions perms(true, false, false);
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    
    // Test that we can create a copy of the address space
    AddressSpace copySpace(*addressSpace);
    
    // Verify copy has the same mapping
    TranslationResult originalResult = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    TranslationResult copyResult = copySpace.translatePage(TEST_IOVA_1, AccessType::Read);
    
    EXPECT_TRUE(originalResult.isOk());
    EXPECT_TRUE(copyResult.isOk());
    EXPECT_EQ(originalResult.getValue().physicalAddress, copyResult.getValue().physicalAddress);
    
    // Verify independent modification after copy
    PagePermissions newPerms(true, true, false);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, newPerms);
    
    // Original should have both pages, copy should have only first
    Result<bool> mapped1 = addressSpace->isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(mapped1.isOk());
    EXPECT_TRUE(mapped1.getValue());
    
    Result<bool> mapped2 = copySpace.isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(mapped2.isOk());
    EXPECT_FALSE(mapped2.getValue());
}

// Test assignment operator behavior
TEST_F(AddressSpaceTest, AssignmentOperator) {
    PagePermissions perms1(true, false, false);
    PagePermissions perms2(true, true, false);
    
    // Set up source address space
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms1);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, perms2);
    
    // Create target address space
    AddressSpace targetSpace;
    Result<size_t> targetCount1 = targetSpace.getPageCount();
    EXPECT_TRUE(targetCount1.isOk());
    EXPECT_EQ(targetCount1.getValue(), 0);
    
    // Test assignment
    targetSpace = *addressSpace;
    
    // Verify assignment worked
    Result<size_t> targetCount2 = targetSpace.getPageCount();
    EXPECT_TRUE(targetCount2.isOk());
    EXPECT_EQ(targetCount2.getValue(), 2);
    
    Result<bool> targetMapped1 = targetSpace.isPageMapped(TEST_IOVA_1);
    EXPECT_TRUE(targetMapped1.isOk());
    EXPECT_TRUE(targetMapped1.getValue());
    
    Result<bool> targetMapped2 = targetSpace.isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(targetMapped2.isOk());
    EXPECT_TRUE(targetMapped2.getValue());
    
    // Test self-assignment safety
    *addressSpace = *addressSpace;
    Result<size_t> selfCount = addressSpace->getPageCount();
    EXPECT_TRUE(selfCount.isOk());
    EXPECT_EQ(selfCount.getValue(), 2);
}

// Test isPageMapped API method
TEST_F(AddressSpaceTest, IsPageMappedAPI) {
    PagePermissions perms(true, true, false);
    
    // Initially no pages mapped
    Result<bool> mapped1 = addressSpace->isPageMapped(TEST_IOVA_1);
    EXPECT_TRUE(mapped1.isOk());
    EXPECT_FALSE(mapped1.getValue());
    
    Result<bool> mapped2 = addressSpace->isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(mapped2.isOk());
    EXPECT_FALSE(mapped2.getValue());
    
    // Map first page
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    Result<bool> mapped3 = addressSpace->isPageMapped(TEST_IOVA_1);
    EXPECT_TRUE(mapped3.isOk());
    EXPECT_TRUE(mapped3.getValue());
    
    Result<bool> mapped4 = addressSpace->isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(mapped4.isOk());
    EXPECT_FALSE(mapped4.getValue());
    
    // Map second page
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, perms);
    Result<bool> mapped5 = addressSpace->isPageMapped(TEST_IOVA_1);
    EXPECT_TRUE(mapped5.isOk());
    EXPECT_TRUE(mapped5.getValue());
    
    Result<bool> mapped6 = addressSpace->isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(mapped6.isOk());
    EXPECT_TRUE(mapped6.getValue());
    
    // Unmap first page
    addressSpace->unmapPage(TEST_IOVA_1);
    Result<bool> mapped7 = addressSpace->isPageMapped(TEST_IOVA_1);
    EXPECT_TRUE(mapped7.isOk());
    EXPECT_FALSE(mapped7.getValue());
    
    Result<bool> mapped8 = addressSpace->isPageMapped(TEST_IOVA_2);
    EXPECT_TRUE(mapped8.isOk());
    EXPECT_TRUE(mapped8.getValue());
}

// Test getPagePermissions API method
TEST_F(AddressSpaceTest, GetPagePermissionsAPI) {
    PagePermissions readOnlyPerms(true, false, false);
    PagePermissions readWritePerms(true, true, false);
    PagePermissions fullPerms(true, true, true);
    
    // Map pages with different permissions
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, readOnlyPerms);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, fullPerms);
    
    // Test permission retrieval
    Result<PagePermissions> permsResult1 = addressSpace->getPagePermissions(TEST_IOVA_1);
    EXPECT_TRUE(permsResult1.isOk());
    PagePermissions perms1 = permsResult1.getValue();
    EXPECT_TRUE(perms1.read);
    EXPECT_FALSE(perms1.write);
    EXPECT_FALSE(perms1.execute);
    
    Result<PagePermissions> permsResult2 = addressSpace->getPagePermissions(TEST_IOVA_2);
    EXPECT_TRUE(permsResult2.isOk());
    PagePermissions perms2 = permsResult2.getValue();
    EXPECT_TRUE(perms2.read);
    EXPECT_TRUE(perms2.write);
    EXPECT_TRUE(perms2.execute);
    
    // Test permissions for unmapped page (should return error)
    IOVA unmappedIOVA = 0x99000000;
    Result<PagePermissions> emptyPermsResult = addressSpace->getPagePermissions(unmappedIOVA);
    EXPECT_TRUE(emptyPermsResult.isError());
    EXPECT_EQ(emptyPermsResult.getError(), SMMUError::PageNotMapped);
}

// Test boundary conditions and edge cases
TEST_F(AddressSpaceTest, BoundaryConditions) {
    PagePermissions perms(true, true, false);
    
    // Test zero address
    IOVA zeroAddr = 0x0;
    addressSpace->mapPage(zeroAddr, TEST_PA_1, perms);
    Result<bool> zeroMapped = addressSpace->isPageMapped(zeroAddr);
    EXPECT_TRUE(zeroMapped.isOk());
    EXPECT_TRUE(zeroMapped.getValue());
    
    // Test maximum representable address within 52-bit ARM SMMU v3 address space
    IOVA maxAddr = 0x000FFFFFFFFFF000ULL;  // Page-aligned max address within 52-bit space
    addressSpace->mapPage(maxAddr, TEST_PA_2, perms);
    Result<bool> maxMapped = addressSpace->isPageMapped(maxAddr);
    EXPECT_TRUE(maxMapped.isOk());
    EXPECT_TRUE(maxMapped.getValue());
    
    // Test addresses that differ only in page offset
    IOVA baseAddr = 0x12345000;
    IOVA offsetAddr1 = 0x12345001;  // Same page, different offset
    IOVA offsetAddr2 = 0x12345FFF;  // Same page, max offset
    
    addressSpace->mapPage(baseAddr, TEST_PA_1, perms);
    
    // All should map to same page
    Result<bool> baseMapped = addressSpace->isPageMapped(baseAddr);
    EXPECT_TRUE(baseMapped.isOk());
    EXPECT_TRUE(baseMapped.getValue());
    
    Result<bool> offset1Mapped = addressSpace->isPageMapped(offsetAddr1);
    EXPECT_TRUE(offset1Mapped.isOk());
    EXPECT_TRUE(offset1Mapped.getValue());
    
    Result<bool> offset2Mapped = addressSpace->isPageMapped(offsetAddr2);
    EXPECT_TRUE(offset2Mapped.isOk());
    EXPECT_TRUE(offset2Mapped.getValue());
    
    // Translation should preserve page offset
    TranslationResult result1 = addressSpace->translatePage(offsetAddr1, AccessType::Read);
    TranslationResult result2 = addressSpace->translatePage(offsetAddr2, AccessType::Read);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, TEST_PA_1 + 0x001);
    EXPECT_EQ(result2.getValue().physicalAddress, TEST_PA_1 + 0xFFF);
}

// Test cache interface methods (even though they're placeholders)
TEST_F(AddressSpaceTest, CacheInterfaceMethods) {
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, perms);
    
    // These methods should not crash and should not affect functionality
    EXPECT_NO_THROW(addressSpace->invalidateCache());
    EXPECT_NO_THROW(addressSpace->invalidatePage(TEST_IOVA_1));
    EXPECT_NO_THROW(addressSpace->invalidatePage(0x99000000));  // Non-existent page
    
    // Verify functionality is unchanged after cache operations
    TranslationResult result = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA_1);
}

// Test large-scale sparse mappings for performance validation
TEST_F(AddressSpaceTest, LargeScaleSparseMapping) {
    PagePermissions perms(true, false, false);
    
    // Create mappings with very large gaps (testing sparse efficiency)
    std::vector<IOVA> sparseAddresses;
    std::vector<PA> physicalAddresses;
    
    // Generate 1000 random sparse addresses
    for (int i = 0; i < 1000; ++i) {
        IOVA addr = (static_cast<uint64_t>(i) << 32) | 0x1000;  // 4GB gaps
        PA pa = 0x40000000ULL + (i * PAGE_SIZE);
        sparseAddresses.push_back(addr);
        physicalAddresses.push_back(pa);
        addressSpace->mapPage(addr, pa, perms);
    }
    
    Result<size_t> largeCount = addressSpace->getPageCount();
    EXPECT_TRUE(largeCount.isOk());
    EXPECT_EQ(largeCount.getValue(), 1000);
    
    // Verify all mappings work correctly
    for (size_t i = 0; i < sparseAddresses.size(); ++i) {
        TranslationResult result = addressSpace->translatePage(sparseAddresses[i], AccessType::Read);
        EXPECT_TRUE(result.isOk()) << "Failed for address " << std::hex << sparseAddresses[i];
        EXPECT_EQ(result.getValue().physicalAddress, physicalAddresses[i]);
    }
    
    // Test that unmapped addresses in gaps still fail
    IOVA gapAddr = (1ULL << 33);  // Address in gap
    TranslationResult gapResult = addressSpace->translatePage(gapAddr, AccessType::Read);
    EXPECT_TRUE(gapResult.isError());
    EXPECT_EQ(gapResult.getError(), SMMUError::PageNotMapped);
}

// Test ARM SMMU v3 fault type compliance
TEST_F(AddressSpaceTest, ARMSMMUv3FaultCompliance) {
    PagePermissions readOnlyPerms(true, false, false);
    PagePermissions writeOnlyPerms(false, true, false);  // Unusual but valid
    PagePermissions executeOnlyPerms(false, false, true);
    
    // Map pages with specific permission combinations
    addressSpace->mapPage(TEST_IOVA_1, TEST_PA_1, readOnlyPerms);
    addressSpace->mapPage(TEST_IOVA_2, TEST_PA_2, writeOnlyPerms);
    addressSpace->mapPage(0x30000000, 0x60000000, executeOnlyPerms);
    
    // Test translation faults (unmapped addresses)
    TranslationResult unmappedResult = addressSpace->translatePage(0x99000000, AccessType::Read);
    EXPECT_TRUE(unmappedResult.isError());
    EXPECT_EQ(unmappedResult.getError(), SMMUError::PageNotMapped);
    
    // Test permission faults for read-only page
    TranslationResult readOK = addressSpace->translatePage(TEST_IOVA_1, AccessType::Read);
    EXPECT_TRUE(readOK.isOk());
    
    TranslationResult writeFault = addressSpace->translatePage(TEST_IOVA_1, AccessType::Write);
    EXPECT_TRUE(writeFault.isError());
    EXPECT_EQ(writeFault.getError(), SMMUError::PagePermissionViolation);
    
    TranslationResult executeFault = addressSpace->translatePage(TEST_IOVA_1, AccessType::Execute);
    EXPECT_TRUE(executeFault.isError());
    EXPECT_EQ(executeFault.getError(), SMMUError::PagePermissionViolation);
    
    // Test permission faults for write-only page
    TranslationResult writeOK = addressSpace->translatePage(TEST_IOVA_2, AccessType::Write);
    EXPECT_TRUE(writeOK.isOk());
    
    TranslationResult readFault = addressSpace->translatePage(TEST_IOVA_2, AccessType::Read);
    EXPECT_TRUE(readFault.isError());
    EXPECT_EQ(readFault.getError(), SMMUError::PagePermissionViolation);
}

// Test page size and alignment compliance
TEST_F(AddressSpaceTest, PageSizeAlignmentCompliance) {
    PagePermissions perms(true, true, false);
    
    // Test that PAGE_SIZE constant is correct (4KB = 4096 bytes)
    EXPECT_EQ(PAGE_SIZE, 4096);
    EXPECT_EQ(PAGE_MASK, 4095);  // PAGE_SIZE - 1
    
    // Test page alignment behavior
    IOVA unalignedIOVA = 0x12345678;  // Not page-aligned
    PA unalignedPA = 0x87654321;      // Not page-aligned
    
    addressSpace->mapPage(unalignedIOVA, unalignedPA, perms);
    
    // Translation should work for any address in the same page
    TranslationResult result1 = addressSpace->translatePage(0x12345000, AccessType::Read);  // Page start
    TranslationResult result2 = addressSpace->translatePage(0x12345678, AccessType::Read);  // Original address
    TranslationResult result3 = addressSpace->translatePage(0x12345FFF, AccessType::Read);  // Page end
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_TRUE(result3.isOk());
    
    // Physical addresses should be page-aligned base plus offset
    PA expectedPABase = unalignedPA & ~PAGE_MASK;  // Page-aligned PA
    EXPECT_EQ(result1.getValue().physicalAddress, expectedPABase + 0x000);
    EXPECT_EQ(result2.getValue().physicalAddress, expectedPABase + 0x678);
    EXPECT_EQ(result3.getValue().physicalAddress, expectedPABase + 0xFFF);
    
    // Next page should not be mapped
    TranslationResult nextPageResult = addressSpace->translatePage(0x12346000, AccessType::Read);
    EXPECT_TRUE(nextPageResult.isError());
    EXPECT_EQ(nextPageResult.getError(), SMMUError::PageNotMapped);
}

// ==========================================================================
// Task 3.2: Address Space Operations - Comprehensive Test Suite
// ==========================================================================

// Test address range mapping operations - mapRange()
TEST_F(AddressSpaceTest, MapRangeBasicOperation) {
    PagePermissions perms(true, true, false);
    IOVA startIova = 0x10000000;
    IOVA endIova = 0x10005000;    // 5 pages (20KB)
    PA startPa = 0x40000000;
    
    // Map the range
    addressSpace->mapRange(startIova, endIova, startPa, perms);
    
    // Verify all pages in range are mapped correctly
    for (IOVA iova = startIova; iova <= endIova; iova += PAGE_SIZE) {
        Result<bool> rangeMapped = addressSpace->isPageMapped(iova);
        EXPECT_TRUE(rangeMapped.isOk());
        EXPECT_TRUE(rangeMapped.getValue());
        
        TranslationResult result = addressSpace->translatePage(iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        
        PA expectedPa = startPa + (iova - startIova);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPa);
    }
    
    // Verify page count is correct (6 pages: 0x10000000, 0x10001000, ..., 0x10005000)
    size_t expectedPages = ((endIova - startIova) / PAGE_SIZE) + 1;
    Result<size_t> rangeCount = addressSpace->getPageCount();
    EXPECT_TRUE(rangeCount.isOk());
    EXPECT_EQ(rangeCount.getValue(), expectedPages);
}

TEST_F(AddressSpaceTest, MapRangeContiguousMapping) {
    PagePermissions perms(true, false, false);
    IOVA startIova = 0x20000000;
    IOVA endIova = 0x2000F000;    // 16 pages (64KB)
    PA startPa = 0x50000000;
    
    addressSpace->mapRange(startIova, endIova, startPa, perms);
    
    // Test contiguous physical mapping
    for (size_t i = 0; i < 16; ++i) {
        IOVA testIova = startIova + (i * PAGE_SIZE);
        PA expectedPa = startPa + (i * PAGE_SIZE);
        
        TranslationResult result = addressSpace->translatePage(testIova, AccessType::Read);
        EXPECT_TRUE(result.isOk()) << "Failed at page " << i;
        EXPECT_EQ(result.getValue().physicalAddress, expectedPa) << "Wrong PA at page " << i;
    }
}

TEST_F(AddressSpaceTest, MapRangeBoundaryConditions) {
    PagePermissions perms(true, true, false);
    
    // Test single page range
    IOVA singlePageStart = 0x30000000;
    IOVA singlePageEnd = 0x30000000;  // Same start and end
    PA singlePagePa = 0x60000000;
    
    addressSpace->mapRange(singlePageStart, singlePageEnd, singlePagePa, perms);
    Result<size_t> singleCount = addressSpace->getPageCount();
    EXPECT_TRUE(singleCount.isOk());
    EXPECT_EQ(singleCount.getValue(), 1);
    
    Result<bool> singleMapped = addressSpace->isPageMapped(singlePageStart);
    EXPECT_TRUE(singleMapped.isOk());
    EXPECT_TRUE(singleMapped.getValue());
    
    // Test zero address range
    addressSpace->clear();
    IOVA zeroStart = 0x0;
    IOVA zeroEnd = 0x1000;  // 2 pages
    PA zeroPa = 0x40000000;
    
    addressSpace->mapRange(zeroStart, zeroEnd, zeroPa, perms);
    Result<bool> zero1Mapped = addressSpace->isPageMapped(0x0);
    EXPECT_TRUE(zero1Mapped.isOk());
    EXPECT_TRUE(zero1Mapped.getValue());
    
    Result<bool> zero2Mapped = addressSpace->isPageMapped(0x1000);
    EXPECT_TRUE(zero2Mapped.isOk());
    EXPECT_TRUE(zero2Mapped.getValue());
    
    // Test large address range
    addressSpace->clear();
    IOVA largeStart = 0xFFFFF000000;
    IOVA largeEnd = 0xFFFFF002000;    // 3 pages at high addresses
    PA largePa = 0x70000000;
    
    addressSpace->mapRange(largeStart, largeEnd, largePa, perms);
    Result<size_t> largeRangeCount = addressSpace->getPageCount();
    EXPECT_TRUE(largeRangeCount.isOk());
    EXPECT_EQ(largeRangeCount.getValue(), 3);
    
    Result<bool> largeStartMapped = addressSpace->isPageMapped(largeStart);
    EXPECT_TRUE(largeStartMapped.isOk());
    EXPECT_TRUE(largeStartMapped.getValue());
    
    Result<bool> largeEndMapped = addressSpace->isPageMapped(largeEnd);
    EXPECT_TRUE(largeEndMapped.isOk());
    EXPECT_TRUE(largeEndMapped.getValue());
}

TEST_F(AddressSpaceTest, MapRangeOverlapHandling) {
    PagePermissions perms1(true, false, false);   // Read-only
    PagePermissions perms2(true, true, false);    // Read-write
    
    // Map initial range
    IOVA range1Start = 0x10000000;
    IOVA range1End = 0x10003000;     // 4 pages
    PA pa1 = 0x40000000;
    
    addressSpace->mapRange(range1Start, range1End, pa1, perms1);
    Result<size_t> overlapCount1 = addressSpace->getPageCount();
    EXPECT_TRUE(overlapCount1.isOk());
    EXPECT_EQ(overlapCount1.getValue(), 4);
    
    // Map overlapping range (should overwrite)
    IOVA range2Start = 0x10002000;   // Overlaps last 2 pages
    IOVA range2End = 0x10005000;     // Plus 2 new pages
    PA pa2 = 0x50000000;
    
    addressSpace->mapRange(range2Start, range2End, pa2, perms2);
    
    // Verify overlapped pages have new mapping and permissions
    TranslationResult overlapResult1 = addressSpace->translatePage(0x10002000, AccessType::Write);
    TranslationResult overlapResult2 = addressSpace->translatePage(0x10003000, AccessType::Write);
    
    EXPECT_TRUE(overlapResult1.isOk());  // Should now allow write
    EXPECT_TRUE(overlapResult2.isOk());  // Should now allow write
    EXPECT_EQ(overlapResult1.getValue().physicalAddress, pa2);
    EXPECT_EQ(overlapResult2.getValue().physicalAddress, pa2 + PAGE_SIZE);
    
    // Verify non-overlapped pages still have original mapping
    TranslationResult originalResult = addressSpace->translatePage(0x10000000, AccessType::Write);
    EXPECT_TRUE(originalResult.isError());  // Should still be read-only
}

TEST_F(AddressSpaceTest, MapRangeInvalidParameters) {
    PagePermissions perms(true, true, false);
    
    // Test invalid range (end before start)
    IOVA invalidStart = 0x20000000;
    IOVA invalidEnd = 0x10000000;    // End < Start
    PA pa = 0x40000000;
    
    // This should not crash but should not map anything
    EXPECT_NO_THROW(addressSpace->mapRange(invalidStart, invalidEnd, pa, perms));
    Result<size_t> invalidCount = addressSpace->getPageCount();
    EXPECT_TRUE(invalidCount.isOk());
    EXPECT_EQ(invalidCount.getValue(), 0);
}

// Test address range unmapping operations - unmapRange()
TEST_F(AddressSpaceTest, UnmapRangeBasicOperation) {
    PagePermissions perms(true, true, false);
    
    // First map a larger range
    IOVA mapStart = 0x10000000;
    IOVA mapEnd = 0x10007000;      // 8 pages
    PA pa = 0x40000000;
    
    addressSpace->mapRange(mapStart, mapEnd, pa, perms);
    Result<size_t> unmapCount1 = addressSpace->getPageCount();
    EXPECT_TRUE(unmapCount1.isOk());
    EXPECT_EQ(unmapCount1.getValue(), 8);
    
    // Unmap middle portion
    IOVA unmapStart = 0x10002000;
    IOVA unmapEnd = 0x10004000;    // 3 pages
    
    addressSpace->unmapRange(unmapStart, unmapEnd);
    
    // Verify unmapped pages are no longer accessible
    for (IOVA iova = unmapStart; iova <= unmapEnd; iova += PAGE_SIZE) {
        Result<bool> unmappedResult = addressSpace->isPageMapped(iova);
        EXPECT_TRUE(unmappedResult.isOk());
        EXPECT_FALSE(unmappedResult.getValue());
        
        TranslationResult result = addressSpace->translatePage(iova, AccessType::Read);
        EXPECT_TRUE(result.isError());
        EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
    }
    
    // Verify remaining pages are still mapped
    Result<bool> remain1 = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(remain1.isOk());
    EXPECT_TRUE(remain1.getValue());
    
    Result<bool> remain2 = addressSpace->isPageMapped(0x10001000);
    EXPECT_TRUE(remain2.isOk());
    EXPECT_TRUE(remain2.getValue());
    
    Result<bool> remain3 = addressSpace->isPageMapped(0x10005000);
    EXPECT_TRUE(remain3.isOk());
    EXPECT_TRUE(remain3.getValue());
    
    Result<bool> remain4 = addressSpace->isPageMapped(0x10006000);
    EXPECT_TRUE(remain4.isOk());
    EXPECT_TRUE(remain4.getValue());
    
    Result<bool> remain5 = addressSpace->isPageMapped(0x10007000);
    EXPECT_TRUE(remain5.isOk());
    EXPECT_TRUE(remain5.getValue());
    
    // Verify correct page count (8 original - 3 unmapped = 5)
    Result<size_t> unmapCount2 = addressSpace->getPageCount();
    EXPECT_TRUE(unmapCount2.isOk());
    EXPECT_EQ(unmapCount2.getValue(), 5);
}

TEST_F(AddressSpaceTest, UnmapRangePartialUnmapping) {
    PagePermissions perms(true, false, false);
    
    // Map individual pages with gaps
    std::vector<IOVA> mappedPages = {
        0x10000000, 0x10001000, 0x10003000, 0x10004000, 0x10006000
    };
    
    for (size_t i = 0; i < mappedPages.size(); ++i) {
        addressSpace->mapPage(mappedPages[i], 0x40000000 + (i * PAGE_SIZE), perms);
    }
    
    Result<size_t> partialUnmapCount = addressSpace->getPageCount();
    EXPECT_TRUE(partialUnmapCount.isOk());
    EXPECT_EQ(partialUnmapCount.getValue(), 5);
    
    // Unmap range that includes both mapped and unmapped pages
    IOVA unmapStart = 0x10000000;
    IOVA unmapEnd = 0x10004000;
    
    addressSpace->unmapRange(unmapStart, unmapEnd);
    
    // Verify only previously mapped pages in range were unmapped
    Result<bool> partial1 = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(partial1.isOk());
    EXPECT_FALSE(partial1.getValue());
    
    Result<bool> partial2 = addressSpace->isPageMapped(0x10001000);
    EXPECT_TRUE(partial2.isOk());
    EXPECT_FALSE(partial2.getValue());
    
    Result<bool> partial3 = addressSpace->isPageMapped(0x10002000);  // Was never mapped anyway
    EXPECT_TRUE(partial3.isOk());
    EXPECT_FALSE(partial3.getValue());
    
    Result<bool> partial4 = addressSpace->isPageMapped(0x10003000);
    EXPECT_TRUE(partial4.isOk());
    EXPECT_FALSE(partial4.getValue());
    
    Result<bool> partial5 = addressSpace->isPageMapped(0x10004000);
    EXPECT_TRUE(partial5.isOk());
    EXPECT_FALSE(partial5.getValue());
    
    // Page outside range should still be mapped
    Result<bool> partial6 = addressSpace->isPageMapped(0x10006000);
    EXPECT_TRUE(partial6.isOk());
    EXPECT_TRUE(partial6.getValue());
    
    Result<size_t> partialCount2 = addressSpace->getPageCount();
    EXPECT_TRUE(partialCount2.isOk());
    EXPECT_EQ(partialCount2.getValue(), 1);
}

TEST_F(AddressSpaceTest, UnmapRangeInvalidParameters) {
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    
    // Test invalid range (end before start)
    IOVA invalidStart = 0x20000000;
    IOVA invalidEnd = 0x10000000;    // End < Start
    
    // Should not crash or affect existing mappings
    EXPECT_NO_THROW(addressSpace->unmapRange(invalidStart, invalidEnd));
    Result<size_t> invalidUnmapCount = addressSpace->getPageCount();
    EXPECT_TRUE(invalidUnmapCount.isOk());
    EXPECT_EQ(invalidUnmapCount.getValue(), 1);
    
    Result<bool> invalidUnmapMapped = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(invalidUnmapMapped.isOk());
    EXPECT_TRUE(invalidUnmapMapped.getValue());
}

TEST_F(AddressSpaceTest, UnmapRangeNonExistentRange) {
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    
    // Unmap range with no mapped pages
    IOVA unmapStart = 0x20000000;
    IOVA unmapEnd = 0x20005000;
    
    // Should not crash or affect existing mappings
    EXPECT_NO_THROW(addressSpace->unmapRange(unmapStart, unmapEnd));
    Result<size_t> nonExistentCount = addressSpace->getPageCount();
    EXPECT_TRUE(nonExistentCount.isOk());
    EXPECT_EQ(nonExistentCount.getValue(), 1);
    
    Result<bool> nonExistentMapped = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(nonExistentMapped.isOk());
    EXPECT_TRUE(nonExistentMapped.getValue());
}

// Test bulk page operations - mapPages()
TEST_F(AddressSpaceTest, MapPagesBulkMapping) {
    PagePermissions perms(true, true, false);
    
    // Prepare bulk mapping data
    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x10000000, 0x40000000},
        {0x20000000, 0x50000000},
        {0x30000000, 0x60000000},
        {0x40000000, 0x70000000},
        {0x50000000, 0x80000000}
    };
    
    // Perform bulk mapping
    addressSpace->mapPages(mappings, perms);
    
    Result<size_t> bulkCount = addressSpace->getPageCount();
    EXPECT_TRUE(bulkCount.isOk());
    EXPECT_EQ(bulkCount.getValue(), 5);
    
    // Verify all mappings are correct
    for (size_t i = 0; i < mappings.size(); ++i) {
        IOVA iova = mappings[i].first;
        PA expectedPa = mappings[i].second;
        
        Result<bool> bulkMapped = addressSpace->isPageMapped(iova);
        EXPECT_TRUE(bulkMapped.isOk());
        EXPECT_TRUE(bulkMapped.getValue());
        
        TranslationResult result = addressSpace->translatePage(iova, AccessType::Read);
        EXPECT_TRUE(result.isOk()) << "Failed for IOVA " << std::hex << iova;
        EXPECT_EQ(result.getValue().physicalAddress, expectedPa) << "Wrong PA for IOVA " << std::hex << iova;
    }
}

TEST_F(AddressSpaceTest, MapPagesEmptyCollection) {
    PagePermissions perms(true, true, false);
    
    // Test with empty vector
    std::vector<std::pair<IOVA, PA>> emptyMappings;
    
    EXPECT_NO_THROW(addressSpace->mapPages(emptyMappings, perms));
    Result<size_t> emptyBulkCount = addressSpace->getPageCount();
    EXPECT_TRUE(emptyBulkCount.isOk());
    EXPECT_EQ(emptyBulkCount.getValue(), 0);
}

TEST_F(AddressSpaceTest, MapPagesMixedAddresses) {
    PagePermissions perms(true, false, true);  // Read and execute
    
    // Mix of aligned and unaligned addresses
    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x10000000, 0x40000000},        // Both aligned
        {0x20001234, 0x50005678},        // Both unaligned
        {0x30000000, 0x60001111},        // IOVA aligned, PA unaligned
        {0x40002222, 0x70000000}         // IOVA unaligned, PA aligned
    };
    
    addressSpace->mapPages(mappings, perms);
    
    Result<size_t> mixedCount = addressSpace->getPageCount();
    EXPECT_TRUE(mixedCount.isOk());
    EXPECT_EQ(mixedCount.getValue(), 4);
    
    // Test that all work correctly with page alignment handling
    for (const auto& mapping : mappings) {
        IOVA iova = mapping.first;
        PA expectedPaBase = mapping.second & ~PAGE_MASK;  // Page-aligned base
        IOVA iovaOffset = iova & PAGE_MASK;
        
        TranslationResult result = addressSpace->translatePage(iova, AccessType::Read);
        EXPECT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().physicalAddress, expectedPaBase + iovaOffset);
    }
}

TEST_F(AddressSpaceTest, MapPagesOverwriteExisting) {
    PagePermissions oldPerms(true, false, false);   // Read-only
    PagePermissions newPerms(true, true, false);    // Read-write
    
    // Initial mappings
    addressSpace->mapPage(0x10000000, 0x40000000, oldPerms);
    addressSpace->mapPage(0x20000000, 0x50000000, oldPerms);
    
    // Bulk mapping that overwrites existing
    std::vector<std::pair<IOVA, PA>> newMappings = {
        {0x10000000, 0x41000000},  // Overwrite first with new PA
        {0x20000000, 0x51000000},  // Overwrite second with new PA
        {0x30000000, 0x60000000}   // New mapping
    };
    
    addressSpace->mapPages(newMappings, newPerms);
    
    Result<size_t> overwriteCount = addressSpace->getPageCount();
    EXPECT_TRUE(overwriteCount.isOk());
    EXPECT_EQ(overwriteCount.getValue(), 3);
    
    // Verify overwrites worked
    TranslationResult result1 = addressSpace->translatePage(0x10000000, AccessType::Write);
    EXPECT_TRUE(result1.isOk());  // Should now allow write
    EXPECT_EQ(result1.getValue().physicalAddress, 0x41000000);  // Should have new PA
    
    TranslationResult result2 = addressSpace->translatePage(0x20000000, AccessType::Write);
    EXPECT_TRUE(result2.isOk());  // Should now allow write
    EXPECT_EQ(result2.getValue().physicalAddress, 0x51000000);  // Should have new PA
}

// Test bulk page operations - unmapPages()
TEST_F(AddressSpaceTest, UnmapPagesBulkUnmapping) {
    PagePermissions perms(true, true, false);
    
    // Set up multiple mappings
    std::vector<IOVA> iovas = {
        0x10000000, 0x20000000, 0x30000000, 0x40000000, 0x50000000, 0x60000000
    };
    
    for (size_t i = 0; i < iovas.size(); ++i) {
        addressSpace->mapPage(iovas[i], 0x40000000 + (i * PAGE_SIZE), perms);
    }
    
    Result<size_t> bulkUnmapCount1 = addressSpace->getPageCount();
    EXPECT_TRUE(bulkUnmapCount1.isOk());
    EXPECT_EQ(bulkUnmapCount1.getValue(), 6);
    
    // Bulk unmap subset
    std::vector<IOVA> unmapIovas = {0x20000000, 0x40000000, 0x60000000};
    addressSpace->unmapPages(unmapIovas);
    
    Result<size_t> bulkUnmapCount2 = addressSpace->getPageCount();
    EXPECT_TRUE(bulkUnmapCount2.isOk());
    EXPECT_EQ(bulkUnmapCount2.getValue(), 3);
    
    // Verify correct pages were unmapped
    Result<bool> bulk1 = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(bulk1.isOk());
    EXPECT_TRUE(bulk1.getValue());
    
    Result<bool> bulk2 = addressSpace->isPageMapped(0x20000000);
    EXPECT_TRUE(bulk2.isOk());
    EXPECT_FALSE(bulk2.getValue());
    
    Result<bool> bulk3 = addressSpace->isPageMapped(0x30000000);
    EXPECT_TRUE(bulk3.isOk());
    EXPECT_TRUE(bulk3.getValue());
    
    Result<bool> bulk4 = addressSpace->isPageMapped(0x40000000);
    EXPECT_TRUE(bulk4.isOk());
    EXPECT_FALSE(bulk4.getValue());
    
    Result<bool> bulk5 = addressSpace->isPageMapped(0x50000000);
    EXPECT_TRUE(bulk5.isOk());
    EXPECT_TRUE(bulk5.getValue());
    
    Result<bool> bulk6 = addressSpace->isPageMapped(0x60000000);
    EXPECT_TRUE(bulk6.isOk());
    EXPECT_FALSE(bulk6.getValue());
}

TEST_F(AddressSpaceTest, UnmapPagesNonExistentPages) {
    PagePermissions perms(true, true, false);
    
    // Map some pages
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    addressSpace->mapPage(0x20000000, 0x50000000, perms);
    
    // Try to unmap mix of existing and non-existent pages
    std::vector<IOVA> unmapIovas = {
        0x10000000,  // Exists
        0x15000000,  // Does not exist
        0x20000000,  // Exists
        0x25000000   // Does not exist
    };
    
    EXPECT_NO_THROW(addressSpace->unmapPages(unmapIovas));
    
    // Should have unmapped only the existing pages
    Result<size_t> nonExistentUnmapCount = addressSpace->getPageCount();
    EXPECT_TRUE(nonExistentUnmapCount.isOk());
    EXPECT_EQ(nonExistentUnmapCount.getValue(), 0);
    
    Result<bool> nonExist1 = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(nonExist1.isOk());
    EXPECT_FALSE(nonExist1.getValue());
    
    Result<bool> nonExist2 = addressSpace->isPageMapped(0x20000000);
    EXPECT_TRUE(nonExist2.isOk());
    EXPECT_FALSE(nonExist2.getValue());
}

TEST_F(AddressSpaceTest, UnmapPagesEmptyCollection) {
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    
    // Test with empty vector
    std::vector<IOVA> emptyUnmaps;
    
    EXPECT_NO_THROW(addressSpace->unmapPages(emptyUnmaps));
    Result<size_t> emptyUnmapCount = addressSpace->getPageCount();
    EXPECT_TRUE(emptyUnmapCount.isOk());
    EXPECT_EQ(emptyUnmapCount.getValue(), 1);  // Should be unchanged
    
    Result<bool> emptyUnmapMapped = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(emptyUnmapMapped.isOk());
    EXPECT_TRUE(emptyUnmapMapped.getValue());
}

TEST_F(AddressSpaceTest, UnmapPagesPerformanceCharacteristics) {
    PagePermissions perms(true, false, false);
    
    // Set up large number of mappings
    std::vector<IOVA> allIovas;
    const size_t numMappings = 1000;
    
    for (size_t i = 0; i < numMappings; ++i) {
        IOVA iova = 0x10000000 + (i * PAGE_SIZE);
        PA pa = 0x40000000 + (i * PAGE_SIZE);
        allIovas.push_back(iova);
        addressSpace->mapPage(iova, pa, perms);
    }
    
    Result<size_t> perfCount1 = addressSpace->getPageCount();
    EXPECT_TRUE(perfCount1.isOk());
    EXPECT_EQ(perfCount1.getValue(), numMappings);
    
    // Bulk unmap all pages (should be efficient)
    addressSpace->unmapPages(allIovas);
    
    Result<size_t> perfCount2 = addressSpace->getPageCount();
    EXPECT_TRUE(perfCount2.isOk());
    EXPECT_EQ(perfCount2.getValue(), 0);
    
    // Verify all are unmapped
    for (IOVA iova : allIovas) {
        Result<bool> perfUnmapped = addressSpace->isPageMapped(iova);
        EXPECT_TRUE(perfUnmapped.isOk());
        EXPECT_FALSE(perfUnmapped.getValue());
    }
}

// Test address space state querying methods - getMappedRanges()
TEST_F(AddressSpaceTest, GetMappedRangesBasic) {
    PagePermissions perms(true, true, false);
    
    // Map some individual pages
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    addressSpace->mapPage(0x10001000, 0x40001000, perms);
    addressSpace->mapPage(0x10002000, 0x40002000, perms);
    
    // Map separate range
    addressSpace->mapPage(0x20000000, 0x50000000, perms);
    addressSpace->mapPage(0x20001000, 0x50001000, perms);
    
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    
    // Should consolidate contiguous pages into ranges
    EXPECT_EQ(ranges.size(), 2);
    
    // First range should be 0x10000000 to 0x10002FFF (3 pages)
    bool foundRange1 = false, foundRange2 = false;
    for (const auto& range : ranges) {
        if (range.startAddress == 0x10000000) {
            EXPECT_EQ(range.endAddress, 0x10002FFF);  // Last byte of third page
            EXPECT_EQ(range.size(), 3 * PAGE_SIZE);   // 3 full pages
            foundRange1 = true;
        } else if (range.startAddress == 0x20000000) {
            EXPECT_EQ(range.endAddress, 0x20001FFF);  // Last byte of second page
            EXPECT_EQ(range.size(), 2 * PAGE_SIZE);   // 2 full pages
            foundRange2 = true;
        }
    }
    
    EXPECT_TRUE(foundRange1);
    EXPECT_TRUE(foundRange2);
}

TEST_F(AddressSpaceTest, GetMappedRangesConsolidation) {
    PagePermissions perms(true, false, false);
    
    // Map large contiguous range using individual mapPage calls
    const IOVA baseAddr = 0x30000000;
    const size_t numPages = 100;
    
    for (size_t i = 0; i < numPages; ++i) {
        IOVA iova = baseAddr + (i * PAGE_SIZE);
        PA pa = 0x60000000 + (i * PAGE_SIZE);
        addressSpace->mapPage(iova, pa, perms);
    }
    
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    
    // Should be consolidated into single range
    EXPECT_EQ(ranges.size(), 1);
    
    const AddressRange& range = ranges[0];
    EXPECT_EQ(range.startAddress, baseAddr);
    EXPECT_EQ(range.endAddress, baseAddr + ((numPages - 1) * PAGE_SIZE) + PAGE_SIZE - 1);  // Last byte of last page
    EXPECT_EQ(range.size(), numPages * PAGE_SIZE);
}

TEST_F(AddressSpaceTest, GetMappedRangesEmptySpace) {
    // Empty address space
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    EXPECT_TRUE(ranges.empty());
}

TEST_F(AddressSpaceTest, GetMappedRangesFragmentedMappings) {
    PagePermissions perms(true, true, false);
    
    // Create fragmented mappings with gaps
    std::vector<IOVA> sparseAddresses = {
        0x10000000,  // Range 1: single page
        0x20000000, 0x20001000,  // Range 2: two contiguous pages
        0x30000000,  // Range 3: single page
        0x40000000, 0x40001000, 0x40002000, 0x40003000  // Range 4: four contiguous pages
    };
    
    for (size_t i = 0; i < sparseAddresses.size(); ++i) {
        addressSpace->mapPage(sparseAddresses[i], 0x50000000 + (i * PAGE_SIZE), perms);
    }
    
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    
    // Should create 4 separate ranges
    EXPECT_EQ(ranges.size(), 4);
    
    // Verify range consolidation logic
    std::vector<std::pair<IOVA, IOVA>> expectedRanges = {
        {0x10000000, 0x10000FFF},  // Single page (end address is last byte)
        {0x20000000, 0x20001FFF},  // Two pages (end address is last byte of second page)
        {0x30000000, 0x30000FFF},  // Single page (end address is last byte)
        {0x40000000, 0x40003FFF}   // Four pages (end address is last byte of fourth page)
    };
    
    // Sort ranges by start address for comparison
    std::sort(ranges.begin(), ranges.end(), 
              [](const AddressRange& a, const AddressRange& b) {
                  return a.startAddress < b.startAddress;
              });
    
    for (size_t i = 0; i < expectedRanges.size(); ++i) {
        EXPECT_EQ(ranges[i].startAddress, expectedRanges[i].first) << "Range " << i << " start mismatch";
        EXPECT_EQ(ranges[i].endAddress, expectedRanges[i].second) << "Range " << i << " end mismatch";
    }
}

// Test address space state querying methods - getAddressSpaceSize()
TEST_F(AddressSpaceTest, GetAddressSpaceSizeBasic) {
    PagePermissions perms(true, true, false);
    
    // Initially empty
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 0);
    
    // Add single page
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), PAGE_SIZE);
    
    // Add another page (not contiguous) - this creates a large span
    addressSpace->mapPage(0x20000000, 0x50000000, perms);
    // Size is from 0x10000000 to 0x20000FFF = 0x10000FFF + 1 = 268439552
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 0x20000000 - 0x10000000 + PAGE_SIZE);
    
    // Add contiguous page - doesn't change the span
    addressSpace->mapPage(0x10001000, 0x40001000, perms);
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 0x20000000 - 0x10000000 + PAGE_SIZE);
}

TEST_F(AddressSpaceTest, GetAddressSpaceSizeAfterOperations) {
    PagePermissions perms(true, false, false);
    
    // Map range
    addressSpace->mapRange(0x10000000, 0x10004000, 0x40000000, perms);  // 5 pages
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 5 * PAGE_SIZE);  // Contiguous range
    
    // Unmap some pages - span remains the same (still 0x10000000 to 0x10004FFF)
    addressSpace->unmapPage(0x10001000);
    addressSpace->unmapPage(0x10003000);
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 5 * PAGE_SIZE);  // Span unchanged
    
    // Clear all
    addressSpace->clear();
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 0);
}

TEST_F(AddressSpaceTest, GetAddressSpaceSizeLargeSpace) {
    PagePermissions perms(true, true, false);
    
    // Map sparse pages across large address space
    const size_t numPages = 1000;
    for (size_t i = 0; i < numPages; ++i) {
        IOVA iova = i * 0x100000000ULL;  // 4GB gaps
        PA pa = 0x40000000 + (i * PAGE_SIZE);
        addressSpace->mapPage(iova, pa, perms);
    }
    
    // Size is from first page (0) to last page (999 * 4GB + PAGE_SIZE - 1)
    uint64_t expectedSize = (numPages - 1) * 0x100000000ULL + PAGE_SIZE;
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), expectedSize);
    Result<size_t> largeSpaceCount = addressSpace->getPageCount();
    EXPECT_TRUE(largeSpaceCount.isOk());
    EXPECT_EQ(largeSpaceCount.getValue(), numPages);
}

// Test address space state querying methods - hasOverlappingMappings()
TEST_F(AddressSpaceTest, HasOverlappingMappingsBasic) {
    PagePermissions perms(true, true, false);
    
    // Map some pages
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    addressSpace->mapPage(0x10001000, 0x40001000, perms);
    addressSpace->mapPage(0x10003000, 0x40003000, perms);  // Gap at 0x10002000
    
    // Test overlapping ranges
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10000000, 0x10000000));  // Exact page
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x0FFF0000, 0x10010000));  // Spans multiple pages
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10000500, 0x10001500));  // Partial overlap
    
    // Test non-overlapping ranges
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x10002000, 0x10002000)); // Gap
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x20000000, 0x20005000)); // Completely separate
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x0FF00000, 0x0FFFF000)); // Before mapped region
}

TEST_F(AddressSpaceTest, HasOverlappingMappingsEdgeCases) {
    PagePermissions perms(true, false, false);
    
    // Map single page
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    
    // Test boundary conditions
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10000000, 0x10000FFF));  // Within page
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x0FFFFFFF, 0x10000001));  // Crosses page start
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10000FFF, 0x10001001));  // Crosses page end
    
    // Test adjacent but non-overlapping
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x0FFFF000, 0x0FFFFFFF)); // Just before
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x10001000, 0x10002000)); // Just after
    
    // Test invalid ranges
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x20000000, 0x10000000)); // End < Start
}

TEST_F(AddressSpaceTest, HasOverlappingMappingsComplexRanges) {
    PagePermissions perms(true, true, false);
    
    // Create complex mapping pattern
    addressSpace->mapRange(0x10000000, 0x10002000, 0x40000000, perms);  // Pages 0,1,2
    addressSpace->mapRange(0x10005000, 0x10007000, 0x40005000, perms);  // Pages 5,6,7 (gap 3,4)
    addressSpace->mapPage(0x10010000, 0x40010000, perms);               // Isolated page 16
    
    // Test various overlap scenarios
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10000000, 0x10003000));  // Spans first range + gap
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10001500, 0x10006500));  // Spans across gap
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x0FF00000, 0x10020000));  // Spans entire space
    
    // Test ranges in gaps
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x10003000, 0x10004000)); // In gap
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x10008000, 0x1000F000)); // In gap
    
    // Test isolated page
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10010000, 0x10010000));  // Exact match
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x1000F500, 0x10010500));  // Partial overlap
}

// Test cache invalidation mechanisms - invalidateRange()
TEST_F(AddressSpaceTest, InvalidateRangeInterface) {
    PagePermissions perms(true, true, false);
    
    // Set up mappings
    addressSpace->mapRange(0x10000000, 0x10005000, 0x40000000, perms);
    
    // Test invalidateRange interface (should not crash)
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x10000000, 0x10002000));
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x20000000, 0x20005000));  // Non-existent range
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x10005000, 0x10003000));  // Invalid range (end < start)
    
    // Verify functionality is unchanged after invalidation
    TranslationResult result = addressSpace->translatePage(0x10001000, AccessType::Read);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, 0x40001000);
    
    Result<size_t> invalidateRangeCount = addressSpace->getPageCount();
    EXPECT_TRUE(invalidateRangeCount.isOk());
    EXPECT_EQ(invalidateRangeCount.getValue(), 6);  // Should be unchanged
}

TEST_F(AddressSpaceTest, InvalidateRangeWithEmptySpace) {
    // Test invalidation on empty address space
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x10000000, 0x10005000));
    
    // Should remain empty
    Result<size_t> emptyInvalidateCount = addressSpace->getPageCount();
    EXPECT_TRUE(emptyInvalidateCount.isOk());
    EXPECT_EQ(emptyInvalidateCount.getValue(), 0);
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 0);
}

TEST_F(AddressSpaceTest, InvalidateRangeBoundaryConditions) {
    PagePermissions perms(true, false, false);
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    
    // Test boundary conditions
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x0, 0xFFFFFFFFFFFFFFFF));    // Max range
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x10000000, 0x10000000));     // Single page
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x0FFFFFFF, 0x10000001));     // Crosses page boundary
    
    // Verify mapping still works
    Result<bool> boundaryMapped = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(boundaryMapped.isOk());
    EXPECT_TRUE(boundaryMapped.getValue());
}

// Test cache invalidation mechanisms - invalidateAll()
TEST_F(AddressSpaceTest, InvalidateAllInterface) {
    PagePermissions perms(true, true, false);
    
    // Set up multiple mappings
    addressSpace->mapRange(0x10000000, 0x10005000, 0x40000000, perms);
    addressSpace->mapRange(0x20000000, 0x20003000, 0x50000000, perms);
    
    // Test invalidateAll interface (should not crash)
    EXPECT_NO_THROW(addressSpace->invalidateAll());
    
    // Verify functionality is unchanged after invalidation
    Result<size_t> invalidateAllCount = addressSpace->getPageCount();
    EXPECT_TRUE(invalidateAllCount.isOk());
    EXPECT_EQ(invalidateAllCount.getValue(), 10);  // 6 + 4 pages
    
    TranslationResult result1 = addressSpace->translatePage(0x10002000, AccessType::Read);
    TranslationResult result2 = addressSpace->translatePage(0x20001000, AccessType::Write);
    
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_EQ(result1.getValue().physicalAddress, 0x40002000);
    EXPECT_EQ(result2.getValue().physicalAddress, 0x50001000);
}

TEST_F(AddressSpaceTest, InvalidateAllWithEmptySpace) {
    // Test invalidateAll on empty address space
    EXPECT_NO_THROW(addressSpace->invalidateAll());
    
    // Should remain empty
    Result<size_t> emptyInvalidateAllCount = addressSpace->getPageCount();
    EXPECT_TRUE(emptyInvalidateAllCount.isOk());
    EXPECT_EQ(emptyInvalidateAllCount.getValue(), 0);
}

TEST_F(AddressSpaceTest, InvalidateAllAfterOperations) {
    PagePermissions perms(true, false, true);  // Read and execute
    
    // Perform various operations
    addressSpace->mapRange(0x10000000, 0x10010000, 0x40000000, perms);
    addressSpace->unmapRange(0x10005000, 0x10008000);
    
    std::vector<std::pair<IOVA, PA>> bulkMappings = {
        {0x20000000, 0x50000000},
        {0x20001000, 0x50001000}
    };
    addressSpace->mapPages(bulkMappings, perms);
    
    // Invalidate all
    EXPECT_NO_THROW(addressSpace->invalidateAll());
    
    // Verify all functionality still works correctly
    Result<bool> after1 = addressSpace->isPageMapped(0x10000000);
    EXPECT_TRUE(after1.isOk());
    EXPECT_TRUE(after1.getValue());
    
    Result<bool> after2 = addressSpace->isPageMapped(0x10005000);  // Was unmapped
    EXPECT_TRUE(after2.isOk());
    EXPECT_FALSE(after2.getValue());
    
    Result<bool> after3 = addressSpace->isPageMapped(0x20000000);
    EXPECT_TRUE(after3.isOk());
    EXPECT_TRUE(after3.getValue());
    
    TranslationResult result = addressSpace->translatePage(0x10000000, AccessType::Execute);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress, 0x40000000);
}

// Integration tests for Task 3.2 functionality
TEST_F(AddressSpaceTest, Task32IntegrationCompleteWorkflow) {
    PagePermissions readWritePerms(true, true, false);
    PagePermissions readOnlyPerms(true, false, false);
    
    // Step 1: Map ranges with mapRange()
    addressSpace->mapRange(0x10000000, 0x10005000, 0x40000000, readWritePerms);
    addressSpace->mapRange(0x20000000, 0x20002000, 0x50000000, readOnlyPerms);
    
    Result<size_t> integrationCount1 = addressSpace->getPageCount();
    EXPECT_TRUE(integrationCount1.isOk());
    EXPECT_EQ(integrationCount1.getValue(), 9);  // 6 + 3 pages
    // Size spans from 0x10000000 to 0x20002FFF
    uint64_t expectedSize = 0x20002FFF - 0x10000000 + 1;
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), expectedSize);
    
    // Step 2: Add bulk mappings with mapPages()
    std::vector<std::pair<IOVA, PA>> bulkMappings = {
        {0x30000000, 0x60000000},
        {0x30001000, 0x60001000},
        {0x30002000, 0x60002000}
    };
    addressSpace->mapPages(bulkMappings, readWritePerms);
    
    Result<size_t> integrationCount2 = addressSpace->getPageCount();
    EXPECT_TRUE(integrationCount2.isOk());
    EXPECT_EQ(integrationCount2.getValue(), 12);
    
    // Step 3: Query mapped ranges
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    EXPECT_EQ(ranges.size(), 3);  // Three separate contiguous ranges
    
    // Step 4: Test overlap detection
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x10000000, 0x10003000));
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x15000000, 0x15005000));
    
    // Step 5: Unmap ranges with unmapRange()
    addressSpace->unmapRange(0x10002000, 0x10004000);  // Remove middle of first range
    
    Result<size_t> integrationCount3 = addressSpace->getPageCount();
    EXPECT_TRUE(integrationCount3.isOk());
    EXPECT_EQ(integrationCount3.getValue(), 9);  // Should have removed 3 pages
    
    // Step 6: Bulk unmap with unmapPages()
    std::vector<IOVA> unmapList = {0x20000000, 0x30001000};
    addressSpace->unmapPages(unmapList);
    
    Result<size_t> integrationCount4 = addressSpace->getPageCount();
    EXPECT_TRUE(integrationCount4.isOk());
    EXPECT_EQ(integrationCount4.getValue(), 7);  // Should have removed 2 more pages
    
    // Step 7: Test cache invalidation interfaces
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x10000000, 0x10010000));
    EXPECT_NO_THROW(addressSpace->invalidateAll());
    
    // Step 8: Verify final state
    // Final pages: 0x10000000, 0x10001000, 0x10005000, 0x20001000, 0x20002000, 0x30000000, 0x30002000
    // Span from 0x10000000 to 0x30002FFF
    uint64_t finalExpectedSize = 0x30002FFF - 0x10000000 + 1;
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), finalExpectedSize);
    
    // Verify remaining mappings work correctly
    TranslationResult result1 = addressSpace->translatePage(0x10000000, AccessType::Read);
    EXPECT_TRUE(result1.isOk());
    
    TranslationResult result2 = addressSpace->translatePage(0x10002000, AccessType::Read);  
    EXPECT_TRUE(result2.isError());  // Should be unmapped
    
    TranslationResult result3 = addressSpace->translatePage(0x30000000, AccessType::Write);
    EXPECT_TRUE(result3.isOk());
}

TEST_F(AddressSpaceTest, Task32PerformanceValidation) {
    PagePermissions perms(true, true, false);
    
    // Test performance characteristics of bulk operations
    const size_t numPages = 10000;
    
    // Bulk map using mapPages() - should be efficient
    std::vector<std::pair<IOVA, PA>> mappings;
    for (size_t i = 0; i < numPages; ++i) {
        IOVA iova = 0x100000000ULL + (i * PAGE_SIZE);  // Start at 4GB
        PA pa = 0x40000000ULL + (i * PAGE_SIZE);
        mappings.push_back({iova, pa});
    }
    
    addressSpace->mapPages(mappings, perms);
    Result<size_t> perfValidationCount1 = addressSpace->getPageCount();
    EXPECT_TRUE(perfValidationCount1.isOk());
    EXPECT_EQ(perfValidationCount1.getValue(), numPages);
    
    // Test getMappedRanges() consolidation efficiency
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    EXPECT_EQ(ranges.size(), 1);  // Should consolidate into single range
    EXPECT_EQ(ranges[0].size(), numPages * PAGE_SIZE);
    
    // Test getAddressSpaceSize() efficiency  
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), numPages * PAGE_SIZE);
    
    // Test hasOverlappingMappings() across large range
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x100000000ULL, 0x200000000ULL));
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x200000000ULL, 0x300000000ULL));
    
    // Bulk unmap using unmapPages() - should be efficient
    std::vector<IOVA> unmapIovas;
    for (size_t i = 0; i < numPages; i += 2) {  // Unmap every other page
        unmapIovas.push_back(0x100000000ULL + (i * PAGE_SIZE));
    }
    
    addressSpace->unmapPages(unmapIovas);
    Result<size_t> perfValidationCount2 = addressSpace->getPageCount();
    EXPECT_TRUE(perfValidationCount2.isOk());
    EXPECT_EQ(perfValidationCount2.getValue(), numPages / 2);
    
    // Verify cache invalidation works on large ranges
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x100000000ULL, 0x200000000ULL));
    EXPECT_NO_THROW(addressSpace->invalidateAll());
}

TEST_F(AddressSpaceTest, Task32ErrorHandlingAndEdgeCases) {
    PagePermissions perms(true, false, false);
    
    // Test all operations on empty address space
    EXPECT_EQ(addressSpace->getMappedRanges().size(), 0);
    EXPECT_EQ(addressSpace->getAddressSpaceSize(), 0);
    EXPECT_FALSE(addressSpace->hasOverlappingMappings(0x10000000, 0x10005000));
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x10000000, 0x10005000));
    EXPECT_NO_THROW(addressSpace->invalidateAll());
    
    // Test operations with invalid parameters
    EXPECT_NO_THROW(addressSpace->mapRange(0x20000000, 0x10000000, 0x40000000, perms));  // end < start
    EXPECT_NO_THROW(addressSpace->unmapRange(0x20000000, 0x10000000));  // end < start
    EXPECT_NO_THROW(addressSpace->invalidateRange(0x20000000, 0x10000000));  // end < start
    
    // Test operations with empty collections
    std::vector<std::pair<IOVA, PA>> emptyMappings;
    std::vector<IOVA> emptyUnmaps;
    
    EXPECT_NO_THROW(addressSpace->mapPages(emptyMappings, perms));
    EXPECT_NO_THROW(addressSpace->unmapPages(emptyUnmaps));
    
    // Test boundary address conditions
    addressSpace->mapPage(0x0, 0x40000000, perms);                     // Zero address
    addressSpace->mapPage(0x000FFFFFFFFFF000ULL, 0x50000000, perms);   // High address within 52-bit space
    
    Result<bool> boundary1 = addressSpace->isPageMapped(0x0);
    EXPECT_TRUE(boundary1.isOk());
    EXPECT_TRUE(boundary1.getValue());
    
    Result<bool> boundary2 = addressSpace->isPageMapped(0x000FFFFFFFFFF000ULL);
    EXPECT_TRUE(boundary2.isOk());
    EXPECT_TRUE(boundary2.getValue());
    
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    EXPECT_EQ(ranges.size(), 2);  // Two separate ranges
    
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x0, 0x0));
    EXPECT_TRUE(addressSpace->hasOverlappingMappings(0x000FFFFFFFFFF000ULL, 0x000FFFFFFFFFF000ULL));
}

} // namespace test
} // namespace smmu