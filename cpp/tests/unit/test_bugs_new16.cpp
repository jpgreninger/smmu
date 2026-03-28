// TDD failing tests for NEW-AUDIT-04 and NEW-AUDIT-05.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// NEW-AUDIT-04 (Both §4.3.3 / §4.3.4): CMD_CFGI_CD / CMD_CFGI_CD_ALL missing
// stage-1 guard
//
//   ARM IHI0070G.b §4.3.3 line 5362:
//     "This command raises CERROR_ILL when stage 1 is not implemented."
//   ARM IHI0070G.b §4.3.4 line 5388:
//     "This command raises CERROR_ILL when stage 1 is not implemented."
//
//   When a stream is configured without stage-1 (bypass-only or stage-2-only),
//   issuing CMD_CFGI_CD or CMD_CFGI_CD_ALL for that stream must:
//     - Return CERROR_ILL in CMDQ_CONS.ERR.
//     - Toggle GERROR.CMDQ_ERR (bit 0).
//
//   Current behavior (WRONG): both commands execute silently — no CERROR_ILL,
//   no GERROR.CMDQ_ERR.
//
//   BEFORE FIX: GERROR.CMDQ_ERR remains 0 → failing tests FAIL.
//   AFTER FIX:  GERROR.CMDQ_ERR is set + CERROR_ILL in CMDQ_CONS.ERR → PASS.
//
//   Baseline/regression tests verify stage-1-enabled streams are NOT affected.
//
// ─────────────────────────────────────────────────────────────────────────────
// NEW-AUDIT-05 (Both §7.3.12 line 27078): injectWalkEabt() missing two-stage
// parameters
//
//   ARM IHI0070G.b §7.3.12: F_WALK_EABT can arise in three contexts:
//     1. S1 walk (isStage2=false, eventClass=1).
//     2. S2 walk of a TT descriptor (isStage2=true, eventClass=1).
//     3. S2 walk of an IPA input (isStage2=true, eventClass=2).
//
//   Current API: injectWalkEabt(streamID, pasid, iova, SecurityState) always
//   emits eventClass=1 and s2=false — the two-stage cases cannot be expressed.
//
//   Desired new API:
//     injectWalkEabt(streamID, pasid, iova, bool isStage2, uint8_t eventClass,
//                    SecurityState ss = SecurityState::NonSecure)
//   Existing default behaviour preserved: isStage2=false, eventClass=1.
//
//   BEFORE FIX: new two-parameter overload does not compile (method missing
//               with that signature) → tests 2 and 3 FAIL.
//   AFTER FIX:  s2 and eventClass fields populated → tests 2 and 3 PASS.
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

// Returns true when GERROR.CMDQ_ERR (bit 0) is active.
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

// Build a bypass-only StreamConfig (STE.Config = 0b100: bypass, no translation).
// stage1Enabled=false, stage2Enabled=false, bypassEnabled=true.
static StreamConfig makeBypassConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    return cfg;
}

// Build a stage-2-only StreamConfig (STE.Config = 0b110: S2 only).
// stage1Enabled=false, stage2Enabled=true.
static StreamConfig makeStage2OnlyConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    return cfg;
}

// Build a stage-1-only StreamConfig.
// stage1Enabled=true, stage2Enabled=false.
static StreamConfig makeStage1Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    return cfg;
}

// Submit CMD_CFGI_CD for (streamID, pasid) and process the command queue.
static void submitCfgiCd(SMMU& smmu, StreamID sid, PASID pasid) {
    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD;
    cmd.streamID = sid;
    cmd.pasid    = pasid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Submit CMD_CFGI_CD_ALL for streamID and process the command queue.
static void submitCfgiCdAll(SMMU& smmu, StreamID sid) {
    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD_ALL;
    cmd.streamID = sid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

} // anonymous namespace

// ============================================================================
// NEW-AUDIT-04: CMD_CFGI_CD missing stage-1 guard (ARM §4.3.3 line 5362)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: bypass-only stream + CMD_CFGI_CD → CERROR_ILL + GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 line 5362: "This command raises CERROR_ILL when stage 1 is not
// implemented."  A bypass-only stream has stage-1 disabled → CERROR_ILL.
//
// BEFORE FIX: CMD_CFGI_CD executes silently; GERROR.CMDQ_ERR remains 0 → FAILS.
// AFTER FIX:  GERROR.CMDQ_ERR toggled + CMDQ_CONS.ERR=CERROR_ILL → PASSES.
TEST(NewAudit04CfgiCdStage1Guard, BypassStream_CfgiCd_RaisesCerrorIll) {
    // §4.3.3: CMD_CFGI_CD on a bypass-only stream (no stage-1) must raise CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x40u;

    // Configure a bypass-only stream (stage-1 not implemented for this stream).
    smmu.configureStream(sid, makeBypassConfig());
    smmu.enableStream(sid);

    // Clear any setup events before the test assertion.
    smmu.clearEventQueue();

    // Submit CMD_CFGI_CD targeting the bypass stream.
    submitCfgiCd(smmu, sid, /*pasid=*/0u);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "NEW-AUDIT-04: CMD_CFGI_CD on a bypass-only stream (stage-1 not "
           "implemented) must toggle GERROR.CMDQ_ERR (bit 0). "
           "ARM §4.3.3 line 5362: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented.' "
           "Current code: no stage-1 guard in CFGI_CD handler → GERROR unchanged. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "NEW-AUDIT-04: CMD_CFGI_CD on bypass stream must set CMDQ_CONS.ERR to "
           "CERROR_ILL (=1) per ARM §4.3.3 line 5362. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: stage-2-only stream + CMD_CFGI_CD → CERROR_ILL + GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3: Stage-2-only (stage2Enabled=true, stage1Enabled=false) also has
// "stage 1 not implemented" for this stream → CERROR_ILL.
//
// BEFORE FIX: CMD_CFGI_CD executes silently → GERROR.CMDQ_ERR remains 0 → FAILS.
// AFTER FIX:  GERROR.CMDQ_ERR toggled + CMDQ_CONS.ERR=CERROR_ILL → PASSES.
TEST(NewAudit04CfgiCdStage1Guard, Stage2OnlyStream_CfgiCd_RaisesCerrorIll) {
    // §4.3.3: CMD_CFGI_CD on a stage-2-only stream (no stage-1 CD) → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x41u;

    // Configure a stage-2-only stream.
    smmu.configureStream(sid, makeStage2OnlyConfig());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    submitCfgiCd(smmu, sid, /*pasid=*/0u);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "NEW-AUDIT-04: CMD_CFGI_CD on a stage-2-only stream (stage-1 not "
           "implemented) must toggle GERROR.CMDQ_ERR (bit 0). "
           "ARM §4.3.3 line 5362: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented.' "
           "A stage-2-only stream has no CD — stage 1 is not implemented. "
           "Current code: no stage-1 guard → GERROR unchanged. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "NEW-AUDIT-04: CMD_CFGI_CD on stage-2-only stream must set "
           "CMDQ_CONS.ERR=CERROR_ILL per ARM §4.3.3. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 (baseline): stage-1-enabled stream + CMD_CFGI_CD → no error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3: When stage 1 IS implemented, CMD_CFGI_CD must execute normally.
// This verifies the new guard fires only when stage-1 is absent.
//
// BEFORE FIX: (already passes — no guard means stage-1 streams work fine).
// AFTER FIX:  must still pass (regression guard).
TEST(NewAudit04CfgiCdStage1Guard, Stage1Stream_CfgiCd_NoError) {
    // Regression/baseline: CMD_CFGI_CD on a stage-1-enabled stream → no CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x42u;

    smmu.configureStream(sid, makeStage1Config());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    submitCfgiCd(smmu, sid, /*pasid=*/0u);

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "NEW-AUDIT-04 baseline: CMD_CFGI_CD on a stage-1-enabled stream must NOT "
           "raise CERROR_ILL (ARM §4.3.3: the guard fires only when stage-1 is absent). "
           "The fix must not over-reject stage-1 streams. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "NEW-AUDIT-04 baseline: CMDQ_CONS.ERR must NOT be CERROR_ILL for a "
           "stage-1-enabled stream.";
}

// ============================================================================
// NEW-AUDIT-04: CMD_CFGI_CD_ALL missing stage-1 guard (ARM §4.3.4 line 5388)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: bypass-only stream + CMD_CFGI_CD_ALL → CERROR_ILL + GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.4 line 5388: "This command raises CERROR_ILL when stage 1 is not
// implemented."
//
// BEFORE FIX: CMD_CFGI_CD_ALL executes silently → GERROR.CMDQ_ERR remains 0 → FAILS.
// AFTER FIX:  GERROR.CMDQ_ERR toggled + CMDQ_CONS.ERR=CERROR_ILL → PASSES.
TEST(NewAudit04CfgiCdAllStage1Guard, BypassStream_CfgiCdAll_RaisesCerrorIll) {
    // §4.3.4: CMD_CFGI_CD_ALL on a bypass-only stream → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x43u;

    smmu.configureStream(sid, makeBypassConfig());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    submitCfgiCdAll(smmu, sid);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "NEW-AUDIT-04: CMD_CFGI_CD_ALL on a bypass-only stream (stage-1 not "
           "implemented) must toggle GERROR.CMDQ_ERR (bit 0). "
           "ARM §4.3.4 line 5388: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented.' "
           "Current code: no stage-1 guard in CFGI_CD_ALL handler → GERROR unchanged. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "NEW-AUDIT-04: CMD_CFGI_CD_ALL on bypass stream must set "
           "CMDQ_CONS.ERR=CERROR_ILL per ARM §4.3.4 line 5388. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ============================================================================
// NEW-AUDIT-05: injectWalkEabt() missing isStage2 / eventClass parameters
// (ARM §7.3.12 line 27078)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (baseline): single-stage walk abort → eventClass=1, s2=false
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12: Single-stage walk abort has CLASS=TT (eventClass=1) and S2=0.
// This is the existing behaviour; the test verifies the new API signature is
// backward-compatible.
//
// Calls the NEW extended signature:
//   injectWalkEabt(sid, pasid, iova, isStage2=false, eventClass=1)
//
// BEFORE FIX: new overload does not exist → fails to compile → FAILS.
// AFTER FIX:  compiles; event has eventClass=1 and s2=false → PASSES.
TEST(NewAudit05WalkEabtTwoStage, SingleStageWalk_EventClass1_S2False) {
    // §7.3.12: single-stage walk abort (isStage2=false, eventClass=1).
    // Baseline: new extended signature must preserve existing behaviour.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 0x70u;
    const PASID    pasid = 0u;
    const IOVA     iova  = 0x1000u;

    // New API: injectWalkEabt(streamID, pasid, iova, isStage2, eventClass, securityState).
    // isStage2=false, eventClass=1 → single-stage TT walk abort (existing behaviour).
    smmu.injectWalkEabt(sid, pasid, iova,
                        /*isStage2=*/false, /*eventClass=*/1u,
                        SecurityState::NonSecure);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "NEW-AUDIT-05 baseline: at least one event must be enqueued after "
           "injectWalkEabt(isStage2=false, eventClass=1)";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_WALK_EABT) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "NEW-AUDIT-05 baseline: F_WALK_EABT event not found in queue";

    EXPECT_EQ(ev->eventClass, 1u)
        << "NEW-AUDIT-05 baseline: single-stage walk abort must have eventClass=1 "
           "(CLASS=TT per ARM §7.3.12). "
           "Got eventClass=" << static_cast<unsigned>(ev->eventClass);

    EXPECT_FALSE(ev->s2)
        << "NEW-AUDIT-05 baseline: single-stage walk abort must have s2=false. "
           "Got s2=" << ev->s2;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: two-stage walk abort at TT level → s2=true, eventClass=1
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12 line 27078: walk abort during stage-2 TT descriptor fetch has
// S2=true and CLASS=TT (eventClass=1).
//
// BEFORE FIX: existing API has no isStage2/eventClass params → new overload
//             does not compile → FAILS.
// AFTER FIX:  event has s2=true and eventClass=1 → PASSES.
TEST(NewAudit05WalkEabtTwoStage, TwoStageWalkTT_S2True_EventClass1) {
    // §7.3.12: two-stage walk abort at TT descriptor level → S2=true, CLASS=TT.
    // This is the two-stage case where the stage-2 table walk itself aborted.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid   = 0x71u;
    const PASID    pasid = 0u;
    const IOVA     iova  = 0x2000u;

    // New API: isStage2=true, eventClass=1 → stage-2 TT walk abort.
    // BEFORE FIX: overload with (bool, uint8_t) params does not exist → compile error.
    smmu.injectWalkEabt(sid, pasid, iova,
                        /*isStage2=*/true, /*eventClass=*/1u,
                        SecurityState::NonSecure);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "NEW-AUDIT-05: at least one event must be enqueued after "
           "injectWalkEabt(isStage2=true, eventClass=1)";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_WALK_EABT) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "NEW-AUDIT-05: F_WALK_EABT event not found (two-stage TT abort test)";

    EXPECT_TRUE(ev->s2)
        << "NEW-AUDIT-05: two-stage walk abort (TT level) must have s2=true "
           "(ARM §7.3.12: S2 field indicates the walk fault occurred in stage-2). "
           "Current code: injectWalkEabt() always sets s2=false. "
           "Got s2=" << ev->s2;

    EXPECT_EQ(ev->eventClass, 1u)
        << "NEW-AUDIT-05: two-stage walk abort (TT level) must have eventClass=1 "
           "(CLASS=TT per ARM §7.3.12). "
           "Got eventClass=" << static_cast<unsigned>(ev->eventClass);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: two-stage walk abort at IPA input level → s2=true, eventClass=2
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12 line 27078: walk abort when the IPA is the input address to the
// stage-2 walk has S2=true and CLASS=IN (eventClass=2).
//
// BEFORE FIX: overload does not exist → compile error → FAILS.
// AFTER FIX:  event has s2=true and eventClass=2 → PASSES.
TEST(NewAudit05WalkEabtTwoStage, TwoStageWalkIPA_S2True_EventClass2) {
    // §7.3.12: two-stage walk abort at IPA input → S2=true, CLASS=IN (eventClass=2).
    // This is the two-stage case where the IPA itself is the faulting address.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid   = 0x72u;
    const PASID    pasid = 0u;
    const IOVA     iova  = 0x3000u;

    // New API: isStage2=true, eventClass=2 → stage-2 IPA input walk abort.
    // BEFORE FIX: overload with (bool, uint8_t) params does not exist → compile error.
    smmu.injectWalkEabt(sid, pasid, iova,
                        /*isStage2=*/true, /*eventClass=*/2u,
                        SecurityState::NonSecure);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "NEW-AUDIT-05: at least one event must be enqueued after "
           "injectWalkEabt(isStage2=true, eventClass=2)";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_WALK_EABT) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "NEW-AUDIT-05: F_WALK_EABT event not found (two-stage IPA abort test)";

    EXPECT_TRUE(ev->s2)
        << "NEW-AUDIT-05: two-stage walk abort (IPA input) must have s2=true "
           "(ARM §7.3.12: S2 flag indicates stage-2 fault). "
           "Current code: injectWalkEabt() always sets s2=false. "
           "Got s2=" << ev->s2;

    EXPECT_EQ(ev->eventClass, 2u)
        << "NEW-AUDIT-05: two-stage walk abort (IPA input) must have eventClass=2 "
           "(CLASS=IN per ARM §7.3.12: IPA is the input address, not a TT descriptor). "
           "Current code: injectWalkEabt() always sets eventClass=1 (TT). "
           "Got eventClass=" << static_cast<unsigned>(ev->eventClass);
}
