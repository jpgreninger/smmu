// CONF-GAP-2: CMD_CFGI_STE for unknown StreamID is a silent no-op (§4.3.1).
// ARM §4.3.1 does NOT generate GERROR.CMDQ_ERR or C_BAD_STREAMID for unknown StreamIDs.
// (BUG-CPP-DBGR-2 originally fixed the GERROR toggle's mutex protection; these
//  tests now verify the correct spec-defined no-op behavior per CONF-GAP-2.)

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
// CONF-GAP-2: CMD_CFGI_STE for an unknown StreamID must be a silent no-op.
// GERROR.CMDQ_ERR must NOT be set (§4.3.1: no error for unknown StreamID).
// -----------------------------------------------------------------------
TEST(GerrorToggleSpec, UnknownStreamID_TogglesGerrorCmdqErrExactlyOnce) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(10);

    uint32_t gerrorBefore = smmu.getGerror();
    EXPECT_EQ(gerrorBefore & GERROR_CMDQ_ERR, 0u) << "No CMDQ_ERR should be active initially";

    CommandEntry cmd = makeCfgiSteCommand(0x999); // definitely not configured
    smmu.executeInvalidationCommand(cmd);

    // CONF-GAP-2: must remain inactive — unknown StreamID is a silent no-op
    uint32_t gerrorAfter = smmu.getGerror();
    EXPECT_EQ(gerrorAfter & GERROR_CMDQ_ERR, 0u)
        << "CONF-GAP-2: CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR (§4.3.1 silent no-op)";
}

// -----------------------------------------------------------------------
// CONF-GAP-2: Two CFGI_STE calls for the same unknown stream — both are
// silent no-ops, GERROR.CMDQ_ERR remains inactive throughout.
// -----------------------------------------------------------------------
TEST(GerrorToggleSpec, TwoCallsForUnknownStream_TogglesBackToInactive) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(10);

    CommandEntry cmd = makeCfgiSteCommand(0x999);

    // Both calls are silent no-ops — GERROR must remain inactive
    smmu.executeInvalidationCommand(cmd);
    uint32_t gerrorAfterFirst = smmu.getGerror();
    EXPECT_EQ(gerrorAfterFirst & GERROR_CMDQ_ERR, 0u)
        << "CONF-GAP-2: first call: CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR";

    smmu.executeInvalidationCommand(cmd);
    uint32_t gerrorAfterSecond = smmu.getGerror();
    EXPECT_EQ(gerrorAfterSecond & GERROR_CMDQ_ERR, 0u)
        << "CONF-GAP-2: second call: CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR";
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
