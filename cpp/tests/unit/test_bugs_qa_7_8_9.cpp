// TDD failing tests for BUG-QA-7, BUG-QA-8, BUG-QA-9
// (C++ implementation).
//
// Each test is written to FAIL with the current code and PASS only after the
// corresponding fix is applied.  No fixes are included here.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-QA-7 (§4.7.3): CMD_SYNC CS=0b10 mislabeled as MSI instead of SEV.
//   ARM spec §4.7.3 defines:
//     CS=0b00 = SIG_NONE  (no signal)
//     CS=0b01 = SIG_IRQ   (interrupt/MSI write)
//     CS=0b10 = SIG_SEV   (PE-level wakeup event, NOT MSI)
//     CS=0b11 = Reserved  → CERROR_ILL
//   Current code has CmdSyncSignalType::Msi = 2.  This is wrong; CS=2 is
//   SIG_SEV.  The enum value must be renamed to CmdSyncSignalType::Sev.
//
// BUG-QA-8 (§7.3.12): F_WALK_EABT CLASS field must be 0b01 (TT).
//   ARM spec §7.3.12 states CLASS=0b01 (TT — translation table descriptor fetch)
//   for F_WALK_EABT events.  Currently no dedicated case exists in the
//   generateEvent() eventClass switch, so F_WALK_EABT falls to default which
//   sets eventClass=0 (CD).  Must be 1 (TT).
//
// BUG-QA-9 (§4.4.2.5/§4.4.2.6): CMD_TLBI_EL3_ALL and CMD_TLBI_EL3_VA must
//   raise CERROR_ILL on the Non-secure command queue.
//   ARM spec §4.4.2.5/§4.4.2.6: "This command is valid only on the Secure
//   Command queue. If issued on the Non-secure Command queue, CERROR_ILL is
//   raised."  This model has only a Non-secure queue, so both commands must
//   ALWAYS raise CERROR_ILL and must NOT perform any TLB invalidation.
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

// Configure a minimal stream so TLBI commands have a valid target.
static void configureStream(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.stage1Enabled   = false;
    cfg.stage2Enabled   = false;
    cfg.bypassEnabled   = true;
    cfg.securityState   = SecurityState::NonSecure;
    s.configureStream(sid, cfg);
}

} // anonymous namespace

// ============================================================================
// BUG-QA-7: CmdSyncSignalType::Sev must exist (CS=2 is SIG_SEV, not SIG_MSI)
// ============================================================================

// Verify that the enum has a Sev member with numeric value 2.
// BEFORE FIX: CmdSyncSignalType::Sev does not compile (member named Msi).
// AFTER FIX:  CmdSyncSignalType::Sev exists with value 2.
TEST(BugQA7CmdSyncSev, SevEnumMemberExists) {
    // This test simply checks that the Sev enumerator exists (compile-time).
    CmdSyncSignalType sev = CmdSyncSignalType::Sev;
    EXPECT_EQ(static_cast<uint8_t>(sev), 2u)
        << "BUG-QA-7: CmdSyncSignalType::Sev must equal 2 (CS=0b10 per ARM §4.7.3)";
}

// Verify that getCmdSyncLastSignalType() returns Sev after processing CS=2.
// BEFORE FIX: returns CmdSyncSignalType::Msi (wrong name, same value).
// AFTER FIX:  returns CmdSyncSignalType::Sev.
TEST(BugQA7CmdSyncSev, CS2ReturnsSevSignalType) {
    SMMU smmu;
    smmu.setSevSupported(true);
    enableSMMU(smmu);

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = 2u;  // SIG_SEV per ARM §4.7.3
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    // Must return Sev (= 2), not Msi (old wrong name).
    CmdSyncSignalType sig = smmu.getCmdSyncLastSignalType();
    EXPECT_EQ(sig, CmdSyncSignalType::Sev)
        << "BUG-QA-7: CS=2 must store CmdSyncSignalType::Sev (§4.7.3 SIG_SEV)";
    EXPECT_EQ(static_cast<uint8_t>(sig), 2u)
        << "BUG-QA-7: CmdSyncSignalType::Sev must have numeric value 2";
}

// Verify that CS=1 still returns Irq (no regression).
TEST(BugQA7CmdSyncSev, CS1StillReturnsIrqSignalType) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = 1u;  // SIG_IRQ
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdSyncLastSignalType(), CmdSyncSignalType::Irq)
        << "BUG-QA-7: CS=1 must still return CmdSyncSignalType::Irq (no regression)";
}

// Verify that Sev != None and Sev != Irq (distinct values).
TEST(BugQA7CmdSyncSev, SevIsDistinctFromNoneAndIrq) {
    EXPECT_NE(CmdSyncSignalType::Sev, CmdSyncSignalType::None);
    EXPECT_NE(CmdSyncSignalType::Sev, CmdSyncSignalType::Irq);
    EXPECT_NE(CmdSyncSignalType::Irq, CmdSyncSignalType::None);
}

// ============================================================================
// BUG-QA-8: F_WALK_EABT eventClass must be 1 (TT per §7.3.12)
// ============================================================================

// Inject an F_WALK_EABT event and verify eventClass == 1 (TT).
// BEFORE FIX: eventClass == 0 (falls through to default).
// AFTER FIX:  eventClass == 1 (TT).
TEST(BugQA8WalkEabtClass, FWalkEabtEventClassIsTT) {
    SMMU smmu;
    enableSMMU(smmu);
    configureStream(smmu, 0x10u);

    smmu.injectWalkEabt(0x10u, 0u, 0x1000u);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-QA-8: expected at least one event after injectWalkEabt()";

    // Find the F_WALK_EABT event.
    const EventEntry* walkEabt = nullptr;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_WALK_EABT) {
            walkEabt = &ev;
            break;
        }
    }
    ASSERT_NE(walkEabt, nullptr)
        << "BUG-QA-8: no F_WALK_EABT event found in event queue";

    EXPECT_EQ(walkEabt->eventClass, 1u)
        << "BUG-QA-8: F_WALK_EABT eventClass must be 1 (TT) per ARM §7.3.12 "
           "(current default sets 0 = CD)";
}

// Verify that multiple injected F_WALK_EABT events all carry eventClass=1.
TEST(BugQA8WalkEabtClass, MultipleWalkEabtEventsAllTT) {
    SMMU smmu;
    enableSMMU(smmu);
    configureStream(smmu, 0x20u);

    smmu.injectWalkEabt(0x20u, 0u, 0x2000u);
    smmu.injectWalkEabt(0x20u, 0u, 0x3000u);

    std::vector<EventEntry> events = smmu.getEventQueue();

    int count = 0;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_WALK_EABT) {
            EXPECT_EQ(ev.eventClass, 1u)
                << "BUG-QA-8: all F_WALK_EABT events must have eventClass=1 (TT)";
            ++count;
        }
    }
    EXPECT_EQ(count, 2)
        << "BUG-QA-8: expected 2 F_WALK_EABT events";
}

// ============================================================================
// BUG-QA-9: CMD_TLBI_EL3_ALL must raise CERROR_ILL on the NS queue (§4.4.2.5)
// ============================================================================

// Submit CMD_TLBI_EL3_ALL and verify CMDQ_CONS.ERR == CERROR_ILL.
// BEFORE FIX: performs TLB flush (no CERROR_ILL raised).
// AFTER FIX:  CERROR_ILL is written to CMDQ_CONS.ERR; GERROR.CMDQ_ERR toggled.
TEST(BugQA9TlbiEl3CerrorIll, TlbiEl3AllRaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);
    configureStream(smmu, 0x01u);

    CommandEntry cmd;
    cmd.type = CommandType::TLBI_EL3_ALL;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-QA-9: CMD_TLBI_EL3_ALL on NS queue must set CERROR_ILL (§4.4.2.5)";

    // GERROR.CMDQ_ERR (bit 0) must be active.
    uint32_t gerr = smmu.getGerror();
    EXPECT_NE(gerr & 0x1u, 0u)
        << "BUG-QA-9: GERROR.CMDQ_ERR must be active after CMD_TLBI_EL3_ALL";
}

// Submit CMD_TLBI_EL3_VA and verify CMDQ_CONS.ERR == CERROR_ILL.
// BEFORE FIX: performs TLB flush (no CERROR_ILL raised).
// AFTER FIX:  CERROR_ILL is written to CMDQ_CONS.ERR.
TEST(BugQA9TlbiEl3CerrorIll, TlbiEl3VaRaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);
    configureStream(smmu, 0x02u);

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_EL3_VA;
    cmd.startAddress = 0x4000u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-QA-9: CMD_TLBI_EL3_VA on NS queue must set CERROR_ILL (§4.4.2.6)";

    uint32_t gerr = smmu.getGerror();
    EXPECT_NE(gerr & 0x1u, 0u)
        << "BUG-QA-9: GERROR.CMDQ_ERR must be active after CMD_TLBI_EL3_VA";
}

// Verify that CMD_TLBI_EL3_ALL does NOT flush TLB entries.
// After mapping a page, issuing CMD_TLBI_EL3_ALL, and translating again,
// the translation must still succeed (TLB was not invalidated).
// BEFORE FIX: TLB is flushed → second translation re-walks and succeeds anyway,
//   but no CERROR_ILL is raised (wrong).
// AFTER FIX:  CERROR_ILL raised; the TLB is not touched.
//   (We verify the error is raised; TLB contents are implementation-internal.)
TEST(BugQA9TlbiEl3CerrorIll, TlbiEl3AllDoesNotSilentlySucceed) {
    SMMU smmu;
    enableSMMU(smmu);
    configureStream(smmu, 0x03u);

    // Confirm no error before the command.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE);

    CommandEntry cmd;
    cmd.type = CommandType::TLBI_EL3_ALL;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    // After the command, there MUST be an error — it must not silently succeed.
    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_NONE)
        << "BUG-QA-9: CMD_TLBI_EL3_ALL must not succeed silently on NS queue (§4.4.2.5)";
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-QA-9: error code must be CERROR_ILL specifically";
}

// Verify queue halts after CERROR_ILL: a subsequent valid command is NOT
// processed until the error is acknowledged (ARM §6.3.17).
TEST(BugQA9TlbiEl3CerrorIll, QueueHaltsAfterCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    // Submit the illegal command followed by a valid CFGI_ALL.
    CommandEntry bad;
    bad.type = CommandType::TLBI_EL3_ALL;
    smmu.submitCommand(bad);

    CommandEntry good;
    good.type = CommandType::CFGI_ALL;
    smmu.submitCommand(good);

    smmu.processCommandQueue();

    // The bad command should have set CERROR_ILL and halted the queue.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-QA-9: CERROR_ILL must be set after CMD_TLBI_EL3_ALL";

    // The second command must still be in the queue (not consumed).
    // Because the queue halted on the bad command (which stays at the front),
    // commandQueue size should still be 2 (bad + good).
    EXPECT_GE(smmu.getCommandQueueSize(), 1u)
        << "BUG-QA-9: queue must halt; subsequent valid command must not be consumed";
}
