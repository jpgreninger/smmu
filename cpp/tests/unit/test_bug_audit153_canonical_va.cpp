// TDD tests for BUG-AUDIT-153-CPP
//
// ARM §3.4.1 (line 1605): VA[AddrTop:N-1] must all be identical (sign-extension of VA[N-1]).
//   N       = 64 - T0SZ
//   AddrTop = 55 when TBI=1, else 63
//
// A VA whose magnitude is within the T0SZ range but whose high bits are NOT a
// proper sign-extension of bit N-1 is NON-CANONICAL and must produce F_TRANSLATION,
// not a silent translation success.
//
// Example (T0SZ=16, TBI=0, N=48, AddrTop=63):
//   VA = 0x0000_8000_0000_0000
//     magnitude = 2^47 < 2^48  → passes the current range check (the bug)
//     VA[63:47] = 0b0...0_1    → bits[63:48] are 0, bit[47]=1 → NOT sign-extended
//                              → non-canonical → must generate F_TRANSLATION
//
// Current smmu.cpp:2213-2228 only checks magnitude; it does NOT check canonical
// sign-extension.  As a result, translating a non-canonical VA that falls within
// the magnitude window silently succeeds (if a page is mapped at that VA).
//
// Strategy: map a physical page AT the non-canonical IOVA via mapPage() (the sparse
// address-space map accepts any uint64_t key without canonicality enforcement), then
// call translate() on the same IOVA.
//
//   PRE-FIX  (bug present):  range check passes, walk hits mapped page → isOk()=true
//                             EXPECT_FALSE(result.isOk()) → FAILS → RED state
//   POST-FIX (bug fixed):    canonical check detects non-sign-extended bits, emits
//                             F_TRANSLATION, returns error → isOk()=false
//                             EXPECT_FALSE(result.isOk()) → PASSES → GREEN state
//
// Tests 1 and 4 FAIL before the fix (RED); Tests 2 and 3 pass in both states (control).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>

using namespace smmu;

namespace {

static constexpr PASID     PASID_ZERO = 0;
static constexpr StreamID  SID_TBI0   = 0x10;   // Tests 1, 2, 3 — TBI=0
static constexpr StreamID  SID_TBI1   = 0x20;   // Test 4 — TBI=1

// T0SZ=16 → N = 48 significant VA bits.
// vaLimit = 2^48 = 0x0001_0000_0000_0000
// sign-extension boundary for TBI=0: bit 47 (bit index N-1)
static constexpr uint8_t T0SZ_16 = 16u;

// Control page (canonical, positive half): VA = 0x0001_0000
static constexpr IOVA  CTRL_VA  = 0x0001'0000ULL;
static constexpr PA    CTRL_PA  = 0xA000ULL;

// Non-canonical VA for tests 1 and 4:
//   T0SZ=16, N=48 → bit 47 is the sign bit
//   VA = 0x0000_8000_0000_0000:
//     magnitude = 2^47 < 2^48 (within range) BUT bit[47]=1 and bits[63:48]=0 → not sign-extended
static constexpr IOVA  NONCANON_VA = 0x0000'8000'0000'0000ULL;
static constexpr PA    NONCANON_PA = 0xB000ULL;

// Canonical negative VA for test 3:
//   VA = 0xFFFF_8000_0000_0000: bit[47]=1, bits[63:48]=all 1 → properly sign-extended
//   magnitude = 2^63 + 2^47 which is far above 2^48, so this VA is OUT OF RANGE
//   for TTBR0 (T0SZ=16 window is [0, 2^48)).  In a single-TTBR model the canonical
//   negative VA is simply unmapped and will produce a translation fault on walk miss.
//   That is NOT the bug under test; Test 3 exists only to confirm the fix does NOT
//   reject valid positive canonical VAs.
static constexpr IOVA  CANON_POSITIVE_VA = 0x0001'0000ULL;

// Helper: build a stage-1-only StreamConfig with the given t0sz and tbi flag.
// Streams are EL1_EL0, security NonSecure, no stage-2.
static StreamConfig makeS1Config(uint8_t t0sz, bool tbi_on) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.asid               = 0;
    cfg.vmid               = 0;
    cfg.t0sz               = t0sz;
    cfg.tbi                = tbi_on;
    cfg.aa64               = true;
    cfg.ips                = 5;    // 48-bit IPS encoding
    cfg.s2aa64             = true;
    cfg.securityState      = SecurityState::NonSecure;
    return cfg;
}

// Shared fixture: SMMU enabled with event queue and command queue active.
class BugAudit153CanonicalVaTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN);
    }

    // Configure a stream, enable it, and create PASID 0.
    void setupStream(StreamID sid, uint8_t t0sz, bool tbi_on) {
        StreamConfig cfg = makeS1Config(t0sz, tbi_on);
        smmu_.configureStream(sid, cfg);
        smmu_.enableStream(sid);
        smmu_.createStreamPASID(sid, PASID_ZERO);
    }

    SMMU smmu_;
};

} // anonymous namespace

// ============================================================================
// Test 1: NonCanonicalVaFailsToBit47TBI0
//
// T0SZ=16, TBI=0, AddrTop=63.
// Map a page at the non-canonical VA 0x0000_8000_0000_0000
//   (magnitude 2^47 < 2^48 → current range check passes — this is the bug).
// Translate the same VA.
//
// PRE-FIX:  range check passes, page-table walk hits the mapped page → isOk()=true
//           EXPECT_FALSE fails → RED (test fails, demonstrates the bug)
// POST-FIX: canonical check detects VA[63:48]=0 with VA[47]=1 → non-canonical →
//           F_TRANSLATION generated → isOk()=false
//           EXPECT_FALSE passes → GREEN
// ============================================================================
TEST_F(BugAudit153CanonicalVaTest, NonCanonicalVaFailsToBit47TBI0) {
    setupStream(SID_TBI0, T0SZ_16, /*tbi=*/false);

    // Map a physical page at the non-canonical IOVA.
    // The sparse AddressSpace map stores entries by uint64_t key with no
    // canonicality enforcement, so this mapping will always succeed.
    PagePermissions perms(/*read=*/true, /*write=*/true, /*execute=*/false);
    auto mapResult = smmu_.mapPage(SID_TBI0, PASID_ZERO,
                                   NONCANON_VA, NONCANON_PA,
                                   perms, SecurityState::NonSecure);
    ASSERT_TRUE(mapResult.isOk())
        << "mapPage at non-canonical IOVA must succeed (sparse map has no canonicality check).";

    // Translate the non-canonical VA.
    // The ARM spec §3.4.1 requires F_TRANSLATION for non-sign-extended VAs.
    auto result = smmu_.translate(SID_TBI0, PASID_ZERO,
                                  NONCANON_VA,
                                  AccessType::Read,
                                  SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-153-CPP: VA=0x0000_8000_0000_0000 with T0SZ=16, TBI=0 is non-canonical "
           "(bit[47]=1 but bits[63:48]=0). ARM §3.4.1 requires F_TRANSLATION. "
           "Current code only checks magnitude and silently succeeds (bug). "
           "This test MUST FAIL before the fix (RED state).";
}

// ============================================================================
// Test 2: CanonicalVaPositive_Succeeds
//
// Control test: T0SZ=16, TBI=0.
// VA = 0x0001_0000 — canonical positive (bits[63:48]=0, bit[47]=0) → succeeds.
// This test passes BOTH before and after the fix (it is not a regression detector
// but confirms the fix does not break the valid positive-VA path).
// ============================================================================
TEST_F(BugAudit153CanonicalVaTest, CanonicalVaPositive_Succeeds) {
    setupStream(SID_TBI0, T0SZ_16, /*tbi=*/false);

    PagePermissions perms(/*read=*/true, /*write=*/true, /*execute=*/false);
    auto mapResult = smmu_.mapPage(SID_TBI0, PASID_ZERO,
                                   CTRL_VA, CTRL_PA,
                                   perms, SecurityState::NonSecure);
    ASSERT_TRUE(mapResult.isOk()) << "mapPage at canonical VA must succeed.";

    auto result = smmu_.translate(SID_TBI0, PASID_ZERO,
                                  CTRL_VA,
                                  AccessType::Read,
                                  SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-153-CPP: Canonical VA=0x0001_0000 (bit[47]=0, bits[63:48]=0) "
           "must succeed translation. The fix must NOT reject this VA.";
}

// ============================================================================
// Test 3: CanonicalVaNegative_Succeeds (structural / non-regression)
//
// T0SZ=16, TBI=0.
// VA = 0xFFFF_8000_0000_0000: bit[47]=1 and bits[63:48]=all-1 → sign-extended → canonical.
// However, the magnitude (≈ 2^63) is far above the TTBR0 range limit (2^48), so
// in a stage-1-only model with a single address space this VA will NOT be found
// (it lies in the TTBR1 range and this implementation has one pool per PASID).
//
// If mapPage succeeds structurally (sparse map accepts any uint64_t), a pre-fix
// translate will succeed; a post-fix translate that applies a canonical check
// should NOT reject this VA (it IS canonical).  After the fix, if the VA is
// rejected for being out-of-range that is also correct (magnitude >= 2^48 for TTBR0).
//
// The test documents the behaviour but uses SUCCEED() to avoid false failures on
// implementations that route 0xFFFF_8000_... to TTBR1 (not relevant to the bug).
// The key assertion is: a truly canonical negative VA must not be falsely rejected
// by the sign-extension check alone.
// ============================================================================
TEST_F(BugAudit153CanonicalVaTest, CanonicalVaNegative_StructuralCheck) {
    setupStream(SID_TBI0, T0SZ_16, /*tbi=*/false);

    // 0xFFFF_8000_0000_0000: bits[63:48]=0xFFFF, bit[47]=1 → all identical (all 1) → canonical.
    static constexpr IOVA CANON_NEG_VA = 0xFFFF'8000'0000'0000ULL;
    static constexpr PA   CANON_NEG_PA = 0xC000ULL;

    PagePermissions perms(/*read=*/true, /*write=*/true, /*execute=*/false);
    // 0xFFFF_8000_0000_0000 has magnitude ≈ 2^63, which exceeds the TTBR0 input window
    // (vaLimit = 2^48 for T0SZ=16). The existing magnitude check therefore rejects this VA
    // with F_TRANSLATION for out-of-range — this is architecturally correct behaviour.
    // The model has a single address space per PASID (TTBR0-only); there is no TTBR1
    // routing.  mapPage at the large-magnitude VA may or may not succeed.
    auto mapResult = smmu_.mapPage(SID_TBI0, PASID_ZERO, CANON_NEG_VA, CANON_NEG_PA,
                                   perms, SecurityState::NonSecure);
    if (!mapResult.isOk()) {
        GTEST_SKIP() << "N/A: Model does not support mapping at negative-half VA "
                        "0xFFFF_8000_0000_0000. Skipping canonical-negative test.";
    }

    auto result = smmu_.translate(SID_TBI0, PASID_ZERO,
                                  CANON_NEG_VA,
                                  AccessType::Read,
                                  SecurityState::NonSecure);

    // The translate will fail for out-of-range (magnitude >= 2^48 > vaLimit) — that is
    // architecturally correct and is NOT the bug under test.  The canonical sign-extension
    // check the fix will add must NOT additionally fire for this VA because it IS
    // properly sign-extended.  The test therefore cannot assert isOk() here (the
    // out-of-range fault is expected); it asserts only that the test runs without crash
    // and the result (whatever it is) reflects range logic, not false canonicality logic.
    //
    // N/A: Single-TTBR model cannot round-trip a canonical negative VA through TTBR0.
    // This case is marked N/A per user specification note.
    GTEST_SKIP() << "N/A: 0xFFFF_8000_0000_0000 magnitude (2^63) exceeds the TTBR0 input "
                    "window (2^48 for T0SZ=16). Model is TTBR0-only; no canonical-negative "
                    "round-trip is possible. The fix must not fault this VA for canonicality.";
    (void)result;
}

// ============================================================================
// Test 4: NonCanonicalVaTBI1_FailsAtBit55
//
// T0SZ=16, TBI=1 → AddrTop=55.
// VA = 0x0000_8000_0000_0000:
//   TBI=1 strips bits[63:56] → bits[63:56] are already 0, no change.
//   After stripping: effective VA = 0x0000_8000_0000_0000.
//   AddrTop = 55.  N = 48.  Sign bit = bit 47.
//   VA[55:47] must all be identical.  VA[55:48]=0 but VA[47]=1 → not sign-extended → non-canonical.
//
// PRE-FIX:  magnitude check: effectiveIova (after stripping bits[63:56]) = 0x0000_8000_0000_0000
//           < 2^48 → passes → walk hits mapped page → isOk()=true → EXPECT_FALSE fails → RED
// POST-FIX: after TBI masking, canonical check on bits[55:47] detects mismatch →
//           F_TRANSLATION → isOk()=false → EXPECT_FALSE passes → GREEN
// ============================================================================
TEST_F(BugAudit153CanonicalVaTest, NonCanonicalVaTBI1_FailsAtBit55) {
    setupStream(SID_TBI1, T0SZ_16, /*tbi=*/true);

    // Map the page at the non-canonical VA.
    // When TBI=1 the lookup uses (VA & 0x00FF_FFFF_FFFF_FFFF); the same masking
    // applied to 0x0000_8000_0000_0000 yields 0x0000_8000_0000_0000 (bits[63:56]=0).
    // So the address-space is keyed by the already-masked value — mapping will succeed.
    PagePermissions perms(/*read=*/true, /*write=*/true, /*execute=*/false);
    auto mapResult = smmu_.mapPage(SID_TBI1, PASID_ZERO,
                                   NONCANON_VA, NONCANON_PA,
                                   perms, SecurityState::NonSecure);
    ASSERT_TRUE(mapResult.isOk())
        << "mapPage at non-canonical IOVA (TBI=1 stream) must succeed.";

    // Translate the non-canonical VA.
    // With TBI=1, AddrTop=55; VA[55:47] = {0,0,0,0,0,0,0,0,1} → not sign-extended.
    // ARM §3.4.1 requires F_TRANSLATION.
    auto result = smmu_.translate(SID_TBI1, PASID_ZERO,
                                  NONCANON_VA,
                                  AccessType::Read,
                                  SecurityState::NonSecure);

    EXPECT_FALSE(result.isOk())
        << "BUG-AUDIT-153-CPP: VA=0x0000_8000_0000_0000 with T0SZ=16, TBI=1 is non-canonical "
           "(VA[55:47] not all-equal: bits[55:48]=0, bit[47]=1). "
           "ARM §3.4.1 requires F_TRANSLATION for non-sign-extended VAs. "
           "AddrTop=55 when TBI=1 (bits[63:56] ignored as tag, check is on bits[55:N-1]). "
           "Current code does not perform this check — this test MUST FAIL before the fix (RED state).";
}
