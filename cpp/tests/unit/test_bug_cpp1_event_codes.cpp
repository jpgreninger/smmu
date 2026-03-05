// BUG-CPP-1: Wrong event codes in handleTranslationFailure()
//
// ARM IHI0070G.b specification requires:
//   Level0-Level3TranslationFault  → EventType::F_TRANSLATION (0x10)  §7.3.13
//   TranslationTableFormatFault    → EventType::F_TRANSLATION (0x10)  §7.3.13
//   ContextDescriptorFormatFault   → EventType::C_BAD_CD      (0x0A)  §7.3.11
//   AccessFlagFault                → EventType::F_ACCESS       (0x12)  §7.3.15
//
// Bug: a fall-through in handleTranslationFailure() causes
// ContextDescriptorFormatFault, TranslationTableFormatFault, and
// Level0-Level3TranslationFault to all emit F_ACCESS (0x12) — wrong.
//
// TDD: these tests MUST FAIL before the fix because the switch fall-through
// causes wrong events, and MUST PASS after the switch cases are separated.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <algorithm>

namespace smmu {
namespace test {

// ── Test constants ────────────────────────────────────────────────────────

static constexpr StreamID CPP1_STREAM_BASE = 0xA0;
static constexpr PASID    CPP1_PASID_0     = 0;
static constexpr PASID    CPP1_PASID_1     = 1;
static constexpr IOVA     CPP1_IOVA        = 0x0010'0000ULL;
static constexpr PA       CPP1_PA_IPA      = 0x2000'0000ULL;  // IPA from stage-1
static constexpr PA       CPP1_PA_FINAL    = 0x4000'0000ULL;  // PA from stage-2

// ── Helper utilities ──────────────────────────────────────────────────────

static std::unique_ptr<SMMU> makeEnabled() {
    auto s = std::unique_ptr<SMMU>(new SMMU());
    s->enable();
    return s;
}

static int countEventType(const SMMU& smmu, EventType t) {
    int n = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == t) {
            ++n;
        }
    }
    return n;
}

// ── Fixture: stage-1-only terminate stream ────────────────────────────────

class EventCodeStage1Test : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = makeEnabled();

        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        streamID = CPP1_STREAM_BASE;
        ASSERT_TRUE(smmu->configureStream(streamID, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(streamID).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(streamID, CPP1_PASID_0).isOk());
    }

    void TearDown() override {
        smmu.reset();
    }

    std::unique_ptr<SMMU> smmu;
    StreamID streamID = 0;
};

// ── Fixture: two-stage terminate stream ───────────────────────────────────

class EventCodeTwoStageTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = makeEnabled();

        // Stream: two-stage translation
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        streamID = CPP1_STREAM_BASE + 1;
        ASSERT_TRUE(smmu->configureStream(streamID, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(streamID).isOk());

        // PASID=0 is the stage-2 address space (aliased by the SMMU model).
        ASSERT_TRUE(smmu->createStreamPASID(streamID, CPP1_PASID_0).isOk());

        // PASID=1 is used for stage-1 so its AS is separate from stage-2.
        ASSERT_TRUE(smmu->createStreamPASID(streamID, CPP1_PASID_1).isOk());
    }

    void TearDown() override {
        smmu.reset();
    }

    std::unique_ptr<SMMU> smmu;
    StreamID streamID = 0;
};

// ── BUG-CPP-1a: Unmapped page (stage-1 only) must produce F_TRANSLATION ──
//
// §7.3.13: Level-N translation fault (page not found at any walk level)
// must generate F_TRANSLATION (0x10), never F_ACCESS (0x12).
//
// FAILS before fix: the fall-through switch cases for Level0-Level3
// TranslationFault all emit F_ACCESS. After the fix they emit F_TRANSLATION.

TEST_F(EventCodeStage1Test, UnmappedPage_Stage1Only_EmitsF_TRANSLATION) {
    // Translate an unmapped address — this triggers a translation fault.
    smmu->translate(streamID, CPP1_PASID_0, CPP1_IOVA, AccessType::Read,
                    SecurityState::NonSecure);

    int ftrans = countEventType(*smmu, EventType::F_TRANSLATION);
    int faccess = countEventType(*smmu, EventType::F_ACCESS);

    EXPECT_GT(ftrans, 0)
        << "§7.3.13 / BUG-CPP-1: An unmapped page (translation fault) must "
           "generate F_TRANSLATION (0x10), not F_ACCESS";
    EXPECT_EQ(faccess, 0)
        << "§7.3.13 / BUG-CPP-1: Translation fault must NOT generate F_ACCESS "
           "(0x12); fall-through in handleTranslationFailure() emits wrong code";
}

// ── BUG-CPP-1b: Unmapped stage-1 page in two-stage stream ────────────────
//
// performBothStagesTranslation classifies the fault as Level1TranslationFault
// in the FaultRecord; the handleTranslationFailure event dispatch must map
// that fault to F_TRANSLATION, not F_ACCESS.

TEST_F(EventCodeTwoStageTest, UnmappedPage_TwoStage_EmitsF_TRANSLATION) {
    // PASID=1 stage-1 AS has no mapping for CPP1_IOVA → level-1 fault.
    smmu->translate(streamID, CPP1_PASID_1, CPP1_IOVA, AccessType::Read,
                    SecurityState::NonSecure);

    int ftrans = countEventType(*smmu, EventType::F_TRANSLATION);
    int faccess = countEventType(*smmu, EventType::F_ACCESS);

    EXPECT_GT(ftrans, 0)
        << "§7.3.13 / BUG-CPP-1: Two-stage unmapped page must generate "
           "F_TRANSLATION (0x10)";
    EXPECT_EQ(faccess, 0)
        << "§7.3.13 / BUG-CPP-1: Two-stage translation fault must NOT generate "
           "F_ACCESS; fix the Level0-Level3TranslationFault switch cases";
}

// ── BUG-CPP-1c: ContextDescriptorFormat (aa64=false) must produce C_BAD_CD ─
//
// §7.3.11: When the CD is invalid (AA64=0 unsupported mode), the SMMU must
// generate C_BAD_CD (0x0A). The handleTranslationFailure() switch case for
// ContextDescriptorFormatFault currently falls through to F_ACCESS.

TEST(EventCodeContextDescriptor, AA64False_EmitsC_BAD_CD) {
    auto smmu = makeEnabled();
    StreamID sid = CPP1_STREAM_BASE + 2;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = false;   // Triggers ContextDescriptorFormatFault path
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP1_PASID_0).isOk());

    smmu->translate(sid, CPP1_PASID_0, CPP1_IOVA, AccessType::Read,
                    SecurityState::NonSecure);

    int cbad = countEventType(*smmu, EventType::C_BAD_CD);
    int faccess = countEventType(*smmu, EventType::F_ACCESS);

    EXPECT_GT(cbad, 0)
        << "§7.3.11 / BUG-CPP-1: AA64=0 (unsupported LPAE) must generate "
           "C_BAD_CD (0x0A)";
    EXPECT_EQ(faccess, 0)
        << "§7.3.11 / BUG-CPP-1: ContextDescriptorFormatFault must NOT generate "
           "F_ACCESS (0x12); the switch fall-through is wrong";
}

// ── BUG-CPP-1d: T0SZ out-of-range must produce C_BAD_CD, not F_ACCESS ────
//
// §7.3.11: An out-of-range T0SZ is a CD format error → C_BAD_CD.
// The handleTranslationFailure() case for ContextDescriptorFormatFault must
// NOT fall through to AccessFlagFault's F_ACCESS handler.

TEST(EventCodeContextDescriptor, T0SZOutOfRange_EmitsC_BAD_CD) {
    auto smmu = makeEnabled();
    StreamID sid = CPP1_STREAM_BASE + 3;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;
    cfg.t0sz               = 40u;  // > 39 — invalid per §5.4
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP1_PASID_0).isOk());

    smmu->translate(sid, CPP1_PASID_0, CPP1_IOVA, AccessType::Read,
                    SecurityState::NonSecure);

    int cbad = countEventType(*smmu, EventType::C_BAD_CD);
    int faccess = countEventType(*smmu, EventType::F_ACCESS);

    EXPECT_GT(cbad, 0)
        << "§7.3.11 / BUG-CPP-1: T0SZ > 39 must generate C_BAD_CD (0x0A)";
    EXPECT_EQ(faccess, 0)
        << "§7.3.11 / BUG-CPP-1: ContextDescriptorFormatFault must NOT generate "
           "F_ACCESS; fix the switch fall-through";
}

// ── BUG-CPP-1e: TranslationFault must NOT produce F_ACCESS ───────────────
//
// §7.3.13: TranslationFault → F_TRANSLATION.  The existing case for
// FaultType::TranslationFault already emits F_TRANSLATION correctly.
// This test is a regression guard ensuring the fix does not break the
// existing correct case.

TEST_F(EventCodeStage1Test, TranslationFault_MustNotProduceF_ACCESS) {
    smmu->translate(streamID, CPP1_PASID_0, CPP1_IOVA + 0x1000,
                    AccessType::Write, SecurityState::NonSecure);

    int faccess = countEventType(*smmu, EventType::F_ACCESS);
    EXPECT_EQ(faccess, 0)
        << "§7.3.13 / BUG-CPP-1: TranslationFault must not produce F_ACCESS; "
           "regression guard for the switch fall-through fix";
}

// ── BUG-CPP-1f: PermissionFault must produce F_PERMISSION, not F_ACCESS ──
//
// §7.3.16: PermissionFault → F_PERMISSION. Regression guard that the
// PermissionFault case is not affected by the fall-through fix.

TEST_F(EventCodeStage1Test, PermissionFault_MustNotProduceF_ACCESS) {
    PagePermissions readOnly(true, false, false);
    ASSERT_TRUE(smmu->mapPage(streamID, CPP1_PASID_0, CPP1_IOVA, CPP1_PA_IPA,
                              readOnly).isOk());

    smmu->translate(streamID, CPP1_PASID_0, CPP1_IOVA, AccessType::Write,
                    SecurityState::NonSecure);

    int fperm = countEventType(*smmu, EventType::F_PERMISSION);
    int faccess = countEventType(*smmu, EventType::F_ACCESS);

    EXPECT_GT(fperm, 0)
        << "§7.3.16 / BUG-CPP-1: PermissionFault must produce F_PERMISSION";
    EXPECT_EQ(faccess, 0)
        << "§7.3.16 / BUG-CPP-1: PermissionFault must not produce F_ACCESS";
}

// ── BUG-CPP-1g: FaultRecord type for two-stage level-1 miss ──────────────
//
// In two-stage translation, when stage-1 has no mapping, the FaultRecord
// should classify the fault as Level1TranslationFault (via
// classifyDetailedTranslationFault). The EVENT emitted must be F_TRANSLATION.
// This test verifies both the FaultRecord type and the event code.

TEST_F(EventCodeTwoStageTest, TwoStageStage1Miss_FaultRecord_AndEvent) {
    // Translate unmapped IOVA — stage-1 fails at level 1.
    auto result = smmu->translate(streamID, CPP1_PASID_1, CPP1_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError());

    // Event queue must contain F_TRANSLATION (not F_ACCESS).
    int ftrans = countEventType(*smmu, EventType::F_TRANSLATION);
    int faccess = countEventType(*smmu, EventType::F_ACCESS);

    EXPECT_GT(ftrans, 0)
        << "§7.3.13 / BUG-CPP-1: Two-stage level-1 miss must emit F_TRANSLATION";
    EXPECT_EQ(faccess, 0)
        << "§7.3.13 / BUG-CPP-1: Two-stage level-1 miss must NOT emit F_ACCESS; "
           "Level1-Level3TranslationFault cases in switch must emit F_TRANSLATION";

    // FaultRecord must exist and have a valid level-based fault type.
    auto eventsResult = smmu->getEvents();
    ASSERT_TRUE(eventsResult.isOk());
    const auto& faults = eventsResult.getValue();
    ASSERT_FALSE(faults.empty())
        << "BUG-CPP-1: At least one FaultRecord must be recorded";

    // The FaultRecord's faultType should be Level1TranslationFault
    // (set by classifyDetailedTranslationFault in performBothStagesTranslation).
    bool foundLevelFault = false;
    for (const auto& fr : faults) {
        if (fr.faultType == FaultType::Level1TranslationFault ||
            fr.faultType == FaultType::Level0TranslationFault ||
            fr.faultType == FaultType::Level2TranslationFault ||
            fr.faultType == FaultType::Level3TranslationFault ||
            fr.faultType == FaultType::TranslationFault) {
            foundLevelFault = true;
            break;
        }
    }
    EXPECT_TRUE(foundLevelFault)
        << "BUG-CPP-1: FaultRecord must have a translation-class fault type";
}

}  // namespace test
}  // namespace smmu
