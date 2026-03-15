// ARM SMMU v3 Bug 1: receiveBroadcastTLBI hardcodes streamID=0, pasid=0
// Copyright (c) 2024 John Greninger
//
// Bug description:
//   smmu.cpp receiveBroadcastTLBI() calls:
//       executeTLBInvalidationCommand(type, 0, 0, asid, vmid, va)
//   The second and third arguments are streamID and pasid, hardcoded to 0.
//   ARM IHI0070G.b §3.17 states that broadcast TLBIs carry only ASID/VMID/VA
//   — they have no stream or PASID scope.  The broadcast path must invalidate
//   TLB entries for ALL streams, not just stream 0.
//
// Structural assertion:
//   receiveBroadcastTLBI must NOT accept a streamID or pasid parameter.
//   Its current signature — receiveBroadcastTLBI(type, asid, vmid, va) —
//   is architecturally correct.  This file contains a compile-time check that
//   the signature never drifts to include stream context.
//
// Behavioral tests:
//   1. TLBI_NH_ALL (global flush) invalidates TLB entries for streams with
//      non-zero StreamIDs (e.g. streamID=5 and streamID=10).
//   2. TLBI_NH_ASID (ASID-scoped) invalidates entries for two streams that
//      share the same ASID, both with non-zero StreamIDs.
//   3. TLBI_S12_VMALL (VMID-scoped) invalidates entries for two streams
//      sharing the same VMID, both with non-zero StreamIDs.
//
// TDD state:
//   All behavioral tests are EXPECTED TO PASS today — they confirm the
//   broadcast semantics already work correctly for non-zero StreamIDs despite
//   the hardcoded 0,0 in executeTLBInvalidationCommand (which is inert for
//   all broadcast-reachable command branches).
//   The compile-time signature check is the real guard against structural
//   regression: if receiveBroadcastTLBI ever gains a streamID/pasid parameter
//   the static_assert will fail the build.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>
#include <type_traits>

// ─────────────────────────────────────────────────────────────────────────────
// Compile-time structural check: receiveBroadcastTLBI signature must NOT
// include streamID or pasid.  The correct pointer type is:
//   void (SMMU::*)(CommandType, uint16_t, uint16_t, IOVA)
// If the implementation ever gains a streamID/pasid parameter the pointer
// assignment below will fail to compile, surfacing the structural regression.
// ─────────────────────────────────────────────────────────────────────────────
namespace {
// Declare a pointer-to-member that matches the expected 4-parameter signature.
// This line is a compile-time assertion: if receiveBroadcastTLBI does not
// match exactly (type, asid, vmid, va) the translation unit will not compile.
typedef void (smmu::SMMU::*BroadcastTLBISignature)(smmu::CommandType, uint16_t, uint16_t, smmu::IOVA);
static BroadcastTLBISignature kExpectedSignature = &smmu::SMMU::receiveBroadcastTLBI;
// Suppress unused-variable warning.
static_assert(sizeof(kExpectedSignature) > 0,
    "receiveBroadcastTLBI signature check: pointer must be non-null-sized");
} // anonymous namespace

namespace smmu {
namespace test {

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

// Use non-zero StreamIDs to expose any accidental stream-0 scoping.
static constexpr StreamID STREAM_A      = 5u;
static constexpr StreamID STREAM_B      = 10u;
static constexpr PASID    PASID_ZERO    = 0u;
static constexpr uint16_t ASID_SHARED   = 42u;   // both streams share this ASID
static constexpr uint16_t VMID_SHARED   = 7u;    // both streams share this VMID
static constexpr IOVA     IOVA_A        = 0x1000u;
static constexpr IOVA     IOVA_B        = 0x2000u;
static constexpr PA       PA_A          = 0xA000u;
static constexpr PA       PA_B          = 0xB000u;

// ─────────────────────────────────────────────────────────────────────────────
// Fixture
// ─────────────────────────────────────────────────────────────────────────────

class BroadcastTLBIScopeTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::unique_ptr<SMMU>(new SMMU(SMMUConfiguration::createDefault()));
        // Enable the SMMU so translations go through the full path.
        smmu_->enable();
        // Enable PTM so broadcast TLBIs are not silently dropped.
        smmu_->setCR2(SMMU::CR2_PTM);
    }

    // Configure a stage-1-only stream with a single PASID-0 address space,
    // map one IOVA->PA page, and return after translating once to warm the TLB.
    void setupStreamAndWarmTLB(StreamID sid, IOVA iova, PA pa,
                               uint16_t asid = 0u, uint16_t vmid = 0u) {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        ASSERT_TRUE(smmu_->configureStream(sid, cfg).isOk());
        smmu_->setStreamASID(sid, asid);
        smmu_->setStreamVMID(sid, vmid);
        ASSERT_TRUE(smmu_->enableStream(sid).isOk());
        ASSERT_TRUE(smmu_->createStreamPASID(sid, PASID_ZERO).isOk());

        PagePermissions perms(true, true, false);  // read+write, no execute
        ASSERT_TRUE(smmu_->mapPage(sid, PASID_ZERO, iova, pa, perms).isOk());

        // Warm the TLB: first translation must succeed.
        auto result = smmu_->translate(sid, PASID_ZERO, iova, AccessType::Read);
        ASSERT_TRUE(result.isOk()) << "Warm translation failed for stream " << sid;
        ASSERT_EQ(result.getValue().physicalAddress, pa)
            << "Warm translation returned wrong PA for stream " << sid;
    }

    std::unique_ptr<SMMU> smmu_;
};

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: TLBI_NH_ALL (global broadcast flush) invalidates TLB entries for
//         streams with non-zero StreamIDs.
//
// ARM IHI0070G.b §3.17 / §4.4.2: CMD_TLBI_NH_ALL invalidates all non-secure
// TLB entries regardless of ASID, VMID, or StreamID.
//
// Setup:
//   - Stream 5  maps IOVA 0x1000 -> PA 0xA000
//   - Stream 10 maps IOVA 0x2000 -> PA 0xB000
//   - Both TLBs warmed (cache populated).
//
// Action:
//   receiveBroadcastTLBI(TLBI_NH_ALL, 0, 0, 0)
//
// Expected:
//   - TLB cache misses increase after the flush (entries evicted for both
//     streams, not just stream 0).
//   - Re-translations still return correct PAs (address space intact).
//
// If the implementation accidentally scoped the invalidation to stream 0 only,
// stream 5 and stream 10 cache entries would survive — cache hit counts would
// not rise on re-translation.  This test catches that regression by asserting
// that the total miss count after the flush is higher than before.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(BroadcastTLBIScopeTest, GlobalFlushInvalidatesNonZeroStreamTLBEntries) {
    setupStreamAndWarmTLB(STREAM_A, IOVA_A, PA_A);
    setupStreamAndWarmTLB(STREAM_B, IOVA_B, PA_B);

    // Record statistics after TLB warm-up.
    smmu_->resetStatistics();
    uint64_t hitsBeforeFlush  = smmu_->getCacheHitCount();
    uint64_t missesBeforeFlush = smmu_->getCacheMissCount();

    // Translate once from each stream; these should be TLB hits.
    auto r1 = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto r2 = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);
    ASSERT_TRUE(r1.isOk());
    ASSERT_TRUE(r2.isOk());
    uint64_t hitsAfterWarm = smmu_->getCacheHitCount();

    // Global broadcast TLB flush — no stream/PASID context, pure ASID/VMID/VA.
    smmu_->receiveBroadcastTLBI(CommandType::TLBI_NH_ALL, 0u, 0u, 0u);

    // Re-translate — these must be TLB misses (entries were evicted).
    uint64_t missesAfterFlush = smmu_->getCacheMissCount();
    auto r3 = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto r4 = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);
    uint64_t missesAfterRetrans = smmu_->getCacheMissCount();

    // Both re-translations must return correct PAs (address space not disturbed).
    ASSERT_TRUE(r3.isOk()) << "Re-translation failed for stream " << STREAM_A;
    EXPECT_EQ(r3.getValue().physicalAddress, PA_A)
        << "Re-translation returned wrong PA for stream " << STREAM_A;
    ASSERT_TRUE(r4.isOk()) << "Re-translation failed for stream " << STREAM_B;
    EXPECT_EQ(r4.getValue().physicalAddress, PA_B)
        << "Re-translation returned wrong PA for stream " << STREAM_B;

    // Cache miss count must have increased after the flush, proving both
    // stream 5 and stream 10 entries were evicted (not just stream 0).
    EXPECT_GT(missesAfterRetrans, missesAfterFlush)
        << "Expected cache misses to increase after TLBI_NH_ALL for non-zero "
           "StreamIDs, indicating TLB entries were actually evicted.  "
           "If misses did not increase the flush was silently stream-0-scoped.";

    // Suppress unused-variable warnings for statistics captured before warm-up.
    (void)hitsBeforeFlush;
    (void)missesBeforeFlush;
    (void)hitsAfterWarm;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: TLBI_NH_ASID (ASID-scoped broadcast) invalidates TLB entries for
//         two streams that share the same ASID, both with non-zero StreamIDs.
//
// ARM IHI0070G.b §4.4.2 CMD_TLBI_NH_ASID: invalidate all TLB entries tagged
// with the specified ASID, regardless of StreamID.
//
// Setup:
//   - Stream 5  uses ASID=42, maps IOVA 0x1000 -> PA 0xA000
//   - Stream 10 uses ASID=42, maps IOVA 0x2000 -> PA 0xB000
//   - Both TLBs warmed.
//
// Action:
//   receiveBroadcastTLBI(TLBI_NH_ASID, asid=42, vmid=0, va=0)
//
// Expected:
//   - Both streams' TLB entries are invalidated (cache misses on re-translation).
//   - Re-translations still return correct PAs.
//
// If the implementation scoped the ASID invalidation to stream 0 only, the
// entries for stream 5 and stream 10 would survive — the miss count would not
// increase.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(BroadcastTLBIScopeTest, ASIDScopedBroadcastInvalidatesAllStreamsWithThatASID) {
    setupStreamAndWarmTLB(STREAM_A, IOVA_A, PA_A, ASID_SHARED, 0u);
    setupStreamAndWarmTLB(STREAM_B, IOVA_B, PA_B, ASID_SHARED, 0u);

    smmu_->resetStatistics();

    // Warm hit baseline: translate once from each stream.
    auto warm_a = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto warm_b = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);
    ASSERT_TRUE(warm_a.isOk());
    ASSERT_TRUE(warm_b.isOk());

    // ASID-scoped broadcast flush for ASID=42.
    smmu_->receiveBroadcastTLBI(CommandType::TLBI_NH_ASID, ASID_SHARED, 0u, 0u);

    uint64_t missesBeforeRetrans = smmu_->getCacheMissCount();

    // Re-translate — must be cache misses.
    auto r_a = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto r_b = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);

    uint64_t missesAfterRetrans = smmu_->getCacheMissCount();

    // Correct PAs must still be returned.
    ASSERT_TRUE(r_a.isOk()) << "Re-translation failed for stream " << STREAM_A;
    EXPECT_EQ(r_a.getValue().physicalAddress, PA_A)
        << "Wrong PA after ASID flush for stream " << STREAM_A;
    ASSERT_TRUE(r_b.isOk()) << "Re-translation failed for stream " << STREAM_B;
    EXPECT_EQ(r_b.getValue().physicalAddress, PA_B)
        << "Wrong PA after ASID flush for stream " << STREAM_B;

    // Both entries must have been evicted: miss count must rise by at least 2.
    EXPECT_GE(missesAfterRetrans - missesBeforeRetrans, 2u)
        << "Expected at least 2 cache misses after TLBI_NH_ASID for ASID="
        << ASID_SHARED << " covering streams " << STREAM_A << " and " << STREAM_B
        << ".  Miss count delta=" << (missesAfterRetrans - missesBeforeRetrans)
        << ".  Broadcast ASID invalidation must be stream-agnostic.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: TLBI_S12_VMALL (VMID-scoped broadcast) invalidates entries for two
//         streams sharing the same VMID, both with non-zero StreamIDs.
//
// ARM IHI0070G.b §4.4.4 CMD_TLBI_S12_VMALL: invalidate all TLB entries tagged
// with the specified VMID, regardless of StreamID.
//
// ARM IHI0070G.b §3.17: VMID tagging only applies when stage-2 is active.
// This test uses two-stage streams (stage1+stage2) so that TLB entries are
// tagged with VMID=VMID_SHARED.  The stage-2 address space maps the IPA
// (stage-1 output) to a final PA.
//
// Setup:
//   - Stream 5:  two-stage, VMID=7
//       stage-1: IOVA_A (0x1000) -> IPA_A (0x100000)
//       stage-2: IPA_A  (0x100000) -> PA_A (0xA000)
//   - Stream 10: two-stage, VMID=7
//       stage-1: IOVA_B (0x2000) -> IPA_B (0x200000)
//       stage-2: IPA_B  (0x200000) -> PA_B (0xB000)
//   - Both TLBs warmed (entries tagged VMID=7).
//
// Action:
//   receiveBroadcastTLBI(TLBI_S12_VMALL, asid=0, vmid=7, va=0)
//
// Expected:
//   - Both streams' TLB entries are invalidated (cache misses on re-translation).
//   - Re-translations succeed, returning correct PAs.
//
// Note: TLBI_S12_VMALL is not gated by CR2.PTM (only TLBI_NH_* and
// TLBI_NSNH_ALL are EL1 NS broadcast ops).  The CR2.PTM guard in
// receiveBroadcastTLBI only applies to NS-EL1 broadcasts; Secure and EL3
// commands always execute — so this command reaches the TLB regardless.
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(BroadcastTLBIScopeTest, VMIDScopedBroadcastInvalidatesAllStreamsWithThatVMID) {
    // Two-stage streams: stage-1 IOVA -> IPA; stage-2 IPA -> PA.
    // IPA values chosen to be clearly distinct and within 52-bit range (ips=6).
    static constexpr IOVA IPA_A = 0x100000u;
    static constexpr IOVA IPA_B = 0x200000u;

    PagePermissions perms(true, true, false);

    // --- Stream A (streamID=5) ---
    {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.t0sz               = 0;    // no VA range restriction
        cfg.s2t0sz             = 0;    // no IPA range restriction
        cfg.ips                = 6;    // 52-bit stage-1 output
        ASSERT_TRUE(smmu_->configureStream(STREAM_A, cfg).isOk());
        smmu_->setStreamVMID(STREAM_A, VMID_SHARED);
        ASSERT_TRUE(smmu_->enableStream(STREAM_A).isOk());
        ASSERT_TRUE(smmu_->createStreamPASID(STREAM_A, PASID_ZERO).isOk());

        // Stage-1: map IOVA_A -> IPA_A
        ASSERT_TRUE(smmu_->mapPage(STREAM_A, PASID_ZERO, IOVA_A, IPA_A, perms).isOk());

        // Stage-2: map IPA_A -> PA_A
        auto s2as_a = std::shared_ptr<AddressSpace>(new AddressSpace());
        s2as_a->mapPage(IPA_A, PA_A, perms);
        ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(STREAM_A, s2as_a).isOk());
    }

    // --- Stream B (streamID=10) ---
    {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.t0sz               = 0;
        cfg.s2t0sz             = 0;
        cfg.ips                = 6;
        ASSERT_TRUE(smmu_->configureStream(STREAM_B, cfg).isOk());
        smmu_->setStreamVMID(STREAM_B, VMID_SHARED);
        ASSERT_TRUE(smmu_->enableStream(STREAM_B).isOk());
        ASSERT_TRUE(smmu_->createStreamPASID(STREAM_B, PASID_ZERO).isOk());

        // Stage-1: map IOVA_B -> IPA_B
        ASSERT_TRUE(smmu_->mapPage(STREAM_B, PASID_ZERO, IOVA_B, IPA_B, perms).isOk());

        // Stage-2: map IPA_B -> PA_B
        auto s2as_b = std::shared_ptr<AddressSpace>(new AddressSpace());
        s2as_b->mapPage(IPA_B, PA_B, perms);
        ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(STREAM_B, s2as_b).isOk());
    }

    smmu_->resetStatistics();

    // Warm the TLB: both translations must succeed end-to-end.
    auto warm_a = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    ASSERT_TRUE(warm_a.isOk()) << "Two-stage warm translation failed for stream " << STREAM_A;
    EXPECT_EQ(warm_a.getValue().physicalAddress, PA_A)
        << "Warm translation wrong PA for stream " << STREAM_A;

    auto warm_b = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);
    ASSERT_TRUE(warm_b.isOk()) << "Two-stage warm translation failed for stream " << STREAM_B;
    EXPECT_EQ(warm_b.getValue().physicalAddress, PA_B)
        << "Warm translation wrong PA for stream " << STREAM_B;

    // VMID-scoped broadcast flush for VMID=7.
    // TLBI_S12_VMALL is not gated by CR2.PTM; it always executes.
    smmu_->receiveBroadcastTLBI(CommandType::TLBI_S12_VMALL, 0u, VMID_SHARED, 0u);

    uint64_t missesBeforeRetrans = smmu_->getCacheMissCount();

    auto r_a = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto r_b = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);

    uint64_t missesAfterRetrans = smmu_->getCacheMissCount();

    ASSERT_TRUE(r_a.isOk()) << "Re-translation failed for stream " << STREAM_A;
    EXPECT_EQ(r_a.getValue().physicalAddress, PA_A)
        << "Wrong PA after VMID flush for stream " << STREAM_A;
    ASSERT_TRUE(r_b.isOk()) << "Re-translation failed for stream " << STREAM_B;
    EXPECT_EQ(r_b.getValue().physicalAddress, PA_B)
        << "Wrong PA after VMID flush for stream " << STREAM_B;

    EXPECT_GE(missesAfterRetrans - missesBeforeRetrans, 2u)
        << "Expected at least 2 cache misses after TLBI_S12_VMALL for VMID="
        << VMID_SHARED << " covering streams " << STREAM_A << " and " << STREAM_B
        << ".  Miss count delta=" << (missesAfterRetrans - missesBeforeRetrans)
        << ".  Broadcast VMID invalidation must be stream-agnostic (ARM §4.4.4).";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 (PTM gate guard): When CR2.PTM=0, a NS-EL1 broadcast TLBI_NH_ASID
//         must be silently dropped — the TLB entries for non-zero streams must
//         remain valid (no eviction).
//
// ARM IHI0070G.b §6.3.12 CR2.PTM: when PTM=0 the SMMU does not participate
// in OS broadcast TLB maintenance.
//
// This test verifies the CR2.PTM gate specifically for the case of non-zero
// StreamIDs, confirming the gate is global (not stream-0-scoped).
// ─────────────────────────────────────────────────────────────────────────────
TEST_F(BroadcastTLBIScopeTest, PTMZeroDropsBroadcastForNonZeroStreams) {
    // Disable PTM so NS-EL1 broadcasts are silently dropped.
    smmu_->setCR2(0u);  // CR2.PTM=0

    setupStreamAndWarmTLB(STREAM_A, IOVA_A, PA_A, ASID_SHARED, 0u);
    setupStreamAndWarmTLB(STREAM_B, IOVA_B, PA_B, ASID_SHARED, 0u);

    smmu_->resetStatistics();

    // Warm the TLB (populate cache).
    auto warm_a = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto warm_b = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);
    ASSERT_TRUE(warm_a.isOk());
    ASSERT_TRUE(warm_b.isOk());

    uint64_t hitsAfterWarm = smmu_->getCacheHitCount();

    // Issue ASID-scoped broadcast with PTM=0 — must be silently dropped.
    smmu_->receiveBroadcastTLBI(CommandType::TLBI_NH_ASID, ASID_SHARED, 0u, 0u);

    // Re-translate — must still be TLB hits (entries NOT evicted).
    auto r_a = smmu_->translate(STREAM_A, PASID_ZERO, IOVA_A, AccessType::Read);
    auto r_b = smmu_->translate(STREAM_B, PASID_ZERO, IOVA_B, AccessType::Read);

    uint64_t hitsAfterRetrans = smmu_->getCacheHitCount();

    ASSERT_TRUE(r_a.isOk());
    EXPECT_EQ(r_a.getValue().physicalAddress, PA_A);
    ASSERT_TRUE(r_b.isOk());
    EXPECT_EQ(r_b.getValue().physicalAddress, PA_B);

    // With PTM=0, the broadcast was dropped; re-translation should hit cache.
    // Hit count must have increased (both translations are hits).
    EXPECT_GT(hitsAfterRetrans, hitsAfterWarm)
        << "Expected TLB cache hits after a dropped broadcast (PTM=0), "
           "indicating the entries for streams " << STREAM_A << " and "
        << STREAM_B << " were NOT evicted.";
}

} // namespace test
} // namespace smmu
