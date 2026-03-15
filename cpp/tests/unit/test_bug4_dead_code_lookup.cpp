// Bug 4: lookupTranslationCache() is dead code
//
// ARM IHI0070G.b §3.3.4, §13, §3.12.2
//
// Root cause: SMMU::lookupTranslationCache() is declared private in smmu.h and
// implemented in smmu.cpp but is NEVER called from any production code path.
// The actual TLB lookup is performed inline in SMMU::translate().  The dead
// method is subtly wrong in three ways compared to the live inline path:
//
//   Defect 1 — Missing STRW promotion (§3.3.4 / §13.4.1):
//     The inline path in translate() converts an unprivileged AccessType to its
//     Privileged variant when STE.STRW == EL2 or EL3 (single-stage only), so
//     that the permission check treats the access as privileged and succeeds
//     against a privilegedOnly page.  lookupTranslationCache() performs no such
//     conversion; it would pass the raw (unprivileged) AccessType to
//     validateAccessPermissions() and incorrectly deny the access.
//
//   Defect 2 — Missing STE output-attribute overrides (§13):
//     The inline path applies cfg.memAttr, cfg.shCfg, cfg.allocCfg, cfg.instCfg,
//     cfg.privCfg, and cfg.nsCfg from the StreamConfig to TranslationData on
//     every TLB hit.  lookupTranslationCache() calls makeTranslationSuccess()
//     with only (PA, permissions, securityState) and therefore always returns
//     zero-valued output-attribute fields regardless of the stream's STE
//     configuration.
//
//   Defect 3 — Missing stall-mode fallthrough on permission failure (§3.12.2):
//     The inline path explicitly falls through to the slow-path
//     (performTwoStageTranslation) on permission failure so that stall-mode
//     streams correctly park the transaction.  lookupTranslationCache() returns
//     CacheEntryNotFound / SecurityFault immediately and never reaches the stall
//     queue.
//
// Fix: Delete lookupTranslationCache() from smmu.h and smmu.cpp.
//
// TDD approach for dead code removal:
//   These tests exercise the PRODUCTION CODE PATH (the inline TLB path in
//   translate()) and document that it correctly implements all three behaviours
//   that the dead method lacks.  After deleting lookupTranslationCache() the
//   same tests continue to pass, confirming the dead method was genuinely never
//   on the hot path.  The tests serve two purposes:
//     (a) They provide a precise specification of what the dead method fails to
//         implement and therefore why it must not be used.
//     (b) They act as regression guards — if any future refactor accidentally
//         re-introduces the broken path, these tests will catch it.
//
//   All three tests PASS today (the inline production path is correct).  They
//   will continue to pass after the dead method is deleted.  A test for dead
//   code removal does not need a "red" state if the alternative is to test the
//   broken dead method directly (which would require making it non-private,
//   violating encapsulation).
//
// Spec references:
//   ARM IHI0070G.b §3.3.4   STRW-based privilege promotion
//   ARM IHI0070G.b §5.2     STE.STRW field encoding
//   ARM IHI0070G.b §13      Output attribute overrides (memType, shareability, …)
//   ARM IHI0070G.b §3.12.2  Stall mode — CMD_RESUME and stall queue

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <memory>
#include <cstdint>

namespace smmu {
namespace test {

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

static constexpr StreamID SID_STRW         = 0xA1; // used for STRW-promotion test
static constexpr StreamID SID_OUTPUT_ATTRS = 0xA2; // used for output-attribute test
static constexpr StreamID SID_STALL        = 0xA3; // used for stall-fallthrough test

static constexpr PASID PASID0 = 0;

// A physically aligned IOVA/PA pair used across tests.
static constexpr IOVA TEST_IOVA = 0x0001'0000ULL;
static constexpr PA   TEST_PA   = 0xDEAD'0000ULL;

// ---------------------------------------------------------------------------
// Helper: build a minimal enabled SMMU with caching on.
// ---------------------------------------------------------------------------
static std::unique_ptr<SMMU> makeSMMU() {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->enableCaching(true);
    return smmu;
}

// ---------------------------------------------------------------------------
// Helper: configure a minimal single-stage stream and create PASID 0.
// Returns false if any setup step failed.
// ---------------------------------------------------------------------------
static bool setupStream(SMMU& smmu, StreamID sid, const StreamConfig& cfg) {
    if (!smmu.configureStream(sid, cfg).isOk()) {
        return false;
    }
    if (!smmu.enableStream(sid).isOk()) {
        return false;
    }
    if (!smmu.createStreamPASID(sid, PASID0).isOk()) {
        return false;
    }
    return true;
}

// ===========================================================================
// Test 1 — STRW promotion applied correctly on the TLB fast path
//
// Defect in lookupTranslationCache() vs inline path:
//   lookupTranslationCache() does not promote the caller's AccessType when
//   STRW == EL2 or EL3.  As a result it would call validateAccessPermissions()
//   with the raw AccessType::Read and return a permission-failure error for a
//   page that has privilegedOnly=true.
//
// The inline translate() path applies the STRW promotion BEFORE the TLB hit
// permission check (translate() lines 368-382), so the promoted access type
// is used for the permission check and the translation succeeds.
//
// Scenario:
//   - STRW = EL2 (non-VHE, single-stage).
//   - Page mapped with privilegedOnly=true (read/write, execute=false).
//   - translate() called with AccessType::Read (unprivileged).
//   - Expected: SUCCEEDS because EL2 STRW promotes Read → ReadPrivileged.
//
// If lookupTranslationCache() were called instead:
//   - No STRW promotion.
//   - validateAccessPermissions(permissions, Read) with privilegedOnly=true
//     would return false.
//   - translate() would fall through to slow path, but the point is that the
//     dead method itself would report the wrong result on a TLB hit.
//
// This test verifies the PRODUCTION PATH (inline) is correct.
// It will still pass after lookupTranslationCache() is deleted.
// ===========================================================================

TEST(Bug4DeadCodeLookup, STRWPromotion_InlinePathPromotesUnprivilegedAccess) {
    auto smmu = makeSMMU();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = StreamWorld::EL2; // EL2 promotes all accesses to privileged
    cfg.t0sz               = 0; // No VA-range restriction

    ASSERT_TRUE(setupStream(*smmu, SID_STRW, cfg))
        << "Stream setup must succeed";

    // Map a privilegedOnly page (read=true, write=true, execute=false, privilegedOnly=true).
    // An unprivileged Read would normally fail this page's permission check.
    PagePermissions privPerms(true, true, false, /*privilegedOnly=*/true);
    ASSERT_TRUE(smmu->mapPage(SID_STRW, PASID0, TEST_IOVA, TEST_PA, privPerms).isOk())
        << "mapPage for privilegedOnly page must succeed";

    // --- First call: guaranteed cache MISS (populate TLB via slow path). ---
    smmu->invalidateStreamCache(SID_STRW);
    TranslationResult miss = smmu->translate(SID_STRW, PASID0, TEST_IOVA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(miss.isOk())
        << "Slow-path (cache miss): STRW=EL2 must promote Read → ReadPrivileged "
           "before the permission check; the privilegedOnly page must be accessible. "
           "Bug 4 reference: lookupTranslationCache() missing STRW promotion (§3.3.4)";
    EXPECT_EQ(miss.getValue().physicalAddress, TEST_PA)
        << "Slow-path PA must equal TEST_PA";

    // --- Second call: guaranteed cache HIT (inline TLB fast path). ---
    // This is the path that lookupTranslationCache() was meant to implement but
    // does not apply STRW promotion.  The inline path in translate() does.
    TranslationResult hit = smmu->translate(SID_STRW, PASID0, TEST_IOVA,
                                             AccessType::Read,
                                             SecurityState::NonSecure);
    ASSERT_TRUE(hit.isOk())
        << "Fast-path (cache hit): STRW=EL2 must promote Read → ReadPrivileged "
           "before the TLB permission check; the privilegedOnly page must be "
           "accessible on TLB hit. "
           "Bug 4: lookupTranslationCache() would fail here because it lacks "
           "STRW promotion — confirming the dead method must not be used (§3.3.4)";
    EXPECT_EQ(hit.getValue().physicalAddress, TEST_PA)
        << "Fast-path PA must equal TEST_PA";
}

// ===========================================================================
// Test 2 — STE output-attribute overrides applied on TLB cache hit
//
// Defect in lookupTranslationCache() vs inline path:
//   lookupTranslationCache() calls makeTranslationSuccess(PA, perms, sec)
//   which constructs a TranslationData with all six output-attribute fields
//   zero-initialised.  It never reads the StreamConfig, so memType,
//   shareability, allocHint, instCfg, privCfg, and nsCfgOut are always 0
//   regardless of STE configuration.
//
//   The inline path in translate() (lines 402-407) applies the StreamConfig
//   output-attribute overrides to TranslationData explicitly on every TLB hit.
//
// Scenario (non-zero output attributes that would be zeroed by dead method):
//   - mtCfg=true, memAttr=5  → memType must be 5
//   - shCfg=3                → shareability must be 3
//   - allocCfg=0xC           → allocHint must be 0xC
//   - privCfg=1              → privCfg must be 1
//   - nsCfg=2                → nsCfgOut must be 2
//   (instCfg=0 to avoid Gap-D INSTCFG override converting Read → Execute)
//
// This test verifies the PRODUCTION PATH on a TLB HIT returns correct attrs.
// It will still pass after lookupTranslationCache() is deleted.
// ===========================================================================

TEST(Bug4DeadCodeLookup, OutputAttributes_TLBHit_CorrectlyAppliesSTEOverrides) {
    auto smmu = makeSMMU();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    // Non-zero STE output-attribute overrides
    cfg.mtCfg    = true;
    cfg.memAttr  = 5;    // memType must be 5 on hit
    cfg.shCfg    = 3;    // shareability must be 3 on hit
    cfg.allocCfg = 0xC;  // allocHint must be 0xC on hit
    cfg.instCfg  = 0;    // 0 = no override; avoids Read→Execute promotion (Gap D)
    cfg.privCfg  = 1;    // privCfg must be 1 on hit
    cfg.nsCfg    = 2;    // nsCfgOut must be 2 on hit

    ASSERT_TRUE(setupStream(*smmu, SID_OUTPUT_ATTRS, cfg))
        << "Stream setup must succeed";

    PagePermissions perms(true, true, false); // read+write, privilegedOnly=false
    ASSERT_TRUE(smmu->mapPage(SID_OUTPUT_ATTRS, PASID0, TEST_IOVA, TEST_PA,
                               perms, SecurityState::NonSecure).isOk())
        << "mapPage must succeed";

    // --- First call: guaranteed miss — populate TLB. ---
    smmu->invalidateStreamCache(SID_OUTPUT_ATTRS);
    TranslationResult miss = smmu->translate(SID_OUTPUT_ATTRS, PASID0, TEST_IOVA,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(miss.isOk())
        << "Slow-path (cache miss) translate must succeed";

    // Verify slow path already returns correct attrs (sanity check).
    {
        const TranslationData& td = miss.getValue();
        EXPECT_EQ(td.memType,      5u)   << "Slow-path memType must be 5";
        EXPECT_EQ(td.shareability, 3u)   << "Slow-path shareability must be 3";
        EXPECT_EQ(td.allocHint,    0xCu) << "Slow-path allocHint must be 0xC";
        EXPECT_EQ(td.privCfg,      1u)   << "Slow-path privCfg must be 1";
        EXPECT_EQ(td.nsCfgOut,     2u)   << "Slow-path nsCfgOut must be 2";
    }

    // --- Second call: guaranteed TLB cache HIT (same IOVA, same stream). ---
    TranslationResult hit = smmu->translate(SID_OUTPUT_ATTRS, PASID0, TEST_IOVA,
                                             AccessType::Read,
                                             SecurityState::NonSecure);
    ASSERT_TRUE(hit.isOk())
        << "Fast-path (cache hit) translate must succeed";

    const TranslationData& td = hit.getValue();

    // These assertions expose what lookupTranslationCache() would return wrong:
    // the dead method calls makeTranslationSuccess() without STE attrs → all 0.
    EXPECT_EQ(td.memType, 5u)
        << "Bug 4: TLB hit must return memType=5 (mtCfg=true, memAttr=5); "
           "lookupTranslationCache() would return 0 — confirming the dead method "
           "is broken and must not be used (ARM §13 STE output attrs)";
    EXPECT_EQ(td.shareability, 3u)
        << "Bug 4: TLB hit must return shareability=3 (shCfg=3); "
           "lookupTranslationCache() would return 0 (ARM §13)";
    EXPECT_EQ(td.allocHint, 0xCu)
        << "Bug 4: TLB hit must return allocHint=0xC (allocCfg=0xC); "
           "lookupTranslationCache() would return 0 (ARM §13)";
    EXPECT_EQ(td.privCfg, 1u)
        << "Bug 4: TLB hit must return privCfg=1; "
           "lookupTranslationCache() would return 0 (ARM §13)";
    EXPECT_EQ(td.nsCfgOut, 2u)
        << "Bug 4: TLB hit must return nsCfgOut=2 (nsCfg=2); "
           "lookupTranslationCache() would return 0 (ARM §13)";
}

// ===========================================================================
// Test 3 — Stall-mode fallthrough from TLB permission failure
//
// Defect in lookupTranslationCache() vs inline path:
//   When the TLB entry exists but the permission check fails (e.g., a write
//   to a read-only page), the inline translate() path falls through to
//   performTwoStageTranslation() (the slow path) so that the stall queue is
//   correctly consulted.  lookupTranslationCache() does not have this fallthrough
//   — it returns CacheEntryNotFound or a plain error and never reaches the stall
//   mode logic, so FaultMode::Stall would be silently ignored.
//
// Scenario:
//   - FaultMode::Stall stream.
//   - Page mapped read-only (write=false).
//   - First translate() with Read → SUCCEEDS, populates TLB.
//   - Second translate() with Write → TLB hit but permission failure.
//   - Expected: SMMUError::Stalled (stall mode respected on permission failure).
//
// If lookupTranslationCache() were called instead of falling through:
//   - The dead method would return CacheEntryNotFound/SecurityFault, not Stalled.
//   - The stall queue would never be consulted.
//
// This test verifies the PRODUCTION PATH falls through correctly.
// It will still pass after lookupTranslationCache() is deleted.
// ===========================================================================

TEST(Bug4DeadCodeLookup, StallModeFallthrough_TLBPermissionFailureRoutesToStallQueue) {
    auto smmu = makeSMMU();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall; // stall on any fault
    cfg.t0sz               = 0;

    ASSERT_TRUE(setupStream(*smmu, SID_STALL, cfg))
        << "Stream setup must succeed";

    // Read-only mapping (write=false). A Write access will fail permission check.
    PagePermissions readOnly(/*read=*/true, /*write=*/false, /*execute=*/false);
    ASSERT_TRUE(smmu->mapPage(SID_STALL, PASID0, TEST_IOVA, TEST_PA,
                               readOnly, SecurityState::NonSecure).isOk())
        << "mapPage must succeed";

    // --- First call: warm up TLB with a Read (succeeds, caches entry). ---
    smmu->invalidateStreamCache(SID_STALL);
    TranslationResult readResult = smmu->translate(SID_STALL, PASID0, TEST_IOVA,
                                                    AccessType::Read,
                                                    SecurityState::NonSecure);
    ASSERT_TRUE(readResult.isOk())
        << "First translate (Read, cache miss) must succeed on a read-only page";

    // --- Second call: Write on same IOVA — TLB HIT but permission failure. ---
    // The inline path falls through to the slow path (§3.12.2 stall-mode handling).
    // lookupTranslationCache() lacks this fallthrough and would return the wrong error.
    TranslationResult writeResult = smmu->translate(SID_STALL, PASID0, TEST_IOVA,
                                                     AccessType::Write,
                                                     SecurityState::NonSecure);
    ASSERT_TRUE(writeResult.isError())
        << "Write on a read-only page must produce an error";
    EXPECT_EQ(writeResult.getError(), SMMUError::Stalled)
        << "Bug 4: FaultMode::Stall stream must return SMMUError::Stalled when a "
           "TLB hit results in a permission failure — the inline translate() path "
           "falls through to performTwoStageTranslation() which invokes the stall "
           "queue.  lookupTranslationCache() lacks this fallthrough and would return "
           "CacheEntryNotFound instead, silently ignoring stall mode (§3.12.2)";
}

} // namespace test
} // namespace smmu
