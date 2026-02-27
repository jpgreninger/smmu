// ARM SMMU v3 New Bug Tests: BUG-NEW-01, BUG-NEW-02, BUG-NEW-05
//
// BUG-NEW-01 (§7.4): EVENTQ_PROD.OVFLG unconditionally toggled on every
//   dropped event — must only toggle when transitioning from non-overflow
//   to overflow state (i.e., when OVFLG == OVACKFLG before the toggle).
//
// BUG-NEW-02 (§8.1): PRI queue overflow missing PRIQ_PROD.OVFLG toggle
//   and incorrectly evicts oldest entry instead of rejecting new ones.
//
// BUG-NEW-05 (§7.3): Two-stage Stage-2 fault records use hardcoded PASID=0
//   instead of the originating SubstreamID (PASID) from the request.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>

namespace smmu {
namespace test {

// ── Helpers ──────────────────────────────────────────────────────────────────

// Minimum queue size allowed by QueueConfiguration validation.
static constexpr size_t SMALL_QUEUE = 16;

// Build an SMMU with the minimum-size event and PRI queues.
static std::unique_ptr<SMMU> makeSmallQueueSMMU() {
    QueueConfiguration qcfg(SMALL_QUEUE, SMALL_QUEUE, SMALL_QUEUE);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto s = std::unique_ptr<SMMU>(new SMMU(cfg));
    s->enable();
    // Enable both event queue and PRI queue.
    s->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    return s;
}

// Build a stream config for stage-1 only (Terminate on fault).
static StreamConfig makeStage1Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    return cfg;
}

// Build a stream config for two-stage translation (Terminate on fault).
static StreamConfig makeTwoStageConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate;
    return cfg;
}

// ── BUG-NEW-01: EVENTQ_PROD.OVFLG must only toggle once ─────────────────────
//
// §7.4: OVFLG is toggled only when transitioning from the non-overflow state
// to the overflow state (i.e., when (OVFLG == OVACKFLG) before the toggle).
// Subsequent dropped events must NOT toggle OVFLG again — that would clear it
// and software would no longer see the overflow condition.
//
// The test fills the queue (SMALL_QUEUE entries), records the OVFLG bit value
// (which should change from 0→1 on the first drop), then drops several more
// events and confirms OVFLG remains SET (1), not toggled back to 0.

TEST(BugNew01Spec, EventQueueOverflow_OVFLGOnlyToggledOnFirstOverflow) {
    auto smmu = makeSmallQueueSMMU();

    static constexpr StreamID TEST_SID = 0x30;
    StreamConfig cfg = makeStage1Config();
    ASSERT_TRUE(smmu->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(TEST_SID, 0).isOk());

    // Confirm OVFLG starts at 0.
    uint32_t prodBefore = smmu->getEventqProdIndex();
    bool ovflgBefore = ((prodBefore >> 31) & 1u) != 0;
    ASSERT_FALSE(ovflgBefore) << "OVFLG must be 0 at reset";

    // Fill the event queue by translating SMALL_QUEUE distinct unmapped IOVAs.
    // Each translation to an unmapped address produces a non-stall fault event.
    for (size_t i = 0; i < SMALL_QUEUE; ++i) {
        IOVA badIova = static_cast<IOVA>(0x8000'0000ULL + (static_cast<uint64_t>(i) << 12));
        smmu->translate(TEST_SID, 0, badIova, AccessType::Read, SecurityState::NonSecure);
    }

    // Verify queue is full.
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_QUEUE)
        << "Event queue must be full before testing overflow behavior";

    // Drop a first event — OVFLG should toggle from 0 to 1.
    IOVA overflow1 = 0xC000'0000ULL;
    smmu->translate(TEST_SID, 0, overflow1, AccessType::Read, SecurityState::NonSecure);
    uint32_t prodAfter1 = smmu->getEventqProdIndex();
    bool ovflgAfter1 = ((prodAfter1 >> 31) & 1u) != 0;
    EXPECT_TRUE(ovflgAfter1)
        << "BUG-NEW-01: OVFLG (bit 31) must be SET after the first event queue overflow (§7.4)";

    // Drop a second event — OVFLG must remain SET (not toggled back to 0).
    IOVA overflow2 = 0xC001'0000ULL;
    smmu->translate(TEST_SID, 0, overflow2, AccessType::Read, SecurityState::NonSecure);
    uint32_t prodAfter2 = smmu->getEventqProdIndex();
    bool ovflgAfter2 = ((prodAfter2 >> 31) & 1u) != 0;
    EXPECT_TRUE(ovflgAfter2)
        << "BUG-NEW-01: OVFLG must remain SET after subsequent dropped events — "
           "must NOT be toggled again (§7.4)";

    // Drop a third event — OVFLG must still be SET.
    IOVA overflow3 = 0xC002'0000ULL;
    smmu->translate(TEST_SID, 0, overflow3, AccessType::Read, SecurityState::NonSecure);
    uint32_t prodAfter3 = smmu->getEventqProdIndex();
    bool ovflgAfter3 = ((prodAfter3 >> 31) & 1u) != 0;
    EXPECT_TRUE(ovflgAfter3)
        << "BUG-NEW-01: OVFLG must remain SET after third dropped event (§7.4)";
}

// ── BUG-NEW-02: PRI queue overflow — PRIQ_PROD.OVFLG and no eviction ─────────
//
// §8.1: On PRI queue overflow:
//   (a) Toggle PRIQ_PROD.OVFLG only when transitioning from non-overflow state.
//   (b) While overflow is active, new entries are inhibited — do NOT evict old ones.
//
// The test uses a SMALL_QUEUE (16-entry) PRI queue:
//   1. Submit SMALL_QUEUE requests → queue fills to capacity.
//   2. Submit one more → overflow, OVFLG toggles to 1, queue still has SMALL_QUEUE entries.
//   3. Submit another → OVFLG must stay 1 (not toggled back to 0).

TEST(BugNew02Spec, PRIQueueOverflow_OVFLGSet_OldEntriesPreserved) {
    auto smmu = makeSmallQueueSMMU();

    // Confirm PRIQ_PROD.OVFLG starts at 0.
    uint32_t priqBefore = smmu->getPriqProdIndex();
    bool priqOvflgBefore = ((priqBefore >> 31) & 1u) != 0;
    ASSERT_FALSE(priqOvflgBefore) << "PRIQ_PROD.OVFLG must be 0 at reset";

    // Fill the PRI queue with SMALL_QUEUE entries.
    for (size_t i = 0; i < SMALL_QUEUE; ++i) {
        PRIEntry req;
        req.streamID = static_cast<StreamID>(0x10 + i);
        req.pasid = 0;
        req.requestedAddress = static_cast<IOVA>(0x1000ULL * (i + 1));
        req.accessType = AccessType::Read;
        req.securityState = SecurityState::NonSecure;
        req.timestamp = 0;
        smmu->submitPageRequest(req);
    }

    ASSERT_EQ(smmu->getPRIQueueSize(), SMALL_QUEUE)
        << "PRI queue must be full before overflow test";

    // Submit one more (overflow — first overflow).
    PRIEntry overflow1;
    overflow1.streamID = 0x90;
    overflow1.pasid = 0;
    overflow1.requestedAddress = 0xA000;
    overflow1.accessType = AccessType::Read;
    overflow1.securityState = SecurityState::NonSecure;
    overflow1.timestamp = 0;
    smmu->submitPageRequest(overflow1);

    // Queue must still have SMALL_QUEUE entries (no eviction of old entries).
    EXPECT_EQ(smmu->getPRIQueueSize(), SMALL_QUEUE)
        << "BUG-NEW-02: PRI queue must retain all old entries on overflow — no eviction (§8.1)";

    // PRIQ_PROD.OVFLG must be SET.
    uint32_t priqAfter1 = smmu->getPriqProdIndex();
    bool priqOvflgAfter1 = ((priqAfter1 >> 31) & 1u) != 0;
    EXPECT_TRUE(priqOvflgAfter1)
        << "BUG-NEW-02: PRIQ_PROD.OVFLG (bit 31) must be SET after first PRI queue overflow (§8.1)";

    // Submit yet another (overflow still active — OVFLG must remain SET, not toggled back).
    PRIEntry overflow2;
    overflow2.streamID = 0x91;
    overflow2.pasid = 0;
    overflow2.requestedAddress = 0xB000;
    overflow2.accessType = AccessType::Read;
    overflow2.securityState = SecurityState::NonSecure;
    overflow2.timestamp = 0;
    smmu->submitPageRequest(overflow2);

    EXPECT_EQ(smmu->getPRIQueueSize(), SMALL_QUEUE)
        << "BUG-NEW-02: PRI queue must still have SMALL_QUEUE entries after second overflow (§8.1)";

    uint32_t priqAfter2 = smmu->getPriqProdIndex();
    bool priqOvflgAfter2 = ((priqAfter2 >> 31) & 1u) != 0;
    EXPECT_TRUE(priqOvflgAfter2)
        << "BUG-NEW-02: PRIQ_PROD.OVFLG must remain SET after subsequent inhibited entries (§8.1)";
}

// ── BUG-NEW-05: Two-stage Stage-2 fault must carry originating PASID ─────────
//
// §7.3: Event records must carry the SubstreamID (PASID) of the originating
// transaction.  In performBothStagesTranslation(), the Stage-2 fault path at
// line 1214 hardcodes PASID=0 instead of using the pasid parameter.
//
// Test strategy:
//   - Configure a stream for both-stage translation.
//   - Create PASID=0 (hypervisor) — this registers as the Stage-2 address space.
//   - Create PASID=42 — this is the Stage-1 address space for the actual request.
//   - Map Stage-1: IOVA 0x4000 → IPA 0x9000 for PASID=42.
//   - Do NOT map IPA 0x9000 in PASID=0's address space (Stage-2 has no mapping).
//   - Stage-1 succeeds (IOVA→IPA), Stage-2 fails (IPA not in Stage-2 table).
//   - The resulting fault event must have pasid == 42, NOT 0.

TEST(BugNew05Spec, TwoStageTranslation_Stage2Fault_PreservesOriginalPASID) {
    auto smmu = std::unique_ptr<SMMU>(new SMMU());
    smmu->enable();
    smmu->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    static constexpr StreamID TEST_SID    = 0x50;
    static constexpr PASID    HYPER_PASID = 0;    // hypervisor / Stage-2 address space
    static constexpr PASID    GUEST_PASID = 42;   // guest / Stage-1 address space
    static constexpr IOVA     TEST_IOVA   = 0x4000ULL;
    static constexpr PA       TEST_IPA    = 0x9000'0000ULL; // IPA produced by Stage-1

    StreamConfig cfg = makeTwoStageConfig();
    ASSERT_TRUE(smmu->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(TEST_SID).isOk());

    // Create PASID=0 first — because stage2Enabled is set, the StreamContext
    // aliases this address space as the Stage-2 page table.
    ASSERT_TRUE(smmu->createStreamPASID(TEST_SID, HYPER_PASID).isOk());

    // Create PASID=42 for the guest (Stage-1).
    ASSERT_TRUE(smmu->createStreamPASID(TEST_SID, GUEST_PASID).isOk());

    // Map Stage-1: IOVA → IPA for the guest PASID.
    // IPA = 0x9000_0000; Stage-2 (PASID=0) has NO mapping for this IPA.
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_SID, GUEST_PASID, TEST_IOVA, TEST_IPA, perms).isOk());
    // Deliberately do NOT call mapPage(TEST_SID, HYPER_PASID, TEST_IPA, ...) so
    // that the Stage-2 lookup of IPA 0x9000_0000 fails with PageNotMapped.

    smmu->clearEventQueue();
    smmu->clearEvents();

    // Trigger translation — Stage-1 succeeds (IOVA→IPA), Stage-2 fails (IPA unmapped).
    TranslationResult result = smmu->translate(TEST_SID, GUEST_PASID, TEST_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "Two-stage translation must fail when Stage-2 has no mapping for the IPA";

    // The bug is in recordComprehensiveFault() which stores the Stage-2 fault record
    // into the FaultHandler with hardcoded pasid=0.  getEvents() returns these FaultRecords.
    //
    // Before the fix: fault.pasid == 0 (hardcoded on line 1214 of smmu.cpp).
    // After the fix:  fault.pasid == GUEST_PASID (42).
    auto faultRecords = smmu->getEvents();
    ASSERT_TRUE(faultRecords.isOk())
        << "getEvents() must succeed";

    bool foundStage2FaultRecord = false;
    for (const auto& fault : faultRecords.getValue()) {
        if (fault.streamID == TEST_SID) {
            EXPECT_EQ(fault.pasid, GUEST_PASID)
                << "BUG-NEW-05: Stage-2 fault record must carry originating PASID="
                << GUEST_PASID << ", not hardcoded 0 (ARM §7.3, smmu.cpp line ~1214)";
            foundStage2FaultRecord = true;
            break;
        }
    }
    EXPECT_TRUE(foundStage2FaultRecord)
        << "A fault record must be generated for the failed two-stage translation";
}

}  // namespace test
}  // namespace smmu
