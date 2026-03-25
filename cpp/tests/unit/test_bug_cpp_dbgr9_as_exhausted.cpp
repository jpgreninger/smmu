// BUG-CPP-DBGR-9: performBothStagesTranslation returns AddressSpaceExhausted instead of
// F_TRANSLATION when stage-2 AS is not configured (§7.3.13)
// TDD: failing test written BEFORE the fix.
// A two-stage stream with no Stage-2 AS configured must produce an F_TRANSLATION event,
// not a configuration fault (C_BAD_STE via AddressSpaceExhausted).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

static constexpr StreamID D9_STREAM = 0x40;
static constexpr PASID    D9_PASID  = 0;
static constexpr IOVA     D9_IOVA   = 0x7000ULL;
static constexpr PA       D9_IPA    = 0xD000ULL;

// -----------------------------------------------------------------------
// Two-stage stream with no Stage-2 AS configured:
// Must produce F_TRANSLATION event (§7.3.13), not a configuration fault.
// -----------------------------------------------------------------------
TEST(AsExhaustedSpec, TwoStage_NoStage2AS_ProducesTranslationFaultNotConfigFault) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true; // stage-2 requested
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(D9_STREAM, cfg);
    smmu.enableStream(D9_STREAM);

    // Use PASID=1 (not 0) so that createStreamPASID does not auto-link the
    // PASID-0 address space as the stage-2 AS (the auto-link fires only for pasid==0).
    static constexpr PASID D9_PASID_NONZERO = 1;
    smmu.createStreamPASID(D9_STREAM, D9_PASID_NONZERO);

    // Stage-1 maps IOVA -> IPA using PASID=1
    PagePermissions s1perms(true, true, false);
    smmu.mapPage(D9_STREAM, D9_PASID_NONZERO, D9_IOVA, D9_IPA, s1perms, SecurityState::NonSecure);

    // Do NOT set a Stage-2 AddressSpace via setStreamStage2AddressSpace() —
    // this is the bug trigger. Without PASID-0, no auto-link fires.

    TranslationResult result = smmu.translate(D9_STREAM, D9_PASID_NONZERO, D9_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError()) << "Translation must fail with no stage-2 AS configured";

    // Must NOT return AddressSpaceExhausted (which causes C_BAD_STE / config fault)
    EXPECT_NE(result.getError(), SMMUError::AddressSpaceExhausted)
        << "AddressSpaceExhausted must NOT be returned (causes wrong C_BAD_STE config fault)";

    // Must return PageNotMapped (which maps to F_TRANSLATION)
    EXPECT_EQ(result.getError(), SMMUError::PageNotMapped)
        << "Must return PageNotMapped so translate() generates F_TRANSLATION (§7.3.13)";

    // Verify F_TRANSLATION event was generated
    auto events = smmu.getEventQueue();
    bool foundFTranslation = false;
    for (const auto& evt : events) {
        if (evt.type == EventType::F_TRANSLATION && evt.streamID == D9_STREAM) {
            foundFTranslation = true;
            break;
        }
    }
    EXPECT_TRUE(foundFTranslation)
        << "F_TRANSLATION event must be generated when stage-2 AS is not configured (§7.3.13)";
}

// -----------------------------------------------------------------------
// Verify the stall guard in translate() does NOT treat this as a config fault
// (AddressSpaceExhausted is in isConfigFault guard — PageNotMapped is not)
// QA-AUDIT-FIX-1: §5.5 — stage-2 faults in two-stage streams use S2S for the
// stall decision.  Set s2s=true so the stage-2 F_TRANSLATION stalls.
// -----------------------------------------------------------------------
TEST(AsExhaustedSpec, TwoStage_NoStage2AS_StallMode_Stalls) {
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Stall; // stall mode (for stage-1 faults)
    cfg.s2s                = true;             // §5.5: stage-2 faults use S2S, not FaultMode
    cfg.aa64               = true;
    smmu.configureStream(D9_STREAM, cfg);
    smmu.enableStream(D9_STREAM);

    // Use PASID=1 to avoid the auto-link that fires for PASID=0 on a stage-2 stream
    static constexpr PASID D9_PASID_STALL = 1;
    smmu.createStreamPASID(D9_STREAM, D9_PASID_STALL);

    PagePermissions s1perms(true, true, false);
    smmu.mapPage(D9_STREAM, D9_PASID_STALL, D9_IOVA, D9_IPA, s1perms, SecurityState::NonSecure);

    // No stage-2 AS
    TranslationResult result = smmu.translate(D9_STREAM, D9_PASID_STALL, D9_IOVA,
                                             AccessType::Read, SecurityState::NonSecure);

    // With PageNotMapped (not a config fault), stall mode should stall the transaction
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "Stall-mode stream with missing stage-2 AS must stall (not abort as config fault)";
}

} // namespace test
} // namespace smmu
