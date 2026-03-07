// BUG-CPP-DBGR-5: STRW privilege must be IGNORED when stage-2 is enabled (§5.2 / §3.3.4)
// TDD: failing test written BEFORE the fix.
// §5.2 says STRW is ignored for Non-secure streams when stage-2 is enabled.
// The TLB fast path currently applies STRW promotion even for two-stage streams;
// the fix adds a `!stage2Enabled` guard.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID D5_STREAM = 0x20;
static constexpr PASID    D5_PASID  = 0;
static constexpr IOVA     D5_IOVA   = 0x5000ULL;
// Stage-1 IOVA -> IPA: D5_IOVA -> D5_IPA
static constexpr PA       D5_IPA    = 0xA000ULL;
// Stage-2 IPA -> PA: D5_IPA -> D5_PA
static constexpr PA       D5_PA     = 0xF000ULL;

// Helper: set up a two-stage stream with STRW=EL2 and a privilegedOnly page.
// The page at stage-1 is mapped as privilegedOnly so that a non-privileged
// access (without STRW suppression) would fail.
// When STRW is IGNORED (because stage-2 is enabled), the privilegedOnly flag
// must still take effect — i.e., a plain Read access should FAIL even with STRW=EL2.
static void setupTwoStageWithStrwEl2(SMMU& smmu) {
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.strw               = StreamWorld::EL2; // STRW=EL2
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D5_STREAM, cfg);
    smmu.enableStream(D5_STREAM);
    smmu.createStreamPASID(D5_STREAM, D5_PASID);

    // Stage-1: map D5_IOVA -> D5_IPA with privilegedOnly=true
    // (so a non-privileged access must be denied)
    PagePermissions s1perms(true, false, false, /*privilegedOnly=*/true);
    smmu.mapPage(D5_STREAM, D5_PASID, D5_IOVA, D5_IPA, s1perms, SecurityState::NonSecure);

    // Stage-2 address space: map D5_IPA -> D5_PA (not privilegedOnly)
    std::shared_ptr<AddressSpace> s2as = std::make_shared<AddressSpace>();
    PagePermissions s2perms(true, false, false);
    s2as->mapPage(D5_IPA, D5_PA, s2perms, SecurityState::NonSecure);
    smmu.setStreamStage2AddressSpace(D5_STREAM, s2as);
}

// -----------------------------------------------------------------------
// Two-stage stream with STRW=EL2 and privilegedOnly stage-1 page:
// A plain Read access MUST still fail (STRW is ignored when stage-2 enabled).
// -----------------------------------------------------------------------
TEST(StrwTwoStageSpec, TwoStage_StrwEl2_PrivilegedOnlyPage_PlainReadFails) {
    SMMU smmu;
    setupTwoStageWithStrwEl2(smmu);

    // Plain (non-privileged) Read — should FAIL because privilegedOnly is set
    // and STRW is ignored in two-stage mode (§5.2)
    TranslationResult result = smmu.translate(D5_STREAM, D5_PASID, D5_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "Plain Read on privilegedOnly page must FAIL even with STRW=EL2 when stage-2 is enabled"
           " (STRW ignored per §5.2)";
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
        << "Must fail with PagePermissionViolation, not a spurious bypass";
}

// -----------------------------------------------------------------------
// Stage-1-only stream with STRW=EL2 and privilegedOnly page:
// A plain Read MUST succeed (STRW suppresses privilegedOnly check).
// This verifies the fix didn't accidentally break single-stage behavior.
// -----------------------------------------------------------------------
TEST(StrwTwoStageSpec, Stage1Only_StrwEl2_PrivilegedOnlyPage_PlainReadSucceeds) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false; // single-stage only
    cfg.strw               = StreamWorld::EL2;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D5_STREAM, cfg);
    smmu.enableStream(D5_STREAM);
    smmu.createStreamPASID(D5_STREAM, D5_PASID);

    // Map with privilegedOnly=true
    PagePermissions perms(true, false, false, /*privilegedOnly=*/true);
    smmu.mapPage(D5_STREAM, D5_PASID, D5_IOVA, D5_PA, perms, SecurityState::NonSecure);

    // Plain Read should succeed because STRW=EL2 suppresses privilegedOnly for stage-1-only
    TranslationResult result = smmu.translate(D5_STREAM, D5_PASID, D5_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "Plain Read on privilegedOnly page must SUCCEED with STRW=EL2 in stage-1-only mode";
}

// -----------------------------------------------------------------------
// Stage-1-only stream with STRW=EL3 and privilegedOnly page: plain Read succeeds
// -----------------------------------------------------------------------
TEST(StrwTwoStageSpec, Stage1Only_StrwEl3_PrivilegedOnlyPage_PlainReadSucceeds) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.strw               = StreamWorld::EL3;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D5_STREAM, cfg);
    smmu.enableStream(D5_STREAM);
    smmu.createStreamPASID(D5_STREAM, D5_PASID);

    PagePermissions perms(true, false, false, /*privilegedOnly=*/true);
    smmu.mapPage(D5_STREAM, D5_PASID, D5_IOVA, D5_PA, perms, SecurityState::NonSecure);

    TranslationResult result = smmu.translate(D5_STREAM, D5_PASID, D5_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "Plain Read on privilegedOnly page must SUCCEED with STRW=EL3 in stage-1-only mode";
}

} // namespace test
} // namespace smmu
