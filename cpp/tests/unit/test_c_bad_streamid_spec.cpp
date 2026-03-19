// ARM SMMU v3: C_BAD_STE event for in-bounds unconfigured StreamID (C++)
//
// SPEC-20 correction: per ARM IHI0070G.b §7.3.3 vs §7.3.5:
//   - C_BAD_STREAMID: StreamID OUTSIDE 2^LOG2SIZE range
//   - C_BAD_STE:      StreamID within range but STE.V=0 (not configured)
//
// With default LOG2SIZE=32, ALL uint32_t StreamIDs are within range.
// Therefore an unconfigured stream must generate C_BAD_STE (0x04), not
// C_BAD_STREAMID (0x02).

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
    // Default LOG2SIZE=32: all uint32_t StreamIDs are within the table range.
    // An unconfigured stream is therefore STE.V=0 → C_BAD_STE, not C_BAD_STREAMID.
    return smmu;
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// §7.3.5: translation on in-range unconfigured StreamID must enqueue C_BAD_STE (0x04)
TEST(CBadStreamidSpec, UnknownStreamGeneratesCBadSteEvent) {
    auto smmu = makeSMMU();

    auto result = smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "expected error for unconfigured stream";

    auto events = smmu->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found) << "expected C_BAD_STE event (0x04) for in-range unconfigured stream (§7.3.5)";
}

/// §7.3.5: must NOT generate F_TRANSLATION for an unconfigured StreamID
TEST(CBadStreamidSpec, UnknownStreamNotFTranslationEvent) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                    AccessType::Write, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    bool hasTranslation = false;
    bool hasCBadSte = false;
    for (const auto& e : events) {
        if (e.type == EventType::F_TRANSLATION) hasTranslation = true;
        if (e.type == EventType::C_BAD_STE) hasCBadSte = true;
    }
    EXPECT_TRUE(hasCBadSte)     << "must have C_BAD_STE event for in-range unconfigured stream";
    EXPECT_FALSE(hasTranslation) << "must NOT have F_TRANSLATION for unconfigured-stream fault";
}

/// §7.3.5: C_BAD_STE event must carry the correct streamID
TEST(CBadStreamidSpec, CBadSteEventCarriesStreamId) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                    AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) {
            EXPECT_EQ(e.streamID, UNKNOWN_SID) << "streamID must match";
            return;
        }
    }
    FAIL() << "no C_BAD_STE event found";
}

/// §7.3.5: each in-range unconfigured stream translation produces its own C_BAD_STE event
TEST(CBadStreamidSpec, MultipleUnknownStreamsEachGenerateCBadSte) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID,  PASID_0, IOVA_1000, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(UNKNOWN_SID2, PASID_0, IOVA_1000, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(UNKNOWN_SID3, PASID_0, IOVA_1000, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    int count = 0;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) ++count;
    }
    EXPECT_EQ(count, 3) << "expected one C_BAD_STE per in-range unconfigured stream";
}

/// §7.3.5: security state must be preserved in the C_BAD_STE event
TEST(CBadStreamidSpec, CBadSteEventCarriesSecurityState) {
    auto smmu = makeSMMU();

    smmu->translate(UNKNOWN_SID, PASID_0, IOVA_1000,
                    AccessType::Read, SecurityState::Secure);

    auto events = smmu->getEventQueue();
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) {
            EXPECT_EQ(e.securityState, SecurityState::Secure);
            return;
        }
    }
    FAIL() << "no C_BAD_STE event found";
}

} // namespace test
} // namespace smmu
