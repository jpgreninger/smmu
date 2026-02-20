// ARM SMMU v3 SMMU Controller Advanced Feature Tests (Simplified)
// Copyright (c) 2024 John Greninger
//
// This test suite provides comprehensive coverage of advanced SMMU controller features
// Targets lines 636-740, 853-892 in smmu.cpp
//
// Coverage Targets:
// - performTwoStageTranslation error paths and edge cases
// - Address size validation (32-bit, 36-bit, 40-bit, 44-bit, 48-bit, 52-bit)
// - Large address handling
// - Stream configuration edge cases
// - Permission validation across stages
// - Complex translation scenarios

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

class SMMUControllerAdvancedTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();

        // Configure basic stream
        StreamConfig config;
        config.translationEnabled = true;
        config.stage1Enabled = true;
        config.stage2Enabled = false;

        smmu->configureStream(TEST_STREAM_ID, config);
        smmu->enableStream(TEST_STREAM_ID);
    }

    void TearDown() override {
        smmu.reset();
    }

    std::unique_ptr<SMMU> smmu;

    // Test constants
    static constexpr StreamID TEST_STREAM_ID = 1;
    static constexpr StreamID TEST_STREAM_ID_2 = 2;
    static constexpr PASID TEST_PASID = 0x1;
    static constexpr PASID HYPERVISOR_PASID = 0x0;
    static constexpr IOVA TEST_IOVA = 0x10000000;
    static constexpr IPA TEST_IPA = 0x20000000;
    static constexpr PA TEST_PA = 0x40000000;
};

// ========== performTwoStageTranslation Error Path Tests ==========

TEST_F(SMMUControllerAdvancedTest, PerformTwoStageTranslation_UnconfiguredStream_Error) {
    // Target: Lines 652-666 - Null stream context defensive check
    TranslationResult result = smmu->translate(999, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::StreamNotConfigured);
}

TEST_F(SMMUControllerAdvancedTest, PerformTwoStageTranslation_TranslationDisabled_BypassMode) {
    // Target: Lines 674-679 - Translation disabled bypass mode
    // ARM §3.11: remove existing stream before reconfiguring
    smmu->removeStream(TEST_STREAM_ID);
    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;

    smmu->configureStream(TEST_STREAM_ID, config);

    // Translation should bypass and return IOVA as PA
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        TranslationData data = result.getValue();
        EXPECT_EQ(data.physicalAddress, TEST_IOVA);  // Bypass mode
    }
}

TEST_F(SMMUControllerAdvancedTest, PerformTwoStageTranslation_NoStagesEnabled_ConfigurationError) {
    // Target: Lines 690-704 - No stages enabled but translation enabled
    // ARM §3.11: remove existing stream before reconfiguring
    smmu->removeStream(TEST_STREAM_ID);
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = false;

    // Configuration should be rejected due to translation enabled but no stages enabled
    VoidResult configResult = smmu->configureStream(TEST_STREAM_ID, config);

    EXPECT_TRUE(configResult.isError());
    EXPECT_EQ(configResult.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(SMMUControllerAdvancedTest, PerformTwoStageTranslation_PermissionViolation_PermissionFault) {
    // Target: Lines 726-741 - Access permission validation failure
    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    // Map with read-only permissions
    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, TEST_PA, perms, SecurityState::NonSecure);

    // Try to write to read-only page
    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA, AccessType::Write);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation);
}

// ========== Address Size Validation Tests ==========

TEST_F(SMMUControllerAdvancedTest, AddressSize_32Bit_ValidAddress) {
    // Test 32-bit address handling
    IOVA iova32 = 0xFFFFFFFF;  // Max 32-bit address
    PA pa32 = 0xF0000000;

    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iova32, pa32, perms, SecurityState::NonSecure);

    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, iova32, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA gets page-aligned during mapping, then IOVA's page offset is added during translation
        PA expectedPA = (pa32 & ~PAGE_MASK) + (iova32 & PAGE_MASK);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
}

TEST_F(SMMUControllerAdvancedTest, AddressSize_36Bit_ValidAddress) {
    // Test 36-bit address handling
    IOVA iova36 = 0x0000000FFFFFFFFF;  // Max 36-bit address
    PA pa36 = 0x0000000F00000000;

    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iova36, pa36, perms, SecurityState::NonSecure);

    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, iova36, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA gets page-aligned during mapping, then IOVA's page offset is added during translation
        PA expectedPA = (pa36 & ~PAGE_MASK) + (iova36 & PAGE_MASK);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
}

TEST_F(SMMUControllerAdvancedTest, AddressSize_40Bit_ValidAddress) {
    // Test 40-bit address handling
    IOVA iova40 = 0x000000FFFFFFFFFF;  // Max 40-bit address
    PA pa40 = 0x000000FF00000000;

    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iova40, pa40, perms, SecurityState::NonSecure);

    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, iova40, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA gets page-aligned during mapping, then IOVA's page offset is added during translation
        PA expectedPA = (pa40 & ~PAGE_MASK) + (iova40 & PAGE_MASK);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
}

TEST_F(SMMUControllerAdvancedTest, AddressSize_44Bit_ValidAddress) {
    // Test 44-bit address handling
    IOVA iova44 = 0x00000FFFFFFFFFFF;  // Max 44-bit address
    PA pa44 = 0x00000FFF00000000;

    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iova44, pa44, perms, SecurityState::NonSecure);

    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, iova44, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA gets page-aligned during mapping, then IOVA's page offset is added during translation
        PA expectedPA = (pa44 & ~PAGE_MASK) + (iova44 & PAGE_MASK);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
}

TEST_F(SMMUControllerAdvancedTest, AddressSize_48Bit_ValidAddress) {
    // Test 48-bit address handling (most common)
    IOVA iova48 = 0x0000FFFFFFFFFFFF;  // Max 48-bit address
    PA pa48 = 0x0000FFFF00000000;

    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iova48, pa48, perms, SecurityState::NonSecure);

    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, iova48, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA gets page-aligned during mapping, then IOVA's page offset is added during translation
        PA expectedPA = (pa48 & ~PAGE_MASK) + (iova48 & PAGE_MASK);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
}

TEST_F(SMMUControllerAdvancedTest, AddressSize_52Bit_ValidAddress) {
    // Test 52-bit address handling (max ARM SMMU v3)
    IOVA iova52 = 0x000FFFFFFFFFFFFF;  // Max 52-bit address
    PA pa52 = 0x000FFFFF00000000;

    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, false, false);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iova52, pa52, perms, SecurityState::NonSecure);

    TranslationResult result = smmu->translate(TEST_STREAM_ID, TEST_PASID, iova52, AccessType::Read);

    EXPECT_TRUE(result.isOk());
    if (result.isOk()) {
        // PA gets page-aligned during mapping, then IOVA's page offset is added during translation
        PA expectedPA = (pa52 & ~PAGE_MASK) + (iova52 & PAGE_MASK);
        EXPECT_EQ(result.getValue().physicalAddress, expectedPA);
    }
}

// ========== Complex Translation Scenarios ==========

TEST_F(SMMUControllerAdvancedTest, ComplexScenario_MultipleStreamsAndPASIDs) {
    // Test multiple streams with multiple PASIDs
    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;

    smmu->configureStream(TEST_STREAM_ID, config);
    smmu->enableStream(TEST_STREAM_ID);
    smmu->configureStream(TEST_STREAM_ID_2, config);
    smmu->enableStream(TEST_STREAM_ID_2);

    PagePermissions perms(true, true, false);

    // Map different translations for each stream/PASID combination
    smmu->createStreamPASID(TEST_STREAM_ID, 1);
    smmu->mapPage(TEST_STREAM_ID, 1, TEST_IOVA, 0x40000000, perms, SecurityState::NonSecure);

    smmu->createStreamPASID(TEST_STREAM_ID, 2);
    smmu->mapPage(TEST_STREAM_ID, 2, TEST_IOVA, 0x50000000, perms, SecurityState::NonSecure);

    smmu->createStreamPASID(TEST_STREAM_ID_2, 1);
    smmu->mapPage(TEST_STREAM_ID_2, 1, TEST_IOVA, 0x60000000, perms, SecurityState::NonSecure);

    // Verify each stream/PASID combination translates correctly
    TranslationResult result1 = smmu->translate(TEST_STREAM_ID, 1, TEST_IOVA, AccessType::Read);
    TranslationResult result2 = smmu->translate(TEST_STREAM_ID, 2, TEST_IOVA, AccessType::Read);
    TranslationResult result3 = smmu->translate(TEST_STREAM_ID_2, 1, TEST_IOVA, AccessType::Read);

    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());
    EXPECT_TRUE(result3.isOk());

    if (result1.isOk() && result2.isOk() && result3.isOk()) {
        EXPECT_EQ(result1.getValue().physicalAddress, 0x40000000);
        EXPECT_EQ(result2.getValue().physicalAddress, 0x50000000);
        EXPECT_EQ(result3.getValue().physicalAddress, 0x60000000);
    }
}

TEST_F(SMMUControllerAdvancedTest, ComplexScenario_AllAccessTypes) {
    // Test all access types (Read, Write, Execute)
    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    // Map different pages with different permissions
    IOVA iovaRead = 0x10000000;
    IOVA iovaWrite = 0x20000000;
    IOVA iovaExecute = 0x30000000;
    IOVA iovaAll = 0x40000000;

    PagePermissions permsRead(true, false, false);
    PagePermissions permsWrite(false, true, false);
    PagePermissions permsExecute(false, false, true);
    PagePermissions permsAll(true, true, true);

    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaRead, 0x10000000, permsRead, SecurityState::NonSecure);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaWrite, 0x20000000, permsWrite, SecurityState::NonSecure);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaExecute, 0x30000000, permsExecute, SecurityState::NonSecure);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaAll, 0x40000000, permsAll, SecurityState::NonSecure);

    // Test read access
    TranslationResult resultRead = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaRead, AccessType::Read);
    EXPECT_TRUE(resultRead.isOk());

    // Test write access (should fail on read-only page)
    TranslationResult resultWriteFail = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaRead, AccessType::Write);
    EXPECT_TRUE(resultWriteFail.isError());

    // Test execute access
    TranslationResult resultExecute = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaExecute, AccessType::Execute);
    EXPECT_TRUE(resultExecute.isOk());

    // Test all permissions page
    TranslationResult resultAllRead = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaAll, AccessType::Read);
    TranslationResult resultAllWrite = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaAll, AccessType::Write);
    TranslationResult resultAllExecute = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaAll, AccessType::Execute);

    EXPECT_TRUE(resultAllRead.isOk());
    EXPECT_TRUE(resultAllWrite.isOk());
    EXPECT_TRUE(resultAllExecute.isOk());
}

TEST_F(SMMUControllerAdvancedTest, ComplexScenario_AllSecurityStates) {
    // Test all security states (NonSecure, Secure, Realm)
    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    IOVA iovaNonSecure = 0x10000000;
    IOVA iovaSecure = 0x20000000;
    IOVA iovaRealm = 0x30000000;

    PagePermissions perms(true, false, false);

    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaNonSecure, 0x40000000, perms, SecurityState::NonSecure);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaSecure, 0x50000000, perms, SecurityState::Secure);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, iovaRealm, 0x60000000, perms, SecurityState::Realm);

    // Test each security state
    TranslationResult resultNonSecure = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaNonSecure, AccessType::Read, SecurityState::NonSecure);
    TranslationResult resultSecure = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaSecure, AccessType::Read, SecurityState::Secure);
    TranslationResult resultRealm = smmu->translate(TEST_STREAM_ID, TEST_PASID, iovaRealm, AccessType::Read, SecurityState::Realm);

    EXPECT_TRUE(resultNonSecure.isOk());
    EXPECT_TRUE(resultSecure.isOk());
    EXPECT_TRUE(resultRealm.isOk());

    if (resultNonSecure.isOk() && resultSecure.isOk() && resultRealm.isOk()) {
        EXPECT_EQ(resultNonSecure.getValue().securityState, SecurityState::NonSecure);
        EXPECT_EQ(resultSecure.getValue().securityState, SecurityState::Secure);
        EXPECT_EQ(resultRealm.getValue().securityState, SecurityState::Realm);
    }
}

TEST_F(SMMUControllerAdvancedTest, ComplexScenario_BoundaryAddresses) {
    // Test boundary addresses (0, page boundaries, max addresses)
    smmu->createStreamPASID(TEST_STREAM_ID, TEST_PASID);

    PagePermissions perms(true, true, false);

    // Test zero address mapping
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, 0x0, 0x80000000, perms, SecurityState::NonSecure);

    // Test page-aligned addresses
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, PAGE_SIZE, 0x81000000, perms, SecurityState::NonSecure);
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, PAGE_SIZE * 2, 0x82000000, perms, SecurityState::NonSecure);

    // Test large addresses
    smmu->mapPage(TEST_STREAM_ID, TEST_PASID, 0x0000FFFF00000000, 0x000FFFFF00000000, perms, SecurityState::NonSecure);

    TranslationResult result0 = smmu->translate(TEST_STREAM_ID, TEST_PASID, 0x0, AccessType::Read);
    TranslationResult result1 = smmu->translate(TEST_STREAM_ID, TEST_PASID, PAGE_SIZE, AccessType::Read);
    TranslationResult result2 = smmu->translate(TEST_STREAM_ID, TEST_PASID, 0x0000FFFF00000000, AccessType::Read);

    EXPECT_TRUE(result0.isOk());
    EXPECT_TRUE(result1.isOk());
    EXPECT_TRUE(result2.isOk());

    if (result0.isOk() && result1.isOk() && result2.isOk()) {
        EXPECT_EQ(result0.getValue().physicalAddress, 0x80000000);
        EXPECT_EQ(result1.getValue().physicalAddress, 0x81000000);
        EXPECT_EQ(result2.getValue().physicalAddress, 0x000FFFFF00000000);
    }
}

} // namespace test
} // namespace smmu
