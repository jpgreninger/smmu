// TDD failing tests for NEW-A and NEW-B.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green).
//
// ─────────────────────────────────────────────────────────────────────────────
// NEW-A (§5.2 STE.S1DSS line 6725): ATS Translation Requests terminated by
//   S1DSS==0b00 + PASID==0 must NOT record an F_STREAM_DISABLED event.
//   ARM §5.2 STE.S1DSS: "For ATS Translation Requests, if the cases described
//   in 0b00 and 0b10 lead to termination, the Translation Request is terminated
//   with a CA and no event is recorded."
//   The S1DSS==0b10 path already suppresses the event (NEW-FINDING-1 fix in
//   test_bugs_new9.cpp).  The S1DSS==0b00 path does not.
//   BEFORE FIX: F_STREAM_DISABLED event IS present for ATS TR → test FAILS.
//   AFTER FIX:  ATS TR aborts, no F_STREAM_DISABLED recorded → test PASSES.
//
// NEW-B (§5.5 CdIllegal): STALL_MODEL==0b01 (terminate-only) + stage1Enabled
//   + CD.S==1 (FaultMode::Stall) is an illegal combination → C_BAD_CD.
//   ARM §5.5 CdIllegal() pseudocode: "if stall_model == '01' && CD.S == '1'
//   then return TRUE".
//   The existing BUG-C2 check covers STALL_MODEL==0b10 + CD.S==0 → C_BAD_CD.
//   NEW-B adds the symmetric case: STALL_MODEL==0b01 + CD.S==1 → C_BAD_CD.
//   BEFORE FIX: configureStream succeeds with no event → test FAILS.
//   AFTER FIX:  C_BAD_CD event recorded → test PASSES.
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

// Check whether any event of the given type is in the event queue.
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
// NEW-A: S1DSS==0b00 ATS TR event suppression (§5.2)
// ============================================================================
//
// ARM §5.2 STE.S1DSS (line 6725): "For ATS Translation Requests, if the cases
// described in 0b00 and 0b10 lead to termination, the Translation Request is
// terminated with a CA and no event is recorded."
//
// S1DSS==0b00 — non-substream transaction on substream-capable stream aborts
// with F_STREAM_DISABLED.  For ATS TR the abort must still happen but the
// event must be suppressed.
//
// BEFORE FIX: recordFault + generateEvent called unconditionally → F_STREAM_DISABLED
//   IS in the event queue even for ATS TR → test NA-1 FAILS.
// AFTER FIX:  event recording guarded by (transactionType != AtsTranslationRequest)
//   → no F_STREAM_DISABLED for ATS TR → test NA-1 PASSES.
//
// IMPORTANT: The stream must have eats!=0 to pass the ATS-support check at
// translate() lines 573-580 (the atsSupported guard).  With eats==0 the SMMU
// returns F_BAD_ATS_TREQ before reaching the S1DSS check, which never exercises
// the suppression path.

// Test NA-1: S1DSS==0b00 ATS TR must NOT record F_STREAM_DISABLED (no event).
TEST(NewFindingA_S1DSS00AtsSuppress, AtsTranslationRequest_S1DSS00_NoEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.s1cdMax            = 1u;    // substream-capable
    cfg.s1dss              = 0x00u; // S1DSS==0b00: abort non-substream transactions
    cfg.eats               = 1u;    // ATS-capable: bypass the F_BAD_ATS_TREQ guard

    smmu.configureStream(20u, cfg);

    // ATS TR with PASID==0 and S1DSS==0b00 must abort WITHOUT recording an event.
    auto result = smmu.translate(20u, 0u, 0x1000u, AccessType::Read,
                                 SecurityState::NonSecure,
                                 TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk())
        << "NA-1: S1DSS==0b00 + ATS TR + PASID==0 must abort";
    EXPECT_FALSE(hasEvent(smmu, EventType::F_STREAM_DISABLED))
        << "NA-1: ATS TR must NOT record F_STREAM_DISABLED when S1DSS==0b00 (ARM §5.2)";
}

// Test NA-2: S1DSS==0b00 Ordinary transaction MUST record F_STREAM_DISABLED (regression).
TEST(NewFindingA_S1DSS00AtsSuppress, OrdinaryTransaction_S1DSS00_RecordsEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.s1cdMax            = 1u;
    cfg.s1dss              = 0x00u;

    smmu.configureStream(21u, cfg);

    auto result = smmu.translate(21u, 0u, 0x1000u, AccessType::Read,
                                 SecurityState::NonSecure,
                                 TransactionType::Ordinary);

    EXPECT_FALSE(result.isOk())
        << "NA-2 regression: S1DSS==0b00 + Ordinary + PASID==0 must still abort";
    EXPECT_TRUE(hasEvent(smmu, EventType::F_STREAM_DISABLED))
        << "NA-2 regression: Ordinary transaction MUST record F_STREAM_DISABLED when S1DSS==0b00 (ARM §5.2)";
}

// ============================================================================
// NEW-B: STALL_MODEL==0b01 + CD.S==1 → C_BAD_CD (§5.5 CdIllegal)
// ============================================================================
//
// ARM §5.5 CdIllegal() pseudocode:
//   "if stall_model == '01' && CD.S == '1' then return TRUE"
//
// When terminate-only mode (STALL_MODEL==0b01) is active, a stage-1 stream
// configured with FaultMode::Stall (CD.S==1) is illegal → C_BAD_CD.
//
// The symmetric BUG-C2 check (STALL_MODEL==0b10 + CD.S==0 → C_BAD_CD) already
// exists.  NEW-B adds the opposite case.
//
// BEFORE FIX: no check for STALL_MODEL==0b01 + FaultMode::Stall → configureStream
//   succeeds without event → test NB-1 FAILS.
// AFTER FIX:  C_BAD_CD event recorded → test NB-1 PASSES.

// Test NB-1: STALL_MODEL==0b01 + stage1Enabled + FaultMode::Stall → C_BAD_CD.
TEST(NewFindingB_StallModel01CDS1, TerminateModel_Stage1_StallMode_Causes_CBAD_CD) {
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x01u); // terminate-only model

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Stall;  // CD.S==1 — illegal with terminate-only model
    cfg.securityState      = SecurityState::NonSecure;

    smmu.configureStream(22u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_CD))
        << "NB-1: STALL_MODEL==0b01 + stage1Enabled + FaultMode::Stall (CD.S==1) must "
           "generate C_BAD_CD event (ARM §5.5 CdIllegal)";
}

// Test NB-2: STALL_MODEL==0b01 + stage1Enabled + FaultMode::Terminate → NO error (regression).
TEST(NewFindingB_StallModel01CDS1, TerminateModel_Stage1_TerminateMode_NoError) {
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x01u); // terminate-only model

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate; // CD.S==0 — valid with terminate-only model
    cfg.securityState      = SecurityState::NonSecure;

    smmu.configureStream(23u, cfg);

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_CD))
        << "NB-2 regression: STALL_MODEL==0b01 + FaultMode::Terminate must NOT generate C_BAD_CD";
}
