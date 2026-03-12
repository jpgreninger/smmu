// BUG-CPP-S1: Double F_TRANSLATION event emitted for two-stage stream
// with stage-2 address space not configured.
//
// ARM IHI0070G.b §3.12.2 (line 11302):
//   "The SMMU does not record more than one fault for each incoming transaction"
//
// Root cause: performTwoStageTranslation() emits F_TRANSLATION explicitly at
// the !stage2AddressSpace guard, then translate() calls handleTranslationFailure()
// which also emits F_TRANSLATION for FaultType::TranslationFault — resulting in
// two events for one transaction.
//
// Fix: Remove the explicit generateEvent(F_TRANSLATION) from the
// !stage2AddressSpace block; let handleTranslationFailure() emit it exactly once.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

class BugCppS1Test : public ::testing::Test {
protected:
    static constexpr StreamID S1_STREAM = 0x51;
    static constexpr PASID    PASID1    = 1;      // non-zero avoids PASID-0 auto-link
    static constexpr IOVA     TEST_IOVA = 0x8000ULL;
    static constexpr PA       TEST_IPA  = 0xA000ULL;

    void SetUp() override {
        smmuObj = std::make_unique<SMMU>();
        smmuObj->enable();

        // Configure a two-stage stream: stage-1 enabled, stage-2 requested
        // but NOT configured (no setStreamStage2AddressSpace call).
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.aa64               = true;
        ASSERT_TRUE(smmuObj->configureStream(S1_STREAM, cfg).isOk());
        ASSERT_TRUE(smmuObj->enableStream(S1_STREAM).isOk());

        // Use PASID=1 to avoid the PASID-0 auto-link that would alias stage-1
        // AS as stage-2 AS — we need stage2AddressSpace to be nullptr.
        ASSERT_TRUE(smmuObj->createStreamPASID(S1_STREAM, PASID1).isOk());

        // Map a stage-1 page (IOVA -> IPA) so stage-1 succeeds and we reach
        // the !stage2AddressSpace guard in performTwoStageTranslation().
        PagePermissions perms(true, true, false);
        ASSERT_TRUE(smmuObj->mapPage(S1_STREAM, PASID1, TEST_IOVA, TEST_IPA,
                                      perms, SecurityState::NonSecure).isOk());

        // Clear any events that may have been generated during setup.
        smmuObj->clearEventQueue();
    }

    std::unique_ptr<SMMU> smmuObj;
};

// -----------------------------------------------------------------------
// Primary test: exactly ONE F_TRANSLATION event must be generated.
//
// §3.12.2 forbids recording more than one fault per transaction.
// With the bug, generateEvent(F_TRANSLATION) fires in:
//   1. performTwoStageTranslation() at the !stage2AddressSpace guard
//   2. handleTranslationFailure() at the FaultType::TranslationFault case
// giving two events.  After the fix, only path (2) fires.
// -----------------------------------------------------------------------
TEST_F(BugCppS1Test, NoStage2AS_ExactlyOneF_TranslationEvent) {
    TranslationResult result = smmuObj->translate(S1_STREAM, PASID1, TEST_IOVA,
                                                   AccessType::Read,
                                                   SecurityState::NonSecure);

    // Translation must fail: no stage-2 AS is a translation fault.
    EXPECT_TRUE(result.isError())
        << "Translation must fail when stage-2 AS is not configured";

    // Count F_TRANSLATION events for this stream.
    auto events = smmuObj->getEventQueue();
    int f_translation_count = 0;
    for (const auto& evt : events) {
        if (evt.type == EventType::F_TRANSLATION && evt.streamID == S1_STREAM) {
            ++f_translation_count;
        }
    }

    // §3.12.2: EXACTLY ONE F_TRANSLATION event per transaction.
    EXPECT_EQ(f_translation_count, 1)
        << "§3.12.2: exactly one F_TRANSLATION event must be emitted per "
           "transaction; got " << f_translation_count;
}

// -----------------------------------------------------------------------
// Secondary test: verify the total event count does not exceed 1
// (no duplicate events of any type for this stream).
// -----------------------------------------------------------------------
TEST_F(BugCppS1Test, NoStage2AS_TotalEventCountIsOne) {
    smmuObj->translate(S1_STREAM, PASID1, TEST_IOVA,
                        AccessType::Read, SecurityState::NonSecure);

    auto events = smmuObj->getEventQueue();
    int stream_event_count = 0;
    for (const auto& evt : events) {
        if (evt.streamID == S1_STREAM) {
            ++stream_event_count;
        }
    }

    EXPECT_EQ(stream_event_count, 1)
        << "§3.12.2: exactly one event total must be emitted per transaction; "
           "got " << stream_event_count;
}

// -----------------------------------------------------------------------
// Verify the emitted event IS F_TRANSLATION (not some other type).
// -----------------------------------------------------------------------
TEST_F(BugCppS1Test, NoStage2AS_EventTypeIsF_Translation) {
    smmuObj->translate(S1_STREAM, PASID1, TEST_IOVA,
                        AccessType::Read, SecurityState::NonSecure);

    auto events = smmuObj->getEventQueue();
    ASSERT_FALSE(events.empty())
        << "At least one event must be emitted for a two-stage translation "
           "fault with no stage-2 AS";

    bool found = false;
    for (const auto& evt : events) {
        if (evt.streamID == S1_STREAM) {
            EXPECT_EQ(evt.type, EventType::F_TRANSLATION)
                << "Event type must be F_TRANSLATION (§7.3.13) for missing stage-2 AS";
            found = true;
        }
    }
    EXPECT_TRUE(found) << "F_TRANSLATION event for stream " << S1_STREAM << " not found";
}

} // namespace test
} // namespace smmu
