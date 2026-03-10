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

// RECINVSID=0 (default) — C_BAD_STREAMID must NOT be recorded for unknown (stream-not-found) stream.
TEST(NewGapsCpp, RECINVSID_Default_NoEventForUnknownStream) {
    auto s = makeSMMU();
    // No streams configured — stream 42 is unknown (stream-not-found path)
    s->translate(42, 0, 0x1000, AccessType::Read);
    auto events = s->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            found = true;
        }
    }
    EXPECT_FALSE(found) << "With RECINVSID=0, C_BAD_STREAMID must NOT be recorded for unknown stream";
}

// RECINVSID=1 — C_BAD_STREAMID IS recorded for unknown stream.
TEST(NewGapsCpp, RECINVSID_Set_EventRecordedForUnknownStream) {
    auto s = makeSMMU();
    s->setCR2(SMMU::CR2_RECINVSID);
    s->translate(42, 0, 0x1000, AccessType::Read);
    auto events = s->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "With RECINVSID=1, C_BAD_STREAMID must be recorded for unknown stream";
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

// CMD_CFGI_STE with RECINVSID=0: no event BUT GERROR.CMDQ_ERR must still be set.
TEST(NewGapsCpp, RECINVSID_Default_CfgiSte_NoEventButGerrorSet) {
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
    EXPECT_FALSE(foundEvent) << "With RECINVSID=0, CMD_CFGI_STE must NOT record C_BAD_STREAMID event";

    // GERROR.CMDQ_ERR must still be set unconditionally
    uint32_t active = s->getGerror() ^ s->getGerrorN();
    EXPECT_NE(active & GERROR_CMDQ_ERR, 0u) << "GERROR.CMDQ_ERR must be set even when RECINVSID=0";
}

// CMD_CFGI_STE with RECINVSID=1: event IS recorded AND GERROR.CMDQ_ERR is set.
TEST(NewGapsCpp, RECINVSID_Set_CfgiSte_EventAndGerrorSet) {
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
    EXPECT_TRUE(foundEvent) << "With RECINVSID=1, CMD_CFGI_STE must record C_BAD_STREAMID event";

    uint32_t active = s->getGerror() ^ s->getGerrorN();
    EXPECT_NE(active & GERROR_CMDQ_ERR, 0u) << "GERROR.CMDQ_ERR must be set";
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

} // namespace test
} // namespace smmu
