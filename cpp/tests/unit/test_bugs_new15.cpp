// TDD failing tests for BUG-AUDIT-01, BUG-AUDIT-02, and BUG-AUDIT-03.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-01 (Both §5.2 SteIllegal()): EATS==0b10 missing NS1ATS check
//
//   ARM IHI0070G.b §5.2 SteIllegal() pseudocode:
//     if STE.EATS == '10' && NS1ATS == '1' then
//         return TRUE;  // → C_BAD_STE
//
//   When IDR0.NS1ATS==1 AND EATS==0b10 the configuration is ILLEGAL, even for a
//   two-stage stream with S2S==0 (which would otherwise be legal per BUG-NEW-J).
//
//   Requires new API: setNS1ATSSupported(bool).
//
//   Current behavior (WRONG): no setNS1ATSSupported() method exists; configureStream()
//   does not check NS1ATS against EATS, so the stream is silently accepted.
//
//   BEFORE FIX: setNS1ATSSupported() won't compile (method missing) → test 1 FAILS.
//   AFTER FIX:  method exists, C_BAD_STE emitted → test 1 PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-02 (Both §6.3.4 IDR3): XNX (bit 4) not gated on S2P
//
//   ARM IHI0070G.b §6.3.4 IDR3 bit 4 (XNX):
//     "This bit is only valid when S2P=1."
//     When IDR0.S2P==0, IDR3.XNX must be 0.
//
//   Current behavior (WRONG): getIDR3() always returns bit 4 set regardless of
//   the setS2PSupported() flag.
//
//   BEFORE FIX: getIDR3() bit 4 == 1 even when S2P=0 → test 1 FAILS (expects 0).
//   AFTER FIX:  getIDR3() bit 4 == 0 when S2P=0 → test 1 PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-03 (Both §4.5.2): CMD_PRI_RESP not silently ignored when SMMUEN==0
//
//   ARM IHI0070G.b §4.5.2 lines 6075-6079:
//     if SMMU_CR0.SMMUEN == '0' then RETURN;   // silently ignore
//
//   When SMMUEN=0, CMD_PRI_RESP must be silently ignored (no CERROR_ILL, no PRG
//   consumption, no change to PRIQ_CONS).
//
//   Current behavior (WRONG): processCommandQueue() gates only on CMDQEN, not
//   SMMUEN.  After disable(), CMD_PRI_RESP is still executed, consuming the head
//   PRQ entry and advancing PRIQ_CONS instead of being silently discarded.
//
//   Test strategy:
//     1. Enable SMMU fully (SMMUEN=1, CMDQEN=1, PRIQEN=1).
//     2. Configure a stream with eats=1 (ATS enabled) so submitPageRequest works.
//     3. Submit a page request and call processPRIQueue() to emit E_PAGE_REQUEST
//        (entry remains in queue, priq_emitted advances).
//     4. Disable SMMUEN (SMMUEN=0) while keeping CMDQEN=1.
//     5. Submit CMD_PRI_RESP and call processCommandQueue().
//     6. Expected: queue size unchanged (PRG entry not consumed); no CERROR_ILL.
//
//   BEFORE FIX: processCommandQueue() processes CMD_PRI_RESP even with SMMUEN=0,
//               consuming the PRG entry → getPRIQueueSize() drops to 0 → FAILS.
//   AFTER FIX:  CMD_PRI_RESP is silently ignored → PRG entry remains → PASSES.
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

// Disable only SMMUEN; keep CMDQEN and PRIQEN active so the command queue
// continues to accept submissions but SMMUEN-gated commands are no-ops.
static void disableSMMUen(SMMU& s) {
    uint32_t cr0 = s.getCR0();
    cr0 &= ~static_cast<uint32_t>(SMMU::CR0_SMMUEN);
    s.setCR0(cr0);
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

// Build a basic two-stage stream configuration with eats field.
static StreamConfig makeTwoStageConfig(uint8_t eats, bool s2s = false) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = eats;
    cfg.s2s                = s2s;
    cfg.t0sz               = 0;
    return cfg;
}

// Build a stage-1-only stream configuration with ATS enabled (eats=1).
static StreamConfig makeStage1ATSConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.eats               = 1u;  // ATS enabled — required for submitPageRequest
    cfg.s2s                = false;
    cfg.t0sz               = 0;
    return cfg;
}

// Submit a page request to generate a pending PRG entry in the PRI queue.
// Returns true if the submission succeeded.
static bool submitAndEmitPageRequest(SMMU& smmu, uint32_t sid, uint16_t prgIdx) {
    PRIEntry req;
    req.streamID         = sid;
    req.pasid            = 0u;
    req.requestedAddress = 0x1000u;
    req.accessType       = AccessType::Read;
    req.isLastRequest    = true;
    req.prgIndex         = prgIdx;
    req.securityState    = SecurityState::NonSecure;
    smmu.submitPageRequest(req);
    // processPRIQueue emits E_PAGE_REQUEST but leaves the entry in the PRI queue.
    smmu.processPRIQueue();
    return smmu.getPRIQueueSize() > 0u;
}

// Build and submit a CMD_PRI_RESP for the given stream/pasid/prgIdx.
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

} // anonymous namespace

// ============================================================================
// BUG-AUDIT-01: EATS==0b10 + NS1ATS==1 → C_BAD_STE (ARM §5.2 SteIllegal)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: NS1ATS=1, EATS=2, two-stage, S2S=0 → C_BAD_STE
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §5.2 SteIllegal(): EATS==0b10 (split-stage ATS) is illegal when
// IDR0.NS1ATS==1, even for a two-stage stream with S2S==0.
//
// BEFORE FIX: setNS1ATSSupported() won't compile (method missing) → FAILS.
// AFTER FIX:  C_BAD_STE emitted → PASSES.
TEST(BugAudit01_NS1ATSEats, NS1ATS1_Eats2_TwoStage_EmitsCBadSte) {
    // §5.2 SteIllegal(): NS1ATS==1 AND EATS==0b10 → C_BAD_STE.
    SMMU smmu;
    enableSMMU(smmu);

    // Enable NS1ATS feature (IDR0.NS1ATS=1).
    // Requires new API: setNS1ATSSupported(bool).
    smmu.setNS1ATSSupported(true);

    // EATS=2 (split-stage ATS), two-stage Config, S2S=0.
    // Without the NS1ATS check this would be accepted (BUG-NEW-J regression test
    // shows EATS=2 + two-stage + S2S=0 is legal when NS1ATS==0).
    StreamConfig cfg = makeTwoStageConfig(/*eats=*/2u, /*s2s=*/false);

    VoidResult result = smmu.configureStream(0x50u, cfg);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-01: configureStream() with NS1ATS=1 AND eats=2 must return "
           "an error (ARM §5.2 SteIllegal: EATS==0b10 AND NS1ATS==1 is illegal). "
           "Current code: no setNS1ATSSupported() method exists.";

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-AUDIT-01: C_BAD_STE event must be emitted when NS1ATS=1 AND eats=2 "
           "(ARM §5.2 SteIllegal()). Current code: no NS1ATS+EATS validation.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (baseline): NS1ATS=0, EATS=2, two-stage, S2S=0 → accepted (no error)
// ─────────────────────────────────────────────────────────────────────────────
//
// When NS1ATS==0, EATS=2 + two-stage + S2S=0 is legal (per BUG-NEW-J regression).
// This test verifies the new NS1ATS guard fires ONLY when NS1ATS==1.
//
// BEFORE FIX: (fails to compile because setNS1ATSSupported() is missing).
// AFTER FIX:  config accepted, no C_BAD_STE → PASSES.
TEST(BugAudit01_NS1ATSEats, NS1ATS0_Eats2_TwoStage_Accepted) {
    // Baseline: NS1ATS=0 AND eats=2 AND two-stage AND S2S=0 must be ACCEPTED.
    // ARM §5.2 SteIllegal(): the NS1ATS+EATS check only fires when NS1ATS==1.
    SMMU smmu;
    enableSMMU(smmu);

    // Disable NS1ATS (or do not set it — the default must be NS1ATS=0).
    smmu.setNS1ATSSupported(false);

    StreamConfig cfg = makeTwoStageConfig(/*eats=*/2u, /*s2s=*/false);

    VoidResult result = smmu.configureStream(0x51u, cfg);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-01 baseline: configureStream() with NS1ATS=0 AND eats=2 AND "
           "two-stage AND S2S=0 must be ACCEPTED (ARM §5.2 SteIllegal: the NS1ATS "
           "guard fires only when NS1ATS==1). The fix must not over-reject this config.";

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-AUDIT-01 baseline: no C_BAD_STE expected for NS1ATS=0 + eats=2 + "
           "two-stage + S2S=0.";
}

// ============================================================================
// BUG-AUDIT-02: IDR3.XNX (bit 4) not gated on S2P (ARM §6.3.4)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: S2P disabled → IDR3 bit 4 (XNX) must be 0
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.4 IDR3 bit 4 (XNX): "RES0 when S2P==0."
// When setS2PSupported(false) is called, getIDR3() must clear bit 4.
//
// BEFORE FIX: getIDR3() returns bit 4 == 1 even when S2P=0 → FAILS.
// AFTER FIX:  getIDR3() returns bit 4 == 0 when S2P=0 → PASSES.
TEST(BugAudit02_IDR3XNX, S2PDisabled_IDR3Bit4_IsZero) {
    // §6.3.4: IDR3.XNX (bit 4) must be 0 when IDR0.S2P==0.
    SMMU smmu;
    enableSMMU(smmu);

    // Disable stage-2 translation (S2P=0).
    smmu.setS2PSupported(false);

    const uint32_t idr3 = smmu.getIDR3();

    EXPECT_EQ(idr3 & (1u << 4u), 0u)
        << "BUG-AUDIT-02: getIDR3() bit 4 (XNX) must be 0 when S2P==0 "
           "(ARM §6.3.4: XNX is RES0 when IDR0.S2P=0). "
           "Current code: getIDR3() always sets bit 4 regardless of S2P setting. "
           "Actual IDR3 value: 0x" << std::hex << idr3;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (regression): S2P enabled (default) → IDR3 bit 4 (XNX) must be 1
// ─────────────────────────────────────────────────────────────────────────────
//
// When S2P=1 (default), IDR3.XNX must still be 1 (mandatory SMMUv3.1+ feature
// when S2P is supported).  This test verifies the fix is properly scoped.
//
// BEFORE FIX: (already passes — bit 4 is always 1 today).
// AFTER FIX:  must still pass — regression guard.
TEST(BugAudit02_IDR3XNX, S2PEnabled_IDR3Bit4_IsOne) {
    // Regression: S2P enabled (default) → IDR3.XNX (bit 4) must be 1.
    // ARM §6.3.4: XNX is mandatory when S2P=1 for SMMUv3.1+.
    SMMU smmu;
    enableSMMU(smmu);

    // Enable stage-2 translation (S2P=1, default).
    smmu.setS2PSupported(true);

    const uint32_t idr3 = smmu.getIDR3();

    EXPECT_NE(idr3 & (1u << 4u), 0u)
        << "BUG-AUDIT-02 regression: getIDR3() bit 4 (XNX) must be 1 when S2P==1 "
           "(ARM §6.3.4: XNX is mandatory for SMMUv3.1+ when S2P=1). "
           "The fix must not clear XNX when S2P is enabled. "
           "Actual IDR3 value: 0x" << std::hex << idr3;
}

// ============================================================================
// BUG-AUDIT-03: CMD_PRI_RESP not silently ignored when SMMUEN==0 (ARM §4.5.2)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: SMMUEN=0, CMD_PRI_RESP submitted → PRG entry NOT consumed
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §4.5.2 lines 6075-6079:
//   if SMMU_CR0.SMMUEN == '0' then RETURN;  // silent ignore — no side-effects
//
// When SMMUEN=0, CMD_PRI_RESP must be silently discarded:
//   - No CERROR_ILL.
//   - PRIQ_CONS must NOT advance.
//   - The pending PRG entry must remain in the PRI queue.
//
// BEFORE FIX: processCommandQueue() gates only on CMDQEN (not SMMUEN),
//             so CMD_PRI_RESP executes, consumes the PRG entry, and
//             getPRIQueueSize() drops to 0 → FAILS.
// AFTER FIX:  CMD_PRI_RESP is silently ignored → PRG entry remains → PASSES.
TEST(BugAudit03_PriRespSMMUEN0, SMMUEN0_PriResp_PRGEntryNotConsumed) {
    // §4.5.2: CMD_PRI_RESP must be silently ignored when SMMUEN==0.
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1, CMDQEN=1, PRIQEN=1

    const uint32_t sid    = 0x60u;
    const uint16_t prgIdx = 0u;

    // Configure a stream with ATS enabled so the page request is accepted.
    smmu.configureStream(sid, makeStage1ATSConfig());
    smmu.enableStream(sid);

    // Submit a page request and emit E_PAGE_REQUEST.
    // After processPRIQueue(), the entry remains in the PRI queue (not consumed).
    bool prqPresent = submitAndEmitPageRequest(smmu, sid, prgIdx);
    ASSERT_TRUE(prqPresent)
        << "BUG-AUDIT-03 setup: PRI entry must be present in queue after submission";

    const size_t queueSizeBefore = smmu.getPRIQueueSize();
    ASSERT_GT(queueSizeBefore, 0u)
        << "BUG-AUDIT-03 setup: PRI queue must be non-empty before CMD_PRI_RESP";

    // Disable SMMUEN while keeping CMDQEN=1 (command queue remains active).
    disableSMMUen(smmu);
    ASSERT_FALSE(smmu.isEnabled())
        << "BUG-AUDIT-03 setup: SMMU must be disabled (SMMUEN=0) for this test";

    // Submit CMD_PRI_RESP — must be silently ignored because SMMUEN=0.
    submitPriResp(smmu, sid, /*pasid=*/0u, prgIdx);

    // The PRG entry must still be present: PRIQ_CONS must NOT have advanced.
    const size_t queueSizeAfter = smmu.getPRIQueueSize();
    EXPECT_EQ(queueSizeAfter, queueSizeBefore)
        << "BUG-AUDIT-03: CMD_PRI_RESP with SMMUEN=0 must NOT consume the PRG "
           "entry (ARM §4.5.2 lines 6075-6079: when SMMUEN=0, CMD_PRI_RESP is "
           "silently ignored with no side-effects). "
           "Current code: processCommandQueue() gates on CMDQEN only, so "
           "CMD_PRI_RESP executes and advances PRIQ_CONS even when SMMUEN=0. "
           "Queue size before: " << queueSizeBefore
        << ", after: " << queueSizeAfter;

    // No CERROR_ILL must be raised (silent ignore, not an error).
    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-AUDIT-03: CMD_PRI_RESP with SMMUEN=0 must NOT raise CERROR_ILL "
           "(ARM §4.5.2: the command is silently ignored, not flagged as illegal). "
           "Current code: if the command were reached, it might raise errors.";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (baseline): SMMUEN=1, CMD_PRI_RESP submitted → PRG entry consumed normally
// ─────────────────────────────────────────────────────────────────────────────
//
// When SMMUEN=1 (normal operation), CMD_PRI_RESP must be processed normally
// and consume the matching PRG entry.  This verifies the fix is scoped to
// SMMUEN=0 only and does not break normal operation.
//
// BEFORE FIX: (already passes — CMD_PRI_RESP is always processed today).
// AFTER FIX:  must still pass — regression guard.
TEST(BugAudit03_PriRespSMMUEN0, SMMUEN1_PriResp_PRGEntryConsumed) {
    // Regression: SMMUEN=1 → CMD_PRI_RESP must be processed normally.
    // ARM §4.5.2: the SMMUEN=0 guard must not fire when SMMUEN=1.
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1, CMDQEN=1, PRIQEN=1

    const uint32_t sid    = 0x61u;
    const uint16_t prgIdx = 0u;

    // Configure stream with ATS enabled.
    smmu.configureStream(sid, makeStage1ATSConfig());
    smmu.enableStream(sid);

    // Submit page request and emit E_PAGE_REQUEST.
    bool prqPresent = submitAndEmitPageRequest(smmu, sid, prgIdx);
    ASSERT_TRUE(prqPresent)
        << "BUG-AUDIT-03 regression setup: PRI entry must be present";

    const size_t queueSizeBefore = smmu.getPRIQueueSize();

    // SMMU is enabled (SMMUEN=1) — CMD_PRI_RESP must be processed normally.
    submitPriResp(smmu, sid, /*pasid=*/0u, prgIdx);

    // The PRG entry must have been consumed (queue shrinks).
    const size_t queueSizeAfter = smmu.getPRIQueueSize();
    EXPECT_LT(queueSizeAfter, queueSizeBefore)
        << "BUG-AUDIT-03 regression: CMD_PRI_RESP with SMMUEN=1 must consume the "
           "matching PRG entry (ARM §4.5.2 normal operation). "
           "The fix must not prevent CMD_PRI_RESP from executing when SMMUEN=1. "
           "Queue size before: " << queueSizeBefore
        << ", after: " << queueSizeAfter;
}
