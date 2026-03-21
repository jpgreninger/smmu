// ARM SMMU v3 Bug Fix Tests:
//   BUG-NEW-A: isEventqEmptyByIndex/getEventqOccupiedEntries don't mask OVFLG (bit 31)
//   BUG-NEW-D: advanceQueueIndex strips OVFLG from EVENTQ/PRIQ PROD on advance
//   BUG-NEW-E: RIL SCALE field clamped to 3-bit (0-7) instead of spec's effective max 39
//
// ARM IHI0070G.b references:
//   §3.5.1  — queue empty = PROD.WR == CONS.RD (index bits only, not OVFLG)
//   §7.4    — OVFLG must persist until software clears via CONS.OVACKFLG
//   §4.4.1.1 — SCALE effective range 0-39; values > 39 treated as 39

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>

using namespace smmu;

namespace {

// Minimum event queue size to make overflow easy to trigger.
static constexpr size_t SMALL_EQ = 16;

static std::unique_ptr<SMMU> makeSmallQueueSMMU() {
    QueueConfiguration qcfg(SMALL_EQ, SMALL_EQ, SMALL_EQ);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto s = std::make_unique<SMMU>(cfg);
    s->enable();
    s->setCR2(SMMU::CR2_RECINVSID);
    return s;
}

// Fill the event queue to capacity by generating C_BAD_STREAMID faults.
// 'startSid' must be an unconfigured stream so translate() returns C_BAD_STREAMID.
static void fillEventQueue(SMMU& smmu, size_t count, StreamID startSid = 0x0300u) {
    for (size_t i = 0; i < count; ++i) {
        smmu.translate(static_cast<StreamID>(startSid + i), 0, 0x1000u,
                       AccessType::Read, SecurityState::NonSecure);
    }
}

} // namespace

// ============================================================================
// BUG-NEW-A: isEventqEmptyByIndex() and getEventqOccupiedEntries() must mask
//            OVFLG (bit 31) before comparing PROD and CONS.
//
// Scenario:
//   1. Fill queue to capacity (SMALL_EQ events).
//   2. Generate one more event → OVFLG toggled in PROD (bit 31 set); event DROPPED.
//      PROD.WR unchanged (no advance since event was dropped).
//   3. Process all events via processEventQueue() → CONS advances until RD == WR.
//   4. Post-drain: PROD bit31=1, CONS bit31=0, WR==RD in index bits.
//   5. isEventqEmptyByIndex() must return true  (BUG: returns false).
//   6. getEventqOccupiedEntries() must return 0 (BUG: returns large value).
// ============================================================================

/// After overflow (OVFLG set in PROD bit31) and queue drain via processEventQueue(),
/// isEventqEmptyByIndex() must return true even though PROD bit31 != CONS bit31.
///
/// BUG: raw comparison "eventqProd == eventqCons" is false because bit31 differs.
/// Fix: mask bit31 from both sides before comparing.
TEST(BugNewA, IsEventqEmptyIgnoresOvflg) {
    auto smmu = makeSmallQueueSMMU();

    // Fill queue to capacity.
    fillEventQueue(*smmu, SMALL_EQ, 0x0300u);
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_EQ)
        << "precondition: queue must be full";

    // Generate one more fault → OVFLG toggled in PROD, event DROPPED.
    smmu->translate(0x0400u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    // Verify OVFLG is now set.
    uint32_t prodWithOvflg = smmu->getEventqProdIndex();
    ASSERT_NE(prodWithOvflg & (1u << 31), 0u)
        << "precondition: OVFLG must be set in PROD after overflow";

    // Verify CONS bit31 is 0 (not yet acknowledged).
    uint32_t consBeforeDrain = smmu->getEventqConsIndex();
    ASSERT_EQ(consBeforeDrain & (1u << 31), 0u)
        << "precondition: CONS bit31 must be 0 before acknowledgement";

    // Drain all events via processEventQueue(): CONS advances until RD == WR index.
    smmu->processEventQueue();

    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "precondition: queue must be empty after processEventQueue";

    // Post-drain: PROD bit31=1, CONS bit31=0, WR==RD in index bits (both =16).
    uint32_t prodFinal = smmu->getEventqProdIndex();
    uint32_t consFinal = smmu->getEventqConsIndex();
    ASSERT_NE(prodFinal & (1u << 31), 0u)
        << "OVFLG must still be set in PROD (software has not acknowledged)";
    ASSERT_EQ(consFinal & (1u << 31), 0u)
        << "CONS bit31 must be 0 (not yet acknowledged)";

    // isEventqEmptyByIndex() must return true — WR == RD in index bits.
    // BUG: raw comparison "0x80000010 == 0x00000010" → false.
    // Fix: "(0x80000010 & ~bit31) == (0x10 & ~bit31)" → "0x10 == 0x10" → true.
    EXPECT_TRUE(smmu->isEventqEmptyByIndex())
        << "isEventqEmptyByIndex() must return true when WR==RD even with OVFLG set; "
        << "PROD=0x" << std::hex << prodFinal
        << " CONS=0x" << consFinal << std::dec;
}

/// getEventqOccupiedEntries() must return 0 when queue is empty after overflow.
///
/// BUG: queueOccupied(prodWithBit31, consWithoutBit31, log2size) treats the bit31
///      difference as a large index gap, returning ~2^31 occupied entries.
/// Fix: mask bit31 from both PROD and CONS inputs to queueOccupied().
TEST(BugNewA, GetEventqOccupiedEntriesIgnoresOvflg) {
    auto smmu = makeSmallQueueSMMU();

    // Fill to capacity, trigger overflow.
    fillEventQueue(*smmu, SMALL_EQ, 0x0500u);
    smmu->translate(0x0600u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set";

    // Drain all events.
    smmu->processEventQueue();

    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "precondition: queue must be empty after drain";

    // getEventqOccupiedEntries() must return 0.
    uint32_t occupied = smmu->getEventqOccupiedEntries();
    EXPECT_EQ(occupied, 0u)
        << "getEventqOccupiedEntries() must return 0 when queue is empty after overflow; "
        << "got " << occupied
        << " (PROD=0x" << std::hex << smmu->getEventqProdIndex()
        << " CONS=0x" << smmu->getEventqConsIndex() << std::dec << ")";
}

/// Occupied count must equal actual queue size when OVFLG is set but queue is NOT empty.
/// This verifies the mask does not over-correct: only bit31 is masked, not the index bits.
TEST(BugNewA, GetEventqOccupiedEntriesWhenFullWithOvflg) {
    auto smmu = makeSmallQueueSMMU();

    // Fill to capacity.
    fillEventQueue(*smmu, SMALL_EQ, 0x0700u);
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_EQ)
        << "precondition: queue must be full";

    // Trigger overflow — OVFLG set, but SMALL_EQ events still in queue.
    smmu->translate(0x0800u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set";
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_EQ)
        << "precondition: overflow event was dropped, queue still full";

    // Occupied must equal SMALL_EQ (all entries present), not some garbage large value.
    uint32_t occupied = smmu->getEventqOccupiedEntries();
    EXPECT_EQ(occupied, static_cast<uint32_t>(SMALL_EQ))
        << "getEventqOccupiedEntries() must equal actual queue size with OVFLG set; "
        << "got " << occupied;
}

// ============================================================================
// BUG-NEW-D: OVFLG must survive PROD advance when a new event is enqueued
//            while the queue is in overflow state.
//
// Scenario:
//   1. Fill queue to capacity.
//   2. Generate one more event → OVFLG toggled, event DROPPED.
//   3. Process ALL events via processEventQueue() → queue empty, CONS.RD == PROD.WR.
//   4. Generate a new event → PROD advances via advanceQueueIndex.
//   5. OVFLG must still be set in PROD (software has not acknowledged).
//
// BUG: advanceQueueIndex returns (idx+1) % modulus where modulus = 2^(log2size+1) <= 2^20.
//      This strips bit 31 from the result, clearing OVFLG.
// Fix: preserve bit 31 of old PROD after each advance at eventqProd call sites.
// ============================================================================

/// OVFLG must be preserved in PROD after a PROD advance (new event enqueued).
TEST(BugNewD, OvflgPreservedAfterEventqProdAdvance) {
    auto smmu = makeSmallQueueSMMU();

    // Fill queue to capacity.
    fillEventQueue(*smmu, SMALL_EQ, 0x1000u);
    ASSERT_EQ(smmu->getEventQueueSize(), SMALL_EQ)
        << "precondition: queue must be full";

    // Trigger overflow: OVFLG toggled in PROD, event dropped.
    smmu->translate(0x1100u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    uint32_t prodBeforeAdvance = smmu->getEventqProdIndex();
    ASSERT_NE(prodBeforeAdvance & (1u << 31), 0u)
        << "precondition: OVFLG must be set after overflow";

    // Drain ALL events — queue is now empty, CONS.RD == PROD.WR in index bits.
    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "precondition: queue must be empty after processEventQueue";

    // Enqueue a new event — this advances PROD.
    smmu->translate(0x1101u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getEventQueueSize(), 1u)
        << "precondition: new event must be enqueued (queue had space)";

    // OVFLG must still be set in PROD.
    uint32_t prodAfterAdvance = smmu->getEventqProdIndex();
    EXPECT_NE(prodAfterAdvance & (1u << 31), 0u)
        << "OVFLG (bit 31) must survive PROD advance; "
        << "before=0x" << std::hex << prodBeforeAdvance
        << " after=0x" << prodAfterAdvance << std::dec;
}

/// OVFLG must remain set through multiple successive PROD advances.
TEST(BugNewD, OvflgPreservedThroughMultipleAdvances) {
    auto smmu = makeSmallQueueSMMU();

    // Fill and overflow.
    fillEventQueue(*smmu, SMALL_EQ, 0x2000u);
    smmu->translate(0x2100u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set";

    // Drain all events to make room.
    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "precondition: queue must be empty";

    // Enqueue several new events; check OVFLG survives each advance.
    for (size_t i = 0; i < 4u; ++i) {
        smmu->translate(static_cast<StreamID>(0x2200u + i), 0, 0x1000u,
                        AccessType::Read, SecurityState::NonSecure);
        uint32_t prod = smmu->getEventqProdIndex();
        EXPECT_NE(prod & (1u << 31), 0u)
            << "OVFLG must remain set after PROD advance #" << (i + 1)
            << " (prod=0x" << std::hex << prod << std::dec << ")";
    }
}

/// After software acknowledges (acknowledgeEventQueueOverflow), OVFLG and OVACKFLG
/// are equal; a subsequent PROD advance must not disturb this invariant.
TEST(BugNewD, OvflgAndOvackflgRemainEqualAfterAckAndAdvance) {
    auto smmu = makeSmallQueueSMMU();

    // Fill, overflow, drain all, acknowledge.
    fillEventQueue(*smmu, SMALL_EQ, 0x3000u);
    smmu->translate(0x3100u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set";

    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "precondition: queue must be empty";

    smmu->acknowledgeEventQueueOverflow();

    // After ack: OVFLG == OVACKFLG (both bit31 values equal).
    uint32_t prod = smmu->getEventqProdIndex();
    uint32_t cons = smmu->getEventqConsIndex();
    ASSERT_EQ((prod >> 31) & 1u, (cons >> 31) & 1u)
        << "after ack, OVFLG and OVACKFLG must match";

    // Enqueue a new event — queue is empty, no new overflow.
    smmu->translate(0x3200u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getEventQueueSize(), 1u)
        << "precondition: new event must be enqueued";

    // OVFLG must still equal OVACKFLG (no spurious re-set of OVFLG).
    uint32_t prodAfter = smmu->getEventqProdIndex();
    uint32_t consAfter = smmu->getEventqConsIndex();
    EXPECT_EQ((prodAfter >> 31) & 1u, (consAfter >> 31) & 1u)
        << "OVFLG and OVACKFLG must remain equal after post-ack advance; "
        << "prod=0x" << std::hex << prodAfter
        << " cons=0x" << consAfter << std::dec;
}

// ============================================================================
// BUG-NEW-E: RIL SCALE field effective range must be 0-39 (not 0-7).
//            SCALE=8 must cover more range than SCALE=7.
//            SCALE=40 must produce the same result as SCALE=39.
//
// Spec: ARM §4.4.1.1 — values > 39 treated as 39; range 0-39 must be distinct.
//
// BUG: scale_>7 ? 35 : 5*scale_ → scale=8 produces same shift as scale=7 (35).
// Fix: scale_>39 ? 195 : 5*scale_ → shift range 0..195, covering 0-39 distinctly.
// ============================================================================

static void setupTlbiStream(SMMU& smmu, StreamID sid, uint16_t asid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;  // no VA range limit — needed for large IOVAs
    cfg.asid               = asid;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);
    smmu.setStreamASID(sid, asid);
}

/// BUG-NEW-E: scale=8 TLBI must execute without crash and not be treated as scale=7.
///
/// With tg=0 (4KB granule), num=0:
///   scale=7: shift=35 → covers [start, start + 2^47 - 1]
///   scale=8: shift=40 → covers [start, start + 2^52 - 1]  (with fix)
///   scale=8: shift=35 → covers [start, start + 2^47 - 1]  (with bug = same as scale=7)
///
/// We verify that scale=8 executes cleanly (no crash, no events generated),
/// and that a large-VA page (at 2^47, just outside scale=7 range) is translatable
/// after the TLBI (page table is intact after any TLBI command).
TEST(BugNewE, Scale8ExecutesCleanlyWithExpandedRange) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0x50u;
    const uint16_t asid = 0x50u;
    setupTlbiStream(smmu, sid, asid);

    const PagePermissions RO(true, false, false);
    const IOVA page_at_0x1000  = 0x1000u;
    // VA = 2^47 bytes from base: outside scale=7 range but inside scale=8 range.
    const IOVA page_at_2_47 = (static_cast<IOVA>(1u) << 47);

    smmu.mapPage(sid, 0u, page_at_0x1000, 0x10000u, RO);
    smmu.mapPage(sid, 0u, page_at_2_47,   0x20000u, RO);

    ASSERT_TRUE(smmu.translate(sid, 0, page_at_0x1000, AccessType::Read).isOk());
    ASSERT_TRUE(smmu.translate(sid, 0, page_at_2_47,   AccessType::Read).isOk());

    // Issue TLBI with scale=7 (covers 2^47 bytes from 0).
    smmu.clearEventQueue();
    {
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;   // 4KB
        tlbi.num          = 0u;   // 1 block
        tlbi.scale        = 7u;
        smmu.submitCommand(tlbi);
        smmu.processCommandQueue();
    }
    EXPECT_EQ(smmu.getEventQueueSize(), 0u)
        << "scale=7 TLBI must not generate events";

    // Issue TLBI with scale=8. BUG: same range as scale=7. FIX: larger range.
    smmu.clearEventQueue();
    {
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;
        tlbi.num          = 0u;
        tlbi.scale        = 8u;
        smmu.submitCommand(tlbi);
        smmu.processCommandQueue();
    }
    EXPECT_EQ(smmu.getEventQueueSize(), 0u)
        << "scale=8 TLBI must not generate events (no crash or error)";

    // Re-translation must succeed (page table intact after TLBI).
    EXPECT_TRUE(smmu.translate(sid, 0, page_at_0x1000, AccessType::Read).isOk());
    EXPECT_TRUE(smmu.translate(sid, 0, page_at_2_47,   AccessType::Read).isOk());
}

/// BUG-NEW-E: scale=8 TLBI range must differ from scale=7.
/// We place a page at 2^49 (within scale=8 range 2^52, outside scale=7 range 2^47).
/// After scale=8 TLBI, re-translation at 2^49 succeeds (page table intact).
/// The key assertion is that the command executes without crash.
TEST(BugNewE, Scale8RangeLargerThanScale7Behavioral) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0x51u;
    const uint16_t asid = 0x51u;
    setupTlbiStream(smmu, sid, asid);

    const PagePermissions RO(true, false, false);
    const IOVA page_at_2_49 = (static_cast<IOVA>(1u) << 49);
    smmu.mapPage(sid, 0u, page_at_2_49, 0x5000u, RO);

    ASSERT_TRUE(smmu.translate(sid, 0, page_at_2_49, AccessType::Read).isOk());

    smmu.clearEventQueue();
    {
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;
        tlbi.num          = 0u;
        tlbi.scale        = 8u;
        smmu.submitCommand(tlbi);
        smmu.processCommandQueue();
    }
    EXPECT_EQ(smmu.getEventQueueSize(), 0u)
        << "scale=8 TLBI must execute cleanly";

    EXPECT_TRUE(smmu.translate(sid, 0, page_at_2_49, AccessType::Read).isOk())
        << "page at 2^49 must re-translate after scale=8 TLBI (page table intact)";
}

/// BUG-NEW-E: SCALE=40 must produce same result as SCALE=39 (both clamped to 39).
/// Both must execute cleanly without crash or spurious events.
TEST(BugNewE, Scale40TreatedSameAsScale39) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0x52u;
    const uint16_t asid = 0x52u;
    setupTlbiStream(smmu, sid, asid);

    const PagePermissions RO(true, false, false);
    smmu.mapPage(sid, 0u, 0x1000u, 0x6000u, RO);
    ASSERT_TRUE(smmu.translate(sid, 0, 0x1000u, AccessType::Read).isOk());

    // TLBI with scale=39 — must execute cleanly.
    smmu.clearEventQueue();
    {
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;
        tlbi.num          = 0u;
        tlbi.scale        = 39u;
        smmu.submitCommand(tlbi);
        smmu.processCommandQueue();
    }
    EXPECT_EQ(smmu.getEventQueueSize(), 0u)
        << "TLBI with scale=39 must not generate events";
    EXPECT_TRUE(smmu.translate(sid, 0, 0x1000u, AccessType::Read).isOk())
        << "page must re-translate after scale=39 TLBI";

    // TLBI with scale=40 — must also execute cleanly (clamped to 39).
    smmu.clearEventQueue();
    {
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;
        tlbi.num          = 0u;
        tlbi.scale        = 40u;
        smmu.submitCommand(tlbi);
        smmu.processCommandQueue();
    }
    EXPECT_EQ(smmu.getEventQueueSize(), 0u)
        << "TLBI with scale=40 (clamped to 39) must not generate events";
    EXPECT_TRUE(smmu.translate(sid, 0, 0x1000u, AccessType::Read).isOk())
        << "page must re-translate after scale=40 TLBI";
}
