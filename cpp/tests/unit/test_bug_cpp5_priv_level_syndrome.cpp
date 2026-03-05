// BUG-CPP-5: Spec violation — determinePrivilegeLevel() infers wrong privilege
//            for Secure and NonSecure streams
//
// ARM IHI0070G.b §13.1.3:
//   "If the privilege level of the transaction cannot be determined from the
//    transaction attributes, it is treated as Unprivileged (EL0)."
//
// The current implementation maps:
//   SecurityState::Secure    (data)    → PrivilegeLevel::EL1   (WRONG)
//   SecurityState::NonSecure (data)    → PrivilegeLevel::EL1   (WRONG)
//
// §13.1.3 default is Unprivileged (EL0) for ALL access types when privilege
// cannot be determined — the spec does not grant Secure/NonSecure a "data→EL1"
// shortcut.  A Secure-EL2 hypervisor DMA would receive a wrong EL1 privilege
// bit in the fault syndrome, violating §7.3 fault syndrome generation rules.
//
// The Root→EL3 and Realm→EL2 mappings are correct and must remain unchanged.
//
// Fix: Change the Secure/NonSecure fallback to return PrivilegeLevel::EL0 for
// all access types (both data and execute).
//
// determinePrivilegeLevel() is called from recordComprehensiveFault(), which is
// called by performBothStagesTranslation().  We use two-stage streams (where
// the PASID used is NOT PASID_0, so stage-1 AS is separate from stage-2) and
// leave the stage-1 PASID address space unmapped, forcing the translation
// through the PASID-not-found path in performBothStagesTranslation which calls
// recordComprehensiveFault().
//
// TDD: Tests MUST FAIL before the fix (syndrome shows EL1 for NonSecure/Secure
// data accesses) and MUST PASS after (syndrome shows EL0).
//
// The test structure is:
//   - Create a two-stage stream (stage1Enabled=true, stage2Enabled=true).
//   - Create PASID_0 (stage-2 anchor).
//   - Translate with PASID_UNKNOWN (never created) → PASIDNotFound via
//     getPASIDAddressSpace() → recordComprehensiveFault() → valid syndrome.
//   - Inspect FaultSyndrome.privilegeLevel.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Test constants ────────────────────────────────────────────────────────

static constexpr StreamID CPP5_STREAM_BASE  = 0xE0;
static constexpr PASID    CPP5_PASID_0      = 0;
static constexpr PASID    CPP5_PASID_MISS   = 99; // Never configured — triggers PASIDNotFound
static constexpr IOVA     CPP5_IOVA         = 0x7000'0000ULL;

// ── Helper: build enabled SMMU ────────────────────────────────────────────

static std::unique_ptr<SMMU> makeEnabled() {
    auto s = std::unique_ptr<SMMU>(new SMMU());
    s->enable();
    return s;
}

// ── Helper: set up a two-stage terminate stream and trigger PASIDNotFound ─
// Returns the FaultSyndrome recorded for the given streamID.
// A two-stage stream with no address space for CPP5_PASID_MISS causes
// performBothStagesTranslation() to call recordComprehensiveFault(), which
// calls determinePrivilegeLevel().  This is the code path under test.

static FaultSyndrome triggerTwoStageMissFault(SMMU& smmu, StreamID sid,
                                              AccessType at, SecurityState ss) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    if (!smmu.configureStream(sid, cfg).isOk()) return FaultSyndrome{};
    if (!smmu.enableStream(sid).isOk())         return FaultSyndrome{};
    // PASID_0 is the stage-2 anchor required for two-stage streams.
    if (!smmu.createStreamPASID(sid, CPP5_PASID_0).isOk()) return FaultSyndrome{};
    // CPP5_PASID_MISS is intentionally NOT created — getPASIDAddressSpace(MISS)
    // returns nullptr → recordComprehensiveFault() with FaultType::TranslationFault.
    smmu.translate(sid, CPP5_PASID_MISS, CPP5_IOVA, at, ss);

    auto evResult = smmu.getEvents();
    if (!evResult.isOk()) return FaultSyndrome{};
    for (const FaultRecord& rec : evResult.getValue()) {
        if (rec.syndrome.validSyndrome && rec.streamID == sid) {
            return rec.syndrome;
        }
    }
    return FaultSyndrome{};
}

// ── BUG-CPP-5a: NonSecure data access must record EL0, not EL1 ───────────
//
// §13.1.3: privilege is unknown for a generic DMA → default = EL0.
//
// FAILS before fix: privilegeLevel == EL1 (buggy heuristic).
// PASSES after fix: privilegeLevel == EL0 (spec default).

TEST(PrivLevelSyndrome, NonSecure_DataAccess_MustBeEL0) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE,
                                                AccessType::Read,
                                                SecurityState::NonSecure);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5: No valid syndrome recorded for NonSecure data fault — "
           "check that two-stage stream triggers recordComprehensiveFault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL0)
        << "§13.1.3 / BUG-CPP-5: NonSecure data access — privilege is unknown, "
           "spec default is EL0 (Unprivileged); "
           "buggy code returns EL1 for the Secure/NonSecure fallback in "
           "determinePrivilegeLevel()";
}

// ── BUG-CPP-5b: Secure data access must record EL0, not EL1 ─────────────
//
// §13.1.3: Secure world does NOT imply EL1; privilege unknown → EL0.
//
// FAILS before fix: privilegeLevel == EL1.
// PASSES after fix: privilegeLevel == EL0.

TEST(PrivLevelSyndrome, Secure_DataAccess_MustBeEL0) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE + 1,
                                                AccessType::Read,
                                                SecurityState::Secure);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5: No valid syndrome recorded for Secure data fault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL0)
        << "§13.1.3 / BUG-CPP-5: Secure data access — privilege is unknown, "
           "spec default is EL0 (Unprivileged); "
           "buggy code returns EL1 for the Secure fallback in "
           "determinePrivilegeLevel()";
}

// ── BUG-CPP-5c: NonSecure write access must also record EL0 ──────────────

TEST(PrivLevelSyndrome, NonSecure_WriteAccess_MustBeEL0) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE + 2,
                                                AccessType::Write,
                                                SecurityState::NonSecure);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5: No valid syndrome for NonSecure write fault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL0)
        << "§13.1.3 / BUG-CPP-5: NonSecure write — privilege unknown → EL0; "
           "buggy code returns EL1";
}

// ── BUG-CPP-5d: NonSecure execute access must also record EL0 ────────────
//
// The buggy code returns EL0 for execute in the Secure/NonSecure branch, so
// this was already correct before the fix. After the fix (returning EL0 for
// all access types), it must still be EL0 — regression guard.

TEST(PrivLevelSyndrome, NonSecure_Execute_StillEL0_Regression) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE + 3,
                                                AccessType::Execute,
                                                SecurityState::NonSecure);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5 regression: No valid syndrome for NonSecure execute fault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL0)
        << "§13.1.3 regression: NonSecure execute was already EL0; "
           "the fix must not change this";
}

// ── BUG-CPP-5e: Root→EL3 mapping must be unchanged ───────────────────────
//
// Root security state canonically implies EL3 (monitor / firmware).
// This mapping is correct and must remain EL3 after the fix.

TEST(PrivLevelSyndrome, Root_MustBeEL3_Unchanged) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE + 4,
                                                AccessType::Read,
                                                SecurityState::Root);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5 regression: No valid syndrome for Root data fault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL3)
        << "§13.1.3 regression: Root state must still map to EL3; "
           "the fix must only change Secure/NonSecure fallback";
}

// ── BUG-CPP-5f: Realm→EL2 mapping must be unchanged ─────────────────────
//
// Realm security state (RME) canonically implies EL2.
// This mapping is correct and must remain EL2 after the fix.

TEST(PrivLevelSyndrome, Realm_MustBeEL2_Unchanged) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE + 5,
                                                AccessType::Read,
                                                SecurityState::Realm);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5 regression: No valid syndrome for Realm data fault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL2)
        << "§13.1.3 regression: Realm state must still map to EL2; "
           "the fix must only change Secure/NonSecure fallback";
}

// ── BUG-CPP-5g: Secure write also must be EL0 ────────────────────────────

TEST(PrivLevelSyndrome, Secure_WriteAccess_MustBeEL0) {
    auto smmu = makeEnabled();
    FaultSyndrome syn = triggerTwoStageMissFault(*smmu, CPP5_STREAM_BASE + 6,
                                                AccessType::Write,
                                                SecurityState::Secure);

    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-5: No valid syndrome for Secure write fault";

    EXPECT_EQ(syn.privilegeLevel, PrivilegeLevel::EL0)
        << "§13.1.3 / BUG-CPP-5: Secure write — privilege unknown → EL0; "
           "buggy code returns EL1";
}

}  // namespace test
}  // namespace smmu
