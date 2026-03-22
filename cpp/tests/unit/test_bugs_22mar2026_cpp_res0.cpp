// Tests for RES0 wire-format field bugs confirmed on 2026-03-22.
// TDD: all tests are written to FAIL before their respective fixes are applied.
//
// BUG-CPP-1a (F_BAD_ATS_TREQ — §7.3.6):
//   The generateEvent() RES0 override block only covers configuration-class events
//   (C_BAD_STREAMID, C_BAD_STE, C_BAD_SUBSTREAMID, C_BAD_CD, F_CFG_CONFLICT).
//   F_BAD_ATS_TREQ is not included, so rnw/ind/pnu are set from the AccessType
//   switch.  Per ARM IHI0070G.b §7.3.6 bits 100/99/98 are RES0 for this event.
//   Fix: add F_BAD_ATS_TREQ to the RES0 override block in BOTH paths (stall +
//   normal), zeroing rnw, ind, and pnu.
//
// BUG-CPP-1b (F_TRANSL_FORBIDDEN — §7.3.8):
//   F_TRANSL_FORBIDDEN is not in the override block.  Per ARM IHI0070G.b §7.3.8
//   RnW (bit 100) IS defined (carries the direction of the presented transaction),
//   but InD (bit 99) and PnU (bit 98) are RES0.
//   Fix: add F_TRANSL_FORBIDDEN to the override block in BOTH paths, zeroing only
//   ind and pnu (not rnw).
//
// BUG-CPP-3 (COMMAND_SYNC_COMPLETION — §7.3.21):
//   generateEvent(COMMAND_SYNC_COMPLETION, ...) is called from processCommandQueue()
//   without an AccessType parameter, so it uses the default AccessType::Read and
//   the switch sets rnw=true.  There is no defined wire format for this event; per
//   §7.3 all upper fields are RES0.
//   Fix: add COMMAND_SYNC_COMPLETION to the RES0 override block in BOTH paths.
//
// BUG-CPP-4 (F_STREAM_DISABLED — §7.3.7):
//   generateEvent(F_STREAM_DISABLED, ...) is called without an AccessType so
//   rnw=true from default AccessType::Read.  Per §7.3.7 all fields above StreamID
//   (including RnW, InD, PnU) are RES0.
//   Fix: add F_STREAM_DISABLED to the RES0 override block in BOTH paths.
//
// BUG-CPP-2 (recordSecurityFault — §7.3.16):
//   The SecurityFault recovery case passes raw `accessType` to recordSecurityFault
//   instead of `eventAccessType` (the post-STRW/INSTCFG/PRIVCFG effective type).
//   The function ultimately calls generateEvent(F_PERMISSION, ..., accessType) so
//   the emitted InD/PnU fields do not reflect STE overrides.
//   Note: the SecurityFault path is effectively dead code in the current
//   implementation (InvalidSecurityState routes to PermissionFault instead), so
//   no test is written for BUG-CPP-2 — the fix is applied prophylactically.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <memory>
#include <vector>

using namespace smmu;

// ============================================================================
// Helper utilities
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Configure a stage-1-only stream capable of translation.
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

// Find the first event of the given type in the queue, or nullptr.
static const EventEntry* findEvent(const std::vector<EventEntry>& events, EventType target) {
    for (const auto& e : events) {
        if (e.type == target) {
            return &e;
        }
    }
    return nullptr;
}

} // namespace

// ============================================================================
// BUG-CPP-1a: F_BAD_ATS_TREQ event must have rnw=false, ind=false, pnu=false.
//
// ARM IHI0070G.b §7.3.6: bits 100 (RnW), 99 (InD), 98 (PnU) are RES0 in the
// F_BAD_ATS_TREQ event record.
//
// Setup: configure a valid stage-1 stream with EATS=0 (no ATS support).
//        Submit an ATS Translation Request — this must produce F_BAD_ATS_TREQ.
//        Assert rnw==false, ind==false, pnu==false regardless of AccessType used.
//
// BEFORE FIX: generateEvent() sets rnw=true (ReadPrivileged → pnu=true, rnw=true)
//             from the accessType switch; F_BAD_ATS_TREQ is absent from the
//             override block, so the assert rnw==false FAILS.
// AFTER FIX:  override block zeros rnw, ind, pnu; assert passes.
// ============================================================================
TEST(BugCpp22Mar2026Res0, FBadAtsTreqHasRnwIndPnuZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xA101u;
    // Stage-1 stream with EATS=0 (default) so ATS TR is unsupported → F_BAD_ATS_TREQ.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.eats               = 0;  // ATS not supported
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);
    smmu.mapPage(sid, 0, 0x1000u, 0x2000u, PagePermissions{true, false, false});

    // Submit ATS Translation Request with a privileged read access type so that
    // before the fix rnw and pnu are set to true by the access-type switch,
    // making the regression clearly observable.
    TranslationResult result = smmu.translate(
        sid, 0, 0x1000u, AccessType::ReadPrivileged, SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);
    EXPECT_TRUE(result.isError())
        << "ATS TR on EATS=0 stream must fail with F_BAD_ATS_TREQ";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::F_BAD_ATS_TREQ);
    ASSERT_NE(ev, nullptr) << "F_BAD_ATS_TREQ event must be present in the event queue";

    // ARM §7.3.6: bits 100/99/98 (RnW/InD/PnU) are RES0.
    EXPECT_FALSE(ev->rnw)
        << "BUG-CPP-1a: F_BAD_ATS_TREQ.rnw must be 0 (RES0 per §7.3.6);"
           " got true (set from AccessType::ReadPrivileged)";
    EXPECT_FALSE(ev->ind)
        << "BUG-CPP-1a: F_BAD_ATS_TREQ.ind must be 0 (RES0 per §7.3.6)";
    EXPECT_FALSE(ev->pnu)
        << "BUG-CPP-1a: F_BAD_ATS_TREQ.pnu must be 0 (RES0 per §7.3.6);"
           " got true (set from AccessType::ReadPrivileged)";
}

// ============================================================================
// BUG-CPP-1b: F_TRANSL_FORBIDDEN event must have ind=false, pnu=false.
//             RnW must reflect the access direction (true for Read).
//
// ARM IHI0070G.b §7.3.8: RnW (bit 100) IS defined; InD (bit 99) and PnU
// (bit 98) are RES0.
//
// Setup: enable SMMU with CR0.ATSCHK=1 (bit 4).  Configure a stream and map a
//        page.  Submit an ATS Translated request to a different address
//        (re-translation will fail) — this produces F_TRANSL_FORBIDDEN.
//
// BEFORE FIX: ExecutePrivileged → ind=true, pnu=true; both asserts FAIL.
// AFTER FIX:  override block zeros ind and pnu only; ind==false, pnu==false,
//             rnw may be true (direction-dependent) — test uses Read.
// ============================================================================
TEST(BugCpp22Mar2026Res0, FTranslForbiddenHasIndPnuZeroRnwDefined) {
    SMMU smmu;
    // Enable ATSCHK (bit 4) so ATS Translated requests are rechecked.
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN
                | SMMU::CR0_PRIQEN | SMMU::CR0_ATSCHK);

    const StreamID sid = 0xA102u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.eats               = 1;  // ATS enabled on this stream
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);
    // Map page at IOVA 0x1000 → PA 0x2000.
    smmu.mapPage(sid, 0, 0x1000u, 0x2000u, PagePermissions{true, false, false});

    // Submit ATS Translated to address 0x9000 (not mapped) — re-translation
    // fails → F_TRANSL_FORBIDDEN.  Use ExecutePrivileged to make ind/pnu non-zero
    // in the buggy path, so the regression is clearly observable.
    TranslationResult result = smmu.translate(
        sid, 0, 0x9000u, AccessType::ExecutePrivileged, SecurityState::NonSecure,
        TransactionType::AtsTranslated);
    EXPECT_TRUE(result.isError())
        << "ATS Translated with ATSCHK=1 and re-translation failure must produce F_TRANSL_FORBIDDEN";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::F_TRANSL_FORBIDDEN);
    ASSERT_NE(ev, nullptr) << "F_TRANSL_FORBIDDEN event must be present in the event queue";

    // ARM §7.3.8: InD (bit 99) and PnU (bit 98) are RES0; RnW (bit 100) is defined.
    EXPECT_FALSE(ev->ind)
        << "BUG-CPP-1b: F_TRANSL_FORBIDDEN.ind must be 0 (RES0 per §7.3.8);"
           " got true (set from AccessType::ExecutePrivileged)";
    EXPECT_FALSE(ev->pnu)
        << "BUG-CPP-1b: F_TRANSL_FORBIDDEN.pnu must be 0 (RES0 per §7.3.8);"
           " got true (set from AccessType::ExecutePrivileged)";
}

// ============================================================================
// BUG-CPP-3: COMMAND_SYNC_COMPLETION event must have rnw=false, ind=false,
//            pnu=false.
//
// ARM IHI0070G.b §7.3.21 (catch-all): COMMAND_SYNC_COMPLETION has no defined
// wire format; all fields above StreamID are RES0.
//
// generateEvent(COMMAND_SYNC_COMPLETION, ...) is called from processCommandQueue()
// without an AccessType argument, so it defaults to AccessType::Read.
// The access-type switch sets rnw=true (Read → rnw=1).
//
// Setup: submit a CMD_SYNC with CS=1 (SIG_IRQ) and process it.
//        Retrieve the COMMAND_SYNC_COMPLETION event.
//        Assert rnw==false, ind==false, pnu==false.
//
// BEFORE FIX: rnw=true from default AccessType::Read; assert FAILS.
// AFTER FIX:  override block zeros all three; assert passes.
// ============================================================================
TEST(BugCpp22Mar2026Res0, CommandSyncCompletionHasRnwIndPnuZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xA103u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    // Submit CMD_SYNC with CS=1 (IRQ signal) → COMMAND_SYNC_COMPLETION event.
    CommandEntry cmd;
    cmd.type      = CommandType::SYNC;
    cmd.streamID  = sid;
    cmd.pasid     = 0;
    cmd.cs        = 1u;  // SIG_IRQ
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk())
        << "CMD_SYNC submission must succeed";

    smmu.processCommandQueue();

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::COMMAND_SYNC_COMPLETION);
    ASSERT_NE(ev, nullptr)
        << "COMMAND_SYNC_COMPLETION event must be present after CMD_SYNC(CS=1)";

    // ARM §7.3 (impl-defined catch-all): all upper fields are RES0.
    EXPECT_FALSE(ev->rnw)
        << "BUG-CPP-3: COMMAND_SYNC_COMPLETION.rnw must be 0 (RES0 per §7.3);"
           " got true (set from default AccessType::Read)";
    EXPECT_FALSE(ev->ind)
        << "BUG-CPP-3: COMMAND_SYNC_COMPLETION.ind must be 0 (RES0 per §7.3)";
    EXPECT_FALSE(ev->pnu)
        << "BUG-CPP-3: COMMAND_SYNC_COMPLETION.pnu must be 0 (RES0 per §7.3)";
}

// ============================================================================
// BUG-CPP-4: F_STREAM_DISABLED event must have rnw=false, ind=false, pnu=false.
//
// ARM IHI0070G.b §7.3.7: all fields above StreamID (including RnW/InD/PnU/SSV)
// are RES0 for F_STREAM_DISABLED.
//
// generateEvent(F_STREAM_DISABLED, ...) is called without an AccessType
// argument; the default is AccessType::Read → rnw=true in the switch.
//
// Setup: configure a substream-capable stream (s1CdMax>0) with S1DSS=0x00.
//        Translate without a PASID (pasid=0) — this triggers F_STREAM_DISABLED.
//        Assert rnw==false, ind==false, pnu==false.
//
// BEFORE FIX: rnw=true from default AccessType::Read; assert FAILS.
// AFTER FIX:  override block zeros all three; assert passes.
// ============================================================================
TEST(BugCpp22Mar2026Res0, FStreamDisabledHasRnwIndPnuZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xA104u;
    // Stage-1 stream, substream-capable (s1cdMax > 0), S1DSS=0 → F_STREAM_DISABLED.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s1cdMax            = 4u;   // enables substream capability (S1CDMax > 0)
    cfg.s1dss              = 0x00u; // S1DSS=0b00 → abort non-substream tx with F_STREAM_DISABLED
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    // Translate with PASID=0 (non-substream) on a substream-capable stream
    // with S1DSS=0 → F_STREAM_DISABLED.
    TranslationResult result = smmu.translate(
        sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "PASID=0 on S1DSS=0 substream-capable stream must fail with F_STREAM_DISABLED";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::F_STREAM_DISABLED);
    ASSERT_NE(ev, nullptr)
        << "F_STREAM_DISABLED event must be present (S1DSS=0, PASID=0, S1CDMax>0)";

    // ARM §7.3.7: all fields above StreamID are RES0.
    EXPECT_FALSE(ev->rnw)
        << "BUG-CPP-4: F_STREAM_DISABLED.rnw must be 0 (RES0 per §7.3.7);"
           " got true (set from default AccessType::Read)";
    EXPECT_FALSE(ev->ind)
        << "BUG-CPP-4: F_STREAM_DISABLED.ind must be 0 (RES0 per §7.3.7)";
    EXPECT_FALSE(ev->pnu)
        << "BUG-CPP-4: F_STREAM_DISABLED.pnu must be 0 (RES0 per §7.3.7)";
}

// ============================================================================
// Regression guard: F_TRANSLATION still sets rnw=true for AccessType::Read.
//
// This test must pass BOTH before and after the fixes, confirming that the
// override block additions do not accidentally suppress correct rnw values
// for normal translation faults.
// ============================================================================
TEST(BugCpp22Mar2026Res0, FTranslationRnwUnchangedForReadAccess) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xA105u;
    configureStage1Stream(smmu, sid);

    // No page mapped at 0x5000 → F_TRANSLATION with AccessType::Read → rnw=true.
    smmu.translate(sid, 0, 0x5000u, AccessType::Read, SecurityState::NonSecure);

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::F_TRANSLATION);
    ASSERT_NE(ev, nullptr) << "F_TRANSLATION event must be present for unmapped address";

    EXPECT_TRUE(ev->rnw)
        << "Regression guard: F_TRANSLATION from Read must still have rnw=true";
    EXPECT_FALSE(ev->ind)
        << "Regression guard: F_TRANSLATION from Read must have ind=false";
    EXPECT_FALSE(ev->pnu)
        << "Regression guard: F_TRANSLATION from Read must have pnu=false";
}
