// §7.3.1 MEV stall guard — stall events must never be merged even when MEV=1
//
// ARM IHI0070G.b §7.3.1: MEV (Merged Event Virq) allows the SMMU to suppress
// duplicate non-stall events for the same stream. However, stall events carry
// a STAG and must be individually retried; merging them loses the STAG context.
//
// Bug: The MEV dedup block in generateEvent() checks only type+streamID+pasid
// but does not exclude stall events. When MEV=1, a second stall event with the
// same type/streamID/pasid is incorrectly suppressed.
//
// Fix needed: Add `&& !isStall` guard to the mevEnabled check, and
// `&& !existing.stall` to the inner merge condition.
//
// TDD: The test below MUST FAIL (red) before the fix is applied, because without
// the fix, the second stall event is suppressed by the MEV dedup check, leaving
// only 1 stall entry instead of 2.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <cstdint>

namespace smmu {
namespace test {

class MevStallGuardTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();

        // Enable SMMU with event queue
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);

        // Configure a stream with MEV=true and stall mode enabled
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.mev                = true;        // STE.MEV = 1 — enables MEV merging
        cfg.faultMode          = FaultMode::Stall; // stall fault model
        cfg.s1cdMax            = 0u;          // no substreams
        cfg.securityState      = SecurityState::NonSecure;
        ASSERT_TRUE(smmu_->configureStream(kSid, cfg).isOk())
            << "configureStream failed";
        ASSERT_TRUE(smmu_->enableStream(kSid).isOk())
            << "enableStream failed";
        ASSERT_TRUE(smmu_->createStreamPASID(kSid, kPasid).isOk())
            << "createStreamPASID failed";
        smmu_->clearEventQueue();
    }

    static constexpr StreamID kSid   = 0x0100;
    static constexpr PASID    kPasid = 0u;
    static constexpr IOVA     kIova  = 0x1000u;

    std::unique_ptr<SMMU> smmu_;
};

// §7.3.1: Two stall events with the same type/streamID/pasid MUST both appear
// in the event queue even when MEV=1. Without the fix, the second event is
// suppressed by the MEV dedup check, leaving only 1 event.
TEST_F(MevStallGuardTest, StallEventsNotMergedWhenMevEnabled) {
    // Inject two stall events via translate().
    // Each translation of an unmapped address on a stall-capable stream should
    // produce a stall event (F_TRANSLATION). Because MEV=1, the buggy code
    // suppresses the second stall event when the first is still in the queue.
    smmu_->translate(kSid, kPasid, kIova, AccessType::Read,
                     SecurityState::NonSecure);
    smmu_->translate(kSid, kPasid, kIova + 0x2000u, AccessType::Read,
                     SecurityState::NonSecure);

    auto queue = smmu_->getEventQueue();

    // §7.3.1: Both stall events must be individually recorded — merging stall
    // events is forbidden because each carries a unique STAG for retry.
    // Without the fix this assertion fails (queue.size() == 1).
    ASSERT_GE(queue.size(), 2u)
        << "§7.3.1: Both stall events must appear in the event queue "
           "even when STE.MEV=1; merging stall events loses STAG context";

    // Verify both entries are actually stall events.
    size_t stallCount = 0;
    for (const auto& ev : queue) {
        if (ev.stall) {
            ++stallCount;
        }
    }
    EXPECT_GE(stallCount, 2u)
        << "§7.3.1: Expected at least 2 stall event entries in the event queue";
}

// §7.3.1: Non-stall duplicate events on a MEV=1 stream SHOULD be merged.
// This verifies the MEV logic still works correctly for the non-stall path.
TEST_F(MevStallGuardTest, NonStallDuplicatesAreStillMergedWhenMevEnabled) {
    // Use a non-stall stream so events are non-stall type.
    StreamConfig nonStallCfg;
    nonStallCfg.translationEnabled = true;
    nonStallCfg.stage1Enabled  = true;
    nonStallCfg.stage2Enabled  = false;
    nonStallCfg.mev            = true;
    nonStallCfg.faultMode      = FaultMode::Terminate; // non-stall
    nonStallCfg.securityState  = SecurityState::NonSecure;
    static constexpr StreamID kSid2 = 0x0200;
    ASSERT_TRUE(smmu_->configureStream(kSid2, nonStallCfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(kSid2).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(kSid2, 0u).isOk());

    // Translate twice on same unmapped address — both should generate F_TRANSLATION
    // non-stall events. MEV=1 should suppress the duplicate.
    smmu_->translate(kSid2, 0u, 0x3000u, AccessType::Read,
                     SecurityState::NonSecure);
    size_t sizeAfterFirst = smmu_->getEventQueue().size();

    smmu_->translate(kSid2, 0u, 0x3000u, AccessType::Read,
                     SecurityState::NonSecure);
    size_t sizeAfterSecond = smmu_->getEventQueue().size();

    // MEV should suppress the duplicate non-stall event: queue size unchanged.
    // (This is the intended MEV behavior for non-stall events.)
    EXPECT_EQ(sizeAfterFirst, sizeAfterSecond)
        << "§7.3.1: Non-stall duplicate events should be merged (suppressed) "
           "by MEV=1";
}

} // namespace test
} // namespace smmu
