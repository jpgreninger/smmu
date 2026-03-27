// TDD failing tests for BUG-NEW-28, BUG-NEW-29, BUG-NEW-30, BUG-NEW-31, BUG-NEW-32
// (C++ side).
//
// Each test is written to FAIL before the fix and PASS after the fix is applied.
// Positive/regression tests that should already PASS are explicitly noted.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-28 (Both, §4.4.4.2):
//   CMD_TLBI_SNH_ALL must raise CERROR_ILL when submitted on the NS command
//   queue.  Per ARM §4.4.4.2: "This command causes CERROR_ILL when used on
//   the Non-secure Command queue."
//   Currently C++ has TLBI_SNH_ALL in a Secure-commands fall-through block
//   calling executeInvalidationCommandLocked with no CERROR_ILL.
//
// BUG-NEW-29 (Both, §4.4.2.11/12/13/14):
//   CMD_TLBI_S_EL2_ALL, CMD_TLBI_S_EL2_VA, CMD_TLBI_S_EL2_VAA,
//   CMD_TLBI_S_EL2_ASID must all raise CERROR_ILL on the NS queue.
//   Per ARM §4.4.2.11-14: each description states "This command is valid only
//   on the Secure Command queue.  On the Non-secure Command queue it causes
//   CERROR_ILL."
//   Currently C++ has all four in a Secure-commands block calling
//   executeInvalidationCommandLocked without raising CERROR_ILL.
//
// BUG-NEW-30 (Both, §4.4.3.3/4.4.3.4):
//   CMD_TLBI_S_S2_IPA and CMD_TLBI_S_S12_VMALL must raise CERROR_ILL on the
//   NS queue.  Per ARM §4.4.3.3/4.4.3.4: "This command is valid only on the
//   Secure Command queue; on the Non-secure Command queue it causes CERROR_ILL."
//   Currently C++ has both in the Secure-commands block without CERROR_ILL.
//
// BUG-NEW-31 (C++ only, §8.2):
//   submitPageRequest() uses XOR (priqen != smmuen) to test for effective
//   PRIQEN==0.  When both PRIQEN=0 AND SMMUEN=0 (fresh un-enabled SMMU), XOR
//   evaluates false so PPRs are silently enqueued rather than triggering
//   auto-failure.  Rust correctly uses AND: effective_priqen = PRIQEN && SMMUEN.
//
// BUG-NEW-32 (Both, §4.3.5):
//   CMD_CFGI_VMS_PIDM is missing two guards:
//   1. SSec=1 on NS queue → CERROR_ILL (§4.1.6, applies to all commands)
//   2. IDR3.MPAM==0 → CERROR_ILL (§4.3.5 requires MPAM feature;
//      this model reports IDR3.MPAM=0)
//   Currently C++ calls invalidatePASIDCache without any guard.
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

// Build a minimal CommandEntry for the given type.
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

// BUG-NEW-40 fix: getGerror() already returns gerrorStatus XOR gerrorNStatus
// (the active error bits).  The former double-XOR recovered the raw gerrorStatus
// instead of the active-error mask.  One mask is sufficient.
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-28: CMD_TLBI_SNH_ALL on NS queue must raise CERROR_ILL (§4.4.4.2)
// ============================================================================
// ARM §4.4.4.2: "This command causes CERROR_ILL when used on the Non-secure
// Command queue."
//
// BEFORE FIX: TLBI_SNH_ALL falls through to executeInvalidationCommandLocked
//             with no error check → CMDQ_ERR not set → test FAILS (red).
// AFTER FIX:  CERROR_ILL written and GERROR.CMDQ_ERR toggled → passes (green).

// Test 1: TLBI_SNH_ALL submitted to NS queue must raise CERROR_ILL.
TEST(BugNew28SnhAllNsQueue, SnhAll_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_SNH_ALL);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-28: CMD_TLBI_SNH_ALL on NS queue must write CERROR_ILL "
           "(ARM §4.4.4.2)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-28: CMD_TLBI_SNH_ALL on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.4.2)";
}

// ============================================================================
// BUG-NEW-29: CMD_TLBI_S_EL2_{ALL,VA,VAA,ASID} on NS queue must raise
// CERROR_ILL (§4.4.2.11/12/13/14)
// ============================================================================
// ARM §4.4.2.11-14: each variant "is valid only on the Secure Command queue;
// on the Non-secure Command queue it causes CERROR_ILL."
//
// BEFORE FIX: all four fall through to executeInvalidationCommandLocked →
//             no CERROR_ILL → CMDQ_ERR not set → tests FAIL (red).
// AFTER FIX:  CERROR_ILL raised and CMDQ_ERR toggled → pass (green).

// Test 2: TLBI_S_EL2_ALL on NS queue must raise CERROR_ILL.
TEST(BugNew29SEL2CommandsNsQueue, S_EL2_ALL_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_ALL);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-29: CMD_TLBI_S_EL2_ALL on NS queue must write CERROR_ILL "
           "(ARM §4.4.2.11)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-29: CMD_TLBI_S_EL2_ALL on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.11)";
}

// Test 3: TLBI_S_EL2_VA on NS queue must raise CERROR_ILL.
TEST(BugNew29SEL2CommandsNsQueue, S_EL2_VA_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_VA);
    cmd.startAddress = 0x0000'1000u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-29: CMD_TLBI_S_EL2_VA on NS queue must write CERROR_ILL "
           "(ARM §4.4.2.12)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-29: CMD_TLBI_S_EL2_VA on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.12)";
}

// Test 4: TLBI_S_EL2_VAA on NS queue must raise CERROR_ILL.
TEST(BugNew29SEL2CommandsNsQueue, S_EL2_VAA_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_VAA);
    cmd.startAddress = 0x0000'2000u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-29: CMD_TLBI_S_EL2_VAA on NS queue must write CERROR_ILL "
           "(ARM §4.4.2.13)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-29: CMD_TLBI_S_EL2_VAA on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.13)";
}

// Test 5: TLBI_S_EL2_ASID on NS queue must raise CERROR_ILL.
TEST(BugNew29SEL2CommandsNsQueue, S_EL2_ASID_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_ASID);
    cmd.asid = 0u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-29: CMD_TLBI_S_EL2_ASID on NS queue must write CERROR_ILL "
           "(ARM §4.4.2.14)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-29: CMD_TLBI_S_EL2_ASID on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.14)";
}

// ============================================================================
// BUG-NEW-30: CMD_TLBI_S_S2_IPA and CMD_TLBI_S_S12_VMALL on NS queue must
// raise CERROR_ILL (§4.4.3.3/§4.4.3.4)
// ============================================================================
// ARM §4.4.3.3: "CMD_TLBI_S_S2_IPA is valid only on the Secure Command queue;
//   on the Non-secure Command queue it causes CERROR_ILL."
// ARM §4.4.3.4: "CMD_TLBI_S_S12_VMALL is valid only on the Secure Command
//   queue; on the Non-secure Command queue it causes CERROR_ILL."
//
// BEFORE FIX: both fall through to executeInvalidationCommandLocked → no
//             CERROR_ILL → CMDQ_ERR not set → tests FAIL (red).
// AFTER FIX:  CERROR_ILL raised and CMDQ_ERR toggled → pass (green).

// Test 6: TLBI_S_S2_IPA on NS queue must raise CERROR_ILL.
TEST(BugNew30SecureStage2CommandsNsQueue, S_S2_IPA_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_S2_IPA);
    cmd.startAddress = 0x0000'4000u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-30: CMD_TLBI_S_S2_IPA on NS queue must write CERROR_ILL "
           "(ARM §4.4.3.3)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-30: CMD_TLBI_S_S2_IPA on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.3.3)";
}

// Test 7: TLBI_S_S12_VMALL on NS queue must raise CERROR_ILL.
TEST(BugNew30SecureStage2CommandsNsQueue, S_S12_VMALL_NS_Queue_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_S12_VMALL);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-30: CMD_TLBI_S_S12_VMALL on NS queue must write CERROR_ILL "
           "(ARM §4.4.3.4)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-30: CMD_TLBI_S_S12_VMALL on NS queue must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.3.4)";
}

// ============================================================================
// BUG-NEW-31 (C++ only, §8.2): submitPageRequest XOR vs AND for effective PRIQEN
// ============================================================================
// ARM §8.2: when effective PRIQEN==0 (PRIQEN=0 OR SMMUEN=0) all incoming PPRs
// must cause an automatic PRG Response with ResponseCode=0b1111 (failure).
//
// C++ bug: the guard uses (priqen != smmuen) — XOR logic.  When BOTH bits are
// 0 (fresh SMMU, never configured), XOR is false so the auto-failure is NOT
// generated and the PPR is silently enqueued instead.
//
// Rust is correct: uses (priqen && smmuen) — AND logic.
//
// BEFORE FIX: XOR guard → with both zero, no auto-failure → PRIQ grows →
//             getPRIAutoFailures().size()==0 → EXPECT_GT FAILS (red).
// AFTER FIX:  AND guard → both zero fires the path → auto-failure pushed →
//             getPRIAutoFailures().size()==1 → passes (green).

// Test 8: fresh SMMU (PRIQEN=0, SMMUEN=0) — submit Last=1 PPR — must produce
//         one auto-failure response.
TEST(BugNew31SubmitPageRequestXorVsAnd, BothZero_LastOne_Must_AutoFail) {
    // Create a fresh SMMU — do NOT call enableSMMU() or setCR0().
    // Both CR0.PRIQEN and CR0.SMMUEN are 0.
    SMMU smmu;

    // Verify initial state: both bits are zero.
    ASSERT_EQ(smmu.getCR0() & (SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN), 0u)
        << "BUG-NEW-31 precondition: CR0.SMMUEN and CR0.PRIQEN must both be 0 "
           "on a fresh SMMU before enableSMMU()";

    PRIEntry req;
    req.streamID         = 0x10u;
    req.pasid            = 0u;
    req.requestedAddress = 0x1000u;
    req.accessType       = AccessType::Read;
    req.isLastRequest    = true;   // Last=1 PPR — a PRG response is required
    req.prgIndex         = 1u;
    smmu.submitPageRequest(req);

    // ARM §8.2: effective PRIQEN==0 → auto-PRG-Response(Failure) must be
    // generated for Last=1 PPRs.
    //
    // BUG: C++ uses XOR — (0 != 0) == false → guard does NOT fire →
    //      no auto-failure recorded, PPR silently accepted.
    // FIX: use AND — (0 && 0) == false → guard DOES fire (same as priqen || !smmuen)
    //      → auto-failure recorded.
    //
    // BEFORE FIX: size==0 → EXPECT_GT(... , 0u) FAILS (red).
    // AFTER FIX:  size==1 → passes (green).
    auto failures = smmu.getPRIAutoFailures();
    EXPECT_GT(failures.size(), 0u)
        << "BUG-NEW-31: submitPageRequest() with PRIQEN=0 AND SMMUEN=0 must "
           "generate an auto-failure PRG_RESPONSE for a Last=1 PPR "
           "(ARM §8.2).  C++ guard uses XOR instead of OR/AND, so the path "
           "is skipped when both bits are 0.";
}

// Test 9 (positive): with PRIQEN=1 but SMMUEN=0, auto-failure must still fire
// (this tests the existing XOR path that already works for PRIQEN=1, SMMUEN=0).
// This is the regression guard — must PASS before AND after the fix.
TEST(BugNew31SubmitPageRequestXorVsAnd, PriqenOne_SmmuentZero_LastOne_AutoFails) {
    SMMU smmu;

    // Set only PRIQEN=1, leave SMMUEN=0.
    smmu.setCR0(SMMU::CR0_PRIQEN);
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_PRIQEN,  0u)  << "precondition: PRIQEN must be 1";
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN,  0u)  << "precondition: SMMUEN must be 0";

    PRIEntry req;
    req.streamID         = 0x11u;
    req.pasid            = 0u;
    req.requestedAddress = 0x2000u;
    req.accessType       = AccessType::Read;
    req.isLastRequest    = true;
    req.prgIndex         = 2u;
    smmu.submitPageRequest(req);

    // The existing XOR guard fires correctly when exactly one of the bits is set.
    // This must PASS both before and after the fix.
    auto failures = smmu.getPRIAutoFailures();
    EXPECT_GT(failures.size(), 0u)
        << "BUG-NEW-31 (positive): PRIQEN=1, SMMUEN=0 must generate auto-failure "
           "PRG_RESPONSE for Last=1 PPR.  This is the existing working path "
           "(ARM §8.2) — should pass before and after the fix.";
}

// ============================================================================
// BUG-NEW-32: CMD_CFGI_VMS_PIDM missing guards (§4.3.5/§4.1.6)
// ============================================================================
// ARM §4.3.5: CMD_CFGI_VMS_PIDM requires IDR3.MPAM==1.  This model reports
// IDR3.MPAM=0, so the command must unconditionally raise CERROR_ILL.
// ARM §4.1.6: Any NS-queue command with SSec=1 is illegal — CERROR_ILL.
//
// BEFORE FIX: invalidatePASIDCache called for ssec=false; no error checks →
//             CMDQ_ERR not set → tests FAIL (red).
// AFTER FIX:  CERROR_ILL raised (both paths) → CMDQ_ERR toggled → pass (green).

// Test 10: CFGI_VMS_PIDM with ssec=true must raise CERROR_ILL per §4.1.6.
TEST(BugNew32CfgiVmsPidmGuards, SSec1_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::CFGI_VMS_PIDM);
    cmd.ssec = true;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-32: CMD_CFGI_VMS_PIDM with ssec=1 must write CERROR_ILL "
           "(ARM §4.1.6: all NS-queue commands with SSec=1 are illegal)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-32: CMD_CFGI_VMS_PIDM with ssec=1 must assert GERROR.CMDQ_ERR "
           "(ARM §4.1.6)";
}

// Test 11: CFGI_VMS_PIDM with ssec=false must still raise CERROR_ILL because
//          IDR3.MPAM==0 makes the command unconditionally illegal per §4.3.5.
TEST(BugNew32CfgiVmsPidmGuards, MPAM0_Always_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    // ssec=false — the SSec guard is NOT the reason this must fail.
    // The MPAM==0 guard must fire regardless.
    CommandEntry cmd = makeCmd(CommandType::CFGI_VMS_PIDM);
    cmd.ssec = false;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-32: CMD_CFGI_VMS_PIDM with ssec=0 must still write CERROR_ILL "
           "because IDR3.MPAM==0 and MPAM is required for this command "
           "(ARM §4.3.5)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-32: CMD_CFGI_VMS_PIDM with ssec=0 must assert GERROR.CMDQ_ERR "
           "because IDR3.MPAM==0 (ARM §4.3.5)";
}
