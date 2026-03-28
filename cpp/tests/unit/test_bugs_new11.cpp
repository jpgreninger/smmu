// TDD failing tests for NEW-BUG-1, NEW-BUG-2, NEW-BUG-3.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green).
//
// ─────────────────────────────────────────────────────────────────────────────
// NEW-BUG-1 (§4.4.1.1): TG Granule Encoding Swapped — TG=1 and TG=3
//
//   ARM IHI0070G.b §4.4.1.1 table: TG field encoding for RIL TLBI commands:
//     TG=0b00 (0) → no range (use as non-RIL fallback / 4KB in some decodings)
//     TG=0b01 (1) → 4KB granule  (4096 bytes per block)
//     TG=0b10 (2) → 16KB granule (16384 bytes per block)
//     TG=0b11 (3) → 64KB granule (65536 bytes per block)
//
//   Current code (smmu.cpp computeRILRangeEnd):
//     case 1: granule = 64 * 1024  ← WRONG (should be 4KB)
//     case 2: granule = 16 * 1024  ← correct
//     default: granule = 4  * 1024  ← handles tg=0 AND tg=3 as 4KB (tg=3 wrong)
//
//   Test strategy (TG=1 case):
//     Map a stage-1 stream with pages at BASE, BASE+4096, ..., BASE+N*4096.
//     Issue TLBI_NH_VA with ril=true, tg=1 (4KB), num=15, scale=0.
//     With CORRECT encoding (TG=1=4KB): range = (15+1) * 2^0 * 4096 = 64KB
//       → covers BASE through BASE+65535 (16 consecutive 4KB pages).
//       A page at BASE+65536 (= BASE + 64KB) must survive (TLB HIT after TLBI).
//     With BUG (TG=1=64KB): range = (15+1) * 2^0 * 65536 = 1MB
//       → covers BASE through BASE+1048575, so BASE+65536 IS evicted → TLB MISS.
//
//   Test strategy (TG=3 case):
//     Map pages at BASE and BASE+65536.
//     Issue TLBI_NH_VA with ril=true, tg=3 (64KB), num=0, scale=0.
//     With CORRECT encoding (TG=3=64KB): range = 1 * 2^0 * 65536 = 64KB
//       → covers BASE through BASE+65535.
//       A page at BASE+65536 must survive (TLB HIT after TLBI).
//     With BUG (TG=3 falls into default=4KB): range = 1 * 2^0 * 4096 = 4KB
//       → only covers BASE through BASE+4095.
//       The page at BASE+65536 trivially survives, but the page at BASE+4096
//       must be EVICTED (inside 64KB range) — with the bug it is NOT evicted.
//
//   BEFORE FIX: assertions about page survival / eviction fail → red.
//   AFTER FIX:  correct granule used → assertions pass → green.
//
// NEW-BUG-2 (§4.4): Reserved RIL Parameter Combination → CERROR_ILL
//
//   ARM IHI0070G.b §4.4: When TG != 0b00 AND NUM == 0 AND SCALE == 0 AND
//   TTL == 0b00, the combination is Reserved and must raise CERROR_ILL.
//
//   Current code: no check for this reserved combination; command executes as
//   a normal (non-range) TLBI or zero-range TLBI without raising CERROR_ILL.
//
//   BEFORE FIX: getCmdqConsErr() != CERROR_ILL → test FAILS.
//   AFTER FIX:  CERROR_ILL set and GERROR.CMDQ_ERR active → test PASSES.
//
// NEW-BUG-3 (§3.9.1.3): ATSCHK=1 Missing F_TRANSL_FORBIDDEN for Bypass Streams
//
//   ARM IHI0070G.b §3.9.1.3: When transactionType == AtsTranslated AND
//   CR0.ATSCHK == 1, the SMMU MUST verify the pre-translated address against
//   its own translation result.  For a bypass stream (STE.Config==0b100), the
//   "re-translation" returns a trivial identity (PA==IOVA) which always
//   succeeds — so F_TRANSL_FORBIDDEN is never emitted.
//
//   Per §3.9.1.3: "If the SMMU is configured with STE.Config == Bypass, a
//   Translated transaction that passes the ATSCHK check ... should return
//   F_TRANSL_FORBIDDEN."  The check must guard the bypass path and produce
//   F_TRANSL_FORBIDDEN regardless of the re-translation outcome.
//
//   Current code: performTwoStageTranslation() for a bypass stream returns
//   success (PA==IOVA), so the ATSCHK path emits no event and returns success
//   instead of F_TRANSL_FORBIDDEN.
//
//   BEFORE FIX: translate(bypass, AtsTranslated, ATSCHK=1) → succeeds (no event)
//     → test FAILS.
//   AFTER FIX:  → F_TRANSL_FORBIDDEN event recorded and result is error → PASSES.
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Returns true when GERROR.CMDQ_ERR is active.
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

// Check whether any event of the given type is in the event queue.
static bool hasEvent(SMMU& smmu, EventType type) {
    std::vector<EventEntry> events = smmu.getEventQueue();
    for (const auto& ev : events) {
        if (ev.type == type) {
            return true;
        }
    }
    return false;
}

// Set up a stage-1 stream with PASID 0, map pages, and warm the TLB.
static void setupStage1Stream(SMMU& smmu, uint32_t sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);
}

} // anonymous namespace

// ============================================================================
// NEW-BUG-1 (TG=1): TG=1 must mean 4KB, NOT 64KB (ARM §4.4.1.1)
// ============================================================================
//
// ARM §4.4.1.1: TG=0b01 (1) = 4KB granule.
// Current code: case 1: granule = 64*1024 (WRONG — treats TG=1 as 64KB).
//
// With TG=1 and NUM=15, SCALE=0:
//   CORRECT (TG=1=4KB): range = 16 * 1 * 4096 = 65536 bytes (64KB).
//     Page at BASE+65536 is OUTSIDE the range → must survive in TLB.
//   BUG (TG=1=64KB):    range = 16 * 1 * 65536 = 1048576 bytes (1MB).
//     Page at BASE+65536 is INSIDE the 1MB range → evicted → TLB MISS.
//
// BEFORE FIX: page at BASE+65536 is evicted (BUG treats TG=1 as 64KB) → test FAILS.
// AFTER FIX:  page at BASE+65536 survives (correct: 64KB range) → test PASSES.
TEST(BugNew1_TgEncoding, TG1_Is4KB_PageBeyond64KBSurvives) {
    // §4.4.1.1: TG=0b01 encodes 4KB granule.
    // With NUM=15, SCALE=0 the range is exactly 16 * 4KB = 64KB.
    // A page at IOVA BASE+64KB (== BASE+65536) must NOT be invalidated.
    SMMU smmu;
    enableSMMU(smmu);
    smmu.enableCaching(true);

    const uint32_t sid   = 1u;
    const uint64_t kBase = 0x00100000u; // 1MB base to keep addresses clean

    setupStage1Stream(smmu, sid);

    // Map 17 consecutive 4KB pages: BASE, BASE+4K, ..., BASE+16*4K.
    for (int i = 0; i <= 16; ++i) {
        IOVA va = kBase + static_cast<uint64_t>(i) * 4096u;
        PA   pa = 0x00200000u + static_cast<uint64_t>(i) * 4096u;
        smmu.mapPage(sid, 0u, va, pa, PagePermissions(true, true, false));
    }

    // Warm the TLB for all pages.
    for (int i = 0; i <= 16; ++i) {
        IOVA va = kBase + static_cast<uint64_t>(i) * 4096u;
        auto r = smmu.translate(sid, 0u, va, AccessType::Read, SecurityState::NonSecure);
        ASSERT_TRUE(r.isOk()) << "precondition: page " << i << " must translate";
    }

    // Issue RIL TLBI_NH_VA: tg=1 (4KB), num=15, scale=0.
    // Correct range: 16 * 2^0 * 4KB = 64KB = [kBase, kBase+65535].
    // Page at kBase+65536 is at offset 64KB — exactly at range boundary, OUTSIDE.
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = sid;
    cmd.pasid        = 0u;
    cmd.asid         = 0u;
    cmd.vmid         = 0u;
    cmd.startAddress = kBase;
    cmd.ril          = true;
    cmd.tg           = 1u;   // 4KB (spec). Current code treats this as 64KB (BUG).
    cmd.num          = 15u;  // NUM+1 = 16 blocks
    cmd.scale        = 0u;   // 2^0 = 1 multiplier

    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // After correct TLBI (range = 64KB), the page at kBase+65536 must still be
    // in the TLB (was not evicted).  Verify by checking cache hits increase.
    uint64_t hitsBefore = smmu.getCacheHitCount();
    IOVA vaOutside = kBase + 65536u;  // = BASE + 64KB exactly (first page outside range)
    auto r = smmu.translate(sid, 0u, vaOutside, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r.isOk())
        << "NEW-BUG-1 (TG=1): page at BASE+64KB must still translate after 64KB range TLBI";

    uint64_t hitsAfter = smmu.getCacheHitCount();
    EXPECT_GT(hitsAfter, hitsBefore)
        << "NEW-BUG-1 (TG=1=4KB): page at BASE+64KB must be a TLB HIT after 64KB range TLBI "
           "(ARM §4.4.1.1: TG=0b01 = 4KB granule). "
           "With the bug (TG=1=64KB, range=1MB), this page is wrongly evicted → MISS.";
}

// ============================================================================
// NEW-BUG-1 (TG=3): TG=3 must mean 64KB, NOT 4KB (ARM §4.4.1.1)
// ============================================================================
//
// ARM §4.4.1.1: TG=0b11 (3) = 64KB granule.
// Current code: default case handles tg=3 as 4KB (WRONG — should be 64KB).
//
// With TG=3 and NUM=0, SCALE=0:
//   CORRECT (TG=3=64KB): range = 1 * 1 * 65536 = 65536 bytes (64KB).
//     Page at BASE+4096 is INSIDE the 64KB range → must be EVICTED → TLB MISS.
//   BUG (TG=3=4KB):      range = 1 * 1 * 4096  = 4096 bytes (4KB).
//     Page at BASE+4096 is at the boundary — still should be evicted? Actually
//     range = [BASE, BASE+4095], so BASE+4096 is the next page, which is OUTSIDE
//     the 4KB range → NOT evicted → TLB HIT.
//
// Test: after TLBI(tg=3, num=0, scale=0), translate BASE+4096.
//   CORRECT (64KB range): BASE+4096 was evicted → MISS → re-walk → HIT count unchanged.
//   BUG (4KB range):      BASE+4096 NOT evicted → still a HIT.
//
// BEFORE FIX: BASE+4096 is a TLB HIT (not evicted, range only covered 4KB) → test FAILS.
// AFTER FIX:  BASE+4096 is a TLB MISS (correctly evicted by 64KB range) → test PASSES.
TEST(BugNew1_TgEncoding, TG3_Is64KB_PageAt4KBOffsetEvicted) {
    // §4.4.1.1: TG=0b11 encodes 64KB granule.
    // With NUM=0, SCALE=0 the range is 1 * 1 * 64KB = 64KB.
    // A page at BASE+4096 is inside this 64KB range and must be evicted.
    SMMU smmu;
    enableSMMU(smmu);
    smmu.enableCaching(true);

    const uint32_t sid   = 2u;
    const uint64_t kBase = 0x00200000u;

    setupStage1Stream(smmu, sid);

    // Map page at BASE and page at BASE+4096.
    smmu.mapPage(sid, 0u, kBase,          0x00300000u, PagePermissions(true, true, false));
    smmu.mapPage(sid, 0u, kBase + 4096u,  0x00301000u, PagePermissions(true, true, false));

    // Warm TLB for both pages.
    auto r1 = smmu.translate(sid, 0u, kBase,         AccessType::Read, SecurityState::NonSecure);
    auto r2 = smmu.translate(sid, 0u, kBase + 4096u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "precondition: BASE page must translate";
    ASSERT_TRUE(r2.isOk()) << "precondition: BASE+4096 page must translate";

    // Issue RIL TLBI_NH_VA: tg=3 (64KB), num=0, scale=0, ttl=1.
    // CORRECT range: 1 * 2^0 * 64KB = 64KB = [kBase, kBase+65535].
    // Page at kBase+4096 is inside this range and must be evicted.
    // NOTE: ttl=1 (L1 hint) is used to avoid the Reserved combination
    // tg!=0 AND num==0 AND scale==0 AND ttl==0 (ARM §4.4 BUG-NEW-B fix).
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = sid;
    cmd.pasid        = 0u;
    cmd.asid         = 0u;
    cmd.vmid         = 0u;
    cmd.startAddress = kBase;
    cmd.ril          = true;
    cmd.tg           = 3u;   // 64KB (spec). Current code treats this as 4KB (BUG: default case).
    cmd.num          = 0u;   // NUM+1 = 1 block
    cmd.scale        = 0u;   // 2^0 = 1 multiplier
    cmd.ttl          = 1u;   // TTL=1 (L1 hint) — avoids Reserved combination (tg!=0, num=0, scale=0, ttl=0)

    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // After correct TLBI (range = 64KB), BASE+4096 must have been evicted.
    // Verify: translate again — should be a cache MISS (hit count does NOT increase).
    uint64_t hitsBefore = smmu.getCacheHitCount();
    auto r3 = smmu.translate(sid, 0u, kBase + 4096u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r3.isOk())
        << "NEW-BUG-1 (TG=3): BASE+4096 must still translate (page is mapped)";
    uint64_t hitsAfter = smmu.getCacheHitCount();

    EXPECT_EQ(hitsAfter, hitsBefore)
        << "NEW-BUG-1 (TG=3=64KB): page at BASE+4096 must be a TLB MISS after 64KB TLBI "
           "(ARM §4.4.1.1: TG=0b11 = 64KB granule — BASE+4096 is inside [BASE, BASE+65535]). "
           "With the bug (TG=3=4KB default, range=4KB), BASE+4096 is wrongly NOT evicted → HIT.";
}

// ============================================================================
// NEW-BUG-2: Reserved RIL Parameter Combination → CERROR_ILL (ARM §4.4)
// ============================================================================
//
// ARM IHI0070G.b §4.4: The combination TG!=0 AND NUM==0 AND SCALE==0 AND TTL==0
// is Reserved.  The SMMU must raise CERROR_ILL for this combination.
//
// Current code: no check exists for this reserved combination.  The command
// is treated as a normal (zero-range or granule-based) invalidation.
//
// BEFORE FIX: getCmdqConsErr() == CERROR_NONE (0), GERROR.CMDQ_ERR not set → FAILS.
// AFTER FIX:  getCmdqConsErr() == CERROR_ILL (1), GERROR.CMDQ_ERR set → PASSES.
TEST(BugNew2_ReservedRilParams, TG1_NUM0_SCALE0_TTL0_RaisesCerrorIll) {
    // §4.4: TG=1 (4KB), NUM=0, SCALE=0, TTL=0 with ril=true is Reserved → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    // Clear any pre-existing error state.
    smmu.clearGerror(smmu.getGerror());

    // Build a TLBI_NH_VA command with the reserved RIL combination.
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = 0u;
    cmd.pasid        = 0u;
    cmd.asid         = 0u;
    cmd.vmid         = 0u;
    cmd.startAddress = 0x1000u;
    cmd.ril          = true;
    cmd.tg           = 1u;   // TG != 0b00
    cmd.num          = 0u;   // NUM == 0
    cmd.scale        = 0u;   // SCALE == 0
    cmd.ttl          = 0u;   // TTL == 0b00 → Reserved combination per §4.4

    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // CERROR_ILL must be set in CMDQ_CONS.ERR.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "NEW-BUG-2: TLBI with TG!=0 AND NUM==0 AND SCALE==0 AND TTL==0 must raise "
           "CERROR_ILL (ARM §4.4: Reserved RIL parameter combination). "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();

    // GERROR.CMDQ_ERR must be active.
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "NEW-BUG-2: TLBI reserved RIL combination must set GERROR.CMDQ_ERR (ARM §4.4).";
}

// ============================================================================
// NEW-BUG-3 (§3.9.1.3): ATSCHK=1 + Bypass Stream + AtsTranslated → F_TRANSL_FORBIDDEN
// ============================================================================
//
// ARM IHI0070G.b §3.9.1.3: When transactionType == AtsTranslated AND
// CR0.ATSCHK == 1, the SMMU re-checks the pre-translated address.
// For a bypass stream (STE.Config==0b100), the SMMU cannot validate the
// address (there is no stage-1 or stage-2 table) and must return
// F_TRANSL_FORBIDDEN per §7.3.8.
//
// Current code: Check 2 (ATSCHK) calls performTwoStageTranslation() on the
// bypass stream, which returns the identity mapping (PA=IOVA) as success,
// so F_TRANSL_FORBIDDEN is never emitted and the translate() call succeeds.
//
// BEFORE FIX: translate(bypass, AtsTranslated, ATSCHK=1) returns success
//   (no F_TRANSL_FORBIDDEN event) → test FAILS.
// AFTER FIX:  F_TRANSL_FORBIDDEN event recorded, result is error → test PASSES.
TEST(BugNew3_AtschkBypass, BypassStream_AtschkEnabled_AtsTranslated_EmitsFTranslForbidden) {
    // §3.9.1.3: Bypass stream + ATSCHK=1 + AtsTranslated → F_TRANSL_FORBIDDEN.
    SMMU smmu;
    // Enable SMMU with ATSCHK (bit 4) set.
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN
                | SMMU::CR0_PRIQEN | SMMU::CR0_ATSCHK);

    const uint32_t sid = 10u;

    // Configure stream as bypass (STE.Config==0b100 — identity PA==IOVA).
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    const IOVA kIOVA = 0x5000u;

    // Issue AtsTranslated transaction to a bypass stream with ATSCHK=1.
    // Per §3.9.1.3, this must generate F_TRANSL_FORBIDDEN and abort.
    auto result = smmu.translate(sid, 0u, kIOVA, AccessType::Read,
                                 SecurityState::NonSecure,
                                 TransactionType::AtsTranslated);

    EXPECT_FALSE(result.isOk())
        << "NEW-BUG-3: Bypass stream + ATSCHK=1 + AtsTranslated must FAIL "
           "(ARM §3.9.1.3: F_TRANSL_FORBIDDEN expected)";

    EXPECT_TRUE(hasEvent(smmu, EventType::F_TRANSL_FORBIDDEN))
        << "NEW-BUG-3: Bypass stream + ATSCHK=1 + AtsTranslated must record "
           "F_TRANSL_FORBIDDEN event (ARM §3.9.1.3 / §7.3.8). "
           "Current bug: performTwoStageTranslation() on bypass returns identity "
           "success → ATSCHK check does not emit the event.";
}

// Regression guard: Bypass stream + ATSCHK=0 + AtsTranslated must still succeed.
// Without ATSCHK the bypass path is the normal bypass translation.
// (No F_TRANSL_FORBIDDEN should be emitted for ATSCHK=0.)
TEST(BugNew3_AtschkBypass, BypassStream_AtschkDisabled_AtsTranslated_Succeeds) {
    // §3.9.1.3: Without ATSCHK=1, AtsTranslated on bypass must succeed normally.
    SMMU smmu;
    // Enable SMMU WITHOUT ATSCHK (bit 4 not set).
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);

    const uint32_t sid = 11u;

    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    const IOVA kIOVA = 0x6000u;

    // AtsTranslated on bypass with ATSCHK=0 — no re-check, must succeed.
    auto result = smmu.translate(sid, 0u, kIOVA, AccessType::Read,
                                 SecurityState::NonSecure,
                                 TransactionType::AtsTranslated);

    EXPECT_TRUE(result.isOk())
        << "NEW-BUG-3 regression: Bypass + ATSCHK=0 + AtsTranslated must succeed "
           "(no ATSCHK re-check without bit 4)";

    EXPECT_FALSE(hasEvent(smmu, EventType::F_TRANSL_FORBIDDEN))
        << "NEW-BUG-3 regression: No F_TRANSL_FORBIDDEN expected when ATSCHK=0";
}
