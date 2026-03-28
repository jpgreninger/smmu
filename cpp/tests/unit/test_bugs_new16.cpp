// TDD failing tests for NEW-AUDIT-04 and NEW-AUDIT-05.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// NEW-AUDIT-04 (Both §4.3.3 / §4.3.4): CMD_CFGI_CD / CMD_CFGI_CD_ALL guard
// condition clarification
//
//   ARM IHI0070G.b §4.3.3 line 5362 / §4.3.4 line 5388:
//     "This command raises CERROR_ILL when stage 1 is not implemented
//      (SMMU_IDR0.S1P == 0)."
//
//   The guard is on the SMMU-GLOBAL IDR0.S1P capability bit, NOT on whether
//   a specific target stream uses stage-1.  When IDR0.S1P==1 (default), ALL
//   CFGI_CD and CFGI_CD_ALL commands must succeed silently — even those
//   targeting bypass-only or stage-2-only streams — because the SMMU hardware
//   as a whole supports stage-1 and may have other streams with CDs cached.
//
//   When IDR0.S1P==0 (global stage-1 absent), any CFGI_CD or CFGI_CD_ALL
//   must raise CERROR_ILL regardless of the target stream's configuration.
//
//   Tests updated (BUG-AUDIT-NEW-02 correction):
//     Test 1 (BypassStream_CfgiCd_NoError_WhenS1PEnabled):
//       IDR0.S1P==1 (default), bypass stream → NO CERROR_ILL.
//       BEFORE FIX: code checked per-stream stage1Enabled, which is false for
//       bypass → incorrectly raised CERROR_ILL → TEST FAILED.
//       AFTER FIX:  guard checks IDR0.S1P (==1) → no error → TEST PASSES.
//
//     Test 2 (Stage2OnlyStream_CfgiCd_NoError_WhenS1PEnabled):
//       IDR0.S1P==1 (default), stage-2-only stream → NO CERROR_ILL.
//       BEFORE FIX: same per-stream bug → incorrectly raised CERROR_ILL → FAILED.
//       AFTER FIX:  IDR0.S1P==1 → no error → PASSES.
//
//     Test 3 (Stage1Stream_CfgiCd_NoError — unchanged):
//       Regression: stage-1-enabled stream + IDR0.S1P==1 → no error.
//       Passes both before and after the fix.
//
//     Test 4 (BypassStream_CfgiCdAll_NoError_WhenS1PEnabled):
//       IDR0.S1P==1 (default), bypass stream + CFGI_CD_ALL → NO CERROR_ILL.
//       BEFORE FIX: per-stream bug → incorrectly raised CERROR_ILL → FAILED.
//       AFTER FIX:  IDR0.S1P==1 → no error → PASSES.
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
// Test 1: bypass-only stream + CMD_CFGI_CD + IDR0.S1P==1 → NO error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 line 5362 / spec line 6605: "This command raises CERROR_ILL when
// stage 1 is not implemented (SMMU_IDR0.S1P == 0)."  The guard condition is
// SMMU-global IDR0.S1P, NOT whether the target stream uses stage-1.
//
// When IDR0.S1P==1 (the default), CFGI_CD must succeed silently even for a
// bypass-only stream, because the SMMU globally supports stage-1.
//
// BEFORE FIX (wrong behavior): code checked per-stream stage1Enabled (false for
//   bypass) → incorrectly raised CERROR_ILL → GERROR.CMDQ_ERR was set → FAILS.
// AFTER FIX (correct behavior): guard checks IDR0.S1P (==1) → no error → PASSES.
TEST(NewAudit04CfgiCdStage1Guard, BypassStream_CfgiCd_NoError_WhenS1PEnabled) {
    // §4.3.3 / spec line 6605: CMD_CFGI_CD on a bypass-only stream with
    // IDR0.S1P==1 (default) must NOT raise CERROR_ILL.  The guard fires only
    // when the SMMU globally lacks stage-1 (IDR0.S1P==0), not when a specific
    // stream happens to use bypass mode.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x40u;

    // Configure a bypass-only stream (stage-1 not used by this particular stream).
    smmu.configureStream(sid, makeBypassConfig());
    smmu.enableStream(sid);

    // Clear any setup events before the test assertion.
    smmu.clearEventQueue();

    // IDR0.S1P is 1 by default (bit 1 is always set when S1P is supported).
    // Submit CMD_CFGI_CD targeting the bypass stream.
    submitCfgiCd(smmu, sid, /*pasid=*/0u);

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-02: CMD_CFGI_CD on a bypass-only stream must NOT toggle "
           "GERROR.CMDQ_ERR when IDR0.S1P==1 (global stage-1 supported). "
           "ARM §4.3.3 spec line 6605: guard fires on IDR0.S1P==0 (SMMU-global), "
           "not on per-stream stage1Enabled. "
           "BEFORE FIX: code checked per-stream stage1Enabled=false → incorrectly "
           "raised CERROR_ILL. "
           "AFTER FIX: code checks IDR0.S1P==1 → no error. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-02: CMD_CFGI_CD on bypass stream with IDR0.S1P==1 must "
           "NOT set CMDQ_CONS.ERR to CERROR_ILL. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: stage-2-only stream + CMD_CFGI_CD + IDR0.S1P==1 → NO error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 spec line 6605: guard condition is IDR0.S1P (SMMU-global), not
// per-stream stage1Enabled.  A stage-2-only stream with IDR0.S1P==1 must not
// cause CERROR_ILL when CFGI_CD is issued.
//
// BEFORE FIX (wrong behavior): code checked per-stream stage1Enabled=false →
//   incorrectly raised CERROR_ILL → GERROR.CMDQ_ERR was set → FAILS.
// AFTER FIX (correct behavior): IDR0.S1P==1 → no error → PASSES.
TEST(NewAudit04CfgiCdStage1Guard, Stage2OnlyStream_CfgiCd_NoError_WhenS1PEnabled) {
    // §4.3.3 / spec line 6605: CMD_CFGI_CD on a stage-2-only stream with
    // IDR0.S1P==1 (default) must NOT raise CERROR_ILL.  The spec condition
    // is the SMMU-global S1P bit; a stage-2-only stream still runs on an SMMU
    // that supports stage-1 globally, so the guard must not fire.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x41u;

    // Configure a stage-2-only stream (stage-1 not used by this stream).
    smmu.configureStream(sid, makeStage2OnlyConfig());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    submitCfgiCd(smmu, sid, /*pasid=*/0u);

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-02: CMD_CFGI_CD on a stage-2-only stream must NOT toggle "
           "GERROR.CMDQ_ERR when IDR0.S1P==1. "
           "ARM §4.3.3 spec line 6605: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented (SMMU_IDR0.S1P == 0).' "
           "The guard checks IDR0.S1P, not per-stream stage1Enabled. "
           "BEFORE FIX: per-stream check raised CERROR_ILL incorrectly. "
           "AFTER FIX: IDR0.S1P==1 → no error. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-02: CMD_CFGI_CD on stage-2-only stream with IDR0.S1P==1 "
           "must NOT set CMDQ_CONS.ERR to CERROR_ILL. "
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
// Test 4: bypass-only stream + CMD_CFGI_CD_ALL + IDR0.S1P==1 → NO error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.4 line 5388 / spec line 6605: same IDR0.S1P global guard applies to
// CFGI_CD_ALL.  When IDR0.S1P==1 (default), bypass stream → no CERROR_ILL.
//
// BEFORE FIX (wrong behavior): per-stream check raised CERROR_ILL for bypass →
//   GERROR.CMDQ_ERR was set → FAILS.
// AFTER FIX (correct behavior): IDR0.S1P==1 → no error → PASSES.
TEST(NewAudit04CfgiCdAllStage1Guard, BypassStream_CfgiCdAll_NoError_WhenS1PEnabled) {
    // §4.3.4 / spec line 6605: CMD_CFGI_CD_ALL on a bypass-only stream with
    // IDR0.S1P==1 (default) must NOT raise CERROR_ILL.
    // The guard is SMMU-global (IDR0.S1P), not per-stream.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x43u;

    smmu.configureStream(sid, makeBypassConfig());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    submitCfgiCdAll(smmu, sid);

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-02: CMD_CFGI_CD_ALL on a bypass-only stream must NOT "
           "toggle GERROR.CMDQ_ERR when IDR0.S1P==1. "
           "ARM §4.3.4 spec line 6605: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented (SMMU_IDR0.S1P == 0).' "
           "Guard is SMMU-global, not per-stream stage1Enabled. "
           "BEFORE FIX: per-stream check raised CERROR_ILL for bypass stream. "
           "AFTER FIX: IDR0.S1P==1 → no error. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-02: CMD_CFGI_CD_ALL on bypass stream with IDR0.S1P==1 "
           "must NOT set CMDQ_CONS.ERR to CERROR_ILL. "
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
