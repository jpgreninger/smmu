// TDD failing tests for BUG-NEW-15, BUG-NEW-16, and BUG-NEW-17
// (C++ implementation).
//
// Each test is written to FAIL with the current code and PASS only after the
// corresponding fix is applied.  No fixes are included here.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-15 (C++ only, §8.2/§6.3.9.5): submitPageRequest() ignores SMMUEN
//
//   submitPageRequest() guards PRI queue acceptance with CR0.PRIQEN but NOT
//   with CR0.SMMUEN.  After enable() followed by disable(), PRIQEN remains
//   set in the CR0 register while SMMUEN is cleared, so PRI requests are
//   incorrectly accepted on a globally-disabled SMMU.
//
//   ARM §6.3.9.5 / §8.2: the effective PRIQEN is (CR0.PRIQEN AND CR0.SMMUEN).
//   When SMMUEN=0 the SMMU is disabled; no PRI queuing must be accepted.
//
//   Rust already implements this correctly (effective_priqen check added).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-16 (Both, §4.7.1/§4.1.6): CMD_RESUME missing ssec validation
//
//   ARM §4.1.6: a Non-Secure command queue entry with SSec=1 is illegal and
//   must raise CERROR_ILL / GERROR.CMDQ_ERR.  Currently CommandEntry has no
//   ssec field so validation is structurally impossible.
//
//   These tests reference cmd.ssec = true, which will NOT COMPILE until the
//   ssec field is added to CommandEntry.  That compile failure is the
//   expected "red" state.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-17 (Both, §4.7.2/§4.1.6): CMD_STALL_TERM missing ssec validation
//
//   Same gap as BUG-NEW-16 but for CMD_STALL_TERM.  A Non-Secure CMD_STALL_TERM
//   with SSec=1 must raise CERROR_ILL / GERROR.CMDQ_ERR.
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <memory>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Build a minimal PRIEntry for stream 0xA0.
static PRIEntry makePRIEntry() {
    PRIEntry req;
    req.streamID         = 0xA0u;
    req.pasid            = 0u;
    req.requestedAddress = 0x1000u;
    req.accessType       = AccessType::Read;
    req.isLastRequest    = true;
    req.prgIndex         = 0u;
    req.securityState    = SecurityState::NonSecure;
    return req;
}

// Check whether GERROR.CMDQ_ERR is active (toggled from GERRORN).
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return ((s.getGerror() ^ s.getGerrorN()) & GERROR_CMDQ_ERR) != 0u;
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-15: submitPageRequest() must check SMMUEN, not just PRIQEN (§8.2)
// ============================================================================

// Test 1: After enable() + disable(), submitPageRequest() must reject the
// request because SMMUEN=0 even though PRIQEN remains set.
//
// BEFORE FIX: submitPageRequest() checks only PRIQEN (which is still 1 after
// disable()); the request is silently accepted and enqueued.
//
// AFTER FIX:  submitPageRequest() computes effective PRIQEN = PRIQEN AND
// SMMUEN; when SMMUEN=0 the request is rejected and the queue stays empty.
TEST(BugNew15PriqenSmmuen, SubmitPageRequestRejectedWhenSMMUENZero) {
    SMMU smmu;

    // Enable the SMMU so that both SMMUEN and PRIQEN are set.
    smmu.enable();
    ASSERT_TRUE(smmu.isEnabled())
        << "BUG-NEW-15 setup: SMMU must be enabled after enable()";

    const uint32_t cr0AfterEnable = smmu.getCR0();
    ASSERT_NE(cr0AfterEnable & SMMU::CR0_PRIQEN, 0u)
        << "BUG-NEW-15 setup: PRIQEN must be set after enable()";
    ASSERT_NE(cr0AfterEnable & SMMU::CR0_SMMUEN, 0u)
        << "BUG-NEW-15 setup: SMMUEN must be set after enable()";

    // Disable the SMMU.  SMMUEN is cleared, but PRIQEN stays set per the
    // current disable() implementation.
    smmu.disable();
    ASSERT_FALSE(smmu.isEnabled())
        << "BUG-NEW-15 setup: SMMU must be disabled after disable()";

    const uint32_t cr0AfterDisable = smmu.getCR0();
    ASSERT_EQ(cr0AfterDisable & SMMU::CR0_SMMUEN, 0u)
        << "BUG-NEW-15 setup: SMMUEN must be cleared after disable()";
    // PRIQEN is expected to remain set (the bug condition).
    ASSERT_NE(cr0AfterDisable & SMMU::CR0_PRIQEN, 0u)
        << "BUG-NEW-15 precondition: PRIQEN must remain set after disable() "
           "(the bug is that submitPageRequest reads only PRIQEN, not SMMUEN)";

    // Record PRI queue size before the call.
    const size_t sizeBefore = smmu.getPRIQueueSize();

    // Submit a page request — must be rejected when SMMUEN=0.
    smmu.submitPageRequest(makePRIEntry());

    // AFTER FIX: queue must remain the same size (request was rejected).
    // BEFORE FIX: queue grows by 1 because PRIQEN=1 bypasses the SMMUEN check.
    const size_t sizeAfter = smmu.getPRIQueueSize();

    EXPECT_EQ(sizeAfter, sizeBefore)
        << "BUG-NEW-15: submitPageRequest() must reject the request when "
           "SMMUEN=0 (even if PRIQEN=1); effective PRIQEN = PRIQEN AND SMMUEN "
           "(ARM §6.3.9.5/§8.2)";
}

// Test 2 (negative): When SMMUEN=1 and PRIQEN=1, submitPageRequest() must
// still accept the request (base-case regression).
TEST(BugNew15PriqenSmmuen, SubmitPageRequestAcceptedWhenBothEnabled) {
    SMMU smmu;
    enableSMMU(smmu);

    const size_t sizeBefore = smmu.getPRIQueueSize();

    smmu.submitPageRequest(makePRIEntry());

    // Queue must grow by exactly 1.
    EXPECT_EQ(smmu.getPRIQueueSize(), sizeBefore + 1u)
        << "BUG-NEW-15 neg: submitPageRequest() must accept the request "
           "when both SMMUEN=1 and PRIQEN=1";
}

// Test 3: After re-enabling the SMMU (SMMUEN=1 again), submitPageRequest()
// must accept requests.  Confirms the fix is conditional on SMMUEN state.
TEST(BugNew15PriqenSmmuen, SubmitPageRequestAcceptedAfterReenable) {
    SMMU smmu;
    smmu.enable();
    smmu.disable();

    // Re-enable the SMMU — SMMUEN is set again.
    smmu.enable();
    ASSERT_TRUE(smmu.isEnabled())
        << "BUG-NEW-15 reenable: SMMU must be enabled after second enable()";

    const size_t sizeBefore = smmu.getPRIQueueSize();

    smmu.submitPageRequest(makePRIEntry());

    EXPECT_EQ(smmu.getPRIQueueSize(), sizeBefore + 1u)
        << "BUG-NEW-15 reenable: submitPageRequest() must accept requests "
           "after SMMUEN is re-set via enable()";
}

// ============================================================================
// BUG-NEW-16: CMD_RESUME with ssec=1 must raise CERROR_ILL (§4.7.1/§4.1.6)
// ============================================================================
//
// NOTE: The ssec field does NOT yet exist on CommandEntry.
// The lines below that reference cmd.ssec = true WILL NOT COMPILE until the
// ssec field is added to CommandEntry.  That compile failure is the expected
// "red" state that demonstrates the missing field and missing validation.
//
// Once ssec is added, the test will compile but the assertion will FAIL at
// runtime because processCommandQueue() does not yet validate the ssec field.
// Only after the validation logic is also added will the test pass (green).

TEST(BugNew16ResumeSsec, ResumeSsec1RaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    // Issue CMD_RESUME with SSec=1 on a Non-Secure command queue.
    // ARM §4.1.6: Non-Secure queue + SSec=1 → CERROR_ILL + GERROR.CMDQ_ERR.
    //
    // BUG-NEW-16: ssec field does not exist yet on CommandEntry.
    // The line below WILL NOT COMPILE — this is the intended "red" state.
    CommandEntry cmd;
    cmd.type   = CommandType::RESUME;
    cmd.stag   = 0u;
    cmd.action = false;
    cmd.ssec   = true;  // BUG-NEW-16: ssec field does not exist yet — compile error
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // AFTER FIX: CERROR_ILL must be set and GERROR.CMDQ_ERR active.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-16: CMD_RESUME with ssec=1 on Non-Secure queue must raise "
           "CERROR_ILL in CMDQ_CONS.ERR (ARM §4.7.1/§4.1.6)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-16: CMD_RESUME with ssec=1 must assert GERROR.CMDQ_ERR "
           "(ARM §4.1.6)";
}

// Negative test: CMD_RESUME with ssec=0 on Non-Secure queue must NOT raise
// CERROR_ILL (normal resume, no stall match is still a silent no-op).
TEST(BugNew16ResumeSsec, ResumeSsec0DoesNotRaiseCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd;
    cmd.type   = CommandType::RESUME;
    cmd.stag   = 0u;
    cmd.action = false;
    cmd.ssec   = false;  // BUG-NEW-16: ssec field does not exist yet — compile error
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // ssec=0 is legal — no CERROR_ILL expected.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE)
        << "BUG-NEW-16 neg: CMD_RESUME with ssec=0 must NOT raise CERROR_ILL "
           "(ARM §4.1.6)";

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-16 neg: GERROR.CMDQ_ERR must NOT be asserted for ssec=0 resume";
}

// ============================================================================
// BUG-NEW-17: CMD_STALL_TERM with ssec=1 must raise CERROR_ILL (§4.7.2/§4.1.6)
// ============================================================================
//
// Same structural gap as BUG-NEW-16 but for CMD_STALL_TERM.
//
// NOTE: References to cmd.ssec below WILL NOT COMPILE until the ssec field
// is added to CommandEntry.  That compile failure is the expected "red" state.

TEST(BugNew17StallTermSsec, StallTermSsec1RaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    // Issue CMD_STALL_TERM with SSec=1 on a Non-Secure command queue.
    // ARM §4.1.6: Non-Secure queue + SSec=1 → CERROR_ILL + GERROR.CMDQ_ERR.
    //
    // BUG-NEW-17: ssec field does not exist yet on CommandEntry.
    // The line below WILL NOT COMPILE — this is the intended "red" state.
    CommandEntry cmd;
    cmd.type     = CommandType::STALL_TERM;
    cmd.streamID = 0x10u;
    cmd.ssec     = true;  // BUG-NEW-17: ssec field does not exist yet — compile error
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // AFTER FIX: CERROR_ILL must be set and GERROR.CMDQ_ERR active.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-17: CMD_STALL_TERM with ssec=1 on Non-Secure queue must raise "
           "CERROR_ILL in CMDQ_CONS.ERR (ARM §4.7.2/§4.1.6)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-17: CMD_STALL_TERM with ssec=1 must assert GERROR.CMDQ_ERR "
           "(ARM §4.1.6)";
}

// Negative test: CMD_STALL_TERM with ssec=0 on Non-Secure queue must NOT raise
// CERROR_ILL.
TEST(BugNew17StallTermSsec, StallTermSsec0DoesNotRaiseCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd;
    cmd.type     = CommandType::STALL_TERM;
    cmd.streamID = 0x11u;
    cmd.ssec     = false;  // BUG-NEW-17: ssec field does not exist yet — compile error
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // ssec=0 is legal — no CERROR_ILL expected.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE)
        << "BUG-NEW-17 neg: CMD_STALL_TERM with ssec=0 must NOT raise CERROR_ILL "
           "(ARM §4.1.6)";

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-17 neg: GERROR.CMDQ_ERR must NOT be asserted for ssec=0 stall-term";
}
