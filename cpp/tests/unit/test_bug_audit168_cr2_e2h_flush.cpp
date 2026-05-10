// TDD tests for BUG-AUDIT-168-CPP
//
// ARM §3.17.5: "A change to CR2.E2H requires invalidation of all EL2 and
// EL2-E2H entries (use ALLE2 or equivalent before toggling E2H)."
//
// This is a software responsibility (OS must issue ALLE2 before toggling E2H),
// but the simulation model should defensively invalidate EL2/EL2_E2H TLB
// entries when the E2H bit transitions, to correctly model required behavior.
//
// BUG: setCR2() stores the new value without checking whether the E2H bit
// changed. When E2H transitions (0→1 or 1→0), stale EL2/EL2_E2H TLB entries
// from the previous E2H state survive and can produce incorrect translations.
//
// FIX: After storing the new CR2 value, compare old vs. new E2H bits. On any
// transition, call tlbCache->invalidateEL2Entries() to evict stale entries.
//
// Test method (mirrors BUG-AUDIT-152/166 pattern):
//   1. With SMMUEN=0, set CR2.E2H=0.
//   2. Enable SMMU (setCR0 with SMMUEN=1).
//   3. Configure an EL2_E2H stream (stage2Enabled=true to bypass §5.2 STRW check).
//   4. Map a page and translate → warm TLB with EL2_E2H entry.
//   5. Unmap the page → subsequent walk will fault on TLB miss.
//   6. Disable SMMU (setCR0 without SMMUEN) — TLB entries are NOT flushed.
//   7. Toggle CR2.E2H (0→1) via setCR2().
//   8. Re-enable SMMU.
//   9. Re-translate the EL2_E2H stream without any explicit TLBI:
//      BEFORE FIX: stale TLB entry survives E2H toggle → re-translate succeeds (WRONG).
//      AFTER FIX:  E2H toggle flushes EL2/EL2_E2H entries → re-translate faults (CORRECT).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

using namespace smmu;

namespace {

static constexpr PASID    PASID_ZERO = 0;
static constexpr IOVA     TEST_IOVA  = 0x1000;
static constexpr PA       TEST_PA    = 0xD000;
static constexpr StreamID SID_E2H    = 0x10;
static constexpr StreamID SID_EL2    = 0x20;  // control: pure EL2 (not E2H)

// Configure a two-stage EL2_E2H (or EL2) stream.
// stage2Enabled=true bypasses §5.2 IgnoreSTESTRW STRW validation so that
// non-EL1_EL0 StreamWorlds are accepted by configureStream().
static void setupEL2E2HStream(SMMU& smmu, StreamID sid, StreamWorld strw) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = strw;
    cfg.asid               = 0x20;
    cfg.vmid               = 0x10;
    cfg.securityState      = SecurityState::NonSecure;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, PASID_ZERO);
}

class BugAudit168CR2E2HFlushTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Start with SMMU disabled so setCR2() takes effect.
        // (Do NOT call setCR0(SMMUEN) here — that would make setCR2() a no-op.)
    }

    SMMU smmu_;
};

} // anonymous namespace

// ============================================================================
// Test 1: CR2.E2H toggle (0→1) must flush EL2_E2H TLB entries
// ============================================================================
//
// §3.17.5: toggling E2H requires ALLE2-equivalent invalidation.
// After the toggle, stale EL2_E2H entries must not be served.
//
// BEFORE FIX: E2H toggle does NOT flush TLB → stale entry survives → re-translate
//             succeeds → isOk() == true → FAIL (entry should have been evicted).
// AFTER FIX:  E2H toggle flushes EL2/EL2_E2H entries → re-translate faults →
//             isOk() == false → PASS.
TEST_F(BugAudit168CR2E2HFlushTest, E2HToggleFlushesEL2E2HEntries) {
    // Step 1: Set CR2.E2H=0 (SMMU is disabled, so this write is accepted).
    smmu_.setCR2(0u);

    // Step 2: Enable SMMU.
    smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Step 3: Configure EL2_E2H stream.
    setupEL2E2HStream(smmu_, SID_E2H, StreamWorld::EL2_E2H);

    // Step 4: Map a page and translate → warm TLB.
    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_E2H, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_E2H, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    // Step 5: Unmap the page — subsequent page-table walk will fault on miss.
    smmu_.unmapPage(SID_E2H, PASID_ZERO, TEST_IOVA);

    // Step 6: Disable SMMU — TLB entries are preserved across disable.
    smmu_.setCR0(0u);

    // Confirm that the SMMU accepted the CR2 write (SMMUEN=0 now).
    // Step 7: Toggle CR2.E2H 0→1.  Without the fix, the TLB is NOT flushed.
    smmu_.setCR2(SMMU::CR2_E2H);

    // Step 8: Re-enable SMMU.
    smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Step 9: Re-translate WITHOUT any explicit TLBI.
    // BEFORE FIX: stale EL2_E2H entry hits → result.isOk() == true → WRONG.
    // AFTER FIX:  entry was evicted by setCR2() → walk faults → result.isOk() == false.
    auto result = smmu_.translate(SID_E2H, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-168-CPP: CR2.E2H toggle (0→1) must invalidate EL2/EL2_E2H "
           "TLB entries (ARM §3.17.5). Re-translate after toggle without explicit TLBI "
           "must fault (stale entry should have been evicted by setCR2()).";
}

// ============================================================================
// Test 2: CR2.E2H toggle (1→0) must flush EL2_E2H TLB entries
// ============================================================================
//
// The flush applies in both directions of the E2H bit transition.
//
// BEFORE FIX: entry survives the 1→0 toggle → re-translate succeeds → FAIL.
// AFTER FIX:  entry evicted → re-translate faults → PASS.
TEST_F(BugAudit168CR2E2HFlushTest, E2HToggleReverseFlushesEL2E2HEntries) {
    // Step 1: Set CR2.E2H=1 while SMMU is disabled.
    smmu_.setCR2(SMMU::CR2_E2H);

    // Step 2: Enable SMMU.
    smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Step 3: Configure EL2_E2H stream.
    setupEL2E2HStream(smmu_, SID_E2H, StreamWorld::EL2_E2H);

    // Step 4: Map and translate → warm TLB.
    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_E2H, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_E2H, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    // Step 5: Unmap.
    smmu_.unmapPage(SID_E2H, PASID_ZERO, TEST_IOVA);

    // Step 6: Disable SMMU.
    smmu_.setCR0(0u);

    // Step 7: Toggle CR2.E2H 1→0.
    smmu_.setCR2(0u);

    // Step 8: Re-enable SMMU.
    smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Step 9: Re-translate — must fault.
    auto result = smmu_.translate(SID_E2H, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-168-CPP: CR2.E2H toggle (1→0) must also invalidate EL2/EL2_E2H "
           "TLB entries (ARM §3.17.5). Re-translate after toggle must fault.";
}

// ============================================================================
// Test 3: No E2H change → TLB entries must NOT be flushed
// ============================================================================
//
// When CR2 is written but E2H bit does NOT change, EL2/EL2_E2H entries must
// survive (no spurious flush). This confirms the fix is precise.
TEST_F(BugAudit168CR2E2HFlushTest, NoCR2E2HChangePreservesEntries) {
    // Step 1: Set CR2 with E2H=0 and some other bits.
    smmu_.setCR2(0u);

    // Step 2: Enable SMMU.
    smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Step 3: Configure EL2_E2H stream and warm TLB.
    setupEL2E2HStream(smmu_, SID_E2H, StreamWorld::EL2_E2H);

    PagePermissions perms(true, true, false);
    smmu_.mapPage(SID_E2H, PASID_ZERO, TEST_IOVA, TEST_PA, perms);

    auto warm = smmu_.translate(SID_E2H, PASID_ZERO, TEST_IOVA,
                                AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(warm.isOk()) << "Initial translate must succeed to warm TLB.";

    // Step 4: Unmap.
    smmu_.unmapPage(SID_E2H, PASID_ZERO, TEST_IOVA);

    // Step 5: Disable SMMU.
    smmu_.setCR0(0u);

    // Step 6: Write CR2 with same E2H=0 (no transition) — must NOT flush.
    smmu_.setCR2(0u);  // E2H stays 0

    // Step 7: Re-enable SMMU.
    smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);

    // Step 8: Re-translate — must succeed (TLB entry survived, no flush occurred).
    auto result = smmu_.translate(SID_E2H, PASID_ZERO, TEST_IOVA,
                                  AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-168-CPP: setCR2() with no E2H change must NOT flush EL2/EL2_E2H "
           "TLB entries. Re-translate should hit the cached entry (no spurious flush).";
}
