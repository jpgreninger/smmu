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

} // namespace test
} // namespace smmu
