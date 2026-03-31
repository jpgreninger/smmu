// ARM SMMU v3 Conformance Gap Tests: NEW-13, NEW-15, NEW-19
// Copyright (c) 2024 John Greninger
//
// NEW-13 (§3.9.1.2/3.9.1.3): SMMUEN=0 must still emit ATS-specific events
//         (F_BAD_ATS_TREQ for TR, F_TRANSL_FORBIDDEN for TT).
// NEW-15 (§6.3.12/§3.9.1.2): CR2.REC_CFG_ATS gates C_BAD_STREAMID recording
//         for ATS Translation Requests.
// NEW-19 (§3.9.1.2): ATS TR on STE.Config=0b000 (disabled stream) must be
//         silent UR — no event generated.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID  = 0x20;
static constexpr PASID    PID  = 0;

// Minimal SMMU with SMMUEN=1 + EVENTQEN=1 + CR0_CMDQEN=1
// CR2 is left at reset value (0); callers that need specific CR2 bits must set
// them BEFORE calling enable() — setCR2() is ignored when SMMUEN=1.
std::unique_ptr<SMMU> makeEnabledSMMU() {
    auto s = std::make_unique<SMMU>();
    s->enable();
    s->setCR0(s->getCR0() | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    return s;
}

// Minimal SMMU pre-configured with the given CR2 value, then enabled.
std::unique_ptr<SMMU> makeEnabledSMMUWithCR2(uint32_t cr2) {
    auto s = std::make_unique<SMMU>();
    s->setCR2(cr2);
    s->enable();
    s->setCR0(s->getCR0() | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    return s;
}

// Configure a stage-1 translation stream with eats support
void setupS1StreamWithAts(SMMU& smmu, StreamID sid = SID, PASID pasid = PID,
                          uint8_t eats = 2) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    // BUG-NEW-J fix: EATS==2 (split-stage ATS) requires two-stage translation per ARM §5.2.
    cfg.stage2Enabled      = (eats == 2u);
    cfg.bypassEnabled      = false;
    cfg.eats               = eats;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Configure a disabled stream (STE.Config=0b000)
void setupDisabledStream(SMMU& smmu, StreamID sid = SID) {
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.eats               = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
}

} // anonymous namespace

// =============================================================================
// NEW-13: SMMUEN=0 ATS TR/TT event generation
// =============================================================================

class New13SmmUenZeroAts : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = makeEnabledSMMU();
        setupS1StreamWithAts(*smmu_);
    }

    void TearDown() override { smmu_.reset(); }

    std::unique_ptr<SMMU> smmu_;
};

// Test 1: SMMUEN=0 + ATS TR + REC_CFG_ATS=1 → F_BAD_ATS_TREQ emitted
TEST_F(New13SmmUenZeroAts, SmmUenZero_AtsTr_RecCfgAts1_EmitsEvent) {
    // CR2 must be set while SMMUEN=0. Disable, set CR2, then disable again for the test.
    smmu_->disable();
    smmu_->setCR2(SMMU::CR2_REC_CFG_ATS);
    smmu_->enable();
    smmu_->disable();
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk()) << "SMMUEN=0 ATS TR must fail";

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_BAD_ATS_TREQ when SMMUEN=0 and REC_CFG_ATS=1";
    EXPECT_EQ(events[0].type, EventType::F_BAD_ATS_TREQ);
}

// Test 2: SMMUEN=0 + ATS TR + REC_CFG_ATS=0 → no event (silent UR)
TEST_F(New13SmmUenZeroAts, SmmUenZero_AtsTr_RecCfgAts0_Silent) {
    // CR2 = 0 (default): REC_CFG_ATS=0
    smmu_->setCR2(0);
    smmu_->disable();
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk()) << "SMMUEN=0 ATS TR must fail";
    EXPECT_TRUE(smmu_->getEventQueue().empty())
        << "No event when SMMUEN=0 and REC_CFG_ATS=0";
}

// Test 3: SMMUEN=0 + ATS TT → F_TRANSL_FORBIDDEN emitted (always, §7.3.8)
TEST_F(New13SmmUenZeroAts, SmmUenZero_AtsTt_EmitsTranslForbidden) {
    smmu_->disable();
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated);

    EXPECT_FALSE(result.isOk()) << "SMMUEN=0 ATS TT must fail";

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_TRANSL_FORBIDDEN when SMMUEN=0";
    EXPECT_EQ(events[0].type, EventType::F_TRANSL_FORBIDDEN);
}

// Test 4: SMMUEN=0 + Ordinary type → bypass / no event (existing behavior preserved)
TEST_F(New13SmmUenZeroAts, SmmUenZero_Ordinary_Bypass) {
    smmu_->disable();
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read);

    // GBPA bypass: identity mapping, no fault
    EXPECT_TRUE(result.isOk()) << "SMMUEN=0 ordinary must bypass";
    EXPECT_TRUE(smmu_->getEventQueue().empty()) << "No event on bypass";
}

// =============================================================================
// NEW-15: CR2.REC_CFG_ATS gates C_BAD_STREAMID for ATS TR
// =============================================================================

class New15RecCfgAts : public ::testing::Test {
protected:
    void SetUp() override {
        // CR2 must be set before enable() — use inline setup.
        // Set RECINVSID=1 so C_BAD_STREAMID would fire for normal traffic.
        // BUG-AUDIT-63: STRTAB_BASE_CFG is RO when SMMUEN=1; set log2size=8 before enable().
        smmu_ = std::unique_ptr<SMMU>(new SMMU());
        smmu_->setCR2(SMMU::CR2_RECINVSID);
        smmu_->setStrtabLog2Size(8u); // range 0-255; 0x200 (512) is out of range
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    }

    void TearDown() override { smmu_.reset(); }

    std::unique_ptr<SMMU> smmu_;
};

// Test 5: ATS TR + out-of-range SID + RECINVSID=1 + REC_CFG_ATS=0 → no C_BAD_STREAMID event
// SPEC-20: C_BAD_STREAMID requires an out-of-range StreamID (§7.3.3).
// Use LOG2SIZE=8 so SID=0x200 (512) is out of range.
TEST_F(New15RecCfgAts, AtsTr_OutOfRangeSid_RecCfgAts0_NoEvent) {
    // log2size=8 (range 0-255) set in SetUp() before enable() per BUG-AUDIT-63.
    // CR2 has RECINVSID=1 but NOT REC_CFG_ATS
    smmu_->clearEventQueue();
    static constexpr StreamID OUT_OF_RANGE_SID = 0x200u; // 512, out of range

    smmu_->translate(OUT_OF_RANGE_SID, PID, 0x1000, AccessType::Read,
                     SecurityState::NonSecure,
                     TransactionType::AtsTranslationRequest);

    auto events = smmu_->getEventQueue();
    bool hasBadSID = false;
    for (auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) { hasBadSID = true; break; }
    }
    EXPECT_FALSE(hasBadSID)
        << "C_BAD_STREAMID must be suppressed for ATS TR when REC_CFG_ATS=0 (§3.9.1.2)";
}

// Test 6: ATS TR + out-of-range SID + RECINVSID=1 + REC_CFG_ATS=1 → C_BAD_STREAMID emitted
// SPEC-20: C_BAD_STREAMID requires an out-of-range StreamID (§7.3.3).
// Use LOG2SIZE=8 so SID=0x200 (512) is out of range [0,255].
TEST_F(New15RecCfgAts, AtsTr_OutOfRangeSid_BothBitsSet_EmitsEvent) {
    // log2size=8 (range 0-255) set in SetUp() before enable() per BUG-AUDIT-63.
    // Add REC_CFG_ATS to the existing RECINVSID. CR2 must be set while SMMUEN=0.
    smmu_->disable();
    smmu_->setCR2(SMMU::CR2_RECINVSID | SMMU::CR2_REC_CFG_ATS);
    smmu_->enable();
    smmu_->clearEventQueue();
    static constexpr StreamID OUT_OF_RANGE_SID = 0x200u; // 512, out of range

    smmu_->translate(OUT_OF_RANGE_SID, PID, 0x1000, AccessType::Read,
                     SecurityState::NonSecure,
                     TransactionType::AtsTranslationRequest);

    auto events = smmu_->getEventQueue();
    bool hasBadSID = false;
    for (auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) { hasBadSID = true; break; }
    }
    EXPECT_TRUE(hasBadSID)
        << "C_BAD_STREAMID must be emitted for ATS TR with out-of-range SID when REC_CFG_ATS=1 and RECINVSID=1";
}

// Test 7: Ordinary translation + out-of-range SID + RECINVSID=1 → C_BAD_STREAMID
//         emitted WITHOUT needing REC_CFG_ATS (§7.3.3, existing behavior preserved).
// SPEC-20: C_BAD_STREAMID requires an out-of-range StreamID (§7.3.3).
TEST_F(New15RecCfgAts, OrdinaryTr_OutOfRangeSid_Recinvsid_EmitsEvent) {
    // log2size=8 (range 0-255) set in SetUp() before enable() per BUG-AUDIT-63.
    // CR2 = RECINVSID only, no REC_CFG_ATS
    smmu_->clearEventQueue();
    static constexpr StreamID OUT_OF_RANGE_SID = 0x200u; // 512, out of range

    smmu_->translate(OUT_OF_RANGE_SID, PID, 0x1000, AccessType::Read);

    auto events = smmu_->getEventQueue();
    bool hasBadSID = false;
    for (auto& e : events) {
        if (e.type == EventType::C_BAD_STREAMID) { hasBadSID = true; break; }
    }
    EXPECT_TRUE(hasBadSID)
        << "C_BAD_STREAMID must fire for out-of-range SID with RECINVSID=1 (§7.3.3)";
}

// =============================================================================
// NEW-19: STE.Config=0b000 (disabled stream) ATS TR is silent UR
// =============================================================================

class New19DisabledStreamAts : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = makeEnabledSMMU();
    }

    void TearDown() override { smmu_.reset(); }

    std::unique_ptr<SMMU> smmu_;
};

// Test 8: ATS TR on disabled stream → silent UR (no event)
TEST_F(New19DisabledStreamAts, AtsTr_DisabledStream_SilentUr) {
    setupDisabledStream(*smmu_);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk()) << "ATS TR on disabled stream must fail";
    EXPECT_TRUE(smmu_->getEventQueue().empty())
        << "STE.Config=0b000 ATS TR must be silent UR — no event";
}

// Test 9: ATS TR on bypass stream → F_BAD_ATS_TREQ still generated
//         (bypass is NOT STE.Config=0b000, it IS configured)
TEST_F(New19DisabledStreamAts, AtsTr_BypassStream_EmitsBadAtsTreq) {
    StreamConfig bypassCfg;
    bypassCfg.translationEnabled = false;
    bypassCfg.stage1Enabled      = false;
    bypassCfg.stage2Enabled      = false;
    bypassCfg.bypassEnabled      = true;
    bypassCfg.eats               = 0;
    ASSERT_TRUE(smmu_->configureStream(SID, bypassCfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID).isOk());
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk()) << "ATS TR on bypass stream must fail";

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_BAD_ATS_TREQ for bypass stream ATS TR";
    EXPECT_EQ(events[0].type, EventType::F_BAD_ATS_TREQ);
}

// Test 10: Ordinary translation on disabled stream → C_BAD_STE or similar
//          (disabled stream behavior for ordinary traffic unchanged)
TEST_F(New19DisabledStreamAts, OrdinaryTr_DisabledStream_NotSilent) {
    setupDisabledStream(*smmu_);
    smmu_->clearEventQueue();

    // Ordinary translation on a disabled stream may produce an error or event;
    // we only assert it does NOT silently succeed.
    TranslationResult result = smmu_->translate(SID, PID, 0x1000, AccessType::Read);
    EXPECT_FALSE(result.isOk())
        << "Ordinary translation on disabled stream must not succeed";
}
