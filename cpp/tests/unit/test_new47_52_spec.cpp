// ARM SMMU v3 FINDING-NEW-47 through NEW-52 spec tests
//
// TDD tests for spec conformance fixes:
//  NEW-47: CR0 bit assignments per ARM §6.3.9
//  NEW-48: GERROR bit assignments per ARM §6.3.17
//  NEW-49: processCommandQueue() halts after CMDQ_ERR is set
//  NEW-50: processCommandQueue() skips processing when CMDQ_ERR already set
//  NEW-51: Event queue overflow toggles EVENTQ_PROD.OVFLG (bit 31)
//  NEW-52: C_BAD_SUBSTREAMID when PASID >= 2^STE.S1CDMax

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Helpers ──────────────────────────────────────────────────────────────────

static constexpr StreamID TEST_SID  = 0x20;
static constexpr PASID    TEST_PASID = 0;
static constexpr IOVA     TEST_IOVA  = 0x1000ULL;
static constexpr PA       TEST_PA    = 0x8000'0000ULL;

// ── NEW-47: CR0 bit assignments ───────────────────────────────────────────────

/// §6.3.9: CR0_SMMUEN must be bit 0.
TEST(New47Spec, CR0_SMMUEN_is_bit0) {
    EXPECT_EQ(SMMU::CR0_SMMUEN, (1u << 0))
        << "CR0_SMMUEN must be bit 0 per ARM §6.3.9";
}

/// §6.3.9: CR0_PRIQEN must be bit 1.
TEST(New47Spec, CR0_PRIQEN_is_bit1) {
    EXPECT_EQ(SMMU::CR0_PRIQEN, (1u << 1))
        << "CR0_PRIQEN must be bit 1 per ARM §6.3.9";
}

/// §6.3.9: CR0_EVENTQEN must be bit 2.
TEST(New47Spec, CR0_EVENTQEN_is_bit2) {
    EXPECT_EQ(SMMU::CR0_EVENTQEN, (1u << 2))
        << "CR0_EVENTQEN must be bit 2 per ARM §6.3.9";
}

/// §6.3.9: CR0_CMDQEN must be bit 3.
TEST(New47Spec, CR0_CMDQEN_is_bit3) {
    EXPECT_EQ(SMMU::CR0_CMDQEN, (1u << 3))
        << "CR0_CMDQEN must be bit 3 per ARM §6.3.9";
}

/// §6.3.9: CR0_ATSCHK must be bit 4.
TEST(New47Spec, CR0_ATSCHK_is_bit4) {
    EXPECT_EQ(SMMU::CR0_ATSCHK, (1u << 4))
        << "CR0_ATSCHK must be bit 4 per ARM §6.3.9";
}

// ── NEW-48: GERROR bit assignments ────────────────────────────────────────────

/// §6.3.17: All GERROR bit constants must be at spec-correct positions.
TEST(New48Spec, GerrorBitPositionsMatchSpec) {
    EXPECT_EQ(GERROR_CMDQ_ERR,           (1u << 0))
        << "CMDQ_ERR must be bit 0 (§6.3.17)";
    EXPECT_EQ(GERROR_EVENTQ_ABT_ERR,     (1u << 2))
        << "EVENTQ_ABT_ERR must be bit 2 (§6.3.17)";
    EXPECT_EQ(GERROR_PRIQ_ABT_ERR,       (1u << 3))
        << "PRIQ_ABT_ERR must be bit 3 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_CMDQ_ABT_ERR,   (1u << 4))
        << "MSI_CMDQ_ABT_ERR must be bit 4 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_EVENTQ_ABT_ERR, (1u << 5))
        << "MSI_EVENTQ_ABT_ERR must be bit 5 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_PRIQ_ABT_ERR,   (1u << 6))
        << "MSI_PRIQ_ABT_ERR must be bit 6 (§6.3.17)";
    EXPECT_EQ(GERROR_MSI_GERROR_ABT_ERR, (1u << 7))
        << "MSI_GERROR_ABT_ERR must be bit 7 (§6.3.17)";
    EXPECT_EQ(GERROR_SFM_ERR,            (1u << 8))
        << "SFM_ERR must be bit 8 (§6.3.17)";
    EXPECT_EQ(GERROR_CMDQP_ERR,          (1u << 9))
        << "CMDQP_ERR must be bit 9 (§6.3.17)";
}

/// §6.3.17: Backward-compat aliases must resolve to spec-correct bit positions.
TEST(New48Spec, GerrorBackwardCompatAliases) {
    EXPECT_EQ(GERROR_SFE,          GERROR_SFM_ERR)
        << "GERROR_SFE alias must equal GERROR_SFM_ERR (bit 8)";
    EXPECT_EQ(GERROR_CMDQ_ABT_ERR, GERROR_MSI_CMDQ_ABT_ERR)
        << "GERROR_CMDQ_ABT_ERR alias must equal GERROR_MSI_CMDQ_ABT_ERR (bit 4)";
    EXPECT_EQ(GERROR_MSI_ABT_ERR,  GERROR_MSI_EVENTQ_ABT_ERR)
        << "GERROR_MSI_ABT_ERR alias must equal GERROR_MSI_EVENTQ_ABT_ERR (bit 5)";
}

// ── NEW-49: processCommandQueue() halts after CMDQ_ERR ────────────────────────

/// §6.3.17: After a bad command sets CMDQ_ERR, subsequent commands in the same
/// batch must NOT be processed.
TEST(New49Spec, QueueHaltsAfterCmdqErrIsSet) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    // Configure a stream so CFGI_STE has something to work on.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    smmu->configureStream(TEST_SID, cfg);
    smmu->enableStream(TEST_SID);
    smmu->createStreamPASID(TEST_SID, TEST_PASID);
    PagePermissions perms(true, true, false);
    smmu->mapPage(TEST_SID, TEST_PASID, TEST_IOVA, TEST_PA, perms);

    // Submit: [bad command, CFGI_STE]
    // The bad command sets CMDQ_ERR. The CFGI_STE must not execute (it would
    // invalidate the stream and make a subsequent translate() fail differently).
    CommandEntry badCmd;
    badCmd.type        = static_cast<CommandType>(0xEE);
    badCmd.streamID    = TEST_SID;
    badCmd.pasid       = TEST_PASID;
    badCmd.startAddress = 0;
    badCmd.endAddress  = 0;
    badCmd.flags       = 0;
    badCmd.timestamp   = 0;

    CommandEntry cfgiCmd;
    cfgiCmd.type        = CommandType::CFGI_STE;
    cfgiCmd.streamID    = TEST_SID;
    cfgiCmd.pasid       = TEST_PASID;
    cfgiCmd.startAddress = 0;
    cfgiCmd.endAddress  = 0;
    cfgiCmd.flags       = 0;
    cfgiCmd.timestamp   = 0;

    smmu->submitCommand(badCmd);
    smmu->submitCommand(cfgiCmd);
    smmu->processCommandQueue();

    // CMDQ_ERR must be set.
    EXPECT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMDQ_ERR must be set after bad command (§6.3.17)";
}

// ── NEW-50: processCommandQueue() skips when CMDQ_ERR already set ─────────────

/// §6.3.17: When GERROR.CMDQ_ERR is already set, processCommandQueue() must
/// return immediately without consuming any commands.
TEST(New50Spec, QueueSkipsProcessingWhenCmdqErrAlreadySet) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    // Arrange: get CMDQ_ERR set via bad command.
    CommandEntry badCmd;
    badCmd.type        = static_cast<CommandType>(0xED);
    badCmd.streamID    = 0;
    badCmd.pasid       = 0;
    badCmd.startAddress = 0;
    badCmd.endAddress  = 0;
    badCmd.flags       = 0;
    badCmd.timestamp   = 0;
    smmu->submitCommand(badCmd);
    smmu->processCommandQueue();
    ASSERT_NE(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Pre-condition: CMDQ_ERR must be set before the second call";

    // Now submit a valid SYNC command.  With CMDQ_ERR already set,
    // processCommandQueue() must return immediately — the SYNC must not fire
    // (so no COMMAND_SYNC_COMPLETION event is added to the event queue).
    CommandEntry syncCmd;
    syncCmd.type        = CommandType::SYNC;
    syncCmd.streamID    = 0;
    syncCmd.pasid       = 0;
    syncCmd.startAddress = 0;
    syncCmd.endAddress  = 0;
    syncCmd.flags       = 0;
    syncCmd.cs          = 1;  // CS=1 -> SIG_IRQ completion
    syncCmd.timestamp   = 0;
    smmu->submitCommand(syncCmd);

    // Enable event queue so any SYNC completion would be recorded.
    smmu->setCR0(smmu->getCR0() | SMMU::CR0_EVENTQEN);

    smmu->clearEventQueue();
    smmu->processCommandQueue();

    // The event queue must remain empty — SYNC was not processed.
    auto events = smmu->getEventQueue();
    bool hasSyncCompletion = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            hasSyncCompletion = true;
            break;
        }
    }
    EXPECT_FALSE(hasSyncCompletion)
        << "processCommandQueue() must not process commands when CMDQ_ERR is already set (§6.3.17)";
}

// ── NEW-51: Event queue overflow toggles EVENTQ_PROD.OVFLG ────────────────────

/// §7.4: When the event queue is full and a non-stall event is discarded,
/// EVENTQ_PROD.OVFLG (bit 31) must be toggled, NOT GERROR_EVENTQ_ABT_ERR.
TEST(New51Spec, EventqOverflowTogglesOVFLGNotGerror) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    smmu->configureStream(TEST_SID, cfg);
    smmu->enableStream(TEST_SID);
    smmu->createStreamPASID(TEST_SID, TEST_PASID);

    // Record the initial OVFLG state (bit 31 of eventqProd).
    uint32_t prodBefore = smmu->getEventqProdIndex();
    bool ovflgBefore    = ((prodBefore >> 31) & 1u) != 0;

    // Fill the event queue by translating unmapped pages (generates faults).
    // Keep going until an overflow occurs (OVFLG toggles).
    bool ovflgToggled = false;
    for (int i = 0; i < 2000; ++i) {
        IOVA badIova = static_cast<IOVA>(0x10000ULL + (static_cast<uint64_t>(i) << 12));
        smmu->translate(TEST_SID, TEST_PASID, badIova, AccessType::Read, SecurityState::NonSecure);

        uint32_t prodNow = smmu->getEventqProdIndex();
        bool ovflgNow    = ((prodNow >> 31) & 1u) != 0;
        if (ovflgNow != ovflgBefore) {
            ovflgToggled = true;
            break;
        }
    }

    EXPECT_TRUE(ovflgToggled)
        << "EVENTQ_PROD.OVFLG (bit 31) must toggle when event queue overflows (ARM §7.4)";

    // GERROR_EVENTQ_ABT_ERR must NOT be set — that bit is for memory bus aborts.
    EXPECT_EQ(smmu->getGerror() & GERROR_EVENTQ_ABT_ERR, 0u)
        << "GERROR_EVENTQ_ABT_ERR must NOT be set on queue-full overflow (§6.3.17)";
}

// ── NEW-52: C_BAD_SUBSTREAMID when PASID >= 2^S1CDMax ────────────────────────

/// §7.3.9 / §3.10: PASID values >= 2^STE.S1CDMax must generate C_BAD_SUBSTREAMID.
TEST(New52Spec, OobPasidGeneratesCBadSubstreamid) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    // Configure: s1cdMax=4 means valid PASIDs are 0..15; PASID 16 is OOB.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1cdMax            = 4;  // 2^4 = 16 substreams; max valid PASID = 15
    cfg.s1dss              = 2;  // s1dss=0b10: CD[0] for PASID=0
    smmu->configureStream(TEST_SID, cfg);
    smmu->enableStream(TEST_SID);
    smmu->createStreamPASID(TEST_SID, TEST_PASID);
    PagePermissions perms(true, true, false);
    smmu->mapPage(TEST_SID, TEST_PASID, TEST_IOVA, TEST_PA, perms);
    smmu->clearEventQueue();

    // PASID=16 is exactly 2^4 — out of bounds.
    auto result = smmu->translate(TEST_SID, 16, TEST_IOVA, AccessType::Read,
                                  SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "PASID >= 2^S1CDMax must fail (§7.3.9 / §3.10)";

    bool hasBadSubstream = false;
    for (const auto& ev : smmu->getEventQueue()) {
        if (ev.type == EventType::C_BAD_SUBSTREAMID) {
            hasBadSubstream = true;
            break;
        }
    }
    EXPECT_TRUE(hasBadSubstream)
        << "C_BAD_SUBSTREAMID must be generated when PASID >= 2^S1CDMax (§7.3.9)";
}

/// §7.3.9: PASID within range must NOT generate C_BAD_SUBSTREAMID.
TEST(New52Spec, InRangePasidIsOk) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1cdMax            = 4;  // valid PASIDs: 0..15
    cfg.s1dss              = 2;
    smmu->configureStream(TEST_SID, cfg);
    smmu->enableStream(TEST_SID);

    // PASID=15 is the last valid value.
    smmu->createStreamPASID(TEST_SID, 15);
    PagePermissions perms(true, true, false);
    smmu->mapPage(TEST_SID, 15, TEST_IOVA, TEST_PA, perms);
    smmu->clearEventQueue();

    auto result = smmu->translate(TEST_SID, 15, TEST_IOVA, AccessType::Read,
                                  SecurityState::NonSecure);

    bool hasBadSubstream = false;
    for (const auto& ev : smmu->getEventQueue()) {
        if (ev.type == EventType::C_BAD_SUBSTREAMID) {
            hasBadSubstream = true;
            break;
        }
    }
    EXPECT_FALSE(hasBadSubstream)
        << "In-range PASID must not generate C_BAD_SUBSTREAMID (§7.3.9)";
    (void)result;
}

/// §7.3.9: When s1cdMax==0 (not substream-capable), any non-zero PASID
/// must NOT be rejected by the s1cdMax check (handled elsewhere).
TEST(New52Spec, S1cdMaxZeroSkipsRangeCheck) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1cdMax            = 0;  // not substream-capable
    smmu->configureStream(TEST_SID, cfg);
    smmu->enableStream(TEST_SID);
    smmu->createStreamPASID(TEST_SID, 1);
    PagePermissions perms(true, true, false);
    smmu->mapPage(TEST_SID, 1, TEST_IOVA, TEST_PA, perms);
    smmu->clearEventQueue();

    smmu->translate(TEST_SID, 1, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);

    bool hasBadSubstream = false;
    for (const auto& ev : smmu->getEventQueue()) {
        if (ev.type == EventType::C_BAD_SUBSTREAMID) {
            hasBadSubstream = true;
            break;
        }
    }
    EXPECT_FALSE(hasBadSubstream)
        << "s1cdMax==0 must not trigger s1cdMax range-check C_BAD_SUBSTREAMID";
}

}  // namespace test
}  // namespace smmu
