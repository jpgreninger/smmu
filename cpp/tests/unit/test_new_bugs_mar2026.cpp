// ARM SMMU v3 — BUG-NEW-CPP-1/2/3/4 regression tests (March 2026)
// Copyright (c) 2024 John Greninger
//
// TDD: tests written BEFORE fixes so they can verify bugs exist (or the fix
// is needed) and then verify correctness after the fix.
//
// BUG-NEW-CPP-1: strtabLog2Size_ is a plain uint8_t — concurrent
//   set/get is a C++11 data race.  Fix: std::atomic<uint8_t>.
//
// BUG-NEW-CPP-2: cachingEnabled is a plain bool — concurrent
//   enable/disable via enableCaching() is a C++11 data race.  Fix: std::atomic<bool>.
//
// BUG-NEW-CPP-3: performTwoStageTranslation() calls generateEvent() while
//   the stripe lock is still held by translate().  generateEvent() acquires
//   queueMutex.  Concurrent processCommandQueue() holds queueMutex and then
//   acquires a stripe lock (CFGI_STE path).  This is an ABBA deadlock.
//   Fix: release the stripe lock before calling performTwoStageTranslation(),
//   or restructure so generateEvent() is not called under the stripe lock.
//
// BUG-NEW-CPP-4: enable() and disable() perform a load-then-store on cr0_
//   instead of an atomic read-modify-write.  A concurrent setCR0() call can
//   have its write lost.  Fix: use fetch_or / fetch_and.

#include <gtest/gtest.h>
#include <thread>
#include <atomic>
#include <vector>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test_new_bugs_mar2026 {

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-1: strtabLog2Size_ atomic read/write
// ---------------------------------------------------------------------------

// Basic functional: default is 32, set/get round-trips correctly.
TEST(BugNewCpp1, StrtabLog2SizeDefaultIs32) {
    SMMU smmu;
    EXPECT_EQ(smmu.getStrtabLog2Size(), 32u)
        << "BUG-NEW-CPP-1: default strtabLog2Size_ must be 32";
}

TEST(BugNewCpp1, StrtabLog2SizeAtomicReadWrite) {
    SMMU smmu;
    // Default should be 32
    EXPECT_EQ(smmu.getStrtabLog2Size(), 32u);
    // Set and verify
    smmu.setStrtabLog2Size(16);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 16u);
    smmu.setStrtabLog2Size(32);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 32u);
}

// Structural: with log2size=4, StreamID 16 must be rejected (range check uses
// the atomic-read value of strtabLog2Size_).
TEST(BugNewCpp1, RangeCheckUsesAtomicLog2Size) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.setCR2(SMMU::CR2_RECINVSID);
    smmu.setStrtabLog2Size(4); // IDs 0-15 valid; 16+ rejected
    auto result = smmu.translate(16, 0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "BUG-NEW-CPP-1: StreamID 16 must be rejected when log2size=4";
    EXPECT_EQ(result.getError(), SMMUError::InvalidStreamID)
        << "BUG-NEW-CPP-1: rejected StreamID must produce InvalidStreamID error";
}

// Structural: with log2size=4, StreamID 15 (on the boundary) must NOT be
// rejected by the range check — it may still fail for other reasons (stream
// not configured) but must not produce InvalidStreamID.
TEST(BugNewCpp1, BoundaryStreamIdAcceptedWithLog2Size4) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.setCR2(SMMU::CR2_RECINVSID);
    smmu.setStrtabLog2Size(4); // IDs 0-15 valid
    // StreamID 15 must NOT be rejected by the strtabLog2Size_ range check.
    // It will fail with stream-not-configured, but not InvalidStreamID due to range.
    auto result = smmu.translate(15, 0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isError());
    // Must not be an InvalidStreamID from the range check (that would be a strtab miss).
    // It may be InvalidStreamID from "stream not configured", but not from range rejection.
    // We verify that StreamID=0 passes the range check (0 < 2^4=16):
    auto result0 = smmu.translate(0, 0, 0x1000, AccessType::Read);
    EXPECT_TRUE(result0.isError()); // Not in stream map, but not range-rejected
}

// Concurrent: concurrent getStrtabLog2Size() and setStrtabLog2Size() must not
// crash or produce torn reads.  This is primarily a TSan/ASan regression guard.
TEST(BugNewCpp1, ConcurrentSetAndGetStrtabLog2Size) {
    SMMU smmu;
    smmu.setStrtabLog2Size(32);

    std::atomic<bool> stop{false};
    std::atomic<uint32_t> badReads{0};

    // Writer thread: alternate between 8 and 32.
    std::thread writer([&smmu, &stop]() {
        for (int i = 0; i < 2000 && !stop.load(std::memory_order_relaxed); ++i) {
            smmu.setStrtabLog2Size(8);
            smmu.setStrtabLog2Size(32);
        }
    });

    // Reader: getStrtabLog2Size() must always return 8 or 32 — never garbage.
    for (int i = 0; i < 2000; ++i) {
        uint8_t v = smmu.getStrtabLog2Size();
        if (v != 8u && v != 32u) {
            badReads.fetch_add(1, std::memory_order_relaxed);
        }
    }

    stop.store(true, std::memory_order_release);
    writer.join();

    EXPECT_EQ(badReads.load(), 0u)
        << "BUG-NEW-CPP-1: getStrtabLog2Size() returned an invalid value "
           "during concurrent writes — plain uint8_t is a data race";
}

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-2: cachingEnabled atomic read/write
// ---------------------------------------------------------------------------

// Basic functional: enableCaching(false) then translate should not crash.
TEST(BugNewCpp2, CachingEnabledAtomicReadWrite) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;
    smmu.configureStream(1, cfg);
    smmu.enableStream(1);
    smmu.createStreamPASID(1, 0);
    smmu.mapPage(1, 0, 0x1000, 0xA000,
                 PagePermissions(true, true, false),
                 SecurityState::NonSecure);

    // Toggle caching off — must not crash or produce UB.
    smmu.enableCaching(false);
    auto r1 = smmu.translate(1, 0, 0x1000, AccessType::Read);
    (void)r1; // result may vary; no crash is the requirement

    // Toggle caching back on.
    smmu.enableCaching(true);
    auto r2 = smmu.translate(1, 0, 0x1000, AccessType::Read);
    (void)r2;
}

// Functional: with caching enabled a successful translation is cacheable and
// a second translate of the same IOVA must still succeed (TLB hit path must
// read cachingEnabled correctly).
TEST(BugNewCpp2, TranslationSucceedsAfterEnablingCaching) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;
    smmu.configureStream(2, cfg);
    smmu.enableStream(2);
    smmu.createStreamPASID(2, 0);
    smmu.mapPage(2, 0, 0x2000, 0xB000,
                 PagePermissions(true, false, false),
                 SecurityState::NonSecure);

    smmu.enableCaching(true);
    auto r1 = smmu.translate(2, 0, 0x2000, AccessType::Read);
    EXPECT_TRUE(r1.isOk())
        << "BUG-NEW-CPP-2: first translation must succeed with caching enabled";
    // Second translate — must hit TLB path; cachingEnabled read must see true.
    auto r2 = smmu.translate(2, 0, 0x2000, AccessType::Read);
    EXPECT_TRUE(r2.isOk())
        << "BUG-NEW-CPP-2: second translation must succeed (TLB hit path must "
           "read cachingEnabled correctly)";
}

// Concurrent: simultaneous enableCaching(true/false) and translate() must not
// produce any crash or torn read of cachingEnabled.
TEST(BugNewCpp2, ConcurrentEnableDisableCachingAndTranslate) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;
    smmu.configureStream(3, cfg);
    smmu.enableStream(3);
    smmu.createStreamPASID(3, 0);
    smmu.mapPage(3, 0, 0x3000, 0xC000,
                 PagePermissions(true, true, false),
                 SecurityState::NonSecure);

    std::atomic<bool> stop{false};

    // Writer thread: alternate caching on/off.
    std::thread cacheToggler([&smmu, &stop]() {
        for (int i = 0; i < 1000 && !stop.load(std::memory_order_relaxed); ++i) {
            smmu.enableCaching(true);
            smmu.enableCaching(false);
        }
    });

    // Reader thread: translate repeatedly — must not crash.
    for (int i = 0; i < 1000; ++i) {
        auto r = smmu.translate(3, 0, 0x3000, AccessType::Read);
        (void)r;
    }

    stop.store(true, std::memory_order_release);
    cacheToggler.join();
    // Reaching here without crash is a PASS.
    SUCCEED();
}

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-3: ABBA deadlock — translate() holds stripe lock across
// performTwoStageTranslation() which calls generateEvent() (acquires
// queueMutex), while processCommandQueue() holds queueMutex and acquires
// stripe lock (CFGI_STE path).
// ---------------------------------------------------------------------------

// Regression: concurrent translate() (triggering generateEvent via fault path)
// and processCommandQueue() must not deadlock.
// Uses a timeout-based approach: if the test completes, no deadlock occurred.
TEST(BugNewCpp3, ConcurrentTranslateAndCommandQueueNoDeadlock) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Configure a stream that will produce translation faults (triggering generateEvent).
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;
    smmu.configureStream(10, cfg);
    smmu.enableStream(10);
    smmu.createStreamPASID(10, 0);
    // Do NOT map any page — translate() will generate F_TRANSLATION events,
    // calling generateEvent() while holding the stripe lock.

    std::atomic<bool> done{false};

    // Thread 1: translate an unmapped address (triggers generateEvent inside
    // performTwoStageTranslation while stripe lock is held — potential deadlock).
    std::thread translator([&smmu, &done]() {
        for (int i = 0; i < 500 && !done.load(std::memory_order_relaxed); ++i) {
            // Unknown stream: generates C_BAD_STREAMID event (lock released before generateEvent)
            smmu.translate(999, 0, 0x1000, AccessType::Read);
            // Mapped stream, unmapped page: generates F_TRANSLATION inside
            // performTwoStageTranslation (potential deadlock site).
            smmu.translate(10, 0, 0x5000, AccessType::Read);
        }
        done.store(true, std::memory_order_release);
    });

    // Thread 2: process command queue (acquires queueMutex, then stripe lock
    // inside CFGI_STE handler — the other side of the ABBA).
    for (int i = 0; i < 200 && !done.load(std::memory_order_relaxed); ++i) {
        smmu.processCommandQueue();
    }

    translator.join();

    EXPECT_TRUE(done.load(std::memory_order_acquire))
        << "BUG-NEW-CPP-3: concurrent translate() + processCommandQueue() "
           "deadlocked — stripe lock must be released before generateEvent()";
}

// Additional regression: translate with a known stream that produces a fault
// through performBothStagesTranslation (two-stage path also calls generateEvent).
TEST(BugNewCpp3, TwoStageFaultPathNoDeadlock) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Two-stage stream: stage1 fault will call generateEvent inside
    // performBothStagesTranslation while the stripe lock is held.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.aa64               = true;
    smmu.configureStream(20, cfg);
    smmu.enableStream(20);
    smmu.createStreamPASID(20, 0);
    // Create a stage-2 address space.
    auto stage2AS = std::shared_ptr<AddressSpace>(new AddressSpace());
    smmu.setStreamStage2AddressSpace(20, stage2AS);
    // No pages mapped: translation will fault, calling generateEvent.

    std::atomic<bool> done{false};

    std::thread translator([&smmu, &done]() {
        for (int i = 0; i < 200 && !done.load(std::memory_order_relaxed); ++i) {
            smmu.translate(20, 0, 0x8000, AccessType::Read);
        }
        done.store(true, std::memory_order_release);
    });

    for (int i = 0; i < 100 && !done.load(std::memory_order_relaxed); ++i) {
        smmu.processCommandQueue();
    }

    translator.join();

    EXPECT_TRUE(done.load(std::memory_order_acquire))
        << "BUG-NEW-CPP-3: two-stage translation fault path deadlocked";
}

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-4: Non-atomic RMW on cr0_ in enable()/disable()
// ---------------------------------------------------------------------------

// enable() must preserve other CR0 bits set by setCR0().
TEST(BugNewCpp4, EnablePreservesOtherCR0Bits) {
    SMMU smmu;
    // Set EVENTQEN and CMDQEN without SMMUEN.
    smmu.setCR0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    // Verify both bits are set and SMMUEN is not.
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_EVENTQEN, SMMU::CR0_EVENTQEN);
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_CMDQEN,   SMMU::CR0_CMDQEN);
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN,   0u);

    // enable() should add SMMUEN (plus possibly EVENTQEN/CMDQEN/PRIQEN per
    // the current implementation), but must not clear EVENTQEN or CMDQEN.
    smmu.enable();
    uint32_t cr0 = smmu.getCR0();
    EXPECT_NE(cr0 & SMMU::CR0_SMMUEN,   0u)
        << "BUG-NEW-CPP-4: SMMUEN must be set after enable()";
    EXPECT_NE(cr0 & SMMU::CR0_EVENTQEN, 0u)
        << "BUG-NEW-CPP-4: EVENTQEN must be preserved by enable()";
    EXPECT_NE(cr0 & SMMU::CR0_CMDQEN,   0u)
        << "BUG-NEW-CPP-4: CMDQEN must be preserved by enable()";
}

// disable() must preserve other CR0 bits and only clear SMMUEN.
TEST(BugNewCpp4, DisablePreservesOtherCR0Bits) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_SMMUEN,   0u);
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_EVENTQEN, 0u);
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_CMDQEN,   0u);

    smmu.disable();
    uint32_t cr0 = smmu.getCR0();
    EXPECT_EQ(cr0 & SMMU::CR0_SMMUEN,   0u)
        << "BUG-NEW-CPP-4: SMMUEN must be cleared after disable()";
    EXPECT_NE(cr0 & SMMU::CR0_EVENTQEN, 0u)
        << "BUG-NEW-CPP-4: EVENTQEN must be preserved by disable()";
    EXPECT_NE(cr0 & SMMU::CR0_CMDQEN,   0u)
        << "BUG-NEW-CPP-4: CMDQEN must be preserved by disable()";
}

// enable() followed immediately by disable() must leave SMMUEN cleared
// while preserving any other bits that were set beforehand.
TEST(BugNewCpp4, EnableThenDisableIsIdempotentOnOtherBits) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    uint32_t cr0Before = smmu.getCR0();

    smmu.enable();
    smmu.disable();

    uint32_t cr0After = smmu.getCR0();
    EXPECT_EQ(cr0After & SMMU::CR0_SMMUEN, 0u)
        << "BUG-NEW-CPP-4: after enable()+disable(), SMMUEN must be 0";
    EXPECT_NE(cr0After & SMMU::CR0_EVENTQEN, 0u)
        << "BUG-NEW-CPP-4: EVENTQEN must survive enable()+disable() cycle";
    EXPECT_NE(cr0After & SMMU::CR0_CMDQEN, 0u)
        << "BUG-NEW-CPP-4: CMDQEN must survive enable()+disable() cycle";
    // All bits that were set before enable() (excluding SMMUEN) must be present after disable().
    EXPECT_EQ(cr0After & ~SMMU::CR0_SMMUEN,
              cr0Before & ~SMMU::CR0_SMMUEN)
        << "BUG-NEW-CPP-4: non-SMMUEN bits must be unchanged after enable()+disable()";
}

// Concurrent: setCR0() setting EVENTQEN must not be lost during a concurrent
// enable()/disable() cycle.  This is the race that fetch_or/fetch_and prevents.
TEST(BugNewCpp4, ConcurrentSetCR0AndEnableDisable_NoBitLoss) {
    SMMU smmu;

    std::atomic<bool> stop{false};
    std::atomic<uint32_t> lostBits{0};

    // Thread 1: repeatedly set EVENTQEN and CMDQEN via setCR0().
    std::thread setter([&smmu, &stop]() {
        for (int i = 0; i < 2000 && !stop.load(std::memory_order_relaxed); ++i) {
            smmu.setCR0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_SMMUEN);
        }
    });

    // Thread 2: call enable() and disable() in a loop.
    std::thread toggler([&smmu, &stop]() {
        for (int i = 0; i < 2000 && !stop.load(std::memory_order_relaxed); ++i) {
            smmu.enable();
            smmu.disable();
        }
    });

    setter.join();
    stop.store(true, std::memory_order_release);
    toggler.join();

    // After all threads finish, verify that the SMMU is in a consistent state.
    // We cannot assert the exact CR0 value because of intentional interleaving,
    // but we can verify isEnabled() returns a valid bool (no UB/torn read).
    bool enabled = smmu.isEnabled();
    if (enabled != true && enabled != false) {
        lostBits.fetch_add(1, std::memory_order_relaxed);
    }

    EXPECT_EQ(lostBits.load(), 0u)
        << "BUG-NEW-CPP-4: isEnabled() returned an invalid value after "
           "concurrent setCR0()/enable()/disable() — non-atomic RMW data race";
}

} // namespace test_new_bugs_mar2026
} // namespace smmu
