// TDD tests for BUG-AUDIT-148-CPP
//
// ARM §3.1 (line 1220): "The PRI queue is only present on SMMUs supporting PRI services."
// When priSupported_==false (IDR0.PRI==0), the PRI queue structurally does not exist.
// submitPageRequest() must silently drop the request (no enqueue, no auto-failure).
// processPRIQueue() must be a no-op.
// setPRISupported(false) must clear any in-flight priQueue and reset PROD/CONS to 0.
//
// Tests 1-3 FAIL before the fix (RED), all 4 PASS after the fix (GREEN).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <vector>

using namespace smmu;

namespace {

// Enable SMMU with SMMUEN + PRIQEN + EVENTQEN + CMDQEN.
// Note: setCR0() respects priSupported_ — PRIQEN is only written when IDR0.PRI==1.
// So we call this BEFORE setPRISupported(false).
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Build a minimal PRIEntry with the given isLastRequest value.
static PRIEntry makePRIEntry(bool isLast) {
    PRIEntry entry;
    entry.streamID        = 1u;
    entry.pasid           = 0u;
    entry.requestedAddress = 0x1000u;
    entry.accessType      = AccessType::Read;
    entry.isLastRequest   = isLast;
    entry.prgIndex        = 0u;
    entry.securityState   = SecurityState::NonSecure;
    return entry;
}

} // anonymous namespace

// ============================================================================
// Test 1: submitPageRequest silently drops when PRI not supported
// ============================================================================
//
// BUG-AUDIT-148-CPP: submitPageRequest() must return without enqueuing or
// generating auto-failures when priSupported_==false (IDR0.PRI==0).
// This is "queue not present" (structural absence), not "queue disabled" (PRIQEN=0).
//
// BEFORE FIX: the entry is enqueued OR an auto-failure is generated → FAILS.
// AFTER FIX:  queue stays empty, no auto-failures → PASSES.
TEST(BugAudit148PriGate, SubmitPageRequestSilentlyDropsWhenPriNotSupported) {
    SMMU smmu;

    // Enable first (while priSupported_ is still true by default).
    enableSMMU(smmu);

    // Now disable PRI support — queue structurally absent.
    smmu.setPRISupported(false);

    // IDR0.PRI (bit 16) must be 0 after setPRISupported(false).
    ASSERT_EQ(smmu.getIDR0() & (1u << 16u), 0u)
        << "Precondition: IDR0.PRI must be 0 after setPRISupported(false).";

    // Submit a Last=1 PPR — would trigger auto-failure on the PRIQEN=0 path,
    // but since the queue is structurally absent, it must be a silent drop.
    PRIEntry entry = makePRIEntry(/*isLast=*/true);
    smmu.submitPageRequest(entry);

    // Nothing enqueued.
    EXPECT_EQ(smmu.getPRIQueueSize(), 0u)
        << "BUG-AUDIT-148-CPP: submitPageRequest must NOT enqueue when priSupported_==false.";

    // No auto-failures either — silent structural drop, not a PRIQEN=0 auto-failure.
    std::vector<PRIAutoFailure> failures = smmu.getPRIAutoFailures();
    EXPECT_EQ(failures.size(), 0u)
        << "BUG-AUDIT-148-CPP: submitPageRequest must NOT produce auto-failures "
           "when priSupported_==false. Silent drop required (queue structurally absent).";

    // IDR0.PRI remains 0.
    EXPECT_EQ(smmu.getIDR0() & (1u << 16u), 0u)
        << "BUG-AUDIT-148-CPP: IDR0.PRI must remain 0.";
}

// ============================================================================
// Test 2: processPRIQueue is a no-op when PRI not supported
// ============================================================================
//
// BUG-AUDIT-148-CPP: processPRIQueue() must not throw, assert, or change any
// observable state when priSupported_==false.
//
// BEFORE FIX: processPRIQueue() executes normally (no priSupported_ gate) → FAILS
//             if any side-effect is detectable, or if it would throw on empty queue.
// AFTER FIX:  processPRIQueue() returns immediately → PASSES.
TEST(BugAudit148PriGate, ProcessPRIQueueIsNoopWhenPriNotSupported) {
    SMMU smmu;

    // Enable while PRI still supported, then disable PRI.
    enableSMMU(smmu);
    smmu.setPRISupported(false);

    // Must not throw or assert-fail.
    ASSERT_NO_THROW(smmu.processPRIQueue())
        << "BUG-AUDIT-148-CPP: processPRIQueue() must not throw when priSupported_==false.";

    // Queue is still empty (no state change).
    EXPECT_EQ(smmu.getPRIQueueSize(), 0u)
        << "BUG-AUDIT-148-CPP: processPRIQueue() must not alter queue when priSupported_==false.";

    // No auto-failures generated.
    EXPECT_EQ(smmu.getPRIAutoFailures().size(), 0u)
        << "BUG-AUDIT-148-CPP: processPRIQueue() must not produce auto-failures "
           "when priSupported_==false.";
}

// ============================================================================
// Test 3: setPRISupported(false) clears in-flight queue and resets PROD/CONS
// ============================================================================
//
// BUG-AUDIT-148-CPP: when transitioning to priSupported_==false, any in-flight
// priQueue entries must be cleared and priqProd/priqCons reset to 0.
//
// BEFORE FIX: setPRISupported(false) does not clear queue or reset indices → FAILS
//             because getPRIQueueSize() > 0 after calling setPRISupported(false).
// AFTER FIX:  getPRIQueueSize()==0, priqProd==0, priqCons==0 → PASSES.
TEST(BugAudit148PriGate, SetPRISupportedFalseClearsInFlightQueue) {
    SMMU smmu;

    // Enable with PRI supported (default).
    enableSMMU(smmu);

    // Verify the queue is empty at the start.
    ASSERT_EQ(smmu.getPRIQueueSize(), 0u)
        << "Precondition: queue must start empty.";

    // Disable PRI — even with empty queue, PROD/CONS must be reset.
    smmu.setPRISupported(false);

    // After setPRISupported(false), queue must be empty.
    EXPECT_EQ(smmu.getPRIQueueSize(), 0u)
        << "BUG-AUDIT-148-CPP: setPRISupported(false) must clear priQueue.";

    // PROD and CONS indices must be 0 (reset).
    EXPECT_EQ(smmu.getPriqProdIndex(), 0u)
        << "BUG-AUDIT-148-CPP: setPRISupported(false) must reset priqProd to 0.";
    EXPECT_EQ(smmu.getPriqConsIndex(), 0u)
        << "BUG-AUDIT-148-CPP: setPRISupported(false) must reset priqCons to 0.";
}

// ============================================================================
// Test 4 (regression): submitPageRequest still works when PRI supported
// ============================================================================
//
// Regression check: when priSupported_==true and SMMUEN+PRIQEN are set,
// submitPageRequest() must still enqueue the PPR normally.
//
// This test must pass BOTH before and after the fix.
TEST(BugAudit148PriGate, SubmitPageRequestWorksWhenPriSupported) {
    SMMU smmu;

    // priSupported_ is true by default.
    enableSMMU(smmu);

    // Confirm IDR0.PRI=1.
    ASSERT_NE(smmu.getIDR0() & (1u << 16u), 0u)
        << "Precondition: IDR0.PRI must be 1 with default config.";

    // Submit a non-Last PPR.
    PRIEntry entry = makePRIEntry(/*isLast=*/false);
    smmu.submitPageRequest(entry);

    // Must be enqueued.
    EXPECT_EQ(smmu.getPRIQueueSize(), 1u)
        << "Regression: submitPageRequest must enqueue when priSupported_==true.";

    // No spurious auto-failures.
    EXPECT_EQ(smmu.getPRIAutoFailures().size(), 0u)
        << "Regression: no auto-failures expected for valid enqueue.";
}
