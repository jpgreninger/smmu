// TDD failing tests for three bugs fixed on 2026-03-23.
//
// BUG-CPP-1 (Peek-before-pop desync in processCommandQueue):
//   At the top of the while loop, the code does:
//     CommandEntry command = commandQueue.front();
//     commandQueue.pop_front();   // popped BEFORE processing
//     processCommand(command, lock);
//   When processCommand() signals GERROR_CMDQ_ERR, the loop breaks without
//   advancing CONS.RD — but the command was ALREADY REMOVED from commandQueue.
//   This desync means a second call to processCommandQueue() sees the NEXT
//   command at queue front, not the erroneous one.  ARM §7.1 requires that
//   the erroneous command remain accessible (CONS.RD == queue front).
//   Similarly, CMD_SYNC CS=3 pops the command before the CS check fires.
//   Fix: Peek-before-pop — remove pop_front() from before processCommand();
//   only pop (and advance CONS.RD) on the success path.
//
// BUG-CPP-2 (CMD_TLBI_NH_ASID ignores VMID):
//   ARM §4.4.2.2: CMD_TLBI_NH_ASID invalidates TLB entries tagged with BOTH
//   VMID AND ASID.  The current implementation calls invalidateByASID(asid),
//   which ignores VMID and over-invalidates entries from other VMIDs that
//   happen to share the same ASID.
//   Fix: add TLBCache::invalidateByVMIDAndASID() and call it from the
//   TLBI_NH_ASID case.
//
// BUG-CPP-3(a)+(b) (STRW validation in configureStream):
//   (a) Only STRW=EL3 (0b11) is rejected for NonSecure streams; ARM §5.2
//       pattern 'x1' means BOTH 0b01 (EL2) AND 0b11 (EL3) are illegal when
//       stage-1 is active.
//   (b) STRW=EL3 (0b11) is also illegal for Secure streams per ARM §5.2
//       SteIllegal() pseudocode.  The check was missing for Secure+EL3.
//   STRW checks only apply when stage1Enabled=true (strwUnused=false).
//
// TDD: each test is written to FAIL with the current code.  After the three
// fixes are applied all tests must pass and no previously-passing tests may
// be broken.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
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

// Build a CMD_SYNC entry with the given CS value.
static CommandEntry makeCmdSync(uint8_t cs) {
    CommandEntry cmd;
    cmd.type     = CommandType::SYNC;
    cmd.streamID = 0;
    cmd.pasid    = 0;
    cmd.cs       = cs;
    return cmd;
}

// Build a harmless CFGI_ALL command (consumes cleanly, no error).
static CommandEntry makeCfgiAll() {
    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_ALL;
    cmd.streamID = 0;
    cmd.pasid    = 0;
    cmd.cs       = 0;
    return cmd;
}

// Configure a stage-1-only stream with the given VMID and ASID, map one page,
// and return true on success.
static bool configureAndMapStream(SMMU& smmu, StreamID sid, uint16_t vmid,
                                  uint16_t asid, IOVA iova, PA pa) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = StreamWorld::EL1_EL0;

    if (!smmu.configureStream(sid, cfg).isOk()) return false;
    smmu.setStreamVMID(sid, vmid);
    smmu.setStreamASID(sid, asid);
    if (!smmu.enableStream(sid).isOk()) return false;
    if (!smmu.createStreamPASID(sid, 0).isOk()) return false;

    PagePermissions perms;
    perms.read           = true;
    perms.write          = false;
    perms.execute        = false;
    perms.privilegedOnly = false;
    if (!smmu.mapPage(sid, 0, iova, pa, perms, SecurityState::NonSecure).isOk()) return false;
    return true;
}

} // namespace

// ============================================================================
// BUG-CPP-1: Peek-before-pop desync
// ============================================================================

// Test B1-1: A single CMD_SYNC CS=3 (CERROR_ILL) must remain at the front of
// commandQueue after processCommandQueue() returns with the error.
//
// BEFORE FIX: commandQueue.pop_front() is called BEFORE the CS check, so when
//   the CS==3 branch fires and breaks, the command is gone from the deque.
//   getCommandQueue() returns an empty vector.
// AFTER FIX: pop_front() is only called on the success path; the erroneous
//   command stays at commandQueue.front() so getCommandQueue()[0] is the
//   CMD_SYNC CS=3 command.
TEST(BugCpp1PeekBeforePop, CS3CommandRemainsInQueueAfterError) {
    SMMU smmu;
    enableSMMU(smmu);

    // Submit exactly one illegal CMD_SYNC (CS=3).
    ASSERT_TRUE(smmu.submitCommand(makeCmdSync(3u)).isOk())
        << "submitCommand must succeed before processCommandQueue";
    ASSERT_EQ(smmu.getCommandQueueSize(), 1u)
        << "precondition: queue must have exactly 1 command";

    smmu.processCommandQueue();

    // CERROR_ILL must have been signalled.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "CERROR_ILL must be set in CMDQ_CONS.ERR after CS=3";

    // CONS.RD must still be 0 (not advanced past the erroneous command).
    uint32_t consRd = smmu.getCmdqConsIndex() & 0xFFFFFu;
    EXPECT_EQ(consRd, 0u)
        << "BUG-CPP-1: CONS.RD must not be advanced when CS=3 error fires";

    // The erroneous command must still be at the front of commandQueue.
    // BEFORE FIX: pop_front() was called before the CS check, so the queue is
    //             now empty and this assertion fails.
    // AFTER FIX:  the command was only peeked, not popped; it remains in place.
    std::vector<CommandEntry> q = smmu.getCommandQueue();
    ASSERT_EQ(q.size(), 1u)
        << "BUG-CPP-1: the erroneous CMD_SYNC CS=3 must still be in commandQueue "
           "(CONS.RD and queue front must be in sync per ARM §7.1). "
           "Before fix, queue is empty because pop_front() ran before the CS check.";
    EXPECT_EQ(q[0].type, CommandType::SYNC)
        << "BUG-CPP-1: first command in queue must be CommandType::SYNC";
    EXPECT_EQ(q[0].cs, 3u)
        << "BUG-CPP-1: the retained command must have cs=3";
}

// Test B1-2: After one valid command followed by CMD_SYNC CS=3, the valid
// command is consumed (not in queue), and the erroneous CMD_SYNC CS=3 remains.
//
// BEFORE FIX: both commands are popped before processing; queue is empty.
// AFTER FIX:  valid command popped on success, erroneous CMD_SYNC stays.
TEST(BugCpp1PeekBeforePop, ValidCommandConsumedBeforCS3Error) {
    SMMU smmu;
    enableSMMU(smmu);

    // First: a valid command that processes cleanly.
    ASSERT_TRUE(smmu.submitCommand(makeCfgiAll()).isOk())
        << "first submitCommand (CFGI_ALL) must succeed";
    // Second: illegal CMD_SYNC CS=3.
    ASSERT_TRUE(smmu.submitCommand(makeCmdSync(3u)).isOk())
        << "second submitCommand (SYNC CS=3) must succeed";
    ASSERT_EQ(smmu.getCommandQueueSize(), 2u)
        << "precondition: queue must have 2 commands";

    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "CERROR_ILL must be set in CMDQ_CONS.ERR after CS=3 error";

    // CONS.RD must be 1 (advanced past the good CFGI_ALL, stopped at CMD_SYNC CS=3).
    uint32_t consRd = smmu.getCmdqConsIndex() & 0xFFFFFu;
    EXPECT_EQ(consRd, 1u)
        << "CONS.RD must be 1 after one successful command and one erroneous CMD_SYNC";

    // Queue must contain exactly the erroneous CMD_SYNC CS=3 — the CFGI_ALL is consumed.
    // BEFORE FIX: queue is empty (both commands popped before processing).
    // AFTER FIX:  CFGI_ALL was popped on success, CMD_SYNC CS=3 remains.
    std::vector<CommandEntry> q = smmu.getCommandQueue();
    ASSERT_EQ(q.size(), 1u)
        << "BUG-CPP-1: after processing [CFGI_ALL, SYNC CS=3], queue must contain "
           "exactly the erroneous SYNC CS=3 (CFGI_ALL was consumed successfully). "
           "Before fix: queue is empty because both commands were popped pre-processing.";
    EXPECT_EQ(q[0].type, CommandType::SYNC)
        << "the remaining command must be SYNC";
    EXPECT_EQ(q[0].cs, 3u)
        << "the remaining command must have cs=3";
}

// Test B1-3: After clearing the GERROR.CMDQ_ERR and calling processCommandQueue
// again, the same CS=3 command is processed again (no consumed-and-lost desync).
//
// BEFORE FIX: the erroneous command was popped from the deque; on the second
//   processCommandQueue() call the queue is empty, so no error is re-triggered.
//   CMDQ_CONS.ERR stays CERROR_ILL from before but no new command is processed.
// AFTER FIX:  the command was NOT popped; the second call encounters CS=3 again
//   and signals CERROR_ILL a second time.
TEST(BugCpp1PeekBeforePop, ReprocessSameCommandAfterGerrorClear) {
    SMMU smmu;
    enableSMMU(smmu);

    ASSERT_TRUE(smmu.submitCommand(makeCmdSync(3u)).isOk());
    ASSERT_EQ(smmu.getCommandQueueSize(), 1u);

    // First processing run — gets CERROR_ILL and breaks.
    smmu.processCommandQueue();
    ASSERT_EQ(smmu.getCmdqConsErr(), CERROR_ILL) << "precondition: CERROR_ILL set";

    // Clear GERROR.CMDQ_ERR by acknowledging it.
    smmu.clearGerror(GERROR_CMDQ_ERR);

    // Second processing run — should encounter the same erroneous command again.
    smmu.processCommandQueue();

    // BEFORE FIX: the command was popped on the first run, so the second run
    //   finds an empty queue and does nothing (CMDQ_CONS.ERR may have been
    //   zeroed by clearGerror but never re-set).
    // AFTER FIX: the command was NOT popped; the second run processes it again
    //   and re-triggers CERROR_ILL.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-CPP-1: after clearing GERROR and reprocessing, the erroneous "
           "CMD_SYNC CS=3 must still be at queue front and re-trigger CERROR_ILL. "
           "Before fix: command was popped on first run; second run finds empty queue "
           "and does not re-set CERROR_ILL.";
}

// ============================================================================
// BUG-CPP-2: CMD_TLBI_NH_ASID must invalidate by VMID AND ASID (joint match)
// ============================================================================

// Test B2-1: CMD_TLBI_NH_ASID with VMID=10 and ASID=5 must NOT invalidate a
// TLB entry that shares ASID=5 but belongs to VMID=20.
//
// Setup:
//   Stream A: VMID=10, ASID=5 → mapped page at IOVA_A → PA_A
//   Stream B: VMID=20, ASID=5 → mapped page at IOVA_B → PA_B
// Populate both TLB entries by translating each stream.
// Issue CMD_TLBI_NH_ASID (VMID=10, ASID=5).
// Translate stream A again — should MISS the TLB (TLB hit would give stale PA).
// Translate stream B again — should still HIT the TLB (not invalidated).
//
// To distinguish TLB-hit from fresh translation, we remap IOVA_A → PA_A_NEW
// WITHOUT clearing the old TLB entry.  A TLB miss causes a fresh lookup in
// the address space (returns PA_A_NEW).  A stale TLB hit returns PA_A.
//
// BEFORE FIX: invalidateByASID(5) evicts BOTH entries; Stream B's TLB entry is
//   gone, so its second translate also does a fresh lookup → returns PA_B.
//   We detect the bug by verifying stream B's TLB entry survived (hit count).
// AFTER FIX:  only Stream A's entry is evicted; Stream B's entry remains intact.
TEST(BugCpp2TlbiNhAsidVmid, TlbiNhAsidDoesNotInvalidateOtherVMIDs) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID  SID_A  = 0xA0u;
    static constexpr StreamID  SID_B  = 0xA1u;
    static constexpr uint16_t  VMID_A = 10u;
    static constexpr uint16_t  VMID_B = 20u;
    static constexpr uint16_t  ASID   = 5u;
    static constexpr IOVA      IOVA_A = 0x1000ULL;
    static constexpr IOVA      IOVA_B = 0x2000ULL;
    static constexpr PA        PA_A   = 0x10000ULL;
    static constexpr PA        PA_B   = 0x20000ULL;

    ASSERT_TRUE(configureAndMapStream(smmu, SID_A, VMID_A, ASID, IOVA_A, PA_A))
        << "failed to configure stream A";
    ASSERT_TRUE(configureAndMapStream(smmu, SID_B, VMID_B, ASID, IOVA_B, PA_B))
        << "failed to configure stream B";

    // Warm both TLB entries by translating each stream once.
    TranslationResult ra1 = smmu.translate(SID_A, 0, IOVA_A,
                                           AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(ra1.isOk()) << "precondition: stream A first translate must succeed";

    TranslationResult rb1 = smmu.translate(SID_B, 0, IOVA_B,
                                           AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(rb1.isOk()) << "precondition: stream B first translate must succeed";

    // Issue CMD_TLBI_NH_ASID targeting VMID=10, ASID=5.
    CommandEntry tlbi;
    tlbi.type     = CommandType::TLBI_NH_ASID;
    tlbi.streamID = 0;
    tlbi.pasid    = 0;
    tlbi.asid     = ASID;
    tlbi.vmid     = VMID_A;
    ASSERT_TRUE(smmu.submitCommand(tlbi).isOk())
        << "TLBI_NH_ASID submit must succeed";
    smmu.processCommandQueue();

    // Capture stream B TLB stats before and after translate.
    uint64_t hitsBefore = smmu.getCacheHitCount();
    uint64_t missesBefore = smmu.getCacheMissCount();

    // Stream B translate after TLBI: should be a TLB HIT (not invalidated).
    TranslationResult rb2 = smmu.translate(SID_B, 0, IOVA_B,
                                           AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(rb2.isOk()) << "stream B second translate must succeed";

    uint64_t hitsAfter   = smmu.getCacheHitCount();
    uint64_t missesAfter = smmu.getCacheMissCount();

    // BEFORE FIX: invalidateByASID evicts Stream B's entry too → TLB MISS.
    //   missesAfter > missesBefore and hitsAfter == hitsBefore.
    // AFTER FIX:  only Stream A evicted → Stream B still cached → TLB HIT.
    //   hitsAfter > hitsBefore.
    EXPECT_GT(hitsAfter, hitsBefore)
        << "BUG-CPP-2: CMD_TLBI_NH_ASID(VMID=10, ASID=5) must NOT invalidate "
           "stream B's TLB entry (VMID=20, ASID=5). Expected a TLB HIT for "
           "stream B after the TLBI. Before fix, invalidateByASID() evicts "
           "all ASID=5 entries regardless of VMID, causing a TLB MISS here.";

    EXPECT_EQ(missesAfter, missesBefore)
        << "BUG-CPP-2: no new misses should occur for stream B after "
           "CMD_TLBI_NH_ASID targeted at VMID=10 (stream B has VMID=20)";

    // Separately verify that Stream A's entry WAS invalidated (it should miss).
    uint64_t missesMidA = smmu.getCacheMissCount();
    TranslationResult ra2 = smmu.translate(SID_A, 0, IOVA_A,
                                           AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(ra2.isOk()) << "stream A second translate must succeed";
    EXPECT_GT(smmu.getCacheMissCount(), missesMidA)
        << "BUG-CPP-2: Stream A's TLB entry (VMID=10, ASID=5) must be invalidated "
           "by CMD_TLBI_NH_ASID(VMID=10, ASID=5)";
}

// Test B2-2: CMD_TLBI_NH_ASID with matching VMID AND ASID must invalidate the
// correct entry (positive case — the entry that shares both VMID and ASID).
TEST(BugCpp2TlbiNhAsidVmid, TlbiNhAsidInvalidatesMatchingVMIDAndASID) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID  SID   = 0xB0u;
    static constexpr uint16_t  VMID  = 30u;
    static constexpr uint16_t  ASID  = 7u;
    static constexpr IOVA      IOVA  = 0x3000ULL;
    static constexpr PA        PA    = 0x30000ULL;

    ASSERT_TRUE(configureAndMapStream(smmu, SID, VMID, ASID, IOVA, PA));

    // Warm the TLB.
    TranslationResult r1 = smmu.translate(SID, 0, IOVA,
                                          AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "first translate must succeed";

    uint64_t missesBeforeTlbi = smmu.getCacheMissCount();

    // Issue CMD_TLBI_NH_ASID with the exact VMID+ASID for this stream.
    CommandEntry tlbi;
    tlbi.type  = CommandType::TLBI_NH_ASID;
    tlbi.asid  = ASID;
    tlbi.vmid  = VMID;
    ASSERT_TRUE(smmu.submitCommand(tlbi).isOk());
    smmu.processCommandQueue();

    // Second translate: TLB miss expected (entry was invalidated).
    TranslationResult r2 = smmu.translate(SID, 0, IOVA,
                                          AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r2.isOk()) << "second translate must succeed";

    EXPECT_GT(smmu.getCacheMissCount(), missesBeforeTlbi)
        << "CMD_TLBI_NH_ASID(VMID=" << VMID << ", ASID=" << ASID << ") must "
           "invalidate the TLB entry for this stream (expecting TLB miss)";
}

// ============================================================================
// BUG-CPP-3(a): NonSecure + STRW bit[0]=1 (EL2 or EL3) must be C_BAD_STE
//               when stage1Enabled=true
// ============================================================================

// Test B3a-1: NonSecure stream with STRW=EL2 (0b01, bit[0]=1) and
//             stage1Enabled=true must generate C_BAD_STE and return an error.
//
// BEFORE FIX: only EL3 (0b11) is checked; EL2 (0b01) is silently accepted.
// AFTER FIX:  bit[0]=1 check catches EL2 too → C_BAD_STE.
TEST(BugCpp3aStrwNonSecureEl2, NonSecureEl2Stage1EnabledIsCBadSTE) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xC0u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = StreamWorld::EL2;     // STRW=0b01 (bit[0]=1)
    // securityState defaults to NonSecure

    VoidResult result = smmu.configureStream(sid, cfg);

    // BEFORE FIX: returns Ok (EL2 was not caught, only EL3 was blocked).
    // AFTER FIX:  returns an error and emits C_BAD_STE.
    EXPECT_TRUE(result.isError())
        << "BUG-CPP-3(a): configureStream with NonSecure+EL2+stage1Enabled "
           "must return an error (C_BAD_STE). Before fix, EL2 was silently "
           "accepted because only STRW==EL3 was checked.";

    // C_BAD_STE event must be in the event queue.
    std::vector<EventEntry> events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE) { found = true; break; }
    }
    EXPECT_TRUE(found)
        << "BUG-CPP-3(a): a C_BAD_STE event must be emitted for NonSecure+EL2 "
           "with stage1Enabled=true (ARM §5.2 bit[0]=1 rule)";
}

// Test B3a-2: NonSecure + STRW=EL2 with stage1Enabled=FALSE (bypass mode,
//             strwUnused=true) must be ALLOWED — STRW is irrelevant without S1.
//
// This must pass both before and after the fix (regression guard).
TEST(BugCpp3aStrwNonSecureEl2, NonSecureEl2Stage1DisabledIsAllowed) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xC1u;
    StreamConfig cfg;
    cfg.translationEnabled = false;   // bypass — stage1Enabled irrelevant
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = StreamWorld::EL2;

    VoidResult result = smmu.configureStream(sid, cfg);
    EXPECT_TRUE(result.isOk())
        << "NonSecure+EL2 with stage1Enabled=false (strwUnused=true) must be "
           "allowed — STRW field is irrelevant when stage-1 is not active";
}

// Test B3a-3: NonSecure + STRW=EL1_EL0 (0b00, bit[0]=0) with stage1Enabled=true
// must be ALLOWED — only bit[0]=1 encodings are illegal for NonSecure.
//
// This must pass both before and after the fix (regression guard).
TEST(BugCpp3aStrwNonSecureEl2, NonSecureEl1El0Stage1EnabledIsAllowed) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xC2u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = StreamWorld::EL1_EL0;  // 0b00 — bit[0]=0

    VoidResult result = smmu.configureStream(sid, cfg);
    EXPECT_TRUE(result.isOk())
        << "NonSecure+EL1_EL0 (STRW=0b00, bit[0]=0) with stage1Enabled=true "
           "must be allowed per ARM §5.2";
}

// ============================================================================
// BUG-CPP-3(b): Secure + STRW=EL3 (0b11) must be C_BAD_STE when stage1Enabled
// ============================================================================

// Test B3b-1: Secure stream with STRW=EL3 (0b11) and stage1Enabled=true must
//             generate C_BAD_STE and return an error.
//
// BEFORE FIX: no check exists for Secure+EL3; configureStream returns Ok.
// AFTER FIX:  the new check fires → error + C_BAD_STE event.
TEST(BugCpp3bStrwSecureEl3, SecureEl3Stage1EnabledIsCBadSTE) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xD0u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = StreamWorld::EL3;        // STRW=0b11
    cfg.securityState      = SecurityState::Secure;   // Secure stream

    VoidResult result = smmu.configureStream(sid, cfg);

    // BEFORE FIX: returns Ok (no Secure+EL3 check existed).
    // AFTER FIX:  returns error, emits C_BAD_STE.
    EXPECT_TRUE(result.isError())
        << "BUG-CPP-3(b): configureStream with Secure+EL3+stage1Enabled must "
           "return an error (C_BAD_STE). Before fix, Secure+EL3 was accepted "
           "because only NonSecure+EL3 was checked.";

    std::vector<EventEntry> events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.type == EventType::C_BAD_STE) { found = true; break; }
    }
    EXPECT_TRUE(found)
        << "BUG-CPP-3(b): a C_BAD_STE event must be emitted for Secure+EL3 "
           "with stage1Enabled=true (ARM §5.2 SteIllegal() pseudocode)";
}

// Test B3b-2: Secure + STRW=EL2 (0b01) with stage1Enabled=true must be ALLOWED.
// ARM §5.2: only STRW=0b11 (EL3) is illegal for Secure streams.
//
// This must pass both before and after the fix (regression guard).
TEST(BugCpp3bStrwSecureEl3, SecureEl2Stage1EnabledIsAllowed) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xD1u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = StreamWorld::EL2;        // 0b01 — valid for Secure
    cfg.securityState      = SecurityState::Secure;

    VoidResult result = smmu.configureStream(sid, cfg);
    EXPECT_TRUE(result.isOk())
        << "Secure+EL2 (STRW=0b01) with stage1Enabled=true must be allowed "
           "per ARM §5.2 — only STRW=EL3 is illegal for Secure streams";
}

// Test B3b-3: Secure + STRW=EL3 with stage1Enabled=FALSE (strwUnused=true)
// must be ALLOWED — STRW is irrelevant when stage-1 is not active.
//
// This must pass both before and after the fix (regression guard).
TEST(BugCpp3bStrwSecureEl3, SecureEl3Stage1DisabledIsAllowed) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xD2u;
    StreamConfig cfg;
    cfg.translationEnabled = false;   // bypass
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = StreamWorld::EL3;
    cfg.securityState      = SecurityState::Secure;

    VoidResult result = smmu.configureStream(sid, cfg);
    EXPECT_TRUE(result.isOk())
        << "Secure+EL3 with stage1Enabled=false (strwUnused=true) must be "
           "allowed — STRW field is irrelevant without stage-1";
}
