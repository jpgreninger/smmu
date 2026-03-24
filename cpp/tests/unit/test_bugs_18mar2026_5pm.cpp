// ARM SMMU v3 Bug-fix Tests: newBugs18Mar2026_5pm.md
// NEW-A: Stall path generateEvent uses raw accessType
// NEW-B: handleTranslationFailure eventAccessType omits STRW promotion
// NEW-C: 4 inline generateEvent calls in two-stage path use raw accessType
// NEW-D: S2PTW Device-memory fault doesn't set tl_stage2FaultCtx
// NEW-E: Null stage-2 address space fault doesn't set tl_stage2FaultCtx
// NEW-F: Final S1∩S2 intersection failure never sets tl_stage2FaultCtx

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>
#include <vector>

using namespace smmu;

namespace {

static const PagePermissions RW(true, true, false);
static const PagePermissions RO(true, false, false);
static const PagePermissions WO(false, true, false);

void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

static EventEntry findEvent(SMMU& s, EventType t) {
    for (const auto& e : s.getEventQueue())
        if (e.type == t) return e;
    return EventEntry();
}

static EventEntry findStallEvent(SMMU& s, EventType t) {
    for (const auto& e : s.getEventQueue())
        if (e.type == t && e.stall) return e;
    return EventEntry();
}

}  // namespace

// ============================================================================
// NEW-A: Stall path uses raw accessType
// ============================================================================

TEST(NewA_5pm, StallPath_INSTCFG1_EventIndIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF0u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Stall; cfg.instCfg = 1; cfg.t0sz = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findStallEvent(smmu, EventType::F_TRANSLATION);
    EXPECT_TRUE(ev.stall) << "Must be a stall event";
    EXPECT_TRUE(ev.ind)
        << "NEW-A: §7.3 stall event InD must be true when INSTCFG=1 promotes Read->Execute";
}

TEST(NewA_5pm, StallPath_PRIVCFG3_EventPnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF1u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Stall; cfg.privCfg = 3; cfg.t0sz = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    smmu.translate(sid, 0, 0x2000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findStallEvent(smmu, EventType::F_TRANSLATION);
    EXPECT_TRUE(ev.stall) << "Must be a stall event";
    EXPECT_TRUE(ev.pnu)
        << "NEW-A: §7.3 stall PnU must be true when PRIVCFG=3 promotes Read->ReadPrivileged";
}

// ============================================================================
// NEW-B: handleTranslationFailure omits STRW promotion
// ============================================================================

TEST(NewB_5pm, STRW_EL2_TerminateEvent_PnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF2u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate; cfg.strw = StreamWorld::EL2; cfg.t0sz = 0;
    // ARM §5.2 BUG-CPP-3(a): STRW=EL2 (bit[0]=1) requires Secure security state
    // when stage-1 is active.
    cfg.securityState = SecurityState::Secure;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    smmu.translate(sid, 0, 0x3000u, AccessType::Read, SecurityState::Secure);

    EventEntry ev = findEvent(smmu, EventType::F_TRANSLATION);
    EXPECT_EQ(ev.type, EventType::F_TRANSLATION);
    EXPECT_TRUE(ev.pnu)
        << "NEW-B: §7.3 PnU must be true when STRW=EL2 promotes Read->ReadPrivileged";
}

// ============================================================================
// NEW-C: Inline generateEvent calls in two-stage path use raw accessType
// ============================================================================

TEST(NewC_5pm, IPS_Violation_PnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF3u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.privCfg = 3; cfg.t0sz = 0;
    cfg.s2t0sz = 0; cfg.s2ps = 5; cfg.ips = 0;  // 32-bit IPS limit
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // Stage-1 maps IOVA -> IPA = 4 GB (at the 32-bit IPS limit, triggers violation).
    PA ipa_at_limit = static_cast<PA>(UINT64_C(0x1'0000'0000));
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, ipa_at_limit, RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    bool found = false; EventEntry ev;
    for (const auto& e : smmu.getEventQueue())
        if (e.type == EventType::F_ADDR_SIZE) { found = true; ev = e; break; }
    ASSERT_TRUE(found) << "Expected F_ADDR_SIZE for IPS violation";
    EXPECT_TRUE(ev.pnu)
        << "NEW-C IPS: §7.3 PnU must reflect PRIVCFG=3 (Read->ReadPrivileged)";
}

TEST(NewC_5pm, S2T0SZ_Violation_PnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF4u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.privCfg = 3; cfg.t0sz = 0;
    cfg.s2t0sz = 16; cfg.s2ps = 5; cfg.ips = 6;  // s2t0sz=16 -> IPA limit = 2^48
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // Stage-1 maps IOVA -> IPA = 2^48 (at the s2t0sz limit).
    PA ipa_at_limit = static_cast<PA>(UINT64_C(1) << 48);
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, ipa_at_limit, RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    bool found = false; EventEntry ev;
    for (const auto& e : smmu.getEventQueue())
        if (e.type == EventType::F_TRANSLATION) { found = true; ev = e; break; }
    ASSERT_TRUE(found) << "Expected F_TRANSLATION for S2T0SZ violation";
    EXPECT_TRUE(ev.pnu)
        << "NEW-C S2T0SZ: §7.3 PnU must reflect PRIVCFG=3 (Read->ReadPrivileged)";
}

TEST(NewC_5pm, S2PS_OAS_Violation_PnuIsTrue) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF5u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.privCfg = 3; cfg.t0sz = 0;
    cfg.s2t0sz = 0; cfg.s2ps = 0; cfg.ips = 6;  // s2ps=0 -> 32-bit PA limit
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // Stage-1: IOVA=0x1000 -> IPA=0x5000 (within all limits).
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, static_cast<PA>(0x5000u), RO, SecurityState::NonSecure).isOk());
    // Stage-2: IPA=0x5000 -> PA=4 GB (exceeds 32-bit S2PS).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    ASSERT_TRUE(s2as->mapPage(0x5000u, static_cast<PA>(UINT64_C(0x1'0000'0000)), RO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    bool found = false; EventEntry ev;
    for (const auto& e : smmu.getEventQueue())
        if (e.type == EventType::F_ADDR_SIZE) { found = true; ev = e; break; }
    ASSERT_TRUE(found) << "Expected F_ADDR_SIZE for S2PS OAS violation";
    EXPECT_TRUE(ev.pnu)
        << "NEW-C S2PS: §7.3 PnU must reflect PRIVCFG=3 (Read->ReadPrivileged)";
}

// ============================================================================
// NEW-D: S2PTW Device-memory fault missing tl_stage2FaultCtx
// ============================================================================

TEST(NewD_5pm, S2PTW_DeviceFault_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF6u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.s2ptw = true; cfg.t0sz = 0;
    cfg.s2t0sz = 0; cfg.s2ps = 5;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr IOVA iova = 0x1000u;
    constexpr IOVA ipa  = 0x8000u;
    constexpr PA   pa   = 0x9000u;
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, static_cast<PA>(ipa), RW, SecurityState::NonSecure).isOk());
    ASSERT_TRUE(smmu.mapStage2DevicePage(sid, ipa, pa, RW, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);

    bool found = false; EventEntry ev;
    for (const auto& e : smmu.getEventQueue())
        if (e.type == EventType::F_PERMISSION) { found = true; ev = e; break; }
    ASSERT_TRUE(found) << "Expected F_PERMISSION for S2PTW Device fault";
    EXPECT_TRUE(ev.s2)
        << "NEW-D: §7.3.16 S2PTW Device fault must have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa))
        << "NEW-D: §7.3.16 S2PTW Device fault must carry IPA=0x8000 in EventEntry";
}

// ============================================================================
// NEW-E: Stage-2 address space translation failure (IPA not mapped in stage-2)
// missing tl_stage2FaultCtx — exercises the same code path as the null-AS guard
// in performBothStagesTranslation (stage-2 returns PageNotMapped → F_TRANSLATION
// must carry s2=true and the IPA).
// ============================================================================

TEST(NewE_5pm, Stage2IPANotMapped_TranslationFault_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF7u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.t0sz = 0; cfg.s2t0sz = 0; cfg.s2ps = 5;
    cfg.ips = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr IOVA iova = 0x1000u;
    constexpr PA   ipa  = 0x5000u;
    // Stage-1: maps IOVA -> IPA.
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, ipa, RO, SecurityState::NonSecure).isOk());
    // Stage-2: provide a SEPARATE address space that does NOT have IPA=0x5000 mapped.
    // This triggers the stage-2 PageNotMapped path (same S2=true/IPA semantics as null AS).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    // s2as intentionally has NO mappings — IPA 0x5000 will not be found.

    smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);

    bool found = false; EventEntry ev;
    for (const auto& e : smmu.getEventQueue())
        if (e.type == EventType::F_TRANSLATION) { found = true; ev = e; break; }
    ASSERT_TRUE(found) << "Expected F_TRANSLATION event for unmapped stage-2 IPA";
    EXPECT_TRUE(ev.s2)
        << "NEW-E: §7.3.13 stage-2 IPA-not-mapped fault must have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa))
        << "NEW-E: §7.3.13 stage-2 IPA-not-mapped fault must carry IPA=0x5000";
}

// ============================================================================
// NEW-F: S1∩S2 intersection failure never sets tl_stage2FaultCtx
// ============================================================================

TEST(NewF_5pm, S1PermitsS2Denies_PermissionFault_S2TrueAndIpaSet) {
    SMMU smmu; enableSMMU(smmu);
    StreamID sid = 0xF8u;
    StreamConfig cfg;
    cfg.translationEnabled = true; cfg.stage1Enabled = true; cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate; cfg.t0sz = 0; cfg.s2t0sz = 0;
    cfg.s2ps = 5; cfg.ips = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr IOVA iova = 0x1000u;
    constexpr PA   ipa  = 0x6000u;
    constexpr PA   pa   = 0x7000u;
    // Stage-1: permits Read (read-only page).
    ASSERT_TRUE(smmu.mapPage(sid, 0, iova, ipa, RO, SecurityState::NonSecure).isOk());
    // Stage-2: denies Read (write-only page).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
    ASSERT_TRUE(s2as->mapPage(ipa, pa, WO, SecurityState::NonSecure).isOk());

    smmu.translate(sid, 0, iova, AccessType::Read, SecurityState::NonSecure);

    bool found = false; EventEntry ev;
    for (const auto& e : smmu.getEventQueue())
        if (e.type == EventType::F_PERMISSION) { found = true; ev = e; break; }
    ASSERT_TRUE(found) << "Expected F_PERMISSION event";
    EXPECT_TRUE(ev.s2)
        << "NEW-F: §7.3.16 S1∩S2 fault where S2 binds must have EventEntry.s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa))
        << "NEW-F: §7.3.16 S1∩S2 fault must carry IPA=0x6000";
}
