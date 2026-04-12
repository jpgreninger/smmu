// TDD failing test for BUG-13.1.4-CPP-A: ATOS must not apply INSTCFG/PRIVCFG overrides.
//
// ARM §13.1.4: ATOS (Address Translation Operation via GATOS/CMD_GATOS) must use the
// raw incoming access type and must NOT have it modified by STE.INSTCFG or STE.PRIVCFG.
//
// BUG: gatosTranslate() calls translate() which calls performTwoStageTranslation()
// which applies INSTCFG/PRIVCFG overrides to the access type before the page walk.
// For GATOS transactions, the access type must be passed through unchanged.
//
// Demonstration:
//   - Stream: instCfg=3 (Force Instruction: Read→Execute)
//   - Page: execute-only (read=false, write=false, execute=true)
//   - Normal translate(Read) → INSTCFG promotes Read→Execute → page has execute=true → SUCCESS
//     (This is correct for normal translation)
//   - gatosTranslate(Read) → INSTCFG must NOT promote Read→Execute → Read stays Read
//                         → page has read=false → FAULT (permission denied)
//     (Before fix: gatosTranslate incorrectly promotes Read→Execute and succeeds)
//     (After  fix: gatosTranslate skips INSTCFG, Read stays Read, faults correctly)
//
// These tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID SID_ATOS = 0xE0u;
static constexpr IOVA     ATOS_IOVA = 0x4000u;
static constexpr PA       ATOS_PA   = 0x5000u;

static bool setupAtosStream(SMMU& smmu, StreamID sid)
{
    smmu.enable();
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    cfg.instCfg            = 3u; // Force Instruction: Read→Execute
    if (!smmu.configureStream(sid, cfg).isOk()) return false;
    if (!smmu.enableStream(sid).isOk()) return false;
    if (!smmu.createStreamPASID(sid, 0u).isOk()) return false;
    return true;
}

// ── BUG-13.1.4-CPP-A Test 1 — ATOS Read must NOT be promoted to Execute ──────
// Normal translate(Read) with instCfg=3 on Execute-only page → success (INSTCFG fires)
// gatosTranslate(Read) with instCfg=3 on Execute-only page → fault (INSTCFG must NOT fire)
//
// Before fix: gatosTranslate incorrectly promotes Read→Execute, returns success (wrong)
// After  fix: gatosTranslate skips INSTCFG, Read stays Read, returns fault (correct)
TEST(BugAudit134CppAtosTest, GatosRead_NotPromotedToExecute_WithInstCfg3)
{
    SMMU smmu;
    ASSERT_TRUE(setupAtosStream(smmu, SID_ATOS));

    // Map page with Execute-only permission (no read, no write)
    PagePermissions execOnly;
    execOnly.read    = false;
    execOnly.write   = false;
    execOnly.execute = true;
    ASSERT_TRUE(smmu.mapPage(SID_ATOS, 0u, ATOS_IOVA, ATOS_PA, execOnly).isOk());

    // Sanity check: normal translate with Read must succeed (INSTCFG promotes Read→Execute)
    TranslationResult normalResult = smmu.translate(SID_ATOS, 0u, ATOS_IOVA,
                                                    AccessType::Read,
                                                    SecurityState::NonSecure);
    EXPECT_TRUE(normalResult.isOk())
        << "Sanity: normal translate with instCfg=3 should promote Read→Execute and succeed";

    // GATOS translate with Read must FAULT (INSTCFG must NOT apply to ATOS per §13.1.4)
    uint64_t gatosResult = smmu.gatosTranslate(SID_ATOS, 0u, ATOS_IOVA,
                                               AccessType::Read,
                                               SecurityState::NonSecure);
    // GATOS_PAR bit[0] = FAULT: 1 means fault, 0 means success
    bool gatosFaulted = (gatosResult & 0x1ULL) != 0u;

    EXPECT_TRUE(gatosFaulted)
        << "BUG-13.1.4-CPP-A: GATOS translate with Read on Execute-only page must FAULT. "
           "§13.1.4: ATOS must not apply INSTCFG overrides — Read must stay Read. "
           "Before fix: INSTCFG promotes Read→Execute, GATOS succeeds (gatosResult=0x"
        << std::hex << gatosResult << std::dec
        << "). After fix: Read stays Read, no execute permission → GATOS faults.";
}

// ── BUG-13.1.4-CPP-A Test 2 — ATOS Execute on Execute-only page → success ────
// GATOS with Execute access on Execute-only page must succeed (no overrides needed).
TEST(BugAudit134CppAtosTest, GatosExecute_ExecuteOnlyPage_Succeeds)
{
    SMMU smmu;
    ASSERT_TRUE(setupAtosStream(smmu, SID_ATOS + 1u));

    PagePermissions execOnly;
    execOnly.read    = false;
    execOnly.write   = false;
    execOnly.execute = true;
    ASSERT_TRUE(smmu.mapPage(SID_ATOS + 1u, 0u, ATOS_IOVA, ATOS_PA, execOnly).isOk());

    // GATOS with Execute: execute permission is set → should succeed
    uint64_t gatosResult = smmu.gatosTranslate(SID_ATOS + 1u, 0u, ATOS_IOVA,
                                               AccessType::Execute,
                                               SecurityState::NonSecure);
    bool gatosFaulted = (gatosResult & 0x1ULL) != 0u;

    EXPECT_FALSE(gatosFaulted)
        << "GATOS Execute on Execute-only page must succeed. gatosResult=0x"
        << std::hex << gatosResult;
}

}  // namespace test
}  // namespace smmu
