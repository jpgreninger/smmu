// TDD failing test for BUG-NEW-F.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-F (§5.5 CdIllegal() pseudocode line 9748):
//
//   ARM IHI0070G.b §5.5 CdIllegal() pseudocode:
//     if STE.S1STALLD == '1' && CD.S == '1' then
//         return TRUE;  // CD is ILLEGAL → C_BAD_CD event
//
//   When s1Stalld=true AND faultMode=FaultMode::Stall (CD.S==1),
//   configureStream() MUST emit C_BAD_CD and return InvalidConfiguration,
//   for ALL values of STALL_MODEL.
//
//   The existing BUG-C3 check (STALL_MODEL!=0 + s1Stalld → C_BAD_STE) already
//   rejects STALL_MODEL≠0 before reaching the CD check, so the only uncovered
//   case is STALL_MODEL==0b00.
//
//   Current behavior (WRONG): configureStream() silently accepts
//   s1Stalld=true + faultMode=Stall when STALL_MODEL==0b00.  No C_BAD_CD
//   is emitted.
//
//   Required behavior: configureStream() must return an error and emit
//   C_BAD_CD event when stage1Enabled=true AND s1Stalld=true AND
//   faultMode=Stall, regardless of STALL_MODEL.
//
//   BEFORE FIX: configureStream() succeeds, no C_BAD_CD event → test 1 FAILS.
//   AFTER FIX:  configureStream() returns error, C_BAD_CD event emitted → PASSES.
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Returns true when the event queue contains at least one event of the given type.
static bool hasEvent(SMMU& smmu, EventType type) {
    std::vector<EventEntry> events = smmu.getEventQueue();
    for (const auto& ev : events) {
        if (ev.type == type) {
            return true;
        }
    }
    return false;
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-F test 1: s1Stalld=true + faultMode=Stall + STALL_MODEL==0 → C_BAD_CD
// ============================================================================
//
// ARM §5.5 CdIllegal() pseudocode line 9748:
//   if STE.S1STALLD == '1' && CD.S == '1' then return TRUE;
//
// This check applies unconditionally — for ALL values of STALL_MODEL, including
// STALL_MODEL==0b00 (the unordered model).
//
// The BUG-C3 fix guards against STALL_MODEL!=0b00 + s1Stalld → C_BAD_STE,
// but that check fires first only when STALL_MODEL != 0.  When STALL_MODEL==0b00,
// the BUG-C3 guard is skipped and the CdIllegal() check at line 9748 applies.
// The current code has no check for STALL_MODEL==0b00 + s1Stalld + CD.S==1.
//
// Setup:
//   setStallModel(0)         — STALL_MODEL==0b00 (BUG-C3 guard will NOT fire)
//   stage1Enabled=true
//   s1Stalld=true            — STE.S1STALLD==1
//   faultMode=FaultMode::Stall  — CD.S==1
//
// BEFORE FIX: configureStream() succeeds (isOk()), no C_BAD_CD event → FAILS.
// AFTER FIX:  configureStream() returns error (!isOk()), C_BAD_CD in queue → PASSES.
TEST(BugNewF_S1StalldCDStall, S1Stalld1_CDS1_STALL_MODEL0_EmitsCBadCd) {
    // §5.5 CdIllegal line 9748: S1STALLD=1 AND CD.S=1 → C_BAD_CD.
    // This must fire for STALL_MODEL==0b00 (the uncovered case).
    SMMU smmu;
    enableSMMU(smmu);

    // Ensure STALL_MODEL==0b00 so the BUG-C3 guard (STALL_MODEL!=0 + s1Stalld)
    // does NOT fire and mask the missing CdIllegal() check.
    smmu.setStallModel(0x00u);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Stall;  // CD.S==1
    cfg.securityState      = SecurityState::NonSecure;
    cfg.s1Stalld           = true;              // STE.S1STALLD==1
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x20u, cfg);

    // configureStream() must return an error (InvalidConfiguration).
    EXPECT_FALSE(result.isOk())
        << "BUG-NEW-F: configureStream() with s1Stalld=true AND faultMode=Stall "
           "AND STALL_MODEL==0b00 must return an error (ARM §5.5 CdIllegal line 9748: "
           "S1STALLD==1 && CD.S==1 → C_BAD_CD). "
           "Current code: no check for this combination when STALL_MODEL==0b00.";

    // C_BAD_CD must appear in the event queue.
    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-NEW-F: C_BAD_CD event must be recorded when s1Stalld=true AND "
           "faultMode=Stall AND STALL_MODEL==0b00 (ARM §5.5 CdIllegal line 9748). "
           "Current code silently accepts this combination with no event.";
}

// ============================================================================
// BUG-NEW-F test 2 (regression): s1Stalld=true + faultMode=Terminate + STALL_MODEL==0
// → ACCEPTED (valid configuration — CD.S==0, so CdIllegal check does not fire)
// ============================================================================
//
// ARM §5.5 CdIllegal() line 9748:
//   if STE.S1STALLD == '1' && CD.S == '1' then return TRUE;
//
// When faultMode=Terminate (CD.S==0), the CdIllegal condition is NOT triggered,
// even with s1Stalld=true.  The combination is valid: the stream aborts all
// stage-1 faults (S1STALLD=1 overrides stall, and CD.S=0 does the same).
//
// This test verifies that the fix is properly scoped: it must reject only the
// s1Stalld=true + CD.S==1 combination, not all s1Stalld=true configs.
//
// BEFORE FIX: (already passes — configureStream accepts this today).
// AFTER FIX:  must still pass — regression guard.
TEST(BugNewF_S1StalldCDStall, S1Stalld1_CDS0_STALL_MODEL0_Accepted) {
    // Regression: s1Stalld=true + faultMode=Terminate + STALL_MODEL==0 must succeed.
    // CD.S==0 (Terminate) → CdIllegal condition is NOT met → valid config.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x00u);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;  // CD.S==0
    cfg.securityState      = SecurityState::NonSecure;
    cfg.s1Stalld           = true;                  // STE.S1STALLD==1 (but CD.S==0 → OK)
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x21u, cfg);

    EXPECT_TRUE(result.isOk())
        << "BUG-NEW-F regression: s1Stalld=true + faultMode=Terminate + STALL_MODEL==0 "
           "must be ACCEPTED (ARM §5.5 CdIllegal: condition is S1STALLD=1 AND CD.S=1; "
           "CD.S=0 means the check does not apply). "
           "The fix must not over-reject configs where faultMode=Terminate.";

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-NEW-F regression: no C_BAD_CD event expected when faultMode=Terminate "
           "(CD.S==0) — the CdIllegal condition only fires when both S1STALLD=1 AND CD.S=1.";
}

// ============================================================================
// BUG-NEW-F test 3 (regression): s1Stalld=false + faultMode=Stall + STALL_MODEL==0
// → ACCEPTED (normal stall configuration)
// ============================================================================
//
// ARM §5.5 CdIllegal() line 9748:
//   if STE.S1STALLD == '1' && CD.S == '1' then return TRUE;
//
// When s1Stalld=false, the CdIllegal condition is NOT triggered regardless of
// faultMode.  This is the normal stall configuration.
//
// This test verifies that the fix targets only the s1Stalld=true + CD.S==1 combo.
//
// BEFORE FIX: (already passes — configureStream accepts this today).
// AFTER FIX:  must still pass — regression guard.
TEST(BugNewF_S1StalldCDStall, S1Stalld0_CDS1_STALL_MODEL0_Accepted) {
    // Regression: s1Stalld=false + faultMode=Stall + STALL_MODEL==0 must succeed.
    // STE.S1STALLD==0 → CdIllegal condition is NOT met → valid stall config.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x00u);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Stall;  // CD.S==1
    cfg.securityState      = SecurityState::NonSecure;
    cfg.s1Stalld           = false;             // STE.S1STALLD==0 → CdIllegal does not apply
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x22u, cfg);

    EXPECT_TRUE(result.isOk())
        << "BUG-NEW-F regression: s1Stalld=false + faultMode=Stall + STALL_MODEL==0 "
           "must be ACCEPTED (ARM §5.5 CdIllegal: condition requires S1STALLD=1; "
           "S1STALLD=0 means the check does not apply). "
           "The fix must not reject normal stall configurations.";

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-NEW-F regression: no C_BAD_CD event expected when s1Stalld=false "
           "(the CdIllegal condition only fires when both S1STALLD=1 AND CD.S=1).";
}
