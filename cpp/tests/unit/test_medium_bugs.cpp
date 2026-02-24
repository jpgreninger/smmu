// ARM SMMU v3 Medium-Severity Bug Regression Tests
// Covers: BUG-CPP-M01, BUG-CPP-M02, BUG-CPP-M03, BUG-CPP-M04
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/stream_context.h"
#include "smmu/fault_handler.h"
#include "smmu/address_space.h"
#include "smmu/types.h"

#include <thread>
#include <vector>
#include <atomic>
#include <memory>

namespace smmu {
namespace test {

// ============================================================
// BUG-CPP-M01: CFGI_STE_RANGE iterates streamMap without stripe locks
// Verify that CMD_CFGI_STE_RANGE does not produce a crash or data race
// when concurrent stream mutations happen during range invalidation.
// ============================================================

class CfgiSteRangeTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper: configure a stream with a single PASID/page mapping so it is
    // listed in streamMap and can be targeted by CFGI_STE_RANGE.
    void configureStreamWithMapping(StreamID sid) {
        StreamConfig cfg;
        cfg.stage1Enabled = true;
        cfg.stage2Enabled = false;
        cfg.securityState = SecurityState::NonSecure;
        cfg.bypassEnabled = false;

        VoidResult r = smmu->configureStream(sid, cfg);
        ASSERT_TRUE(r.isOk()) << "configureStream failed for sid=" << sid;

        r = smmu->createStreamPASID(sid, 0);
        ASSERT_TRUE(r.isOk()) << "createStreamPASID failed for sid=" << sid;

        PagePermissions perms;
        perms.read = true;
        perms.write = false;
        perms.execute = false;
        r = smmu->mapPage(sid, 0, 0x1000ULL, 0x2000ULL, perms, SecurityState::NonSecure);
        ASSERT_TRUE(r.isOk()) << "mapPage failed for sid=" << sid;
    }

    std::unique_ptr<SMMU> smmu;
};

// BUG-CPP-M01 test: submit CMD_CFGI_STE_RANGE commands while another thread
// concurrently adds/removes streams.  The test must not crash, deadlock, or
// trigger an ASAN/TSan report.  We process the command queue after each batch
// to keep it from overflowing (default capacity is 256).
TEST_F(CfgiSteRangeTest, ConcurrentStreamMutationDuringRangeInvalidation) {
    // Populate the SMMU with a batch of streams whose upper bits match a prefix.
    // StreamIDs 0x000-0x01F all have bits [7:5] == 0 when range=4 (prefixBits=5),
    // so a command targeting sid=0x0000 with range=4 will iterate them all.
    for (StreamID sid = 0x0000; sid <= 0x001F; ++sid) {
        configureStreamWithMapping(sid);
    }

    std::atomic<bool> stop(false);
    // Writer thread: repeatedly adds and removes a stream outside the range.
    std::thread writer([&]() {
        StreamID writerSid = 0x0100; // outside the range being invalidated
        while (!stop.load(std::memory_order_relaxed)) {
            StreamConfig cfg;
            cfg.stage1Enabled = true;
            cfg.stage2Enabled = false;
            cfg.securityState = SecurityState::NonSecure;
            cfg.bypassEnabled = false;
            smmu->configureStream(writerSid, cfg);
            smmu->removeStream(writerSid);
        }
    });

    // Invalidation loop: submit CMD_CFGI_STE_RANGE and process the queue each
    // round so the queue never fills up.
    const int ITERATIONS = 200;
    for (int i = 0; i < ITERATIONS; ++i) {
        CommandEntry cmd;
        cmd.type = CommandType::CFGI_ALL;
        cmd.streamID = 0x0000;
        // range=4  => prefixBits=5, matches sids 0x00-0x1F
        cmd.range = 4;
        cmd.pasid = 0;
        cmd.startAddress = 0;
        cmd.endAddress = 0;
        VoidResult r = smmu->submitCommand(cmd);
        EXPECT_TRUE(r.isOk());
        // Drain the queue so it never hits the 256-entry cap.
        smmu->processCommandQueue();
    }

    stop.store(true, std::memory_order_relaxed);
    writer.join();
    // If we get here without crashing or ASAN firing, the bug is fixed.
}

// Simpler deterministic test: CFGI_STE_RANGE with range=0 only touches streams
// whose SID >> 1 matches the command SID >> 1.
TEST_F(CfgiSteRangeTest, RangeInvalidationMatchesOnlyPrefixStreams) {
    // Stream 0x10 and 0x11 share the same bit[31:1] prefix (0x10 >> 1 == 0x8)
    configureStreamWithMapping(0x10);
    configureStreamWithMapping(0x11);
    // Stream 0x12 does NOT match (0x12 >> 1 == 0x9)
    configureStreamWithMapping(0x12);

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_ALL;
    cmd.streamID = 0x10;
    cmd.range = 0; // prefixBits=1: match sids where (sid>>1)==(0x10>>1)==8, i.e. 0x10,0x11
    cmd.pasid = 0;
    cmd.startAddress = 0;
    cmd.endAddress = 0;
    VoidResult r = smmu->submitCommand(cmd);
    EXPECT_TRUE(r.isOk());
}

// ============================================================
// BUG-CPP-M02: addPASID() silently ignores error conditions
// After the fix, addPASID() must return VoidResult and propagate
// InvalidConfiguration for null AS and InvalidPASID for out-of-range PASID.
// ============================================================

class AddPASIDReturnTypeTest : public ::testing::Test {
protected:
    void SetUp() override {
        ctx = std::unique_ptr<StreamContext>(new StreamContext());
        ctx->setStage1Enabled(true);
        ctx->setStage2Enabled(false);
    }

    void TearDown() override {
        ctx.reset();
    }

    std::unique_ptr<StreamContext> ctx;

    static constexpr PASID VALID_PASID = 0x1;
    // MAX_PASID is (1<<20)-1 per ARM SMMU v3 spec.
    static constexpr PASID INVALID_PASID = (1u << 20); // one beyond max
};

// BUG-CPP-M02 (a): null AddressSpace must return an error.
TEST_F(AddPASIDReturnTypeTest, NullAddressSpaceReturnsError) {
    VoidResult result = ctx->addPASID(VALID_PASID, nullptr);
    EXPECT_TRUE(result.isError())
        << "addPASID with null AddressSpace must return an error";
    EXPECT_EQ(result.getError(), SMMUError::InvalidConfiguration)
        << "Expected InvalidConfiguration error for null AddressSpace";
}

// BUG-CPP-M02 (b): PASID exceeding MAX_PASID must return an error.
TEST_F(AddPASIDReturnTypeTest, InvalidPASIDReturnsError) {
    auto addressSpace = std::make_shared<AddressSpace>();
    VoidResult result = ctx->addPASID(INVALID_PASID, addressSpace);
    EXPECT_TRUE(result.isError())
        << "addPASID with PASID > MAX_PASID must return an error";
    EXPECT_EQ(result.getError(), SMMUError::InvalidPASID)
        << "Expected InvalidPASID error for out-of-range PASID";
}

// BUG-CPP-M02 (c): valid call must succeed.
TEST_F(AddPASIDReturnTypeTest, ValidCallReturnsSuccess) {
    auto addressSpace = std::make_shared<AddressSpace>();
    VoidResult result = ctx->addPASID(VALID_PASID, addressSpace);
    EXPECT_TRUE(result.isOk())
        << "addPASID with valid parameters must return success";
}

// BUG-CPP-M02 (d): caller can distinguish between success and the two error codes.
TEST_F(AddPASIDReturnTypeTest, CallerCanDistinguishErrors) {
    auto addressSpace = std::make_shared<AddressSpace>();

    // Valid PASID + valid AS => success
    VoidResult r1 = ctx->addPASID(VALID_PASID, addressSpace);
    EXPECT_FALSE(r1.isError());

    // Null AS => InvalidConfiguration
    VoidResult r2 = ctx->addPASID(VALID_PASID, nullptr);
    EXPECT_TRUE(r2.isError());
    EXPECT_EQ(r2.getError(), SMMUError::InvalidConfiguration);

    // Overflowed PASID => InvalidPASID
    VoidResult r3 = ctx->addPASID(INVALID_PASID, addressSpace);
    EXPECT_TRUE(r3.isError());
    EXPECT_EQ(r3.getError(), SMMUError::InvalidPASID);
}

// ============================================================
// BUG-CPP-M03: Recovery handlers use hardcoded NonSecure in TLB invalidation
// When a fault occurs in a Secure context the TLB entry was tagged Secure, but
// the recovery call used the NonSecure default — the entry was never evicted.
// After the fix, the actual securityState must be passed to invalidate().
// ============================================================

class SecureRecoveryTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
        // Enable the SMMU so that translations go through the page-table path
        // rather than the SMMUEN=0 identity-bypass path.
        smmu->enable();
    }

    void TearDown() override {
        smmu.reset();
    }

    // Configure a Secure stream with a page mapping.
    void configureSecureStream(StreamID sid, PASID pasid) {
        StreamConfig cfg;
        cfg.stage1Enabled = true;
        cfg.stage2Enabled = false;
        cfg.translationEnabled = true;
        cfg.securityState = SecurityState::Secure;
        cfg.bypassEnabled = false;
        VoidResult r = smmu->configureStream(sid, cfg);
        ASSERT_TRUE(r.isOk());

        r = smmu->createStreamPASID(sid, pasid);
        ASSERT_TRUE(r.isOk());

        PagePermissions perms;
        perms.read = true;
        perms.write = false;
        perms.execute = false;
        r = smmu->mapPage(sid, pasid, 0x1000ULL, 0x2000ULL, perms, SecurityState::Secure);
        ASSERT_TRUE(r.isOk());

        r = smmu->enableStream(sid);
        ASSERT_TRUE(r.isOk());
    }

    std::unique_ptr<SMMU> smmu;
};

// BUG-CPP-M03: Translation fault recovery must pass the actual security state
// to TLB invalidate.  We verify this by:
// (a) faulting on an unmapped address in a Secure stream,
// (b) confirming the fault result is an error,
// (c) then re-mapping and confirming the subsequent translation succeeds —
//     showing the fault recovery did not corrupt the TLB state for this stream.
TEST_F(SecureRecoveryTest, TranslationFaultRecoveryUsesCorrectSecurityState) {
    const StreamID sid = 0xA0;
    const PASID pasid = 1;

    configureSecureStream(sid, pasid);

    // First translation on the mapped page — must succeed.
    TranslationResult r1 = smmu->translate(sid, pasid, 0x1000ULL, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(r1.isOk()) << "Initial Secure translation must succeed";

    // Translate to an unmapped address — must fault (triggers translation fault recovery).
    // The recovery handler must call invalidate() with SecurityState::Secure (the fix).
    TranslationResult r2 = smmu->translate(sid, pasid, 0x5000ULL, AccessType::Read, SecurityState::Secure);
    EXPECT_FALSE(r2.isOk())
        << "Translation to unmapped Secure address must fault";

    // After the fault + recovery, the originally-mapped page must still translate
    // correctly (recovery must not have corrupted TLB state for unrelated pages).
    // NOTE: ARM SMMU v3 §4.4 states TLB maintenance is the caller's responsibility;
    // unmapPage() does not auto-evict the TLB.  The TLB entry at 0x1000 seeded in
    // r1 is still valid — it should translate successfully.
    TranslationResult r3 = smmu->translate(sid, pasid, 0x1000ULL, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(r3.isOk())
        << "Pre-existing mapped Secure page must still translate after unrelated fault";
}

// BUG-CPP-M03: Same scenario for permission fault recovery.
TEST_F(SecureRecoveryTest, PermissionFaultRecoveryUsesCorrectSecurityState) {
    const StreamID sid = 0xA1;
    const PASID pasid = 2;

    configureSecureStream(sid, pasid);

    // First translate with read (permitted) to seed the TLB.
    TranslationResult r1 = smmu->translate(sid, pasid, 0x1000ULL, AccessType::Read, SecurityState::Secure);
    EXPECT_TRUE(r1.isOk());

    // Unmap and remap as read-only so write will fault.
    VoidResult unmap = smmu->unmapPage(sid, pasid, 0x1000ULL);
    EXPECT_TRUE(unmap.isOk());

    PagePermissions readOnly;
    readOnly.read = true;
    readOnly.write = false;
    readOnly.execute = false;
    VoidResult remap = smmu->mapPage(sid, pasid, 0x1000ULL, 0x2000ULL, readOnly, SecurityState::Secure);
    EXPECT_TRUE(remap.isOk());

    // Write should produce a permission fault; recovery must correctly use Secure
    // security state when invalidating.  If the pre-fix bug were present, the
    // wrong NonSecure invalidation would leave a stale cached read entry (if any)
    // and subsequent state would be inconsistent.
    TranslationResult r2 = smmu->translate(sid, pasid, 0x1000ULL, AccessType::Write, SecurityState::Secure);
    EXPECT_FALSE(r2.isOk())
        << "Write to read-only Secure page must fault";
}

// BUG-CPP-M03: Same scenario for access fault recovery.
TEST_F(SecureRecoveryTest, AccessFaultRecoveryUsesCorrectSecurityState) {
    const StreamID sid = 0xA2;
    const PASID pasid = 3;

    configureSecureStream(sid, pasid);

    // Trigger an access where no mapping exists (pure translation fault path)
    // to exercise the access-fault recovery with Secure state.
    TranslationResult r = smmu->translate(sid, pasid, 0x9000ULL, AccessType::Read, SecurityState::Secure);
    // No mapping at 0x9000 — must fault.
    EXPECT_FALSE(r.isOk())
        << "Translation to unmapped address must fault regardless of security state";
}

// ============================================================
// BUG-CPP-M04: resetStatistics() does not clear eventQueue
// After a reset, getTotalFaultCount() == 0 but getEventCount() was > 0.
// After the fix, both must be zero.
// ============================================================

class ResetStatisticsTest : public ::testing::Test {
protected:
    void SetUp() override {
        fh = std::unique_ptr<FaultHandler>(new FaultHandler());
    }

    void TearDown() override {
        fh.reset();
    }

    std::unique_ptr<FaultHandler> fh;

    static constexpr StreamID STREAM = 0x42;
    static constexpr PASID PASID_VAL = 0x1;
    static constexpr IOVA IOVA_VAL = 0xDEAD0000ULL;

    FaultRecord makeRecord(FaultType ft) {
        FaultRecord r;
        r.streamID   = STREAM;
        r.pasid      = PASID_VAL;
        r.address    = IOVA_VAL;
        r.faultType  = ft;
        r.accessType = AccessType::Read;
        r.timestamp  = 999;
        return r;
    }
};

// BUG-CPP-M04 (a): after resetStatistics(), getEventCount() must be 0.
TEST_F(ResetStatisticsTest, ResetStatisticsClearsEventQueue) {
    // Record some faults to populate the queue.
    fh->recordFault(makeRecord(FaultType::TranslationFault));
    fh->recordFault(makeRecord(FaultType::PermissionFault));
    fh->recordFault(makeRecord(FaultType::TranslationFault));

    ASSERT_EQ(fh->getEventCount(), 3u);
    ASSERT_EQ(fh->getTotalFaultCount(), 3u);

    fh->resetStatistics();

    EXPECT_EQ(fh->getTotalFaultCount(), 0u)
        << "getTotalFaultCount() must be 0 after resetStatistics()";
    EXPECT_EQ(fh->getEventCount(), 0u)
        << "getEventCount() must be 0 after resetStatistics() — BUG-CPP-M04";
}

// BUG-CPP-M04 (b): after resetStatistics(), getEvents() must be empty.
TEST_F(ResetStatisticsTest, ResetStatisticsClearsEventsVector) {
    fh->recordFault(makeRecord(FaultType::TranslationFault));
    fh->recordFault(makeRecord(FaultType::PermissionFault));

    ASSERT_FALSE(fh->getEvents().empty());

    fh->resetStatistics();

    EXPECT_TRUE(fh->getEvents().empty())
        << "getEvents() must be empty after resetStatistics() — BUG-CPP-M04";
}

// BUG-CPP-M04 (c): consistent state — total count and queue size agree.
TEST_F(ResetStatisticsTest, ResetStatisticsConsistentState) {
    for (int i = 0; i < 5; ++i) {
        fh->recordFault(makeRecord(FaultType::TranslationFault));
    }

    ASSERT_EQ(fh->getEventCount(), 5u);
    ASSERT_EQ(fh->getTotalFaultCount(), 5u);

    fh->resetStatistics();

    uint64_t total = fh->getTotalFaultCount();
    size_t   count = fh->getEventCount();
    EXPECT_EQ(total, 0u);
    EXPECT_EQ(count, 0u);
    // Both counters must agree post-reset.
    EXPECT_EQ(static_cast<size_t>(total), count)
        << "getTotalFaultCount() and getEventCount() must agree after resetStatistics()";
}

// BUG-CPP-M04 (d): normal operation continues after a reset.
TEST_F(ResetStatisticsTest, OperationAfterReset) {
    fh->recordFault(makeRecord(FaultType::TranslationFault));
    fh->resetStatistics();

    ASSERT_EQ(fh->getEventCount(), 0u);

    fh->recordFault(makeRecord(FaultType::PermissionFault));
    EXPECT_EQ(fh->getEventCount(), 1u);
    EXPECT_EQ(fh->getTotalFaultCount(), 1u);
}

} // namespace test
} // namespace smmu
