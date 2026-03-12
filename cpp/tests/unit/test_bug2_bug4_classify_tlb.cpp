// BUG-2: classifyAccess() returns Unknown for privileged AccessTypes
// BUG-4: Deprecated TLBCache::lookup(bool) and invalidatePage() hardcode NonSecure
//
// TDD: These tests MUST FAIL before any fix is applied.
//
// BUG-2 spec reference: ARM IHI0070G.b §7.3 — the InD field in fault syndrome
// is 1-bit (0=Data, 1=Instruction). There is no "Unknown" encoding.
// classifyAccess() is a private method observed through FaultSyndrome::accessClass
// on FaultRecord objects returned by getEvents(). A two-stage stream is required
// because recordComprehensiveFault (which populates syndrome) is only called from
// the two-stage translation path performBothStagesTranslation().
//   AccessType::ExecutePrivileged  → InstructionFetch
//   AccessType::ReadPrivileged     → DataAccess
//   AccessType::WritePrivileged    → DataAccess
//   AccessType::ReadWritePrivileged→ DataAccess
//   AccessType::ReadWrite          → DataAccess
//
// BUG-4 spec reference: ARM §3.13 — TLB lookups must be keyed on
// (StreamID, SubstreamID/PASID, IPA/VA, SecurityState). A deprecated helper
// that silently discards SecurityState will miss Secure entries.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/tlb_cache.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ============================================================
// BUG-2: classifyAccess() — observed via FaultSyndrome::accessClass
// ============================================================
//
// Two-stage stream setup mirrors test_bug_cpp2_fault_syndrome.cpp.
// PASID=0 holds the stage-2 address space.
// PASID=1 holds the stage-1 address space (with restricted permissions).
// Accessing PASID=1 causes a permission fault in stage-1, which calls
// recordComprehensiveFault() → generateFaultSyndrome() → classifyAccess().

static constexpr StreamID B2_STREAM   = 0xC0;
static constexpr PASID    B2_PASID_S2 = 0;    // stage-2 PASID
static constexpr PASID    B2_PASID_S1 = 1;    // stage-1 PASID
static constexpr IOVA     B2_IOVA     = 0x3000'0000ULL;
static constexpr PA       B2_IPA      = 0x5000'0000ULL;
static constexpr PA       B2_PA       = 0x7000'0000ULL;

// Build a two-stage SMMU.
// Stage-1 (PASID=1): read-only page (write/execute access will fault).
// Stage-2 (PASID=0): read-write page (no restriction from stage-2).
static std::unique_ptr<SMMU> makeB2Smmu(bool withExecute = false) {
    auto s = std::unique_ptr<SMMU>(new SMMU());
    s->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    s->configureStream(B2_STREAM, cfg);
    s->enableStream(B2_STREAM);
    s->createStreamPASID(B2_STREAM, B2_PASID_S2);
    s->createStreamPASID(B2_STREAM, B2_PASID_S1);

    // Stage-1: read-only for write tests; read+write (no execute) for execute tests.
    PagePermissions s1perms = withExecute
        ? PagePermissions(true, true, false)   // no execute → execute access faults
        : PagePermissions(true, false, false);  // no write   → write access faults

    s->mapPage(B2_STREAM, B2_PASID_S1, B2_IOVA, B2_IPA, s1perms, SecurityState::NonSecure);

    // Stage-2: read+write+execute allowed (not the restricting factor).
    PagePermissions s2perms(true, true, true);
    s->mapPage(B2_STREAM, B2_PASID_S2, B2_IPA, B2_PA, s2perms, SecurityState::NonSecure);
    return s;
}

// Drain all events and return the first valid syndrome.
static FaultSyndrome drainSyndrome(SMMU& smmu) {
    auto evResult = smmu.getEvents();
    if (!evResult.isOk()) {
        return FaultSyndrome{};
    }
    for (const FaultRecord& rec : evResult.getValue()) {
        if (rec.syndrome.validSyndrome) {
            return rec.syndrome;
        }
    }
    return FaultSyndrome{};
}

// ── Test: ExecutePrivileged → InstructionFetch ────────────────────────────
//
// Stage-1 has no execute permission → ExecutePrivileged faults.
// classifyAccess(ExecutePrivileged) must return InstructionFetch (not Unknown).

TEST(ClassifyAccessBug2, ExecutePrivilegedProducesInstructionFetchClass) {
    auto smmu = makeB2Smmu(/*withExecute=*/true); // stage-1 has no execute
    smmu->translate(B2_STREAM, B2_PASID_S1, B2_IOVA,
                    AccessType::ExecutePrivileged, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-2: No valid syndrome for ExecutePrivileged — ensure two-stage path fires";
    EXPECT_EQ(syn.accessClass, AccessClassification::InstructionFetch)
        << "ARM §7.3: ExecutePrivileged must classify as InstructionFetch, not Unknown. "
           "classifyAccess() is missing AccessType::ExecutePrivileged case.";
}

// ── Test: ReadPrivileged → DataAccess ────────────────────────────────────
//
// Unmapped address on stage-1 → translation fault (classifyAccess is still called).

TEST(ClassifyAccessBug2, ReadPrivilegedProducesDataAccessClass) {
    auto smmu = makeB2Smmu();
    constexpr IOVA unmapped = 0x9000'0000ULL;  // not mapped in stage-1
    smmu->translate(B2_STREAM, B2_PASID_S1, unmapped,
                    AccessType::ReadPrivileged, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-2: No valid syndrome for ReadPrivileged on unmapped page";
    EXPECT_EQ(syn.accessClass, AccessClassification::DataAccess)
        << "ARM §7.3: ReadPrivileged must classify as DataAccess, not Unknown.";
}

// ── Test: WritePrivileged → DataAccess ───────────────────────────────────
//
// Stage-1 is read-only → WritePrivileged faults on permission.

TEST(ClassifyAccessBug2, WritePrivilegedProducesDataAccessClass) {
    auto smmu = makeB2Smmu(); // stage-1: read-only
    smmu->translate(B2_STREAM, B2_PASID_S1, B2_IOVA,
                    AccessType::WritePrivileged, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-2: No valid syndrome for WritePrivileged";
    EXPECT_EQ(syn.accessClass, AccessClassification::DataAccess)
        << "ARM §7.3: WritePrivileged must classify as DataAccess, not Unknown.";
}

// ── Test: ReadWritePrivileged → DataAccess ────────────────────────────────

TEST(ClassifyAccessBug2, ReadWritePrivilegedProducesDataAccessClass) {
    auto smmu = makeB2Smmu(); // stage-1: read-only → ReadWritePrivileged faults
    smmu->translate(B2_STREAM, B2_PASID_S1, B2_IOVA,
                    AccessType::ReadWritePrivileged, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-2: No valid syndrome for ReadWritePrivileged";
    EXPECT_EQ(syn.accessClass, AccessClassification::DataAccess)
        << "ARM §7.3: ReadWritePrivileged must classify as DataAccess, not Unknown.";
}

// ── Test: ReadWrite → DataAccess ──────────────────────────────────────────

TEST(ClassifyAccessBug2, ReadWriteProducesDataAccessClass) {
    auto smmu = makeB2Smmu(); // stage-1: read-only → ReadWrite faults
    smmu->translate(B2_STREAM, B2_PASID_S1, B2_IOVA,
                    AccessType::ReadWrite, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-2: No valid syndrome for ReadWrite";
    EXPECT_EQ(syn.accessClass, AccessClassification::DataAccess)
        << "ARM §7.3: ReadWrite must classify as DataAccess, not Unknown.";
}

// ── Regression guards — pre-existing AccessType cases ────────────────────

TEST(ClassifyAccessBug2, ExecuteProducesInstructionFetchClass) {
    auto smmu = makeB2Smmu(/*withExecute=*/true);
    smmu->translate(B2_STREAM, B2_PASID_S1, B2_IOVA,
                    AccessType::Execute, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "Regression: No syndrome for Execute (existing case)";
    EXPECT_EQ(syn.accessClass, AccessClassification::InstructionFetch);
}

TEST(ClassifyAccessBug2, WriteProducesDataAccessClass) {
    auto smmu = makeB2Smmu(); // stage-1: read-only → Write faults
    smmu->translate(B2_STREAM, B2_PASID_S1, B2_IOVA,
                    AccessType::Write, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "Regression: No syndrome for Write (existing case)";
    EXPECT_EQ(syn.accessClass, AccessClassification::DataAccess);
}

TEST(ClassifyAccessBug2, ReadProducesDataAccessClass) {
    auto smmu = makeB2Smmu();
    constexpr IOVA unmapped = 0x9000'0000ULL;
    smmu->translate(B2_STREAM, B2_PASID_S1, unmapped,
                    AccessType::Read, SecurityState::NonSecure);

    FaultSyndrome syn = drainSyndrome(*smmu);
    ASSERT_TRUE(syn.validSyndrome)
        << "Regression: No syndrome for Read (existing case)";
    EXPECT_EQ(syn.accessClass, AccessClassification::DataAccess);
}

// ============================================================
// BUG-4: Deprecated TLBCache::lookup(CacheEntry&) and
//        invalidatePage() must accept SecurityState parameter
// ============================================================
//
// The deprecated bool TLBCache::lookup(StreamID, PASID, IOVA, CacheEntry&)
// internally called lookup(streamID, pasid, iova) which defaulted to
// SecurityState::NonSecure. If the entry was inserted as Secure it would
// not be found.
//
// The fix: add `SecurityState securityState = SecurityState::NonSecure`
// to both deprecated signatures and forward it correctly.

class TLBCacheDeprecatedTest : public ::testing::Test {
protected:
    void SetUp() override {
        cache = std::unique_ptr<TLBCache>(new TLBCache(64));
    }

    std::unique_ptr<TLBCache> cache;

    static constexpr StreamID STREAM  = 0x10;
    static constexpr PASID    PASID_V = 0x1;
    static constexpr IOVA     IOVA_V  = 0x1000'0000ULL;
    static constexpr PA       PA_V    = 0x8000'0000ULL;

    TLBEntry makeEntry(SecurityState ss) {
        TLBEntry e;
        e.streamID        = STREAM;
        e.pasid           = PASID_V;
        e.iova            = IOVA_V;
        e.physicalAddress = PA_V;
        e.permissions     = PagePermissions(true, false, false);
        e.securityState   = ss;
        e.valid           = true;
        e.timestamp       = 42;
        return e;
    }
};

// Inserting a Secure entry and looking it up via the deprecated overload
// with SecurityState::Secure must find the entry.
TEST_F(TLBCacheDeprecatedTest, SecureEntryFoundByDeprecatedLookupWithSecureState) {
    cache->insert(makeEntry(SecurityState::Secure));

    CacheEntry found;
    bool hit = cache->lookup(STREAM, PASID_V, IOVA_V, found, SecurityState::Secure);
    EXPECT_TRUE(hit)
        << "BUG-4: deprecated lookup must find Secure entry when "
           "SecurityState::Secure is passed";
    if (hit) {
        EXPECT_EQ(found.physicalAddress, PA_V);
        EXPECT_EQ(found.securityState, SecurityState::Secure);
    }
}

// The same Secure entry must NOT be found when queried as NonSecure.
TEST_F(TLBCacheDeprecatedTest, SecureEntryNotFoundByDeprecatedLookupWithNonSecureState) {
    cache->insert(makeEntry(SecurityState::Secure));

    CacheEntry found;
    bool hit = cache->lookup(STREAM, PASID_V, IOVA_V, found, SecurityState::NonSecure);
    EXPECT_FALSE(hit)
        << "BUG-4: a Secure entry must not be returned for a NonSecure lookup";
}

// Backward compat: NonSecure entry found with no SecurityState argument.
TEST_F(TLBCacheDeprecatedTest, NonSecureEntryFoundByDeprecatedLookupDefaultArg) {
    cache->insert(makeEntry(SecurityState::NonSecure));

    CacheEntry found;
    bool hit = cache->lookup(STREAM, PASID_V, IOVA_V, found);
    EXPECT_TRUE(hit)
        << "BUG-4: backward-compat default (NonSecure) must still find NonSecure entries";
}

// invalidatePage with SecurityState::Secure must evict only the Secure entry.
TEST_F(TLBCacheDeprecatedTest, InvalidatePageSecureEvictsOnlySecureEntry) {
    cache->insert(makeEntry(SecurityState::Secure));
    cache->insert(makeEntry(SecurityState::NonSecure));

    cache->invalidatePage(STREAM, PASID_V, IOVA_V, SecurityState::Secure);

    CacheEntry found;
    EXPECT_FALSE(cache->lookup(STREAM, PASID_V, IOVA_V, found, SecurityState::Secure))
        << "BUG-4: invalidatePage(Secure) must evict the Secure entry";
    EXPECT_TRUE(cache->lookup(STREAM, PASID_V, IOVA_V, found, SecurityState::NonSecure))
        << "BUG-4: invalidatePage(Secure) must leave NonSecure entry intact";
}

} // namespace test
} // namespace smmu
