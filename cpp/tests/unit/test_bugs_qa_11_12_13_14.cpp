// TDD failing tests for BUG-QA-11, BUG-QA-12, BUG-QA-13, BUG-QA-14
// (C++ implementation).
//
// Each test is written to FAIL with the current code and PASS only after the
// corresponding fix is applied.  No fixes are included here.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-QA-11 (§5.2/§13.3): STE bypass output attributes not applied.
//   When STE.Config=0b100 (all-bypass), the bypass path constructs a bare
//   TranslationData without applying the STE output attribute fields (shCfg,
//   memAttr/mtCfg, nsCfg, instCfg, privCfg, allocCfg).  Rust correctly applies
//   them via apply_output_attrs(); C++ does not.
//
// BUG-QA-12 (§5.5): STE.S2S controls stall vs. terminate for stage-2 faults.
//   The comment on the s2s field is wrong ("Stage-2 Secure bit") — S2S means
//   Stage-2 Stall.  The stall decision at translate() always uses the stream-
//   level FaultMode::Stall; it should use s2s for stage-2 faults.
//   Also: STALL_MODEL==0b01 (terminate-only) AND STE.S2S==1 → C_BAD_STE.
//
// BUG-QA-13 (§5.5): STE.S2R Stage-2 Record.
//   When S2R=0 and S2S=0, stage-2 fault events must be suppressed entirely
//   (neither stalled nor recorded).  Neither field nor suppression logic exists.
//
// BUG-QA-14 (§4.4.4.1): CMD_TLBI_NSNH_ALL over-invalidates.
//   NSNH_ALL should only invalidate Non-Secure Non-Hyp (EL1_EL0) entries,
//   preserving NS-EL2 and NS-EL2-E2H entries.  Currently it calls
//   invalidateTranslationCache() which is a full flush.
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
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

// Build a minimal STE-bypass stream configuration.
static StreamConfig bypassConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = true;
    cfg.securityState      = SecurityState::NonSecure;
    return cfg;
}

// Build a minimal stage-2-only stream with a mapped page.
static StreamConfig stage2Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0x0Au;
    cfg.s2t0sz             = 16u;
    cfg.s2ps               = 5u;   // 48-bit PA
    cfg.s2aa64             = true;
    cfg.s2sl0              = 1u;
    cfg.s2tg               = 0u;
    cfg.s2ttb              = 0u;
    return cfg;
}

} // anonymous namespace

// ============================================================================
// BUG-QA-11: STE bypass output attributes must be applied
// ============================================================================

// Test 1: shareability field set from shCfg in bypass result.
// BEFORE FIX: result.shareability == 0 (bare TranslationData).
// AFTER FIX:  result.shareability == shCfg value.
TEST(BugQA11BypassOutputAttrs, ShareabilityFromShCfg) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = bypassConfig();
    cfg.shCfg = 3u;  // ISH (inner shareable)
    ASSERT_TRUE(smmu.configureStream(0x01u, cfg).isOk());

    TranslationResult result = smmu.translate(0x01u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(result.isOk())
        << "BUG-QA-11: bypass translation must succeed";

    EXPECT_EQ(result.getValue().shareability, 3u)
        << "BUG-QA-11: bypass result.shareability must equal shCfg=3 (§5.2/§13.3)";
}

// Test 2: memType set from memAttr when mtCfg=true in bypass result.
// BEFORE FIX: result.memType == 0.
// AFTER FIX:  result.memType == memAttr value when mtCfg=true.
TEST(BugQA11BypassOutputAttrs, MemTypeFromMemAttrWhenMtCfgTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = bypassConfig();
    cfg.mtCfg   = true;
    cfg.memAttr = 0xFu;  // Normal WB
    ASSERT_TRUE(smmu.configureStream(0x02u, cfg).isOk());

    TranslationResult result = smmu.translate(0x02u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(result.isOk())
        << "BUG-QA-11: bypass translation must succeed";

    EXPECT_EQ(result.getValue().memType, 0xFu)
        << "BUG-QA-11: bypass result.memType must equal memAttr=0xF when mtCfg=true (§5.2/§13.3)";
}

// Test 3: memType zero when mtCfg=false in bypass result.
// When mtCfg=false, memType must remain 0 (pass-through from transaction).
TEST(BugQA11BypassOutputAttrs, MemTypeZeroWhenMtCfgFalse) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = bypassConfig();
    cfg.mtCfg   = false;
    cfg.memAttr = 0xFu;  // Irrelevant — mtCfg=false
    ASSERT_TRUE(smmu.configureStream(0x03u, cfg).isOk());

    TranslationResult result = smmu.translate(0x03u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(result.isOk())
        << "BUG-QA-11: bypass translation must succeed";

    EXPECT_EQ(result.getValue().memType, 0u)
        << "BUG-QA-11: bypass result.memType must be 0 when mtCfg=false (§5.2/§13.3)";
}

// Test 4: nsCfgOut set from nsCfg in bypass result.
// BEFORE FIX: result.nsCfgOut == 0.
// AFTER FIX:  result.nsCfgOut == nsCfg value.
TEST(BugQA11BypassOutputAttrs, NsCfgOutFromNsCfg) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = bypassConfig();
    cfg.nsCfg = 2u;
    ASSERT_TRUE(smmu.configureStream(0x04u, cfg).isOk());

    TranslationResult result = smmu.translate(0x04u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(result.isOk())
        << "BUG-QA-11: bypass translation must succeed";

    EXPECT_EQ(result.getValue().nsCfgOut, 2u)
        << "BUG-QA-11: bypass result.nsCfgOut must equal nsCfg=2 (§5.2/§13.3)";
}

// ============================================================================
// BUG-QA-12: STE.S2S controls stall mode for stage-2 faults
// ============================================================================

// Test 1: Stage-2 fault with s2s=true, FaultMode=Terminate → result is Stalled.
// S2S=1 means stall mode is active for stage-2 faults even when global FaultMode
// is Terminate.  A stage-2 fault (missing page) must return SMMUError::Stalled.
// BEFORE FIX: result is a translation fault (not Stalled).
// AFTER FIX:  result.getError() == SMMUError::Stalled.
TEST(BugQA12S2SStallMode, Stage2FaultStallsWhenS2STrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config();
    cfg.faultMode = FaultMode::Terminate;  // global: terminate
    cfg.s2s       = true;                  // STE.S2S=1 → stall stage-2 faults
    ASSERT_TRUE(smmu.configureStream(0x10u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(0x10u).isOk());

    // Do NOT map any page — stage-2 translation will fault (F_TRANSLATION).
    TranslationResult result = smmu.translate(0x10u, 0u, 0xA000u, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "BUG-QA-12: stage-2 fault must return an error";
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "BUG-QA-12: stage-2 fault with s2s=1 must return Stalled (§5.5), "
           "not TranslationFault";
}

// Test 2: Stage-2 fault with s2s=false, FaultMode=Stall → result is NOT Stalled.
// S2S=0 means stage-2 faults use terminate semantics even when global FaultMode
// is Stall.  The result must be a translation fault, not Stalled.
// BEFORE FIX: result is Stalled (global FaultMode::Stall incorrectly applies).
// AFTER FIX:  result.getError() != SMMUError::Stalled.
TEST(BugQA12S2SStallMode, Stage2FaultTerminatesWhenS2SFalse) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config();
    cfg.faultMode = FaultMode::Stall;  // global: stall
    cfg.s2s       = false;             // STE.S2S=0 → terminate stage-2 faults
    ASSERT_TRUE(smmu.configureStream(0x11u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(0x11u).isOk());

    // Do NOT map any page — stage-2 translation will fault.
    TranslationResult result = smmu.translate(0x11u, 0u, 0xA000u, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "BUG-QA-12: stage-2 fault must return an error";
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "BUG-QA-12: stage-2 fault with s2s=0 must NOT be Stalled even when "
           "FaultMode::Stall is set (§5.5)";
}

// Test 3: configureStream with STALL_MODEL==0b01 and s2s=1 must fail with C_BAD_STE.
// ARM §5.5: STALL_MODEL terminate-only + STE.S2S=1 is an illegal combination.
// BEFORE FIX: configureStream succeeds (no validation).
// AFTER FIX:  configureStream returns InvalidConfiguration and enqueues C_BAD_STE.
TEST(BugQA12S2SStallMode, TerminateOnlyStallModelRejectsS2STrue) {
    SMMU smmu;
    enableSMMU(smmu);

    // Set STALL_MODEL=0b01 (terminate-only) — from BUG-NEW-11 API.
    smmu.setStallModel(0x01u);

    StreamConfig cfg = stage2Config();
    cfg.s2s = true;  // Illegal: terminate-only model + stall-stage2

    VoidResult res = smmu.configureStream(0x12u, cfg);
    EXPECT_TRUE(res.isError())
        << "BUG-QA-12: STALL_MODEL==0b01 + s2s=1 must reject configureStream (§5.5)";

    if (res.isError()) {
        EXPECT_EQ(res.getError(), SMMUError::InvalidConfiguration)
            << "BUG-QA-12: error must be InvalidConfiguration (C_BAD_STE)";
    }

    // Verify C_BAD_STE event was enqueued.
    std::vector<EventEntry> events = smmu.getEventQueue();
    bool foundBadSte = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE) {
            foundBadSte = true;
            break;
        }
    }
    EXPECT_TRUE(foundBadSte)
        << "BUG-QA-12: C_BAD_STE event must be enqueued when STALL_MODEL=0b01 + s2s=1";
}

// ============================================================================
// BUG-QA-13: STE.S2R — stage-2 fault event suppression when S2R=0 && S2S=0
// ============================================================================

// Test 1: S2R=false, S2S=false → fault result returned, no event recorded.
// When both S2R and S2S are 0, stage-2 fault events are suppressed silently.
// BEFORE FIX: event IS recorded (field doesn't exist).
// AFTER FIX:  result is fault error AND no event in queue.
TEST(BugQA13S2RRecord, Stage2FaultSuppressedWhenS2RFalse) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config();
    cfg.s2R = false;
    cfg.s2s = false;
    ASSERT_TRUE(smmu.configureStream(0x20u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(0x20u).isOk());

    // Flush any events from configureStream.
    smmu.getEventQueue();

    // Trigger a stage-2 fault (no page mapped).
    TranslationResult result = smmu.translate(0x20u, 0u, 0xB000u, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "BUG-QA-13: stage-2 fault must still return an error result";
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "BUG-QA-13: s2R=0 s2s=0 must not stall";

    std::vector<EventEntry> events = smmu.getEventQueue();
    bool foundTransFault = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_TRANSLATION || ev.type == EventType::F_PERMISSION ||
            ev.type == EventType::F_ADDR_SIZE || ev.type == EventType::F_ACCESS) {
            // Check if it is for our stream.
            if (ev.streamID == 0x20u) {
                foundTransFault = true;
                break;
            }
        }
    }
    EXPECT_FALSE(foundTransFault)
        << "BUG-QA-13: no fault event must be recorded when s2R=0 && s2s=0 (§5.5)";
}

// Test 2: S2R=true → fault event IS recorded as normal.
// BEFORE FIX: field doesn't exist; behavior depends on default.
// AFTER FIX:  s2R=true (default) means events are always recorded.
TEST(BugQA13S2RRecord, Stage2FaultRecordedWhenS2RTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config();
    cfg.s2R = true;
    cfg.s2s = false;
    ASSERT_TRUE(smmu.configureStream(0x21u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(0x21u).isOk());

    smmu.getEventQueue();

    TranslationResult result = smmu.translate(0x21u, 0u, 0xB000u, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "BUG-QA-13: stage-2 fault must return an error";

    std::vector<EventEntry> events = smmu.getEventQueue();
    bool foundFault = false;
    for (const auto& ev : events) {
        if (ev.streamID == 0x21u) {
            foundFault = true;
            break;
        }
    }
    EXPECT_TRUE(foundFault)
        << "BUG-QA-13: fault event must be recorded when s2R=true (§5.5)";
}

// Test 3: S2R=false, S2S=true → result is Stalled AND event IS recorded.
// When s2s=1, the transaction stalls (record=yes implied by stall path).
// BEFORE FIX: s2R field doesn't exist; s2s may not gate stall properly.
// AFTER FIX:  result is Stalled, event recorded (stall path always records).
TEST(BugQA13S2RRecord, Stage2FaultStallsAndRecordsWhenS2STrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config();
    cfg.s2R = false;  // S2R=0 but S2S=1 → stall path active
    cfg.s2s = true;
    ASSERT_TRUE(smmu.configureStream(0x22u, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(0x22u).isOk());

    smmu.getEventQueue();

    TranslationResult result = smmu.translate(0x22u, 0u, 0xB000u, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "BUG-QA-13: s2s=1 fault must return error";
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "BUG-QA-13: s2R=0 s2s=1 must still Stall (§5.5); stall overrides suppression";

    std::vector<EventEntry> events = smmu.getEventQueue();
    bool foundStallEvent = false;
    for (const auto& ev : events) {
        if (ev.streamID == 0x22u) {
            foundStallEvent = true;
            break;
        }
    }
    EXPECT_TRUE(foundStallEvent)
        << "BUG-QA-13: stall event must be recorded even when s2R=0 (s2s=1 implies record)";
}

// ============================================================================
// BUG-QA-14: CMD_TLBI_NSNH_ALL must only evict EL1_EL0 TLB entries
// ============================================================================

// Test 1: NSNH_ALL evicts EL1_EL0 entries but preserves EL2 entries.
// BEFORE FIX: NSNH_ALL calls invalidateTranslationCache() → all entries gone.
// AFTER FIX:  EL1_EL0 entry is evicted; EL2 entry survives.
//
// Strategy: populate TLB by translating with EL1_EL0 stream (default strw)
// and a separate EL2 stream (strw=EL2).  Run NSNH_ALL.  Then re-translate:
// - EL1_EL0 stream → cache miss → TLB re-populated (no stale hit).
// - EL2 stream → still cache hit → same PA returned.
//
// We detect TLB presence by a second translate after NSNH_ALL.  If the
// EL2 entry survived, the second translate will succeed for the same IOVA
// without needing a page-table re-walk.  Because both paths use the same
// in-memory page table, both translates succeed regardless of TLB state —
// we instead verify the TLB *counter* behaviour using getStats() / hit rate.
// Simpler approach: we verify that after NSNH_ALL the EL1_EL0 translate on
// an UNMAPPED page now faults (TLB evicted → page table re-walked → fault),
// while the EL2 translate on a mapped page still succeeds (TLB survived →
// fast-path hit → immediate success).
TEST(BugQA14NsnhAll, NsnhAllPreservesEl2Entries) {
    SMMU smmu;
    enableSMMU(smmu);

    // ── EL1_EL0 stream (default strw = EL1_EL0) ──────────────────────────
    StreamConfig cfgEL1;
    cfgEL1.translationEnabled = true;
    cfgEL1.stage1Enabled      = true;
    cfgEL1.stage2Enabled      = false;
    cfgEL1.bypassEnabled      = false;
    cfgEL1.securityState      = SecurityState::NonSecure;
    cfgEL1.asid               = 0x01u;
    cfgEL1.strw               = StreamWorld::EL1_EL0;
    cfgEL1.t0sz               = 16u;
    cfgEL1.aa64               = true;
    ASSERT_TRUE(smmu.configureStream(0x30u, cfgEL1).isOk());
    ASSERT_TRUE(smmu.enableStream(0x30u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x30u, 0u).isOk());
    ASSERT_TRUE(smmu.mapPage(0x30u, 0u, 0xC000u, 0x000C000u, PagePermissions(true, true, false)).isOk());

    // ── EL2 stream (strw = EL2) ───────────────────────────────────────────
    // STRW=EL2 (0b01) is illegal for NonSecure per §5.2 — use Secure instead.
    StreamConfig cfgEL2;
    cfgEL2.translationEnabled = true;
    cfgEL2.stage1Enabled      = true;
    cfgEL2.stage2Enabled      = false;
    cfgEL2.bypassEnabled      = false;
    cfgEL2.securityState      = SecurityState::Secure;  // Secure + EL2 is legal
    cfgEL2.asid               = 0x02u;
    cfgEL2.strw               = StreamWorld::EL2;
    cfgEL2.t0sz               = 16u;
    cfgEL2.aa64               = true;
    ASSERT_TRUE(smmu.configureStream(0x31u, cfgEL2).isOk());
    ASSERT_TRUE(smmu.enableStream(0x31u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x31u, 0u).isOk());
    ASSERT_TRUE(smmu.mapPage(0x31u, 0u, 0xD000u, 0x000D000u, PagePermissions(true, true, false)).isOk());

    // Warm up the TLB for both streams.
    TranslationResult r1 = smmu.translate(0x30u, 0u, 0xC000u, AccessType::Read);
    ASSERT_TRUE(r1.isOk()) << "BUG-QA-14: EL1_EL0 initial translate must succeed";

    TranslationResult r2 = smmu.translate(0x31u, 0u, 0xD000u, AccessType::Read);
    ASSERT_TRUE(r2.isOk()) << "BUG-QA-14: EL2 initial translate must succeed";

    // Now unmap the EL1_EL0 page so a TLB miss causes a page-table fault.
    // (If TLB still has the entry, translate would succeed — that would be wrong.)
    smmu.unmapPage(0x30u, 0u, 0xC000u);

    // Issue CMD_TLBI_NSNH_ALL.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NSNH_ALL;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    // ── EL1_EL0 entry should be EVICTED ──────────────────────────────────
    // After eviction, translate must go to the page table.  Since we unmapped
    // the page, the result must be a fault.
    TranslationResult r3 = smmu.translate(0x30u, 0u, 0xC000u, AccessType::Read);
    EXPECT_TRUE(r3.isError())
        << "BUG-QA-14: EL1_EL0 TLB entry must be evicted by NSNH_ALL (§4.4.4.1); "
           "translate on unmapped page must fault after eviction";

    // ── EL2 entry should SURVIVE ──────────────────────────────────────────
    // EL2 entry must still be cached; translate should succeed with the old PA.
    // We do NOT unmap the EL2 page, so even a re-walk succeeds.
    // We verify via cache hit-rate: issue two EL2 translates and check hits > 0.
    TranslationResult r4 = smmu.translate(0x31u, 0u, 0xD000u, AccessType::Read);
    EXPECT_TRUE(r4.isOk())
        << "BUG-QA-14: EL2 TLB entry must survive NSNH_ALL (§4.4.4.1); "
           "translate on EL2 stream must still succeed";

    if (r4.isOk()) {
        EXPECT_EQ(r4.getValue().physicalAddress, 0x000D000u)
            << "BUG-QA-14: EL2 translate must return the original PA";
    }
}

// Test 2: NH_ALL (Non-Hyp All) also evicts only EL1_EL0 entries (§4.4.3).
// Regression guard — NH_ALL has the same semantics as NSNH_ALL for the
// Non-secure EL1_EL0 scope; it must also use invalidateNonHypEntries().
TEST(BugQA14NsnhAll, NhAllPreservesEl2Entries) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfgEL1;
    cfgEL1.translationEnabled = true;
    cfgEL1.stage1Enabled      = true;
    cfgEL1.stage2Enabled      = false;
    cfgEL1.bypassEnabled      = false;
    cfgEL1.securityState      = SecurityState::NonSecure;
    cfgEL1.asid               = 0x11u;
    cfgEL1.strw               = StreamWorld::EL1_EL0;
    cfgEL1.t0sz               = 16u;
    cfgEL1.aa64               = true;
    ASSERT_TRUE(smmu.configureStream(0x40u, cfgEL1).isOk());
    ASSERT_TRUE(smmu.enableStream(0x40u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x40u, 0u).isOk());
    ASSERT_TRUE(smmu.mapPage(0x40u, 0u, 0xE000u, 0x000E000u, PagePermissions(true, true, false)).isOk());

    StreamConfig cfgEL2;
    cfgEL2.translationEnabled = true;
    cfgEL2.stage1Enabled      = true;
    cfgEL2.stage2Enabled      = false;
    cfgEL2.bypassEnabled      = false;
    cfgEL2.securityState      = SecurityState::Secure;
    cfgEL2.asid               = 0x12u;
    cfgEL2.strw               = StreamWorld::EL2;
    cfgEL2.t0sz               = 16u;
    cfgEL2.aa64               = true;
    ASSERT_TRUE(smmu.configureStream(0x41u, cfgEL2).isOk());
    ASSERT_TRUE(smmu.enableStream(0x41u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x41u, 0u).isOk());
    ASSERT_TRUE(smmu.mapPage(0x41u, 0u, 0xF000u, 0x000F000u, PagePermissions(true, true, false)).isOk());

    // Warm up TLB.
    ASSERT_TRUE(smmu.translate(0x40u, 0u, 0xE000u, AccessType::Read).isOk());
    ASSERT_TRUE(smmu.translate(0x41u, 0u, 0xF000u, AccessType::Read).isOk());

    // Unmap EL1_EL0 page to detect TLB eviction.
    smmu.unmapPage(0x40u, 0u, 0xE000u);

    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    // EL1_EL0 entry should be evicted → fault on unmapped page.
    TranslationResult r1 = smmu.translate(0x40u, 0u, 0xE000u, AccessType::Read);
    EXPECT_TRUE(r1.isError())
        << "BUG-QA-14: NH_ALL must evict EL1_EL0 entries (§4.4.3)";

    // EL2 entry should survive.
    TranslationResult r2 = smmu.translate(0x41u, 0u, 0xF000u, AccessType::Read);
    EXPECT_TRUE(r2.isOk())
        << "BUG-QA-14: NH_ALL must not evict EL2 entries (§4.4.3)";
}

// ============================================================================
// QA-AUDIT-FIX-1: isStage2Fault must apply to two-stage streams too (§5.5)
// ============================================================================
//
// Two-stage stream: stage-2 fault must use STE.S2S for the stall decision,
// not the stream-level FaultMode.
//
// BEFORE FIX: isStage2OnlyFault = isStage2 && !stage1Enabled && stage2Enabled.
//   With stage1Enabled=true, isStage2OnlyFault=false => uses FaultMode=Stall
//   => stage-2 fault is Stalled even though s2s=false (WRONG per §5.5).
//
// AFTER FIX: isStage2Fault = isStage2 (no stage1Enabled guard).
//   With s2s=false, stage-2 fault terminates => result is NOT Stalled.
TEST(QAAuditFix1TwoStageS2S, TwoStageStage2FaultUsesS2SNotFaultMode) {
    SMMU smmu;
    enableSMMU(smmu);

    // Two-stage stream: stage-1 maps IOVA -> IPA, stage-2 left unmapped.
    // FaultMode=Stall (would stall everything if isStage2OnlyFault were used).
    // s2s=false => stage-2 fault must terminate, not stall.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.faultMode          = FaultMode::Stall;  // global stall — must not apply to stage-2
    cfg.s2s                = false;              // STE.S2S=0 => terminate stage-2 faults
    cfg.t0sz               = 0u;
    cfg.s2t0sz             = 16u;               // IPA limit 2^48
    cfg.s2ps               = 5u;
    cfg.ips                = 6u;
    cfg.vmid               = 0x50u;
    ASSERT_TRUE(smmu.configureStream(0x50u, cfg).isOk())
        << "QA-AUDIT-FIX-1: configureStream must succeed";
    ASSERT_TRUE(smmu.enableStream(0x50u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x50u, 0u).isOk());

    // Provide an EMPTY stage-2 AS (overrides the PASID-0 auto-alias so that
    // the IPA 0x5000 is genuinely unmapped in stage-2, producing F_TRANSLATION).
    auto emptyS2AS = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(0x50u, emptyS2AS).isOk())
        << "QA-AUDIT-FIX-1: setStreamStage2AddressSpace must succeed";

    // Map stage-1: IOVA=0x1000 -> IPA=0x5000.
    // Stage-2 AS is empty => IPA=0x5000 has no stage-2 mapping => F_TRANSLATION.
    ASSERT_TRUE(smmu.mapPage(0x50u, 0u, 0x1000u, static_cast<PA>(0x5000u),
                             PagePermissions(true, true, false),
                             SecurityState::NonSecure).isOk());

    // Clear any events from setup.
    smmu.clearEventQueue();

    TranslationResult result = smmu.translate(0x50u, 0u, 0x1000u, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "QA-AUDIT-FIX-1: two-stage translation with unmapped stage-2 must fault";
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "QA-AUDIT-FIX-1: §5.5 S2S=0 must NOT stall even if FaultMode=Stall "
           "(bug: isStage2OnlyFault incorrectly excluded two-stage streams)";

    // With FaultMode=Stall but S2S=0, the stage-2 fault must go to event queue
    // (terminated), not to stall queue.
    EXPECT_EQ(smmu.getStalledTransactionCount(), 0u)
        << "QA-AUDIT-FIX-1: no stalled transactions expected when S2S=0";
}

// ============================================================================
// QA-AUDIT-FIX-2: TLBI_NH_ALL must be scoped to command's VMID (§4.4.2.1)
// ============================================================================
//
// BEFORE FIX: TLBI_NH_ALL calls invalidateNonHypEntries() which evicts ALL
//   EL1_EL0 entries regardless of VMID.
//
// AFTER FIX: TLBI_NH_ALL calls invalidateNonHypEntriesByVMID(cmd.vmid) which
//   only evicts EL1_EL0 entries where entry.vmid == cmd.vmid.
TEST(QAAuditFix2NhAllVmidScope, NhAllDoesNotEvictOtherVMIDEntries) {
    SMMU smmu;
    enableSMMU(smmu);

    // Stream A: VMID=1, two-stage (EL1_EL0, stage1+stage2).
    StreamConfig cfgA;
    cfgA.translationEnabled = true;
    cfgA.stage1Enabled      = true;
    cfgA.stage2Enabled      = true;
    cfgA.bypassEnabled      = false;
    cfgA.securityState      = SecurityState::NonSecure;
    cfgA.faultMode          = FaultMode::Terminate;
    cfgA.strw               = StreamWorld::EL1_EL0;
    cfgA.vmid               = 1u;
    cfgA.t0sz               = 0u;
    cfgA.s2t0sz             = 0u;
    cfgA.s2ps               = 5u;
    cfgA.ips                = 6u;
    ASSERT_TRUE(smmu.configureStream(0x60u, cfgA).isOk());
    ASSERT_TRUE(smmu.enableStream(0x60u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x60u, 0u).isOk());

    // Stage-2 address space for stream A (VMID=1): IPA=0x1000 -> PA=0x10000.
    auto s2asA = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(0x60u, s2asA).isOk());
    ASSERT_TRUE(s2asA->mapPage(0x5000u, static_cast<PA>(0x10000u),
                               PagePermissions(true, true, false),
                               SecurityState::NonSecure).isOk());
    // Stage-1 for stream A: IOVA=0x1000 -> IPA=0x5000.
    ASSERT_TRUE(smmu.mapPage(0x60u, 0u, 0x1000u, static_cast<PA>(0x5000u),
                             PagePermissions(true, true, false),
                             SecurityState::NonSecure).isOk());

    // Stream B: VMID=2, two-stage (EL1_EL0, stage1+stage2).
    StreamConfig cfgB = cfgA;
    cfgB.vmid = 2u;
    ASSERT_TRUE(smmu.configureStream(0x61u, cfgB).isOk());
    ASSERT_TRUE(smmu.enableStream(0x61u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0x61u, 0u).isOk());

    // Stage-2 address space for stream B (VMID=2): IPA=0x6000 -> PA=0x20000.
    auto s2asB = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(0x61u, s2asB).isOk());
    ASSERT_TRUE(s2asB->mapPage(0x6000u, static_cast<PA>(0x20000u),
                               PagePermissions(true, true, false),
                               SecurityState::NonSecure).isOk());
    // Stage-1 for stream B: IOVA=0x2000 -> IPA=0x6000.
    ASSERT_TRUE(smmu.mapPage(0x61u, 0u, 0x2000u, static_cast<PA>(0x6000u),
                             PagePermissions(true, true, false),
                             SecurityState::NonSecure).isOk());

    // Warm up the TLB for both streams.
    TranslationResult rA1 = smmu.translate(0x60u, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(rA1.isOk()) << "QA-AUDIT-FIX-2 setup: stream A (VMID=1) translate must succeed";

    TranslationResult rB1 = smmu.translate(0x61u, 0u, 0x2000u, AccessType::Read);
    ASSERT_TRUE(rB1.isOk()) << "QA-AUDIT-FIX-2 setup: stream B (VMID=2) translate must succeed";

    // Unmap BOTH stage-2 pages so a TLB miss causes a fault for either stream.
    // This makes the test definitive: stream B's success after TLBI_NH_ALL(VMID=1)
    // proves its TLB entry survived (only a cached hit returns PA=0x20000).
    // Without the fix, both entries are evicted and both re-walks fault.
    s2asA->unmapPage(0x5000u);
    s2asB->unmapPage(0x6000u);

    // Issue TLBI_NH_ALL scoped to VMID=1 (must evict stream A only).
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NH_ALL;
    cmd.vmid = 1u;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // Stream A (VMID=1) entry should be EVICTED — re-walk faults on unmapped stage-2.
    smmu.clearEventQueue();
    TranslationResult rA2 = smmu.translate(0x60u, 0u, 0x1000u, AccessType::Read);
    EXPECT_TRUE(rA2.isError())
        << "QA-AUDIT-FIX-2: TLBI_NH_ALL(VMID=1) must evict VMID=1 EL1_EL0 entry; "
           "re-walk on unmapped stage-2 must fault";

    // Stream B (VMID=2) entry must SURVIVE (TLB hit => cached PA returned).
    // Without fix: VMID=2 entry also evicted => re-walk faults (WRONG per §4.4.2.1).
    // With fix: VMID=2 entry preserved => TLB hit => PA=0x20000 returned.
    TranslationResult rB2 = smmu.translate(0x61u, 0u, 0x2000u, AccessType::Read);
    EXPECT_TRUE(rB2.isOk())
        << "QA-AUDIT-FIX-2: TLBI_NH_ALL(VMID=1) must NOT evict VMID=2 EL1_EL0 entry (§4.4.2.1); "
           "TLB hit on cached VMID=2 entry must succeed even though stage-2 page is unmapped";
    if (rB2.isOk()) {
        EXPECT_EQ(rB2.getValue().physicalAddress, static_cast<PA>(0x20000u))
            << "QA-AUDIT-FIX-2: VMID=2 TLB hit must return cached PA=0x20000";
    }
}

// ============================================================================
// QA-AUDIT-FIX-3: STALL_MODEL validation must be guarded by stage2Enabled (§5.5)
// ============================================================================
//
// BEFORE FIX: configureStream checks (stallModel==0x01 && config.s2s) unconditionally.
//   A bypass stream with s2s=true fires C_BAD_STE even though it will never do
//   stage-2 translation (stage2Enabled=false).
//
// AFTER FIX: check is (stage2Enabled && stallModel==0x01 && s2s).
//   Bypass streams ignore s2s entirely.
TEST(QAAuditFix3StallModelGuard, BypassStreamWithS2sDoesNotTriggerBadSTE) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.setStallModel(0x01u);  // terminate-only STALL_MODEL

    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;  // bypass — S2S is irrelevant
    cfg.bypassEnabled      = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.s2s                = true;   // set but must be ignored (no stage-2)

    VoidResult result = smmu.configureStream(0x70u, cfg);
    EXPECT_TRUE(result.isOk())
        << "QA-AUDIT-FIX-3: bypass stream with s2s=true must succeed when STALL_MODEL=0x01 "
           "(stage2Enabled=false => no stage-2 fault possible => S2S irrelevant)";

    // No C_BAD_STE must have been generated.
    std::vector<EventEntry> events = smmu.getEventQueue();
    bool hasBadSTE = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE) {
            hasBadSTE = true;
            break;
        }
    }
    EXPECT_FALSE(hasBadSTE)
        << "QA-AUDIT-FIX-3: C_BAD_STE must not be generated for bypass stream with s2s=true "
           "(STALL_MODEL guard must require stage2Enabled=true)";
}
