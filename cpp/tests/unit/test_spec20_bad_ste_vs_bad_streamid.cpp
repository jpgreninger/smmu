// SPEC-20: In-bounds unconfigured stream must emit C_BAD_STE, not C_BAD_STREAMID
//
// ARM IHI0070G.b §7.3.3 C_BAD_STREAMID: fires when StreamID is OUTSIDE the
// configured table range (>= 2^LOG2SIZE).
//
// ARM IHI0070G.b §7.3.5 C_BAD_STE: fires when StreamID is WITHIN table bounds
// but STE.V=0 (stream not configured / not in streamMap).
//
// ARM IHI0070G.b §6.3.12 RECINVSID: gates C_BAD_STREAMID recording ONLY.
// C_BAD_STE is ALWAYS recorded regardless of RECINVSID.
//
// TDD: these tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

using namespace smmu;

namespace {

// Create a fully enabled SMMU with CR2.RECINVSID=1 and LOG2SIZE=8
// (StreamIDs 0-255 are within the configured table range).
static std::unique_ptr<SMMU> makeTestSMMU() {
    SMMUConfiguration cfg;
    auto smmu = std::make_unique<SMMU>(cfg);
    smmu->enable();
    // Enable RECINVSID so C_BAD_STREAMID events are recorded when triggered
    smmu->setCR2(SMMU::CR2_RECINVSID);
    // LOG2SIZE=8 → valid range is StreamIDs 0-255
    smmu->setStrtabLog2Size(8u);
    return smmu;
}

static bool hasEvent(SMMU& smmu, EventType t) {
    for (const auto& e : smmu.getEventQueue()) {
        if (e.type == t) return true;
    }
    return false;
}

} // namespace

// ---------------------------------------------------------------------------
// SPEC-20-A: StreamID within LOG2SIZE range but not configured → C_BAD_STE
// ---------------------------------------------------------------------------

// StreamID=50 is within the range 0-255 (LOG2SIZE=8) but has never been
// configured. The STE is absent (STE.V=0 equivalent). Per §7.3.5 this must
// generate C_BAD_STE, NOT C_BAD_STREAMID.
TEST(Spec20BadSteVsBadStreamid, InBoundsUnconfiguredStreamEmitsCBadSte) {
    auto smmu = makeTestSMMU();

    // StreamID 50 is within range [0, 255] but never configured.
    auto result = smmu->translate(50u, 0u, 0x1000u,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "expected error for unconfigured stream";

    EXPECT_TRUE(hasEvent(*smmu, EventType::C_BAD_STE))
        << "SPEC-20: in-bounds unconfigured stream must emit C_BAD_STE (§7.3.5)";
    EXPECT_FALSE(hasEvent(*smmu, EventType::C_BAD_STREAMID))
        << "SPEC-20: in-bounds stream must NOT emit C_BAD_STREAMID (§7.3.3)";
}

// ---------------------------------------------------------------------------
// SPEC-20-B: StreamID outside LOG2SIZE range still emits C_BAD_STREAMID
// ---------------------------------------------------------------------------

// StreamID=300 is outside the range 0-255 (LOG2SIZE=8). Per §7.3.3 this must
// continue to generate C_BAD_STREAMID.
TEST(Spec20BadSteVsBadStreamid, OutOfBoundsStreamEmitsCBadStreamid) {
    auto smmu = makeTestSMMU();

    // StreamID 300 (0x12C) exceeds 2^8=256, so it is out of range.
    auto result = smmu->translate(300u, 0u, 0x1000u,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "expected error for out-of-range stream";

    EXPECT_TRUE(hasEvent(*smmu, EventType::C_BAD_STREAMID))
        << "SPEC-20: out-of-range stream must emit C_BAD_STREAMID (§7.3.3)";
    EXPECT_FALSE(hasEvent(*smmu, EventType::C_BAD_STE))
        << "SPEC-20: out-of-range stream must NOT emit C_BAD_STE";
}

// ---------------------------------------------------------------------------
// SPEC-20-C: C_BAD_STE is always recorded regardless of RECINVSID
// ---------------------------------------------------------------------------

// Even when RECINVSID=0 (C_BAD_STREAMID suppressed), C_BAD_STE must still be
// recorded per §6.3.12 — RECINVSID gates ONLY C_BAD_STREAMID.
TEST(Spec20BadSteVsBadStreamid, CBadSteRecordedEvenWithRecinvsidZero) {
    SMMUConfiguration cfg;
    auto smmu = std::make_unique<SMMU>(cfg);
    smmu->enable();
    // RECINVSID=0: C_BAD_STREAMID recording is suppressed
    smmu->setCR2(0u);
    smmu->setStrtabLog2Size(8u);

    // StreamID 50 is in-range but not configured → should emit C_BAD_STE
    smmu->translate(50u, 0u, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(hasEvent(*smmu, EventType::C_BAD_STE))
        << "SPEC-20: C_BAD_STE must be recorded even when RECINVSID=0 (§6.3.12)";
}

// ---------------------------------------------------------------------------
// SPEC-20-D: Configured stream within range does NOT emit C_BAD_STE
// ---------------------------------------------------------------------------

// A properly configured + enabled stream that successfully translates must
// produce neither C_BAD_STE nor C_BAD_STREAMID.
TEST(Spec20BadSteVsBadStreamid, ConfiguredStreamProducesNoConfigEvent) {
    auto smmu = makeTestSMMU();

    StreamID sid = 10u;
    StreamConfig cfg;
    cfg.translationEnabled = false; // bypass
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());

    smmu->translate(sid, 0u, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(hasEvent(*smmu, EventType::C_BAD_STE))
        << "properly configured stream must not emit C_BAD_STE";
    EXPECT_FALSE(hasEvent(*smmu, EventType::C_BAD_STREAMID))
        << "properly configured in-range stream must not emit C_BAD_STREAMID";
}

// ---------------------------------------------------------------------------
// SPEC-20-E: C_BAD_STE carries the correct streamID
// ---------------------------------------------------------------------------
TEST(Spec20BadSteVsBadStreamid, CBadSteCarriesCorrectStreamId) {
    auto smmu = makeTestSMMU();

    StreamID sid = 77u;
    smmu->translate(sid, 0u, 0x2000u, AccessType::Read, SecurityState::NonSecure);

    for (const auto& e : smmu->getEventQueue()) {
        if (e.type == EventType::C_BAD_STE) {
            EXPECT_EQ(e.streamID, sid)
                << "SPEC-20: C_BAD_STE must carry the StreamID that caused the fault";
            return;
        }
    }
    FAIL() << "no C_BAD_STE event found";
}
