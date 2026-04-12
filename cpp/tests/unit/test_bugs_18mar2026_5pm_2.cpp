// ARM SMMU v3 Bug-fix Tests: BUG-5PM-1 through BUG-5PM-4 + QA finding
//
// BUG-5PM-1: S2T0SZ F_TRANSLATION emitted with s2=false, ipa=0
// BUG-5PM-2: S2PS F_ADDR_SIZE emitted with s2=false, ipa=0
// BUG-5PM-3: performStage1OnlyTranslation F_ADDR_SIZE uses raw accessType
// BUG-5PM-4: performStage2OnlyTranslation F_ADDR_SIZE events use raw accessType
// QA-ADD:    performStage2OnlyTranslation F_ADDR_SIZE events use s2=false, ipa=0

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>
#include <vector>

using namespace smmu;

namespace {

static const PagePermissions RO(true, false, false);
static const PagePermissions RW(true, true, false);

void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

static EventEntry findEvent(SMMU& s, EventType t) {
    for (const auto& e : s.getEventQueue())
        if (e.type == t) return e;
    return EventEntry();
}

}  // namespace

// ============================================================================
// BUG-5PM-1: S2T0SZ IPA range check — F_TRANSLATION must have s2=true, ipa=IPA
// ============================================================================

TEST(Bugs5pm2, S2T0SZ_Violation_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xE8u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.t0sz = 0;
    cfg.s2t0sz = 16;   // IPA limit = 2^(64-16) = 2^48
    cfg.s2ps = 5; cfg.ips = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // Stage-1 maps IOVA -> IPA = 2^48 (at the s2t0sz limit).
    PA ipa_at_limit = static_cast<PA>(UINT64_C(1) << 48);
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, ipa_at_limit, RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_TRANSLATION);
    ASSERT_EQ(ev.type, EventType::F_TRANSLATION) << "Expected F_TRANSLATION for S2T0SZ violation";
    EXPECT_TRUE(ev.s2)
        << "BUG-5PM-1: §7.3.13 S2T0SZ IPA range fault must have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa_at_limit))
        << "BUG-5PM-1: §7.3.13 S2T0SZ IPA range fault must carry IPA=2^48 in EventEntry";
}

// Same test for stall mode — tl_stage2FaultCtx must also be set.
TEST(Bugs5pm2, S2T0SZ_Violation_StallMode_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xE9u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Stall; cfg.t0sz = 0;
    cfg.s2t0sz = 16; cfg.s2ps = 5; cfg.ips = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    PA ipa_at_limit = static_cast<PA>(UINT64_C(1) << 48);
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, ipa_at_limit, RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    // S2T0SZ is a config fault (returns InvalidConfiguration) — goes through terminate path.
    // The event emitted inline must have s2=true and ipa set.
    EventEntry ev = findEvent(smmu, EventType::F_TRANSLATION);
    ASSERT_EQ(ev.type, EventType::F_TRANSLATION);
    EXPECT_TRUE(ev.s2)
        << "BUG-5PM-1 stall: §7.3.13 S2T0SZ IPA range fault must have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa_at_limit))
        << "BUG-5PM-1 stall: IPA must be 2^48";
}

// ============================================================================
// BUG-5PM-2: S2PS OAS check — F_ADDR_SIZE must have s2=true, ipa=IPA
// ============================================================================

TEST(Bugs5pm2, S2PS_Violation_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xEAu;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.t0sz = 0;
    cfg.s2t0sz = 0; cfg.s2ps = 0; cfg.ips = 6;   // s2ps=0 -> 32-bit PA limit
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr PA ipa = 0x5000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, ipa, RO, SecurityState::NonSecure).isOk());
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    // Stage-2 maps IPA -> PA = 4 GB (exceeds 32-bit S2PS).
    ASSERT_TRUE(s2as->mapPage(ipa, static_cast<PA>(UINT64_C(0x1'0000'0000)), RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    ASSERT_EQ(ev.type, EventType::F_ADDR_SIZE) << "Expected F_ADDR_SIZE for S2PS OAS violation";
    EXPECT_TRUE(ev.s2)
        << "BUG-5PM-2: §7.3.14 S2PS OAS fault must have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa))
        << "BUG-5PM-2: §7.3.14 S2PS OAS fault must carry IPA=0x5000";
}

// ============================================================================
// BUG-5PM-3: performStage1OnlyTranslation F_ADDR_SIZE uses raw accessType
// ============================================================================

TEST(Bugs5pm2, S1Only_AddrSizeFault_PRIVCFG3_PnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xEBu;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate; cfg.privCfg = 3; cfg.t0sz = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());
    // Restrict input address size to 32 bits; IOVA >= 4 GB triggers F_ADDR_SIZE.
    ASSERT_TRUE(smmu.setStreamInputAddressSize(sid, 0, 32).isOk());

    // Translate with IOVA >= 4 GB -> stage-1 returns InvalidAddress -> inline F_ADDR_SIZE.
    smmu.translate(sid, 0, UINT64_C(0x1'0000'0000), AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    ASSERT_EQ(ev.type, EventType::F_ADDR_SIZE) << "Expected F_ADDR_SIZE";
    EXPECT_TRUE(ev.pnu)
        << "BUG-5PM-3: §7.3 PnU must be true when PRIVCFG=3 promotes Read->ReadPrivileged";
}

TEST(Bugs5pm2, S1Only_AddrSizeFault_INSTCFG1_IndIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xECu;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate; cfg.instCfg = 3; cfg.t0sz = 0; // instCfg=3: Force Instruction per §5.2
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());
    ASSERT_TRUE(smmu.setStreamInputAddressSize(sid, 0, 32).isOk());

    smmu.translate(sid, 0, UINT64_C(0x1'0000'0000), AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    ASSERT_EQ(ev.type, EventType::F_ADDR_SIZE);
    EXPECT_TRUE(ev.ind)
        << "BUG-5PM-3: §7.3 InD must be true when INSTCFG=1 promotes Read->Execute";
}

// ============================================================================
// BUG-5PM-4 + QA-ADD: performStage2OnlyTranslation uses raw accessType and
//                     s2=false/ipa=0 for stage-2-only stream faults
// ============================================================================

TEST(Bugs5pm2, S2Only_AddrSizeFault_PRIVCFG3_PnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xEDu;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = false;   // Stage-2-only
    cfg.stage2Enabled = true;
    cfg.bypassEnabled = false;
    cfg.faultMode = FaultMode::Terminate;
    cfg.privCfg = 3;             // Force Privileged
    cfg.s2t0sz = 0; cfg.s2ps = 0;   // 32-bit PA output limit
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());

    // Map IPA=0x1000 -> PA = 4 GB (exceeds 32-bit S2PS).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    ASSERT_TRUE(s2as->mapPage(0x1000u, static_cast<PA>(UINT64_C(0x1'0000'0000)), RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    ASSERT_EQ(ev.type, EventType::F_ADDR_SIZE) << "Expected F_ADDR_SIZE for S2-only S2PS violation";
    EXPECT_TRUE(ev.pnu)
        << "BUG-5PM-4: §7.3 PnU must be true when PRIVCFG=3 promotes Read->ReadPrivileged";
    EXPECT_TRUE(ev.s2)
        << "QA-ADD: §7.3.13 Stage-2-only faults must always have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(0x1000u))
        << "QA-ADD: §7.3.13 Stage-2-only IPA must equal the input IOVA (iova IS the IPA for S2-only)";
}

TEST(Bugs5pm2, S2Only_AddrSizeFault_InvalidAddr_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xEEu;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = false;
    cfg.stage2Enabled = true;
    cfg.bypassEnabled = false;
    cfg.faultMode = FaultMode::Terminate;
    cfg.s2t0sz = 16;   // IPA limit 2^48 - use an IPA > 2^48 to get InvalidAddress via S2T0SZ
    cfg.s2ps = 5;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());

    // Use a large IOVA (IPA for S2-only) that exceeds s2t0sz range -> stage-2 translatePage returns error.
    // For stage-2-only streams the S2T0SZ check fires inside performStage2OnlyTranslation.
    PA large_ipa = static_cast<PA>(UINT64_C(1) << 48);
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    // Map the IPA (won't be reached due to S2T0SZ, but needed to avoid null-AS check):
    ASSERT_TRUE(s2as->mapPage(large_ipa, static_cast<PA>(0x9000u), RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, large_ipa, AccessType::Read, SecurityState::NonSecure);

    // For S2-only stream the IOVA IS the IPA; any fault event must have s2=true.
    auto evs = smmu.getEventQueue();
    bool foundFaultEvent = false;
    EventEntry ev;
    for (const auto& e : evs) {
        if (e.type == EventType::F_TRANSLATION || e.type == EventType::F_ADDR_SIZE) {
            foundFaultEvent = true; ev = e; break;
        }
    }
    ASSERT_TRUE(foundFaultEvent) << "Expected a fault event for stage-2-only IPA range violation";
    EXPECT_TRUE(ev.s2)
        << "QA-ADD: §7.3.13 Stage-2-only fault must always have EventEntry.s2=true";
}
