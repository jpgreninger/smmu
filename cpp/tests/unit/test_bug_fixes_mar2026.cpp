// ARM SMMU v3 — Bug-fix regression tests (March 2026)
// Copyright (c) 2024 John Greninger
//
// TDD: each test is written BEFORE the corresponding fix so it fails in the
// "red" state.  After the fix it must pass ("green") and the full suite must
// continue to pass.
//
// BUG-1: advanceQueueIndex wipes CMDQ_CONS.ERR (bits [31:24]) on every dequeue.
//   Fix: store back only the RD field; preserve upper bits via RMW.
//
// BUG-2: processCommandQueue checks GERROR.CMDQ_ERR BEFORE acquiring queueMutex.
//   A concurrent signalGerror() between the read and the lock makes the check
//   ineffective.  Fix: move the check inside the lock.
//
// BUG-5: STAG allocator tries only one candidate slot, then falls back to
//   terminate if that slot is occupied.  Fix: scan up to 65535 slots.
//
// BUG-8: TLB fast-path returns privilegedOnly=true for EL2/EL3 STRW streams.
//   Fix: clear privilegedOnly on the TranslationData when strw==EL2 or EL3.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"

using namespace smmu;

// ============================================================================
// BUG-1: advanceQueueIndex must preserve CMDQ_CONS.ERR bits [31:24]
// ============================================================================

// Write a CERROR_ILL into CMDQ_CONS.ERR via an illegal CMD_SYNC (CS=3), then
// submit a valid NOP and call processCommandQueue() again.  Because GERROR is
// now active the second processCommandQueue() is a no-op, so the RD advance
// from the first dequeue is the only store that should have touched cmdqCons.
// The ERR field must still be CERROR_ILL after that advance.
TEST(Bug1CmdqConsErr, ErrFieldPreservedAfterAdvance) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CERROR_ILL by submitting CMD_SYNC with CS=3 (reserved).
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu.submitCommand(illSync).isOk());

    smmu.processCommandQueue();

    // ERR field must be CERROR_ILL (1) after processing the illegal command.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-1: CMDQ_CONS.ERR must be CERROR_ILL after illegal CMD_SYNC";

    // Acknowledge the GERROR so the queue can be processed again.
    smmu.clearGerror(GERROR_CMDQ_ERR);

    // Submit another legal PREFETCH_CONFIG command (no-op) and process it.
    CommandEntry nop;
    nop.type = CommandType::PREFETCH_CONFIG;
    ASSERT_TRUE(smmu.submitCommand(nop).isOk());
    smmu.processCommandQueue();

    // After processing the command the ERR field must still be CERROR_ILL
    // (software has not written CERROR_NONE; the RD advance must not wipe it).
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-1: advanceQueueIndex must NOT clear CMDQ_CONS.ERR on dequeue";
}

// Verify that the ERR field survives repeated RD advances.
TEST(Bug1CmdqConsErr, ErrFieldSurvivesMultipleAdvances) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Set ERR to CERROR_ILL via illegal sync.
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu.submitCommand(illSync).isOk());
    smmu.processCommandQueue();
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL);

    // Acknowledge so we can enqueue more.
    smmu.clearGerror(GERROR_CMDQ_ERR);

    // Submit and process 5 PREFETCH_CONFIG commands; each dequeue advances RD.
    for (int i = 0; i < 5; ++i) {
        CommandEntry nop;
        nop.type = CommandType::PREFETCH_CONFIG;
        ASSERT_TRUE(smmu.submitCommand(nop).isOk());
        smmu.processCommandQueue();
    }

    // ERR must still be CERROR_ILL; never cleared by advanceQueueIndex.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-1: ERR must survive multiple RD advances";
}

// ============================================================================
// BUG-2: GERROR.CMDQ_ERR check must be inside queueMutex
// ============================================================================

// Functional regression: after signalGerror sets CMDQ_ERR, processCommandQueue
// must not process any commands — even the very first one.
// (This test is already partially covered by other tests but we add a focused
// one here for the TOCTOU window specifically.)
TEST(Bug2GerrorToctou, CommandsNotProcessedWhenCmdqErrActive) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR via illegal command.
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu.submitCommand(illSync).isOk());
    smmu.processCommandQueue();

    // GERROR.CMDQ_ERR must now be active.
    EXPECT_NE(smmu.getGerror() & GERROR_CMDQ_ERR, 0u)
        << "BUG-2: GERROR.CMDQ_ERR must be active after illegal command";

    // Record RD index — must NOT advance on the next processCommandQueue call.
    uint32_t rdBefore = smmu.getCmdqConsIndex();

    // Enqueue a PREFETCH_CONFIG (no-op); without the fix it could be consumed
    // despite the active error.
    CommandEntry nop;
    nop.type = CommandType::PREFETCH_CONFIG;
    ASSERT_TRUE(smmu.submitCommand(nop).isOk());

    // processCommandQueue must halt at the CMDQ_ERR check inside the lock.
    smmu.processCommandQueue();

    uint32_t rdAfter = smmu.getCmdqConsIndex();
    EXPECT_EQ(rdBefore, rdAfter)
        << "BUG-2: processCommandQueue must not advance RD when CMDQ_ERR is active";
}

// After clearGerror, commands must be processed again.
TEST(Bug2GerrorToctou, CommandsProcessedAfterClearGerror) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Trigger CMDQ_ERR.
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu.submitCommand(illSync).isOk());
    smmu.processCommandQueue();

    // Acknowledge the error.
    smmu.clearGerror(GERROR_CMDQ_ERR);
    EXPECT_EQ(smmu.getGerror() & GERROR_CMDQ_ERR, 0u);

    uint32_t rdBefore = smmu.getCmdqConsIndex();

    CommandEntry nop;
    nop.type = CommandType::PREFETCH_CONFIG;
    ASSERT_TRUE(smmu.submitCommand(nop).isOk());
    smmu.processCommandQueue();

    uint32_t rdAfter = smmu.getCmdqConsIndex();
    EXPECT_NE(rdBefore, rdAfter)
        << "BUG-2: After clearGerror, processCommandQueue must resume processing";
}

// ============================================================================
// BUG-5: STAG allocator must scan all free slots before falling back to terminate
// ============================================================================

// Set up a stream in stall mode, fill all but one STAG slot, then trigger a
// new stall.  The allocator must find the one remaining free slot rather than
// immediately falling back to terminate mode.
//
// We cannot fill 65534 slots directly (that would require 65534 pending
// transactions) so instead we verify the basic wrap-around scanning behaviour:
// pre-occupy the first N candidate STAGs by manually inserting them into the
// stall queue via real translations, then check that the next stall still gets
// a valid STAG.
//
// A simpler approach that still exercises the scan: trigger two stalls such that
// the second one would collide with the first if only one slot were checked.
// We verify both end up in the stall queue with distinct non-zero STAGs.
TEST(Bug5StagScan, TwoStallsGetDistinctStags) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    // Configure stream with stage-1 in stall mode.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall;
    cfg.strw               = StreamWorld::EL1_EL0;

    ASSERT_TRUE(smmu.configureStream(1u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(1u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(1u, 0u).isOk());

    // Map nothing — translate to unmapped page to cause F_TRANSLATION stall.
    smmu.translate(1u, 0u, 0x1000u, AccessType::Read);
    smmu.translate(1u, 0u, 0x2000u, AccessType::Read);

    // Both transactions must appear in the stall queue.
    auto stalled = smmu.getStalledTransactions();
    ASSERT_EQ(stalled.size(), 2u)
        << "BUG-5: Both faulting translations must produce stall records";

    uint16_t stag0 = stalled[0].stag;
    uint16_t stag1 = stalled[1].stag;

    EXPECT_NE(stag0, 0u) << "BUG-5: STAG must not be 0 (reserved)";
    EXPECT_NE(stag1, 0u) << "BUG-5: STAG must not be 0 (reserved)";
    EXPECT_NE(stag0, stag1) << "BUG-5: Both stalled transactions must get distinct STAGs";
}

// Trigger multiple stalls and verify each gets a distinct non-zero STAG.
// This exercises the "skip already-occupied slot" scan behaviour when the
// counter wraps around or when a previously-seen slot is still occupied.
TEST(Bug5StagScan, ScanSkipsOccupiedSlots) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall;
    cfg.strw               = StreamWorld::EL1_EL0;

    ASSERT_TRUE(smmu.configureStream(2u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(2u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(2u, 0u).isOk());

    // Trigger 5 distinct stalls — none are aborted, so each slot stays occupied.
    static const size_t NUM_STALLS = 5u;
    for (size_t i = 0; i < NUM_STALLS; ++i) {
        IOVA iova = 0x1000u + static_cast<IOVA>(i) * 0x1000u;
        smmu.translate(2u, 0u, iova, AccessType::Read);
    }

    auto stalled = smmu.getStalledTransactions();
    ASSERT_EQ(stalled.size(), NUM_STALLS)
        << "BUG-5: Each faulting translation must produce a stall record";

    // Verify all STAGs are non-zero and mutually distinct.
    for (size_t i = 0; i < stalled.size(); ++i) {
        EXPECT_NE(stalled[i].stag, 0u)
            << "BUG-5: STAG[" << i << "] must not be 0 (reserved)";
        for (size_t j = i + 1; j < stalled.size(); ++j) {
            EXPECT_NE(stalled[i].stag, stalled[j].stag)
                << "BUG-5: STAG[" << i << "] and STAG[" << j << "] must be distinct";
        }
    }
}

// ============================================================================
// BUG-8: TLB fast-path must clear privilegedOnly for EL2/EL3 STRW streams
// ============================================================================

// Helper: configure an SMMU stream with EL2 STRW and a page mapped
// with privilegedOnly=true, warm the TLB via a first translate() call (slow
// path), then translate again (fast path) and check the returned permissions.
static void setupEl2StreamWithPrivPage(SMMU& smmu, StreamID sid) {
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.enableCaching(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.strw               = StreamWorld::EL2;

    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // Map a page as privilegedOnly=true (AP[1]=0 in hardware).
    PagePermissions perms;
    perms.read           = true;
    perms.write          = false;
    perms.execute        = false;
    perms.privilegedOnly = true;

    ASSERT_TRUE(smmu.mapPage(sid, 0u, 0x1000u, 0x8000u, perms).isOk());
}

// Slow-path (first translation, cache miss): EL2 stream, privilegedOnly page.
// The translation must SUCCEED (EL2 ignores AP[1]) and the returned
// permissions must have privilegedOnly=false.
TEST(Bug8PrivilegedOnlyEl2, SlowPathClearPrivilegedOnly) {
    SMMU smmu;
    setupEl2StreamWithPrivPage(smmu, 10u);

    // First translation: slow path (cache cold).
    TranslationResult result = smmu.translate(10u, 0u, 0x1000u, AccessType::Read);

    ASSERT_TRUE(result.isOk())
        << "BUG-8 slow path: EL2 stream must succeed on privilegedOnly page";

    const TranslationData& data = result.getValue();
    EXPECT_FALSE(data.permissions.privilegedOnly)
        << "BUG-8 slow path: privilegedOnly must be false for EL2 STRW after translation";
}

// Fast-path (second translation, cache warm): same as above but the TLB is
// warm from the first call, so the fast-path branch executes.
TEST(Bug8PrivilegedOnlyEl2, FastPathClearPrivilegedOnly) {
    SMMU smmu;
    setupEl2StreamWithPrivPage(smmu, 11u);

    // Warm the TLB.
    TranslationResult warmup = smmu.translate(11u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(warmup.isOk()) << "Warm-up translation failed unexpectedly";

    // Second translation: fast-path TLB hit.
    TranslationResult result = smmu.translate(11u, 0u, 0x1000u, AccessType::Read);

    ASSERT_TRUE(result.isOk())
        << "BUG-8 fast path: EL2 stream must succeed on privilegedOnly page (TLB hit)";

    const TranslationData& data = result.getValue();
    EXPECT_FALSE(data.permissions.privilegedOnly)
        << "BUG-8 fast path: privilegedOnly must be false for EL2 STRW on TLB hit";
}

// EL3 stream — same expectation: privilegedOnly must be cleared.
TEST(Bug8PrivilegedOnlyEl3, FastPathClearPrivilegedOnly) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.enableCaching(true);

    StreamConfig cfg;
    cfg.translationEnabled  = true;
    cfg.stage1Enabled       = true;
    cfg.stage2Enabled       = false;
    cfg.strw                = StreamWorld::EL3;
    cfg.securityState       = SecurityState::Secure;

    ASSERT_TRUE(smmu.configureStream(12u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(12u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(12u, 0u).isOk());

    PagePermissions perms;
    perms.read           = true;
    perms.write          = false;
    perms.execute        = false;
    perms.privilegedOnly = true;

    ASSERT_TRUE(smmu.mapPage(12u, 0u, 0x1000u, 0x9000u, perms, SecurityState::Secure).isOk());

    // Warm the TLB.
    TranslationResult warmup = smmu.translate(12u, 0u, 0x1000u, AccessType::Read, SecurityState::Secure);
    ASSERT_TRUE(warmup.isOk()) << "EL3 warm-up failed";

    // Fast-path hit.
    TranslationResult result = smmu.translate(12u, 0u, 0x1000u, AccessType::Read, SecurityState::Secure);
    ASSERT_TRUE(result.isOk())
        << "BUG-8: EL3 stream must succeed on privilegedOnly page (TLB hit)";

    const TranslationData& data = result.getValue();
    EXPECT_FALSE(data.permissions.privilegedOnly)
        << "BUG-8: privilegedOnly must be false for EL3 STRW on TLB hit";
}

// ============================================================================
// BUG-10: fault priority — F_ADDR_SIZE must precede F_PERMISSION in two-stage
// ============================================================================
//
// ARM IHI0070G.b §7.3 defines fault priority ordering: F_ADDR_SIZE (§7.3.14)
// has higher priority than F_PERMISSION (§7.3.16).  When both conditions hold
// simultaneously in performBothStagesTranslation() — that is, the stage-2
// output PA exceeds 2^S2PS AND the intersected permissions deny the access —
// the SMMU must emit F_ADDR_SIZE, not F_PERMISSION.
//
// Before the fix the permission check ran before the S2PS OAS check, so the
// test below produces F_PERMISSION and FAILS.  After the fix (S2PS check moved
// before the permission check) it must produce F_ADDR_SIZE and PASS.

// Helper fixture: build a two-stage SMMU with a 52-bit OAS so we can map PAs
// above the 32-bit S2PS threshold (s2ps=0 → PA limit = 2^32).
class Bug10FaultPriorityTest : public ::testing::Test {
protected:
    void SetUp() override {
        // 52-bit OAS so the SMMU can physically hold a PA above 2^32.
        SMMUConfiguration smmuCfg;
        AddressConfiguration addrCfg;
        addrCfg.maxPASize = 52u;
        smmuCfg.setAddressConfiguration(addrCfg);
        smmu_ = std::make_unique<SMMU>(smmuCfg);
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// BUG-10: when PA > S2PS limit AND permissions denied, F_ADDR_SIZE must win.
// Before fix: F_PERMISSION fires first.  After fix: F_ADDR_SIZE fires first.
TEST_F(Bug10FaultPriorityTest, AddrSizeTakesPriorityOverPermission) {
    static constexpr StreamID SID  = 50u;
    static constexpr PASID    PID  = 0u;
    static constexpr IOVA     IOVA_VAL = 0x1000u;
    static constexpr IPA      TEST_IPA = 0x2000u;
    // PA is above the 32-bit S2PS limit (s2ps=0 → PA must be < 2^32 = 0x1_0000_0000).
    static constexpr PA       OUT_PA   = (UINT64_C(1) << 32u) + 0x1000u;

    // Two-stage stream; s2ps=0 restricts stage-2 output PA to 32 bits.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = 0u;   // no stage-1 VA restriction
    cfg.s2t0sz             = 0u;   // no IPA range restriction
    cfg.ips                = 6u;   // 52-bit IPS — no IPA output restriction
    cfg.s2ps               = 0u;   // 32-bit S2PS limit — OUT_PA will exceed this

    ASSERT_TRUE(smmu_->configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(SID, PID).isOk());

    // Stage-2 address space with read-only mapping (so a Write access is denied).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(SID, s2as).isOk());

    // Stage-1: map IOVA -> IPA (read+write so S1 does not block the write).
    PagePermissions s1perms;
    s1perms.read  = true;
    s1perms.write = true;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, IOVA_VAL, TEST_IPA, s1perms).isOk());

    // Stage-2: map IPA -> PA (read-only AND PA exceeds S2PS limit).
    PagePermissions s2perms;
    s2perms.read  = true;
    s2perms.write = false;   // write denied → F_PERMISSION if checked first
    ASSERT_TRUE(s2as->mapPage(TEST_IPA, OUT_PA, s2perms).isOk());

    // Perform a Write translation — this hits BOTH violations simultaneously.
    auto r = smmu_->translate(SID, PID, IOVA_VAL, AccessType::Write);
    EXPECT_TRUE(r.isError())
        << "BUG-10: translation with PA > S2PS + write-denied must fail";

    // Inspect the event queue: F_ADDR_SIZE must be present, not just F_PERMISSION.
    auto ev = smmu_->getEventQueue();
    bool has_f_addr_size  = false;
    bool has_f_permission = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE)  { has_f_addr_size  = true; }
        if (e.type == EventType::F_PERMISSION) { has_f_permission = true; }
    }
    EXPECT_TRUE(has_f_addr_size)
        << "BUG-10: §7.3 fault priority — F_ADDR_SIZE must be emitted when PA > S2PS";
    EXPECT_FALSE(has_f_permission)
        << "BUG-10: §7.3 fault priority — F_PERMISSION must NOT fire when F_ADDR_SIZE has priority";
}

// ============================================================================
// EL1_EL0 stream — privilegedOnly must NOT be cleared (privilege checks apply).
TEST(Bug8PrivilegedOnlyEl1El0, FastPathKeepsPrivilegedOnly) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    smmu.enableCaching(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.strw               = StreamWorld::EL1_EL0;

    ASSERT_TRUE(smmu.configureStream(13u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(13u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(13u, 0u).isOk());

    PagePermissions perms;
    perms.read           = true;
    perms.write          = false;
    perms.execute        = false;
    perms.privilegedOnly = true;

    ASSERT_TRUE(smmu.mapPage(13u, 0u, 0x1000u, 0xA000u, perms).isOk());

    // Warm the TLB (use privileged access to succeed on this privileged-only page).
    TranslationResult warmup = smmu.translate(13u, 0u, 0x1000u, AccessType::ReadPrivileged);
    ASSERT_TRUE(warmup.isOk()) << "EL1_EL0 privileged warm-up failed";

    // Fast-path hit with privileged access.
    TranslationResult result = smmu.translate(13u, 0u, 0x1000u, AccessType::ReadPrivileged);
    ASSERT_TRUE(result.isOk())
        << "BUG-8 control: EL1_EL0 privileged access to privileged-only page must succeed";

    const TranslationData& data = result.getValue();
    // For EL1_EL0, privilegedOnly should remain as-is from page table.
    EXPECT_TRUE(data.permissions.privilegedOnly)
        << "BUG-8 control: EL1_EL0 must NOT clear privilegedOnly (privilege checks maintained)";
}

// ============================================================================
// BUG-14: getRecentFaults() must use a closed lower bound [earliestTime, currentTime]
// ============================================================================
//
// ARM SMMU fault management requires that any fault whose timestamp equals the
// exact window start boundary (earliestTime = currentTime - timeWindow) is
// included in the result.  The original implementation used strict '>' which
// excluded the boundary point.
//
// TDD: this test is written BEFORE the fix so it FAILS in the red state.
// The fix changes '>' to '>=' on the lower-bound comparison in
// fault_handler.cpp::getRecentFaults().

#include "smmu/fault_handler.h"

// BUG-14: a fault whose timestamp equals earliestTime must be included.
TEST(Bug14GetRecentFaultsBoundary, FaultAtExactBoundaryIsIncluded) {
    FaultHandler handler;

    // Choose a current time and window so that earliestTime = currentTime - timeWindow.
    const uint64_t currentTime  = 1000u;
    const uint64_t timeWindow   = 500u;
    const uint64_t earliestTime = currentTime - timeWindow;  // 500

    // Inject a fault record whose timestamp is exactly at the boundary.
    FaultRecord boundaryFault;
    boundaryFault.streamID   = 0x1u;
    boundaryFault.pasid      = 0u;
    boundaryFault.address    = 0x1000u;
    boundaryFault.faultType  = FaultType::TranslationFault;
    boundaryFault.accessType = AccessType::Read;
    boundaryFault.timestamp  = earliestTime;   // exactly at the boundary

    handler.recordFault(boundaryFault);

    // Also inject a fault well inside the window and one clearly outside.
    FaultRecord insideFault  = boundaryFault;
    insideFault.address      = 0x2000u;
    insideFault.timestamp    = currentTime - 100u;   // 900 — clearly inside

    FaultRecord outsideFault = boundaryFault;
    outsideFault.address     = 0x3000u;
    outsideFault.timestamp   = earliestTime - 1u;    // 499 — clearly outside

    handler.recordFault(insideFault);
    handler.recordFault(outsideFault);

    auto recent = handler.getRecentFaults(currentTime, timeWindow);

    // The inside fault must always be present.
    bool foundInside   = false;
    bool foundBoundary = false;
    bool foundOutside  = false;
    for (const auto& f : recent) {
        if (f.address == 0x2000u) { foundInside   = true; }
        if (f.address == 0x1000u) { foundBoundary = true; }
        if (f.address == 0x3000u) { foundOutside  = true; }
    }

    EXPECT_TRUE(foundInside)
        << "BUG-14: fault inside the window must always be returned";
    EXPECT_TRUE(foundBoundary)
        << "BUG-14: fault at exact boundary (timestamp == earliestTime) must be included "
           "— closed interval [earliestTime, currentTime] is required";
    EXPECT_FALSE(foundOutside)
        << "BUG-14: fault before the window must not be returned";
}

// BUG-14: a fault at currentTime itself (upper boundary) must be included.
TEST(Bug14GetRecentFaultsBoundary, FaultAtCurrentTimeIsIncluded) {
    FaultHandler handler;

    const uint64_t currentTime = 2000u;
    const uint64_t timeWindow  = 1000u;

    FaultRecord f;
    f.streamID   = 0x2u;
    f.pasid      = 0u;
    f.address    = 0x4000u;
    f.faultType  = FaultType::PermissionFault;
    f.accessType = AccessType::Write;
    f.timestamp  = currentTime;   // exactly at the upper boundary

    handler.recordFault(f);

    auto recent = handler.getRecentFaults(currentTime, timeWindow);

    bool found = false;
    for (const auto& rec : recent) {
        if (rec.address == 0x4000u) { found = true; }
    }
    EXPECT_TRUE(found)
        << "BUG-14: fault at exact currentTime must be returned (upper boundary)";
}
