// ARM SMMU v3 FINDING-M-06: GERROR register — command queue error conditions
//
// TDD spec tests for SMMU_GERROR global error status register.
//
// Spec: ARM IHI0070G.b §6.3.17 (SMMU_GERROR), §6.3.18 (SMMU_GERRORN),
//       §7.5 (Global error recording)
//
// Requirements:
// - GERROR must be 0 after construction and reset.
// - GERROR_CMDQ_ERR (bit 7) must be set when processCommand handles an unknown
//   command type (default: arm in the command switch).
// - getGerror() reads the current GERROR value.
// - clearGerror(bits) clears only the specified bits (SMMU_GERRORN semantics).
// - GERROR bit constants must match ARM IHI0070G.b §6.3.17 values.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Constants ──────────────────────────────────────────────────────────────

static constexpr StreamID TEST_STREAM = 0x10;
static constexpr PASID    TEST_PASID  = 0;

// ── Fixture ────────────────────────────────────────────────────────────────

class GErrorTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();
        // ARM §6.3.9: SMMU starts disabled; enable globally before tests.
        smmu->enable();
    }

    std::unique_ptr<SMMU> smmu;
};

// ── §6.3.17: GERROR reset state ────────────────────────────────────────────

/// §6.3.17: GERROR must be 0 after construction.
TEST_F(GErrorTest, GerrorZeroAfterConstruction) {
    EXPECT_EQ(smmu->getGerror(), 0u)
        << "SMMU_GERROR must be zero after construction";
}

/// §6.3.17: GERROR must be 0 after reset().
TEST_F(GErrorTest, GerrorZeroAfterReset) {
    smmu->reset();
    EXPECT_EQ(smmu->getGerror(), 0u)
        << "SMMU_GERROR must be zero after reset()";
}

// ── §6.3.17: GERROR bit constants ──────────────────────────────────────────

/// §6.3.17: Verify all GERROR bit constant values match spec table (ARM IHI0070G.b §6.3.17).
TEST_F(GErrorTest, GerrorBitConstantsMatchSpec) {
    EXPECT_EQ(GERROR_CMDQ_ERR,           (1u << 0)) << "CMDQ_ERR must be bit 0 (§6.3.17)";
    EXPECT_EQ(GERROR_EVENTQ_ABT_ERR,     (1u << 2)) << "EVENTQ_ABT_ERR must be bit 2 (§6.3.17)";
    EXPECT_EQ(GERROR_PRIQ_ABT_ERR,       (1u << 3)) << "PRIQ_ABT_ERR must be bit 3 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_CMDQ_ABT_ERR,   (1u << 4)) << "MSI_CMDQ_ABT_ERR must be bit 4 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_EVENTQ_ABT_ERR, (1u << 5)) << "MSI_EVENTQ_ABT_ERR must be bit 5 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_PRIQ_ABT_ERR,   (1u << 6)) << "MSI_PRIQ_ABT_ERR must be bit 6 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_GERROR_ABT_ERR, (1u << 7)) << "MSI_GERROR_ABT_ERR must be bit 7 (§6.3.17)";
    EXPECT_EQ(GERROR_SFM_ERR,            (1u << 8)) << "SFM_ERR must be bit 8 (§6.3.17)";
    EXPECT_EQ(GERROR_CMDQP_ERR,          (1u << 9)) << "CMDQP_ERR must be bit 9 (§6.3.17)";
    // Backward-compat aliases
    EXPECT_EQ(GERROR_SFE,          GERROR_SFM_ERR)             << "SFE alias must equal SFM_ERR (bit 8)";
    EXPECT_EQ(GERROR_MSI_ABT_ERR,  GERROR_MSI_EVENTQ_ABT_ERR) << "MSI_ABT_ERR alias must equal MSI_EVENTQ_ABT_ERR (bit 5)";
    EXPECT_EQ(GERROR_CMDQ_ABT_ERR, GERROR_MSI_CMDQ_ABT_ERR)   << "CMDQ_ABT_ERR alias must equal MSI_CMDQ_ABT_ERR (bit 4)";
}

// ── §6.3.17: CMDQ_ERR set on command error ─────────────────────────────────

/// §6.3.17: CMDQ_ERR must be set when an unknown command type is processed.
///
/// In C++ the default: arm of processCommand() fires for unknown CommandType
/// values (e.g., values cast from an integer that has no named enum constant).
TEST_F(GErrorTest, CmdqErrSetOnUnknownCommandType) {
    // Cast an unused integer to CommandType to exercise the default: arm.
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);  // no named variant for 0xFF
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();

    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "GERROR.CMDQ_ERR must be set after processing an unknown command type";
}

/// BUG-NEW-05 fix: ARM §6.3.17 defines no event for "unknown command opcode".
/// Only GERROR.CMDQ_ERR is set; no C_BAD_STE event is generated.
/// The event queue must be empty after processing an unknown command type.
TEST_F(GErrorTest, CBadSteEventGeneratedWithCmdqErr) {
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFE);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();

    // ARM §6.3.17: GERROR.CMDQ_ERR must be set for unknown command opcodes.
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "GERROR.CMDQ_ERR must be set for unknown command type";

    // ARM spec defines no event type for unknown command opcode — the event
    // queue must NOT contain a C_BAD_STE entry (that event type is for
    // malformed stream table entries, not unknown command opcodes).
    auto events = smmu->getEventQueue();
    bool hasBadSte = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE) {
            hasBadSte = true;
            break;
        }
    }
    EXPECT_FALSE(hasBadSte) << "No C_BAD_STE event should be generated for unknown command opcode";
}

// ── §6.3.18: clearGerror — SMMU_GERRORN semantics ──────────────────────────

/// §6.3.18: clearGerror on zero GERROR is a no-op.
TEST_F(GErrorTest, ClearGerrorNoopOnZero) {
    smmu->clearGerror(GERROR_CMDQ_ERR);
    EXPECT_EQ(smmu->getGerror(), 0u);
}

/// §6.3.18: clearGerror clears only the CMDQ_ERR bit after it has been set.
TEST_F(GErrorTest, ClearGerrorClearsCmdqErrBit) {
    // Trigger CMDQ_ERR
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u);

    // Clear CMDQ_ERR
    smmu->clearGerror(GERROR_CMDQ_ERR);
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR bit must be cleared by clearGerror(GERROR_CMDQ_ERR)";
}

/// §6.3.18: clearGerror clears only the specified bits; other bits unaffected.
TEST_F(GErrorTest, ClearGerrorDoesNotClearOtherBits) {
    // Set CMDQ_ERR via bad command
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u);

    // Clear a different bit (SFM_ERR) — should not affect CMDQ_ERR
    smmu->clearGerror(GERROR_SFM_ERR);
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must remain set after clearing a different GERROR bit";
}

/// §6.3.17/18: After reset(), GERROR must be zero even if it was set.
TEST_F(GErrorTest, ResetClearsGerror) {
    // Set CMDQ_ERR
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u);

    smmu->reset();
    EXPECT_EQ(smmu->getGerror(), 0u)
        << "reset() must clear all GERROR bits";
}

/// §6.3.17: Valid commands must NOT set CMDQ_ERR.
TEST_F(GErrorTest, ValidCommandDoesNotSetCmdqErr) {
    CommandEntry syncCmd;
    syncCmd.type = CommandType::SYNC;
    syncCmd.streamID = 0;
    syncCmd.pasid = 0;
    syncCmd.startAddress = 0;
    syncCmd.endAddress = 0;
    syncCmd.flags = 0;
    syncCmd.timestamp = 0;

    smmu->submitCommand(syncCmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "GERROR.CMDQ_ERR must not be set for valid commands";
}

/// §6.3.17: CMDQ_ERR accumulates across multiple bad commands in one batch.
TEST_F(GErrorTest, CmdqErrAccumulatesAcrossMultipleBadCommands) {
    for (int i = 0; i < 3; ++i) {
        CommandEntry badCmd;
        badCmd.type = static_cast<CommandType>(0xE0 + i);  // unused opcodes
        badCmd.streamID = TEST_STREAM;
        badCmd.pasid = TEST_PASID;
        badCmd.startAddress = 0;
        badCmd.endAddress = 0;
        badCmd.flags = 0;
        badCmd.timestamp = 0;
        smmu->submitCommand(badCmd);
    }
    smmu->processCommandQueue();

    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must remain set after multiple bad commands";

    // Clear once — all accumulated bits cleared (only CMDQ_ERR bit)
    smmu->clearGerror(GERROR_CMDQ_ERR);
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u);
}

// ── BUG-03/SPEC-09: GERROR/GERRORN toggle protocol ─────────────────────────
// ARM IHI0070G.b §6.3.19/6.3.20: The SMMU toggles GERROR[x] ONLY when the
// error is currently INACTIVE (GERROR[x] == GERRORN[x]).  Software acknowledges
// by writing GERRORN[x] to match GERROR[x].  Active errors are indicated by
// GERROR XOR GERRORN != 0.
//
// These tests use getGerrorN() to verify the internal GERRORN register state,
// which distinguishes the spec-correct implementation from a naive direct-clear
// approach.  getGerrorN() returns the raw SMMU_GERRORN register value.

/// BUG-03/SPEC-09: GERRORN must be 0 at construction (ARM §6.3.18 reset state).
TEST_F(GErrorTest, GerrornZeroAfterConstruction) {
    EXPECT_EQ(smmu->getGerrorN(), 0u)
        << "SMMU_GERRORN must be zero after construction (ARM §6.3.18)";
}

/// BUG-03/SPEC-09: GERRORN must be 0 after reset() (ARM §6.3.18 reset state).
TEST_F(GErrorTest, GerrornZeroAfterReset) {
    // Trigger and acknowledge an error to set GERRORN != 0
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;
    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    smmu->clearGerror(GERROR_CMDQ_ERR);
    // GERRORN should be non-zero (toggled to acknowledge)
    ASSERT_NE(smmu->getGerrorN(), 0u)
        << "GERRORN must be non-zero after acknowledging an active error";

    smmu->reset();
    EXPECT_EQ(smmu->getGerrorN(), 0u)
        << "SMMU_GERRORN must be zero after reset() (ARM §6.3.18)";
    EXPECT_EQ(smmu->getGerror(), 0u)
        << "Active errors (GERROR XOR GERRORN) must be zero after reset()";
}

/// BUG-03/SPEC-09: clearGerror() toggles GERRORN[x], NOT clears GERROR[x].
/// After acknowledging CMDQ_ERR, GERRORN must be non-zero and GERROR active
/// bits must be zero — the hardware GERROR register is NOT directly modified.
TEST_F(GErrorTest, ClearGerrorTogglesGerrorn) {
    // Trigger CMDQ_ERR
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;
    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active before acknowledgement";
    ASSERT_EQ(smmu->getGerrorN() & GERROR_CMDQ_ERR, 0u)
        << "GERRORN.CMDQ_ERR must be 0 before acknowledgement";

    // Software acknowledges: clearGerror() writes to GERRORN
    smmu->clearGerror(GERROR_CMDQ_ERR);

    // After acknowledge: GERROR active bits must be 0, GERRORN must be toggled
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR active bit must be 0 after software acknowledgement";
    EXPECT_NE(smmu->getGerrorN() & GERROR_CMDQ_ERR, 0u)
        << "GERRORN.CMDQ_ERR must be toggled (=1) after software acknowledgement "
           "— clearGerror() must update GERRORN, NOT clear GERROR directly";
}

/// BUG-03/SPEC-09: clearGerror() on an already-inactive bit is a no-op.
/// GERRORN must NOT be modified when the targeted error bit is not active.
/// ARM §6.3.18: only active errors (GERROR[x] != GERRORN[x]) can be acknowledged.
TEST_F(GErrorTest, ClearGerrorOnInactiveBitDoesNotModifyGerrorn) {
    // No errors are active at this point
    ASSERT_EQ(smmu->getGerror(), 0u);
    ASSERT_EQ(smmu->getGerrorN(), 0u);

    // Attempt to clear an inactive bit
    smmu->clearGerror(GERROR_CMDQ_ERR);

    EXPECT_EQ(smmu->getGerrorN(), 0u)
        << "GERRORN must not be modified when acknowledging an inactive error bit";
    EXPECT_EQ(smmu->getGerror(), 0u)
        << "Active error bits must remain 0 when clearing an already-inactive bit";
}

/// BUG-03/SPEC-09: When an error is already active (GERROR[x] != GERRORN[x]),
/// signalling the same error again must NOT change GERROR[x].
/// ARM §6.3.19: "The SMMU does not toggle a bit when an error is already active."
/// This test verifies the XOR-toggle-only-if-inactive constraint.
TEST_F(GErrorTest, ToggleProtocolDoesNotRetoggleActiveError) {
    // Trigger CMDQ_ERR once — error becomes active
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xFF);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active after first bad command";

    // Acknowledge the error so queue processing can resume
    smmu->clearGerror(GERROR_CMDQ_ERR);
    ASSERT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be inactive after software acknowledge via clearGerror";

    // Trigger CMDQ_ERR a second time — must become active again
    CommandEntry badCmd2;
    badCmd2.type = static_cast<CommandType>(0xFE);
    badCmd2.streamID = TEST_STREAM;
    badCmd2.pasid = TEST_PASID;
    badCmd2.startAddress = 0;
    badCmd2.endAddress = 0;
    badCmd2.flags = 0;
    badCmd2.timestamp = 0;

    smmu->submitCommand(badCmd2);
    smmu->processCommandQueue();
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active again after second bad command";
}

/// BUG-03/SPEC-09: Submitting two back-to-back bad commands must NOT cause
/// CMDQ_ERR to oscillate.  The first signal activates the bit; the second
/// must be suppressed because the error is already active.
/// After processing halts on the first bad command, CMDQ_ERR stays active.
TEST_F(GErrorTest, ToggleProtocolDoesNotFlipAlreadyActiveError) {
    CommandEntry badCmd1;
    badCmd1.type = static_cast<CommandType>(0xA0);
    badCmd1.streamID = TEST_STREAM;
    badCmd1.pasid = TEST_PASID;
    badCmd1.startAddress = 0;
    badCmd1.endAddress = 0;
    badCmd1.flags = 0;
    badCmd1.timestamp = 0;

    CommandEntry badCmd2;
    badCmd2.type = static_cast<CommandType>(0xA1);
    badCmd2.streamID = TEST_STREAM;
    badCmd2.pasid = TEST_PASID;
    badCmd2.startAddress = 0;
    badCmd2.endAddress = 0;
    badCmd2.flags = 0;
    badCmd2.timestamp = 0;

    smmu->submitCommand(badCmd1);
    smmu->submitCommand(badCmd2);
    // processCommandQueue() halts on CMDQ_ERR after the first bad command;
    // the second command is not processed.  CMDQ_ERR must remain active.
    smmu->processCommandQueue();

    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must remain active (=1) after processing a bad command; "
           "a second signal while already active must not toggle bit back to 0";
}

/// BUG-03/SPEC-09: Software acknowledges GERROR via SMMU_GERRORN (clearGerror).
/// After acknowledgement, the error is inactive and queue processing can resume.
TEST_F(GErrorTest, GerrornAcknowledgementMakesErrorInactive) {
    CommandEntry badCmd;
    badCmd.type = static_cast<CommandType>(0xBB);
    badCmd.streamID = TEST_STREAM;
    badCmd.pasid = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress = 0;
    badCmd.flags = 0;
    badCmd.timestamp = 0;

    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active before acknowledgement";

    smmu->clearGerror(GERROR_CMDQ_ERR);

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "After software acknowledges via GERRORN, CMDQ_ERR active bit must be 0";
}

/// BUG-03/SPEC-09: Acknowledging an inactive bit (SFM_ERR not set) must not
/// affect other active bits (CMDQ_ERR remains active).
TEST_F(GErrorTest, GerrornAcknowledgesOnlySpecifiedBit) {
    // Trigger CMDQ_ERR
    CommandEntry badCmd1;
    badCmd1.type = static_cast<CommandType>(0xCC);
    badCmd1.streamID = TEST_STREAM;
    badCmd1.pasid = TEST_PASID;
    badCmd1.startAddress = 0;
    badCmd1.endAddress = 0;
    badCmd1.flags = 0;
    badCmd1.timestamp = 0;

    smmu->submitCommand(badCmd1);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active after first bad command";

    // Acknowledge CMDQ_ERR so queue processing can resume
    smmu->clearGerror(GERROR_CMDQ_ERR);
    ASSERT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be inactive after acknowledge";

    // Trigger CMDQ_ERR again
    CommandEntry badCmd2;
    badCmd2.type = static_cast<CommandType>(0xCD);
    badCmd2.streamID = TEST_STREAM;
    badCmd2.pasid = TEST_PASID;
    badCmd2.startAddress = 0;
    badCmd2.endAddress = 0;
    badCmd2.flags = 0;
    badCmd2.timestamp = 0;

    smmu->submitCommand(badCmd2);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active again";

    // Attempt to clear SFM_ERR (inactive) — must not affect CMDQ_ERR
    smmu->clearGerror(GERROR_SFM_ERR);
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must remain active when acknowledging an inactive bit";
    EXPECT_EQ(smmu->getGerrorN() & GERROR_SFM_ERR, 0u)
        << "GERRORN.SFM_ERR must not be toggled when SFM_ERR is not active";
}

}  // namespace test
}  // namespace smmu
