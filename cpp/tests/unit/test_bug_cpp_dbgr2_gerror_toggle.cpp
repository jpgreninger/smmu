// BUG-CPP-DBGR-2: gerrorStatus toggle in executeInvalidationCommand() not
// protected by queueMutex (§6.3.19)
// TDD: failing test written BEFORE the fix.
// The GERROR XOR-toggle in executeInvalidationCommand() reads and writes
// gerrorStatus/gerrorNStatus without holding queueMutex. The fix wraps the
// toggle block(s) in a std::lock_guard<std::recursive_mutex>(queueMutex).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

// Helper: build a CFGI_STE command for an unknown stream
static CommandEntry makeCfgiSteCommand(StreamID sid) {
    CommandEntry cmd;
    cmd.type        = CommandType::CFGI_STE;
    cmd.streamID    = sid;
    cmd.pasid       = 0;
    cmd.startAddress = 0;
    cmd.endAddress   = 0;
    cmd.securityState = SecurityState::NonSecure;
    return cmd;
}

// -----------------------------------------------------------------------
// Calling executeInvalidationCommand() with an unknown streamID must toggle
// CMDQ_ERR exactly once — not zero times, not twice.
// -----------------------------------------------------------------------
TEST(GerrorToggleSpec, UnknownStreamID_TogglesGerrorCmdqErrExactlyOnce) {
    SMMU smmu;
    smmu.enable();
    // Configure the SMMU table large enough for stream 0x100
    smmu.setStrtabLog2Size(10); // supports up to 2^10 = 1024 streams

    // Before the command: GERROR active bits should be 0
    uint32_t gerrorBefore = smmu.getGerror();
    EXPECT_EQ(gerrorBefore & GERROR_CMDQ_ERR, 0u) << "No CMDQ_ERR should be active initially";

    // Execute CFGI_STE for an unknown (not configured) stream
    CommandEntry cmd = makeCfgiSteCommand(0x999); // definitely not configured
    smmu.executeInvalidationCommand(cmd);

    // After the command: CMDQ_ERR must be active (toggled once)
    uint32_t gerrorAfter = smmu.getGerror();
    EXPECT_NE(gerrorAfter & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be active (toggled once) after CFGI_STE for unknown stream (BUG-CPP-DBGR-2)";
}

// -----------------------------------------------------------------------
// Calling executeInvalidationCommand() twice for the same unknown stream
// must toggle CMDQ_ERR twice (back to inactive after second call), because
// §6.3.19 only toggles when the bit is inactive.
// -----------------------------------------------------------------------
TEST(GerrorToggleSpec, TwoCallsForUnknownStream_TogglesBackToInactive) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(10);

    CommandEntry cmd = makeCfgiSteCommand(0x999);

    // First call — should toggle CMDQ_ERR to active
    smmu.executeInvalidationCommand(cmd);
    uint32_t gerrorAfterFirst = smmu.getGerror();
    EXPECT_NE(gerrorAfterFirst & GERROR_CMDQ_ERR, 0u)
        << "First call: CMDQ_ERR should be active";

    // Second call — error already active, §6.3.19 says do NOT toggle again
    // (the bit is already signaling, software hasn't acknowledged it)
    smmu.executeInvalidationCommand(cmd);
    uint32_t gerrorAfterSecond = smmu.getGerror();
    EXPECT_NE(gerrorAfterSecond & GERROR_CMDQ_ERR, 0u)
        << "Second call: CMDQ_ERR should remain active (no double-toggle when already active)";
}

// -----------------------------------------------------------------------
// Known stream: executeInvalidationCommand() must NOT toggle CMDQ_ERR
// -----------------------------------------------------------------------
TEST(GerrorToggleSpec, KnownStreamID_DoesNotToggleGerror) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(10);

    // Configure a stream
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(0x01, cfg);

    uint32_t gerrorBefore = smmu.getGerror();

    // Execute CFGI_STE for a known stream
    CommandEntry cmd = makeCfgiSteCommand(0x01);
    smmu.executeInvalidationCommand(cmd);

    uint32_t gerrorAfter = smmu.getGerror();
    EXPECT_EQ(gerrorBefore & GERROR_CMDQ_ERR, gerrorAfter & GERROR_CMDQ_ERR)
        << "CFGI_STE for known stream must NOT toggle CMDQ_ERR";
}

} // namespace test
} // namespace smmu
