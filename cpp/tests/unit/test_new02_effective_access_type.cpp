// ARM SMMU v3 — Bug NEW-02: T0SZ/EPD0 generateEvent() uses raw accessType
//                           instead of effective accessType
//
// ARM IHI0070G.b §7.3: EventEntry fields RnW, InD, and PnU must reflect the
// *post-STE-override* effective access type, not the raw incoming accessType.
// The effective access type is derived by applying, in order:
//   1. STRW override  (STRW=EL2/EL3 → promote to Privileged variants)
//   2. INSTCFG override (instCfg=1: Read→Execute; instCfg=2: Execute→Read)
//   3. PRIVCFG override (privCfg=2: demote Privileged; privCfg=3: promote)
//
// Two call sites in performTwoStageTranslation() pass raw accessType to
// generateEvent() instead of the computed effective type:
//   1. T0SZ check   — line ~1615
//   2. EPD0 check   — line ~1629
//
// Tests:
//   new02_t0sz_privcfg3_pnu_true:
//     PRIVCFG=3 (promote-to-privileged), T0SZ=16, plain Read access.
//     IOVA exceeds VA range → F_TRANSLATION.
//     pnu MUST be true (effective type = ReadPrivileged).
//     BEFORE FIX: pnu=false (raw Read used). AFTER FIX: pnu=true.
//
//   new02_epd0_privcfg3_pnu_true:
//     PRIVCFG=3 (promote-to-privileged), EPD0=1, plain Read access.
//     F_TRANSLATION due to EPD0.
//     pnu MUST be true (effective type = ReadPrivileged).
//     BEFORE FIX: pnu=false (raw Read used). AFTER FIX: pnu=true.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

using namespace smmu;

namespace {

static constexpr StreamID SID = 0xB0u;
static constexpr PASID    PID = 0u;

void enableEventQueue(SMMU& smmu) {
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Returns the last F_TRANSLATION EventEntry from the queue, or a default entry
// (pnu=false) if none is found.
EventEntry findLastFTranslation(const SMMU& smmu) {
    const auto& q = smmu.getEventQueue();
    for (auto it = q.rbegin(); it != q.rend(); ++it) {
        if (it->type == EventType::F_TRANSLATION) {
            return *it;
        }
    }
    return EventEntry(); // default: pnu=false
}

} // anonymous namespace

// =============================================================================
// Test 1: T0SZ path — PRIVCFG=3 promotes Read to ReadPrivileged → pnu must be true
//
// §7.3: PnU reflects the *effective* access type after STE overrides.
// PRIVCFG=3 (Force Privileged) promotes plain Read → ReadPrivileged.
// The T0SZ check fires before the page-table walk, so it must use the
// effective type when calling generateEvent().
//
// BEFORE FIX: generateEvent() receives raw accessType=Read → pnu=false → FAIL.
// AFTER FIX:  generateEvent() receives effectiveAccessType=ReadPrivileged → pnu=true → PASS.
// =============================================================================
TEST(New02EffectiveAccessType, new02_t0sz_privcfg3_pnu_true) {
    SMMU smmu;
    enableEventQueue(smmu);

    // Stage-1-only stream with PRIVCFG=3 (Force Privileged) and T0SZ=16
    // (VA limit = 2^48 = 0x0001_0000_0000_0000).
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 16u;  // 48-bit VA space
    cfg.privCfg            = 3u;   // Force Privileged: Read → ReadPrivileged
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, PID).isOk());

    // IOVA at exactly 2^48: out of T0SZ range → F_TRANSLATION.
    static constexpr IOVA OVER_IOVA = UINT64_C(1) << 48u;
    auto r = smmu.translate(SID, PID, OVER_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "IOVA at T0SZ limit must produce a translation error";

    EventEntry ev = findLastFTranslation(smmu);
    EXPECT_EQ(ev.type, EventType::F_TRANSLATION)
        << "F_TRANSLATION event must be generated for T0SZ violation";
    EXPECT_TRUE(ev.pnu)
        << "NEW-02: §7.3 PnU must reflect effective access type. "
           "PRIVCFG=3 promotes Read→ReadPrivileged so pnu must be true. "
           "Bug: generateEvent() passes raw accessType (Read) instead of "
           "effectiveAccessType (ReadPrivileged).";
}

// =============================================================================
// Test 2: EPD0 path — PRIVCFG=3 promotes Read to ReadPrivileged → pnu must be true
//
// EPD0=1 disables the TTBR0 translation walk → F_TRANSLATION before any
// page-table lookup.  The same effective-access-type override must apply.
//
// BEFORE FIX: generateEvent() receives raw accessType=Read → pnu=false → FAIL.
// AFTER FIX:  generateEvent() receives effectiveAccessType=ReadPrivileged → pnu=true → PASS.
// =============================================================================
TEST(New02EffectiveAccessType, new02_epd0_privcfg3_pnu_true) {
    SMMU smmu;
    enableEventQueue(smmu);

    // Stage-1-only stream with PRIVCFG=3 and EPD0=1 (TTBR0 disabled).
    // Use t0sz=0 so the T0SZ check does not fire before EPD0.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 0u;   // no VA range restriction
    cfg.epd0               = true; // TTBR0 walk disabled → F_TRANSLATION
    cfg.privCfg            = 3u;   // Force Privileged: Read → ReadPrivileged
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, PID).isOk());

    static constexpr IOVA ANY_IOVA = 0x1000ULL;
    auto r = smmu.translate(SID, PID, ANY_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "EPD0=1 must produce a translation error";

    EventEntry ev = findLastFTranslation(smmu);
    EXPECT_EQ(ev.type, EventType::F_TRANSLATION)
        << "F_TRANSLATION event must be generated for EPD0 fault";
    EXPECT_TRUE(ev.pnu)
        << "NEW-02: §7.3 PnU must reflect effective access type. "
           "PRIVCFG=3 promotes Read→ReadPrivileged so pnu must be true. "
           "Bug: generateEvent() passes raw accessType (Read) instead of "
           "effectiveAccessType (ReadPrivileged).";
}
