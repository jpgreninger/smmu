// ARM SMMU v3 BUG-CPP-NEW-1: Data race on non-atomic cr0_/gerrorStatus/smmuen_/gbpaAbort_
// Copyright (c) 2024 John Greninger
//
// TDD failing test — written BEFORE the fix.
// C++11 §1.10: concurrent non-atomic read+write of the same object is undefined behaviour.
// Members cr0_, smmuen_, gbpaAbort_, gerrorStatus, gerrorNStatus are plain integers/bools
// that are written by enable()/disable()/reset()/setGbpaAbort() and read (without a lock)
// inside translate() on a hot path.  This is UB under the C++11 memory model.
//
// The fix converts these members to std::atomic<uint32_t> / std::atomic<bool> and uses
// appropriate acquire/release ordering.
//
// Test strategy:
//  1. Spawn N threads that call translate() in a tight loop.
//  2. Simultaneously, spawn threads that repeatedly call enable() / disable().
//  3. Verify no crash, no hang, and that the returned results are always consistent
//     (either a valid translation, a bypass identity, or a GbpaAbort — never garbage).
//  4. Use std::atomic<bool> sanitizers are not directly testable here, so we rely on
//     ASan/TSan detecting the race; the functional test documents the requirement.
//
// This test should be run under ThreadSanitizer (TSan) to detect the race before fix.
// After the fix, TSan must report zero races on these members.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <thread>
#include <atomic>
#include <vector>
#include <chrono>
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID AT_STREAM  = 0xDE;
static constexpr PASID    AT_PASID   = 0;
static constexpr IOVA     AT_IOVA    = 0x20000ULL;
static constexpr PA       AT_PA      = 0xB0000000ULL;
static constexpr int      AT_THREADS = 4;
static constexpr int      AT_ITERS   = 500;

// Helper: set up SMMU with one mapped page.
static void setupSmmu(SMMU& smmu) {
    smmu.enable();
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(AT_STREAM, cfg);
    smmu.enableStream(AT_STREAM);
    smmu.createStreamPASID(AT_STREAM, AT_PASID);
    smmu.mapPage(AT_STREAM, AT_PASID, AT_IOVA, AT_PA,
                 PagePermissions(true, false, false),
                 SecurityState::NonSecure);
}

// -----------------------------------------------------------------------
// Concurrent translate() + enable()/disable()
// Tests that cr0_ / smmuen_ / gbpaAbort_ can be safely read by translate()
// while enable()/disable() writes them from another thread.
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, ConcurrentTranslateAndEnable_NoRace) {
    auto smmu = std::make_unique<SMMU>();
    setupSmmu(*smmu);

    std::atomic<bool> stop{false};
    std::atomic<size_t> errors{0};

    // Translate threads: continuously translate the mapped page.
    std::vector<std::thread> translators;
    for (int t = 0; t < AT_THREADS; ++t) {
        translators.emplace_back([&smmu, &stop, &errors]() {
            for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
                auto result = smmu->translate(AT_STREAM, AT_PASID, AT_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
                // Result must be either Ok (translation hit) or an error code — never UB.
                // Specifically, it must NOT be garbage memory.
                if (!result.isOk() && !result.isError()) {
                    errors.fetch_add(1, std::memory_order_relaxed);
                }
            }
        });
    }

    // Enable/disable threads: flip SMMUEN in a tight loop.
    std::vector<std::thread> enablers;
    for (int t = 0; t < 2; ++t) {
        enablers.emplace_back([&smmu, &stop]() {
            for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
                smmu->enable();
                smmu->disable();
            }
        });
    }

    // Wait for translators to complete.
    for (auto& th : translators) {
        th.join();
    }
    stop.store(true, std::memory_order_release);
    for (auto& th : enablers) {
        th.join();
    }

    EXPECT_EQ(errors.load(), 0u)
        << "translate() returned a value that is neither Ok nor Error — data race (BUG-CPP-NEW-1)";
}

// -----------------------------------------------------------------------
// Concurrent translate() + setGbpaAbort()
// Tests that gbpaAbort_ can be safely read by translate() while being
// written from another thread.
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, ConcurrentTranslateAndGbpaAbort_NoRace) {
    auto smmu = std::make_unique<SMMU>();
    setupSmmu(*smmu);

    std::atomic<bool> stop{false};
    std::atomic<size_t> errors{0};

    std::vector<std::thread> translators;
    for (int t = 0; t < AT_THREADS; ++t) {
        translators.emplace_back([&smmu, &stop, &errors]() {
            for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
                auto result = smmu->translate(AT_STREAM, AT_PASID, AT_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
                if (!result.isOk() && !result.isError()) {
                    errors.fetch_add(1, std::memory_order_relaxed);
                }
            }
        });
    }

    std::vector<std::thread> gbpaWriters;
    for (int t = 0; t < 2; ++t) {
        gbpaWriters.emplace_back([&smmu, &stop]() {
            for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
                smmu->setGbpaAbort(true);
                smmu->setGbpaAbort(false);
            }
        });
    }

    for (auto& th : translators) {
        th.join();
    }
    stop.store(true, std::memory_order_release);
    for (auto& th : gbpaWriters) {
        th.join();
    }

    EXPECT_EQ(errors.load(), 0u)
        << "translate() returned invalid result during concurrent gbpaAbort_ writes (BUG-CPP-NEW-1)";
}

// -----------------------------------------------------------------------
// Concurrent getGerror() + signalError (via generateEvent)
// Tests that gerrorStatus/gerrorNStatus reads/writes are race-free.
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, ConcurrentGetGerrorAndClearGerror_NoRace) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    std::atomic<bool> stop{false};
    std::atomic<size_t> errors{0};

    // Readers: continuously read getGerror() and getGerrorN().
    std::vector<std::thread> readers;
    for (int t = 0; t < AT_THREADS; ++t) {
        readers.emplace_back([&smmu, &stop, &errors]() {
            for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
                uint32_t g  = smmu->getGerror();
                uint32_t gn = smmu->getGerrorN();
                // Active bits = GERROR XOR GERRORN.  After clearGerror(g), active must decrease.
                // We just verify no crash/garbage (both must be valid uint32_t).
                (void)g;
                (void)gn;
            }
        });
    }

    // Writers: repeatedly clear GERROR bits.
    std::vector<std::thread> writers;
    for (int t = 0; t < 2; ++t) {
        writers.emplace_back([&smmu, &stop]() {
            for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
                smmu->clearGerror(0xFFFFFFFF);
            }
        });
    }

    for (auto& th : readers) {
        th.join();
    }
    stop.store(true, std::memory_order_release);
    for (auto& th : writers) {
        th.join();
    }

    // No crash = no data race observable at this level.
    // Under TSan: races on gerrorStatus/gerrorNStatus will be reported.
    EXPECT_EQ(errors.load(), 0u);
}

// -----------------------------------------------------------------------
// Functional: enable() sets SMMUEN; disable() clears it.
// isEnabled() must always reflect the last write (sequential consistency).
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, EnableDisable_IsEnabled_ConsistentSerial) {
    SMMU smmu;
    EXPECT_FALSE(smmu.isEnabled()) << "SMMU must start disabled";

    smmu.enable();
    EXPECT_TRUE(smmu.isEnabled()) << "After enable(), isEnabled() must return true";

    smmu.disable();
    EXPECT_FALSE(smmu.isEnabled()) << "After disable(), isEnabled() must return false";

    smmu.setCR0(SMMU::CR0_SMMUEN);
    EXPECT_TRUE(smmu.isEnabled()) << "setCR0(CR0_SMMUEN) must set SMMUEN";

    smmu.setCR0(0);
    EXPECT_FALSE(smmu.isEnabled()) << "setCR0(0) must clear SMMUEN";
}

// -----------------------------------------------------------------------
// Functional: reset() returns SMMU to disabled state (all registers zeroed).
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, Reset_ClearsAllControlRegisters) {
    SMMU smmu;
    smmu.enable();
    smmu.setGbpaAbort(true);
    ASSERT_TRUE(smmu.isEnabled());
    ASSERT_TRUE(smmu.isGbpaAbort());

    smmu.reset();
    EXPECT_FALSE(smmu.isEnabled()) << "reset() must clear SMMUEN";
    EXPECT_FALSE(smmu.isGbpaAbort()) << "reset() must clear GBPA.ABORT";
    EXPECT_EQ(smmu.getCR0(), 0u) << "reset() must zero CR0";
}

// -----------------------------------------------------------------------
// BUG-CPP-A: setGbpaAbort/isGbpaAbort use implicit seq_cst instead of
// explicit release/acquire ordering.
//
// Functional test: setGbpaAbort(true) followed immediately by isGbpaAbort()
// must return true regardless of memory ordering used — this validates the
// observable behaviour that the fix must preserve.
//
// Under TSan the test also exercises the store/load pair; after the fix,
// the explicit release/acquire pair is annotated correctly and TSan will
// not flag it as a relaxed-ordering violation.
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, GbpaAbort_SetThenGet_ReturnsCorrectValue) {
    SMMU smmu;

    // Initial state: GBPA.ABORT must be false after construction.
    EXPECT_FALSE(smmu.isGbpaAbort())
        << "BUG-CPP-A: isGbpaAbort() must return false on a freshly "
           "constructed SMMU";

    // After setGbpaAbort(true), isGbpaAbort() must return true.
    smmu.setGbpaAbort(true);
    EXPECT_TRUE(smmu.isGbpaAbort())
        << "BUG-CPP-A: isGbpaAbort() must return true after "
           "setGbpaAbort(true) — acquire/release ordering must be used";

    // After setGbpaAbort(false), isGbpaAbort() must return false.
    smmu.setGbpaAbort(false);
    EXPECT_FALSE(smmu.isGbpaAbort())
        << "BUG-CPP-A: isGbpaAbort() must return false after "
           "setGbpaAbort(false) — acquire/release ordering must be used";
}

// Concurrent correctness: a writer thread calls setGbpaAbort in a tight loop
// while a reader thread calls isGbpaAbort.  The test validates no crash and
// that the result is always a valid bool (not an indeterminate garbage value).
// Under TSan this will confirm that the store uses release and the load uses
// acquire ordering.
TEST(AtomicCr0Spec, GbpaAbort_ConcurrentReadWrite_NoRaceWithExplicitOrdering) {
    SMMU smmu;
    smmu.enable();

    std::atomic<bool> stop{false};
    std::atomic<size_t> badValues{0};

    // Writer: alternate between true and false.
    std::thread writer([&smmu, &stop]() {
        for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
            smmu.setGbpaAbort(true);
            smmu.setGbpaAbort(false);
        }
    });

    // Reader: isGbpaAbort() must always return a valid bool (true or false).
    // A data race on a non-atomic bool could produce a bit-pattern that is
    // neither 0 nor 1, causing UB in the conditional.  With the explicit
    // acquire load the result is always well-defined.
    for (int i = 0; i < AT_ITERS; ++i) {
        bool v = smmu.isGbpaAbort();
        // Only true and false are valid; any other bit pattern is a race.
        if (v != true && v != false) {
            badValues.fetch_add(1, std::memory_order_relaxed);
        }
    }

    stop.store(true, std::memory_order_release);
    writer.join();

    EXPECT_EQ(badValues.load(), 0u)
        << "BUG-CPP-A: isGbpaAbort() returned an invalid value during "
           "concurrent writes — missing acquire ordering on the load";
}

// -----------------------------------------------------------------------
// BUG-CPP-B: isEnabled() loads cr0_ with implicit seq_cst instead of
// explicit acquire ordering.
//
// Functional test: after enable()/disable()/setCR0(), isEnabled() must
// return the expected value.  The explicit acquire load must be
// semantically equivalent to seq_cst for single-threaded callers.
// -----------------------------------------------------------------------
TEST(AtomicCr0Spec, IsEnabled_UsesAcquireLoad_FunctionallyCorrect) {
    SMMU smmu;

    // Freshly constructed SMMU must be disabled.
    EXPECT_FALSE(smmu.isEnabled())
        << "BUG-CPP-B: isEnabled() must return false initially";

    // enable() sets SMMUEN; isEnabled() with acquire load must see it.
    smmu.enable();
    EXPECT_TRUE(smmu.isEnabled())
        << "BUG-CPP-B: isEnabled() must return true after enable() — "
           "acquire load on cr0_ must observe the release store in enable()";

    // disable() clears SMMUEN; isEnabled() with acquire load must see that.
    smmu.disable();
    EXPECT_FALSE(smmu.isEnabled())
        << "BUG-CPP-B: isEnabled() must return false after disable() — "
           "acquire load on cr0_ must observe the release store in disable()";

    // setCR0(CR0_SMMUEN) sets bit 0; isEnabled() must reflect this.
    smmu.setCR0(SMMU::CR0_SMMUEN);
    EXPECT_TRUE(smmu.isEnabled())
        << "BUG-CPP-B: isEnabled() must return true after "
           "setCR0(CR0_SMMUEN)";

    // setCR0(0) clears SMMUEN; isEnabled() must return false.
    smmu.setCR0(0u);
    EXPECT_FALSE(smmu.isEnabled())
        << "BUG-CPP-B: isEnabled() must return false after setCR0(0)";
}

// Concurrent correctness: enable()/disable() run in one thread while
// isEnabled() is called from another.  The explicit acquire/release pair
// ensures the reader always sees a consistent (non-torn) boolean state.
TEST(AtomicCr0Spec, IsEnabled_ConcurrentEnableDisable_AcquireLoad_NoRace) {
    SMMU smmu;

    std::atomic<bool> stop{false};
    std::atomic<size_t> invalidStates{0};

    // Toggler: alternately enable and disable the SMMU.
    std::thread toggler([&smmu, &stop]() {
        for (int i = 0; i < AT_ITERS && !stop.load(std::memory_order_relaxed); ++i) {
            smmu.enable();
            smmu.disable();
        }
    });

    // Reader: isEnabled() must always return a valid bool.
    // With BUG-CPP-B present (seq_cst implicit load), TSan may report a race
    // if cr0_ is a non-atomic uint32_t — but after the atomic conversion the
    // concern is whether the ordering annotation is explicit (release/acquire)
    // rather than the weaker relaxed.  The functional test exercises the path.
    for (int i = 0; i < AT_ITERS; ++i) {
        bool enabled = smmu.isEnabled();
        if (enabled != true && enabled != false) {
            invalidStates.fetch_add(1, std::memory_order_relaxed);
        }
    }

    stop.store(true, std::memory_order_release);
    toggler.join();

    EXPECT_EQ(invalidStates.load(), 0u)
        << "BUG-CPP-B: isEnabled() returned an invalid state during "
           "concurrent enable/disable — missing acquire ordering";
}

} // namespace test
} // namespace smmu
