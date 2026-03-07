// BUG-CPP-DBGR-12: No faultTypeToSMMUError() for C_BAD_STREAMID (§7.3.3)
// TDD: failing test written BEFORE the fix.
// When a stream ID is not found in the stream table (streamMap miss),
// translate() currently returns SMMUError::StreamNotConfigured.
// The spec (§7.3.3) says C_BAD_STREAMID corresponds to an invalid stream ID,
// which should map to SMMUError::InvalidStreamID.
//
// Note: the strtab range check (log2size < 32) already returns InvalidStreamID.
// The streamMap miss path returns StreamNotConfigured — this is the bug.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

// -----------------------------------------------------------------------
// Stream not configured (not in streamMap): must return InvalidStreamID
// (not StreamNotConfigured) and generate C_BAD_STREAMID event
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, UnconfiguredStream_ReturnsInvalidStreamID) {
    SMMU smmu;
    smmu.enable();
    // Set a large enough stream table to avoid the strtab range check
    smmu.setStrtabLog2Size(16); // supports streams 0..65535

    // Stream 0x1234 is NOT configured
    TranslationResult result = smmu.translate(0x1234, 0, 0x1000ULL,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "Unconfigured stream must return an error";
    EXPECT_EQ(result.getError(), SMMUError::InvalidStreamID)
        << "Unconfigured stream (streamMap miss) must return SMMUError::InvalidStreamID "
           "(§7.3.3 C_BAD_STREAMID), not StreamNotConfigured";
}

// -----------------------------------------------------------------------
// C_BAD_STREAMID event must be generated for the streamMap miss case
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, UnconfiguredStream_GeneratesCBadStreamIDEvent) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(16);

    smmu.translate(0x5678, 0, 0x2000ULL, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu.getEventQueue();
    bool foundCBadStreamId = false;
    for (const auto& evt : events) {
        if (evt.type == EventType::C_BAD_STREAMID && evt.streamID == 0x5678u) {
            foundCBadStreamId = true;
            break;
        }
    }
    EXPECT_TRUE(foundCBadStreamId)
        << "C_BAD_STREAMID event must be generated for unconfigured stream (§7.3.3)";
}

// -----------------------------------------------------------------------
// Stream out of range (strtab range check): must return InvalidStreamID
// This was already working; verify it still works after the fix
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, OutOfRangeStream_ReturnsInvalidStreamID) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(4); // only streams 0..15 valid

    // Stream 0x20 is out of range
    TranslationResult result = smmu.translate(0x20, 0, 0x1000ULL,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidStreamID)
        << "Out-of-range stream must return InvalidStreamID";
}

// -----------------------------------------------------------------------
// Configured stream must still work correctly after the fix (regression)
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, ConfiguredStream_TranslatesCorrectly) {
    SMMU smmu;
    smmu.enable();
    smmu.setStrtabLog2Size(16);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    smmu.configureStream(0x0001, cfg);
    smmu.enableStream(0x0001);
    smmu.createStreamPASID(0x0001, 0);
    smmu.mapPage(0x0001, 0, 0x1000ULL, 0xDEAD0000ULL,
                 PagePermissions(true, false, false), SecurityState::NonSecure);

    TranslationResult result = smmu.translate(0x0001, 0, 0x1000ULL,
                                             AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "Configured stream must translate successfully";
    EXPECT_EQ(result.getValue().physicalAddress, 0xDEAD0000ULL);
}

} // namespace test
} // namespace smmu
