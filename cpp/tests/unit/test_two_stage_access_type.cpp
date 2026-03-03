// ARM SMMU v3 BUG-CPP-1: Stage-2 access type in two-stage translation
//
// Spec: ARM IHI0070G.b §3.3.2, §3.12.1, §3.13.5, Ch.15, §16.5
//
// Root cause:
//   performBothStagesTranslation() in smmu.cpp called
//   stage2AddressSpace->translatePage(intermediatePA, accessType, securityState)
//   using the incoming accessType for the Stage-2 IPA->PA lookup.  Per the ARM
//   spec, the Stage-2 page-table walk itself must use AccessType::Read; the
//   real accessType is applied only at the Stage-1 x Stage-2 permission
//   intersection step.
//
//   When Stage-2 maps an IPA as read-only, a WRITE access would spuriously
//   fail at the Stage-2 translatePage() call (PermissionViolation) rather
//   than at the correct intersection step.  A READ access to such an IPA
//   previously worked but only by coincidence.
//
// Fix (smmu.cpp performBothStagesTranslation, line ~1247):
//   Change:  stage2AddressSpace->translatePage(intermediatePA, accessType, securityState)
//   To:      stage2AddressSpace->translatePage(intermediatePA, AccessType::Read, securityState)
//   Then check the real accessType against the intersected permissions (same
//   pattern as StreamContext::translateUnlocked BUG-NEW2-05 fix).
//
// Test structure (TDD — tests written BEFORE the fix):
//   Test 1: S2=read-only, READ  -> must succeed (PA returned).
//   Test 2: S2=read-only, WRITE -> must fail with PermissionViolation.
//   Test 3: S2=read-write, READ  -> must succeed.
//   Test 4: S2=read-write, WRITE -> must succeed.
//   Test 5: Regression — S1=rw, S2=ro, READ succeeds, proves Stage-2 lookup
//           uses Read (not Write) access internally.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================
// Test fixture
// ============================================================

class TwoStageAccessTypeTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::unique_ptr<SMMU>(new SMMU());
        smmu_->enable();
    }

    void TearDown() override {
        smmu_.reset();
    }

    // Configure a two-stage stream with:
    //   Stage-1 AS: iova -> ipa  (with s1Perms)
    //   Stage-2 AS: ipa  -> pa   (with s2Perms)
    void configureTwoStage(StreamID sid, PASID pasid,
                           IOVA iova, IPA ipa, PA pa,
                           PagePermissions s1Perms,
                           PagePermissions s2Perms)
    {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.securityState      = SecurityState::NonSecure;

        ASSERT_TRUE(smmu_->configureStream(sid, cfg).isOk())
            << "configureStream failed";
        ASSERT_TRUE(smmu_->enableStream(sid).isOk())
            << "enableStream failed";
        ASSERT_TRUE(smmu_->createStreamPASID(sid, pasid).isOk())
            << "createStreamPASID failed";

        // Stage-1: iova -> ipa
        ASSERT_TRUE(smmu_->mapPage(sid, pasid, iova, ipa, s1Perms).isOk())
            << "mapPage (Stage-1) failed";

        // Stage-2: ipa -> pa
        auto s2AS = std::make_shared<AddressSpace>();
        s2AS->mapPage(ipa, pa, s2Perms);
        ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(sid, s2AS).isOk())
            << "setStreamStage2AddressSpace failed";
    }

    std::unique_ptr<SMMU> smmu_;

    static constexpr StreamID SID   = 0x20;
    static constexpr PASID    PASID_ = 0;
};

// ============================================================
// BUG-CPP-1 Test 1:
// Stage-2 maps final IPA as read-only.
// READ must SUCCEED — Stage-2 lookup uses Read semantics -> no fault.
// ============================================================

TEST_F(TwoStageAccessTypeTest, Stage2ReadOnly_ReadSucceeds) {
    static constexpr IOVA iova = 0x0001'0000ULL;
    static constexpr IPA  ipa  = 0x8000'0000ULL;
    static constexpr PA   pa   = 0xC000'0000ULL;

    PagePermissions s1Perms;
    s1Perms.read  = true;
    s1Perms.write = true;

    PagePermissions s2Perms;
    s2Perms.read  = true;
    s2Perms.write = false; // read-only in Stage-2

    configureTwoStage(SID, PASID_, iova, ipa, pa, s1Perms, s2Perms);

    TranslationResult result = smmu_->translate(
        SID, PASID_, iova, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-CPP-1: READ to read-only Stage-2 IPA must succeed. "
           "Stage-2 lookup must use AccessType::Read, not the incoming accessType.";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, pa)
            << "Translated PA must equal mapped PA";
    }
}

// ============================================================
// BUG-CPP-1 Test 2:
// Stage-2 maps final IPA as read-only.
// WRITE must FAIL with PermissionViolation (via intersection, not spurious S2 fault).
// ============================================================

TEST_F(TwoStageAccessTypeTest, Stage2ReadOnly_WriteFails) {
    static constexpr IOVA iova = 0x0002'0000ULL;
    static constexpr IPA  ipa  = 0x8001'0000ULL;
    static constexpr PA   pa   = 0xC001'0000ULL;

    PagePermissions s1Perms;
    s1Perms.read  = true;
    s1Perms.write = true;

    PagePermissions s2Perms;
    s2Perms.read  = true;
    s2Perms.write = false; // read-only in Stage-2

    configureTwoStage(SID, PASID_, iova, ipa, pa, s1Perms, s2Perms);

    TranslationResult result = smmu_->translate(
        SID, PASID_, iova, AccessType::Write, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "WRITE to read-only Stage-2 IPA must fail";
    if (result.isError()) {
        EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
            << "Error must be PagePermissionViolation via intersection, not a spurious Stage-2 access error";
    }
}

// ============================================================
// BUG-CPP-1 Test 3:
// Stage-2 maps final IPA as read-write.
// READ must succeed.
// ============================================================

TEST_F(TwoStageAccessTypeTest, Stage2ReadWrite_ReadSucceeds) {
    static constexpr IOVA iova = 0x0003'0000ULL;
    static constexpr IPA  ipa  = 0x8002'0000ULL;
    static constexpr PA   pa   = 0xC002'0000ULL;

    PagePermissions s1Perms;
    s1Perms.read  = true;
    s1Perms.write = true;

    PagePermissions s2Perms;
    s2Perms.read  = true;
    s2Perms.write = true;

    configureTwoStage(SID, PASID_, iova, ipa, pa, s1Perms, s2Perms);

    TranslationResult result = smmu_->translate(
        SID, PASID_, iova, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "READ to read-write Stage-2 IPA must succeed";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, pa);
    }
}

// ============================================================
// BUG-CPP-1 Test 4:
// Stage-2 maps final IPA as read-write.
// WRITE must succeed.
// ============================================================

TEST_F(TwoStageAccessTypeTest, Stage2ReadWrite_WriteSucceeds) {
    static constexpr IOVA iova = 0x0004'0000ULL;
    static constexpr IPA  ipa  = 0x8003'0000ULL;
    static constexpr PA   pa   = 0xC003'0000ULL;

    PagePermissions s1Perms;
    s1Perms.read  = true;
    s1Perms.write = true;

    PagePermissions s2Perms;
    s2Perms.read  = true;
    s2Perms.write = true;

    configureTwoStage(SID, PASID_, iova, ipa, pa, s1Perms, s2Perms);

    TranslationResult result = smmu_->translate(
        SID, PASID_, iova, AccessType::Write, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "WRITE to read-write Stage-2 IPA must succeed";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, pa);
    }
}

// ============================================================
// BUG-CPP-1 Test 5 (regression):
// Stage-2 read-only IPA: READ succeeds and returns correct PA.
// This test was spuriously failing before the fix because the Stage-2 lookup
// used AccessType::Write (the incoming access type) instead of AccessType::Read.
// ============================================================

TEST_F(TwoStageAccessTypeTest, Stage2ReadOnly_Regression_ReadCorrectPA) {
    static constexpr IOVA iova = 0x0005'0000ULL;
    static constexpr IPA  ipa  = 0x8004'0000ULL;
    static constexpr PA   pa   = 0xC004'0000ULL;

    PagePermissions s1Perms;
    s1Perms.read  = true;
    s1Perms.write = true; // S1 fully permissive

    PagePermissions s2Perms;
    s2Perms.read  = true;
    s2Perms.write = false; // S2 read-only

    configureTwoStage(SID, PASID_, iova, ipa, pa, s1Perms, s2Perms);

    // READ: intersection = (s1.read=true && s2.read=true) = true -> permitted.
    // Stage-2 lookup itself must use Read access so no spurious PermissionFault.
    TranslationResult r = smmu_->translate(
        SID, PASID_, iova, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-1 regression: READ with S1=rw, S2=ro must succeed. "
           "Before the fix, performBothStagesTranslation used the incoming "
           "accessType for the Stage-2 translatePage() call, causing a spurious "
           "PermissionFault when a write access was made to any read-only Stage-2 IPA.";
    if (r.isOk()) {
        EXPECT_EQ(r.getValue().physicalAddress, pa)
            << "PA must be correct after two-stage translation";
        EXPECT_TRUE(r.getValue().permissions.read)
            << "Intersected permissions must allow read";
        EXPECT_FALSE(r.getValue().permissions.write)
            << "Intersected permissions must deny write (S2 is read-only)";
    }
}

}  // namespace test
}  // namespace smmu
