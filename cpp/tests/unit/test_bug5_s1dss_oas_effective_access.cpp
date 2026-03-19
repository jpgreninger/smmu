// ARM SMMU v3 — BUG-5: S1DSS==0x01 OAS fault event uses raw accessType
//                      instead of effectiveAccessType
//
// ARM IHI0070G.b §7.3.14: "The InD/PnU attributes are the post-STE override
// values."  In performTwoStageTranslation(), the S1DSS==0x01 OAS overflow path
// calls generateEvent(F_ADDR_SIZE, ..., accessType) BEFORE the effectiveAccessType
// computation block (the three-step STRW→INSTCFG→PRIVCFG override chain).
// Consequently, for streams with STRW=EL2/EL3 or PRIVCFG=3, the pnu/ind/rnw
// fields in the emitted F_ADDR_SIZE event are wrong.
//
// Fix: move the effectiveAccessType computation to before the S1DSS block, and
// replace accessType with effectiveAccessType at the generateEvent() call site
// inside the s1dss == 0x01 OAS overflow path.
//
// Tests:
//   bug5_s1dss01_privcfg3_oas_fault_pnu_true:
//     Stage-1 stream with S1DSS=0x01, PRIVCFG=3 (Force Privileged),
//     s1cdMax=4.  PASID=0, plain Read, IOVA >= 2^52 (OAS overflow).
//     Expected: F_ADDR_SIZE emitted, pnu=true.
//     BEFORE FIX: pnu=false (raw Read used). AFTER FIX: pnu=true.
//
//   bug5_s1dss01_strw_el2_oas_fault_pnu_true:
//     Stage-1 stream with S1DSS=0x01, STRW=EL2, s1cdMax=4.
//     PASID=0, plain Read, IOVA >= 2^52 (OAS overflow).
//     Expected: F_ADDR_SIZE emitted, pnu=true.
//     BEFORE FIX: pnu=false (raw Read used). AFTER FIX: pnu=true.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

using namespace smmu;

namespace {

static constexpr StreamID SID_A = 0xC0u;
static constexpr StreamID SID_B = 0xC1u;
static constexpr PASID    PID   = 0u;

// IOVA that exceeds the default 52-bit OAS (2^52 = 0x0010_0000_0000_0000).
static constexpr IOVA OVER_OAS_IOVA = UINT64_C(1) << 52u;

void enableEventQueue(SMMU& smmu) {
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Return the most recent F_ADDR_SIZE EventEntry from the queue, or a
// default-constructed one (pnu=false) if none is found.
EventEntry findLastFAddrSize(const SMMU& smmu) {
    const auto& q = smmu.getEventQueue();
    for (auto it = q.rbegin(); it != q.rend(); ++it) {
        if (it->type == EventType::F_ADDR_SIZE) {
            return *it;
        }
    }
    return EventEntry();
}

} // anonymous namespace

// =============================================================================
// Test 1: S1DSS==0x01, PRIVCFG=3 (Force Privileged), OAS overflow → pnu=true
//
// §7.3.14: PnU must reflect the *post-STE-override* effective access type.
// PRIVCFG=3 promotes plain Read → ReadPrivileged, so pnu must be true.
// The S1DSS==0x01 OAS-overflow path calls generateEvent() before the
// effectiveAccessType computation, so it uses raw Read → pnu=false (BUG).
//
// Setup:
//   - stage1Enabled=true, stage2Enabled=false
//   - s1dss=0x01 (bypass stage-1 for non-substream), s1cdMax=4
//   - privCfg=3 (Force Privileged)
//   - t0sz=0 (no VA-range restriction, so T0SZ check does not fire first)
//   - IOVA = 2^52 (exceeds 52-bit OAS) → S1DSS bypass OAS check fires
//
// BEFORE FIX: generateEvent() gets raw accessType=Read → pnu=false → FAIL.
// AFTER  FIX: generateEvent() gets effectiveAccessType=ReadPrivileged → pnu=true → PASS.
// =============================================================================
TEST(Bug5S1dssOasEffectiveAccess, bug5_s1dss01_privcfg3_oas_fault_pnu_true) {
    SMMU smmu;
    enableEventQueue(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = 0u;    // no VA range restriction
    cfg.privCfg            = 3u;    // Force Privileged: Read → ReadPrivileged
    cfg.s1dss              = 0x01u; // S1DSS=0b01: non-substream PASID=0 bypasses stage-1
    cfg.s1cdMax            = 4u;    // stream is substream-capable → s1dss active
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu.configureStream(SID_A, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID_A).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID_A, PID).isOk());

    // PASID=0 on a substream-capable stream → S1DSS==0x01 bypass path fires.
    // IOVA exceeds default 52-bit OAS → OAS overflow check fires → F_ADDR_SIZE.
    auto r = smmu.translate(SID_A, PID, OVER_OAS_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "OAS overflow on S1DSS==0x01 bypass must produce a translation error";

    EventEntry ev = findLastFAddrSize(smmu);
    EXPECT_EQ(ev.type, EventType::F_ADDR_SIZE)
        << "F_ADDR_SIZE event must be generated for OAS overflow in S1DSS==0x01 path";
    EXPECT_TRUE(ev.pnu)
        << "BUG-5: §7.3.14 PnU must reflect the effective (post-STE-override) access "
           "type. PRIVCFG=3 promotes Read→ReadPrivileged so pnu must be true. "
           "Before fix: generateEvent() passes raw accessType (Read) instead of "
           "effectiveAccessType (ReadPrivileged) because the effectiveAccessType "
           "computation block comes AFTER the S1DSS OAS fault site.";
}

// =============================================================================
// Test 2: S1DSS==0x01, STRW=EL2, OAS overflow → pnu=true
//
// §7.3.14: PnU must reflect the *post-STE-override* effective access type.
// STRW=EL2 promotes plain Read → ReadPrivileged (for stage-1-only streams),
// so pnu must be true.  Same S1DSS==0x01 OAS path fires before STRW override
// is applied.
//
// Setup:
//   - stage1Enabled=true, stage2Enabled=false (STRW is honoured when S2 absent)
//   - s1dss=0x01, s1cdMax=4
//   - strw=EL2
//   - t0sz=0
//   - IOVA = 2^52 → OAS overflow
//
// BEFORE FIX: generateEvent() gets raw accessType=Read → pnu=false → FAIL.
// AFTER  FIX: generateEvent() gets effectiveAccessType=ReadPrivileged → pnu=true → PASS.
// =============================================================================
TEST(Bug5S1dssOasEffectiveAccess, bug5_s1dss01_strw_el2_oas_fault_pnu_true) {
    SMMU smmu;
    enableEventQueue(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false; // STRW promotion applies only when S2 absent
    cfg.t0sz               = 0u;    // no VA range restriction
    cfg.strw               = StreamWorld::EL2; // STRW=EL2: Read → ReadPrivileged
    cfg.s1dss              = 0x01u; // S1DSS=0b01: non-substream PASID=0 bypasses stage-1
    cfg.s1cdMax            = 4u;    // stream is substream-capable → s1dss active
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu.configureStream(SID_B, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID_B).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID_B, PID).isOk());

    auto r = smmu.translate(SID_B, PID, OVER_OAS_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "OAS overflow on S1DSS==0x01 bypass must produce a translation error";

    EventEntry ev = findLastFAddrSize(smmu);
    EXPECT_EQ(ev.type, EventType::F_ADDR_SIZE)
        << "F_ADDR_SIZE event must be generated for OAS overflow in S1DSS==0x01 path";
    EXPECT_TRUE(ev.pnu)
        << "BUG-5: §7.3.14 PnU must reflect the effective (post-STE-override) access "
           "type. STRW=EL2 promotes Read→ReadPrivileged so pnu must be true. "
           "Before fix: generateEvent() passes raw accessType (Read) instead of "
           "effectiveAccessType (ReadPrivileged) because the effectiveAccessType "
           "computation block comes AFTER the S1DSS OAS fault site.";
}
