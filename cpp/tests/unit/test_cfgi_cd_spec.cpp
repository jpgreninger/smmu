// ARM SMMU v3 FINDING-NEW-12: CMD_CFGI_CD and CMD_CFGI_CD_ALL command processing
//
// TDD spec tests for ARM §4.3.3 CMD_CFGI_CD and §4.3.4 CMD_CFGI_CD_ALL.
//
// Spec references:
//   §4.3.3  CMD_CFGI_CD (0x05) — Invalidate cached Context Descriptor for (SID, SSID)
//   §4.3.4  CMD_CFGI_CD_ALL (0x06) — Invalidate all cached CDs for SID
//   §6.3.17 SMMU_GERROR — GERROR_CMDQ_ERR must NOT be set for valid commands
//   §7.3.5  C_BAD_STE — must NOT be generated for valid commands

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <vector>
#include <memory>

namespace smmu {
namespace test {

// ── Fixture ────────────────────────────────────────────────────────────────

class SMMUCfgiCdTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
        smmu->enable();

        // Configure a stream with Stage 1 translation in Terminate mode
        StreamConfig config;
        config.translationEnabled = true;
        config.stage1Enabled = true;
        config.stage2Enabled = false;
        config.faultMode = FaultMode::Terminate;
        smmu->configureStream(STREAM_ID, config);
        smmu->enableStream(STREAM_ID);
        smmu->createStreamPASID(STREAM_ID, PASID_1);
        smmu->createStreamPASID(STREAM_ID, PASID_2);

        // Map pages for both PASIDs
        PagePermissions perms(true, true, false);
        smmu->mapPage(STREAM_ID, PASID_1, TEST_IOVA, TEST_PA_1, perms);
        smmu->mapPage(STREAM_ID, PASID_2, TEST_IOVA, TEST_PA_2, perms);

        // Enable caching so invalidation commands have observable effect
        smmu->enableCaching(true);

        // Clear any events generated during setup
        smmu->clearEventQueue();
    }

    void TearDown() override {
        smmu.reset();
    }

    std::unique_ptr<SMMU> smmu;

    static constexpr StreamID STREAM_ID   = 0x42;
    static constexpr PASID    PASID_1     = 1;
    static constexpr PASID    PASID_2     = 2;
    static constexpr IOVA     TEST_IOVA   = 0x1000;
    static constexpr PA       TEST_PA_1   = 0x10000;
    static constexpr PA       TEST_PA_2   = 0x20000;

    // Helper: build a CFGI_CD command for (STREAM_ID, pasid)
    CommandEntry makeCfgiCd(PASID pasid) {
        CommandEntry cmd;
        cmd.type      = CommandType::CFGI_CD;
        cmd.streamID  = STREAM_ID;
        cmd.pasid     = pasid;
        return cmd;
    }

    // Helper: build a CFGI_CD_ALL command for STREAM_ID
    CommandEntry makeCfgiCdAll() {
        CommandEntry cmd;
        cmd.type      = CommandType::CFGI_CD_ALL;
        cmd.streamID  = STREAM_ID;
        return cmd;
    }

    // Helper: check event queue for presence of C_BAD_STE
    bool hasCBadSteEvent() {
        auto events = smmu->getEventQueue();
        for (const auto& ev : events) {
            if (ev.type == EventType::C_BAD_STE) {
                return true;
            }
        }
        return false;
    }
};

// ── §4.1.1: Opcode values ─────────────────────────────────────────────────

/// §4.1.1: CMD_CFGI_CD must have opcode 0x05.
TEST_F(SMMUCfgiCdTest, CfgiCdHasCorrectOpcode) {
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_CD), 0x05)
        << "CMD_CFGI_CD must have opcode 0x05 per ARM IHI0070G.b §4.1.1";
}

/// §4.1.1: CMD_CFGI_CD_ALL must have opcode 0x06.
TEST_F(SMMUCfgiCdTest, CfgiCdAllHasCorrectOpcode) {
    EXPECT_EQ(static_cast<int>(CommandType::CFGI_CD_ALL), 0x06)
        << "CMD_CFGI_CD_ALL must have opcode 0x06 per ARM IHI0070G.b §4.1.1";
}

// ── §6.3.17: GERROR must not be set for valid commands ────────────────────

/// §6.3.17: Processing CMD_CFGI_CD must NOT set GERROR_CMDQ_ERR.
TEST_F(SMMUCfgiCdTest, CfgiCdDoesNotSetGerror) {
    smmu->submitCommand(makeCfgiCd(PASID_1));
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMD_CFGI_CD is a valid command and must not set GERROR_CMDQ_ERR";
}

/// §6.3.17: Processing CMD_CFGI_CD_ALL must NOT set GERROR_CMDQ_ERR.
TEST_F(SMMUCfgiCdTest, CfgiCdAllDoesNotSetGerror) {
    smmu->submitCommand(makeCfgiCdAll());
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMD_CFGI_CD_ALL is a valid command and must not set GERROR_CMDQ_ERR";
}

// ── §7.3.5: C_BAD_STE must not be generated for valid commands ───────────

/// §7.3.5: Processing CMD_CFGI_CD must NOT generate a C_BAD_STE event.
TEST_F(SMMUCfgiCdTest, CfgiCdDoesNotGenerateBadSteEvent) {
    smmu->submitCommand(makeCfgiCd(PASID_1));
    smmu->processCommandQueue();

    EXPECT_FALSE(hasCBadSteEvent())
        << "CMD_CFGI_CD must not produce a C_BAD_STE event";
}

/// §7.3.5: Processing CMD_CFGI_CD_ALL must NOT generate a C_BAD_STE event.
TEST_F(SMMUCfgiCdTest, CfgiCdAllDoesNotGenerateBadSteEvent) {
    smmu->submitCommand(makeCfgiCdAll());
    smmu->processCommandQueue();

    EXPECT_FALSE(hasCBadSteEvent())
        << "CMD_CFGI_CD_ALL must not produce a C_BAD_STE event";
}

// ── §4.3.3: CMD_CFGI_CD invalidates the (SID, SSID) CD cache ─────────────

/// §4.3.3: After CMD_CFGI_CD the stream/PASID can still translate correctly.
/// Invalidation must flush the cache entry but not destroy the page mapping.
TEST_F(SMMUCfgiCdTest, CfgiCdInvalidatesPasidCache) {
    // Warm the cache with a successful translation
    TranslationResult before = smmu->translate(STREAM_ID, PASID_1, TEST_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(before.isOk()) << "Pre-invalidation translation must succeed";

    // Issue CMD_CFGI_CD to flush the cache entry for (STREAM_ID, PASID_1)
    smmu->submitCommand(makeCfgiCd(PASID_1));
    smmu->processCommandQueue();

    // The page mapping is still present — translation must still succeed
    TranslationResult after = smmu->translate(STREAM_ID, PASID_1, TEST_IOVA,
                                              AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(after.isOk())
        << "Translation must still succeed after CMD_CFGI_CD (mapping is intact)";
    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u);
}

/// §4.3.4: After CMD_CFGI_CD_ALL both PASIDs in the stream still translate.
TEST_F(SMMUCfgiCdTest, CfgiCdAllInvalidatesAllPasids) {
    // Warm cache for both PASIDs
    ASSERT_TRUE(smmu->translate(STREAM_ID, PASID_1, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk());
    ASSERT_TRUE(smmu->translate(STREAM_ID, PASID_2, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk());

    // Issue CMD_CFGI_CD_ALL — flushes all CD entries for STREAM_ID
    smmu->submitCommand(makeCfgiCdAll());
    smmu->processCommandQueue();

    // Mappings still exist — both translations must still succeed
    EXPECT_TRUE(smmu->translate(STREAM_ID, PASID_1, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk())
        << "PASID_1 translation must succeed after CMD_CFGI_CD_ALL";
    EXPECT_TRUE(smmu->translate(STREAM_ID, PASID_2, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk())
        << "PASID_2 translation must succeed after CMD_CFGI_CD_ALL";
}

// ── §4.3.3: CMD_CFGI_CD is PASID-scoped ───────────────────────────────────

/// §4.3.3: CMD_CFGI_CD for PASID_1 must not affect PASID_2 translations.
TEST_F(SMMUCfgiCdTest, CfgiCdForWrongPasidDoesNotAffectOtherPasid) {
    // Warm cache for both PASIDs
    ASSERT_TRUE(smmu->translate(STREAM_ID, PASID_1, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk());
    ASSERT_TRUE(smmu->translate(STREAM_ID, PASID_2, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk());

    // Invalidate only PASID_1's CD entry
    smmu->submitCommand(makeCfgiCd(PASID_1));
    smmu->processCommandQueue();

    // PASID_2's mapping is unaffected — must still translate successfully
    EXPECT_TRUE(smmu->translate(STREAM_ID, PASID_2, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure).isOk())
        << "CMD_CFGI_CD for PASID_1 must not disrupt PASID_2 translations";
}

// ── §4.3.3 / §4.3.4: Command followed by SYNC ────────────────────────────

/// CMD_CFGI_CD followed by CMD_SYNC must process without errors.
TEST_F(SMMUCfgiCdTest, CfgiCdFollowedBySync) {
    smmu->submitCommand(makeCfgiCd(PASID_1));

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    smmu->submitCommand(sync);

    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMD_CFGI_CD + CMD_SYNC must not set GERROR_CMDQ_ERR";
    EXPECT_FALSE(hasCBadSteEvent())
        << "CMD_CFGI_CD + CMD_SYNC must not produce C_BAD_STE";
}

/// CMD_CFGI_CD_ALL followed by CMD_SYNC must process without errors.
TEST_F(SMMUCfgiCdTest, CfgiCdAllFollowedBySync) {
    smmu->submitCommand(makeCfgiCdAll());

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    smmu->submitCommand(sync);

    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMD_CFGI_CD_ALL + CMD_SYNC must not set GERROR_CMDQ_ERR";
    EXPECT_FALSE(hasCBadSteEvent())
        << "CMD_CFGI_CD_ALL + CMD_SYNC must not produce C_BAD_STE";
}

// ── §4.3.3: Unknown stream is a no-op ────────────────────────────────────

/// §4.3.3: CMD_CFGI_CD for an unconfigured StreamID must be a no-op (no GERROR, no events).
TEST_F(SMMUCfgiCdTest, CfgiCdUnknownStreamIsNoOp) {
    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD;
    cmd.streamID = 0xDEAD; // stream never configured
    cmd.pasid    = PASID_1;

    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getGerror() & GERROR_CMDQ_ERR, 0u)
        << "CMD_CFGI_CD for unknown stream must not set GERROR_CMDQ_ERR";
    EXPECT_FALSE(hasCBadSteEvent())
        << "CMD_CFGI_CD for unknown stream must not generate C_BAD_STE";
}

} // namespace test
} // namespace smmu
