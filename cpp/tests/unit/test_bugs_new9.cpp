// TDD failing tests for BUG-A, BUG-B, BUG-C1, BUG-C2, BUG-C3, BUG-D, BUG-G.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-A (§6.3.1): IDR0.VMW (bit 17) must be RES0 when IDR0.S2P==0.
//   VMW (VMID Wildcard) is a stage-2 feature.  When stage-2 translation is
//   not supported (S2P=0), the VMW capability is undefined and the bit must
//   read as zero.
//   Current code: bit 17 is hardcoded to 1 unconditionally.
//   BEFORE FIX: getIDR0() & (1<<17) == 1 even when S2P=0 → test FAILS.
//   AFTER FIX:  getIDR0() & (1<<17) == 0 when S2P=0 → test PASSES.
//
// BUG-B (§6.3.1): IDR0.Hyp (bit 9) must be RES0 when IDR0.S2P==0.
//   The Hypervisor extension (Hyp) requires stage-2 support; advertising Hyp
//   without S2P is architecturally incoherent and the bit must read as zero.
//   Current code: setHypSupported(true) sets bit 9 independently of S2P.
//   BEFORE FIX: getIDR0() & (1<<9) == 1 even when S2P=0 → test FAILS.
//   AFTER FIX:  getIDR0() & (1<<9) == 0 when S2P=0 → test PASSES.
//
// BUG-C1 (§5.5): STALL_MODEL==0b10 (forced-stall) requires STE.S2S==1 for
//   stage-2-enabled streams.  When stage2Enabled=true AND s2s=false the
//   configuration is illegal → C_BAD_STE must be emitted.
//   Current code: only checks STALL_MODEL==0b01 (terminate-only) + s2s=true.
//   BEFORE FIX: configureStream succeeds with no event → test FAILS.
//   AFTER FIX:  C_BAD_STE event recorded → test PASSES.
//
// BUG-C2 (§5.5): STALL_MODEL==0b10 (forced-stall) requires CD.S==1
//   (faultMode==FaultMode::Stall) for stage-1-enabled streams.  When
//   stage1Enabled=true AND faultMode==Terminate the configuration is illegal
//   → C_BAD_CD must be emitted.
//   Current code: no check for STALL_MODEL==0b10.
//   BEFORE FIX: configureStream succeeds with no event → test FAILS.
//   AFTER FIX:  C_BAD_CD event recorded → test PASSES.
//
// BUG-C3 (§5.5): STALL_MODEL!=0b00 + STE.S1STALLD==1 is an illegal
//   combination.  When stall is required (STALL_MODEL==0b01 or 0b10) but the
//   STE disables stall for stage-1, the STE is misconfigured → C_BAD_STE.
//   Current code: no check for this combination.
//   BEFORE FIX: configureStream succeeds with no event → test FAILS.
//   AFTER FIX:  C_BAD_STE event recorded → test PASSES.
//
// BUG-D (ARM IHI0070G.b §4.1.6 re-evaluation): SSec field is RES0 in the
//   encodings of TLBI_NH_ALL, TLBI_NH_ASID, TLBI_NH_VA, TLBI_NH_VAA,
//   TLBI_S12_VMALL, TLBI_S2_IPA, and TLBI_NSNH_ALL.  Because the SSec bit is
//   RES0 in those command encodings, setting it to 1 is CONSTRAINED UNPREDICTABLE
//   behaviour — NOT a defined CERROR_ILL path.  The correct model behaviour is
//   to execute normally (or silently ignore the reserved bit) without raising
//   GERROR.CMDQ_ERR.
//   The BUG-NEW-38 fix (test_bugs_new8.cpp) incorrectly added SSec guards to
//   these seven commands.
//   BEFORE FIX (= after BUG-NEW-38 fix): GERROR.CMDQ_ERR is set → test FAILS.
//   AFTER FIX:  commands execute normally, GERROR.CMDQ_ERR not set → PASSES.
//   (ATC_INV is excluded: it has a defined SSec field and the guard is correct.)
//
// BUG-G (§6.3.4): IDR3.MPAM is at bit [7], not bit [6].
//   The CFGI_VMS_PIDM handler currently checks bit 6; the spec places MPAM at
//   bit 7 (IDR3[7]).  Bit 6 is RES0.
//   Since this model always returns MPAM=0 (not supported), the check currently
//   fires correctly (CERROR_ILL always), but for the wrong reason.  The test
//   documents the bit-position discrepancy by verifying the IDR3 return value
//   and that the CFGI_VMS_PIDM CERROR_ILL guard uses the correct bit when MPAM
//   is definitionally absent.  A separate test verifies that bit 6 of IDR3 is
//   always 0 (RES0 per spec).
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

static CommandEntry makeCmd(CommandType type) {
    CommandEntry cmd;
    cmd.type         = type;
    cmd.cs           = 0u;
    cmd.ssec         = false;
    cmd.streamID     = 0u;
    cmd.pasid        = 0u;
    cmd.asid         = 0u;
    cmd.vmid         = 0u;
    cmd.startAddress = 0u;
    return cmd;
}

// Returns true when GERROR.CMDQ_ERR is active.
// getGerror() returns the active bits (GERROR XOR GERRORN already applied).
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
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

// Build a minimal stage-1-only stream config.
static StreamConfig makeStage1Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    return cfg;
}

// Build a minimal stage-2-only stream config.
static StreamConfig makeStage2Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    return cfg;
}

} // anonymous namespace

// ============================================================================
// BUG-A: IDR0.VMW (bit 17) must be RES0 when IDR0.S2P==0 (§6.3.1)
// ============================================================================

// ARM §6.3.1: VMW (VMID wildcard in CR0) is a stage-2 feature.  When S2P=0
// (stage-2 not supported), VMW is meaningless; the bit must read as 0 (RES0).
//
// BEFORE FIX: bit 17 is hardcoded to 1 in getIDR0() regardless of S2P → FAILS.
// AFTER FIX:  bit 17 is gated on s2pSupported_ (0 when S2P=0) → PASSES.
TEST(BugNewA_IDR0_VMW, VMW_RES0_WhenS2PDisabled) {
    // §6.3.1: VMW (bit 17) must be RES0 when IDR0.S2P==0.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setS2PSupported(false);

    uint32_t idr0 = smmu.getIDR0();

    // S2P (bit 0) must be 0 — prerequisite.
    ASSERT_EQ(idr0 & (1u << 0u), 0u)
        << "BUG-A prerequisite: IDR0.S2P must be 0 after setS2PSupported(false)";

    // VMW (bit 17) must be 0 when S2P==0.
    EXPECT_EQ(idr0 & (1u << 17u), 0u)
        << "BUG-A: IDR0.VMW (bit 17) must be RES0 when IDR0.S2P==0 (ARM §6.3.1). "
           "Got IDR0=0x" << std::hex << idr0;
}

// Secondary check: VMW must still be 1 when S2P is enabled (no regression).
TEST(BugNewA_IDR0_VMW, VMW_Set_WhenS2PEnabled) {
    // §6.3.1: VMW (bit 17) must be reported when S2P=1.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setS2PSupported(true);

    uint32_t idr0 = smmu.getIDR0();

    // S2P (bit 0) must be 1 — prerequisite.
    ASSERT_NE(idr0 & (1u << 0u), 0u)
        << "BUG-A prerequisite: IDR0.S2P must be 1 after setS2PSupported(true)";

    // VMW (bit 17) must be 1 when S2P==1.
    EXPECT_NE(idr0 & (1u << 17u), 0u)
        << "BUG-A regression: IDR0.VMW (bit 17) must be 1 when IDR0.S2P==1 (ARM §6.3.1)";
}

// ============================================================================
// BUG-B: IDR0.Hyp (bit 9) must be RES0 when IDR0.S2P==0 (§6.3.1)
// ============================================================================

// ARM §6.3.1: The Hypervisor extension (Hyp) requires stage-2 support.
// When S2P=0, Hyp cannot function and bit 9 must read as 0 (RES0).
//
// BEFORE FIX: setHypSupported(true) sets bit 9 independently of S2P → FAILS.
// AFTER FIX:  bit 9 is cleared when S2P=0 even if setHypSupported(true) → PASSES.
TEST(BugNewB_IDR0_Hyp, Hyp_RES0_WhenS2PDisabled) {
    // §6.3.1: Hyp (bit 9) must be RES0 when IDR0.S2P==0.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setHypSupported(true);
    smmu.setS2PSupported(false);

    uint32_t idr0 = smmu.getIDR0();

    // S2P (bit 0) must be 0 — prerequisite.
    ASSERT_EQ(idr0 & (1u << 0u), 0u)
        << "BUG-B prerequisite: IDR0.S2P must be 0 after setS2PSupported(false)";

    // Hyp (bit 9) must be 0 when S2P==0.
    EXPECT_EQ(idr0 & (1u << 9u), 0u)
        << "BUG-B: IDR0.Hyp (bit 9) must be RES0 when IDR0.S2P==0 (ARM §6.3.1). "
           "Got IDR0=0x" << std::hex << idr0;
}

// Secondary check: Hyp must still be 1 when S2P is enabled (no regression).
TEST(BugNewB_IDR0_Hyp, Hyp_Set_WhenS2PEnabled) {
    // §6.3.1: Hyp (bit 9) may be set when S2P=1 and Hyp is supported.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setHypSupported(true);
    smmu.setS2PSupported(true);

    uint32_t idr0 = smmu.getIDR0();

    ASSERT_NE(idr0 & (1u << 0u), 0u)
        << "BUG-B prerequisite: IDR0.S2P must be 1 after setS2PSupported(true)";

    EXPECT_NE(idr0 & (1u << 9u), 0u)
        << "BUG-B regression: IDR0.Hyp (bit 9) must be 1 when S2P=1 and Hyp supported";
}

// ============================================================================
// BUG-C1: STALL_MODEL==0b10 + stage2Enabled + s2s==false → C_BAD_STE (§5.5)
// ============================================================================

// ARM §5.5: When STALL_MODEL==0b10 (forced stall), every stage-2-capable stream
// MUST have S2S=1 (stage-2 stall enabled); S2S=0 is illegal → C_BAD_STE.
//
// BEFORE FIX: no validation for STALL_MODEL==0b10 → configureStream succeeds
//   without event → test FAILS.
// AFTER FIX:  C_BAD_STE event recorded → test PASSES.
TEST(BugNewC1_StallModel2, ForcedStall_Stage2_S2SFalse_Causes_CBAD_STE) {
    // §5.5: STALL_MODEL==0b10 + stage2Enabled + s2s=false → C_BAD_STE.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x02u);  // forced-stall

    StreamConfig cfg = makeStage2Config();
    cfg.s2s = false;  // illegal: forced stall requires S2S=1 for stage-2 streams

    smmu.configureStream(1u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-C1: STALL_MODEL==0b10 + stage2Enabled + s2s=false must generate "
           "C_BAD_STE event (ARM §5.5)";
}

// ============================================================================
// BUG-C2: STALL_MODEL==0b10 + stage1Enabled + CD.S==0 → C_BAD_CD (§5.5)
// ============================================================================

// ARM §5.5: When STALL_MODEL==0b10 (forced stall), every stage-1-capable stream
// MUST have CD.S=1 (stall mode); faultMode==Terminate (CD.S=0) is illegal → C_BAD_CD.
//
// BEFORE FIX: no validation for STALL_MODEL==0b10 → configureStream succeeds
//   without event → test FAILS.
// AFTER FIX:  C_BAD_CD event recorded → test PASSES.
TEST(BugNewC2_StallModel2, ForcedStall_Stage1_CDSFalse_Causes_CBAD_CD) {
    // §5.5: STALL_MODEL==0b10 + stage1Enabled + faultMode==Terminate (CD.S=0) → C_BAD_CD.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x02u);  // forced-stall

    StreamConfig cfg = makeStage1Config();
    cfg.faultMode = FaultMode::Terminate;  // CD.S=0 — illegal with forced-stall model

    smmu.configureStream(2u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_CD))
        << "BUG-C2: STALL_MODEL==0b10 + stage1Enabled + faultMode==Terminate must "
           "generate C_BAD_CD event (ARM §5.5)";
}

// ============================================================================
// BUG-C3: STALL_MODEL!=0b00 + s1Stalld==true → C_BAD_STE (§5.5)
// ============================================================================

// ARM §5.5: STE.S1STALLD=1 disables stall for stage-1.  When STALL_MODEL
// requires stall (0b01 = terminate-only is fine for stage-2; 0b10 = forced
// stall), combining it with S1STALLD=1 for a stage-1 stream is illegal → C_BAD_STE.
//
// Specifically: STALL_MODEL==0b10 AND s1Stalld==true on a stage-1 stream is
// contradictory: the model mandates stall but the STE disables it.
//
// BEFORE FIX: no validation → configureStream succeeds without event → FAILS.
// AFTER FIX:  C_BAD_STE event recorded → test PASSES.
TEST(BugNewC3_S1StalldConflict, ForcedStall_S1StalldTrue_Causes_CBAD_STE) {
    // §5.5: STALL_MODEL!=0b00 AND s1Stalld=1 for a stage-1 stream → C_BAD_STE.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setStallModel(0x02u);  // forced-stall (STALL_MODEL != 0b00)

    StreamConfig cfg = makeStage1Config();
    cfg.faultMode = FaultMode::Stall;  // attempt to satisfy CD.S=1 requirement
    cfg.s1Stalld  = true;              // but then S1STALLD overrides: contradictory

    smmu.configureStream(3u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-C3: STALL_MODEL!=0b00 AND s1Stalld=true must generate C_BAD_STE "
           "event (ARM §5.5)";
}

// ============================================================================
// BUG-D: SSec RES0 commands must NOT raise CERROR_ILL (ARM §4.1.6 re-evaluation)
// ============================================================================

// ARM IHI0070G.b: TLBI_NH_ALL, TLBI_NH_ASID, TLBI_NH_VA, TLBI_NH_VAA,
// TLBI_S12_VMALL, TLBI_S2_IPA, TLBI_NSNH_ALL do NOT have an SSec field in
// their command encoding — the corresponding bit is RES0.
//
// Setting a RES0 bit to 1 is CONSTRAINED UNPREDICTABLE (per ARM Architecture
// Reference Manual B2.13.2); the SMMU may ignore the bit and execute the
// command normally.  It must NOT raise CERROR_ILL for this reason.
//
// The BUG-NEW-38 fix (test_bugs_new8.cpp) incorrectly added SSec==1 → CERROR_ILL
// guards to these seven commands.  The tests below assert the CORRECT behaviour:
// the commands execute without error even when ssec==true.
//
// BEFORE FIX (= current code after BUG-NEW-38): GERROR.CMDQ_ERR is set → FAILS.
// AFTER FIX:  GERROR.CMDQ_ERR is NOT set → PASSES.
//
// NOTE: ATC_INV is excluded — it has a real SSec field and the guard is correct.

// Helper: submit a command with ssec=true, process, check no CERROR_ILL fired.
// Returns true when the command executed without error (correct behaviour).
static bool checkSSecNoError(SMMU& smmu, CommandType type) {
    // Clear any pre-existing gerror state before this test.
    smmu.clearGerror(smmu.getGerror());

    CommandEntry cmd = makeCmd(type);
    cmd.ssec = true;
    if (!smmu.submitCommand(cmd).isOk()) {
        return false;
    }
    smmu.processCommandQueue();
    // Correct: no CERROR_ILL (GERROR.CMDQ_ERR not active).
    return !isGerrorCmdqErrActive(smmu);
}

// Test D-1: TLBI_NH_ALL with SSec=1 must NOT raise CERROR_ILL.
// §4.4.2.1: TLBI_NH_ALL has no SSec field (RES0 in encoding).
TEST(BugNewD_SSecRES0Commands, TlbiNhAll_SSec1_NoError) {
    // §4.4.2.1: TLBI_NH_ALL has RES0 SSec; setting it to 1 is CONSTRAINED
    // UNPREDICTABLE and must not cause CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_NH_ALL))
        << "BUG-D: TLBI_NH_ALL with ssec=1 must NOT raise CERROR_ILL — SSec is "
           "RES0 in this command's encoding (ARM IHI0070G.b §4.4.2.1)";
}

// Test D-2: TLBI_NH_ASID with SSec=1 must NOT raise CERROR_ILL.
// §4.4.2.2: TLBI_NH_ASID has no SSec field (RES0 in encoding).
TEST(BugNewD_SSecRES0Commands, TlbiNhAsid_SSec1_NoError) {
    // §4.4.2.2: TLBI_NH_ASID has RES0 SSec.
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_NH_ASID))
        << "BUG-D: TLBI_NH_ASID with ssec=1 must NOT raise CERROR_ILL — SSec is "
           "RES0 in this command's encoding (ARM IHI0070G.b §4.4.2.2)";
}

// Test D-3: TLBI_NH_VA with SSec=1 must NOT raise CERROR_ILL.
// §4.4.2.3 (per ARM IHI0070G.b): TLBI_NH_VA has no SSec field (RES0 in encoding).
TEST(BugNewD_SSecRES0Commands, TlbiNhVa_SSec1_NoError) {
    // §4.4.2.3: TLBI_NH_VA has RES0 SSec.
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_NH_VA))
        << "BUG-D: TLBI_NH_VA with ssec=1 must NOT raise CERROR_ILL — SSec is "
           "RES0 in this command's encoding (ARM IHI0070G.b §4.4.2.3)";
}

// Test D-4: TLBI_NH_VAA with SSec=1 must NOT raise CERROR_ILL.
// §4.4.2.4 (per ARM IHI0070G.b): TLBI_NH_VAA has no SSec field (RES0 in encoding).
TEST(BugNewD_SSecRES0Commands, TlbiNhVaa_SSec1_NoError) {
    // §4.4.2.4: TLBI_NH_VAA has RES0 SSec.
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_NH_VAA))
        << "BUG-D: TLBI_NH_VAA with ssec=1 must NOT raise CERROR_ILL — SSec is "
           "RES0 in this command's encoding (ARM IHI0070G.b §4.4.2.4)";
}

// Test D-5: TLBI_S12_VMALL with SSec=1 must NOT raise CERROR_ILL.
// §4.4.3.1: TLBI_S12_VMALL has no SSec field (RES0 in encoding).
// Note: this test assumes S2P=1 so the S2P guard does not fire first.
TEST(BugNewD_SSecRES0Commands, TlbiS12Vmall_SSec1_NoError) {
    // §4.4.3.1: TLBI_S12_VMALL has RES0 SSec.  S2P must be enabled so the
    // S2P==0 guard (BUG-NEW-39) does not fire before the SSec check.
    SMMU smmu;
    enableSMMU(smmu);
    smmu.setS2PSupported(true);   // ensure S2P guard does not trigger
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_S12_VMALL))
        << "BUG-D: TLBI_S12_VMALL with ssec=1 must NOT raise CERROR_ILL — SSec "
           "is RES0 in this command's encoding (ARM IHI0070G.b §4.4.3.1)";
}

// Test D-6: TLBI_S2_IPA with SSec=1 must NOT raise CERROR_ILL.
// §4.4.3.2: TLBI_S2_IPA has no SSec field (RES0 in encoding).
// Note: this test assumes S2P=1 so the S2P guard does not fire first.
TEST(BugNewD_SSecRES0Commands, TlbiS2Ipa_SSec1_NoError) {
    // §4.4.3.2: TLBI_S2_IPA has RES0 SSec.  S2P must be enabled.
    SMMU smmu;
    enableSMMU(smmu);
    smmu.setS2PSupported(true);   // ensure S2P guard does not trigger
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_S2_IPA))
        << "BUG-D: TLBI_S2_IPA with ssec=1 must NOT raise CERROR_ILL — SSec is "
           "RES0 in this command's encoding (ARM IHI0070G.b §4.4.3.2)";
}

// Test D-7: TLBI_NSNH_ALL with SSec=1 must NOT raise CERROR_ILL.
// §4.4.4.1: TLBI_NSNH_ALL has no SSec field (RES0 in encoding).
TEST(BugNewD_SSecRES0Commands, TlbiNsnhAll_SSec1_NoError) {
    // §4.4.4.1: TLBI_NSNH_ALL has RES0 SSec.
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecNoError(smmu, CommandType::TLBI_NSNH_ALL))
        << "BUG-D: TLBI_NSNH_ALL with ssec=1 must NOT raise CERROR_ILL — SSec is "
           "RES0 in this command's encoding (ARM IHI0070G.b §4.4.4.1)";
}

// ============================================================================
// BUG-G: IDR3.MPAM is at bit [7], not bit [6] (§6.3.4)
// ============================================================================

// ARM IHI0070G.b §6.3.4 SMMU_IDR3:
//   bit [7]: MPAM — Memory Partitioning and Monitoring extension support.
//   bit [6]: RES0
//
// Current code (smmu.cpp line 4607):
//   if ((getIDR3() & (1u << 6u)) == 0u) → uses bit 6 (RES0).
// Correct code should use:
//   if ((getIDR3() & (1u << 7u)) == 0u) → uses bit 7 (MPAM).
//
// Since this model never sets MPAM (neither bit 6 nor bit 7 is set in
// getIDR3()), the functional outcome (CERROR_ILL always) is accidentally
// correct.  The tests below:
//   G-1: Assert IDR3 bit 6 is 0 (RES0 per spec — currently correct).
//   G-2: Assert IDR3 bit 7 is 0 in this model (MPAM not implemented — correct).
//   G-3: Assert CFGI_VMS_PIDM raises CERROR_ILL (model has no MPAM — correct
//        functionally, wrong bit used internally).
// These tests will PASS today but document the invariants.  When a fix is
// applied (move the check from bit 6 to bit 7), these tests continue to pass.
// An additional test G-4 documents what SHOULD change: if a future API allows
// setting MPAM=1 via bit 7, CFGI_VMS_PIDM should succeed — but that cannot be
// tested without a new setter API.  We skip G-4 and document the gap.

// Test G-1: IDR3 bit 6 must be 0 (RES0 per ARM §6.3.4).
TEST(BugNewG_IDR3MPAM, IDR3_Bit6_IsRES0) {
    // §6.3.4: IDR3[6] is RES0.  Must read as 0.
    SMMU smmu;
    enableSMMU(smmu);
    uint32_t idr3 = smmu.getIDR3();
    EXPECT_EQ(idr3 & (1u << 6u), 0u)
        << "BUG-G: IDR3 bit 6 must be RES0 (ARM §6.3.4). Got IDR3=0x"
        << std::hex << idr3;
}

// Test G-2: IDR3 bit 7 (MPAM) is 0 in this model (MPAM not implemented).
TEST(BugNewG_IDR3MPAM, IDR3_Bit7_IsMpam_NotSet) {
    // §6.3.4: IDR3[7] = MPAM.  This model does not implement MPAM → must be 0.
    SMMU smmu;
    enableSMMU(smmu);
    uint32_t idr3 = smmu.getIDR3();
    EXPECT_EQ(idr3 & (1u << 7u), 0u)
        << "BUG-G: IDR3 bit 7 (MPAM, per ARM §6.3.4) must be 0 in this model "
           "(MPAM not implemented). Got IDR3=0x" << std::hex << idr3;
}

// Test G-3: CFGI_VMS_PIDM must raise CERROR_ILL when IDR3.MPAM==0 (§4.3.5).
// ARM §4.3.5: CMD_CFGI_VMS_PIDM requires IDR3.MPAM=1.  When MPAM is absent
// (bit 7 == 0), the command must raise CERROR_ILL.
// The CURRENT code checks bit 6 (wrong), not bit 7 (correct), but since
// neither is set, CERROR_ILL fires either way.  This test verifies the
// observable behaviour is correct even before the bit-position fix.
TEST(BugNewG_IDR3MPAM, CfgiVmsPidm_NoMpam_RaisesCerrorIll) {
    // §4.3.5: CFGI_VMS_PIDM when IDR3.MPAM==0 (bit 7 not set) → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    // Sanity: IDR3.MPAM (bit 7) must be 0 in this model.
    ASSERT_EQ(smmu.getIDR3() & (1u << 7u), 0u)
        << "BUG-G prerequisite: IDR3.MPAM (bit 7) must be 0";

    // Clear any pre-existing error.
    smmu.clearGerror(smmu.getGerror());

    CommandEntry cmd = makeCmd(CommandType::CFGI_VMS_PIDM);
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-G: CFGI_VMS_PIDM with IDR3.MPAM=0 (bit 7) must raise CERROR_ILL (§4.3.5)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-G: CFGI_VMS_PIDM with IDR3.MPAM=0 must set GERROR.CMDQ_ERR (§4.3.5)";
}

// ============================================================================
// BUG-E: F_STREAM_DISABLED S1DSS==0b10 + SSV==1 + PASID==0 (§3.9/§7.3.7)
// ============================================================================
//
// ARM §3.9: When STE.S1DSS==0b10, a transaction arriving with SubstreamID=0
// AND SSV=1 should abort with F_STREAM_DISABLED (event 0x06).
//
// BEFORE FIX: translate() has no ssv parameter; this path is unimplemented.
// AFTER FIX:  translate(..., ssv=true) generates F_STREAM_DISABLED → PASSES.

// Helper: configure a stage-1 stream with s1cdMax>0 and s1dss==0b10.
static uint32_t configStage1WithS1DSS2(SMMU& smmu) {
    const uint32_t kStream = 10u;
    StreamConfig cfg = makeStage1Config();
    cfg.s1cdMax = 1u;    // substream-capable
    cfg.s1dss   = 0x02u; // use CD[0]
    smmu.configureStream(kStream, cfg);
    smmu.enableStream(kStream);
    smmu.createStreamPASID(kStream, 0u);
    smmu.mapPage(kStream, 0u, 0x1000u, 0x1000u, PagePermissions(true, true, false));
    return kStream;
}

// Test E-1: S1DSS==0b10 + SSV==1 + PASID==0 → F_STREAM_DISABLED.
TEST(BugNewE_S1DSS10_SSV, S1DSS10_SSV1_PASID0_Generates_FStreamDisabled) {
    // §3.9 / §7.3.7: S1DSS==0b10 + SSV==1 + PASID==0 → F_STREAM_DISABLED.
    SMMU smmu;
    enableSMMU(smmu);
    uint32_t sid = configStage1WithS1DSS2(smmu);

    auto result = smmu.translate(sid, 0u, 0x1000u, AccessType::Read,
                                 SecurityState::NonSecure,
                                 TransactionType::Ordinary,
                                 /*ssv=*/true);

    EXPECT_FALSE(result.isOk())
        << "BUG-E: S1DSS==0b10 + SSV==1 + PASID==0 must abort";
    EXPECT_TRUE(hasEvent(smmu, EventType::F_STREAM_DISABLED))
        << "BUG-E: S1DSS==0b10 + SSV==1 + PASID==0 must generate F_STREAM_DISABLED event (ARM §3.9/§7.3.7)";
}

// Test E-2: S1DSS==0b10 + SSV==0 + PASID==0 → must NOT abort (normal CD[0] path).
TEST(BugNewE_S1DSS10_SSV, S1DSS10_SSV0_PASID0_NormalTranslation) {
    // §3.9: S1DSS==0b10 + SSV==0 + PASID==0 → use CD[0] normally (no abort).
    SMMU smmu;
    enableSMMU(smmu);
    uint32_t sid = configStage1WithS1DSS2(smmu);

    smmu.translate(sid, 0u, 0x1000u, AccessType::Read,
                   SecurityState::NonSecure,
                   TransactionType::Ordinary,
                   /*ssv=*/false);

    EXPECT_FALSE(hasEvent(smmu, EventType::F_STREAM_DISABLED))
        << "BUG-E regression: S1DSS==0b10 + SSV==0 must NOT generate F_STREAM_DISABLED";
}
