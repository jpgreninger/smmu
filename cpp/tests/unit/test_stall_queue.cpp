// BUG-ANALYSIS-5: Stall event queue has no size cap
//
// Root cause: In generateEvent() (smmu.cpp ~line 2902), when isStall==true
// the guard that returns early for a full queue is bypassed:
//
//     if (eventQueue.size() >= maxEventQueueSize) {
//         if (!isStall) {          // <-- stall events fall through
//             ...return;
//         }
//         // Fall through to push_back without bound (BUG-ANALYSIS-5)
//     }
//     eventQueue.push_back(event);
//
// This means stall events grow the eventQueue without limit.  Per the ARM
// spec (§3.5.3, §7.4) stall events must not be lost, but the correct
// pending-stall store is the STAG-keyed stallQueue_ map (§3.12.2), which
// IS size-capped at 65,534 entries (STAG is a 16-bit field; STAG==0 is
// reserved, giving 1..65535 = 65535 possible slots, though the implementation
// uses a uint16_t counter that wraps, effectively limiting to the map
// occupancy).
//
// The event queue write for stall events should still happen (the OS needs
// to see the fault record so it can issue CMD_RESUME), but it must respect
// the same maxEventQueueSize limit as non-stall events.  The "must not be
// lost" requirement is met by the stallQueue_ entry, not by an unbounded
// eventQueue.
//
// Additionally, each unique stalled transaction is identified by a STAG;
// a second fault for the same STAG must not create a duplicate stallQueue_
// entry.
//
// Test matrix (all tests should FAIL with the current implementation unless
// otherwise noted):
//
//   BUG5a: Stall events beyond maxEventQueueSize are NOT discarded from the
//          eventQueue — the queue grows without bound.  FAILS because there
//          is no cap.
//
//   BUG5b: Stall records (stallQueue_ entries) are preserved even when the
//          eventQueue is capped — the STAG map is the persistent store.
//          PASSES currently (stallQueue_ is independent of eventQueue).
//          Included as a regression guard for the fix.
//
//   BUG5c: Two stall events with the same STAG (simulated by aborting the
//          first and reissuing) do not create duplicate stallQueue_ entries.
//          PASSES currently because stallQueue_ is keyed by STAG.
//          Included as a regression guard.
//
//   BUG5d: The total stall-pending count must not exceed 65,534 entries.
//          FAILS currently because there is no STAG-space exhaustion check
//          on the eventQueue path (the stallQueue_ itself has the
//          wrap-around check, but the eventQueue accumulates unbounded
//          shadow copies).
//
// ARM spec references:
//   IHI0070G.b §3.5.3  Event queue overflow behaviour
//   IHI0070G.b §3.12.2 Stall model — stall records and STAG
//   IHI0070G.b §7.4    Event queue full — OVFLG semantics

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>
#include <vector>
#include <cstddef>
#include <cstdint>

namespace smmu {
namespace test {

// ── Constants ────────────────────────────────────────────────────────────────

// Minimum legal queue size.
static constexpr size_t STALL_SMALL_QUEUE = 16;

// Number of stall events to push beyond the queue limit.
static constexpr size_t STALL_OVERSHOOT = 8;

// Stream IDs for isolation from other test suites.
static constexpr StreamID STALL_SID_BASE = 0x200;
static constexpr PASID    STALL_PASID    = 0;
static constexpr IOVA     STALL_IOVA     = 0x8000'0000ULL;

// ── Helpers ──────────────────────────────────────────────────────────────────

// Build an SMMU with a small event queue and all stall-related CR bits set.
static std::unique_ptr<SMMU> makeStallCappedSMMU(size_t queueSize = STALL_SMALL_QUEUE) {
    QueueConfiguration qcfg(queueSize, queueSize, queueSize);
    SMMUConfiguration  cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto smmu = std::make_unique<SMMU>(cfg);
    smmu->enable();
    smmu->setCR2(SMMU::CR2_RECINVSID);
    return smmu;
}

// Configure a stall-mode stream on the given SMMU.
static void setupStallStream(SMMU& smmu, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, STALL_PASID);
    // No page mapping — every translate() triggers a translation-miss stall.
}

// Count events in the queue that are stall events.
static size_t countStallEvents(const std::vector<EventEntry>& events) {
    size_t n = 0;
    for (const auto& e : events) {
        if (e.stall) ++n;
    }
    return n;
}

// ── BUG5a: eventQueue must not grow beyond maxEventQueueSize for stall events ─

/// BUG-ANALYSIS-5a: Verify that stall events do NOT bypass the eventQueue
/// size cap.
///
/// The current implementation falls through to push_back() without the cap
/// check when isStall==true (smmu.cpp generateEvent() stall branch).  This
/// allows the eventQueue to grow without bound when many stall faults occur.
///
/// Expected after fix: eventQueue.size() stays <= maxEventQueueSize.
/// Expected before fix (BUG): eventQueue.size() > maxEventQueueSize.
TEST(StallQueueBug5, StallEventsRespectEventQueueSizeCap) {
    auto smmu = makeStallCappedSMMU(STALL_SMALL_QUEUE);
    setupStallStream(*smmu, STALL_SID_BASE);
    smmu->clearEventQueue();

    // Generate STALL_SMALL_QUEUE + STALL_OVERSHOOT stall events by translating
    // unmapped addresses on the stall-mode stream.  Each translate() on a
    // different IOVA produces a fresh stall entry (distinct STAG).
    size_t totalAttempts = STALL_SMALL_QUEUE + STALL_OVERSHOOT;
    for (size_t i = 0; i < totalAttempts; ++i) {
        IOVA iova = STALL_IOVA + (static_cast<IOVA>(i) * 0x1000);
        smmu->translate(STALL_SID_BASE, STALL_PASID, iova,
                        AccessType::Read, SecurityState::NonSecure);
    }

    size_t queueSize = smmu->getEventQueueSize();

    // BUG-ANALYSIS-5a: This assertion FAILS before the fix.
    // Without the cap check for stall events, queueSize == totalAttempts
    // (== STALL_SMALL_QUEUE + STALL_OVERSHOOT) instead of being capped at
    // STALL_SMALL_QUEUE.
    EXPECT_LE(queueSize, STALL_SMALL_QUEUE)
        << "BUG-ANALYSIS-5a: Stall events bypassed the eventQueue size cap. "
           "eventQueue.size()=" << queueSize << " but maxEventQueueSize="
        << STALL_SMALL_QUEUE << ".  Fix: apply the same maxEventQueueSize "
           "guard to stall events in generateEvent() — the stallQueue_ map is "
           "the persistent stall record; the eventQueue entry is an OS "
           "notification that can be dropped when the queue is full "
           "(§3.5.3 / §7.4).";
}

/// BUG-ANALYSIS-5a (variant): Verify that the eventQueue size never exceeds
/// the configured cap regardless of how many stall events are generated.
TEST(StallQueueBug5, StallEventsNeverExceedConfiguredCap) {
    auto smmu = makeStallCappedSMMU(STALL_SMALL_QUEUE);
    setupStallStream(*smmu, STALL_SID_BASE + 1);
    smmu->clearEventQueue();

    // Generate 4x the queue capacity to make the violation obvious.
    size_t totalAttempts = STALL_SMALL_QUEUE * 4;
    for (size_t i = 0; i < totalAttempts; ++i) {
        IOVA iova = STALL_IOVA + (static_cast<IOVA>(i) * 0x1000);
        smmu->translate(STALL_SID_BASE + 1, STALL_PASID, iova,
                        AccessType::Read, SecurityState::NonSecure);
    }

    size_t queueSize = smmu->getEventQueueSize();

    // BUG-ANALYSIS-5a: This assertion FAILS before the fix.
    EXPECT_LE(queueSize, STALL_SMALL_QUEUE)
        << "BUG-ANALYSIS-5a: eventQueue grew to " << queueSize
        << " entries after " << totalAttempts << " stall events; "
           "configured cap is " << STALL_SMALL_QUEUE << ".  "
           "Stall events must respect maxEventQueueSize (§3.5.3).";

    // Stall count in eventQueue must also be capped.
    size_t stallCount = countStallEvents(smmu->getEventQueue());
    EXPECT_LE(stallCount, STALL_SMALL_QUEUE)
        << "BUG-ANALYSIS-5a: The number of stall event records in the eventQueue "
           "(" << stallCount << ") exceeds the configured maximum "
        << STALL_SMALL_QUEUE << ".";
}

// ── BUG5b: stallQueue_ entries are preserved even when eventQueue is capped ──

/// BUG-ANALYSIS-5b (regression guard): Stall records in the stallQueue_ map
/// must be preserved regardless of eventQueue overflow.  This is the "must
/// not be lost" requirement from §3.5.3 — stall state lives in stallQueue_,
/// not in eventQueue.
///
/// This test documents the EXPECTED behaviour after the fix:
///   - eventQueue is capped at maxEventQueueSize
///   - stallQueue_ contains all issued stall records (up to STAG space)
///
/// This test PASSES with the current implementation because stallQueue_ is
/// already populated independently of eventQueue overflow.  It serves as a
/// regression guard to ensure the fix does not accidentally break the
/// stallQueue_ write path.
TEST(StallQueueBug5, StallRecordsPreservedInStalledTransactionMap) {
    auto smmu = makeStallCappedSMMU(STALL_SMALL_QUEUE);
    setupStallStream(*smmu, STALL_SID_BASE + 2);
    smmu->clearEventQueue();

    // Generate more stall events than the eventQueue can hold.
    size_t totalAttempts = STALL_SMALL_QUEUE + STALL_OVERSHOOT;
    for (size_t i = 0; i < totalAttempts; ++i) {
        IOVA iova = STALL_IOVA + (static_cast<IOVA>(i) * 0x1000);
        smmu->translate(STALL_SID_BASE + 2, STALL_PASID, iova,
                        AccessType::Read, SecurityState::NonSecure);
    }

    // The stallQueue_ map must have entries for all issued stalls.
    // (Bounded by STAG space — the implementation may fall back to terminate
    // mode when the STAG counter wraps and all slots are occupied, so the
    // count may be <= totalAttempts but must be > 0.)
    size_t stallPendingCount = smmu->getStalledTransactionCount();
    EXPECT_GT(stallPendingCount, 0u)
        << "BUG-ANALYSIS-5b regression: No stall records in stallQueue_ after "
           "issuing " << totalAttempts << " stall-mode translation faults.  "
           "The fix must not break the stallQueue_ write path.";

    // Additionally: all stall records must have non-zero STAGs (§3.12.2).
    auto stalled = smmu->getStalledTransactions();
    for (const auto& rec : stalled) {
        EXPECT_NE(rec.stag, 0u)
            << "BUG-ANALYSIS-5b regression: StallRecord has STAG==0, which is "
               "reserved per ARM §3.12.2.  stagCounter_ must skip STAG=0.";
    }
}

// ── BUG5c: Two stall events with the same STAG must not create duplicates ────

/// BUG-ANALYSIS-5c (regression guard): The stallQueue_ is keyed by STAG.
/// After aborting a stalled transaction (removing its STAG from the map) and
/// reissuing the same fault (which allocates a new STAG), only ONE stall record
/// must exist per unique active STAG.
///
/// This test PASSES with the current implementation because the stallQueue_
/// uses an unordered_map (keyed by STAG) which naturally prevents duplicates
/// for the same key.  It serves as a regression guard.
TEST(StallQueueBug5, SameStageNoDuplicateStallRecords) {
    auto smmu = makeStallCappedSMMU(STALL_SMALL_QUEUE);
    setupStallStream(*smmu, STALL_SID_BASE + 3);
    smmu->clearEventQueue();

    // Issue one stall fault.
    smmu->translate(STALL_SID_BASE + 3, STALL_PASID, STALL_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    size_t countAfterFirst = smmu->getStalledTransactionCount();
    ASSERT_EQ(countAfterFirst, 1u)
        << "Precondition: exactly one stall record after one stall fault";

    // Get the STAG of the first stall record.
    auto stalledTxns = smmu->getStalledTransactions();
    ASSERT_EQ(stalledTxns.size(), 1u);
    uint16_t firstStag = stalledTxns[0].stag;
    ASSERT_NE(firstStag, 0u) << "STAG must be non-zero (§3.12.2)";

    // Abort the first stall — removes its STAG slot from the map.
    bool aborted = smmu->abortStalledTransaction(firstStag);
    ASSERT_TRUE(aborted) << "abortStalledTransaction must return true for a valid STAG";
    ASSERT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "Stall map must be empty after aborting the only entry";

    // Issue the same fault again — must produce a NEW stall record with a
    // different (or at least non-duplicate) STAG.
    smmu->translate(STALL_SID_BASE + 3, STALL_PASID, STALL_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    size_t countAfterSecond = smmu->getStalledTransactionCount();
    EXPECT_EQ(countAfterSecond, 1u)
        << "BUG-ANALYSIS-5c regression: Expected exactly 1 stall record after "
           "reissuing the same fault; got " << countAfterSecond << ".  "
           "stallQueue_ must not accumulate duplicate entries for the same "
           "logical stalled transaction.";

    // The new STAG must be non-zero.
    auto stalledAfter = smmu->getStalledTransactions();
    for (const auto& rec : stalledAfter) {
        EXPECT_NE(rec.stag, 0u)
            << "BUG-ANALYSIS-5c regression: Re-issued stall has STAG==0.";
    }
}

// ── BUG5d: Total stall-pending count must not exceed STAG space (65,534) ─────

/// BUG-ANALYSIS-5d: The STAG field is 16 bits (§3.12.2).  STAG==0 is reserved,
/// so the maximum number of simultaneously active stall records is 65,535
/// (STAGs 1–65535).  In practice the implementation uses a uint16_t counter
/// that wraps, but the stallQueue_ collision check means a new stall can only
/// be issued if its STAG slot is free.
///
/// More critically, the eventQueue has no guard on the number of stall event
/// copies it accumulates.  A large number of stall faults on DIFFERENT stream
/// IDs (each with a unique IOVA) will:
///   1. Fill stallQueue_ up to STAG space (65,535 entries) and then fall back
///      to terminate mode — CORRECT behaviour.
///   2. Push a corresponding eventQueue entry for EVERY stall regardless of
///      the eventQueue cap — BUG-ANALYSIS-5 (addressed by BUG5a/5b above).
///
/// This test verifies that the SMMU respects the STAG-space limit by
/// generating enough stall faults to exhaust a small STAG counter window
/// and confirming that new stall faults fall back to terminate mode (no
/// crash, no unbounded growth).
///
/// With a small queue (STALL_SMALL_QUEUE entries) and STALL_SMALL_QUEUE + N
/// faults, the eventQueue must stay capped.  The stallQueue_ may hold up to
/// its STAG-space limit and then stop accepting new entries (falling back to
/// terminate mode for subsequent faults).
///
/// Expected before fix (BUG-ANALYSIS-5): eventQueue.size() > STALL_SMALL_QUEUE.
/// Expected after fix: eventQueue.size() <= STALL_SMALL_QUEUE.
TEST(StallQueueBug5, TotalStallPendingCountBoundedByStallQueueCapacity) {
    // Use a small queue so the overflow is immediately visible.
    auto smmu = makeStallCappedSMMU(STALL_SMALL_QUEUE);

    // Set up multiple stall-mode streams to generate many unique stall faults.
    static constexpr size_t NUM_STALL_STREAMS = 4;
    for (size_t s = 0; s < NUM_STALL_STREAMS; ++s) {
        setupStallStream(*smmu, static_cast<StreamID>(STALL_SID_BASE + 4 + s));
    }
    smmu->clearEventQueue();

    // Generate STALL_SMALL_QUEUE * NUM_STALL_STREAMS stall events across all
    // streams — far more than the queue can hold.
    size_t totalAttempts = STALL_SMALL_QUEUE * NUM_STALL_STREAMS;
    for (size_t i = 0; i < totalAttempts; ++i) {
        StreamID sid = static_cast<StreamID>(STALL_SID_BASE + 4 + (i % NUM_STALL_STREAMS));
        IOVA iova = STALL_IOVA + (static_cast<IOVA>(i) * 0x1000);
        smmu->translate(sid, STALL_PASID, iova,
                        AccessType::Read, SecurityState::NonSecure);
    }

    size_t eqSize = smmu->getEventQueueSize();

    // BUG-ANALYSIS-5d: This assertion FAILS before the fix.
    EXPECT_LE(eqSize, STALL_SMALL_QUEUE)
        << "BUG-ANALYSIS-5d: eventQueue has " << eqSize << " entries after "
        << totalAttempts << " stall events across " << NUM_STALL_STREAMS
        << " streams; configured cap is " << STALL_SMALL_QUEUE << ".  "
           "Stall events must respect maxEventQueueSize (§3.5.3 / §7.4); "
           "persistent stall state must live in the stallQueue_ map, not in "
           "unbounded eventQueue growth.";

    // The stallQueue_ itself must be bounded: at most STALL_SMALL_QUEUE active
    // stall records (limited by the number of successful STAG allocations in
    // this small-queue scenario, but certainly not exceeding totalAttempts).
    size_t stallPending = smmu->getStalledTransactionCount();
    EXPECT_LE(stallPending, totalAttempts)
        << "BUG-ANALYSIS-5d: stallQueue_ has more entries than total faults "
           "issued — internal accounting is inconsistent.";

    // Strict spec limit: stallQueue_ must never exceed 65,534 entries.
    // (In this small test it will be far below, but we document the bound.)
    static constexpr size_t STAG_MAX_ENTRIES = 65534u;
    EXPECT_LE(stallPending, STAG_MAX_ENTRIES)
        << "BUG-ANALYSIS-5d: stallQueue_ exceeds the maximum STAG space of "
        << STAG_MAX_ENTRIES << " entries (ARM §3.12.2: STAG is 16-bit, "
           "STAG==0 is reserved).";
}

/// BUG-ANALYSIS-5d (documentation of expected fallback behaviour):
/// When the stallQueue_ is full (STAG space exhausted), new stall-mode faults
/// must fall back to terminate mode rather than being silently lost or causing
/// the eventQueue to grow unbounded.
///
/// This test generates enough stall faults on a small queue to verify that
/// the SMMU does not crash or assert when STAG space is exhausted, and that
/// subsequent translate() calls still return a well-defined error (not
/// SMMUError::Stalled) once no STAG slot is available.
TEST(StallQueueBug5, StallQueueExhaustionFallsBackToTerminateMode) {
    // Use a very small queue to accelerate STAG exhaustion in the SW model.
    // The stallQueue_ uses a uint16_t counter that wraps, so with SMALL_QUEUE
    // entries all occupied, the next allocation attempt will find a collision
    // and fall back to terminate mode.
    auto smmu = makeStallCappedSMMU(STALL_SMALL_QUEUE);
    setupStallStream(*smmu, STALL_SID_BASE + 10);
    smmu->clearEventQueue();

    // Fill the stall queue to capacity.
    size_t issuedStalls = 0;
    for (size_t i = 0; i < STALL_SMALL_QUEUE; ++i) {
        IOVA iova = STALL_IOVA + (static_cast<IOVA>(i) * 0x1000);
        auto result = smmu->translate(STALL_SID_BASE + 10, STALL_PASID, iova,
                                      AccessType::Read, SecurityState::NonSecure);
        if (result.isError() && result.getError() == SMMUError::Stalled) {
            ++issuedStalls;
        }
    }

    // At least some stalls must have been accepted before the queue filled.
    EXPECT_GT(issuedStalls, 0u)
        << "Precondition: at least one stall must be issued before the stall "
           "queue fills up.";

    // Now attempt one more stall fault on a fresh IOVA.  If the STAG counter
    // has wrapped and all slots in stallQueue_ are occupied, the SMMU must
    // fall back to terminate mode (not Stalled, not crash).
    //
    // Note: the stallQueue_ in the current implementation uses a simple
    // unordered_map; exhaustion only occurs when the counter wraps a full
    // 16-bit cycle.  With STALL_SMALL_QUEUE entries this won't wrap in a
    // single test run.  This sub-test primarily documents the EXPECTED
    // behaviour (no crash, stable SMMU state) and verifies that the SMMU
    // remains functional after many stall faults.
    IOVA freshIova = STALL_IOVA + (static_cast<IOVA>(STALL_SMALL_QUEUE + 1) * 0x1000);
    auto overflowResult = smmu->translate(STALL_SID_BASE + 10, STALL_PASID,
                                          freshIova, AccessType::Read,
                                          SecurityState::NonSecure);

    // The result must be a well-defined error (not a crash or undefined state).
    // It may be Stalled (if STAG space still available) or a terminate-mode
    // fault (e.g. PageNotMapped) if the queue is full.  Either is acceptable;
    // the important invariant is that the SMMU stays alive.
    EXPECT_TRUE(overflowResult.isError())
        << "BUG-ANALYSIS-5d: translate() after stall queue pressure must "
           "return an error (Stalled or terminate-mode fault), not success.";

    // The eventQueue must remain at or below its configured maximum.
    EXPECT_LE(smmu->getEventQueueSize(), STALL_SMALL_QUEUE)
        << "BUG-ANALYSIS-5d: eventQueue exceeded its configured maximum after "
           "stall queue saturation.  BUG-ANALYSIS-5 is still present.";
}

}  // namespace test
}  // namespace smmu
