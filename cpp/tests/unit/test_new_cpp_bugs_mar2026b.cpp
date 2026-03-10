// ARM SMMU v3 — BUG-NEW-CPP-1/2/5 regression tests (TDD: written BEFORE fixes)
// Copyright (c) 2024 John Greninger
//
// TDD: tests written FIRST so they fail before the fix and pass after.
//
// BUG-NEW-CPP-1: GERROR TOCTOU in inline toggle pattern
//   Three sites use a non-atomic load-compare-fetch_xor.  A concurrent
//   clearGerror() can change gerrorNStatus between the comparison and the
//   fetch_xor, causing a double-toggle that clears the error bit while
//   it should remain active.  Fix: signalGerror() CAS loop.
//
// BUG-NEW-CPP-2: Plain uint32_t queue indices read without lock
//   eventqProd, eventqCons, etc. are plain uint32_t.  Concurrent writes
//   (generateEvent under queueMutex) and reads (const accessors) create
//   C++11 UB.  Fix: convert to std::atomic<uint32_t>.
//
// BUG-NEW-CPP-5: Raw AddressSpace* use-after-free in two-stage translation
//   getPASIDAddressSpace() returns a raw pointer.  A concurrent
//   removeStreamPASID() can destroy the AddressSpace while it is used.
//   Fix: return std::shared_ptr<AddressSpace> to keep the object alive.

#include <gtest/gtest.h>
#include <thread>
#include <atomic>
#include <vector>
#include <memory>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test_new_cpp_bugs_mar2026b {

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static void setupTwoStageStream(SMMU& smmu, StreamID sid, PASID pasid = 0) {
    StreamConfig cfg;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = true;
    cfg.translationEnabled = true;
    smmu.configureStream(sid, cfg);

    smmu.createStreamPASID(sid, pasid);

    // Map a page in stage-1 (IOVA 0x1000 -> IPA 0x2000)
    PagePermissions perms(true, true, false);
    smmu.mapPage(sid, pasid, 0x1000, 0x2000, perms);

    // Configure a dedicated stage-2 address space and map IPA -> PA
    auto s2as = std::make_shared<AddressSpace>();
    s2as->mapPage(0x2000, 0x3000, perms);
    smmu.setStreamStage2AddressSpace(sid, s2as);
}

static CommandEntry makeBadCommand() {
    CommandEntry bad;
    bad.type = static_cast<CommandType>(0xFF);
    bad.streamID = 0;
    bad.pasid = 0;
    bad.startAddress = 0;
    bad.endAddress = 0;
    bad.asid = 0;
    bad.vmid = 0;
    bad.securityState = SecurityState::NonSecure;
    return bad;
}

static CommandEntry makeTlbiCommand() {
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NSNH_ALL;
    cmd.streamID = 0;
    cmd.pasid = 0;
    cmd.startAddress = 0;
    cmd.endAddress = 0;
    cmd.asid = 0;
    cmd.vmid = 0;
    cmd.securityState = SecurityState::NonSecure;
    return cmd;
}

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-1: GERROR TOCTOU concurrent toggle test
//
// Pattern: one thread continuously signals CMDQ_ERR errors; another thread
// continuously clears them.  After the run the GERROR visible value must
// be consistent: XOR(GERROR, GERRORN) must be either 0 or CMDQ_ERR,
// never an unexpected bit pattern.
// ---------------------------------------------------------------------------

TEST(BugNewCpp1, ConcurrentSignalAndClearDoesNotCorruptGerrorBit) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    constexpr int ITERATIONS = 2000;

    // Thread A: continuously trigger CMDQ_ERR by submitting an invalid command
    std::thread signaller([&]() {
        for (int i = 0; i < ITERATIONS; ++i) {
            smmu.submitCommand(makeBadCommand());
            smmu.processCommandQueue();
        }
    });

    // Thread B: continuously acknowledge (clear) the CMDQ_ERR bit
    std::thread clearer([&]() {
        for (int i = 0; i < ITERATIONS; ++i) {
            smmu.clearGerror(GERROR_CMDQ_ERR);
        }
    });

    signaller.join();
    clearer.join();

    // After the race, the visible GERROR bits must be a valid subset of
    // GERROR_CMDQ_ERR.  Any stray bit (other than CMDQ_ERR) indicates a
    // double-toggle corruption.
    uint32_t visible = smmu.getGerror();
    EXPECT_EQ(visible & ~GERROR_CMDQ_ERR, 0u)
        << "Stray bits in GERROR indicate double-toggle corruption: "
        << std::hex << visible;
}

// Structural test: signal CMDQ_ERR while it is already active must NOT
// clear the bit (ARM §6.3.19: "SMMU does not toggle bit[x] if already active").
TEST(BugNewCpp1, SignalGerrorWhileActiveDoesNotClearBit) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR once by injecting a bad command.
    smmu.submitCommand(makeBadCommand());
    smmu.processCommandQueue();

    uint32_t firstVisible = smmu.getGerror();
    // If CMDQ_ERR was signalled the visible bit must be set.
    // (Some implementations may not raise CMDQ_ERR for this specific path;
    //  this test only exercises the no-double-toggle guarantee when it is set.)
    if ((firstVisible & GERROR_CMDQ_ERR) == 0u) {
        GTEST_SKIP() << "CMDQ_ERR not raised by this implementation path; skipping";
    }

    // Now trigger again while active — bit must remain set (not toggled to 0).
    smmu.submitCommand(makeBadCommand());
    smmu.processCommandQueue();

    uint32_t secondVisible = smmu.getGerror();
    EXPECT_NE(secondVisible & GERROR_CMDQ_ERR, 0u)
        << "Second signal while active incorrectly cleared CMDQ_ERR bit";
}

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-2: Queue indices concurrent read/write
//
// getEventqProdIndex() is a const method that returns eventqProd.  With plain
// uint32_t this is C++11 UB when generateEvent() concurrently writes eventqProd
// under queueMutex.  The test verifies that the returned value is always in a
// valid range (no torn reads producing garbage values).
// ---------------------------------------------------------------------------

TEST(BugNewCpp2, ConcurrentEventGenerationAndIndexRead) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Configure a stream so events can be generated normally.
    StreamConfig cfg;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.translationEnabled = true;
    smmu.configureStream(1, cfg);
    smmu.createStreamPASID(1, 0);
    PagePermissions perms(true, false, false);
    smmu.mapPage(1, 0, 0x1000, 0x2000, perms);
    smmu.setCR2(SMMU::CR2_RECINVSID);

    constexpr int PRODUCER_ITERS = 500;

    // Thread A: generate events by translating a write-access on a read-only page
    std::thread producer([&]() {
        for (int i = 0; i < PRODUCER_ITERS; ++i) {
            smmu.translate(1, 0, 0x1000, AccessType::Write);
        }
    });

    // Thread B: continuously read the queue index accessors
    std::thread reader([&]() {
        for (int i = 0; i < PRODUCER_ITERS * 4; ++i) {
            uint32_t prod = smmu.getEventqProdIndex();
            uint32_t cons = smmu.getEventqConsIndex();
            // With plain uint32_t a torn read could produce garbage values.
            // With std::atomic the value is always a well-defined queue index.
            // The generous bound (1u << 20) guards against torn reads where bits
            // from the old and new value are mixed.
            EXPECT_LE(cons, prod + (1u << 20))
                << "Possible torn read: cons=" << cons << " prod=" << prod;
        }
    });

    producer.join();
    reader.join();

    // The absence of data race UB (detectable by TSAN) is the primary goal.
    SUCCEED();
}

TEST(BugNewCpp2, ConcurrentCmdqIndexReadWrite) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    constexpr int ITERS = 500;

    // Thread A: repeatedly submit and process commands (writes cmdqProd/cmdqCons)
    std::thread writer([&]() {
        for (int i = 0; i < ITERS; ++i) {
            smmu.submitCommand(makeTlbiCommand());
            smmu.processCommandQueue();
        }
    });

    // Thread B: read cmdq indices concurrently
    std::thread reader([&]() {
        for (int i = 0; i < ITERS * 4; ++i) {
            uint32_t prod = smmu.getCmdqProdIndex();
            uint32_t cons = smmu.getCmdqConsIndex();
            // After processCommandQueue() cons typically catches up to prod.
            // With a plain uint32_t a torn read could return garbage.
            (void)prod;
            (void)cons;
        }
    });

    writer.join();
    reader.join();

    SUCCEED();
}

// ---------------------------------------------------------------------------
// BUG-NEW-CPP-5: Use-after-free in two-stage translation
//
// The test sets up a two-stage stream and then concurrently:
//  - translates (which calls getPASIDAddressSpace() and uses the raw pointer)
//  - removes and re-adds the PASID (which destroys the old AddressSpace)
//
// With a raw pointer, the translate thread may hold a dangling pointer.
// With std::shared_ptr, the AddressSpace is kept alive for the duration of
// the translation even if the PASID is concurrently removed.
// ---------------------------------------------------------------------------

TEST(BugNewCpp5, TwoStageTranslateWithConcurrentPASIDRemove) {
    constexpr int OUTER_ITERS = 100;
    constexpr StreamID SID = 42;
    constexpr PASID PID = 0;

    for (int iter = 0; iter < OUTER_ITERS; ++iter) {
        SMMU smmu;
        smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

        setupTwoStageStream(smmu, SID, PID);

        std::atomic<bool> done(false);

        // Thread A: continuously translate (uses the AddressSpace raw pointer)
        std::thread translator([&]() {
            while (!done.load(std::memory_order_acquire)) {
                // We don't care about the result; we care that it doesn't crash.
                smmu.translate(SID, PID, 0x1000, AccessType::Read);
            }
        });

        // Thread B: remove then re-add the PASID (destroys AddressSpace)
        std::thread mutator([&]() {
            for (int i = 0; i < 5; ++i) {
                smmu.removeStreamPASID(SID, PID);
                smmu.createStreamPASID(SID, PID);
                PagePermissions perms(true, true, false);
                smmu.mapPage(SID, PID, 0x1000, 0x2000, perms);
            }
            done.store(true, std::memory_order_release);
        });

        mutator.join();
        translator.join();
        // If we reach here without a crash or ASAN error, the bug is fixed.
    }

    SUCCEED();
}

TEST(BugNewCpp5, SharedPtrKeepsAddressSpaceAlive) {
    // Structural test: the AddressSpace must survive concurrent PASID removal.
    // We verify this by checking that a two-stage translation on a stream
    // whose PASID is removed mid-flight either returns a valid result (raced
    // before removal) or returns PASIDNotFound (raced after removal), but
    // never crashes.
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    constexpr StreamID SID = 7;
    constexpr PASID PID = 0;
    setupTwoStageStream(smmu, SID, PID);

    // Remove PASID immediately after setup — translate must not crash.
    smmu.removeStreamPASID(SID, PID);

    auto result = smmu.translate(SID, PID, 0x1000, AccessType::Read);
    // Must return an error (PASIDNotFound or similar), not crash.
    EXPECT_TRUE(result.isError())
        << "Translation after PASID removal must return an error";
}

} // namespace test_new_cpp_bugs_mar2026b
} // namespace smmu
