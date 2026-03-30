// TDD failing tests for BUG-AUDIT-47 — CD T0SZ/T1SZ minimum bound check.
//
// ARM IHI0070G.b §5.4 CDTxSZInvalid():
//   For STT=0, VAX=0, IAS=48 (this model): txsz_min=16, txsz_max=39.
//   Mandatory check for SMMUv3.2+.
//
// Current code only checks the maximum (>39). Values in [1,15] are accepted
// silently — no C_BAD_CD event, no error return.
//
// BEFORE FIX: t0sz=10 accepted silently → tests FAIL.
// AFTER FIX:  C_BAD_CD emitted, error returned → tests PASS.

#include <gtest/gtest.h>
#include "smmu/smmu.h"

using namespace smmu;

// Helper: enable all queues and the SMMU global enable.
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);
}

// Helper: check whether any event of the given type is present in the event queue.
static bool hasEvent(SMMU& smmu, EventType type) {
    std::vector<EventEntry> events = smmu.getEventQueue();
    for (const auto& ev : events) {
        if (ev.type == type) {
            return true;
        }
    }
    return false;
}

// Helper: configure and enable a stage-1 stream with the given t0sz/t1sz.
// Optionally maps a page at 0x1000 so translateUnlocked() is reached.
static void setupStage1Stream(SMMU& smmu, StreamID sid,
                               uint8_t t0sz, uint8_t t1sz = 0u,
                               bool mapPages = true) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;
    cfg.t0sz               = t0sz;
    cfg.t1sz               = t1sz;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);
    if (mapPages) {
        PagePermissions perms(true, false, false);
        smmu.mapPage(sid, 0u, 0x1000u, 0x80001000u, perms);
    }
}

// ─── BUG-AUDIT-47 tests — must FAIL before fix, PASS after fix ───────────────

// Negative: t0sz=10 is below txsz_min=16.  translateUnlocked() must emit
// C_BAD_CD (ARM §5.4 CDTxSZInvalid).
//
// BEFORE FIX: only the >39 check exists → t0sz=10 is silently accepted →
//             no C_BAD_CD enqueued → test FAILS.
// AFTER FIX:  t0sz != 0 && t0sz < 16 triggers generateEvent(C_BAD_CD) → PASS.
TEST(BugAudit47, T0SzBelowMin_EmitsEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 200u;
    const IOVA     iova = 0x1000u;

    setupStage1Stream(smmu, sid, /*t0sz=*/10u);

    smmu.translate(sid, 0u, iova, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-AUDIT-47: t0sz=10 < 16 must emit C_BAD_CD "
           "(ARM §5.4 CDTxSZInvalid: txsz_min=16 for STT=0, VAX=0)";
}

// Negative: t0sz=16 (valid), t1sz=10 (below minimum) → C_BAD_CD must be emitted.
// The spec check applies to both T0SZ and T1SZ independently.
//
// BEFORE FIX: only the >39 check exists for both fields → t1sz=10 silently
//             accepted → no event → test FAILS.
// AFTER FIX:  t1sz != 0 && t1sz < 16 triggers generateEvent(C_BAD_CD) → PASS.
TEST(BugAudit47, T1SzBelowMin_EmitsEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 201u;
    const IOVA     iova = 0x1000u;

    setupStage1Stream(smmu, sid, /*t0sz=*/16u, /*t1sz=*/10u);

    smmu.translate(sid, 0u, iova, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-AUDIT-47: t1sz=10 < 16 must emit C_BAD_CD "
           "(ARM §5.4 CDTxSZInvalid: txsz_min applies to T1SZ as well)";
}

// Negative: t0sz=10 → translate() must return an error result (not ok).
// Exercises the return value path rather than the event queue.
//
// BEFORE FIX: no minimum-bound check → translateUnlocked() proceeds, reaches
//             VA range check (which passes because 2^(64-10) is huge), and
//             either finds the mapping or faults for a different reason.
//             In any case CDTxSZInvalid is not caught → result.isOk() may be
//             true → test FAILS.
// AFTER FIX:  makeTranslationError(SMMUError::InvalidConfiguration) returned
//             immediately → result.isOk() == false → test PASSES.
TEST(BugAudit47, T0SzBelowMin_TranslateFails) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 202u;
    const IOVA     iova = 0x1000u;

    setupStage1Stream(smmu, sid, /*t0sz=*/10u);

    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-47: t0sz=10 < 16 must cause translate() to return an error "
           "(ARM §5.4 CDTxSZInvalid → C_BAD_CD → InvalidConfiguration)";
}

// ─── Regression guard tests — must PASS both before AND after the fix ─────────

// Positive: t0sz=0 is the model sentinel for "no VA range restriction".
// The fix must NOT reject this value; the exclusion condition is (t0sz != 0u).
//
// This test passes before and after the fix.
TEST(BugAudit47, T0SzSentinelZero_Accepted) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 203u;
    const IOVA     iova = 0x1000u;

    setupStage1Stream(smmu, sid, /*t0sz=*/0u);

    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-AUDIT-47: t0sz=0 (sentinel: no restriction) must NOT emit C_BAD_CD — "
           "the fix must only reject non-zero values below 16";
    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-47: t0sz=0 (sentinel) must allow successful translation";
}

// Positive: t0sz=16 is exactly txsz_min — the lower boundary must be accepted.
// t0sz=16 → valid IOVAs are [0, 2^(64-16)) = [0, 2^48).  0x1000 is well within range.
//
// This test passes before and after the fix.
TEST(BugAudit47, T0SzAtMinBoundary_Accepted) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 204u;
    const IOVA     iova = 0x1000u;

    setupStage1Stream(smmu, sid, /*t0sz=*/16u);

    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-AUDIT-47: t0sz=16 (lower boundary of valid range [16,39]) must NOT emit C_BAD_CD "
           "(ARM §5.4 CDTxSZInvalid: txsz_min=16)";
    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-47: t0sz=16 (lower boundary) must allow successful translation";
}

// Positive: t0sz=39 is exactly txsz_max — the upper boundary must be accepted.
// t0sz=39 → valid IOVAs are [0, 2^(64-39)) = [0, 2^25) = [0, 32 MiB).
// IOVA=0x1000 is within that window.
// The existing >39 check already handled this correctly; this test guards against
// any regression introduced by the minimum-bound fix.
//
// This test passes before and after the fix.
TEST(BugAudit47, T0SzAtMaxBoundary_Accepted) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 205u;
    const IOVA     iova = 0x1000u;  // well within 2^25 = 32 MiB

    setupStage1Stream(smmu, sid, /*t0sz=*/39u);

    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-AUDIT-47: t0sz=39 (upper boundary of valid range [16,39]) must NOT emit C_BAD_CD "
           "(ARM §5.4 CDTxSZInvalid: txsz_max=39)";
    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-47: t0sz=39 (upper boundary) must allow successful translation";
}
