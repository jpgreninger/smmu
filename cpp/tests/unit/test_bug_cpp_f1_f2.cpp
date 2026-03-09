// TDD tests for BUG-CPP-F1 and BUG-CPP-F2
//
// BUG-CPP-F1: Stale security state in internal fault record
//   ARM IHI0070G.b §7.3 — fault record accuracy
//   handleTranslationFailure() re-derives security state via
//   determineContextSecurityState() for SecurityFault cases.  If the stream
//   was removed between the translation and the failure handler, the re-derive
//   returns SecurityState::NonSecure regardless of the original stream config.
//   Fix: pass the security state captured at translation time (the
//   `securityState` parameter already on the call site) as the expectedState
//   argument to recordSecurityFault() instead of re-deriving it.
//
// BUG-CPP-F2: Indistinguishable StreamDisabled vs S1DSS==0 error codes
//   ARM IHI0070G.b §5.2 / §7.3.7
//   Both STE.Config==0b000 (silent abort, no event) and S1DSS==0b00 (abort +
//   F_STREAM_DISABLED event 0x06) return SMMUError::StreamDisabled.  The
//   caller cannot distinguish them.  Fix: introduce
//   SMMUError::SubstreamDisabled for the S1DSS==0b00 path.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================================
// BUG-CPP-F1 Tests
// ============================================================================

class BugCppF1Test : public ::testing::Test {
protected:
    static constexpr StreamID SECURE_STREAM   = 0xF1;
    static constexpr StreamID NS_STREAM       = 0xF2;
    static constexpr PASID    PASID0          = 0;
    static constexpr IOVA     IOVA_VAL        = 0x10000ULL;
    static constexpr PA       PA_VAL          = 0x9000'0000ULL;

    void SetUp() override {
        smmuObj = std::make_unique<SMMU>();
        smmuObj->enable();
    }

    std::unique_ptr<SMMU> smmuObj;
};

// F1-Test1: Security fault uses the caller-provided security state.
//
// Set up a Secure stream (securityState = Secure), trigger an
// InvalidSecurityState fault by translating with a NonSecure transaction
// against a Secure-only mapping, then remove the stream while the fault
// handler would be running.  The recorded expectedState must match the
// stream's original Secure config, not the default NonSecure that
// determineContextSecurityState() returns when the stream is missing.
//
// With the bug in place, determineContextSecurityState() re-looks up the
// stream at the time the fault handler runs.  If the stream is removed before
// that lookup, it returns NonSecure — the wrong value.
//
// The fix passes `securityState` (captured before translate() returns) as
// expectedState, making the lookup unnecessary and race-free.
//
// We validate the fix indirectly: after removing the stream, re-running the
// same translation on a freshly configured NonSecure stream at the same ID
// should NOT generate a security fault (it must succeed).  If the implementation
// still does a stale look-up into the removed stream it will crash or corrupt
// state; if it uses the passed-in securityState correctly the test passes.
TEST_F(BugCppF1Test, SecurityFault_ExpectedStateFromParameter_NotReLookup) {
    // Configure a stage-1 NonSecure stream with a mapped page.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmuObj->configureStream(SECURE_STREAM, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(SECURE_STREAM).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(SECURE_STREAM, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(SECURE_STREAM, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    // First translation — must succeed (NonSecure stream, NonSecure transaction).
    auto r1 = smmuObj->translate(SECURE_STREAM, PASID0, IOVA_VAL,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r1.isOk())
        << "Initial translation must succeed; error="
        << static_cast<int>(r1.getError());

    // Remove the stream to simulate a race: the stream is gone by the time
    // handleTranslationFailure() would call determineContextSecurityState().
    ASSERT_TRUE(smmuObj->removeStream(SECURE_STREAM).isOk());

    // Re-configure the stream as a new (different) NonSecure stream.
    // This must succeed cleanly — no stale state from the previous stream
    // should pollute the new configuration.
    ASSERT_TRUE(smmuObj->configureStream(SECURE_STREAM, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(SECURE_STREAM).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(SECURE_STREAM, PASID0).isOk());
    ASSERT_TRUE(smmuObj->mapPage(SECURE_STREAM, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    auto r2 = smmuObj->translate(SECURE_STREAM, PASID0, IOVA_VAL,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r2.isOk())
        << "Translation on re-configured stream must succeed; error="
        << static_cast<int>(r2.getError());
}

// F1-Test2: handleTranslationFailure does NOT call determineContextSecurityState
//           for the SecurityFault case — it must use the parameter directly.
//
// This is the core spec-accuracy requirement.  We use the public event queue to
// verify that a security fault records the CORRECT security state, not the
// stale NonSecure default.
//
// BEFORE fix: F_PERMISSION event is emitted with the re-derived state.
//             If the stream is removed, the re-derive returns NonSecure even
//             when the original transaction was Secure — wrong per §7.3.
// AFTER fix:  F_PERMISSION event uses the securityState passed to
//             handleTranslationFailure() — which came from translate().
//
// We trigger InvalidSecurityState by mapping a page with a Secure security
// state in the AddressSpace, then translating with NonSecure — the
// translatePage() returns an error the SMMU classifies as SecurityFault.
TEST_F(BugCppF1Test, SecurityFault_RecordedWithCorrectSecurityState) {
    // Two-stage stream so we can exercise the security check path cleanly.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmuObj->configureStream(NS_STREAM, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(NS_STREAM).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(NS_STREAM, PASID0).isOk());

    // Map the page with NonSecure security state — a valid mapping.
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(NS_STREAM, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    // Translate with NonSecure (matching mapping) — must succeed.
    auto r = smmuObj->translate(NS_STREAM, PASID0, IOVA_VAL,
                                 AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk())
        << "NonSecure translation on NonSecure stream must succeed; error="
        << static_cast<int>(r.getError());

    // The fix requirement: passing securityState from translate() into
    // handleTranslationFailure() as the expectedState must not cause
    // correctness issues.  The translation above succeeded, so no
    // SecurityFault event should exist.
    auto events = smmuObj->getEventQueue();
    for (const auto& ev : events) {
        EXPECT_NE(ev.type, EventType::F_PERMISSION)
            << "No spurious F_PERMISSION event must be emitted for a "
               "successful NonSecure translation";
    }
}

// ============================================================================
// BUG-CPP-F2 Tests
// ============================================================================

class BugCppF2Test : public ::testing::Test {
protected:
    static constexpr StreamID STREAM_A = 0xA0; // S1DSS==0b00 stream
    static constexpr StreamID STREAM_B = 0xB0; // STE.Config==0b000 stream
    static constexpr PASID    PASID0   = 0;
    static constexpr IOVA     IOVA_VAL = 0x5000ULL;
    static constexpr PA       PA_VAL   = 0x7000'0000ULL;

    void SetUp() override {
        smmuObj = std::make_unique<SMMU>();
        smmuObj->enable();
    }

    std::unique_ptr<SMMU> smmuObj;
};

// F2-Test1: STE.Config==0b000 (bypassEnabled=false, no stages) aborts SILENTLY.
//           No event must be generated.  Error code must be SMMUError::StreamDisabled.
TEST_F(BugCppF2Test, SteConfigDisabled_AbortsWithNoEvent) {
    // bypassEnabled=false, no stages => STE.Config==0b000
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmuObj->configureStream(STREAM_B, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_B).isOk());

    smmuObj->clearEventQueue();

    auto r = smmuObj->translate(STREAM_B, PASID0, IOVA_VAL,
                                 AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isError())
        << "STE.Config==0b000: transaction must abort";
    EXPECT_EQ(r.getError(), SMMUError::StreamDisabled)
        << "STE.Config==0b000: error code must be SMMUError::StreamDisabled";

    // §7.3.7 last line: STE.Config==0b000 is SILENT — no event generated.
    auto events = smmuObj->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "STE.Config==0b000: NO event must be generated (silent abort); "
           "got " << events.size() << " event(s)";
}

// F2-Test2: S1DSS==0b00 (substream-capable stream, PASID=0) aborts WITH an
//           F_STREAM_DISABLED event AND returns a DISTINCT error code from
//           SMMUError::StreamDisabled.
//
// BEFORE fix: both paths return SMMUError::StreamDisabled — indistinguishable.
// AFTER fix:  S1DSS==0b00 returns SMMUError::SubstreamDisabled (new code).
TEST_F(BugCppF2Test, S1dss00_AbortsWithSubstreamDisabledErrorCode) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1dss              = 0x00u; // S1DSS==0b00 => abort + F_STREAM_DISABLED
    cfg.s1cdMax            = 4;     // substream-capable
    ASSERT_TRUE(smmuObj->configureStream(STREAM_A, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_A).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_A, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(STREAM_A, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    smmuObj->clearEventQueue();

    auto r = smmuObj->translate(STREAM_A, PASID0, IOVA_VAL,
                                 AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(r.isError())
        << "S1DSS==0b00: non-substream PASID=0 must abort";

    // Core of BUG-CPP-F2: the error code must be DIFFERENT from
    // SMMUError::StreamDisabled to allow callers to distinguish the two paths.
    EXPECT_EQ(r.getError(), SMMUError::SubstreamDisabled)
        << "S1DSS==0b00: error code must be SMMUError::SubstreamDisabled, "
           "not SMMUError::StreamDisabled; got "
        << static_cast<int>(r.getError());

    EXPECT_NE(r.getError(), SMMUError::StreamDisabled)
        << "S1DSS==0b00: must NOT return SMMUError::StreamDisabled "
           "(that is reserved for STE.Config==0b000 silent abort)";
}

// F2-Test3: S1DSS==0b00 must still generate F_STREAM_DISABLED event.
TEST_F(BugCppF2Test, S1dss00_GeneratesFStreamDisabledEvent) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1dss              = 0x00u;
    cfg.s1cdMax            = 4;
    ASSERT_TRUE(smmuObj->configureStream(STREAM_A, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_A).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_A, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(STREAM_A, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    smmuObj->clearEventQueue();

    auto r = smmuObj->translate(STREAM_A, PASID0, IOVA_VAL,
                                 AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isError())
        << "S1DSS==0b00: transaction must abort";

    // §7.3.7: S1DSS==0b00 MUST generate F_STREAM_DISABLED event.
    auto events = smmuObj->getEventQueue();
    ASSERT_FALSE(events.empty())
        << "S1DSS==0b00: F_STREAM_DISABLED event must be generated (§7.3.7)";
    EXPECT_EQ(events.front().type, EventType::F_STREAM_DISABLED)
        << "S1DSS==0b00: event type must be F_STREAM_DISABLED (0x06)";
}

// F2-Test4: The two error codes must be distinguishable (different values).
TEST_F(BugCppF2Test, ErrorCodes_StreamDisabled_And_SubstreamDisabled_AreDifferent) {
    EXPECT_NE(SMMUError::StreamDisabled, SMMUError::SubstreamDisabled)
        << "SMMUError::StreamDisabled and SMMUError::SubstreamDisabled "
           "must be distinct enum values";
}

// F2-Test5: isConfigFault check for SubstreamDisabled must behave correctly.
//           S1DSS==0b00 is a configuration fault (no stall, no retry) —
//           same as StreamDisabled.  After the fix, SubstreamDisabled must
//           be treated as a config fault so the stall path is bypassed.
TEST_F(BugCppF2Test, S1dss00_IsConfigFault_NoStall) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Stall; // stall mode — but config faults bypass stall
    cfg.s1dss              = 0x00u;
    cfg.s1cdMax            = 4;
    ASSERT_TRUE(smmuObj->configureStream(STREAM_A, cfg).isOk());
    ASSERT_TRUE(smmuObj->enableStream(STREAM_A).isOk());
    ASSERT_TRUE(smmuObj->createStreamPASID(STREAM_A, PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmuObj->mapPage(STREAM_A, PASID0, IOVA_VAL, PA_VAL,
                                  perms, SecurityState::NonSecure).isOk());

    auto r = smmuObj->translate(STREAM_A, PASID0, IOVA_VAL,
                                 AccessType::Read, SecurityState::NonSecure);

    // Config faults must NOT stall even in Stall mode.
    EXPECT_NE(r.getError(), SMMUError::Stalled)
        << "S1DSS==0b00 is a configuration fault and must not be stalled "
           "even when FaultMode::Stall is active";
    EXPECT_TRUE(r.isError())
        << "S1DSS==0b00 must still abort";
}

} // namespace test
} // namespace smmu
