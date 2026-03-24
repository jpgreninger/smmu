// Tests for bugs found on 2026-03-21.
// TDD: all tests are written to FAIL before their respective fixes are applied.
//
// BUG-CPP-1/2: getEventQueue() advances eventqCons — it should be a pure snapshot.
//   ARM §3.5.1 states CONS.RD advances only when an entry is consumed (removed).
//   getEventQueue() is a non-destructive snapshot; processEventQueue() is the
//   consumption path that must advance CONS.
//   Fix: remove the CONS-advancement loop from getEventQueue().
//
// BUG-CPP-7: STRW=EL2/EL3 WriteExecute promotion missing from STRW switch.
//   NOTE: AccessType::WriteExecute and AccessType::ReadWriteExecute do NOT exist
//   in the C++ enum (WriteExecute was removed as architecturally impossible per
//   ARM §7.3: "InD==0 when RnW==0 — instruction fetches can never be writes").
//   This test verifies all existing non-privileged access types are promoted
//   correctly by the STRW switch and that the bug, as literally described, has
//   no applicable fix (the type does not exist).
//
// BUG-CPP-8: SecurityFault path in handleTranslationFailure() does not propagate
//   tl_stage2FaultCtx into the generated event.  For two-stage streams, a
//   security-state mismatch at stage-2 must produce an F_PERMISSION event with
//   s2=true and ipa set to the IPA produced by stage-1.
//   Fix: in the SecurityFault case, replace the recordSecurityFault() call with
//   a direct generateEvent(F_PERMISSION, ..., isStage2, ipa) using the
//   thread-local stage-2 fault context — or change the per-stage SMMUError
//   classification of InvalidSecurityState from SecurityFault to PermissionFault
//   so that the PermissionFault case (which already propagates tl_stage2FaultCtx)
//   handles the event emission.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helper utilities
// ============================================================================

namespace {

// Enable SMMU with all queue-enable bits set (SMMUEN | EVENTQEN | CMDQEN | PRIQEN).
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Configure and enable a stage-1-only stream with t0sz=0 (no VA restriction).
static void configureStage1Stream(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0);
}

// Configure and enable a stage-1-only stream with the given STRW world.
static void configureStage1StreamSTRW(SMMU& s, StreamID sid, StreamWorld strw) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = strw;
    // ARM §5.2 BUG-CPP-3(a): STRW bit[0]=1 (EL2/EL3) requires Secure security
    // state when stage-1 is active.
    if (strw == StreamWorld::EL2 || strw == StreamWorld::EL3) {
        cfg.securityState = SecurityState::Secure;
    }
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0);
}

} // namespace

// ============================================================================
// BUG-CPP-1/2: getEventQueue() must NOT advance eventqCons (pure snapshot)
//
// ARM §3.5.1: The software consumer advances CONS when consuming (removing)
// entries.  getEventQueue() returns a snapshot copy — it does NOT remove
// entries from the queue.  Therefore it must NOT advance eventqCons.
//
// The previous BUG-AUDIT-4 "fix" incorrectly added CONS advancement to
// getEventQueue().  The correct design is:
//   - getEventQueue()       → pure snapshot, CONS unchanged
//   - processEventQueue()   → removes entries, CONS advances
//   - clearEventQueue()     → empties queue, CONS/PROD reset to 0
//
// These tests FAIL before the fix because the current code advances CONS
// inside getEventQueue(), making a second call return 0 events and changing
// the CONS index after the first call.
// ============================================================================

// Test 1: Calling getEventQueue() twice returns the same events both times.
//
// The queue must act as a snapshot: repeated calls must yield the same events
// until processEventQueue() or clearEventQueue() actually removes them.
TEST(BugCpp12GetEventQueueIsSnapshot, SecondCallReturnsSameEvents) {
    SMMU smmu;
    enableSMMU(smmu);
    const StreamID sid = 0x42u;
    configureStage1Stream(smmu, sid);

    // Trigger a fault to produce one event (no page mapped → F_TRANSLATION).
    smmu.translate(sid, 0, 0x1000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_GE(smmu.getEventQueueSize(), 1u) << "precondition: at least one event must be queued";

    // First snapshot.
    std::vector<EventEntry> first = smmu.getEventQueue();
    ASSERT_GE(first.size(), 1u) << "first call must return at least one event";

    // Second snapshot — must return the same number of events.
    std::vector<EventEntry> second = smmu.getEventQueue();
    EXPECT_EQ(second.size(), first.size())
        << "getEventQueue() is a pure snapshot: second call must return the same events "
           "as the first call (CONS must NOT advance on snapshot reads)";
}

// Test 2: eventqCons must NOT change after getEventQueue().
//
// A non-destructive snapshot read must leave the CONS register unchanged.
// This test captures CONS before and after the call and asserts they match.
TEST(BugCpp12GetEventQueueIsSnapshot, ConsDoesNotAdvanceAfterGet) {
    SMMU smmu;
    enableSMMU(smmu);
    const StreamID sid = 0x43u;
    configureStage1Stream(smmu, sid);

    // Generate one fault event.
    smmu.translate(sid, 0, 0x2000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_GE(smmu.getEventQueueSize(), 1u) << "precondition: event must be queued";

    // Capture CONS before the snapshot.
    uint32_t consBefore = smmu.getEventqConsIndex() & ~(1u << 31);

    // Take snapshot — must NOT advance CONS.
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_GE(events.size(), 1u) << "snapshot must return the queued event";

    // CONS must be identical after the call.
    uint32_t consAfter = smmu.getEventqConsIndex() & ~(1u << 31);
    EXPECT_EQ(consAfter, consBefore)
        << "getEventQueue() is a pure snapshot: CONS must NOT advance "
           "(ARM §3.5.1 — CONS advances only on actual consumption)";
}

// Test 3: processEventQueue() DOES advance CONS (regression guard).
//
// This confirms that CONS advancement still happens via the correct path
// (processEventQueue), not via getEventQueue().
TEST(BugCpp12GetEventQueueIsSnapshot, ProcessEventQueueAdvancesCons) {
    SMMU smmu;
    enableSMMU(smmu);
    const StreamID sid = 0x44u;
    configureStage1Stream(smmu, sid);

    // Generate one fault event.
    smmu.translate(sid, 0, 0x3000u, AccessType::Read, SecurityState::NonSecure);
    ASSERT_GE(smmu.getEventQueueSize(), 1u) << "precondition: event must be queued";

    uint32_t consBefore = smmu.getEventqConsIndex() & ~(1u << 31);
    uint32_t prodBefore = smmu.getEventqProdIndex() & ~(1u << 31);
    ASSERT_NE(consBefore, prodBefore) << "precondition: CONS must be behind PROD";

    // processEventQueue() must advance CONS to PROD.
    smmu.processEventQueue();

    uint32_t consAfter = smmu.getEventqConsIndex() & ~(1u << 31);
    uint32_t prodAfter = smmu.getEventqProdIndex() & ~(1u << 31);
    EXPECT_EQ(consAfter, prodAfter)
        << "processEventQueue() must advance CONS to PROD after consuming all events";
}

// Test 4: Multiple calls to getEventQueue() between faults are consistent.
//
// Verify that after generating N events, repeated snapshot calls all return N.
TEST(BugCpp12GetEventQueueIsSnapshot, MultipleSnapshotsConsistentWithQueueSize) {
    SMMU smmu;
    enableSMMU(smmu);
    const StreamID sid = 0x45u;
    configureStage1Stream(smmu, sid);

    // Generate 3 fault events (different addresses → 3 separate faults).
    smmu.translate(sid, 0, 0x4000u, AccessType::Read, SecurityState::NonSecure);
    smmu.translate(sid, 0, 0x5000u, AccessType::Read, SecurityState::NonSecure);
    smmu.translate(sid, 0, 0x6000u, AccessType::Read, SecurityState::NonSecure);

    size_t queueSize = smmu.getEventQueueSize();
    ASSERT_GE(queueSize, 1u) << "precondition: at least one event must be queued";

    // Three consecutive snapshots must all return queueSize events.
    EXPECT_EQ(smmu.getEventQueue().size(), queueSize)
        << "first snapshot must return all queued events";
    EXPECT_EQ(smmu.getEventQueue().size(), queueSize)
        << "second snapshot must return all queued events (CONS did not advance)";
    EXPECT_EQ(smmu.getEventQueue().size(), queueSize)
        << "third snapshot must return all queued events (CONS did not advance)";
}

// ============================================================================
// BUG-CPP-7: STRW=EL2/EL3 WriteExecute promotion missing from switch.
//
// NOTE: AccessType::WriteExecute and AccessType::ReadWriteExecute do NOT exist
// in the C++ enum.  WriteExecute was removed as architecturally impossible per
// ARM §7.3: an instruction fetch (InD=1) cannot also be a write (RnW=0).
// Therefore no case needs to be added to the STRW switch for these types.
//
// These tests confirm that all existing non-privileged access types ARE
// correctly promoted by the STRW=EL2 switch, and that Write+privilegedOnly
// with STRW=EL2 succeeds (the promotion path works).
// ============================================================================

// Helper: create a stage-1-only stream with STRW=EL2, map a privilegedOnly page,
// translate with the given access type, and return whether translation succeeded.
static bool translateWithSTRWEL2(AccessType accessType) {
    SMMU smmu;
    enableSMMU(smmu);
    const StreamID sid = 0x60u;
    configureStage1StreamSTRW(smmu, sid, StreamWorld::EL2);

    // Map a privilegedOnly page at 0x1000 → 0x2000.
    PagePermissions perms;
    perms.read          = true;
    perms.write         = true;
    perms.execute       = true;
    perms.privilegedOnly = true; // unprivileged access denied
    // EL2 stream is now Secure (ARM §5.2 BUG-CPP-3(a)); use Secure for map and translate.
    smmu.mapPage(sid, 0, 0x1000u, 0x2000u, perms, SecurityState::Secure);

    TranslationResult result = smmu.translate(sid, 0, 0x1000u, accessType,
                                              SecurityState::Secure);
    return result.isOk();
}

// Write access with STRW=EL2 must be promoted to WritePrivileged and succeed
// against a privilegedOnly page (confirms the Write→WritePrivileged path works).
TEST(BugCpp7STRWWritePromotion, WriteWithSTRWEL2PassesPrivilegedOnlyPage) {
    // This test must PASS both before and after the fix: Write is already in
    // the switch and WriteExecute does not exist in the AccessType enum.
    EXPECT_TRUE(translateWithSTRWEL2(AccessType::Write))
        << "Write + STRW=EL2 must be promoted to WritePrivileged and succeed "
           "against a privilegedOnly page (Write→WritePrivileged already in switch)";
}

// ReadWrite access with STRW=EL2 must succeed against a privilegedOnly page.
TEST(BugCpp7STRWWritePromotion, ReadWriteWithSTRWEL2PassesPrivilegedOnlyPage) {
    EXPECT_TRUE(translateWithSTRWEL2(AccessType::ReadWrite))
        << "ReadWrite + STRW=EL2 must succeed against a privilegedOnly page";
}

// Read access with STRW=EL2 must succeed against a privilegedOnly page.
TEST(BugCpp7STRWWritePromotion, ReadWithSTRWEL2PassesPrivilegedOnlyPage) {
    EXPECT_TRUE(translateWithSTRWEL2(AccessType::Read))
        << "Read + STRW=EL2 must succeed against a privilegedOnly page";
}

// Execute access with STRW=EL2 must succeed against a privilegedOnly page.
TEST(BugCpp7STRWWritePromotion, ExecuteWithSTRWEL2PassesPrivilegedOnlyPage) {
    EXPECT_TRUE(translateWithSTRWEL2(AccessType::Execute))
        << "Execute + STRW=EL2 must succeed against a privilegedOnly page";
}

// ReadExecute access with STRW=EL2 must succeed against a privilegedOnly page.
TEST(BugCpp7STRWWritePromotion, ReadExecuteWithSTRWEL2PassesPrivilegedOnlyPage) {
    EXPECT_TRUE(translateWithSTRWEL2(AccessType::ReadExecute))
        << "ReadExecute + STRW=EL2 must succeed against a privilegedOnly page";
}

// ============================================================================
// BUG-CPP-8: SecurityFault path does not propagate tl_stage2FaultCtx.
//
// ARM §7.3.16: A security-state mismatch at stage-2 must generate F_PERMISSION
// with s2=true and ipa set to the IPA produced by stage-1 translation.
//
// The current code routes SMMUError::InvalidSecurityState through the
// SecurityFault case in handleTranslationFailure(), which calls
// recordSecurityFault().  recordSecurityFault() calls
//   generateEvent(F_PERMISSION, ..., false, 0, accessType)
// without the tl_stage2FaultCtx fields.  As a result, the generated event has
// s2=false and ipa=0 instead of s2=true and ipa=<correct IPA>.
//
// Fix: change the InvalidSecurityState → SecurityFault mapping in the per-stage
// classification switches to PermissionFault so that the PermissionFault case
// in handleTranslationFailure() (which already calls
//   generateEvent(F_PERMISSION, ..., isStage2, ipa)
// using tl_stage2FaultCtx) handles event emission correctly.
//
// These tests FAIL before the fix because s2 is false in the generated event.
// ============================================================================

// Test: Two-stage stream where stage-2 has a security-state mismatch.
// Stage-1 maps IOVA → IPA (NonSecure output), but stage-2 page is mapped as
// Secure.  Translation must fail with F_PERMISSION, s2=true, ipa=IPA.
TEST(BugCpp8SecurityFaultStage2, TwoStageSecurityMismatchEmitsS2TrueIPA) {
    static constexpr StreamID   SID  = 0x80u;
    static constexpr PASID      PID  = 0u;
    static constexpr IOVA       VA   = 0xA000u;
    static constexpr PA         IPA  = 0xB000u;
    static constexpr PA         PA_V = 0xC000u;

    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.t0sz               = 0;
    smmu.configureStream(SID, cfg);
    smmu.enableStream(SID);
    smmu.createStreamPASID(SID, PID);

    // Stage-1: map VA → IPA (NonSecure output security state).
    PagePermissions s1perms(true, false, false);
    smmu.mapPage(SID, PID, VA, IPA, s1perms, SecurityState::NonSecure);

    // Stage-2: map IPA → PA but as Secure — mismatch with stage-1 NonSecure output.
    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions s2perms(true, false, false);
    s2as->mapPage(IPA, PA_V, s2perms, SecurityState::Secure); // Secure ≠ NonSecure output
    smmu.setStreamStage2AddressSpace(SID, s2as);

    // Translation must fail (security mismatch).
    TranslationResult result = smmu.translate(SID, PID, VA, AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isError())
        << "Two-stage translation with security-state mismatch at stage-2 must fail";

    // Inspect the generated event.
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_GE(events.size(), 1u) << "At least one F_PERMISSION event must be in the queue";

    // Find the F_PERMISSION event.
    const EventEntry* permEvent = nullptr;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_PERMISSION) {
            permEvent = &ev;
            break;
        }
    }
    ASSERT_NE(permEvent, nullptr)
        << "An F_PERMISSION event must be generated for security-state mismatch "
           "(ARM §7.3.16)";

    // ARM §7.3.13: For stage-2 faults, s2 must be true and ipa must be the IPA.
    EXPECT_TRUE(permEvent->s2)
        << "BUG-CPP-8: F_PERMISSION from stage-2 security mismatch must have s2=true "
           "(ARM §7.3.13); got s2=false because SecurityFault path does not propagate "
           "tl_stage2FaultCtx";
    EXPECT_EQ(permEvent->ipa & ~0xFFFULL, IPA & ~0xFFFULL)
        << "BUG-CPP-8: F_PERMISSION from stage-2 security mismatch must carry "
           "ipa=<stage-1 IPA output> (ARM §7.3.13); got ipa=0 because the "
           "SecurityFault path calls recordSecurityFault() which does not pass "
           "tl_stage2FaultCtx";
}

// Test: Stage-1-only stream with a security mismatch must emit F_PERMISSION
// with s2=false (not a stage-2 fault) — regression guard.
TEST(BugCpp8SecurityFaultStage2, Stage1OnlySecurityMismatchEmitsS2False) {
    static constexpr StreamID   SID = 0x81u;
    static constexpr PASID      PID = 0u;
    static constexpr IOVA       VA  = 0xD000u;
    static constexpr PA         PA_V = 0xE000u;

    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    smmu.configureStream(SID, cfg);
    smmu.enableStream(SID);
    smmu.createStreamPASID(SID, PID);

    // Map VA → PA as Secure; translate as NonSecure → mismatch.
    PagePermissions perms(true, false, false);
    smmu.mapPage(SID, PID, VA, PA_V, perms, SecurityState::Secure);

    TranslationResult result = smmu.translate(SID, PID, VA, AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isError())
        << "Stage-1-only translation with security-state mismatch must fail";

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_GE(events.size(), 1u) << "At least one event must be in the queue";

    const EventEntry* permEvent = nullptr;
    for (const auto& ev : events) {
        if (ev.type == EventType::F_PERMISSION) {
            permEvent = &ev;
            break;
        }
    }
    ASSERT_NE(permEvent, nullptr)
        << "An F_PERMISSION event must be generated for security-state mismatch "
           "(ARM §7.3.16)";

    // Stage-1-only fault: s2 must be false.
    EXPECT_FALSE(permEvent->s2)
        << "Stage-1-only security mismatch: s2 must be false (not a stage-2 fault)";
}
