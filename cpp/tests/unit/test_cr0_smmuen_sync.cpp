// ARM SMMU v3 BUG-CPP-3: smmuen_ / cr0_ partial sync
//
// Spec: ARM IHI0070G.b §6.3.9, §6.3.9.6
//
// Root cause:
//   The spec defines a single authoritative bit: SMMU_CR0.SMMUEN (bit 0 of
//   cr0_).  The implementation maintains a shadow bool smmuen_ alongside cr0_,
//   so enable()/disable() must set BOTH atomically.  isEnabled() returns
//   smmuen_ rather than (cr0_ & CR0_SMMUEN), creating a potential split-brain:
//   if setCR0() is used to clear or set SMMUEN without going through enable()/
//   disable(), smmuen_ diverges from cr0_ and isEnabled() returns the wrong
//   value.
//
//   Concretely:
//     setCR0(0)          clears cr0_ bit-0, but smmuen_ stays true if enable()
//                        was called before.  isEnabled() returns true (wrong).
//     setCR0(CR0_SMMUEN) sets  cr0_ bit-0, but smmuen_ stays false if enable()
//                        was not called.  isEnabled() returns false (wrong).
//
// Fix:
//   Make isEnabled() authoritative from cr0_:
//       return (cr0_ & CR0_SMMUEN) != 0;
//   The redundant smmuen_ can be kept for backwards-compat (enable()/disable()
//   still set it) but isEnabled() must not rely on it.
//
// Tests (written BEFORE the fix):
//   1. setCR0(0) after enable()  -> isEnabled() returns false.
//   2. setCR0(CR0_SMMUEN)        -> isEnabled() returns true.
//   3. enable() + setCR0(0)      -> isEnabled() returns false.
//   4. disable() + setCR0(CR0_SMMUEN) -> isEnabled() returns true.
//   5. translate() uses cr0_ (not smmuen_): after setCR0(0), translation
//      bypasses (SMMUEN=0 bypass path); after setCR0(CR0_SMMUEN), translation
//      goes through the normal path (SMMUEN=1).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================
// Test fixture
// ============================================================

class Cr0SmmuentSyncTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::unique_ptr<SMMU>(new SMMU());
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;

    static constexpr StreamID ANY_SID  = 0xAA;
    static constexpr PASID    ANY_PASID = 0;
    static constexpr IOVA     ANY_IOVA  = 0x1000ULL;
};

// ============================================================
// BUG-CPP-3 Test 1:
// After enable() sets smmuen_=true and cr0_|=CR0_SMMUEN,
// calling setCR0(0) must clear cr0_ bit-0 => isEnabled() returns false.
// (Pre-fix: isEnabled() returned smmuen_=true, wrong.)
// ============================================================

TEST_F(Cr0SmmuentSyncTest, SetCR0_Zero_AfterEnable_IsDisabled) {
    smmu_->enable();
    ASSERT_TRUE(smmu_->isEnabled())
        << "SMMU must be enabled after enable()";

    // Write CR0=0: clears SMMUEN bit.
    smmu_->setCR0(0u);

    EXPECT_FALSE(smmu_->isEnabled())
        << "BUG-CPP-3: isEnabled() must return false after setCR0(0). "
           "The implementation must read from cr0_, not the shadow smmuen_.";
}

// ============================================================
// BUG-CPP-3 Test 2:
// getCR0() after setCR0(0) must return 0 (sanity check that
// setCR0 updates cr0_ correctly).
// ============================================================

TEST_F(Cr0SmmuentSyncTest, GetCR0_AfterSetCR0_ReturnsCorrectValue) {
    smmu_->enable();
    ASSERT_NE(smmu_->getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "CR0.SMMUEN must be 1 after enable()";

    smmu_->setCR0(0u);
    EXPECT_EQ(smmu_->getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "getCR0() must reflect the value written via setCR0()";
}

// ============================================================
// BUG-CPP-3 Test 3:
// Without calling enable(), use setCR0(CR0_SMMUEN) to set the bit.
// isEnabled() must return true.
// (Pre-fix: isEnabled() returned smmuen_=false, wrong.)
// ============================================================

TEST_F(Cr0SmmuentSyncTest, SetCR0_WithSMMUEN_IsEnabled) {
    // SMMU starts disabled.
    ASSERT_FALSE(smmu_->isEnabled())
        << "SMMU must start disabled";

    smmu_->setCR0(SMMU::CR0_SMMUEN);

    EXPECT_TRUE(smmu_->isEnabled())
        << "BUG-CPP-3: isEnabled() must return true after setCR0(CR0_SMMUEN). "
           "The implementation must read from cr0_, not the shadow smmuen_.";
}

// ============================================================
// BUG-CPP-3 Test 4:
// disable() + setCR0(CR0_SMMUEN): isEnabled() must return true.
// ============================================================

TEST_F(Cr0SmmuentSyncTest, Disable_ThenSetCR0_SMMUEN_IsEnabled) {
    smmu_->enable();
    smmu_->disable();
    ASSERT_FALSE(smmu_->isEnabled());

    smmu_->setCR0(SMMU::CR0_SMMUEN);
    EXPECT_TRUE(smmu_->isEnabled())
        << "isEnabled() must return true after setCR0(CR0_SMMUEN), "
           "even without calling enable()";
}

// ============================================================
// BUG-CPP-3 Test 5:
// Verify translation path uses cr0_, not smmuen_:
// After setCR0(0) (clears SMMUEN), translate() must BYPASS (GBPA.ABORT=0
// => identity mapping PA==IOVA), not go through the stream lookup path.
// ============================================================

TEST_F(Cr0SmmuentSyncTest, SetCR0_Zero_TranslationBypasses) {
    smmu_->enable();
    smmu_->setCR0(0u); // clear SMMUEN via cr0 path

    // GBPA.ABORT defaults to false => bypass (identity mapping).
    TranslationResult result = smmu_->translate(
        ANY_SID, ANY_PASID, ANY_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-CPP-3: With cr0_.SMMUEN=0, translate() must bypass (return success "
           "with PA==IOVA), not go through the stream lookup path.";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, static_cast<PA>(ANY_IOVA))
            << "Bypass must produce identity mapping (PA==IOVA)";
    }
}

// ============================================================
// BUG-CPP-3 Test 6:
// After setCR0(CR0_SMMUEN) (sets SMMUEN without calling enable()),
// translate() must go through the normal path:
// unconfigured stream => StreamNotConfigured error.
// ============================================================

TEST_F(Cr0SmmuentSyncTest, SetCR0_SMMUEN_TranslationGoesNormalPath) {
    smmu_->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // ANY_SID is not configured => normal path returns StreamNotConfigured.
    TranslationResult result = smmu_->translate(
        ANY_SID, ANY_PASID, ANY_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "BUG-CPP-3: With cr0_.SMMUEN=1, translate() must use the normal "
           "stream path, not bypass.";
    if (result.isError()) {
        // In-range unconfigured stream → C_BAD_STE → StreamNotConfigured (§7.3.5).
        EXPECT_EQ(result.getError(), SMMUError::StreamNotConfigured)
            << "In-range unconfigured stream must return StreamNotConfigured (§7.3.5 C_BAD_STE)";
    }
}

// ============================================================
// BUG-CPP-3 Test 7:
// Multiple enable/disable cycles using only setCR0() (no enable()/disable())
// must keep isEnabled() consistent with the CR0.SMMUEN bit.
// ============================================================

TEST_F(Cr0SmmuentSyncTest, MultipleCR0Toggles_IsEnabledConsistent) {
    for (int cycle = 0; cycle < 4; ++cycle) {
        smmu_->setCR0(SMMU::CR0_SMMUEN);
        EXPECT_TRUE(smmu_->isEnabled())
            << "cycle " << cycle << ": isEnabled() must be true after setCR0(CR0_SMMUEN)";

        smmu_->setCR0(0u);
        EXPECT_FALSE(smmu_->isEnabled())
            << "cycle " << cycle << ": isEnabled() must be false after setCR0(0)";
    }
}

}  // namespace test
}  // namespace smmu
