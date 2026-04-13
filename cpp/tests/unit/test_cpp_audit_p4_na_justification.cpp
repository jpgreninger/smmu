// ARM SMMU v3 Priority 4 N/A justification and §13.1.7 Device→OSH conformance tests
//
// Priority 4 items and their resolution:
//   §3.3.1.2 (2-level stream table) — N/A: C++ uses single-level table; documented in TASKS_CPP_AUDIT.md
//   §3.3.1.3 (SID range walk)       — N/A: same as above
//   §3.5.5   (PRI queue model)      — Resolved by P2 §8 work (BUG-SEC8-SECURE-PRI-CPP)
//   §9.1     (GATOS)                — N/A: GATOS is an optional debug feature; no C++ GATOS registers
//   §13.1.2  (NS IPA clamping)      — Resolved by P1/P2 nsCfg path; verified in P3 tests
//   §13.1.5  (HTTU / AF update)     — Resolved by P3 BUG-HTTU-S2-CPP fix
//   §13.1.7  (Device/NC → OSH)      — BUG-13.1.7-CPP: REAL BUG — fixed here (this file)
//
// BUG-13.1.7-CPP: ARM §13.1.7 Rule 1 — Device and Non-Cacheable memory types must
//   always use Outer Shareable (OSH=2) regardless of STE.SHCfg.
//   The GBPA path (smmu.cpp:~311) already has the guard:
//     if (td.memType == 0x00u) td.shareability = 2u;
//   But applyOutputAttrs in stream_context.cpp and the TLB fast path in smmu.cpp
//   both set shareability unconditionally from STE.SHCfg without this guard.
//
//   Fix: in both locations check effective memory type:
//     mtCfg=true:  effectiveDevice = (memAttr == 0x00u)
//     mtCfg=false: effectiveDevice = (pageAttr == 0x00u)  [requires TLBEntry.pageAttr]
//     if (effectiveDevice) shareability = 2u;  // OSH

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>

namespace smmu {
namespace test {

// ── §13.1.7 Device→OSH enforcement ──────────────────────────────────────────

class P4DeviceOshTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
    }

    // Configure stream 0 with stage-1 only.  Maps one Normal page via smmu_->mapPage().
    // shCfg: STE.SHCfg value (e.g. 3=ISH); mtCfg/memAttr: STE memory override.
    void configureNormalStream(uint8_t shCfg, bool mtCfg = false, uint8_t memAttr = 0xFFu) {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.bypassEnabled      = false;
        cfg.mtCfg              = mtCfg;
        cfg.memAttr            = memAttr;
        cfg.shCfg              = shCfg;
        ASSERT_TRUE(smmu_->configureStream(kSid, cfg).isOk());
        ASSERT_TRUE(smmu_->enableStream(kSid).isOk());
        ASSERT_TRUE(smmu_->createStreamPASID(kSid, kPasid).isOk());

        PagePermissions rw;
        rw.read  = true;
        rw.write = true;
        ASSERT_TRUE(smmu_->mapPage(kSid, kPasid, kIova, kPA, rw, SecurityState::NonSecure).isOk());
    }

    // Configure stream 0 with stage-1 only.  Maps one Device page via smmu_->mapPageDevice().
    void configureDeviceStream(uint8_t shCfg, bool mtCfg = false, uint8_t memAttr = 0xFFu) {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.bypassEnabled      = false;
        cfg.mtCfg              = mtCfg;
        cfg.memAttr            = memAttr;
        cfg.shCfg              = shCfg;
        ASSERT_TRUE(smmu_->configureStream(kSid, cfg).isOk());
        ASSERT_TRUE(smmu_->enableStream(kSid).isOk());
        ASSERT_TRUE(smmu_->createStreamPASID(kSid, kPasid).isOk());

        PagePermissions rw;
        rw.read  = true;
        rw.write = true;
        ASSERT_TRUE(smmu_->mapPageDevice(kSid, kPasid, kIova, kPA, rw, SecurityState::NonSecure).isOk());
    }

    static constexpr StreamID kSid  = 0;
    static constexpr PASID    kPasid = 0;
    static constexpr IOVA     kIova = 0x10000;
    static constexpr PA       kPA   = 0xC0000000ULL;

    std::unique_ptr<SMMU> smmu_;
};

// ─────────────────────────────── slow-path tests ────────────────────────────

// Test 1: Device page (page-table attr, mtCfg=false), shCfg=ISH → output OSH=2
// This is the slow path: TLB is cold, full translation runs, applyOutputAttrs fires.
TEST_F(P4DeviceOshTest, DevicePageMtCfgFalseSlowPathForcesOsh) {
    configureDeviceStream(/*shCfg=*/3u, /*mtCfg=*/false);

    auto result = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Translation must succeed for a mapped Device page";

    const TranslationData& td = result.getValue();
    // ARM §13.1.7 Rule 1: Device memory must always use OSH (2)
    EXPECT_EQ(td.shareability, 2u)
        << "BUG-13.1.7-CPP: Device page (mtCfg=false) must force OSH=2, got "
        << static_cast<int>(td.shareability);
}

// Test 2: Normal page with mtCfg=false, shCfg=ISH → output shareability stays ISH=3
// Regression guard: normal pages must NOT be forced to OSH.
TEST_F(P4DeviceOshTest, NormalPageMtCfgFalseDoesNotForceOsh) {
    configureNormalStream(/*shCfg=*/3u, /*mtCfg=*/false);

    auto result = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Translation must succeed for a mapped Normal page";

    const TranslationData& td = result.getValue();
    EXPECT_EQ(td.shareability, 3u)
        << "Normal page (mtCfg=false) must keep STE shCfg=ISH=3, got "
        << static_cast<int>(td.shareability);
}

// Test 3: Device via STE override (mtCfg=true, memAttr=0x00), shCfg=ISH → OSH=2
TEST_F(P4DeviceOshTest, DeviceMemAttrMtCfgTrueSlowPathForcesOsh) {
    configureNormalStream(/*shCfg=*/3u, /*mtCfg=*/true, /*memAttr=*/0x00u);

    auto result = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk());

    const TranslationData& td = result.getValue();
    EXPECT_EQ(td.shareability, 2u)
        << "BUG-13.1.7-CPP: Device via STE (mtCfg=true/memAttr=0x00) must force OSH=2, got "
        << static_cast<int>(td.shareability);
}

// ─────────────────────────────── TLB fast-path tests ────────────────────────

// Test 4: TLB fast path — after first translation (populates TLB), second translation
// must also return OSH=2 for Device page (mtCfg=false).
TEST_F(P4DeviceOshTest, DevicePageMtCfgFalseTlbFastPathForcesOsh) {
    configureDeviceStream(/*shCfg=*/3u, /*mtCfg=*/false);

    // First translation — populates TLB (slow path)
    auto result1 = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result1.isOk());

    // Second translation — hits TLB fast path
    auto result2 = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result2.isOk()) << "TLB-hit translation must succeed";

    const TranslationData& td = result2.getValue();
    EXPECT_EQ(td.shareability, 2u)
        << "BUG-13.1.7-CPP: Device page TLB fast path must force OSH=2, got "
        << static_cast<int>(td.shareability);
}

// Test 5: TLB fast path — Normal page must NOT be forced to OSH (regression)
TEST_F(P4DeviceOshTest, NormalPageMtCfgFalseTlbFastPathDoesNotForceOsh) {
    configureNormalStream(/*shCfg=*/3u, /*mtCfg=*/false);

    // First translation — populates TLB
    auto result1 = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result1.isOk());

    // Second translation — hits TLB fast path
    auto result2 = smmu_->translate(kSid, kPasid, kIova, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result2.isOk());

    const TranslationData& td = result2.getValue();
    EXPECT_EQ(td.shareability, 3u)
        << "Normal page TLB fast path must keep ISH=3, got "
        << static_cast<int>(td.shareability);
}

} // namespace test
} // namespace smmu
