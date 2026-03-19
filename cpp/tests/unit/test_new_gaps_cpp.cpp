// ARM SMMU v3 New Gaps: RECINVSID (§6.3.12 / §7.3.3) and BUG-CPP-C success-path null.
//
// TDD tests — must FAIL before the fixes are applied.
//
// Gap 1: CR2.RECINVSID gate on C_BAD_STREAMID event recording (§6.3.12).
//   - When RECINVSID=0 (reset default): no C_BAD_STREAMID event is recorded.
//   - When RECINVSID=1:                 C_BAD_STREAMID event IS recorded.
//   - GERROR.CMDQ_ERR is always toggled (unconditional) for CMD_CFGI_STE.
//
// Gap 2: BUG-CPP-C — streamContext not nulled on translate() success path.
//   (Covered by ASAN/pointer-safety; no direct behavioral test needed beyond
//    confirming the success path compiles and runs cleanly.)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static std::unique_ptr<SMMU> makeSMMU() {
    auto s = std::make_unique<SMMU>();
    s->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    return s;
}

// ---------------------------------------------------------------------------
// Gap 1: CR2 / RECINVSID
// ---------------------------------------------------------------------------

// RECINVSID=0 (default) — C_BAD_STREAMID must NOT be recorded for out-of-range StreamID.
TEST(NewGapsCpp, RECINVSID_Default_NoEventForOutOfRange) {
    auto s = makeSMMU();
    s->setStrtabLog2Size(4); // StreamIDs 0-15 valid; 16+ are out-of-range
    // CR2.RECINVSID == 0 at reset — event must NOT be recorded
    s->translate(16, 0, 0x1000, AccessType::Read);
    auto events = s->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            found = true;
        }
    }
    EXPECT_FALSE(found) << "With RECINVSID=0, C_BAD_STREAMID must NOT be recorded for out-of-range StreamID";
}

// RECINVSID=1 — C_BAD_STREAMID IS recorded for out-of-range StreamID.
TEST(NewGapsCpp, RECINVSID_Set_EventRecordedForOutOfRange) {
    auto s = makeSMMU();
    s->setStrtabLog2Size(4); // StreamIDs 0-15 valid; 16+ are out-of-range
    s->setCR2(SMMU::CR2_RECINVSID);
    s->translate(16, 0, 0x1000, AccessType::Read);
    auto events = s->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "With RECINVSID=1, C_BAD_STREAMID must be recorded for out-of-range StreamID";
}

// Translate still returns an error regardless of RECINVSID.
TEST(NewGapsCpp, RECINVSID_Default_TranslateStillErrors) {
    auto s = makeSMMU();
    s->setStrtabLog2Size(4);
    // RECINVSID=0 — no event, but translation must still fail
    auto result = s->translate(16, 0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isError()) << "Translation must still fail even when RECINVSID=0";
    EXPECT_EQ(result.getError(), SMMUError::InvalidStreamID);
}

// SPEC-20: stream 42 is within the default LOG2SIZE=32 range but not configured.
// This is STE.V=0 → C_BAD_STE (§7.3.5), NOT C_BAD_STREAMID.
// C_BAD_STE must be recorded regardless of RECINVSID (§6.3.12).
TEST(NewGapsCpp, RECINVSID_Default_InBoundsUnconfiguredStream_EmitsCBadSte) {
    auto s = makeSMMU();
    // No streams configured — stream 42 is in-range but STE.V=0
    s->translate(42, 0, 0x1000, AccessType::Read);
    auto events = s->getEventQueue();
    bool foundCBadSte = false;
    bool foundCBadStreamId = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) foundCBadSte = true;
        if (e.type == EventType::C_BAD_STREAMID) foundCBadStreamId = true;
    }
    EXPECT_TRUE(foundCBadSte) << "In-range unconfigured stream must emit C_BAD_STE (§7.3.5)";
    EXPECT_FALSE(foundCBadStreamId) << "C_BAD_STREAMID must NOT be emitted for in-range stream";
}

// C_BAD_STE is always recorded regardless of RECINVSID (§6.3.12).
TEST(NewGapsCpp, RECINVSID_Set_InBoundsUnconfiguredStream_StillEmitsCBadSte) {
    auto s = makeSMMU();
    s->setCR2(SMMU::CR2_RECINVSID);
    s->translate(42, 0, 0x1000, AccessType::Read);
    auto events = s->getEventQueue();
    bool foundCBadSte = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) foundCBadSte = true;
    }
    EXPECT_TRUE(foundCBadSte) << "C_BAD_STE must be recorded for in-range unconfigured stream (§7.3.5)";
}

// setCR2 / getCR2 round-trip.
TEST(NewGapsCpp, CR2_GetSet_RoundTrip) {
    auto s = makeSMMU();
    EXPECT_EQ(s->getCR2(), 0u) << "CR2 must reset to 0";
    s->setCR2(SMMU::CR2_RECINVSID);
    EXPECT_EQ(s->getCR2(), SMMU::CR2_RECINVSID) << "getCR2() must reflect set value";
    s->setCR2(0u);
    EXPECT_EQ(s->getCR2(), 0u) << "getCR2() must reflect cleared value";
}

// CONF-GAP-2: CMD_CFGI_STE for unknown stream is a silent no-op (§4.3.1).
// Neither RECINVSID value should produce a C_BAD_STREAMID event or GERROR.CMDQ_ERR.
TEST(NewGapsCpp, RECINVSID_Default_CfgiSte_SilentNoOp) {
    auto s = makeSMMU();
    // CR2.RECINVSID == 0 at reset
    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = 0xDEAD; // stream not configured
    cmd.pasid = 0;
    cmd.startAddress = 0;
    cmd.securityState = SecurityState::NonSecure;
    s->submitCommand(cmd);
    s->processCommandQueue();

    // C_BAD_STREAMID event must NOT be recorded
    auto events = s->getEventQueue();
    bool foundEvent = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            foundEvent = true;
        }
    }
    EXPECT_FALSE(foundEvent) << "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT record C_BAD_STREAMID";

    // GERROR.CMDQ_ERR must NOT be set — silent no-op
    uint32_t active = s->getGerror() ^ s->getGerrorN();
    EXPECT_EQ(active & GERROR_CMDQ_ERR, 0u) << "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR";
}

// CONF-GAP-2: With RECINVSID=1, CMD_CFGI_STE for unknown stream is still a silent no-op.
TEST(NewGapsCpp, RECINVSID_Set_CfgiSte_StillSilentNoOp) {
    auto s = makeSMMU();
    s->setCR2(SMMU::CR2_RECINVSID);
    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = 0xDEAD;
    cmd.pasid = 0;
    cmd.startAddress = 0;
    cmd.securityState = SecurityState::NonSecure;
    s->submitCommand(cmd);
    s->processCommandQueue();

    auto events = s->getEventQueue();
    bool foundEvent = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            foundEvent = true;
        }
    }
    EXPECT_FALSE(foundEvent) << "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT record C_BAD_STREAMID even when RECINVSID=1";

    uint32_t active = s->getGerror() ^ s->getGerrorN();
    EXPECT_EQ(active & GERROR_CMDQ_ERR, 0u) << "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT set GERROR.CMDQ_ERR";
}

// ---------------------------------------------------------------------------
// Gap 2: BUG-CPP-C — success path does not null streamContext before return.
// Verify that a successful translation still returns the correct PA after the fix.
// (Pointer-safety aspect is caught by ASAN.)
// ---------------------------------------------------------------------------

TEST(NewGapsCpp, SuccessPath_ReturnsCorrectPA) {
    SMMU s;
    s.enable(); // sets SMMUEN=1

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    s.configureStream(1, cfg);
    s.enableStream(1);
    s.createStreamPASID(1, 0);

    PagePermissions perms(true, true, false); // read-write
    s.mapPage(1, 0, 0x1000, 0xDEAD000, perms);

    auto result = s.translate(1, 0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isOk()) << "Translation must succeed on success path";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, static_cast<PA>(0xDEAD000u))
            << "PA must be correct on success path";
    }
}

// ── Gap 4: CONF-GAP-4 CMD_DPTI_ALL / CMD_DPTI_PA ──────────────────────────

/// §4.6.1: CMD_DPTI_ALL when IDR3.DPT=0 (no dirty-page tracking) must
/// generate CERROR_ILL and set GERROR.CMDQ_ERR.  The current no-op silently
/// swallows the command, violating §4.6.1.
TEST(ConfGap4, DptiAllSetsGerrorCmdqErr) {
    auto s = makeSMMU();

    smmu::CommandEntry cmd;
    cmd.type = smmu::CommandType::DPTI_ALL;
    cmd.streamID = 0;
    cmd.pasid    = 0;

    s->submitCommand(cmd);
    s->processCommandQueue();

    const uint32_t active = s->getGerror() ^ s->getGerrorN();
    EXPECT_NE(active & smmu::GERROR_CMDQ_ERR, 0u)
        << "CONF-GAP-4: CMD_DPTI_ALL must set GERROR.CMDQ_ERR when IDR3.DPT=0 (§4.6.1)";

    EXPECT_EQ(s->getCmdqConsErr(), smmu::CERROR_ILL)
        << "CONF-GAP-4: CMDQ_CONS.ERR must be CERROR_ILL for CMD_DPTI_ALL when DPT not supported";
}

/// §4.6.1: CMD_DPTI_PA when IDR3.DPT=0 must also generate CERROR_ILL and
/// set GERROR.CMDQ_ERR.
TEST(ConfGap4, DptiPaSetsGerrorCmdqErr) {
    auto s = makeSMMU();

    smmu::CommandEntry cmd;
    cmd.type = smmu::CommandType::DPTI_PA;
    cmd.streamID = 0;
    cmd.pasid    = 0;

    s->submitCommand(cmd);
    s->processCommandQueue();

    const uint32_t active = s->getGerror() ^ s->getGerrorN();
    EXPECT_NE(active & smmu::GERROR_CMDQ_ERR, 0u)
        << "CONF-GAP-4: CMD_DPTI_PA must set GERROR.CMDQ_ERR when IDR3.DPT=0 (§4.6.1)";

    EXPECT_EQ(s->getCmdqConsErr(), smmu::CERROR_ILL)
        << "CONF-GAP-4: CMDQ_CONS.ERR must be CERROR_ILL for CMD_DPTI_PA when DPT not supported";
}

} // namespace test
} // namespace smmu
