// TDD failing tests for BUG-NEW-G, BUG-NEW-H, BUG-NEW-I, and BUG-NEW-J.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked
// as a documentation/regression test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-G (Both §4.5.2): IDR0.PRI==0 → CERROR_ILL for CMD_PRI_RESP
//
//   ARM IHI0070G.b §4.5.2: CMD_PRI_RESP is only valid when the PRI feature is
//   supported (IDR0.PRI==1).  When IDR0.PRI==0 the command is ILLEGAL and the
//   SMMU must raise CERROR_ILL and toggle GERROR.CMDQ_ERR.
//
//   Requires new API: setPRISupported(bool).
//
//   Current behavior (WRONG): no setPRISupported() API exists; CMD_PRI_RESP
//   always executes without checking IDR0.PRI, so the command succeeds silently
//   with no CERROR_ILL.
//
//   Required behavior: setPRISupported(false) + submitCommand(PRI_RESP) →
//   CERROR_ILL in CMDQ_CONS.ERR and GERROR.CMDQ_ERR active.
//
//   BEFORE FIX: setPRISupported(false) won't compile (method missing) → FAILS.
//   AFTER FIX:  method exists, CERROR_ILL raised → PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-H (C++ only §8.1): Missing setPriqConsOvackflg() in C++
//
//   Rust has set_priq_cons_ovackflg(uint8_t value) but C++ has no equivalent.
//   The ARM spec (§8.1 / §6.3.96) has SMMU_PRIQ_CONS.OVACKFLG (bit 31) which
//   software writes to acknowledge a PRI queue overflow condition.
//
//   Requires new API: setPriqConsOvackflg(uint8_t value).
//   The method must set bit 31 of the internal PRIQ_CONS register when value==1,
//   and clear bit 31 when value==0.  The existing getPriqConsIndex() accessor
//   must reflect the bit.
//
//   BEFORE FIX: setPriqConsOvackflg(1) won't compile (method missing) → FAILS.
//   AFTER FIX:  method exists, getPriqConsIndex() has bit 31 set → PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-I (Both §7.3.13): eventClass for stage-2 IPA faults
//
//   SPECIFICATION DOCUMENT — not a failing test.
//
//   ARM IHI0070G.b §7.3.13 / Table D8-3 CLASS field encoding:
//     0b00 (0) = CD-fetch class (structure fetch from stage-2 AT)
//     0b01 (1) = TT-walk class  (translation table walk through stage-2 AT)
//     0b10 (2) = IN class       (input address translation stage-2 fault)
//     0b11 (3) = Reserved
//
//   In the SW model, only CLASS=2 (IN) faults can be generated; the model does
//   not simulate a hardware TT walk and therefore CLASS=0 (CD) and CLASS=1 (TT)
//   are not reachable.  The existing code correctly sets eventClass=2 for all
//   F_TRANSLATION / F_ADDR_SIZE / F_PERMISSION / F_ACCESS faults.
//
//   This test verifies the current CORRECT behavior for stage-2-only streams:
//   an IPA→PA translation fault must produce an event with s2=true and
//   eventClass=2 (IN).  The test passes both before and after any fix.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-J (Both §5.2): EATS validation missing in configureStream()
//
//   ARM IHI0070G.b §5.2 SteIllegal() pseudocode:
//     (a) EATS==0b10 (split-stage ATS) AND Config != two-stage (0b111) → C_BAD_STE
//     (b) EATS==0b10 (split-stage ATS) AND S2S==1                    → C_BAD_STE
//     (c) EATS==0b01 (combined-stage ATS) AND S2S==1 AND stage2Enabled → C_BAD_STE
//
//   Current behavior (WRONG): configureStream() does not validate the eats field
//   against Config/S2S.  All three cases succeed silently without C_BAD_STE.
//
//   BEFORE FIX: configureStream() returns Ok, no C_BAD_STE event → FAILS.
//   AFTER FIX:  configureStream() returns error, C_BAD_STE emitted → PASSES.
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

// Returns true when GERROR.CMDQ_ERR is active (GERROR[0] != GERRORN[0]).
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

// Returns true when the event queue contains at least one event of the given type.
static bool hasEvent(SMMU& smmu, EventType type) {
    std::vector<EventEntry> events = smmu.getEventQueue();
    for (const auto& ev : events) {
        if (ev.type == type) {
            return true;
        }
    }
    return false;
}

// Build and submit a CMD_PRI_RESP command.
// prgIndex identifies the page request group being responded to.
static void submitPriResp(SMMU& smmu, uint32_t sid, uint32_t pasid,
                           uint16_t prgIdx) {
    CommandEntry cmd;
    cmd.type     = CommandType::PRI_RESP;
    cmd.streamID = sid;
    cmd.pasid    = pasid;
    cmd.prgIndex = prgIdx;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();
}

// Configure a stage-2-only stream and map a stage-2 page.
// Returns the IPA used so the caller can attempt translation through it.
static void setupStage2OnlyStream(SMMU& smmu, uint32_t sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-G: IDR0.PRI==0 gates CMD_PRI_RESP (ARM §4.5.2)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: setPRISupported(false) + CMD_PRI_RESP → CERROR_ILL + GERROR.CMDQ_ERR
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.5.2: CMD_PRI_RESP is only legal when IDR0.PRI==1.
// When IDR0.PRI==0, the command is ILLEGAL → CERROR_ILL in CMDQ_CONS.ERR and
// GERROR.CMDQ_ERR must be toggled.
//
// This test FAILS before the fix because:
//   1. setPRISupported() does not exist (compilation error), OR
//   2. After the API is added but before the guard is wired in, CMD_PRI_RESP
//      executes without checking IDR0.PRI and getCmdqConsErr() returns 0.
//
// BEFORE FIX: won't compile (no setPRISupported) → test FAILS.
// AFTER FIX:  CERROR_ILL raised, GERROR.CMDQ_ERR active → test PASSES.
TEST(BugNewG_PriRespPriDisabled, PRI0_PriResp_RaisesCerrorIll) {
    // §4.5.2: IDR0.PRI==0 makes CMD_PRI_RESP illegal → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    // Disable PRI feature (IDR0.PRI=0).
    smmu.setPRISupported(false);

    // Submit a CMD_PRI_RESP — must be rejected with CERROR_ILL when PRI==0.
    submitPriResp(smmu, /*sid=*/1u, /*pasid=*/0u, /*prgIdx=*/0u);

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-G: CMD_PRI_RESP with IDR0.PRI==0 must set CMDQ_CONS.ERR=CERROR_ILL "
           "(ARM §4.5.2: PRI_RESP is only legal when PRI feature is supported). "
           "Current code: no setPRISupported() method and no PRI check in PRI_RESP handler.";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-G: GERROR.CMDQ_ERR must be active when CMD_PRI_RESP is rejected "
           "with CERROR_ILL (ARM §4.5.2 + §6.3.17). "
           "Current code: GERROR.CMDQ_ERR not toggled because no PRI check exists.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (regression): setPRISupported(true) + CMD_PRI_RESP → no error
// ─────────────────────────────────────────────────────────────────────────────
//
// When IDR0.PRI==1 (default), CMD_PRI_RESP must be processed normally.
// This test verifies that the guard is properly scoped to PRI==0 only.
//
// BEFORE FIX: (fails to compile because setPRISupported() is missing).
// AFTER FIX:  PRI_RESP accepted, CMDQ_CONS.ERR==CERROR_NONE → PASSES.
TEST(BugNewG_PriRespPriDisabled, PRI1_PriResp_NoError) {
    // Regression: IDR0.PRI==1 (default) → CMD_PRI_RESP must be accepted.
    SMMU smmu;
    enableSMMU(smmu);

    // Explicitly set PRI supported (default is true, but be explicit).
    smmu.setPRISupported(true);

    // Submit a CMD_PRI_RESP — must NOT raise CERROR_ILL when PRI==1.
    submitPriResp(smmu, /*sid=*/2u, /*pasid=*/0u, /*prgIdx=*/0u);

    EXPECT_NE(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-G regression: CMD_PRI_RESP with IDR0.PRI==1 must NOT raise CERROR_ILL "
           "(ARM §4.5.2: PRI_RESP is legal when PRI feature is supported). "
           "The guard must fire only when PRI==0.";

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-G regression: GERROR.CMDQ_ERR must NOT be active when CMD_PRI_RESP "
           "is accepted normally (IDR0.PRI==1).";
}

// ============================================================================
// BUG-NEW-H (C++ only §8.1): Missing setPriqConsOvackflg() in C++
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: setPriqConsOvackflg(1) persists bit 31 in PRIQ_CONS
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM IHI0070G.b §8.1 / §6.3.96: SMMU_PRIQ_CONS.OVACKFLG (bit 31).
// Software writes bit 31 to acknowledge an observed PRI queue overflow.
// The C++ model must provide setPriqConsOvackflg(uint8_t) which sets or clears
// bit 31 of the internal priqCons register.  The existing getPriqConsIndex()
// read-back must reflect the updated bit.
//
// BEFORE FIX: setPriqConsOvackflg() won't compile (method missing) → FAILS.
// AFTER FIX:  bit 31 is set in getPriqConsIndex() return value → PASSES.
TEST(BugNewH_PriqConsOvackflg, SetOvackflg1_PersistsBit31) {
    // §8.1 / §6.3.96: PRIQ_CONS.OVACKFLG (bit 31) must be settable via API.
    SMMU smmu;
    enableSMMU(smmu);

    smmu.setPriqConsOvackflg(1u);

    const uint32_t consVal = smmu.getPriqConsIndex();
    EXPECT_NE(consVal & (1u << 31u), 0u)
        << "BUG-NEW-H: getPriqConsIndex() must have bit 31 set after "
           "setPriqConsOvackflg(1) (ARM §8.1 / §6.3.96: PRIQ_CONS.OVACKFLG is bit 31). "
           "Current code: setPriqConsOvackflg() method is missing from the C++ API.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: setPriqConsOvackflg(0) clears bit 31 in PRIQ_CONS
// ─────────────────────────────────────────────────────────────────────────────
//
// After setting bit 31, calling setPriqConsOvackflg(0) must clear it.
//
// BEFORE FIX: won't compile (method missing) → FAILS.
// AFTER FIX:  bit 31 cleared → PASSES.
TEST(BugNewH_PriqConsOvackflg, SetOvackflg0_ClearsBit31) {
    // §8.1 / §6.3.96: writing OVACKFLG=0 must clear bit 31.
    SMMU smmu;
    enableSMMU(smmu);

    // Set then clear bit 31.
    smmu.setPriqConsOvackflg(1u);
    smmu.setPriqConsOvackflg(0u);

    const uint32_t consVal = smmu.getPriqConsIndex();
    EXPECT_EQ(consVal & (1u << 31u), 0u)
        << "BUG-NEW-H: getPriqConsIndex() must have bit 31 cleared after "
           "setPriqConsOvackflg(0) (ARM §8.1 / §6.3.96: PRIQ_CONS.OVACKFLG is bit 31). "
           "Current code: setPriqConsOvackflg() method is missing from the C++ API.";
}

// ============================================================================
// BUG-NEW-I (§7.3.13): eventClass=2 (IN) for stage-2 IPA faults
//
// SPECIFICATION DOCUMENT — not a failing test.
// This test verifies existing CORRECT behavior and passes both before and after
// any code change.  It documents that the SW model correctly sets CLASS=2 (IN)
// for all stage-2 IPA translation faults, per ARM §7.3.13 Table D8-3.
// ============================================================================
TEST(BugNewI_Stage2EventClass, Stage2IpaFault_EventClassIs2_IN) {
    // SPECIFICATION DOCUMENT: stage-2-only stream with an IPA outside the
    // stage-2 address space must generate F_TRANSLATION with eventClass=2 (IN).
    //
    // ARM §7.3.13 Table D8-3:
    //   CLASS=0b10 (2) = IN — input address stage-2 fault.
    //
    // This is the only CLASS value reachable in the SW model; CLASS=0 (CD) and
    // CLASS=1 (TT) require a hardware TT walk that the SW model does not simulate.
    //
    // The test passes both before and after any fix — it is a regression guard.
    SMMU smmu;
    enableSMMU(smmu);

    const uint32_t sid = 0x30u;
    setupStage2OnlyStream(smmu, sid);

    // Translate an IPA that has no stage-2 mapping → F_TRANSLATION.
    const IOVA unmappedIpa = 0xDEAD0000u;
    auto result = smmu.translate(sid, 0u, unmappedIpa, AccessType::Read,
                                 SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "BUG-NEW-I doc: stage-2-only stream with no mapping must fault";

    // Find the F_TRANSLATION event and verify eventClass==2 (IN).
    std::vector<EventEntry> events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_TRANSLATION && ev.streamID == sid) {
            found = true;
            EXPECT_EQ(ev.eventClass, 2u)
                << "BUG-NEW-I doc: stage-2 IPA fault must set eventClass=2 (IN) "
                   "per ARM §7.3.13 Table D8-3.  "
                   "CLASS=0 (CD) and CLASS=1 (TT) require HW TT walk — unreachable in SW model.";
            EXPECT_TRUE(ev.s2)
                << "BUG-NEW-I doc: stage-2 IPA fault must set s2=true (§7.3.13).";
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-NEW-I doc: F_TRANSLATION event for the stage-2 fault not found in queue.";
}

// ============================================================================
// BUG-NEW-J: EATS field validation in configureStream() (ARM §5.2 SteIllegal)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: EATS=2 + stage1-only config (Config != two-stage) → C_BAD_STE
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b10 (split-stage ATS) is only legal for
// two-stage streams (STE.Config=0b111).  Any other Config combined with
// EATS=2 is illegal → C_BAD_STE.
//
// BEFORE FIX: configureStream() succeeds with no error, no C_BAD_STE → FAILS.
// AFTER FIX:  configureStream() returns error, C_BAD_STE emitted → PASSES.
TEST(BugNewJ_EatsValidation, Eats2_Stage1Only_EmitsCBadSte) {
    // §5.2 SteIllegal: EATS=0b10 with stage1-only (not two-stage) → C_BAD_STE.
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;  // Not two-stage — EATS=2 is illegal here.
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = 2u;  // EATS=0b10: split-stage ATS (requires two-stage Config)
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x40u, cfg);

    EXPECT_FALSE(result.isOk())
        << "BUG-NEW-J: configureStream() with eats=2 AND stage1-only Config must return "
           "an error (ARM §5.2 SteIllegal: EATS==0b10 requires Config=two-stage 0b111). "
           "Current code: no EATS validation exists.";

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-NEW-J: C_BAD_STE event must be recorded when eats=2 AND stage1-only "
           "(ARM §5.2 SteIllegal: EATS=0b10 with Config != 0b111 is illegal). "
           "Current code: no C_BAD_STE emitted.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: EATS=2 + two-stage + S2S=1 → C_BAD_STE
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b10 AND STE.S2S==1 → C_BAD_STE,
// even when the Config IS two-stage.
//
// BEFORE FIX: configureStream() succeeds, no C_BAD_STE → FAILS.
// AFTER FIX:  C_BAD_STE emitted → PASSES.
TEST(BugNewJ_EatsValidation, Eats2_TwoStage_S2S1_EmitsCBadSte) {
    // §5.2 SteIllegal: EATS=0b10 AND S2S==1 → C_BAD_STE (even for two-stage Config).
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;  // Two-stage Config (0b111).
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = 2u;   // EATS=0b10: split-stage ATS
    cfg.s2s                = true; // STE.S2S=1 — incompatible with EATS=0b10
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x41u, cfg);

    EXPECT_FALSE(result.isOk())
        << "BUG-NEW-J: configureStream() with eats=2 AND S2S=1 must return an error "
           "(ARM §5.2 SteIllegal: EATS==0b10 AND S2S=1 is illegal regardless of Config). "
           "Current code: no EATS+S2S validation.";

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-NEW-J: C_BAD_STE event must be recorded when eats=2 AND S2S=1 "
           "(ARM §5.2 SteIllegal). Current code: no C_BAD_STE emitted.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: EATS=1 + S2S=1 + stage2Enabled → C_BAD_STE
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b01 AND STE.S2S==1 AND stage2Enabled → C_BAD_STE.
//
// BEFORE FIX: configureStream() succeeds, no C_BAD_STE → FAILS.
// AFTER FIX:  C_BAD_STE emitted → PASSES.
TEST(BugNewJ_EatsValidation, Eats1_S2S1_Stage2Enabled_EmitsCBadSte) {
    // §5.2 SteIllegal: EATS=0b01 AND S2S==1 AND stage2Enabled → C_BAD_STE.
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;  // stage2 enabled — EATS=1+S2S=1 is illegal here.
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = 1u;   // EATS=0b01: combined-stage ATS
    cfg.s2s                = true; // STE.S2S=1 — incompatible with EATS=0b01+stage2
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x42u, cfg);

    EXPECT_FALSE(result.isOk())
        << "BUG-NEW-J: configureStream() with eats=1 AND S2S=1 AND stage2Enabled must "
           "return an error (ARM §5.2 SteIllegal: EATS==0b01 AND STE.S2S=1 AND stage2 "
           "enabled is illegal). Current code: no EATS+S2S validation.";

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-NEW-J: C_BAD_STE event must be recorded when eats=1 AND S2S=1 AND "
           "stage2Enabled (ARM §5.2 SteIllegal). Current code: no C_BAD_STE emitted.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 (regression): EATS=2 + two-stage + S2S=0 → accepted
// ─────────────────────────────────────────────────────────────────────────────
//
// EATS=2 (split-stage ATS) is legal when Config=two-stage AND S2S=0.
// This test verifies the fix does not over-reject valid configurations.
//
// BEFORE FIX: (already passes — no EATS validation means this is accepted).
// AFTER FIX:  must still pass — regression guard.
TEST(BugNewJ_EatsValidation, Eats2_TwoStage_S2S0_Accepted) {
    // Regression: EATS=2 + two-stage + S2S=0 must be ACCEPTED.
    // ARM §5.2 SteIllegal: EATS=0b10 is only illegal when Config != 0b111 OR S2S=1.
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;  // Two-stage (Config=0b111) — OK for EATS=2.
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = 2u;   // EATS=0b10: split-stage ATS — legal with two-stage.
    cfg.s2s                = false; // S2S=0 — legal combination.
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x43u, cfg);

    EXPECT_TRUE(result.isOk())
        << "BUG-NEW-J regression: configureStream() with eats=2 AND two-stage AND S2S=0 "
           "must be ACCEPTED (ARM §5.2 SteIllegal: EATS=0b10 is only illegal when "
           "Config != 0b111 or S2S=1). The fix must not over-reject this valid config.";

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-NEW-J regression: no C_BAD_STE expected for eats=2 + two-stage + S2S=0.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 (regression): EATS=1 + S2S=0 → accepted
// ─────────────────────────────────────────────────────────────────────────────
//
// EATS=1 (combined-stage ATS) is legal when S2S=0 (regardless of stage config).
//
// BEFORE FIX: (already passes).
// AFTER FIX:  must still pass — regression guard.
TEST(BugNewJ_EatsValidation, Eats1_S2S0_Accepted) {
    // Regression: EATS=1 + S2S=0 must be ACCEPTED.
    // ARM §5.2 SteIllegal: EATS=0b01 is only illegal when S2S=1 AND stage2 is enabled.
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = 1u;    // EATS=0b01: combined-stage ATS — legal with S2S=0.
    cfg.s2s                = false; // S2S=0 → no conflict.
    cfg.t0sz               = 0;

    VoidResult result = smmu.configureStream(0x44u, cfg);

    EXPECT_TRUE(result.isOk())
        << "BUG-NEW-J regression: configureStream() with eats=1 AND S2S=0 must be ACCEPTED "
           "(ARM §5.2 SteIllegal: EATS=0b01 only conflicts with S2S=1 AND stage2Enabled). "
           "The fix must not over-reject this valid config.";

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-NEW-J regression: no C_BAD_STE expected for eats=1 + S2S=0.";
}
