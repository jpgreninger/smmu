// ARM SMMU v3 TLB Cache Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/tlb_cache.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

class TLBCacheTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Create TLB cache with reasonable size for testing
        tlbCache = std::unique_ptr<TLBCache>(new TLBCache(64));  // 64 entries
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
    TLBEntry createTLBEntry(StreamID streamID, PASID pasid, IOVA iova, PA pa, PagePermissions perms) {
        TLBEntry entry;
        entry.streamID = streamID;
        entry.pasid = pasid;
        entry.iova = iova;
        entry.physicalAddress = pa;
        entry.permissions = perms;
        entry.valid = true;
        entry.timestamp = 0;
        return entry;
    }
};

// Test default construction
TEST_F(TLBCacheTest, DefaultConstruction) {
    ASSERT_NE(tlbCache, nullptr);
    
    // Initially empty
    EXPECT_EQ(tlbCache->getSize(), 0);
    EXPECT_EQ(tlbCache->getCapacity(), 64);
    EXPECT_EQ(tlbCache->getHitRate(), 0.0);
}

// Test single entry insertion and lookup
TEST_F(TLBCacheTest, SingleEntryInsertionAndLookup) {
    PagePermissions perms(true, true, false);  // Read-write
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    
    // Insert entry
    tlbCache->insert(entry);
    EXPECT_EQ(tlbCache->getSize(), 1);
    
    // Lookup entry
    TLBEntry* foundEntry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    ASSERT_NE(foundEntry, nullptr);
    
    EXPECT_EQ(foundEntry->streamID, TEST_STREAM_ID);
    EXPECT_EQ(foundEntry->pasid, TEST_PASID);
    EXPECT_EQ(foundEntry->iova, TEST_IOVA_1);
    EXPECT_EQ(foundEntry->physicalAddress, TEST_PA_1);
    EXPECT_TRUE(foundEntry->valid);
    EXPECT_TRUE(foundEntry->permissions.read);
    EXPECT_TRUE(foundEntry->permissions.write);
    EXPECT_FALSE(foundEntry->permissions.execute);
}

// Test cache miss
TEST_F(TLBCacheTest, CacheMiss) {
    // Lookup non-existent entry
    TLBEntry* foundEntry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    EXPECT_EQ(foundEntry, nullptr);
    
    // Hit rate should be 0 (no hits, 1 miss)
    double hitRate = tlbCache->getHitRate();
    EXPECT_EQ(hitRate, 0.0);
}

// Test multiple entries
TEST_F(TLBCacheTest, MultipleEntries) {
    PagePermissions perms1(true, false, false);  // Read-only
    PagePermissions perms2(true, true, true);    // Read-write-execute
    
    TLBEntry entry1 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms1);
    TLBEntry entry2 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2, TEST_PA_2, perms2);
    
    tlbCache->insert(entry1);
    tlbCache->insert(entry2);
    
    EXPECT_EQ(tlbCache->getSize(), 2);
    
    // Lookup both entries
    TLBEntry* found1 = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    TLBEntry* found2 = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2);
    
    ASSERT_NE(found1, nullptr);
    ASSERT_NE(found2, nullptr);
    
    EXPECT_EQ(found1->physicalAddress, TEST_PA_1);
    EXPECT_EQ(found2->physicalAddress, TEST_PA_2);
    
    // Verify different permissions
    EXPECT_FALSE(found1->permissions.write);
    EXPECT_TRUE(found2->permissions.write);
}

// Test cache invalidation by entry
TEST_F(TLBCacheTest, InvalidateEntry) {
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    
    // Insert and verify
    tlbCache->insert(entry);
    TLBEntry* found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    ASSERT_NE(found, nullptr);
    
    // Invalidate entry
    tlbCache->invalidate(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    
    // Verify entry is no longer found
    found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    EXPECT_EQ(found, nullptr);
    EXPECT_EQ(tlbCache->getSize(), 0);
}

// Test cache invalidation by stream
TEST_F(TLBCacheTest, InvalidateByStream) {
    PagePermissions perms(true, true, false);
    
    // Add entries for different streams
    TLBEntry entry1 = createTLBEntry(0x1000, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    TLBEntry entry2 = createTLBEntry(0x2000, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    TLBEntry entry3 = createTLBEntry(0x1000, TEST_PASID, TEST_IOVA_2, TEST_PA_2, perms);
    
    tlbCache->insert(entry1);
    tlbCache->insert(entry2);
    tlbCache->insert(entry3);
    
    EXPECT_EQ(tlbCache->getSize(), 3);
    
    // Invalidate all entries for stream 0x1000
    tlbCache->invalidateByStream(0x1000);
    
    // Only stream 0x2000 entry should remain
    EXPECT_EQ(tlbCache->getSize(), 1);
    
    TLBEntry* found = tlbCache->lookup(0x2000, TEST_PASID, TEST_IOVA_1);
    EXPECT_NE(found, nullptr);
    
    found = tlbCache->lookup(0x1000, TEST_PASID, TEST_IOVA_1);
    EXPECT_EQ(found, nullptr);
}

// Test cache invalidation by PASID
TEST_F(TLBCacheTest, InvalidateByPASID) {
    PagePermissions perms(true, true, false);
    
    // Add entries for different PASIDs
    TLBEntry entry1 = createTLBEntry(TEST_STREAM_ID, 0x1, TEST_IOVA_1, TEST_PA_1, perms);
    TLBEntry entry2 = createTLBEntry(TEST_STREAM_ID, 0x2, TEST_IOVA_1, TEST_PA_1, perms);
    TLBEntry entry3 = createTLBEntry(TEST_STREAM_ID, 0x1, TEST_IOVA_2, TEST_PA_2, perms);
    
    tlbCache->insert(entry1);
    tlbCache->insert(entry2);
    tlbCache->insert(entry3);
    
    EXPECT_EQ(tlbCache->getSize(), 3);
    
    // Invalidate all entries for PASID 0x1 on this stream
    tlbCache->invalidateByPASID(TEST_STREAM_ID, 0x1);
    
    // Only PASID 0x2 entry should remain
    EXPECT_EQ(tlbCache->getSize(), 1);
    
    TLBEntry* found = tlbCache->lookup(TEST_STREAM_ID, 0x2, TEST_IOVA_1);
    EXPECT_NE(found, nullptr);
    
    found = tlbCache->lookup(TEST_STREAM_ID, 0x1, TEST_IOVA_1);
    EXPECT_EQ(found, nullptr);
}

// Test cache clear
TEST_F(TLBCacheTest, CacheClear) {
    PagePermissions perms(true, true, false);
    
    // Add multiple entries
    for (int i = 0; i < 10; ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, 
                                       TEST_IOVA_1 + i * PAGE_SIZE, 
                                       TEST_PA_1 + i * PAGE_SIZE, perms);
        tlbCache->insert(entry);
    }
    
    EXPECT_EQ(tlbCache->getSize(), 10);
    
    // Clear cache
    tlbCache->clear();
    
    EXPECT_EQ(tlbCache->getSize(), 0);
    
    // Verify no entries can be found
    for (int i = 0; i < 10; ++i) {
        TLBEntry* found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1 + i * PAGE_SIZE);
        EXPECT_EQ(found, nullptr);
    }
}

// Test cache capacity and eviction
TEST_F(TLBCacheTest, CacheEviction) {
    PagePermissions perms(true, true, false);
    size_t capacity = tlbCache->getCapacity();
    
    // Fill cache beyond capacity
    for (size_t i = 0; i < capacity + 10; ++i) {
        TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, 
                                       TEST_IOVA_1 + i * PAGE_SIZE, 
                                       TEST_PA_1 + i * PAGE_SIZE, perms);
        tlbCache->insert(entry);
    }
    
    // Cache should not exceed capacity
    EXPECT_LE(tlbCache->getSize(), capacity);
    
    // Most recent entries should be present
    TLBEntry* recentEntry = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, 
                                            TEST_IOVA_1 + (capacity + 5) * PAGE_SIZE);
    EXPECT_NE(recentEntry, nullptr);
}

// Test hit rate calculation
TEST_F(TLBCacheTest, HitRateCalculation) {
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    
    tlbCache->insert(entry);
    
    // 3 hits
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
    
    // 2 misses
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2);
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, 0x30000000);
    
    // Hit rate should be 3/5 = 60%
    double hitRate = tlbCache->getHitRate();
    EXPECT_NEAR(hitRate, 0.6, 0.01);
}

// Test cache statistics
TEST_F(TLBCacheTest, CacheStatistics) {
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    
    tlbCache->insert(entry);
    
    // Generate some hits and misses
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Hit
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2);  // Miss
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Hit
    
    EXPECT_EQ(tlbCache->getHitCount(), 2);
    EXPECT_EQ(tlbCache->getMissCount(), 1);
    EXPECT_EQ(tlbCache->getTotalLookups(), 3);
}

// Test LRU (Least Recently Used) eviction policy
TEST_F(TLBCacheTest, LRUEvictionPolicy) {
    // Create small cache for testing LRU
    auto smallCache = std::unique_ptr<TLBCache>(new TLBCache(3));  // Only 3 entries
    
    PagePermissions perms(true, true, false);
    
    // Fill cache to capacity
    TLBEntry entry1 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, 0x10000000, 0x40000000, perms);
    TLBEntry entry2 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, 0x20000000, 0x50000000, perms);
    TLBEntry entry3 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, 0x30000000, 0x60000000, perms);
    
    smallCache->insert(entry1);
    smallCache->insert(entry2);
    smallCache->insert(entry3);
    
    EXPECT_EQ(smallCache->getSize(), 3);
    
    // Access entry1 to make it recently used
    smallCache->lookup(TEST_STREAM_ID, TEST_PASID, 0x10000000);
    
    // Insert new entry (should evict least recently used, which is entry2)
    TLBEntry entry4 = createTLBEntry(TEST_STREAM_ID, TEST_PASID, 0x40000000, 0x70000000, perms);
    smallCache->insert(entry4);
    
    EXPECT_EQ(smallCache->getSize(), 3);
    
    // entry1 should still be present (recently accessed)
    TLBEntry* found1 = smallCache->lookup(TEST_STREAM_ID, TEST_PASID, 0x10000000);
    EXPECT_NE(found1, nullptr);
    
    // entry4 should be present (newly inserted)
    TLBEntry* found4 = smallCache->lookup(TEST_STREAM_ID, TEST_PASID, 0x40000000);
    EXPECT_NE(found4, nullptr);
    
    // entry2 should have been evicted
    TLBEntry* found2 = smallCache->lookup(TEST_STREAM_ID, TEST_PASID, 0x20000000);
    EXPECT_EQ(found2, nullptr);
}

// Test concurrent access simulation
TEST_F(TLBCacheTest, ConcurrentAccessSimulation) {
    PagePermissions perms(true, true, false);
    
    // Simulate multiple simultaneous accesses to same entry
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    tlbCache->insert(entry);
    
    // Multiple lookups (simulating concurrent access)
    for (int i = 0; i < 100; ++i) {
        TLBEntry* found = tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);
        EXPECT_NE(found, nullptr);
        EXPECT_EQ(found->physicalAddress, TEST_PA_1);
    }
    
    EXPECT_EQ(tlbCache->getHitCount(), 100);
    EXPECT_EQ(tlbCache->getHitRate(), 1.0);
}

// Test cache reset functionality
TEST_F(TLBCacheTest, CacheReset) {
    PagePermissions perms(true, true, false);
    TLBEntry entry = createTLBEntry(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1, TEST_PA_1, perms);
    
    tlbCache->insert(entry);
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_1);  // Generate hit
    tlbCache->lookup(TEST_STREAM_ID, TEST_PASID, TEST_IOVA_2);  // Generate miss
    
    EXPECT_GT(tlbCache->getSize(), 0);
    EXPECT_GT(tlbCache->getTotalLookups(), 0);
    
    // Reset cache
    tlbCache->reset();
    
    EXPECT_EQ(tlbCache->getSize(), 0);
    EXPECT_EQ(tlbCache->getHitCount(), 0);
    EXPECT_EQ(tlbCache->getMissCount(), 0);
    EXPECT_EQ(tlbCache->getTotalLookups(), 0);
    EXPECT_EQ(tlbCache->getHitRate(), 0.0);
}

} // namespace test
} // namespace smmu