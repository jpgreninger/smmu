// ARM SMMU v3 FINDING-NEW-08: Stall queue and CMD_RESUME/CMD_STALL_TERM semantics
//
// TDD spec tests for ARM §3.12.2 stall model, §4.6 CMD_RESUME, §4.7 CMD_STALL_TERM.
//
// Spec references:
//   §3.12.2  Stall model — CD.S=1 (FaultMode::Stall) halts transaction, awaits CMD_RESUME
//   §4.6     CMD_RESUME — STAG, Ac (action), Ab (abort) bits determine outcome
//   §4.7     CMD_STALL_TERM — abort ALL stalled transactions for a StreamID

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <set>

namespace smmu {
namespace test {

// ── Fixture ────────────────────────────────────────────────────────────────

class SMMUStallResumeTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
        smmu->enable();
        // Configure stall-mode stream (STREAM_ID)
        StreamConfig stallConfig;
        stallConfig.translationEnabled = true;
        stallConfig.stage1Enabled = true;
        stallConfig.stage2Enabled = false;
        stallConfig.faultMode = FaultMode::Stall;
        smmu->configureStream(STREAM_ID, stallConfig);
        smmu->enableStream(STREAM_ID);
        smmu->createStreamPASID(STREAM_ID, PASID_0);
        // Map NO pages — any translate() on STREAM_ID triggers a translation fault
        // which, in stall mode, should enqueue a StallRecord.
    }

    void TearDown() override {
        smmu.reset();
    }

    // Configure and enable a second stream in stall mode
    void configureStream2() {
        StreamConfig stallConfig2;
        stallConfig2.translationEnabled = true;
        stallConfig2.stage1Enabled = true;
        stallConfig2.stage2Enabled = false;
        stallConfig2.faultMode = FaultMode::Stall;
        smmu->configureStream(STREAM_ID_2, stallConfig2);
        smmu->enableStream(STREAM_ID_2);
        smmu->createStreamPASID(STREAM_ID_2, PASID_0);
    }

    std::unique_ptr<SMMU> smmu;
    static constexpr StreamID STREAM_ID   = 0x100;
    static constexpr StreamID STREAM_ID_2 = 0x200;
    static constexpr PASID    PASID_0     = 0;
    static constexpr IOVA     TEST_IOVA   = 0x10000;
};

// ── §3.12.2: Baseline fault mode behavior ─────────────────────────────────

/// Stall mode not active by default: Terminate mode returns PageNotMapped, not Stalled.
TEST_F(SMMUStallResumeTest, TerminateModeReturnsFaultNotStalled) {
    // Configure a separate stream with Terminate mode
    StreamConfig terminateConfig;
    terminateConfig.translationEnabled = true;
    terminateConfig.stage1Enabled = true;
    terminateConfig.stage2Enabled = false;
    terminateConfig.faultMode = FaultMode::Terminate;
    smmu->configureStream(0x300, terminateConfig);
    smmu->enableStream(0x300);
    smmu->createStreamPASID(0x300, PASID_0);

    TranslationResult result = smmu->translate(0x300, PASID_0, TEST_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isError());
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "Terminate mode must not return Stalled";
}

// ── §3.12.2: Stall mode triggers stall on fault ───────────────────────────

/// Stall mode: translation fault returns SMMUError::Stalled.
TEST_F(SMMUStallResumeTest, StallModeReturnsStalled) {
    TranslationResult result = smmu->translate(STREAM_ID, PASID_0, TEST_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "Stall mode fault must return SMMUError::Stalled";
}

/// Stall record is enqueued after a stall fault.
TEST_F(SMMUStallResumeTest, StallRecordEnqueued) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    EXPECT_EQ(smmu->getStalledTransactionCount(), 1u)
        << "One stall record must be present after one stall fault";
}

/// Stall record carries the correct StreamID.
TEST_F(SMMUStallResumeTest, StallRecordHasCorrectStreamID) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);
    EXPECT_EQ(stalls[0].streamID, STREAM_ID)
        << "StallRecord.streamID must match the stream that faulted";
}

// ── §4.6: CMD_RESUME retire semantics ────────────────────────────────────

/// CMD_RESUME with matching STAG and StreamID retires the stall record.
TEST_F(SMMUStallResumeTest, CmdResumeMatchingRetires) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);

    CommandEntry cmd;
    cmd.type     = CommandType::RESUME;
    cmd.streamID = STREAM_ID;
    cmd.stag     = stalls[0].stag;
    cmd.action   = false;
    cmd.abort    = false;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "CMD_RESUME with matching STAG+StreamID must retire the stall record";
}

/// CMD_RESUME with correct STAG but wrong StreamID is a no-op.
TEST_F(SMMUStallResumeTest, CmdResumeWrongStreamIDIsNoop) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);

    CommandEntry cmd;
    cmd.type     = CommandType::RESUME;
    cmd.streamID = STREAM_ID + 1; // wrong StreamID
    cmd.stag     = stalls[0].stag;
    cmd.action   = false;
    cmd.abort    = false;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 1u)
        << "CMD_RESUME with wrong StreamID must leave the stall record intact";
}

/// CMD_RESUME with non-existent STAG is a no-op.
TEST_F(SMMUStallResumeTest, CmdResumeWrongStagIsNoop) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getStalledTransactionCount(), 1u);

    CommandEntry cmd;
    cmd.type     = CommandType::RESUME;
    cmd.streamID = STREAM_ID;
    cmd.stag     = 0xFFFF; // non-existent STAG
    cmd.action   = false;
    cmd.abort    = false;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 1u)
        << "CMD_RESUME with unknown STAG must leave the stall record intact";
}

/// CMD_RESUME Ac=1 (retry) retires the record.
TEST_F(SMMUStallResumeTest, CmdResumeRetryRetiresRecord) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);

    CommandEntry cmd;
    cmd.type     = CommandType::RESUME;
    cmd.streamID = STREAM_ID;
    cmd.stag     = stalls[0].stag;
    cmd.action   = true;  // Ac=1: retry
    cmd.abort    = false;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "CMD_RESUME Ac=1 (retry) must retire the stall record";
}

/// CMD_RESUME Ac=0, Ab=0 (terminate successfully) retires the record.
TEST_F(SMMUStallResumeTest, CmdResumeTerminateRetiresRecord) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);

    CommandEntry cmd;
    cmd.type     = CommandType::RESUME;
    cmd.streamID = STREAM_ID;
    cmd.stag     = stalls[0].stag;
    cmd.action   = false; // Ac=0
    cmd.abort    = false; // Ab=0: terminate successfully
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "CMD_RESUME Ac=0 Ab=0 (terminate) must retire the stall record";
}

/// CMD_RESUME Ac=0, Ab=1 (abort with bus error) retires the record.
TEST_F(SMMUStallResumeTest, CmdResumeAbortRetiresRecord) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);

    CommandEntry cmd;
    cmd.type     = CommandType::RESUME;
    cmd.streamID = STREAM_ID;
    cmd.stag     = stalls[0].stag;
    cmd.action   = false; // Ac=0
    cmd.abort    = true;  // Ab=1: abort with bus error
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "CMD_RESUME Ac=0 Ab=1 (abort) must retire the stall record";
}

// ── §4.7: CMD_STALL_TERM ─────────────────────────────────────────────────

/// CMD_STALL_TERM retires all stall records for the given StreamID.
TEST_F(SMMUStallResumeTest, CmdStallTermRetiresAllForStream) {
    // Trigger two stalls on the same stream (different IOVAs, unmapped)
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA,          AccessType::Read, SecurityState::NonSecure);
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA + 0x1000, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getStalledTransactionCount(), 2u);

    CommandEntry cmd;
    cmd.type     = CommandType::STALL_TERM;
    cmd.streamID = STREAM_ID;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "CMD_STALL_TERM must retire all stall records for the stream";
}

/// CMD_STALL_TERM does not affect stall records for other streams.
TEST_F(SMMUStallResumeTest, CmdStallTermDoesNotAffectOtherStreams) {
    configureStream2();

    // One stall on stream 1, one on stream 2
    smmu->translate(STREAM_ID,   PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(STREAM_ID_2, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getStalledTransactionCount(), 2u);

    // Terminate only stream 1
    CommandEntry cmd;
    cmd.type     = CommandType::STALL_TERM;
    cmd.streamID = STREAM_ID;
    smmu->submitCommand(cmd);
    smmu->processCommandQueue();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 1u)
        << "CMD_STALL_TERM must leave stall records for other streams intact";

    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);
    EXPECT_EQ(stalls[0].streamID, STREAM_ID_2)
        << "Remaining stall must belong to stream 2";
}

// ── §7.3 / §3.5.3: Stall event bit ──────────────────────────────────────

/// After triggering a stall, the event queue contains an event with stall=true.
TEST_F(SMMUStallResumeTest, StallFaultGeneratesEventWithStallTrue) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto events = smmu->getEventQueue();
    ASSERT_FALSE(events.empty()) << "At least one event must be in the queue";
    bool foundStallEvent = false;
    for (const auto& ev : events) {
        if (ev.stall) {
            foundStallEvent = true;
            break;
        }
    }
    EXPECT_TRUE(foundStallEvent)
        << "A stall fault must produce an event with stall=true (§7.3, §3.5.3)";
}

// ── abortStalledTransaction() ─────────────────────────────────────────────

/// abortStalledTransaction() returns true and reduces count for a valid STAG.
TEST_F(SMMUStallResumeTest, AbortStalledTransactionSuccess) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    auto stalls = smmu->getStalledTransactions();
    ASSERT_EQ(stalls.size(), 1u);

    bool removed = smmu->abortStalledTransaction(stalls[0].stag);
    EXPECT_TRUE(removed) << "abortStalledTransaction must return true for a known STAG";
    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "Count must drop to zero after abort";
}

/// abortStalledTransaction() returns false for an unknown STAG.
TEST_F(SMMUStallResumeTest, AbortStalledTransactionUnknownStag) {
    bool removed = smmu->abortStalledTransaction(0xABCD);
    EXPECT_FALSE(removed)
        << "abortStalledTransaction must return false for an unknown STAG";
}

// ── reset() clears stall queue ────────────────────────────────────────────

/// reset() clears all stall records.
TEST_F(SMMUStallResumeTest, ResetClearsStallQueue) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getStalledTransactionCount(), 1u);

    smmu->reset();

    EXPECT_EQ(smmu->getStalledTransactionCount(), 0u)
        << "reset() must clear the stall queue";
}

// ── Multiple stalls get unique STAGs ─────────────────────────────────────

/// Three consecutive stalls on the same stream produce three distinct STAGs.
TEST_F(SMMUStallResumeTest, MultipleStallsGetUniqueSTAGs) {
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA,          AccessType::Read, SecurityState::NonSecure);
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA + 0x1000, AccessType::Read, SecurityState::NonSecure);
    smmu->translate(STREAM_ID, PASID_0, TEST_IOVA + 0x2000, AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(smmu->getStalledTransactionCount(), 3u);

    auto stalls = smmu->getStalledTransactions();
    std::set<uint16_t> stags;
    for (const auto& r : stalls) {
        stags.insert(r.stag);
    }
    EXPECT_EQ(stags.size(), 3u)
        << "Each stall fault must produce a unique STAG";
}

} // namespace test
} // namespace smmu
