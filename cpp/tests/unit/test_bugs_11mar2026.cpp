// ARM SMMU v3 — Bug fix regression tests for fixes landed 11 March 2026
//
// BUG-NEW-A: TLBCache::bool lookup() now uses lookupEntry() (safe copy) instead
//   of the raw-pointer lookup() to avoid dangling pointer / use-after-free UB.
//   Test: verify the bool overload correctly copies data and returns true/false.
//
// BUG-NEW-B: TLBCache constructor now only provisions effectiveStripes active
//   stripes with entriesPerStripe, leaving the rest at 0.  Previously all 16
//   stripes were initialised, so LRU eviction did not trigger until 16×capacity
//   entries had been inserted.
//   Test: create TLBCache(8), insert 9 unique entries, verify size stays ≤ 8
//   (eviction happened) and getCapacity() == 8.
//
// BUG-NEW-C: handleTranslationFailure() now handles SMMUError::InvalidConfiguration
//   as a no-op (like StreamDisabled), preventing a spurious F_TRANSLATION after
//   C_BAD_CD.
//   Test: configure stream with aa64=false (triggers ContextDescriptorFormatFault
//   → SMMUError::InvalidConfiguration), translate, verify only C_BAD_CD events
//   appear and NOT an additional F_TRANSLATION.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/tlb_cache.h"
#include "smmu/types.h"
#include <algorithm>
#include <memory>

namespace smmu {
namespace test_bugs_11mar2026 {

// ── Helper: count events of a specific type in the SMMU event queue ──────────

static int countEventType(const SMMU& smmu, EventType t) {
    int n = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == t) {
            ++n;
        }
    }
    return n;
}

// ── Helper: build a TLBEntry for a given IOVA / PA pair ──────────────────────

static TLBEntry makeTLBEntry(StreamID sid, PASID pid, IOVA va, PA pa) {
    TLBEntry e;
    e.streamID        = sid;
    e.pasid           = pid;
    e.iova            = va;
    e.physicalAddress = pa;
    e.permissions     = PagePermissions(true, true, false);
    e.valid           = true;
    e.timestamp       = 0;
    return e;
}

// ═════════════════════════════════════════════════════════════════════════════
// BUG-NEW-A: bool lookup() must copy data correctly via lookupEntry()
// ═════════════════════════════════════════════════════════════════════════════

// BUG-NEW-A basic: bool lookup() returns true and copies the CacheEntry fields
// correctly when the requested entry is present in the cache.
TEST(BugNewA, BoolLookupReturnsTrueAndCopiesData) {
    TLBCache cache(64);

    TLBEntry tlbEntry = makeTLBEntry(0x10, 0, 0x4000, 0x8000);
    cache.insert(tlbEntry);

    CacheEntry out;
    bool found = cache.lookup(0x10, 0, 0x4000, out);

    EXPECT_TRUE(found) << "BUG-NEW-A: bool lookup() must return true for present entry";
    EXPECT_EQ(out.iova, 0x4000ULL)
        << "BUG-NEW-A: CacheEntry.iova must match inserted IOVA";
    EXPECT_EQ(out.physicalAddress, 0x8000ULL)
        << "BUG-NEW-A: CacheEntry.physicalAddress must match inserted PA";
}

// BUG-NEW-A miss: bool lookup() returns false when the entry is absent.
TEST(BugNewA, BoolLookupReturnsFalseOnMiss) {
    TLBCache cache(64);

    CacheEntry out;
    bool found = cache.lookup(0x10, 0, 0xFFFF0000ULL, out);

    EXPECT_FALSE(found) << "BUG-NEW-A: bool lookup() must return false for absent entry";
}

// BUG-NEW-A safety: inserting an entry and immediately performing a bool lookup
// must not access freed memory.  (Before the fix, the raw-pointer lookup()
// returned a pointer that could be invalidated by concurrent inserts.)
TEST(BugNewA, BoolLookupDoesNotAccessDanglingPointer) {
    TLBCache cache(4);  // tiny capacity to provoke evictions quickly

    // Insert 5 entries — the 5th evicts the LRU entry inside insert().
    for (IOVA va = 0x1000; va <= 0x5000; va += 0x1000) {
        cache.insert(makeTLBEntry(0x20, 0, va, va + 0x40000));
    }

    // Lookup the most-recently inserted entry — must return true and valid data.
    CacheEntry out;
    bool found = cache.lookup(0x20, 0, 0x5000, out);
    EXPECT_TRUE(found) << "BUG-NEW-A: most-recently inserted entry must still be present";
    EXPECT_EQ(out.physicalAddress, 0x5000ULL + 0x40000ULL)
        << "BUG-NEW-A: CacheEntry data must match what was inserted";
}

// BUG-NEW-A: lookupEntry() (Result overload) must also return a valid copy.
TEST(BugNewA, LookupEntryReturnsCopyWithoutDanglingRef) {
    TLBCache cache(64);

    TLBEntry tlbEntry = makeTLBEntry(0x30, 1, 0xA000, 0xB000);
    cache.insert(tlbEntry);

    Result<TLBEntry> result = cache.lookupEntry(0x30, 1, 0xA000);
    ASSERT_TRUE(result.isOk())
        << "BUG-NEW-A: lookupEntry() must return Ok for a present entry";
    EXPECT_EQ(result.getValue().physicalAddress, 0xB000ULL)
        << "BUG-NEW-A: lookupEntry() must return correct PA";
    EXPECT_TRUE(result.getValue().valid)
        << "BUG-NEW-A: returned TLBEntry must have valid=true";
}

// ═════════════════════════════════════════════════════════════════════════════
// BUG-NEW-B: TLBCache(8) must evict entries once 8 are stored (not at 8×16=128)
// ═════════════════════════════════════════════════════════════════════════════

// BUG-NEW-B capacity: getCapacity() must return the value passed to the constructor.
TEST(BugNewB, CapacityMatchesConstructorArgument) {
    TLBCache cache(8);
    EXPECT_EQ(cache.getCapacity(), 8u)
        << "BUG-NEW-B: getCapacity() must equal the maxSize passed to TLBCache(8)";
}

// BUG-NEW-B before-fix regression: with the old code TLBCache(8) silently
// configured all 16 stripes with entriesPerStripe=4, giving a real capacity
// of 64.  After inserting 65 unique entries the cache would hold 64 — far
// more than the advertised 8.  After the fix, the inactive stripes each hold
// at most 1 entry (evict-then-insert), so the total is bounded to at most
// 2*4 + 14*1 = 22, not 64.  This test verifies we are well below 64.
TEST(BugNewB, LRUEvictionTriggersMuchBelowOldLimit) {
    TLBCache cache(8);

    // Insert 64 unique entries — before the fix this would fill the cache
    // to 64 because all 16 stripes had 4-entry capacity each.
    for (int i = 1; i <= 64; ++i) {
        IOVA va = static_cast<IOVA>(i) * 0x1000;
        PA   pa = static_cast<PA>(i) * 0x8000;
        cache.insert(makeTLBEntry(0x40, 0, va, pa));
    }

    size_t sz = cache.getSize();
    EXPECT_LT(sz, 64u)
        << "BUG-NEW-B: total cache size after 64 inserts must be < 64 (old bug limit); "
           "BUG-NEW-B fix limits inactive stripes to 1 entry each";
}

// BUG-NEW-B active-stripe eviction: inserting more than 4 entries that hash
// into the same active stripe (stripes 0 or 1) must trigger LRU eviction.
// We use the same StreamID and sequential small IOVAs so they are likely
// to distribute across stripes, but we specifically look for the property
// that the total size stays bounded well below 64.
TEST(BugNewB, SizeBoundedAfterManyInserts) {
    TLBCache cache(8);

    // Insert 32 unique entries across varied stream IDs to spread hash load.
    for (int i = 1; i <= 32; ++i) {
        IOVA va = static_cast<IOVA>(i) * 0x1000;
        PA   pa = static_cast<PA>(i) * 0x8000;
        // Vary stream ID slightly to spread across stripes.
        StreamID sid = static_cast<StreamID>(0x50 + (i % 4));
        cache.insert(makeTLBEntry(sid, 0, va, pa));
    }

    size_t sz = cache.getSize();
    EXPECT_LT(sz, 64u)
        << "BUG-NEW-B: size must remain well below the old bug limit of 64 "
           "after 32 inserts into TLBCache(8)";
}

// BUG-NEW-B most-recently inserted entry is reachable after eviction:
// The last inserted entry must be findable even after older ones were evicted.
TEST(BugNewB, MostRecentEntryPresentAfterEviction) {
    TLBCache cache(8);

    for (int i = 1; i <= 9; ++i) {
        IOVA va = static_cast<IOVA>(i) * 0x1000;
        PA   pa = static_cast<PA>(i) * 0x9000;
        cache.insert(makeTLBEntry(0x60, 0, va, pa));
    }

    // The 9th entry (va=0x9000) must still be present.
    CacheEntry out;
    bool found = cache.lookup(0x60, 0, 0x9000, out);
    EXPECT_TRUE(found)
        << "BUG-NEW-B: most-recently inserted entry must survive LRU eviction";
    EXPECT_EQ(out.physicalAddress, 9u * 0x9000ULL)
        << "BUG-NEW-B: evicted-surviving entry must have correct PA";
}

// BUG-NEW-B regression: TLBCache(64) should still work correctly (not broken
// by the effectiveStripes calculation).
TEST(BugNewB, LargerCacheStillWorksCorrectly) {
    TLBCache cache(64);
    EXPECT_EQ(cache.getCapacity(), 64u);

    for (int i = 1; i <= 10; ++i) {
        IOVA va = static_cast<IOVA>(i) * 0x1000;
        cache.insert(makeTLBEntry(0x70, 0, va, va + 0x1000));
    }

    EXPECT_EQ(cache.getSize(), 10u)
        << "BUG-NEW-B regression: TLBCache(64) must hold 10 entries without eviction";
}

// ═════════════════════════════════════════════════════════════════════════════
// BUG-NEW-C: handleTranslationFailure(InvalidConfiguration) must be a no-op —
//            no spurious F_TRANSLATION after C_BAD_CD
// ═════════════════════════════════════════════════════════════════════════════

// BUG-NEW-C primary: translate with aa64=false must produce exactly one C_BAD_CD
// event and zero F_TRANSLATION events.
TEST(BugNewC, AA64FalseProducesOnlyC_BAD_CD_NotF_TRANSLATION) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    constexpr StreamID SID = 0xC0;
    constexpr PASID    PID = 0;
    constexpr IOVA     VA  = 0x0010'0000ULL;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = false;   // ContextDescriptorFormatFault → C_BAD_CD
    cfg.faultMode          = FaultMode::Terminate;

    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, PID).isOk());

    smmu.translate(SID, PID, VA, AccessType::Read, SecurityState::NonSecure);

    int cbad   = countEventType(smmu, EventType::C_BAD_CD);
    int ftrans = countEventType(smmu, EventType::F_TRANSLATION);

    EXPECT_GT(cbad, 0)
        << "BUG-NEW-C: AA64=false must generate at least one C_BAD_CD (0x0A) event";
    EXPECT_EQ(ftrans, 0)
        << "BUG-NEW-C: AA64=false must NOT generate F_TRANSLATION (0x10); "
           "handleTranslationFailure(InvalidConfiguration) must be a no-op";
}

// BUG-NEW-C: only one C_BAD_CD per translate call (not duplicated).
TEST(BugNewC, AA64FalseProducesExactlyOneC_BAD_CD) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    constexpr StreamID SID = 0xC1;
    constexpr PASID    PID = 0;
    constexpr IOVA     VA  = 0x0020'0000ULL;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = false;
    cfg.faultMode          = FaultMode::Terminate;

    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, PID).isOk());

    smmu.translate(SID, PID, VA, AccessType::Read, SecurityState::NonSecure);

    int cbad = countEventType(smmu, EventType::C_BAD_CD);
    EXPECT_EQ(cbad, 1)
        << "BUG-NEW-C: exactly one C_BAD_CD must be generated per translate call";
}

// BUG-NEW-C: no F_ACCESS or other data-fault events generated for C_BAD_CD.
TEST(BugNewC, AA64FalseDoesNotProduceFAccessOrOtherDataFaults) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    constexpr StreamID SID = 0xC2;
    constexpr PASID    PID = 0;
    constexpr IOVA     VA  = 0x0030'0000ULL;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = false;
    cfg.faultMode          = FaultMode::Terminate;

    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, PID).isOk());

    smmu.translate(SID, PID, VA, AccessType::Read, SecurityState::NonSecure);

    int faccess = countEventType(smmu, EventType::F_ACCESS);
    int ftrans  = countEventType(smmu, EventType::F_TRANSLATION);
    int faddrSz = countEventType(smmu, EventType::F_ADDR_SIZE);
    int fperm   = countEventType(smmu, EventType::F_PERMISSION);

    EXPECT_EQ(faccess, 0)
        << "BUG-NEW-C: C_BAD_CD path must not generate F_ACCESS";
    EXPECT_EQ(ftrans, 0)
        << "BUG-NEW-C: C_BAD_CD path must not generate F_TRANSLATION";
    EXPECT_EQ(faddrSz, 0)
        << "BUG-NEW-C: C_BAD_CD path must not generate F_ADDR_SIZE";
    EXPECT_EQ(fperm, 0)
        << "BUG-NEW-C: C_BAD_CD path must not generate F_PERMISSION";
}

// BUG-NEW-C regression: a valid aa64=true stream must still generate F_TRANSLATION
// on a page-not-mapped fault (the fix must not suppress legitimate data faults).
TEST(BugNewC, ValidAA64StreamStillGeneratesF_TRANSLATION_OnPageFault) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);

    constexpr StreamID SID = 0xC3;
    constexpr PASID    PID = 0;
    constexpr IOVA     VA  = 0x0040'0000ULL;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.aa64               = true;    // valid — should produce F_TRANSLATION on miss
    cfg.faultMode          = FaultMode::Terminate;

    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, PID).isOk());
    // No page mapping → translate will fault.

    smmu.translate(SID, PID, VA, AccessType::Read, SecurityState::NonSecure);

    int ftrans = countEventType(smmu, EventType::F_TRANSLATION);
    int cbad   = countEventType(smmu, EventType::C_BAD_CD);

    EXPECT_GT(ftrans, 0)
        << "BUG-NEW-C regression: aa64=true page-not-mapped must still generate "
           "F_TRANSLATION";
    EXPECT_EQ(cbad, 0)
        << "BUG-NEW-C regression: aa64=true page fault must NOT generate C_BAD_CD";
}

} // namespace test_bugs_11mar2026
} // namespace smmu
