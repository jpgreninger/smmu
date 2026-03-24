// Tests for bugs fixed on 2026-03-21 (debugger audit follow-up).
// TDD: all tests are written to FAIL before their respective fixes are applied.
//
// BUG-CPP-1 (Constructor split-brain):
//   SMMU(const SMMUConfiguration&) initializer-list sets maxEventQueueSize,
//   maxCommandQueueSize, maxPRIQueueSize, cmdqLog2Size, eventqLog2Size, priqLog2Size
//   from the incoming config.  When config.isValid()==false the body overwrites
//   this->configuration with SMMUConfiguration::createDefault(), but the six
//   sizing members are NOT recomputed.  They therefore remain inconsistent with
//   the fallback configuration.
//   Fix: after the fallback assignment recompute all six members from configuration.
//
// BUG-CPP-5 (C_BAD_STE/STREAMID/CD have rnw=1 — ARM §7.3.1 RES0):
//   generateEvent() runs the accessType switch for ALL event types including
//   configuration-class events (C_BAD_STREAMID, C_BAD_STE, C_BAD_SUBSTREAMID,
//   C_BAD_CD, F_CFG_CONFLICT).  For those events the RnW, InD, PnU fields are
//   RES0 (must be zero) per ARM IHI0070G.b §7.3.1.  The switch sets them from
//   the default AccessType::Read case (rnw=1).
//   Fix: after the switch block in BOTH paths (stall-pending + normal) add a
//   RES0 override for configuration-class event types.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helper utilities
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Configure and enable a stage-1-only stream (no PASID map needed for fault tests).
static void configureStage1Stream(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0);
}

} // namespace

// ============================================================================
// BUG-CPP-1: Constructor split-brain — invalid config leaves sizing members
//            inconsistent with the fallback SMMUConfiguration::createDefault().
//
// When SMMU(const SMMUConfiguration& config) receives an invalid config it falls
// back to the default, but the six queue-sizing members (maxEventQueueSize,
// maxCommandQueueSize, maxPRIQueueSize, cmdqLog2Size, eventqLog2Size,
// priqLog2Size) still hold the values from the original invalid config.
//
// These tests FAIL before the fix because the log2 sizes do not match the
// default configuration's queue sizes.
// ============================================================================

// Test 1: eventqLog2Size must match the default queue size after invalid config fallback.
//
// Default eventQueueSize = 512 → log2(512) = 9.
// An invalid config uses queue sizes < MIN_QUEUE_SIZE (4); using size=3 (log2=2)
// makes the mismatch observable (default log2 = 9 for eventQueue=512).
TEST(BugCpp1ConstructorSplitBrain, EventqLog2MatchesDefaultAfterInvalidConfig) {
    // Build a config that fails isValid(): queue sizes below the minimum (4).
    QueueConfiguration badQueue(/*eventSize=*/3, /*commandSize=*/3, /*priSize=*/3);
    ASSERT_FALSE(badQueue.isValid()) << "precondition: badQueue must be invalid";

    CacheConfiguration  defaultCache;
    AddressConfiguration defaultAddr;
    ResourceLimits       defaultLimits;
    SMMUConfiguration    badCfg(badQueue, defaultCache, defaultAddr, defaultLimits);
    ASSERT_FALSE(badCfg.isValid()) << "precondition: badCfg must be invalid";

    SMMU smmu(badCfg);

    // Compute expected log2 from the default queue config.
    SMMUConfiguration def = SMMUConfiguration::createDefault();
    const size_t defEventQSize = def.getQueueConfiguration().eventQueueSize; // 512

    // computeLog2Size(512) == 9.
    uint32_t expectedLog2 = 0;
    size_t n = 1;
    while (n < defEventQSize) { n <<= 1; ++expectedLog2; }

    EXPECT_EQ(smmu.getEventqLog2Size(), expectedLog2)
        << "BUG-CPP-1: after invalid-config fallback, eventqLog2Size must equal "
           "log2(default.eventQueueSize)=" << expectedLog2
        << " but it equals " << smmu.getEventqLog2Size()
        << " (still set from the invalid config's queue size=1 → log2=0)";
}

// Test 2: cmdqLog2Size must match the default queue size after invalid config fallback.
//
// Default commandQueueSize = 256 → log2(256) = 8.
TEST(BugCpp1ConstructorSplitBrain, CmdqLog2MatchesDefaultAfterInvalidConfig) {
    QueueConfiguration badQueue(3, 3, 3); // 3 < MIN_QUEUE_SIZE (4) — invalid
    CacheConfiguration  defaultCache;
    AddressConfiguration defaultAddr;
    ResourceLimits       defaultLimits;
    SMMUConfiguration    badCfg(badQueue, defaultCache, defaultAddr, defaultLimits);
    ASSERT_FALSE(badCfg.isValid()) << "precondition: badCfg must be invalid";

    SMMU smmu(badCfg);

    SMMUConfiguration def = SMMUConfiguration::createDefault();
    const size_t defCmdQSize = def.getQueueConfiguration().commandQueueSize; // 256

    uint32_t expectedLog2 = 0;
    size_t n = 1;
    while (n < defCmdQSize) { n <<= 1; ++expectedLog2; }

    EXPECT_EQ(smmu.getCmdqLog2Size(), expectedLog2)
        << "BUG-CPP-1: after invalid-config fallback, cmdqLog2Size must equal "
           "log2(default.commandQueueSize)=" << expectedLog2
        << " but it equals " << smmu.getCmdqLog2Size()
        << " (still set from the invalid config's queue size=1 → log2=0)";
}

// Test 3: With a valid (default) config the sizing members must be consistent
//         (regression guard — must pass both before and after the fix).
TEST(BugCpp1ConstructorSplitBrain, ValidConfigSizingIsConsistent) {
    SMMUConfiguration validCfg = SMMUConfiguration::createDefault();
    ASSERT_TRUE(validCfg.isValid()) << "precondition: default config must be valid";

    SMMU smmu(validCfg);

    SMMUConfiguration def = SMMUConfiguration::createDefault();
    const size_t defEventQSize   = def.getQueueConfiguration().eventQueueSize;
    const size_t defCmdQSize     = def.getQueueConfiguration().commandQueueSize;

    auto log2Of = [](size_t cap) -> uint32_t {
        if (cap <= 1) return 0;
        uint32_t k = 0; size_t n = 1;
        while (n < cap) { n <<= 1; ++k; }
        return k;
    };

    EXPECT_EQ(smmu.getEventqLog2Size(), log2Of(defEventQSize))
        << "Valid config: eventqLog2Size must match log2(eventQueueSize)";
    EXPECT_EQ(smmu.getCmdqLog2Size(),   log2Of(defCmdQSize))
        << "Valid config: cmdqLog2Size must match log2(commandQueueSize)";
}

// ============================================================================
// BUG-CPP-5: C_BAD_STE / C_BAD_STREAMID / C_BAD_CD have rnw=1 (ARM §7.3.1 RES0).
//
// ARM IHI0070G.b §7.3.1: For configuration-class events (C_BAD_STREAMID,
// C_BAD_STE, C_BAD_SUBSTREAMID, C_BAD_CD, F_CFG_CONFLICT) the RnW, InD, and
// PnU fields are RES0 and must be written as 0.  The current generateEvent()
// runs the accessType switch for ALL event types; the default AccessType::Read
// case sets rnw=1 so config events are emitted with rnw=1 instead of 0.
//
// These tests FAIL before the fix because rnw, ind, pnu are non-zero for
// configuration-class events.
// ============================================================================

// Test 4: C_BAD_STREAMID event must have rnw=false, ind=false, pnu=false.
//
// To trigger C_BAD_STREAMID: use a StreamID that exceeds the stream-table log2
// size bound.  The default strtabLog2Size_=32 is very permissive; we call
// setStreamTableSize() to set it to a small value and then translate with a
// StreamID >= 2^size.
//
// Alternative approach: enable SMMU, DO NOT configure stream 0x9999, then
// translate — this hits C_BAD_STREAMID when RECINVSID=1 (CR2 bit 1).
TEST(BugCpp5ConfigEventRnWIsRES0, CBadStreamIDHasRnWIndPnuZero) {
    SMMU smmu;
    enableSMMU(smmu);

    // Enable RECINVSID so C_BAD_STREAMID events are recorded (§6.3.12 CR2 bit 1).
    smmu.setCR2(smmu.getCR2() | (1u << 1));

    // Reduce stream table size so StreamID 0xFF00 is out-of-range (2^8=256 entries).
    smmu.setStrtabLog2Size(8);

    // StreamID 0xFF00 >= 2^8=256 → C_BAD_STREAMID.
    TranslationResult result = smmu.translate(0xFF00u, 0, 0x1000u,
                                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isError())
        << "Translation with out-of-range StreamID must fail with C_BAD_STREAMID";

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_GE(events.size(), 1u) << "At least one event must be in the queue";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr) << "A C_BAD_STREAMID event must be present in the queue";

    // ARM §7.3.1: RnW/InD/PnU are RES0 for configuration-class events.
    EXPECT_FALSE(ev->rnw)
        << "BUG-CPP-5: C_BAD_STREAMID.rnw must be 0 (RES0 per ARM §7.3.1); got 1";
    EXPECT_FALSE(ev->ind)
        << "BUG-CPP-5: C_BAD_STREAMID.ind must be 0 (RES0 per ARM §7.3.1)";
    EXPECT_FALSE(ev->pnu)
        << "BUG-CPP-5: C_BAD_STREAMID.pnu must be 0 (RES0 per ARM §7.3.1)";
}

// Test 5: C_BAD_STE event must have rnw=false, ind=false, pnu=false.
//
// To trigger C_BAD_STE: configure a stream with an illegal STE configuration.
// A stream with translationEnabled=true, stage1Enabled=false, stage2Enabled=false
// uses config 0b100 (bypass).  We need a truly invalid STE config; the easiest
// way is to configure a stream with stage1Enabled=true but aa64=false AND
// translationEnabled=true, which historically triggers C_BAD_STE.  However
// the safest approach (matching other tests) is to use the STRW=EL3+NonSecure
// combination which is guaranteed C_BAD_STE (§5.2.2).
TEST(BugCpp5ConfigEventRnWIsRES0, CBadSTEHasRnWIndPnuZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0x20u;
    // STRW=EL3 on a NonSecure stream is explicitly forbidden per ARM §5.2.2
    // → C_BAD_STE.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = StreamWorld::EL3;
    // securityState left at NonSecure (default) in configureStream.
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);

    // Translate — STRW=EL3 + NonSecure stream → C_BAD_STE.
    TranslationResult result = smmu.translate(sid, 0, 0x1000u,
                                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isError())
        << "STRW=EL3 on NonSecure stream must fail with C_BAD_STE (ARM §5.2.2)";

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_GE(events.size(), 1u) << "At least one event must be in the queue";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_STE) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "A C_BAD_STE event must be present in the queue "
           "(STRW=EL3 + NonSecure stream, ARM §5.2.2)";

    // ARM §7.3.1: RnW/InD/PnU are RES0 for configuration-class events.
    EXPECT_FALSE(ev->rnw)
        << "BUG-CPP-5: C_BAD_STE.rnw must be 0 (RES0 per ARM §7.3.1); got 1";
    EXPECT_FALSE(ev->ind)
        << "BUG-CPP-5: C_BAD_STE.ind must be 0 (RES0 per ARM §7.3.1)";
    EXPECT_FALSE(ev->pnu)
        << "BUG-CPP-5: C_BAD_STE.pnu must be 0 (RES0 per ARM §7.3.1)";
}

// Test 6: Normal fault events (F_TRANSLATION) must still set rnw from access type
//         (regression guard — rnw=true for Read, rnw=false for Write).
TEST(BugCpp5ConfigEventRnWIsRES0, FaultEventsStillSetRnWFromAccessType) {
    SMMU smmu;
    enableSMMU(smmu);
    const StreamID sid = 0x30u;
    configureStage1Stream(smmu, sid);

    // No page mapped → F_TRANSLATION with AccessType::Read → rnw=true.
    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_GE(events.size(), 1u);
    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_TRANSLATION) { ev = &e; break; }
    }
    ASSERT_NE(ev, nullptr) << "F_TRANSLATION event must be present";
    EXPECT_TRUE(ev->rnw)
        << "F_TRANSLATION from Read access must have rnw=true (regression guard)";
}
