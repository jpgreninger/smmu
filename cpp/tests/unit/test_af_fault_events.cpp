// ARM SMMU v3 AF Fault Event Tests
//
// Verifies correct event generation for Access Flag (AF=0) faults across all
// three translation paths (stage-1-only, stage-2-only, two-stage).
//
// Bug NEW-A: handleTranslationFailure() had no case for AccessFlagFaultError
//            → fell to default → F_TRANSLATION instead of F_ACCESS.
// Bug NEW-B: performBothStagesTranslation() stage-2 error switch had no case
//            for AccessFlagFaultError → stage2FaultType = Stage2PermissionFault
//            → no F_ACCESS with s2=true emitted.
// Bug NEW-C: performStage2OnlyTranslation() emitted F_ACCESS inline AND returned
//            the error, causing handleTranslationFailure (once NEW-A fixed) to
//            emit a second F_ACCESS — violating ARM §7.2 / §3.12.2.
// Bug NEW-D: performStage1OnlyTranslation() relied on handleTranslationFailure()
//            to emit F_ACCESS, which (before NEW-A fix) emitted F_TRANSLATION.
//            Verify that after NEW-A is fixed exactly one F_ACCESS is emitted.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>

using namespace smmu;

namespace {

static const PagePermissions RW(true, true, false);

void enableSMMU(SMMU& smmu) {
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

StreamConfig stage1Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    cfg.t0sz = 0;
    cfg.ha = false;
    cfg.affd = false;
    return cfg;
}

StreamConfig stage2Config() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = false;
    cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate;
    cfg.s2t0sz = 0;
    cfg.s2ps = 5;
    cfg.s2ha = false;
    cfg.s2affd = false;
    return cfg;
}

StreamConfig twoStageConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate;
    cfg.t0sz = 0;
    cfg.s2t0sz = 0;
    cfg.s2ps = 5;
    cfg.ha = false;
    cfg.affd = false;
    cfg.s2ha = false;
    cfg.s2affd = false;
    return cfg;
}

// Count events of a given EventType in the event queue.
int countEvents(const SMMU& smmu, EventType et) {
    int count = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == et) {
            ++count;
        }
    }
    return count;
}

// Return the first EventEntry of the given type, or a default-constructed one.
EventEntry findEvent(const SMMU& smmu, EventType et) {
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == et) {
            return ev;
        }
    }
    return EventEntry();
}

} // namespace

// ============================================================================
// NEW-A / NEW-D: Stage-1-only stream with AF=0 page must emit exactly
//                one F_ACCESS event (not F_TRANSLATION).
// ============================================================================

/// Stage-1-only stream, AF=0 page, terminate mode → exactly 1 F_ACCESS event.
TEST(AfFaultEvents, Stage1OnlyAfFaultEmitsF_ACCESS) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF0u;
    ASSERT_TRUE(smmu.configureStream(sid, stage1Config()).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    // accessFlag=false creates an AF=0 entry (simulates first-access before OS sets AF).
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RW, SecurityState::NonSecure,
                             /*accessFlag=*/false).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "AF=0, HA=false, AFFD=false: translation must fault";

    // Must have exactly one F_ACCESS event.
    int accessCount = countEvents(smmu, EventType::F_ACCESS);
    EXPECT_EQ(accessCount, 1)
        << "Stage-1-only AF=0 fault must emit exactly 1 F_ACCESS event (§7.3.15)";

    // Must NOT have any F_TRANSLATION event.
    int translCount = countEvents(smmu, EventType::F_TRANSLATION);
    EXPECT_EQ(translCount, 0)
        << "Stage-1-only AF=0 fault must NOT emit F_TRANSLATION (§7.3.15)";
}

// ============================================================================
// NEW-C: Stage-2-only stream with AF=0 page must emit exactly ONE F_ACCESS
//        event — not two (double-emit bug before fix).
// ============================================================================

/// Stage-2-only stream, AF=0 stage-2 page, terminate mode → exactly 1 F_ACCESS event.
TEST(AfFaultEvents, Stage2OnlyAfFaultEmitsExactlyOneEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF1u;
    ASSERT_TRUE(smmu.configureStream(sid, stage2Config()).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // For stage-2-only streams the IOVA is treated as the IPA directly.
    IOVA ipa = 0x3000u;
    PA pa = 0x4000u;

    // Use a dedicated stage-2 AS so we can map an AF=0 entry.
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    // Map with accessFlag=false to create AF=0 entry in stage-2 AS.
    ASSERT_TRUE(s2as->mapPage(ipa, pa, RW, SecurityState::NonSecure,
                              /*accessFlag=*/false).isOk());

    auto result = smmu.translate(sid, 0, ipa, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "AF=0 stage-2 page: translation must fault";

    // Must have exactly one F_ACCESS event (not two — the double-emit bug).
    int accessCount = countEvents(smmu, EventType::F_ACCESS);
    EXPECT_EQ(accessCount, 1)
        << "Stage-2-only AF=0 fault must emit exactly 1 F_ACCESS event "
           "(§7.2 / §3.12.2: one fault per transaction)";

    // Must NOT have any F_TRANSLATION event.
    int translCount = countEvents(smmu, EventType::F_TRANSLATION);
    EXPECT_EQ(translCount, 0)
        << "Stage-2-only AF=0 fault must NOT emit F_TRANSLATION";
}

// ============================================================================
// NEW-B: Two-stage stream where stage-2 descriptor has AF=0 must emit
//        exactly one F_ACCESS event with s2=true.
// ============================================================================

/// Two-stage stream, stage-2 page has AF=0, terminate mode → 1 F_ACCESS with s2=true.
TEST(AfFaultEvents, BothStagesAfFaultStage2EmitsF_ACCESS_WithS2True) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF2u;
    ASSERT_TRUE(smmu.configureStream(sid, twoStageConfig()).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x5000u;
    IOVA ipa  = 0x6000u;
    PA pa     = 0x7000u;

    // Stage-1: IOVA → IPA (AF=1, normal mapping).
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, static_cast<PA>(ipa), RW,
                             SecurityState::NonSecure).isOk());

    // Stage-2: IPA → PA with AF=0.
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    ASSERT_TRUE(s2as->mapPage(ipa, pa, RW, SecurityState::NonSecure,
                              /*accessFlag=*/false).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "Stage-2 AF=0 page in two-stage stream must fault";

    // Must have exactly one F_ACCESS event.
    int accessCount = countEvents(smmu, EventType::F_ACCESS);
    EXPECT_EQ(accessCount, 1)
        << "Two-stage stream stage-2 AF=0 fault must emit exactly 1 F_ACCESS (§7.3.15)";

    // The F_ACCESS event must have s2=true (fault occurred at stage-2).
    EventEntry ev = findEvent(smmu, EventType::F_ACCESS);
    EXPECT_TRUE(ev.s2)
        << "F_ACCESS event from stage-2 AF fault must have s2=true (§7.3.13)";

    // Must NOT have any F_TRANSLATION event.
    int translCount = countEvents(smmu, EventType::F_TRANSLATION);
    EXPECT_EQ(translCount, 0)
        << "Two-stage stage-2 AF=0 fault must NOT emit F_TRANSLATION";
}

// ============================================================================
// Regression: AF faults must never produce F_TRANSLATION for any stream type.
// ============================================================================

/// AF=0 page in a stage-1-only stream must not produce F_TRANSLATION.
TEST(AfFaultEvents, AfFaultDoesNotEmitF_TRANSLATION) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF3u;
    ASSERT_TRUE(smmu.configureStream(sid, stage1Config()).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x8000u;
    PA pa = 0x9000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RW, SecurityState::NonSecure,
                             /*accessFlag=*/false).isOk());

    smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);

    // F_TRANSLATION must NOT be present.
    EXPECT_EQ(countEvents(smmu, EventType::F_TRANSLATION), 0)
        << "AF=0 fault must NOT generate F_TRANSLATION — only F_ACCESS (§7.3.15 vs §7.3.13)";

    // F_ACCESS must be present.
    EXPECT_EQ(countEvents(smmu, EventType::F_ACCESS), 1)
        << "AF=0 fault must generate F_ACCESS (§7.3.15)";
}
