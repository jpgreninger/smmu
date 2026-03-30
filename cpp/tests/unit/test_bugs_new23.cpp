// TDD failing tests for BUG-AUDIT-44, BUG-AUDIT-45, and BUG-AUDIT-46.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-44 (§5.2): S1CDMax > SSIDSIZE silently drops C_BAD_STE event
//
//   ARM IHI0070G.b §5.2 SteIllegal():
//     "if UInt(STE.S1CDMax) > UInt(SMMU_IDR1.SSIDSIZE) then return TRUE"
//     SSIDSIZE == 20.
//   Current code: isConfigurationValid() rejects s1cdMax > 20 but never calls
//   generateEvent(), so no C_BAD_STE is enqueued.
//
//   BEFORE FIX: stage1Enabled=true, s1cdMax=21 → rejection but NO event → FAIL.
//   AFTER FIX:  C_BAD_STE is emitted AND rejection is preserved → PASS.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-45 (§5.2): S2T0SZ out-of-range accepted silently
//
//   ARM IHI0070G.b §5.2 STES2T0SZInvalid():
//     STT=0, 4KB granule, IAS=48 → valid S2T0SZ range is [16, 39].
//   Current code: no range check exists; non-zero values in [1, 15] and [40, 255]
//   are accepted silently, leading to undefined translation behavior.
//
//   Model convention: s2t0sz==0 is a "no IPA range restriction" sentinel used
//   throughout the model (guarded by `if (s2t0sz > 0u)` in translation paths).
//   The fix only rejects non-zero values outside [16, 39].
//
//   BEFORE FIX: stage2Enabled=true, s2t0sz=10 → accepted, no event → FAIL.
//   AFTER FIX:  C_BAD_STE emitted and stream rejected → PASS.
//   s2t0sz=0 continues to be accepted (sentinel value, not architecturally invalid).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-AUDIT-46 (§6.3.1): setStallModel(0b11) stores reserved value
//
//   ARM IHI0070G.b §6.3.1 IDR0 bits[25:24]:
//     0b00 = stall+terminate, 0b01 = terminate-only, 0b10 = stall-only,
//     0b11 = RESERVED.
//   Current code: setStallModel() accepts any uint8_t value including 3
//   (0b11 = reserved) and stores it directly.
//
//   BEFORE FIX: setStallModel(3) → getStallModel() returns 3 → FAIL.
//   AFTER FIX:  setStallModel(3) is a no-op; prior value preserved → PASS.
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"

using namespace smmu;

// ─── BUG-AUDIT-44 tests ──────────────────────────────────────────────────────

// Negative: stage1Enabled=true, s1cdMax=21 (>SSIDSIZE=20) → C_BAD_STE event.
// Current code rejects the configuration but does NOT emit the event.
// BEFORE FIX: no event → test FAILS.
// AFTER FIX:  C_BAD_STE is enqueued for streamID → test PASSES.
TEST(BugAudit44, S1CdMaxExceedsSsidSize_EmitsEvent) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.s1cdMax            = 21u;  // > SSIDSIZE(20) — SteIllegal per §5.2
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(100u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-44: s1cdMax=21 > SSIDSIZE=20 must be rejected (ARM §5.2 SteIllegal)";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 100u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-44: C_BAD_STE must be generated when s1cdMax > SSIDSIZE=20 (ARM §5.2)";
}

// Regression: stage1Enabled=true, s1cdMax=21 → configureStream returns error
// (stream not configured).  This exercises the existing rejection path.
// Passes before and after the fix — serves as regression guard.
TEST(BugAudit44, S1CdMaxExceedsSsidSize_Rejected) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.s1cdMax            = 21u;  // > SSIDSIZE(20)
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(101u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-44: s1cdMax=21 must be rejected (regression guard)";
}

// Positive: stage1Enabled=true, s1cdMax=20 (== SSIDSIZE) → must succeed.
// Boundary value that is exactly at the limit — must not be rejected.
TEST(BugAudit44, S1CdMaxAtLimit_Accepted) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.s1cdMax            = 20u;  // == SSIDSIZE(20) — boundary: must be accepted
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 0u;

    VoidResult result = s.configureStream(102u, cfg);
    EXPECT_FALSE(result.isError())
        << "BUG-AUDIT-44: s1cdMax=20 == SSIDSIZE=20 must be accepted (boundary, ARM §5.2)";
}

// ─── BUG-AUDIT-45 tests ──────────────────────────────────────────────────────

// Helper: build a minimal valid stage-2-only StreamConfig.
// s2t0sz is left at the caller's chosen value; all other fields are legal.
static StreamConfig makeStage2Config(uint8_t s2t0sz) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2aa64             = true;   // required (BUG-AUDIT-42 fix rejects s2aa64=false)
    cfg.s2ps               = 5u;     // 48-bit output PA size
    cfg.s2t0sz             = s2t0sz;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 10u;
    return cfg;
}

// Negative: stage2Enabled=true, s2t0sz=10 (< 16) → C_BAD_STE event.
// BEFORE FIX: no range check → accepted silently → event queue empty → FAIL.
// AFTER FIX:  C_BAD_STE emitted, stream rejected → PASS.
TEST(BugAudit45, S2T0SzBelowRange_EmitsEvent) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg = makeStage2Config(10u);  // below [16, 39]
    VoidResult result = s.configureStream(110u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-45: s2t0sz=10 < 16 must be rejected (ARM §5.2 STES2T0SZInvalid)";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 110u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-45: C_BAD_STE must be generated for s2t0sz=10 < 16 (ARM §5.2)";
}

// Negative: stage2Enabled=true, s2t0sz=40 (> 39) → C_BAD_STE event.
// BEFORE FIX: no range check → accepted silently → event queue empty → FAIL.
// AFTER FIX:  C_BAD_STE emitted, stream rejected → PASS.
TEST(BugAudit45, S2T0SzAboveRange_EmitsEvent) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg = makeStage2Config(40u);  // above [16, 39]
    VoidResult result = s.configureStream(111u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-45: s2t0sz=40 > 39 must be rejected (ARM §5.2 STES2T0SZInvalid)";

    std::vector<EventEntry> events = s.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE && ev.streamID == 111u) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-AUDIT-45: C_BAD_STE must be generated for s2t0sz=40 > 39 (ARM §5.2)";
}

// Regression: stage2Enabled=true, s2t0sz=10 → configureStream returns error.
// Tests that the stream is actually rejected (not just missing the event).
TEST(BugAudit45, S2T0SzBelowRange_Rejected) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg = makeStage2Config(10u);
    VoidResult result = s.configureStream(112u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-45: s2t0sz=10 must be rejected (regression guard)";
}

// Regression: stage2Enabled=true, s2t0sz=40 → configureStream returns error.
TEST(BugAudit45, S2T0SzAboveRange_Rejected) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    StreamConfig cfg = makeStage2Config(40u);
    VoidResult result = s.configureStream(113u, cfg);
    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-45: s2t0sz=40 must be rejected (regression guard)";
}

// Positive: stage2Enabled=true, s2t0sz=16 and s2t0sz=39 → must both succeed.
// Both boundary values of the valid range [16, 39] must be accepted.
// Also tests s2t0sz=0 (model sentinel for "no restriction") is accepted.
TEST(BugAudit45, S2T0SzAtBoundaries_Accepted) {
    SMMU s;
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN | SMMU::CR0_PRIQEN);
    s.setS2PSupported(true);

    // Lower boundary: s2t0sz == 16
    StreamConfig cfg_lo = makeStage2Config(16u);
    cfg_lo.vmid = 20u;
    VoidResult result_lo = s.configureStream(114u, cfg_lo);
    EXPECT_FALSE(result_lo.isError())
        << "BUG-AUDIT-45: s2t0sz=16 (lower boundary) must be accepted (ARM §5.2)";

    // Upper boundary: s2t0sz == 39
    StreamConfig cfg_hi = makeStage2Config(39u);
    cfg_hi.vmid = 21u;
    VoidResult result_hi = s.configureStream(115u, cfg_hi);
    EXPECT_FALSE(result_hi.isError())
        << "BUG-AUDIT-45: s2t0sz=39 (upper boundary) must be accepted (ARM §5.2)";

    // Sentinel value: s2t0sz == 0 means "no IPA range restriction" in this model.
    // The fix must not reject this value; it is a pre-existing model convention.
    StreamConfig cfg_z = makeStage2Config(0u);
    cfg_z.vmid = 22u;
    VoidResult result_z = s.configureStream(116u, cfg_z);
    EXPECT_FALSE(result_z.isError())
        << "BUG-AUDIT-45: s2t0sz=0 (sentinel: no restriction) must be accepted by the model";
}

// ─── BUG-AUDIT-46 tests ──────────────────────────────────────────────────────

// Negative: setStallModel(3) must be silently ignored (0b11 = Reserved).
// BEFORE FIX: value 3 is stored → getStallModel() returns 3 → test FAILS.
// AFTER FIX:  value 3 is rejected; prior value (0) is preserved → test PASSES.
TEST(BugAudit46, SetStallModelReservedValueIgnored) {
    SMMU s;
    // Default stallModel is 0.  Calling setStallModel(3) must be a no-op.
    s.setStallModel(3u);
    EXPECT_EQ(s.getStallModel(), 0u)
        << "BUG-AUDIT-46: setStallModel(3) must be ignored; 0b11 is Reserved per ARM §6.3.1 IDR0[25:24]";
}

// Positive: setStallModel(0), setStallModel(1), setStallModel(2) all accepted.
// These are the three architecturally defined values.
TEST(BugAudit46, SetStallModelValidValues) {
    SMMU s;

    s.setStallModel(0u);
    EXPECT_EQ(s.getStallModel(), 0u)
        << "BUG-AUDIT-46: setStallModel(0) must be accepted (stall+terminate, ARM §6.3.1)";

    s.setStallModel(1u);
    EXPECT_EQ(s.getStallModel(), 1u)
        << "BUG-AUDIT-46: setStallModel(1) must be accepted (terminate-only, ARM §6.3.1)";

    s.setStallModel(2u);
    EXPECT_EQ(s.getStallModel(), 2u)
        << "BUG-AUDIT-46: setStallModel(2) must be accepted (stall-only, ARM §6.3.1)";
}

// Negative: after setStallModel(3), IDR0 bits[25:24] must be 0b00, not 0b11.
// Even if the reserved value somehow reached the IDR0 register the bits must not
// expose a reserved encoding to software.
// BEFORE FIX: IDR0[25:24] would be 0b11 → test FAILS.
// AFTER FIX:  IDR0[25:24] remains 0b00 (prior value) → test PASSES.
TEST(BugAudit46, GetIdr0StallModelNotReserved) {
    SMMU s;
    s.setStallModel(3u);  // must be silently rejected

    const uint32_t idr0 = s.getIDR0();
    const uint32_t stall_model_bits = (idr0 >> 24) & 0x3u;
    EXPECT_NE(stall_model_bits, 3u)
        << "BUG-AUDIT-46: IDR0 STALL_MODEL[25:24] must not be 0b11 (Reserved) after setStallModel(3)";
    EXPECT_EQ(stall_model_bits, 0u)
        << "BUG-AUDIT-46: IDR0 STALL_MODEL[25:24] must remain 0b00 (prior value) after rejected setStallModel(3)";
}
