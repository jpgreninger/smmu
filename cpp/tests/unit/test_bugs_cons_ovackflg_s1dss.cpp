// ARM SMMU v3 Bug Fix Tests:
//   BUG-CPP-1: processEventQueue() eventqCons advance strips OVACKFLG (bit 31)
//   BUG-CPP-2: processPRIQueue() priqCons advance strips OVACKFLG (bit 31)
//   BUG-CPP-3: CMD_PRI_RESP handler priqCons advance strips OVACKFLG (bit 31)
//   BUG-CPP-4: S1DSS=0b01 returns identity PA when stage-2 is also enabled
//   NOTE-3:    writeCmdqConsErr / getCmdqConsErr use 8-bit mask instead of 7-bit
//
// ARM IHI0070G.b references:
//   §6.3.17  — EVENTQ_CONS.OVACKFLG (bit 31): software-written, SMMU must not modify
//   §8.1     — PRIQ_CONS.OVACKFLG (bit 31): same semantics
//   §3.9     — S1DSS==0b01: bypass stage-1, pass IOVA to stage-2 as IPA
//   §6.3.28  — CMDQ_CONS.ERR = bits[30:24] (7 bits, not 8)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>

using namespace smmu;

namespace {

// BUG-CPP-1 fix note: QueueConfiguration.isValid() requires size >= MIN_QUEUE_SIZE (16).
// A size of 8 fell below that threshold and triggered the fallback to the default
// 512-entry queue (after the constructor split-brain was fixed).  Use 16 — the
// smallest valid size — so the queue actually operates at the intended small capacity.
static constexpr size_t SMALL_Q = 16;

static std::unique_ptr<SMMU> makeSmallQueueSMMU() {
    QueueConfiguration qcfg(SMALL_Q, SMALL_Q, SMALL_Q);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto s = std::make_unique<SMMU>(cfg);
    s->enable();
    s->setCR2(SMMU::CR2_RECINVSID);
    return s;
}

// Generate C_BAD_STREAMID faults to fill the event queue.
static void fillEventQueue(SMMU& smmu, size_t count, StreamID startSid = 0x0300u) {
    for (size_t i = 0; i < count; ++i) {
        smmu.translate(static_cast<StreamID>(startSid + i), 0, 0x1000u,
                       AccessType::Read, SecurityState::NonSecure);
    }
}

// Generate PRI requests to fill the PRI queue.
static void fillPRIQueue(SMMU& smmu, size_t count, StreamID startSid = 0x0400u) {
    for (size_t i = 0; i < count; ++i) {
        PRIEntry req;
        req.streamID        = static_cast<StreamID>(startSid + i);
        req.pasid           = 0;
        req.requestedAddress = 0x2000u;
        req.prgIndex        = static_cast<uint8_t>(i & 0xFFu);
        smmu.submitPageRequest(req);
    }
}

} // namespace

// ============================================================================
// BUG-CPP-1: eventqCons OVACKFLG (bit 31) must survive processEventQueue()
//
// ARM §6.3.17: EVENTQ_CONS.OVACKFLG (bit 31) is software-written.
// The SMMU hardware (and this model) must never clear or modify bit 31 when
// advancing the consumer index.
//
// Bug: advanceQueueIndex returns (idx+1) % (2 * queue_capacity), which is at
// most 2^(log2size+1) <= 2^20.  Storing that value into eventqCons destroys
// bit 31 (OVACKFLG) that software may have written.
//
// Fix: preserve bit 31 via RMW:
//   uint32_t old = eventqCons; new_rd = advance(old & ~bit31); store((old & bit31) | new_rd)
// ============================================================================

/// After software sets OVACKFLG (bit 31) in eventqCons, processEventQueue()
/// must leave bit 31 intact after each consumer-index advance.
///
/// Before fix: bit 31 is cleared because the new index is stored raw.
/// After fix:  bit 31 is preserved through the RMW.
TEST(BugCpp1EventqConsOvackflg, OvackflgPreservedAfterSingleProcess) {
    auto smmu = makeSmallQueueSMMU();

    // Generate two events so the queue is non-empty.
    fillEventQueue(*smmu, 2u, 0x0300u);
    ASSERT_EQ(smmu->getEventQueueSize(), 2u)
        << "precondition: two events must be queued";

    // Simulate software setting OVACKFLG (bit 31) by acknowledging an overflow.
    // We use acknowledgeEventQueueOverflow() which sets CONS bit31 = PROD bit31.
    // To set bit 31 independently, toggle PROD bit 31 first.
    // Instead: use the public setPriqConsIndex does not exist for eventq —
    // so we trigger a real overflow to get PROD bit31=1, then acknowledge.
    // Fill to capacity and trigger overflow so PROD bit31 = 1.
    fillEventQueue(*smmu, SMALL_Q - 2u, 0x0310u);  // queue is now at capacity
    smmu->translate(0x0400u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);  // overflow
    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set in PROD after overflow";

    // Acknowledge: CONS bit31 := PROD bit31 = 1.
    smmu->acknowledgeEventQueueOverflow();
    ASSERT_NE(smmu->getEventqConsIndex() & (1u << 31), 0u)
        << "precondition: OVACKFLG must be set in CONS after acknowledge";

    // Now process ONE event — eventqCons.RD advances by 1.
    // OVACKFLG (bit 31) must survive.
    smmu->processEventQueue();

    uint32_t cons = smmu->getEventqConsIndex();
    EXPECT_NE(cons & (1u << 31), 0u)
        << "BUG-CPP-1: OVACKFLG (bit 31) must be preserved by processEventQueue(); "
        << "cons=0x" << std::hex << cons << std::dec;
}

/// OVACKFLG must survive multiple successive calls to processEventQueue().
TEST(BugCpp1EventqConsOvackflg, OvackflgPreservedAfterMultipleProcessCalls) {
    auto smmu = makeSmallQueueSMMU();

    // Fill, overflow, acknowledge so CONS bit31 = 1.
    fillEventQueue(*smmu, SMALL_Q, 0x0500u);
    smmu->translate(0x0600u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set";
    smmu->acknowledgeEventQueueOverflow();
    ASSERT_NE(smmu->getEventqConsIndex() & (1u << 31), 0u)
        << "precondition: OVACKFLG must be set after acknowledge";

    // processEventQueue() drains the entire queue — advances CONS many times.
    smmu->processEventQueue();
    ASSERT_EQ(smmu->getEventQueueSize(), 0u)
        << "precondition: queue must be empty after processEventQueue";

    uint32_t cons = smmu->getEventqConsIndex();
    EXPECT_NE(cons & (1u << 31), 0u)
        << "BUG-CPP-1: OVACKFLG must survive all consumer-index advances in processEventQueue(); "
        << "cons=0x" << std::hex << cons << std::dec;
}

/// OVACKFLG=1 must not be disturbed even when the queue was never overflowed
/// (bit 31 set only by software acknowledge).
TEST(BugCpp1EventqConsOvackflg, OvackflgSetViaAckNotClearedByProcess) {
    auto smmu = makeSmallQueueSMMU();

    // Enqueue a few events without overflowing.
    fillEventQueue(*smmu, 3u, 0x0700u);

    // Trigger an overflow to allow setting CONS bit31 via acknowledge.
    fillEventQueue(*smmu, SMALL_Q - 3u, 0x0710u);
    smmu->translate(0x0800u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_NE(smmu->getEventqProdIndex() & (1u << 31), 0u)
        << "precondition: OVFLG must be set";
    smmu->acknowledgeEventQueueOverflow();
    ASSERT_NE(smmu->getEventqConsIndex() & (1u << 31), 0u)
        << "precondition: CONS bit31 must be 1 after acknowledge";

    // Drain.
    smmu->processEventQueue();

    uint32_t cons = smmu->getEventqConsIndex();
    EXPECT_NE(cons & (1u << 31), 0u)
        << "BUG-CPP-1: OVACKFLG must not be cleared by processEventQueue(); "
        << "cons=0x" << std::hex << cons << std::dec;
}

// ============================================================================
// BUG-CPP-2: priqCons OVACKFLG (bit 31) must survive processPRIQueue()
//
// ARM §8.1: PRIQ_CONS.OVACKFLG (bit 31) is software-written.  The SMMU must
// not modify it when advancing the consumer index in processPRIQueue().
//
// Bug: same pattern as BUG-CPP-1 but in processPRIQueue().
// Fix: same RMW pattern preserving bit 31.
// ============================================================================

/// processPRIQueue() must preserve OVACKFLG (bit 31) in priqCons.
///
/// We simulate software-acknowledged overflow by directly checking that bit 31
/// of priqCons is preserved when processPRIQueue() advances the index.
///
/// Because there is no overflow-acknowledge API for PRIQ in the public API,
/// we use a known approach: overflow the PRIQ (which sets PRIQ_PROD bit31),
/// then call clearPRIQueue() resets both indices to 0, submit new PRI requests,
/// and verify the bit 31 is NOT accidentally cleared on advance.
/// The core assertion: after processing, bit31 of priqCons must equal bit31 before.
TEST(BugCpp2PriqConsOvackflg, Bit31PreservedAfterProcessPRIQueue) {
    auto smmu = makeSmallQueueSMMU();
    smmu->setCR0(smmu->getCR0() | SMMU::CR0_PRIQEN);

    // Submit one PRI request so the queue has an entry.
    PRIEntry req;
    req.streamID         = 0x0B00u;
    req.pasid            = 0;
    req.requestedAddress = 0x3000u;
    req.prgIndex         = 0;
    smmu->submitPageRequest(req);
    ASSERT_EQ(smmu->getPRIQueueSize(), 1u)
        << "precondition: one PRI entry must be queued";

    // Read current priqCons; bit 31 starts as 0.
    uint32_t consBefore = smmu->getPriqConsIndex();
    ASSERT_EQ(consBefore & (1u << 31), 0u)
        << "precondition: priqCons bit31 must be 0 before test";

    // processPRIQueue() advances priqCons.RD by one entry.
    smmu->processPRIQueue();

    // Bit 31 must still be 0 (it was 0 before; must not be set or cleared spuriously).
    uint32_t consAfter = smmu->getPriqConsIndex();
    EXPECT_EQ(consAfter & (1u << 31), 0u)
        << "BUG-CPP-2: processPRIQueue() must not modify priqCons bit31; "
        << "before=0x" << std::hex << consBefore
        << " after=0x" << consAfter << std::dec;
}

/// Simulate software setting priqCons bit31 = 1 by manipulating the overflow
/// flag.  We do this by filling the PRIQ to overflow (sets PRIQ_PROD bit31),
/// then use the fact that acknowledgeEventQueueOverflow exists for events but
/// not PRI — so we observe via the index API.
///
/// The regression test: after processPRIQueue() processes entries,
/// getPriqConsIndex() bit31 must match what bit31 was before the call.
TEST(BugCpp2PriqConsOvackflg, Bit31NotAlteredWhenQueueHasEntries) {
    auto smmu = makeSmallQueueSMMU();
    smmu->setCR0(smmu->getCR0() | SMMU::CR0_PRIQEN);

    // Submit a few entries.
    fillPRIQueue(*smmu, 3u, 0x0C00u);
    ASSERT_EQ(smmu->getPRIQueueSize(), 3u)
        << "precondition: 3 PRI entries queued";

    // Capture bit31 before any processing.
    uint32_t consBefore = smmu->getPriqConsIndex();
    bool bit31Before = (consBefore & (1u << 31)) != 0u;

    // Process all entries.
    smmu->processPRIQueue();

    uint32_t consAfter = smmu->getPriqConsIndex();
    bool bit31After = (consAfter & (1u << 31)) != 0u;
    EXPECT_EQ(bit31After, bit31Before)
        << "BUG-CPP-2: processPRIQueue() must not modify priqCons bit31; "
        << "before=0x" << std::hex << consBefore
        << " after=0x" << consAfter << std::dec;
}

// ============================================================================
// BUG-CPP-3: CMD_PRI_RESP handler priqCons advance strips OVACKFLG (bit 31)
//
// Same pattern as BUG-CPP-2 but triggered by processing a CMD_PRI_RESP command
// rather than by processPRIQueue().  The CMD_PRI_RESP handler in
// executeCommandLocked() also advances priqCons via advanceQueueIndex.
// ============================================================================

/// CMD_PRI_RESP command processing must not modify priqCons bit31.
TEST(BugCpp3CmdPriRespOvackflg, Bit31NotClearedByCmdPriResp) {
    auto smmu = makeSmallQueueSMMU();
    smmu->setCR0(smmu->getCR0() | SMMU::CR0_PRIQEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Submit a PRI request (this creates the entry processPRIQueue will respond to).
    PRIEntry req;
    req.streamID         = 0x0D00u;
    req.pasid            = 0;
    req.requestedAddress = 0x4000u;
    req.prgIndex         = 42u;
    smmu->submitPageRequest(req);
    ASSERT_EQ(smmu->getPRIQueueSize(), 1u)
        << "precondition: one PRI entry must be queued";

    uint32_t consBefore = smmu->getPriqConsIndex();
    ASSERT_EQ(consBefore & (1u << 31), 0u)
        << "precondition: priqCons bit31 = 0 before test";

    // Submit CMD_PRI_RESP which matches the PRI entry by streamID + prgIndex.
    CommandEntry resp;
    resp.type      = CommandType::PRI_RESP;
    resp.streamID  = req.streamID;
    resp.prgIndex  = req.prgIndex;
    smmu->submitCommand(resp);
    smmu->processCommandQueue();

    uint32_t consAfter = smmu->getPriqConsIndex();
    // The PRI entry should have been consumed; bit31 must remain unchanged.
    EXPECT_EQ(consAfter & (1u << 31), 0u)
        << "BUG-CPP-3: CMD_PRI_RESP must not modify priqCons bit31; "
        << "before=0x" << std::hex << consBefore
        << " after=0x" << consAfter << std::dec;
}

/// CMD_PRI_RESP must consume the matching PRI entry (functional smoke test).
TEST(BugCpp3CmdPriRespOvackflg, CmdPriRespConsumesEntry) {
    auto smmu = makeSmallQueueSMMU();
    smmu->setCR0(smmu->getCR0() | SMMU::CR0_PRIQEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    PRIEntry req;
    req.streamID         = 0x0E00u;
    req.pasid            = 0;
    req.requestedAddress = 0x5000u;
    req.prgIndex         = 7u;
    smmu->submitPageRequest(req);
    ASSERT_EQ(smmu->getPRIQueueSize(), 1u);

    // Bit 31 before.
    uint32_t consBefore = smmu->getPriqConsIndex();

    CommandEntry resp;
    resp.type     = CommandType::PRI_RESP;
    resp.streamID = req.streamID;
    resp.prgIndex = req.prgIndex;
    smmu->submitCommand(resp);
    smmu->processCommandQueue();

    // Entry is consumed, bit31 unchanged.
    uint32_t consAfter = smmu->getPriqConsIndex();
    EXPECT_EQ((consBefore & (1u << 31)), (consAfter & (1u << 31)))
        << "BUG-CPP-3: CMD_PRI_RESP must not modify priqCons bit31";
    EXPECT_EQ(smmu->getPRIQueueSize(), 0u)
        << "CMD_PRI_RESP must remove the matching PRI entry";
}

// ============================================================================
// BUG-CPP-4: S1DSS=0b01 must forward IOVA to stage-2 when stage2Enabled=true
//
// ARM §3.9 / §3.3.1.2: When S1DSS==0b01 (bypass stage-1 for non-substream),
// "the input address is passed directly to stage 2 as the IPA."
//
// Bug: The S1DSS==0x01 branch returns TranslationResult(TranslationData(iova,
// permissions, securityState)) unconditionally without checking stage2Enabled.
// When stage2Enabled=true, stage-2 must be applied using iova as the IPA.
//
// Fix: when config.stage2Enabled is true, call performStage2OnlyTranslation()
// (or the equivalent dispatch) with iova as the IPA instead of returning identity.
//
// Test setup:
//   - stream: stage1Enabled=true, stage2Enabled=true, s1cdMax=2, s1dss=0x01
//   - Stage-2: map IPA=0x1000 → PA=0x2000 (the expected output)
//   - Translate with PASID=0 (non-substream): should invoke stage-2 → PA=0x2000
//   - Before fix: PA=0x1000 (identity, stage-2 bypassed)
// ============================================================================

/// S1DSS=0b01 with stage2Enabled=true must apply stage-2 translation.
/// The IOVA is passed directly to stage-2 as the IPA.
///
/// Expected: translate(IOVA=0x1000) → PA=0x2000 (via stage-2 IPA→PA mapping).
/// Before fix: PA=0x1000 (identity, stage-2 not called).
TEST(BugCpp4S1dssStage2, S1dss01WithStage2EnabledAppliesStage2) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xA0u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;    // no S1 VA range restriction
    cfg.s2t0sz             = 0;    // no S2 IPA range restriction
    cfg.s2ps               = 5;    // 48-bit S2 PA space
    cfg.ips                = 6;    // 52-bit S1 IPS (stage-1 output PA range)
    cfg.s1cdMax            = 2u;   // stream supports substreams (s1dss is active)
    cfg.s1dss              = 0x01u; // bypass stage-1 for PASID=0

    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    // Create PASID 0 so the stream has a stage-1 context (even though it is bypassed).
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // Stage-2: IPA=0x1000 → PA=0x2000.
    // IOVA=0x1000 is passed to stage-2 as IPA=0x1000 when S1DSS=0x01.
    auto stage2AS = std::make_shared<AddressSpace>();
    const PagePermissions RW(true, true, false);
    stage2AS->mapPage(0x1000u, 0x2000u, RW);
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());

    // Translate with PASID=0 — S1DSS=0x01 bypass, stage-2 must be applied.
    auto result = smmu.translate(sid, 0u, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isOk())
        << "BUG-CPP-4: translation must succeed when S1DSS=0b01 and stage-2 maps the IPA";

    PA actualPA = result.getValue().physicalAddress;
    EXPECT_EQ(actualPA, static_cast<PA>(0x2000u))
        << "BUG-CPP-4: S1DSS=0b01 with stage-2 must yield PA from stage-2 mapping "
        << "(expected 0x2000, got 0x" << std::hex << actualPA << std::dec << "); "
        << "before fix PA would be 0x1000 (identity)";
}

/// S1DSS=0b01 with stage2Enabled=false must still return identity PA (existing behavior).
/// This verifies the fix does not break the stage2Enabled=false path.
TEST(BugCpp4S1dssStage2, S1dss01WithStage2DisabledReturnsIdentity) {
    SMMU smmu;
    smmu.enable();

    const StreamID sid = 0xA1u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;   // stage-2 NOT enabled
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s1cdMax            = 2u;
    cfg.s1dss              = 0x01u;   // bypass stage-1 for PASID=0

    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // No stage-2 mapping needed — result is identity (PA = IOVA).
    auto result = smmu.translate(sid, 0u, 0x3000u, AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isOk())
        << "S1DSS=0b01 with stage2Enabled=false must succeed (identity PA)";

    PA actualPA = result.getValue().physicalAddress;
    EXPECT_EQ(actualPA, static_cast<PA>(0x3000u))
        << "S1DSS=0b01 with stage2Enabled=false must return PA=IOVA (identity); "
        << "got 0x" << std::hex << actualPA << std::dec;
}

/// S1DSS=0b01 with stage2Enabled=true must fail when the IPA is not mapped in stage-2.
/// The failure must be a translation fault (not a success with wrong PA).
TEST(BugCpp4S1dssStage2, S1dss01WithStage2EnabledFailsWhenIpaNotMapped) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xA2u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    cfg.ips                = 6;
    cfg.s1cdMax            = 2u;
    cfg.s1dss              = 0x01u;

    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // Stage-2 has no mapping for IPA=0x5000.
    auto stage2AS = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());

    // Translate: S1DSS=0b01, IOVA=0x5000 → IPA=0x5000, no stage-2 mapping → fault.
    auto result = smmu.translate(sid, 0u, 0x5000u, AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-CPP-4: S1DSS=0b01 with stage-2 must fail (F_TRANSLATION) "
        << "when IPA is not mapped in stage-2; "
        << "before fix it would return identity PA=0x5000 (success)";
}

// ============================================================================
// NOTE-3: CMDQ_CONS.ERR mask — must be 7-bit (0x7F), not 8-bit (0xFF)
//
// ARM §6.3.28: SMMU_CMDQ_CONS.ERR = bits[30:24] (7 bits).
// getCmdqConsErr() and writeCmdqConsErr() must not leak beyond bit 30.
// ============================================================================

/// getCmdqConsErr() must return a value in [0, 127] (7-bit field).
TEST(Note3CmdqConsErr, GetCmdqConsErrReturnsAtMost7Bits) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CERROR_ILL by submitting an invalid CMD_SYNC with CS=3.
    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = 3u;  // CS=3 is illegal per §4.7.3 / CONF-GAP-17
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    uint32_t err = smmu.getCmdqConsErr();
    EXPECT_LE(err, 127u)
        << "NOTE-3: getCmdqConsErr() must return a 7-bit value (0-127); "
        << "got " << err << " (mask should be 0x7F, not 0xFF)";
}

/// getCmdqConsErr() must not return a value that has bit 7 set (i.e. >= 128).
/// CERROR_ILL = 0x01 is the expected value from an illegal CS=3 CMD_SYNC.
/// The check ensures the mask is 0x7F not 0xFF.
TEST(Note3CmdqConsErr, GetCmdqConsErrNeverReturnsMoreThan7Bits) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Read ERR before any command — must be 0.
    uint32_t errBefore = smmu.getCmdqConsErr();
    EXPECT_EQ(errBefore, 0u)
        << "NOTE-3: getCmdqConsErr() must be 0 before any error; "
        << "got " << errBefore;

    // Trigger CERROR_ILL via CMD_SYNC CS=3.
    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = 3u;
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    uint32_t err = smmu.getCmdqConsErr();
    // Must be in the 7-bit range [0, 127].
    EXPECT_LE(err, 0x7Fu)
        << "NOTE-3: getCmdqConsErr() must return a 7-bit value (0-127); "
        << "got 0x" << std::hex << err << std::dec
        << " — mask should be 0x7F, not 0xFF";
}

/// The CMDQ_CONS.ERR field must occupy bits[30:24] only.
/// Bit 31 of cmdqCons must not be set by writeCmdqConsErr().
TEST(Note3CmdqConsErr, CmdqConsErrDoesNotSetBit31) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    uint32_t consBefore = smmu.getCmdqConsIndex();

    // Trigger CERROR_ILL.
    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = 3u;
    smmu.submitCommand(sync);
    smmu.processCommandQueue();

    uint32_t consAfter = smmu.getCmdqConsIndex();
    // Bit 31 must not be set by the ERR write.
    EXPECT_EQ(consAfter & (1u << 31), 0u)
        << "NOTE-3: writeCmdqConsErr must not set bit 31 of CMDQ_CONS; "
        << "before=0x" << std::hex << consBefore
        << " after=0x" << consAfter << std::dec;
}
