// BUG-CPP-6 — STRW TLB re-check: switch default swallows already-privileged
//             AccessType variants without comment (clarity / latent risk)
//
// ARM IHI0070G.b §13.4.1 / §3.3.4 — STRW-aware TLB fast path
// ──────────────────────────────────────────────────────────────
//
// When a stream is configured with STRW=EL2 or STRW=EL3 the SMMU treats ALL
// transactions as privileged.  translate() implements a two-stage TLB
// fast-path:
//
//   Stage A (line ~203):  Lookup with raw accessType.
//                         validateAccessPermissions checks !privilegedOnly for
//                         unprivileged variants.  A privileged-only page will
//                         therefore fail here for Read/Write/Execute/ReadWrite.
//
//   Stage B (line ~288):  If STRW suppresses privilege, promote the raw access
//                         type to its *Privileged counterpart and retry the TLB
//                         with validateAccessPermissions(..., ReadPrivileged)
//                         which does NOT check !privilegedOnly.  This is the
//                         STRW re-check optimisation.
//
// The switch inside Stage B only explicitly handles the four base (unprivileged)
// variants.  The already-privileged variants (ReadPrivileged etc.) fall through
// to `default: break`, leaving effectiveAccessType == accessType, so the
// condition (effectiveAccessType != accessType) is false and Stage B is skipped.
//
// THIS IS CORRECT BEHAVIOUR: for pre-elevated access types Stage A already
// uses validateAccessPermissions(..., ReadPrivileged) which does NOT gate on
// !privilegedOnly, so Stage A hits immediately on a warm TLB.  Stage B is
// therefore unnecessary and safely skipped.
//
// The only issue is clarity: a silent `default: break` with no comment leaves
// the reader uncertain whether the pre-elevated cases were forgotten.  The fix
// adds explicit case labels with an explanatory comment.
//
// ── TDD EXPECTATION ──────────────────────────────────────────────────────────
//
// Because the behaviour is already correct these tests PASS both before and
// after the fix.  Running them before the clarity fix confirms the invariant;
// running them after confirms the fix did not regress anything.
//
// Test scenarios:
//
//   T1: STRW=EL2 stream, privileged-only page.  First call with Read:
//       Stage A misses (privilegedOnly gate); Stage B promotes Read →
//       ReadPrivileged and hits.  Correct PA is returned.
//
//   T2: Same stream.  Second call with Read (warm TLB):
//       Same path as T1 — still hits via Stage B.
//
//   T3: Same stream.  Call with ReadPrivileged:
//       Stage A lookup uses ReadPrivileged; validateAccessPermissions does NOT
//       check privilegedOnly → hit immediately.  Stage B never entered.
//
//   T4: ReadPrivileged result == Read result (same PA).
//
//   T5: STRW=EL1_EL0 stream (privilege checks active), privileged-only page.
//       Read → permission denied (no re-check promotion).
//       ReadPrivileged → permission denied (Stage A passes permission gate but
//       slow path still checks page-table permissions; page is privilegedOnly so
//       the EL1_EL0 stream does not suppress the privilege check in the slow
//       path either).  Specifically: translate() with ReadPrivileged but
//       STRW=EL1_EL0 → EL1_EL0 does not promote, so Stage A fails (no entry
//       yet) and slow path runs; the slow path re-validates permissions using
//       the raw accessType as well, so only a correctly-permitted page succeeds.
//
//   T6: STRW=EL3 stream mirrors EL2 behaviour (EL3 also suppresses privilege).
//
//   T7: Write and ReadWrite variants on STRW=EL2 — confirm promotion and hit.
//
//   T8: Execute variant on STRW=EL2 — confirm promotion and hit.
//
//   T9: Already-elevated WritePrivileged on STRW=EL2 — Stage A hits directly.
//
//  T10: Already-elevated ExecutePrivileged on STRW=EL2 — Stage A hits directly.
//
// ── Root-cause summary ───────────────────────────────────────────────────────
//
// No functional defect.  The `default: break` is a code clarity issue.
// Fix: add explicit case labels for ReadPrivileged / WritePrivileged /
//      ExecutePrivileged / ReadWritePrivileged with a comment explaining that
//      these are already at the correct privilege level so no promotion is
//      needed, and Stage A will already have succeeded on a warm TLB.
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Constants ────────────────────────────────────────────────────────────────

static constexpr StreamID CPP6_SID_EL2      = 0xC0;
static constexpr StreamID CPP6_SID_EL3      = 0xC1;
static constexpr StreamID CPP6_SID_EL1      = 0xC2;
static constexpr PASID    CPP6_PASID        = 0;
static constexpr IOVA     CPP6_IOVA         = 0x4000'0000ULL;
static constexpr PA       CPP6_PA           = 0x8000'0000ULL;

// ── Helpers ──────────────────────────────────────────────────────────────────

static std::unique_ptr<SMMU> makeEnabledSMMU() {
    auto smmu = std::unique_ptr<SMMU>(new SMMU());
    smmu->enable();
    return smmu;
}

// Set up a single-stage stream with the given STRW and map a privileged-only
// page.  Returns false on any configuration error.
static bool setupStream(SMMU& smmu, StreamID sid, StreamWorld strw,
                        PagePermissions perms) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = strw;

    if (!smmu.configureStream(sid, cfg).isOk()) return false;
    if (!smmu.enableStream(sid).isOk())         return false;
    if (!smmu.createStreamPASID(sid, CPP6_PASID).isOk()) return false;
    if (!smmu.mapPage(sid, CPP6_PASID, CPP6_IOVA, CPP6_PA, perms).isOk()) return false;
    return true;
}

// privileged-only read+write page
static PagePermissions privOnlyRW()      { return PagePermissions(true, true, false, true);  }
// privileged-only execute page
static PagePermissions privOnlyExec()    { return PagePermissions(false, false, true, true);  }
// normal (non-privileged) read+write page
static PagePermissions normalRW()        { return PagePermissions(true, true, false, false); }

// ── T1: STRW=EL2, unprivileged Read on warm TLB hit via Stage-B re-check ────
//
// First translate call with Read: Stage A fails (privilegedOnly gate);
// Stage B promotes Read → ReadPrivileged and hits the TLB → correct PA.

TEST(BugCpp6StageB, T1_ReadUnpriv_StageA_Miss_StageB_Hit_EL2) {
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, CPP6_SID_EL2, StreamWorld::EL2, privOnlyRW()));

    // Cold call — slow path runs and populates TLB.
    TranslationResult r1 = smmu->translate(CPP6_SID_EL2, CPP6_PASID, CPP6_IOVA,
                                           AccessType::Read,
                                           SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk())
        << "T1: STRW=EL2 stream with privileged-only page — Read must succeed "
           "(STRW suppresses privilege check per ARM §13.4.1)";

    // Warm call — TLB is populated.  Stage A fails (unprivileged Read vs
    // privilegedOnly entry), Stage B promotes to ReadPrivileged and hits.
    TranslationResult r2 = smmu->translate(CPP6_SID_EL2, CPP6_PASID, CPP6_IOVA,
                                           AccessType::Read,
                                           SecurityState::NonSecure);
    ASSERT_TRUE(r2.isOk())
        << "T1: Warm Read on STRW=EL2 stream must hit via Stage-B re-check";

    // Both calls must return the same PA.
    EXPECT_EQ(r1.getValue().physicalAddress, r2.getValue().physicalAddress)
        << "T1: Cold and warm Read must map to the same PA";
}

// ── T2: STRW=EL2, ReadPrivileged directly — Stage A hits on warm TLB ────────
//
// When the caller already supplies ReadPrivileged, Stage A calls
// validateAccessPermissions(..., ReadPrivileged) which does NOT gate on
// !privilegedOnly, so it hits immediately.  Stage B (`effectiveAccessType !=
// accessType`) evaluates to false and is skipped entirely.
//
// This is the scenario in which the silent `default: break` is reached in the
// original code: effectiveAccessType stays == ReadPrivileged == accessType, so
// the condition is false.  The test verifies that the result is still correct.

TEST(BugCpp6StageB, T2_ReadPrivileged_StageA_Hits_Directly_EL2) {
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, CPP6_SID_EL2, StreamWorld::EL2, privOnlyRW()));

    // Populate TLB with a Read call first so the entry exists.
    smmu->translate(CPP6_SID_EL2, CPP6_PASID, CPP6_IOVA,
                    AccessType::Read,
                    SecurityState::NonSecure);

    // Now call with ReadPrivileged: Stage A should hit directly.
    TranslationResult rPriv = smmu->translate(CPP6_SID_EL2, CPP6_PASID, CPP6_IOVA,
                                              AccessType::ReadPrivileged,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(rPriv.isOk())
        << "T2: ReadPrivileged on warm STRW=EL2 TLB must hit at Stage A directly "
           "(Stage B skipped because effectiveAccessType == accessType)";
}

// ── T3: Read and ReadPrivileged return the same PA ───────────────────────────

TEST(BugCpp6StageB, T3_Read_And_ReadPrivileged_Return_Same_PA_EL2) {
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, CPP6_SID_EL2, StreamWorld::EL2, privOnlyRW()));

    TranslationResult rRead = smmu->translate(CPP6_SID_EL2, CPP6_PASID, CPP6_IOVA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    TranslationResult rReadPriv = smmu->translate(CPP6_SID_EL2, CPP6_PASID, CPP6_IOVA,
                                                  AccessType::ReadPrivileged,
                                                  SecurityState::NonSecure);

    ASSERT_TRUE(rRead.isOk())     << "T3: Read must succeed on STRW=EL2 stream";
    ASSERT_TRUE(rReadPriv.isOk()) << "T3: ReadPrivileged must succeed on STRW=EL2 stream";

    EXPECT_EQ(rRead.getValue().physicalAddress,
              rReadPriv.getValue().physicalAddress)
        << "T3: Read and ReadPrivileged must resolve to the same PA for the same IOVA";
}

// ── T4: STRW=EL1_EL0 does NOT suppress privilege — Read on privOnly fails ───
//
// With STRW=EL1_EL0 Stage B promotion is never applied (only EL2 / EL3 trigger
// it).  A Read on a privileged-only page must be rejected by the slow path.

TEST(BugCpp6StageB, T4_EL1EL0_Read_On_PrivPage_Denied) {
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, CPP6_SID_EL1, StreamWorld::EL1_EL0, privOnlyRW()));

    TranslationResult r = smmu->translate(CPP6_SID_EL1, CPP6_PASID, CPP6_IOVA,
                                          AccessType::Read,
                                          SecurityState::NonSecure);
    EXPECT_FALSE(r.isOk())
        << "T4: Read on a privileged-only page with STRW=EL1_EL0 must be denied "
           "(privilege checks are NOT suppressed for EL1_EL0)";
}

// ── T5: STRW=EL3 mirrors EL2 — Read on privOnly succeeds via Stage-B ────────

TEST(BugCpp6StageB, T5_EL3_Read_On_PrivPage_Succeeds) {
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, CPP6_SID_EL3, StreamWorld::EL3, privOnlyRW()));

    TranslationResult r = smmu->translate(CPP6_SID_EL3, CPP6_PASID, CPP6_IOVA,
                                          AccessType::Read,
                                          SecurityState::NonSecure);
    ASSERT_TRUE(r.isOk())
        << "T5: STRW=EL3 must suppress privilege checks just like EL2 "
           "(ARM §13.4.1: both EL2 and EL3 suppress privilege)";
}

// ── T6: Write variant on STRW=EL2 succeeds via Stage-B re-check ─────────────

TEST(BugCpp6StageB, T6_Write_StageBRecheck_EL2) {
    // Use a unique stream ID to avoid state from other tests.
    static constexpr StreamID SID = 0xC3;
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, SID, StreamWorld::EL2, privOnlyRW()));

    // Cold call with Write.
    TranslationResult rCold = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                              AccessType::Write,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(rCold.isOk())
        << "T6: Write on STRW=EL2 privOnly page must succeed (cold call)";

    // Warm call — hits via Stage-B re-check (Write → WritePrivileged).
    TranslationResult rWarm = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                              AccessType::Write,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(rWarm.isOk())
        << "T6: Write on STRW=EL2 privOnly page must succeed (warm TLB, Stage-B)";
}

// ── T7: WritePrivileged on STRW=EL2 hits Stage A directly (no Stage-B) ──────
//
// This exercises the `default: break` path in the original switch.
// effectiveAccessType stays WritePrivileged == accessType → Stage B skipped.
// Stage A already used WritePrivileged which ignores !privilegedOnly → hits.

TEST(BugCpp6StageB, T7_WritePrivileged_StageA_Hit_EL2) {
    static constexpr StreamID SID = 0xC4;
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, SID, StreamWorld::EL2, privOnlyRW()));

    // Warm the TLB with a Write first.
    smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                    AccessType::Write,
                    SecurityState::NonSecure);

    TranslationResult rPriv = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                              AccessType::WritePrivileged,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(rPriv.isOk())
        << "T7: WritePrivileged on warm STRW=EL2 TLB must hit at Stage A "
           "(Stage B skipped — already at correct privilege level)";
}

// ── T8: Execute variant on STRW=EL2 succeeds via Stage-B re-check ───────────

TEST(BugCpp6StageB, T8_Execute_StageBRecheck_EL2) {
    static constexpr StreamID SID = 0xC5;
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, SID, StreamWorld::EL2, privOnlyExec()));

    TranslationResult r = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                          AccessType::Execute,
                                          SecurityState::NonSecure);
    ASSERT_TRUE(r.isOk())
        << "T8: Execute on STRW=EL2 privileged-only execute page must succeed";
}

// ── T9: ExecutePrivileged on STRW=EL2 hits Stage A directly ─────────────────
//
// Exercises the `default: break` path for ExecutePrivileged.

TEST(BugCpp6StageB, T9_ExecutePrivileged_StageA_Hit_EL2) {
    static constexpr StreamID SID = 0xC6;
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, SID, StreamWorld::EL2, privOnlyExec()));

    // Warm the TLB.
    smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                    AccessType::Execute,
                    SecurityState::NonSecure);

    TranslationResult rPriv = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                              AccessType::ExecutePrivileged,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(rPriv.isOk())
        << "T9: ExecutePrivileged on warm STRW=EL2 TLB must hit at Stage A";
}

// ── T10: ReadWrite and ReadWritePrivileged on STRW=EL2 ───────────────────────

TEST(BugCpp6StageB, T10_ReadWrite_And_ReadWritePrivileged_EL2) {
    static constexpr StreamID SID = 0xC7;
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, SID, StreamWorld::EL2, privOnlyRW()));

    // ReadWrite: Stage A fails (unprivileged check), Stage B promotes and hits.
    TranslationResult rRW = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                            AccessType::ReadWrite,
                                            SecurityState::NonSecure);
    ASSERT_TRUE(rRW.isOk())
        << "T10: ReadWrite on STRW=EL2 privOnly page must succeed via Stage-B";

    // ReadWritePrivileged: Stage A hits directly (default: break path in switch).
    TranslationResult rRWPriv = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                                AccessType::ReadWritePrivileged,
                                                SecurityState::NonSecure);
    ASSERT_TRUE(rRWPriv.isOk())
        << "T10: ReadWritePrivileged on warm STRW=EL2 TLB must hit at Stage A";

    EXPECT_EQ(rRW.getValue().physicalAddress,
              rRWPriv.getValue().physicalAddress)
        << "T10: ReadWrite and ReadWritePrivileged must resolve to the same PA";
}

// ── T11: Normal (non-privileged) page on STRW=EL2 — Read hits Stage A ───────
//
// For a non-privileged-only page, Stage A always hits on a warm TLB regardless
// of STRW.  Verifies that the STRW re-check does not interfere with the common
// case.

TEST(BugCpp6StageB, T11_NormalPage_StageA_Hits_STRW_EL2) {
    static constexpr StreamID SID = 0xC8;
    auto smmu = makeEnabledSMMU();
    ASSERT_TRUE(setupStream(*smmu, SID, StreamWorld::EL2, normalRW()));

    // Cold call populates TLB.
    smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                    AccessType::Read,
                    SecurityState::NonSecure);

    // Warm call: Stage A hits because privilegedOnly==false,
    // validateAccessPermissions(Read) → true without needing Stage B.
    TranslationResult r = smmu->translate(SID, CPP6_PASID, CPP6_IOVA,
                                          AccessType::Read,
                                          SecurityState::NonSecure);
    ASSERT_TRUE(r.isOk())
        << "T11: Warm Read on non-privileged page with STRW=EL2 must hit at Stage A";
}

} // namespace test
} // namespace smmu
