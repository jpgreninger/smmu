// BUG-1: STRW privilege promotion in translateUnlocked() must be guarded by !stage2Enabled
//
// ARM IHI0070G.b §5.2: "If STE.Config == 0b11x (stage 2 translation enabled) and the STE
// is Non-secure, STRW is IGNORED and the effective StreamWorld is NS-EL1."
//
// The slow path StreamContext::translateUnlocked() applied STRW=EL2/EL3 privilege promotion
// (upgrading AccessType to a Privileged variant) with NO guard on stage2Enabled.  This caused
// a non-privileged access on a two-stage stream with a privilegedOnly stage-1 page to succeed
// when it should fail — STRW must be ignored when stage-2 is enabled.
//
// The TLB fast path in smmu.cpp correctly wraps the same promotion with
//   if (!streamCfgForTlb.stage2Enabled && ...)
// This test verifies the slow path (StreamContext::translate() called directly) applies
// the same guard.
//
// TDD: This test MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"

#include <memory>

namespace smmu {
namespace test {

// -----------------------------------------------------------------------
// Helper: configure a StreamContext with both stage-1 and stage-2 enabled,
// STRW=EL2, a privilegedOnly stage-1 page, and a matching stage-2 page.
// -----------------------------------------------------------------------
static void setupCtxTwoStageStrwEl2(StreamContext& ctx, IOVA s1iova, PA s1ipa, PA s2pa) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.strw               = StreamWorld::EL2;
    cfg.faultMode          = FaultMode::Terminate;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    // Stage-1: map s1iova -> s1ipa, privilegedOnly=true
    // A plain (non-privileged) Read must fail when STRW is correctly ignored.
    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions s1perms(true, false, false, /*privilegedOnly=*/true);
    s1as->mapPage(s1iova, s1ipa, s1perms);
    ctx.addPASID(0, s1as);

    // Stage-2: map s1ipa -> s2pa, not privilegedOnly
    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions s2perms(true, false, false, /*privilegedOnly=*/false);
    s2as->mapPage(s1ipa, s2pa, s2perms);
    ctx.setStage2AddressSpace(s2as);
}

// -----------------------------------------------------------------------
// BUG-1 Slow-Path Test 1:
// Two-stage stream, STRW=EL2, privilegedOnly stage-1 page.
// A plain Read (non-privileged) via StreamContext::translate() MUST FAIL.
// (§5.2: STRW is ignored when stage-2 is enabled — privilegedOnly must apply.)
//
// Before fix: STRW=EL2 promotes Read -> ReadPrivileged in translateUnlocked(),
// bypassing the privilegedOnly check, causing the translation to SUCCEED (wrong).
// After fix:  The !stage2Enabled guard prevents promotion; plain Read fails with
// PagePermissionViolation (correct).
// -----------------------------------------------------------------------
TEST(Bug1StrwSlowPathStage2, TwoStage_StrwEl2_PrivPageSlowPath_PlainReadMustFail) {
    static constexpr IOVA S1_IOVA = 0x1000ULL;
    static constexpr PA   S1_IPA  = 0xA000ULL;
    static constexpr PA   S2_PA   = 0xF000ULL;

    StreamContext ctx;
    setupCtxTwoStageStrwEl2(ctx, S1_IOVA, S1_IPA, S2_PA);

    // Call translate() directly — this exercises translateUnlocked() (the slow path).
    // With the bug: STRW=EL2 incorrectly promotes Read -> ReadPrivileged even though
    // stage2 is enabled, so the privilegedOnly check is bypassed and the result is Ok.
    // With the fix: No promotion occurs (stage2Enabled guards it), so plain Read hits
    // the !intersected.privilegedOnly check and returns PagePermissionViolation.
    TranslationResult result = ctx.translate(0, S1_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "Plain Read on privilegedOnly page in two-stage stream with STRW=EL2 must FAIL "
           "(STRW ignored per ARM §5.2 when stage-2 is enabled)";
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
        << "Must fail with PagePermissionViolation, not a spurious successful translation";
}

// -----------------------------------------------------------------------
// BUG-1 Slow-Path Test 2 (EL3 variant):
// Same as Test 1 but STRW=EL3.  §5.2 applies identically for both EL2 and EL3.
// -----------------------------------------------------------------------
TEST(Bug1StrwSlowPathStage2, TwoStage_StrwEl3_PrivPageSlowPath_PlainReadMustFail) {
    static constexpr IOVA S1_IOVA = 0x2000ULL;
    static constexpr PA   S1_IPA  = 0xB000ULL;
    static constexpr PA   S2_PA   = 0xE000ULL;

    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.strw               = StreamWorld::EL3;
    cfg.faultMode          = FaultMode::Terminate;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions s1perms(true, false, false, /*privilegedOnly=*/true);
    s1as->mapPage(S1_IOVA, S1_IPA, s1perms);
    ctx.addPASID(0, s1as);

    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions s2perms(true, false, false, /*privilegedOnly=*/false);
    s2as->mapPage(S1_IPA, S2_PA, s2perms);
    ctx.setStage2AddressSpace(s2as);

    TranslationResult result = ctx.translate(0, S1_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "Plain Read on privilegedOnly page in two-stage stream with STRW=EL3 must FAIL "
           "(STRW ignored per ARM §5.2 when stage-2 is enabled)";
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
        << "Must fail with PagePermissionViolation for STRW=EL3 two-stage stream";
}

// -----------------------------------------------------------------------
// BUG-1 Slow-Path Test 3 (regression: stage-1-only still correct):
// Single-stage stream, STRW=EL2, privilegedOnly stage-1 page.
// A plain Read MUST SUCCEED — STRW suppression is valid in stage-1-only mode.
// This confirms the fix did not accidentally break the single-stage STRW path.
// -----------------------------------------------------------------------
TEST(Bug1StrwSlowPathStage2, Stage1Only_StrwEl2_PrivPageSlowPath_PlainReadMustSucceed) {
    static constexpr IOVA S1_IOVA = 0x3000ULL;
    static constexpr PA   S1_PA   = 0xC000ULL;

    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.strw               = StreamWorld::EL2;
    cfg.faultMode          = FaultMode::Terminate;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions s1perms(true, false, false, /*privilegedOnly=*/true);
    s1as->mapPage(S1_IOVA, S1_PA, s1perms);
    ctx.addPASID(0, s1as);

    TranslationResult result = ctx.translate(0, S1_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isOk())
        << "Plain Read on privilegedOnly page in stage-1-only stream with STRW=EL2 must SUCCEED "
           "(STRW=EL2 suppresses privilege check per ARM §3.3.4 when stage-2 is NOT enabled)";
}

// -----------------------------------------------------------------------
// BUG-1 Slow-Path Test 4 (regression: EL2_E2H not affected):
// Two-stage stream, STRW=EL2_E2H (VHE), privilegedOnly stage-1 page.
// Plain Read must FAIL — VHE never suppresses privilege checks.
// This ensures the fix does not accidentally widen the promotion to EL2_E2H.
// -----------------------------------------------------------------------
TEST(Bug1StrwSlowPathStage2, TwoStage_StrwEl2E2H_PrivPageSlowPath_PlainReadMustFail) {
    static constexpr IOVA S1_IOVA = 0x4000ULL;
    static constexpr PA   S1_IPA  = 0xD000ULL;
    static constexpr PA   S2_PA   = 0x10000ULL;

    StreamContext ctx;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.strw               = StreamWorld::EL2_E2H;
    cfg.faultMode          = FaultMode::Terminate;
    ctx.updateConfiguration(cfg);
    ctx.enableStream();

    auto s1as = std::make_shared<AddressSpace>();
    PagePermissions s1perms(true, false, false, /*privilegedOnly=*/true);
    s1as->mapPage(S1_IOVA, S1_IPA, s1perms);
    ctx.addPASID(0, s1as);

    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions s2perms(true, false, false, /*privilegedOnly=*/false);
    s2as->mapPage(S1_IPA, S2_PA, s2perms);
    ctx.setStage2AddressSpace(s2as);

    TranslationResult result = ctx.translate(0, S1_IOVA, AccessType::Read);

    EXPECT_TRUE(result.isError())
        << "Plain Read on privilegedOnly page with STRW=EL2_E2H must FAIL "
           "(VHE does not suppress privilege checks per ARM §D5.1.3)";
    EXPECT_EQ(result.getError(), SMMUError::PagePermissionViolation)
        << "Must fail with PagePermissionViolation for EL2_E2H two-stage stream";
}

} // namespace test
} // namespace smmu
