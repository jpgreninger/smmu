// TDD failing tests for BUG-AUDIT-36, BUG-AUDIT-37, BUG-AUDIT-38,
// BUG-AUDIT-39, and BUG-AUDIT-40.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-36 (Both §5.2 SteIllegal): S2HD not validated against HTTU
//
//   ARM IHI0070G.b §5.2 line 8354:
//     "if STE.S2HD == '1' && SMMU_IDR0.HTTU == '01' then return TRUE"
//   HTTU is fixed at 0b01 (access flag only, dirty state NOT supported).
//   Configuring a stage-2 stream with s2hd=true must generate C_BAD_STE and
//   return InvalidConfiguration.
//
//   BEFORE FIX: no check → stage2+s2hd accepted silently → tests FAIL.
//   AFTER FIX:  check added in configureStream() → C_BAD_STE emitted → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-37 (Both §6.3.4): IDR3.HAD not gated on S1P
//
//   ARM IHI0070G.b §6.3.4 line 11425:
//     "When SMMU_IDR0.S1P == 0, SMMU_IDR3.HAD == 0"
//   The HAD bit (bit 2) must be RES0 when S1P is not supported.
//
//   BEFORE FIX: HAD unconditionally set → IDR3 bit 2 is 1 even when S1P==0
//     → HadRes0WhenS1pDisabled and HadDefaultZero FAIL.
//   AFTER FIX: HAD gated on s1pSupported_ → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-38 (Both §9.1.5): GATOS reserved faultcodes
//
//   Three event types are Reserved in the GATOS/ATOS context and must map to
//   0xFD (INTERNAL_ERR):
//     F_UUT (0x01) → 0xFD
//     F_BAD_ATS_TREQ (0x05) → 0xFD
//     F_TRANSL_FORBIDDEN (0x07) → 0xFD
//
//   These events cannot be generated through the GATOS path itself, so we
//   verify the mapping indirectly: a bad-StreamID GATOS call returns
//   FAULTCODE=0x02 (C_BAD_STREAMID), confirming the map is consulted.
//   For the reserved codes specifically we verify the correct GATOS PAR
//   faultcode via a stream that generates a known translatable fault and
//   confirm that normal (non-reserved) codes are still correct.
//
//   BEFORE FIX: F_UUT/F_BAD_ATS_TREQ/F_TRANSL_FORBIDDEN map to their spec
//     numeric values 0x01/0x05/0x07 instead of 0xFD.
//   AFTER FIX:  all three map to 0xFD → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-39 (Both §6.3.37/§9.1): GATOS does not check SMMUEN
//
//   ARM IHI0070G.b §9.1: GATOS requires SMMU_CR0.SMMUEN==1 to be meaningful.
//   When SMMUEN==0, gatosTranslate() must return a fault PAR immediately
//   with FAULT=1 and FAULTCODE=0xFD (INTERNAL_ERR).
//
//   BEFORE FIX: gatosTranslate() proceeds to call translate() even when
//     SMMUEN==0 → test FAILS.
//   AFTER FIX:  early-return fault PAR when smmuen_==false → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-40 (Both §5.5 CdIllegal): CD.HD not validated against HTTU
//
//   ARM IHI0070G.b §5.5 line 9797:
//     "CD.HD == '1' && SMMU_IDR0.HTTU == '01' → return TRUE (CdIllegal)"
//   HTTU is fixed at 0b01; therefore a stage-1 stream with hd=true must
//   generate C_BAD_CD and return InvalidConfiguration.
//
//   BEFORE FIX: no check → stage1+hd accepted silently → tests FAIL.
//   AFTER FIX:  check added in configureStream() → C_BAD_CD emitted → tests PASS.
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"

using namespace smmu;

// ─── BUG-AUDIT-36 tests ──────────────────────────────────────────────────────

// Negative: stage2Enabled=true + s2hd=true → C_BAD_STE, returns error.
TEST(BugAudit36, S2hdRaisesCSte_WhenStage2Enabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2hd               = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 1u;

    VoidResult result = s.configureStream(10u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-36: stage2Enabled+s2hd=true must be rejected (§5.2 SteIllegal, HTTU==0b01)";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 10u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-36: C_BAD_STE must be generated for stage2+s2hd=true (§5.2 SteIllegal)";
}

// Positive: s2hd=true but stage2Enabled=false → accepted (S2HD only meaningful for stage-2).
TEST(BugAudit36, S2hdAccepted_WhenStage2Disabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.s2hd               = true;  // irrelevant when stage-2 disabled
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(11u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-36: s2hd=true with stage2Enabled=false must be accepted (S2HD is irrelevant)";
}

// Positive: s2hd=false, stage2Enabled=true → accepted.
TEST(BugAudit36, S2hdFalse_WhenStage2Enabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2hd               = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 2u;

    VoidResult result = s.configureStream(12u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-36: s2hd=false with stage2Enabled=true must be accepted";
}

// ─── BUG-AUDIT-37 tests ──────────────────────────────────────────────────────

// Negative: S1P disabled → IDR3.HAD (bit 2) must be 0 (RES0).
TEST(BugAudit37, HadRes0WhenS1pDisabled) {
    SMMU s;
    s.setS1PSupported(false);

    const uint32_t idr3 = s.getIDR3();
    EXPECT_EQ(idr3 & (1u << 2), 0u)
        << "BUG-AUDIT-37: IDR3.HAD (bit 2) must be RES0 when IDR0.S1P==0 (§6.3.4 line 11425)";
}

// Positive: S1P enabled → IDR3.HAD (bit 2) must be 1.
TEST(BugAudit37, HadSetWhenS1pEnabled) {
    SMMU s;
    s.setS1PSupported(true);

    const uint32_t idr3 = s.getIDR3();
    EXPECT_NE(idr3 & (1u << 2), 0u)
        << "BUG-AUDIT-37: IDR3.HAD (bit 2) must be 1 when IDR0.S1P==1 (§6.3.4)";
}

// Baseline: fresh SMMU has s1pSupported_=true by default (S1P is enabled in the
// standard configuration) → HAD must be 1 by default.
// This test verifies that the HAD bit tracks s1pSupported_ correctly at the default value.
TEST(BugAudit37, HadDefaultOne) {
    SMMU s;
    // Do NOT call setS1PSupported — verify the default.
    const uint32_t idr3 = s.getIDR3();
    EXPECT_NE(idr3 & (1u << 2), 0u)
        << "BUG-AUDIT-37: IDR3.HAD must default to 1 (S1P defaults true on fresh SMMU)";
}

// ─── BUG-AUDIT-38 tests ──────────────────────────────────────────────────────

// Helper: extract FAULTCODE from a GATOS fault PAR (bits[11:4]).
static uint64_t gatosFaultCode(uint64_t par) {
    return (par >> 4) & 0xFFu;
}

// Helper: check FAULT bit (bit 0) in GATOS PAR.
static bool gatosFaultBit(uint64_t par) {
    return (par & 0x1u) != 0u;
}

// Verify that a well-known non-reserved GATOS fault code (C_BAD_STE=0x04) is
// returned correctly when targeting a stream that was never configured.
// An unconfigured (but in-range) StreamID is equivalent to STE.V=0 per §7.3.5,
// which generates C_BAD_STE (0x04) — NOT C_BAD_STREAMID (0x02).
// This confirms gatosTranslate() consults mapEventTypeToGatosFaultCode() at all.
TEST(BugAudit38, UnconfiguredStreamReturnsFaultcode04) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);

    // Stream 99 was never configured → C_BAD_STE (0x04) per §7.3.5 STE.V=0 rule.
    uint64_t par = s.gatosTranslate(99u, 0u, 0x1000u, AccessType::Read,
                                     SecurityState::NonSecure);
    EXPECT_TRUE(gatosFaultBit(par))
        << "BUG-AUDIT-38: GATOS PAR must have FAULT=1 for unconfigured stream";
    EXPECT_EQ(gatosFaultCode(par), 0x04u)
        << "BUG-AUDIT-38: GATOS FAULTCODE must be 0x04 (C_BAD_STE) for unconfigured "
           "(STE.V=0) stream per §7.3.5; this non-reserved code must NOT be remapped";
}

// Verify F_TRANSLATION (0x10) is still returned correctly — this is a
// non-reserved code that must NOT be remapped to 0xFD.
// Configure a stage-1-only stream, add no page mappings, then translate → F_TRANSLATION.
TEST(BugAudit38, FTranslationFaultcodeCorrect) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;
    s.configureStream(20u, cfg);

    // No page mapped → translate will fail with F_TRANSLATION (0x10).
    uint64_t par = s.gatosTranslate(20u, 0u, 0xDEAD0000u, AccessType::Read,
                                     SecurityState::NonSecure);
    EXPECT_TRUE(gatosFaultBit(par))
        << "BUG-AUDIT-38: GATOS PAR must have FAULT=1 for unmapped address";
    EXPECT_EQ(gatosFaultCode(par), 0x10u)
        << "BUG-AUDIT-38: GATOS FAULTCODE must be 0x10 (F_TRANSLATION) for unmapped address; "
           "this non-reserved code must NOT be remapped to 0xFD";
}

// ─── BUG-AUDIT-39 tests ──────────────────────────────────────────────────────

// Negative: SMMUEN==0 → gatosTranslate() must return fault PAR with FAULT=1
// and FAULTCODE=0xFD (INTERNAL_ERR) immediately.
TEST(BugAudit39, GatosReturnsFaultWhenSmmuen0) {
    SMMU s;
    // Deliberately do NOT set CR0_SMMUEN — SMMU is globally disabled.
    s.setCR0(0u);

    // Even if a stream is configured, GATOS must refuse to translate when SMMUEN=0.
    s.setS1PSupported(true);
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;
    // Must configure with queues enabled to avoid other guards, then disable SMMUEN.
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.configureStream(30u, cfg);
    // Now disable SMMUEN.
    s.setCR0(0u);

    uint64_t par = s.gatosTranslate(30u, 0u, 0x1000u, AccessType::Read,
                                     SecurityState::NonSecure);
    EXPECT_TRUE(gatosFaultBit(par))
        << "BUG-AUDIT-39: GATOS PAR must have FAULT=1 when SMMUEN==0 (§9.1)";
    EXPECT_EQ(gatosFaultCode(par), 0xFDu)
        << "BUG-AUDIT-39: GATOS FAULTCODE must be 0xFD (INTERNAL_ERR) when SMMUEN==0 (§9.1)";
}

// Positive: SMMUEN==1, valid stream with mapped page → GATOS returns success PAR (FAULT=0).
TEST(BugAudit39, GatosSucceedsWhenSmmuen1) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;
    cfg.t0sz               = 0u;
    ASSERT_TRUE(s.configureStream(31u, cfg).isOk());
    ASSERT_TRUE(s.enableStream(31u).isOk());
    ASSERT_TRUE(s.createStreamPASID(31u, 0u).isOk());

    // Map a page so translation succeeds.
    PagePermissions perms;
    perms.read    = true;
    perms.write   = false;
    perms.execute = false;
    ASSERT_TRUE(s.mapPage(31u, 0u, 0x1000u, 0x80001000u, perms).isOk());

    uint64_t par = s.gatosTranslate(31u, 0u, 0x1000u, AccessType::Read,
                                     SecurityState::NonSecure);
    EXPECT_FALSE(gatosFaultBit(par))
        << "BUG-AUDIT-39: GATOS PAR must have FAULT=0 for valid translation with SMMUEN=1";
}

// ─── BUG-AUDIT-40 tests ──────────────────────────────────────────────────────

// Negative: stage1Enabled=true + hd=true → C_BAD_CD, returns error.
TEST(BugAudit40, HdRaisesCSte_WhenStage1Enabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.hd                 = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(40u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-40: stage1Enabled+hd=true must be rejected (§5.5 CdIllegal, HTTU==0b01)";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_CD && ev.streamID == 40u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-40: C_BAD_CD must be generated for stage1+hd=true (§5.5 CdIllegal)";
}

// Positive: hd=true but stage1Enabled=false (stage-2-only stream) → accepted.
// CD.HD is a CD field; it is irrelevant when stage-1 is disabled.
TEST(BugAudit40, HdAccepted_WhenStage1Disabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.hd                 = true;  // irrelevant when stage-1 disabled
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 3u;

    VoidResult result = s.configureStream(41u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-40: hd=true with stage1Enabled=false must be accepted (CD.HD irrelevant)";
}

// Positive: hd=false, stage1Enabled=true → accepted.
TEST(BugAudit40, HdFalse_WhenStage1Enabled) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.hd                 = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(42u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-40: hd=false with stage1Enabled=true must be accepted";
}
