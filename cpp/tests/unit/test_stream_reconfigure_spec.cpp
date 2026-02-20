// ARM SMMU v3 FINDING-L-06: Stream Reconfiguration Without Invalidation
//
// TDD spec tests verifying that SMMU::configureStream rejects attempts to
// reconfigure an already-configured stream without prior invalidation.
//
// ARM §3.11 requires a CMD_CFGI_STE + CMD_SYNC sequence before changing a
// stream table entry.  Per the recommendation, we follow the Rust model:
// return StreamAlreadyConfigured when configureStream is called on an
// already-configured stream.
//
// Correct reconfiguration sequence:
//   1. removeStream(id)          — remove existing entry
//   2. configureStream(id, cfg)  — install new entry
//
// These tests are RED before the fix and GREEN after.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Test constants ────────────────────────────────────────────────────────
static const StreamID REC_STREAM_ID = 0x10;
static const PASID    REC_PASID     = 0;

// ── Fixture ───────────────────────────────────────────────────────────────
class StreamReconfigureSpec : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());

        // Configure a basic stage-1 stream
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        ASSERT_TRUE(smmu->configureStream(REC_STREAM_ID, cfg).isOk());
    }

    void TearDown() override { smmu.reset(); }

    std::unique_ptr<SMMU> smmu;
};

// ── Tests ─────────────────────────────────────────────────────────────────

// §3.11: Calling configureStream on an already-configured stream must fail.
TEST_F(StreamReconfigureSpec, ConfigureExistingStream_ReturnsError) {
    StreamConfig newCfg;
    newCfg.translationEnabled = false;
    newCfg.stage1Enabled      = false;
    newCfg.stage2Enabled      = false;
    newCfg.faultMode          = FaultMode::Stall;

    VoidResult result = smmu->configureStream(REC_STREAM_ID, newCfg);

    EXPECT_TRUE(result.isError())
        << "configureStream on existing stream must return an error per ARM §3.11";
}

// The error code must be StreamAlreadyConfigured (not a generic error).
TEST_F(StreamReconfigureSpec, ConfigureExistingStream_ErrorCode) {
    StreamConfig newCfg;
    newCfg.translationEnabled = true;
    newCfg.stage1Enabled      = true;
    newCfg.stage2Enabled      = true;
    newCfg.faultMode          = FaultMode::Terminate;

    VoidResult result = smmu->configureStream(REC_STREAM_ID, newCfg);

    ASSERT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::StreamAlreadyConfigured)
        << "error code must be StreamAlreadyConfigured";
}

// Correct reconfiguration path: removeStream + configureStream must succeed.
TEST_F(StreamReconfigureSpec, RemoveThenConfigure_Succeeds) {
    ASSERT_TRUE(smmu->removeStream(REC_STREAM_ID).isOk());

    StreamConfig newCfg;
    newCfg.translationEnabled = false;
    newCfg.stage1Enabled      = false;
    newCfg.stage2Enabled      = false;
    newCfg.faultMode          = FaultMode::Stall;

    VoidResult result = smmu->configureStream(REC_STREAM_ID, newCfg);
    EXPECT_TRUE(result.isOk())
        << "configureStream after removeStream must succeed";
}

// A new (never-configured) stream must still be configurable.
TEST_F(StreamReconfigureSpec, FreshStream_ConfigureSucceeds) {
    const StreamID freshID = REC_STREAM_ID + 1;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;

    VoidResult result = smmu->configureStream(freshID, cfg);
    EXPECT_TRUE(result.isOk())
        << "configureStream on a new stream ID must succeed";
}

// After removeStream the stream is no longer configured.
TEST_F(StreamReconfigureSpec, AfterRemove_StreamNotConfigured) {
    ASSERT_TRUE(smmu->removeStream(REC_STREAM_ID).isOk());

    auto r = smmu->isStreamConfigured(REC_STREAM_ID);
    ASSERT_TRUE(r.isOk());
    EXPECT_FALSE(r.getValue())
        << "stream must not appear configured after removeStream";
}

} // namespace test
} // namespace smmu
