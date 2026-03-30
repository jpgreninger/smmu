// TDD failing tests for BUG-AUDIT-41, BUG-AUDIT-42, and BUG-AUDIT-43.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-41 (Both §6.3.1): IDR0.Hyp missing S1P gate
//
//   ARM IHI0070G.b §6.3.1:
//     IDR0.Hyp (bit 9) must be RES0 when S1P==0 OR S2P==0.
//   Current code gates only on (hypSupported_ && s2pSupported_).
//   Missing gate: s1pSupported_.
//
//   BEFORE FIX: hyp=true, s2p=true, s1p=false → bit 9 is 1 → tests FAIL.
//   AFTER FIX:  bit 9 must be 0 when s1p=false → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-42 (Both §5.2 SteIllegal): VMSAv8-32 not validated against TTF
//
//   ARM IHI0070G.b §5.2 SteIllegal():
//     "if STE.S2AA64=='0' && TTF[0]=='0' then return TRUE"
//   TTF is hardcoded 0b10 (AArch64-only), meaning TTF[0]==0 always.
//   Therefore any stage-2 stream with s2aa64=false must be rejected with
//   C_BAD_STE.
//
//   BEFORE FIX: no check → stage2+s2aa64=false accepted silently → tests FAIL.
//   AFTER FIX:  C_BAD_STE emitted, Err returned → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-43 (Both §5.2 SteIllegal): S1P/S2P capability not checked
//
//   ARM IHI0070G.b §5.2 SteIllegal():
//     "if Config=='1x1' && S1P=='0' then return TRUE"   → C_BAD_STE
//     "if Config=='11x' && S2P=='0' then return TRUE"   → C_BAD_STE
//   ('1x1' means stage1 enabled; '11x' means stage2 enabled)
//
//   BEFORE FIX: no S1P/S2P capability-vs-config check → silently accepted → FAIL.
//   AFTER FIX:  C_BAD_STE emitted when capability absent → PASS.
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"

using namespace smmu;

// ─── BUG-AUDIT-41 tests ──────────────────────────────────────────────────────

// Negative: hyp=true, s2p=true, s1p=false → IDR0.Hyp (bit 9) must be 0 (RES0).
// The spec states Hyp is RES0 when S1P==0 OR S2P==0.
// The current code is missing the s1p gate.
TEST(BugAudit41, HypRes0WhenS1pDisabled) {
    SMMU s;
    s.setHypSupported(true);
    s.setS2PSupported(true);
    s.setS1PSupported(false);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_EQ(idr0 & (1u << 9), 0u)
        << "BUG-AUDIT-41: IDR0.Hyp (bit 9) must be RES0 when IDR0.S1P==0 (ARM §6.3.1)";
}

// Positive: hyp=true, s2p=true, s1p=true → IDR0.Hyp (bit 9) must be 1.
// All three capability gates active — Hyp should be reported.
TEST(BugAudit41, HypSetWhenAllCapabilitiesEnabled) {
    SMMU s;
    s.setHypSupported(true);
    s.setS2PSupported(true);
    s.setS1PSupported(true);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_NE(idr0 & (1u << 9), 0u)
        << "BUG-AUDIT-41: IDR0.Hyp (bit 9) must be 1 when hyp=true, s1p=true, s2p=true (ARM §6.3.1)";
}

// Positive: hyp=false → IDR0.Hyp must be 0 regardless of S1P/S2P.
// This is the existing hypSupported_ gate (regression guard).
TEST(BugAudit41, HypRes0WhenHypDisabled) {
    SMMU s;
    s.setHypSupported(false);
    s.setS1PSupported(true);
    s.setS2PSupported(true);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_EQ(idr0 & (1u << 9), 0u)
        << "BUG-AUDIT-41: IDR0.Hyp must be 0 when hypSupported_=false (existing gate, regression guard)";
}

// Positive: s2p=false → IDR0.Hyp must be 0 regardless of hyp/s1p.
// This is the existing s2p gate (regression guard).
TEST(BugAudit41, HypRes0WhenS2pDisabled) {
    SMMU s;
    s.setHypSupported(true);
    s.setS1PSupported(true);
    s.setS2PSupported(false);

    const uint32_t idr0 = s.getIDR0();
    EXPECT_EQ(idr0 & (1u << 9), 0u)
        << "BUG-AUDIT-41: IDR0.Hyp must be 0 when s2pSupported_=false (existing gate, regression guard)";
}

// ─── BUG-AUDIT-42 tests ──────────────────────────────────────────────────────

// Negative: stage2Enabled=true, s2aa64=false → C_BAD_STE, returns error.
// TTF is hardcoded 0b10 (AArch64 only), so TTF[0]=0.
// ARM §5.2: "if STE.S2AA64=='0' && TTF[0]=='0' then SteIllegal → C_BAD_STE".
TEST(BugAudit42, S2aa64FalseRaisesCSteWhenStage2Enabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2aa64             = false;  // VMSAv8-32 stage-2 requested
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 1u;

    VoidResult result = s.configureStream(50u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-42: stage2Enabled=true + s2aa64=false must be rejected "
           "(ARM §5.2 SteIllegal: S2AA64==0 && TTF[0]==0)";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 50u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-42: C_BAD_STE must be generated for stage2+s2aa64=false (ARM §5.2 SteIllegal)";
}

// Positive: stage2Enabled=true, s2aa64=true → accepted.
// AArch64 stage-2 tables are always supported (TTF=0b10 covers AArch64 S1+S2).
TEST(BugAudit42, S2aa64TrueIsAcceptedWhenStage2Enabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2aa64             = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 2u;

    VoidResult result = s.configureStream(51u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-42: stage2Enabled=true + s2aa64=true must be accepted (AArch64 stage-2 is supported)";
}

// Positive: stage2Enabled=false, s2aa64=false → accepted.
// S2AA64 is irrelevant when stage-2 is disabled; no C_BAD_STE should fire.
TEST(BugAudit42, S2aa64FalseIsAcceptedWhenStage2Disabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.s2aa64             = false;  // irrelevant when stage-2 disabled
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(52u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-42: s2aa64=false with stage2Enabled=false must be accepted "
           "(S2AA64 is irrelevant when stage-2 disabled)";
}

// ─── BUG-AUDIT-43 tests ──────────────────────────────────────────────────────

// Negative: s1pSupported=false, stage1Enabled=true → C_BAD_STE.
// ARM §5.2: "if Config=='1x1' && S1P=='0' then SteIllegal → C_BAD_STE"
// (Config='1x1' means stage1 is enabled in the STE config field.)
TEST(BugAudit43, S1pAbsentStage1EnabledRaisesCSte) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(false);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(60u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-43: stage1Enabled=true + S1P==0 must be rejected "
           "(ARM §5.2 SteIllegal Config=='1x1' && S1P=='0')";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 60u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-43: C_BAD_STE must be generated when S1P==0 and stage1Enabled=true";
}

// Negative: s2pSupported=false, stage2Enabled=true → C_BAD_STE.
// ARM §5.2: "if Config=='11x' && S2P=='0' then SteIllegal → C_BAD_STE"
// (Config='11x' means stage2 is enabled in the STE config field.)
TEST(BugAudit43, S2pAbsentStage2EnabledRaisesCSte) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(false);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 3u;

    VoidResult result = s.configureStream(61u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-43: stage2Enabled=true + S2P==0 must be rejected "
           "(ARM §5.2 SteIllegal Config=='11x' && S2P=='0')";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 61u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-43: C_BAD_STE must be generated when S2P==0 and stage2Enabled=true";
}

// Positive: s1pSupported=false, stage1Enabled=false → accepted (no false positive).
// When stage-1 is not requested, the S1P capability is irrelevant.
TEST(BugAudit43, S1pAbsentStage1DisabledIsAccepted) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(false);
    s.setS2PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 4u;

    VoidResult result = s.configureStream(62u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-43: S1P==0 with stage1Enabled=false must be accepted "
           "(S1P capability irrelevant when stage-1 not configured)";
}

// Positive: s2pSupported=false, stage2Enabled=false → accepted (no false positive).
// When stage-2 is not requested, the S2P capability is irrelevant.
TEST(BugAudit43, S2pAbsentStage2DisabledIsAccepted) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(false);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(63u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-43: S2P==0 with stage2Enabled=false must be accepted "
           "(S2P capability irrelevant when stage-2 not configured)";
}
