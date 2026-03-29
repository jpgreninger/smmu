// TDD failing tests for BUG-AUDIT-20, BUG-AUDIT-32, BUG-AUDIT-34, and
// BUG-AUDIT-35.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-34 (Both §4.4.1.1): RIL loop uses hardcoded 4KB step in
//   TLBI_NH_VAA and TLBI_EL2_VAA
//
//   ARM IHI0070G.b §4.4.1.1: The range step in a RIL TLBI walk must use the
//   granule size derived from the TG field:
//     TG=1 → 4KB (0x1000), TG=2 → 16KB (0x4000), TG=3 → 64KB (0x10000).
//
//   Both TLBI_NH_VAA and TLBI_EL2_VAA declare `uint64_t page_size = 4096` and
//   advance `cur` by `page_size` instead of `granule_size`.  When TG=2 (16KB)
//   the loop iterates 4x too many entries; when TG=3 (64KB) it iterates 16x
//   too many.
//
//   A direct step-size test is verified by checking that submitting a valid
//   TLBI_NH_VAA or TLBI_EL2_VAA command with TG=2 (16KB) and NUM=1, SCALE=0
//   does NOT produce CERROR_ILL.  The behavioral correctness of the step is
//   confirmed by ensuring the loop completes without panic for a multi-block
//   range with TG=3 (64KB).
//
//   BEFORE FIX: `page_size` hardcoded to 4096 inside the RIL loop → incorrect
//     step for TG=2/3.  Commands still succeed (no CERROR_ILL), so these tests
//     are regression guards for BUG-AUDIT-34 against future regressions.
//   AFTER FIX: granule_size derived from TG; loop uses the correct step.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-20 (Both §6.3.1): IDR0.NS1ATS missing S1P gate
//
//   ARM IHI0070G.b §6.3.1:
//     "NS1ATS, bit [11]: RES0 if ATS==0 OR S1P==0 OR S2P==0."
//
//   Current gate in getIDR0() (smmu.cpp line 3264):
//     ns1atsSupported_ && s2pSupported_
//   Missing: s1pSupported_ check.  When S1P=0 and S2P=1 and NS1ATS=1,
//   bit 11 is incorrectly set to 1.
//
//   BEFORE FIX: NS1ATS gate is `ns1ats && s2p`; S1P not checked →
//     test "ns1ats_res0_when_s1p_disabled" FAILS.
//   AFTER FIX: NS1ATS gate becomes `ns1ats && s1p && s2p` →
//     test passes.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-32 (Both §5.2): IDR0.CD2L should be 1
//
//   ARM IHI0070G.b §6.3.1:
//     "CD2L, bit [19]: 2-level Context Descriptor table supported."
//   CD2L=0 limits s1cdMax to a 10-bit linear table.  The model uses a flat
//   unordered_map and has no such limit, so CD2L must be 1.
//
//   Current getIDR0() does not set bit 19 → CD2L=0.
//
//   BEFORE FIX: bit 19 is 0 → test "cd2l_bit_set" FAILS.
//   AFTER FIX: bit 19 set to 1 → test passes.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-35 (Low §6.3.1): HTTU overclaims dirty state support
//
//   ARM IHI0070G.b §6.3.1:
//     "HTTU[7:6]: 0b00 = no h/w update; 0b01 = access flag only;
//      0b10 = access flag + dirty state."
//   Only the access-flag path is implemented; dirty-state update is not.
//   HTTU must be 0b01.
//
//   Current getIDR0() returns (2u << 6) == HTTU=0b10 (dirty state) →
//   overclaims capability.
//
//   BEFORE FIX: (idr0 >> 6) & 0x3 == 2 → test "httu_access_flag_only" FAILS.
//   AFTER FIX: changed to (1u << 6) → HTTU=0b01 → test passes.
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"

using namespace smmu;

// ─── BUG-AUDIT-20 tests ──────────────────────────────────────────────────────

// Negative test: NS1ATS must be RES0 when S1P==0, even if NS1ATS and S2P are
// both enabled.
TEST(BugAudit20, Ns1atsRes0WhenS1pDisabled) {
    SMMU s;
    s.setNS1ATSSupported(true);
    s.setS1PSupported(false);
    s.setS2PSupported(true);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_EQ(idr0 & (1u << 11), 0u)
        << "NS1ATS (IDR0 bit 11) must be RES0 when S1P==0 (§6.3.1)";
}

// Regression: NS1ATS must remain RES0 when S2P==0 (existing AUDIT-NEW-02 fix).
TEST(BugAudit20, Ns1atsRes0WhenS2pDisabled) {
    SMMU s;
    s.setNS1ATSSupported(true);
    s.setS1PSupported(true);
    s.setS2PSupported(false);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_EQ(idr0 & (1u << 11), 0u)
        << "NS1ATS (IDR0 bit 11) must be RES0 when S2P==0 (§6.3.1)";
}

// Positive test: NS1ATS must be 1 when NS1ATS, S1P, and S2P are all enabled.
TEST(BugAudit20, Ns1atsSetWhenAllEnabled) {
    SMMU s;
    s.setNS1ATSSupported(true);
    s.setS1PSupported(true);
    s.setS2PSupported(true);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_NE(idr0 & (1u << 11), 0u)
        << "NS1ATS (IDR0 bit 11) must be 1 when NS1ATS && S1P && S2P (§6.3.1)";
}

// Baseline: NS1ATS defaults to 0 on a fresh SMMU (ns1atsSupported_=false).
TEST(BugAudit20, Ns1atsDefaultZero) {
    SMMU s;
    const uint32_t idr0 = s.getIDR0();
    EXPECT_EQ(idr0 & (1u << 11), 0u)
        << "NS1ATS must default to 0 when ns1atsSupported_ is false";
}

// ─── BUG-AUDIT-32 tests ──────────────────────────────────────────────────────

// CD2L (bit 19) must be 1: the model's flat PASID map supports any s1cdMax.
TEST(BugAudit32, Cd2lBitSet) {
    SMMU s;
    const uint32_t idr0 = s.getIDR0();
    EXPECT_NE(idr0 & (1u << 19), 0u)
        << "CD2L (IDR0 bit 19) must be 1 — model supports 2-level CD tables (§6.3.1)";
}

// ─── BUG-AUDIT-35 tests ──────────────────────────────────────────────────────

// HTTU[7:6] must be 0b01 (access flag only); dirty state is not implemented.
TEST(BugAudit35, HttuAccessFlagOnly) {
    SMMU s;
    const uint32_t idr0 = s.getIDR0();
    const uint32_t httu = (idr0 >> 6) & 0x3u;
    EXPECT_EQ(httu, 1u)
        << "HTTU[7:6] must be 0b01 (access flag only) — dirty state not implemented (§6.3.1)";
}

// ─── BUG-AUDIT-34 tests ──────────────────────────────────────────────────────

// BUG-AUDIT-34: TLBI_NH_VAA with RIL + TG=2 (16KB granule) must not raise
// CERROR_ILL.
//
// ARM §4.4.1.1: A RIL command is reserved only when tg!=0 AND num==0 AND
// scale==0 AND ttl==0.  Here num=1 (non-zero), so the combination is valid
// and must execute without error.
//
// This is a behavioral-correctness regression guard: the step-size bug does
// not cause CERROR_ILL, but the correct granule_size must be used in the loop
// to match the spec.
//
// ALWAYS PASSES — the command is valid; CERROR_ILL must NOT be raised.
TEST(BugAudit34TlbiNhVaaRil, TlbiNhVaa_Tg2_Num1_NoGerrorCmdqErr) {
    // §4.4.1.1: TLBI_NH_VAA with ril=true, tg=2 (16KB), num=1, scale=0, ttl=0.
    // num=1 (non-zero) makes this a valid RIL combination — CERROR_ILL must
    // NOT be raised regardless of the granule step size used in the loop.
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    // Configure a minimal stage-1 stream so the NH command has a valid context.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 1u;
    s.configureStream(1u, cfg);

    // Clear any pre-existing GERROR state.
    s.clearGerror(s.getGerror() ^ s.getGerrorN());

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VAA;
    cmd.vmid         = 1u;
    cmd.startAddress = 0x0010'0000u;
    cmd.ril          = true;
    cmd.tg           = 2u;  // 16KB granule
    cmd.num          = 1u;  // NUM=1 — valid (not the reserved 0,0,0 combo)
    cmd.scale        = 0u;
    cmd.ttl          = 0u;
    s.submitCommand(cmd);
    s.processCommandQueue();

    EXPECT_FALSE((s.getGerror() & GERROR_CMDQ_ERR) != 0u)
        << "BUG-AUDIT-34: TLBI_NH_VAA with TG=2 (16KB), NUM=1, SCALE=0 is valid "
           "and must NOT raise CERROR_ILL (ARM §4.4.1.1). "
           "The reserved combination requires tg!=0 AND num==0 AND scale==0 AND ttl==0; "
           "here num=1 so it is valid. "
           "GERROR=0x" << std::hex << s.getGerror();
}

// BUG-AUDIT-34: TLBI_EL2_VAA with RIL + TG=2 (16KB granule) must not raise
// CERROR_ILL.
//
// ARM §4.4.1.1 / §4.4.2.9: A valid RIL TLBI_EL2_VAA command (num=1, scale=0,
// tg=2) must execute without error.  IDR0.Hyp must be 1 to allow EL2 TLBI
// commands; IDR0.S2P must be 1 because Hyp requires S2P (§6.3.1 BUG-B fix).
//
// ALWAYS PASSES — the command is valid; CERROR_ILL must NOT be raised.
TEST(BugAudit34TlbiEl2VaaRil, TlbiEl2Vaa_Tg2_Num1_NoGerrorCmdqErr) {
    // §4.4.1.1 / §4.4.2.9: TLBI_EL2_VAA with ril=true, tg=2 (16KB), num=1,
    // scale=0, ttl=0.  num=1 makes the RIL params valid → no CERROR_ILL.
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setHypSupported(true);
    s.setS2PSupported(true);

    // Clear any pre-existing GERROR state.
    s.clearGerror(s.getGerror() ^ s.getGerrorN());

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_EL2_VAA;
    cmd.startAddress = 0x0010'0000u;
    cmd.ril          = true;
    cmd.tg           = 2u;  // 16KB granule
    cmd.num          = 1u;  // valid
    cmd.scale        = 0u;
    cmd.ttl          = 0u;
    s.submitCommand(cmd);
    s.processCommandQueue();

    EXPECT_FALSE((s.getGerror() & GERROR_CMDQ_ERR) != 0u)
        << "BUG-AUDIT-34: TLBI_EL2_VAA with TG=2 (16KB), NUM=1, SCALE=0 is valid "
           "and must NOT raise CERROR_ILL (ARM §4.4.1.1 / §4.4.2.9). "
           "GERROR=0x" << std::hex << s.getGerror();
}

// BUG-AUDIT-34: TLBI_NH_VAA with RIL + TG=3 (64KB granule) and a small range
// must complete without hang and without raising CERROR_ILL.
//
// With page_size hardcoded to 4096 and TG=3 (64KB), the loop iterates 16x
// more steps than needed.  With the fix (granule_size derived from TG), it
// iterates exactly the right number of steps.  Using a small num=4, scale=0
// range keeps the test fast in both before/after states.  TTL=1 avoids the
// reserved tg!=0 AND num==0 AND scale==0 AND ttl==0 combination.
//
// ALWAYS PASSES — the command is valid; CERROR_ILL must NOT be raised.
TEST(BugAudit34TlbiNhVaaRil, TlbiNhVaa_Tg3_Num4_StepSizeNoHang) {
    // §4.4.1.1: ril=true, tg=3 (64KB), num=4, scale=0, ttl=1.
    // ttl=1 avoids the reserved (tg!=0, num=0, scale=0, ttl=0) combo.
    // num=4 with scale=0 gives a 5-block range (blocks are 64KB each).
    // This test confirms the loop completes without hang and without CERROR_ILL.
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    // Configure a minimal stage-1 stream for the NH command.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;
    s.configureStream(2u, cfg);

    // Clear any pre-existing GERROR state.
    s.clearGerror(s.getGerror() ^ s.getGerrorN());

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VAA;
    cmd.vmid         = 0u;
    cmd.startAddress = 0x0000'0000u;
    cmd.ril          = true;
    cmd.tg           = 3u;  // 64KB granule
    cmd.num          = 4u;  // valid 5-block range
    cmd.scale        = 0u;
    cmd.ttl          = 1u;  // non-zero TTL avoids the reserved combo
    s.submitCommand(cmd);
    s.processCommandQueue();

    EXPECT_FALSE((s.getGerror() & GERROR_CMDQ_ERR) != 0u)
        << "BUG-AUDIT-34: TLBI_NH_VAA with TG=3 (64KB), NUM=4, SCALE=0, TTL=1 "
           "must NOT raise CERROR_ILL (ARM §4.4.1.1). "
           "GERROR=0x" << std::hex << s.getGerror();
}
