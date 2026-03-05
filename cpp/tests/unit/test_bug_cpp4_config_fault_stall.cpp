// BUG-CPP-4: Spec violation — PASIDNotFound and AddressSpaceExhausted can
//            incorrectly enter stall mode
//
// ARM IHI0070G.b §3.12.2 states:
//   "A configuration error always terminates the instigating transaction with
//    an abort and must never cause the transaction to be stalled."
//
// §7.3 intro similarly:
//   "Configuration faults always result in an abort."
//
// PASIDNotFound maps to C_BAD_SUBSTREAMID (configuration fault class).
// AddressSpaceExhausted maps to C_BAD_STE (configuration fault class).
//
// Neither error is eligible for stall mode.  Only F_TRANSLATION, F_ADDR_SIZE,
// F_ACCESS, and F_PERMISSION (data faults) may stall.
//
// Root cause: the isConfigFault boolean guard in translate() (smmu.cpp ~lines
// 317-321) lists five SMMUError values but omits PASIDNotFound and
// AddressSpaceExhausted.  Therefore both of those config faults slip through
// the guard and are allowed to stall.
//
// Fix: add SMMUError::PASIDNotFound and SMMUError::AddressSpaceExhausted to
// the isConfigFault expression.
//
// TDD: These tests MUST FAIL (observe Stalled) before the fix and PASS (observe
// abort/error, not Stalled) after.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Test constants ────────────────────────────────────────────────────────

static constexpr StreamID CPP4_STREAM_BASE = 0xD0;
static constexpr PASID    CPP4_PASID_0     = 0;
static constexpr PASID    CPP4_PASID_UNKNOWN = 42;  // Never configured
static constexpr IOVA     CPP4_IOVA        = 0x6000'0000ULL;

// ── Helper ────────────────────────────────────────────────────────────────

static std::unique_ptr<SMMU> makeEnabled() {
    auto s = std::unique_ptr<SMMU>(new SMMU());
    s->enable();
    return s;
}

// ── BUG-CPP-4a: PASIDNotFound must NOT stall ──────────────────────────────
//
// Scenario: two-stage stream configured with FaultMode::Stall, but the PASID
// used for the translate() call has never been created (no address space).
// The two-stage path returns SMMUError::PASIDNotFound for the missing stage-1 AS.
// This is a C_BAD_SUBSTREAMID (configuration fault) and must never stall.
//
// FAILS before fix: result is SMMUError::Stalled (config fault allowed to stall).
// PASSES after fix: result is an error other than Stalled.

TEST(ConfigFaultStall, PASIDNotFound_MustNotStall) {
    auto smmu = makeEnabled();
    const StreamID sid = CPP4_STREAM_BASE;

    // Two-stage stall-mode stream.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Stall;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());

    // Only PASID_0 is created (stage-2 AS anchor).
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP4_PASID_0).isOk());

    // Translate with CPP4_PASID_UNKNOWN — no stage-1 AS exists → PASIDNotFound.
    TranslationResult result = smmu->translate(sid, CPP4_PASID_UNKNOWN, CPP4_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isError())
        << "BUG-CPP-4: PASIDNotFound should return an error, not success";

    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "BUG-CPP-4 §3.12.2: PASIDNotFound is a configuration fault "
           "(C_BAD_SUBSTREAMID) and must never stall; "
           "fix: add SMMUError::PASIDNotFound to the isConfigFault guard";
}

// ── BUG-CPP-4b: AddressSpaceExhausted must NOT stall ─────────────────────
//
// Scenario: two-stage stream in FaultMode::Stall, stage-2 address space is
// absent (getStage2AddressSpace() returns null).  The two-stage path returns
// SMMUError::AddressSpaceExhausted for the missing stage-2 AS.
// This is a C_BAD_STE (configuration fault) and must never stall.
//
// To ensure getStage2AddressSpace() returns null: only create PASID_1 (not
// PASID_0) so that the stage-2 auto-link in createPASID() is never triggered.
// PASID_1's AS is used for stage-1 (with a mapped page so stage-1 succeeds);
// getStage2AddressSpace() returns null → AddressSpaceExhausted.
//
// FAILS before fix: result is SMMUError::Stalled.
// PASSES after fix: result is an error other than Stalled.

TEST(ConfigFaultStall, AddressSpaceExhausted_MustNotStall) {
    auto smmu = makeEnabled();
    const StreamID sid = CPP4_STREAM_BASE + 1;
    static constexpr PASID PASID_1 = 1;

    // Two-stage stall-mode stream.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Stall;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());

    // Only create PASID_1 (NOT PASID_0) so stage-2 AS auto-link is never
    // triggered.  PASID_1's AS becomes stage-1; getStage2AddressSpace() returns null.
    ASSERT_TRUE(smmu->createStreamPASID(sid, PASID_1).isOk());
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(smmu->mapPage(sid, PASID_1, CPP4_IOVA, 0x1000'0000ULL, rw).isOk());
    // Stage-1 will succeed (PASID_1 AS has the mapping).
    // Stage-2 AS is null → AddressSpaceExhausted.

    TranslationResult result = smmu->translate(sid, PASID_1, CPP4_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isError())
        << "BUG-CPP-4: Missing stage-2 AS should return an error, not success";

    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "BUG-CPP-4 §3.12.2: AddressSpaceExhausted is a configuration fault "
           "(C_BAD_STE) and must never stall; "
           "fix: add SMMUError::AddressSpaceExhausted to the isConfigFault guard";
}

// ── BUG-CPP-4c: Genuine translation fault CAN still stall (regression) ───
//
// A stall-mode stream with a mapped stage-1 AS but no mapping for a given
// IOVA should still return Stalled (not abort).  Verifies the fix does not
// suppress stalling for legitimate data faults.

TEST(ConfigFaultStall, TranslationFault_CanStill_Stall) {
    auto smmu = makeEnabled();
    const StreamID sid = CPP4_STREAM_BASE + 2;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP4_PASID_0).isOk());
    // No page mapped → translation miss.

    TranslationResult result = smmu->translate(sid, CPP4_PASID_0, CPP4_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "BUG-CPP-4 regression: Translation fault (PageNotMapped) in stall "
           "mode must still return Stalled — fix must not suppress legitimate stalls";
}

// ── BUG-CPP-4d: Permission fault CAN still stall (regression) ────────────
//
// Regression guard: permission fault is a data fault and is stall-eligible.

TEST(ConfigFaultStall, PermissionFault_CanStill_Stall) {
    auto smmu = makeEnabled();
    const StreamID sid = CPP4_STREAM_BASE + 3;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP4_PASID_0).isOk());

    // Map page as read-only; a write access produces a permission fault.
    PagePermissions readOnly(true, false, false);
    ASSERT_TRUE(smmu->mapPage(sid, CPP4_PASID_0, CPP4_IOVA, 0x2000'0000ULL, readOnly).isOk());

    TranslationResult result = smmu->translate(sid, CPP4_PASID_0, CPP4_IOVA,
                                               AccessType::Write, SecurityState::NonSecure);

    ASSERT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::Stalled)
        << "BUG-CPP-4 regression: Permission fault in stall mode must still "
           "return Stalled — fix must not suppress permission-fault stalls";
}

// ── BUG-CPP-4e: StreamNotConfigured must NOT stall (existing guard, regression)

TEST(ConfigFaultStall, StreamNotConfigured_MustNotStall) {
    auto smmu = makeEnabled();
    // Translate on a stream that was never configured.
    const StreamID unconfiguredSID = CPP4_STREAM_BASE + 0x40;

    TranslationResult result = smmu->translate(unconfiguredSID, CPP4_PASID_0, CPP4_IOVA,
                                               AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isError());
    EXPECT_NE(result.getError(), SMMUError::Stalled)
        << "BUG-CPP-4 regression: StreamNotConfigured must never stall "
           "(already in the isConfigFault guard; regression guard)";
}

}  // namespace test
}  // namespace smmu
