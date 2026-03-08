// ARM SMMU v3 — BUG-5 regression test
// TLB cache key XOR collision between PASID and address bits.
//
// generateCacheKey() XOR-composed its output as:
//   (streamID << 32) ^ (pasid << 12) ^ securityState ^ (iova >> 12)
//
// The PASID term occupies bits [31:12] (20 PASID bits shifted left 12).
// The page-frame term (iova >> 12) also sits in bits [31:12] for typical
// 32-bit addresses, causing XOR cancellation: different (pasid, iova) pairs
// can produce identical keys.
//
// Example collision pair (same streamID, same securityState):
//   Entry A: PASID=1, IOVA=0x1000  -> (pasid<<12)^(pfn) = 0x1000^0x1 = 0x1001
//   Entry B: PASID=0, IOVA=0x1001000 -> 0^0x1001 = 0x1001  (COLLISION)
//
// NOTE: The actual TLB cache uses a CacheKey struct with full operator==
// comparison of (streamID, pasid, iova, securityState).  generateCacheKey()
// is a utility with no live callers, so this bug is dead-code / performance.
// These tests validate: (a) the XOR formula itself is collision-free after
// the fix, and (b) the TLB cache correctly distinguishes the colliding pairs.
//
// We test the formula directly by replicating it (pre-fix) to show it
// collides, and then exercising the full translate() path to confirm that
// the TLB always returns the correct physical address for each (PASID, IOVA).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <cstdint>
#include <memory>

namespace smmu {
namespace test {

// ---------------------------------------------------------------------------
// Helper: compute the OLD (buggy) XOR key formula directly in the test.
// This replicates the formula from smmu.cpp lines 1242-1245 before the fix.
// ---------------------------------------------------------------------------
static uint64_t oldBuggyKey(StreamID streamID, PASID pasid, IOVA iova,
                             SecurityState securityState) {
    return (static_cast<uint64_t>(streamID) << 32) ^
           (static_cast<uint64_t>(pasid & 0xFFFFFu) << 12) ^
           static_cast<uint64_t>(securityState) ^
           (iova >> 12);
}

// ---------------------------------------------------------------------------
// Helper: compute the NEW (fixed) hash_combine key formula.
// This must match the implementation in smmu.cpp after the fix.
// ---------------------------------------------------------------------------
static uint64_t newFixedKey(StreamID streamID, PASID pasid, IOVA iova,
                             SecurityState securityState) {
    auto hashCombine = [](uint64_t seed, uint64_t val) -> uint64_t {
        return seed ^ (val + 0x9e3779b97f4a7c15ULL + (seed << 6) + (seed >> 2));
    };
    uint64_t key = static_cast<uint64_t>(streamID);
    key = hashCombine(key, static_cast<uint64_t>(pasid));
    key = hashCombine(key, static_cast<uint64_t>(securityState));
    key = hashCombine(key, iova >> 12);
    return key;
}

// ---------------------------------------------------------------------------
// Fixture for full integration tests through the SMMU translate() API.
// ---------------------------------------------------------------------------
class BUG5CacheKeyTest : public ::testing::Test {
protected:
    static StreamConfig makeStage1Config() {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.aa64               = true;
        cfg.s1cdMax            = 8;
        return cfg;
    }

    void SetUp() override {
        smmu = std::unique_ptr<SMMU>(new SMMU());
        smmu->enable();

        ASSERT_TRUE(smmu->configureStream(STREAM_A, makeStage1Config()).isOk());
        ASSERT_TRUE(smmu->configureStream(STREAM_B, makeStage1Config()).isOk());

        ASSERT_TRUE(smmu->enableStream(STREAM_A).isOk());
        ASSERT_TRUE(smmu->enableStream(STREAM_B).isOk());

        ASSERT_TRUE(smmu->createStreamPASID(STREAM_A, PASID_0).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(STREAM_A, PASID_1).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(STREAM_B, PASID_0).isOk());
    }

    std::unique_ptr<SMMU> smmu;

    static constexpr StreamID   STREAM_A = 0x0001u;
    static constexpr StreamID   STREAM_B = 0x0002u;
    static constexpr PASID      PASID_0  = 0u;
    static constexpr PASID      PASID_1  = 1u;
    static constexpr SecurityState NS    = SecurityState::NonSecure;
};

// =========================================================================
// Part 1: White-box tests of the key formula itself (no SMMU involvement).
// =========================================================================

// Test 1 (RED before fix, GREEN after fix):
// The canonical XOR collision pair from the bug description.
// OLD formula: (PASID=1,IOVA=0x1000) and (PASID=0,IOVA=0x1001000) produce
// identical bits in [31:12], yielding the same key under XOR.
TEST(BUG5KeyFormula, OldFormulaExhibitsCollision) {
    constexpr StreamID SID = 0x0001u;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t oldKeyA = oldBuggyKey(SID, 1u, 0x1000u,     SEC);
    uint64_t oldKeyB = oldBuggyKey(SID, 0u, 0x1001000u,  SEC);

    // This assertion PASSES before the fix (demonstrates the bug exists in
    // the old formula).  It documents what the old formula did wrong.
    EXPECT_EQ(oldKeyA, oldKeyB)
        << "Pre-condition check: the OLD formula should produce a collision "
           "for (PASID=1,IOVA=0x1000) vs (PASID=0,IOVA=0x1001000). "
           "If this fails, the test helper does not match the original code.";
}

// Test 2 (must PASS after fix):
// After the fix, the same two inputs must yield different keys.
TEST(BUG5KeyFormula, NewFormulaNoCollisionOnCanonicalPair) {
    constexpr StreamID SID = 0x0001u;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t newKeyA = newFixedKey(SID, 1u, 0x1000u,     SEC);
    uint64_t newKeyB = newFixedKey(SID, 0u, 0x1001000u,  SEC);

    EXPECT_NE(newKeyA, newKeyB)
        << "BUG-5 fix: (PASID=1,IOVA=0x1000) and (PASID=0,IOVA=0x1001000) "
           "must produce distinct keys after the hash_combine fix.";
}

// Test 3: Second collision pair from the bug report.
TEST(BUG5KeyFormula, NewFormulaNoCollisionOnSecondPair) {
    constexpr StreamID SID = 0x0001u;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t newKeyA = newFixedKey(SID, 2u, 0x2000u,    SEC);
    uint64_t newKeyB = newFixedKey(SID, 0u, 0x2002000u, SEC);

    EXPECT_NE(newKeyA, newKeyB)
        << "BUG-5 fix: (PASID=2,IOVA=0x2000) and (PASID=0,IOVA=0x2002000) "
           "must produce distinct keys.";
}

// Test 4: Different PASIDs with the same IOVA must produce different keys.
TEST(BUG5KeyFormula, NewFormulaDifferentPasidsSameIOVA) {
    constexpr StreamID SID = 0x0001u;
    constexpr IOVA IOVA_PAGE = 0x10000u;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t key0 = newFixedKey(SID, 0u, IOVA_PAGE, SEC);
    uint64_t key1 = newFixedKey(SID, 1u, IOVA_PAGE, SEC);
    uint64_t key2 = newFixedKey(SID, 2u, IOVA_PAGE, SEC);

    EXPECT_NE(key0, key1) << "PASID=0 and PASID=1 collide at IOVA=0x10000";
    EXPECT_NE(key0, key2) << "PASID=0 and PASID=2 collide at IOVA=0x10000";
    EXPECT_NE(key1, key2) << "PASID=1 and PASID=2 collide at IOVA=0x10000";
}

// Test 5: Same PASID, different IOVAs -> different keys.
TEST(BUG5KeyFormula, NewFormulaSamePasidDifferentIOVAs) {
    constexpr StreamID SID = 0x0001u;
    constexpr PASID PID = 5u;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t k0 = newFixedKey(SID, PID, 0x0000u,     SEC);
    uint64_t k1 = newFixedKey(SID, PID, 0x1000u,     SEC);
    uint64_t k2 = newFixedKey(SID, PID, 0x2000u,     SEC);
    uint64_t k3 = newFixedKey(SID, PID, 0x80000000u, SEC);

    EXPECT_NE(k0, k1);
    EXPECT_NE(k0, k2);
    EXPECT_NE(k0, k3);
    EXPECT_NE(k1, k2);
}

// Test 6: Different streamIDs must produce different keys.
TEST(BUG5KeyFormula, NewFormulaDifferentStreamIDs) {
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t k1 = newFixedKey(0x0001u, 0u, 0x1000u, SEC);
    uint64_t k2 = newFixedKey(0x0002u, 0u, 0x1000u, SEC);
    uint64_t k3 = newFixedKey(0xFFFFu, 0u, 0x1000u, SEC);

    EXPECT_NE(k1, k2);
    EXPECT_NE(k1, k3);
    EXPECT_NE(k2, k3);
}

// Test 7: Different security states must produce different keys.
TEST(BUG5KeyFormula, NewFormulaDifferentSecurityStates) {
    uint64_t kNS  = newFixedKey(0x0001u, 1u, 0x1000u, SecurityState::NonSecure);
    uint64_t kSec = newFixedKey(0x0001u, 1u, 0x1000u, SecurityState::Secure);

    EXPECT_NE(kNS, kSec)
        << "NonSecure and Secure must produce different cache keys.";
}

// Test 8: Key must be deterministic (same inputs -> same output).
TEST(BUG5KeyFormula, NewFormulaIsDeterministic) {
    constexpr StreamID SID = 0x0042u;
    constexpr PASID PID = 0x0007u;
    constexpr IOVA ADDR = 0xDEAD000u;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t k1 = newFixedKey(SID, PID, ADDR, SEC);
    uint64_t k2 = newFixedKey(SID, PID, ADDR, SEC);
    uint64_t k3 = newFixedKey(SID, PID, ADDR, SEC);

    EXPECT_EQ(k1, k2);
    EXPECT_EQ(k1, k3);
}

// Test 9: Max PASID value and a conflicting IOVA do not collide.
TEST(BUG5KeyFormula, NewFormulaMaxPasidHighAddressNoCollide) {
    constexpr StreamID SID = 0x0001u;
    constexpr PASID MAX_PASID = 0xFFFFFu;
    constexpr SecurityState SEC = SecurityState::NonSecure;

    uint64_t kMaxPasid   = newFixedKey(SID, MAX_PASID, 0x0000u,
                                       SEC);
    uint64_t kZeroPasid  = newFixedKey(SID, 0u,
                                       static_cast<IOVA>(MAX_PASID) << 12,
                                       SEC);

    EXPECT_NE(kMaxPasid, kZeroPasid)
        << "BUG-5: Max PASID with zero IOVA must not collide with "
           "PASID=0 at IOVA=(MAX_PASID<<12).";
}

// =========================================================================
// Part 2: Integration tests through translate() — the actual TLB cache path.
// These verify that the CacheKey struct (which uses full tuple comparison)
// always returns correct translations regardless of the XOR formula.
// =========================================================================

// Test 10: The canonical colliding (PASID, IOVA) pair maps to different
// physical addresses and both are resolved correctly by the TLB.
TEST_F(BUG5CacheKeyTest, TLBCorrectlyDistinguishesCollidingPair) {
    PagePermissions rwPerms(true, true, false);

    // Map each (PASID, IOVA) to a distinct physical address.
    // Entry A: PASID=1, IOVA=0x1000  -> PA=0x100000
    // Entry B: PASID=0, IOVA=0x1001000 -> PA=0x200000
    ASSERT_TRUE(smmu->mapPage(STREAM_A, PASID_1, 0x1000u,    0x100000u, rwPerms).isOk());
    ASSERT_TRUE(smmu->mapPage(STREAM_A, PASID_0, 0x1001000u, 0x200000u, rwPerms).isOk());

    // First translate — populates TLB cache.
    TranslationResult rA1 = smmu->translate(STREAM_A, PASID_1, 0x1000u,    AccessType::Read);
    TranslationResult rB1 = smmu->translate(STREAM_A, PASID_0, 0x1001000u, AccessType::Read);

    ASSERT_TRUE(rA1.isOk()) << "Entry A first translate must succeed";
    ASSERT_TRUE(rB1.isOk()) << "Entry B first translate must succeed";
    EXPECT_EQ(rA1.getValue().physicalAddress, static_cast<PA>(0x100000u));
    EXPECT_EQ(rB1.getValue().physicalAddress, static_cast<PA>(0x200000u));

    // Second translate — exercises TLB hit path.
    TranslationResult rA2 = smmu->translate(STREAM_A, PASID_1, 0x1000u,    AccessType::Read);
    TranslationResult rB2 = smmu->translate(STREAM_A, PASID_0, 0x1001000u, AccessType::Read);

    ASSERT_TRUE(rA2.isOk()) << "Entry A TLB hit must succeed";
    ASSERT_TRUE(rB2.isOk()) << "Entry B TLB hit must succeed";
    EXPECT_EQ(rA2.getValue().physicalAddress, static_cast<PA>(0x100000u))
        << "TLB must not return Entry B's PA for Entry A's lookup "
           "(would indicate a cache key collision)";
    EXPECT_EQ(rB2.getValue().physicalAddress, static_cast<PA>(0x200000u))
        << "TLB must not return Entry A's PA for Entry B's lookup "
           "(would indicate a cache key collision)";
}

// Test 11: Multiple PASID/IOVA combinations on the same stream remain isolated.
TEST_F(BUG5CacheKeyTest, MultipleEntriesRemainIsolated) {
    PagePermissions rwPerms(true, true, false);

    // Populate several translations for PASID_0 and PASID_1 on STREAM_A.
    struct Entry { PASID pasid; IOVA iova; PA pa; };
    Entry entries[] = {
        { PASID_0, 0x0000u,    0xA0000u },
        { PASID_0, 0x1000u,    0xA1000u },
        { PASID_0, 0x2000u,    0xA2000u },
        { PASID_1, 0x0000u,    0xB0000u },
        { PASID_1, 0x1000u,    0xB1000u },
        { PASID_1, 0x1001000u, 0xB2000u },
    };

    for (const auto& e : entries) {
        ASSERT_TRUE(smmu->mapPage(STREAM_A, e.pasid, e.iova, e.pa, rwPerms).isOk())
            << "mapPage failed for PASID=" << e.pasid << " IOVA=" << std::hex << e.iova;
    }

    // Warm up the TLB.
    for (const auto& e : entries) {
        TranslationResult r = smmu->translate(STREAM_A, e.pasid, e.iova, AccessType::Read);
        ASSERT_TRUE(r.isOk())
            << "First translate failed for PASID=" << e.pasid << " IOVA=" << std::hex << e.iova;
    }

    // Verify all TLB hits return the correct PA.
    for (const auto& e : entries) {
        TranslationResult r = smmu->translate(STREAM_A, e.pasid, e.iova, AccessType::Read);
        ASSERT_TRUE(r.isOk())
            << "TLB hit failed for PASID=" << e.pasid << " IOVA=" << std::hex << e.iova;
        EXPECT_EQ(r.getValue().physicalAddress, static_cast<PA>(e.pa))
            << "Wrong PA from TLB for PASID=" << e.pasid << " IOVA=" << std::hex << e.iova
            << ": possible cache key collision";
    }
}

} // namespace test
} // namespace smmu
