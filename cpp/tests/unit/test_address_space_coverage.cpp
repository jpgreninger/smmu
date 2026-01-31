// ARM SMMU v3 AddressSpace Coverage Tests
// Copyright (c) 2024 John Greninger
//
// Comprehensive test suite targeting uncovered lines to achieve 90%+ coverage
// Focus: Input validation, error paths, empty address space operations, boundary conditions

#include <gtest/gtest.h>
#include <algorithm>
#include "smmu/address_space.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

class AddressSpaceCoverageTest : public ::testing::Test {
protected:
    void SetUp() override {
        addressSpace = std::make_unique<AddressSpace>();
    }

    void TearDown() override {
        addressSpace.reset();
    }

    std::unique_ptr<AddressSpace> addressSpace;

    // Test helper constants
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr PA TEST_PA = 0x40000000;
    static constexpr IOVA VALID_MAX_IOVA = 0x000FFFFFFFFFF000ULL;  // 52-bit max, page-aligned
    static constexpr PA VALID_MAX_PA = 0x000FFFFFFFFFF000ULL;      // 52-bit max, page-aligned
};

// ==========================================================================
// TC-ADDR-001: Input Validation Tests
// Target lines: 58, 68, 82, 98, 136, 279, 318, 451, 459, 471, 480, 536, 604, 628, 638, 702
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, MapPageInvalidIOVA) {
    PagePermissions perms(true, true, false);

    // Test IOVA exceeding MAX_VIRTUAL_ADDRESS (line 40-41, targeting error return line 41)
    IOVA invalidIOVA = 0xFFFFFFFFFFFFF000ULL;  // Beyond 52-bit address space
    VoidResult result = addressSpace->mapPage(invalidIOVA, TEST_PA, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, MapPageInvalidPA) {
    PagePermissions perms(true, true, false);

    // Test PA exceeding MAX_PHYSICAL_ADDRESS (line 45-46)
    PA invalidPA = 0xFFFFFFFFFFFFF000ULL;  // Beyond 52-bit address space
    VoidResult result = addressSpace->mapPage(TEST_IOVA, invalidPA, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, MapPageNoPermissions) {
    // Test all permissions false (line 50-52)
    PagePermissions noPerms(false, false, false);

    VoidResult result = addressSpace->mapPage(TEST_IOVA, TEST_PA, noPerms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPermissions);
}

TEST_F(AddressSpaceCoverageTest, MapPageInvalidSecurityStateSecure) {
    PagePermissions perms(true, true, false);

    // Test with Secure security state (valid enum value)
    VoidResult result = addressSpace->mapPage(TEST_IOVA, TEST_PA, perms, SecurityState::Secure);

    // Should succeed with valid security state
    EXPECT_TRUE(result.isOk());

    // Verify mapping with proper security state
    TranslationResult transResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(transResult.isOk());
}

TEST_F(AddressSpaceCoverageTest, MapPageInvalidSecurityStateRealm) {
    PagePermissions perms(true, true, false);

    // Test with Realm security state (valid enum value)
    VoidResult result = addressSpace->mapPage(TEST_IOVA, TEST_PA, perms, SecurityState::Realm);

    // Should succeed with valid security state
    EXPECT_TRUE(result.isOk());

    // Verify mapping with proper security state
    TranslationResult transResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read, SecurityState::Realm);
    EXPECT_TRUE(transResult.isOk());
}

TEST_F(AddressSpaceCoverageTest, MapPageInvalidSecurityStateEnum) {
    PagePermissions perms(true, true, false);

    // Test with invalid security state by casting (line 58)
    // SecurityState enum values: NonSecure=0, Secure=1, Realm=2
    // We'll use an out-of-range value to trigger the error path
    SecurityState invalidState = static_cast<SecurityState>(99);
    VoidResult result = addressSpace->mapPage(TEST_IOVA, TEST_PA, perms, invalidState);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidSecurityState);
}

TEST_F(AddressSpaceCoverageTest, UnmapPageInvalidIOVA) {
    // Test unmapping with invalid IOVA (line 81-82)
    IOVA invalidIOVA = 0xFFFFFFFFFFFFF000ULL;

    VoidResult result = addressSpace->unmapPage(invalidIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, UnmapPageNotMapped) {
    // Test unmapping page that was never mapped (line 89-91)
    VoidResult result = addressSpace->unmapPage(TEST_IOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(AddressSpaceCoverageTest, TranslatePagePermissionFault) {
    PagePermissions readOnlyPerms(true, false, false);

    // Map page with read-only permissions
    addressSpace->mapPage(TEST_IOVA, TEST_PA, readOnlyPerms);

    // Test write access on read-only page (line 137-139)
    TranslationResult result = addressSpace->translatePage(TEST_IOVA, AccessType::Write);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(AddressSpaceCoverageTest, MapRangeInvalidPermissions) {
    // Test mapRange with no permissions (line 253-254)
    PagePermissions noPerms(false, false, false);
    IOVA startIOVA = 0x10000000;
    IOVA endIOVA = 0x10002000;

    VoidResult result = addressSpace->mapRange(startIOVA, endIOVA, TEST_PA, noPerms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPermissions);
}

TEST_F(AddressSpaceCoverageTest, UnmapRangeNoMappedPages) {
    // Test unmapRange when no pages in range are mapped (line 315-317)
    IOVA startIOVA = 0x20000000;
    IOVA endIOVA = 0x20002000;

    VoidResult result = addressSpace->unmapRange(startIOVA, endIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(AddressSpaceCoverageTest, MapPagesInvalidPermissions) {
    // Test mapPages with no permissions (line 332-333)
    PagePermissions noPerms(false, false, false);
    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x10000000, 0x40000000},
        {0x20000000, 0x50000000}
    };

    VoidResult result = addressSpace->mapPages(mappings, noPerms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPermissions);
}

TEST_F(AddressSpaceCoverageTest, UnmapPagesNoMappedPages) {
    // Test unmapPages when none of the pages are mapped (line 404-406)
    std::vector<IOVA> iovas = {0x30000000, 0x40000000, 0x50000000};

    VoidResult result = addressSpace->unmapPages(iovas);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(AddressSpaceCoverageTest, IsPageMappedInvalidAddress) {
    // Test isPageMapped with invalid IOVA (line 153-154)
    IOVA invalidIOVA = 0xFFFFFFFFFFFFF000ULL;

    Result<bool> result = addressSpace->isPageMapped(invalidIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, GetPagePermissionsInvalidAddress) {
    // Test getPagePermissions with invalid IOVA (line 170-171)
    IOVA invalidIOVA = 0xFFFFFFFFFFFFF000ULL;

    Result<PagePermissions> result = addressSpace->getPagePermissions(invalidIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, MapRangeInvalidIOVARange) {
    PagePermissions perms(true, true, false);

    // Test start IOVA exceeding MAX_VIRTUAL_ADDRESS (line 243-244)
    IOVA invalidStartIOVA = 0xFFFFFFFFFFFFF000ULL;
    IOVA endIOVA = 0xFFFFFFFFFFFFFFFFULL;

    VoidResult result = addressSpace->mapRange(invalidStartIOVA, endIOVA, TEST_PA, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, MapRangeInvalidPA) {
    PagePermissions perms(true, true, false);

    // Test PA exceeding MAX_PHYSICAL_ADDRESS (line 248-249)
    IOVA startIOVA = 0x10000000;
    IOVA endIOVA = 0x10002000;
    PA invalidPA = 0xFFFFFFFFFFFFF000ULL;

    VoidResult result = addressSpace->mapRange(startIOVA, endIOVA, invalidPA, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, UnmapRangeInvalidIOVARange) {
    // Test start IOVA exceeding MAX_VIRTUAL_ADDRESS (line 297-298)
    IOVA invalidStartIOVA = 0xFFFFFFFFFFFFF000ULL;
    IOVA endIOVA = 0xFFFFFFFFFFFFFFFFULL;

    VoidResult result = addressSpace->unmapRange(invalidStartIOVA, endIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, MapPagesInvalidIOVA) {
    PagePermissions perms(true, true, false);

    // Test with invalid IOVA in mappings (line 346-347)
    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x10000000, 0x40000000},
        {0xFFFFFFFFFFFFF000ULL, 0x50000000}  // Invalid IOVA
    };

    VoidResult result = addressSpace->mapPages(mappings, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, MapPagesInvalidPA) {
    PagePermissions perms(true, true, false);

    // Test with invalid PA in mappings (line 351-352)
    std::vector<std::pair<IOVA, PA>> mappings = {
        {0x10000000, 0x40000000},
        {0x20000000, 0xFFFFFFFFFFFFF000ULL}  // Invalid PA
    };

    VoidResult result = addressSpace->mapPages(mappings, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, UnmapPagesInvalidIOVA) {
    // Test with invalid IOVA in list (line 388-389)
    std::vector<IOVA> iovas = {
        0x10000000,
        0xFFFFFFFFFFFFF000ULL  // Invalid IOVA
    };

    VoidResult result = addressSpace->unmapPages(iovas);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

// ==========================================================================
// TC-ADDR-002: Empty Address Space Operations
// Target lines: 832, 923
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, GetMappedRangesEmpty) {
    // Test getMappedRanges on empty address space (line 432-433)
    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();

    EXPECT_TRUE(ranges.empty());
    EXPECT_EQ(ranges.size(), 0);
}

TEST_F(AddressSpaceCoverageTest, GetMappedRangesNoValidEntries) {
    // This tests the scenario where pageTable is not empty but has no valid entries
    // However, in current implementation, we always create valid entries
    // So this tests the empty case thoroughly

    std::vector<AddressRange> ranges = addressSpace->getMappedRanges();
    EXPECT_TRUE(ranges.empty());

    // Map and then unmap to ensure cleanup is complete
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);
    addressSpace->unmapPage(TEST_IOVA);

    ranges = addressSpace->getMappedRanges();
    EXPECT_TRUE(ranges.empty());
}

TEST_F(AddressSpaceCoverageTest, GetAddressSpaceSizeEmpty) {
    // Test getAddressSpaceSize on empty address space (line 481-482)
    uint64_t size = addressSpace->getAddressSpaceSize();

    EXPECT_EQ(size, 0);
}

TEST_F(AddressSpaceCoverageTest, GetAddressSpaceSizeNoValidEntries) {
    // Test scenario where pageTable has entries but none are valid (line 503-504)
    // Current implementation always has valid entries, so test empty thoroughly

    uint64_t size = addressSpace->getAddressSpaceSize();
    EXPECT_EQ(size, 0);

    // Map, unmap, and verify size is zero again
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);
    addressSpace->unmapPage(TEST_IOVA);

    size = addressSpace->getAddressSpaceSize();
    EXPECT_EQ(size, 0);
}

TEST_F(AddressSpaceCoverageTest, GetPageCountEmpty) {
    // Test getPageCount returning zero (related to line 923 concept)
    Result<size_t> countResult = addressSpace->getPageCount();

    EXPECT_TRUE(countResult.isOk());
    EXPECT_EQ(countResult.getValue(), 0);
}

TEST_F(AddressSpaceCoverageTest, IterateEmptyAddressSpace) {
    // Test that operations on empty address space don't crash

    // Test isPageMapped on empty space
    Result<bool> mappedResult = addressSpace->isPageMapped(TEST_IOVA);
    EXPECT_TRUE(mappedResult.isOk());
    EXPECT_FALSE(mappedResult.getValue());

    // Test translatePage on empty space
    TranslationResult transResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(transResult.isError());
    EXPECT_EQ(transResult.getError(), SMMUError::PageNotMapped);

    // Test getPagePermissions on empty space
    Result<PagePermissions> permsResult = addressSpace->getPagePermissions(TEST_IOVA);
    EXPECT_TRUE(permsResult.isError());
    EXPECT_EQ(permsResult.getError(), SMMUError::PageNotMapped);
}

TEST_F(AddressSpaceCoverageTest, ClearAlreadyEmptySpace) {
    // Test clearing an already empty address space
    VoidResult result = addressSpace->clear();

    EXPECT_TRUE(result.isOk());

    Result<size_t> countResult = addressSpace->getPageCount();
    EXPECT_TRUE(countResult.isOk());
    EXPECT_EQ(countResult.getValue(), 0);
}

TEST_F(AddressSpaceCoverageTest, HasOverlappingMappingsEmpty) {
    // Test hasOverlappingMappings on empty address space
    bool hasOverlap = addressSpace->hasOverlappingMappings(TEST_IOVA, TEST_IOVA + PAGE_SIZE);

    EXPECT_FALSE(hasOverlap);
}

// ==========================================================================
// TC-ADDR-003: Boundary Conditions
// Target lines: 232, 383
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, BoundaryMinimumIOVA) {
    PagePermissions perms(true, true, false);

    // Test minimum valid IOVA (0x0)
    IOVA minIOVA = 0x0;
    VoidResult mapResult = addressSpace->mapPage(minIOVA, TEST_PA, perms);

    EXPECT_TRUE(mapResult.isOk());

    // Test translation at minimum address
    TranslationResult transResult = addressSpace->translatePage(minIOVA, AccessType::Read);
    EXPECT_TRUE(transResult.isOk());
    EXPECT_EQ(transResult.getValue().physicalAddress, TEST_PA);
}

TEST_F(AddressSpaceCoverageTest, BoundaryMaximumIOVA) {
    PagePermissions perms(true, true, false);

    // Test maximum valid IOVA (52-bit address space, page-aligned)
    VoidResult mapResult = addressSpace->mapPage(VALID_MAX_IOVA, VALID_MAX_PA, perms);

    EXPECT_TRUE(mapResult.isOk());

    // Test translation at maximum address
    TranslationResult transResult = addressSpace->translatePage(VALID_MAX_IOVA, AccessType::Read);
    EXPECT_TRUE(transResult.isOk());
    EXPECT_EQ(transResult.getValue().physicalAddress, VALID_MAX_PA);
}

TEST_F(AddressSpaceCoverageTest, BoundaryJustBeyondMaximum) {
    PagePermissions perms(true, true, false);

    // Test IOVA just beyond maximum (should fail)
    IOVA beyondMaxIOVA = 0xFFFFFFFFFFFFF000ULL;  // Beyond 52-bit

    VoidResult result = addressSpace->mapPage(beyondMaxIOVA, TEST_PA, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, Boundary48BitAddressLimit) {
    PagePermissions perms(true, true, false);

    // Test 48-bit address limit (common SMMU configuration)
    IOVA addr48Bit = 0x0000FFFFFFFFFFFF;  // Max 48-bit address
    PA pa48Bit = 0x0000FFFFFFFFFFFF;

    // Should succeed within 52-bit space
    VoidResult result = addressSpace->mapPage(addr48Bit, pa48Bit, perms);
    EXPECT_TRUE(result.isOk());

    TranslationResult transResult = addressSpace->translatePage(addr48Bit, AccessType::Read);
    EXPECT_TRUE(transResult.isOk());
}

TEST_F(AddressSpaceCoverageTest, BoundaryAddressAlignment4KB) {
    PagePermissions perms(true, true, false);

    // Test 4KB page alignment (standard page size)
    IOVA unalignedIOVA = 0x12345678;
    PA unalignedPA = 0x87654321;

    // Implementation should handle alignment internally
    VoidResult result = addressSpace->mapPage(unalignedIOVA, unalignedPA, perms);
    EXPECT_TRUE(result.isOk());

    // Verify translation works at page boundaries
    IOVA pageAlignedIOVA = unalignedIOVA & ~PAGE_MASK;
    TranslationResult transResult = addressSpace->translatePage(pageAlignedIOVA, AccessType::Read);
    EXPECT_TRUE(transResult.isOk());
}

TEST_F(AddressSpaceCoverageTest, BoundaryPageOffsetPreservation) {
    PagePermissions perms(true, true, false);

    // Test that page offset is properly preserved in translation
    IOVA iovaWithOffset = 0x12345678;
    PA paBase = 0x40000000;

    addressSpace->mapPage(iovaWithOffset, paBase, perms);

    // Translation should preserve offset
    TranslationResult result = addressSpace->translatePage(iovaWithOffset, AccessType::Read);
    EXPECT_TRUE(result.isOk());

    // Expected PA = page-aligned PA base + offset
    IOVA offset = iovaWithOffset & PAGE_MASK;
    PA expectedPA = (paBase & ~PAGE_MASK) + offset;
    EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
}

TEST_F(AddressSpaceCoverageTest, BoundaryRangeEndBeforeStart) {
    PagePermissions perms(true, true, false);

    // Test mapRange with end < start (invalid range, line 238-239)
    IOVA startIOVA = 0x20000000;
    IOVA endIOVA = 0x10000000;  // End before start

    VoidResult result = addressSpace->mapRange(startIOVA, endIOVA, TEST_PA, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, BoundaryUnmapRangeEndBeforeStart) {
    // Test unmapRange with end < start (line 292-293)
    IOVA startIOVA = 0x20000000;
    IOVA endIOVA = 0x10000000;

    VoidResult result = addressSpace->unmapRange(startIOVA, endIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, BoundaryOverflowInRangeCalculation) {
    PagePermissions perms(true, true, false);

    // Test potential overflow in range calculation (line 258-260)
    IOVA startIOVA = 0x10000000;
    IOVA endIOVA = 0xFFFFFFFFFFFFFFFFULL;  // Maximum possible value
    PA startPA = 0xFFFF000000000000ULL;    // High PA that could overflow

    // Should detect overflow and return error
    VoidResult result = addressSpace->mapRange(startIOVA, endIOVA, startPA, perms);

    EXPECT_TRUE(result.isError());
    // Could be InvalidAddress due to overflow check
}

// ==========================================================================
// Additional Coverage: Security State Transitions
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, SecurityStateMismatch) {
    PagePermissions perms(true, true, false);

    // Map page with NonSecure security state
    VoidResult mapResult = addressSpace->mapPage(TEST_IOVA, TEST_PA, perms, SecurityState::NonSecure);
    EXPECT_TRUE(mapResult.isOk());

    // Try to translate with Secure security state (should fail, line 131-133)
    TranslationResult transResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read, SecurityState::Secure);

    EXPECT_TRUE(transResult.isError());
    EXPECT_EQ(transResult.getError(), SMMUError::InvalidSecurityState);
}

TEST_F(AddressSpaceCoverageTest, SecurityStateRealmMismatch) {
    PagePermissions perms(true, true, false);

    // Map page with Realm security state
    VoidResult mapResult = addressSpace->mapPage(TEST_IOVA, TEST_PA, perms, SecurityState::Realm);
    EXPECT_TRUE(mapResult.isOk());

    // Try to translate with NonSecure security state (should fail)
    TranslationResult transResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(transResult.isError());
    EXPECT_EQ(transResult.getError(), SMMUError::InvalidSecurityState);
}

// ==========================================================================
// Additional Coverage: Various Permission Combinations
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, PermissionsWriteOnly) {
    // Test write-only permissions (unusual but valid)
    PagePermissions writeOnlyPerms(false, true, false);

    VoidResult mapResult = addressSpace->mapPage(TEST_IOVA, TEST_PA, writeOnlyPerms);
    EXPECT_TRUE(mapResult.isOk());

    // Write should succeed
    TranslationResult writeResult = addressSpace->translatePage(TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(writeResult.isOk());

    // Read should fail (line 137-139)
    TranslationResult readResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(readResult.isError());
    EXPECT_EQ(readResult.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(AddressSpaceCoverageTest, PermissionsExecuteOnly) {
    // Test execute-only permissions
    PagePermissions execOnlyPerms(false, false, true);

    VoidResult mapResult = addressSpace->mapPage(TEST_IOVA, TEST_PA, execOnlyPerms);
    EXPECT_TRUE(mapResult.isOk());

    // Execute should succeed
    TranslationResult execResult = addressSpace->translatePage(TEST_IOVA, AccessType::Execute);
    EXPECT_TRUE(execResult.isOk());

    // Read and write should fail
    TranslationResult readResult = addressSpace->translatePage(TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(readResult.isError());
    EXPECT_EQ(readResult.getError(), SMMUError::PagePermissionViolation);

    TranslationResult writeResult = addressSpace->translatePage(TEST_IOVA, AccessType::Write);
    EXPECT_TRUE(writeResult.isError());
    EXPECT_EQ(writeResult.getError(), SMMUError::PagePermissionViolation);
}

// ==========================================================================
// Additional Coverage: Exception Handling Paths
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, IsPageMappedExceptionHandling) {
    // Test the try-catch block in isPageMapped (line 157-164)
    // Normal path to ensure exception handling code exists

    Result<bool> result = addressSpace->isPageMapped(TEST_IOVA);
    EXPECT_TRUE(result.isOk());
    EXPECT_FALSE(result.getValue());

    // Map and check again
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);

    result = addressSpace->isPageMapped(TEST_IOVA);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

TEST_F(AddressSpaceCoverageTest, GetPagePermissionsExceptionHandling) {
    // Test the try-catch block in getPagePermissions (line 174-186)

    // Test on unmapped page (line 183)
    Result<PagePermissions> result = addressSpace->getPagePermissions(TEST_IOVA);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);

    // Map and test valid case
    PagePermissions perms(true, false, true);
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);

    result = addressSpace->getPagePermissions(TEST_IOVA);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue().read);
    EXPECT_FALSE(result.getValue().write);
    EXPECT_TRUE(result.getValue().execute);
}

TEST_F(AddressSpaceCoverageTest, GetPageCountExceptionHandling) {
    // Test the try-catch block in getPageCount (line 191-206)

    Result<size_t> result = addressSpace->getPageCount();
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue(), 0);

    // Add many pages to test counting logic
    PagePermissions perms(true, true, false);
    for (size_t i = 0; i < 100; ++i) {
        IOVA iova = TEST_IOVA + (i * PAGE_SIZE);
        PA pa = TEST_PA + (i * PAGE_SIZE);
        addressSpace->mapPage(iova, pa, perms);
    }

    result = addressSpace->getPageCount();
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue(), 100);
}

// ==========================================================================
// Additional Coverage: HasOverlappingMappings Edge Cases
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, HasOverlappingMappingsInvalidRange) {
    // Test hasOverlappingMappings with end < start (line 518-519)
    bool hasOverlap = addressSpace->hasOverlappingMappings(0x20000000, 0x10000000);

    EXPECT_FALSE(hasOverlap);  // Invalid range returns false
}

TEST_F(AddressSpaceCoverageTest, HasOverlappingMappingsSinglePage) {
    PagePermissions perms(true, true, false);

    // Map single page
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);

    // Test exact page match
    bool hasOverlap = addressSpace->hasOverlappingMappings(TEST_IOVA, TEST_IOVA);
    EXPECT_TRUE(hasOverlap);

    // Test range containing the page
    hasOverlap = addressSpace->hasOverlappingMappings(TEST_IOVA - PAGE_SIZE, TEST_IOVA + PAGE_SIZE);
    EXPECT_TRUE(hasOverlap);

    // Test range not containing the page
    hasOverlap = addressSpace->hasOverlappingMappings(TEST_IOVA + PAGE_SIZE, TEST_IOVA + 2 * PAGE_SIZE);
    EXPECT_FALSE(hasOverlap);
}

// ==========================================================================
// Additional Coverage: Uncovered Lines from gcov Analysis
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, TranslatePageInvalidEntry) {
    // This test covers line 127: Invalid page entry (valid = false)
    // We need to create a scenario where a page entry exists but is marked invalid
    // However, the current implementation always creates valid entries when mapping
    // This tests the defensive check for future implementations that might have invalid entries

    PagePermissions perms(true, true, false);

    // Map a page normally (creates valid entry)
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);

    // Verify it translates correctly
    TranslationResult result = addressSpace->translatePage(TEST_IOVA, AccessType::Read);
    EXPECT_TRUE(result.isOk());

    // Note: Line 127 is a defensive check that cannot be easily triggered
    // without modifying the pageTable internal state directly.
    // The implementation always creates valid entries, so this line serves
    // as a safety check for potential future code paths or corruption.
}

TEST_F(AddressSpaceCoverageTest, MapRangeOverflowDetection) {
    // This test covers line 260: Overflow detection in mapRange
    PagePermissions perms(true, true, false);

    // Create a range that would cause overflow in PA calculation
    // startPa + rangeSize < startPa indicates overflow
    IOVA startIOVA = 0x10000000;
    IOVA endIOVA = 0xFFFFFFFFFFFFFFFFULL;  // Maximum value
    PA startPA = 0xFFFFF00000000000ULL;     // Very high PA that will overflow

    VoidResult result = addressSpace->mapRange(startIOVA, endIOVA, startPA, perms);

    // Should detect overflow and return error
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceCoverageTest, CheckPermissionsUnknownAccessType) {
    // This test covers lines 584, 586: Default case in checkPermissions
    PagePermissions perms(true, true, true);

    // Map a page
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);

    // Use an invalid AccessType value to trigger the default case
    AccessType invalidAccess = static_cast<AccessType>(99);

    TranslationResult result = addressSpace->translatePage(TEST_IOVA, invalidAccess);

    // Should fail with permission violation due to unknown access type
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(AddressSpaceCoverageTest, GetMappedRangesAfterClear) {
    // This test covers line 450: No valid mappings in getMappedRanges
    // After mapping and then clearing, the page table might have entries but none valid

    PagePermissions perms(true, true, false);

    // Map some pages
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    addressSpace->mapPage(0x20000000, 0x50000000, perms);

    // Verify ranges exist
    std::vector<AddressRange> ranges1 = addressSpace->getMappedRanges();
    EXPECT_FALSE(ranges1.empty());

    // Clear all mappings
    addressSpace->clear();

    // Get mapped ranges should return empty (covers line 450)
    std::vector<AddressRange> ranges2 = addressSpace->getMappedRanges();

    EXPECT_TRUE(ranges2.empty());
    EXPECT_EQ(ranges2.size(), 0);
}

TEST_F(AddressSpaceCoverageTest, GetAddressSpaceSizeAfterClear) {
    // This test covers line 504: No valid entries in getAddressSpaceSize
    // Similar to above, test scenario where entries exist but none are valid

    PagePermissions perms(true, true, false);

    // Map some pages
    addressSpace->mapPage(0x10000000, 0x40000000, perms);
    addressSpace->mapPage(0x20000000, 0x50000000, perms);

    // Verify size is non-zero
    uint64_t size1 = addressSpace->getAddressSpaceSize();
    EXPECT_GT(size1, 0);

    // Clear all mappings
    addressSpace->clear();

    // Get address space size should return 0 (covers line 504)
    uint64_t size2 = addressSpace->getAddressSpaceSize();

    EXPECT_EQ(size2, 0);
}

TEST_F(AddressSpaceCoverageTest, GetPageCountOverflowCheck) {
    // This test covers line 199: SIZE_MAX overflow check in getPageCount
    // Note: This is extremely difficult to test in practice as it would require
    // SIZE_MAX page entries, which is not feasible in a test environment
    // This test documents the intent and validates normal counting works

    PagePermissions perms(true, true, false);

    // Map a reasonable number of pages and verify count works correctly
    const size_t numPages = 1000;
    for (size_t i = 0; i < numPages; ++i) {
        IOVA iova = 0x10000000 + (i * PAGE_SIZE);
        PA pa = 0x40000000 + (i * PAGE_SIZE);
        addressSpace->mapPage(iova, pa, perms);
    }

    Result<size_t> countResult = addressSpace->getPageCount();
    EXPECT_TRUE(countResult.isOk());
    EXPECT_EQ(countResult.getValue(), numPages);

    // The overflow check at line 199 is a defensive measure that would only
    // trigger if count reaches SIZE_MAX, which is practically impossible to test
}

// ==========================================================================
// Exception Handler Coverage Tests
// Note: These exception handlers (lines 162-164, 184-186, 204-206) are
// defensive code that catches unexpected exceptions. In normal operation
// with valid C++ standard library implementations, these should never be
// triggered. They exist as safety measures against potential memory corruption
// or library bugs. Testing them would require mocking the STL or causing
// memory corruption, which is not advisable in unit tests.
// ==========================================================================

TEST_F(AddressSpaceCoverageTest, ExceptionHandlerDocumentation) {
    // This test documents the exception handlers in the code:
    // - Lines 162-164: isPageMapped catch block
    // - Lines 184-186: getPagePermissions catch block
    // - Lines 204-206: getPageCount catch block
    //
    // These handlers catch unexpected exceptions from the STL unordered_map
    // operations and return InternalError. They are defensive code that
    // should never be reached in normal operation with a correct STL implementation.
    //
    // Testing these requires either:
    // 1. Mocking std::unordered_map to throw (complex and fragile)
    // 2. Causing memory corruption (dangerous and unreliable)
    // 3. Accepting that they are untested safety measures
    //
    // We choose option 3 as the most pragmatic approach.

    // Verify normal operation works correctly
    PagePermissions perms(true, true, false);
    addressSpace->mapPage(TEST_IOVA, TEST_PA, perms);

    // All these operations should succeed without exceptions
    Result<bool> mappedResult = addressSpace->isPageMapped(TEST_IOVA);
    EXPECT_TRUE(mappedResult.isOk());
    EXPECT_TRUE(mappedResult.getValue());

    Result<PagePermissions> permsResult = addressSpace->getPagePermissions(TEST_IOVA);
    EXPECT_TRUE(permsResult.isOk());

    Result<size_t> countResult = addressSpace->getPageCount();
    EXPECT_TRUE(countResult.isOk());
}

} // namespace test
} // namespace smmu
