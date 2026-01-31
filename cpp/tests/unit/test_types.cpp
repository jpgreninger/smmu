// ARM SMMU v3 Types Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/types.h"

namespace smmu {
namespace test {

class TypesTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Setup test fixtures if needed
    }

    void TearDown() override {
        // Cleanup test fixtures if needed
    }
};

// Test core type definitions and sizes
TEST_F(TypesTest, CoreTypeSizes) {
    // Verify type sizes meet SMMU v3 specification requirements
    EXPECT_EQ(sizeof(StreamID), 4);  // 32-bit
    EXPECT_EQ(sizeof(PASID), 4);     // 32-bit (20-bit used)
    EXPECT_EQ(sizeof(IOVA), 8);      // 64-bit
    EXPECT_EQ(sizeof(IPA), 8);       // 64-bit
    EXPECT_EQ(sizeof(PA), 8);        // 64-bit
}

// Test enumeration values
TEST_F(TypesTest, EnumValues) {
    // Test AccessType enumeration
    AccessType readAccess = AccessType::Read;
    AccessType writeAccess = AccessType::Write;
    AccessType executeAccess = AccessType::Execute;
    
    EXPECT_NE(readAccess, writeAccess);
    EXPECT_NE(writeAccess, executeAccess);
    EXPECT_NE(readAccess, executeAccess);
}

// Test SecurityState enumeration
TEST_F(TypesTest, SecurityStateEnum) {
    SecurityState nonSecure = SecurityState::NonSecure;
    SecurityState secure = SecurityState::Secure;
    SecurityState realm = SecurityState::Realm;
    
    EXPECT_NE(nonSecure, secure);
    EXPECT_NE(secure, realm);
    EXPECT_NE(nonSecure, realm);
}

// Test TranslationStage enumeration
TEST_F(TypesTest, TranslationStageEnum) {
    TranslationStage stage1 = TranslationStage::Stage1Only;
    TranslationStage stage2 = TranslationStage::Stage2Only;
    TranslationStage both = TranslationStage::BothStages;
    TranslationStage disabled = TranslationStage::Disabled;
    
    EXPECT_NE(stage1, stage2);
    EXPECT_NE(stage1, both);
    EXPECT_NE(stage1, disabled);
}

// Test FaultType enumeration
TEST_F(TypesTest, FaultTypeEnum) {
    FaultType translation = FaultType::TranslationFault;
    FaultType permission = FaultType::PermissionFault;
    FaultType addressSize = FaultType::AddressSizeFault;
    FaultType access = FaultType::AccessFault;
    
    EXPECT_NE(translation, permission);
    EXPECT_NE(permission, addressSize);
    EXPECT_NE(addressSize, access);
}

// Test TranslationResult structure
TEST_F(TypesTest, TranslationResultStructure) {
    // Test default constructor (failure case)
    TranslationResult failureResult;
    EXPECT_TRUE(failureResult.isError());
    EXPECT_FALSE(failureResult.isOk());
    EXPECT_EQ(failureResult.getError(), SMMUError::InternalError);
    
    // Test success constructor using helper function
    PA testPA = 0x12345000;
    TranslationResult successResult = makeTranslationSuccess(testPA);
    EXPECT_TRUE(successResult.isOk());
    EXPECT_FALSE(successResult.isError());
    EXPECT_EQ(successResult.getValue().physicalAddress, testPA);
}

// Test PagePermissions structure
TEST_F(TypesTest, PagePermissions) {
    // Test default constructor
    PagePermissions defaultPerms;
    EXPECT_FALSE(defaultPerms.read);
    EXPECT_FALSE(defaultPerms.write);
    EXPECT_FALSE(defaultPerms.execute);
    
    // Test parameterized constructor
    PagePermissions rwxPerms(true, true, true);
    EXPECT_TRUE(rwxPerms.read);
    EXPECT_TRUE(rwxPerms.write);
    EXPECT_TRUE(rwxPerms.execute);
    
    // Test read-only permissions
    PagePermissions roPerms(true, false, false);
    EXPECT_TRUE(roPerms.read);
    EXPECT_FALSE(roPerms.write);
    EXPECT_FALSE(roPerms.execute);
}

// Test PageEntry structure
TEST_F(TypesTest, PageEntry) {
    // Test default constructor
    PageEntry defaultEntry;
    EXPECT_EQ(defaultEntry.physicalAddress, 0);
    EXPECT_FALSE(defaultEntry.valid);
    EXPECT_FALSE(defaultEntry.permissions.read);
    
    // Test parameterized constructor
    PA testPA = 0xDEADBEEF000;
    PagePermissions perms(true, true, false);
    PageEntry validEntry(testPA, perms);
    
    EXPECT_EQ(validEntry.physicalAddress, testPA);
    EXPECT_TRUE(validEntry.valid);
    EXPECT_TRUE(validEntry.permissions.read);
    EXPECT_TRUE(validEntry.permissions.write);
    EXPECT_FALSE(validEntry.permissions.execute);
}

// Test FaultRecord structure
TEST_F(TypesTest, FaultRecord) {
    FaultRecord defaultRecord;
    
    EXPECT_EQ(defaultRecord.streamID, 0);
    EXPECT_EQ(defaultRecord.pasid, 0);
    EXPECT_EQ(defaultRecord.address, 0);
    EXPECT_EQ(defaultRecord.faultType, FaultType::TranslationFault);
    EXPECT_EQ(defaultRecord.accessType, AccessType::Read);
    EXPECT_EQ(defaultRecord.timestamp, 0);
}

// Test configuration constants
TEST_F(TypesTest, ConfigurationConstants) {
    EXPECT_EQ(MAX_STREAM_ID, 0xFFFFFFFF);
    EXPECT_EQ(MAX_PASID, 0xFFFFF);  // 20-bit PASID space
    EXPECT_EQ(PAGE_SIZE, 4096);     // 4KB pages
    EXPECT_EQ(PAGE_MASK, 4095);     // PAGE_SIZE - 1
}

// Test page alignment utilities
TEST_F(TypesTest, PageAlignment) {
    // Test that PAGE_MASK can be used for alignment checks
    uint64_t alignedAddress = 0x12345000;
    uint64_t misalignedAddress = 0x12345678;
    
    EXPECT_EQ(alignedAddress & PAGE_MASK, 0);
    EXPECT_NE(misalignedAddress & PAGE_MASK, 0);
}

// Test address range validity
TEST_F(TypesTest, AddressRanges) {
    // Test IOVA (Input Output Virtual Address) range
    IOVA maxIOVA = 0xFFFFFFFFFFFFFFFF;
    IOVA minIOVA = 0x0;
    EXPECT_GE(maxIOVA, minIOVA);
    
    // Test PA (Physical Address) range
    PA maxPA = 0xFFFFFFFFFFFFFFFF;
    PA minPA = 0x0;
    EXPECT_GE(maxPA, minPA);
}

// Test PASID range validity
TEST_F(TypesTest, PASIDRange) {
    PASID validPASID = 0x12345;
    PASID maxValidPASID = MAX_PASID;
    PASID invalidPASID = MAX_PASID + 1;
    
    EXPECT_LE(validPASID, MAX_PASID);
    EXPECT_EQ(maxValidPASID, MAX_PASID);
    EXPECT_GT(invalidPASID, MAX_PASID);
}

} // namespace test
} // namespace smmu