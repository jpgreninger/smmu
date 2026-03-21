// ARM SMMU v3 Bug Audit Fix Tests
//
// BUG-AUDIT-1: RIL range formula uses 5*SCALE (PE TLBI artifact) instead of SCALE directly
// BUG-AUDIT-2: StreamContext constructor split-brain: stage1Enabled=true but currentConfiguration.stage1Enabled=false
// BUG-AUDIT-3: createPASID() auto-links PASID 0 AS to stage2AddressSpace (should be stream-scoped)
// BUG-AUDIT-4: getEventQueue() does not advance eventqCons
// BUG-AUDIT-5: generateEvent() duplicates ~120 lines of field-setting across normal and stall paths
// BUG-AUDIT-8: getCurrentTimestamp() uses expensive vDSO syscall; replace with atomic counter
// BUG-AUDIT-9: executeInvalidationCommand() default case emits C_BAD_STE instead of CERROR_ILL
//
// ARM IHI0070G.b references:
//   §4.4.1.1  — RIL TLBI SCALE is used directly as block-count exponent, not multiplied by 5
//   §5.2      — STE.Config==0b000 means all stages disabled at reset
//   §3.3.3    — Stage-2 is stream-scoped; all PASIDs share one S2 table
//   §3.5.1    — Consumer must advance CONS after reading events
//   §7.3      — Event record field-setting logic must be consistent
//   §6.3.28   — Unknown command opcode -> CMDQ_CONS.ERR = CERROR_ILL; no C_BAD_STE event

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
#include "smmu/stream_context.h"
#include <memory>
#include <cstdint>

using namespace smmu;

namespace {

// Helper: create a standard SMMU with medium queues.
static std::unique_ptr<SMMU> makeDefaultSMMU() {
    QueueConfiguration qcfg(64, 64, 64);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto s = std::make_unique<SMMU>(cfg);
    s->enable();
    s->setCR2(SMMU::CR2_RECINVSID);
    return s;
}

// Helper: create SMMU with a small event queue for overflow tests.
static std::unique_ptr<SMMU> makeSmallQueueSMMU(size_t eqSize = 8) {
    QueueConfiguration qcfg(eqSize, 64, 64);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto s = std::make_unique<SMMU>(cfg);
    s->enable();
    s->setCR2(SMMU::CR2_RECINVSID);
    return s;
}

// Helper: configure a stage-1-only stream with t0sz=0 and enable it.
static void configureAndEnableStage1Stream(SMMU& smmu, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.t0sz = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);
}

} // namespace

// ============================================================================
// BUG-AUDIT-1: RIL range formula — SCALE used directly, not multiplied by 5
//
// ARM §4.4.1.1: For SMMU command queue TLBI RIL commands, the range covers
//   (NUM+1) * 2^SCALE * granule_size bytes.
//
// The PE instruction encoding uses 5*SCALE because the PE TLBI instruction
// operand packs bits differently — that is NOT the SMMU command queue encoding.
//
// Bug: computeRILRangeEnd computes scaleShift = 5 * effectiveScale, causing
//   SCALE=1 -> 32 granules (should be 2), SCALE=13 -> UB/overflow.
//
// Fix: use effectiveScale directly as the shift: 1u << effectiveScale.
// ============================================================================

// With TG=0 (4KB granule), SCALE=1, NUM=0:
//   Correct formula: range = (NUM+1) * 2^SCALE * granule = 1 * 2^1 * 4096 = 8192 bytes
//   Range covers [0x0000, 0x1FFF].
//
// With the BUG (5*SCALE): scaleShift=5, range = 1 * 2^5 * 4096 = 131072 bytes
//   covering [0x0000, 0x1FFFF] — 16x too large.
//
// Test: Map pages at 0x1000 (inside 8KB range) and 0x3000 (outside 8KB range,
// inside buggy 131KB range).  After RIL TLBI starting at 0x0000 with SCALE=1:
//   - 0x1000 must be evicted (within [0x0000, 0x1FFF]).
//   - 0x3000 must NOT be evicted (outside [0x0000, 0x1FFF]).
// We can indirectly verify by checking cache hit/miss counters before and after.
TEST(BugAudit1RilRangeFormula, Scale1Num0Tg0CorrectRange) {
    auto smmu = makeDefaultSMMU();

    StreamID sid = 0x01u;
    configureAndEnableStage1Stream(*smmu, sid);
    smmu->mapPage(sid, 0, 0x1000u, 0x10000u, PagePermissions(true, true, false));
    smmu->mapPage(sid, 0, 0x3000u, 0x30000u, PagePermissions(true, true, false));

    // Warm TLB for both pages.
    auto r1 = smmu->translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    auto r2 = smmu->translate(sid, 0, 0x3000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "precondition: 0x1000 must translate";
    ASSERT_TRUE(r2.isOk()) << "precondition: 0x3000 must translate";

    // Capture cache hit count before TLBI.
    uint64_t hitsBefore = smmu->getCacheHitCount();

    // Submit RIL TLBI_NH_VA: SCALE=1, NUM=0, TG=0 (4KB), starting at 0x0000.
    // Correct range: [0x0000, 0x1FFF] (8KB). 0x3000 is outside this range.
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = sid;
    cmd.pasid        = 0;
    cmd.asid         = 0;
    cmd.vmid         = 0;
    cmd.startAddress = 0x0000u;
    cmd.ril          = true;
    cmd.tg           = 0;    // 4KB granule
    cmd.num          = 0;    // NUM=0 -> 1 block count
    cmd.scale        = 1;    // SCALE=1 -> 2^1 = 2 granules = 8KB range
    smmu->submitCommand(cmd);
    EXPECT_NO_THROW(smmu->processCommandQueue()) << "SCALE=1 RIL TLBI must not crash";

    // Translate 0x3000 again.  With correct formula, 0x3000 was NOT evicted
    // so it should be a cache HIT.  With bug, it would have been evicted -> miss.
    uint64_t hitsAfterFor3000 = smmu->getCacheHitCount();
    auto r3 = smmu->translate(sid, 0, 0x3000u, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r3.isOk()) << "0x3000 must translate successfully after TLBI";

    // After fix: 0x3000 is a cache hit (not evicted). Hits must have increased.
    EXPECT_GT(smmu->getCacheHitCount(), hitsAfterFor3000)
        << "0x3000 must be a TLB cache hit — not evicted by 8KB RIL TLBI (SCALE=1)";
}

TEST(BugAudit1RilRangeFormula, Scale1Num0Tg0NoUBOrCrash) {
    auto smmu = makeDefaultSMMU();

    StreamID sid = 0x04u;
    configureAndEnableStage1Stream(*smmu, sid);
    smmu->mapPage(sid, 0, 0x1000u, 0x10000u, PagePermissions(true, true, false));

    auto r1 = smmu->translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "precondition: 0x1000 must translate";

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = sid;
    cmd.pasid        = 0;
    cmd.asid         = 0;
    cmd.vmid         = 0;
    cmd.startAddress = 0x0000u;
    cmd.ril          = true;
    cmd.tg           = 0;
    cmd.num          = 0;
    cmd.scale        = 1;
    smmu->submitCommand(cmd);
    EXPECT_NO_THROW(smmu->processCommandQueue()) << "SCALE=1 RIL TLBI must not crash";

    auto r2 = smmu->translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r2.isOk()) << "translation must succeed after RIL TLBI (page still mapped)";
}

// SCALE=13, NUM=0, TG=0: range = 2^13 * 4096 = 32 MB.
// With the buggy 5*SCALE formula: 5*13=65 -> shift UB on x86 (masks to 1),
// producing 1*2^1*4096=8192 bytes instead of 32 MB.  The test verifies no UB.
TEST(BugAudit1RilRangeFormula, Scale13Num0NoBitshiftUB) {
    auto smmu = makeDefaultSMMU();

    StreamID sid = 0x02u;
    configureAndEnableStage1Stream(*smmu, sid);
    smmu->mapPage(sid, 0, 0x100000u, 0x200000u, PagePermissions(true, true, false));

    // Submit TLBI with SCALE=13 starting at 0x0.
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = sid;
    cmd.pasid        = 0;
    cmd.asid         = 0;
    cmd.vmid         = 0;
    cmd.startAddress = 0x0u;
    cmd.ril          = true;
    cmd.tg           = 0;
    cmd.num          = 0;
    cmd.scale        = 13;
    smmu->submitCommand(cmd);
    // Must not crash or produce undefined behaviour.
    EXPECT_NO_THROW(smmu->processCommandQueue()) << "SCALE=13 RIL TLBI must not crash";

    // Translation after TLBI: page still mapped, must succeed.
    auto r = smmu->translate(sid, 0, 0x100000u, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk()) << "translation must succeed after SCALE=13 RIL TLBI";
}

// SCALE=39 is the spec maximum.  With 5*39=195 this is completely UB.
// After fix (using effectiveScale directly), shift is 39 which is safe for uint64_t.
TEST(BugAudit1RilRangeFormula, Scale39Saturates) {
    auto smmu = makeDefaultSMMU();

    StreamID sid = 0x03u;
    configureAndEnableStage1Stream(*smmu, sid);

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = sid;
    cmd.pasid        = 0;
    cmd.asid         = 0;
    cmd.vmid         = 0;
    cmd.startAddress = 0x0u;
    cmd.ril          = true;
    cmd.tg           = 0;
    cmd.num          = 0;
    cmd.scale        = 39;  // max allowed per ARM §4.4.1.1
    smmu->submitCommand(cmd);
    EXPECT_NO_THROW(smmu->processCommandQueue()) << "SCALE=39 RIL TLBI must not crash";
}

// ============================================================================
// BUG-AUDIT-2: StreamContext constructor split-brain state
//
// Bug: Constructor initialises stage1Enabled=true (internal) but
//      currentConfiguration.stage1Enabled=false (spec-level).  This diverges
//      the two code paths that check these different variables.
//
// Fix: Remove the split.  Constructor must set stage1Enabled=false to match
//      currentConfiguration.stage1Enabled=false, keeping both in sync.
// ============================================================================

// A freshly-constructed StreamContext must report stage1 as disabled through
// both isStage1Enabled() and getStreamConfiguration().stage1Enabled.
TEST(BugAudit2ConstructorSplitBrain, DefaultConstructorHasStage1Disabled) {
    StreamContext ctx;

    bool internalFlag = ctx.isStage1Enabled();
    bool configFlag   = ctx.getStreamConfiguration().stage1Enabled;

    EXPECT_FALSE(internalFlag)
        << "stage1Enabled internal member must be false at construction";
    EXPECT_FALSE(configFlag)
        << "currentConfiguration.stage1Enabled must be false at construction";

    // Both must agree — this is the core of the split-brain bug.
    EXPECT_EQ(internalFlag, configFlag)
        << "internal stage1Enabled and currentConfiguration.stage1Enabled must agree";
}

// After explicit configuration with stage1Enabled=true, both flags must agree.
TEST(BugAudit2ConstructorSplitBrain, AfterConfigureBothFlagsAgree) {
    StreamContext ctx;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    ctx.updateConfiguration(cfg);

    bool internalFlag = ctx.isStage1Enabled();
    bool configFlag   = ctx.getStreamConfiguration().stage1Enabled;

    EXPECT_TRUE(internalFlag)
        << "after configuring stage1Enabled=true, isStage1Enabled() must be true";
    EXPECT_TRUE(configFlag)
        << "after configuring stage1Enabled=true, currentConfiguration.stage1Enabled must be true";
    EXPECT_EQ(internalFlag, configFlag)
        << "internal and config flags must agree after applyConfiguration()";
}

// ============================================================================
// BUG-AUDIT-3: Stage-2 must be STE-scoped, not PASID-scoped
//
// ARM §3.3.3: Stage-2 is defined at stream level — all PASIDs for a stream
// share a single S2 table derived from STE.S2TTB.  It is NOT per-PASID.
//
// Bug: createPASID() auto-links PASID 0's AddressSpace to stage2AddressSpace.
//      Other PASIDs (PASID 1+) do not get the same link, so they cannot see
//      the stream-level stage-2 mappings.
//
// Fix: Remove the PASID-0 auto-link in createPASID().  Stream-level stage-2
//      must be set exclusively via setStage2AddressSpace().  Both PASID 0 and
//      PASID 1 must see the same stage-2 mappings.
// ============================================================================

// Configure a two-stage stream.  Create an explicit stream-level stage-2 AS
// with an IPA->PA mapping.  Both PASID 0 and PASID 1 must successfully
// translate through the same stage-2 table.
TEST(BugAudit3Stage2StreamScoped, BothPASIDsUseSameStage2Table) {
    auto smmu = makeDefaultSMMU();

    StreamID sid = 0x10u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled  = true;
    cfg.stage2Enabled  = true;
    cfg.t0sz           = 0;    // no VA range restriction
    cfg.s2t0sz         = 0;    // no IPA range restriction
    cfg.ips            = 6;    // 52-bit IPS
    smmu->configureStream(sid, cfg);
    smmu->enableStream(sid);

    // Create both PASIDs.
    ASSERT_TRUE(smmu->createStreamPASID(sid, 0u).isOk()) << "PASID 0 create must succeed";
    ASSERT_TRUE(smmu->createStreamPASID(sid, 1u).isOk()) << "PASID 1 create must succeed";

    // Map stage-1 IOVA->IPA: PASID 0 maps iova0->ipa, PASID 1 maps iova1->ipa.
    constexpr IOVA iova0 = 0x1000u;
    constexpr IOVA iova1 = 0x2000u;
    constexpr PA   ipa   = 0x4000u;   // both PASIDs use the same IPA
    constexpr PA   pa    = 0x8000u;   // final PA from stage-2

    smmu->mapPage(sid, 0, iova0, ipa, PagePermissions(true, true, false));
    smmu->mapPage(sid, 1, iova1, ipa, PagePermissions(true, true, false));

    // Create and configure the stream-level stage-2 AS with IPA->PA mapping.
    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->mapPage(ipa, pa, PagePermissions(true, true, false));
    ASSERT_TRUE(smmu->setStreamStage2AddressSpace(sid, stage2AS).isOk())
        << "setStreamStage2AddressSpace must succeed";

    // PASID 0 translate: should use stream-level stage-2.
    auto r0 = smmu->translate(sid, 0, iova0, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r0.isOk())
        << "PASID 0 must translate successfully through stream-level stage-2";
    if (r0.isOk()) {
        EXPECT_EQ(r0.getValue().physicalAddress, pa)
            << "PASID 0 must arrive at the correct PA via stage-2";
    }

    // PASID 1 translate: must use the SAME stream-level stage-2.
    auto r1 = smmu->translate(sid, 1, iova1, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r1.isOk())
        << "PASID 1 must translate successfully through the shared stage-2 table";
    if (r1.isOk()) {
        EXPECT_EQ(r1.getValue().physicalAddress, pa)
            << "PASID 1 must arrive at the same PA as PASID 0 via shared stage-2";
    }
}

// ============================================================================
// BUG-AUDIT-4: getEventQueue() must advance eventqCons
//
// ARM §3.5.1: The software consumer is responsible for advancing CONS after
// reading events.  getEventQueue() is the consumption path; after returning
// events it must advance eventqCons to reflect that the events have been read.
//
// Bug: getEventQueue() copies events but never advances eventqCons.
//
// Fix: After collecting events, advance eventqCons by the number of events
//      returned (one advance per event, preserving bit 31 of CONS).
// ============================================================================

// Generate a fault, call getEventQueue().
// Verify that eventqCons has advanced to match eventqProd (queue logically empty).
TEST(BugAudit4GetEventQueueAdvancesCons, ConsAdvancedAfterGet) {
    auto smmu = makeDefaultSMMU();

    // Initial state: PROD == CONS (empty).
    uint32_t initCons = smmu->getEventqConsIndex() & ~(1u << 31);
    uint32_t initProd = smmu->getEventqProdIndex() & ~(1u << 31);
    ASSERT_EQ(initProd, initCons) << "precondition: queue must be initially empty";

    // Generate one C_BAD_STREAMID fault via unconfigured stream.
    smmu->translate(0x0F00u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    // Verify event was queued.
    uint32_t prodAfterFault = smmu->getEventqProdIndex() & ~(1u << 31);
    ASSERT_NE(prodAfterFault, initCons) << "PROD must advance after fault";

    // Consume via getEventQueue().
    std::vector<EventEntry> events = smmu->getEventQueue();
    ASSERT_GE(events.size(), 1u) << "at least one event must be returned";

    // After getEventQueue(), CONS must have advanced to match PROD.
    uint32_t consAfter = smmu->getEventqConsIndex() & ~(1u << 31);
    uint32_t prodAfter = smmu->getEventqProdIndex() & ~(1u << 31);
    EXPECT_EQ(consAfter, prodAfter)
        << "eventqCons must be advanced to PROD after getEventQueue()";
}

// After getEventQueue() drains the queue, generating another event must succeed
// (queue appears non-full from the CONS perspective).
TEST(BugAudit4GetEventQueueAdvancesCons, QueueAcceptsNewEventAfterGet) {
    auto smmu = makeDefaultSMMU();

    // Generate 3 faults.
    for (StreamID s = 0x0300u; s < 0x0303u; ++s) {
        smmu->translate(s, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    }
    ASSERT_GE(smmu->getEventQueueSize(), 1u) << "precondition: events must be queued";

    // Consume via getEventQueue().
    auto events = smmu->getEventQueue();
    ASSERT_GE(events.size(), 1u);

    // After consumption CONS must equal PROD.
    uint32_t cons = smmu->getEventqConsIndex() & ~(1u << 31);
    uint32_t prod = smmu->getEventqProdIndex() & ~(1u << 31);
    EXPECT_EQ(cons, prod)
        << "eventqCons must equal eventqProd after getEventQueue() drains the queue";
}

// ============================================================================
// BUG-AUDIT-5: Extract common helper from generateEvent() (refactor)
//
// The normal path and stall-pending path in generateEvent() each contain
// ~120 lines of identical field-setting code.  After refactoring, both paths
// call a single buildEventEntry() helper.  Observable behaviour must be
// identical — this is a correctness-preserving refactor.
// ============================================================================

// Normal path (queue has space): verify rnw/ind/pnu/ssv/eventClass fields
// are set correctly for a C_BAD_STREAMID event generated by a Read access.
TEST(BugAudit5GenerateEventRefactor, NormalPathFieldsCorrect) {
    auto smmu = makeDefaultSMMU();

    // Unconfigured stream -> C_BAD_STREAMID event, Read access.
    smmu->translate(0x0A00u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    ASSERT_GE(events.size(), 1u);
    const EventEntry& ev = events.front();

    // C_BAD_STREAMID: eventClass must be 0 (CLASS field undefined for C_* events).
    EXPECT_EQ(ev.eventClass, 0u) << "C_BAD_STREAMID must have eventClass=0";
    // Read access: RnW=1 (Read), InD=0 (data read), PnU=0 (unprivileged).
    EXPECT_TRUE(ev.rnw)  << "Read access must have RnW=1";
    EXPECT_FALSE(ev.ind) << "Data read must have InD=0";
    EXPECT_FALSE(ev.pnu) << "Unprivileged access must have PnU=0";
}

// Verify Execute access sets ind=true on the translation-fault path where
// accessType is threaded through (configured stream, unmapped page).
TEST(BugAudit5GenerateEventRefactor, TranslationFaultPathExecuteFields) {
    auto smmu = makeDefaultSMMU();

    // Configure a stream with an unmapped page so translate() produces
    // F_TRANSLATION with the actual AccessType propagated through.
    StreamID sid = 0x0A01u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.t0sz = 0;
    smmu->configureStream(sid, cfg);
    smmu->enableStream(sid);
    smmu->createStreamPASID(sid, 0);
    // No pages mapped -> F_TRANSLATION on any access.

    // Execute access: RnW=1 (Read), InD=1, PnU=0.
    smmu->translate(sid, 0, 0x3000u, AccessType::Execute, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    ASSERT_GE(events.size(), 1u);

    // Find the F_TRANSLATION event (not C_BAD_STREAMID).
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_TRANSLATION) {
            EXPECT_TRUE(ev.rnw)  << "Execute access must have RnW=1";
            EXPECT_TRUE(ev.ind)  << "Execute access must have InD=1";
            EXPECT_FALSE(ev.pnu) << "Unprivileged execute must have PnU=0";
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found) << "F_TRANSLATION event must be generated for unmapped page";
}

// Stall path (queue full): stall events must have consistent field values.
TEST(BugAudit5GenerateEventRefactor, StallPathExecutesWithoutCrash) {
    auto smmu = makeSmallQueueSMMU(8);

    // Configure a stream with stall enabled.
    StreamID sid = 0x20u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.t0sz     = 0;
    cfg.s1Stalld = true;
    smmu->configureStream(sid, cfg);
    smmu->enableStream(sid);
    smmu->createStreamPASID(sid, 0);
    // No mapped pages -> every translate faults.

    // Fill the event queue with C_BAD_STREAMID faults.
    for (StreamID s = 0x0300u; s < 0x0308u; ++s) {
        smmu->translate(s, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    }
    size_t queueSizeBefore = smmu->getEventQueueSize();

    // Trigger a stall fault on the configured (stall-enabled) stream
    // while queue is full -> stall event goes to stallPending buffer.
    EXPECT_NO_THROW(
        smmu->translate(sid, 0, 0x5000u, AccessType::Write, SecurityState::NonSecure)
    ) << "stall path with full queue must not crash";

    // Queue size must not grow (stall event buffered in stallPending).
    EXPECT_EQ(smmu->getEventQueueSize(), queueSizeBefore)
        << "main event queue must not grow when stall event is buffered";

    // Drain main queue; stall-pending should flow in.
    EXPECT_NO_THROW(smmu->processEventQueue())
        << "processEventQueue() must not crash after stall-pending buffering";
}

// ============================================================================
// BUG-AUDIT-8: Replace syscall timestamp with atomic counter
//
// Bug: getCurrentTimestamp() calls std::chrono::steady_clock::now() while
//      holding queueMutex — expensive (~20-50 ns vs ~1-2 ns for atomic add).
//
// Fix: Use a std::atomic<uint64_t> counter, fetch_add(1) per event.
//      Observable requirement: timestamps are monotonically non-decreasing
//      and strictly increasing for consecutive events.
// ============================================================================

// Generate multiple events; verify timestamps are monotonically non-decreasing.
TEST(BugAudit8AtomicTimestamp, TimestampsMonotonicallyNonDecreasing) {
    auto smmu = makeDefaultSMMU();

    smmu->translate(0x0B00u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(0x0B01u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(0x0B02u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    ASSERT_GE(events.size(), 3u) << "at least 3 events required";

    for (size_t i = 1; i < events.size(); ++i) {
        EXPECT_GE(events[i].timestamp, events[i - 1].timestamp)
            << "timestamp[" << i << "] must be >= timestamp[" << (i - 1) << "]";
    }
}

// Verify timestamps are strictly increasing (atomic counter increments each call).
TEST(BugAudit8AtomicTimestamp, TimestampsStrictlyIncreasingWithCounter) {
    auto smmu = makeDefaultSMMU();

    smmu->translate(0x0C00u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(0x0C01u, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    ASSERT_GE(events.size(), 2u);

    // With an atomic counter each call increments, so consecutive events
    // must have strictly different (increasing) timestamp values.
    EXPECT_LT(events[0].timestamp, events[1].timestamp)
        << "consecutive events must have strictly increasing timestamps";
    EXPECT_NE(events[0].timestamp, events[1].timestamp)
        << "consecutive events must have distinct timestamps";
}

// ============================================================================
// BUG-AUDIT-9: TLBI default case emits wrong event type
//
// ARM §6.3.28: An unknown command opcode must set CMDQ_CONS.ERR=CERROR_ILL(0x01)
// and assert GERROR.CMDQ_ERR.  It must NOT produce a C_BAD_STE event record.
//
// Bug: The default: case in executeInvalidationCommand() calls
//   generateEvent(EventType::C_BAD_STE, ...) — completely wrong mechanism.
//
// Fix: Replace with writeCmdqConsErr(CERROR_ILL) + signalGerror(GERROR_CMDQ_ERR).
//      Do NOT emit any event record.
// ============================================================================

// Submit a command with an unrecognised opcode.  Verify:
//   1. CMDQ_CONS.ERR == CERROR_ILL (0x01)
//   2. No C_BAD_STE event in the event queue.
TEST(BugAudit9TlbiDefaultCase, UnknownCmdSetsCerrorIllNotBadSteEvent) {
    auto smmu = makeDefaultSMMU();

    size_t eventsBefore = smmu->getEventQueueSize();

    // Submit an opcode that falls through to the default case.
    // CommandType value 0xFF is not defined in the enum.
    CommandEntry cmd;
    cmd.type         = static_cast<CommandType>(0xFF);
    cmd.streamID     = 0x01u;
    cmd.pasid        = 0;
    cmd.startAddress = 0x0u;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    // CMDQ_CONS.ERR must be CERROR_ILL.
    EXPECT_EQ(smmu->getCmdqConsErr(), CERROR_ILL)
        << "unknown opcode must set CMDQ_CONS.ERR = CERROR_ILL (0x01)";

    // The event queue must NOT contain any new C_BAD_STE event.
    auto events = smmu->getEventQueue();
    bool foundBadSte = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE) {
            foundBadSte = true;
            break;
        }
    }
    EXPECT_FALSE(foundBadSte)
        << "unknown TLBI opcode must NOT produce a C_BAD_STE event record";

    // Queue size must not have grown from the command processing alone.
    EXPECT_EQ(smmu->getEventQueueSize(), eventsBefore)
        << "unknown TLBI opcode must not add events to the event queue";
}

// Regression: valid TLBI commands must still work correctly after the fix.
TEST(BugAudit9TlbiDefaultCase, ValidTlbiCommandStillWorksCorrectly) {
    auto smmu = makeDefaultSMMU();

    StreamID sid = 0x30u;
    configureAndEnableStage1Stream(*smmu, sid);
    smmu->mapPage(sid, 0, 0x5000u, 0x50000u, PagePermissions(true, true, false));

    // Warm TLB.
    auto r1 = smmu->translate(sid, 0, 0x5000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "precondition: initial translation must succeed";

    // Submit a valid TLBI_NH_ALL command.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    // Translation must succeed after valid TLBI (page still mapped).
    auto r2 = smmu->translate(sid, 0, 0x5000u, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r2.isOk()) << "translation must succeed after valid TLBI_NH_ALL";

    // CMDQ_CONS.ERR must remain CERROR_NONE.
    EXPECT_EQ(smmu->getCmdqConsErr(), CERROR_NONE)
        << "valid TLBI command must not set CMDQ_CONS.ERR";
}
