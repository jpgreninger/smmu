// TDD failing tests for BUG-AUDIT-164-CPP: OSH enforcement misses Device sub-types and
// Normal Non-Cacheable memory types.
//
// ARM §3.15 / §13.1.7 Rule 1: ALL Device memory types and Normal Non-Cacheable MUST use
// Outer Shareable (OSH=2) regardless of STE.SHCfg / GBPA.SHCfg.
//
// Device memory encoding (§6.3.22 MemAttr / SHCfg):
//   0x0 = Device-nGnRnE
//   0x1 = Device-nGnRE
//   0x2 = Device-nGRE
//   0x3 = Device-GRE
//   0x4 = Reserved (Device alias per SMMU model)
//   0x5 = Normal Non-Cacheable (inner-NC / outer-NC) — also requires OSH
//   0x8 = Reserved (Device alias per SMMU model)
//   0xC = Reserved (Device alias per SMMU model)
//
// BUG: Three sites only check `== 0x00u`, missing values 0x1–0x3, 0x4, 0x5, 0x8, 0xC:
//   1. stream_context.cpp:1213  (mtCfg=true branch, applyOutputAttrs lambda)
//   2. smmu.cpp:563             (TLB fast path, mtCfg=true branch)
//   3. smmu.cpp:311             (GBPA bypass path)
//
// These tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"
#include <memory>

namespace smmu {
namespace test {

// ─── Site 1 tests: stream_context.cpp applyOutputAttrs (mtCfg=true) ─────────

static TranslationResult translateWithMemAttr(uint8_t memAttr, uint8_t shCfg)
{
    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.mtCfg              = true;
    cfg.memAttr            = memAttr;
    cfg.shCfg              = shCfg;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto as = std::make_shared<AddressSpace>();
    PagePermissions rw(true, true, false);
    as->mapPage(0x1000u, 0x2000u, rw);
    ctx.addPASID(0u, as);
    ctx.setStage1Enabled(true);

    return ctx.translate(0u, 0x1000u, AccessType::Read, SecurityState::NonSecure);
}

// memAttr=0x1 (Device-nGnRE), shCfg=1 (ISH) → must force OSH (2)
TEST(BugAudit164CppOshTest, SC_DeviceNGnRE_ForcesOSH)
{
    auto result = translateWithMemAttr(0x1u, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP: Device-nGnRE (memAttr=0x1) with ISH must force OSH=2. "
           "§3.15/§13.1.7 Rule 1 applies to all Device sub-types, not just 0x00.";
}

// memAttr=0x2 (Device-nGRE), shCfg=1 (ISH) → must force OSH (2)
TEST(BugAudit164CppOshTest, SC_DeviceNGRE_ForcesOSH)
{
    auto result = translateWithMemAttr(0x2u, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP: Device-nGRE (memAttr=0x2) with ISH must force OSH=2.";
}

// memAttr=0x3 (Device-GRE), shCfg=1 (ISH) → must force OSH (2)
TEST(BugAudit164CppOshTest, SC_DeviceGRE_ForcesOSH)
{
    auto result = translateWithMemAttr(0x3u, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP: Device-GRE (memAttr=0x3) with ISH must force OSH=2.";
}

// memAttr=0x5 (Normal Non-Cacheable), shCfg=1 (ISH) → must force OSH (2)
TEST(BugAudit164CppOshTest, SC_NormalNC_ForcesOSH)
{
    auto result = translateWithMemAttr(0x5u, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP: Normal-iNC-oNC (memAttr=0x5) with ISH must force OSH=2. "
           "§3.15 line 3721 requires OSH for Normal Non-Cacheable.";
}

// memAttr=0x8 (Reserved/Device), shCfg=1 (ISH) → must force OSH (2)
TEST(BugAudit164CppOshTest, SC_Reserved0x8_ForcesOSH)
{
    auto result = translateWithMemAttr(0x8u, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP: Reserved Device alias (memAttr=0x8) with ISH must force OSH=2.";
}

// memAttr=0xC (Reserved/Device), shCfg=1 (ISH) → must force OSH (2)
TEST(BugAudit164CppOshTest, SC_Reserved0xC_ForcesOSH)
{
    auto result = translateWithMemAttr(0xCu, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP: Reserved Device alias (memAttr=0xC) with ISH must force OSH=2.";
}

// Regression: memAttr=0x0 (Device-nGnRnE) still forces OSH — must not regress
TEST(BugAudit164CppOshTest, SC_DeviceNGnRnE_ForcesOSH_Regression)
{
    auto result = translateWithMemAttr(0x0u, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "Regression: Device-nGnRnE (0x00) must still force OSH=2.";
}

// Regression: Normal WB (0xFF) does NOT force OSH — must not regress
TEST(BugAudit164CppOshTest, SC_NormalWB_KeepsConfiguredSH)
{
    auto result = translateWithMemAttr(0xFFu, 1u);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 1u)
        << "Normal WB (0xFF) must keep configured ISH (1), not force OSH.";
}

// ─── Site 3 tests: smmu.cpp GBPA bypass path ─────────────────────────────────

static void setGbpa(SMMU& smmu, bool mtCfg, uint8_t memAttr, uint8_t shCfg)
{
    GbpaConfig gbpa;
    gbpa.abort    = false;
    gbpa.mtCfg    = mtCfg;
    gbpa.memAttr  = memAttr;
    gbpa.shCfg    = shCfg;
    gbpa.allocCfg = 0;
    gbpa.instCfg  = 0;
    gbpa.privCfg  = 0;
    smmu.setGbpaConfig(gbpa);
}

// GBPA: Device-nGnRE (0x1), ISH → must force OSH
TEST(BugAudit164CppOshGbpaTest, Gbpa_DeviceNGnRE_ForcesOSH)
{
    SMMU smmu;
    setGbpa(smmu, true, 0x1u, 1u);
    auto result = smmu.translate(0x10u, 0u, 0x5000u, AccessType::Read,
                                 SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP GBPA site: Device-nGnRE (0x1) must force OSH=2.";
}

// GBPA: Normal-iNC-oNC (0x5), ISH → must force OSH
TEST(BugAudit164CppOshGbpaTest, Gbpa_NormalNC_ForcesOSH)
{
    SMMU smmu;
    setGbpa(smmu, true, 0x5u, 1u);
    auto result = smmu.translate(0x11u, 0u, 0x5000u, AccessType::Read,
                                 SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP GBPA site: Normal-iNC-oNC (0x5) must force OSH=2.";
}

// GBPA: Device-GRE (0x3), ISH → must force OSH
TEST(BugAudit164CppOshGbpaTest, Gbpa_DeviceGRE_ForcesOSH)
{
    SMMU smmu;
    setGbpa(smmu, true, 0x3u, 1u);
    auto result = smmu.translate(0x12u, 0u, 0x5000u, AccessType::Read,
                                 SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP GBPA site: Device-GRE (0x3) must force OSH=2.";
}

// GBPA: Reserved (0x8), ISH → must force OSH
TEST(BugAudit164CppOshGbpaTest, Gbpa_Reserved0x8_ForcesOSH)
{
    SMMU smmu;
    setGbpa(smmu, true, 0x8u, 1u);
    auto result = smmu.translate(0x13u, 0u, 0x5000u, AccessType::Read,
                                 SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-AUDIT-164-CPP GBPA site: Reserved/Device (0x8) must force OSH=2.";
}

// GBPA regression: Normal WB (0xFF) keeps ISH
TEST(BugAudit164CppOshGbpaTest, Gbpa_NormalWB_KeepsConfiguredSH)
{
    SMMU smmu;
    setGbpa(smmu, true, 0xFFu, 1u);
    auto result = smmu.translate(0x14u, 0u, 0x5000u, AccessType::Read,
                                 SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().shareability, 1u)
        << "GBPA regression: Normal WB (0xFF) must keep configured ISH, not force OSH.";
}

}  // namespace test
}  // namespace smmu
