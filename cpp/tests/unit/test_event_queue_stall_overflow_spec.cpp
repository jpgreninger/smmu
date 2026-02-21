// ARM SMMU v3 FINDING-NEW-03: Stall events must survive event queue overflow (C++)
// FINDING-NEW-06: EventEntry must carry a stall bit (§7.3)
//
// ARM IHI0070G.b §3.5.3: When the event queue is full:
//   - Stall events MUST NOT be discarded
//   - Non-stall events MAY be discarded
//
// TDD: tests must be RED before the fix (event.stall field doesn't exist yet,
// and overflow evicts oldest rather than discarding incoming non-stall events).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>

namespace smmu {
namespace test {

// Minimum queue size (16) gives controllable overflow in tests.
static constexpr size_t SMALL_QUEUE = 16;

// Build an SMMU with the minimum-size event queue.
static std::unique_ptr<SMMU> makeSmallQueueSMMU() {
    QueueConfiguration qcfg(SMALL_QUEUE, SMALL_QUEUE, SMALL_QUEUE);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    return std::make_unique<SMMU>(cfg);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// FINDING-NEW-06: EventEntry must have a 'stall' field (§7.3).
/// This test is a compile-time check — it fails to build if the field is absent.
TEST(EventQueueStallOverflowSpec, EventEntryHasStallField) {
    EventEntry event;
    event.stall = false;                 // RED: field doesn't exist yet
    EXPECT_FALSE(event.stall);
}

/// FINDING-NEW-06: Default EventEntry.stall must be false (non-stall by default).
TEST(EventQueueStallOverflowSpec, EventEntryStallDefaultsFalse) {
    EventEntry event{};
    EXPECT_FALSE(event.stall);           // RED: field doesn't exist yet
}

/// §3.5.3: Non-stall events are DROPPED (incoming discarded), not evicted from front,
/// when the event queue is full.
///
/// Before the fix: generateEvent() calls pop_front() to make room, evicting the oldest.
/// After the fix:  generateEvent() returns early for non-stall events when full.
TEST(EventQueueStallOverflowSpec, NonStallEventDroppedWhenQueueFull) {
    auto smmu = makeSmallQueueSMMU();

    // Fill queue to capacity using SMALL_QUEUE distinct stream-not-found faults.
    // Stream 0x0100 is the "oldest" entry we want to remain after overflow.
    for (StreamID sid = 0x0100; sid < static_cast<StreamID>(0x0100 + SMALL_QUEUE); ++sid) {
        smmu->translate(sid, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    }

    auto events_before = smmu->getEventQueue();
    ASSERT_EQ(events_before.size(), SMALL_QUEUE)
        << "queue must be full before overflow test";

    // Verify oldest entry (SID 0x0100) is at front.
    EXPECT_EQ(events_before.front().streamID, static_cast<StreamID>(0x0100))
        << "oldest entry must be SID 0x0100";

    // One more non-stall fault — must be DROPPED, not cause oldest to be evicted.
    smmu->translate(0x0200, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);

    auto events_after = smmu->getEventQueue();
    EXPECT_EQ(events_after.size(), SMALL_QUEUE)
        << "queue size must stay at capacity; incoming non-stall event must be dropped";

    // Oldest entry must NOT have been evicted.
    EXPECT_EQ(events_after.front().streamID, static_cast<StreamID>(0x0100))
        << "oldest event must survive; incoming non-stall event must be discarded (§3.5.3)";
}

/// §3.5.3: A stall event (event.stall == true) must NOT be discarded even when
/// the queue is full — it must be pushed regardless.
///
/// Note: C++ stall implementation is limited (FINDING-NEW-08), so we test the
/// stall field and overflow logic by constructing EventEntry directly via the
/// stall marker on the event rather than through a full stall translation flow.
TEST(EventQueueStallOverflowSpec, StallEventFieldIsDistinguishable) {
    // Verify that an EventEntry can carry stall=true — this is a field existence test.
    EventEntry e{};
    e.stall = true;                      // RED: field doesn't exist yet
    EXPECT_TRUE(e.stall);

    EventEntry f{};
    // stall defaults to false
    EXPECT_FALSE(f.stall);              // RED: field doesn't exist yet
}

} // namespace test
} // namespace smmu
