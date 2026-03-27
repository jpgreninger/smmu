// TDD failing tests for BUG-NEW-24, BUG-NEW-25, BUG-NEW-26, BUG-NEW-27 (C++ side).
//
// Each test is written to FAIL before the fix and PASS after the fix is applied.
// Positive/regression tests that should already PASS are explicitly noted.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-24 (C++ only, §4.4.2.8/9/10):
//   CMD_TLBI_EL2_VA, CMD_TLBI_EL2_VAA, CMD_TLBI_EL2_ASID are missing the
//   IDR0.Hyp==0 → CERROR_ILL guard.  They fall through into the shared block
//   at smmu.cpp lines 4388-4390 with no guard.  Only CMD_TLBI_EL2_ALL (separate
//   case) already has the guard.
//   Per ARM §4.4.2.8/9/10: "When SMMU_IDR0.Hyp == 0, this command causes
//   CERROR_ILL."
//
// BUG-NEW-25 (C++ side, §4.2.1/4.2.2/4.1.6):
//   CMD_PREFETCH_CONFIG and CMD_PREFETCH_ADDR do not check SSec=1 and therefore
//   do not raise CERROR_ILL.  All NS-queue commands with SSec=1 must raise
//   CERROR_ILL per §4.1.6.
//
// BUG-NEW-26 (C++ only, §4.7.3/4.8):
//   CMD_SYNC completion event inherits stream security state via a stream lookup.
//   CMD_SYNC has no architectural StreamID; the event must always be NonSecure.
//   In C++ (smmu.cpp lines ~4545-4565), cs != 0 triggers a stream lookup which
//   can yield Secure.  The fix: always use SecurityState::NonSecure.
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
    cmd.type  = type;
    cmd.cs    = 0u;
    cmd.ssec  = false;
    return cmd;
}

// Check whether GERROR.CMDQ_ERR is active (GERROR XOR GERRORN has bit 0 set).
// BUG-NEW-40 fix: getGerror() already returns gerrorStatus XOR gerrorNStatus
// (the active error bits).  The former double-XOR recovered the raw gerrorStatus
// instead of the active-error mask.  One mask is sufficient.
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-24: CMD_TLBI_EL2_VA/VAA/ASID must check IDR0.Hyp (§4.4.2.8/9/10)
// ============================================================================
// ARM §4.4.2.8/9/10: "When SMMU_IDR0.Hyp == 0, this command causes CERROR_ILL."
// The fix breaks TLBI_EL2_VA, TLBI_EL2_VAA, and TLBI_EL2_ASID out of the
// shared fall-through block and adds an IDR0.Hyp guard for each.

// Test 1: When Hyp=false (setHypSupported(false)), CMD_TLBI_EL2_VA must
// activate GERROR.CMDQ_ERR and write CERROR_ILL.
//
// BEFORE FIX: no guard → command executes without error → CMDQ_ERR stays 0
//             → EXPECT_TRUE(isGerrorCmdqErrActive) FAILS (red).
// AFTER FIX:  guard fires → CERROR_ILL written → CMDQ_ERR toggled → passes.
TEST(BugNew24El2VaVaaAsidHypGuard, El2VA_Missing_IDR0Hyp_Guard) {
    SMMU smmu;
    enableSMMU(smmu);

    // Override: clear IDR0.Hyp to simulate a non-hypervisor implementation.
    smmu.setHypSupported(false);
    ASSERT_EQ(smmu.getIDR0() & (1u << 9u), 0u)
        << "BUG-NEW-24 precondition: IDR0.Hyp (bit 9) must be 0 after setHypSupported(false)";

    CommandEntry cmd = makeCmd(CommandType::TLBI_EL2_VA);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-24: CMD_TLBI_EL2_VA with IDR0.Hyp=0 must write CERROR_ILL "
           "(ARM §4.4.2.8)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-24: CMD_TLBI_EL2_VA with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.8)";
}

// Test 2: When Hyp=false, CMD_TLBI_EL2_VAA must activate GERROR.CMDQ_ERR.
//
// BEFORE FIX: no guard → no CERROR → test FAILS (red).
// AFTER FIX:  guard fires → CERROR_ILL and CMDQ_ERR set → passes.
TEST(BugNew24El2VaVaaAsidHypGuard, El2VAA_Missing_IDR0Hyp_Guard) {
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setHypSupported(false);
    ASSERT_EQ(smmu.getIDR0() & (1u << 9u), 0u)
        << "BUG-NEW-24 precondition: IDR0.Hyp must be 0";

    CommandEntry cmd = makeCmd(CommandType::TLBI_EL2_VAA);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-24: CMD_TLBI_EL2_VAA with IDR0.Hyp=0 must write CERROR_ILL "
           "(ARM §4.4.2.9)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-24: CMD_TLBI_EL2_VAA with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.9)";
}

// Test 3: When Hyp=false, CMD_TLBI_EL2_ASID must activate GERROR.CMDQ_ERR.
//
// BEFORE FIX: no guard → no CERROR → test FAILS (red).
// AFTER FIX:  guard fires → CERROR_ILL and CMDQ_ERR set → passes.
TEST(BugNew24El2VaVaaAsidHypGuard, El2ASID_Missing_IDR0Hyp_Guard) {
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setHypSupported(false);
    ASSERT_EQ(smmu.getIDR0() & (1u << 9u), 0u)
        << "BUG-NEW-24 precondition: IDR0.Hyp must be 0";

    CommandEntry cmd = makeCmd(CommandType::TLBI_EL2_ASID);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-24: CMD_TLBI_EL2_ASID with IDR0.Hyp=0 must write CERROR_ILL "
           "(ARM §4.4.2.10)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-24: CMD_TLBI_EL2_ASID with IDR0.Hyp=0 must assert GERROR.CMDQ_ERR "
           "(ARM §4.4.2.10)";
}

// Test 4 (positive): When Hyp=true (default), CMD_TLBI_EL2_VA must NOT raise
// CERROR_ILL.
//
// This positive path must PASS both before and after the fix (regression guard).
TEST(BugNew24El2VaVaaAsidHypGuard, Hyp1_EL2VA_Succeeds) {
    SMMU smmu;
    enableSMMU(smmu);

    // IDR0.Hyp defaults to true (bit 9 = 1).
    ASSERT_NE(smmu.getIDR0() & (1u << 9u), 0u)
        << "BUG-NEW-24 precondition: IDR0.Hyp (bit 9) must be 1 by default";

    CommandEntry cmd = makeCmd(CommandType::TLBI_EL2_VA);
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE)
        << "BUG-NEW-24 (positive): CMD_TLBI_EL2_VA with IDR0.Hyp=1 must NOT "
           "write CERROR_ILL (ARM §4.4.2.8)";
    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-24 (positive): CMD_TLBI_EL2_VA with IDR0.Hyp=1 must NOT "
           "assert GERROR.CMDQ_ERR (ARM §4.4.2.8)";
}

// ============================================================================
// BUG-NEW-25 (C++): CMD_PREFETCH_CONFIG and CMD_PREFETCH_ADDR with SSec=1
// must raise CERROR_ILL (§4.1.6: all NS-queue commands with SSec=1 are illegal)
// ============================================================================

// Test 5: CMD_PREFETCH_CONFIG with ssec=true must raise CERROR_ILL.
//
// BEFORE FIX: both commands are unconditional no-ops at smmu.cpp lines 4337-4345
//             with no ssec check → CMDQ_ERR not set → test FAILS (red).
// AFTER FIX:  ssec guard added → CERROR_ILL written → CMDQ_ERR toggled → passes.
TEST(BugNew25PrefetchSsecGuard, PrefetchConfig_SSec1_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::PREFETCH_CONFIG);
    cmd.ssec = true;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-25: CMD_PREFETCH_CONFIG with ssec=1 must write CERROR_ILL "
           "(ARM §4.1.6)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-25: CMD_PREFETCH_CONFIG with ssec=1 must assert GERROR.CMDQ_ERR "
           "(ARM §4.1.6)";
}

// Test 6: CMD_PREFETCH_ADDR with ssec=true must raise CERROR_ILL.
//
// BEFORE FIX: unconditional no-op → no CERROR → test FAILS (red).
// AFTER FIX:  ssec guard fires → CERROR_ILL and CMDQ_ERR set → passes.
TEST(BugNew25PrefetchSsecGuard, PrefetchAddr_SSec1_Must_Raise_CERROR_ILL) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::PREFETCH_ADDR);
    cmd.ssec = true;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-25: CMD_PREFETCH_ADDR with ssec=1 must write CERROR_ILL "
           "(ARM §4.1.6)";
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-25: CMD_PREFETCH_ADDR with ssec=1 must assert GERROR.CMDQ_ERR "
           "(ARM §4.1.6)";
}

// Test 7 (positive): CMD_PREFETCH_CONFIG with ssec=false must NOT raise error.
//
// This positive path must PASS both before and after the fix (regression guard).
TEST(BugNew25PrefetchSsecGuard, PrefetchConfig_SSec0_NoError) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd = makeCmd(CommandType::PREFETCH_CONFIG);
    cmd.ssec = false;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE)
        << "BUG-NEW-25 (positive): CMD_PREFETCH_CONFIG with ssec=0 must NOT "
           "write CERROR_ILL (ARM §4.1.6)";
    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-25 (positive): CMD_PREFETCH_CONFIG with ssec=0 must NOT "
           "assert GERROR.CMDQ_ERR (ARM §4.1.6)";
}

// ============================================================================
// BUG-NEW-26 (C++): CMD_SYNC completion event must always use NonSecure
// ============================================================================
// ARM §4.7.3/§4.8: CMD_SYNC has no StreamID operand; the completion event's
// security state must always be SecurityState::NonSecure.
// In C++ (smmu.cpp lines ~4545-4565), when cs != 0 the code looks up the
// stream by command.streamID and uses that stream's security state.
// If a Secure stream has been configured for that StreamID, the event
// incorrectly inherits Secure.

// Test 8: Configure a Secure stream; issue CMD_SYNC with cs=1 pointing to
// that stream's StreamID.  The COMMAND_SYNC_COMPLETION event must have
// securityState == NonSecure.
//
// BEFORE FIX: stream lookup returns Secure → event.securityState=Secure
//             → EXPECT_EQ(Secure, NonSecure) FAILS (red).
// AFTER FIX:  stream lookup removed, always NonSecure → event OK → passes.
TEST(BugNew26CmdSyncSecState, CompletionEvent_AlwaysNonSecure) {
    SMMU smmu;
    enableSMMU(smmu);

    // Configure and enable a Secure stream so the stream-lookup code can find
    // it with isStreamEnabled() == true.  Without enableStream() the C++ code
    // falls back to NonSecure unconditionally, masking the bug.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.securityState      = SecurityState::Secure;
    cfg.strw               = StreamWorld::EL1_EL0;
    smmu.configureStream(0x60u, cfg);
    smmu.enableStream(0x60u);

    // CMD_SYNC with cs=1 (SIG_IRQ) to trigger a COMMAND_SYNC_COMPLETION event.
    // stream_id=0x60 points at the Secure stream; the event must ignore this.
    CommandEntry cmd = makeCmd(CommandType::SYNC);
    cmd.cs       = 1u;
    cmd.streamID = 0x60u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    const std::vector<EventEntry> events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            EXPECT_EQ(ev.securityState, SecurityState::NonSecure)
                << "BUG-NEW-26: COMMAND_SYNC_COMPLETION must always use "
                   "SecurityState::NonSecure — CMD_SYNC has no StreamID "
                   "operand; the completion event belongs to the NS command "
                   "queue domain (ARM §4.7.3/§4.8)";
            found = true;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-NEW-26: COMMAND_SYNC_COMPLETION event must be generated for CS=1";
}
