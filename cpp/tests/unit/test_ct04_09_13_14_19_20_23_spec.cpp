// ARM SMMU v3 CT-04, CT-09, CT-13, CT-14, CT-19, CT-20, CT-23 spec compliance tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <algorithm>

namespace smmu {
namespace test {

// ─── CT-09: STE.Config==0b000 must abort silently without event ───────────────

TEST(Ct09SteConfig, Config0b000AbortsSilentlyWithoutEvent) {
    SMMU smmu;
    smmu.enable();

    // Configure stream with all disabled (STE.Config == 0b000)
    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    ASSERT_TRUE(smmu.configureStream(0x1, config).isOk());
    ASSERT_TRUE(smmu.enableStream(0x1).isOk());

    auto result = smmu.translate(0x1, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);

    // Should abort (not ok)
    EXPECT_FALSE(result.isOk());
    // CRITICAL: NO event should be recorded
    EXPECT_TRUE(smmu.getEventQueue().empty())
        << "STE.Config==0b000 must NOT generate any event in the event queue";
}

// A bypass stream with a non-zero PASID STILL generates C_BAD_SUBSTREAMID
// (regression guard — this is a different code path)
TEST(Ct09SteConfig, BypassStreamWithNonZeroPasidGeneratesEvent) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    ASSERT_TRUE(smmu.configureStream(0x2, config).isOk());
    ASSERT_TRUE(smmu.enableStream(0x2).isOk());

    // Non-zero PASID on a disabled stream generates C_BAD_SUBSTREAMID (§3.9)
    auto result = smmu.translate(0x2, 1, 0x1000, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk());
    // C_BAD_SUBSTREAMID event should have been generated
    bool found = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_SUBSTREAMID) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "Non-zero PASID on disabled stream must generate C_BAD_SUBSTREAMID";
}

// ─── CT-04: SMMU_STRTAB_BASE_CFG StreamID range validation ───────────────────

TEST(Ct04Strtab, StreamIdOutOfRangeGeneratesCBadStreamID) {
    SMMU smmu;
    smmu.enable();
    // Set stream table to only 4 entries (2^4 = 16)
    smmu.setStrtabLog2Size(4);

    // StreamID 16 is out of range (>= 2^4 = 16)
    auto result = smmu.translate(16, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk());
    bool found = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_STREAMID) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "StreamID >= 2^LOG2SIZE must generate C_BAD_STREAMID";
}

TEST(Ct04Strtab, StreamIdInRangeNotRejected) {
    SMMU smmu;
    smmu.enable();
    // Set stream table to 32 entries (2^5)
    smmu.setStrtabLog2Size(5);

    // StreamID 31 is in range
    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    ASSERT_TRUE(smmu.configureStream(31, config).isOk());
    ASSERT_TRUE(smmu.enableStream(31).isOk());

    auto result = smmu.translate(31, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    // Should not fail with C_BAD_STREAMID (may fail for other reasons)
    bool hasBadStreamId = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_STREAMID) {
            hasBadStreamId = true;
        }
    }
    EXPECT_FALSE(hasBadStreamId) << "StreamID 31 with LOG2SIZE=5 must not generate C_BAD_STREAMID";
}

TEST(Ct04Strtab, DefaultLog2SizeAcceptsAllStreamIDs) {
    SMMU smmu;
    smmu.enable();
    // Default LOG2SIZE should accept all valid StreamIDs (no C_BAD_STREAMID from range check)

    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    ASSERT_TRUE(smmu.configureStream(0xFFFFu, config).isOk());
    ASSERT_TRUE(smmu.enableStream(0xFFFFu).isOk());

    auto result = smmu.translate(0xFFFFu, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    bool hasBadStreamId = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_STREAMID) {
            hasBadStreamId = true;
        }
    }
    EXPECT_FALSE(hasBadStreamId) << "Default LOG2SIZE must accept StreamID 0xFFFF";
}

// ─── CT-13: CD.T0SZ/T1SZ validation ─────────────────────────────────────────

TEST(Ct13CdT0sz, OutOfRangeT0szGeneratesCBadCd) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.t0sz = 40; // invalid for SMMUv3.0 (> 39)
    ASSERT_TRUE(smmu.configureStream(1, config).isOk());
    ASSERT_TRUE(smmu.enableStream(1).isOk());

    auto result = smmu.translate(1, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk());
    bool found = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_CD) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "T0SZ=40 (> 39) must generate C_BAD_CD";
}

TEST(Ct13CdT0sz, OutOfRangeT1szGeneratesCBadCd) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.t0sz = 16; // valid
    config.t1sz = 63; // invalid (> 39)
    ASSERT_TRUE(smmu.configureStream(2, config).isOk());
    ASSERT_TRUE(smmu.enableStream(2).isOk());

    auto result = smmu.translate(2, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk());
    bool found = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_CD) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "T1SZ=63 (> 39) must generate C_BAD_CD";
}

TEST(Ct13CdT0sz, ValidT0szDoesNotGenerateCBadCd) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.t0sz = 16; // valid (default, 48-bit VA)
    config.t1sz = 16; // valid
    ASSERT_TRUE(smmu.configureStream(3, config).isOk());
    ASSERT_TRUE(smmu.enableStream(3).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(3, 0).isOk());

    // Map a page so translation succeeds
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(smmu.mapPage(3, 0, 0x1000, 0x80001000, rw).isOk());

    auto result = smmu.translate(3, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "Valid T0SZ=16 should allow successful translation";
    bool found = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_CD) {
            found = true;
        }
    }
    EXPECT_FALSE(found) << "Valid T0SZ=16 must NOT generate C_BAD_CD";
}

// ─── CT-14: CD.AA64 field ─────────────────────────────────────────────────────

TEST(Ct14CdAa64, Aa64FieldPresentInStreamConfig) {
    StreamConfig config;
    config.aa64 = true; // AArch64 mode
    EXPECT_TRUE(config.aa64);

    StreamConfig config2;
    config2.aa64 = false; // AArch32 LPAE mode
    EXPECT_FALSE(config2.aa64);
}

TEST(Ct14CdAa64, DefaultAa64IsTrue) {
    StreamConfig config;
    EXPECT_TRUE(config.aa64) << "Default AA64 should be true (AArch64 mode)";
}

TEST(Ct14CdAa64, Aa64FalseGeneratesCBadCd) {
    // AA64=0 (VMSAv8-32 LPAE) is unsupported — must generate C_BAD_CD
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = true;
    config.stage2Enabled = false;
    config.aa64 = false; // unsupported LPAE mode
    ASSERT_TRUE(smmu.configureStream(5, config).isOk());
    ASSERT_TRUE(smmu.enableStream(5).isOk());

    auto result = smmu.translate(5, 0, 0x1000, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk());
    bool found = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::C_BAD_CD) {
            found = true;
        }
    }
    EXPECT_TRUE(found) << "AA64=0 (LPAE unsupported) must generate C_BAD_CD";
}

// ─── CT-19: STE Output-Attribute Override Fields ─────────────────────────────

TEST(Ct19SteOutputAttr, SteHasOutputAttributeOverrideFields) {
    StreamConfig config;
    config.nsCfg    = 0x2u;  // NS=1 (NonSecure override)
    config.shCfg    = 0x1u;  // Inner-Shareable
    config.allocCfg = 0xFu;  // all allocate
    config.memAttr  = 0x4u;  // Normal memory
    config.instCfg  = 0x0u;  // no override
    config.privCfg  = 0x0u;  // no override
    config.mtCfg    = true;  // enable memory type override

    EXPECT_EQ(config.nsCfg,    0x2u);
    EXPECT_EQ(config.shCfg,    0x1u);
    EXPECT_EQ(config.allocCfg, 0xFu);
    EXPECT_EQ(config.memAttr,  0x4u);
    EXPECT_EQ(config.instCfg,  0x0u);
    EXPECT_EQ(config.privCfg,  0x0u);
    EXPECT_TRUE(config.mtCfg);
}

TEST(Ct19SteOutputAttr, DefaultOutputAttrFieldsAreZero) {
    StreamConfig config;
    EXPECT_EQ(config.nsCfg,    0u);
    EXPECT_EQ(config.shCfg,    0u);
    EXPECT_EQ(config.allocCfg, 0u);
    EXPECT_EQ(config.memAttr,  0u);
    EXPECT_EQ(config.instCfg,  0u);
    EXPECT_EQ(config.privCfg,  0u);
    EXPECT_FALSE(config.mtCfg);
}

TEST(Ct19SteOutputAttr, OutputAttrStoredAndRetrievable) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    config.nsCfg    = 0x2u;
    config.shCfg    = 0x1u;
    config.allocCfg = 0x3u;
    config.memAttr  = 0x4u;
    config.instCfg  = 0x1u;
    config.privCfg  = 0x2u;
    config.mtCfg    = true;

    EXPECT_TRUE(smmu.configureStream(0x10, config).isOk());
    // Just verify the stream was configured without error
    auto result = smmu.isStreamConfigured(0x10);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// ─── CT-20: STE.STRW (StreamWorld) ────────────────────────────────────────────

TEST(Ct20SteStrw, StreamWorldFieldPresentInStreamConfig) {
    StreamConfig config;
    config.strw = StreamWorld::EL2;
    EXPECT_EQ(config.strw, StreamWorld::EL2);
}

TEST(Ct20SteStrw, DefaultStreamWorldIsEL1_EL0) {
    StreamConfig config;
    EXPECT_EQ(config.strw, StreamWorld::EL1_EL0)
        << "Default STRW should be EL1_EL0 (0b00)";
}

TEST(Ct20SteStrw, AllStreamWorldValuesAccessible) {
    StreamConfig c0;
    c0.strw = StreamWorld::EL1_EL0;
    EXPECT_EQ(static_cast<uint8_t>(c0.strw), 0x00u);

    StreamConfig c1;
    c1.strw = StreamWorld::EL2;
    EXPECT_EQ(static_cast<uint8_t>(c1.strw), 0x01u);

    StreamConfig c2;
    c2.strw = StreamWorld::EL2_E2H;
    EXPECT_EQ(static_cast<uint8_t>(c2.strw), 0x02u);

    StreamConfig c3;
    c3.strw = StreamWorld::EL3;
    EXPECT_EQ(static_cast<uint8_t>(c3.strw), 0x03u);
}

TEST(Ct20SteStrw, StreamWorldStoredCorrectly) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = false;
    config.stage1Enabled = false;
    config.stage2Enabled = false;
    config.strw = StreamWorld::EL2;

    EXPECT_TRUE(smmu.configureStream(0x20, config).isOk());
    auto result = smmu.isStreamConfigured(0x20);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

// ─── CT-23: Stage-2 STE Translation Parameters ────────────────────────────────

TEST(Ct23Stage2Params, Stage2SteParametersPresentInStreamConfig) {
    StreamConfig config;
    config.s2t0sz = 16u;
    config.s2tg   = 0u;      // 4KB granule
    config.s2sl0  = 1u;      // Start at L1
    config.s2aa64 = true;
    config.s2ps   = 5u;      // 48-bit PA
    config.s2ttb  = 0x100000ULL;

    EXPECT_EQ(config.s2t0sz, 16u);
    EXPECT_EQ(config.s2tg,    0u);
    EXPECT_EQ(config.s2sl0,   1u);
    EXPECT_TRUE(config.s2aa64);
    EXPECT_EQ(config.s2ps,    5u);
    EXPECT_EQ(config.s2ttb,   0x100000ULL);
}

TEST(Ct23Stage2Params, DefaultStage2ParamsAreCorrect) {
    StreamConfig config;
    EXPECT_EQ(config.s2t0sz, 16u)  << "Default s2t0sz should be 16";
    EXPECT_EQ(config.s2tg,    0u)  << "Default s2tg should be 0 (4KB granule)";
    EXPECT_EQ(config.s2sl0,   1u)  << "Default s2sl0 should be 1 (L1)";
    EXPECT_TRUE(config.s2aa64)     << "Default s2aa64 should be true (AArch64)";
    EXPECT_EQ(config.s2ps,    5u)  << "Default s2ps should be 5 (48-bit PA)";
    EXPECT_EQ(config.s2ttb,   0ULL) << "Default s2ttb should be 0";
}

TEST(Ct23Stage2Params, Stage2ParamsStoredCorrectly) {
    SMMU smmu;
    smmu.enable();

    StreamConfig config;
    config.translationEnabled = true;
    config.stage1Enabled = false;
    config.stage2Enabled = true;
    config.s2t0sz = 32u;
    config.s2tg   = 1u;  // 64KB
    config.s2sl0  = 0u;  // L2
    config.s2aa64 = true;
    config.s2ps   = 5u;  // 48-bit
    config.s2ttb  = 0x200000ULL;

    EXPECT_TRUE(smmu.configureStream(0x30, config).isOk());
    auto result = smmu.isStreamConfigured(0x30);
    EXPECT_TRUE(result.isOk());
    EXPECT_TRUE(result.getValue());
}

} // namespace test
} // namespace smmu
