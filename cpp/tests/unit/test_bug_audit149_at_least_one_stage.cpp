// TDD tests for BUG-AUDIT-149-CPP
//
// ARM §3.3 (line 1429): "An SMMU must support at least one stage of translation."
// setS1PSupported(false) and setS2PSupported(false) must each refuse to clear
// the last supported stage.  If both would be false, the call is silently ignored.
//
// Tests 1 and 2 FAIL before the fix (RED), all 4 PASS after the fix (GREEN).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <cstdint>

using namespace smmu;

namespace {

// Both stages supported by default — just verify the helper works.
class BugAudit149AtLeastOneStageTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Both S1P and S2P default to true.
        ASSERT_TRUE(smmu_.isS1PSupported()) << "Precondition: S1P must be true at construction.";
        ASSERT_TRUE(smmu_.isS2PSupported()) << "Precondition: S2P must be true at construction.";
    }

    SMMU smmu_;
};

} // anonymous namespace

// ============================================================================
// Test 1: Disabling S1 when S2 is already disabled must be a no-op
// (the call is silently ignored to preserve the at-least-one-stage invariant)
// ============================================================================
//
// BEFORE FIX: setS1PSupported(false) stores false unconditionally → S1P becomes
//             false → zero supported stages → FAILS.
// AFTER FIX:  call is rejected because S2P is already false → S1P stays true.
TEST_F(BugAudit149AtLeastOneStageTest, DisableS1WhenS2AlreadyDisabledIsNoop) {
    // First, disable S2 while S1 is still true — this should succeed.
    smmu_.setS2PSupported(false);
    ASSERT_FALSE(smmu_.isS2PSupported()) << "Precondition: S2P should be disabled now.";
    ASSERT_TRUE(smmu_.isS1PSupported())  << "Precondition: S1P must still be true.";

    // Now attempt to disable S1 — this must be a no-op (S2 is already false).
    smmu_.setS1PSupported(false);

    // §3.3 line 1429: at least one stage must always be supported.
    EXPECT_TRUE(smmu_.isS1PSupported())
        << "BUG-AUDIT-149-CPP: setS1PSupported(false) must be ignored when "
           "S2P is already false (§3.3 line 1429: at least one stage required).";
}

// ============================================================================
// Test 2: Disabling S2 when S1 is already disabled must be a no-op
// ============================================================================
//
// BEFORE FIX: setS2PSupported(false) stores false unconditionally → zero stages.
// AFTER FIX:  call is rejected because S1P is already false → S2P stays true.
TEST_F(BugAudit149AtLeastOneStageTest, DisableS2WhenS1AlreadyDisabledIsNoop) {
    // First, disable S1 while S2 is still true.
    smmu_.setS1PSupported(false);
    ASSERT_FALSE(smmu_.isS1PSupported()) << "Precondition: S1P should be disabled now.";
    ASSERT_TRUE(smmu_.isS2PSupported())  << "Precondition: S2P must still be true.";

    // Now attempt to disable S2 — this must be a no-op (S1 is already false).
    smmu_.setS2PSupported(false);

    // §3.3 line 1429: at least one stage must always be supported.
    EXPECT_TRUE(smmu_.isS2PSupported())
        << "BUG-AUDIT-149-CPP: setS2PSupported(false) must be ignored when "
           "S1P is already false (§3.3 line 1429: at least one stage required).";
}

// ============================================================================
// Test 3: Disabling S1 when S2 is still enabled must succeed
// ============================================================================
//
// The guard must NOT block this case — S2 is still enabled, so the invariant
// is preserved.
TEST_F(BugAudit149AtLeastOneStageTest, DisableS1WhenS2EnabledSucceeds) {
    // S2P is true (default). Disabling S1 is allowed.
    smmu_.setS1PSupported(false);

    EXPECT_FALSE(smmu_.isS1PSupported())
        << "BUG-AUDIT-149-CPP: setS1PSupported(false) must succeed when S2P is true.";
    EXPECT_TRUE(smmu_.isS2PSupported())
        << "S2P must remain true (not affected by S1P change).";
}

// ============================================================================
// Test 4: Disabling S2 when S1 is still enabled must succeed
// ============================================================================
//
// The guard must NOT block this case — S1 is still enabled.
TEST_F(BugAudit149AtLeastOneStageTest, DisableS2WhenS1EnabledSucceeds) {
    // S1P is true (default). Disabling S2 is allowed.
    smmu_.setS2PSupported(false);

    EXPECT_FALSE(smmu_.isS2PSupported())
        << "BUG-AUDIT-149-CPP: setS2PSupported(false) must succeed when S1P is true.";
    EXPECT_TRUE(smmu_.isS1PSupported())
        << "S1P must remain true (not affected by S2P change).";
}
