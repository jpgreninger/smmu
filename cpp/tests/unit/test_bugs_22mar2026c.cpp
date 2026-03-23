// BUG-CPP-1 (test_bugs_22mar2026c.cpp): FaultRecord.faultType for STE.V=0 case.
//
// ARM IHI0070G.b §7.3.3 C_BAD_STREAMID (0x02): StreamID is OUTSIDE the
//   configured stream table range (>= 2^LOG2SIZE).
// ARM IHI0070G.b §7.3.5 C_BAD_STE (0x04): StreamID is WITHIN table bounds
//   but STE.V=0 (no valid STE / stream not configured).
//
// The EventEntry emitted is already correct (C_BAD_STE via EventType::C_BAD_STE).
// The bug: the FaultRecord populated at smmu.cpp ~line 385 sets
//   fault.faultType = FaultType::BadStreamID   <-- WRONG
// It must instead set
//   fault.faultType = FaultType::BadSTE         <-- CORRECT
//
// TDD: this test MUST FAIL before the fix is applied (BadSTE does not exist in
// the enum yet, or the assignment still uses BadStreamID).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

using namespace smmu;

namespace {

// Build a fully enabled SMMU with a small stream-table range (LOG2SIZE=8,
// so StreamIDs 0-255 are in-range).
static std::unique_ptr<SMMU> makeSmmu() {
    SMMUConfiguration cfg;
    auto smmu = std::make_unique<SMMU>(cfg);
    smmu->enable();
    smmu->setStrtabLog2Size(8u);
    return smmu;
}

} // namespace

// ---------------------------------------------------------------------------
// BUG-CPP-1-A: In-bounds unconfigured stream — FaultRecord.faultType must be
//              FaultType::BadSTE, not FaultType::BadStreamID.
// ---------------------------------------------------------------------------
TEST(BugCpp1SteV0FaultType, InRangeUnconfiguredStreamFaultRecordMustBeBadSTE) {
    auto smmu = makeSmmu();

    // StreamID 42 is within [0, 255] but never configured (STE.V=0 equivalent).
    const StreamID sid = 42u;
    auto result = smmu->translate(sid, 0u, 0x1000u,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "translation on unconfigured stream must return an error";

    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk()) << "getEvents() must succeed";
    ASSERT_FALSE(events.getValue().empty())
        << "at least one FaultRecord must have been recorded";

    const FaultRecord& fr = events.getValue()[0];
    EXPECT_EQ(fr.streamID, sid);

    // PRIMARY ASSERTION: BUG-CPP-1
    // Before fix: FaultType::BadStreamID  (WRONG — §7.3.3 is for out-of-range)
    // After fix:  FaultType::BadSTE       (CORRECT — §7.3.5 is for STE.V=0)
    EXPECT_EQ(fr.faultType, FaultType::BadSTE)
        << "BUG-CPP-1: in-bounds unconfigured stream (STE.V=0) must record "
           "FaultType::BadSTE per ARM IHI0070G.b §7.3.5, not FaultType::BadStreamID";
}

// ---------------------------------------------------------------------------
// BUG-CPP-1-B: Out-of-range StreamID — FaultRecord.faultType must remain
//              FaultType::BadStreamID (regression guard; must pass before and
//              after the fix).
// ---------------------------------------------------------------------------
TEST(BugCpp1SteV0FaultType, OutOfRangeStreamFaultRecordMustBeBadStreamID) {
    auto smmu = makeSmmu();

    // StreamID 300 exceeds 2^8=256, so it is out-of-range → §7.3.3 C_BAD_STREAMID.
    // Enable RECINVSID so the event is recorded.
    smmu->setCR2(SMMU::CR2_RECINVSID);

    const StreamID sid = 300u;
    auto result = smmu->translate(sid, 0u, 0x1000u,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "translation on out-of-range StreamID must return an error";

    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk()) << "getEvents() must succeed";
    ASSERT_FALSE(events.getValue().empty())
        << "at least one FaultRecord must have been recorded for out-of-range StreamID";

    const FaultRecord& fr = events.getValue()[0];
    EXPECT_EQ(fr.streamID, sid);

    // Regression guard: out-of-range StreamID keeps FaultType::BadStreamID.
    EXPECT_EQ(fr.faultType, FaultType::BadStreamID)
        << "out-of-range StreamID must record FaultType::BadStreamID per §7.3.3";
}

// ---------------------------------------------------------------------------
// BUG-CPP-1-C: EventEntry type consistency — in-bounds unconfigured stream
//              must emit EventType::C_BAD_STE in the event queue (already
//              correct; this is a regression guard that must pass before and
//              after the fix).
// ---------------------------------------------------------------------------
TEST(BugCpp1SteV0FaultType, InRangeUnconfiguredStreamEventEntryMustBeC_BAD_STE) {
    auto smmu = makeSmmu();

    const StreamID sid = 77u;
    smmu->translate(sid, 0u, 0x2000u, AccessType::Read, SecurityState::NonSecure);

    bool foundBadSte = false;
    bool foundBadStreamid = false;
    for (const auto& ev : smmu->getEventQueue()) {
        if (ev.type == EventType::C_BAD_STE) foundBadSte = true;
        if (ev.type == EventType::C_BAD_STREAMID) foundBadStreamid = true;
    }

    EXPECT_TRUE(foundBadSte)
        << "EventType::C_BAD_STE must appear in event queue for in-bounds "
           "unconfigured stream (§7.3.5)";
    EXPECT_FALSE(foundBadStreamid)
        << "EventType::C_BAD_STREAMID must NOT appear for in-bounds stream (§7.3.3)";
}
