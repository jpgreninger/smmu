// TDD failing test for BUG-AUDIT-113
//
// Bug: ARM §5.2 IgnoreSTESTRW() pseudocode states that for Config==0b11x
// (stage-2 enabled), STRW is IGNORED.  The current implementation at
// cpp/src/smmu/smmu.cpp line 1044 computes:
//
//   bool strwUnused = !config.stage1Enabled;
//
// This is wrong because it only skips STRW validation for stage-1-disabled
// streams (stage-2-only or bypass).  For a two-stage stream (stage1=true,
// stage2=true, Config=0b111), stage1Enabled=true so strwUnused=false and
// STRW validation runs.  The bit[0]=1 check then rejects STRW=EL2 (0b01)
// with C_BAD_STE, but the spec says STRW must be IGNORED for Config=0b11x.
//
// Fix (not applied here): change the formula to:
//   bool strwUnused = !config.stage1Enabled || config.stage2Enabled;
//
// This test MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID SID_TWO_STAGE = 0xB0;

// ── BUG-AUDIT-113: two-stage stream with STRW=EL2 must not be rejected ─────────

/// ARM §5.2 IgnoreSTESTRW(): Config=0b11x (stage2Enabled) means STRW is
/// IGNORED.  configureStream() with stage1=true, stage2=true, STRW=EL2 (0b01)
/// on a NonSecure stream must SUCCEED (return SMMUError::Success / isOk()).
///
/// Before the fix the implementation incorrectly validates STRW for two-stage
/// streams and rejects this configuration with C_BAD_STE.
TEST(BugAudit113Test, TwoStageNonSecure_STRW_EL2_ConfigureSucceeds) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.strw               = StreamWorld::EL2;  // 0b01 — STRW must be IGNORED per §5.2 IgnoreSTESTRW()

    VoidResult result = smmu.configureStream(SID_TWO_STAGE, cfg);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-113: two-stage (Config=0b111) NonSecure stream with STRW=EL2 "
           "must be accepted — ARM §5.2 IgnoreSTESTRW() says STRW is IGNORED when "
           "stage-2 is enabled.  Got error: "
        << static_cast<int>(result.getError());
}

/// Complementary check: no C_BAD_STE event must be generated when STRW is
/// correctly ignored for a two-stage stream.
TEST(BugAudit113Test, TwoStageNonSecure_STRW_EL2_NoCBadSteEvent) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.strw               = StreamWorld::EL2;  // 0b01 — must be ignored

    smmu.configureStream(SID_TWO_STAGE, cfg);

    const auto& events = smmu.getEventQueue();
    bool hasCBadSte = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) {
            hasCBadSte = true;
            break;
        }
    }

    EXPECT_FALSE(hasCBadSte)
        << "BUG-AUDIT-113: C_BAD_STE must NOT be generated when STRW is IGNORED "
           "for a two-stage stream (ARM §5.2 IgnoreSTESTRW()).";
}

// ── Regression: single-stage NonSecure stream, STRW=EL2 (0b01) still illegal ──

/// ARM §5.2 SteIllegal(): for a stage-1-only (Config=0b101) NonSecure stream,
/// STRW=EL2 (bit[0]=1) IS illegal and must still be rejected with C_BAD_STE.
/// This regression test ensures the fix does not accidentally allow STRW=EL2
/// for streams where stage-2 is NOT enabled.
TEST(BugAudit113Test, Stage1Only_NonSecure_STRW_EL2_StillRejected) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;   // stage-1 only — STRW validation applies
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.strw               = StreamWorld::EL2;  // 0b01 — ILLEGAL for stage-1-only NS stream

    VoidResult result = smmu.configureStream(SID_TWO_STAGE + 1, cfg);

    EXPECT_FALSE(result.isOk())
        << "Regression: stage-1-only NonSecure stream with STRW=EL2 must still be "
           "rejected with C_BAD_STE (ARM §5.2 SteIllegal()).";
}

} // namespace test
} // namespace smmu
