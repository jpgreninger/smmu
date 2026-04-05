// TDD tests for BUG-AUDIT-109, BUG-AUDIT-110, BUG-AUDIT-111, BUG-AUDIT-112
//
// BUG-AUDIT-109: stage-1-enabled, s1cdMax==0, SSV=1, non-zero PASID → C_BAD_SUBSTREAMID
// BUG-AUDIT-110: stage-1-enabled, s1cdMax>0, S1DSS==0b11 (reserved) → F_STREAM_DISABLED
// BUG-AUDIT-111: bypass/stage-2-only, SSV=1, PASID=0 → C_BAD_SUBSTREAMID
// BUG-AUDIT-112: stage-1-enabled, s1cdMax==0, SSV=1, PASID=0 → C_BAD_SUBSTREAMID
//
// ARM §5.2 STE.S1CDMax line 6691:
//   "If this field is 0, then any transaction using this STE that is presented
//    with SSV=1 is terminated with an abort, and a C_BAD_SUBSTREAMID event is
//    generated."
//
// ARM §5.2 STE.S1DSS line 6716:
//   "0b11 Reserved (behaves as 0b00)."
//
// ARM §3.3.2 line 1354:
//   "Transactions provided with a SubstreamID are terminated when stage 1
//    translation is not enabled."

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Helpers ──────────────────────────────────────────────────────────────────

static constexpr StreamID SID_S1_CDMAX0   = 0xA0;  // stage-1, s1cdMax==0
static constexpr StreamID SID_S1_CDMAX2   = 0xA1;  // stage-1, s1cdMax==2, s1dss==0x03
static constexpr StreamID SID_BYPASS      = 0xA2;  // bypass stream
static constexpr StreamID SID_S2_ONLY     = 0xA3;  // stage-2-only
static constexpr StreamID SID_S1_CDMAX2_OK = 0xA4; // stage-1, s1cdMax==2, s1dss==0x02 (valid)

static constexpr IOVA TEST_IOVA = 0x1000ULL;
static constexpr PA   TEST_PA   = 0x80000000ULL;

static bool hasEvent(const std::vector<EventEntry>& events, EventType t) {
    for (const auto& e : events) {
        if (e.type == t) {
            return true;
        }
    }
    return false;
}

class BugAudit109_112_Test : public ::testing::Test {
protected:
    void SetUp() override {
        smmuPtr = std::unique_ptr<SMMU>(new SMMU());
        smmuPtr->enable();

        // Stream A0: stage-1 enabled, s1cdMax==0 (not substream-capable)
        // Any SSV=1 transaction must abort with C_BAD_SUBSTREAMID per §5.2 line 6691
        {
            StreamConfig cfg;
            cfg.translationEnabled = true;
            cfg.stage1Enabled      = true;
            cfg.stage2Enabled      = false;
            cfg.faultMode          = FaultMode::Terminate;
            cfg.s1cdMax            = 0;
            cfg.s1dss              = 0x02;  // irrelevant when s1cdMax==0
            smmuPtr->configureStream(SID_S1_CDMAX0, cfg);
            smmuPtr->enableStream(SID_S1_CDMAX0);
            smmuPtr->createStreamPASID(SID_S1_CDMAX0, 0);
            PagePermissions perms(true, true, false);
            smmuPtr->mapPage(SID_S1_CDMAX0, 0, TEST_IOVA, TEST_PA, perms);
        }

        // Stream A1: stage-1 enabled, s1cdMax==2, S1DSS==0b11 (reserved)
        // Must behave like 0b00 → F_STREAM_DISABLED per §5.2 line 6716
        {
            StreamConfig cfg;
            cfg.translationEnabled = true;
            cfg.stage1Enabled      = true;
            cfg.stage2Enabled      = false;
            cfg.faultMode          = FaultMode::Terminate;
            cfg.s1cdMax            = 2;
            cfg.s1dss              = 0x03;  // RESERVED — behaves as 0b00
            smmuPtr->configureStream(SID_S1_CDMAX2, cfg);
            smmuPtr->enableStream(SID_S1_CDMAX2);
            smmuPtr->createStreamPASID(SID_S1_CDMAX2, 0);
            PagePermissions perms(true, true, false);
            smmuPtr->mapPage(SID_S1_CDMAX2, 0, TEST_IOVA, TEST_PA, perms);
        }

        // Stream A2: bypass (translationEnabled=false, bypassEnabled=true)
        {
            StreamConfig cfg;
            cfg.translationEnabled = false;
            cfg.stage1Enabled      = false;
            cfg.stage2Enabled      = false;
            cfg.bypassEnabled      = true;
            cfg.faultMode          = FaultMode::Terminate;
            smmuPtr->configureStream(SID_BYPASS, cfg);
            smmuPtr->enableStream(SID_BYPASS);
        }

        // Stream A3: stage-2-only (stage1Enabled=false, stage2Enabled=true)
        {
            StreamConfig cfg;
            cfg.translationEnabled = true;
            cfg.stage1Enabled      = false;
            cfg.stage2Enabled      = true;
            cfg.faultMode          = FaultMode::Terminate;
            smmuPtr->configureStream(SID_S2_ONLY, cfg);
            smmuPtr->enableStream(SID_S2_ONLY);
            smmuPtr->createStreamPASID(SID_S2_ONLY, 0);
            PagePermissions perms(true, true, false);
            smmuPtr->mapPage(SID_S2_ONLY, 0, TEST_IOVA, TEST_PA, perms);
        }

        // Stream A4: stage-1 enabled, s1cdMax==2, S1DSS==0b10 (valid — use CD[0])
        // Regression control: valid substream (PASID=1) must still succeed
        {
            StreamConfig cfg;
            cfg.translationEnabled = true;
            cfg.stage1Enabled      = true;
            cfg.stage2Enabled      = false;
            cfg.faultMode          = FaultMode::Terminate;
            cfg.s1cdMax            = 2;
            cfg.s1dss              = 0x02;
            smmuPtr->configureStream(SID_S1_CDMAX2_OK, cfg);
            smmuPtr->enableStream(SID_S1_CDMAX2_OK);
            smmuPtr->createStreamPASID(SID_S1_CDMAX2_OK, 0);
            smmuPtr->createStreamPASID(SID_S1_CDMAX2_OK, 1);
            PagePermissions perms(true, true, false);
            smmuPtr->mapPage(SID_S1_CDMAX2_OK, 0, TEST_IOVA, TEST_PA, perms);
            smmuPtr->mapPage(SID_S1_CDMAX2_OK, 1, TEST_IOVA, TEST_PA, perms);
        }

        smmuPtr->clearEventQueue();
    }

    void TearDown() override {
        smmuPtr.reset();
    }

    std::unique_ptr<SMMU> smmuPtr;
};

// ── BUG-AUDIT-109: s1cdMax==0, SSV=1, non-zero PASID → abort + C_BAD_SUBSTREAMID ──

/// §5.2 line 6691: s1cdMax==0, SSV=1, PASID=5 must abort.
TEST_F(BugAudit109_112_Test, Audit109_S1CdMax0_SSV1_NonZeroPasid_Fails) {
    auto result = smmuPtr->translate(SID_S1_CDMAX0, 5, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, true);
    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-109: s1cdMax==0 + SSV=1 + PASID=5 must abort (§5.2 line 6691)";
}

/// §5.2 line 6691: s1cdMax==0, SSV=1, PASID=5 must generate C_BAD_SUBSTREAMID event.
TEST_F(BugAudit109_112_Test, Audit109_S1CdMax0_SSV1_NonZeroPasid_EmitsEvent) {
    smmuPtr->translate(SID_S1_CDMAX0, 5, TEST_IOVA,
                       AccessType::Read, SecurityState::NonSecure,
                       TransactionType::Ordinary, true);
    EXPECT_TRUE(hasEvent(smmuPtr->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "BUG-AUDIT-109: C_BAD_SUBSTREAMID (0x08) must be emitted (§5.2 line 6691)";
}

// ── BUG-AUDIT-112: s1cdMax==0, SSV=1, PASID=0 → abort + C_BAD_SUBSTREAMID ──────

/// §5.2 line 6691: s1cdMax==0, SSV=1, PASID=0 must abort.
TEST_F(BugAudit109_112_Test, Audit112_S1CdMax0_SSV1_Pasid0_Fails) {
    auto result = smmuPtr->translate(SID_S1_CDMAX0, 0, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, true);
    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-112: s1cdMax==0 + SSV=1 + PASID=0 must abort (§5.2 line 6691)";
}

/// §5.2 line 6691: s1cdMax==0, SSV=1, PASID=0 must generate C_BAD_SUBSTREAMID event.
TEST_F(BugAudit109_112_Test, Audit112_S1CdMax0_SSV1_Pasid0_EmitsEvent) {
    smmuPtr->translate(SID_S1_CDMAX0, 0, TEST_IOVA,
                       AccessType::Read, SecurityState::NonSecure,
                       TransactionType::Ordinary, true);
    EXPECT_TRUE(hasEvent(smmuPtr->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "BUG-AUDIT-112: C_BAD_SUBSTREAMID (0x08) must be emitted (§5.2 line 6691)";
}

// ── BUG-AUDIT-110: S1DSS==0b11 (reserved) → F_STREAM_DISABLED ───────────────────

/// §5.2 line 6716: S1DSS==0b11 (reserved) must abort (behaves as 0b00).
TEST_F(BugAudit109_112_Test, Audit110_S1dss11_Pasid0_Fails) {
    // pasid=0, ssv=false: this is the "non-substream" transaction path where
    // S1DSS controls dispatch for substream-capable streams.
    auto result = smmuPtr->translate(SID_S1_CDMAX2, 0, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, false);
    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-110: S1DSS==0b11 (reserved) must abort like 0b00 (§5.2 line 6716)";
}

/// §5.2 line 6716: S1DSS==0b11 must generate F_STREAM_DISABLED event.
TEST_F(BugAudit109_112_Test, Audit110_S1dss11_Pasid0_EmitsFStreamDisabled) {
    smmuPtr->translate(SID_S1_CDMAX2, 0, TEST_IOVA,
                       AccessType::Read, SecurityState::NonSecure,
                       TransactionType::Ordinary, false);
    EXPECT_TRUE(hasEvent(smmuPtr->getEventQueue(), EventType::F_STREAM_DISABLED))
        << "BUG-AUDIT-110: F_STREAM_DISABLED (0x06) must be emitted for S1DSS==0b11";
}

// ── BUG-AUDIT-111: bypass + SSV=1 + PASID=0 → C_BAD_SUBSTREAMID ─────────────────

/// §3.3.2 line 1354: bypass stream, SSV=1, PASID=0 must abort.
TEST_F(BugAudit109_112_Test, Audit111_Bypass_SSV1_Pasid0_Fails) {
    auto result = smmuPtr->translate(SID_BYPASS, 0, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, true);
    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-111: bypass + SSV=1 + PASID=0 must abort (§3.3.2 line 1354)";
}

/// §3.3.2 line 1354: bypass stream, SSV=1, PASID=0 must generate C_BAD_SUBSTREAMID.
TEST_F(BugAudit109_112_Test, Audit111_Bypass_SSV1_Pasid0_EmitsEvent) {
    smmuPtr->translate(SID_BYPASS, 0, TEST_IOVA,
                       AccessType::Read, SecurityState::NonSecure,
                       TransactionType::Ordinary, true);
    EXPECT_TRUE(hasEvent(smmuPtr->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "BUG-AUDIT-111: C_BAD_SUBSTREAMID must be emitted for bypass + SSV=1 + PASID=0";
}

/// §3.3.2 line 1354: stage-2-only stream, SSV=1, PASID=0 must abort.
TEST_F(BugAudit109_112_Test, Audit111_Stage2Only_SSV1_Pasid0_Fails) {
    auto result = smmuPtr->translate(SID_S2_ONLY, 0, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, true);
    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-111: stage-2-only + SSV=1 + PASID=0 must abort (§3.3.2 line 1354)";
}

/// §3.3.2 line 1354: stage-2-only stream, SSV=1, PASID=0 must generate C_BAD_SUBSTREAMID.
TEST_F(BugAudit109_112_Test, Audit111_Stage2Only_SSV1_Pasid0_EmitsEvent) {
    smmuPtr->translate(SID_S2_ONLY, 0, TEST_IOVA,
                       AccessType::Read, SecurityState::NonSecure,
                       TransactionType::Ordinary, true);
    EXPECT_TRUE(hasEvent(smmuPtr->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "BUG-AUDIT-111: C_BAD_SUBSTREAMID must be emitted for stage-2-only + SSV=1 + PASID=0";
}

// ── Regression: valid substream must still work ────────────────────────────────

/// Regression: stage-1-enabled, s1cdMax==2, s1dss==0b10, SSV=1, PASID=1 → succeeds.
TEST_F(BugAudit109_112_Test, Regression_ValidSubstream_Succeeds) {
    auto result = smmuPtr->translate(SID_S1_CDMAX2_OK, 1, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, true);
    EXPECT_TRUE(result.isOk())
        << "Regression: valid substream (PASID=1, s1cdMax=2) must succeed";
    EXPECT_FALSE(hasEvent(smmuPtr->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "Regression: no C_BAD_SUBSTREAMID for valid substream";
}

/// Regression: bypass stream, SSV=false, PASID=0 → identity result (must still work).
TEST_F(BugAudit109_112_Test, Regression_Bypass_SSV0_Pasid0_Succeeds) {
    auto result = smmuPtr->translate(SID_BYPASS, 0, TEST_IOVA,
                                     AccessType::Read, SecurityState::NonSecure,
                                     TransactionType::Ordinary, false);
    EXPECT_TRUE(result.isOk())
        << "Regression: bypass + SSV=false + PASID=0 must succeed with identity mapping";
    EXPECT_FALSE(hasEvent(smmuPtr->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "Regression: no C_BAD_SUBSTREAMID for bypass + SSV=false";
}

} // namespace test
} // namespace smmu
