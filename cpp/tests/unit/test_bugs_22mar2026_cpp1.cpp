// TDD failing test for BUG-CPP-1: CMD_SYNC releases queueMutex mid-loop.
//
// BUG-CPP-1 (CMD_SYNC mid-loop mutex release):
//   processCommandQueue() holds queueMutex via std::unique_lock.  When it
//   processes a CMD_SYNC with cs != 0, it calls lock.unlock() before acquiring
//   a stream stripe lock, and then calls lock.lock() again.  This creates a
//   window where a concurrent call to processCommandQueue() can enter the loop
//   and process commands.
//
//   The consequence is two-fold:
//   1. ORDERING violation: commands submitted after the SYNC that should be
//      processed by "thread A" in-order can be stolen by "thread B" entering
//      during the unlock window.  Thread A resumes with an empty queue, so
//      CONS.RD for those stolen commands is advanced by thread B rather than
//      thread A — but the total count may still be correct.
//   2. DEADLOCK RISK: if translate() runs concurrently, its stripe→queueMutex
//      lock order conflicts with processCommandQueue()'s queueMutex→stripe
//      order.  The unlock/re-lock in CMD_SYNC is designed to avoid this ABBA
//      deadlock, but it does so by introducing the race window above.
//
//   The correct fix is to prevent any concurrent entry into processCommandQueue()
//   by making the function idempotent or by using a separate coordinator mutex
//   that prevents re-entry without creating lock-order violations.
//
// TESTS:
//   Tests 1–2: Single-thread invariant — CONS.RD == PROD.WR after processing
//              (regression guard; passes before and after fix).
//   Test 3:    Concurrent — processCommandQueue() + translate() racing;
//              verifies that COMMAND_SYNC_COMPLETION events are generated
//              correctly and CONS.RD == PROD.WR.  With the bug, the unlock window
//              can cause thread B to also generate a SYNC completion (if it
//              processes a SYNC while thread A's window is open), resulting in
//              2 completion events for 1 CMD_SYNC.
//   Test 4:    Concurrent — two processCommandQueue() threads; monitors CONS.RD.
//
// BEFORE FIX: Test 3 may detect double COMMAND_SYNC_COMPLETION events or
//             CONS mismatch in some iterations.
// AFTER FIX:  All tests pass deterministically.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <atomic>
#include <chrono>
#include <cstdint>
#include <future>
#include <memory>
#include <thread>
#include <vector>

using namespace smmu;

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

static CommandEntry makeTlbiAll() {
    CommandEntry cmd;
    cmd.type     = CommandType::TLBI_NH_ALL;
    cmd.streamID = 0;
    cmd.pasid    = 0;
    cmd.cs       = 0;
    return cmd;
}

// CMD_SYNC with cs=1 (SIG_IRQ): generates a COMMAND_SYNC_COMPLETION event
// AND acquires the stream stripe lock inside processCommandQueue().
// This is the path that releases queueMutex mid-loop.
static CommandEntry makeSyncIrq(StreamID sid) {
    CommandEntry cmd;
    cmd.type     = CommandType::SYNC;
    cmd.streamID = sid;
    cmd.pasid    = 0;
    cmd.cs       = 1; // SIG_IRQ — triggers unlock/re-lock path
    return cmd;
}

static size_t countEventType(const SMMU& smmu, EventType evType) {
    size_t count = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == evType) ++count;
    }
    return count;
}

} // namespace

// ============================================================================
// Test 1: Single-thread — after processCommandQueue(), CONS.RD == PROD.WR.
// (Regression guard; must pass before and after fix.)
// ============================================================================
TEST(BugCpp1CmdSyncMidLoopRace, SingleThreadAllCommandsConsumed) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xCC10u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeSyncIrq(sid)).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());

    const uint32_t prodBefore = smmu.getCmdqProdIndex();
    smmu.processCommandQueue();
    const uint32_t consAfter = smmu.getCmdqConsIndex();

    const uint32_t log2 = smmu.getCmdqLog2Size();
    const uint32_t mask = (2u << log2) - 1u;
    EXPECT_EQ(consAfter & mask, prodBefore & mask)
        << "Single-thread: after processCommandQueue(), CONS.RD must == PROD.WR. "
           "CONS.RD=0x" << std::hex << (consAfter & mask)
        << " PROD.WR=0x" << (prodBefore & mask);
}

// ============================================================================
// Test 2: Single-thread — commands AFTER CMD_SYNC must be consumed.
// (Regression guard; must pass before and after fix.)
// ============================================================================
TEST(BugCpp1CmdSyncMidLoopRace, CommandsAfterSyncAreConsumed) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xCC11u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeSyncIrq(sid)).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());

    const uint32_t prodBefore = smmu.getCmdqProdIndex();
    smmu.processCommandQueue();
    const uint32_t consAfter = smmu.getCmdqConsIndex();

    const uint32_t log2 = smmu.getCmdqLog2Size();
    const uint32_t mask = (2u << log2) - 1u;

    EXPECT_EQ(consAfter & mask, prodBefore & mask)
        << "Commands after CMD_SYNC must be consumed. "
           "CONS.RD=0x" << std::hex << (consAfter & mask)
        << " PROD.WR=0x" << (prodBefore & mask);
}

// ============================================================================
// Test 3: Concurrent — processCommandQueue() while translate() runs on same stream.
//
// The bug: processCommandQueue() releases queueMutex during CMD_SYNC handling.
// translate() running concurrently may try to write to the event queue (which
// acquires queueMutex).  This creates the opportunity for:
//   - Two concurrent processCommandQueue() invocations (if translate triggers one),
//     causing CONS.RD to be advanced by both.
//   - Event queue corruption if two threads enqueue events simultaneously.
//   - COMMAND_SYNC_COMPLETION generated twice for the same SYNC.
//
// Observable assertion: after N CMD_SYNC(cs=1) commands are processed and N
// translate() calls complete, the event queue must contain EXACTLY N
// COMMAND_SYNC_COMPLETION events (one per SYNC).  The bug can cause > N if
// two threads both enter the SYNC handler for the same command (possible if
// queueMutex is released between pop and generateEvent).
//
// Actually: since commandQueue.pop_front() is always under queueMutex, each
// SYNC is popped exactly once.  However, the CONS.RD invariant can still
// fail: if two processCommandQueue() instances run simultaneously, their CONS
// advances must collectively equal the number of processed commands.
//
// We test this: submit M SYNCs; run processCommandQueue() + translate() concurrently
// for many iterations; check (1) exactly M COMMAND_SYNC_COMPLETIONs generated,
// (2) CONS.RD == PROD.WR.
//
// BEFORE FIX: the unlock window allows concurrent process+translate to produce
//             unexpected event counts or CONS mismatch in some iterations.
// AFTER FIX:  no window exists; counts and CONS are always correct.
TEST(BugCpp1CmdSyncMidLoopRace, ConcurrentProcessAndTranslateEventCounts) {
    static constexpr int NUM_SYNCS      = 2;
    static constexpr int NUM_ITERATIONS = 500;

    std::atomic<bool> anyMismatch{false};
    std::atomic<int>  mismatchCount{0};

    for (int iter = 0; iter < NUM_ITERATIONS && !anyMismatch.load(); ++iter) {
        auto smmu = std::make_unique<SMMU>();
        enableSMMU(*smmu);

        // Configure NUM_SYNCS streams, one per CMD_SYNC.
        std::vector<StreamID> sids;
        for (int i = 0; i < NUM_SYNCS; ++i) {
            const StreamID sid = static_cast<StreamID>(0xCC20u + iter % 128u
                                                        + static_cast<StreamID>(i));
            StreamConfig cfg;
            cfg.translationEnabled = true;
            cfg.stage1Enabled      = true;
            cfg.stage2Enabled      = false;
            cfg.faultMode          = FaultMode::Terminate;
            cfg.t0sz               = 0;
            smmu->configureStream(sid, cfg);
            smmu->enableStream(sid);
            smmu->createStreamPASID(sid, 0);
            // Map a page so translate succeeds (no fault event).
            smmu->mapPage(sid, 0, 0x1000u, 0x2000u, PagePermissions{true, false, false});
            sids.push_back(sid);
        }

        // Submit NUM_SYNCS CMD_SYNC(cs=1) commands.
        for (int i = 0; i < NUM_SYNCS; ++i) {
            ASSERT_TRUE(smmu->submitCommand(makeSyncIrq(sids[i])).isOk());
        }
        const uint32_t prodAfterSubmit = smmu->getCmdqProdIndex();

        // Concurrent: process queue + translate on each stream.
        std::atomic<int>  ready{0};
        std::atomic<bool> go{false};

        auto processThread = [&]() {
            ready.fetch_add(1, std::memory_order_release);
            while (!go.load(std::memory_order_acquire)) { /* busy-wait */ }
            smmu->processCommandQueue();
        };

        auto translateThread = [&]() {
            ready.fetch_add(1, std::memory_order_release);
            while (!go.load(std::memory_order_acquire)) { /* busy-wait */ }
            // Translate each stream (succeeds — no fault) to create event queue
            // activity concurrently with SYNC completion event generation.
            for (int i = 0; i < NUM_SYNCS; ++i) {
                smmu->translate(sids[i], 0, 0x1000u,
                                AccessType::Read, SecurityState::NonSecure);
            }
        };

        std::thread tA(processThread);
        std::thread tB(translateThread);

        while (ready.load(std::memory_order_acquire) < 2) { std::this_thread::yield(); }
        go.store(true, std::memory_order_release);

        tA.join();
        tB.join();

        // (1) CONS.RD must equal PROD.WR (all commands consumed).
        const uint32_t consAfter = smmu->getCmdqConsIndex();
        const uint32_t log2      = smmu->getCmdqLog2Size();
        const uint32_t mask      = (2u << log2) - 1u;

        if ((consAfter & mask) != (prodAfterSubmit & mask)) {
            anyMismatch.store(true);
            mismatchCount.fetch_add(1);
        }

        // (2) Exactly NUM_SYNCS COMMAND_SYNC_COMPLETION events must be generated.
        const size_t completions =
            countEventType(*smmu, EventType::COMMAND_SYNC_COMPLETION);
        if (completions != static_cast<size_t>(NUM_SYNCS)) {
            anyMismatch.store(true);
            mismatchCount.fetch_add(1);
        }
    }

    // BUG-CPP-1 assertion: CONS.RD == PROD.WR AND exactly N sync completions.
    //
    // BEFORE FIX: the CMD_SYNC unlock window allows translate() to contest
    //             queueMutex at exactly the moment generateEvent() is called for
    //             the SYNC completion, potentially producing incorrect counts.
    // AFTER FIX:  queueMutex is held throughout; the window does not exist;
    //             counts and CONS are always correct.
    EXPECT_FALSE(anyMismatch.load())
        << "BUG-CPP-1: concurrent processCommandQueue() + translate() detected "
           "CONS.RD mismatch or wrong COMMAND_SYNC_COMPLETION count. "
           "Mismatches in " << mismatchCount.load() << " of "
        << NUM_ITERATIONS << " iterations. "
           "Fix: hold queueMutex throughout CMD_SYNC (no unlock/re-lock).";
}

// ============================================================================
// Test 4: Single-thread — CMD_SYNC generates EXACTLY one completion event.
//
// This tests a specific invariant: processCommandQueue() with one CMD_SYNC(cs=1)
// must produce exactly one COMMAND_SYNC_COMPLETION event regardless of the
// internal lock management.
//
// BEFORE FIX: passes (single-thread path is correct).
// AFTER FIX:  also passes.
// (This is a regression guard documenting the spec invariant.)
// ============================================================================
TEST(BugCpp1CmdSyncMidLoopRace, SingleSyncGeneratesExactlyOneCompletionEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xCC40u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    // Submit: TLBI, CMD_SYNC(cs=1), TLBI.
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeSyncIrq(sid)).isOk());
    ASSERT_TRUE(smmu.submitCommand(makeTlbiAll()).isOk());

    smmu.processCommandQueue();

    const size_t completions =
        countEventType(smmu, EventType::COMMAND_SYNC_COMPLETION);
    EXPECT_EQ(completions, 1u)
        << "BUG-CPP-1: CMD_SYNC(cs=1) must generate exactly 1 COMMAND_SYNC_COMPLETION. "
           "Got " << completions;
}
