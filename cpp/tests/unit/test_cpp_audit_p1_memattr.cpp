// TDD failing test for BUG-13.1.5-CPP: Two-stage translation discards S1 memory attributes.
//
// ARM §13.1.5: When both Stage-1 and Stage-2 translations are active, the final
// memory attribute must be the combination (strength ordering) of S1 and S2 attributes:
//   - Device memory (pageAttr=0x00) is the strongest restriction — it wins over any Normal.
//   - Normal WB/WA (pageAttr=0xFF) is the weakest.
//
// BUG: smmu.cpp performTwoStageTranslation() line 2724 sets:
//     twoStageResult.pageAttr = stage2Data.pageAttr;
// This discards S1's pageAttr entirely. When S1 is Device and S2 is Normal,
// the result should be Device (0x00), but the code returns Normal (0xFF).
//
// These tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/address_space.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID SID_S1DEV_S2NRM  = 0xD0u;
static constexpr StreamID SID_S1NRM_S2DEV  = 0xD1u;
static constexpr StreamID SID_S1NRM_S2NRM  = 0xD2u;

static constexpr IOVA    BASE_IOVA   = 0x10000u;
static constexpr PA      STAGE1_PA   = 0x20000u; // IPA (S1 output = S2 input)
static constexpr PA      FINAL_PA    = 0x30000u; // PA  (S2 output)

// Helper: configure a two-stage stream
static bool configureTwoStageStream(SMMU& smmu, StreamID sid)
{
    smmu.enable();
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    cfg.s2ps               = 0;
    if (!smmu.configureStream(sid, cfg).isOk()) return false;
    if (!smmu.enableStream(sid).isOk()) return false;
    if (!smmu.createStreamPASID(sid, 0u).isOk()) return false;
    return true;
}

// ── BUG-13.1.5-CPP Test 1 ───────────────────────────────────────────────────
// S1 maps IOVA→IPA with Device memory (pageAttr=0x00)
// S2 maps IPA→PA  with Normal memory  (pageAttr=0xFF)
// Combined output must be Device (0x00) — Device wins per §13.1.5 strength ordering.
//
// Before fix: result.pageAttr == 0xFF (S2 Normal wins, S1 Device discarded)
// After  fix: result.pageAttr == 0x00 (Device from S1 wins)
TEST(BugAudit135CppTest, S1Device_S2Normal_CombinedIsDevice)
{
    SMMU smmu;
    ASSERT_TRUE(configureTwoStageStream(smmu, SID_S1DEV_S2NRM));

    // Stage-1: map IOVA→IPA as Device memory (pageAttr=0x00)
    PagePermissions rw(true, true, false);
    VoidResult r1 = smmu.mapPageDevice(SID_S1DEV_S2NRM, 0u, BASE_IOVA,
                                       static_cast<PA>(STAGE1_PA), rw);
    ASSERT_TRUE(r1.isOk()) << "S1 mapPageDevice failed";

    // Stage-2: map IPA→PA as Normal memory via setStreamStage2AddressSpace
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(s2as->mapPage(STAGE1_PA, FINAL_PA, rw).isOk()) << "S2 AS mapPage failed";
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(SID_S1DEV_S2NRM, s2as).isOk());

    TranslationResult result = smmu.translate(SID_S1DEV_S2NRM, 0u, BASE_IOVA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Translation must succeed";

    // Combined pageAttr must be 0x00 (Device) since S1 is Device
    EXPECT_EQ(result.getValue().pageAttr, 0x00u)
        << "BUG-13.1.5-CPP: S1=Device(0x00) + S2=Normal(0xFF) must combine to Device(0x00). "
           "Before fix: pageAttr=" << static_cast<int>(result.getValue().pageAttr)
        << " (S2 Normal wins because S1 pageAttr is discarded).";
}

// ── BUG-13.1.5-CPP Test 2 ───────────────────────────────────────────────────
// S1 Normal + S2 Device: combined must be Device (0x00)
// Uses mapStage2DevicePage to set stage-2 Device.
// This test likely passes before fix (S2 Device wins by luck), but confirms symmetry.
TEST(BugAudit135CppTest, S1Normal_S2Device_CombinedIsDevice)
{
    SMMU smmu;
    ASSERT_TRUE(configureTwoStageStream(smmu, SID_S1NRM_S2DEV));

    // Stage-1: map IOVA→IPA as Normal memory
    PagePermissions rw(true, true, false);
    VoidResult r1 = smmu.mapPage(SID_S1NRM_S2DEV, 0u, BASE_IOVA,
                                 static_cast<PA>(STAGE1_PA), rw);
    ASSERT_TRUE(r1.isOk()) << "S1 mapPage failed";

    // Stage-2: map IPA→PA as Device memory using mapStage2DevicePage
    VoidResult r2 = smmu.mapStage2DevicePage(SID_S1NRM_S2DEV, STAGE1_PA, FINAL_PA, rw);
    ASSERT_TRUE(r2.isOk()) << "S2 mapStage2DevicePage failed";

    TranslationResult result = smmu.translate(SID_S1NRM_S2DEV, 0u, BASE_IOVA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Translation must succeed";

    EXPECT_EQ(result.getValue().pageAttr, 0x00u)
        << "BUG-13.1.5-CPP: S1=Normal(0xFF) + S2=Device(0x00) must combine to Device(0x00).";
}

// ── BUG-13.1.5-CPP Test 3 ───────────────────────────────────────────────────
// S1 Normal + S2 Normal = Normal (0xFF) — sanity check, must pass before and after fix.
TEST(BugAudit135CppTest, S1Normal_S2Normal_CombinedIsNormal)
{
    SMMU smmu;
    ASSERT_TRUE(configureTwoStageStream(smmu, SID_S1NRM_S2NRM));

    PagePermissions rw(true, true, false);
    VoidResult r1 = smmu.mapPage(SID_S1NRM_S2NRM, 0u, BASE_IOVA,
                                 static_cast<PA>(STAGE1_PA), rw);
    ASSERT_TRUE(r1.isOk()) << "S1 mapPage failed";

    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(s2as->mapPage(STAGE1_PA, FINAL_PA, rw).isOk()) << "S2 AS mapPage failed";
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(SID_S1NRM_S2NRM, s2as).isOk());

    TranslationResult result = smmu.translate(SID_S1NRM_S2NRM, 0u, BASE_IOVA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Translation must succeed";

    EXPECT_EQ(result.getValue().pageAttr, 0xFFu)
        << "S1=Normal(0xFF) + S2=Normal(0xFF) must combine to Normal(0xFF).";
}

}  // namespace test
}  // namespace smmu
