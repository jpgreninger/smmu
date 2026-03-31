// TDD failing tests for BUG-AUDIT-63 and BUG-AUDIT-64.
//
// BUG-AUDIT-63 (C++ §6.3.24/§6.3.25): STRTAB_BASE setters not guarded by SMMUEN.
//   Spec §6.3.24/§6.3.25 line 13807: SMMU_STRTAB_BASE and SMMU_STRTAB_BASE_CFG are
//   RO when CR0.SMMUEN==1. Writes while SMMUEN==1 must be silently IGNORED.
//   Affected setters: setStrtabFormat(), setStrtabSplit(), setStrtabLog2Size().
//   BEFORE FIX: setters apply the write even when SMMUEN=1 → tests FAIL.
//   AFTER FIX:  setters silently ignore writes when SMMUEN=1 → tests PASS.
//
// BUG-AUDIT-64 (C++ §9.1.3/§9.1.5): gatosTranslate() missing INV_STAGE for
//   bypass/disabled streams.
//   Spec §9.1.3 line 27699: GATOS for bypass (Config=0b100) or disabled/abort
//   (Config=0b0xx) must result in INV_STAGE: FAULT=1, FAULTCODE=0xFE.
//   BEFORE FIX: gatosTranslate() calls translate() which returns success/wrong code.
//   AFTER FIX:  gatosTranslate() pre-checks config and returns FAULT=1+FAULTCODE=0xFE.

#include <gtest/gtest.h>
#include <memory>
#include "smmu/smmu.h"
#include "smmu/address_space.h"

using namespace smmu;

// Helper: enable all queues and the SMMU global enable.
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);
    s.setS2PSupported(true);
}

// ─── BUG-AUDIT-63 tests — STRTAB setters must be ignored when SMMUEN=1 ─────────

// BUG-AUDIT-63 test 1: setStrtabFormat() must be silently ignored when SMMUEN=1.
//
// ARM §6.3.24/§6.3.25 line 13807: STRTAB_BASE_CFG.FMT is RO when SMMUEN=1.
// Write Linear before enable, then try to change to TwoLevel after enable.
// The value must remain Linear.
//
// BEFORE FIX: getStrtabFormat() returns TwoLevel (write applied) → FAILS.
// AFTER FIX:  getStrtabFormat() returns Linear (write ignored) → PASSES.
TEST(BugAudit63, SetStrtabFormatIgnoredWhenEnabled) {
    SMMU smmu;

    // Set Linear format before enabling — must be accepted.
    smmu.setStrtabFormat(StreamTableFormat::Linear);
    ASSERT_EQ(smmu.getStrtabFormat(), StreamTableFormat::Linear)
        << "BUG-AUDIT-63 precondition: setStrtabFormat(Linear) before enable must succeed.";

    // Enable SMMU.
    enableSMMU(smmu);

    // Attempt to change format while SMMUEN=1 — must be silently ignored.
    smmu.setStrtabFormat(StreamTableFormat::TwoLevel);

    StreamTableFormat fmt = smmu.getStrtabFormat();
    EXPECT_EQ(fmt, StreamTableFormat::Linear)
        << "BUG-AUDIT-63: setStrtabFormat(TwoLevel) must be silently ignored when "
           "SMMUEN=1. ARM §6.3.24/§6.3.25 line 13807: STRTAB_BASE_CFG is RO when "
           "SMMUEN=1. Current code applies the write unconditionally.";
}

// BUG-AUDIT-63 test 2: setStrtabSplit() must be silently ignored when SMMUEN=1.
//
// ARM §6.3.25 line 13807: STRTAB_BASE_CFG.SPLIT is RO when SMMUEN=1.
// Set split=8 before enable, then attempt to change to split=10 after enable.
// The value must remain 8.
//
// BEFORE FIX: getStrtabSplit() returns 10 (write applied) → FAILS.
// AFTER FIX:  getStrtabSplit() returns 8 (write ignored) → PASSES.
TEST(BugAudit63, SetStrtabSplitIgnoredWhenEnabled) {
    SMMU smmu;

    // Set split=8 before enabling — must be accepted.
    smmu.setStrtabSplit(8u);
    ASSERT_EQ(smmu.getStrtabSplit(), 8u)
        << "BUG-AUDIT-63 precondition: setStrtabSplit(8) before enable must succeed.";

    // Enable SMMU.
    enableSMMU(smmu);

    // Attempt to change split while SMMUEN=1 — must be silently ignored.
    smmu.setStrtabSplit(10u);

    uint8_t split = smmu.getStrtabSplit();
    EXPECT_EQ(split, 8u)
        << "BUG-AUDIT-63: setStrtabSplit(10) must be silently ignored when SMMUEN=1. "
           "ARM §6.3.25 line 13807: STRTAB_BASE_CFG.SPLIT is RO when SMMUEN=1. "
           "Current code applies the write unconditionally.";
}

// BUG-AUDIT-63 test 3: setStrtabLog2Size() must be silently ignored when SMMUEN=1.
//
// ARM §6.3.25 line 13807: STRTAB_BASE_CFG.LOG2SIZE is RO when SMMUEN=1.
// Set log2size=16 before enable, then attempt to change to 20 after enable.
// The value must remain 16.
//
// BEFORE FIX: getStrtabLog2Size() returns 20 (write applied) → FAILS.
// AFTER FIX:  getStrtabLog2Size() returns 16 (write ignored) → PASSES.
TEST(BugAudit63, SetStrtabLog2SizeIgnoredWhenEnabled) {
    SMMU smmu;

    // Set log2size=16 before enabling — must be accepted.
    smmu.setStrtabLog2Size(16u);
    ASSERT_EQ(smmu.getStrtabLog2Size(), 16u)
        << "BUG-AUDIT-63 precondition: setStrtabLog2Size(16) before enable must succeed.";

    // Enable SMMU.
    enableSMMU(smmu);

    // Attempt to change log2size while SMMUEN=1 — must be silently ignored.
    smmu.setStrtabLog2Size(20u);

    uint8_t log2sz = smmu.getStrtabLog2Size();
    EXPECT_EQ(log2sz, 16u)
        << "BUG-AUDIT-63: setStrtabLog2Size(20) must be silently ignored when SMMUEN=1. "
           "ARM §6.3.25 line 13807: STRTAB_BASE_CFG.LOG2SIZE is RO when SMMUEN=1. "
           "Current code applies the write unconditionally.";
}

// ─── BUG-AUDIT-64 tests — GATOS returns INV_STAGE for bypass/disabled streams ──

// BUG-AUDIT-64 test 4: gatosTranslate() must return INV_STAGE for a bypass stream.
//
// ARM §9.1.3 line 27699: GATOS on a bypass stream (Config=0b100, bypassEnabled=true)
// must return INV_STAGE: FAULT=1, FAULTCODE=0xFE (INV_STAGE).
//
// BEFORE FIX: gatosTranslate() calls translate() → returns identity-mapped PA (FAULT=0)
//             or a different fault code → test FAILS.
// AFTER FIX:  gatosTranslate() pre-checks config → returns FAULT=1+FAULTCODE=0xFE → PASSES.
TEST(BugAudit64, GatosTranslate_BypassStream_ReturnsInvStage) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 200u;

    // Configure stream as bypass (Config=0b100).
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "BUG-AUDIT-64 precondition: bypass stream configuration must succeed.";

    uint64_t par = smmu.gatosTranslate(sid, 0u, 0x1000u,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(par & 0x1u, 0x1u)
        << "BUG-AUDIT-64: gatosTranslate() for a bypass stream must return FAULT=1 "
           "(bit[0]=1). ARM §9.1.3 line 27699: bypass stream → INV_STAGE. "
           "Current code calls translate() which returns a successful PA. "
           "par=0x" << std::hex << par;

    uint64_t faultcode = (par >> 4u) & 0xFFu;
    EXPECT_EQ(faultcode, 0xFEu)
        << "BUG-AUDIT-64: gatosTranslate() for a bypass stream must return FAULTCODE=0xFE "
           "(INV_STAGE) in PAR bits[11:4]. ARM §9.1.3 line 27699. "
           "Current code returns FAULTCODE from translate() result. "
           "par=0x" << std::hex << par;
}

// BUG-AUDIT-64 test 5: gatosTranslate() must return INV_STAGE for a disabled stream.
//
// ARM §9.1.3 line 27699: GATOS on a disabled/abort stream (Config=0b0xx,
// stage1Enabled=false, stage2Enabled=false, bypassEnabled=false) must return
// INV_STAGE: FAULT=1, FAULTCODE=0xFE.
//
// BEFORE FIX: gatosTranslate() calls translate() → returns wrong fault code → FAILS.
// AFTER FIX:  gatosTranslate() pre-checks config → returns FAULT=1+FAULTCODE=0xFE → PASSES.
TEST(BugAudit64, GatosTranslate_DisabledStream_ReturnsInvStage) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 201u;

    // Configure stream as disabled/abort (all stages false, bypass false).
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "BUG-AUDIT-64 precondition: disabled stream configuration must succeed.";

    uint64_t par = smmu.gatosTranslate(sid, 0u, 0x2000u,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(par & 0x1u, 0x1u)
        << "BUG-AUDIT-64: gatosTranslate() for a disabled/abort stream must return "
           "FAULT=1 (bit[0]=1). ARM §9.1.3 line 27699: disabled stream → INV_STAGE. "
           "Current code calls translate() which returns wrong fault code. "
           "par=0x" << std::hex << par;

    uint64_t faultcode = (par >> 4u) & 0xFFu;
    EXPECT_EQ(faultcode, 0xFEu)
        << "BUG-AUDIT-64: gatosTranslate() for a disabled/abort stream must return "
           "FAULTCODE=0xFE (INV_STAGE) in PAR bits[11:4]. ARM §9.1.3 line 27699. "
           "Current code returns wrong faultcode. "
           "par=0x" << std::hex << par;
}

// BUG-AUDIT-64 test 6: gatosTranslate() must return INV_STAGE for an absent stream.
//
// ARM §9.1.3 line 27699: GATOS for a StreamID with no STE (absent stream) must
// return INV_STAGE: FAULT=1, FAULTCODE=0xFE.
//
// BEFORE FIX: gatosTranslate() calls translate() which emits C_BAD_STREAMID and
//             returns a fault with a different FAULTCODE (not 0xFE) → test FAILS.
// AFTER FIX:  gatosTranslate() pre-checks streamMap → returns FAULT=1+FAULTCODE=0xFE
//             before calling translate() → PASSES.
TEST(BugAudit64, GatosTranslate_AbsentStream_ReturnsInvStage) {
    SMMU smmu;
    enableSMMU(smmu);

    // StreamID 999 has no configured stream.
    const StreamID sid = 999u;

    uint64_t par = smmu.gatosTranslate(sid, 0u, 0x3000u,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(par & 0x1u, 0x1u)
        << "BUG-AUDIT-64: gatosTranslate() for an absent stream must return FAULT=1. "
           "ARM §9.1.3 line 27699: absent stream → INV_STAGE. "
           "par=0x" << std::hex << par;

    uint64_t faultcode = (par >> 4u) & 0xFFu;
    EXPECT_EQ(faultcode, 0xFEu)
        << "BUG-AUDIT-64: gatosTranslate() for an absent stream must return "
           "FAULTCODE=0xFE (INV_STAGE) in PAR bits[11:4]. ARM §9.1.3 line 27699. "
           "Current code calls translate() which returns C_BAD_STREAMID faultcode. "
           "par=0x" << std::hex << par;
}

// BUG-AUDIT-64 test 7 (regression): gatosTranslate() for a stage1-only stream with
// a valid mapping must NOT return FAULTCODE=0xFE (INV_STAGE).
//
// Stage1-only streams are active translation streams — they must proceed through
// translate() normally and return success (FAULT=0) or a legitimate fault code,
// not INV_STAGE.
//
// BEFORE / AFTER FIX: stage1-only valid stream → FAULT=0 → PASSES.
TEST(BugAudit64, GatosTranslate_Stage1Stream_Proceeds) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 202u;
    const IOVA     iova = 0x4000u;
    const PA       pa   = 0xB000u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0u;  // sentinel — accept any IOVA
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "BUG-AUDIT-64 regression: stage1 stream configuration must succeed.";
    ASSERT_TRUE(smmu.enableStream(sid).isOk())
        << "BUG-AUDIT-64 regression: enableStream must succeed.";
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk())
        << "BUG-AUDIT-64 regression: createStreamPASID must succeed.";

    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, pa, ro, SecurityState::NonSecure).isOk())
        << "BUG-AUDIT-64 regression: mapPage must succeed.";

    uint64_t par = smmu.gatosTranslate(sid, 0u, iova,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    // Must not be INV_STAGE (FAULTCODE=0xFE).
    uint64_t faultcode = (par >> 4u) & 0xFFu;
    EXPECT_NE(faultcode, 0xFEu)
        << "BUG-AUDIT-64 regression: gatosTranslate() for an active stage1 stream "
           "must NOT return FAULTCODE=0xFE (INV_STAGE). ARM §9.1.3 line 27699: "
           "INV_STAGE applies only to bypass/disabled/absent streams. "
           "par=0x" << std::hex << par;

    // Translation must succeed (FAULT=0).
    EXPECT_EQ(par & 0x1u, 0u)
        << "BUG-AUDIT-64 regression: gatosTranslate() for a valid stage1 mapping "
           "must return FAULT=0. par=0x" << std::hex << par;
}
