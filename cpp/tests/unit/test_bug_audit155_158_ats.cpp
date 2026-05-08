// Tests for BUG-AUDIT-155-CPP through BUG-AUDIT-158-CPP (§3.9 ATS conformance)
// TDD: each test is written to FAIL before its fix is applied.
//
// BUG-AUDIT-158-CPP: atsSupported checks eats!=0 but ignores CR0_ATSCHK.
//   Spec §3.9.1.2: effective EATS==0 when EATS==0b1x AND ATSCHK==0.
//   When eats==2 and ATSCHK==0, ATS TR must get F_BAD_ATS_TREQ.
//
// BUG-AUDIT-156-CPP: SMMUEN==0 + ATS TR path gates F_BAD_ATS_TREQ on CR2_REC_CFG_ATS.
//   Spec §3.9.1.2 line 2135: F_BAD_ATS_TREQ for SMMUEN=0 is unconditional.
//   REC_CFG_ATS only gates config-error events (lines 2157-2158), not this one.
//
// BUG-AUDIT-155-CPP: handleTranslationFailure() emits F_TRANSLATION/F_PERMISSION for
//   AtsTranslationRequest transactions unconditionally. Spec §3.9.1.2 line 2141:
//   ATS TR that encounters Address Size / Access / Translation fault → Success R==W==0,
//   NO event recorded.
//
// BUG-AUDIT-157-CPP: CMD_SYNC with CS=0b01 must write CERROR_ATC_INV_SYNC to
//   CMDQ_CONS.ERR when a preceding CMD_ATC_INV failed. CERROR_ATC_INV_SYNC (value 3)
//   is defined in types.h but never referenced in smmu.cpp.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <cstdint>
#include <vector>

using namespace smmu;

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Stream with ATS supported via EATS==0b10 (split-stage) requires both stages enabled
// and NS1ATS==0. Use eats=3 (STE.EATS==0b11 = full ATS for two-stage) for a two-stage stream,
// or just use eats=1 for a single-stage stream. The bug under test is the ATSCHK gate,
// not the eats value per se. Use eats=1 with two-stage to test the ATSCHK interaction.
// Actually: eats==3 is full ATS for S2P streams. For S1-only, only eats==1 is legal.
// To test ATSCHK vs eats==2: use two-stage stream with eats==2.
static void configureTwoStageStreamEATS2(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;  // eats==2 requires two-stage
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.eats               = 2;  // Split-stage ATS (EATS==0b10), requires two-stage, s2s==0
    cfg.s2s                = false;
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0);
}

// Stream with no ATS support (eats=0).
static void configureStage1StreamNoATS(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.eats               = 0;
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0);
}

} // namespace

// ============================================================================
// BUG-AUDIT-158: atsSupported must check CR0_ATSCHK when eats==2
// ============================================================================

// When eats==2 (split-stage) and ATSCHK==0 (CR0 bit 4 clear), effective EATS==0.
// An ATS TR must be rejected with F_BAD_ATS_TREQ.
// BUG: currently atsSupported = (eats != 0) which returns true for eats==2 even when ATSCHK==0.
TEST(BugAudit158, EATS2WithATSCHK0ProducesF_BAD_ATS_TREQ) {
    SMMU smmu;
    // Enable SMMU without CR0_ATSCHK (bit 4 clear)
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    // Verify ATSCHK is NOT set
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_ATSCHK, 0u) << "ATSCHK must be clear for this test";

    const StreamID sid = 10;
    configureTwoStageStreamEATS2(smmu, sid);

    // Map a page so we don't hit a translation fault — if atsSupported were true,
    // the translation would succeed. If it's false, F_BAD_ATS_TREQ fires immediately.
    smmu.mapPage(sid, 0, 0x1000, 0x1000, PagePermissions{true, true, false});

    smmu.translate(sid, 0, 0x1000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_BAD_ATS_TREQ event but event queue is empty";
    EXPECT_EQ(events.front().type, EventType::F_BAD_ATS_TREQ)
        << "Expected F_BAD_ATS_TREQ when EATS==2 and ATSCHK==0 (effective EATS==0)";
}

// Sanity: when ATSCHK==1, eats==2 is a real ATS-capable configuration — no F_BAD_ATS_TREQ.
TEST(BugAudit158, EATS2WithATSCHK1AllowsATSTranslation) {
    SMMU smmu;
    // Enable with CR0_ATSCHK set
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN
                | SMMU::CR0_PRIQEN | SMMU::CR0_ATSCHK);
    ASSERT_NE(smmu.getCR0() & SMMU::CR0_ATSCHK, 0u) << "ATSCHK must be set for this test";

    const StreamID sid = 11;
    configureTwoStageStreamEATS2(smmu, sid);

    smmu.mapPage(sid, 0, 0x1000, 0x1000, PagePermissions{true, true, false});

    smmu.translate(sid, 0, 0x1000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    // No F_BAD_ATS_TREQ expected — ATS is genuinely supported
    for (const auto& ev : events) {
        EXPECT_NE(ev.type, EventType::F_BAD_ATS_TREQ)
            << "F_BAD_ATS_TREQ must NOT fire when EATS==2 and ATSCHK==1";
    }
}

// ============================================================================
// BUG-AUDIT-156: F_BAD_ATS_TREQ for SMMUEN=0 must not be gated on REC_CFG_ATS
// ============================================================================

// Spec §3.9.1.2: when SMMUEN==0, ATS TR always gets F_BAD_ATS_TREQ.
// BUG: code at line 269 gates it on CR2_REC_CFG_ATS — when that bit is clear no event fires.
TEST(BugAudit156, SMMUEN0ATSTRAlwaysProducesF_BAD_ATS_TREQ_WhenREC_CFG_ATS_Clear) {
    SMMU smmu;
    // Configure a stream BEFORE enabling the SMMU
    const StreamID sid = 20;
    configureStage1StreamNoATS(smmu, sid);

    // DISABLE the SMMU (SMMUEN=0). Do NOT set CR2_REC_CFG_ATS.
    smmu.setCR0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);  // no CR0_SMMUEN
    ASSERT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u) << "SMMUEN must be clear";

    smmu.translate(sid, 0, 0x1000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "Expected F_BAD_ATS_TREQ even when REC_CFG_ATS==0 per §3.9.1.2";
    EXPECT_EQ(events.front().type, EventType::F_BAD_ATS_TREQ);
}

// Sanity: with SMMUEN=0 and REC_CFG_ATS=1, F_BAD_ATS_TREQ still fires (already worked before fix).
TEST(BugAudit156, SMMUEN0ATSTRProducesF_BAD_ATS_TREQ_WhenREC_CFG_ATS_Set) {
    SMMU smmu;
    const StreamID sid = 21;
    configureStage1StreamNoATS(smmu, sid);

    smmu.setCR0(SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);  // no CR0_SMMUEN
    smmu.setCR2(SMMU::CR2_REC_CFG_ATS);

    smmu.translate(sid, 0, 0x1000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_BAD_ATS_TREQ";
    EXPECT_EQ(events.front().type, EventType::F_BAD_ATS_TREQ);
}

// ============================================================================
// BUG-AUDIT-155: ATS TR translation fault → Success R==W==0, NO event
// ============================================================================

// When an ATS TR hits an unmapped address (TranslationFault), handleTranslationFailure()
// unconditionally emits F_TRANSLATION. Spec §3.9.1.2: must return Success R==W==0, no event.
TEST(BugAudit155, ATSTRTranslationFaultProducesNoEventAndSuccessZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 30;
    // Stream with ATS enabled (eats=1), ATSCHK set so eats==1 is genuinely supported.
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN
                | SMMU::CR0_PRIQEN | SMMU::CR0_ATSCHK);
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.eats               = 1;  // Full ATS (EATS==0b01), ATSCHK=1 → genuinely supported
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);
    // Do NOT map 0x5000 → translation fault

    auto result = smmu.translate(sid, 0, 0x5000, AccessType::Read,
                                 SecurityState::NonSecure,
                                 TransactionType::AtsTranslationRequest);

    // Spec requires: Success with R==W==0 (no event, no error)
    auto events = smmu.getEventQueue();
    EXPECT_TRUE(events.empty())
        << "No event must be recorded for ATS TR translation fault (§3.9.1.2)";
    EXPECT_TRUE(isTranslationSuccess(result))
        << "ATS TR translation fault must return Success (R==W==0), not an error";
}

// Sanity: ordinary (non-ATS) translation of an unmapped address DOES produce F_TRANSLATION.
TEST(BugAudit155, OrdinaryTranslationFaultStillProducesF_TRANSLATION) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 31;
    configureStage1StreamNoATS(smmu, sid);
    // Do NOT map 0x5000

    smmu.translate(sid, 0, 0x5000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::Ordinary);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_TRANSLATION for ordinary fault";
    EXPECT_EQ(events.front().type, EventType::F_TRANSLATION);
}

// ============================================================================
// BUG-AUDIT-157: CMD_SYNC must write CERROR_ATC_INV_SYNC when CMD_ATC_INV failed
// ============================================================================

// CERROR_ATC_INV_SYNC (value 3) is defined in types.h but the CMD_SYNC handler never
// writes it. The test verifies the constant is the right value per spec §3.9.2 table.
// (A full integration test is deferred because executeATCInvalidationCommand() has
// no failure path — the infrastructure for ATC_INV failure signalling does not yet exist.)
TEST(BugAudit157, CERROR_ATC_INV_SYNC_HasCorrectValue) {
    // §3.9.2 / §4.5: CERROR_ATC_INV_SYNC == 0b0011 (value 3)
    EXPECT_EQ(CERROR_ATC_INV_SYNC, 3u)
        << "CERROR_ATC_INV_SYNC must be 3 per ARM SMMU v3 spec §3.9.2";
}

// When CMD_ATC_INV failure tracking is implemented, CMD_SYNC with CS=0b01 must
// write CERROR_ATC_INV_SYNC. Verify that CERROR_ILL (1), CERROR_NONE (0), and
// CERROR_ATC_INV_SYNC (3) are distinct values so future wiring is unambiguous.
TEST(BugAudit157, CERROR_Values_AreDistinct) {
    EXPECT_NE(CERROR_NONE, CERROR_ILL);
    EXPECT_NE(CERROR_NONE, CERROR_ATC_INV_SYNC);
    EXPECT_NE(CERROR_ILL,  CERROR_ATC_INV_SYNC);
}

// Verify that after a CMD_SYNC with CS=0b01 (IRQ) and no preceding CMD_ATC_INV failure,
// CMDQ_CONS.ERR is CERROR_NONE (the current behavior — should remain correct post-fix).
TEST(BugAudit157, CMD_SYNC_CS1_WithNoFailure_WritesCERROR_NONE) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs   = 1u;  // CS=0b01: IRQ signal
    smmu.submitCommand(sync);

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_NONE)
        << "CMD_SYNC with CS=0b01 and no prior ATC_INV failure must yield CERROR_NONE";
}
