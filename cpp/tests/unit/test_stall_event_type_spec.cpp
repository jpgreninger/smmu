// ARM SMMU v3 FINDING-NEW-13: Stall mode must generate the correct event type
//
// TDD spec tests verifying that when FaultMode::Stall is active the event
// written to the event queue carries the EventType that matches the actual
// fault — not always F_TRANSLATION (0x10).
//
// Spec references:
//   §7.3    Event records — each fault type has a distinct event code
//   §3.12.2 Stall model — stalled events carry stall=true and the correct type
//   §7.3.13 F_TRANSLATION (0x10)
//   §7.3.16 F_PERMISSION  (0x13)
//   §7.3.14 F_ADDR_SIZE   (0x11)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Helpers ─────────────────────────────────────────────────────────────────

static constexpr StreamID STALL_SID  = 0x50;
static constexpr PASID    PASID_0    = 0;
static constexpr IOVA     IOVA_BASE  = 0x5000;
static constexpr PA       PA_BASE    = 0x90000;

static bool hasStallEvent(const std::vector<EventEntry>& events, EventType t) {
    for (const auto& e : events) {
        if (e.type == t && e.stall) return true;
    }
    return false;
}

static bool hasAnyStallEvent(const std::vector<EventEntry>& events) {
    for (const auto& e : events) {
        if (e.stall) return true;
    }
    return false;
}

// ── Fixture ─────────────────────────────────────────────────────────────────

class StallEventTypeTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
        smmu->enable();
    }

    void TearDown() override { smmu.reset(); }

    // Set up a stall-mode stream with stage-1-only; create PASID_0 but map NO pages.
    // Any translate on IOVA_BASE will produce a translation fault (PageNotMapped).
    void setupStallStream() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Stall;
        smmu->configureStream(STALL_SID, cfg);
        smmu->enableStream(STALL_SID);
        smmu->createStreamPASID(STALL_SID, PASID_0);
        smmu->clearEventQueue();
    }

    // Set up a stall-mode stream with a read-only page mapped.
    // A Write access will produce a permission fault (PagePermissionViolation).
    void setupStallStreamWithReadOnlyPage() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Stall;
        smmu->configureStream(STALL_SID, cfg);
        smmu->enableStream(STALL_SID);
        smmu->createStreamPASID(STALL_SID, PASID_0);
        PagePermissions readOnly(true, false, false);
        smmu->mapPage(STALL_SID, PASID_0, IOVA_BASE, PA_BASE, readOnly);
        smmu->clearEventQueue();
    }

    // Set up a stall-mode stream with an address-size limit of 32 bits.
    // An IOVA >= 4 GB will produce an address-size fault.
    void setupStallStreamWithAddressSizeLimit() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Stall;
        smmu->configureStream(STALL_SID, cfg);
        smmu->enableStream(STALL_SID);
        smmu->createStreamPASID(STALL_SID, PASID_0);
        // Restrict input address size to 32 bits; anything >= 4 GB → F_ADDR_SIZE
        smmu->setStreamInputAddressSize(STALL_SID, PASID_0, 32);
        smmu->clearEventQueue();
    }

    std::unique_ptr<SMMU> smmu;
};

// ── §7.3.13: Translation fault in stall mode → F_TRANSLATION ────────────────

/// §7.3.13: Unmapped IOVA in stall mode must generate F_TRANSLATION stall event.
TEST_F(StallEventTypeTest, TranslationFaultGeneratesFTranslationEvent) {
    setupStallStream();
    auto result = smmu->translate(STALL_SID, PASID_0, IOVA_BASE,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "Unmapped IOVA in stall mode must fail";
    EXPECT_EQ(result.getError(), SMMUError::Stalled) << "Must return Stalled error";
    EXPECT_TRUE(hasStallEvent(smmu->getEventQueue(), EventType::F_TRANSLATION))
        << "Stall event must carry F_TRANSLATION (0x10) for a translation fault";
}

// ── §7.3.16: Permission fault in stall mode → F_PERMISSION ──────────────────

/// §7.3.16: Write to read-only page in stall mode must generate F_PERMISSION stall event.
TEST_F(StallEventTypeTest, PermissionFaultGeneratesFPermissionEvent) {
    setupStallStreamWithReadOnlyPage();
    auto result = smmu->translate(STALL_SID, PASID_0, IOVA_BASE,
                                  AccessType::Write, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "Write to read-only page in stall mode must fail";
    EXPECT_EQ(result.getError(), SMMUError::Stalled) << "Must return Stalled error";
    EXPECT_TRUE(hasStallEvent(smmu->getEventQueue(), EventType::F_PERMISSION))
        << "Stall event must carry F_PERMISSION (0x13) for a permission fault, not F_TRANSLATION";
}

/// §7.3.16: No spurious F_TRANSLATION event when permission fault occurs in stall mode.
TEST_F(StallEventTypeTest, PermissionFaultDoesNotGenerateFTranslationEvent) {
    setupStallStreamWithReadOnlyPage();
    smmu->translate(STALL_SID, PASID_0, IOVA_BASE,
                    AccessType::Write, SecurityState::NonSecure);
    auto events = smmu->getEventQueue();
    // Must have a stall event, but it must NOT be F_TRANSLATION
    EXPECT_TRUE(hasAnyStallEvent(events))
        << "A stall event must be generated";
    bool spuriousFTranslation = false;
    for (const auto& e : events) {
        if (e.stall && e.type == EventType::F_TRANSLATION) {
            spuriousFTranslation = true;
        }
    }
    EXPECT_FALSE(spuriousFTranslation)
        << "F_TRANSLATION must NOT be generated for a permission fault in stall mode";
}

// ── §7.3.14: Address-size fault in stall mode → F_ADDR_SIZE ─────────────────

/// §7.3.14: Over-range IOVA in stall mode must generate F_ADDR_SIZE stall event.
TEST_F(StallEventTypeTest, AddrSizeFaultGeneratesFAddrSizeEvent) {
    setupStallStreamWithAddressSizeLimit();
    constexpr IOVA OVER_RANGE_IOVA = 0x1'0000'0000ULL; // > 32-bit limit
    auto result = smmu->translate(STALL_SID, PASID_0, OVER_RANGE_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "Over-range IOVA in stall mode must fail";
    EXPECT_EQ(result.getError(), SMMUError::Stalled) << "Must return Stalled error";
    EXPECT_TRUE(hasStallEvent(smmu->getEventQueue(), EventType::F_ADDR_SIZE))
        << "Stall event must carry F_ADDR_SIZE (0x11) for an address-size fault, not F_TRANSLATION";
}

// ── §3.12.2: Stall event has stall=true flag ─────────────────────────────────

/// §3.12.2: All stall events must have the stall flag set.
TEST_F(StallEventTypeTest, StallEventHasStallFlagSet) {
    setupStallStream();
    smmu->translate(STALL_SID, PASID_0, IOVA_BASE,
                    AccessType::Read, SecurityState::NonSecure);
    auto events = smmu->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.streamID == STALL_SID) {
            EXPECT_TRUE(e.stall) << "Event from stall-mode stream must have stall=true";
            found = true;
        }
    }
    EXPECT_TRUE(found) << "At least one event must be generated for the stalled translation";
}

} // namespace test
} // namespace smmu
