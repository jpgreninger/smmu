// TDD failing tests for Bug 2: SMMU::reset() does not clear priAutoFailures_
//
// ARM SMMU v3 spec §3.11 / §6.3.9 require that reset() returns the SMMU to a
// clean power-on state.  priAutoFailures_ (populated on PRIQ overflow per
// GAP-H / §3.13.6) must be cleared on reset, but reset() never calls
// clearPRIAutoFailures().  These tests capture that bug in the red state.
//
// Expected behaviour after the fix:
//   getPRIAutoFailures().empty() == true immediately after reset()
//
// Current (buggy) behaviour:
//   getPRIAutoFailures() still returns the stale records → tests FAIL.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

namespace {

// Minimum PRIQ capacity accepted by the implementation (16 entries).
static constexpr size_t PRI_CAPACITY = 16;
static constexpr StreamID TEST_SID   = 0x20;
static constexpr PASID    TEST_PID   = 0;

// Build an SMMU with a small PRIQ (16 entries) so overflow is easy to trigger.
std::unique_ptr<SMMU> makeSMMU() {
    SMMUConfiguration config = SMMUConfiguration::createDefault();
    QueueConfiguration qcfg(512, 256, static_cast<size_t>(PRI_CAPACITY));
    config.setQueueConfiguration(qcfg);
    auto smmu = std::make_unique<SMMU>(config);
    smmu->enable();
    return smmu;
}

// Fill the PRIQ to capacity and then submit one extra request, which must
// overflow and deposit one record in priAutoFailures_.
// Returns the prgIndex of the overflow entry.
uint16_t triggerOneOverflow(SMMU& smmu) {
    // Fill the queue to capacity.
    for (size_t i = 0; i < PRI_CAPACITY; ++i) {
        PRIEntry req;
        req.streamID         = TEST_SID;
        req.pasid            = TEST_PID;
        req.requestedAddress = static_cast<IOVA>(i << 12);
        req.accessType       = AccessType::Read;
        req.isLastRequest    = false;
        req.prgIndex         = static_cast<uint16_t>(i);
        smmu.submitPageRequest(req);
    }

    // Submit the overflow entry.
    static constexpr uint16_t OVERFLOW_PRG = 200;
    PRIEntry overflow;
    overflow.streamID         = TEST_SID;
    overflow.pasid            = TEST_PID;
    overflow.requestedAddress = UINT64_C(0xABC000);
    overflow.accessType       = AccessType::Read;
    overflow.isLastRequest    = true;
    overflow.prgIndex         = OVERFLOW_PRG;
    smmu.submitPageRequest(overflow);

    return OVERFLOW_PRG;
}

} // anonymous namespace

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: A single auto-failure deposited before reset() must be gone after it.
// ─────────────────────────────────────────────────────────────────────────────
TEST(Bug2ResetPriFailures, Reset_ClearsPRIAutoFailures) {
    auto smmu = makeSMMU();

    // Verify the PRIQ really fills and the overflow produces exactly one failure.
    triggerOneOverflow(*smmu);
    ASSERT_EQ(smmu->getPRIQueueSize(), PRI_CAPACITY)
        << "Setup: PRIQ must be at capacity before overflow";
    ASSERT_EQ(smmu->getPRIAutoFailures().size(), 1u)
        << "Setup: exactly one auto-failure must be present before reset()";

    // --- action under test ---
    smmu->reset();

    // ARM §3.11 / §6.3.9: reset must return SMMU to clean power-on state.
    // priAutoFailures_ must be empty after reset().
    //
    // BUG: reset() never clears priAutoFailures_, so this assertion FAILS
    // in the current (unfixed) code, giving us the required red state.
    EXPECT_TRUE(smmu->getPRIAutoFailures().empty())
        << "Bug 2: reset() must clear priAutoFailures_ but stale records remain";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: Multiple auto-failures must all be gone after reset().
// ─────────────────────────────────────────────────────────────────────────────
TEST(Bug2ResetPriFailures, Reset_ClearsPRIAutoFailures_MultipleFailures) {
    auto smmu = makeSMMU();

    // Trigger a first overflow to get the first failure record.
    triggerOneOverflow(*smmu);
    ASSERT_EQ(smmu->getPRIAutoFailures().size(), 1u)
        << "Setup: first overflow must produce one auto-failure";

    // Drain the PRIQ so we can overflow again, then add two more failures.
    smmu->clearPRIQueue();

    // Fill again and overflow twice — each overflow adds one failure entry.
    for (size_t i = 0; i < PRI_CAPACITY; ++i) {
        PRIEntry req;
        req.streamID         = TEST_SID;
        req.pasid            = TEST_PID;
        req.requestedAddress = static_cast<IOVA>((i + 100) << 12);
        req.accessType       = AccessType::Write;
        req.isLastRequest    = false;
        req.prgIndex         = static_cast<uint16_t>(i + 100);
        smmu->submitPageRequest(req);
    }

    PRIEntry ov2;
    ov2.streamID         = TEST_SID;
    ov2.pasid            = TEST_PID;
    ov2.requestedAddress = UINT64_C(0xDEF000);
    ov2.accessType       = AccessType::Read;
    ov2.isLastRequest    = true;
    ov2.prgIndex         = 201;
    smmu->submitPageRequest(ov2);

    PRIEntry ov3;
    ov3.streamID         = TEST_SID;
    ov3.pasid            = TEST_PID;
    ov3.requestedAddress = UINT64_C(0xFED000);
    ov3.accessType       = AccessType::Read;
    ov3.isLastRequest    = true;
    ov3.prgIndex         = 202;
    smmu->submitPageRequest(ov3);

    // Three overflow entries submitted against a full queue → 3 auto-failures total
    // (1 from the first fillPRIQ/overflow + 2 just submitted).
    auto failures = smmu->getPRIAutoFailures();
    ASSERT_GE(failures.size(), 2u)
        << "Setup: at least two auto-failures must be present before reset()";

    // --- action under test ---
    smmu->reset();

    // All failure records must be gone after reset.
    //
    // BUG: reset() never clears priAutoFailures_, so the vector returned here
    // is still non-empty — this assertion FAILS in the unfixed code.
    EXPECT_TRUE(smmu->getPRIAutoFailures().empty())
        << "Bug 2: reset() must clear all priAutoFailures_ records but "
        << smmu->getPRIAutoFailures().size() << " stale record(s) remain";
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: After reset() a fresh overflow on the re-enabled SMMU produces
//         exactly one new failure (not a stale + new mix).
// ─────────────────────────────────────────────────────────────────────────────
TEST(Bug2ResetPriFailures, Reset_AfterReset_FreshOverflowProducesExactlyOneFailure) {
    auto smmu = makeSMMU();

    // Generate a pre-reset failure.
    triggerOneOverflow(*smmu);
    ASSERT_EQ(smmu->getPRIAutoFailures().size(), 1u)
        << "Setup: one pre-reset failure must exist";

    // Reset and re-enable.
    smmu->reset();
    smmu->enable();

    // After reset the PRIQ is also cleared.  Fill it again and overflow once.
    triggerOneOverflow(*smmu);

    // If reset() had properly cleared priAutoFailures_ the count should be 1.
    // With the bug the stale record from before reset() is still there, making
    // the count 2 → the assertion below FAILS in the unfixed code.
    auto failures = smmu->getPRIAutoFailures();
    EXPECT_EQ(failures.size(), 1u)
        << "Bug 2: after reset() + re-enable, a single overflow must produce "
        "exactly 1 auto-failure, not " << failures.size()
        << " (stale pre-reset record not cleared)";
}
