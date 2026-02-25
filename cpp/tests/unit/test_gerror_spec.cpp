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

}  // namespace test
}  // namespace smmu
