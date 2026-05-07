// ARM SMMU v3 — Conformance Gap C and Gap D fixes
// Copyright (c) 2024 John Greninger
//
// TDD spec tests for two conformance gaps:
//
//   GAP C  §3.4.1 / §5.4: CD.T0SZ VA range enforcement
//     - IOVA at or above 2^(64-T0SZ) must produce F_TRANSLATION (not F_ADDR_SIZE).
//     - IOVAs within the T0SZ-defined range must translate normally.
//     - T0SZ==0 (full 64-bit range) imposes no restriction.
//
//   GAP D  §3.2 / §13.5: STE.INSTCFG access-type override before permission checks
//     - INSTCFG=1 (Force Instruction): Read→Execute, ReadPrivileged→ExecutePrivileged.
//     - INSTCFG=2 (Force Data):        Execute→Read, ExecutePrivileged→ReadPrivileged.
//     - Override is applied before the page-table permission check.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID       = 0x10;
static constexpr PASID    PID       = 0;
static constexpr IOVA     BASE_IOVA = 0x1000;
static constexpr PA       BASE_PA   = 0x9000;

// Set up a stage-1-only stream with t0sz applied at the STE/CD layer through
// StreamConfig.  The stream is fully enabled and a PASID-0 context is created.
void setupStage1StreamT0SZ(SMMU& smmu, uint8_t t0sz, StreamID sid = SID, PASID pasid = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.t0sz               = t0sz;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "configureStream must succeed for t0sz=" << static_cast<unsigned>(t0sz);
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Set up a stage-1-only stream with instCfg applied through StreamConfig.
void setupStage1StreamInstCFG(SMMU& smmu, uint8_t instCfg, StreamID sid = SID, PASID pasid = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.instCfg            = instCfg;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "configureStream must succeed for instCfg=" << static_cast<unsigned>(instCfg);
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

} // anonymous namespace

// ═════════════════════════════════════════════════════════════════════════════
// GAP C: CD.T0SZ VA range enforcement (ARM IHI0070G.b §3.4.1 / §5.4)
// ═════════════════════════════════════════════════════════════════════════════

class GapCT0SZTests : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// T0SZ=16 → 48-bit VA space → limit = 2^48 = 0x0001_0000_0000_0000.
// IOVA == limit (2^48) is out of range → must generate F_TRANSLATION.
TEST_F(GapCT0SZTests, GapC_T0SZ16_IOVAAtLimit_FTranslation) {
    setupStage1StreamT0SZ(*smmu_, 16u);

    // Map a page well within range so a within-range translate would succeed.
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // IOVA exactly at the limit (2^48): out of range.
    static constexpr IOVA OVER_IOVA = UINT64_C(1) << 48u;
    auto r = smmu_->translate(SID, PID, OVER_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "GAP C: IOVA at T0SZ limit must produce an error";

    auto ev = smmu_->getEventQueue();
    bool has_f_translation = false;
    bool has_f_addr_size   = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) { has_f_translation = true; }
        if (e.type == EventType::F_ADDR_SIZE)   { has_f_addr_size   = true; }
    }
    EXPECT_TRUE(has_f_translation)
        << "GAP C: §3.4.1 — T0SZ VA range violation must generate F_TRANSLATION";
    EXPECT_FALSE(has_f_addr_size)
        << "GAP C: T0SZ VA range violation must NOT generate F_ADDR_SIZE "
           "(that is for OAS violations, not VA-range violations)";
}

// T0SZ=16 → IOVA = 0x0000_FFFF_FFFF_F000 (just below 2^48) is within range → success.
TEST_F(GapCT0SZTests, GapC_T0SZ16_IOVAWithinRange_Succeeds) {
    setupStage1StreamT0SZ(*smmu_, 16u);

    // Map IOVA in the canonical positive half: below 2^47 (sign bit 0, bits[63:47] all 0).
    // (UINT64_C(1)<<48)-0x1000 = 0xFFFF_FFFF_F000 is non-canonical per ARM §3.4.1
    // (bit[47]=1 with bits[63:48]=0); use 0x7FFF_FFFF_F000 instead (bit[47]=0).
    static constexpr IOVA WITHIN_IOVA = (UINT64_C(1) << 47u) - 0x1000u; // last canonical page
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, WITHIN_IOVA, BASE_PA, perms).isOk());

    auto r = smmu_->translate(SID, PID, WITHIN_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "GAP C: IOVA within T0SZ range must translate successfully";
}

// T0SZ=0 → no upper-limit restriction on IOVA.  A large IOVA must still be able
// to translate (provided the page is mapped), not be rejected by VA-range check.
TEST_F(GapCT0SZTests, GapC_T0SZ0_LargeIOVA_NoRangeRestriction) {
    setupStage1StreamT0SZ(*smmu_, 0u);

    // A large IOVA that would exceed a 48-bit T0SZ range but is within the
    // 52-bit MAX_VIRTUAL_ADDRESS accepted by AddressSpace::mapPage().
    // 2^48 = 0x0001_0000_0000_0000; pick a page well above that boundary.
    static constexpr IOVA LARGE_IOVA = (UINT64_C(1) << 48u) | 0x2000u;
    PagePermissions perms(true, false, false);
    ASSERT_TRUE(smmu_->mapPage(SID, PID, LARGE_IOVA, BASE_PA, perms).isOk());

    auto r = smmu_->translate(SID, PID, LARGE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "GAP C: T0SZ==0 imposes no VA-range restriction; large IOVA must succeed";

    // No F_TRANSLATION event should be queued.
    auto ev = smmu_->getEventQueue();
    for (const auto& e : ev) {
        EXPECT_NE(e.type, EventType::F_TRANSLATION)
            << "GAP C: T0SZ==0 — no F_TRANSLATION event must be generated for in-range IOVA";
    }
}

// Only a single F_TRANSLATION event must be generated, not two.
TEST_F(GapCT0SZTests, GapC_T0SZ16_NoDuplicateEvent) {
    setupStage1StreamT0SZ(*smmu_, 16u);

    static constexpr IOVA OVER_IOVA = UINT64_C(1) << 48u;
    smmu_->translate(SID, PID, OVER_IOVA, AccessType::Read);

    auto ev = smmu_->getEventQueue();
    unsigned f_translation_count = 0u;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            ++f_translation_count;
        }
    }
    EXPECT_EQ(f_translation_count, 1u)
        << "GAP C: exactly one F_TRANSLATION event must be emitted per T0SZ-range violation "
           "(no duplicate from the outer error handler)";
}

// ═════════════════════════════════════════════════════════════════════════════
// GAP D: STE.INSTCFG access-type override (ARM IHI0070G.b §3.2 / §13.5)
// ═════════════════════════════════════════════════════════════════════════════

class GapDInstCFGTests : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// INSTCFG=3 (Force Instruction per §5.2 0b11), page mapped RX (execute allowed).
// Incoming access=Read → overridden to Execute → permission check passes.
TEST_F(GapDInstCFGTests, GapD_InstCFG1_ReadBecomesExecute_Succeeds) {
    // instCfg=3 (0b11): Force Instruction per ARM §5.2 bits[115:114]
    setupStage1StreamInstCFG(*smmu_, 3u);

    // Map page with read + execute permissions (no write).
    PagePermissions perms;
    perms.read    = true;
    perms.write   = false;
    perms.execute = true;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Translate with Read: INSTCFG forces Read→Execute, execute is permitted.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "GAP D: INSTCFG=1 (Force Inst) — Read overridden to Execute; "
           "execute permission is set, so translation must succeed";
}

// INSTCFG=3 (Force Instruction per §5.2 0b11), page mapped R (read allowed, execute denied).
// Incoming access=Read → overridden to Execute → permission check fails.
TEST_F(GapDInstCFGTests, GapD_InstCFG1_ReadBecomesExecute_Denied) {
    // instCfg=3 (0b11): Force Instruction per ARM §5.2 bits[115:114]
    setupStage1StreamInstCFG(*smmu_, 3u);

    // Map page read-only (execute denied).
    PagePermissions perms;
    perms.read    = true;
    perms.write   = false;
    perms.execute = false;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Translate with Read: INSTCFG forces Read→Execute, execute is denied.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "GAP D: INSTCFG=1 (Force Inst) — Read overridden to Execute; "
           "execute permission is NOT set, so translation must fail";
}

// INSTCFG=2 (Force Data), page mapped RW (read/write allowed, execute irrelevant).
// Incoming access=Execute → overridden to Read → permission check passes.
TEST_F(GapDInstCFGTests, GapD_InstCFG2_ExecuteBecomesRead_Succeeds) {
    // instCfg=2: Force Data
    setupStage1StreamInstCFG(*smmu_, 2u);

    // Map page read-write (execute not set, but that doesn't matter — Read is checked).
    PagePermissions perms;
    perms.read    = true;
    perms.write   = true;
    perms.execute = false;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Translate with Execute: INSTCFG forces Execute→Read, read is permitted.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(r.isOk())
        << "GAP D: INSTCFG=2 (Force Data) — Execute overridden to Read; "
           "read permission is set, so translation must succeed";
}

// INSTCFG=2 (Force Data), page mapped X-only (execute allowed, read denied).
// Incoming access=Execute → overridden to Read → permission check fails.
TEST_F(GapDInstCFGTests, GapD_InstCFG2_ExecuteBecomesRead_Denied) {
    // instCfg=2: Force Data
    setupStage1StreamInstCFG(*smmu_, 2u);

    // Map page execute-only (read denied).
    PagePermissions perms;
    perms.read    = false;
    perms.write   = false;
    perms.execute = true;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Translate with Execute: INSTCFG forces Execute→Read, read is denied.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(r.isError())
        << "GAP D: INSTCFG=2 (Force Data) — Execute overridden to Read; "
           "read permission is NOT set, so translation must fail";
}

// INSTCFG=0: no override — normal behavior.
// Page mapped read-only; Read succeeds and Execute fails as expected.
TEST_F(GapDInstCFGTests, GapD_InstCFG0_NoOverride_NormalBehavior) {
    // instCfg=0: no override (default)
    setupStage1StreamInstCFG(*smmu_, 0u);

    // Map page read-only.
    PagePermissions perms;
    perms.read    = true;
    perms.write   = false;
    perms.execute = false;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Read must succeed (no override, read is permitted).
    auto rRead = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(rRead.isOk())
        << "GAP D: INSTCFG=0 — Read on read-only page must succeed without override";

    // Execute must fail (no override, execute is not permitted).
    auto rExec = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(rExec.isError())
        << "GAP D: INSTCFG=0 — Execute on non-executable page must fail without override";
}
