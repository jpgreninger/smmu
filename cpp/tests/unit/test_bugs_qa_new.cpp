// TDD failing tests for BUG-QA-1, BUG-QA-2, BUG-QA-4, BUG-QA-5, BUG-QA-6
// (C++ implementation).
//
// Each test is written to FAIL with the current code and PASS only after the
// corresponding fix is applied.  No fixes are included here.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-QA-1 (§6.3.1): IDR0.STALL_MODEL bits[25:24] must reflect runtime value.
//   After setStallModel(0x01), getIDR0() bits[25:24] must read 0b01.
//   Currently getIDR0() hard-codes 0b00 regardless of stallModel_.
//
// BUG-QA-2 (§6.3.6): IDR5.STALL_MAX[31:16] must be 0 when STALL_MODEL==0b01.
//   §6.3.6: STALL_MAX is RES0 when IDR0.STALL_MODEL == 0b01 (terminate-only).
//   Currently getIDR5() hard-codes STALL_MAX = 64 regardless of stallModel_.
//
// BUG-QA-4 (§8.1): auto-PRG ResponseCode for overflow with no PASID → Success.
//   §8.1: when the overflowing PPR has no PASID (pasid==0 and not substream-
//   capable), the auto-PRG_RESPONSE must carry responseCode=0 (Success).
//   PRIAutoFailure struct must expose a responseCode field.
//
// BUG-QA-5 (§8.1): while PRIQ overflow is active (OVFLG!=OVACKFLG), new PPRs
//   must be rejected even if the queue has physical space.
//   Currently submitPageRequest() only checks size(), not overflow state.
//
// BUG-QA-6 (§7.3.19): E_PAGE_REQUEST permission bits for ReadExecute / ReadExecutePrivileged.
//   ReadExecute         → ur=1 AND ux=1 (current code sets ux=1 but NOT ur=1).
//   ReadExecutePrivileged → pr=1 AND px=1 (current code sets px=0, ux=1 instead of px=1).
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <memory>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Minimum PRIQ size for overflow tests.  Must be small enough to overflow
// quickly but still >= the implementation minimum (4).
static constexpr size_t SMALL_PRIQ = 4;

static std::unique_ptr<SMMU> makeSmallPRIQ() {
    SMMUConfiguration config = SMMUConfiguration::createDefault();
    QueueConfiguration qcfg(512, 256, static_cast<size_t>(SMALL_PRIQ));
    config.setQueueConfiguration(qcfg);
    auto s = std::make_unique<SMMU>(config);
    s->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    return s;
}

// Fill the PRIQ to exactly its capacity with non-Last Read requests.
static void fillPRIQ(SMMU& smmu, size_t count, StreamID sid, PASID pid,
                     uint16_t startPrg) {
    for (size_t i = 0; i < count; ++i) {
        PRIEntry req;
        req.streamID         = sid;
        req.pasid            = pid;
        req.requestedAddress = static_cast<IOVA>((startPrg + i) << 12);
        req.accessType       = AccessType::Read;
        req.isLastRequest    = false;
        req.prgIndex         = static_cast<uint16_t>(startPrg + i);
        smmu.submitPageRequest(req);
    }
}

} // anonymous namespace

// ============================================================================
// BUG-QA-1: IDR0.STALL_MODEL[25:24] must reflect the runtime stallModel_ value
// ============================================================================
//
// ARM IHI0070G.b §6.3.1: STALL_MODEL is at bits[25:24] of IDR0.
//   0b00 = stall+terminate both supported (default)
//   0b01 = terminate-only (stall disabled)
//
// After setStallModel(0x01), getIDR0() bits[25:24] must return 0b01.
//
// BEFORE FIX: IDR0 bits[25:24] == 0b00 regardless of setStallModel().
// AFTER FIX:  IDR0 bits[25:24] == 0b01 after setStallModel(0x01).

TEST(BugQA1IDR0StallModel, IDR0ReflectsDefaultStallModel) {
    SMMU smmu;
    enableSMMU(smmu);
    // Default stall model is 0b00; IDR0 bits[25:24] must be 0b00.
    uint32_t idr0 = smmu.getIDR0();
    uint32_t stall_model_bits = (idr0 >> 24) & 0x3u;
    EXPECT_EQ(stall_model_bits, 0u)
        << "BUG-QA-1: IDR0.STALL_MODEL[25:24] must be 0b00 when stallModel_==0";
}

TEST(BugQA1IDR0StallModel, IDR0ReflectsTerminateOnlyStallModel) {
    SMMU smmu;
    enableSMMU(smmu);

    // Set terminate-only model (0b01).
    smmu.setStallModel(0x01u);

    // IDR0.STALL_MODEL[25:24] must now read 0b01.
    // BEFORE FIX: returns 0b00 (hardcoded).
    // AFTER FIX:  returns 0b01.
    uint32_t idr0 = smmu.getIDR0();
    uint32_t stall_model_bits = (idr0 >> 24) & 0x3u;
    EXPECT_EQ(stall_model_bits, 1u)
        << "BUG-QA-1: IDR0.STALL_MODEL[25:24] must be 0b01 after setStallModel(0x01) (§6.3.1)";
}

TEST(BugQA1IDR0StallModel, IDR0ReflectsStallModelAfterReset) {
    SMMU smmu;
    enableSMMU(smmu);

    // Set terminate-only, then reset → should return to 0b00.
    smmu.setStallModel(0x01u);
    smmu.reset();
    uint32_t idr0 = smmu.getIDR0();
    uint32_t stall_model_bits = (idr0 >> 24) & 0x3u;
    EXPECT_EQ(stall_model_bits, 0u)
        << "BUG-QA-1: IDR0.STALL_MODEL must return to 0b00 after reset()";
}

// ============================================================================
// BUG-QA-2: IDR5.STALL_MAX[31:16] must be 0 when STALL_MODEL == 0b01
// ============================================================================
//
// ARM IHI0070G.b §6.3.6: STALL_MAX[31:16] is RES0 when IDR0.STALL_MODEL==0b01.
// When STALL_MODEL==0b00, STALL_MAX is non-zero (e.g. 64).
//
// BEFORE FIX: IDR5 STALL_MAX[31:16] == 64 regardless of stall model.
// AFTER FIX:  IDR5 STALL_MAX[31:16] == 0 when setStallModel(0x01).

TEST(BugQA2IDR5StallMax, IDR5StallMaxNonZeroForDefaultModel) {
    SMMU smmu;
    enableSMMU(smmu);
    // Default stall model 0b00: STALL_MAX must be non-zero.
    uint32_t idr5 = smmu.getIDR5();
    uint32_t stall_max = (idr5 >> 16) & 0xFFFFu;
    EXPECT_GT(stall_max, 0u)
        << "BUG-QA-2: IDR5.STALL_MAX[31:16] must be non-zero when STALL_MODEL==0b00 (§6.3.6)";
}

TEST(BugQA2IDR5StallMax, IDR5StallMaxZeroForTerminateOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    // Set terminate-only model (0b01).
    smmu.setStallModel(0x01u);

    // STALL_MAX must be RES0 (== 0) when STALL_MODEL == 0b01.
    // BEFORE FIX: returns 64 (hardcoded).
    // AFTER FIX:  returns 0.
    uint32_t idr5 = smmu.getIDR5();
    uint32_t stall_max = (idr5 >> 16) & 0xFFFFu;
    EXPECT_EQ(stall_max, 0u)
        << "BUG-QA-2: IDR5.STALL_MAX[31:16] must be 0 (RES0) when IDR0.STALL_MODEL==0b01 (§6.3.6)";
}

TEST(BugQA2IDR5StallMax, IDR5StallMaxRestoredAfterReset) {
    SMMU smmu;
    enableSMMU(smmu);

    // Set terminate-only, reset → STALL_MAX should come back to non-zero.
    smmu.setStallModel(0x01u);
    smmu.reset();
    uint32_t idr5 = smmu.getIDR5();
    uint32_t stall_max = (idr5 >> 16) & 0xFFFFu;
    EXPECT_GT(stall_max, 0u)
        << "BUG-QA-2: IDR5.STALL_MAX[31:16] must be non-zero after reset() (§6.3.6)";
}

// ============================================================================
// BUG-QA-4: auto-PRG ResponseCode — no PASID → responseCode==0 (Success)
// ============================================================================
//
// ARM IHI0070G.b §8.1: when the overflowing Last=1 PPR has no PASID
// (pasid==0 on a non-substream stream), the auto-PRG_RESPONSE must carry
// responseCode=0 (Success).  The struct must expose a responseCode field.
//
// BEFORE FIX: PRIAutoFailure has no responseCode field → compilation fails.
// AFTER FIX:  responseCode==0 for the no-PASID case.

TEST(BugQA4AutoPRGResponseCode, NoPasidOverflowHasSuccessResponseCode) {
    auto smmu = makeSmallPRIQ();

    static constexpr StreamID SID = 0x10u;
    static constexpr PASID    PID = 0u;      // no PASID

    // Fill queue to capacity with non-Last requests (no auto-failures).
    fillPRIQ(*smmu, SMALL_PRIQ, SID, PID, 0u);
    ASSERT_EQ(smmu->getPRIQueueSize(), SMALL_PRIQ)
        << "precondition: PRIQ must be at capacity";
    ASSERT_TRUE(smmu->getPRIAutoFailures().empty())
        << "precondition: no auto-failures before overflow";

    // Submit a Last=1 PPR without PASID — triggers overflow auto-response.
    PRIEntry overflow;
    overflow.streamID         = SID;
    overflow.pasid            = PID;
    overflow.requestedAddress = 0xF000u;
    overflow.accessType       = AccessType::Read;
    overflow.isLastRequest    = true;
    overflow.prgIndex         = 100u;
    smmu->submitPageRequest(overflow);

    auto failures = smmu->getPRIAutoFailures();
    ASSERT_EQ(failures.size(), 1u)
        << "BUG-QA-4: exactly one auto-failure must be generated for Last=1 overflow";

    // ARM §8.1: no PASID → responseCode=0 (Success) per spec.
    // BEFORE FIX: responseCode field does not exist → compile error.
    // AFTER FIX:  responseCode == 0.
    EXPECT_EQ(failures.front().responseCode, static_cast<uint8_t>(0u))
        << "BUG-QA-4: no-PASID overflow must produce responseCode=0 (Success) per §8.1";
}

TEST(BugQA4AutoPRGResponseCode, NoPasidIncludePasidIsFalse) {
    auto smmu = makeSmallPRIQ();

    static constexpr StreamID SID = 0x11u;
    static constexpr PASID    PID = 0u;

    fillPRIQ(*smmu, SMALL_PRIQ, SID, PID, 0u);

    PRIEntry overflow;
    overflow.streamID         = SID;
    overflow.pasid            = PID;
    overflow.requestedAddress = 0xF000u;
    overflow.accessType       = AccessType::Read;
    overflow.isLastRequest    = true;
    overflow.prgIndex         = 50u;
    smmu->submitPageRequest(overflow);

    auto failures = smmu->getPRIAutoFailures();
    ASSERT_EQ(failures.size(), 1u);

    // includePASID must be false when no PASID is present.
    // BEFORE FIX: field does not exist → compile error.
    // AFTER FIX:  includePASID == false.
    EXPECT_FALSE(failures.front().includePASID)
        << "BUG-QA-4: no-PASID overflow must have includePASID=false per §8.1";
}

// ============================================================================
// BUG-QA-5: overflow-active inhibition — new PPRs rejected when OVFLG!=OVACKFLG
// ============================================================================
//
// ARM IHI0070G.b §8.1: once PRIQ overflow is active (PRIQ_PROD.OVFLG differs
// from PRIQ_CONS.OVACKFLG), subsequent PPRs must be rejected and treated as if
// the queue were still full — even if the software consumer drains the queue
// and frees physical space.
//
// BEFORE FIX: submitPageRequest() only checks size() >= maxPRIQueueSize;
//             after draining, new PPRs bypass the overflow-inhibition check.
// AFTER FIX:  overflow-active state (OVFLG != OVACKFLG) prevents enqueue.

TEST(BugQA5OverflowInhibition, NewPPRsRejectedWhileOverflowActive) {
    auto smmu = makeSmallPRIQ();

    static constexpr StreamID SID = 0x20u;
    static constexpr PASID    PID = 0u;

    // Step 1: fill queue to capacity.
    fillPRIQ(*smmu, SMALL_PRIQ, SID, PID, 0u);
    ASSERT_EQ(smmu->getPRIQueueSize(), SMALL_PRIQ);

    // Step 2: cause overflow (Last=1) — triggers OVFLG toggle.
    PRIEntry ovf;
    ovf.streamID         = SID;
    ovf.pasid            = PID;
    ovf.requestedAddress = 0xA000u;
    ovf.accessType       = AccessType::Read;
    ovf.isLastRequest    = true;
    ovf.prgIndex         = 200u;
    smmu->submitPageRequest(ovf);
    ASSERT_EQ(smmu->getPRIAutoFailures().size(), 1u)
        << "precondition: overflow must have been triggered";

    // Step 3: drain the queue (simulate software consuming entries).
    smmu->processPRIQueue();
    EXPECT_EQ(smmu->getPRIQueueSize(), 0u)
        << "precondition: queue must be empty after processPRIQueue()";

    // Step 4: clear auto-failure list so counts start fresh.
    smmu->clearPRIAutoFailures();

    // Step 5: submit a new Last=1 PPR while overflow is still active.
    // Overflow is active until OVACKFLG is updated (software acknowledges via
    // PRIQ_CONS write or equivalent).  Since software has not yet acknowledged,
    // the new PPR must be rejected with another auto-failure (or silently
    // discarded if it is non-Last).  For Last=1, an auto-failure must appear.
    //
    // BEFORE FIX: size()==0 < maxPRIQueueSize → enqueued normally, no auto-failure.
    // AFTER FIX:  overflow is active → rejected with auto-failure.
    PRIEntry newReq;
    newReq.streamID         = SID;
    newReq.pasid            = PID;
    newReq.requestedAddress = 0xB000u;
    newReq.accessType       = AccessType::Read;
    newReq.isLastRequest    = true;
    newReq.prgIndex         = 201u;
    smmu->submitPageRequest(newReq);

    auto failures = smmu->getPRIAutoFailures();
    EXPECT_EQ(failures.size(), 1u)
        << "BUG-QA-5: while PRIQ overflow is active (OVFLG!=OVACKFLG), new Last=1 "
           "PPRs must be rejected with an auto-failure even when the queue is empty (§8.1)";
}

TEST(BugQA5OverflowInhibition, NonLastPPRSilentlyDiscardedWhileOverflowActive) {
    auto smmu = makeSmallPRIQ();

    static constexpr StreamID SID = 0x21u;
    static constexpr PASID    PID = 0u;

    // Fill and overflow.
    fillPRIQ(*smmu, SMALL_PRIQ, SID, PID, 0u);
    PRIEntry ovf;
    ovf.streamID         = SID;
    ovf.pasid            = PID;
    ovf.requestedAddress = 0xC000u;
    ovf.accessType       = AccessType::Read;
    ovf.isLastRequest    = true;
    ovf.prgIndex         = 300u;
    smmu->submitPageRequest(ovf);
    ASSERT_EQ(smmu->getPRIAutoFailures().size(), 1u);

    smmu->processPRIQueue();
    smmu->clearPRIAutoFailures();

    // Non-Last PPR while overflow is active → silently discarded, no auto-failure.
    PRIEntry nonLast;
    nonLast.streamID         = SID;
    nonLast.pasid            = PID;
    nonLast.requestedAddress = 0xD000u;
    nonLast.accessType       = AccessType::Read;
    nonLast.isLastRequest    = false;
    nonLast.prgIndex         = 301u;
    smmu->submitPageRequest(nonLast);

    // Non-Last → no auto-failure (BEFORE FIX: may be enqueued as normal entry).
    EXPECT_TRUE(smmu->getPRIAutoFailures().empty())
        << "BUG-QA-5: non-Last PPR while overflow active must not produce auto-failure";
    // But must NOT appear in the queue (it should be rejected).
    // BEFORE FIX: the entry appears in the queue.
    // AFTER FIX:  the entry is rejected.
    EXPECT_EQ(smmu->getPRIQueueSize(), 0u)
        << "BUG-QA-5: non-Last PPR while overflow active must be rejected (not enqueued)";
}

// ============================================================================
// BUG-QA-6: E_PAGE_REQUEST permission bits — ReadExecute / ReadExecutePrivileged
// ============================================================================
//
// ARM IHI0070G.b §7.3.19:
//   ReadExecute            → ur=1, ux=1 (both unprivileged read AND execute)
//   ReadExecutePrivileged  → pr=1, px=1 (both privileged read AND execute)
//
// Current bugs:
//   ReadExecute: sets ux=1 but NOT ur=1 (ur is false, should be true).
//   ReadExecutePrivileged: sets ux=1 instead of px=1, and pr=0 (should be pr=1 px=1).
//
// BEFORE FIX: ReadExecute → ur=false, ux=true (ur missing).
//             ReadExecutePrivileged → px=false, ux=true (wrong bit).
// AFTER FIX:  ReadExecute → ur=true, ux=true.
//             ReadExecutePrivileged → pr=true, px=true, ux=false.

// Helper: submit a page request with the given access type and return
// the E_PAGE_REQUEST event from the event queue.
static EventEntry getPageRequestEvent(SMMU& smmu, StreamID sid, PASID pid,
                                      uint16_t prgIdx, AccessType acc) {
    PRIEntry req;
    req.streamID         = sid;
    req.pasid            = pid;
    req.requestedAddress = static_cast<IOVA>(prgIdx << 12);
    req.accessType       = acc;
    req.isLastRequest    = true;
    req.prgIndex         = prgIdx;
    smmu.submitPageRequest(req);

    std::vector<EventEntry> events = smmu.getEventQueue();
    for (auto it = events.rbegin(); it != events.rend(); ++it) {
        if (it->type == EventType::E_PAGE_REQUEST && it->streamID == sid) {
            return *it;
        }
    }
    // Return a zeroed event if not found (test will fail due to unset fields).
    return EventEntry{};
}

TEST(BugQA6PageReqPermBits, ReadExecuteSetsUrAndUx) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID    = 0x30u;
    static constexpr PASID    PID    = 0u;
    static constexpr uint16_t PRGIDX = 10u;

    EventEntry ev = getPageRequestEvent(smmu, SID, PID, PRGIDX, AccessType::ReadExecute);

    // ARM §7.3.19: ReadExecute → unprivileged read + execute.
    // ur must be true.
    // BEFORE FIX: ur==false (bug — ReadExecute only sets ux).
    // AFTER FIX:  ur==true.
    EXPECT_TRUE(ev.ur)
        << "BUG-QA-6: ReadExecute must set ur=true (unprivileged read) per §7.3.19";

    // ux must also be true.
    EXPECT_TRUE(ev.ux)
        << "BUG-QA-6: ReadExecute must set ux=true (unprivileged execute) per §7.3.19";

    // No write or privileged bits.
    EXPECT_FALSE(ev.uw)
        << "BUG-QA-6: ReadExecute must NOT set uw (§7.3.19)";
    EXPECT_FALSE(ev.pr)
        << "BUG-QA-6: ReadExecute must NOT set pr (§7.3.19)";
    EXPECT_FALSE(ev.pw)
        << "BUG-QA-6: ReadExecute must NOT set pw (§7.3.19)";
    EXPECT_FALSE(ev.px)
        << "BUG-QA-6: ReadExecute must NOT set px (§7.3.19)";
}

TEST(BugQA6PageReqPermBits, ReadExecutePrivilegedSetsPrAndPx) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID    = 0x31u;
    static constexpr PASID    PID    = 0u;
    static constexpr uint16_t PRGIDX = 11u;

    EventEntry ev = getPageRequestEvent(smmu, SID, PID, PRGIDX, AccessType::ReadExecutePrivileged);

    // ARM §7.3.19: ReadExecutePrivileged → privileged read + execute.
    // pr must be true.
    // BEFORE FIX: pr==false.
    // AFTER FIX:  pr==true.
    EXPECT_TRUE(ev.pr)
        << "BUG-QA-6: ReadExecutePrivileged must set pr=true (privileged read) per §7.3.19";

    // px must be true.
    // BEFORE FIX: px==false, ux==true (wrong bits set).
    // AFTER FIX:  px==true.
    EXPECT_TRUE(ev.px)
        << "BUG-QA-6: ReadExecutePrivileged must set px=true (privileged execute) per §7.3.19";

    // Unprivileged bits must NOT be set.
    EXPECT_FALSE(ev.ur)
        << "BUG-QA-6: ReadExecutePrivileged must NOT set ur (§7.3.19)";
    EXPECT_FALSE(ev.uw)
        << "BUG-QA-6: ReadExecutePrivileged must NOT set uw (§7.3.19)";
    EXPECT_FALSE(ev.ux)
        << "BUG-QA-6: ReadExecutePrivileged must NOT set ux (§7.3.19)";
    EXPECT_FALSE(ev.pw)
        << "BUG-QA-6: ReadExecutePrivileged must NOT set pw (§7.3.19)";
}

// Regression: plain Read still sets ur only (no regressions from the fix).
TEST(BugQA6PageReqPermBits, PlainReadSetsUrOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    EventEntry ev = getPageRequestEvent(smmu, 0x32u, 0u, 20u, AccessType::Read);

    EXPECT_TRUE(ev.ur);
    EXPECT_FALSE(ev.uw);
    EXPECT_FALSE(ev.ux);
    EXPECT_FALSE(ev.pr);
    EXPECT_FALSE(ev.pw);
    EXPECT_FALSE(ev.px);
}

// Regression: plain Write still sets uw only.
TEST(BugQA6PageReqPermBits, PlainWriteSetsUwOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    EventEntry ev = getPageRequestEvent(smmu, 0x33u, 0u, 21u, AccessType::Write);

    EXPECT_FALSE(ev.ur);
    EXPECT_TRUE(ev.uw);
    EXPECT_FALSE(ev.ux);
    EXPECT_FALSE(ev.pr);
    EXPECT_FALSE(ev.pw);
    EXPECT_FALSE(ev.px);
}

// Regression: ReadPrivileged sets pr only.
TEST(BugQA6PageReqPermBits, ReadPrivilegedSetsPrOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    EventEntry ev = getPageRequestEvent(smmu, 0x34u, 0u, 22u, AccessType::ReadPrivileged);

    EXPECT_FALSE(ev.ur);
    EXPECT_FALSE(ev.uw);
    EXPECT_FALSE(ev.ux);
    EXPECT_TRUE(ev.pr);
    EXPECT_FALSE(ev.pw);
    EXPECT_FALSE(ev.px);
}

// Regression: ExecutePrivileged sets px only.
TEST(BugQA6PageReqPermBits, ExecutePrivilegedSetsPxOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    EventEntry ev = getPageRequestEvent(smmu, 0x35u, 0u, 23u, AccessType::ExecutePrivileged);

    EXPECT_FALSE(ev.ur);
    EXPECT_FALSE(ev.uw);
    EXPECT_FALSE(ev.ux);
    EXPECT_FALSE(ev.pr);
    EXPECT_FALSE(ev.pw);
    EXPECT_TRUE(ev.px);
}
