// Regression/TDD tests for BUG-NEW-37, BUG-NEW-38, BUG-NEW-39.
//
// BUG-NEW-37 (§4.4.2.3/§4.4.2.4):
//   TLBI_NH_VA and TLBI_NH_VAA lack VMID scope — they call non-VMID-scoped
//   TLBCache helpers and therefore evict entries belonging to OTHER VMIDs that
//   share the same VA/ASID.  The spec requires that only entries tagged with the
//   operand VMID are evicted.
//   Fix: three new TLBCache helpers + two updated switch cases.
//
// BUG-NEW-38 (§4.1.6):
//   Eight NS-queue commands (TLBI_NH_ALL, TLBI_NH_ASID, TLBI_NH_VA,
//   TLBI_NH_VAA, TLBI_S12_VMALL, TLBI_S2_IPA, TLBI_NSNH_ALL, ATC_INV) share a
//   single fall-through case block and have no SSec guard.  When submitted with
//   SSec=1 on the NS queue they silently execute instead of raising CERROR_ILL.
//   Fix: split into individual cases each with an SSec guard.
//
// BUG-NEW-39 (§4.4.3.1/§4.4.3.2):
//   TLBI_S12_VMALL and TLBI_S2_IPA are missing an IDR0.S2P==0 → CERROR_ILL
//   guard (mirroring the IDR0.Hyp guard already present for TLBI_EL2_ALL).
//   Fix: add s2pSupported_ atomic + setS2PSupported(bool) + isS2PSupported()
//   API, update getIDR0() bit 0, and add guards in processCommand().
//
// All BUG-NEW-37/38/39 tests are written to FAIL before the fix and PASS after.
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

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
// getGerror() already returns gerrorStatus XOR gerrorNStatus (the active bits),
// so a single mask suffices — no double-XOR.
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

// Configure a minimal stage-1-only stream with a given ASID and VMID.
static void configureStage1Stream(SMMU& smmu, StreamID sid, uint16_t asid, uint16_t vmid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.asid               = asid;
    cfg.vmid               = vmid;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-37: TLBI_NH_VA must be VMID-scoped (§4.4.2.4)
// ============================================================================
// Set up two streams sharing the same VA+ASID but with different VMIDs.
// After populating the TLB via translate(), issue TLBI_NH_VA targeting VMID-A.
// The entry for VMID-B must NOT be evicted: a subsequent translate() for
// VMID-B must still succeed without a fault (TLB still warm).

// Test 1: TLBI_NH_VA targeting VMID-A must NOT evict the VMID-B entry.
TEST(BugNew37TlbiNhVaVmidScope, NhVa_TargetsVmidA_PreservesVmidB_Entry) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sidA  = 10u;
    const StreamID sidB  = 20u;
    const uint16_t asid  = 42u;
    const uint16_t vmidA = 1u;
    const uint16_t vmidB = 2u;
    const IOVA     va    = 0x5000u;
    const PA       paA   = 0x100000u;
    const PA       paB   = 0x200000u;

    configureStage1Stream(smmu, sidA, asid, vmidA);
    configureStage1Stream(smmu, sidB, asid, vmidB);

    PagePermissions perms;
    perms.read    = true;
    perms.write   = true;
    perms.execute = false;

    smmu.mapPage(sidA, 0u, va, paA, perms);
    smmu.mapPage(sidB, 0u, va, paB, perms);

    // Warm TLB for both streams.
    auto resA1 = smmu.translate(sidA, 0u, va, AccessType::Read);
    ASSERT_TRUE(resA1.isOk()) << "Pre-condition: stream A translate must succeed";

    auto resB1 = smmu.translate(sidB, 0u, va, AccessType::Read);
    ASSERT_TRUE(resB1.isOk()) << "Pre-condition: stream B translate must succeed";

    // Issue TLBI_NH_VA targeting VMID-A only.
    CommandEntry cmd = makeCmd(CommandType::TLBI_NH_VA);
    cmd.vmid         = vmidA;
    cmd.asid         = asid;
    cmd.startAddress = va;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // Translation for stream B must still succeed (VMID-B entry preserved).
    auto resB2 = smmu.translate(sidB, 0u, va, AccessType::Read);
    EXPECT_TRUE(resB2.isOk())
        << "BUG-NEW-37: TLBI_NH_VA must be VMID-scoped; VMID-B entry must survive";
    if (resB2.isOk()) {
        EXPECT_EQ(resB2.getValue().physicalAddress, paB)
            << "BUG-NEW-37: VMID-B entry PA must be unchanged after VMID-A invalidation";
    }
}

// Test 2: TLBI_NH_VAA targeting VMID-A must NOT evict the VMID-B entry.
TEST(BugNew37TlbiNhVaaVmidScope, NhVaa_TargetsVmidA_PreservesVmidB_Entry) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sidA  = 30u;
    const StreamID sidB  = 40u;
    const uint16_t asid  = 77u;
    const uint16_t vmidA = 3u;
    const uint16_t vmidB = 4u;
    const IOVA     va    = 0x9000u;
    const PA       paA   = 0x300000u;
    const PA       paB   = 0x400000u;

    configureStage1Stream(smmu, sidA, asid, vmidA);
    configureStage1Stream(smmu, sidB, asid, vmidB);

    PagePermissions perms;
    perms.read    = true;
    perms.write   = true;
    perms.execute = false;

    smmu.mapPage(sidA, 0u, va, paA, perms);
    smmu.mapPage(sidB, 0u, va, paB, perms);

    // Warm TLB for both streams.
    auto resA1 = smmu.translate(sidA, 0u, va, AccessType::Read);
    ASSERT_TRUE(resA1.isOk()) << "Pre-condition: stream A translate must succeed";

    auto resB1 = smmu.translate(sidB, 0u, va, AccessType::Read);
    ASSERT_TRUE(resB1.isOk()) << "Pre-condition: stream B translate must succeed";

    // Issue TLBI_NH_VAA targeting VMID-A only.
    CommandEntry cmd = makeCmd(CommandType::TLBI_NH_VAA);
    cmd.vmid         = vmidA;
    cmd.startAddress = va;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // Translation for stream B must still succeed (VMID-B entry preserved).
    auto resB2 = smmu.translate(sidB, 0u, va, AccessType::Read);
    EXPECT_TRUE(resB2.isOk())
        << "BUG-NEW-37: TLBI_NH_VAA must be VMID-scoped; VMID-B entry must survive";
    if (resB2.isOk()) {
        EXPECT_EQ(resB2.getValue().physicalAddress, paB)
            << "BUG-NEW-37: VMID-B entry PA must be unchanged after VMID-A invalidation";
    }
}

// ============================================================================
// BUG-NEW-38 (§4.1.6): SSec=1 on NS queue must raise CERROR_ILL for 8 commands
// ============================================================================
// Each test submits the command with ssec=true and verifies CERROR_ILL + GERROR.

// Helper: submits cmd with ssec=true, processes queue, checks CERROR_ILL and GERROR.
// Returns true if the error was correctly detected.
static bool checkSSecRaisesIll(SMMU& smmu, CommandType type) {
    smmu.clearGerror(smmu.getGerror()); // clear any pre-existing error

    CommandEntry cmd = makeCmd(type);
    cmd.ssec = true;
    if (!smmu.submitCommand(cmd).isOk()) {
        return false;
    }
    smmu.processCommandQueue();
    return (smmu.getCmdqConsErr() == CERROR_ILL) && isGerrorCmdqErrActive(smmu);
}

// Test 3: TLBI_NH_ALL with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiNhAll_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_NH_ALL))
        << "BUG-NEW-38: TLBI_NH_ALL with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 4: TLBI_NH_ASID with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiNhAsid_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_NH_ASID))
        << "BUG-NEW-38: TLBI_NH_ASID with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 5: TLBI_NH_VA with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiNhVa_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_NH_VA))
        << "BUG-NEW-38: TLBI_NH_VA with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 6: TLBI_NH_VAA with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiNhVaa_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_NH_VAA))
        << "BUG-NEW-38: TLBI_NH_VAA with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 7: TLBI_S12_VMALL with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiS12Vmall_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_S12_VMALL))
        << "BUG-NEW-38: TLBI_S12_VMALL with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 8: TLBI_S2_IPA with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiS2Ipa_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_S2_IPA))
        << "BUG-NEW-38: TLBI_S2_IPA with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 9: TLBI_NSNH_ALL with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, TlbiNsnhAll_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::TLBI_NSNH_ALL))
        << "BUG-NEW-38: TLBI_NSNH_ALL with SSec=1 must raise CERROR_ILL on NS queue";
}

// Test 10: ATC_INV with SSec=1 must raise CERROR_ILL (§4.1.6).
TEST(BugNew38SSecGuard8Commands, AtcInv_SSec1_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);
    EXPECT_TRUE(checkSSecRaisesIll(smmu, CommandType::ATC_INV))
        << "BUG-NEW-38: ATC_INV with SSec=1 must raise CERROR_ILL on NS queue";
}

// ============================================================================
// BUG-NEW-39 (§4.4.3.1/§4.4.3.2): IDR0.S2P==0 guard for TLBI_S12_VMALL / TLBI_S2_IPA
// ============================================================================
// When IDR0.S2P==0 (no stage-2 support), TLBI_S12_VMALL and TLBI_S2_IPA must
// raise CERROR_ILL — mirroring the IDR0.Hyp guard already present for EL2 cmds.

// Test 11: TLBI_S12_VMALL when S2P==0 must raise CERROR_ILL (§4.4.3.1).
TEST(BugNew39S2PGuard, TlbiS12Vmall_S2P0_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setS2PSupported(false);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S12_VMALL);
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-39: TLBI_S12_VMALL with IDR0.S2P==0 must write CERROR_ILL";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-39: TLBI_S12_VMALL with IDR0.S2P==0 must activate GERROR.CMDQ_ERR";
}

// Test 12: TLBI_S2_IPA when S2P==0 must raise CERROR_ILL (§4.4.3.2).
TEST(BugNew39S2PGuard, TlbiS2Ipa_S2P0_Raises_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setS2PSupported(false);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S2_IPA);
    cmd.startAddress = 0x1000u;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-39: TLBI_S2_IPA with IDR0.S2P==0 must write CERROR_ILL";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-39: TLBI_S2_IPA with IDR0.S2P==0 must activate GERROR.CMDQ_ERR";
}
