// ARM SMMU v3 Conformance Gap Tests:
//   NEW-GAP-J: F_ACCESS when HA=0, AF=0, AFFD=0 (§3.13.2)
//   NEW-GAP-K: CD.WXN and CD.UWXN write-execute-never (§5.4)
//   NEW-GAP-L: STE.S2PTW protected table walk (§5.2)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>

using namespace smmu;

namespace {

static const PagePermissions RW(true, true, false);
static const PagePermissions RWX(true, true, true);
static const PagePermissions RX(true, false, true);

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
    return cfg;
}

}  // namespace

// ============================================================================
// NEW-GAP-J: F_ACCESS generated when HA=0, AFFD=0, AF=0 (§3.13.2)
// ============================================================================

/// Baseline: translation succeeds when AF=1 (mapPage sets AF=true by default).
TEST(NewGapJ, NoFaultWhenAFIsTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xC0u;
    StreamConfig cfg = stage1Config();
    cfg.ha = false;
    cfg.affd = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    // mapPage creates AF=true by default — no F_ACCESS.
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RW, SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "AF=true: translation must succeed";

    auto evResult = smmu.getEvents();
    ASSERT_TRUE(evResult.isOk());
    bool hasAccessFault = false;
    for (const auto& e : evResult.getValue()) {
        if (e.faultType == FaultType::AccessFlagFault) hasAccessFault = true;
    }
    EXPECT_FALSE(hasAccessFault) << "AF=true: no F_ACCESS event expected";
}

/// NEW-GAP-J Core: F_ACCESS generated when page has AF=0, HA=false, AFFD=false.
TEST(NewGapJ, FaultWhenAFIsZeroHAFalseAFFDFalse) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xC1u;
    StreamConfig cfg = stage1Config();
    cfg.ha = false;
    cfg.affd = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    // Map with AF=false (simulating OS creating a fresh mapping before first access).
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RW,
                             SecurityState::NonSecure, /*accessFlag=*/false).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "AF=0, HA=false, AFFD=false: must return fault";

    auto evResult = smmu.getEvents();
    ASSERT_TRUE(evResult.isOk());
    bool hasAccessFault = false;
    for (const auto& e : evResult.getValue()) {
        if (e.faultType == FaultType::AccessFlagFault) hasAccessFault = true;
    }
    EXPECT_TRUE(hasAccessFault) << "AF=0, HA=false, AFFD=false: F_ACCESS event must be generated";
}

/// AFFD=true suppresses F_ACCESS even when AF=0.
TEST(NewGapJ, NoFaultWhenAFFDTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xC2u;
    StreamConfig cfg = stage1Config();
    cfg.ha = false;
    cfg.affd = true;  // AFFD=1: no AF fault
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RW,
                             SecurityState::NonSecure, /*accessFlag=*/false).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "AFFD=true: F_ACCESS must be suppressed";
}

/// HA=true suppresses F_ACCESS (HTTU manages AF automatically).
TEST(NewGapJ, NoFaultWhenHATrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xC3u;
    StreamConfig cfg = stage1Config();
    cfg.ha = true;   // HA=1: hardware manages AF
    cfg.affd = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RW,
                             SecurityState::NonSecure, /*accessFlag=*/false).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "HA=true: hardware manages AF, no F_ACCESS";
}

// ============================================================================
// NEW-GAP-K: CD.WXN and CD.UWXN write-execute-never (§5.4)
// ============================================================================

/// WXN=1: Execute access to a writable page must fault with F_PERMISSION.
TEST(NewGapK, WXNBlocksExecuteOnWritablePage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xD0u;
    StreamConfig cfg = stage1Config();
    cfg.wxn = true;  // WXN=1: writable → execute-never
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    // Page has write+execute permission — WXN should override the execute permission.
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RWX, SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Execute, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "WXN=1: Execute on writable page must fault";

    auto evResult = smmu.getEvents();
    ASSERT_TRUE(evResult.isOk());
    bool hasPermFault = false;
    for (const auto& e : evResult.getValue()) {
        if (e.faultType == FaultType::PermissionFault) hasPermFault = true;
    }
    EXPECT_TRUE(hasPermFault) << "WXN=1: F_PERMISSION event must be generated";
}

/// WXN=1: Read access to a writable page is allowed.
TEST(NewGapK, WXNAllowsReadOnWritablePage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xD1u;
    StreamConfig cfg = stage1Config();
    cfg.wxn = true;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RWX, SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "WXN=1: Read on writable page must succeed";
}

/// WXN=1: Execute on a read-only (non-writable) executable page is allowed.
TEST(NewGapK, WXNAllowsExecuteOnReadOnlyPage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xD4u;
    StreamConfig cfg = stage1Config();
    cfg.wxn = true;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    // Read-only + execute but NOT writable: WXN must NOT block this.
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RX, SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Execute, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "WXN=1: Execute on read-only (non-writable) page must succeed";
}

/// UWXN=1: ExecutePrivileged access to an unprivileged-writable page must fault.
TEST(NewGapK, UWXNBlocksPrivExecuteOnUnprivWritablePage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xD2u;
    StreamConfig cfg = stage1Config();
    cfg.uwxn = true;  // UWXN=1: unprivileged-writable → not executable for privileged
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    // Unprivileged-writable+executable page (privilegedOnly=false).
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RWX, SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::ExecutePrivileged,
                                 SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "UWXN=1: ExecutePrivileged on unprivileged-writable page must fault";

    auto evResult = smmu.getEvents();
    ASSERT_TRUE(evResult.isOk());
    bool hasPermFault = false;
    for (const auto& e : evResult.getValue()) {
        if (e.faultType == FaultType::PermissionFault) hasPermFault = true;
    }
    EXPECT_TRUE(hasPermFault) << "UWXN=1: F_PERMISSION event must be generated";
}

/// UWXN=1: Unprivileged Execute on unprivileged-writable page is allowed.
TEST(NewGapK, UWXNAllowsUnprivExecuteOnUnprivWritablePage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xD3u;
    StreamConfig cfg = stage1Config();
    cfg.uwxn = true;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    PA pa = 0x2000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, pa, RWX, SecurityState::NonSecure).isOk());

    // Unprivileged Execute on unprivileged-writable page — UWXN only gates privileged.
    auto result = smmu.translate(sid, 0, iova, AccessType::Execute, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "UWXN=1: unprivileged Execute on unprivileged-writable page must succeed";
}

// ============================================================================
// NEW-GAP-L: STE.S2PTW protected table walk (§5.2)
// ============================================================================

/// S2PTW=1: Two-stage translation through a Device-type stage-2 page must fault.
TEST(NewGapL, S2PTWBlocksTranslationThroughDevicePage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xE0u;
    StreamConfig cfg = twoStageConfig();
    cfg.s2ptw = true;  // S2PTW=1: Device-memory stage-2 page → F_PERMISSION
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x1000u;
    IOVA ipa  = 0x8000u;
    PA pa     = 0x9000u;

    // Stage-1: IOVA 0x1000 → IPA 0x8000
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, static_cast<PA>(ipa), RW,
                             SecurityState::NonSecure).isOk());
    // Stage-2: IPA 0x8000 → PA 0x9000, mapped as DEVICE memory via new API.
    ASSERT_TRUE(smmu.mapStage2DevicePage(sid, ipa, pa, RW,
                                         SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk()) << "S2PTW=1: Device-type stage-2 page must cause F_PERMISSION";

    auto evResult = smmu.getEvents();
    ASSERT_TRUE(evResult.isOk());
    bool hasPermFault = false;
    for (const auto& e : evResult.getValue()) {
        if (e.faultType == FaultType::PermissionFault) hasPermFault = true;
    }
    EXPECT_TRUE(hasPermFault) << "S2PTW=1: F_PERMISSION event expected";
}

/// S2PTW=1, Normal memory stage-2 page: translation succeeds.
TEST(NewGapL, S2PTWAllowsTranslationThroughNormalPage) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xE1u;
    StreamConfig cfg = twoStageConfig();
    cfg.s2ptw = true;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x2000u;
    IOVA ipa  = 0xA000u;
    PA pa     = 0xB000u;

    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, static_cast<PA>(ipa), RW,
                             SecurityState::NonSecure).isOk());
    // Normal (non-Device) stage-2 page: use regular setStreamStage2AddressSpace.
    auto stage2AS = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());
    ASSERT_TRUE(stage2AS->mapPage(ipa, pa, RW, SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "S2PTW=1, Normal memory: translation must succeed";
}

/// S2PTW=0: Device-type stage-2 page does not fault.
TEST(NewGapL, NoFaultWhenS2PTWFalse) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xE2u;
    StreamConfig cfg = twoStageConfig();
    cfg.s2ptw = false;  // S2PTW=0: no protection
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    IOVA iova = 0x3000u;
    IOVA ipa  = 0xC000u;
    PA pa     = 0xD000u;

    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, static_cast<PA>(ipa), RW,
                             SecurityState::NonSecure).isOk());
    ASSERT_TRUE(smmu.mapStage2DevicePage(sid, ipa, pa, RW,
                                         SecurityState::NonSecure).isOk());

    auto result = smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk()) << "S2PTW=0: Device-type stage-2 page must not fault";
}
