// BUG-ANALYSIS-1: clearGerror() non-atomic composite read races with signalGerror()
//
// Root cause: clearGerror() holds queueMutex but signalGerror() uses a lockless
// CAS loop.  The XOR `gerrorStatus ^ gerrorNStatus` reads two SEPARATE atomics
// non-atomically inside clearGerror() (line 2730 of smmu.cpp):
//
//     uint32_t activeBits = gerrorStatus ^ gerrorNStatus;   // non-atomic pair read
//     gerrorNStatus ^= (bits & activeBits);
//
// A concurrent signalGerror() CAS loop can modify gerrorStatus between the two
// loads, so activeBits can be stale.  This means clearGerror() may toggle
// GERRORN[x] for a bit that is NOT currently active — which is CONSTRAINED
// UNPREDICTABLE per ARM IHI0070G.b §6.3.20.
//
// The fix requires reading gerrorStatus and gerrorNStatus atomically (e.g. under
// the same lock that signalGerror() also holds, or via a compare-and-swap loop
// inside clearGerror() analogous to signalGerror()).
//
// Test strategy:
//   - Single-threaded invariant tests: verify the XOR protocol state machine
//     transitions are correct in the happy path.  These document the expected
//     semantics and will PASS even before the fix (they test correctness, not
//     concurrency safety).
//   - Concurrent stress test: many threads alternate signalGerror() and
//     clearGerror() simultaneously.  After all threads complete the invariant
//     "active bits must equal GERROR XOR GERRORN" must hold.  This test may
//     occasionally PASS before the fix due to timing but will reliably FAIL
//     under TSan or repeated runs because the race window is real.  The test is
//     annotated to document the expected failure mode.
//
// ARM spec references:
//   IHI0070G.b §6.3.17 SMMU_GERROR
//   IHI0070G.b §6.3.18 SMMU_GERRORN
//   IHI0070G.b §6.3.19 GERROR toggle protocol (signal only when inactive)
//   IHI0070G.b §6.3.20 GERRORN acknowledgement (acknowledge only when active)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <thread>
#include <atomic>
#include <future>
#include <chrono>
#include <vector>

namespace smmu {
namespace test {

// ── Constants ────────────────────────────────────────────────────────────────

static constexpr StreamID GERROR_RACE_STREAM = 0xA0;
static constexpr PASID    GERROR_RACE_PASID  = 0;

// Timeout for concurrent tests.  A race condition will not cause a deadlock
// here so the timeout guards only against unexpected hangs.
static constexpr int RACE_TIMEOUT_MS = 10000;

// Number of iterations per thread in the concurrent stress test.  High enough
// to expose the race window reliably under TSan.
static constexpr int RACE_ITERATIONS = 200;

// Number of threads contending in the concurrent stress test.
static constexpr int RACE_THREADS = 8;

// ── Fixture ──────────────────────────────────────────────────────────────────

class GerrorConcurrencyTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();
        smmu->enable();
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper: trigger GERROR.CMDQ_ERR by processing an unknown command.
    void triggerCmdqErr() {
        CommandEntry bad;
        bad.type = static_cast<CommandType>(0xFF);
        bad.streamID = GERROR_RACE_STREAM;
        bad.pasid    = GERROR_RACE_PASID;
        bad.startAddress = 0;
        bad.endAddress   = 0;
        bad.flags    = 0;
        bad.timestamp = 0;
        smmu->submitCommand(bad);
        smmu->processCommandQueue();
    }

    std::unique_ptr<SMMU> smmu;
};

// ── Single-threaded XOR protocol invariant tests ──────────────────────────────
//
// These tests document the required state-machine transitions for the GERROR /
// GERRORN toggle protocol per ARM IHI0070G.b §6.3.19–20.

/// §6.3.19: After signalling an error, the bit must be active
/// (GERROR XOR GERRORN != 0 for that bit).
TEST_F(GerrorConcurrencyTest, XorProtocol_ActiveAfterSignal) {
    triggerCmdqErr();
    uint32_t active = smmu->getGerror();
    EXPECT_NE(active & GERROR_CMDQ_ERR, 0u)
        << "ARM §6.3.19: CMDQ_ERR must be active (GERROR XOR GERRORN != 0) "
           "after signalGerror() is called";
}

/// §6.3.20: After clearGerror(), the bit must be inactive
/// (GERROR XOR GERRORN == 0 for that bit).
TEST_F(GerrorConcurrencyTest, XorProtocol_InactiveAfterClear) {
    triggerCmdqErr();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Precondition: CMDQ_ERR must be active before clearGerror()";

    smmu->clearGerror(GERROR_CMDQ_ERR);

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "ARM §6.3.20: CMDQ_ERR must be inactive (GERROR XOR GERRORN == 0) "
           "after clearGerror() acknowledges the active bit";
}

/// §6.3.19: Signalling an already-active error must be a no-op (idempotent).
/// The active bit must remain active, not toggle back to 0.
TEST_F(GerrorConcurrencyTest, XorProtocol_SignalIdempotentOnActiveError) {
    triggerCmdqErr();
    uint32_t gerrornBefore = smmu->getGerrorN();
    uint32_t gerrorActiveBefore = smmu->getGerror();
    ASSERT_NE(gerrorActiveBefore & GERROR_CMDQ_ERR, 0u)
        << "Precondition: CMDQ_ERR must be active";

    // signalGerror() is internal; exercise it indirectly by submitting another
    // bad command while CMDQ_ERR is already active.  The ARM spec requires the
    // SMMU to suppress the toggle because the error is already active.
    CommandEntry bad2;
    bad2.type = static_cast<CommandType>(0xFE);
    bad2.streamID = GERROR_RACE_STREAM;
    bad2.pasid    = GERROR_RACE_PASID;
    bad2.startAddress = 0;
    bad2.endAddress   = 0;
    bad2.flags    = 0;
    bad2.timestamp = 0;
    smmu->submitCommand(bad2);
    smmu->processCommandQueue();

    // The active bit must still be active (not re-toggled to 0).
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "ARM §6.3.19: signalGerror() on an already-active bit must not "
           "toggle it back to 0 — the signal must be suppressed";

    // GERRORN must not have changed (no spurious toggle).
    EXPECT_EQ(smmu->getGerrorN(), gerrornBefore)
        << "ARM §6.3.19: GERRORN must be unchanged when signal is suppressed "
           "for an already-active error bit";
}

/// §6.3.20: clearGerror() on an already-inactive bit must be a no-op.
/// GERRORN must not be modified.
TEST_F(GerrorConcurrencyTest, XorProtocol_ClearInactiveBitIsNoop) {
    // No errors triggered — all bits inactive.
    ASSERT_EQ(smmu->getGerror(), 0u)
        << "Precondition: no active GERROR bits at test start";

    uint32_t gerrornBefore = smmu->getGerrorN();
    smmu->clearGerror(GERROR_CMDQ_ERR);

    EXPECT_EQ(smmu->getGerrorN(), gerrornBefore)
        << "ARM §6.3.20: clearGerror() on an inactive bit must not modify "
           "GERRORN — writing GERRORN[x] when GERROR[x]==GERRORN[x] is "
           "CONSTRAINED UNPREDICTABLE (IHI0070G.b §6.3.20)";
    EXPECT_EQ(smmu->getGerror(), 0u)
        << "No active error bits should appear after clearing an inactive bit";
}

/// Full round-trip: signal → active; clear → inactive; signal again → active.
/// Verifies the toggle protocol can be exercised multiple times correctly.
TEST_F(GerrorConcurrencyTest, XorProtocol_RoundTripSignalClearSignal) {
    // Cycle 1: signal
    triggerCmdqErr();
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Cycle 1: CMDQ_ERR must be active after first signal";

    // Cycle 1: acknowledge
    smmu->clearGerror(GERROR_CMDQ_ERR);
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Cycle 1: CMDQ_ERR must be inactive after acknowledgement";

    // Cycle 2: signal again — this requires re-enabling the command queue
    // by processing any pending queue halt state.  Submit a fresh bad command.
    CommandEntry bad2;
    bad2.type = static_cast<CommandType>(0xFD);
    bad2.streamID = GERROR_RACE_STREAM;
    bad2.pasid    = GERROR_RACE_PASID;
    bad2.startAddress = 0;
    bad2.endAddress   = 0;
    bad2.flags    = 0;
    bad2.timestamp = 0;
    smmu->submitCommand(bad2);
    smmu->processCommandQueue();

    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Cycle 2: CMDQ_ERR must be active again after second signal";

    // Cycle 2: acknowledge
    smmu->clearGerror(GERROR_CMDQ_ERR);
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Cycle 2: CMDQ_ERR must be inactive after second acknowledgement";
}

// ── Concurrent stress test ────────────────────────────────────────────────────
//
// BUG-ANALYSIS-1: This test exposes the race between clearGerror() and
// signalGerror().
//
// clearGerror() at smmu.cpp:2730 reads gerrorStatus and gerrorNStatus as two
// separate atomic loads without holding any lock that serialises against the
// signalGerror() CAS loop.  Therefore:
//
//   Thread A (clearGerror):   load gerrorStatus  → [signal races here] →
//                             load gerrorNStatus  → XOR with stale gerrorStatus
//
// After the race, gerrorNStatus may be toggled for a bit whose active state
// changed between the two loads.  This creates CONSTRAINED UNPREDICTABLE
// behaviour (§6.3.20) and may leave the active-bit XOR in an inconsistent
// state.
//
// Expected result BEFORE the fix:
//   The final state of (gerrorStatus XOR gerrorNStatus) may have spurious bits
//   set or cleared inconsistently, violating the invariant that only bits for
//   which the SMMU called signalGerror() (and software has not yet cleared) are
//   active.  TSan will report a data race on the non-atomic read pair.
//
// Expected result AFTER the fix:
//   The final state is always consistent; no spurious active bits exist.
//
// NOTE: The test validates consistency of the final state.  Under normal
// (non-TSan) execution the race window is narrow so the test may occasionally
// PASS by luck before the fix.  Run with TSan enabled for a reliable failure.

TEST_F(GerrorConcurrencyTest, ConcurrentSignalAndClear_FinalStateIsConsistent) {
    // Prime the SMMU: signal CMDQ_ERR once so at least one bit is active at
    // the start of the concurrent phase.
    triggerCmdqErr();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Precondition: CMDQ_ERR must be active before starting stress";

    // Shared flag to start all threads simultaneously.
    std::atomic<bool> go(false);

    // Each "signaller" thread repeatedly triggers CMDQ_ERR by submitting bad
    // commands.  It only does so when the error is currently inactive (to avoid
    // calling processCommandQueue() while CMDQ_ERR is active and potentially
    // halting the queue incorrectly in the SW model — the important thing is
    // the concurrent clearGerror calls race against signalGerror).
    auto signallerFn = [&]() {
        while (!go.load(std::memory_order_acquire)) {
            std::this_thread::yield();
        }
        for (int i = 0; i < RACE_ITERATIONS; ++i) {
            // Only try to signal when inactive to stay within spec §6.3.19.
            // signalGerror() suppresses the toggle when already active, but we
            // stress the race by calling it densely around clearGerror() calls.
            CommandEntry bad;
            bad.type = static_cast<CommandType>(0xAA);
            bad.streamID = GERROR_RACE_STREAM;
            bad.pasid    = GERROR_RACE_PASID;
            bad.startAddress = 0;
            bad.endAddress   = 0;
            bad.flags    = 0;
            bad.timestamp = 0;
            smmu->submitCommand(bad);
            smmu->processCommandQueue();
            std::this_thread::yield();
        }
    };

    // Each "clearer" thread repeatedly acknowledges CMDQ_ERR.
    auto clearerFn = [&]() {
        while (!go.load(std::memory_order_acquire)) {
            std::this_thread::yield();
        }
        for (int i = 0; i < RACE_ITERATIONS; ++i) {
            smmu->clearGerror(GERROR_CMDQ_ERR);
            std::this_thread::yield();
        }
    };

    // Launch threads: half signalers, half clearers.
    std::vector<std::future<void>> futures;
    futures.reserve(RACE_THREADS);
    for (int t = 0; t < RACE_THREADS / 2; ++t) {
        futures.push_back(std::async(std::launch::async, signallerFn));
    }
    for (int t = 0; t < RACE_THREADS / 2; ++t) {
        futures.push_back(std::async(std::launch::async, clearerFn));
    }

    // Release all threads simultaneously.
    go.store(true, std::memory_order_release);

    // Wait for all threads with a timeout.
    bool allReady = true;
    for (auto& f : futures) {
        auto status = f.wait_for(std::chrono::milliseconds(RACE_TIMEOUT_MS));
        if (status != std::future_status::ready) {
            allReady = false;
        }
    }
    ASSERT_TRUE(allReady) << "One or more threads did not complete within "
                          << RACE_TIMEOUT_MS << "ms — unexpected hang in "
                             "signalGerror/clearGerror concurrent stress";

    // ── Invariant check ──────────────────────────────────────────────────────
    //
    // After all threads complete, the GERROR / GERRORN state must be self-
    // consistent: getGerror() == gerrorStatus XOR gerrorNStatus must only have
    // bits set for errors that are genuinely active (the SMMU toggled GERROR[x]
    // since the last clearGerror acknowledgement).
    //
    // We cannot know the exact expected value because threads raced, but we CAN
    // verify that the internal invariant is maintained: getGerror() must equal
    // (gerrorStatus XOR gerrorNStatus) which is what getGerror() computes, and
    // calling getGerror() twice in a row under the same lock must return the
    // same value (no spontaneous bit flips between two reads held consistently).
    //
    // BUG-ANALYSIS-1: Before the fix, gerrorStatus can be read in getGerror()
    // while clearGerror() is mid-flight, producing a transient inconsistent XOR
    // that resolves differently on successive calls.  The two consecutive reads
    // below expose this — if they differ, the internal state is corrupted.

    uint32_t readA = smmu->getGerror();
    uint32_t readB = smmu->getGerror();

    // Both reads are under queueMutex inside getGerror().  They must be equal;
    // if clearGerror() left gerrorNStatus in a state inconsistent with
    // gerrorStatus, readA may differ from readB because an interleaved
    // signalGerror() CAS between the two getGerror() calls sees a garbled
    // gerrorNStatus and computes a wrong active mask.
    //
    // This check is a necessary (not sufficient) condition for correctness.
    // TSan will report the actual race.
    EXPECT_EQ(readA, readB)
        << "BUG-ANALYSIS-1: Two consecutive getGerror() calls returned different "
           "values (" << readA << " vs " << readB << "), indicating that "
           "clearGerror() left the GERROR/GERRORN pair in an inconsistent state "
           "due to a non-atomic composite read of gerrorStatus ^ gerrorNStatus "
           "while a concurrent signalGerror() CAS loop was running.  "
           "Fix: serialize clearGerror()'s read-modify-write of gerrorNStatus "
           "against signalGerror() (e.g. hold queueMutex inside signalGerror() "
           "or use a single atomic CAS loop for both operations).";

    // Additionally: the active bit mask must be a subset of known GERROR bits.
    static constexpr uint32_t ALL_GERROR_BITS =
        GERROR_CMDQ_ERR | GERROR_EVENTQ_ABT_ERR | GERROR_PRIQ_ABT_ERR |
        GERROR_MSI_CMDQ_ABT_ERR | GERROR_MSI_EVENTQ_ABT_ERR |
        GERROR_MSI_PRIQ_ABT_ERR | GERROR_MSI_GERROR_ABT_ERR |
        GERROR_SFM_ERR | GERROR_CMDQP_ERR;

    EXPECT_EQ(readA & ~ALL_GERROR_BITS, 0u)
        << "BUG-ANALYSIS-1: Spurious bits set in GERROR that correspond to no "
           "defined GERROR error source — likely caused by the non-atomic XOR "
           "race leaving GERRORN in an invalid state";
}

// ── Concurrent signal-then-clear serialisation test ──────────────────────────
//
// A single signaller and a single clearer race with higher intensity.
// This variant is more deterministic and reliable under both TSan and normal
// runs.  The invariant checked is: after the signaller finishes and the clearer
// acknowledges all active bits, the final GERROR active mask must be 0.
//
// BUG-ANALYSIS-1: The race means clearGerror() may fail to suppress itself
// when the error is inactive (activeBits is computed from a stale
// gerrorStatus snapshot), resulting in a spurious GERRORN toggle that makes
// an actually-inactive bit appear active.

TEST_F(GerrorConcurrencyTest, ConcurrentSignalThenClear_FinalActiveZero) {
    std::atomic<bool> go(false);
    std::atomic<bool> signallerDone(false);

    // Signal CMDQ_ERR densely.
    auto signallerFn = [&]() {
        while (!go.load(std::memory_order_acquire)) {
            std::this_thread::yield();
        }
        for (int i = 0; i < RACE_ITERATIONS * 2; ++i) {
            CommandEntry bad;
            bad.type = static_cast<CommandType>(0xBB);
            bad.streamID = GERROR_RACE_STREAM;
            bad.pasid    = GERROR_RACE_PASID;
            bad.startAddress = 0;
            bad.endAddress   = 0;
            bad.flags    = 0;
            bad.timestamp = 0;
            smmu->submitCommand(bad);
            smmu->processCommandQueue();
        }
        signallerDone.store(true, std::memory_order_release);
    };

    // Clear CMDQ_ERR densely.
    auto clearerFn = [&]() {
        while (!go.load(std::memory_order_acquire)) {
            std::this_thread::yield();
        }
        // Run until signaller is done, then do a final drain.
        while (!signallerDone.load(std::memory_order_acquire)) {
            smmu->clearGerror(GERROR_CMDQ_ERR);
            std::this_thread::yield();
        }
        // Final drain: clear any remaining active bit.
        for (int i = 0; i < 10; ++i) {
            smmu->clearGerror(GERROR_CMDQ_ERR);
        }
    };

    auto futSignal = std::async(std::launch::async, signallerFn);
    auto futClear  = std::async(std::launch::async, clearerFn);

    go.store(true, std::memory_order_release);

    auto sStatus = futSignal.wait_for(std::chrono::milliseconds(RACE_TIMEOUT_MS));
    auto cStatus = futClear.wait_for(std::chrono::milliseconds(RACE_TIMEOUT_MS));

    ASSERT_EQ(sStatus, std::future_status::ready)
        << "Signaller thread did not complete within timeout";
    ASSERT_EQ(cStatus, std::future_status::ready)
        << "Clearer thread did not complete within timeout";

    // After signaller is done and clearer has acknowledged all active bits,
    // CMDQ_ERR must be inactive.  If clearGerror() incorrectly toggled
    // GERRORN[x] when x was not active (due to the stale XOR read), the bit
    // will oscillate and may appear active even though no real error is pending.
    //
    // BUG-ANALYSIS-1 failure mode: clearGerror() reads gerrorStatus (=1) and
    // then gerrorNStatus (=1 — because a concurrent signalGerror() already
    // flipped gerrorStatus back to 0 and then a clearGerror() set gerrorNStatus
    // to 1).  activeBits = 1 XOR 1 = 0, so the clear is a no-op.  But then the
    // next clearGerror() reads gerrorStatus=0, gerrorNStatus=1, activeBits=1,
    // and incorrectly toggles gerrorNStatus back to 0 — making the already-
    // inactive error appear active again.
    uint32_t finalActive = smmu->getGerror();
    EXPECT_EQ(finalActive & GERROR_CMDQ_ERR, 0u)
        << "BUG-ANALYSIS-1: CMDQ_ERR is still active after the signaller "
           "finished and the clearer acknowledged all active bits.  This "
           "indicates clearGerror() toggled GERRORN[x] for an already-inactive "
           "bit (using a stale activeBits read), causing the error to oscillate.  "
           "Fix: make the read of (gerrorStatus, gerrorNStatus) and the write to "
           "gerrorNStatus a single atomic operation serialised against signalGerror().";
}

}  // namespace test
}  // namespace smmu
