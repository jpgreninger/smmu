// ARM SMMU v3 FINDING-NEW-25, NEW-26, NEW-27, NEW-28 spec tests
// Copyright (c) 2024 John Greninger
//
// TDD tests for:
// - FINDING-NEW-25: TLB fast-path permission fault bypasses stall mode check (§3.12.2)
// - FINDING-NEW-26: Stall EventEntry missing STAG field (§3.12.2, §7.3)
// - FINDING-NEW-27: CMD_SYNC CS field not modeled; SIG_NONE generates spurious event (§4.8)
// - FINDING-NEW-28: generateEvent() sets incorrect errorCode values (§7.3)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <algorithm>

namespace smmu {
namespace test {

// ─── Test fixtures ────────────────────────────────────────────────────────────

static constexpr StreamID N25_STREAM_A = 0xB0;
static constexpr StreamID N25_STREAM_B = 0xB1;
static constexpr StreamID N26_STREAM_A = 0xC0;
static constexpr StreamID N26_STREAM_B = 0xC1;
static constexpr StreamID N27_STREAM   = 0xD0;
static constexpr StreamID N28_STREAM   = 0xE0;
static constexpr PASID    TEST_PASID   = 0;
static constexpr IOVA     PAGE_A       = 0x5000ULL;
static constexpr IOVA     PAGE_B       = 0x6000ULL;
static constexpr IOVA     PAGE_C       = 0x7000ULL;
static constexpr IOVA     PAGE_D       = 0x8000ULL;
static constexpr PA       PHYS_BASE    = 0x9000'0000ULL;

static void basicStream(SMMU& smmu, StreamID sid, FaultMode fm = FaultMode::Terminate) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = fm;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, TEST_PASID).isOk());
}

static int countEvents(SMMU& smmu, EventType type) {
    int n = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == type) { ++n; }
    }
    return n;
}

// ─── FINDING-NEW-25: TLB fast-path must respect FaultMode::Stall (§3.12.2) ───

// When a stream is configured with FaultMode::Stall and the TLB has a cached
// read-only entry, a write attempt via the fast-path must return Stalled,
// not PagePermissionViolation.
TEST(New25Spec, StallMode_TLBCacheHit_PermFault_ReturnsStalled) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->enableCaching(true);
    basicStream(*smmu, N25_STREAM_A, FaultMode::Stall);

    PagePermissions roPerms(true, false, false);  // read=true, write=false, exec=false
    ASSERT_TRUE(smmu->mapPage(N25_STREAM_A, TEST_PASID, PAGE_A, PHYS_BASE, roPerms).isOk());

    // Warm TLB with a successful read
    auto r1 = smmu->translate(N25_STREAM_A, TEST_PASID, PAGE_A,
                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk()) << "Initial read must succeed to populate TLB";

    smmu->clearEventQueue();

    // Write via TLB fast-path → stall mode must intercept before returning the raw fault
    auto r2 = smmu->translate(N25_STREAM_A, TEST_PASID, PAGE_A,
                               AccessType::Write, SecurityState::NonSecure);
    ASSERT_TRUE(r2.isError()) << "Write on read-only TLB entry must fail";
    EXPECT_EQ(r2.getError(), SMMUError::Stalled)
        << "§3.12.2: TLB fast-path permission fault must be Stalled when FaultMode::Stall";
}

// A StallRecord must be enqueued when the fast-path stalls a transaction.
TEST(New25Spec, StallMode_TLBCacheHit_PermFault_EnqueuesStallRecord) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->enableCaching(true);
    basicStream(*smmu, N25_STREAM_B, FaultMode::Stall);

    PagePermissions roPerms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(N25_STREAM_B, TEST_PASID, PAGE_B, PHYS_BASE + 0x1000, roPerms).isOk());

    auto r1 = smmu->translate(N25_STREAM_B, TEST_PASID, PAGE_B,
                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk());

    auto r2 = smmu->translate(N25_STREAM_B, TEST_PASID, PAGE_B,
                               AccessType::Write, SecurityState::NonSecure);
    ASSERT_EQ(r2.getError(), SMMUError::Stalled);

    EXPECT_GT(smmu->getStalledTransactionCount(), 0u)
        << "§3.12.2: A StallRecord must be enqueued for a TLB fast-path permission fault in stall mode";
}

// Non-stall streams must still return PagePermissionViolation on TLB fast-path fault
// (regression guard: stall fix must not change terminate-mode behaviour).
TEST(New25Spec, TerminateMode_TLBCacheHit_PermFault_ReturnsPermViolation) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->enableCaching(true);
    basicStream(*smmu, N25_STREAM_A + 0x10, FaultMode::Terminate);

    PagePermissions roPerms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(N25_STREAM_A + 0x10, TEST_PASID, PAGE_A, PHYS_BASE, roPerms).isOk());

    auto r1 = smmu->translate(N25_STREAM_A + 0x10, TEST_PASID, PAGE_A,
                               AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk());

    auto r2 = smmu->translate(N25_STREAM_A + 0x10, TEST_PASID, PAGE_A,
                               AccessType::Write, SecurityState::NonSecure);
    ASSERT_TRUE(r2.isError());
    EXPECT_EQ(r2.getError(), SMMUError::PagePermissionViolation)
        << "FaultMode::Terminate stream must still return PagePermissionViolation on TLB fast-path fault";
}

// ─── FINDING-NEW-26: Stall EventEntry must carry the STAG field (§3.12.2) ───

// When a stall event is placed in the event queue, the EventEntry.stag field
// must match the STAG allocated for the stalled transaction.
TEST(New26Spec, StallEvent_EventEntry_HasNonZeroStag) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N26_STREAM_A, FaultMode::Stall);

    // Attempt an unmapped translation to trigger a stall fault
    auto r = smmu->translate(N26_STREAM_A, TEST_PASID, PAGE_C,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(r.getError(), SMMUError::Stalled) << "Translation must stall";

    // Find the stall event in the event queue
    auto events = smmu->getEventQueue();
    auto it = std::find_if(events.begin(), events.end(),
                           [](const EventEntry& e) { return e.stall; });
    ASSERT_NE(it, events.end()) << "A stall event must be present in the event queue";

    // §3.12.2: The STAG field must be populated so software can correlate the
    // event to the correct CMD_RESUME command.
    EXPECT_NE(it->stag, 0u)
        << "§3.12.2: EventEntry.stag must be non-zero for stall events";
}

// The stag in the EventEntry must match the STAG of the corresponding StallRecord.
TEST(New26Spec, StallEvent_EventEntryStag_MatchesStallRecord) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N26_STREAM_B, FaultMode::Stall);

    auto r = smmu->translate(N26_STREAM_B, TEST_PASID, PAGE_D,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_EQ(r.getError(), SMMUError::Stalled);

    // Retrieve the allocated STAG from the stall queue
    auto stalls = smmu->getStalledTransactions();
    ASSERT_FALSE(stalls.empty()) << "At least one StallRecord must exist";
    uint16_t expectedStag = stalls.front().stag;

    // Verify EventEntry.stag matches
    auto events = smmu->getEventQueue();
    auto it = std::find_if(events.begin(), events.end(),
                           [](const EventEntry& e) { return e.stall; });
    ASSERT_NE(it, events.end());
    EXPECT_EQ(it->stag, expectedStag)
        << "§3.12.2: EventEntry.stag must match the StallRecord.stag allocated for the same fault";
}

// Non-stall events must have stag==0 (no stall tag applies).
TEST(New26Spec, NonStallEvent_EventEntry_HasZeroStag) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N26_STREAM_A + 0x10, FaultMode::Terminate);

    // Unmapped address → F_TRANSLATION (no stall)
    auto r = smmu->translate(N26_STREAM_A + 0x10, TEST_PASID, PAGE_C,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r.isError());

    auto events = smmu->getEventQueue();
    ASSERT_FALSE(events.empty());
    for (const auto& e : events) {
        if (!e.stall) {
            EXPECT_EQ(e.stag, 0u)
                << "Non-stall EventEntry must have stag==0";
        }
    }
}

// ─── FINDING-NEW-27: CMD_SYNC CS field must control completion signal (§4.8) ───

// CMD_SYNC with CS=0b00 (SIG_NONE) must NOT generate a COMMAND_SYNC_COMPLETION event.
TEST(New27Spec, SYNC_CS_SIG_NONE_DoesNotGenerateCompletionEvent) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs = 0x00; // SIG_NONE — no completion signal
    ASSERT_TRUE(smmu->submitCommand(sync).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::COMMAND_SYNC_COMPLETION), 0)
        << "§4.8: CMD_SYNC with CS=SIG_NONE (0b00) must not generate COMMAND_SYNC_COMPLETION";
}

// CMD_SYNC with CS=0b01 (SIG_IRQ) must generate exactly one COMMAND_SYNC_COMPLETION event.
TEST(New27Spec, SYNC_CS_SIG_IRQ_GeneratesCompletionEvent) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs = 0x01; // SIG_IRQ — generate completion event
    ASSERT_TRUE(smmu->submitCommand(sync).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::COMMAND_SYNC_COMPLETION), 1)
        << "§4.8: CMD_SYNC with CS=SIG_IRQ (0b01) must generate exactly one COMMAND_SYNC_COMPLETION";
}

// CMD_SYNC with CS=0b10 (SIG_MSI) must also generate a completion event.
TEST(New27Spec, SYNC_CS_SIG_MSI_GeneratesCompletionEvent) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    CommandEntry sync;
    sync.type = CommandType::SYNC;
    sync.cs = 0x02; // SIG_MSI
    ASSERT_TRUE(smmu->submitCommand(sync).isOk());
    smmu->processCommandQueue();

    EXPECT_EQ(countEvents(*smmu, EventType::COMMAND_SYNC_COMPLETION), 1)
        << "§4.8: CMD_SYNC with CS=SIG_MSI (0b10) must generate exactly one COMMAND_SYNC_COMPLETION";
}

// Multiple SYNCs with different CS values: only those with CS!=0 generate events.
TEST(New27Spec, Multiple_SYNC_Mixed_CS_CorrectEventCount) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    // Submit three SYNCs: SIG_NONE, SIG_IRQ, SIG_NONE  → expect exactly 1 event
    for (uint8_t cs : {0x00, 0x01, 0x00}) {
        CommandEntry sync;
        sync.type = CommandType::SYNC;
        sync.cs   = cs;
        ASSERT_TRUE(smmu->submitCommand(sync).isOk());
        smmu->processCommandQueue();
    }

    EXPECT_EQ(countEvents(*smmu, EventType::COMMAND_SYNC_COMPLETION), 1)
        << "§4.8: Only CMD_SYNC with CS!=SIG_NONE must generate completion events";
}

// ─── FINDING-NEW-28: generateEvent() errorCode must be 0 (§7.3) ───

// F_TRANSLATION events must have errorCode==0 (the EventType discriminant
// itself identifies the fault; a secondary errorCode field is not defined
// by ARM §7.3 and was incorrectly set to 0x01).
TEST(New28Spec, FTranslation_Event_ErrorCode_IsZero) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N28_STREAM, FaultMode::Terminate);

    // Unmapped address → F_TRANSLATION event
    auto r = smmu->translate(N28_STREAM, TEST_PASID, 0xDEAD'0000ULL,
                              AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r.isError());

    auto events = smmu->getEventQueue();
    auto it = std::find_if(events.begin(), events.end(),
                           [](const EventEntry& e) { return e.type == EventType::F_TRANSLATION; });
    ASSERT_NE(it, events.end()) << "F_TRANSLATION event must be present";
    EXPECT_EQ(it->errorCode, 0u)
        << "§7.3: F_TRANSLATION EventEntry.errorCode must be 0 (was incorrectly set to 0x01)";
}

// F_PERMISSION events must have errorCode==0.
TEST(New28Spec, FPermission_Event_ErrorCode_IsZero) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N28_STREAM + 1, FaultMode::Terminate);

    PagePermissions roPerms(true, false, false);
    ASSERT_TRUE(smmu->mapPage(N28_STREAM + 1, TEST_PASID, 0x1000ULL, PHYS_BASE, roPerms).isOk());

    auto r = smmu->translate(N28_STREAM + 1, TEST_PASID, 0x1000ULL,
                              AccessType::Write, SecurityState::NonSecure);
    ASSERT_TRUE(r.isError());

    auto events = smmu->getEventQueue();
    auto it = std::find_if(events.begin(), events.end(),
                           [](const EventEntry& e) { return e.type == EventType::F_PERMISSION; });
    ASSERT_NE(it, events.end()) << "F_PERMISSION event must be present";
    EXPECT_EQ(it->errorCode, 0u)
        << "§7.3: F_PERMISSION EventEntry.errorCode must be 0 (was incorrectly set to 0x02)";
}

// All event types generated internally must have errorCode==0.
TEST(New28Spec, AllFaultEvents_ErrorCode_IsZero) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    basicStream(*smmu, N28_STREAM + 2, FaultMode::Terminate);

    // F_TRANSLATION
    smmu->translate(N28_STREAM + 2, TEST_PASID, 0xBAD'0000ULL,
                    AccessType::Read, SecurityState::NonSecure);

    for (const auto& e : smmu->getEventQueue()) {
        EXPECT_EQ(e.errorCode, 0u)
            << "§7.3: All EventEntry.errorCode fields must be 0; event type=" << static_cast<int>(e.type);
    }
}

} // namespace test
} // namespace smmu
