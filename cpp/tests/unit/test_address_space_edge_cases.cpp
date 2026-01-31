// ARM SMMU v3 Address Space Edge Cases Tests
// Copyright (c) 2024 John Greninger
//
// This test suite provides comprehensive coverage of edge cases in address_space.cpp
// Focuses on boundary conditions, error handling, and exceptional scenarios.
//
// Coverage Targets:
// - Boundary addresses (0x0, MAX_VIRTUAL_ADDRESS, MAX_PHYSICAL_ADDRESS)
// - Page alignment edge cases
// - Permission bit combinations
// - Large page mappings
// - Overlapping mappings (error cases)
// - Unmapping non-existent pages
// - Exception handlers (lines 157-164, 174-186, 191-206)
// - Security state validation (lines 130-134)
// - Address range validation (lines 236-249)
// - Large range operations

#include <gtest/gtest.h>
#include "smmu/address_space.h"
#include "smmu/types.h"
#include <memory>
#include <limits>

namespace smmu {
namespace test {

class AddressSpaceEdgeCasesTest : public ::testing::Test {
protected:
    void SetUp() override {
        addressSpace = std::make_unique<AddressSpace>();
    }

    void TearDown() override {
        addressSpace.reset();
    }

    std::unique_ptr<AddressSpace> addressSpace;

    // Test constants
    static constexpr IOVA ZERO_ADDRESS = 0x0;
    static constexpr IOVA PAGE_ALIGNED = 0x1000;
    static constexpr IOVA UNALIGNED_ADDRESS = 0x1234;
    static constexpr PA TEST_PA = 0x40000000;
};

// ========== Boundary Address Tests ==========

TEST_F(AddressSpaceEdgeCasesTest, MapPage_ZeroIOVA_Success) {
    // Test mapping at address 0x0
    PagePermissions perms(true, false, false);
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(ZERO_ADDRESS, pa, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // Verify translation works
    TranslationResult transResult = addressSpace->translatePage(ZERO_ADDRESS, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(transResult.isOk());
    if (transResult.isOk()) {
        EXPECT_EQ(transResult.getValue().physicalAddress, pa);
    }
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_MaxVirtualAddress_Success) {
    // Test mapping at maximum virtual address
    PagePermissions perms(true, false, false);
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(MAX_VIRTUAL_ADDRESS, pa, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    TranslationResult transResult = addressSpace->translatePage(MAX_VIRTUAL_ADDRESS, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(transResult.isOk());
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_OverMaxVirtualAddress_ReturnsError) {
    // Target: Lines 38-41 - IOVA validation
    PagePermissions perms(true, false, false);
    PA pa = 0x80000000;
    IOVA overMax = MAX_VIRTUAL_ADDRESS + 1;

    VoidResult result = addressSpace->mapPage(overMax, pa, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_MaxPhysicalAddress_Success) {
    // Test mapping to maximum physical address
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;

    VoidResult result = addressSpace->mapPage(iova, MAX_PHYSICAL_ADDRESS, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_OverMaxPhysicalAddress_ReturnsError) {
    // Target: Lines 43-46 - PA validation
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;
    PA overMax = MAX_PHYSICAL_ADDRESS + 1;

    VoidResult result = addressSpace->mapPage(iova, overMax, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

// ========== Page Alignment Edge Cases ==========

TEST_F(AddressSpaceEdgeCasesTest, MapPage_UnalignedIOVA_MapsToPageNumber) {
    // IOVA is converted to page number, so unaligned addresses should work
    PagePermissions perms(true, false, false);
    IOVA unaligned = 0x12345;
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(unaligned, pa, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // Translation should work for any address in the same page
    IOVA alignedIOVA = (unaligned / PAGE_SIZE) * PAGE_SIZE;
    TranslationResult transResult = addressSpace->translatePage(alignedIOVA, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(transResult.isOk());
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_UnalignedPA_AlignedAutomatically) {
    // Target: Line 65 - PA alignment to page boundary
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;
    PA unalignedPA = 0x80001234;  // Unaligned PA

    VoidResult result = addressSpace->mapPage(iova, unalignedPA, perms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // Translation should return page-aligned PA
    TranslationResult transResult = addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(transResult.isOk());
    if (transResult.isOk()) {
        PA alignedPA = (unalignedPA / PAGE_SIZE) * PAGE_SIZE;
        EXPECT_EQ(transResult.getValue().physicalAddress, alignedPA);
    }
}

TEST_F(AddressSpaceEdgeCasesTest, TranslatePage_PageOffset_PreservedInResult) {
    // Target: Lines 143-144 - Page offset preservation
    PagePermissions perms(true, false, false);
    IOVA baseIOVA = 0x10000000;
    PA basePA = 0x80000000;

    addressSpace->mapPage(baseIOVA, basePA, perms, SecurityState::NonSecure);

    // Translate with offset
    uint64_t offset = 0x567;
    IOVA iovaWithOffset = baseIOVA + offset;

    TranslationResult result = addressSpace->translatePage(iovaWithOffset, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA should include the offset
        EXPECT_EQ(result.getValue().physicalAddress, basePA + offset);
    }
}

// ========== Permission Bit Combinations ==========

TEST_F(AddressSpaceEdgeCasesTest, MapPage_NoPermissions_ReturnsError) {
    // Target: Lines 48-51 - Permission validation
    PagePermissions noPerms(false, false, false);  // All permissions disabled
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(iova, pa, noPerms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPermissions);
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_ReadOnlyPermission_Success) {
    PagePermissions readOnly(true, false, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(iova, pa, readOnly, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // Read should succeed
    TranslationResult readResult = addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(readResult.isOk());

    // Write should fail
    TranslationResult writeResult = addressSpace->translatePage(iova, AccessType::Write, SecurityState::NonSecure);
    EXPECT_TRUE(writeResult.isError());
    EXPECT_EQ(writeResult.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_WriteOnlyPermission_Success) {
    PagePermissions writeOnly(false, true, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(iova, pa, writeOnly, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // Write should succeed
    TranslationResult writeResult = addressSpace->translatePage(iova, AccessType::Write, SecurityState::NonSecure);
    EXPECT_TRUE(writeResult.isOk());

    // Read should fail
    TranslationResult readResult = addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(readResult.isError());
    EXPECT_EQ(readResult.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_ExecuteOnlyPermission_Success) {
    PagePermissions executeOnly(false, false, true);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(iova, pa, executeOnly, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // Execute should succeed
    TranslationResult execResult = addressSpace->translatePage(iova, AccessType::Execute, SecurityState::NonSecure);
    EXPECT_TRUE(execResult.isOk());

    // Read should fail
    TranslationResult readResult = addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(readResult.isError());
    EXPECT_EQ(readResult.getError(), SMMUError::PagePermissionViolation);
}

TEST_F(AddressSpaceEdgeCasesTest, MapPage_AllPermissions_Success) {
    PagePermissions allPerms(true, true, true);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapPage(iova, pa, allPerms, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());

    // All access types should succeed
    EXPECT_TRUE(addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure).isOk());
    EXPECT_TRUE(addressSpace->translatePage(iova, AccessType::Write, SecurityState::NonSecure).isOk());
    EXPECT_TRUE(addressSpace->translatePage(iova, AccessType::Execute, SecurityState::NonSecure).isOk());
}

// ========== Security State Validation Tests ==========

TEST_F(AddressSpaceEdgeCasesTest, MapPage_InvalidSecurityState_ReturnsError) {
    // Target: Lines 53-58 - Security state validation
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    // Use invalid security state (cast from invalid value)
    SecurityState invalidState = static_cast<SecurityState>(99);

    VoidResult result = addressSpace->mapPage(iova, pa, perms, invalidState);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidSecurityState);
}

TEST_F(AddressSpaceEdgeCasesTest, TranslatePage_SecurityStateMismatch_ReturnsSecurityFault) {
    // Target: Lines 130-134 - Security state mismatch
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    // Map with Secure state
    addressSpace->mapPage(iova, pa, perms, SecurityState::Secure);

    // Try to access with NonSecure state
    TranslationResult result = addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidSecurityState);
}

TEST_F(AddressSpaceEdgeCasesTest, TranslatePage_AllSecurityStatesWork) {
    PagePermissions perms(true, false, false);
    IOVA iovaNonSecure = 0x10000000;
    IOVA iovaSecure = 0x20000000;
    IOVA iovaRealm = 0x30000000;
    PA pa = 0x80000000;

    // Map with different security states
    addressSpace->mapPage(iovaNonSecure, pa, perms, SecurityState::NonSecure);
    addressSpace->mapPage(iovaSecure, pa + PAGE_SIZE, perms, SecurityState::Secure);
    addressSpace->mapPage(iovaRealm, pa + 2*PAGE_SIZE, perms, SecurityState::Realm);

    // Each should translate only with matching security state
    EXPECT_TRUE(addressSpace->translatePage(iovaNonSecure, AccessType::Read, SecurityState::NonSecure).isOk());
    EXPECT_TRUE(addressSpace->translatePage(iovaSecure, AccessType::Read, SecurityState::Secure).isOk());
    EXPECT_TRUE(addressSpace->translatePage(iovaRealm, AccessType::Read, SecurityState::Realm).isOk());
}

// ========== Overlapping Mappings Tests ==========

TEST_F(AddressSpaceEdgeCasesTest, MapPage_OverwriteExisting_UpdatesMapping) {
    PagePermissions perms1(true, false, false);
    PagePermissions perms2(true, true, false);
    IOVA iova = 0x10000000;
    PA pa1 = 0x80000000;
    PA pa2 = 0x90000000;

    // Map once
    VoidResult result1 = addressSpace->mapPage(iova, pa1, perms1, SecurityState::NonSecure);
    ASSERT_TRUE(result1.isOk());

    // Overwrite with different mapping
    VoidResult result2 = addressSpace->mapPage(iova, pa2, perms2, SecurityState::NonSecure);
    EXPECT_TRUE(result2.isOk());

    // Translation should use new mapping
    TranslationResult transResult = addressSpace->translatePage(iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(transResult.isOk());
    if (transResult.isOk()) {
        EXPECT_EQ(transResult.getValue().physicalAddress, pa2);
        EXPECT_TRUE(transResult.getValue().permissions.write);  // New permissions
    }
}

// ========== Unmapping Tests ==========

TEST_F(AddressSpaceEdgeCasesTest, UnmapPage_NonExistentPage_ReturnsError) {
    // Target: Lines 86-91 - Unmapping non-existent page
    IOVA unmappedIOVA = 0x10000000;

    VoidResult result = addressSpace->unmapPage(unmappedIOVA);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(AddressSpaceEdgeCasesTest, UnmapPage_InvalidAddress_ReturnsError) {
    // Target: Lines 79-82 - IOVA validation in unmapPage
    IOVA overMax = MAX_VIRTUAL_ADDRESS + 1;

    VoidResult result = addressSpace->unmapPage(overMax);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, UnmapPage_AlreadyUnmapped_ReturnsError) {
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    // Map and then unmap
    addressSpace->mapPage(iova, pa, perms, SecurityState::NonSecure);
    VoidResult result1 = addressSpace->unmapPage(iova);
    ASSERT_TRUE(result1.isOk());

    // Try to unmap again
    VoidResult result2 = addressSpace->unmapPage(iova);

    EXPECT_TRUE(result2.isError());
    EXPECT_EQ(result2.getError(), SMMUError::PageNotMapped);
}

// ========== Exception Handler Tests ==========

TEST_F(AddressSpaceEdgeCasesTest, IsPageMapped_ValidAddress_NoException) {
    // Target: Lines 157-164 - Exception handling in isPageMapped
    PagePermissions perms(true, false, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    addressSpace->mapPage(iova, pa, perms, SecurityState::NonSecure);

    Result<bool> result = addressSpace->isPageMapped(iova);

    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

TEST_F(AddressSpaceEdgeCasesTest, IsPageMapped_InvalidAddress_ReturnsError) {
    // Target: Lines 152-155 - Address validation in isPageMapped
    IOVA overMax = MAX_VIRTUAL_ADDRESS + 1;

    Result<bool> result = addressSpace->isPageMapped(overMax);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, GetPagePermissions_ValidPage_ReturnsPermissions) {
    // Target: Lines 174-186 - Permission query
    PagePermissions perms(true, true, false);
    IOVA iova = 0x10000000;
    PA pa = 0x80000000;

    addressSpace->mapPage(iova, pa, perms, SecurityState::NonSecure);

    Result<PagePermissions> result = addressSpace->getPagePermissions(iova);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        PagePermissions retrieved = result.getValue();
        EXPECT_TRUE(retrieved.read);
        EXPECT_TRUE(retrieved.write);
        EXPECT_FALSE(retrieved.execute);
    }
}

TEST_F(AddressSpaceEdgeCasesTest, GetPagePermissions_UnmappedPage_ReturnsError) {
    // Target: Lines 182-183 - Unmapped page error
    IOVA unmapped = 0x10000000;

    Result<PagePermissions> result = addressSpace->getPagePermissions(unmapped);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped);
}

TEST_F(AddressSpaceEdgeCasesTest, GetPagePermissions_InvalidAddress_ReturnsError) {
    // Target: Lines 169-172 - Address validation
    IOVA overMax = MAX_VIRTUAL_ADDRESS + 1;

    Result<PagePermissions> result = addressSpace->getPagePermissions(overMax);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, GetPageCount_EmptyAddressSpace_ReturnsZero) {
    // Target: Lines 190-206 - Page count calculation
    Result<size_t> result = addressSpace->getPageCount();

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue(), 0u);
}

TEST_F(AddressSpaceEdgeCasesTest, GetPageCount_MultipleMappings_ReturnsCorrectCount) {
    // Target: Lines 191-202 - Count valid entries
    PagePermissions perms(true, false, false);
    PA pa = 0x80000000;

    // Map 10 pages
    for (int i = 0; i < 10; i++) {
        addressSpace->mapPage(0x10000000 + i * PAGE_SIZE, pa + i * PAGE_SIZE, perms, SecurityState::NonSecure);
    }

    Result<size_t> result = addressSpace->getPageCount();

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue(), 10u);
}

// ========== Range Operation Tests ==========

TEST_F(AddressSpaceEdgeCasesTest, MapRange_InvalidRange_EndBeforeStart_ReturnsError) {
    // Target: Lines 237-240 - Range validation
    PagePermissions perms(true, false, false);
    IOVA start = 0x20000000;
    IOVA end = 0x10000000;  // End before start
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapRange(start, end, pa, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, MapRange_OverMaxAddress_ReturnsError) {
    // Target: Lines 242-245 - Address limit validation
    PagePermissions perms(true, false, false);
    IOVA start = MAX_VIRTUAL_ADDRESS - PAGE_SIZE;
    IOVA end = MAX_VIRTUAL_ADDRESS + PAGE_SIZE;  // Exceeds max
    PA pa = 0x80000000;

    VoidResult result = addressSpace->mapRange(start, end, pa, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, MapRange_OverMaxPhysicalAddress_ReturnsError) {
    // Target: Lines 247-249 - PA validation
    PagePermissions perms(true, false, false);
    IOVA start = 0x10000000;
    IOVA end = 0x20000000;
    PA pa = MAX_PHYSICAL_ADDRESS + 1;

    VoidResult result = addressSpace->mapRange(start, end, pa, perms);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidAddress);
}

TEST_F(AddressSpaceEdgeCasesTest, Clear_AfterMappings_RemovesAll) {
    // Target: Lines 210-217 - Clear operation
    PagePermissions perms(true, false, false);
    PA pa = 0x80000000;

    // Map several pages
    for (int i = 0; i < 5; i++) {
        addressSpace->mapPage(0x10000000 + i * PAGE_SIZE, pa + i * PAGE_SIZE, perms, SecurityState::NonSecure);
    }

    ASSERT_EQ(addressSpace->getPageCount().getValue(), 5u);

    VoidResult result = addressSpace->clear();

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(addressSpace->getPageCount().getValue(), 0u);
}

} // namespace test
} // namespace smmu
