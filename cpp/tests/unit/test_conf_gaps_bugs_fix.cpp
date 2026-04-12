// ARM SMMU v3 Bug-fix Tests:
//   BUG-CPP-3: Missing F_ADDR_SIZE event for stage-2 address-size fault
//              in two-stage translation when stage-2 translatePage returns
//              InvalidAddress (§7.3.14, IHI0070G_b).
//   BUG-CPP-5: TOCTOU race in handleTranslationFailure config re-fetch —
//              event InD/PnU must reflect post-STE override values at
//              translation time (§7.3, IHI0070G_b).

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
    for (const auto& e : s.getEventQueue()) {
        if (e.type == t) {
            return e;
        }
    }
    return EventEntry();
}

}  // namespace

// ============================================================================
// BUG-CPP-3: stage-2 address-size fault must emit F_ADDR_SIZE with s2=true
// ============================================================================

// BUG-CPP-3 core: when stage-2 translatePage returns InvalidAddress
// (IPA >= 2^stage2InputAddressSizeBits), an F_ADDR_SIZE event with
// s2=true and ipa=<IPA> must be generated (§7.3.14).
TEST(BugCpp3, TwoStageS2AddrSizeFaultEmitsEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF0u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;    // no S1 VA range restriction
    cfg.s2t0sz             = 0;    // no S2 IPA range restriction
    cfg.s2ps               = 5;    // 48-bit PA space
    cfg.ips                = 6;    // 52-bit IPS — stage-1 output unconstrained
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // Stage-1: IOVA 0x1000 → IPA = 4 GB (first address above 32-bit space)
    constexpr PA ipa = static_cast<PA>(UINT64_C(0x1'0000'0000));
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000u, ipa, RO, SecurityState::NonSecure).isOk());

    // Stage-2: separate AS with inputAddressSizeBits=32 so the IPA (4 GB) triggers
    // an address-size fault when stage-2 translatePage is called.
    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->setInputAddressSize(32);  // limits stage-2 IPA space to 32 bits
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());

    // Translation must fail (address-size fault at stage-2).
    auto result = smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "BUG-CPP-3: two-stage S2 address-size fault must return error";

    // §7.3.14: F_ADDR_SIZE event must be present in the event queue.
    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    EXPECT_EQ(ev.type, EventType::F_ADDR_SIZE)
        << "BUG-CPP-3: §7.3.14 — F_ADDR_SIZE event must be generated on stage-2 address-size fault";
}

// BUG-CPP-3 s2 flag: the F_ADDR_SIZE event must carry s2=true to indicate
// the fault originated at stage-2 translation, not stage-1 (§7.3).
TEST(BugCpp3, TwoStageS2AddrSizeFaultEventHasS2True) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF1u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    cfg.ips                = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr PA ipa = static_cast<PA>(UINT64_C(0x1'0000'0000));
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x2000u, ipa, RO, SecurityState::NonSecure).isOk());

    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->setInputAddressSize(32);
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());

    smmu.translate(sid, 0, 0x2000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    ASSERT_EQ(ev.type, EventType::F_ADDR_SIZE)
        << "BUG-CPP-3: F_ADDR_SIZE event must be present";
    EXPECT_TRUE(ev.s2)
        << "BUG-CPP-3: §7.3 — F_ADDR_SIZE from stage-2 fault must have s2=true";
}

// BUG-CPP-3 ipa field: the F_ADDR_SIZE event must carry the IPA that was
// input to stage-2 (i.e. the stage-1 output) in the ipa field (§7.3).
TEST(BugCpp3, TwoStageS2AddrSizeFaultEventCarriesIPA) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF2u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    cfg.ips                = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr PA ipa = static_cast<PA>(UINT64_C(0x1'0000'0000));
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x3000u, ipa, RO, SecurityState::NonSecure).isOk());

    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->setInputAddressSize(32);
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());

    smmu.translate(sid, 0, 0x3000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    ASSERT_EQ(ev.type, EventType::F_ADDR_SIZE)
        << "BUG-CPP-3: F_ADDR_SIZE event must be present";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa))
        << "BUG-CPP-3: §7.3 — F_ADDR_SIZE event must carry the IPA (stage-1 output)";
}

// BUG-CPP-3 stall mode: stage-2 address-size fault in stall mode must also
// emit an F_ADDR_SIZE event (the stall path must handle it correctly).
TEST(BugCpp3, TwoStageS2AddrSizeFaultStallModeEmitsEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF3u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Stall;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    cfg.ips                = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    constexpr PA ipa = static_cast<PA>(UINT64_C(0x1'0000'0000));
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x4000u, ipa, RO, SecurityState::NonSecure).isOk());

    auto stage2AS = std::make_shared<AddressSpace>();
    stage2AS->setInputAddressSize(32);
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, stage2AS).isOk());

    smmu.translate(sid, 0, 0x4000u, AccessType::Read, SecurityState::NonSecure);

    // In stall mode, an F_ADDR_SIZE event must still be generated.
    EventEntry ev = findEvent(smmu, EventType::F_ADDR_SIZE);
    EXPECT_EQ(ev.type, EventType::F_ADDR_SIZE)
        << "BUG-CPP-3: stall mode — F_ADDR_SIZE event must be generated on S2 address-size fault";
    EXPECT_TRUE(ev.s2)
        << "BUG-CPP-3: stall mode — F_ADDR_SIZE must have s2=true";
    EXPECT_EQ(ev.ipa, static_cast<uint64_t>(ipa))
        << "BUG-CPP-3: stall mode — F_ADDR_SIZE must carry the IPA";
}

// ============================================================================
// BUG-CPP-5: handleTranslationFailure must use effective (post-override)
//            access type for event InD/PnU fields (§7.3, IHI0070G_b).
// ============================================================================

// BUG-CPP-5 baseline: with PRIVCFG=3 (promote to privileged), a translation
// fault's F_TRANSLATION event must have pnu=true reflecting the override
// applied at translation time — not whatever config is present at event-emit time.
//
// This is a deterministic test documenting the expected behaviour.  The
// underlying TOCTOU race cannot be reproduced deterministically in a
// single-threaded test; the fix (passing effective AccessType directly to
// handleTranslationFailure instead of re-fetching config) closes the window.
TEST(BugCpp5, PRIVCFG3_TranslationFault_PnuIsTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF8u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.privCfg            = 3;    // promote unprivileged to privileged
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());
    // Do NOT map the IOVA — this causes a translation fault.

    smmu.translate(sid, 0, 0x5000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_TRANSLATION);
    ASSERT_EQ(ev.type, EventType::F_TRANSLATION)
        << "BUG-CPP-5: F_TRANSLATION event must be generated";
    // PRIVCFG=3 promotes Read → ReadPrivileged; pnu must be true.
    EXPECT_TRUE(ev.pnu)
        << "BUG-CPP-5: §7.3 — PRIVCFG=3 promotes to privileged; event pnu must be true";
}

// BUG-CPP-5 INSTCFG=1 (data→instruction): with INSTCFG=1, a Read is treated
// as Execute.  F_TRANSLATION event must have ind=true.
TEST(BugCpp5, INSTCFG1_TranslationFault_IndIsTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xF9u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.instCfg            = 3;    // Force Instruction per §5.2 (0b11): Read → Execute
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());
    // Do NOT map the IOVA — this causes a translation fault.

    smmu.translate(sid, 0, 0x6000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_TRANSLATION);
    ASSERT_EQ(ev.type, EventType::F_TRANSLATION)
        << "BUG-CPP-5: F_TRANSLATION event must be generated";
    // INSTCFG=1 maps Read → Execute; ind must be true.
    EXPECT_TRUE(ev.ind)
        << "BUG-CPP-5: §7.3 — INSTCFG=1 maps Read→Execute; event ind must be true";
}

// BUG-CPP-5 two-stage: with PRIVCFG=3 on a two-stage stream, a translation
// fault's F_TRANSLATION event must have pnu=true.
TEST(BugCpp5, PRIVCFG3_TwoStage_TranslationFault_PnuIsTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamID sid = 0xFAu;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.privCfg            = 3;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    cfg.ips                = 6;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());
    // Do NOT map anything — stage-1 will produce a translation fault.

    smmu.translate(sid, 0, 0x7000u, AccessType::Read, SecurityState::NonSecure);

    EventEntry ev = findEvent(smmu, EventType::F_TRANSLATION);
    ASSERT_EQ(ev.type, EventType::F_TRANSLATION)
        << "BUG-CPP-5: F_TRANSLATION event must be generated";
    EXPECT_TRUE(ev.pnu)
        << "BUG-CPP-5: §7.3 — PRIVCFG=3 on two-stage stream; event pnu must be true";
}
