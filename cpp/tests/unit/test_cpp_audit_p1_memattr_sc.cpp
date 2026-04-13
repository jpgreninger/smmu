// TDD failing test for BUG-13.1.5-CPP (stream_context.cpp path):
// StreamContext::translateUnlocked() two-stage path discards S1 memory attribute.
//
// ARM §13.1.5: When both Stage-1 and Stage-2 translations are active, the final
// memory attribute must be the combination (strength ordering) of S1 and S2 attributes:
//   - Device memory (pageAttr=0x00) is the strongest restriction — it wins over Normal.
//   - Normal WB/WA (pageAttr=0xFF) is the weakest.
//
// BUG: stream_context.cpp translateUnlocked() two-stage success block (line ~1411):
//     return makeSuccess(applyOutputAttrs(TranslationData(stage2Result.getValue().physicalAddress,
//                                                         intersected,
//                                                         stage2Result.getValue().securityState)));
// The TranslationData constructor does NOT carry forward stage1's pageAttr, so the
// result always has pageAttr derived only from stage2 (default = 0xFF for Normal).
// When S1=Device(0x00) and S2=Normal(0xFF), the result must be Device(0x00), not Normal(0xFF).
//
// This is a DIFFERENT code path than smmu.cpp::performBothStagesTranslation (already fixed).
// This path is exercised when StreamContext::translate() is called directly.
//
// These tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"

#include <memory>

namespace smmu {
namespace test {

static constexpr IOVA SC_BASE_IOVA = 0xA000u;
static constexpr PA   SC_STAGE1_PA = 0xB000u; // IPA (S1 output = S2 input)
static constexpr PA   SC_FINAL_PA  = 0xC000u; // PA  (S2 output)

// Helper: set up a StreamContext with both stages enabled
static void setupTwoStageCtx(StreamContext& ctx)
{
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();
}

// ── BUG-13.1.5-CPP-SC Test 1 ────────────────────────────────────────────────
// S1 maps IOVA→IPA as Device memory (AddressSpace::mapPageDevice → pageAttr=0x00)
// S2 maps IPA→PA  as Normal memory  (AddressSpace::mapPage       → pageAttr=0xFF)
// StreamContext::translate() two-stage path must combine: Device wins → 0x00
//
// Before fix: result.pageAttr == 0xFF (S2 Normal, S1 pageAttr discarded)
// After  fix: result.pageAttr == 0x00 (Device wins per §13.1.5)
TEST(BugAudit135CppScTest, S1Device_S2Normal_CombinedIsDevice_StreamContextDirect)
{
    StreamContext ctx;
    setupTwoStageCtx(ctx);

    // Stage-1: map IOVA→IPA as Device memory
    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(s1as->mapPageDevice(SC_BASE_IOVA, static_cast<PA>(SC_STAGE1_PA), rw).isOk())
        << "S1 mapPageDevice failed";
    ASSERT_TRUE(ctx.addPASID(0u, s1as).isOk());

    // Stage-2: map IPA→PA as Normal memory (default)
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(s2as->mapPage(SC_STAGE1_PA, SC_FINAL_PA, rw).isOk())
        << "S2 mapPage failed";
    ctx.setStage2AddressSpace(s2as);

    // Call StreamContext::translate() directly — exercises stream_context.cpp path
    TranslationResult result = ctx.translate(0u, SC_BASE_IOVA, AccessType::Read,
                                             SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Two-stage translation must succeed";

    // Combined pageAttr must be 0x00 (Device wins)
    EXPECT_EQ(result.getValue().pageAttr, 0x00u)
        << "BUG-13.1.5-CPP (stream_context.cpp path): S1=Device(0x00) + S2=Normal(0xFF) "
           "must combine to Device(0x00) per §13.1.5 strength ordering. "
           "Got pageAttr=0x" << std::hex << static_cast<int>(result.getValue().pageAttr);
}

// ── BUG-13.1.5-CPP-SC Test 2 ────────────────────────────────────────────────
// S1 Normal + S2 Device: combined must be Device (0x00).
// This likely passes before fix since S2 pageAttr is 0x00 (Device) and the current
// code uses S2's pageAttr. Confirms symmetry and that the fix doesn't regress this case.
TEST(BugAudit135CppScTest, S1Normal_S2Device_CombinedIsDevice_StreamContextDirect)
{
    StreamContext ctx;
    setupTwoStageCtx(ctx);

    // Stage-1: map IOVA→IPA as Normal memory
    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(s1as->mapPage(SC_BASE_IOVA, static_cast<PA>(SC_STAGE1_PA), rw).isOk())
        << "S1 mapPage failed";
    ASSERT_TRUE(ctx.addPASID(0u, s1as).isOk());

    // Stage-2: map IPA→PA as Device memory
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(s2as->mapPageDevice(SC_STAGE1_PA, SC_FINAL_PA, rw).isOk())
        << "S2 mapPageDevice failed";
    ctx.setStage2AddressSpace(s2as);

    TranslationResult result = ctx.translate(0u, SC_BASE_IOVA, AccessType::Read,
                                             SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Two-stage translation must succeed";

    EXPECT_EQ(result.getValue().pageAttr, 0x00u)
        << "BUG-13.1.5-CPP (stream_context.cpp path): S1=Normal(0xFF) + S2=Device(0x00) "
           "must combine to Device(0x00). "
           "Got pageAttr=0x" << std::hex << static_cast<int>(result.getValue().pageAttr);
}

// ── BUG-13.1.5-CPP-SC Test 3 ────────────────────────────────────────────────
// S1 Normal + S2 Normal = Normal (0xFF) — sanity check, must pass before and after fix.
TEST(BugAudit135CppScTest, S1Normal_S2Normal_CombinedIsNormal_StreamContextDirect)
{
    StreamContext ctx;
    setupTwoStageCtx(ctx);

    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(s1as->mapPage(SC_BASE_IOVA, static_cast<PA>(SC_STAGE1_PA), rw).isOk());
    ASSERT_TRUE(ctx.addPASID(0u, s1as).isOk());

    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(s2as->mapPage(SC_STAGE1_PA, SC_FINAL_PA, rw).isOk());
    ctx.setStage2AddressSpace(s2as);

    TranslationResult result = ctx.translate(0u, SC_BASE_IOVA, AccessType::Read,
                                             SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Two-stage translation must succeed";

    EXPECT_EQ(result.getValue().pageAttr, 0xFFu)
        << "S1=Normal(0xFF) + S2=Normal(0xFF) must combine to Normal(0xFF). "
           "Got pageAttr=0x" << std::hex << static_cast<int>(result.getValue().pageAttr);
}

}  // namespace test
}  // namespace smmu
