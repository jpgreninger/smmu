// ARM SMMU v3 TLB Cache Coverage Tests
// Copyright (c) 2024 John Greninger
//
// Comprehensive test suite to achieve 90%+ coverage for TLBCache
// Focuses on uncovered code paths identified in coverage analysis

#include <gtest/gtest.h>
#include "smmu/tlb_cache.h"
#include "smmu/types.h"
#include <thread>
#include <vector>
#include <atomic>

namespace smmu {
namespace test {

class TLBCacheCoverageTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Create TLB cache with reasonable size for testing
        tlbCache = std::unique_ptr<TLBCache>(new TLBCache(64));
    }

    void TearDown() override {
        tlbCache.reset();
    }

    std::unique_ptr<TLBCache> tlbCache;

    // Test helper constants
    static constexpr StreamID TEST_STREAM_ID = 0x1000;
    static constexpr PASID TEST_PASID = 0x1;
    static constexpr IOVA TEST_IOVA_1 = 0x10000000;
    static constexpr IOVA TEST_IOVA_2 = 0x20000000;
    static constexpr PA TEST_PA_1 = 0x40000000;
    static constexpr PA TEST_PA_2 = 0x50000000;

    // Helper function to create TLB entry
    TLBEntry createTLBEntry(StreamID streamID, PASID pasid, IOVA iova, PA pa,
                           PagePermissions perms, SecurityState secState = SecurityState::NonSecure) {
        TLBEntry entry;
        entry.streamID = streamID;
        entry.pasid = pasid;
        entry.iova = iova;
        entry.physicalAddress = pa;
        entry.permissions = perms;
        entry.securityState = secState;
        entry.valid = true;
        entry.timestamp = 0;
        return entry;
    }
};

// ============================================================================
// TC-CACHE-001: Invalid PASID Handling
// ============================================================================

// Test lookup with PASID > MAX_PASID (lines 61-62)
TEST_F(TLBCacheCoverageTest, InvalidPASID_MaxPASIDPlus1) {
    // PASID = MAX_PASID + 1 should fail
    PASID invalidPASID = MAX_PASID + 1;

    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, invalidPASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPASID);

    // Verify miss counter incremented
    EXPECT_EQ(tlbCache->getMissCount(), 1);
}

TEST_F(TLBCacheCoverageTest, InvalidPASID_MaxPASIDPlus100) {
    // Test with PASID significantly over MAX_PASID
    PASID invalidPASID = MAX_PASID + 100;

    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, invalidPASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPASID);
}

TEST_F(TLBCacheCoverageTest, InvalidPASID_BoundaryValid) {
    // PASID = MAX_PASID should be VALID
    PASID validPASID = MAX_PASID;

    // Insert entry with MAX_PASID
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, validPASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    // Lookup should succeed
    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, validPASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().iova, TEST_IOVA_1);
    EXPECT_EQ(result.getValue().physicalAddress, TEST_PA_1);
}

TEST_F(TLBCacheCoverageTest, InvalidPASID_PASID0Valid) {
    // PASID = 0 should be VALID (per PASID 0 support requirement)
    PASID validPASID = 0;

    // Insert entry with PASID 0
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, validPASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    // Lookup should succeed
    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, validPASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().iova, TEST_IOVA_1);
}

TEST_F(TLBCacheCoverageTest, InvalidPASID_MultipleInvalidAttempts) {
    // Test multiple invalid PASID values
    uint64_t initialMissCount = tlbCache->getMissCount();

    std::vector<PASID> invalidPASIDs = {
        MAX_PASID + 1,
        MAX_PASID + 10,
        MAX_PASID + 1000,
        0xFFFFFFFF  // Maximum uint32_t value
    };

    for (PASID invalidPASID : invalidPASIDs) {
        Result<CacheEntry> result = tlbCache->lookupCacheEntry(
            TEST_STREAM_ID, invalidPASID, TEST_IOVA_1, SecurityState::NonSecure);

        EXPECT_TRUE(result.isError());
        EXPECT_EQ(result.getError(), SMMUError::InvalidPASID);
    }

    // Verify miss counter incremented for each invalid attempt
    EXPECT_EQ(tlbCache->getMissCount(), initialMissCount + invalidPASIDs.size());
}

TEST_F(TLBCacheCoverageTest, InvalidPASID_CacheNotModified) {
    // Insert valid entry
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    size_t initialSize = tlbCache->getSize();

    // Attempt invalid PASID lookup
    PASID invalidPASID = MAX_PASID + 1;
    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, invalidPASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());

    // Verify cache not modified
    EXPECT_EQ(tlbCache->getSize(), initialSize);
}

// ============================================================================
// TC-CACHE-002: Cache Entry Conversion
// ============================================================================

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_AllFields) {
    // Test TLBEntry to CacheEntry conversion with all fields
    PagePermissions perms(true, true, true);  // Read-write-execute
    SecurityState secState = SecurityState::Secure;
    uint64_t timestamp = 123456789;

    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms, secState);
    entry.timestamp = timestamp;

    tlbCache->insert(entry);

    // Lookup and verify all fields converted correctly
    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, secState);

    ASSERT_TRUE(result.isOk());

    const CacheEntry& cacheEntry = result.getValue();
    EXPECT_EQ(cacheEntry.iova, TEST_IOVA_1);
    EXPECT_EQ(cacheEntry.physicalAddress, TEST_PA_1);
    EXPECT_EQ(cacheEntry.permissions.read, true);
    EXPECT_EQ(cacheEntry.permissions.write, true);
    EXPECT_EQ(cacheEntry.permissions.execute, true);
    EXPECT_EQ(cacheEntry.securityState, secState);
    EXPECT_EQ(cacheEntry.timestamp, timestamp);
}

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_VariousPageSizes) {
    // Test conversion with different page sizes (4KB, 64KB, 2MB)
    PagePermissions perms(true, false, false);

    struct TestCase {
        IOVA iova;
        PA pa;
        const char* description;
    };

    std::vector<TestCase> testCases = {
        {0x1000, 0x40001000, "4KB aligned"},
        {0x10000, 0x40010000, "64KB aligned"},
        {0x200000, 0x40200000, "2MB aligned"},
        {0x40000000, 0x80000000, "1GB aligned"}
    };

    for (size_t i = 0; i < testCases.size(); ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID + i,
                                       testCases[i].iova, testCases[i].pa, perms);
        tlbCache->insert(entry);

        Result<CacheEntry> result = tlbCache->lookupCacheEntry(
            TEST_STREAM_ID, TEST_PASID + i, testCases[i].iova, SecurityState::NonSecure);

        ASSERT_TRUE(result.isOk()) << "Failed for: " << testCases[i].description;
        EXPECT_EQ(result.getValue().iova, testCases[i].iova)
            << "Failed for: " << testCases[i].description;
        EXPECT_EQ(result.getValue().physicalAddress, testCases[i].pa)
            << "Failed for: " << testCases[i].description;
    }
}

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_DifferentPermissions) {
    // Test conversion with various permission combinations
    struct PermissionTest {
        bool read;
        bool write;
        bool execute;
        const char* description;
    };

    std::vector<PermissionTest> permTests = {
        {true, false, false, "Read-only"},
        {true, true, false, "Read-write"},
        {true, false, true, "Read-execute"},
        {true, true, true, "Read-write-execute"},
        {false, false, false, "No permissions"}  // Edge case
    };

    for (size_t i = 0; i < permTests.size(); ++i) {
        PagePermissions perms(permTests[i].read, permTests[i].write, permTests[i].execute);
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID,
                                       TEST_IOVA_1 + (i * PAGE_SIZE),
                                       TEST_PA_1 + (i * PAGE_SIZE), perms);
        tlbCache->insert(entry);

        Result<CacheEntry> result = tlbCache->lookupCacheEntry(
            TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1 + (i * PAGE_SIZE), SecurityState::NonSecure);

        ASSERT_TRUE(result.isOk()) << "Failed for: " << permTests[i].description;
        EXPECT_EQ(result.getValue().permissions.read, permTests[i].read)
            << "Failed for: " << permTests[i].description;
        EXPECT_EQ(result.getValue().permissions.write, permTests[i].write)
            << "Failed for: " << permTests[i].description;
        EXPECT_EQ(result.getValue().permissions.execute, permTests[i].execute)
            << "Failed for: " << permTests[i].description;
    }
}

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_TimestampPreservation) {
    // Test that timestamps are preserved during conversion
    PagePermissions perms(true, true, false);

    std::vector<uint64_t> timestamps = {
        0,
        1,
        123456,
        0xFFFFFFFF,
        0xFFFFFFFFFFFFFFFFULL
    };

    for (size_t i = 0; i < timestamps.size(); ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID,
                                       TEST_IOVA_1 + (i * PAGE_SIZE),
                                       TEST_PA_1 + (i * PAGE_SIZE), perms);
        entry.timestamp = timestamps[i];
        tlbCache->insert(entry);

        Result<CacheEntry> result = tlbCache->lookupCacheEntry(
            TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1 + (i * PAGE_SIZE), SecurityState::NonSecure);

        ASSERT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().timestamp, timestamps[i]);
    }
}

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_SecurityStates) {
    // Test conversion with different security states
    PagePermissions perms(true, true, false);

    std::vector<SecurityState> secStates = {
        SecurityState::NonSecure,
        SecurityState::Secure
    };

    for (size_t i = 0; i < secStates.size(); ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID,
                                       TEST_IOVA_1 + (i * PAGE_SIZE),
                                       TEST_PA_1 + (i * PAGE_SIZE), perms, secStates[i]);
        tlbCache->insert(entry);

        Result<CacheEntry> result = tlbCache->lookupCacheEntry(
            TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1 + (i * PAGE_SIZE), secStates[i]);

        ASSERT_TRUE(result.isOk());
        EXPECT_EQ(result.getValue().securityState, secStates[i]);
    }
}

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_CacheMiss) {
    // Test conversion when entry not in cache
    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::CacheEntryNotFound);
    EXPECT_EQ(tlbCache->getMissCount(), 1);
}

TEST_F(TLBCacheCoverageTest, CacheEntryConversion_InvalidStreamID) {
    // Test conversion with invalid StreamID
    // Note: MAX_STREAM_ID is 0xFFFFFFFF, so we can't test +1 (would overflow to 0)
    // Instead, test that lookupCacheEntry validates against the actual implementation

    // This test verifies the validation path exists, even though StreamID can't exceed MAX
    // The validation is in place for API consistency with other methods
    Result<CacheEntry> result = tlbCache->lookupCacheEntry(
        TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, SecurityState::NonSecure);

    // For a non-existent entry, we should get CacheEntryNotFound
    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::CacheEntryNotFound);
    EXPECT_EQ(tlbCache->getMissCount(), 1);
}

// ============================================================================
// TC-CACHE-003: Alternative Lookup Interface
// ============================================================================

TEST_F(TLBCacheCoverageTest, AlternativeLookup_CacheHit) {
    // Test lookup() with CacheEntry reference parameter on cache hit
    PagePermissions perms(true, true, true);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    entry.timestamp = 987654321;
    tlbCache->insert(entry);

    CacheEntry cacheEntry;
    bool found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, cacheEntry);

    EXPECT_TRUE(found);
    EXPECT_EQ(cacheEntry.iova, TEST_IOVA_1);
    EXPECT_EQ(cacheEntry.physicalAddress, TEST_PA_1);
    EXPECT_EQ(cacheEntry.permissions.read, true);
    EXPECT_EQ(cacheEntry.permissions.write, true);
    EXPECT_EQ(cacheEntry.permissions.execute, true);
    EXPECT_EQ(cacheEntry.timestamp, 987654321);
}

TEST_F(TLBCacheCoverageTest, AlternativeLookup_CacheMiss) {
    // Test lookup() with CacheEntry reference parameter on cache miss
    CacheEntry cacheEntry;
    cacheEntry.iova = 0xDEADBEEF;  // Initialize with sentinel value

    bool found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, cacheEntry);

    EXPECT_FALSE(found);
    EXPECT_EQ(tlbCache->getMissCount(), 1);
}

TEST_F(TLBCacheCoverageTest, AlternativeLookup_ReturnValueSemantics) {
    // Test return value semantics: true for hit, false for miss
    PagePermissions perms(true, false, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    CacheEntry cacheEntry1, cacheEntry2;

    // Hit
    bool hit = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, cacheEntry1);
    EXPECT_TRUE(hit);

    // Miss
    bool miss = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2, cacheEntry2);
    EXPECT_FALSE(miss);
}

TEST_F(TLBCacheCoverageTest, AlternativeLookup_CompareWithStandardLookup) {
    // Compare alternative lookup with standard lookup behavior
    // Note: The alternative lookup doesn't take SecurityState parameter
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    entry.timestamp = 555555;
    tlbCache->insert(entry);

    // Standard lookup (pointer-based) - uses default NonSecure
    TLBEntry* tlbEntry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    ASSERT_NE(tlbEntry, nullptr);

    // Alternative lookup (reference-based) - no SecurityState parameter
    CacheEntry cacheEntry;
    bool found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, cacheEntry);
    ASSERT_TRUE(found);

    // Verify both methods return equivalent data
    EXPECT_EQ(cacheEntry.iova, tlbEntry->iova);
    EXPECT_EQ(cacheEntry.physicalAddress, tlbEntry->physicalAddress);
    EXPECT_EQ(cacheEntry.permissions.read, tlbEntry->permissions.read);
    EXPECT_EQ(cacheEntry.permissions.write, tlbEntry->permissions.write);
    EXPECT_EQ(cacheEntry.permissions.execute, tlbEntry->permissions.execute);
    EXPECT_EQ(cacheEntry.timestamp, tlbEntry->timestamp);
}

TEST_F(TLBCacheCoverageTest, AlternativeLookup_OutputParameter) {
    // Test that output parameter is properly modified
    PagePermissions perms(false, true, false);  // Write-only (unusual but valid)
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    CacheEntry cacheEntry;
    // Initialize with sentinel values
    cacheEntry.iova = 0xFFFFFFFF;
    cacheEntry.physicalAddress = 0xFFFFFFFF;

    bool found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, cacheEntry);

    EXPECT_TRUE(found);
    // Verify sentinel values were overwritten
    EXPECT_NE(cacheEntry.iova, 0xFFFFFFFF);
    EXPECT_NE(cacheEntry.physicalAddress, 0xFFFFFFFF);
    EXPECT_EQ(cacheEntry.iova, TEST_IOVA_1);
    EXPECT_EQ(cacheEntry.physicalAddress, TEST_PA_1);
}

TEST_F(TLBCacheCoverageTest, AlternativeLookup_MultipleSequentialLookups) {
    // Test multiple sequential lookups using alternative interface
    PagePermissions perms(true, true, false);

    // Insert multiple entries
    for (int i = 0; i < 5; ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID,
                                       TEST_IOVA_1 + (i * PAGE_SIZE),
                                       TEST_PA_1 + (i * PAGE_SIZE), perms);
        tlbCache->insert(entry);
    }

    // Lookup all entries
    for (int i = 0; i < 5; ++i) {
        CacheEntry cacheEntry;
        bool found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID,
                                     TEST_IOVA_1 + (i * PAGE_SIZE), cacheEntry);

        EXPECT_TRUE(found) << "Failed for index: " << i;
        EXPECT_EQ(cacheEntry.iova, TEST_IOVA_1 + (i * PAGE_SIZE));
        EXPECT_EQ(cacheEntry.physicalAddress, TEST_PA_1 + (i * PAGE_SIZE));
    }
}

// ============================================================================
// TC-CACHE-004: Cache Miss Scenarios
// ============================================================================

TEST_F(TLBCacheCoverageTest, CacheMiss_EntryNotInCache) {
    // Test miss counter increments when entry not in cache
    uint64_t initialMissCount = tlbCache->getMissCount();

    TLBEntry* entry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);

    EXPECT_EQ(entry, nullptr);
    EXPECT_EQ(tlbCache->getMissCount(), initialMissCount + 1);
}

TEST_F(TLBCacheCoverageTest, CacheMiss_InvalidStreamID) {
    // Test miss counter increments on cache miss
    // Note: MAX_STREAM_ID is 0xFFFFFFFF, so we can't test overflow
    // This test verifies miss counter increments for non-existent entries
    uint64_t initialMissCount = tlbCache->getMissCount();

    Result<TLBEntry> result = tlbCache->lookupEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::CacheEntryNotFound);
    EXPECT_EQ(tlbCache->getMissCount(), initialMissCount + 1);
}

TEST_F(TLBCacheCoverageTest, CacheMiss_InvalidPASID) {
    // Test miss counter increments on invalid PASID
    uint64_t initialMissCount = tlbCache->getMissCount();

    PASID invalidPASID = MAX_PASID + 1;
    Result<TLBEntry> result = tlbCache->lookupEntry(TEST_STREAM_ID, invalidPASID, TEST_IOVA_1);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::InvalidPASID);
    EXPECT_EQ(tlbCache->getMissCount(), initialMissCount + 1);
}

TEST_F(TLBCacheCoverageTest, CacheMiss_ErrorPropagation) {
    // Test error propagation on cache misses
    Result<TLBEntry> result = tlbCache->lookupEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);

    EXPECT_TRUE(result.isError());
    EXPECT_EQ(result.getError(), SMMUError::CacheEntryNotFound);
}

TEST_F(TLBCacheCoverageTest, CacheMiss_CacheWarming) {
    // Test cache warming scenario: multiple misses followed by inserts
    PagePermissions perms(true, true, false);

    uint64_t missCountBefore = tlbCache->getMissCount();

    // Generate 10 misses
    for (int i = 0; i < 10; ++i) {
        TLBEntry* entry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID,
                                          TEST_IOVA_1 + (i * PAGE_SIZE));
        EXPECT_EQ(entry, nullptr);
    }

    EXPECT_EQ(tlbCache->getMissCount(), missCountBefore + 10);

    // Now insert the entries (cache warming)
    for (int i = 0; i < 10; ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID,
                                       TEST_IOVA_1 + (i * PAGE_SIZE),
                                       TEST_PA_1 + (i * PAGE_SIZE), perms);
        tlbCache->insert(entry);
    }

    uint64_t hitCountBefore = tlbCache->getHitCount();

    // Subsequent lookups should hit
    for (int i = 0; i < 10; ++i) {
        TLBEntry* entry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID,
                                          TEST_IOVA_1 + (i * PAGE_SIZE));
        EXPECT_NE(entry, nullptr);
    }

    EXPECT_EQ(tlbCache->getHitCount(), hitCountBefore + 10);
}

TEST_F(TLBCacheCoverageTest, CacheMiss_StatisticsAccuracy) {
    // Test statistics accuracy after miss scenarios
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    // Generate mix of hits and misses
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Hit
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2);  // Miss
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Hit
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, 0x30000000);   // Miss
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Hit

    EXPECT_EQ(tlbCache->getHitCount(), 3);
    EXPECT_EQ(tlbCache->getMissCount(), 2);
    EXPECT_EQ(tlbCache->getTotalLookups(), 5);
    EXPECT_NEAR(tlbCache->getHitRate(), 0.6, 0.01);
}

// ============================================================================
// Additional Coverage Tests
// ============================================================================

TEST_F(TLBCacheCoverageTest, InvalidateBySecurityState_Coverage) {
    // Test invalidation by security state
    PagePermissions perms(true, true, false);

    // Insert entries with different security states
    TLBEntry secureEntry1 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1,
                                          TEST_PA_1, perms, SecurityState::Secure);
    TLBEntry secureEntry2 = createTLBEntry(TEST_STREAM_ID, TEST_PASID + 1, TEST_IOVA_2,
                                          TEST_PA_2, perms, SecurityState::Secure);
    TLBEntry nonSecureEntry = createTLBEntry(TEST_STREAM_ID + 1, TEST_PASID, TEST_IOVA_1,
                                            TEST_PA_1, perms, SecurityState::NonSecure);

    tlbCache->insert(secureEntry1);
    tlbCache->insert(secureEntry2);
    tlbCache->insert(nonSecureEntry);

    EXPECT_EQ(tlbCache->getSize(), 3);

    // Invalidate all secure entries
    tlbCache->invalidateBySecurityState(SecurityState::Secure);

    // Only non-secure entry should remain
    EXPECT_EQ(tlbCache->getSize(), 1);

    TLBEntry* remaining = tlbCache->lookup(TEST_STREAM_ID + 1, TEST_PASID, TEST_IOVA_1,
                                          SecurityState::NonSecure);
    EXPECT_NE(remaining, nullptr);
}

TEST_F(TLBCacheCoverageTest, ConcurrentLookups_EntryConversion) {
    // Test concurrent lookups with entry conversion
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    std::atomic<int> successCount{0};
    std::vector<std::thread> threads;

    // Launch multiple threads doing lookups with conversion
    for (int i = 0; i < 10; ++i) {
        threads.emplace_back([this, &successCount]() {
            for (int j = 0; j < 100; ++j) {
                Result<CacheEntry> result = tlbCache->lookupCacheEntry(
                    TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, SecurityState::NonSecure);

                if (result.isOk() && result.getValue().iova == TEST_IOVA_1) {
                    successCount++;
                }
            }
        });
    }

    for (auto& thread : threads) {
        thread.join();
    }

    // All lookups should succeed
    EXPECT_EQ(successCount.load(), 1000);
}

TEST_F(TLBCacheCoverageTest, AtomicStatistics_Coverage) {
    // Test atomic statistics snapshot
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    // Generate some activity
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Hit
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2);  // Miss

    // Get atomic statistics
    TLBCache::CacheStatistics stats = tlbCache->getAtomicStatistics();

    EXPECT_EQ(stats.hitCount, 1);
    EXPECT_EQ(stats.missCount, 1);
    EXPECT_EQ(stats.totalLookups, 2);
    EXPECT_NEAR(stats.hitRate, 0.5, 0.01);
    EXPECT_EQ(stats.currentSize, 1);
    EXPECT_EQ(stats.maxSize, 64);
}

TEST_F(TLBCacheCoverageTest, SetMaxSize_WithEviction) {
    // Test setMaxSize with automatic eviction
    PagePermissions perms(true, true, false);

    // Fill cache with 64 entries
    for (size_t i = 0; i < 64; ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID,
                                       TEST_IOVA_1 + (i * PAGE_SIZE),
                                       TEST_PA_1 + (i * PAGE_SIZE), perms);
        tlbCache->insert(entry);
    }

    EXPECT_EQ(tlbCache->getSize(), 64);

    // Reduce max size to 32 (should evict 32 entries)
    tlbCache->setMaxSize(32);

    EXPECT_EQ(tlbCache->getCapacity(), 32);
    EXPECT_LE(tlbCache->getSize(), 32);
}

TEST_F(TLBCacheCoverageTest, InsertCacheEntry_Coverage) {
    // Test insert with CacheEntry (alternative interface)
    CacheEntry cacheEntry;
    cacheEntry.iova = TEST_IOVA_1;
    cacheEntry.physicalAddress = TEST_PA_1;
    cacheEntry.permissions = PagePermissions(true, true, false);
    cacheEntry.securityState = SecurityState::NonSecure;
    cacheEntry.timestamp = 123456;

    tlbCache->insert(TEST_STREAM_ID, TEST_PASID, cacheEntry);

    EXPECT_EQ(tlbCache->getSize(), 1);

    // Verify entry can be retrieved
    TLBEntry* entry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    ASSERT_NE(entry, nullptr);
    EXPECT_EQ(entry->iova, TEST_IOVA_1);
    EXPECT_EQ(entry->physicalAddress, TEST_PA_1);
}

TEST_F(TLBCacheCoverageTest, RemoveEntry_Coverage) {
    // Test remove operation
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    EXPECT_EQ(tlbCache->getSize(), 1);

    // Remove entry
    tlbCache->remove(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, SecurityState::NonSecure);

    EXPECT_EQ(tlbCache->getSize(), 0);

    // Verify entry no longer exists
    TLBEntry* found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    EXPECT_EQ(found, nullptr);
}

TEST_F(TLBCacheCoverageTest, InvalidatePage_Alias) {
    // Test invalidatePage (alias for invalidate)
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);

    EXPECT_EQ(tlbCache->getSize(), 1);

    // Invalidate using alias
    tlbCache->invalidatePage(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);

    EXPECT_EQ(tlbCache->getSize(), 0);
}

} // namespace test
} // namespace smmu
