// TDD: Tests for GAP-1 (STE output-attribute override fields)
// These tests MUST FAIL before implementation.
#include <gtest/gtest.h>
#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"

using namespace smmu;

// ===== GAP-1: TranslationData output-attribute fields =====

TEST(OutputAttributeFieldsTest, TranslationDataHasMemTypeField) {
    TranslationData data;
    // Default memType should be 0
    EXPECT_EQ(data.memType, 0u);
}

TEST(OutputAttributeFieldsTest, TranslationDataHasShareabilityField) {
    TranslationData data;
    EXPECT_EQ(data.shareability, 0u);
}

TEST(OutputAttributeFieldsTest, TranslationDataHasAllocHintField) {
    TranslationData data;
    EXPECT_EQ(data.allocHint, 0u);
}

TEST(OutputAttributeFieldsTest, TranslationDataHasInstCfgField) {
    TranslationData data;
    EXPECT_EQ(data.instCfg, 0u);
}

TEST(OutputAttributeFieldsTest, TranslationDataHasPrivCfgField) {
    TranslationData data;
    EXPECT_EQ(data.privCfg, 0u);
}

TEST(OutputAttributeFieldsTest, TranslationDataHasNsCfgOutField) {
    TranslationData data;
    EXPECT_EQ(data.nsCfgOut, 0u);
}

TEST(OutputAttributeOverrideTest, MtCfgTrueAppliesMemAttr) {
    // When MTCFG=true, memAttr from StreamConfig should appear in TranslationData.memType
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.mtCfg = true;
    cfg.memAttr = 0x07; // memory type 7
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    as->mapPage(0x1000, 0x2000, perms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().memType, 0x07u);
}

TEST(OutputAttributeOverrideTest, MtCfgFalseUsesDefault) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.mtCfg = false;
    cfg.memAttr = 0x07;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    as->mapPage(0x1000, 0x2000, perms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    ASSERT_TRUE(result.isOk());
    // MTCFG=false: memType should be 0 (from-translation default)
    EXPECT_EQ(result.getValue().memType, 0u);
}

TEST(OutputAttributeOverrideTest, ShCfgApplied) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.shCfg = 3; // ISH
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    as->mapPage(0x1000, 0x2000, perms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 3u);
}

TEST(OutputAttributeOverrideTest, InstCfgApplied) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.instCfg = 2; // Instruction
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    as->mapPage(0x1000, 0x2000, perms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().instCfg, 2u);
}

TEST(OutputAttributeOverrideTest, PrivCfgApplied) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.privCfg = 2; // Privileged
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    as->mapPage(0x1000, 0x2000, perms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().privCfg, 2u);
}

TEST(OutputAttributeOverrideTest, NsCfgApplied) {
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.nsCfg = 2; // NonSecure
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions perms(true, false, false);
    as->mapPage(0x1000, 0x2000, perms);
    ctx.addPASID(0, as);
    ctx.setStage1Enabled(true);

    TranslationResult result = ctx.translate(0, 0x1000, AccessType::Read);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().nsCfgOut, 2u);
}
