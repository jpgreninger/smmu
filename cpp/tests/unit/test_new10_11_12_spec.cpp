// ARM SMMU v3 Conformance Gap Tests: NEW-10, NEW-11, NEW-12
// Copyright (c) 2024 John Greninger
//
// NEW-10 (§6.3.96): acknowledgeEventQueueOverflow() — OVACKFLG writable
// NEW-11 (§7.3.2):  reportUnsupportedTransaction() — F_UUT event generation
// NEW-12 (§3.9):    ATS Transaction Types (OT/TR/TT) enforcement

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID  = 0x20;
static constexpr PASID    PID  = 0;

// Enable SMMU + EVENTQEN
void enableSMMU(SMMU& smmu) {
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);
}

// Configure a minimal stage-1-only translation stream
void setupS1Stream(SMMU& smmu, StreamID sid = SID, PASID pasid = PID,
                   uint8_t eats = 0) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.eats               = eats;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

// Configure a bypass stream
void setupBypassStream(SMMU& smmu, StreamID sid = SID, PASID /*pasid*/ = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = true;
    cfg.eats               = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
}

// Map a page and return its IOVA
IOVA setupPage(SMMU& smmu, StreamID sid = SID, PASID pasid = PID,
               IOVA iova = 0x1000, PA pa = 0x2000) {
    PagePermissions perms;
    perms.read  = true;
    perms.write = true;
    perms.execute = false;
    EXPECT_TRUE(smmu.mapPage(sid, pasid, iova, pa, perms).isOk());
    return iova;
}

// Minimum queue size used by overflow tests so overflow is easily triggered.
static constexpr size_t SMALL_QUEUE = 16;

// Build an SMMU with a small event queue (makes overflow tests fast and controllable).
std::unique_ptr<SMMU> makeSmallQueueSMMU() {
    QueueConfiguration qcfg(SMALL_QUEUE, SMALL_QUEUE, SMALL_QUEUE);
    SMMUConfiguration cfg(qcfg, CacheConfiguration(), AddressConfiguration(), ResourceLimits());
    auto smmu = std::make_unique<SMMU>(cfg);
    smmu->enable();
    smmu->setCR0(smmu->getCR0() | SMMU::CR0_EVENTQEN);
    // Enable C_BAD_STREAMID recording so unknown-stream faults fill the queue.
    smmu->setCR2(SMMU::CR2_RECINVSID);
    return smmu;
}

// Fill the event queue to trigger overflow using a small-queue SMMU.
// The smmu must already have RECINVSID=1.
void fillEventQueue(SMMU& smmu, size_t maxSz) {
    // Fire (maxSz + 2) attempts to ensure overflow
    for (size_t i = 0; i <= maxSz + 1; ++i) {
        StreamID badSID = 0xDEAD0000u + static_cast<StreamID>(i);
        smmu.translate(badSID, 0, 0x1000, AccessType::Read);
    }
}

} // anonymous namespace

// =============================================================================
// NEW-10: acknowledgeEventQueueOverflow()
// =============================================================================

class New10EventQOverflow : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = makeSmallQueueSMMU();
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// Test 1: acknowledge does NOT clear queued events; OVFLG becomes == OVACKFLG
TEST_F(New10EventQOverflow, AcknowledgeDoesNotClearQueue) {
    fillEventQueue(*smmu_, SMALL_QUEUE);

    // Verify overflow is active: OVFLG bit31 of PROD != OVACKFLG bit31 of CONS
    uint32_t prodBefore = smmu_->getEventqProdIndex();
    uint32_t consBefore = smmu_->getEventqConsIndex();
    bool overflowActive = ((prodBefore >> 31) & 1u) != ((consBefore >> 31) & 1u);
    EXPECT_TRUE(overflowActive) << "Expected overflow to be active before acknowledge";

    // How many events are queued
    size_t eventsBefore = smmu_->getEventQueue().size();
    EXPECT_GT(eventsBefore, 0u) << "Expected events in queue before acknowledge";

    // Acknowledge the overflow
    smmu_->acknowledgeEventQueueOverflow();

    // Events must still be in the queue (not cleared)
    size_t eventsAfter = smmu_->getEventQueue().size();
    EXPECT_EQ(eventsAfter, eventsBefore) << "acknowledgeEventQueueOverflow() must not clear events";

    // Overflow inactive: OVFLG == OVACKFLG after acknowledge
    uint32_t prodAfter = smmu_->getEventqProdIndex();
    uint32_t consAfter = smmu_->getEventqConsIndex();
    bool overflowInactive = ((prodAfter >> 31) & 1u) == ((consAfter >> 31) & 1u);
    EXPECT_TRUE(overflowInactive) << "Overflow should be inactive after acknowledge";
}

// Test 2: no-op when there is no overflow
TEST_F(New10EventQOverflow, NoOpWhenNoOverflow) {
    // No overflow has occurred yet
    uint32_t consBefore = smmu_->getEventqConsIndex();
    uint32_t ackFlagBefore = (consBefore >> 31) & 1u;

    smmu_->acknowledgeEventQueueOverflow();

    uint32_t consAfter = smmu_->getEventqConsIndex();
    uint32_t ackFlagAfter = (consAfter >> 31) & 1u;

    // OVACKFLG must be unchanged when no overflow was active
    EXPECT_EQ(ackFlagBefore, ackFlagAfter)
        << "acknowledgeEventQueueOverflow() must be no-op when no overflow";
}

// Test 3: acknowledge after draining events
TEST_F(New10EventQOverflow, AcknowledgeAfterDrain) {
    fillEventQueue(*smmu_, SMALL_QUEUE);

    // Overflow is active; drain all events
    smmu_->clearEventQueue();

    // After clear, prod/cons both reset to 0, so overflow flag is gone.
    // Calling acknowledge on a no-overflow state must still be a no-op.
    uint32_t consBefore = smmu_->getEventqConsIndex();
    uint32_t ackFlagBefore = (consBefore >> 31) & 1u;

    smmu_->acknowledgeEventQueueOverflow();

    uint32_t consAfter = smmu_->getEventqConsIndex();
    uint32_t ackFlagAfter = (consAfter >> 31) & 1u;

    // OVACKFLG unchanged (already matched OVFLG after reset)
    EXPECT_EQ(ackFlagBefore, ackFlagAfter);
    // Queue remains empty
    EXPECT_EQ(smmu_->getEventQueue().size(), 0u);
}

// =============================================================================
// NEW-11: reportUnsupportedTransaction() — F_UUT event
// =============================================================================

class New11FUut : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        enableSMMU(*smmu_);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// Test 4: F_UUT event emitted when EVENTQEN=1
TEST_F(New11FUut, EmitsEventWhenEventqenEnabled) {
    smmu_->clearEventQueue();

    smmu_->reportUnsupportedTransaction(SID, PID, 0x5000,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    auto events = smmu_->getEventQueue();
    ASSERT_EQ(events.size(), 1u) << "Expected exactly one F_UUT event";
    EXPECT_EQ(events[0].type, EventType::F_UUT);
    EXPECT_EQ(events[0].streamID, SID);
    EXPECT_EQ(events[0].pasid, PID);
    EXPECT_EQ(events[0].address, static_cast<IOVA>(0x5000));
}

// Test 5: F_UUT silently dropped when EVENTQEN=0
TEST_F(New11FUut, SilentWhenEventqenDisabled) {
    // Disable EVENTQEN (keep SMMUEN=1)
    uint32_t cr0 = smmu_->getCR0();
    cr0 &= ~SMMU::CR0_EVENTQEN;
    smmu_->setCR0(cr0);

    smmu_->reportUnsupportedTransaction(SID, PID, 0x5000,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(smmu_->getEventQueue().size(), 0u)
        << "F_UUT must be silently dropped when EVENTQEN=0";
}

// Test 6: normal translation still works after reportUnsupportedTransaction()
TEST_F(New11FUut, TranslationStillWorksAfterFUut) {
    setupS1Stream(*smmu_);
    setupPage(*smmu_);

    // Emit an F_UUT for a different stream/address
    smmu_->reportUnsupportedTransaction(0x99, 0, 0xABCD,
                                        AccessType::Write,
                                        SecurityState::NonSecure);

    // Regular translation must succeed
    TranslationResult result = smmu_->translate(SID, PID, 0x1000, AccessType::Read);
    EXPECT_TRUE(result.isOk()) << "translate() must succeed after reportUnsupportedTransaction()";
}

// =============================================================================
// NEW-12: ATS Transaction Types (OT / TR / TT)
// =============================================================================

class New12Ats : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        enableSMMU(*smmu_);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// Test 7: TR type with eats=0 → F_BAD_ATS_TREQ (§7.3.6)
TEST_F(New12Ats, TrType_EatsZero_BadAtsTreq) {
    setupS1Stream(*smmu_, SID, PID, /*eats=*/0);
    setupPage(*smmu_);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk()) << "TR on eats=0 stream must fail";

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_BAD_ATS_TREQ event";
    EXPECT_EQ(events[0].type, EventType::F_BAD_ATS_TREQ);
}

// Test 8: TR type on bypass stream → F_BAD_ATS_TREQ (§7.3.6)
TEST_F(New12Ats, TrType_BypassStream_BadAtsTreq) {
    setupBypassStream(*smmu_);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_FALSE(result.isOk()) << "TR on bypass stream must fail";

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_BAD_ATS_TREQ event";
    EXPECT_EQ(events[0].type, EventType::F_BAD_ATS_TREQ);
}

// Test 9: TR type with eats != 0 and translation-enabled stream → success
TEST_F(New12Ats, TrType_EatsNonZero_Succeeds) {
    setupS1Stream(*smmu_, SID, PID, /*eats=*/2);
    setupPage(*smmu_);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslationRequest);

    EXPECT_TRUE(result.isOk()) << "TR with eats=2 on translation-enabled stream must succeed";
    EXPECT_TRUE(smmu_->getEventQueue().empty()) << "No error event expected on success";
}

// Test 10: TT type with ATSCHK=0 → no re-check, succeeds
TEST_F(New12Ats, TtType_AtschkOff_NoRecheck) {
    setupS1Stream(*smmu_);
    setupPage(*smmu_);
    // Ensure ATSCHK=0
    smmu_->setCR0(smmu_->getCR0() & ~SMMU::CR0_ATSCHK);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated);

    EXPECT_TRUE(result.isOk()) << "TT with ATSCHK=0 must succeed without re-check";
    EXPECT_TRUE(smmu_->getEventQueue().empty()) << "No event expected with ATSCHK=0";
}

// Test 11: TT type with ATSCHK=1 and mapped page → success
TEST_F(New12Ats, TtType_AtschkOn_MappedPage_Succeeds) {
    setupS1Stream(*smmu_);
    setupPage(*smmu_);
    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_ATSCHK);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated);

    EXPECT_TRUE(result.isOk()) << "TT with ATSCHK=1 on mapped page must succeed";
    EXPECT_TRUE(smmu_->getEventQueue().empty()) << "No error event expected on success";
}

// Test 12: TT type with ATSCHK=1 and unmapped address → F_TRANSL_FORBIDDEN (§7.3.8)
TEST_F(New12Ats, TtType_AtschkOn_UnmappedPage_TranslForbidden) {
    setupS1Stream(*smmu_);
    // Do NOT map any page at 0x9000
    smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_ATSCHK);
    smmu_->clearEventQueue();

    TranslationResult result = smmu_->translate(
        SID, PID, 0x9000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::AtsTranslated);

    EXPECT_FALSE(result.isOk()) << "TT with ATSCHK=1 on unmapped page must fail";

    auto events = smmu_->getEventQueue();
    ASSERT_FALSE(events.empty()) << "Expected F_TRANSL_FORBIDDEN event";
    EXPECT_EQ(events[0].type, EventType::F_TRANSL_FORBIDDEN);
}

// Test 13: Ordinary type (default) — behavior unchanged
TEST_F(New12Ats, OrdinaryType_BehaviorPreserved) {
    setupS1Stream(*smmu_);
    setupPage(*smmu_);
    smmu_->clearEventQueue();

    // With explicit Ordinary
    TranslationResult r1 = smmu_->translate(
        SID, PID, 0x1000, AccessType::Read,
        SecurityState::NonSecure,
        TransactionType::Ordinary);
    EXPECT_TRUE(r1.isOk()) << "Ordinary type on mapped page must succeed";

    // With default (no transactionType argument) — same address, same PASID
    // Need to re-map because the translate() call may exhaust nothing here;
    // just verify the default overload still compiles and works.
    TranslationResult r2 = smmu_->translate(SID, PID, 0x1000, AccessType::Read);
    EXPECT_TRUE(r2.isOk()) << "Default (Ordinary) type must also succeed";

    EXPECT_TRUE(smmu_->getEventQueue().empty()) << "No error events for ordinary type";
}
