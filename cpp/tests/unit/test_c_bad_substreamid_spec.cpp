// ARM SMMU v3 FINDING-NEW-11: C_BAD_SUBSTREAMID for stage-2-only / bypass + non-zero PASID
//
// TDD spec tests verifying that a non-zero PASID presented to a stream that
// has no stage-1 translation (stage-2-only or bypass) generates a
// C_BAD_SUBSTREAMID event (event code 0x08) per ARM IHI0070G.b §7.3.9.
//
// Spec reference:
//   §3.9:  "Supply of a PASID or SubstreamID to a configuration without stage
//           1 translation causes the translation to fail.  Such transactions
//           are terminated with an abort and C_BAD_SUBSTREAMID is recorded."
//   §7.3.9: C_BAD_SUBSTREAMID — event code 0x08.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Helpers ─────────────────────────────────────────────────────────────────

static constexpr StreamID S2_ONLY_SID  = 0x10;
static constexpr StreamID BYPASS_SID   = 0x11;
static constexpr StreamID S1_ONLY_SID  = 0x12;
static constexpr StreamID BOTH_SID     = 0x13;
static constexpr PASID    NONZERO_PASID = 1;
static constexpr PASID    PASID_ZERO    = 0;
static constexpr IOVA     TEST_IOVA    = 0x1000;
static constexpr PA       TEST_PA      = 0x20000;

static bool hasEvent(const std::vector<EventEntry>& events, EventType t) {
    for (const auto& e : events) {
        if (e.type == t) return true;
    }
    return false;
}

class CBadSubstreamidTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
        smmu->enable();

        // Stage-2-only stream: stage1 disabled, stage2 enabled
        StreamConfig s2cfg;
        s2cfg.translationEnabled = true;
        s2cfg.stage1Enabled      = false;
        s2cfg.stage2Enabled      = true;
        s2cfg.faultMode          = FaultMode::Terminate;
        smmu->configureStream(S2_ONLY_SID, s2cfg);
        smmu->enableStream(S2_ONLY_SID);
        // Map IPA → PA for stage-2 context (PASID 0)
        smmu->createStreamPASID(S2_ONLY_SID, PASID_ZERO);
        PagePermissions perms(true, true, false);
        smmu->mapPage(S2_ONLY_SID, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

        // Bypass stream: translationEnabled = false
        StreamConfig bypassCfg;
        bypassCfg.translationEnabled = false;
        bypassCfg.stage1Enabled      = false;
        bypassCfg.stage2Enabled      = false;
        bypassCfg.faultMode          = FaultMode::Terminate;
        smmu->configureStream(BYPASS_SID, bypassCfg);
        smmu->enableStream(BYPASS_SID);

        // Stage-1-only stream (control: must NOT generate C_BAD_SUBSTREAMID)
        StreamConfig s1cfg;
        s1cfg.translationEnabled = true;
        s1cfg.stage1Enabled      = true;
        s1cfg.stage2Enabled      = false;
        s1cfg.faultMode          = FaultMode::Terminate;
        smmu->configureStream(S1_ONLY_SID, s1cfg);
        smmu->enableStream(S1_ONLY_SID);
        smmu->createStreamPASID(S1_ONLY_SID, NONZERO_PASID);
        smmu->mapPage(S1_ONLY_SID, NONZERO_PASID, TEST_IOVA, TEST_PA, perms);

        // Both-stages stream (control: must NOT generate C_BAD_SUBSTREAMID)
        StreamConfig bothCfg;
        bothCfg.translationEnabled = true;
        bothCfg.stage1Enabled      = true;
        bothCfg.stage2Enabled      = true;
        bothCfg.faultMode          = FaultMode::Terminate;
        smmu->configureStream(BOTH_SID, bothCfg);
        smmu->enableStream(BOTH_SID);
        smmu->createStreamPASID(BOTH_SID, NONZERO_PASID);
        smmu->mapPage(BOTH_SID, NONZERO_PASID, TEST_IOVA, TEST_PA, perms);

        smmu->clearEventQueue();
    }

    void TearDown() override { smmu.reset(); }

    std::unique_ptr<SMMU> smmu;
};

// ── §7.3.9 opcode ────────────────────────────────────────────────────────────

/// §7.3.9: C_BAD_SUBSTREAMID must have event code 0x08.
TEST_F(CBadSubstreamidTest, EventOpcodeIs0x08) {
    EXPECT_EQ(static_cast<int>(EventType::C_BAD_SUBSTREAMID), 0x08)
        << "C_BAD_SUBSTREAMID must be event code 0x08 per ARM IHI0070G.b §7.3.9";
}

// ── Stage-2-only + non-zero PASID ────────────────────────────────────────────

/// §3.9 / §7.3.9: Stage-2-only stream with non-zero PASID must abort.
TEST_F(CBadSubstreamidTest, Stage2OnlyNonZeroPasidFails) {
    auto result = smmu->translate(S2_ONLY_SID, NONZERO_PASID, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "Stage-2-only stream with non-zero PASID must fail (§3.9)";
}

/// §3.9 / §7.3.9: Stage-2-only + non-zero PASID must generate C_BAD_SUBSTREAMID.
TEST_F(CBadSubstreamidTest, Stage2OnlyNonZeroPasidGeneratesEvent) {
    smmu->translate(S2_ONLY_SID, NONZERO_PASID, TEST_IOVA,
                    AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(hasEvent(smmu->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "C_BAD_SUBSTREAMID (0x08) must be recorded per ARM IHI0070G.b §7.3.9";
}

/// §3.9: Stage-2-only stream with PASID=0 must succeed (no C_BAD_SUBSTREAMID).
TEST_F(CBadSubstreamidTest, Stage2OnlyPasidZeroSucceeds) {
    auto result = smmu->translate(S2_ONLY_SID, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "Stage-2-only stream with PASID=0 must succeed";
    EXPECT_FALSE(hasEvent(smmu->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "PASID=0 on stage-2-only stream must not generate C_BAD_SUBSTREAMID";
}

// ── Bypass + non-zero PASID ──────────────────────────────────────────────────

/// §3.9 / §7.3.9: Bypass stream with non-zero PASID must abort.
TEST_F(CBadSubstreamidTest, BypassNonZeroPasidFails) {
    auto result = smmu->translate(BYPASS_SID, NONZERO_PASID, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "Bypass stream with non-zero PASID must fail (§3.9)";
}

/// §3.9 / §7.3.9: Bypass stream + non-zero PASID must generate C_BAD_SUBSTREAMID.
TEST_F(CBadSubstreamidTest, BypassNonZeroPasidGeneratesEvent) {
    smmu->translate(BYPASS_SID, NONZERO_PASID, TEST_IOVA,
                    AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(hasEvent(smmu->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "C_BAD_SUBSTREAMID (0x08) must be recorded per ARM IHI0070G.b §7.3.9";
}

/// §3.9: Bypass stream with PASID=0 must succeed (identity mapping, no fault).
TEST_F(CBadSubstreamidTest, BypassPasidZeroSucceeds) {
    auto result = smmu->translate(BYPASS_SID, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "Bypass stream with PASID=0 must succeed with identity PA";
    EXPECT_FALSE(hasEvent(smmu->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "PASID=0 on bypass stream must not generate C_BAD_SUBSTREAMID";
}

// ── Control: stage-1 and both-stages must NOT fault on non-zero PASID ────────

/// §3.9: Stage-1-only stream with non-zero PASID must NOT generate C_BAD_SUBSTREAMID.
TEST_F(CBadSubstreamidTest, Stage1OnlyNonZeroPasidIsOk) {
    auto result = smmu->translate(S1_ONLY_SID, NONZERO_PASID, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "Stage-1-only with non-zero PASID must succeed";
    EXPECT_FALSE(hasEvent(smmu->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "Stage-1-only stream must not produce C_BAD_SUBSTREAMID for non-zero PASID";
}

/// §3.9: Both-stages stream with non-zero PASID must NOT generate C_BAD_SUBSTREAMID.
/// Note: translation may fail for other reasons (stage-2 address space not fully
/// configured here), but the key requirement is no C_BAD_SUBSTREAMID is produced.
TEST_F(CBadSubstreamidTest, BothStagesNonZeroPasidIsOk) {
    smmu->translate(BOTH_SID, NONZERO_PASID, TEST_IOVA,
                    AccessType::Read, SecurityState::NonSecure);
    EXPECT_FALSE(hasEvent(smmu->getEventQueue(), EventType::C_BAD_SUBSTREAMID))
        << "Both-stages stream must not produce C_BAD_SUBSTREAMID for non-zero PASID";
}

// ── Event carries correct StreamID ───────────────────────────────────────────

/// §7.3.9: C_BAD_SUBSTREAMID event must report the faulting StreamID.
TEST_F(CBadSubstreamidTest, Stage2OnlyEventCarriesStreamId) {
    smmu->translate(S2_ONLY_SID, NONZERO_PASID, TEST_IOVA,
                    AccessType::Read, SecurityState::NonSecure);
    auto events = smmu->getEventQueue();
    bool found = false;
    for (const auto& e : events) {
        if (e.type == EventType::C_BAD_SUBSTREAMID && e.streamID == S2_ONLY_SID) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "C_BAD_SUBSTREAMID event must carry the faulting StreamID";
}

} // namespace test
} // namespace smmu
