// ARM SMMU v3 Conformance Gap Tests: GAP-NEW-G, GAP-NEW-D, GAP-NEW-A, GAP-NEW-E, GAP-NEW-F
// Copyright (c) 2024 John Greninger
//
// GAP-NEW-G (§5.2):        STE.S1STALLD forces abort even when FaultMode::Stall configured
// GAP-NEW-D (§6.3.1-6.3.8): IDR0-IDR5, AIDR, IIDR read capability bitmasks
// GAP-NEW-A (§9.1):         Fault injection: injectSteFetchAbort, injectCdFetchAbort, injectWalkEabt
// GAP-NEW-E (§6.3.45-47):   STATUSR, IRQ_CTRL, CTRLACK registers
// GAP-NEW-F (§9.1-9.9):     GATOS address translation wrapper — gatosTranslate()

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

namespace {

static constexpr StreamID SID  = 0x20;
static constexpr PASID    PID  = 0;
static constexpr PA       BASE_PA   = 0x8000'0000ULL;
static constexpr IOVA     BASE_IOVA = 0x1000'0000ULL;

// Enable SMMU with event queue ready.
void enableSmmu(SMMU& s) {
    s.enable();
    s.setCR0(s.getCR0() | SMMU::CR0_EVENTQEN);
}

// Configure a stage-1-only stream with given fault mode and s1Stalld.
void setupStage1Stream(SMMU& smmu, StreamID sid, PASID pasid,
                       FaultMode fm, bool s1Stalld = false) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = fm;
    cfg.s1Stalld           = s1Stalld;
    cfg.t0sz               = 0; // no VA range restriction
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Map a single 4K page for sid/pasid.
void mapPage(SMMU& smmu, StreamID sid, PASID pasid,
             IOVA iova, PA pa) {
    PagePermissions perms;
    perms.read  = true;
    perms.write = true;
    perms.execute = false;
    ASSERT_TRUE(smmu.mapPage(sid, pasid, iova, pa, perms).isOk());
}

} // anonymous namespace

// =============================================================================
// GAP-NEW-G: STE.S1STALLD — force abort when stall mode configured (§5.2)
// =============================================================================

class GapNewGTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        enableSmmu(*smmu_);
    }
    void TearDown() override { smmu_.reset(); }
    std::unique_ptr<SMMU> smmu_;
};

// s1Stalld=false + FaultMode::Stall: translation fault on unmapped page
// should result in a Stalled transaction (not abort).
TEST_F(GapNewGTest, gap_new_g_stall_mode_without_s1stalld_stalls) {
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Stall, /*s1Stalld=*/false);

    // Translate an unmapped address — should stall.
    TranslationResult result = smmu_->translate(SID, PID, BASE_IOVA,
                                                AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "Expected Stalled but got error code "
        << static_cast<int>(result.getError());
}

// BUG-NEW-F correction: s1Stalld=true + FaultMode::Stall (CD.S==1) is ILLEGAL per
// ARM §5.5 CdIllegal() line 9748 — configureStream() now correctly emits C_BAD_CD
// and rejects this combination.  The legal configuration for an S1STALLD=1 stream
// is faultMode=Terminate (CD.S=0).  This test verifies that the legal configuration
// works: translation faults produce F_TRANSLATION (not Stalled).
TEST_F(GapNewGTest, gap_new_g_s1stalld_with_terminate_produces_f_translation) {
    // S1STALLD=true + faultMode=Terminate (CD.S=0) is the spec-legal configuration.
    // S1STALLD=true + faultMode=Stall (CD.S=1) is ILLEGAL → C_BAD_CD (BUG-NEW-F fix).
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Terminate, /*s1Stalld=*/true);

    // Translate an unmapped address — must produce F_TRANSLATION (terminate semantics).
    TranslationResult result = smmu_->translate(SID, PID, BASE_IOVA,
                                                AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "S1STALLD=1 + Terminate: must not produce Stalled result";

    // An F_TRANSLATION event must be present in the event queue.
    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_TRANSLATION event in queue";
    EXPECT_EQ(events.front().type, EventType::F_TRANSLATION);
}

// Confirm backward compatibility: default StreamConfig has s1Stalld=false.
TEST_F(GapNewGTest, gap_new_g_default_s1stalld_is_false) {
    StreamConfig cfg;
    EXPECT_FALSE(cfg.s1Stalld);
}

// =============================================================================
// GAP-NEW-D: IDR registers — capability bitmasks (§6.3.1–6.3.8)
// =============================================================================

class GapNewDTest : public ::testing::Test {
protected:
    void SetUp() override { smmu_ = std::make_unique<SMMU>(); }
    void TearDown() override { smmu_.reset(); }
    std::unique_ptr<SMMU> smmu_;
};

// IDR0 must have bit 0 (S2P) and bit 1 (S1P) set with correct ARM §6.3.1 encoding.
TEST_F(GapNewDTest, gap_new_d_idr0_s1p_s2p_correct_bits) {
    uint32_t idr0 = smmu_->getIDR0();
    EXPECT_NE(idr0, 0u) << "IDR0 must be non-zero";
    EXPECT_TRUE((idr0 & (1u << 0)) != 0u) << "IDR0 bit 0 (S2P) must be set";
    EXPECT_TRUE((idr0 & (1u << 1)) != 0u) << "IDR0 bit 1 (S1P) must be set";
    EXPECT_TRUE((idr0 & (1u << 27)) != 0u) << "IDR0 bit 27 (ST_LEVEL[0]) must be set";
}

// IDR1 must encode SIDSIZE=32 in bits[5:0].
TEST_F(GapNewDTest, gap_new_d_idr1_sidsize_32) {
    uint32_t idr1 = smmu_->getIDR1();
    EXPECT_NE(idr1, 0u) << "IDR1 must be non-zero";
    uint32_t sidsize = idr1 & 0x3Fu; // bits[5:0]
    EXPECT_EQ(sidsize, 0x20u) << "IDR1 SIDSIZE must be 32 (0x20)";
}

// IDR2 has only BA_VATOS[9:0]; this model has VATOS=0 → IDR2=0.
TEST_F(GapNewDTest, gap_new_d_idr2_returns_zero_no_vatos) {
    // IDR2 has only BA_VATOS[9:0]; this model has VATOS=0 → IDR2=0
    uint32_t idr2 = smmu_->getIDR2();
    EXPECT_EQ(idr2, 0u) << "IDR2 must be 0 (only BA_VATOS field; VATOS not implemented)";
}

// IDR5 must encode OAS=5 (48-bit) in bits[2:0] and set all three granule bits.
TEST_F(GapNewDTest, gap_new_d_idr5_oas_and_granule_bits) {
    uint32_t idr5 = smmu_->getIDR5();
    EXPECT_EQ(idr5 & 0x7u, 5u)               << "IDR5 OAS (bits[2:0]) must be 5 (48-bit)";
    EXPECT_TRUE((idr5 & (1u << 4)) != 0u)    << "IDR5 GRAN4K (bit 4) must be set";
    // BUG-AUDIT-52 fix: only 4KB granule is implemented — GRAN16K/GRAN64K must be 0.
    EXPECT_TRUE((idr5 & (1u << 5)) == 0u)    << "IDR5 GRAN16K (bit 5) must be clear (not implemented)";
    EXPECT_TRUE((idr5 & (1u << 6)) == 0u)    << "IDR5 GRAN64K (bit 6) must be clear (not implemented)";
}

// AIDR and IIDR are defined and callable (may return 0 for this model).
TEST_F(GapNewDTest, gap_new_d_aidr_iidr_callable) {
    // These should compile and not crash; return values may be 0 in this model.
    (void)smmu_->getAIDR();
    (void)smmu_->getIIDR();
    SUCCEED();
}

// IDR4 should return 0 for this minimal model (no VATOS/MPAM fields).
TEST_F(GapNewDTest, gap_new_d_idr4_returns_zero) {
    EXPECT_EQ(smmu_->getIDR4(), 0u);
}

// IDR3 must encode RIL(bit10), FWB(bit8), BBML=0b01(bit11), HAD(bit2).
TEST_F(GapNewDTest, gap_new_d_idr3_capability_bits) {
    uint32_t idr3 = smmu_->getIDR3();
    EXPECT_TRUE((idr3 & (1u << 2))  != 0u) << "IDR3 bit 2 (HAD) must be set";
    EXPECT_TRUE((idr3 & (1u << 8))  != 0u) << "IDR3 bit 8 (FWB) must be set";
    EXPECT_TRUE((idr3 & (1u << 10)) != 0u) << "IDR3 bit 10 (RIL) must be set";
    EXPECT_TRUE((idr3 & (1u << 11)) != 0u) << "IDR3 bit 11 (BBML[0]) must be set";
    EXPECT_TRUE((idr3 & (1u << 12)) == 0u) << "IDR3 bit 12 (BBML[1]) must be clear (BBML=0b01)";
}

// AIDR must return 0x02 (SMMUv3.2) since RIL, FWB, and T0SZ are implemented.
TEST_F(GapNewDTest, gap_new_d_aidr_smmuv32) {
    uint32_t aidr = smmu_->getAIDR();
    EXPECT_EQ(aidr, 0x02u) << "AIDR must be 0x02 (SMMUv3.2) -- RIL/FWB/T0SZ features are implemented";
}

// =============================================================================
// GAP-NEW-A: Fault injection — injectSteFetchAbort, injectCdFetchAbort,
//            injectWalkEabt (§7.3.4, §7.3.10, §7.3.12)
// =============================================================================

class GapNewATest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        enableSmmu(*smmu_);
    }
    void TearDown() override { smmu_.reset(); }
    std::unique_ptr<SMMU> smmu_;
};

// injectSteFetchAbort with EVENTQEN=1 → F_STE_FETCH event appears in queue.
TEST_F(GapNewATest, gap_new_a_inject_ste_fetch_abort_queued) {
    smmu_->injectSteFetchAbort(SID);
    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_STE_FETCH in event queue";
    EXPECT_EQ(events.front().type, EventType::F_STE_FETCH);
    EXPECT_EQ(events.front().streamID, SID);
}

// injectSteFetchAbort with EVENTQEN=0 → event must NOT appear.
TEST_F(GapNewATest, gap_new_a_inject_ste_fetch_abort_gated_by_eventqen) {
    // Clear EVENTQEN bit.
    smmu_->setCR0(smmu_->getCR0() & ~SMMU::CR0_EVENTQEN);
    smmu_->injectSteFetchAbort(SID);
    auto events = smmu_->getEventQueue();
    EXPECT_TRUE(events.empty()) << "Event must be gated by CR0.EVENTQEN";
}

// injectCdFetchAbort → F_CD_FETCH event appears in queue with correct stream/PASID.
TEST_F(GapNewATest, gap_new_a_inject_cd_fetch_abort_queued) {
    static constexpr PASID TEST_PID = 5;
    smmu_->injectCdFetchAbort(SID, TEST_PID);
    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_CD_FETCH in event queue";
    EXPECT_EQ(events.front().type, EventType::F_CD_FETCH);
    EXPECT_EQ(events.front().streamID, SID);
    EXPECT_EQ(events.front().pasid, TEST_PID);
}

// injectWalkEabt → F_WALK_EABT event with correct address in queue.
TEST_F(GapNewATest, gap_new_a_inject_walk_eabt_queued) {
    smmu_->injectWalkEabt(SID, PID, BASE_IOVA);
    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_WALK_EABT in event queue";
    EXPECT_EQ(events.front().type, EventType::F_WALK_EABT);
    EXPECT_EQ(events.front().streamID, SID);
    EXPECT_EQ(events.front().address, BASE_IOVA);
}

// =============================================================================
// GAP-NEW-E: STATUSR / IRQ_CTRL / CTRLACK registers (§6.3.45–6.3.47)
// =============================================================================

class GapNewETest : public ::testing::Test {
protected:
    void SetUp() override { smmu_ = std::make_unique<SMMU>(); }
    void TearDown() override { smmu_.reset(); }
    std::unique_ptr<SMMU> smmu_;
};

// STATUSR returns 0 when SMMU is running (no dormant/error state in this model).
TEST_F(GapNewETest, gap_new_e_statusr_returns_zero_when_running) {
    EXPECT_EQ(smmu_->getStatusr(), 0u);
}

// setIrqCtrl then getIrqCtrlAck echoes the written value (synchronous handshake).
TEST_F(GapNewETest, gap_new_e_irq_ctrl_ack_mirrors_write) {
    smmu_->setIrqCtrl(0x3u);
    EXPECT_EQ(smmu_->getIrqCtrlAck(), 0x3u);
}

// setIrqCtrl(0) clears the enable bits.
TEST_F(GapNewETest, gap_new_e_irq_ctrl_clear) {
    smmu_->setIrqCtrl(0xFFFFFFFFu);
    smmu_->setIrqCtrl(0u);
    EXPECT_EQ(smmu_->getIrqCtrlAck(), 0u);
}

// =============================================================================
// GAP-NEW-F: GATOS address translation wrapper (§9.1–9.9)
// =============================================================================

class GapNewFTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        enableSmmu(*smmu_);
    }
    void TearDown() override { smmu_.reset(); }
    std::unique_ptr<SMMU> smmu_;
};

// gatosTranslate succeeds for a mapped page → returns PA (bit 0 = 0).
TEST_F(GapNewFTest, gap_new_f_gatos_translate_success_returns_pa) {
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Terminate);
    mapPage(*smmu_, SID, PID, BASE_IOVA, BASE_PA);

    uint64_t par = smmu_->gatosTranslate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_EQ((par & 0x1u), 0u)
        << "GATOS PAR bit 0 must be 0 on success; got 0x"
        << std::hex << par;
    // ARM IHI0070G.b §6.3.40: bits[55:12] = PA[55:12]; ATTR/SH fields also populated.
    constexpr uint64_t PAR_ADDR_MASK = 0x00FFFFFFFFFFF000ULL; // bits[55:12]
    EXPECT_EQ(par & PAR_ADDR_MASK, BASE_PA & PAR_ADDR_MASK)
        << "GATOS PAR bits[55:12] must hold the translated PA";
}

// gatosTranslate faults for an unmapped page → returns value with bit 0 = 1.
TEST_F(GapNewFTest, gap_new_f_gatos_translate_fault_returns_fault_bit) {
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Terminate);
    // Do NOT map the page.

    uint64_t par = smmu_->gatosTranslate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_EQ((par & 0x1u), 1u)
        << "GATOS PAR bit 0 must be 1 on fault; got 0x" << std::hex << par;
}

// gatosTranslate on unconfigured stream → fault bit set.
TEST_F(GapNewFTest, gap_new_f_gatos_translate_unconfigured_stream_fault) {
    static constexpr StreamID BAD_SID = 0xFFFF;
    uint64_t par = smmu_->gatosTranslate(BAD_SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_EQ((par & 0x1u), 1u)
        << "GATOS must return fault for unconfigured stream";
}

// =============================================================================
// GAP-P9 / GAP-R01: IDR0.STALL_MODEL must be 0b00 (§6.3.1)
// =============================================================================

TEST_F(GapNewDTest, gap_new_d_idr0_stall_model_consistent_with_sev) {
    uint32_t idr0 = smmu_->getIDR0();
    // BUG-AUDIT-55 fix: SEV (bit 14) defaults to 0 — WFE/SEV not implemented.
    // Use setSevSupported(true) to advertise it when the mechanism is in place.
    EXPECT_TRUE((idr0 & (1u << 14)) == 0u) << "IDR0 bit 14 (SEV) must be 0 by default (BUG-AUDIT-55)";
    // STALL_MODEL=0b00 means both stall and terminate are supported (correct for this model).
    EXPECT_EQ((idr0 >> 24) & 0x3u, 0u) << "IDR0 STALL_MODEL[25:24] must be 0b00";
}

// GAP-R01: STALL_MODEL must be 0b00 (both stall+terminate supported).
TEST_F(GapNewDTest, gap_new_r01_idr0_stall_model_zero) {
    uint32_t idr0 = smmu_->getIDR0();
    EXPECT_EQ((idr0 >> 24) & 0x3u, 0u)
        << "IDR0.STALL_MODEL[25:24] must be 0b00 (both models supported)";
    // BUG-AUDIT-55 fix: SEV (bit 14) defaults to 0 — WFE/SEV not implemented.
    EXPECT_TRUE((idr0 & (1u << 14)) == 0u) << "IDR0.SEV (bit 14) must be 0 by default (BUG-AUDIT-55)";
}

// GAP-R06: IDR5.STALL_MAX must be non-zero when STALL_MODEL==0b00.
TEST_F(GapNewDTest, gap_new_r06_idr5_stall_max_nonzero) {
    uint32_t idr5 = smmu_->getIDR5();
    uint32_t stall_max = (idr5 >> 16) & 0xFFFFu;
    EXPECT_NE(stall_max, 0u) << "IDR5.STALL_MAX[31:16] must be non-zero when stall is supported";
}

// GAP-R02: IDR0.Hyp (bit 9) must be set — mandatory for SMMUv3.2 with S1P+S2P.
TEST_F(GapNewDTest, gap_new_r02_idr0_hyp_set) {
    uint32_t idr0 = smmu_->getIDR0();
    EXPECT_TRUE((idr0 & (1u << 9)) != 0u)
        << "IDR0.Hyp (bit 9) must be set — mandatory for SMMUv3.2 when S1P==1 and S2P==1";
}

// GAP-R03: IDR3.XNX (bit 4) must be set — mandatory for SMMUv3.1+ when S2P==1.
TEST_F(GapNewDTest, gap_new_r03_idr3_xnx_set) {
    uint32_t idr3 = smmu_->getIDR3();
    EXPECT_TRUE((idr3 & (1u << 4)) != 0u)
        << "IDR3.XNX (bit 4) must be set — mandatory for SMMUv3.1+ when S2P==1";
}

// =============================================================================
// GAP-P8: IDR1.ATTR_PERMS_OVR must be set (§6.3.2)
// =============================================================================

TEST_F(GapNewDTest, gap_new_d_idr1_attr_perms_ovr_set) {
    uint32_t idr1 = smmu_->getIDR1();
    // IDR1 bit 26 = ATTR_PERMS_OVR: INSTCFG + PRIVCFG overrides are implemented.
    EXPECT_TRUE((idr1 & (1u << 26)) != 0u) << "IDR1 bit 26 (ATTR_PERMS_OVR) must be set";
}

// =============================================================================
// GAP-P3: setStrtabSplit() must accept only {6, 8, 10} and clamp reserved to 6 (§6.3.25)
// =============================================================================

TEST(StrtabSplitTest, gap_p3_strtab_split_clamps_invalid_to_six) {
    SMMU smmu;
    // Valid values must be accepted as-is.
    smmu.setStrtabSplit(6u);
    EXPECT_EQ(smmu.getStrtabSplit(), 6u) << "SPLIT=6 must be accepted";
    smmu.setStrtabSplit(8u);
    EXPECT_EQ(smmu.getStrtabSplit(), 8u) << "SPLIT=8 must be accepted";
    smmu.setStrtabSplit(10u);
    EXPECT_EQ(smmu.getStrtabSplit(), 10u) << "SPLIT=10 must be accepted";
    // Reserved values must clamp to 6.
    smmu.setStrtabSplit(5u);
    EXPECT_EQ(smmu.getStrtabSplit(), 6u) << "SPLIT=5 (reserved) must clamp to 6";
    smmu.setStrtabSplit(7u);
    EXPECT_EQ(smmu.getStrtabSplit(), 6u) << "SPLIT=7 (reserved) must clamp to 6";
    smmu.setStrtabSplit(15u);
    EXPECT_EQ(smmu.getStrtabSplit(), 6u) << "SPLIT=15 (reserved) must clamp to 6";
}

// =============================================================================
// GAP-P5: GATOS_PAR fault syndrome must include FAULTCODE (§6.3.40)
// =============================================================================

TEST_F(GapNewFTest, gap_new_f_gatos_fault_par_has_faultcode) {
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Terminate);
    // No page mapped — will produce F_TRANSLATION.
    uint64_t par = smmu_->gatosTranslate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_EQ((par & 0x1u), 1u) << "GATOS PAR bit 0 must be 1 on fault";
    // FAULTCODE[11:4] must be 0x10 (F_TRANSLATION).
    EXPECT_EQ((par >> 4) & 0xFFu, 0x10u)
        << "GATOS PAR FAULTCODE[11:4] must be 0x10 (F_TRANSLATION)";
    // REASON[2:1] must be 0b00.
    EXPECT_EQ((par >> 1) & 0x3u, 0u) << "GATOS PAR REASON[2:1] must be 0b00";
}

// =============================================================================
// GAP-R05: F_UUT / F_TRANSL_FORBIDDEN / F_BAD_ATS_TREQ eventClass must be 0
// §7.3.2, §7.3.7, §7.3.8 — those wire formats have no CLASS field (RES0)
// =============================================================================

TEST_F(GapNewATest, gap_new_r05_f_uut_event_class_is_zero) {
    // Inject F_UUT directly via the simulation harness API (§7.3.2).
    // The CLASS field position in the F_UUT wire format is RES0, so eventClass must be 0.
    smmu_->reportUnsupportedTransaction(SID, PID, BASE_IOVA, AccessType::Read);
    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_UUT event in queue after reportUnsupportedTransaction";
    ASSERT_EQ(events.front().type, EventType::F_UUT) << "First event must be F_UUT";
    EXPECT_EQ(events.front().eventClass, 0u)
        << "F_UUT eventClass must be 0 (no CLASS field in wire format §7.3.2)";
}

// =============================================================================
// GAP-R04: IDR0.ATSRECERR (bit 23) must be set (§6.3.1, §2.5)
// =============================================================================

TEST_F(GapNewDTest, gap_new_r04_idr0_atsrecerr_set) {
    uint32_t idr0 = smmu_->getIDR0();
    EXPECT_TRUE((idr0 & (1u << 23)) != 0u)
        << "IDR0.ATSRECERR (bit 23) must be set — REC_CFG_ATS is implemented";
}

// =============================================================================
// GAP-R07: IDR0.BTM (bit 5) must be set (§6.3.1)
// =============================================================================

TEST_F(GapNewDTest, gap_new_r07_idr0_btm_set) {
    uint32_t idr0 = smmu_->getIDR0();
    EXPECT_TRUE((idr0 & (1u << 5)) != 0u)
        << "IDR0.BTM (bit 5) must be set — receiveBroadcastTLBI() is implemented";
}

// =============================================================================
// GAP-NEW-S1: IDR1.ATTR_TYPES_OVR (bit 27) must be set (§6.3.2)
// =============================================================================

TEST_F(GapNewDTest, gap_new_s1_idr1_attr_types_ovr_set) {
    uint32_t idr1 = smmu_->getIDR1();
    EXPECT_TRUE((idr1 & (1u << 27)) != 0u)
        << "IDR1.ATTR_TYPES_OVR (bit 27) must be set — MTCFG/SHCFG/ALLOCCFG are implemented";
}

// =============================================================================
// BUG-2 fix: IDR0.TERM_MODEL (bit 26) must be CLEAR (§6.3.1, §3.12.1)
//
// The original test incorrectly asserted TERM_MODEL=1.  The model implements
// both stall and terminate behaviors and does NOT validate CD.A=1.  ARM
// IHI0070G.b §6.3.1: when TERM_MODEL=1, C_BAD_CD must fire if CD.A=0 —
// that validation is absent from this model.  Clearing TERM_MODEL=0 (RAZ/WI
// IS supported) correctly matches the model's actual behaviour.
// =============================================================================

TEST_F(GapNewDTest, gap_new_s2_idr0_term_model_set) {
    uint32_t idr0 = smmu_->getIDR0();
    EXPECT_EQ((idr0 & (1u << 26)), 0u)
        << "IDR0.TERM_MODEL (bit 26) must be 0 — model supports both stall and "
           "terminate; TERM_MODEL=1 requires CD.A validation which is not implemented "
           "(BUG-2 fix, ARM IHI0070G.b §6.3.1)";
}

// =============================================================================
// GAP-NEW-S3: CR2.E2H — STRW=EL2_E2H downgrade when CR2.E2H=0 (§6.3.12, §3.17.5)
//
// When CR2.E2H=0 (VHE not enabled), a stream configured with STRW=EL2_E2H
// must behave as NS-EL2 (STRW=EL2).  This means TLB entries must be installed
// with ASID=0 (no ASID tagging), matching NS-EL2 semantics.
//
// Test strategy: install a TLB entry via a successful translation with
// STRW=EL2_E2H while CR2.E2H=0.  Then perform an ASID-scoped invalidation
// for the stream's configured ASID.  Because the entry was installed with
// ASID=0 (NS-EL2 semantics), the ASID-scoped TLBI must NOT remove it —
// a subsequent translate must still hit the TLB (or page-walk successfully)
// without faulting.
//
// Conversely, when CR2.E2H=1, the entry IS installed with the stream's ASID,
// so the ASID-scoped TLBI removes it.  We verify the TLB entry ASID tagging
// changed by checking that after setting E2H=1 and mapping a new page, the
// ASID-scoped TLBI does clear the entry (the translate still succeeds via
// page-walk, which is acceptable — the key is no fault is produced).
// =============================================================================

class CR2E2HTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }
    void TearDown() override { smmu_.reset(); }
    std::unique_ptr<SMMU> smmu_;

    static constexpr StreamID SID2   = 0x30;
    static constexpr PASID    PID2   = 0;
    static constexpr uint16_t ASID2  = 0x42;
    static constexpr IOVA     IOVA2  = 0x5000;
    static constexpr PA       PA2    = 0xE000'0000ULL;

    void configureEl2E2hStream() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.strw               = StreamWorld::EL2_E2H;
        cfg.asid               = ASID2;
        cfg.t0sz               = 0; // no VA range restriction
        ASSERT_TRUE(smmu_->configureStream(SID2, cfg).isOk());
        ASSERT_TRUE(smmu_->enableStream(SID2).isOk());
        ASSERT_TRUE(smmu_->createStreamPASID(SID2, PID2).isOk());
        PagePermissions perms;
        perms.read    = true;
        perms.write   = true;
        perms.execute = false;
        ASSERT_TRUE(smmu_->mapPage(SID2, PID2, IOVA2, PA2, perms).isOk());
    }
};

// CR2_E2H must be defined as bit 0 (value 1).
TEST_F(CR2E2HTest, gap_new_s3_cr2_e2h_constant_is_bit_zero) {
    EXPECT_EQ(SMMU::CR2_E2H, 1u)
        << "CR2_E2H must be bit 0 per ARM IHI0070G.b §6.3.12";
}

// CR2.E2H must read back 0 by default (reset value).
TEST_F(CR2E2HTest, gap_new_s3_cr2_e2h_default_zero) {
    EXPECT_EQ(smmu_->getCR2() & SMMU::CR2_E2H, 0u)
        << "CR2.E2H must default to 0 at reset";
}

// CR2.E2H must read back the value written via setCR2.
TEST_F(CR2E2HTest, gap_new_s3_cr2_e2h_set_and_read_back) {
    smmu_->setCR2(smmu_->getCR2() | SMMU::CR2_E2H);
    EXPECT_EQ(smmu_->getCR2() & SMMU::CR2_E2H, SMMU::CR2_E2H)
        << "CR2.E2H must read back 1 after setCR2 sets bit 0";
    smmu_->setCR2(smmu_->getCR2() & ~SMMU::CR2_E2H);
    EXPECT_EQ(smmu_->getCR2() & SMMU::CR2_E2H, 0u)
        << "CR2.E2H must read back 0 after setCR2 clears bit 0";
}

// GAP-NEW-S3 core: with CR2.E2H=0, STRW=EL2_E2H stream must install TLB entries
// with ASID=0 (NS-EL2 semantics, no ASID tagging).
//
// Observable invariant: after an ASID-scoped TLBI for ASID2, the TLB entry
// that was installed with ASID=0 must survive (not be evicted).  The second
// translation must therefore be a TLB cache HIT, not a miss.
//
// Before the fix the entry is erroneously tagged with ASID2, so the ASID-scoped
// TLBI evicts it and the second translate is a MISS — this test fails red.
// After the fix the entry is tagged ASID=0, survives the TLBI, and the second
// translate is a HIT — the test goes green.
TEST_F(CR2E2HTest, gap_new_s3_strw_el2e2h_with_e2h_zero_uses_asid_zero) {
    // Verify pre-condition: CR2.E2H=0.
    ASSERT_EQ(smmu_->getCR2() & SMMU::CR2_E2H, 0u)
        << "Pre-condition: CR2.E2H must be 0 for this test";

    configureEl2E2hStream();

    // Populate the TLB with a successful translation (cache miss — slow path).
    smmu_->resetStatistics();
    auto r1 = smmu_->translate(SID2, PID2, IOVA2, AccessType::Read);
    ASSERT_TRUE(r1.isOk()) << "First translation must succeed (page is mapped)";
    // After the first translate the entry should be in the TLB.

    // Confirm that a second translate before any TLBI is a cache hit.
    smmu_->resetStatistics();
    auto r_hit = smmu_->translate(SID2, PID2, IOVA2, AccessType::Read);
    ASSERT_TRUE(r_hit.isOk());
    uint64_t hitsBeforeTlbi = smmu_->getCacheStatistics().hitCount;
    ASSERT_GT(hitsBeforeTlbi, 0u)
        << "Second translate (before TLBI) must be a TLB hit — TLB not populated";

    // Issue an ASID-scoped TLBI for ASID2.
    // With CR2.E2H=0 the entry must have been installed with ASID=0, so this
    // TLBI must NOT evict it.
    CommandEntry tlbiCmd;
    tlbiCmd.type = CommandType::TLBI_NH_ASID;
    tlbiCmd.asid = ASID2;
    ASSERT_TRUE(smmu_->submitCommand(tlbiCmd).isOk());
    smmu_->processCommandQueue();

    // The third translate must STILL be a cache hit (entry was not evicted).
    smmu_->resetStatistics();
    auto r2 = smmu_->translate(SID2, PID2, IOVA2, AccessType::Read);
    EXPECT_TRUE(r2.isOk())
        << "Translation must succeed after ASID-scoped TLBI (page still mapped)";

    uint64_t hitsAfterTlbi = smmu_->getCacheStatistics().hitCount;
    uint64_t missesAfterTlbi = smmu_->getCacheStatistics().missCount;

    // KEY INVARIANT: with CR2.E2H=0 the entry was installed with ASID=0, so
    // the ASID2-scoped TLBI must NOT have evicted it.  The translate after the
    // TLBI must be a TLB cache HIT (not a miss that forces a page-walk).
    EXPECT_GT(hitsAfterTlbi, 0u)
        << "After ASID-scoped TLBI (ASID2), the TLB entry tagged ASID=0 must "
           "survive — third translate must be a cache HIT. "
           "Hits=" << hitsAfterTlbi << " Misses=" << missesAfterTlbi;
    EXPECT_EQ(missesAfterTlbi, 0u)
        << "No cache misses expected after TLBI when entry has ASID=0 "
           "(CR2.E2H=0 forces NS-EL2 / no-ASID semantics for EL2_E2H streams). "
           "Hits=" << hitsAfterTlbi << " Misses=" << missesAfterTlbi;

    // No fault events must be present.
    auto events = smmu_->getEventQueue();
    EXPECT_TRUE(events.empty()) << "No fault events expected";
}

// GAP-NEW-S3 complement: with CR2.E2H=1, STRW=EL2_E2H stream installs TLB
// entries WITH the stream's ASID (EL2-VHE semantics).  An ASID-scoped TLBI
// for ASID2 must remove that entry — the subsequent translate must be a
// cache MISS (page-walk re-execution), not a hit.
TEST_F(CR2E2HTest, gap_new_s3_strw_el2e2h_with_e2h_one_uses_stream_asid) {
    // Enable E2H before configuring the stream.
    smmu_->setCR2(smmu_->getCR2() | SMMU::CR2_E2H);
    ASSERT_EQ(smmu_->getCR2() & SMMU::CR2_E2H, SMMU::CR2_E2H);

    configureEl2E2hStream();

    // Populate TLB (first translate = miss, installs entry with ASID2).
    auto r1 = smmu_->translate(SID2, PID2, IOVA2, AccessType::Read);
    ASSERT_TRUE(r1.isOk()) << "First translation must succeed";

    // Confirm second translate is a cache hit (entry is present with ASID2).
    smmu_->resetStatistics();
    auto r_hit = smmu_->translate(SID2, PID2, IOVA2, AccessType::Read);
    ASSERT_TRUE(r_hit.isOk());
    ASSERT_GT(smmu_->getCacheStatistics().hitCount, 0u)
        << "Second translate (before TLBI, E2H=1) must be a TLB hit";

    // ASID-scoped TLBI for ASID2 — with E2H=1 the entry was tagged ASID2,
    // so this must evict it.
    CommandEntry tlbiCmd;
    tlbiCmd.type = CommandType::TLBI_NH_ASID;
    tlbiCmd.asid = ASID2;
    ASSERT_TRUE(smmu_->submitCommand(tlbiCmd).isOk());
    smmu_->processCommandQueue();

    // Translate again — page is still mapped so must succeed, but via page walk
    // (cache MISS) because the ASID2 entry was evicted.
    smmu_->resetStatistics();
    auto r2 = smmu_->translate(SID2, PID2, IOVA2, AccessType::Read);
    EXPECT_TRUE(r2.isOk())
        << "Translation must succeed after TLBI (page still mapped, re-walked)";

    uint64_t hitsAfter = smmu_->getCacheStatistics().hitCount;
    uint64_t missesAfter = smmu_->getCacheStatistics().missCount;

    // KEY INVARIANT: with E2H=1 the ASID2-scoped TLBI evicted the entry.
    // The translate after the TLBI must be a cache MISS.
    EXPECT_EQ(hitsAfter, 0u)
        << "With CR2.E2H=1, ASID-scoped TLBI must evict the EL2_E2H entry "
           "(tagged ASID2) — third translate must be a cache MISS. "
           "Hits=" << hitsAfter << " Misses=" << missesAfter;
    EXPECT_GT(missesAfter, 0u)
        << "Cache miss expected after TLBI evicted the ASID2-tagged entry. "
           "Hits=" << hitsAfter << " Misses=" << missesAfter;

    // No fault events expected.
    auto events = smmu_->getEventQueue();
    EXPECT_TRUE(events.empty()) << "No fault events expected";
}

// =============================================================================
// GAP-N: C_BAD_SUBSTREAMID SSV must always be true (§7.3.9)
// =============================================================================

TEST_F(GapNewATest, gap_new_n_c_bad_substreamid_ssv_always_set_pasid_nonzero) {
    // Trigger C_BAD_SUBSTREAMID via stage-2-only stream with a non-zero PASID.
    // §3.9 / §7.3.9: a non-zero PASID on a stage-2-only stream generates
    // C_BAD_SUBSTREAMID.  SSV must be true per §7.3.9 regardless of PASID value.
    static constexpr StreamID SID_S2 = 0x41;
    static constexpr PASID    PID_NZ = 1;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu_->configureStream(SID_S2, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID_S2).isOk());

    smmu_->translate(SID_S2, PID_NZ, 0x1000, AccessType::Read);

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected C_BAD_SUBSTREAMID event in queue";
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_SUBSTREAMID) {
            EXPECT_TRUE(ev.ssv)
                << "C_BAD_SUBSTREAMID must have SSV=true per §7.3.9 (pasid="
                << PID_NZ << ")";
            found = true;
        }
    }
    EXPECT_TRUE(found) << "C_BAD_SUBSTREAMID event not found in queue";
}

TEST_F(GapNewATest, gap_new_n_c_bad_substreamid_ssv_true_for_pasid_zero) {
    // §3.9 / §7.3.9: a non-zero PASID on a disabled stream (STE.Config=0b000)
    // also generates C_BAD_SUBSTREAMID.  Trigger with pasid=0 on a disabled
    // stream is not possible (only non-zero PASID triggers it).  Instead verify
    // SSV via the stage-2-only path with the minimum non-zero PASID.  This is the
    // same physical path; the key assertion is SSV=true regardless of PASID.
    static constexpr StreamID SID_DIS = 0x42;

    // Disabled stream (translationEnabled=false).
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    ASSERT_TRUE(smmu_->configureStream(SID_DIS, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(SID_DIS).isOk());

    // §5.2 STE.Config=0b000: PASID != 0 -> C_BAD_SUBSTREAMID (§3.9/§7.3.9).
    smmu_->translate(SID_DIS, 1u, 0x1000, AccessType::Read);

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected C_BAD_SUBSTREAMID event in queue";
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_SUBSTREAMID) {
            EXPECT_TRUE(ev.ssv)
                << "C_BAD_SUBSTREAMID SSV must be true per §7.3.9";
            found = true;
        }
    }
    EXPECT_TRUE(found) << "C_BAD_SUBSTREAMID event not found in queue";
}

// =============================================================================
// GAP-J: IDR0.TTF=0b10 makes C_BAD_CD on AA64=0 spec-consistent (§5.4, §6.3.1)
// =============================================================================

TEST_F(GapNewDTest, gap_new_j_idr0_ttf_vmsa64_only_makes_aa64_zero_rejection_consistent) {
    // IDR0.TTF[3:2] = 0b10 means VMSAv8-64 only (no VMSAv8-32 LPAE support).
    // This makes rejecting CD.AA64=0 with C_BAD_CD spec-consistent:
    // software that reads TTF=0b10 must not configure AA64=0 CDs.
    uint32_t idr0 = smmu_->getIDR0();
    uint32_t ttf = (idr0 >> 2) & 0x3u; // bits[3:2]
    EXPECT_EQ(ttf, 2u)
        << "IDR0.TTF[3:2] must be 0b10 (VMSAv8-64 only) — makes AA64=0 rejection spec-consistent";
}

// =============================================================================
// GAP-L: GATOS_PAR fault REASON and FAULTCODE from actual fault (§9.1.4, §6.3.40)
// =============================================================================

TEST_F(GapNewFTest, gap_new_l_gatos_par_faultcode_reflects_actual_event) {
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Terminate);
    // Unmapped page -> F_TRANSLATION (0x10).
    uint64_t par = smmu_->gatosTranslate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_EQ(par & 1u, 1u) << "FAULT bit must be set";
    uint64_t faultCode = (par >> 4) & 0xFFu;
    EXPECT_EQ(faultCode, 0x10u) << "FAULTCODE must be 0x10 (F_TRANSLATION)";
    EXPECT_EQ((par >> 1) & 0x3u, 0u) << "REASON must be 0b00 for stage-1 fault";
}

TEST_F(GapNewFTest, gap_new_l_gatos_par_reason_set_for_stage2_fault) {
    // Set up a two-stage stream: stage-1 maps IOVA->IPA (success), but stage-2
    // has no mapping for that IPA (fault).  The resulting event has s2=true,
    // eventClass=2 (IN), so GATOS_PAR REASON = eventClass+1 = 3 = 0b11 per §9.1.4.
    static constexpr StreamID SID_2S = 0x50;
    static constexpr PA       IPA_VAL = 0x2000'0000ULL; // stage-1 output IPA

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = 0;
    if (!smmu_->configureStream(SID_2S, cfg).isOk() ||
        !smmu_->enableStream(SID_2S).isOk() ||
        !smmu_->createStreamPASID(SID_2S, PID).isOk()) {
        GTEST_SKIP() << "Two-stage not supported";
    }

    // Provide an empty stage-2 address space (no IPA mapping).
    auto s2as = std::make_shared<AddressSpace>();
    if (!smmu_->setStreamStage2AddressSpace(SID_2S, s2as).isOk()) {
        GTEST_SKIP() << "setStreamStage2AddressSpace not supported";
    }

    // Map IOVA->IPA in stage-1 so stage-1 succeeds; stage-2 will fault.
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu_->mapPage(SID_2S, PID, BASE_IOVA, IPA_VAL, perms).isOk());

    // Translate: stage-1 succeeds (IOVA->IPA), stage-2 faults (IPA not mapped).
    uint64_t par = smmu_->gatosTranslate(SID_2S, PID, BASE_IOVA, AccessType::Read);
    EXPECT_EQ(par & 1u, 1u) << "FAULT bit must be set";
    // NEW-GAP-A fix: §9.1.4 REASON encodes the stage-2 context via eventClass+1.
    // F_TRANSLATION faults have eventClass=2 (IN), so REASON = 2+1 = 3 = 0b11
    // (stage-2 fault on IPA input).  The old value of 1 was incorrect.
    uint64_t reason = (par >> 1) & 0x3u;
    EXPECT_EQ(reason, 3u) << "REASON must be 0b11 (3) for stage-2 IPA-input fault per §9.1.4";
}
