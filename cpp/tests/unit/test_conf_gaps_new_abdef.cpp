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

// s1Stalld=true + FaultMode::Stall: S1STALLD overrides stall — must abort
// with an F_TRANSLATION event in the queue (not Stalled).
TEST_F(GapNewGTest, gap_new_g_s1stalld_forces_abort_not_stall) {
    setupStage1Stream(*smmu_, SID, PID, FaultMode::Stall, /*s1Stalld=*/true);

    // Translate an unmapped address — S1STALLD should force terminate.
    TranslationResult result = smmu_->translate(SID, PID, BASE_IOVA,
                                                AccessType::Read);
    EXPECT_TRUE(result.isError());
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "S1STALLD=1 must not produce Stalled result";

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
    EXPECT_TRUE((idr5 & (1u << 5)) != 0u)    << "IDR5 GRAN16K (bit 5) must be set";
    EXPECT_TRUE((idr5 & (1u << 6)) != 0u)    << "IDR5 GRAN64K (bit 6) must be set";
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
