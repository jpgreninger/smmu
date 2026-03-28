// TDD failing tests for BUG-AUDIT-NEW-01, BUG-AUDIT-NEW-02, and BUG-AUDIT-NEW-03.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-NEW-01 (Both §4.4.1.1 / §4.4.3.2): TLBI_S2_IPA missing RIL
// reserved-parameter CERROR_ILL check
//
//   ARM IHI0070G.b §4.4.1.1 line 6117 (applies to all RIL-capable commands):
//     "If RIL==1 and TG!=0 and NUM==0 and SCALE==0 and TTL==0b00, then
//      CERROR_ILL is raised."
//   ARM IHI0070G.b §4.4.3.2 (CMD_TLBI_S2_IPA description):
//     Supports RIL range-based invalidation; the reserved-param check applies.
//
//   The pattern already applied to TLBI_NH_VA and TLBI_NH_VAA (smmu.cpp lines
//   4621-4626 and 4635-4639):
//     if (command.ril && command.tg != 0u && command.num == 0u
//             && command.scale == 0u && command.ttl == 0u) {
//         writeCmdqConsErr(CERROR_ILL); signalGerror(GERROR_CMDQ_ERR); break;
//     }
//   This pattern is absent from the TLBI_S2_IPA case (lines 4657-4668).
//
//   Current behavior (WRONG): TLBI_S2_IPA with ril=true, tg!=0, num=0,
//   scale=0, ttl=0 executes the TLB invalidation without raising CERROR_ILL.
//
//   BEFORE FIX: no RIL check → command executes silently → GERROR.CMDQ_ERR
//   remains 0 → Test 1 FAILS.
//   AFTER FIX:  RIL reserved-param check added → GERROR.CMDQ_ERR toggled +
//   CMDQ_CONS.ERR=CERROR_ILL → Test 1 PASSES.
//
//   Tests 2 and 3 are regression guards: valid RIL and ril=false paths must
//   continue to succeed without error both before and after the fix.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-NEW-03 (Both §4.4.2): IDR0.S1P not configurable; NH_* commands
// missing IDR0.S1P==0 CERROR_ILL guard
//
//   ARM IHI0070G.b §4.4.2 (CMD_TLBI_NH_ALL, §4.4.2.1):
//     "If SMMU_IDR0.S1P==0, CERROR_ILL."
//   ARM IHI0070G.b §4.4.2 (CMD_TLBI_NH_ASID, §4.4.2.2):
//     "If SMMU_IDR0.S1P==0, CERROR_ILL."
//   ARM IHI0070G.b §4.4.2 (CMD_TLBI_NH_VA, §4.4.2.3):
//     "If SMMU_IDR0.S1P==0, CERROR_ILL."
//
//   IDR0.S1P (bit 1) is currently hardcoded to 1 in getIDR0() (smmu.cpp line
//   3254: `| (1u << 1)`).  There is no setS1PSupported() API to configure it,
//   and the TLBI_NH_* command handlers have no S1P==0 guard.
//
//   Two separate bugs:
//     (a) No setS1PSupported(bool) API → cannot configure IDR0.S1P at runtime.
//     (b) TLBI_NH_ALL / TLBI_NH_ASID / TLBI_NH_VA do not check IDR0.S1P.
//
//   BEFORE FIX:
//     - setS1PSupported() does not exist → tests calling it fail to compile.
//     - Even if S1P were forced to 0, TLBI_NH_* would not raise CERROR_ILL.
//   AFTER FIX:
//     - setS1PSupported(false) clears IDR0 bit 1.
//     - TLBI_NH_ALL / TLBI_NH_ASID / TLBI_NH_VA raise CERROR_ILL when S1P==0.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-NEW-02 (Both §4.3.3 / §4.3.4): CFGI_CD / CFGI_CD_ALL global
// IDR0.S1P guard — new tests for the S1P==0 path
//
//   ARM IHI0070G.b §4.3.3 line 5362 / spec line 6605:
//     "This command raises CERROR_ILL when stage 1 is not implemented
//      (SMMU_IDR0.S1P == 0)."
//
//   The tests in test_bugs_new16.cpp were corrected to verify that when
//   IDR0.S1P==1, bypass/stage-2-only streams do NOT raise CERROR_ILL.
//   This file adds the complementary tests: when IDR0.S1P==0 (via the new
//   setS1PSupported(false) API), CFGI_CD and CFGI_CD_ALL MUST raise CERROR_ILL
//   regardless of the target stream's stage-1 configuration.
//
//   BEFORE FIX:
//     - setS1PSupported() does not exist → tests fail to compile.
//     - Even if S1P could be cleared, CFGI_CD guard checked per-stream state,
//       not IDR0.S1P, so a stage-1-enabled stream would not raise CERROR_ILL.
//   AFTER FIX:
//     - setS1PSupported(false) clears IDR0 bit 1.
//     - CFGI_CD and CFGI_CD_ALL check IDR0.S1P==0 → CERROR_ILL raised.
//
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

// Build a stage-1-only StreamConfig.
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

// Build a bypass-only StreamConfig.
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

// Submit CMD_TLBI_S2_IPA with the given RIL fields and process the queue.
static void submitTlbiS2Ipa(SMMU& smmu, bool ril, uint8_t tg, uint8_t num,
                             uint8_t scale, uint8_t ttl, uint16_t vmid = 0u) {
    CommandEntry cmd;
    cmd.type  = CommandType::TLBI_S2_IPA;
    cmd.vmid  = vmid;
    cmd.ril   = ril;
    cmd.tg    = tg;
    cmd.num   = num;
    cmd.scale = scale;
    cmd.ttl   = ttl;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Submit CMD_TLBI_NH_ALL and process the queue.
static void submitTlbiNhAll(SMMU& smmu, uint16_t vmid = 0u) {
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    cmd.vmid = vmid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Submit CMD_TLBI_NH_ASID and process the queue.
static void submitTlbiNhAsid(SMMU& smmu, uint16_t vmid = 0u, uint16_t asid = 0u) {
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ASID;
    cmd.vmid = vmid;
    cmd.asid = asid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Submit CMD_TLBI_NH_VA and process the queue.
static void submitTlbiNhVa(SMMU& smmu, uint16_t vmid = 0u, uint16_t asid = 0u) {
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.vmid         = vmid;
    cmd.asid         = asid;
    cmd.startAddress = 0x1000u;
    // ril=false, tg=0 → valid (no RIL reserved-param check triggered)
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Submit CMD_CFGI_CD for (streamID, pasid) and process the queue.
static void submitCfgiCd(SMMU& smmu, StreamID sid, PASID pasid = 0u) {
    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD;
    cmd.streamID = sid;
    cmd.pasid    = pasid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Submit CMD_CFGI_CD_ALL for streamID and process the queue.
static void submitCfgiCdAll(SMMU& smmu, StreamID sid) {
    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD_ALL;
    cmd.streamID = sid;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

} // anonymous namespace

// ============================================================================
// BUG-AUDIT-NEW-01: TLBI_S2_IPA missing RIL reserved-param CERROR_ILL check
// (ARM §4.4.1.1 / §4.4.3.2)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: TLBI_S2_IPA with ril=true, tg=1, num=0, scale=0, ttl=0 → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.1.1: when RIL=1 and TG!=0, the combination NUM==0, SCALE==0,
// TTL==0b00 is reserved and must raise CERROR_ILL.  This applies to all
// RIL-capable TLBI commands including TLBI_S2_IPA.
//
// The same check is already present for TLBI_NH_VA (smmu.cpp lines 4621-4626):
//   if (command.ril && command.tg != 0u && command.num == 0u
//           && command.scale == 0u && command.ttl == 0u) { CERROR_ILL }
// TLBI_S2_IPA (lines 4657-4668) has an S2P guard but no RIL reserved check.
//
// BEFORE FIX: TLBI_S2_IPA handler falls through to executeInvalidationCommandLocked()
//   with no RIL check → TLB op executes silently → GERROR.CMDQ_ERR remains 0 → FAILS.
// AFTER FIX:  RIL reserved-param check added before TLB op → CERROR_ILL raised → PASSES.
TEST(AuditNew01TlbiS2IpaRil, TlbiS2Ipa_RilReserved_RaisesCerrorIll) {
    // §4.4.1.1 / §4.4.3.2: TLBI_S2_IPA with ril=true, tg=1 (64KB), num=0,
    // scale=0, ttl=0 is a reserved RIL combination — CERROR_ILL must be raised.
    // BUG-AUDIT-NEW-01: this check is absent from the TLBI_S2_IPA handler.
    SMMU smmu;
    enableSMMU(smmu);

    // S2P must be enabled so the command is not rejected for the S2P==0 reason
    // before reaching the RIL check.
    smmu.setS2PSupported(true);

    smmu.clearEventQueue();

    // ril=true, tg=1 (non-zero granule = 64KB), num=0, scale=0, ttl=0:
    // reserved RIL combination per ARM §4.4.1.1.
    // BEFORE FIX: silent TLB op, no error.
    // AFTER FIX:  CERROR_ILL + GERROR_CMDQ_ERR.
    submitTlbiS2Ipa(smmu, /*ril=*/true, /*tg=*/1u, /*num=*/0u,
                    /*scale=*/0u, /*ttl=*/0u);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-01: TLBI_S2_IPA with ril=true, tg=1, num=0, scale=0, "
           "ttl=0 must toggle GERROR.CMDQ_ERR (bit 0). "
           "ARM §4.4.1.1: 'If RIL==1 and TG!=0 and NUM==0 and SCALE==0 and "
           "TTL==0b00, CERROR_ILL.' "
           "The same reserved-param check is applied to TLBI_NH_VA (smmu.cpp "
           "lines 4621-4626) but is absent from TLBI_S2_IPA (lines 4657-4668). "
           "BEFORE FIX: command executes silently. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-01: TLBI_S2_IPA reserved RIL combination must set "
           "CMDQ_CONS.ERR to CERROR_ILL per ARM §4.4.1.1. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (regression): TLBI_S2_IPA with ril=true, tg=1, num=4 → no error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.1.1: the reserved combination requires num==0 AND scale==0 AND
// ttl==0.  When num!=0 (num=4 here), the RIL params are valid → no CERROR_ILL.
//
// This test passes both before and after the fix (regression guard: the fix
// must not over-reject valid RIL operands).
TEST(AuditNew01TlbiS2IpaRil, TlbiS2Ipa_RilValid_NoError) {
    // §4.4.1.1: ril=true, tg=1, num=4 (non-zero) → valid RIL combination.
    // The reserved-param check fires only when num==0 AND scale==0 AND ttl==0.
    // A non-zero num means the range is well-defined → command must succeed.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setS2PSupported(true);

    smmu.clearEventQueue();

    // ril=true, tg=1 (64KB), num=4, scale=1, ttl=0: valid RIL range.
    // num!=0 → not the reserved combination → must execute without CERROR_ILL.
    submitTlbiS2Ipa(smmu, /*ril=*/true, /*tg=*/1u, /*num=*/4u,
                    /*scale=*/1u, /*ttl=*/0u);

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-01 regression: TLBI_S2_IPA with ril=true, tg=1, num=4 "
           "(non-zero) must NOT raise CERROR_ILL. "
           "ARM §4.4.1.1: reserved check requires num==0 AND scale==0 AND ttl==0. "
           "Here num=4 → valid operand → command must succeed silently. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-01 regression: valid RIL TLBI_S2_IPA must not set "
           "CMDQ_CONS.ERR to CERROR_ILL.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 (regression): TLBI_S2_IPA with ril=false → no error
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.1.1: the reserved-param check is gated on ril=true.  When ril=false
// the TG/NUM/SCALE/TTL fields are ignored entirely → no CERROR_ILL regardless
// of their values.
//
// This test passes both before and after the fix (regression guard).
TEST(AuditNew01TlbiS2IpaRil, TlbiS2Ipa_RilFalse_NoError) {
    // §4.4.1.1: ril=false → RIL check not applicable; tg/num/scale/ttl ignored.
    // Even with tg=1, num=0, scale=0, ttl=0 (which would be reserved when
    // ril=true), the command must succeed silently when ril=false.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setS2PSupported(true);

    smmu.clearEventQueue();

    // ril=false: the RIL reserved-param check must not trigger.
    // tg=1, num=0, scale=0, ttl=0 would be reserved if ril were true,
    // but ril=false gates off the entire check.
    submitTlbiS2Ipa(smmu, /*ril=*/false, /*tg=*/1u, /*num=*/0u,
                    /*scale=*/0u, /*ttl=*/0u);

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-01 regression: TLBI_S2_IPA with ril=false must NOT "
           "raise CERROR_ILL regardless of tg/num/scale/ttl values. "
           "ARM §4.4.1.1: the reserved-param check is gated on ril==1. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-01 regression: ril=false TLBI_S2_IPA must not set "
           "CMDQ_CONS.ERR to CERROR_ILL.";
}

// ============================================================================
// BUG-AUDIT-NEW-03: IDR0.S1P not configurable + NH_* commands missing S1P guard
// (ARM §4.4.2.1 / §4.4.2.2 / §4.4.2.3)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (regression/baseline): IDR0.S1P==1 by default
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1 SMMU_IDR0 bit 1 (S1P): stage-1 translation present.  The default
// model advertises S1P=1.  This test verifies the bit is set without any API
// call — it is a regression guard that must pass both before and after the fix.
TEST(AuditNew03S1P, S1PSupported_DefaultTrue_Idr0Bit1Set) {
    // §6.3.1: IDR0.S1P (bit 1) must be set in a freshly constructed SMMU
    // (stage-1 translation is supported by default in this model).
    SMMU smmu;
    enableSMMU(smmu);

    const uint32_t idr0  = smmu.getIDR0();
    const uint32_t s1p   = (idr0 >> 1u) & 1u;

    EXPECT_EQ(s1p, 1u)
        << "BUG-AUDIT-NEW-03 baseline: IDR0.S1P (bit 1) must be 1 by default. "
           "ARM §6.3.1: S1P indicates stage-1 translation is present. "
           "This model supports stage-1 by default. "
           "IDR0=0x" << std::hex << idr0;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: setS1PSupported(false) → IDR0.S1P (bit 1) reads as 0
// ─────────────────────────────────────────────────────────────────────────────
//
// After calling setS1PSupported(false), IDR0 bit 1 must be cleared.
//
// BEFORE FIX: setS1PSupported() does not exist → fails to compile → FAILS.
// AFTER FIX:  method added; IDR0 bit 1 is gated on s1pSupported_ → bit 1
// reads 0 after setS1PSupported(false) → PASSES.
TEST(AuditNew03S1P, SetS1PSupported_False_Idr0Bit1Clear) {
    // §6.3.1: setS1PSupported(false) must clear IDR0.S1P (bit 1).
    // BEFORE FIX: setS1PSupported() does not exist → compile error.
    // AFTER FIX:  IDR0 bit 1 = 0 after the call.
    SMMU smmu;
    enableSMMU(smmu);

    // BEFORE FIX: this line does not compile (method does not exist).
    // AFTER FIX:  clears IDR0 bit 1.
    smmu.setS1PSupported(false);

    const uint32_t idr0 = smmu.getIDR0();
    const uint32_t s1p  = (idr0 >> 1u) & 1u;

    EXPECT_EQ(s1p, 0u)
        << "BUG-AUDIT-NEW-03: IDR0.S1P (bit 1) must be 0 after "
           "setS1PSupported(false). "
           "ARM §6.3.1: S1P is cleared when stage-1 translation is absent. "
           "Current code: IDR0 bit 1 is hardcoded to 1 (smmu.cpp line 3254: "
           "'| (1u << 1)'); no setS1PSupported() API exists. "
           "BEFORE FIX: method missing → compile error. "
           "AFTER FIX: bit 1 reads 0 → PASSES. "
           "IDR0=0x" << std::hex << idr0;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: TLBI_NH_ALL with IDR0.S1P==0 → CERROR_ILL + GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.2.1 CMD_TLBI_NH_ALL: "If SMMU_IDR0.S1P==0, CERROR_ILL."
//
// When S1P is disabled (setS1PSupported(false)), TLBI_NH_ALL must raise
// CERROR_ILL because the NH (non-hypervisor) TLB invalidation commands only
// make sense when stage-1 translation is present.
//
// BEFORE FIX: setS1PSupported() does not exist → compile error → FAILS.
//   Even if it did exist, TLBI_NH_ALL has no S1P guard → silent execution.
// AFTER FIX:  S1P guard added to TLBI_NH_ALL handler → CERROR_ILL → PASSES.
TEST(AuditNew03S1P, TlbiNhAll_S1PDisabled_RaisesCerrorIll) {
    // §4.4.2.1: TLBI_NH_ALL with IDR0.S1P==0 must raise CERROR_ILL.
    // BEFORE FIX: setS1PSupported() missing → compile error.
    // AFTER FIX:  S1P guard in TLBI_NH_ALL handler → CERROR_ILL raised.
    SMMU smmu;
    enableSMMU(smmu);

    // Disable S1P so TLBI_NH_ALL should raise CERROR_ILL.
    // BEFORE FIX: this line does not compile (method does not exist).
    smmu.setS1PSupported(false);

    smmu.clearEventQueue();

    submitTlbiNhAll(smmu);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-03: TLBI_NH_ALL must toggle GERROR.CMDQ_ERR (bit 0) "
           "when IDR0.S1P==0. "
           "ARM §4.4.2.1: 'If SMMU_IDR0.S1P==0, CERROR_ILL.' "
           "Current code: TLBI_NH_ALL case (smmu.cpp) has no S1P guard. "
           "BEFORE FIX: command executes silently; GERROR unchanged. "
           "AFTER FIX:  S1P guard raises CERROR_ILL. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-03: TLBI_NH_ALL with S1P==0 must set CMDQ_CONS.ERR "
           "to CERROR_ILL per ARM §4.4.2.1. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: TLBI_NH_ASID with IDR0.S1P==0 → CERROR_ILL + GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.2.2 CMD_TLBI_NH_ASID: "If SMMU_IDR0.S1P==0, CERROR_ILL."
//
// BEFORE FIX: setS1PSupported() missing → compile error → FAILS.
//   Also: no S1P guard in TLBI_NH_ASID → silent execution if S1P could be cleared.
// AFTER FIX:  S1P guard added → CERROR_ILL → PASSES.
TEST(AuditNew03S1P, TlbiNhAsid_S1PDisabled_RaisesCerrorIll) {
    // §4.4.2.2: TLBI_NH_ASID with IDR0.S1P==0 must raise CERROR_ILL.
    // BEFORE FIX: setS1PSupported() missing → compile error.
    // AFTER FIX:  S1P guard in TLBI_NH_ASID handler → CERROR_ILL raised.
    SMMU smmu;
    enableSMMU(smmu);

    // BEFORE FIX: this line does not compile.
    smmu.setS1PSupported(false);

    smmu.clearEventQueue();

    submitTlbiNhAsid(smmu);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-03: TLBI_NH_ASID must toggle GERROR.CMDQ_ERR (bit 0) "
           "when IDR0.S1P==0. "
           "ARM §4.4.2.2: 'If SMMU_IDR0.S1P==0, CERROR_ILL.' "
           "Current code: TLBI_NH_ASID case has no S1P guard. "
           "BEFORE FIX: command executes silently. "
           "AFTER FIX:  S1P guard raises CERROR_ILL. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-03: TLBI_NH_ASID with S1P==0 must set CMDQ_CONS.ERR "
           "to CERROR_ILL per ARM §4.4.2.2. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: TLBI_NH_VA with IDR0.S1P==0 → CERROR_ILL + GERROR_CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.4.2.3 CMD_TLBI_NH_VA: "If SMMU_IDR0.S1P==0, CERROR_ILL."
//
// BEFORE FIX: setS1PSupported() missing → compile error → FAILS.
//   Also: no S1P guard in TLBI_NH_VA → silent execution (ril=false bypasses
//   the existing RIL check; the S1P check is entirely absent).
// AFTER FIX:  S1P guard added → CERROR_ILL → PASSES.
TEST(AuditNew03S1P, TlbiNhVa_S1PDisabled_RaisesCerrorIll) {
    // §4.4.2.3: TLBI_NH_VA with IDR0.S1P==0 must raise CERROR_ILL.
    // BEFORE FIX: setS1PSupported() missing → compile error.
    // AFTER FIX:  S1P guard in TLBI_NH_VA handler → CERROR_ILL raised.
    SMMU smmu;
    enableSMMU(smmu);

    // BEFORE FIX: this line does not compile.
    smmu.setS1PSupported(false);

    smmu.clearEventQueue();

    // Submit TLBI_NH_VA with ril=false to avoid triggering the existing RIL
    // reserved-param check before the S1P guard can be evaluated.
    submitTlbiNhVa(smmu);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-03: TLBI_NH_VA must toggle GERROR.CMDQ_ERR (bit 0) "
           "when IDR0.S1P==0. "
           "ARM §4.4.2.3: 'If SMMU_IDR0.S1P==0, CERROR_ILL.' "
           "Current code: TLBI_NH_VA case (smmu.cpp ~line 4614) has only the RIL "
           "reserved-param check but no S1P guard. "
           "BEFORE FIX: command executes silently (ril=false bypasses RIL check). "
           "AFTER FIX:  S1P guard raises CERROR_ILL before TLB op. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-03: TLBI_NH_VA with S1P==0 must set CMDQ_CONS.ERR "
           "to CERROR_ILL per ARM §4.4.2.3. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ============================================================================
// BUG-AUDIT-NEW-02: CFGI_CD / CFGI_CD_ALL global IDR0.S1P guard
// (ARM §4.3.3 line 5362 / §4.3.4 line 5388) — S1P==0 path
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: CFGI_CD with IDR0.S1P==0 → CERROR_ILL regardless of stream config
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.3 spec line 6605: "This command raises CERROR_ILL when stage 1 is
// not implemented (SMMU_IDR0.S1P == 0)."
//
// When IDR0.S1P==0, CFGI_CD must raise CERROR_ILL for ANY stream, including
// streams that have stage-1 enabled in their per-stream configuration.
// (Before the BUG-AUDIT-NEW-02 fix, the guard was per-stream, not global; a
// stage-1-enabled stream would therefore not raise CERROR_ILL.  The fix moves
// the guard to the SMMU-global IDR0.S1P bit.)
//
// BEFORE FIX: setS1PSupported() does not exist → compile error → FAILS.
//   Also: existing per-stream guard would not raise CERROR_ILL for a
//   stage-1-enabled stream even if S1P could be cleared.
// AFTER FIX:  IDR0.S1P==0 check applied universally → CERROR_ILL → PASSES.
TEST(AuditNew02CfgiCdGlobal, CfgiCd_S1PDisabled_RaisesCerrorIll) {
    // §4.3.3 / spec line 6605: CFGI_CD with IDR0.S1P==0 must raise CERROR_ILL
    // regardless of the target stream's stage-1 configuration.
    // BEFORE FIX: setS1PSupported() missing → compile error.
    // AFTER FIX:  IDR0.S1P==0 guard → CERROR_ILL raised.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x50u;

    // Configure a stage-1-enabled stream.  With the old per-stream guard,
    // this would NOT raise CERROR_ILL (stage1Enabled=true).  After the fix,
    // the guard checks the global IDR0.S1P==0 and raises CERROR_ILL for all
    // CFGI_CD commands regardless of per-stream stage-1 state.
    smmu.configureStream(sid, makeStage1Config());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    // BEFORE FIX: this line does not compile.
    smmu.setS1PSupported(false);

    submitCfgiCd(smmu, sid, /*pasid=*/0u);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-02: CFGI_CD must toggle GERROR.CMDQ_ERR (bit 0) when "
           "IDR0.S1P==0, regardless of the target stream's stage-1 configuration. "
           "ARM §4.3.3 spec line 6605: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented (SMMU_IDR0.S1P == 0).' "
           "The guard is SMMU-global, not per-stream. "
           "BEFORE FIX: setS1PSupported() missing; old per-stream guard would not "
           "raise CERROR_ILL for a stage-1-enabled stream. "
           "AFTER FIX:  IDR0.S1P==0 guard fires for all CFGI_CD commands. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-02: CFGI_CD with IDR0.S1P==0 must set CMDQ_CONS.ERR "
           "to CERROR_ILL per ARM §4.3.3. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: CFGI_CD_ALL with IDR0.S1P==0 → CERROR_ILL
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.3.4 line 5388 / spec line 6605: same IDR0.S1P guard applies to
// CFGI_CD_ALL.  When S1P==0, CFGI_CD_ALL must raise CERROR_ILL.
//
// BEFORE FIX: setS1PSupported() missing → compile error → FAILS.
// AFTER FIX:  IDR0.S1P==0 guard in CFGI_CD_ALL handler → CERROR_ILL → PASSES.
TEST(AuditNew02CfgiCdGlobal, CfgiCdAll_S1PDisabled_RaisesCerrorIll) {
    // §4.3.4 / spec line 6605: CFGI_CD_ALL with IDR0.S1P==0 must raise CERROR_ILL.
    // BEFORE FIX: setS1PSupported() missing → compile error.
    // AFTER FIX:  IDR0.S1P==0 global guard → CERROR_ILL raised.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x51u;

    // Configure a bypass stream.  The guard is IDR0.S1P-based (global), not
    // per-stream, so even a bypass stream must trigger CERROR_ILL when S1P==0.
    smmu.configureStream(sid, makeBypassConfig());
    smmu.enableStream(sid);

    smmu.clearEventQueue();

    // BEFORE FIX: this line does not compile.
    smmu.setS1PSupported(false);

    submitCfgiCdAll(smmu, sid);

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-NEW-02: CFGI_CD_ALL must toggle GERROR.CMDQ_ERR (bit 0) "
           "when IDR0.S1P==0. "
           "ARM §4.3.4 spec line 6605: 'This command raises CERROR_ILL when stage 1 "
           "is not implemented (SMMU_IDR0.S1P == 0).' "
           "The guard is SMMU-global (IDR0.S1P), not per-stream stage1Enabled. "
           "BEFORE FIX: setS1PSupported() missing. "
           "AFTER FIX:  IDR0.S1P==0 guard → CERROR_ILL. "
           "GERROR value: 0x" << std::hex << smmu.getGerror();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-AUDIT-NEW-02: CFGI_CD_ALL with IDR0.S1P==0 must set CMDQ_CONS.ERR "
           "to CERROR_ILL per ARM §4.3.4. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();
}
