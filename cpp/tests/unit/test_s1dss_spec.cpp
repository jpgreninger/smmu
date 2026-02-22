// ARM SMMU v3 FINDING-NEW-17 and FINDING-NEW-18
// Copyright (c) 2024 John Greninger
//
// TDD spec tests for:
// - CommandEntry.leaf field (FINDING-NEW-17, ARM §4.3.1/§4.3.3)
// - StreamConfig.s1dss and s1cdMax fields (FINDING-NEW-18, ARM §3.9/§5.2)
// - S1DSS routing for non-substream transactions (§7.3.7)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// constants
static constexpr StreamID S1DSS_STREAM = 0x50;
static constexpr PASID    S1DSS_PASID0 = 0;
static constexpr IOVA     S1DSS_IOVA   = 0x2000ULL;
static constexpr PA       S1DSS_PA     = 0x8000'0000ULL;

// NEW-17: CommandEntry has a leaf field defaulting to false.
TEST(S1dssSpec, CommandEntry_HasLeafField_DefaultFalse) {
    CommandEntry cmd;
    EXPECT_FALSE(cmd.leaf)
        << "CommandEntry.leaf must default to false (Leaf=0 — full invalidation)";
}

// NEW-17: leaf field can be set to true.
TEST(S1dssSpec, CommandEntry_LeafFieldSettable) {
    CommandEntry cmd;
    cmd.leaf = true;
    EXPECT_TRUE(cmd.leaf);
}

// NEW-18: StreamConfig has s1dss defaulting to 0b10.
TEST(S1dssSpec, StreamConfig_HasS1dssField_Default0b10) {
    StreamConfig cfg;
    EXPECT_EQ(cfg.s1dss, 2u)
        << "StreamConfig.s1dss must default to 0b10 (use CD[0])";
}

// NEW-18: StreamConfig has s1cdMax defaulting to 0.
TEST(S1dssSpec, StreamConfig_HasS1cdMaxField_Default0) {
    StreamConfig cfg;
    EXPECT_EQ(cfg.s1cdMax, 0u)
        << "StreamConfig.s1cdMax must default to 0 (not substream-capable)";
}

// NEW-18: When s1cdMax == 0 (not substream-capable), s1dss is ignored and
// PASID=0 always uses CD[0] — existing behavior preserved.
TEST(S1dssSpec, S1cdMax0_PASID0_UsesCD0_ExistingBehavior) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1dss              = 0x00;  // would abort, but s1cdMax==0 so ignored
    cfg.s1cdMax            = 0;
    ASSERT_TRUE(smmu->configureStream(S1DSS_STREAM, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(S1DSS_STREAM).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(S1DSS_STREAM, S1DSS_PASID0).isOk());

    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA, S1DSS_PA, perms).isOk());

    auto r = smmu->translate(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA,
                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk())
        << "s1cdMax==0 must not apply S1DSS routing; PASID=0 uses CD[0] normally";
}

// NEW-18 S1DSS==0b00: non-substream (PASID=0) on substream-capable stream
// aborts with F_STREAM_DISABLED and generates the event (§7.3.7).
TEST(S1dssSpec, S1dss00_PASID0_AbortsWithFStreamDisabled) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1dss              = 0x00;  // non-substream transactions abort
    cfg.s1cdMax            = 4;     // stream supports up to 2^4 substreams
    ASSERT_TRUE(smmu->configureStream(S1DSS_STREAM, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(S1DSS_STREAM).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(S1DSS_STREAM, S1DSS_PASID0).isOk());
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA, S1DSS_PA, perms).isOk());

    auto r = smmu->translate(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA,
                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isError())
        << "S1DSS==0b00: non-substream PASID=0 must abort";

    // §7.3.7: F_STREAM_DISABLED event IS generated for S1DSS==0b00
    // (distinct from STE.Config==0b000 which is silent).
    auto events = smmu->getEventQueue();
    ASSERT_FALSE(events.empty())
        << "S1DSS==0b00 must generate F_STREAM_DISABLED event (§7.3.7)";
    EXPECT_EQ(events.front().type, EventType::F_STREAM_DISABLED)
        << "Event must be F_STREAM_DISABLED (0x06)";
}

// NEW-18 S1DSS==0b01: non-substream (PASID=0) bypasses stage-1 (identity PA).
TEST(S1dssSpec, S1dss01_PASID0_BypassesStage1) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1dss              = 0x01;  // non-substream transactions bypass stage-1
    cfg.s1cdMax            = 4;
    ASSERT_TRUE(smmu->configureStream(S1DSS_STREAM, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(S1DSS_STREAM).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(S1DSS_STREAM, S1DSS_PASID0).isOk());
    // No page mapped — stage-1 bypass means identity PA regardless.

    auto r = smmu->translate(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA,
                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk())
        << "S1DSS==0b01: non-substream PASID=0 must succeed with bypass";
    if (r.isOk()) {
        EXPECT_EQ(r.getValue().physicalAddress, S1DSS_IOVA)
            << "S1DSS==0b01 bypass: PA must equal IOVA (identity mapping)";
    }
}

// NEW-18 S1DSS==0b10: non-substream (PASID=0) uses CD[0] — normal translation.
TEST(S1dssSpec, S1dss10_PASID0_UsesCD0) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1dss              = 0x02;  // use CD[0] (default)
    cfg.s1cdMax            = 4;
    ASSERT_TRUE(smmu->configureStream(S1DSS_STREAM, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(S1DSS_STREAM).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(S1DSS_STREAM, S1DSS_PASID0).isOk());
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu->mapPage(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA, S1DSS_PA, perms).isOk());

    auto r = smmu->translate(S1DSS_STREAM, S1DSS_PASID0, S1DSS_IOVA,
                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(r.isOk())
        << "S1DSS==0b10: PASID=0 uses CD[0] and must succeed with mapped page";
    if (r.isOk()) {
        EXPECT_EQ(r.getValue().physicalAddress, S1DSS_PA)
            << "S1DSS==0b10 CD[0] lookup must return the mapped PA";
    }
}

} // namespace test
} // namespace smmu
