// ARM SMMU v3 Tenth-Pass Bug Tests — BUG-11 and BUG-14 (C++)
//
// BUG-11: s1cdMax >= 32 causes UB on shift `1u << s1cdMax`; no validation
//         that s1cdMax <= 20 (SMMU_IDR1.SSIDSIZE max). (§5.2)
// BUG-14: C_BAD_STREAMID event for unknown stream hardcodes NonSecure;
//         security state must match the command's context. (§7.3.3)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <algorithm>

namespace smmu {
namespace test {

// ── Helpers ──────────────────────────────────────────────────────────────────

// SMMU is not copyable; use unique_ptr for test helpers.
static std::unique_ptr<SMMU> makeSmmu() {
    auto s = std::unique_ptr<SMMU>(new SMMU());
    s->enable();
    // §6.3.12 CR2.RECINVSID=1: enable C_BAD_STREAMID event recording for tests
    // that verify event generation via CMD_CFGI_STE with unknown StreamIDs.
    s->setCR2(SMMU::CR2_RECINVSID);
    return s;
}

static StreamConfig makeStage1TerminateConfig() {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.faultMode = FaultMode::Terminate;
    return cfg;
}

static bool hasEventType(SMMU& smmu, EventType type) {
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == type) { return true; }
    }
    return false;
}

// ── BUG-11: s1cdMax validation and shift guard (C++) ─────────────────────────
//
// ARM §5.2 STE.S1CDMax + SMMU_IDR1.SSIDSIZE: valid range 0–20.
// isConfigurationValid() must reject s1cdMax > 20.
// Additionally, the shift `1u << config.s1cdMax` at the C_BAD_SUBSTREAMID
// check (~line 961) is UB for s1cdMax >= 32.
//
// Tests FAIL before fix: configureStream currently returns success for s1cdMax=21.

/// §5.2 / BUG-11: s1cdMax=21 must be rejected by configureStream.
TEST(Bug11CppS1cdMax, ExceedsSsidsizeMaxFails) {
    auto smmu = makeSmmu();
    StreamConfig cfg = makeStage1TerminateConfig();
    cfg.s1cdMax = 21;  // > 20 (SSIDSIZE max)
    auto result = smmu->configureStream(0x10, cfg);
    EXPECT_TRUE(result.isError())
        << "§5.2 / BUG-11: s1cdMax=21 must fail validation (SSIDSIZE max is 20)";
}

/// §5.2 / BUG-11: s1cdMax=20 must be accepted (exactly at SSIDSIZE maximum).
TEST(Bug11CppS1cdMax, AtSsidsizeMaxSucceeds) {
    auto smmu = makeSmmu();
    StreamConfig cfg = makeStage1TerminateConfig();
    cfg.s1cdMax = 20;
    auto result = smmu->configureStream(0x11, cfg);
    EXPECT_FALSE(result.isError())
        << "§5.2: s1cdMax=20 must succeed (SSIDSIZE max is 20)";
}

/// §5.2 / BUG-11: s1cdMax=0 (not substream-capable) must always be valid.
TEST(Bug11CppS1cdMax, ZeroAlwaysValid) {
    auto smmu = makeSmmu();
    StreamConfig cfg = makeStage1TerminateConfig();
    cfg.s1cdMax = 0;
    auto result = smmu->configureStream(0x12, cfg);
    EXPECT_FALSE(result.isError()) << "s1cdMax=0 (not substream-capable) must be valid";
}

/// §5.2 / BUG-11: s1cdMax=31 must be rejected (> 20, would be UB on shift).
TEST(Bug11CppS1cdMax, S1cdMax31Rejected) {
    auto smmu = makeSmmu();
    StreamConfig cfg = makeStage1TerminateConfig();
    cfg.s1cdMax = 31;
    auto result = smmu->configureStream(0x13, cfg);
    EXPECT_TRUE(result.isError())
        << "§5.2 / BUG-11: s1cdMax=31 must fail validation";
}

/// §7.3.9 / BUG-11: A valid s1cdMax=2 stream correctly generates C_BAD_SUBSTREAMID
/// for PASID >= 2^2 = 4.  Verifies shift guard works correctly post-fix.
TEST(Bug11CppS1cdMax, ValidS1cdMaxEnforcesSubstreamRange) {
    auto smmu = makeSmmu();
    StreamConfig cfg = makeStage1TerminateConfig();
    cfg.s1cdMax = 2;  // supports SubstreamID 0..3
    ASSERT_TRUE(smmu->configureStream(0x14, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(0x14).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(0x14, 0).isOk());

    // PASID=4 >= 2^2=4 → C_BAD_SUBSTREAMID
    auto result = smmu->translate(0x14, 4, 0x1000, AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isError());
    EXPECT_TRUE(hasEventType(*smmu, EventType::C_BAD_SUBSTREAMID))
        << "§7.3.9: PASID >= 2^s1cdMax must generate C_BAD_SUBSTREAMID";
}

// ── CONF-GAP-2 / BUG-14: CMD_CFGI_STE for unknown StreamID is a silent no-op ──
//
// ARM §4.3.1: CMD_CFGI_STE is a cache invalidation command.  If the StreamID
// is unknown there is nothing cached to evict.  The command must complete
// silently — no C_BAD_STREAMID event, no GERROR.CMDQ_ERR, regardless of
// the security state field in CommandEntry.
//
// (BUG-14 originally fixed security state propagation in the now-removed
// C_BAD_STREAMID generation path.  These tests now verify the correct no-op
// behavior per CONF-GAP-2.)

/// CONF-GAP-2: Secure CMD_CFGI_STE for unknown stream → silent no-op.
TEST(Bug14CppCbadStreamidSecState, ProcessCommandCarriesSecureState) {
    auto smmu = makeSmmu();

    // Stream 0x99 is not configured — CMD_CFGI_STE must be a silent no-op.
    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = 0x99;
    cmd.securityState = SecurityState::Secure;

    ASSERT_TRUE(smmu->submitCommand(cmd).isOk());
    smmu->processCommandQueue();

    auto events = smmu->getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STREAMID) {
            found = true;
            break;
        }
    }
    EXPECT_FALSE(found) << "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT generate C_BAD_STREAMID (§4.3.1 silent no-op)";
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u) << "CONF-GAP-2: must NOT set GERROR.CMDQ_ERR";
}

/// CONF-GAP-2: NonSecure CMD_CFGI_STE for unknown stream → silent no-op.
TEST(Bug14CppCbadStreamidSecState, ProcessCommandNonSecurePreserved) {
    auto smmu = makeSmmu();

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = 0xAA;
    cmd.securityState = SecurityState::NonSecure;

    ASSERT_TRUE(smmu->submitCommand(cmd).isOk());
    smmu->processCommandQueue();

    auto events = smmu->getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STREAMID) {
            found = true;
            break;
        }
    }
    EXPECT_FALSE(found) << "CONF-GAP-2: CMD_CFGI_STE for unknown stream must NOT generate C_BAD_STREAMID (§4.3.1 silent no-op)";
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u) << "CONF-GAP-2: must NOT set GERROR.CMDQ_ERR";
}

}  // namespace test
}  // namespace smmu
