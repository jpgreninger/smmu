// Bug 3: Stall-pending EventEntry missing wire-format fields
//
// ARM IHI0070G.b §7.3: Every event record must carry RnW, PnU, InD, SSV,
// S2, CLASS, and IPA fields.
//
// Root cause: In generateEvent() (smmu.cpp), when the event queue is full AND
// the event is a stall-eligible fault, the event is parked in stallPending_
// instead of eventQueue.  The stall-pending path only sets:
//   type, streamID, pasid, address, securityState, timestamp, stall, stag,
//   errorCode
// All other §7.3 wire-format fields (ssv, eventClass, rnw, ind, pnu, s2, ipa,
// nsipa) remain zero-initialized.  When the stall-pending event is later flushed
// into the event queue (at the next generateEvent() call), it carries stale
// zero values for those fields — violating §7.3.
//
// TDD: These tests MUST FAIL (red state) before the fix because the
// stall-pending path does not populate the §7.3 wire-format fields.
//
// Test strategy:
//   1. Build an SMMU with a small event queue (capacity SMALL_QUEUE).
//   2. Fill the event queue to capacity with C_BAD_STREAMID faults on
//      unconfigured streams (non-stall; queue reaches maxEventQueueSize).
//   3. Trigger a stall-eligible translation fault that uses specific,
//      observable access attributes:
//        - AccessType::WritePrivileged → rnw=true (write), pnu=true (privileged)
//        - non-zero PASID              → ssv=true
//        - two-stage config            → s2=true, ipa != 0
//      Because the queue is full and isStall=true the event goes to stallPending_.
//   4. Drain the main event queue (processEventQueue()).
//   5. Trigger one harmless non-stall fault on a separate non-stall stream so
//      that generateEvent() flushes stallPending_ into the now-empty queue.
//   6. Inspect the flushed event entry and assert the §7.3 fields match what
//      was used during the fault.
//
// Spec references:
//   ARM IHI0070G.b §3.5.3   Event queue overflow / stall handling
//   ARM IHI0070G.b §7.3     Event record wire format
//   ARM IHI0070G.b §7.3.13  F_TRANSLATION: RnW, PnU, InD, SSV, S2, IPA

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>
#include <vector>
#include <cstdint>

namespace smmu {
namespace test {

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

static constexpr size_t   SMALL_QUEUE        = 16;
static constexpr StreamID FILLER_SID_BASE    = 0x0A00; // Non-configured SIDs for queue fill
static constexpr StreamID STALL_SID          = 0x0B01; // Stall-mode, two-stage stream
static constexpr StreamID FLUSH_TRIGGER_SID  = 0x0C00; // Non-stall stream; triggers stallPending_ drain
static constexpr PASID    NONZERO_PASID      = 7u;     // Must be != 0 so ssv=true
static constexpr IOVA     STALL_IOVA         = 0x0002'0000ULL; // Unmapped → F_TRANSLATION stall
static constexpr IOVA     FLUSH_IOVA         = 0x0003'0000ULL; // For the flush-trigger fault
static constexpr IOVA     STAGE1_IOVA        = 0x0002'0000ULL; // Same as STALL_IOVA — no mapping
static constexpr PA       STAGE2_IPA         = 0x0004'0000ULL; // IPA used in stage-2 lookup
static constexpr PA       STAGE2_PA          = 0x0005'0000ULL; // Final PA (stage-2 output)

// ---------------------------------------------------------------------------
// Helper: build SMMU with small queue and CR2_RECINVSID enabled so that
// C_BAD_STREAMID faults are recorded and can be used to fill the queue.
// ---------------------------------------------------------------------------

static std::unique_ptr<SMMU> makeSmallQueueSMMU() {
    QueueConfiguration qcfg(SMALL_QUEUE, SMALL_QUEUE, SMALL_QUEUE);
    SMMUConfiguration  cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto smmu = std::make_unique<SMMU>(cfg);
    smmu->enable();
    // CR2_RECINVSID: record C_BAD_STREAMID so unknown-stream faults fill the queue.
    smmu->setCR2(SMMU::CR2_RECINVSID);
    return smmu;
}

// Configure a stall-mode two-stage stream.  Stage-1 maps no pages (any
// translation on STAGE1_IOVA will fault) so the F_TRANSLATION fault stalls.
// The stage-2 AS is set up with a valid mapping so that the two-stage path
// reaches stage-2 before the stage-1 miss is detected, ensuring s2/ipa fields
// would be non-zero in a correct implementation.
//
// Note: t0sz=0 avoids VA-range-exceeded checks (no 48-bit cap).
static void setupTwoStageStallStream(SMMU& smmu) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Stall;
    cfg.t0sz               = 0;   // No VA-range restriction
    cfg.s2t0sz             = 0;   // No IPA-range restriction
    cfg.ips                = 6;   // 52-bit stage-1 output (IPS)
    smmu.configureStream(STALL_SID, cfg);
    smmu.enableStream(STALL_SID);
    // Create the PASID that will be used for the stall fault.
    smmu.createStreamPASID(STALL_SID, NONZERO_PASID);
    // Set up a stage-2 address space.  We do NOT map STAGE2_IPA → STAGE2_PA
    // yet — this means a stage-2 fault will fire, which is an F_TRANSLATION
    // fault on the IPA, making s2=true and ipa=STAGE2_IPA observable.
    // But we need stage-1 to produce the IPA so we map stage-1: STALL_IOVA→STAGE2_IPA.
    smmu.mapPage(STALL_SID, NONZERO_PASID, STAGE1_IOVA, STAGE2_IPA,
                 PagePermissions(true, true, false));
    // Stage-2 AS with no mapping for STAGE2_IPA → will produce stage-2 F_TRANSLATION.
    auto s2as = std::shared_ptr<AddressSpace>(new AddressSpace());
    smmu.setStreamStage2AddressSpace(STALL_SID, s2as);
}

// Configure a non-stall terminate-mode stream used only to trigger a
// generateEvent() call so that stallPending_ is flushed into the queue.
static void setupFlushTriggerStream(SMMU& smmu) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    smmu.configureStream(FLUSH_TRIGGER_SID, cfg);
    smmu.enableStream(FLUSH_TRIGGER_SID);
    smmu.createStreamPASID(FLUSH_TRIGGER_SID, 0);
    // No page mapping — any translate() will produce F_TRANSLATION (non-stall).
}

// Fill the event queue to exactly SMALL_QUEUE entries by issuing faults on
// unconfigured stream IDs (C_BAD_STREAMID recorded because CR2_RECINVSID=1).
static void fillEventQueue(SMMU& smmu) {
    for (StreamID sid = FILLER_SID_BASE;
         sid < static_cast<StreamID>(FILLER_SID_BASE + SMALL_QUEUE);
         ++sid) {
        smmu.translate(sid, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    }
}

// Find the first event in the queue that matches the stall stream.
// Returns nullptr if not found.
static const EventEntry* findStallEvent(const std::vector<EventEntry>& events) {
    for (const auto& e : events) {
        if (e.streamID == STALL_SID) {
            return &e;
        }
    }
    return nullptr;
}

// ---------------------------------------------------------------------------
// Test 1: StallPendingEvent_HasCorrectRnWField
//
// A stall event triggered with AccessType::WritePrivileged must have rnw=true
// after being deferred through stallPending_ and flushed into the event queue.
//
// BUG: The stall-pending path does not copy the accessType-derived fields, so
// rnw stays false (zero-initialized) regardless of the actual access type.
// This test FAILS before the fix.
// ---------------------------------------------------------------------------

TEST(Bug3StallPendingFields, StallPendingEvent_HasCorrectRnWField) {
    auto smmu = makeSmallQueueSMMU();
    setupTwoStageStallStream(*smmu);
    setupFlushTriggerStream(*smmu);

    // Step 1: Fill the event queue to capacity.
    fillEventQueue(*smmu);
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_QUEUE)
        << "Precondition: event queue must be full before stall fault";

    // Step 2: Trigger a stall fault with WritePrivileged access.
    // rnw should be true (write), pnu should be true (privileged).
    // Because the queue is full the event goes to stallPending_.
    auto stallResult = smmu->translate(STALL_SID, NONZERO_PASID, STALL_IOVA,
                                       AccessType::WritePrivileged,
                                       SecurityState::NonSecure);
    // The translation must stall (not succeed).
    ASSERT_TRUE(stallResult.isError())
        << "Stall-mode fault must return an error";
    EXPECT_EQ(stallResult.getError(), SMMUError::Stalled)
        << "Stall-mode fault must return SMMUError::Stalled";

    // The event queue must still be at capacity (stall event went to stallPending_).
    EXPECT_EQ(smmu->getEventQueueSize(), SMALL_QUEUE)
        << "Event queue must remain at capacity; stall event must go to stallPending_";

    // Step 3: Drain the main event queue.
    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "Event queue must be empty after processEventQueue()";

    // Step 4: Trigger one non-stall fault on the flush-trigger stream.
    // generateEvent() will drain stallPending_ into the now-empty event queue
    // before inserting the new event.
    smmu->translate(FLUSH_TRIGGER_SID, 0, FLUSH_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    // Step 5: Inspect the flushed stall event.
    auto events = smmu->getEventQueue();
    const EventEntry* stallEvent = findStallEvent(events);
    ASSERT_NE(stallEvent, nullptr)
        << "Stall event must be present in event queue after flush from stallPending_";

    // BUG: rnw is zero-initialized in stallPending_ path.
    // After Bug3 fix: rnw/ind/pnu must be populated from accessType.
    // RnW-GAP fix: ARM §7.3 RnW=0=Write. WritePrivileged is a write → rnw=false.
    EXPECT_FALSE(stallEvent->rnw)
        << "Bug 3 + RnW-GAP: rnw must be false for WritePrivileged access "
           "(ARM §7.3 RnW=0=Write); stall-pending path must populate accessType fields";
}

// ---------------------------------------------------------------------------
// Test 2: StallPendingEvent_HasCorrectSSVField
//
// A stall event triggered with a non-zero PASID must have ssv=true after being
// deferred through stallPending_ and flushed into the event queue.
//
// BUG: The stall-pending path does not set ssv, so it stays false regardless
// of whether PASID != 0.
// This test FAILS before the fix.
// ---------------------------------------------------------------------------

TEST(Bug3StallPendingFields, StallPendingEvent_HasCorrectSSVField) {
    auto smmu = makeSmallQueueSMMU();
    setupTwoStageStallStream(*smmu);
    setupFlushTriggerStream(*smmu);

    // Step 1: Fill the event queue to capacity.
    fillEventQueue(*smmu);
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_QUEUE)
        << "Precondition: event queue must be full before stall fault";

    // Step 2: Trigger a stall fault with NONZERO_PASID (SSV should be true).
    auto stallResult = smmu->translate(STALL_SID, NONZERO_PASID, STALL_IOVA,
                                       AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(stallResult.isError());
    EXPECT_EQ(stallResult.getError(), SMMUError::Stalled);

    // Step 3: Drain and flush.
    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u);
    smmu->translate(FLUSH_TRIGGER_SID, 0, FLUSH_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    // Step 4: Inspect the flushed event.
    auto events = smmu->getEventQueue();
    const EventEntry* stallEvent = findStallEvent(events);
    ASSERT_NE(stallEvent, nullptr)
        << "Stall event must be present in event queue after flush";

    // BUG: ssv is zero-initialized in stallPending_ path.
    // After fix: ssv must be true because NONZERO_PASID != 0.
    EXPECT_TRUE(stallEvent->ssv)
        << "Bug 3: ssv must be true when PASID=" << NONZERO_PASID
        << " (non-zero); stall-pending path does not set ssv (§7.3 SSV)";

    // Confirm PASID is preserved correctly (should always pass — it is copied).
    EXPECT_EQ(stallEvent->pasid, NONZERO_PASID)
        << "pasid field must survive the stall-pending path";
}

// ---------------------------------------------------------------------------
// Test 3: StallPendingEvent_HasCorrectStage2Fields
//
// A stall event triggered during a two-stage translation fault (stage-2 miss)
// must have s2=true and ipa != 0 after being deferred through stallPending_
// and flushed into the event queue.
//
// BUG: The stall-pending path does not copy isStage2 or ipaValue into the
// EventEntry, so s2 stays false and ipa stays 0.
// This test FAILS before the fix.
// ---------------------------------------------------------------------------

TEST(Bug3StallPendingFields, StallPendingEvent_HasCorrectStage2Fields) {
    auto smmu = makeSmallQueueSMMU();
    setupTwoStageStallStream(*smmu);
    setupFlushTriggerStream(*smmu);

    // Step 1: Fill the event queue to capacity.
    fillEventQueue(*smmu);
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_QUEUE)
        << "Precondition: event queue must be full before stall fault";

    // Step 2: Trigger a stall fault on the two-stage stream.
    // Stage-1 maps STALL_IOVA -> STAGE2_IPA, but stage-2 has no mapping for
    // STAGE2_IPA, so an F_TRANSLATION fault fires during stage-2 lookup with
    // isStage2=true and ipaValue=STAGE2_IPA.
    auto stallResult = smmu->translate(STALL_SID, NONZERO_PASID, STALL_IOVA,
                                       AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(stallResult.isError());
    EXPECT_EQ(stallResult.getError(), SMMUError::Stalled);

    // Step 3: Drain and flush.
    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u);
    smmu->translate(FLUSH_TRIGGER_SID, 0, FLUSH_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    // Step 4: Inspect the flushed event.
    auto events = smmu->getEventQueue();
    const EventEntry* stallEvent = findStallEvent(events);
    ASSERT_NE(stallEvent, nullptr)
        << "Stall event must be present in event queue after flush";

    // BUG: s2 is zero-initialized in stallPending_ path.
    // After fix: s2 must be true because the fault occurred in stage-2.
    EXPECT_TRUE(stallEvent->s2)
        << "Bug 3: s2 must be true for a stage-2 translation fault; "
           "stall-pending path does not copy isStage2 parameter (§7.3 S2)";

    // BUG: ipa is zero-initialized in stallPending_ path.
    // After fix: ipa must equal STAGE2_IPA (the IPA that caused the stage-2 fault).
    EXPECT_NE(stallEvent->ipa, 0u)
        << "Bug 3: ipa must be non-zero for a stage-2 translation fault; "
           "stall-pending path does not copy ipaValue parameter (§7.3 IPA)";

    EXPECT_EQ(stallEvent->ipa, STAGE2_IPA)
        << "Bug 3: ipa must equal the stage-1 output IPA=" << std::hex << STAGE2_IPA
        << " but got ipa=" << stallEvent->ipa
        << "; stall-pending path does not copy ipaValue (§7.3 IPA)";
}

} // namespace test
} // namespace smmu
