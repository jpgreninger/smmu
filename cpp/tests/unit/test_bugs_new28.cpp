// TDD failing tests for BUG-AUDIT-65 through BUG-AUDIT-68.
//
// BUG-AUDIT-65 (Both §4.7.3): CMD_SYNC CS=0b10 (SIG_SEV) is not gated on IDR0.SEV.
//   Spec §4.7.3: "Use of SIG_SEV only sends an event when SMMU_IDR0.SEV is set,
//   otherwise, no event is available to a PE and SIG_SEV is equivalent to SIG_NONE."
//   BEFORE FIX: sevSupported_==false is ignored; CmdSyncSignalType::Sev is stored and
//               COMMAND_SYNC_COMPLETION event is generated regardless.
//   AFTER FIX:  when sevSupported_==false, CS=2 is treated as SIG_NONE — no signal
//               stored, no completion event generated.
//
// BUG-AUDIT-66 (Both §6.3.11): setCR1() uses a single combined guard
//   (SMMUEN | CMDQEN | EVENTQEN | PRIQEN) for both TABLE_* and QUEUE_* fields.
//   Spec §6.3.11:
//     TABLE_* (bits[11:6]): RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
//     QUEUE_* (bits[5:0]):  RO when ANY of CMDQEN/EVENTQEN/PRIQEN is 1, NOT gated by SMMUEN.
//   BEFORE FIX:
//     (a) SMMUEN=1 + all queues disabled → QUEUE_* writes blocked (wrong; spec says writable).
//     (b) SMMUEN=0 + queues enabled → TABLE_* writes blocked (wrong; spec says writable).
//   AFTER FIX:
//     (a) QUEUE_* is only blocked by queue-enable bits; SMMUEN alone does not block them.
//     (b) TABLE_* is only blocked by SMMUEN; queue enables alone do not block them.
//
// BUG-AUDIT-67 (Both §6.3.12/6.3.24/6.3.25): setCR2/setStrtabFormat/setStrtabSplit/
//   setStrtabLog2Size check only CR0.SMMUEN; they should also check CR0ACK.SMMUEN.
//   Spec: these registers are RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
//   NOTE: in this synchronous model CR0ACK always mirrors CR0, so this is functionally
//   benign. Tests verify the CR0 guard works and document the spec intent.
//
// BUG-AUDIT-68 (Both §6.3.11): setCR1() TABLE_* guard missing CR0ACK.SMMUEN check.
//   Spec: TABLE_* is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
//   Current code only checks CR0.SMMUEN.
//   NOTE: functionally benign in synchronous model; tests document spec compliance.

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

// Helper: submit and process a CMD_SYNC with the given CS value.
static void submitSync(SMMU& s, uint8_t cs) {
    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = cs;
    s.submitCommand(sync);
    s.processCommandQueue();
}

// ─── BUG-AUDIT-65 tests — CMD_SYNC SIG_SEV gated on IDR0.SEV ─────────────────

// BUG-AUDIT-65 test 1: CS=2 (SIG_SEV) with sev_supported=false must behave as SIG_NONE.
//
// ARM §4.7.3: "Use of SIG_SEV only sends an event when SMMU_IDR0.SEV is set,
// otherwise, no event is available to a PE and SIG_SEV is equivalent to SIG_NONE."
//
// When IDR0.SEV==0 (setSevSupported(false)), a CMD_SYNC with CS=2 must:
//   - NOT store CmdSyncSignalType::Sev in cmdSyncLastSig_.
//   - NOT generate a COMMAND_SYNC_COMPLETION event.
//
// BEFORE FIX: cmdSyncLastSig_ is set to Sev and completion event is generated → FAILS.
// AFTER FIX:  CS=2 is treated as SIG_NONE → signal remains None, no event → PASSES.
TEST(BugAudit65, SevCS2WithSevUnsupportedIsNoop) {
    SMMU smmu;
    enableSMMU(smmu);

    // IDR0.SEV=0 (default).
    smmu.setSevSupported(false);
    ASSERT_EQ((smmu.getIDR0() >> 14u) & 1u, 0u)
        << "BUG-AUDIT-65 precondition: IDR0.SEV must be 0 when setSevSupported(false).";

    // Clear any pre-existing events.
    smmu.clearEventQueue();

    // Submit CMD_SYNC with CS=2 (SIG_SEV).
    submitSync(smmu, 2u);

    // Signal type must NOT have been updated to Sev.
    CmdSyncSignalType sig = smmu.getCmdSyncLastSignalType();
    EXPECT_NE(sig, CmdSyncSignalType::Sev)
        << "BUG-AUDIT-65: CS=2 with IDR0.SEV==0 must NOT set signal type to Sev. "
           "ARM §4.7.3: SIG_SEV is equivalent to SIG_NONE when IDR0.SEV=0. "
           "Current code stores Sev unconditionally (ignores sevSupported_). "
           "signal_type=" << static_cast<int>(sig);

    // No COMMAND_SYNC_COMPLETION event must have been generated.
    std::vector<EventEntry> events = smmu.getEventQueue();
    size_t completions = 0u;
    for (const auto& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            ++completions;
        }
    }
    EXPECT_EQ(completions, 0u)
        << "BUG-AUDIT-65: CS=2 with IDR0.SEV==0 must NOT generate COMMAND_SYNC_COMPLETION. "
           "ARM §4.7.3: SIG_SEV == SIG_NONE when IDR0.SEV=0; no event should be written. "
           "Current code generates the completion event unconditionally. "
           "completions=" << completions;
}

// BUG-AUDIT-65 test 2: CS=2 (SIG_SEV) with sev_supported=true must behave normally.
//
// When IDR0.SEV==1 (setSevSupported(true)), a CMD_SYNC with CS=2 must:
//   - Store CmdSyncSignalType::Sev in cmdSyncLastSig_.
//   - Generate a COMMAND_SYNC_COMPLETION event.
//
// BEFORE FIX: this test PASSES (Sev is always stored regardless of sevSupported_).
// AFTER FIX:  this test still PASSES (now gated correctly on sevSupported_=true).
TEST(BugAudit65, SevCS2WithSevSupportedGeneratesEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    // Enable IDR0.SEV.
    smmu.setSevSupported(true);
    ASSERT_NE((smmu.getIDR0() >> 14u) & 1u, 0u)
        << "BUG-AUDIT-65 precondition: IDR0.SEV must be 1 when setSevSupported(true).";

    smmu.clearEventQueue();

    // Submit CMD_SYNC with CS=2 (SIG_SEV).
    submitSync(smmu, 2u);

    // Signal type must be Sev.
    CmdSyncSignalType sig = smmu.getCmdSyncLastSignalType();
    EXPECT_EQ(sig, CmdSyncSignalType::Sev)
        << "BUG-AUDIT-65: CS=2 with IDR0.SEV==1 must store CmdSyncSignalType::Sev. "
           "ARM §4.7.3: SIG_SEV is operational when IDR0.SEV=1. "
           "signal_type=" << static_cast<int>(sig);

    // Exactly one COMMAND_SYNC_COMPLETION event must have been generated.
    std::vector<EventEntry> events = smmu.getEventQueue();
    size_t completions = 0u;
    for (const auto& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            ++completions;
        }
    }
    EXPECT_EQ(completions, 1u)
        << "BUG-AUDIT-65: CS=2 with IDR0.SEV==1 must generate exactly one "
           "COMMAND_SYNC_COMPLETION event. ARM §4.7.3. "
           "completions=" << completions;
}

// BUG-AUDIT-65 test 3: CS=1 (SIG_IRQ) must always work regardless of sev_supported.
//
// SIG_IRQ is not affected by IDR0.SEV; it must always generate a completion event.
// This test verifies no regression from the BUG-AUDIT-65 fix.
//
// BEFORE / AFTER FIX: CS=1 always works → PASSES.
TEST(BugAudit65, IrqCS1AlwaysWorksRegardlessOfSevSupported) {
    SMMU smmu;
    enableSMMU(smmu);

    // IDR0.SEV=0.
    smmu.setSevSupported(false);
    smmu.clearEventQueue();

    // Submit CMD_SYNC with CS=1 (SIG_IRQ).
    submitSync(smmu, 1u);

    // Signal type must be Irq.
    EXPECT_EQ(smmu.getCmdSyncLastSignalType(), CmdSyncSignalType::Irq)
        << "BUG-AUDIT-65 regression: CS=1 (SIG_IRQ) must always store CmdSyncSignalType::Irq "
           "regardless of IDR0.SEV. ARM §4.7.3: SIG_IRQ is independent of SEV.";

    // Exactly one completion event must be generated.
    std::vector<EventEntry> events = smmu.getEventQueue();
    size_t completions = 0u;
    for (const auto& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            ++completions;
        }
    }
    EXPECT_EQ(completions, 1u)
        << "BUG-AUDIT-65 regression: CS=1 (SIG_IRQ) must generate exactly one "
           "COMMAND_SYNC_COMPLETION event regardless of IDR0.SEV. "
           "completions=" << completions;
}

// ─── BUG-AUDIT-66 tests — setCR1() split guard for TABLE_* vs QUEUE_* ─────────

// BUG-AUDIT-66 test 4: QUEUE_* fields must be writable when SMMUEN=1 but all
// queue-enables are cleared.
//
// ARM §6.3.11: QUEUE_* (bits[5:0]) are RO only when CMDQEN=1 OR EVENTQEN=1 OR PRIQEN=1.
// SMMUEN alone must NOT prevent QUEUE_* writes.
//
// Scenario: Enable SMMU+queues. Disable all queues (CR0=SMMUEN only). Attempt
// to write QUEUE_SH. The write must succeed (bits must update).
//
// BEFORE FIX: combined guard (SMMUEN | CMDQEN | EVENTQEN | PRIQEN) blocks the entire
//             write when SMMUEN=1 even though no queues are enabled → FAILS.
// AFTER FIX:  QUEUE_* are written when all queue-enables are cleared, even if SMMUEN=1
//             → PASSES.
TEST(BugAudit66, QueueFieldsWritableWhenSMMUENSetButQueuesDisabled) {
    SMMU smmu;
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);

    // Set CR1 to a known initial value (before enabling anything).
    smmu.setCR1(0u);
    ASSERT_EQ(smmu.getCR1(), 0u)
        << "BUG-AUDIT-66 precondition: setCR1(0) before enable must succeed.";

    // Enable all queues + SMMUEN.
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);

    // Now disable all queues but keep SMMUEN.
    smmu.setCR0(SMMU::CR0_SMMUEN);
    ASSERT_EQ(smmu.getCR0() & (SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN), 0u)
        << "BUG-AUDIT-66 precondition: all queue-enables must be cleared.";
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-66 precondition: SMMUEN must still be set.";

    // Attempt to write QUEUE_SH (bits[5:4]).
    const uint32_t queueShVal = SMMU::CR1_QUEUE_SH;  // e.g. 0x30
    smmu.setCR1(queueShVal);

    // QUEUE_* must have been written because no queue is enabled.
    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & SMMU::CR1_QUEUE_SH, queueShVal)
        << "BUG-AUDIT-66: QUEUE_* bits must be writable when SMMUEN=1 but all queue-enables "
           "are cleared. ARM §6.3.11: QUEUE_* is RO only when a queue-enable bit is set. "
           "SMMUEN alone must not block QUEUE_* writes. "
           "Current code uses a combined guard (SMMUEN | CMDQEN | EVENTQEN | PRIQEN) that "
           "incorrectly prevents QUEUE_* writes when SMMUEN=1 even with no queues enabled. "
           "cr1=0x" << std::hex << cr1;
}

// BUG-AUDIT-66 test 5: TABLE_* fields must be writable when queues are enabled
// but SMMUEN=0.
//
// ARM §6.3.11: TABLE_* (bits[11:6]) are RO only when SMMUEN=1 OR CR0ACK.SMMUEN=1.
// Queue-enables alone (with SMMUEN=0) must NOT prevent TABLE_* writes.
//
// Scenario: Set CR1 to 0 with all disabled. Enable CMDQEN+EVENTQEN (not SMMUEN).
// Attempt to write TABLE_SH. The write must succeed.
//
// BEFORE FIX: combined guard blocks the write because CMDQEN+EVENTQEN are set → FAILS.
// AFTER FIX:  TABLE_* written because SMMUEN=0 → PASSES.
TEST(BugAudit66, TableFieldsWritableWhenQueuesEnabledButSMMUENCleared) {
    SMMU smmu;
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);

    // Set CR1 to 0 before any enables.
    smmu.setCR1(0u);
    ASSERT_EQ(smmu.getCR1(), 0u)
        << "BUG-AUDIT-66 precondition: setCR1(0) must succeed before enabling.";

    // Enable CMDQEN and EVENTQEN but NOT SMMUEN.
    smmu.setCR0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-66 precondition: SMMUEN must be 0.";
    ASSERT_NE(smmu.getCR0() & (SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN), 0u)
        << "BUG-AUDIT-66 precondition: CMDQEN+EVENTQEN must be set.";

    // Attempt to write TABLE_SH (bits[11:10]).
    const uint32_t tableShVal = SMMU::CR1_TABLE_SH;  // e.g. 0xC00
    smmu.setCR1(tableShVal);

    // TABLE_* must have been written because SMMUEN=0.
    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & SMMU::CR1_TABLE_SH, tableShVal)
        << "BUG-AUDIT-66: TABLE_* bits must be writable when SMMUEN=0, even if queue-enable "
           "bits are set. ARM §6.3.11: TABLE_* is RO only when SMMUEN=1. "
           "Queue enables alone must not block TABLE_* writes. "
           "Current code uses a combined guard that incorrectly blocks TABLE_* when "
           "CMDQEN or EVENTQEN is set (regardless of SMMUEN). "
           "cr1=0x" << std::hex << cr1;
}

// ─── BUG-AUDIT-67 tests — setCR2/STRTAB guards missing CR0ACK.SMMUEN ─────────

// BUG-AUDIT-67 test 6: setCR2() guard — verify CR0.SMMUEN blocks the write.
//
// ARM §6.3.12: SMMU_CR2 is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
// Current code checks only CR0.SMMUEN (which equals CR0ACK.SMMUEN in the
// synchronous model), so the guard still fires correctly in practice.
//
// This test verifies the existing CR0.SMMUEN guard works correctly for setCR2().
// It also documents that CR0ACK (which mirrors CR0 in the sync model) would need
// to be checked independently for full spec compliance.
//
// BEFORE / AFTER FIX: setCR2() while SMMUEN=1 is blocked → PASSES.
TEST(BugAudit67, SetCR2BlockedBySMMUEN) {
    SMMU smmu;

    // Set a known CR2 value before enabling.
    smmu.setCR2(SMMU::CR2_PTM);
    ASSERT_EQ(smmu.getCR2() & SMMU::CR2_PTM, SMMU::CR2_PTM)
        << "BUG-AUDIT-67 precondition: setCR2(CR2_PTM) before enable must succeed.";

    // Enable SMMU (sets CR0.SMMUEN=1 which also sets CR0ACK.SMMUEN=1 in sync model).
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);

    // Attempt to clear CR2 while SMMUEN=1 — must be silently ignored.
    smmu.setCR2(0u);

    uint32_t cr2 = smmu.getCR2();
    EXPECT_EQ(cr2 & SMMU::CR2_PTM, SMMU::CR2_PTM)
        << "BUG-AUDIT-67: setCR2() must be silently ignored when CR0.SMMUEN=1. "
           "ARM §6.3.12: SMMU_CR2 is RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. "
           "cr2=0x" << std::hex << cr2;
}

// BUG-AUDIT-67 test 7: setCR2() is writable when SMMUEN=0 and CR0ACK.SMMUEN=0.
//
// When both CR0.SMMUEN and CR0ACK.SMMUEN are 0, setCR2() must succeed.
// In the synchronous model these are always equal, so this tests the baseline case.
//
// BEFORE / AFTER FIX: setCR2() while SMMUEN=0 succeeds → PASSES.
TEST(BugAudit67, SetCR2WritableWhenSMMUENCleared) {
    SMMU smmu;

    // Enable then disable to confirm SMMUEN=0.
    smmu.setCR0(SMMU::CR0_SMMUEN);
    smmu.setCR0(0u);
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-67 precondition: SMMUEN must be 0.";
    ASSERT_EQ(smmu.getCR0ACK() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-67 precondition: CR0ACK.SMMUEN must be 0 (sync model mirrors CR0).";

    // Write CR2 — must succeed.
    smmu.setCR2(SMMU::CR2_RECINVSID);

    uint32_t cr2 = smmu.getCR2();
    EXPECT_EQ(cr2 & SMMU::CR2_RECINVSID, SMMU::CR2_RECINVSID)
        << "BUG-AUDIT-67: setCR2() must succeed when CR0.SMMUEN=0 and CR0ACK.SMMUEN=0. "
           "ARM §6.3.12: SMMU_CR2 is only RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. "
           "cr2=0x" << std::hex << cr2;
}

// BUG-AUDIT-67 test 8: setCR0ACK() is a public API — verify CR0ACK independently
// tracks CR0 in the synchronous model.
//
// In the synchronous model, setCR0() always writes both CR0 and CR0ACK atomically.
// This test verifies that CR0ACK can also be set independently via setCR0ACK().
// (If a future implementation supports asynchronous CR0ACK update, the guards
// in setCR2/setStrtab* must use CR0ACK.SMMUEN as well.)
//
// BEFORE / AFTER FIX: CR0ACK mirrors CR0 after setCR0() → PASSES.
TEST(BugAudit67, CR0ACKMirrorsCR0AfterSetCR0) {
    SMMU smmu;

    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    uint32_t cr0 = smmu.getCR0();
    uint32_t cr0ack = smmu.getCR0ACK();

    EXPECT_EQ(cr0 & SMMU::CR0_SMMUEN, cr0ack & SMMU::CR0_SMMUEN)
        << "BUG-AUDIT-67: CR0ACK.SMMUEN must mirror CR0.SMMUEN in the synchronous model. "
           "ARM §6.3.10: SMMU_CR0ACK is the hardware acknowledgement of SMMU_CR0. "
           "cr0=0x" << std::hex << cr0 << " cr0ack=0x" << cr0ack;

    // Verify setCR0ACK() directly sets CR0ACK.
    smmu.setCR0ACK(0u);
    EXPECT_EQ(smmu.getCR0ACK() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-67: setCR0ACK(0) must clear CR0ACK.SMMUEN. "
           "This API enables future testing of async CR0ACK update paths.";
}

// ─── BUG-AUDIT-68 tests — setCR1() TABLE_* guard missing CR0ACK.SMMUEN ───────

// BUG-AUDIT-68 test 9: TABLE_* write guard checks CR0.SMMUEN.
//
// ARM §6.3.11: TABLE_* (bits[11:6]) are RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
// Current code only checks CR0.SMMUEN. In the synchronous model CR0ACK always mirrors
// CR0, so this is functionally benign, but testing the existing guard is important.
//
// This test verifies that setCR1() TABLE_* write is blocked when CR0.SMMUEN=1.
//
// BEFORE / AFTER FIX: TABLE_* write blocked when SMMUEN=1 → PASSES.
TEST(BugAudit68, TableFieldsBlockedBySMMUEN) {
    SMMU smmu;

    // Set TABLE_SH before enabling.
    smmu.setCR1(0u);
    ASSERT_EQ(smmu.getCR1(), 0u)
        << "BUG-AUDIT-68 precondition: setCR1(0) must succeed before enable.";

    // Enable SMMU.
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);

    // Attempt to write TABLE_SH while SMMUEN=1 — must be blocked.
    smmu.setCR1(SMMU::CR1_TABLE_SH);

    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & SMMU::CR1_TABLE_SH, 0u)
        << "BUG-AUDIT-68: TABLE_* bits must be RO when CR0.SMMUEN=1. "
           "ARM §6.3.11: TABLE_* is RO when SMMUEN=1 OR CR0ACK.SMMUEN=1. "
           "cr1=0x" << std::hex << cr1;
}

// BUG-AUDIT-68 test 10: setCR0ACK() sets CR0ACK independently from CR0;
// TABLE_* guard behavior when CR0.SMMUEN cleared but CR0ACK.SMMUEN still set.
//
// ARM §6.3.11: TABLE_* is RO when CR0.SMMUEN==1 OR CR0ACK.SMMUEN==1.
// Current code checks only CR0.SMMUEN. This test artificially sets CR0ACK.SMMUEN=1
// while clearing CR0.SMMUEN=0 to expose the missing guard.
//
// Scenario:
//   1. Enable SMMU (CR0.SMMUEN=1, CR0ACK.SMMUEN=1).
//   2. Manually clear CR0 only (CR0.SMMUEN=0) while leaving CR0ACK.SMMUEN=1
//      via setCR0ACK() called after clearing CR0.
//   3. Attempt TABLE_* write — spec says it must be blocked because CR0ACK.SMMUEN=1.
//
// BEFORE FIX: setCR1() TABLE_* only checks CR0.SMMUEN; with CR0.SMMUEN=0 but
//             CR0ACK.SMMUEN=1, the write proceeds → TABLE_* bits get set → FAILS.
// AFTER FIX:  setCR1() also checks CR0ACK.SMMUEN; write is blocked → PASSES.
TEST(BugAudit68, TableFieldsBlockedByCR0ACKSMMUEN) {
    SMMU smmu;

    // Set CR1 to 0 initially.
    smmu.setCR1(0u);
    ASSERT_EQ(smmu.getCR1(), 0u)
        << "BUG-AUDIT-68 precondition: setCR1(0) must succeed.";

    // Enable SMMU (sets both CR0.SMMUEN and CR0ACK.SMMUEN to 1).
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    smmu.setS1PSupported(true);
    smmu.setS2PSupported(true);
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-68 precondition: CR0.SMMUEN must be 1.";
    ASSERT_NE(smmu.getCR0ACK() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-68 precondition: CR0ACK.SMMUEN must be 1.";

    // Manually clear CR0.SMMUEN while leaving CR0ACK.SMMUEN=1 via direct setCR0ACK().
    // First clear CR0 so the SMMU sees SMMUEN=0.
    smmu.setCR0(0u);
    // Restore CR0ACK.SMMUEN=1 to simulate an async hardware-not-yet-acknowledged state.
    smmu.setCR0ACK(SMMU::CR0_SMMUEN);

    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-68 precondition: CR0.SMMUEN must be 0 after clearing CR0.";
    ASSERT_NE(smmu.getCR0ACK() & SMMU::CR0_SMMUEN, 0u)
        << "BUG-AUDIT-68 precondition: CR0ACK.SMMUEN must still be 1.";

    // Attempt TABLE_* write — spec says must be blocked (CR0ACK.SMMUEN=1).
    smmu.setCR1(SMMU::CR1_TABLE_SH);

    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & SMMU::CR1_TABLE_SH, 0u)
        << "BUG-AUDIT-68: TABLE_* must be RO when CR0ACK.SMMUEN=1 even if CR0.SMMUEN=0. "
           "ARM §6.3.11: TABLE_* guard must include CR0ACK.SMMUEN. "
           "Current code only checks CR0.SMMUEN; the CR0ACK.SMMUEN guard is missing. "
           "cr1=0x" << std::hex << cr1;
}
