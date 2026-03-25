// Regression tests for BUG-NEW-1 (audit of S1DSS==0b01 stage-2 fault paths).
//
// BUG-NEW-1 ANALYSIS:
//   The bug report claimed that three sub-paths in the S1DSS=0b01 + stage2Enabled=true
//   block inside performTwoStageTranslation() call recordFault() but never call
//   generateEvent(), leaving the Event queue empty after a fault.
//
//   FINDING: The bug does NOT exist in the current implementation.
//   All three sub-paths return error codes that flow through the central
//   handleTranslationFailure() dispatcher in SMMU::translate() (smmu.cpp ~line 879),
//   which calls generateEvent() for every fault type:
//     - PageNotMapped  → FaultType::TranslationFault → generateEvent(F_TRANSLATION,...)
//     - PagePermissionViolation → FaultType::PermissionFault → generateEvent(F_PERMISSION,...)
//   Additionally, each sub-path correctly sets tl_stage2FaultCtx.isStage2=true and
//   tl_stage2FaultCtx.ipa=iova before returning, so generateEvent() receives the
//   correct stage-2 context (s2=true, ipa=iova).
//
//   These tests serve as REGRESSION GUARDS: they verify that events are correctly
//   emitted for all three sub-paths and will catch any future regression that removes
//   the generateEvent() call from handleTranslationFailure() or changes the error
//   codes returned by the sub-paths.
//
// Sub-path 1 — null stage-2 AS (line ~1732–1745):
//   Returns PageNotMapped → handleTranslationFailure → generateEvent(F_TRANSLATION).
//
// Sub-path 2 — translatePage() returns error (line ~1761–1773):
//   Returns PageNotMapped (from translatePage missing entry) →
//   handleTranslationFailure → generateEvent(F_TRANSLATION).
//
// Sub-path 3 — permission check fails (line ~1791–1803):
//   Returns PagePermissionViolation → handleTranslationFailure → generateEvent(F_PERMISSION).
//
// ARM IHI0070G.b §7.2.1: faulting transactions must be recorded in the Event
// queue.  There is no S1DSS exclusion.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Configure a stream with S1DSS=0b01 and stage2Enabled=true.
// s1cdMax=2 makes the stream substream-capable so s1dss is active.
// Creates PASID 0 (which auto-links as stage-2 AS when stage2Enabled=true).
static void configureS1dssStage2Stream(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;   // 48-bit S2 PA space
    cfg.ips                = 6;   // 52-bit S1 IPS
    cfg.s1cdMax            = 2u;  // substream-capable → s1dss is active
    cfg.s1dss              = 0x01u; // non-substream PASID=0 bypasses stage-1
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    // Create PASID 0 so the stream context exists.  With stage2Enabled=true,
    // createStreamPASID(sid, 0) auto-links PASID 0's address space as the
    // stage-2 AS (stream_context.cpp BUG-07 fix).
    s.createStreamPASID(sid, 0u);
}

} // namespace

// ============================================================================
// TEST 1: S1DSS_NullS2AS_EmitsF_TRANSLATION
//
// Sub-path 1: s1dssS2AS == nullptr (no stage-2 AS installed).
// Returns SMMUError::PageNotMapped → handleTranslationFailure() →
// generateEvent(F_TRANSLATION, ..., isStage2=true, ipa=iova).
//
// To reach the null-S2-AS branch: the stream must have stage2Enabled=true
// but no PASID 0 created (createStreamPASID auto-links PASID 0 as stage-2 AS).
// Configure the stream WITHOUT calling createStreamPASID so s1dssS2AS is null.
//
// ARM §7.2.1: F_TRANSLATION event must appear in the Event queue.
// ============================================================================
TEST(BugNew1S1dssEventQueue, S1DSS_NullS2AS_EmitsF_TRANSLATION) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 0xE0u;
    const IOVA     iova = 0x1000u;

    // Configure WITHOUT createStreamPASID so the stage-2 AS pointer is null.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    cfg.ips                = 6;
    cfg.s1cdMax            = 2u;
    cfg.s1dss              = 0x01u;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    // Deliberately do NOT call createStreamPASID(sid, 0) → stage-2 AS stays null.

    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    // Translation must fail (sub-path 1 fires: null stage-2 AS).
    ASSERT_TRUE(result.isError())
        << "S1DSS=0b01 with null stage-2 AS must return an error";

    // Event queue must contain at least one F_TRANSLATION event.
    // ARM §7.2.1: faulting transactions must be recorded in the Event queue.
    // handleTranslationFailure() calls generateEvent(F_TRANSLATION,...) for
    // PageNotMapped errors (which is what sub-path 1 returns).
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-1 sub-path 1: Event queue must not be empty after a fault "
           "in the S1DSS=0b01 null-stage-2-AS path (ARM §7.2.1).";

    bool foundFTranslation = false;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::F_TRANSLATION) {
            foundFTranslation = true;
            break;
        }
    }
    EXPECT_TRUE(foundFTranslation)
        << "BUG-NEW-1 sub-path 1: an F_TRANSLATION event must be present in the "
           "Event queue after the null-stage-2-AS fault (ARM §7.3 / §7.2.1).";
}

// ============================================================================
// TEST 2: S1DSS_S2TranslateFail_EmitsF_TRANSLATION
//
// Sub-path 2: a stage-2 AS is installed but the IPA is NOT mapped.
// translatePage() returns FaultType::TranslationFault → SMMUError::PageNotMapped.
// handleTranslationFailure() → generateEvent(F_TRANSLATION, ..., isStage2=true, ipa=iova).
//
// Setup: install a stage-2 AS that maps 0x8000→0x9000 only, then translate 0x4000.
// ARM §7.2.1: F_TRANSLATION event must appear in the Event queue.
// ============================================================================
TEST(BugNew1S1dssEventQueue, S1DSS_S2TranslateFail_EmitsF_TRANSLATION) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid         = 0xE1u;
    const IOVA     unmappedIPA = 0x4000u;  // NOT mapped in stage-2

    configureS1dssStage2Stream(smmu, sid);

    // Replace the auto-linked stage-2 AS with one that does NOT map unmappedIPA.
    auto stage2AS = std::make_shared<AddressSpace>();
    const PagePermissions RW(true, true, false);
    stage2AS->mapPage(0x8000u, 0x9000u, RW);   // maps 0x8000, not 0x4000
    smmu.setStreamStage2AddressSpace(sid, stage2AS);

    TranslationResult result = smmu.translate(sid, 0u, unmappedIPA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    // Translation must fail (IPA not in stage-2 page table → sub-path 2).
    ASSERT_TRUE(result.isError())
        << "S1DSS=0b01 with unmapped IPA must return an error";

    // Event queue must contain at least one F_TRANSLATION event.
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-1 sub-path 2: Event queue must not be empty after a "
           "fault in the S1DSS=0b01 translatePage()-fails path (ARM §7.2.1).";

    bool foundFTranslation = false;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::F_TRANSLATION) {
            foundFTranslation = true;
            break;
        }
    }
    EXPECT_TRUE(foundFTranslation)
        << "BUG-NEW-1 sub-path 2: an F_TRANSLATION event must be present in the "
           "Event queue after the translatePage()-fails fault (ARM §7.3 / §7.2.1).";
}

// ============================================================================
// TEST 3: S1DSS_S2PermissionDeny_EmitsF_PERMISSION
//
// Sub-path 3: IPA is mapped read-only but access is Write → permission check fails.
// Returns SMMUError::PagePermissionViolation.
// handleTranslationFailure() → generateEvent(F_PERMISSION, ..., isStage2=true, ipa=iova).
//
// ARM §7.2.1: F_PERMISSION event must appear in the Event queue.
// ============================================================================
TEST(BugNew1S1dssEventQueue, S1DSS_S2PermissionDeny_EmitsF_PERMISSION) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid      = 0xE2u;
    const IOVA     mappedIPA = 0x5000u;

    configureS1dssStage2Stream(smmu, sid);

    // Install a stage-2 AS with a read-only mapping for the IPA we will write.
    auto stage2AS = std::make_shared<AddressSpace>();
    PagePermissions readOnly;
    readOnly.read           = true;
    readOnly.write          = false;   // write NOT allowed
    readOnly.execute        = false;
    readOnly.privilegedOnly = false;
    stage2AS->mapPage(mappedIPA, 0xA000u, readOnly);
    smmu.setStreamStage2AddressSpace(sid, stage2AS);

    // Write access on a read-only page → sub-path 3: permission check fails.
    TranslationResult result = smmu.translate(sid, 0u, mappedIPA,
                                              AccessType::Write,
                                              SecurityState::NonSecure);

    // Translation must fail (write to read-only page).
    ASSERT_TRUE(result.isError())
        << "S1DSS=0b01 with write access to read-only stage-2 page must return an error";

    // Event queue must contain at least one F_PERMISSION event.
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-1 sub-path 3: Event queue must not be empty after a "
           "permission fault in the S1DSS=0b01 path (ARM §7.2.1).";

    bool foundFPermission = false;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::F_PERMISSION) {
            foundFPermission = true;
            break;
        }
    }
    EXPECT_TRUE(foundFPermission)
        << "BUG-NEW-1 sub-path 3: an F_PERMISSION event must be present in the "
           "Event queue after the permission-check-fails fault (ARM §7.3 / §7.2.1).";
}

// ============================================================================
// TEST 4: S1DSS_S2TranslateFail_EventHasStage2Context
//
// Additional regression guard: verify that F_TRANSLATION events emitted by
// the S1DSS=0b01 stage-2 fault paths carry s2=true and ipa=iova.
//
// ARM §7.3 (CONF-GAP-20/NEW-2): stage-2 faults must set S2=1 and IPA in the
// EventEntry wire format.  The three sub-paths set tl_stage2FaultCtx.isStage2=true
// and tl_stage2FaultCtx.ipa=iova before returning; handleTranslationFailure()
// propagates these values to generateEvent().
// ============================================================================
TEST(BugNew1S1dssEventQueue, S1DSS_S2TranslateFail_EventHasStage2Context) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid      = 0xE3u;
    const IOVA     testIPA  = 0x6000u;

    configureS1dssStage2Stream(smmu, sid);

    // Stage-2 AS has no mapping for testIPA → sub-path 2 fires.
    auto stage2AS = std::make_shared<AddressSpace>();
    const PagePermissions RW(true, true, false);
    stage2AS->mapPage(0xF000u, 0x10000u, RW);  // maps a different address
    smmu.setStreamStage2AddressSpace(sid, stage2AS);

    smmu.translate(sid, 0u, testIPA, AccessType::Read, SecurityState::NonSecure);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty()) << "precondition: event queue must have an event";

    const EventEntry* ftrans = nullptr;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::F_TRANSLATION) { ftrans = &ev; break; }
    }
    ASSERT_NE(ftrans, nullptr) << "F_TRANSLATION event must be present";

    // ARM §7.3 (NEW-2): s2=true and ipa=IPA for stage-2 faults.
    EXPECT_TRUE(ftrans->s2)
        << "F_TRANSLATION from S1DSS=0b01 stage-2 path must have s2=true "
           "(tl_stage2FaultCtx.isStage2 is set before returning from sub-path 2)";
    EXPECT_EQ(ftrans->ipa, testIPA)
        << "F_TRANSLATION from S1DSS=0b01 stage-2 path must have ipa=testIPA "
           "(tl_stage2FaultCtx.ipa=iova set before returning from sub-path 2)";
}
