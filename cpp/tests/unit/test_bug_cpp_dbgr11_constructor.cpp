// BUG-CPP-DBGR-11: StreamContext constructor stage1Enabled=true but translationEnabled=false
// (§5.2 STE.Config==0b000 means all stages disabled)
// TDD: failing test written BEFORE the fix.
// A freshly constructed StreamContext (before any updateConfiguration() call) must have
// stage1Enabled=false and translationEnabled=false to match STE.Config==0b000.

#include <gtest/gtest.h>
#include "smmu/stream_context.h"
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

// -----------------------------------------------------------------------
// Freshly constructed StreamContext must have stage1Enabled=false
// -----------------------------------------------------------------------
TEST(StreamContextConstructorSpec, FreshContext_Stage1Enabled_IsFalse) {
    StreamContext ctx;
    StreamConfig cfg = ctx.getStreamConfiguration();
    EXPECT_FALSE(cfg.stage1Enabled)
        << "§5.2 STE.Config==0b000: freshly constructed StreamContext must have stage1Enabled=false";
}

// -----------------------------------------------------------------------
// Freshly constructed StreamContext must have translationEnabled=false
// -----------------------------------------------------------------------
TEST(StreamContextConstructorSpec, FreshContext_TranslationEnabled_IsFalse) {
    StreamContext ctx;
    StreamConfig cfg = ctx.getStreamConfiguration();
    EXPECT_FALSE(cfg.translationEnabled)
        << "§5.2 STE.Config==0b000: freshly constructed StreamContext must have translationEnabled=false";
}

// -----------------------------------------------------------------------
// Freshly constructed StreamContext must have stage2Enabled=false
// -----------------------------------------------------------------------
TEST(StreamContextConstructorSpec, FreshContext_Stage2Enabled_IsFalse) {
    StreamContext ctx;
    StreamConfig cfg = ctx.getStreamConfiguration();
    EXPECT_FALSE(cfg.stage2Enabled)
        << "§5.2 STE.Config==0b000: freshly constructed StreamContext must have stage2Enabled=false";
}

// -----------------------------------------------------------------------
// A freshly configured stream in the SMMU must be consistent:
// configureStream() with translationEnabled=false must keep stage1Enabled=false
// -----------------------------------------------------------------------
TEST(StreamContextConstructorSpec, ConfiguredStream_NoTranslation_Stage1IsFalse) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    smmu.configureStream(0x60, cfg);

    // Verify translate() returns StreamDisabled (not a spurious translation)
    TranslationResult result = smmu.translate(0x60, 0, 0x1000ULL,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "Translation must fail for a stream with translationEnabled=false";
}

// -----------------------------------------------------------------------
// After updateConfiguration() with stage1Enabled=true, it should be true
// (verifies that updateConfiguration() correctly sets stage1Enabled)
// -----------------------------------------------------------------------
TEST(StreamContextConstructorSpec, AfterUpdateConfig_Stage1Enabled_IsTrue) {
    StreamContext ctx;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    VoidResult result = ctx.updateConfiguration(cfg);
    ASSERT_TRUE(result.isOk()) << "updateConfiguration() must succeed";

    StreamConfig readback = ctx.getStreamConfiguration();
    EXPECT_TRUE(readback.stage1Enabled)
        << "After updateConfiguration() with stage1Enabled=true, it must be true";
    EXPECT_TRUE(readback.translationEnabled)
        << "After updateConfiguration() with translationEnabled=true, it must be true";
}

} // namespace test
} // namespace smmu
