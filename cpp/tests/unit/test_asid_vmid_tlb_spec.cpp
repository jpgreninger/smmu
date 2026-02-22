// ARM SMMU v3 FINDING-NEW-19 and FINDING-NEW-20:
// VMID-targeted and ASID-targeted TLB invalidation
//
// FINDING-NEW-19: CMD_TLBI_S12_VMALL and CMD_TLBI_S2_IPA should perform
// VMID-targeted TLB invalidation, not stream-wide flush. Currently TLBEntry
// and StreamTableEntry have no vmid field.
//
// FINDING-NEW-20: CMD_TLBI_NH_ASID and CMD_TLBI_EL2_ASID should perform
// ASID-targeted TLB invalidation, not global flush. Currently TLBEntry has
// no asid field.
//
// Spec references:
//   ARM IHI0070G.b §4.3.6 CMD_TLBI_S12_VMALL — Invalidate TLB entries for VMID
//   ARM IHI0070G.b §4.3.8 CMD_TLBI_S2_IPA — Invalidate by IPA for VMID
//   ARM IHI0070G.b §4.3.1 CMD_TLBI_NH_ASID — Invalidate TLB entries for ASID (NH)
//   ARM IHI0070G.b §4.3.3 CMD_TLBI_EL2_ASID — Invalidate TLB entries for ASID (EL2)
//
// TDD "red" state: These tests must FAIL to compile because the following
// fields/methods do not exist yet:
//   - StreamConfig::vmid  (uint16_t)
//   - StreamConfig::asid  (uint16_t)
//   - TLBEntry::vmid      (uint16_t)
//   - TLBEntry::asid      (uint16_t)
//   - CommandEntry::vmid  (uint16_t)
//   - CommandEntry::asid  (uint16_t)
//   - TLBCache::invalidateByASID(uint16_t)
//   - TLBCache::invalidateByVMID(uint16_t)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/tlb_cache.h"
#include <memory>
#include <vector>

namespace smmu {
namespace test {

// ── Constants ────────────────────────────────────────────────────────────────

// Stream 0x10: VMID=1, ASID=10
static constexpr StreamID STREAM_VMID1_ASID10  = 0x10;
// Stream 0x20: VMID=2, ASID=20
static constexpr StreamID STREAM_VMID2_ASID20  = 0x20;
// Stream 0x30: VMID=1, ASID=30 (shares VMID=1 with stream 0x10)
static constexpr StreamID STREAM_VMID1_ASID30  = 0x30;

static constexpr uint16_t VMID_1 = 1;
static constexpr uint16_t VMID_2 = 2;
static constexpr uint16_t ASID_10 = 10;
static constexpr uint16_t ASID_20 = 20;
static constexpr uint16_t ASID_30 = 30;

static constexpr PASID  PASID_ZERO = 0;
static constexpr IOVA   TEST_IOVA  = 0x1000;

// Distinct physical addresses per stream so we can verify correct result
static constexpr PA PA_STREAM_10 = 0xA000;
static constexpr PA PA_STREAM_20 = 0xB000;
static constexpr PA PA_STREAM_30 = 0xC000;

// ── Fixture ──────────────────────────────────────────────────────────────────

class ASIDVMIDTLBSpecTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::unique_ptr<SMMU>(new SMMU(SMMUConfiguration::createDefault()));
        smmu_->enable();

        PagePermissions perms(true, true, false);  // RW, no execute

        // ── Stream 0x10: VMID=1, ASID=10, stage2 enabled ──────────────────
        StreamConfig cfg10;
        cfg10.translationEnabled = true;
        cfg10.stage1Enabled      = true;
        cfg10.stage2Enabled      = true;
        cfg10.faultMode          = FaultMode::Terminate;
        cfg10.vmid               = VMID_1;   // NEW field — triggers compile error
        cfg10.asid               = ASID_10;  // NEW field — triggers compile error
        smmu_->configureStream(STREAM_VMID1_ASID10, cfg10);
        smmu_->enableStream(STREAM_VMID1_ASID10);
        smmu_->createStreamPASID(STREAM_VMID1_ASID10, PASID_ZERO);
        smmu_->mapPage(STREAM_VMID1_ASID10, PASID_ZERO, TEST_IOVA, PA_STREAM_10, perms);

        // ── Stream 0x20: VMID=2, ASID=20, stage2 enabled ──────────────────
        StreamConfig cfg20;
        cfg20.translationEnabled = true;
        cfg20.stage1Enabled      = true;
        cfg20.stage2Enabled      = true;
        cfg20.faultMode          = FaultMode::Terminate;
        cfg20.vmid               = VMID_2;   // NEW field — triggers compile error
        cfg20.asid               = ASID_20;  // NEW field — triggers compile error
        smmu_->configureStream(STREAM_VMID2_ASID20, cfg20);
        smmu_->enableStream(STREAM_VMID2_ASID20);
        smmu_->createStreamPASID(STREAM_VMID2_ASID20, PASID_ZERO);
        smmu_->mapPage(STREAM_VMID2_ASID20, PASID_ZERO, TEST_IOVA, PA_STREAM_20, perms);

        // ── Stream 0x30: VMID=1, ASID=30, stage2 enabled ──────────────────
        StreamConfig cfg30;
        cfg30.translationEnabled = true;
        cfg30.stage1Enabled      = true;
        cfg30.stage2Enabled      = true;
        cfg30.faultMode          = FaultMode::Terminate;
        cfg30.vmid               = VMID_1;   // NEW field — same VMID as stream 0x10
        cfg30.asid               = ASID_30;  // NEW field — triggers compile error
        smmu_->configureStream(STREAM_VMID1_ASID30, cfg30);
        smmu_->enableStream(STREAM_VMID1_ASID30);
        smmu_->createStreamPASID(STREAM_VMID1_ASID30, PASID_ZERO);
        smmu_->mapPage(STREAM_VMID1_ASID30, PASID_ZERO, TEST_IOVA, PA_STREAM_30, perms);

        // Warm TLB for all three streams
        {
            auto r = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO, TEST_IOVA,
                                      AccessType::Read, SecurityState::NonSecure);
            ASSERT_TRUE(r.isOk()) << "SetUp: stream 0x10 initial translation must succeed";
        }
        {
            auto r = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO, TEST_IOVA,
                                      AccessType::Read, SecurityState::NonSecure);
            ASSERT_TRUE(r.isOk()) << "SetUp: stream 0x20 initial translation must succeed";
        }
        {
            auto r = smmu_->translate(STREAM_VMID1_ASID30, PASID_ZERO, TEST_IOVA,
                                      AccessType::Read, SecurityState::NonSecure);
            ASSERT_TRUE(r.isOk()) << "SetUp: stream 0x30 initial translation must succeed";
        }

        smmu_->clearEventQueue();
    }

    void TearDown() override {
        smmu_.reset();
    }

    // Helper: issue CMD_TLBI_S12_VMALL targeted at the given VMID
    void issueTlbiS12Vmall(uint16_t vmid) {
        CommandEntry cmd;
        cmd.type = CommandType::TLBI_S12_VMALL;
        cmd.vmid = vmid;  // NEW field — triggers compile error
        smmu_->submitCommand(cmd);
        smmu_->processCommandQueue();
    }

    // Helper: issue CMD_TLBI_S2_IPA targeted at the given VMID
    void issueTlbiS2Ipa(uint16_t vmid) {
        CommandEntry cmd;
        cmd.type = CommandType::TLBI_S2_IPA;
        cmd.vmid = vmid;  // NEW field — triggers compile error
        smmu_->submitCommand(cmd);
        smmu_->processCommandQueue();
    }

    // Helper: issue CMD_TLBI_NH_ASID targeted at the given ASID
    void issueTlbiNhAsid(uint16_t asid) {
        CommandEntry cmd;
        cmd.type = CommandType::TLBI_NH_ASID;
        cmd.asid = asid;  // NEW field — triggers compile error
        smmu_->submitCommand(cmd);
        smmu_->processCommandQueue();
    }

    // Helper: issue CMD_TLBI_EL2_ASID targeted at the given ASID
    void issueTlbiEl2Asid(uint16_t asid) {
        CommandEntry cmd;
        cmd.type = CommandType::TLBI_EL2_ASID;
        cmd.asid = asid;  // NEW field — triggers compile error
        smmu_->submitCommand(cmd);
        smmu_->processCommandQueue();
    }

    // Helper: populate TLB for all three streams, then unmap all pages.
    // After unmapping, a TLB hit will succeed; a TLB miss forces a page-table
    // walk which returns PageNotMapped.  This lets us definitively distinguish
    // hit from miss without exposing internal cache state.
    void populateTlbThenUnmapAll() {
        // Ensure TLB is warm
        smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO, TEST_IOVA,
                         AccessType::Read, SecurityState::NonSecure);
        smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO, TEST_IOVA,
                         AccessType::Read, SecurityState::NonSecure);
        smmu_->translate(STREAM_VMID1_ASID30, PASID_ZERO, TEST_IOVA,
                         AccessType::Read, SecurityState::NonSecure);
        // Unmap pages from address space so a TLB miss causes a fault
        smmu_->unmapPage(STREAM_VMID1_ASID10, PASID_ZERO, TEST_IOVA);
        smmu_->unmapPage(STREAM_VMID2_ASID20, PASID_ZERO, TEST_IOVA);
        smmu_->unmapPage(STREAM_VMID1_ASID30, PASID_ZERO, TEST_IOVA);
    }

    std::unique_ptr<SMMU> smmu_;
};

// ── FINDING-NEW-19: VMID-targeted invalidation ───────────────────────────────

/// §4.3.6: CMD_TLBI_S12_VMALL with vmid=1 must evict TLB entries for streams
/// with VMID=1 (0x10, 0x30) and must NOT evict entries for stream with VMID=2
/// (0x20).
///
/// Proof method: After populating TLB and unmapping pages, a CMD_TLBI_S12_VMALL
/// targeting VMID=1 invalidates streams 0x10 and 0x30.  Subsequent translation
/// of stream 0x20 succeeds (TLB hit — page is not in address space but the
/// cached PA is served).  Subsequent translations of streams 0x10 and 0x30
/// fault with PageNotMapped (TLB miss — forced page-table walk finds no mapping).
TEST_F(ASIDVMIDTLBSpecTest, VMIDTargetedInvalidation_TLBI_S12_VMALL) {
    populateTlbThenUnmapAll();

    // Invalidate VMID=1 — should evict streams 0x10 and 0x30, not 0x20
    issueTlbiS12Vmall(VMID_1);

    // Stream 0x20 (VMID=2) must still be a TLB hit — translation succeeds
    // even though the page is no longer in the address space
    TranslationResult r20 = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r20.isOk())
        << "FINDING-NEW-19: stream 0x20 (VMID=2) must be a TLB hit after "
           "CMD_TLBI_S12_VMALL targeting VMID=1; page is unmapped so a TLB miss "
           "would fault";

    // Stream 0x10 (VMID=1) must be a TLB miss — page-table walk fails
    TranslationResult r10 = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_FALSE(r10.isOk())
        << "FINDING-NEW-19: stream 0x10 (VMID=1) must be a TLB miss after "
           "CMD_TLBI_S12_VMALL targeting VMID=1; page is unmapped so walk must fault";

    // Stream 0x30 (VMID=1) must also be a TLB miss — page-table walk fails
    TranslationResult r30 = smmu_->translate(STREAM_VMID1_ASID30, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_FALSE(r30.isOk())
        << "FINDING-NEW-19: stream 0x30 (VMID=1) must be a TLB miss after "
           "CMD_TLBI_S12_VMALL targeting VMID=1; page is unmapped so walk must fault";
}

/// §4.3.8: CMD_TLBI_S2_IPA with vmid=2 must evict TLB entries for stream 0x20
/// (VMID=2) and must NOT evict entries for streams 0x10 and 0x30 (VMID=1).
TEST_F(ASIDVMIDTLBSpecTest, VMIDTargetedInvalidation_TLBI_S2_IPA) {
    populateTlbThenUnmapAll();

    // Invalidate VMID=2 — should evict stream 0x20, not 0x10 or 0x30
    issueTlbiS2Ipa(VMID_2);

    // Stream 0x10 (VMID=1) must still be a TLB hit
    TranslationResult r10 = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r10.isOk())
        << "FINDING-NEW-19: stream 0x10 (VMID=1) must be a TLB hit after "
           "CMD_TLBI_S2_IPA targeting VMID=2";

    // Stream 0x30 (VMID=1) must still be a TLB hit
    TranslationResult r30 = smmu_->translate(STREAM_VMID1_ASID30, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r30.isOk())
        << "FINDING-NEW-19: stream 0x30 (VMID=1) must be a TLB hit after "
           "CMD_TLBI_S2_IPA targeting VMID=2";

    // Stream 0x20 (VMID=2) must be a TLB miss — page-table walk fails
    TranslationResult r20 = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_FALSE(r20.isOk())
        << "FINDING-NEW-19: stream 0x20 (VMID=2) must be a TLB miss after "
           "CMD_TLBI_S2_IPA targeting VMID=2; page is unmapped so walk must fault";
}

// ── FINDING-NEW-20: ASID-targeted invalidation ───────────────────────────────

/// §4.3.1: CMD_TLBI_NH_ASID with asid=10 must evict TLB entries for stream 0x10
/// (ASID=10) and must NOT evict entries for streams 0x20 (ASID=20) or 0x30
/// (ASID=30).
TEST_F(ASIDVMIDTLBSpecTest, ASIDTargetedInvalidation_TLBI_NH_ASID) {
    populateTlbThenUnmapAll();

    // Invalidate ASID=10 — should evict stream 0x10 only
    issueTlbiNhAsid(ASID_10);

    // Stream 0x20 (ASID=20) must still be a TLB hit
    TranslationResult r20 = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r20.isOk())
        << "FINDING-NEW-20: stream 0x20 (ASID=20) must be a TLB hit after "
           "CMD_TLBI_NH_ASID targeting ASID=10";

    // Stream 0x30 (ASID=30) must still be a TLB hit
    TranslationResult r30 = smmu_->translate(STREAM_VMID1_ASID30, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r30.isOk())
        << "FINDING-NEW-20: stream 0x30 (ASID=30) must be a TLB hit after "
           "CMD_TLBI_NH_ASID targeting ASID=10";

    // Stream 0x10 (ASID=10) must be a TLB miss — page-table walk fails
    TranslationResult r10 = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_FALSE(r10.isOk())
        << "FINDING-NEW-20: stream 0x10 (ASID=10) must be a TLB miss after "
           "CMD_TLBI_NH_ASID targeting ASID=10; page is unmapped so walk must fault";
}

/// §4.3.3: CMD_TLBI_EL2_ASID with asid=20 must evict TLB entries for stream 0x20
/// (ASID=20) and must NOT evict entries for streams 0x10 (ASID=10) or 0x30
/// (ASID=30).
TEST_F(ASIDVMIDTLBSpecTest, ASIDTargetedInvalidation_TLBI_EL2_ASID) {
    populateTlbThenUnmapAll();

    // Invalidate ASID=20 — should evict stream 0x20 only
    issueTlbiEl2Asid(ASID_20);

    // Stream 0x10 (ASID=10) must still be a TLB hit
    TranslationResult r10 = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r10.isOk())
        << "FINDING-NEW-20: stream 0x10 (ASID=10) must be a TLB hit after "
           "CMD_TLBI_EL2_ASID targeting ASID=20";

    // Stream 0x30 (ASID=30) must still be a TLB hit
    TranslationResult r30 = smmu_->translate(STREAM_VMID1_ASID30, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r30.isOk())
        << "FINDING-NEW-20: stream 0x30 (ASID=30) must be a TLB hit after "
           "CMD_TLBI_EL2_ASID targeting ASID=20";

    // Stream 0x20 (ASID=20) must be a TLB miss — page-table walk fails
    TranslationResult r20 = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_FALSE(r20.isOk())
        << "FINDING-NEW-20: stream 0x20 (ASID=20) must be a TLB miss after "
           "CMD_TLBI_EL2_ASID targeting ASID=20; page is unmapped so walk must fault";
}

// ── Struct field tests — compile-time red state ───────────────────────────────

/// FINDING-NEW-19/20: StreamConfig must expose a vmid field (uint16_t).
TEST_F(ASIDVMIDTLBSpecTest, StreamConfig_HasVMIDField) {
    StreamConfig sc;
    sc.vmid = 42;  // NEW field — triggers compile error before implementation
    EXPECT_EQ(sc.vmid, static_cast<uint16_t>(42))
        << "FINDING-NEW-19: StreamConfig must have uint16_t vmid field";
}

/// FINDING-NEW-20: StreamConfig must expose an asid field (uint16_t).
TEST_F(ASIDVMIDTLBSpecTest, StreamConfig_HasASIDField) {
    StreamConfig sc;
    sc.asid = 99;  // NEW field — triggers compile error before implementation
    EXPECT_EQ(sc.asid, static_cast<uint16_t>(99))
        << "FINDING-NEW-20: StreamConfig must have uint16_t asid field";
}

/// FINDING-NEW-19/20: TLBEntry must expose both asid and vmid fields (uint16_t).
TEST_F(ASIDVMIDTLBSpecTest, TLBEntry_HasASIDAndVMIDFields) {
    TLBEntry entry;
    entry.asid = 5;   // NEW field — triggers compile error before implementation
    entry.vmid = 3;   // NEW field — triggers compile error before implementation
    EXPECT_EQ(entry.asid, static_cast<uint16_t>(5))
        << "FINDING-NEW-20: TLBEntry must have uint16_t asid field";
    EXPECT_EQ(entry.vmid, static_cast<uint16_t>(3))
        << "FINDING-NEW-19: TLBEntry must have uint16_t vmid field";
}

/// FINDING-NEW-19/20: CommandEntry must expose both asid and vmid fields
/// (uint16_t) so that TLBI commands can carry the invalidation target.
TEST_F(ASIDVMIDTLBSpecTest, CommandEntry_HasASIDAndVMIDFields) {
    CommandEntry cmd;
    cmd.asid = 7;   // NEW field — triggers compile error before implementation
    cmd.vmid = 9;   // NEW field — triggers compile error before implementation
    EXPECT_EQ(cmd.asid, static_cast<uint16_t>(7))
        << "FINDING-NEW-20: CommandEntry must have uint16_t asid field";
    EXPECT_EQ(cmd.vmid, static_cast<uint16_t>(9))
        << "FINDING-NEW-19: CommandEntry must have uint16_t vmid field";
}

// ── Global invalidation must still work ──────────────────────────────────────

/// CMD_TLBI_NH_ALL (global) must evict all TLB entries regardless of ASID/VMID.
/// After global invalidation both streams require re-translation; since pages
/// are still mapped the re-translation succeeds.
TEST_F(ASIDVMIDTLBSpecTest, GlobalInvalidation_TLBI_NH_ALL_UnaffectedByASID) {
    // Confirm TLB is warm first
    {
        auto r10 = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO, TEST_IOVA,
                                    AccessType::Read, SecurityState::NonSecure);
        ASSERT_TRUE(r10.isOk()) << "Pre-check: stream 0x10 must translate successfully";
        auto r20 = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO, TEST_IOVA,
                                    AccessType::Read, SecurityState::NonSecure);
        ASSERT_TRUE(r20.isOk()) << "Pre-check: stream 0x20 must translate successfully";
    }

    // Issue global invalidation
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    smmu_->submitCommand(cmd);
    smmu_->processCommandQueue();

    // Pages are still mapped — both streams must translate successfully
    // (re-translation via page-table walk)
    TranslationResult r10 = smmu_->translate(STREAM_VMID1_ASID10, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r10.isOk())
        << "CMD_TLBI_NH_ALL: stream 0x10 must still translate after global "
           "invalidation (page mapping still present)";

    TranslationResult r20 = smmu_->translate(STREAM_VMID2_ASID20, PASID_ZERO,
                                              TEST_IOVA, AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(r20.isOk())
        << "CMD_TLBI_NH_ALL: stream 0x20 must still translate after global "
           "invalidation (page mapping still present)";
}

} // namespace test
} // namespace smmu
