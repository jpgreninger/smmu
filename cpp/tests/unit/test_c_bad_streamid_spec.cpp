// ARM SMMU v3 FINDING-NEW-07: C_BAD_STREAMID event for unknown StreamID (C++)
//
// TDD spec tests verifying that a translation request for an unconfigured
// StreamID generates a C_BAD_STREAMID event (event code 0x02) in the event
// queue per ARM IHI0070G.b §7.3.3, not a generic F_TRANSLATION (0x10).
//
// FINDING-NEW-07 — tests must be RED before the fix.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID UNKNOWN_SID  = 0xFF;
static constexpr StreamID UNKNOWN_SID2 = 0xFE;
static constexpr StreamID UNKNOWN_SID3 = 0xFD;
static constexpr PASID    PASID_0      = 0;
static constexpr IOVA     IOVA_1000    = 0x1000;

// ── Helper ──────────────────────────────────────────────────────────────────

static std::unique_ptr<SMMU> makeSMMU() {
    SMMUConfiguration cfg;
    auto smmu = std::make_unique<SMMU>(cfg);
    // ARM §6.3.9: SMMU starts disabled; enable globally before tests.
    smmu->enable();
    return smmu;
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// §7.3.3: translation on unconfigured StreamID must enqueue C_BAD_STREAMID (0x02)
TEST(CBadStreamidSpec, UnknownStreamGeneratesCBadStreamidEvent) {
    auto smmu = makeSMMU();

    auto result = smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "expected error for unconfigured stream";

    auto events = smmu->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found) << "expected C_BAD_STREAMID event (0x02) for unknown stream";
}

/// §7.3.3: must NOT generate F_TRANSLATION for an unknown StreamID
TEST(CBadStreamidSpec, UnknownStreamNotFTranslationEvent) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                    AccessType::Write, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    bool hasTranslation = false;
    bool hasCBadStreamid = false;
    for (const auto& e : events) {
        if (e.type == EventType::F_TRANSLATION) hasTranslation = true;
        if (e.type == EventType::C_BAD_STREAMID) hasCBadStreamid = true;
    }
    EXPECT_TRUE(hasCBadStreamid)  << "must have C_BAD_STREAMID event";
    EXPECT_FALSE(hasTranslation)  << "must NOT have F_TRANSLATION for unknown-stream fault";
}

/// §7.3.3: C_BAD_STREAMID event must carry the correct streamID
TEST(CBadStreamidSpec, CBadStreamidEventCarriesStreamId) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                    AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            EXPECT_EQ(e.streamID, UNKNOWN_SID) << "streamID must match";
            return;
        }
    }
    FAIL() << "no C_BAD_STREAMID event found";
}

/// §7.3.3: each unknown-stream translation produces its own C_BAD_STREAMID event
TEST(CBadStreamidSpec, MultipleUnknownStreamsEachGenerateCBadStreamid) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID,  PASID_0, IOVA_1000, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(UNKNOWN_SID2, PASID_0, IOVA_1000, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(UNKNOWN_SID3, PASID_0, IOVA_1000, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    int count = 0;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) ++count;
    }
    EXPECT_EQ(count, 3) << "expected one C_BAD_STREAMID per unknown stream";
}

/// §7.3.3: security state must be preserved in the C_BAD_STREAMID event
TEST(CBadStreamidSpec, CBadStreamidEventCarriesSecurityState) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                    AccessType::Read, SecurityState::Secure);

    auto events = smmu->getEventQueue();
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            EXPECT_EQ(e.securityState, SecurityState::Secure);
            return;
        }
    }
    FAIL() << "no C_BAD_STREAMID event found";
}

} // namespace test
} // namespace smmu
