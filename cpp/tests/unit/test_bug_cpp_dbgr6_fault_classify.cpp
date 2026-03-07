// BUG-CPP-DBGR-6: fault misclassification in performBothStagesTranslation (§7.3.14, §7.3.16)
// TDD: failing test written BEFORE the fix.
// Stage-2 permission violation must produce FaultType::PermissionFault (F_PERMISSION event),
// not Stage2PermissionFault.
// Stage-1 permission violation must produce FaultType::PermissionFault, not AccessFault.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID D6_STREAM = 0x30;
static constexpr PASID    D6_PASID  = 0;
static constexpr IOVA     D6_IOVA   = 0x6000ULL;
static constexpr PA       D6_IPA    = 0xB000ULL;
static constexpr PA       D6_PA     = 0xC000ULL;

// Helper: set up two-stage stream
static void setupTwoStageBase(SMMU& smmu, PagePermissions s1perms, PagePermissions s2perms) {
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.strw               = StreamWorld::EL1_EL0; // no STRW suppression
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D6_STREAM, cfg);
    smmu.enableStream(D6_STREAM);
    smmu.createStreamPASID(D6_STREAM, D6_PASID);

    // Stage-1: IOVA -> IPA
    smmu.mapPage(D6_STREAM, D6_PASID, D6_IOVA, D6_IPA, s1perms, SecurityState::NonSecure);

    // Stage-2: IPA -> PA
    std::shared_ptr<AddressSpace> s2as = std::make_shared<AddressSpace>();
    s2as->mapPage(D6_IPA, D6_PA, s2perms, SecurityState::NonSecure);
    smmu.setStreamStage2AddressSpace(D6_STREAM, s2as);
}

// -----------------------------------------------------------------------
// Stage-2 permission violation: write to a read-only stage-2 page
// Must produce F_PERMISSION event (not Stage2PermissionFault)
// -----------------------------------------------------------------------
TEST(FaultClassifySpec, Stage2PermissionViolation_ProducesPermissionFaultEvent) {
    SMMU smmu;
    // Stage-1: read+write allowed
    PagePermissions s1perms(true, true, false);
    // Stage-2: read-only (write not allowed)
    PagePermissions s2perms(true, false, false);
    setupTwoStageBase(smmu, s1perms, s2perms);

    TranslationResult result = smmu.translate(D6_STREAM, D6_PASID, D6_IOVA,
                                             AccessType::Write, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "Write to read-only stage-2 page must fail";
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
        << "Error must be PagePermissionViolation (§7.3.16 F_PERMISSION)";

    // Verify F_PERMISSION event was generated (not some wrong event code)
    auto events = smmu.getEventQueue();
    bool foundFPermission = false;
    for (const auto& evt : events) {
        if (evt.type == EventType::F_PERMISSION && evt.streamID == D6_STREAM) {
            foundFPermission = true;
            break;
        }
    }
    EXPECT_TRUE(foundFPermission)
        << "F_PERMISSION event must be generated for stage-2 permission violation (§7.3.16)";
}

// -----------------------------------------------------------------------
// Stage-1 permission violation: write to a read-only stage-1 page
// Must produce F_PERMISSION event (not AccessFault or F_TRANSLATION)
// -----------------------------------------------------------------------
TEST(FaultClassifySpec, Stage1PermissionViolation_ProducesPermissionFaultEvent) {
    SMMU smmu;
    // Stage-1: read-only
    PagePermissions s1perms(true, false, false);
    // Stage-2: read+write allowed
    PagePermissions s2perms(true, true, false);
    setupTwoStageBase(smmu, s1perms, s2perms);

    TranslationResult result = smmu.translate(D6_STREAM, D6_PASID, D6_IOVA,
                                             AccessType::Write, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "Write to read-only stage-1 page must fail";
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
        << "Error must be PagePermissionViolation";

    auto events = smmu.getEventQueue();
    bool foundFPermission = false;
    for (const auto& evt : events) {
        if (evt.type == EventType::F_PERMISSION && evt.streamID == D6_STREAM) {
            foundFPermission = true;
            break;
        }
    }
    EXPECT_TRUE(foundFPermission)
        << "F_PERMISSION event must be generated for stage-1 permission violation (§7.3.16)";
}

// -----------------------------------------------------------------------
// Stage-2 translation fault (page not mapped in stage-2)
// Must produce F_TRANSLATION event (not AccessFault)
// -----------------------------------------------------------------------
TEST(FaultClassifySpec, Stage2PageNotMapped_ProducesTranslationFaultEvent) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D6_STREAM, cfg);
    smmu.enableStream(D6_STREAM);
    smmu.createStreamPASID(D6_STREAM, D6_PASID);

    // Stage-1 maps IOVA -> IPA
    PagePermissions s1perms(true, true, false);
    smmu.mapPage(D6_STREAM, D6_PASID, D6_IOVA, D6_IPA, s1perms, SecurityState::NonSecure);

    // Stage-2 AS exists but does NOT map D6_IPA
    std::shared_ptr<AddressSpace> s2as = std::make_shared<AddressSpace>();
    // Map a DIFFERENT address so D6_IPA is unmapped
    s2as->mapPage(0x99000ULL, D6_PA, PagePermissions(true, true, false), SecurityState::NonSecure);
    smmu.setStreamStage2AddressSpace(D6_STREAM, s2as);

    TranslationResult result = smmu.translate(D6_STREAM, D6_PASID, D6_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "Stage-2 page not mapped must fail";

    auto events = smmu.getEventQueue();
    bool foundFTranslation = false;
    for (const auto& evt : events) {
        if ((evt.type == EventType::F_TRANSLATION) && evt.streamID == D6_STREAM) {
            foundFTranslation = true;
            break;
        }
    }
    EXPECT_TRUE(foundFTranslation)
        << "F_TRANSLATION event must be generated for stage-2 page not mapped (§7.3.13)";
}

} // namespace test
} // namespace smmu
