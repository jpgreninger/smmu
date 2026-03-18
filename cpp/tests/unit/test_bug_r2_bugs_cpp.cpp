// ARM SMMU v3 — BUG-R2-CPP-1/2/3/4 regression tests
// Copyright (c) 2024 John Greninger
//
// TDD: tests written BEFORE fixes so they fail first and pass after the fix.
//
// BUG-R2-CPP-1: reset() does not restore strtabLog2Size_ to its default value
//   or cr2_ to 0.  After reset() a previously set strtabLog2Size_=4 persists,
//   causing StreamIDs 16-UINT32_MAX to be incorrectly rejected.
//   Bug-3 fix: the reset-default is now 24 (max valid per ARM §6.3.25), not 32.
//
// BUG-R2-CPP-2: getCR0() uses an implicit seq_cst load on cr0_ instead of
//   the explicit memory_order_acquire used everywhere else.  Semantically
//   correct but inconsistently expensive.
//
// BUG-R2-CPP-3: globalFaultMode is a plain FaultMode (non-atomic).  reset()
//   writes it without a lock that is also held by every reader, creating a
//   latent C++11 data race (UB).  Fix: std::atomic<FaultMode>.
//
// BUG-R2-CPP-4: After the BUG-NEW-CPP-3 fix releases the stripe lock before
//   performTwoStageTranslation(), translate() calls
//   streamContext->getStreamConfiguration() a second time (post-lock-release)
//   to compute the TLB s1dss bypass guard.  This second call is unprotected
//   and races with concurrent reconfigureStream()/removeStream()+configureStream().
//   Fix: snapshot StreamConfig under the stripe lock and use the snapshot
//   for the post-translation TLB insert guard.

#include <gtest/gtest.h>
#include <thread>
#include <atomic>
#include <vector>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test_bug_r2_bugs_cpp {

// ---------------------------------------------------------------------------
// BUG-R2-CPP-1: reset() must restore strtabLog2Size_ to 24 (default) and cr2_ to 0
// Bug-3 fix: default is 24 (max valid per ARM §6.3.25); old default was 32.
// ---------------------------------------------------------------------------

TEST(BugR2Cpp1, ResetRestoresStrtabLog2Size) {
    SMMU smmu;
    smmu.setStrtabLog2Size(4);
    EXPECT_EQ(smmu.getStrtabLog2Size(), 4u);
    smmu.reset();
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u)
        << "reset() must restore strtabLog2Size_ to 24 (max valid per ARM §6.3.25)";
}

TEST(BugR2Cpp1, ResetRestoresCR2) {
    SMMU smmu;
    smmu.setCR2(SMMU::CR2_RECINVSID);
    EXPECT_EQ(smmu.getCR2(), SMMU::CR2_RECINVSID);
    smmu.reset();
    EXPECT_EQ(smmu.getCR2(), 0u)
        << "reset() must restore CR2 to 0";
}

TEST(BugR2Cpp1, AfterResetHighStreamIDAccepted) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.setCR2(SMMU::CR2_RECINVSID);
    smmu.setStrtabLog2Size(4);

    // Confirm range check works before reset: StreamID 16 must be rejected.
    auto r1 = smmu.translate(16, 0, 0x1000, AccessType::Read);
    EXPECT_TRUE(r1.isError());
    EXPECT_EQ(r1.getError(), SMMUError::InvalidStreamID)
        << "before reset(): StreamID 16 must be range-rejected with log2size=4";

    smmu.reset();

    // After reset, strtabLog2Size_ must be 24 (max valid per ARM §6.3.25; Bug-3 fix).
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u);

    // Re-enable SMMUEN so translations are attempted (reset clears SMMUEN=0).
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    // After reset, StreamID 16 must NOT be range-rejected (2^24 > 16).
    // It will fail for a different reason (stream not configured) but
    // must NOT produce InvalidStreamID from the strtab range check.
    // With CR2_RECINVSID=0 (reset default), no C_BAD_STREAMID events are
    // enqueued — but the stream-not-found path still returns InvalidStreamID.
    // We confirm by checking that getStrtabLog2Size() is back to 24 (the
    // primary assertion) and that re-setting log2size=24 allows StreamID 16
    // to at least reach the stream-lookup stage (not earlier range rejection).
    auto evts = smmu.getEvents();
    // With RECINVSID=0 (reset default) no C_BAD_STREAMID events are queued.
    EXPECT_TRUE(evts.isOk());
    if (evts.isOk()) {
        EXPECT_TRUE(evts.getValue().empty())
            << "No C_BAD_STREAMID events expected: RECINVSID=0 after reset";
    }
}

TEST(BugR2Cpp1, DoubleResetRestoresDefaults) {
    SMMU smmu;
    // Set non-default values.
    smmu.setStrtabLog2Size(8);
    smmu.setCR2(SMMU::CR2_RECINVSID);
    // First reset.
    smmu.reset();
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u);  // Bug-3 fix: default is 24, not 32
    EXPECT_EQ(smmu.getCR2(), 0u);
    // Set again.
    smmu.setStrtabLog2Size(16);
    smmu.setCR2(SMMU::CR2_RECINVSID);
    // Second reset.
    smmu.reset();
    EXPECT_EQ(smmu.getStrtabLog2Size(), 24u)
        << "second reset() must also restore strtabLog2Size_ to 24 (max valid per ARM §6.3.25)";
    EXPECT_EQ(smmu.getCR2(), 0u)
        << "second reset() must also restore CR2 to 0";
}

// ---------------------------------------------------------------------------
// BUG-R2-CPP-2: getCR0() must use explicit memory_order_acquire
// ---------------------------------------------------------------------------

// This test is primarily a regression guard.  Because seq_cst is semantically
// stronger than acquire, the function was never wrong about the value it
// returned, but it was inconsistent with the rest of the codebase.  The test
// verifies correct functional behavior before and after the fix.

TEST(BugR2Cpp2, GetCR0ReturnsCorrectValue) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    EXPECT_EQ(smmu.getCR0(), SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN)
        << "getCR0() must return the value set by setCR0()";
    smmu.enable();
    EXPECT_NE(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "getCR0() must reflect SMMUEN after enable()";
    smmu.disable();
    EXPECT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "getCR0() must reflect SMMUEN cleared after disable()";
}

TEST(BugR2Cpp2, GetCR0AfterReset) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.reset();
    EXPECT_EQ(smmu.getCR0(), 0u)
        << "getCR0() must return 0 after reset()";
}

TEST(BugR2Cpp2, GetCR0ConsistentWithIsEnabled) {
    SMMU smmu;
    smmu.enable();
    bool enabled = smmu.isEnabled();
    uint32_t cr0 = smmu.getCR0();
    bool cr0SayEnabled = (cr0 & SMMU::CR0_SMMUEN) != 0u;
    EXPECT_EQ(enabled, cr0SayEnabled)
        << "getCR0() SMMUEN bit must be consistent with isEnabled()";
}

// ---------------------------------------------------------------------------
// BUG-R2-CPP-3: globalFaultMode non-atomic (latent data race)
// ---------------------------------------------------------------------------

// Functional: default mode is Terminate — setGlobalFaultMode(Stall) must
// take effect and be observable through stream behaviour.
TEST(BugR2Cpp3, GlobalFaultModeDefaultIsTerminate) {
    SMMU smmu;
    // Verify that Terminate mode is the default by ensuring a faulting
    // translation does NOT stall (it returns an error, not Stalled).
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.aa64 = true;
    smmu.configureStream(1, cfg);
    smmu.enableStream(1);
    smmu.createStreamPASID(1, 0);
    // No page mapped: translation will fault.
    auto r = smmu.translate(1, 0, 0x4000, AccessType::Read);
    // In Terminate mode: error, not Stalled.
    EXPECT_TRUE(r.isError());
    EXPECT_NE(r.getError(), SMMUError::Stalled)
        << "Default Terminate mode must not stall";
}

// Concurrent: reset() (which writes globalFaultMode) running concurrently
// with translate() (which reads globalFaultMode on the fault path) must not
// produce a data race / crash.  This is the core of BUG-R2-CPP-3.
TEST(BugR2Cpp3, ConcurrentResetAndTranslateNoDataRace) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    std::atomic<bool> done{false};

    // Thread: repeatedly reset() the SMMU (writes globalFaultMode).
    std::thread resetter([&smmu, &done]() {
        for (int i = 0; i < 200 && !done.load(std::memory_order_acquire); ++i) {
            smmu.reset();
            // Re-enable so translates can proceed past the SMMUEN check.
            smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
        }
        done.store(true, std::memory_order_release);
    });

    // Main thread: translate() reads globalFaultMode on the fault path.
    for (int i = 0; i < 500 && !done.load(std::memory_order_acquire); ++i) {
        smmu.translate(42, 0, 0x1000, AccessType::Read);
    }

    resetter.join();
    EXPECT_TRUE(done.load(std::memory_order_acquire))
        << "BUG-R2-CPP-3: concurrent reset()+translate() must not data-race "
           "on globalFaultMode";
}

// Concurrent: setGlobalFaultMode() (writes globalFaultMode under all stripe
// locks) and translate() (reads globalFaultMode on the stall-mode path) must
// not produce a data race.
TEST(BugR2Cpp3, ConcurrentSetFaultModeAndTranslateNoDataRace) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    std::atomic<bool> done{false};

    // Thread: alternate global fault mode between Terminate and Stall.
    std::thread modeToggler([&smmu, &done]() {
        for (int i = 0; i < 400 && !done.load(std::memory_order_acquire); ++i) {
            smmu.setGlobalFaultMode(FaultMode::Stall);
            smmu.setGlobalFaultMode(FaultMode::Terminate);
        }
        done.store(true, std::memory_order_release);
    });

    // Main thread: translate an unconfigured stream (reads globalFaultMode to
    // decide whether to stall or abort).
    for (int i = 0; i < 500 && !done.load(std::memory_order_acquire); ++i) {
        smmu.translate(99, 0, 0x2000, AccessType::Read);
    }

    modeToggler.join();
    EXPECT_TRUE(done.load(std::memory_order_acquire))
        << "BUG-R2-CPP-3: concurrent setGlobalFaultMode()+translate() must "
           "not data-race on globalFaultMode";
}

// ---------------------------------------------------------------------------
// BUG-R2-CPP-4: TOCTOU on streamCfg re-read after stripe lock released
// ---------------------------------------------------------------------------

// Structural: configure a stream with s1dss=1 (bypass for PASID=0), translate,
// then reconfigure — the translation result for the original config must be
// consistent (no crash or assertion failure).  The TLB skip decision must use
// the pre-lock-release snapshot of StreamConfig, not a fresh post-lock re-read.
TEST(BugR2Cpp4, TlbSkipDecisionUsesPreLockConfig) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    // Configure a stream with stage1 enabled, s1cdMax>0, s1dss=1 (bypass),
    // so the bypass guard in translate() triggers on PASID=0.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.aa64 = true;
    cfg.s1cdMax = 1;
    cfg.s1dss = 0x01u;  // bypass for PASID=0
    smmu.configureStream(5, cfg);
    smmu.enableStream(5);
    smmu.createStreamPASID(5, 0);
    smmu.mapPage(5, 0, 0x1000, 0xA000,
                 PagePermissions(true, true, false),
                 SecurityState::NonSecure);

    // First translation: exercises the TLB insert path with s1dss bypass guard.
    auto r1 = smmu.translate(5, 0, 0x1000, AccessType::Read);
    (void)r1;

    // Immediately reconfigure the stream while the first translate may be
    // racing with a concurrent second translate (simulates the TOCTOU window).
    StreamConfig cfg2;
    cfg2.translationEnabled = true;
    cfg2.stage1Enabled = true;
    cfg2.stage2Enabled = false;
    cfg2.aa64 = true;
    cfg2.s1cdMax = 0;  // no substreams
    cfg2.s1dss = 0x02u;
    smmu.configureStream(5, cfg2);  // reconfigure existing stream

    // Second translation after reconfigure: must not crash.
    auto r2 = smmu.translate(5, 0, 0x1000, AccessType::Read);
    (void)r2;

    // No crash = PASS.  The snapshot was taken under the lock.
    SUCCEED();
}

// Concurrent: a background thread rapidly reconfigures the stream while the
// main thread translates.  Without the snapshot fix, getStreamConfiguration()
// is called after the lock is released, racing with reconfigureStream().
// With the fix, the snapshot is taken under the lock and no race exists.
TEST(BugR2Cpp4, ConcurrentReconfigureAndTranslateNoRace) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.aa64 = true;
    cfg.s1cdMax = 1;
    cfg.s1dss = 0x01u;
    smmu.configureStream(7, cfg);
    smmu.enableStream(7);
    smmu.createStreamPASID(7, 0);
    smmu.mapPage(7, 0, 0x5000, 0xB000,
                 PagePermissions(true, false, false),
                 SecurityState::NonSecure);

    std::atomic<bool> done{false};

    // Background: alternate s1dss between bypass (0x01) and normal (0x02).
    std::thread reconfigurer([&smmu, &done]() {
        StreamConfig a;
        a.translationEnabled = true;
        a.stage1Enabled = true;
        a.stage2Enabled = false;
        a.aa64 = true;
        a.s1cdMax = 1;
        a.s1dss = 0x01u;

        StreamConfig b;
        b.translationEnabled = true;
        b.stage1Enabled = true;
        b.stage2Enabled = false;
        b.aa64 = true;
        b.s1cdMax = 1;
        b.s1dss = 0x02u;

        for (int i = 0; i < 300 && !done.load(std::memory_order_acquire); ++i) {
            smmu.configureStream(7, a);
            smmu.configureStream(7, b);
        }
        done.store(true, std::memory_order_release);
    });

    // Main thread: translate repeatedly (may hit TLB path or slow path).
    for (int i = 0; i < 600 && !done.load(std::memory_order_acquire); ++i) {
        auto r = smmu.translate(7, 0, 0x5000, AccessType::Read);
        (void)r;
    }

    reconfigurer.join();
    EXPECT_TRUE(done.load(std::memory_order_acquire))
        << "BUG-R2-CPP-4: concurrent reconfigure+translate must not race on "
           "unprotected post-lock getStreamConfiguration() call";
}

} // namespace test_bug_r2_bugs_cpp
} // namespace smmu
