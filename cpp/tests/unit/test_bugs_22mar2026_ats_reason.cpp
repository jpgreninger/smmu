// Tests for 2026-03-22 fixes: ATS R/W/X/P fields, F_UUT reason field, rnw comment polarity.
// TDD: all tests are written to FAIL before their respective fixes are applied.
//
// Item 1 (Moderate): §7.3.6 — F_BAD_ATS_TREQ ATS-specific permission fields
//   EventEntry must carry ats_r/ats_w/ats_x/ats_p (bits [95:92]) populated from the
//   raw (pre-override) AccessType when event type == F_BAD_ATS_TREQ.  All other event
//   types must leave these fields at their default false/0 value (RES0).
//
// Item 2 (Low): §7.3.2 — F_UUT Reason field
//   EventEntry must carry a `reason` field (uint16_t).  The SW model always emits 0
//   (IMPLEMENTATION DEFINED per spec) but the field must exist and be zero-initialized.
//
// Item 3 (Low): rnw comment polarity
//   The comment for EventEntry::rnw must correctly state
//   "RnW (Read-not-Write) — true=read, false=write".
//   This item has no runtime test (comment change only); it is verified by code review.

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

// Enable SMMU with all queue-enable bits set.
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Configure and enable a stage-1-only stream that does NOT support ATS
// (eats=0 is the default). F_BAD_ATS_TREQ fires when an ATS TR hits such a stream.
static void configureStage1StreamNoATS(SMMU& s, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.eats               = 0;  // ATS not supported — triggers F_BAD_ATS_TREQ
    s.configureStream(sid, cfg);
    s.enableStream(sid);
    s.createStreamPASID(sid, 0);
}

} // namespace

// ============================================================================
// Item 1 — ATS R/W/X/P fields for F_BAD_ATS_TREQ
// ============================================================================

// Test 1: ATS TR with AccessType::Read
// Expect: ats_r=true, ats_w=false, ats_x=false, ats_p=false
TEST(AtsReasonFields, ATSRead) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 1;
    configureStage1StreamNoATS(smmu, sid);

    smmu.translate(sid, 0, 0x1000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty());
    const EventEntry& ev = events.front();
    EXPECT_EQ(ev.type, EventType::F_BAD_ATS_TREQ);
    EXPECT_TRUE(ev.ats_r)  << "ats_r must be true for Read access";
    EXPECT_FALSE(ev.ats_w) << "ats_w must be false for Read access";
    EXPECT_FALSE(ev.ats_x) << "ats_x must be false for Read access";
    EXPECT_FALSE(ev.ats_p) << "ats_p must be false for Read access";
}

// Test 2: ATS TR with AccessType::Write
// Expect: ats_r=false, ats_w=true, ats_x=false, ats_p=false
TEST(AtsReasonFields, ATSWrite) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 2;
    configureStage1StreamNoATS(smmu, sid);

    smmu.translate(sid, 0, 0x1000, AccessType::Write,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty());
    const EventEntry& ev = events.front();
    EXPECT_EQ(ev.type, EventType::F_BAD_ATS_TREQ);
    EXPECT_FALSE(ev.ats_r) << "ats_r must be false for Write access";
    EXPECT_TRUE(ev.ats_w)  << "ats_w must be true for Write access";
    EXPECT_FALSE(ev.ats_x) << "ats_x must be false for Write access";
    EXPECT_FALSE(ev.ats_p) << "ats_p must be false for Write access";
}

// Test 3: ATS TR with AccessType::ReadPrivileged
// Expect: ats_r=true, ats_w=false, ats_x=false, ats_p=true
TEST(AtsReasonFields, ATSReadPrivileged) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 3;
    configureStage1StreamNoATS(smmu, sid);

    smmu.translate(sid, 0, 0x1000, AccessType::ReadPrivileged,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty());
    const EventEntry& ev = events.front();
    EXPECT_EQ(ev.type, EventType::F_BAD_ATS_TREQ);
    EXPECT_TRUE(ev.ats_r)  << "ats_r must be true for ReadPrivileged";
    EXPECT_FALSE(ev.ats_w) << "ats_w must be false for ReadPrivileged";
    EXPECT_FALSE(ev.ats_x) << "ats_x must be false for ReadPrivileged";
    EXPECT_TRUE(ev.ats_p)  << "ats_p must be true for ReadPrivileged";
}

// Test 4: ATS TR with AccessType::Execute
// Expect: ats_r=true, ats_w=false, ats_x=true, ats_p=false
TEST(AtsReasonFields, ATSExecute) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 4;
    configureStage1StreamNoATS(smmu, sid);

    smmu.translate(sid, 0, 0x1000, AccessType::Execute,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty());
    const EventEntry& ev = events.front();
    EXPECT_EQ(ev.type, EventType::F_BAD_ATS_TREQ);
    EXPECT_TRUE(ev.ats_r)  << "ats_r must be true for Execute (instruction fetch has read)";
    EXPECT_FALSE(ev.ats_w) << "ats_w must be false for Execute";
    EXPECT_TRUE(ev.ats_x)  << "ats_x must be true for Execute";
    EXPECT_FALSE(ev.ats_p) << "ats_p must be false for unprivileged Execute";
}

// Test 5: Non-ATS event (F_TRANSLATION) — ATS fields must remain false (RES0)
TEST(AtsReasonFields, NonATSEventHasZeroATSFields) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 5;
    // Stage-1-only stream with a translation enabled but no page mapped → F_TRANSLATION
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);

    // No page mapped — ordinary translate will produce F_TRANSLATION
    smmu.translate(sid, 0, 0x5000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::Ordinary);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty());
    const EventEntry& ev = events.front();
    EXPECT_EQ(ev.type, EventType::F_TRANSLATION);
    // ATS-specific fields must be RES0 for all non-ATS events
    EXPECT_FALSE(ev.ats_r) << "ats_r must be false (RES0) for non-ATS event";
    EXPECT_FALSE(ev.ats_w) << "ats_w must be false (RES0) for non-ATS event";
    EXPECT_FALSE(ev.ats_x) << "ats_x must be false (RES0) for non-ATS event";
    EXPECT_FALSE(ev.ats_p) << "ats_p must be false (RES0) for non-ATS event";
}

// ============================================================================
// Item 2 — reason field exists and is zero
// ============================================================================

// Test 6: Any event must carry reason == 0 (field must exist; SW model always emits 0)
TEST(AtsReasonFields, ReasonFieldIsZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 6;
    configureStage1StreamNoATS(smmu, sid);

    smmu.translate(sid, 0, 0x1000, AccessType::Read,
                   SecurityState::NonSecure, TransactionType::AtsTranslationRequest);

    auto events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty());
    const EventEntry& ev = events.front();
    EXPECT_EQ(ev.type, EventType::F_BAD_ATS_TREQ);
    EXPECT_EQ(ev.reason, static_cast<uint16_t>(0))
        << "reason must be 0 — SW model emits IMPLEMENTATION DEFINED value 0 per §7.3.2";
}
