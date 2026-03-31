// SPEC-20 update: correct expected behavior for in-bounds unconfigured streams.
//
// ARM IHI0070G.b §7.3.3 C_BAD_STREAMID: StreamID OUTSIDE the configured table range.
// ARM IHI0070G.b §7.3.5 C_BAD_STE: StreamID WITHIN range but STE.V=0 (not configured).
//
// When a streamID passes the LOG2SIZE bounds check but is not in streamMap (STE.V=0
// equivalent), the correct event is C_BAD_STE and the correct error is
// SMMUError::StreamNotConfigured.  C_BAD_STREAMID and SMMUError::InvalidStreamID are
// reserved for StreamIDs that are outside the LOG2SIZE range.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

// -----------------------------------------------------------------------
// Stream not configured (not in streamMap, but within LOG2SIZE range):
// per SPEC-20 (§7.3.5) must return StreamNotConfigured and emit C_BAD_STE.
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, UnconfiguredStream_ReturnsStreamNotConfigured) {
    SMMU smmu;
    // BUG-AUDIT-63: STRTAB_BASE_CFG is RO when SMMUEN=1; set log2size before enable().
    smmu.setStrtabLog2Size(16); // supports streams 0..65535; 0x1234 is in-range
    smmu.enable();

    // Stream 0x1234 is within range but NOT configured (STE.V=0 equivalent)
    TranslationResult result = smmu.translate(0x1234, 0, 0x1000ULL,
                                             AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError()) << "Unconfigured stream must return an error";
    EXPECT_EQ(result.getError(), SMMUError::StreamNotConfigured)
        << "In-bounds unconfigured stream (STE.V=0) must return StreamNotConfigured "
           "(§7.3.5 C_BAD_STE), not InvalidStreamID";
}

// -----------------------------------------------------------------------
// C_BAD_STE event must be generated for the streamMap miss (in-range) case.
// C_BAD_STE is always recorded regardless of RECINVSID (§6.3.12).
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, UnconfiguredStream_GeneratesCBadSteEvent) {
    SMMU smmu;
    // BUG-AUDIT-63: STRTAB_BASE_CFG is RO when SMMUEN=1; set log2size before enable().
    smmu.setStrtabLog2Size(16);
    smmu.enable();
    // RECINVSID=0 intentionally — C_BAD_STE must still be recorded (§6.3.12)

    smmu.translate(0x5678, 0, 0x2000ULL, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu.getEventQueue();
    bool foundCBadSte = false;
    for (const auto& evt : events) {
        if (evt.type == EventType::C_BAD_STE && evt.streamID == 0x5678u) {
            foundCBadSte = true;
            break;
        }
    }
    EXPECT_TRUE(foundCBadSte)
        << "C_BAD_STE event must be generated for in-bounds unconfigured stream (§7.3.5)";
}

// -----------------------------------------------------------------------
// Stream out of range (strtab range check): must return InvalidStreamID
// This was already working; verify it still works after the fix
// -----------------------------------------------------------------------
TEST(BadStreamIdSpec, OutOfRangeStream_ReturnsInvalidStreamID) {
    SMMU smmu;
    // BUG-AUDIT-63: STRTAB_BASE_CFG is RO when SMMUEN=1; set log2size before enable().
    smmu.setStrtabLog2Size(4); // only streams 0..15 valid
    smmu.enable();

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
    // BUG-AUDIT-63: STRTAB_BASE_CFG is RO when SMMUEN=1; set log2size before enable().
    smmu.setStrtabLog2Size(16);
    smmu.enable();

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
