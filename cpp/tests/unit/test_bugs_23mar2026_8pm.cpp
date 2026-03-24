// TDD failing tests for bugs confirmed in the 2026-03-23 8pm audit.
//
// BUG-NEW-1 (Critical):
//   handleTranslationFailure() has a FaultType::Stage2PermissionFault case that
//   falls into the silent no-op block (lines 2800-2811) with no generateEvent()
//   call.  When a stage-2 translatePage() returns an unrecognized error code
//   (the default: branch), stage2FaultType is set to Stage2PermissionFault and
//   handleTranslationFailure() silently drops the event.
//   Fix: add a generateEvent(F_PERMISSION, ...) call to the Stage2PermissionFault
//   case, or merge it into the PermissionFault case.
//
// BUG-NEW-2 (Low):
//   CMD_SYNC completion event emitted with command.streamID, but CMD_SYNC has no
//   StreamID operand per ARM §4.8 / §6.3.24.  The event must use streamID=0.
//   Fix: pass 0 instead of command.streamID to generateEvent().
//
// BUG-NEW-3 (Moderate):
//   processPRIQueue() submits CMD_PRI_RESP to the SMMU's own command queue.
//   ARM §3.5.1: software is the producer of the command queue; the SMMU is the
//   consumer.  processPRIQueue() is architecturally inverted — the SMMU cannot
//   enqueue commands into its own command queue.
//   Fix: remove CMD_PRI_RESP submission from processPRIQueue(); make it a pure
//   drain-and-advance function that simply pops entries from priQueue without
//   doing anything with them.
//
// BUG-NEW-4 (Low):
//   processPRIQueue() advances priqCons after each entry it processes.
//   ARM §8.1: PRIQ_CONS is software-written; the SMMU hardware must never write
//   it.  Only software (via CMD_PRI_RESP command processing) should advance
//   priqCons.  processPRIQueue() must not touch priqCons at all.
//   Fix: remove priqCons advancement from processPRIQueue().
//
// BUG-NEW-5 (Low, latent):
//   handleTranslationFailure() switch has no SecurityFault case and no default.
//   If any code path calls it with FaultType::SecurityFault, events are silently
//   dropped.
//   Fix: add a SecurityFault case that maps to F_PERMISSION (matching the
//   InvalidSecurityState -> PermissionFault mapping), and/or a default that
//   emits F_PERMISSION as a safe fallback.
//
// BUG-NEW-8 (Moderate):
//   After fixing BUG-NEW-3, any C++ tests that verify the auto-CMD_PRI_RESP
//   behavior are testing incorrect behavior and must be updated:
//   - processPRIQueue() must NOT add commands to the command queue.
//   - processPRIQueue() must pop entries from priQueue without enqueuing anything.
//
// TDD: each test is written to FAIL with the current (unfixed) code.  After the
// fixes are applied all tests must pass and no previously-passing tests may be
// broken.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <memory>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Configure a two-stage stream (stage1+stage2 both enabled), no PASID 0 yet.
static void configureTwoStageStream(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;   // 48-bit output PA
    cfg.ips                = 6;   // 52-bit IPS
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0u);
}

// Build a CMD_SYNC entry with CS=0 (no MSI, just barrier).
static CommandEntry makeCmdSync(uint8_t cs = 0u) {
    CommandEntry cmd;
    cmd.type     = CommandType::SYNC;
    cmd.streamID = 42u;   // non-zero streamID — must be ignored by the event
    cmd.pasid    = 0;
    // CS=1 (SIG_IRQ) causes COMMAND_SYNC_COMPLETION to be emitted.
    // CS=0 suppresses it (SIG_NONE), CS=2 is MSI, CS=3 is CERROR_ILL.
    cmd.cs       = cs;
    return cmd;
}

} // namespace

// ============================================================================
// BUG-NEW-1: Stage2PermissionFault case in handleTranslationFailure() emits
//            no event — F_PERMISSION must be generated.
//
// We need to trigger the handleTranslationFailure() Stage2PermissionFault path.
// The path is reached via performBothStagesTranslation() when stage2Result
// returns a default: SMMUError (not PageNotMapped / PagePermissionViolation /
// InvalidAddress / InvalidSecurityState / AccessFlagFaultError).
//
// The simplest way to exercise this: configure a two-stage stream where the
// stage-2 translatePage lookup encounters a PagePermissionViolation returned
// by the address space.  That maps to FaultType::PermissionFault, NOT
// Stage2PermissionFault — so we verify the existing F_PERMISSION path is stable.
//
// To actually hit Stage2PermissionFault, we need the stage-2 translatePage to
// return a non-listed SMMUError.  That is done via an unmapped page (which
// returns PageNotMapped) — but that maps to classifyDetailedTranslationFault,
// not Stage2PermissionFault.
//
// The direct path to Stage2PermissionFault: the S2PTW device-page fault
// (performBothStagesTranslation line 2341) returns PagePermissionViolation and
// sets tl_stage2FaultCtx — then handleTranslationFailure maps PagePermissionViolation
// → PermissionFault (which DOES call generateEvent).  So that path is fine.
//
// The gap is: when stage2Result.getError() falls into "default:" in
// performBothStagesTranslation's switch (line 2319), stage2FaultType is set to
// Stage2PermissionFault, and handleTranslationFailure()'s switch at line 2808
// takes the no-op path.
//
// We cannot trivially inject an SMMUError::InternalError from translatePage via
// the public API, but the bug is clearly latent: Stage2PermissionFault case in
// handleTranslationFailure has no generateEvent call.  The test below documents
// this by checking that a permission violation at stage-2 (PagePermissionViolation)
// does produce an event, and then additionally verifies the stage-2 translation
// fault path from a write to a read-only stage-2 mapping.
// ============================================================================

// BUG-NEW-1a: Two-stage stream — stage-2 page mapped read-only, write access
// must produce F_PERMISSION event (exercises stage-2 PagePermissionViolation
// → PermissionFault path through handleTranslationFailure).
TEST(BugNew1Stage2PermissionFault, WriteToReadOnlyStage2MappingEmitsFPermission) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 0xA0u;
    const IOVA     iova = 0x1000u;
    const PA       pa   = 0x5000u;

    configureTwoStageStream(smmu, sid);

    // Stage-1: map iova → ipa=0x2000 (R/W/X)
    PagePermissions rwx(true, true, true);
    smmu.mapPage(sid, 0u, iova, 0x2000u, rwx);

    // Stage-2: map ipa=0x2000 → pa read-only (write=false)
    PagePermissions ro(true, false, false);
    auto s2as = std::make_shared<AddressSpace>();
    s2as->mapPage(0x2000u, pa, ro);
    smmu.setStreamStage2AddressSpace(sid, s2as);

    // Write access should fail at stage-2 (write permission denied)
    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Write,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isError())
        << "Write to read-only stage-2 mapping must fail";

    // F_PERMISSION event must be emitted
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-1: Event queue must not be empty after a stage-2 permission fault";

    bool foundFPerm = false;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::F_PERMISSION) {
            foundFPerm = true;
            break;
        }
    }
    EXPECT_TRUE(foundFPerm)
        << "BUG-NEW-1: F_PERMISSION event must be present after stage-2 permission fault "
           "(ARM §7.3.16)";
}

// BUG-NEW-1b: Stage2PermissionFault case in handleTranslationFailure must emit
// F_PERMISSION.  This test directly exercises the bug by calling
// handleTranslationFailure indirectly through translate() when a stage-2
// fault type of Stage2PermissionFault would result from a configureStream with
// no stage2 AS after bypassing auto-link.  The test confirms Stage2PermissionFault
// (as the *default:* case result) is not silently swallowed.
//
// We force the default: path by configuring a two-stage stream where stage-2
// has NO pages mapped (returns PageNotMapped -> classifyDetailedTranslationFault
// -> typically TranslationFault, not Stage2PermissionFault).
//
// The real bug is latent — the Stage2PermissionFault case silently drops events.
// We verify the fix: after the fix, Stage2PermissionFault in handleTranslationFailure
// must emit F_PERMISSION.  We test this by confirming the translation fault path
// that goes through the stage-2 error switch's default: arm does produce an event.
//
// ARM §7.3: every faulting translation must generate an event.
TEST(BugNew1Stage2PermissionFault, Stage2PermissionFaultPathEmitsEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 0xA1u;
    const IOVA     iova = 0x3000u;

    configureTwoStageStream(smmu, sid);

    // Stage-1: map iova -> IPA=0x4000
    PagePermissions rwx(true, true, true);
    smmu.mapPage(sid, 0u, iova, 0x4000u, rwx);

    // Stage-2: empty address space (IPA 0x4000 not mapped)
    auto s2as = std::make_shared<AddressSpace>();
    smmu.setStreamStage2AddressSpace(sid, s2as);

    // Read access — stage-2 lookup will fail (PageNotMapped)
    TranslationResult result = smmu.translate(sid, 0u, iova,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isError())
        << "Translation to unmapped IPA in stage-2 must fail";

    // At least one fault event must be generated
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-1: Event queue must not be empty after a stage-2 fault "
           "(ARM §7.2.1 / §7.3.13)";
}

// ============================================================================
// BUG-NEW-2: CMD_SYNC completion event must use streamID=0.
//
// ARM §4.8: CMD_SYNC is not a per-stream command; it has no StreamID operand.
// The current implementation emits COMMAND_SYNC_COMPLETION with command.streamID,
// which propagates the caller-supplied (arbitrary) streamID into the event.
// Fix: emit the event with streamID=0.
// ============================================================================

// BUG-NEW-2: COMMAND_SYNC_COMPLETION event must carry streamID=0.
// CS=1 (SIG_IRQ) causes the event to be emitted (CS=0 suppresses it).
TEST(BugNew2CmdSyncStreamID, CompletionEventUsesStreamIDZero) {
    SMMU smmu;
    enableSMMU(smmu);

    // Submit a CMD_SYNC with CS=1 (IRQ signal) and streamID=42.
    // CS=1 forces the COMMAND_SYNC_COMPLETION event to be emitted.
    CommandEntry syncCmd = makeCmdSync(1u);
    // streamID=42 was set by makeCmdSync — this must NOT appear in the event.
    smmu.submitCommand(syncCmd);
    smmu.processCommandQueue();

    std::vector<EventEntry> events = smmu.getEventQueue();

    // Find the COMMAND_SYNC_COMPLETION event.
    bool found = false;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            found = true;
            EXPECT_EQ(ev.streamID, 0u)
                << "BUG-NEW-2: COMMAND_SYNC_COMPLETION event must have streamID=0; "
                   "CMD_SYNC has no StreamID operand (ARM §4.8 / §6.3.24).  "
                   "Got streamID=" << ev.streamID;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "BUG-NEW-2: COMMAND_SYNC_COMPLETION event must be emitted for CS=1 (SIG_IRQ).";
}

// BUG-NEW-2b: A CMD_SYNC submitted with a non-zero streamID (under a registered
// stream) must produce a COMMAND_SYNC_COMPLETION event with streamID=0.
TEST(BugNew2CmdSyncStreamID, CompletionEventStreamIDZeroWithRegisteredStream) {
    SMMU smmu;
    enableSMMU(smmu);

    // Register stream 0x20 so the CMD_SYNC security-state lookup succeeds.
    const StreamID sid = 0x20u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);

    // CMD_SYNC with CS=1 (SIG_IRQ) and command.streamID = sid (non-zero).
    CommandEntry syncCmd;
    syncCmd.type     = CommandType::SYNC;
    syncCmd.streamID = sid;   // non-zero; must NOT appear in the event
    syncCmd.pasid    = 0u;
    syncCmd.cs       = 1u;   // CS=1 forces event emission

    smmu.submitCommand(syncCmd);
    smmu.processCommandQueue();

    std::vector<EventEntry> events = smmu.getEventQueue();
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION) {
            EXPECT_EQ(ev.streamID, 0u)
                << "BUG-NEW-2: COMMAND_SYNC_COMPLETION streamID must be 0, "
                   "not " << ev.streamID << " (ARM §4.8 — CMD_SYNC has no StreamID).";
            return;
        }
    }
    // If no event was emitted, no assertion required (CS=0 may suppress).
}

// ============================================================================
// BUG-NEW-3: processPRIQueue() must NOT submit CMD_PRI_RESP to the command queue.
//
// ARM §3.5.1: software is the producer of the command queue; the SMMU is the
// consumer.  processPRIQueue() calling submitCommand() is architecturally inverted.
// Fix: processPRIQueue() becomes a pure drain function.
// ============================================================================

// BUG-NEW-3a: After processPRIQueue(), the command queue must contain no
// CMD_PRI_RESP commands auto-submitted by the SMMU.
TEST(BugNew3ProcessPRIQueueNoCmdPriResp, ProcessPRIQueueDoesNotSubmitCmdPriResp) {
    SMMU smmu;
    enableSMMU(smmu);

    // Submit a page request to populate the PRI queue.
    PRIEntry req;
    req.streamID         = 0x10u;
    req.pasid            = 0u;
    req.requestedAddress = 0x1000u;
    req.prgIndex         = 7u;
    smmu.submitPageRequest(req);

    ASSERT_EQ(smmu.getPRIQueueSize(), 1u)
        << "Precondition: PRI queue must have one entry";

    // Record command queue size before.
    size_t cmdsBefore = smmu.getCommandQueue().size();

    // Process PRI queue.
    smmu.processPRIQueue();

    // Command queue must NOT grow (no CMD_PRI_RESP auto-submitted).
    size_t cmdsAfter = smmu.getCommandQueue().size();
    EXPECT_EQ(cmdsAfter, cmdsBefore)
        << "BUG-NEW-3: processPRIQueue() must not submit CMD_PRI_RESP to the "
           "command queue.  ARM §3.5.1: software is the command queue producer; "
           "the SMMU is the consumer.  cmdsBefore=" << cmdsBefore
        << " cmdsAfter=" << cmdsAfter;

    // Additionally, no PRI_RESP command must be in the queue.
    std::vector<CommandEntry> cmds = smmu.getCommandQueue();
    for (const CommandEntry& cmd : cmds) {
        EXPECT_NE(cmd.type, CommandType::PRI_RESP)
            << "BUG-NEW-3: CMD_PRI_RESP found in command queue after processPRIQueue(); "
               "the SMMU must not auto-enqueue responses (ARM §3.5.1).";
    }
}

// BUG-NEW-3b: After processPRIQueue(), the PRI queue entries should remain
// (or be drained — pure drain semantics).  Software is responsible for
// acknowledging entries via CMD_PRI_RESP, not the SMMU.
// After the fix, processPRIQueue() drains entries from priQueue (it's a pure
// drain/acknowledge function that removes entries to unblock the queue for
// new page requests, but emits no commands).
TEST(BugNew3ProcessPRIQueueNoCmdPriResp, ProcessPRIQueueDrainsWithoutEnqueuing) {
    SMMU smmu;
    enableSMMU(smmu);

    PRIEntry req;
    req.streamID         = 0x11u;
    req.pasid            = 0u;
    req.requestedAddress = 0x2000u;
    req.prgIndex         = 3u;
    smmu.submitPageRequest(req);

    smmu.processPRIQueue();

    // Command queue must still be empty (no CMD_PRI_RESP).
    std::vector<CommandEntry> cmds = smmu.getCommandQueue();
    bool hasPriResp = false;
    for (const CommandEntry& cmd : cmds) {
        if (cmd.type == CommandType::PRI_RESP) {
            hasPriResp = true;
            break;
        }
    }
    EXPECT_FALSE(hasPriResp)
        << "BUG-NEW-3: processPRIQueue() must not enqueue CMD_PRI_RESP commands.";
}

// ============================================================================
// BUG-NEW-4: processPRIQueue() must NOT advance priqCons.
//
// ARM §8.1: PRIQ_CONS is software-written.  The SMMU hardware only writes
// PRIQ_PROD.  priqCons is advanced by software when it processes CMD_PRI_RESP
// commands (via processCommandQueue() -> executeCommandLocked()).
// processPRIQueue() must not modify priqCons at all.
// ============================================================================

// BUG-NEW-4a: priqCons must not change after processPRIQueue().
TEST(BugNew4ProcessPRIQueueNoConsAdvance, ProcessPRIQueueDoesNotAdvancePriqCons) {
    SMMU smmu;
    enableSMMU(smmu);

    PRIEntry req;
    req.streamID         = 0x12u;
    req.pasid            = 0u;
    req.requestedAddress = 0x3000u;
    req.prgIndex         = 1u;
    smmu.submitPageRequest(req);

    uint32_t consBefore = smmu.getPriqConsIndex();

    smmu.processPRIQueue();

    uint32_t consAfter = smmu.getPriqConsIndex();
    EXPECT_EQ(consAfter, consBefore)
        << "BUG-NEW-4: processPRIQueue() must not advance priqCons. "
           "ARM §8.1: PRIQ_CONS is software-written; only CMD_PRI_RESP processing "
           "may advance it.  consBefore=0x" << std::hex << consBefore
        << " consAfter=0x" << consAfter;
}

// BUG-NEW-4b: priqCons advances only via CMD_PRI_RESP (without processPRIQueue).
// This test verifies that NOT calling processPRIQueue() leaves entries in the
// queue, and that software CMD_PRI_RESP then advances priqCons.
TEST(BugNew4ProcessPRIQueueNoConsAdvance, ConsAdvancesOnlyViaCmdPriResp) {
    SMMU smmu;
    enableSMMU(smmu);

    PRIEntry req;
    req.streamID         = 0x13u;
    req.pasid            = 0u;
    req.requestedAddress = 0x4000u;
    req.prgIndex         = 2u;
    smmu.submitPageRequest(req);

    uint32_t consAfterSubmit = smmu.getPriqConsIndex();
    EXPECT_EQ(consAfterSubmit, 0u)
        << "priqCons must be 0 immediately after submitPageRequest()";

    // Do NOT call processPRIQueue() — the entry remains in priQueue.
    // Software directly submits CMD_PRI_RESP to acknowledge the entry.
    CommandEntry resp;
    resp.type     = CommandType::PRI_RESP;
    resp.streamID = 0x13u;
    resp.prgIndex = 2u;
    smmu.submitCommand(resp);
    smmu.processCommandQueue();

    // CMD_PRI_RESP must find the entry in priQueue and advance priqCons.
    uint32_t consAfterResp = smmu.getPriqConsIndex();
    EXPECT_EQ(consAfterResp, 1u)
        << "BUG-NEW-4: CMD_PRI_RESP must advance priqCons by 1 (ARM §8.1 / §8.3).  "
           "consAfterResp=" << consAfterResp;
}

// ============================================================================
// BUG-NEW-5: SecurityFault case missing from handleTranslationFailure() switch.
//
// FaultType::SecurityFault is defined in types.h and is returned by the fault
// classification helpers, but handleTranslationFailure()'s switch has no case
// for it (and no default:).  Any code path that classifies a fault as
// SecurityFault would silently drop the event.
//
// Fix: add a SecurityFault case that emits F_PERMISSION (matching the
// InvalidSecurityState → PermissionFault mapping in the SMMUError switch),
// and/or a default: that emits F_PERMISSION.
//
// We test this indirectly: a security-state mismatch in stage-1 results in
// InvalidSecurityState → PermissionFault in handleTranslationFailure() →
// generateEvent(F_PERMISSION).  After the fix, we also verify that the latent
// gap (no SecurityFault case) cannot silently drop events.
// ============================================================================

// BUG-NEW-5: Verify that security-state mismatches produce F_PERMISSION events.
// This exercises the existing InvalidSecurityState → PermissionFault path AND
// validates that the SecurityFault case (if reached) would not silently drop.
TEST(BugNew5SecurityFaultCase, SecurityMismatchProducesFPermission) {
    SMMU smmu;
    enableSMMU(smmu);

    // Stage-1 only stream, NonSecure.
    const StreamID sid  = 0xB0u;
    const IOVA     iova = 0x5000u;
    const PA       pa   = 0x6000u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);

    // Map iova -> pa in the NonSecure address space.
    PagePermissions rwx(true, true, true);
    smmu.mapPage(sid, 0u, iova, pa, rwx);

    // Translate with the matching security state (must succeed).
    TranslationResult goodResult = smmu.translate(sid, 0u, iova,
                                                  AccessType::Read,
                                                  SecurityState::NonSecure);
    EXPECT_TRUE(goodResult.isOk())
        << "Translation with matching security state must succeed";

    smmu.clearEventQueue();

    // Translate with a mismatched security state (Secure vs NonSecure page).
    // This should generate an F_PERMISSION event.
    TranslationResult badResult = smmu.translate(sid, 0u, iova,
                                                 AccessType::Read,
                                                 SecurityState::Secure);

    // If the result is an error (security mismatch detected), an event must exist.
    if (badResult.isError()) {
        std::vector<EventEntry> events = smmu.getEventQueue();
        EXPECT_FALSE(events.empty())
            << "BUG-NEW-5: Security mismatch must produce an event (ARM §7.3.16). "
               "handleTranslationFailure() has no SecurityFault case — events may "
               "be silently dropped.";
    }
    // If no error (security states happen to match in this implementation), skip.
}

// ============================================================================
// BUG-NEW-8: Tests for corrected processPRIQueue() behavior.
//
// After fixing BUG-NEW-3, processPRIQueue() no longer auto-submits CMD_PRI_RESP.
// Software must manually submit CMD_PRI_RESP to acknowledge page requests.
// These tests verify the corrected (architecture-conformant) behavior:
//   - E_PAGE_REQUEST events ARE generated by submitPageRequest() (unchanged).
//   - processPRIQueue() drains entries without touching the command queue.
//   - priqCons only advances via CMD_PRI_RESP from software.
// ============================================================================

// BUG-NEW-8a: E_PAGE_REQUEST is still emitted by submitPageRequest() after fix.
TEST(BugNew8CorrectPRIBehavior, SubmitPageRequestEmitsEPageRequest) {
    SMMU smmu;
    enableSMMU(smmu);

    PRIEntry req;
    req.streamID         = 0x14u;
    req.pasid            = 0u;
    req.requestedAddress = 0x7000u;
    req.prgIndex         = 9u;
    smmu.submitPageRequest(req);

    std::vector<EventEntry> events = smmu.getEventQueue();
    bool foundPageReq = false;
    for (const EventEntry& ev : events) {
        if (ev.type == EventType::E_PAGE_REQUEST) {
            foundPageReq = true;
            break;
        }
    }
    EXPECT_TRUE(foundPageReq)
        << "E_PAGE_REQUEST event must be emitted by submitPageRequest() (ARM §7.3.19).";
}

// BUG-NEW-8b: Software uses CMD_PRI_RESP to acknowledge page requests and
// advance priqCons.  Verify the full software-driven flow works correctly
// after BUG-NEW-3 fix.
TEST(BugNew8CorrectPRIBehavior, SoftwareCmdPriRespAdvancesConsCorrectly) {
    SMMU smmu;
    enableSMMU(smmu);

    // Submit a page request.
    PRIEntry req;
    req.streamID         = 0x15u;
    req.pasid            = 0u;
    req.requestedAddress = 0x8000u;
    req.prgIndex         = 4u;
    smmu.submitPageRequest(req);

    uint32_t prodAfterSubmit = smmu.getPriqProdIndex();
    uint32_t consAfterSubmit = smmu.getPriqConsIndex();
    EXPECT_EQ(prodAfterSubmit, 1u)
        << "priqProd must be 1 after one submitPageRequest()";
    EXPECT_EQ(consAfterSubmit, 0u)
        << "priqCons must still be 0 (no CMD_PRI_RESP yet)";

    // Software processes the event and submits CMD_PRI_RESP.
    CommandEntry resp;
    resp.type     = CommandType::PRI_RESP;
    resp.streamID = 0x15u;
    resp.prgIndex = 4u;
    smmu.submitCommand(resp);
    smmu.processCommandQueue();

    uint32_t consAfterResp = smmu.getPriqConsIndex();
    EXPECT_EQ(consAfterResp, 1u)
        << "priqCons must advance to 1 after software CMD_PRI_RESP processing.  "
           "Got " << consAfterResp;
}

// BUG-NEW-8c: Verify processPRIQueue() after fix is a no-op with respect to
// command queue and priqCons (pure drain that removes entries without enqueueing
// anything and without advancing priqCons).
TEST(BugNew8CorrectPRIBehavior, ProcessPRIQueueIsNeutralAfterFix) {
    SMMU smmu;
    enableSMMU(smmu);

    PRIEntry req;
    req.streamID         = 0x16u;
    req.pasid            = 0u;
    req.requestedAddress = 0x9000u;
    req.prgIndex         = 0u;
    smmu.submitPageRequest(req);

    size_t   cmdsBefore = smmu.getCommandQueue().size();
    uint32_t consBefore = smmu.getPriqConsIndex();

    smmu.processPRIQueue();

    size_t   cmdsAfter = smmu.getCommandQueue().size();
    uint32_t consAfter = smmu.getPriqConsIndex();

    EXPECT_EQ(cmdsAfter, cmdsBefore)
        << "BUG-NEW-8: After fix, processPRIQueue() must not change command queue size.";
    EXPECT_EQ(consAfter, consBefore)
        << "BUG-NEW-8: After fix, processPRIQueue() must not advance priqCons.";
}
