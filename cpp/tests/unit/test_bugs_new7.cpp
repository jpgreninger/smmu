// Regression tests for BUG-NEW-33 (C++ dead-code removal).
//
// BUG-NEW-33 (C++, §4.4):
//   Dead code was removed from executeInvalidationCommandLocked(): the 9
//   Secure-queue-only commands listed below were unreachable there because
//   processCommand() already intercepts them and writes CERROR_ILL before
//   calling the helper.  The removal changes nothing observable — these tests
//   confirm that CERROR_ILL is STILL raised through processCommand() after
//   the dead code was deleted.
//
//   The 9 commands (all must raise CERROR_ILL on the NS queue):
//     TLBI_EL3_ALL  (§4.1.1 / §4.4.2.9)
//     TLBI_EL3_VA   (§4.1.1 / §4.4.2.9)
//     TLBI_S_EL2_ALL  (§4.4.2.11)
//     TLBI_S_EL2_VA   (§4.4.2.12)
//     TLBI_S_EL2_VAA  (§4.4.2.13)
//     TLBI_S_EL2_ASID (§4.4.2.14)
//     TLBI_S_S12_VMALL (§4.4.3.4)
//     TLBI_S_S2_IPA    (§4.4.3.3)
//     TLBI_SNH_ALL     (§4.4.4.2)
//
// All tests are REGRESSION GUARDS — they should PASS both before and after
// the dead-code removal.
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
// BUG-NEW-33: dead-code removal regression guards
// ============================================================================
// All nine tests follow the same structure:
//   1. Enable SMMU.
//   2. Submit the Secure-queue-only command to the NS queue.
//   3. Process the command queue.
//   4. Verify that CMDQ_CONS.ERR == CERROR_ILL and GERROR.CMDQ_ERR is active.
//
// These are REGRESSION GUARDS — they must PASS both before and after the
// dead-code removal.

// Test 1: TLBI_EL3_ALL on NS queue must still raise CERROR_ILL (§4.4.2.9).
TEST(BugNew33DeadCodeRegression, El3All_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_EL3_ALL);
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_EL3_ALL must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_EL3_ALL must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 2: TLBI_EL3_VA on NS queue must still raise CERROR_ILL (§4.4.2.9).
TEST(BugNew33DeadCodeRegression, El3Va_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_EL3_VA);
    cmd.startAddress = 0x0000'1000u;
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_EL3_VA must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_EL3_VA must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 3: TLBI_S_EL2_ALL on NS queue must still raise CERROR_ILL (§4.4.2.11).
TEST(BugNew33DeadCodeRegression, SEL2All_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_ALL);
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_S_EL2_ALL must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_S_EL2_ALL must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 4: TLBI_S_EL2_VA on NS queue must still raise CERROR_ILL (§4.4.2.12).
TEST(BugNew33DeadCodeRegression, SEL2Va_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_VA);
    cmd.startAddress = 0x0000'2000u;
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_S_EL2_VA must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_S_EL2_VA must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 5: TLBI_S_EL2_VAA on NS queue must still raise CERROR_ILL (§4.4.2.13).
TEST(BugNew33DeadCodeRegression, SEL2Vaa_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_VAA);
    cmd.startAddress = 0x0000'3000u;
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_S_EL2_VAA must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_S_EL2_VAA must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 6: TLBI_S_EL2_ASID on NS queue must still raise CERROR_ILL (§4.4.2.14).
TEST(BugNew33DeadCodeRegression, SEL2Asid_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_EL2_ASID);
    cmd.asid = 0u;
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_S_EL2_ASID must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_S_EL2_ASID must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 7: TLBI_S_S12_VMALL on NS queue must still raise CERROR_ILL (§4.4.3.4).
TEST(BugNew33DeadCodeRegression, SS12Vmall_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_S12_VMALL);
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_S_S12_VMALL must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_S_S12_VMALL must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 8: TLBI_S_S2_IPA on NS queue must still raise CERROR_ILL (§4.4.3.3).
TEST(BugNew33DeadCodeRegression, SS2Ipa_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_S_S2_IPA);
    cmd.startAddress = 0x0000'4000u;
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_S_S2_IPA must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_S_S2_IPA must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}

// Test 9: TLBI_SNH_ALL on NS queue must still raise CERROR_ILL (§4.4.4.2).
TEST(BugNew33DeadCodeRegression, SnhAll_Still_Raises_CERROR_ILL) {
    SMMU s;
    enableSMMU(s);

    CommandEntry cmd = makeCmd(CommandType::TLBI_SNH_ALL);
    ASSERT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();

    EXPECT_EQ(s.getCmdqConsErr(), CERROR_ILL)
        << "TLBI_SNH_ALL must still write CERROR_ILL after dead-code removal (BUG-NEW-33)";
    EXPECT_TRUE(isGerrorCmdqErrActive(s))
        << "TLBI_SNH_ALL must still raise GERROR.CMDQ_ERR after dead-code removal (BUG-NEW-33)";
}
