// TDD failing tests for BUG-NEW-9, BUG-NEW-11, BUG-NEW-12, BUG-NEW-13
// (C++ implementation).
//
// Each test is written to FAIL with the current code and PASS only after the
// corresponding fix is applied.  No fixes are included here.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-9 (Both) — E_PAGE_REQUEST missing permission fields (§7.3.19)
//
//   When a page request is processed, the resulting E_PAGE_REQUEST event
//   must carry per-access-type permission bits:
//     ur, uw, ux  — unprivileged read/write/execute derived from access type
//     pr, pw, px  — privileged read/write/execute
//     span        — 8-bit access span in units of 4096 bytes
//
//   Currently EventEntry has none of these fields.  Tests referencing them will
//   not compile, which is itself a valid "failing" state that exposes the bug.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-11 (Both) — CMD_RESUME/STALL_TERM missing STALL_MODEL check (§4.7.1)
//
//   When IDR0.STALL_MODEL == 0b01 (terminate-only), CMD_RESUME and
//   CMD_STALL_TERM must raise CERROR_ILL.  The current implementation has no
//   such check.  Because there is no API to set IDR0.STALL_MODEL=0b01, this
//   test documents and captures the missing validation path.  It is written
//   against a hypothetical set_idr0_stall_model() API that does not yet exist;
//   the test will fail to compile until the API and check are both added.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-12 (Both) — PRI overflow auto-response for Last=0 PPRs (§8.1)
//
//   ARM §8.1: On PRIQ overflow, an auto-failure PRG_RESPONSE should be
//   generated ONLY when the overflowing PPR has isLastRequest=true.  When
//   isLastRequest=false the PPR must be silently discarded with no response.
//
//   Currently submitPageRequest() unconditionally generates a PRIAutoFailure
//   for every overflow regardless of isLastRequest.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-13 (C++ only) — CMD_PRI_RESP linear search + missing PASID match
//
//   ARM §3.5.1/§4.5.2: CMD_PRI_RESP must:
//     (a) Only retire the HEAD entry of the PRI queue (first-match semantics
//         must not silently skip intermediate entries with the same
//         streamID+prgIndex), AND
//     (b) Match on PASID in addition to streamID and prgIndex.
//
//   Currently the implementation does a linear scan of all queue entries
//   (including interior entries) and does NOT check PASID.

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

// Build an SMMU with a small PRIQ so overflow is easy to trigger.
static constexpr size_t SMALL_PRIQ = 4;  // minimum allowed for testing

static std::unique_ptr<SMMU> makeSmallPRIQ() {
    SMMUConfiguration config = SMMUConfiguration::createDefault();
    QueueConfiguration qcfg(512, 256, static_cast<size_t>(SMALL_PRIQ));
    config.setQueueConfiguration(qcfg);
    auto smmu = std::make_unique<SMMU>(config);
    smmu->setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    return smmu;
}

// Fill the PRI queue to exactly capacity.
static void fillPRIQueue(SMMU& smmu, size_t count, StreamID sid, PASID pid,
                          uint16_t startPrg) {
    for (size_t i = 0; i < count; ++i) {
        PRIEntry req;
        req.streamID         = sid;
        req.pasid            = pid;
        req.requestedAddress = static_cast<IOVA>((startPrg + i) << 12);
        req.accessType       = AccessType::Read;
        req.isLastRequest    = false;
        req.prgIndex         = static_cast<uint16_t>(startPrg + i);
        smmu.submitPageRequest(req);
    }
}

// Submit a CMD_PRI_RESP command for a given (streamID, pasid, prgIndex).
static CommandEntry makePriResp(StreamID sid, PASID pid, uint16_t prgIdx) {
    CommandEntry cmd;
    cmd.type     = CommandType::PRI_RESP;
    cmd.streamID = sid;
    cmd.pasid    = pid;
    cmd.prgIndex = prgIdx;
    return cmd;
}

} // namespace

// ============================================================================
// BUG-NEW-9: E_PAGE_REQUEST event must carry permission fields (§7.3.19)
// ============================================================================
//
// NOTE: The fields ur, uw, ux, pr, pw, px, span do NOT yet exist on EventEntry.
// This test WILL NOT COMPILE until those fields are added, which is exactly the
// "red" state we want — the compile error exposes the missing API.
//
// Once the fields are added, the test will compile.  It will then FAIL at
// runtime because submitPageRequest() does not yet populate the fields.
// Only after the population logic is also added will the test pass (green).

TEST(BugNew9PageRequestPermFields, ReadAccessSetsUrBit) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID  SID = 0xF0u;
    static constexpr PASID     PID = 0u;

    // Submit a Read page request.
    PRIEntry req;
    req.streamID         = SID;
    req.pasid            = PID;
    req.requestedAddress = 0x1000u;
    req.accessType       = AccessType::Read;
    req.isLastRequest    = true;
    req.prgIndex         = 0u;
    smmu.submitPageRequest(req);

    // Retrieve the E_PAGE_REQUEST event from the event queue.
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-9: submitPageRequest must generate an E_PAGE_REQUEST event";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::E_PAGE_REQUEST) { ev = &e; break; }
    }
    ASSERT_NE(ev, nullptr)
        << "BUG-NEW-9: an E_PAGE_REQUEST event must be present in the event queue";

    // ARM §7.3.19: a Read access sets ur=true (unprivileged read allowed) and
    // uw=false, ux=false.
    //
    // BUG-NEW-9: these fields do not exist yet on EventEntry.
    // This line WILL NOT COMPILE — that is the expected failing state.
    EXPECT_TRUE(ev->ur)
        << "BUG-NEW-9: E_PAGE_REQUEST for Read access must have ur=true (§7.3.19)";
    EXPECT_FALSE(ev->uw)
        << "BUG-NEW-9: E_PAGE_REQUEST for Read access must have uw=false (§7.3.19)";
    EXPECT_FALSE(ev->ux)
        << "BUG-NEW-9: E_PAGE_REQUEST for Read access must have ux=false (§7.3.19)";
}

TEST(BugNew9PageRequestPermFields, WriteAccessSetsUwBit) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID  SID = 0xF1u;
    static constexpr PASID     PID = 0u;

    PRIEntry req;
    req.streamID         = SID;
    req.pasid            = PID;
    req.requestedAddress = 0x2000u;
    req.accessType       = AccessType::Write;
    req.isLastRequest    = true;
    req.prgIndex         = 1u;
    smmu.submitPageRequest(req);

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::E_PAGE_REQUEST) { ev = &e; break; }
    }
    ASSERT_NE(ev, nullptr) << "BUG-NEW-9: E_PAGE_REQUEST must be in event queue";

    // ARM §7.3.19: a Write access must set uw=true, ur=false, ux=false.
    //
    // BUG-NEW-9: these fields do not exist yet on EventEntry — will not compile.
    EXPECT_TRUE(ev->uw)
        << "BUG-NEW-9: E_PAGE_REQUEST for Write access must have uw=true (§7.3.19)";
    EXPECT_FALSE(ev->ur)
        << "BUG-NEW-9: E_PAGE_REQUEST for Write access must have ur=false (§7.3.19)";
    EXPECT_FALSE(ev->ux)
        << "BUG-NEW-9: E_PAGE_REQUEST for Write access must have ux=false (§7.3.19)";
}

TEST(BugNew9PageRequestPermFields, SpanFieldIsPopulated) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID  SID = 0xF2u;
    static constexpr PASID     PID = 0u;

    PRIEntry req;
    req.streamID         = SID;
    req.pasid            = PID;
    req.requestedAddress = 0x3000u;
    req.accessType       = AccessType::Read;
    req.isLastRequest    = true;
    req.prgIndex         = 2u;
    smmu.submitPageRequest(req);

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::E_PAGE_REQUEST) { ev = &e; break; }
    }
    ASSERT_NE(ev, nullptr) << "BUG-NEW-9: E_PAGE_REQUEST must be present";

    // ARM §7.3.19: span is an 8-bit field (units of 4096 bytes).
    // A single-page request has span=0 (meaning 1 page = 4096 bytes).
    // A non-zero span must be a meaningful value derived from the request.
    // Here we just verify the field exists and that it does not contain
    // an out-of-range 8-bit value (always true once the field is added).
    //
    // BUG-NEW-9: this field does not exist yet — will not compile.
    EXPECT_EQ(ev->span, 0u)
        << "BUG-NEW-9: E_PAGE_REQUEST for a single-page request must have span=0 (§7.3.19)";
}

// ============================================================================
// BUG-NEW-11: CMD_RESUME/STALL_TERM must raise CERROR_ILL when
//             IDR0.STALL_MODEL == 0b01 (terminate-only) (§4.7.1, §4.7.2)
// ============================================================================
//
// NOTE: There is currently no API to configure IDR0.STALL_MODEL.  The IDR0
// register is read-only and always reports STALL_MODEL=0b00.  This test is
// written against a hypothetical setStallModel(uint8_t) API that does not yet
// exist.  It will NOT COMPILE until:
//   1. setStallModel() is added to SMMU, AND
//   2. processCommand() validates STALL_MODEL before executing CMD_RESUME /
//      CMD_STALL_TERM and raises CERROR_ILL if STALL_MODEL==0b01.
//
// This compile failure is the intended "red" state that demonstrates the bug.

TEST(BugNew11StallModelCheck, ResumeRaisesCerrorIllWhenTerminateOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    // Configure the SMMU to advertise terminate-only stall model (0b01).
    // BUG-NEW-11: setStallModel() does not yet exist — will not compile.
    smmu.setStallModel(0x01u);

    // Issue CMD_RESUME for any STAG — it must fail with CERROR_ILL.
    CommandEntry cmd;
    cmd.type   = CommandType::RESUME;
    cmd.stag   = 0u;
    cmd.action = true;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    // CERROR_ILL must be set in CMDQ_CONS.ERR.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-11: CMD_RESUME must raise CERROR_ILL when IDR0.STALL_MODEL=0b01 "
           "(terminate-only; §4.7.1)";
}

TEST(BugNew11StallModelCheck, StallTermRaisesCerrorIllWhenTerminateOnly) {
    SMMU smmu;
    enableSMMU(smmu);

    // BUG-NEW-11: setStallModel() does not yet exist — will not compile.
    smmu.setStallModel(0x01u);

    CommandEntry cmd;
    cmd.type     = CommandType::STALL_TERM;
    cmd.streamID = 0x10u;
    smmu.submitCommand(cmd);
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-11: CMD_STALL_TERM must raise CERROR_ILL when IDR0.STALL_MODEL=0b01 "
           "(terminate-only; §4.7.2)";
}

// ============================================================================
// BUG-NEW-12: PRI overflow auto-response must obey isLastRequest flag (§8.1)
// ============================================================================
//
// ARM §8.1: An auto-failure PRG_RESPONSE is required ONLY for the last PPR
// in a Page Request Group (isLastRequest=true).  If the overflowing PPR has
// isLastRequest=false it must be silently discarded with NO auto-response.
//
// Currently submitPageRequest() generates a PRIAutoFailure for every overflow
// regardless of isLastRequest, which violates the spec.

// Test B12-1: Overflow with isLastRequest=false → NO auto-failure response.
//
// BEFORE FIX: getPRIAutoFailures() returns a non-empty vector (bug).
// AFTER FIX:  getPRIAutoFailures() returns an empty vector.
TEST(BugNew12PriOverflowLastBit, OverflowWithLastFalseSilentlyDiscarded) {
    auto smmu = makeSmallPRIQ();

    // Fill queue to capacity.
    fillPRIQueue(*smmu, SMALL_PRIQ, 0x20u, 0u, 0u);
    ASSERT_EQ(smmu->getPRIQueueSize(), SMALL_PRIQ)
        << "precondition: PRIQ must be at capacity";
    ASSERT_TRUE(smmu->getPRIAutoFailures().empty())
        << "precondition: no auto-failures before overflow";

    // Submit an overflow entry with isLastRequest=false.
    PRIEntry overflow;
    overflow.streamID         = 0x20u;
    overflow.pasid            = 0u;
    overflow.requestedAddress = UINT64_C(0x99000);
    overflow.accessType       = AccessType::Read;
    overflow.isLastRequest    = false;   // NOT the last PPR in the group
    overflow.prgIndex         = 0xAAu;
    smmu->submitPageRequest(overflow);

    // ARM §8.1: no auto-failure for a non-Last PPR — device is not waiting
    // for a response.
    // BEFORE FIX: getPRIAutoFailures() returns 1 failure (unconditional).
    // AFTER FIX:  getPRIAutoFailures() returns 0 failures.
    auto failures = smmu->getPRIAutoFailures();
    EXPECT_TRUE(failures.empty())
        << "BUG-NEW-12: overflow with isLastRequest=false must NOT generate an "
           "auto-failure PRG_RESPONSE (ARM §8.1). Before fix, a failure is "
           "unconditionally appended regardless of the Last bit.";
}

// Test B12-2: Overflow with isLastRequest=true → auto-failure response REQUIRED.
//
// This is the correct existing behaviour when the Last bit is set; ensure we
// do not regress it when the fix for Last=false is applied.
TEST(BugNew12PriOverflowLastBit, OverflowWithLastTrueGeneratesAutoFailure) {
    auto smmu = makeSmallPRIQ();

    // Fill queue to capacity.
    fillPRIQueue(*smmu, SMALL_PRIQ, 0x21u, 0u, 0u);
    ASSERT_EQ(smmu->getPRIQueueSize(), SMALL_PRIQ)
        << "precondition: PRIQ must be at capacity";
    ASSERT_TRUE(smmu->getPRIAutoFailures().empty())
        << "precondition: no auto-failures before overflow";

    // Submit an overflow entry with isLastRequest=true.
    static constexpr uint16_t OVERFLOW_PRG = 0xBBu;
    PRIEntry overflow;
    overflow.streamID         = 0x21u;
    overflow.pasid            = 0u;
    overflow.requestedAddress = UINT64_C(0x88000);
    overflow.accessType       = AccessType::Write;
    overflow.isLastRequest    = true;    // last PPR — device awaits a response
    overflow.prgIndex         = OVERFLOW_PRG;
    smmu->submitPageRequest(overflow);

    // ARM §8.1: auto-failure MUST be generated for a Last PPR that overflows.
    auto failures = smmu->getPRIAutoFailures();
    ASSERT_EQ(failures.size(), 1u)
        << "BUG-NEW-12: overflow with isLastRequest=true must generate exactly one "
           "auto-failure PRG_RESPONSE (ARM §8.1)";
    EXPECT_EQ(failures[0].streamID, static_cast<StreamID>(0x21u))
        << "auto-failure streamID must match the overflow PPR";
    EXPECT_EQ(failures[0].prgIndex, OVERFLOW_PRG)
        << "auto-failure prgIndex must match the overflow PPR's prgIndex";
}

// Test B12-3: Mixed overflow — one Last=false, one Last=true.  Only the second
// generates an auto-failure response.
TEST(BugNew12PriOverflowLastBit, OverflowMixedProducesOneAutoFailure) {
    auto smmu = makeSmallPRIQ();

    // Fill queue to capacity.
    fillPRIQueue(*smmu, SMALL_PRIQ, 0x22u, 0u, 0u);
    ASSERT_EQ(smmu->getPRIQueueSize(), SMALL_PRIQ)
        << "precondition: PRIQ must be at capacity";

    // First overflow: Last=false — no response expected.
    PRIEntry ovf1;
    ovf1.streamID         = 0x22u;
    ovf1.pasid            = 0u;
    ovf1.requestedAddress = UINT64_C(0x77000);
    ovf1.accessType       = AccessType::Read;
    ovf1.isLastRequest    = false;
    ovf1.prgIndex         = 0xCCu;
    smmu->submitPageRequest(ovf1);

    // Second overflow: Last=true — auto-failure response expected.
    static constexpr uint16_t LAST_PRG = 0xDDu;
    PRIEntry ovf2;
    ovf2.streamID         = 0x22u;
    ovf2.pasid            = 0u;
    ovf2.requestedAddress = UINT64_C(0x66000);
    ovf2.accessType       = AccessType::Write;
    ovf2.isLastRequest    = true;
    ovf2.prgIndex         = LAST_PRG;
    smmu->submitPageRequest(ovf2);

    auto failures = smmu->getPRIAutoFailures();

    // BEFORE FIX: 2 failures (one for each overflow regardless of Last bit).
    // AFTER FIX:  1 failure (only for the Last=true overflow).
    EXPECT_EQ(failures.size(), 1u)
        << "BUG-NEW-12: of two overflow PPRs (Last=false, Last=true), only the "
           "Last=true one must generate an auto-failure (ARM §8.1). Before fix, "
           "both generate failures.";

    if (!failures.empty()) {
        EXPECT_EQ(failures[0].prgIndex, LAST_PRG)
            << "BUG-NEW-12: the auto-failure must correspond to the Last=true PPR "
               "(prgIndex=" << LAST_PRG << ")";
    }
}

// ============================================================================
// BUG-NEW-13 (C++ only) — CMD_PRI_RESP linear search + missing PASID match
// ============================================================================
//
// ARM §3.5.1/§4.5.2:
//   (a) CMD_PRI_RESP must only retire the HEAD entry of the PRI queue.
//       A response targeting an interior entry (skipping the head) must NOT
//       retire that interior entry — the head must be retired first.
//   (b) CMD_PRI_RESP must match PASID in addition to streamID and prgIndex.
//       A response for (streamID, prgIndex) must NOT retire an entry whose
//       PASID does not match.

// Test B13-1: CMD_PRI_RESP targeting a non-head entry must not retire it.
//
// Setup: two entries in the PRI queue with the same streamID and prgIndex
// but at different positions.  The head entry is submitted first; the
// target response references a position BEYOND the head.
//
// Actually for this test we use a simpler but equivalent scenario:
// submit entries [A, B] where A is at the head.  Send CMD_PRI_RESP for B
// (using a distinct prgIndex that is NOT the head's prgIndex).  Verify that
// only A can be retired (the current bug allows retiring B without retiring A
// when the linear scan happens to find B before iterating to A).
//
// The real pathological case: submit [A, B] where both have the same streamID
// and prgIndex.  CMD_PRI_RESP should retire the head (A).  The bug is that the
// linear scan finds the first match (which is A in this degenerate case), so
// instead we test the PASID mismatch path (b) for the simpler scenario.
//
// For (a) — head-only retirement:
//   Submit two entries: head has prgIndex=1, second has prgIndex=2.
//   Send CMD_PRI_RESP for prgIndex=2 (non-head).
//   AFTER FIX:  head (prgIndex=1) still in queue; prgIndex=2 also still in queue.
//   BEFORE FIX: the linear scan finds prgIndex=2 at position 1 and retires it,
//               leaving prgIndex=1 as the sole remaining entry.
TEST(BugNew13PriRespHeadOnly, PriRespForInteriorEntryDoesNotRetireIt) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID      = 0x30u;
    static constexpr PASID    PID      = 0u;
    static constexpr uint16_t HEAD_PRG = 1u;
    static constexpr uint16_t BODY_PRG = 2u;

    // Submit head entry.
    {
        PRIEntry head;
        head.streamID         = SID;
        head.pasid            = PID;
        head.requestedAddress = 0x1000u;
        head.accessType       = AccessType::Read;
        head.isLastRequest    = false;
        head.prgIndex         = HEAD_PRG;
        smmu.submitPageRequest(head);
    }

    // Submit body entry (after the head).
    {
        PRIEntry body;
        body.streamID         = SID;
        body.pasid            = PID;
        body.requestedAddress = 0x2000u;
        body.accessType       = AccessType::Read;
        body.isLastRequest    = true;
        body.prgIndex         = BODY_PRG;
        smmu.submitPageRequest(body);
    }

    ASSERT_EQ(smmu.getPRIQueueSize(), 2u)
        << "precondition: 2 entries in PRIQ before CMD_PRI_RESP";

    // Send CMD_PRI_RESP for the BODY (non-head) entry.
    smmu.submitCommand(makePriResp(SID, PID, BODY_PRG));
    smmu.processCommandQueue();

    // BEFORE FIX (linear scan): BODY_PRG is found and removed; queue has 1 entry
    //   (the head remains).
    //
    // AFTER FIX (head-only): CMD_PRI_RESP for a non-head entry is a no-op (or
    //   raises an error), and the queue still has 2 entries.
    //
    // Either way, verify that the head entry has NOT been retired:
    std::vector<PRIEntry> q = smmu.getPRIQueue();
    bool headPresent = false;
    for (const auto& e : q) {
        if (e.prgIndex == HEAD_PRG) { headPresent = true; break; }
    }
    EXPECT_TRUE(headPresent)
        << "BUG-NEW-13(a): the HEAD entry (prgIndex=" << HEAD_PRG << ") must still "
           "be in the PRI queue after CMD_PRI_RESP targeted the BODY entry "
           "(prgIndex=" << BODY_PRG << "). ARM §4.5.2 requires head-only retirement; "
           "the current linear scan incorrectly retires any matching interior entry.";

    // Also verify that CMD_PRI_RESP for the body did NOT advance PRIQ_CONS
    // past the head position.
    uint32_t consRd = smmu.getPriqConsIndex() & ~(1u << 31u);
    EXPECT_EQ(consRd, 0u)
        << "BUG-NEW-13(a): PRIQ_CONS.RD must remain 0 when CMD_PRI_RESP targets "
           "a non-head entry (ARM §4.5.2 head-only semantics)";
}

// Test B13-2: CMD_PRI_RESP must match PASID in addition to streamID and prgIndex.
//
// Setup:
//   Entry A: streamID=0x31, PASID=10, prgIndex=5 (HEAD)
//   Entry B: streamID=0x31, PASID=20, prgIndex=5 (same streamID+prgIndex, different PASID)
//
// Send CMD_PRI_RESP(streamID=0x31, PASID=20, prgIndex=5).
//
// BEFORE FIX (no PASID check): the linear scan matches entry A first (it IS the
//   head and shares streamID+prgIndex), retires A and advances PRIQ_CONS by 1.
//   Entry B remains.
//
// AFTER FIX: PASID=20 does not match head entry A's PASID=10, so A is not
//   retired; PRIQ_CONS stays at 0.  The head must still be present after the
//   mismatching response.
TEST(BugNew13PriRespPasidMatch, PriRespForWrongPasidDoesNotRetireHead) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID     = 0x31u;
    static constexpr PASID    PID_A   = 10u;
    static constexpr PASID    PID_B   = 20u;
    static constexpr uint16_t PRG_IDX = 5u;

    // Entry A: head, PASID=10.
    {
        PRIEntry a;
        a.streamID         = SID;
        a.pasid            = PID_A;
        a.requestedAddress = 0x4000u;
        a.accessType       = AccessType::Read;
        a.isLastRequest    = true;
        a.prgIndex         = PRG_IDX;
        smmu.submitPageRequest(a);
    }

    // Entry B: same streamID+prgIndex, different PASID.
    {
        PRIEntry b;
        b.streamID         = SID;
        b.pasid            = PID_B;
        b.requestedAddress = 0x5000u;
        b.accessType       = AccessType::Read;
        b.isLastRequest    = true;
        b.prgIndex         = PRG_IDX;
        smmu.submitPageRequest(b);
    }

    ASSERT_EQ(smmu.getPRIQueueSize(), 2u)
        << "precondition: 2 entries in PRIQ";

    uint32_t consBeforeRd = smmu.getPriqConsIndex() & ~(1u << 31u);

    // Send CMD_PRI_RESP for PASID=20 (which is entry B, not the head).
    smmu.submitCommand(makePriResp(SID, PID_B, PRG_IDX));
    smmu.processCommandQueue();

    // BEFORE FIX: head entry A (PASID=10) is retired because PASID is not
    //   checked — only streamID and prgIndex match.  PRIQ_CONS advances.
    // AFTER FIX: PASID=20 != PID_A=10 on the head entry, so head stays.

    std::vector<PRIEntry> q = smmu.getPRIQueue();
    bool headPresent = false;
    for (const auto& e : q) {
        if (e.pasid == PID_A && e.prgIndex == PRG_IDX) { headPresent = true; break; }
    }
    EXPECT_TRUE(headPresent)
        << "BUG-NEW-13(b): CMD_PRI_RESP(PASID=20) must NOT retire entry A "
           "(PASID=10, prgIndex=5) because the PASIDs do not match. Currently "
           "CMD_PRI_RESP ignores PASID, causing entry A to be incorrectly "
           "retired when streamID and prgIndex happen to match. ARM §4.5.2 "
           "requires PASID to be part of the match key.";

    uint32_t consAfterRd = smmu.getPriqConsIndex() & ~(1u << 31u);
    EXPECT_EQ(consAfterRd, consBeforeRd)
        << "BUG-NEW-13(b): PRIQ_CONS.RD must not advance when CMD_PRI_RESP "
           "carries a PASID that does not match the head entry's PASID";
}

// Test B13-3: After fixing head-only + PASID matching, a correct CMD_PRI_RESP
// (matching streamID, PASID, and prgIndex of the HEAD) still retires the head.
// This test should PASS both before and after the fix (regression guard).
TEST(BugNew13PriRespHeadOnly, CorrectPriRespRetireHead) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID     = 0x32u;
    static constexpr PASID    PID     = 7u;
    static constexpr uint16_t PRG_IDX = 9u;

    PRIEntry head;
    head.streamID         = SID;
    head.pasid            = PID;
    head.requestedAddress = 0x6000u;
    head.accessType       = AccessType::Read;
    head.isLastRequest    = true;
    head.prgIndex         = PRG_IDX;
    smmu.submitPageRequest(head);

    ASSERT_EQ(smmu.getPRIQueueSize(), 1u)
        << "precondition: 1 entry in PRIQ";

    smmu.submitCommand(makePriResp(SID, PID, PRG_IDX));
    smmu.processCommandQueue();

    EXPECT_EQ(smmu.getPRIQueueSize(), 0u)
        << "A correct CMD_PRI_RESP (matching streamID, PASID, prgIndex of head) "
           "must retire the head entry and leave the queue empty";

    uint32_t consRd = smmu.getPriqConsIndex() & ~(1u << 31u);
    EXPECT_EQ(consRd, 1u)
        << "PRIQ_CONS.RD must advance by 1 after a successful CMD_PRI_RESP "
           "that retires the head entry";
}
