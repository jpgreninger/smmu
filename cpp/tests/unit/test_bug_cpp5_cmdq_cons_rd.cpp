// ARM SMMU v3 Bug Fix Test: BUG-CPP-5
//
//   BUG-CPP-5: CMDQ CONS.RD advanced before command is processed.
//
// ARM §7.1: "SMMU_(*_)CMDQ_CONS.RD remains pointing to the erroneous command
// in the Command queue."  Older (successfully processed) commands have already
// been consumed, but the erroneous command itself must remain the current CONS.RD
// so software can identify which command failed.
//
// Current broken sequence in processCommandQueue():
//   1. pop command from internal deque
//   2. advance CONS.RD unconditionally   <-- BUG
//   3. call processCommand()             <-- error set here, but CONS.RD already wrong
//
// Required fix:
//   advance CONS.RD ONLY after processCommand() returns without signalling an error.
//   On CERROR_ILL / CERROR_ABT leave CONS.RD pointing at the erroneous command.
//
// Test cases:
//   BugCpp5CmdqConsRd/ConsRdNotAdvancedOnCerrorIll:
//     Submit exactly one CMD_SYNC with CS=3 (reserved → CERROR_ILL).
//     After processCommandQueue():
//       - CMDQ_CONS.ERR must equal CERROR_ILL (1)
//       - CMDQ_CONS.RD  must equal 0 (still pointing at the erroneous command)
//       Before fix: CONS.RD == 1 (advanced past the bad command).
//       After  fix: CONS.RD == 0 (retained at the bad command).
//
//   BugCpp5CmdqConsRd/ConsRdAdvancedNormallyOnSuccess:
//     Submit one normal CMD_SYNC with CS=0 (no completion signal).
//     After processCommandQueue():
//       - CMDQ_CONS.ERR must equal CERROR_NONE (0)
//       - CMDQ_CONS.RD  must equal 1 (advanced past the good command)
//
//   BugCpp5CmdqConsRd/ConsRdAtSecondCommandOnCerrorIll:
//     Submit two commands: first a valid CFGI_ALL, then an illegal CMD_SYNC CS=3.
//     After processCommandQueue():
//       - CMDQ_CONS.ERR must equal CERROR_ILL (1)
//       - CMDQ_CONS.RD  must equal 1 (advanced past the first good command,
//                                      but retained at the second bad command)
//       Before fix: CONS.RD == 2 (advanced past both commands).
//       After  fix: CONS.RD == 1 (advanced past first, retained at second).
//
// ARM IHI0070G.b references:
//   §7.1   — CMDQ_CONS.RD semantics on error
//   §4.8   — CMD_SYNC CS=0b11 → CERROR_ILL
//   §6.3.28 — CMDQ_CONS register layout

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>
#include <cstdint>

using namespace smmu;

namespace {

static std::unique_ptr<SMMU> makeTestSMMU() {
    auto s = std::make_unique<SMMU>();
    s->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    return s;
}

} // namespace

// ============================================================================
// BUG-CPP-5: CONS.RD must remain pointing at the erroneous command
// ============================================================================

/// After one illegal CMD_SYNC (CS=3), CONS.RD must still be 0 (not advanced).
///
/// BEFORE FIX: CONS.RD == 1 (advanced unconditionally before processCommand()).
/// AFTER FIX:  CONS.RD == 0 (not advanced because CERROR_ILL was signalled).
TEST(BugCpp5CmdqConsRd, ConsRdNotAdvancedOnCerrorIll) {
    auto smmu = makeTestSMMU();

    // Submit exactly one illegal CMD_SYNC (CS=3 is Reserved → CERROR_ILL).
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu->submitCommand(illSync).isOk())
        << "Precondition: submitCommand must succeed (queue not full)";

    // PROD.RD must be 1 after the submit.
    ASSERT_EQ(smmu->getCmdqProdIndex(), 1u)
        << "Precondition: PROD.RD must be 1 after submitting one command";

    smmu->processCommandQueue();

    // CERROR_ILL must be set in CONS.ERR.
    EXPECT_EQ(smmu->getCmdqConsErr(), CERROR_ILL)
        << "BUG-CPP-5 precondition: CMDQ_CONS.ERR must be CERROR_ILL (1) "
           "after illegal CMD_SYNC CS=3";

    // CONS.RD must NOT have been advanced — it must remain at index 0,
    // pointing at the erroneous command.
    uint32_t consRd = smmu->getCmdqConsIndex() & 0xFFFFFu;
    EXPECT_EQ(consRd, 0u)
        << "BUG-CPP-5: CMDQ_CONS.RD must remain 0 (pointing at the erroneous "
           "command) after CERROR_ILL is set. "
           "Before fix: CONS.RD == 1 (advanced unconditionally before "
           "processCommand() is called). "
           "After fix: CONS.RD == 0 (advance skipped because error was detected).";
}

/// A valid CMD_SYNC (CS=0) must still advance CONS.RD normally.
///
/// This is a regression guard: the fix must not prevent CONS.RD from
/// advancing on successful commands.
TEST(BugCpp5CmdqConsRd, ConsRdAdvancedNormallyOnSuccess) {
    auto smmu = makeTestSMMU();

    // Submit one good CMD_SYNC with CS=0 (SIG_NONE — no completion signal).
    CommandEntry okSync;
    okSync.type = CommandType::SYNC;
    okSync.cs   = 0u;
    ASSERT_TRUE(smmu->submitCommand(okSync).isOk());

    smmu->processCommandQueue();

    // No error must be set.
    EXPECT_EQ(smmu->getCmdqConsErr(), CERROR_NONE)
        << "Regression: no CERROR must be set after a valid CMD_SYNC CS=0";

    // CONS.RD must have advanced to 1.
    uint32_t consRd = smmu->getCmdqConsIndex() & 0xFFFFFu;
    EXPECT_EQ(consRd, 1u)
        << "Regression: CMDQ_CONS.RD must be 1 (advanced past the good command) "
           "after a successful CMD_SYNC.";
}

/// Two commands: one good (CFGI_ALL), then one bad (CMD_SYNC CS=3).
/// CONS.RD must advance past the first (good) command but stay at the second
/// (bad) command.
///
/// BEFORE FIX: CONS.RD == 2 (both commands advanced before processing).
/// AFTER FIX:  CONS.RD == 1 (only the good command advanced; bad one retained).
TEST(BugCpp5CmdqConsRd, ConsRdAtSecondCommandOnCerrorIll) {
    auto smmu = makeTestSMMU();

    // First command: CFGI_ALL (always succeeds, no error path).
    CommandEntry cfgiAll;
    cfgiAll.type = CommandType::CFGI_ALL;
    ASSERT_TRUE(smmu->submitCommand(cfgiAll).isOk());

    // Second command: illegal CMD_SYNC CS=3.
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu->submitCommand(illSync).isOk());

    // PROD.RD must be 2 after two submits.
    ASSERT_EQ(smmu->getCmdqProdIndex(), 2u)
        << "Precondition: PROD.RD must be 2 after submitting two commands";

    smmu->processCommandQueue();

    // CERROR_ILL must be set.
    EXPECT_EQ(smmu->getCmdqConsErr(), CERROR_ILL)
        << "BUG-CPP-5 precondition: CMDQ_CONS.ERR must be CERROR_ILL (1)";

    // CONS.RD must be 1: advanced past CFGI_ALL (index 0), but retained
    // at the illegal CMD_SYNC (index 1).
    uint32_t consRd = smmu->getCmdqConsIndex() & 0xFFFFFu;
    EXPECT_EQ(consRd, 1u)
        << "BUG-CPP-5: CMDQ_CONS.RD must be 1 after one good + one bad command. "
           "The CONS.RD must have advanced past the good CFGI_ALL (index 0) but "
           "must remain pointing at the bad CMD_SYNC (index 1). "
           "Before fix: CONS.RD == 2 (advanced past both unconditionally). "
           "After  fix: CONS.RD == 1 (only advanced past successfully-processed commands).";
}
