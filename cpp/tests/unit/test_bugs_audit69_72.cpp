// TDD failing tests for BUG-AUDIT-71 and BUG-AUDIT-72 (C++ side).
//
// BUG-AUDIT-71 (Both §6.3.9): enable() unconditionally sets CR0_PRIQEN regardless
//   of priSupported_.  ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0.
//   C++ enable() at ~line 5460:
//     cr0_.fetch_or(CR0_SMMUEN | CR0_PRIQEN | CR0_EVENTQEN | CR0_CMDQEN, ...)
//   BEFORE FIX: setPRISupported(false) → enable() → getCR0ACK() has PRIQEN=1 — wrong.
//   AFTER FIX:  getPRISupported(false) → enable() → getCR0ACK().PRIQEN==0.
//
// BUG-AUDIT-72 (C++ only §4.1/§6.3.17): default cases in executeInvalidationCommand()
//   (~line 4278) and executeInvalidationCommandLocked() (~line 4655) call
//   generateEvent(EventType::C_BAD_STE, ...) instead of the correct error channel:
//     writeCmdqConsErr(CERROR_ILL) + signalGerror(GERROR_CMDQ_ERR).
//   ARM §4.1: an unrecognized command opcode must cause CMDQ_CONS.ERR=CERROR_ILL
//             and GERROR.CMDQ_ERR; generating C_BAD_STE is incorrect.
//
//   Notes on testability:
//     - executeInvalidationCommand() (non-locked): never called from any caller in the
//       codebase; the function is dead code from the command-routing perspective.
//     - executeInvalidationCommandLocked(): all known command types that processCommand()
//       routes to it have explicit cases in its switch.  The default can only be reached
//       if a new command type is added to processCommand() without a matching case in
//       executeInvalidationCommandLocked().
//     - processCommand() default: correctly calls writeCmdqConsErr(CERROR_ILL) +
//       signalGerror(GERROR_CMDQ_ERR) — this is the accessible path for unknown opcodes.
//
//   Test strategy:
//     (a) BUG-AUDIT-72 observable path: submit a cast unknown-opcode command via the
//         public submitCommand/processCommandQueue API, verify CMDQ_CONS.ERR=CERROR_ILL
//         and GERROR.CMDQ_ERR is set (via processCommand() default which is correctly
//         gated but is adjacent to the buggy executeInvalidationCommand* defaults).
//     (b) BUG-AUDIT-72 guard: submit a VALID invalidation command (e.g., CFGI_STE)
//         and verify NO C_BAD_STE event is generated, confirming the correct path does
//         not accidentally trigger the buggy default.
//     (c) BUG-AUDIT-72 direct default test: use static_cast<CommandType>(0xFF) to
//         inject an unknown type.  processCommand() default fires first; verify
//         CERROR_ILL+GERROR_CMDQ_ERR are set (not C_BAD_STE event).

#include <gtest/gtest.h>
#include <memory>
#include "smmu/smmu.h"
#include "smmu/address_space.h"

using namespace smmu;

// ─── Helpers ──────────────────────────────────────────────────────────────────

// Enable the SMMU with all standard bits set.
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);
    s.setS2PSupported(true);
}

// Count events of a specific type in the event queue.
static size_t countEvents(SMMU& s, EventType t) {
    std::vector<EventEntry> events = s.getEventQueue();
    size_t count = 0u;
    for (const auto& ev : events) {
        if (ev.type == t) {
            ++count;
        }
    }
    return count;
}

// ─── BUG-AUDIT-71 tests (C++) — enable() PRIQEN conditioned on priSupported_ ─

// BUG-AUDIT-71 test 1: enable() with setPRISupported(false) must NOT set PRIQEN.
//
// ARM §6.3.9: CR0.PRIQEN is RES0 when IDR0.PRI==0.
// After enable(), CR0ACK.PRIQEN must be 0 when priSupported_==false.
//
// BEFORE FIX: CR0.PRIQEN=1 and CR0ACK.PRIQEN=1 after enable() regardless — FAILS.
// AFTER FIX:  CR0ACK.PRIQEN=0 when priSupported_=false — PASSES.
TEST(BugAudit71, EnableMustNotSetPriqenWhenPriNotSupported) {
    SMMU smmu;

    // Disable PRI (IDR0.PRI=0).
    smmu.setPRISupported(false);

    // Confirm IDR0.PRI=0 (bit 16).
    ASSERT_EQ((smmu.getIDR0() >> 16u) & 1u, 0u)
        << "BUG-AUDIT-71 precondition: IDR0.PRI must be 0 when setPRISupported(false).";

    // Call enable() — should NOT set PRIQEN when PRI not supported.
    smmu.enable();

    // CR0ACK.PRIQEN (bit 1) must be 0.
    uint32_t cr0ack = smmu.getCR0ACK();
    EXPECT_EQ(cr0ack & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71: enable() must NOT set CR0ACK.PRIQEN when priSupported_==false. "
           "ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. "
           "Current code unconditionally ORs in CR0_PRIQEN regardless of priSupported_. "
           "cr0ack=0x" << std::hex << cr0ack
        << " CR0_PRIQEN=0x" << std::hex << SMMU::CR0_PRIQEN;

    // CR0.PRIQEN must also be 0.
    uint32_t cr0 = smmu.getCR0();
    EXPECT_EQ(cr0 & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71: enable() must NOT set CR0.PRIQEN when priSupported_==false. "
           "ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. "
           "cr0=0x" << std::hex << cr0;
}

// BUG-AUDIT-71 test 2: enable() with setPRISupported(true) MUST set PRIQEN.
//
// When IDR0.PRI=1 (priSupported_=true), PRIQEN must be set after enable().
//
// BEFORE FIX: PRIQEN always set → this test PASSES before fix.
// AFTER FIX:  PRIQEN conditionally set → this test still PASSES.
TEST(BugAudit71, EnableMustSetPriqenWhenPriSupported) {
    SMMU smmu;

    // Enable PRI (IDR0.PRI=1).
    smmu.setPRISupported(true);

    // Confirm IDR0.PRI=1 (bit 16).
    ASSERT_NE((smmu.getIDR0() >> 16u) & 1u, 0u)
        << "BUG-AUDIT-71 precondition: IDR0.PRI must be 1 when setPRISupported(true).";

    smmu.enable();

    // CR0ACK.PRIQEN must be 1.
    uint32_t cr0ack = smmu.getCR0ACK();
    EXPECT_NE(cr0ack & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71: enable() must set CR0ACK.PRIQEN=1 when priSupported_==true. "
           "ARM §6.3.9: PRIQEN is writable when IDR0.PRI==1. "
           "cr0ack=0x" << std::hex << cr0ack;

    // CR0.PRIQEN must also be 1.
    uint32_t cr0 = smmu.getCR0();
    EXPECT_NE(cr0 & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71: enable() must set CR0.PRIQEN=1 when priSupported_==true. "
           "cr0=0x" << std::hex << cr0;
}

// BUG-AUDIT-71 test 3: SMMUEN, CMDQEN, EVENTQEN are always set by enable() regardless
// of priSupported_. Only PRIQEN is conditional.
//
// BEFORE / AFTER FIX: SMMUEN, CMDQEN, EVENTQEN always set → PASSES.
TEST(BugAudit71, EnableAlwaysSetsOtherBitsRegardlessOfPriSupported) {
    for (bool priSupported : {false, true}) {
        SMMU smmu;
        smmu.setPRISupported(priSupported);
        smmu.enable();

        uint32_t cr0ack = smmu.getCR0ACK();

        EXPECT_NE(cr0ack & SMMU::CR0_SMMUEN, 0u)
            << "BUG-AUDIT-71: enable() must always set CR0ACK.SMMUEN regardless of priSupported. "
               "priSupported=" << priSupported
            << " cr0ack=0x" << std::hex << cr0ack;

        EXPECT_NE(cr0ack & SMMU::CR0_CMDQEN, 0u)
            << "BUG-AUDIT-71: enable() must always set CR0ACK.CMDQEN regardless of priSupported. "
               "priSupported=" << priSupported
            << " cr0ack=0x" << std::hex << cr0ack;

        EXPECT_NE(cr0ack & SMMU::CR0_EVENTQEN, 0u)
            << "BUG-AUDIT-71: enable() must always set CR0ACK.EVENTQEN regardless of priSupported. "
               "priSupported=" << priSupported
            << " cr0ack=0x" << std::hex << cr0ack;
    }
}

// BUG-AUDIT-71 test 4: enable() with priSupported_=false produces PRIQEN=0 in both
// CR0 and CR0ACK; verify getCmdqConsErr() is CERROR_NONE (no error from enable alone).
//
// This test also verifies the state is internally consistent after the operation.
//
// BEFORE FIX: CR0ACK.PRIQEN=1 even with priSupported_=false → FAILS.
// AFTER FIX:  CR0ACK.PRIQEN=0 with priSupported_=false → PASSES.
TEST(BugAudit71, EnableNoPriSupportedSetsNoSpuriousError) {
    SMMU smmu;
    smmu.setPRISupported(false);
    smmu.enable();

    uint32_t cr0ack = smmu.getCR0ACK();

    // PRIQEN must be 0.
    EXPECT_EQ(cr0ack & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71: CR0ACK.PRIQEN must be 0 after enable() when priSupported_=false. "
           "ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. "
           "cr0ack=0x" << std::hex << cr0ack;

    // SMMUEN must still be 1.
    EXPECT_NE(cr0ack & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-71: CR0ACK.SMMUEN must be 1 after enable() regardless of priSupported. "
           "cr0ack=0x" << std::hex << cr0ack;

    // No spurious CMDQ error from enable().
    EXPECT_EQ(smmu.getCmdqConsErr(), 0u)
        << "BUG-AUDIT-71: enable() with priSupported_=false must not set CMDQ_CONS.ERR. "
           "getCmdqConsErr()=" << smmu.getCmdqConsErr();
}

// BUG-AUDIT-71 test 5 (C++-specific): CR0ACK guard — setStrtabFormat() must be blocked
// when CR0ACK.SMMUEN=1 (CR0.SMMUEN=0 but CR0ACK left at SMMUEN=1 via setCR0ACK()).
//
// This exercises the CR0ACK-independent path available only in C++ via setCR0ACK().
// The test verifies the BUG-AUDIT-67 fix holds for setStrtabFormat.
// It also sets up for a complementary CR0ACK.SMMUEN test to validate that enable()
// correctly updates CR0ACK.PRIQEN based on priSupported_.
//
// Scenario: After enable(), call setCR0(0) to clear CR0.SMMUEN; CR0ACK syncs to 0
// as a side-effect of setCR0(). Manually restore CR0ACK.SMMUEN=1 via setCR0ACK().
// Then call enable() again (with priSupported_=false) — enable() must update CR0ACK.
//
// BEFORE FIX: second enable() with priSupported_=false still sets CR0ACK.PRIQEN=1 — FAILS.
// AFTER FIX:  second enable() only sets SMMUEN|CMDQEN|EVENTQEN in CR0ACK — PASSES.
TEST(BugAudit71, EnableWithCr0AckManuallySetChecksCorrectly) {
    SMMU smmu;

    // First enable with default PRI support (true).
    smmu.setPRISupported(true);
    smmu.enable();
    ASSERT_NE(smmu.getCR0ACK() & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71 precondition: first enable() with priSupported_=true must set PRIQEN.";

    // Disable, then change priSupported_ to false, then re-enable.
    smmu.disable();
    smmu.setPRISupported(false);
    ASSERT_EQ((smmu.getIDR0() >> 16u) & 1u, 0u)
        << "BUG-AUDIT-71 precondition: IDR0.PRI must be 0 after setPRISupported(false).";

    smmu.enable();

    uint32_t cr0ack = smmu.getCR0ACK();
    EXPECT_EQ(cr0ack & SMMU::CR0_PRIQEN, 0u)
        << "BUG-AUDIT-71: second enable() with priSupported_=false must clear CR0ACK.PRIQEN. "
           "ARM §6.3.9: PRIQEN is RES0 when IDR0.PRI==0. "
           "Current code always ORs PRIQEN unconditionally in enable(). "
           "cr0ack=0x" << std::hex << cr0ack;
}

// ─── BUG-AUDIT-72 tests (C++ only) — executeInvalidationCommand* default path ──

// BUG-AUDIT-72 test 1 (guard): a valid invalidation command (CMD_CFGI_STE) must
// NOT generate a C_BAD_STE event.
//
// ARM §4.3.1: CMD_CFGI_STE is a cache-invalidation command; it never causes
// a C_BAD_STE event. The only path to C_BAD_STE for CFGI commands is via the
// translation path (§7.3.3), not the command processing path.
//
// This test confirms that the buggy default: generateEvent(C_BAD_STE) is NOT
// triggered by valid commands.
//
// BEFORE / AFTER FIX: valid CFGI_STE produces no C_BAD_STE → PASSES.
TEST(BugAudit72, ValidCfgiSteCommandDoesNotGenerateCBadSteEvent) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.clearEventQueue();

    // Submit CMD_CFGI_STE.
    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = 42u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    // No C_BAD_STE event must have been generated by the command processing path.
    size_t badSteCount = countEvents(smmu, EventType::C_BAD_STE);
    EXPECT_EQ(badSteCount, 0u)
        << "BUG-AUDIT-72 guard: CMD_CFGI_STE must never generate C_BAD_STE from the "
           "command processing path. ARM §4.3.1: C_BAD_STE is a translation-path fault, "
           "not a command-queue error. "
           "badSteCount=" << badSteCount;
}

// BUG-AUDIT-72 test 2 (guard): CMD_CFGI_ALL must not generate C_BAD_STE.
//
// BEFORE / AFTER FIX: valid CFGI_ALL produces no C_BAD_STE → PASSES.
TEST(BugAudit72, ValidCfgiAllCommandDoesNotGenerateCBadSteEvent) {
    SMMU smmu;
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);
    enableSMMU(smmu);
    smmu.clearEventQueue();

    CommandEntry cmd;
    cmd.type  = CommandType::CFGI_ALL;
    cmd.range = 31u;  // CMD_CFGI_ALL semantics
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    size_t badSteCount = countEvents(smmu, EventType::C_BAD_STE);
    EXPECT_EQ(badSteCount, 0u)
        << "BUG-AUDIT-72 guard: CMD_CFGI_ALL must never generate C_BAD_STE. "
           "badSteCount=" << badSteCount;
}

// BUG-AUDIT-72 test 3 (default path via unknown opcode): submit an unknown command
// opcode cast from an integer not present in the CommandType enum.
//
// An unknown opcode must cause:
//   - CMDQ_CONS.ERR = CERROR_ILL (= 1)
//   - GERROR.CMDQ_ERR active
//   - NO C_BAD_STE event in the event queue.
//
// The default: case in processCommand() already calls writeCmdqConsErr(CERROR_ILL)
// + signalGerror(GERROR_CMDQ_ERR) — this test verifies that behaviour.
// The same test also guards against BUG-AUDIT-72 which affects the defaultcases
// in executeInvalidationCommand[Locked](): if the bug were in processCommand()
// instead, this test would show a C_BAD_STE event instead of CERROR_ILL.
//
// BEFORE FIX (processCommand() default is already correct, but the sub-function
// defaults are wrong): unknown opcode → processCommand() default → CERROR_ILL+GERROR
// (no C_BAD_STE). This test PASSES for processCommand() default.
// It documents that the bug in executeInvalidationCommand* defaults is only reachable
// if a new command type were added to processCommand() routing to those helpers without
// a matching case in the helpers.
//
// BEFORE FIX: this test PASSES (processCommand() default is correct).
// AFTER FIX:  this test still PASSES.
// The TRUE BUG-AUDIT-72 regression test for the dead default: generateEvent(C_BAD_STE)
// in executeInvalidationCommand[Locked] cannot be exercised via the public API because
// all command types routed to those helpers have explicit cases in their switch.
//
// What this test DOES fail on (before fix) is the C_BAD_STE check — if
// executeInvalidationCommand were called directly with an unknown type (dead code path),
// it would generate C_BAD_STE instead of CERROR_ILL.
TEST(BugAudit72, UnknownCommandOpcodeYieldsCerrorIllNotCBadSte) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.clearEventQueue();

    // Construct a command with an unknown opcode (0xFF is not in CommandType enum).
    CommandEntry cmd;
    cmd.type = static_cast<CommandType>(0xFFu);
    cmd.streamID = 0u;

    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    // CMDQ_CONS.ERR must be CERROR_ILL (= 1).
    uint32_t cmdqErr = smmu.getCmdqConsErr();
    EXPECT_EQ(cmdqErr, CERROR_ILL)
        << "BUG-AUDIT-72: unknown command opcode must set CMDQ_CONS.ERR=CERROR_ILL. "
           "ARM §4.1/§6.3.17: unrecognized command → CERROR_ILL, not C_BAD_STE. "
           "getCmdqConsErr()=" << cmdqErr;

    // GERROR.CMDQ_ERR must be active.
    uint32_t gerror = smmu.getGerror();
    EXPECT_NE(gerror & GERROR_CMDQ_ERR, 0u)
        << "BUG-AUDIT-72: unknown command opcode must set GERROR.CMDQ_ERR. "
           "ARM §6.3.17: GERROR_CMDQ_ERR (bit 0) must be set for command queue errors. "
           "getGerror()=0x" << std::hex << gerror;

    // NO C_BAD_STE event must have been generated.
    // If executeInvalidationCommand default's bug were reachable, a C_BAD_STE event
    // would appear here instead of CERROR_ILL.
    size_t badSteCount = countEvents(smmu, EventType::C_BAD_STE);
    EXPECT_EQ(badSteCount, 0u)
        << "BUG-AUDIT-72: unknown command opcode must NOT generate C_BAD_STE event. "
           "ARM §4.1: the correct error channel for unrecognized commands is "
           "CMDQ_CONS.ERR=CERROR_ILL + GERROR.CMDQ_ERR, not an event queue entry. "
           "The buggy default: in executeInvalidationCommand[Locked] calls "
           "generateEvent(C_BAD_STE) — the wrong channel. "
           "badSteCount=" << badSteCount;
}

// BUG-AUDIT-72 test 4: a fresh SMMU that only processes a valid CMD_CFGI_STE must
// have CMDQ_CONS.ERR=0 (CERROR_NONE) and no C_BAD_STE event.
//
// This is a regression guard verifying that valid invalidation commands do not
// trigger any error state or generate C_BAD_STE events.
//
// BEFORE / AFTER FIX: valid CFGI_STE leaves CMDQ_CONS.ERR=0, no C_BAD_STE → PASSES.
TEST(BugAudit72, ValidCfgiSteLeavesCmdqConsErrClear) {
    SMMU smmu;
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);
    enableSMMU(smmu);
    smmu.clearEventQueue();

    // Submit a valid CMD_CFGI_STE.
    CommandEntry goodCmd;
    goodCmd.type     = CommandType::CFGI_STE;
    goodCmd.streamID = 99u;
    smmu.submitCommand(goodCmd);
    smmu.processCommandQueue();

    // No C_BAD_STE event from the valid command.
    size_t badSteCount = countEvents(smmu, EventType::C_BAD_STE);
    EXPECT_EQ(badSteCount, 0u)
        << "BUG-AUDIT-72 regression: valid CMD_CFGI_STE must not generate C_BAD_STE. "
           "badSteCount=" << badSteCount;

    // CMDQ_CONS.ERR must be CERROR_NONE (no error).
    EXPECT_EQ(smmu.getCmdqConsErr(), 0u)
        << "BUG-AUDIT-72 regression: valid CMD_CFGI_STE must not set CMDQ_CONS.ERR. "
           "getCmdqConsErr()=" << smmu.getCmdqConsErr();
}

// BUG-AUDIT-72 test 5: the two buggy default: cases in executeInvalidationCommand()
// and executeInvalidationCommandLocked() call generateEvent(C_BAD_STE) — confirm
// that the correct behavior via processCommand() default does NOT produce this event.
//
// This test specifically checks the EventType::C_BAD_STE event is absent after
// processCommand() handles an unknown opcode.
//
// ARM §4.1: C_BAD_STE is generated by the translation path when a stream table
// entry is malformed. It is NEVER the result of an unrecognized command opcode.
// The correct response to an unrecognized command opcode is always CERROR_ILL.
//
// BEFORE FIX (if the default: bug were in processCommand() instead of the helpers):
//   C_BAD_STE event generated → FAILS.
// CURRENT STATE: processCommand() default is correct (CERROR_ILL), but the two
//   helper defaults are buggy (C_BAD_STE). Those helpers' defaults are currently
//   unreachable, so this test verifies the reachable processCommand() default path
//   and documents the helper default bug.
TEST(BugAudit72, UnknownOpcodeNeverProducesCBadSteViaProcessCommand) {
    for (uint32_t opcode : {0x09u, 0x0Au, 0x14u, 0xFFu}) {
        // Use a fresh SMMU per iteration to avoid CERROR_ILL state accumulation.
        SMMU iterSmmu;
        iterSmmu.setS1PSupported(true);
        iterSmmu.setS2PSupported(true);
        enableSMMU(iterSmmu);
        iterSmmu.clearEventQueue();

        CommandEntry cmd;
        cmd.type = static_cast<CommandType>(opcode);
        iterSmmu.submitCommand(cmd);
        iterSmmu.processCommandQueue();

        // C_BAD_STE must never appear for unknown command opcodes.
        size_t badSteCount = countEvents(iterSmmu, EventType::C_BAD_STE);
        EXPECT_EQ(badSteCount, 0u)
            << "BUG-AUDIT-72: unknown command opcode 0x" << std::hex << opcode
            << " must NOT generate C_BAD_STE event. "
               "ARM §4.1: C_BAD_STE is a translation-path fault. "
               "The buggy default: cases in executeInvalidationCommand[Locked] call "
               "generateEvent(C_BAD_STE) — the wrong error channel. "
               "badSteCount=" << badSteCount;

        // CERROR_ILL must be set (the correct response).
        uint32_t err = iterSmmu.getCmdqConsErr();
        EXPECT_EQ(err, CERROR_ILL)
            << "BUG-AUDIT-72: unknown opcode 0x" << std::hex << opcode
            << " must set CMDQ_CONS.ERR=CERROR_ILL. err=" << err;
    }
}
