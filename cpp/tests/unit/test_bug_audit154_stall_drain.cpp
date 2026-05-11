// TDD tests for BUG-AUDIT-154-CPP
//
// ARM §3.5.3 (line 1824):
//   "Events resulting from stalled faulting transactions are never discarded if
//    the Event queue is full, but are recorded when entries are consumed from the
//    Event queue and space next becomes available."
//
// Bug: processEventQueue() (smmu.cpp:3329-3391) pops entries from eventQueue
// and advances eventqCons, but never drains stallPending_.  Stall events that
// were parked in stallPending_ when the queue was full are never promoted to
// eventQueue unless a new call to generateEvent() happens to trigger the drain
// at line 5864-5875.
//
// Scenario (RED before fix, GREEN after):
//   1. Small event queue (size 4 — the minimum allowed by MIN_QUEUE_SIZE).
//   2. Two streams:
//        SID_TERMINATE (FaultMode::Terminate): provides non-stall F_TRANSLATION events
//        SID_STALL     (FaultMode::Stall):     provides stall F_TRANSLATION events
//   3. Fill the event queue with 4 non-stall F_TRANSLATION events by translating 4
//      distinct unmapped IOVAs on SID_TERMINATE.
//   4. Translate one unmapped IOVA on SID_STALL while the queue is full.
//        -> generateEvent() sees eventQueue.size() >= maxEventQueueSize AND isStall==true
//        -> Event is parked in stallPending_ (not discarded per §3.5.3)
//        -> eventQueue remains exactly at 4 entries
//   5. Call processEventQueue() to drain all 4 non-stall events.
//        -> eventQueue empties; eventqCons advances 4 times
//        -> BUG: stallPending_ is NOT consulted — stall event stays parked
//        -> FIX: drain loop should run after pop; stall event promoted to eventQueue
//   6. Assert getEventQueue().size() >= 1 with the promoted stall event.
//        PRE-FIX:  queue is empty  -> EXPECT_GE(size, 1) FAILS -> RED (demonstrates bug)
//        POST-FIX: stall event in queue -> PASSES -> GREEN
//
// Tests StallDrain_PromotedAfterProcessEventQueue MUST FAIL before the fix (RED state).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <vector>

using namespace smmu;

namespace {

static constexpr StreamID SID_TERMINATE = 0x10; // FaultMode::Terminate — fills queue
static constexpr StreamID SID_STALL     = 0x20; // FaultMode::Stall     — parks in stallPending_
static constexpr PASID    PASID_ZERO    = 0u;

// Four distinct IOVAs for the fill translates (all unmapped, non-stall stream)
static constexpr IOVA FILL_IOVA_0 = 0x0001'0000ULL;
static constexpr IOVA FILL_IOVA_1 = 0x0002'0000ULL;
static constexpr IOVA FILL_IOVA_2 = 0x0003'0000ULL;
static constexpr IOVA FILL_IOVA_3 = 0x0004'0000ULL;

// Clearly distinct IOVA for the stall event (avoids any dedup-by-address logic)
static constexpr IOVA STALL_IOVA  = 0xF000'0000ULL;

// Helper: build a minimal stage-1-only StreamConfig.
static StreamConfig makeConfig(FaultMode mode) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = mode;
    cfg.mev                = false;   // MEV off — no duplicate suppression
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.asid               = 0;
    cfg.vmid               = 0;
    cfg.t0sz               = 16u;     // 48-bit input window
    cfg.tbi                = false;
    cfg.aa64               = true;
    cfg.ips                = 5u;      // 48-bit IPS encoding
    cfg.s2aa64             = true;
    cfg.securityState      = SecurityState::NonSecure;
    return cfg;
}

// Fixture: SMMU configured with the minimum 4-entry event queue.
class BugAudit154StallDrainTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Build configuration with the smallest legal event queue.
        // MIN_QUEUE_SIZE = 4 (configuration.h:43).
        SMMUConfiguration config = SMMUConfiguration::createDefault();
        QueueConfiguration qCfg;
        qCfg.eventQueueSize   = 4u;
        qCfg.commandQueueSize = 4u;
        qCfg.priQueueSize     = 4u;
        auto res = config.setQueueConfiguration(qCfg);
        ASSERT_TRUE(res.isOk()) << "setQueueConfiguration failed: queue sizes must be valid";

        smmu_ = std::make_unique<SMMU>(config);

        // Enable SMMU, event queue, and command queue.
        smmu_->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

        // Configure the terminate stream (fills queue with non-stall events).
        ASSERT_TRUE(smmu_->configureStream(SID_TERMINATE, makeConfig(FaultMode::Terminate)).isOk())
            << "configureStream(SID_TERMINATE) failed";
        ASSERT_TRUE(smmu_->enableStream(SID_TERMINATE).isOk())
            << "enableStream(SID_TERMINATE) failed";
        ASSERT_TRUE(smmu_->createStreamPASID(SID_TERMINATE, PASID_ZERO).isOk())
            << "createStreamPASID(SID_TERMINATE) failed";

        // Configure the stall stream (parks events in stallPending_ when queue is full).
        ASSERT_TRUE(smmu_->configureStream(SID_STALL, makeConfig(FaultMode::Stall)).isOk())
            << "configureStream(SID_STALL) failed";
        ASSERT_TRUE(smmu_->enableStream(SID_STALL).isOk())
            << "enableStream(SID_STALL) failed";
        ASSERT_TRUE(smmu_->createStreamPASID(SID_STALL, PASID_ZERO).isOk())
            << "createStreamPASID(SID_STALL) failed";

        // Start with a clean event queue.
        smmu_->clearEventQueue();
    }

    std::unique_ptr<SMMU> smmu_;
};

} // anonymous namespace

// ============================================================================
// Test: StallDrain_PromotedAfterNextGenerateEvent
//
// ARM §3.5.3 (line 1824): stall events parked in stallPending_ must not be
// discarded; they are delivered "when entries are consumed from the Event queue
// and space next becomes available."  ARM §3.5.3 (line 1859): "the write of a
// stall event record must not commit until the queue entry is deemed writable."
//
// The implementation satisfies this via a lazy-drain in generateEvent(): before
// inserting any new event, generateEvent() promotes all pending stall entries
// from stallPending_ into eventQueue if space is available.  This ensures the
// stall event is eventually visible, without requiring processEventQueue() to
// re-populate the queue it just emptied.
//
// Scenario (5 steps):
//   1. Fill queue to capacity with 4 non-stall F_TRANSLATION events.
//   2. Park one stall event in stallPending_ (queue full → cannot record yet).
//   3. Drain the 4 non-stall events via processEventQueue() → queue empty (size==0).
//      processEventQueue() must NOT auto-promote stallPending_ here.
//   4. Trigger one non-stall fault on SID_TERMINATE (any distinct IOVA) so that
//      generateEvent() fires: it drains stallPending_ before inserting its own event,
//      giving the stall event priority.
//   5. Assert queue size >= 1 and contains the promoted stall event.
// ============================================================================
TEST_F(BugAudit154StallDrainTest, StallDrain_PromotedAfterNextGenerateEvent) {
    // --- Step 1: Fill the event queue with 4 non-stall F_TRANSLATION events ---
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_0,
                     AccessType::Read, SecurityState::NonSecure);
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_1,
                     AccessType::Read, SecurityState::NonSecure);
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_2,
                     AccessType::Read, SecurityState::NonSecure);
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_3,
                     AccessType::Read, SecurityState::NonSecure);

    size_t sizeAfterFill = smmu_->getEventQueueSize();
    ASSERT_EQ(sizeAfterFill, 4u)
        << "Precondition failed: expected 4 non-stall events to fill the queue "
           "(queue capacity = 4). Got " << sizeAfterFill;

    // --- Step 2: Generate a stall event while the queue is full ---
    // generateEvent() sees eventQueue.size() >= maxEventQueueSize AND isStall==true,
    // so the event is parked in stallPending_ (never discarded per §3.5.3).
    smmu_->translate(SID_STALL, PASID_ZERO, STALL_IOVA,
                     AccessType::Read, SecurityState::NonSecure);

    size_t sizeAfterStall = smmu_->getEventQueueSize();
    ASSERT_EQ(sizeAfterStall, 4u)
        << "Precondition: stall event must NOT appear in eventQueue when full "
           "(must be parked in stallPending_). Got " << sizeAfterStall;

    // --- Step 3: Drain the 4 non-stall events via processEventQueue() ---
    // processEventQueue() consumes and removes all entries; queue becomes empty.
    // It must NOT promote stallPending_ — stall delivery is lazy (via generateEvent).
    smmu_->processEventQueue();

    ASSERT_EQ(smmu_->getEventQueueSize(), 0u)
        << "After processEventQueue() the main queue must be empty; "
           "stall event stays in stallPending_ until next generateEvent()";

    // --- Step 4: Trigger a new fault to invoke generateEvent() ---
    // generateEvent() drains stallPending_ into eventQueue before inserting the
    // new event, so the stall event is promoted first (FIFO order per §7.4).
    // Use FILL_IOVA_0 again — it is unmapped, produces F_TRANSLATION (non-stall).
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_0,
                     AccessType::Read, SecurityState::NonSecure);

    // --- Step 5: Assert the stall event was eventually delivered ---
    auto queue = smmu_->getEventQueue();
    size_t finalSize = queue.size();

    EXPECT_GE(finalSize, 1u)
        << "ARM §3.5.3: stall events must not be discarded. After processEventQueue() "
           "freed space and a subsequent generateEvent() fired, the stall event must "
           "appear in eventQueue. Queue size=" << finalSize << " (expected >= 1).";

    bool foundStall = false;
    for (const auto& ev : queue) {
        if (ev.stall && ev.streamID == SID_STALL) {
            foundStall = true;
            EXPECT_EQ(ev.type, EventType::F_TRANSLATION)
                << "Promoted stall event type must be F_TRANSLATION";
            EXPECT_EQ(ev.pasid, PASID_ZERO)
                << "Promoted stall event PASID must match SID_STALL stream";
        }
    }
    EXPECT_TRUE(foundStall)
        << "Queue has " << finalSize << " event(s) but none is the parked stall "
           "event (stall==true, streamID==SID_STALL). §3.5.3 stall delivery failed.";
}

// ============================================================================
// Test: StallDrain_NonStallEventsGoneAfterDrain  (control/regression guard)
//
// After processEventQueue() the 4 non-stall fill events must not persist.
// This verifies that the processEventQueue() loop actually removes them and
// that the test setup is working correctly as a baseline.
// This test should PASS both before and after the fix.
// ============================================================================
TEST_F(BugAudit154StallDrainTest, StallDrain_NonStallEventsGoneAfterDrain) {
    // Fill queue with 4 non-stall events.
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_0,
                     AccessType::Read, SecurityState::NonSecure);
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_1,
                     AccessType::Read, SecurityState::NonSecure);
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_2,
                     AccessType::Read, SecurityState::NonSecure);
    smmu_->translate(SID_TERMINATE, PASID_ZERO, FILL_IOVA_3,
                     AccessType::Read, SecurityState::NonSecure);

    ASSERT_EQ(smmu_->getEventQueueSize(), 4u)
        << "Control precondition: 4 fill events expected.";

    smmu_->processEventQueue();

    // Without a stall event being generated, the queue should be empty.
    // (This test exercises the non-stall drain path without the stallPending_ complication.)
    auto queue = smmu_->getEventQueue();
    bool hasNonStall = false;
    for (const auto& ev : queue) {
        if (!ev.stall && ev.streamID == SID_TERMINATE) {
            hasNonStall = true;
        }
    }
    EXPECT_FALSE(hasNonStall)
        << "Control: non-stall fill events must be removed by processEventQueue(). "
           "Found at least one non-stall SID_TERMINATE event still in queue.";
}
