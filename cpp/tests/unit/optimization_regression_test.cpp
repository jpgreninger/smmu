// ARM SMMU v3 Optimization Regression Tests
// QA.5 Task 1: Ensure optimizations don't break functionality
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/tlb_cache.h"
#include "smmu/address_space.h"
#include "smmu/memory_pool.h"
#include "smmu/types.h"
#include <vector>
#include <random>
#include <set>

using namespace smmu;

class OptimizationRegressionTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Standard test setup
    }
    
    void TearDown() override {
        // Standard test cleanup
    }
};

// Test 1: TLB Cache Hash Function Regression Test
TEST_F(OptimizationRegressionTest, TLBCacheHashFunctionRegression) {
    TLBCache cache(1024);
    
    // Test basic functionality with optimized hash function
    TLBEntry entry1;
    entry1.streamID = 100;
    entry1.pasid = 200;
    entry1.iova = 0x10000;
    entry1.physicalAddress = 0x20000;
    entry1.permissions = PagePermissions(true, true, false);
    entry1.securityState = SecurityState::NonSecure;
    entry1.valid = true;
    entry1.timestamp = 12345;
    
    // Insert entry
    cache.insert(entry1);
    
    // Lookup should succeed
    auto result = cache.lookupEntry(entry1.streamID, entry1.pasid, entry1.iova, entry1.securityState);
    ASSERT_FALSE(result.isError());
    
    const TLBEntry& retrieved = result.getValue();
    EXPECT_EQ(retrieved.streamID, entry1.streamID);
    EXPECT_EQ(retrieved.pasid, entry1.pasid);
    EXPECT_EQ(retrieved.iova, entry1.iova);
    EXPECT_EQ(retrieved.physicalAddress, entry1.physicalAddress);
    EXPECT_EQ(retrieved.securityState, entry1.securityState);
    
    // Test with different security states (hash function should handle properly)
    entry1.securityState = SecurityState::Secure;
    cache.insert(entry1);
    
    auto secureResult = cache.lookupEntry(entry1.streamID, entry1.pasid, entry1.iova, SecurityState::Secure);
    ASSERT_FALSE(secureResult.isError());
    EXPECT_EQ(secureResult.getValue().securityState, SecurityState::Secure);
    
    // Original non-secure entry should still be there
    auto nonSecureResult = cache.lookupEntry(entry1.streamID, entry1.pasid, entry1.iova, SecurityState::NonSecure);
    ASSERT_FALSE(nonSecureResult.isError());
    EXPECT_EQ(nonSecureResult.getValue().securityState, SecurityState::NonSecure);
}

// Test 2: TLB Cache Secondary Index Invalidation Regression Test
TEST_F(OptimizationRegressionTest, TLBCacheInvalidationRegression) {
    TLBCache cache(1024);
    
    // Populate cache with multiple streams and PASIDs
    std::vector<TLBEntry> entries;
    for (int s = 0; s < 10; ++s) {
        for (int p = 0; p < 5; ++p) {
            TLBEntry entry;
            entry.streamID = s;
            entry.pasid = p;
            entry.iova = 0x10000 + (s * 0x1000) + (p * 0x100);
            entry.physicalAddress = 0x20000 + entry.iova;
            entry.permissions = PagePermissions(true, true, false);
            entry.securityState = SecurityState::NonSecure;
            entry.valid = true;
            cache.insert(entry);
            entries.push_back(entry);
        }
    }
    
    // Verify all entries are present
    for (const auto& entry : entries) {
        auto result = cache.lookupEntry(entry.streamID, entry.pasid, entry.iova, entry.securityState);
        ASSERT_FALSE(result.isError()) << "Failed to find entry: stream=" << entry.streamID 
                                       << " pasid=" << entry.pasid << " iova=0x" << std::hex << entry.iova;
    }
    
    // Test stream invalidation
    cache.invalidateStream(5);
    
    // Entries for stream 5 should be gone
    for (const auto& entry : entries) {
        auto result = cache.lookupEntry(entry.streamID, entry.pasid, entry.iova, entry.securityState);
        if (entry.streamID == 5) {
            EXPECT_TRUE(result.isError()) << "Entry should be invalidated: stream=" << entry.streamID;
        } else {
            EXPECT_FALSE(result.isError()) << "Entry should still exist: stream=" << entry.streamID;
        }
    }
    
    // Test PASID invalidation
    cache.invalidatePASID(3, 2);
    
    // Check that only stream 3, PASID 2 entries are gone
    for (const auto& entry : entries) {
        auto result = cache.lookupEntry(entry.streamID, entry.pasid, entry.iova, entry.securityState);
        if (entry.streamID == 5 || (entry.streamID == 3 && entry.pasid == 2)) {
            EXPECT_TRUE(result.isError()) << "Entry should be invalidated: stream=" << entry.streamID 
                                         << " pasid=" << entry.pasid;
        } else {
            EXPECT_FALSE(result.isError()) << "Entry should still exist: stream=" << entry.streamID 
                                          << " pasid=" << entry.pasid;
        }
    }
}

// Test 3: AddressSpace Bulk Operations Regression Test  
TEST_F(OptimizationRegressionTest, AddressSpaceBulkOperationsRegression) {
    AddressSpace addressSpace;
    PagePermissions perms(true, true, false);
    
    // Test bulk mapping with prefetching optimization
    std::vector<std::pair<IOVA, PA>> mappings;
    const int numPages = 100;
    
    for (int i = 0; i < numPages; ++i) {
        IOVA iova = 0x100000 + (i * PAGE_SIZE);
        PA pa = 0x200000 + (i * PAGE_SIZE);
        mappings.push_back(std::make_pair(iova, pa));
    }
    
    // Bulk map should succeed
    auto mapResult = addressSpace.mapPages(mappings, perms);
    ASSERT_FALSE(mapResult.isError()) << "Bulk mapping failed";
    
    // Verify all mappings are correct
    for (const auto& mapping : mappings) {
        auto transResult = addressSpace.translatePage(mapping.first, AccessType::Read);
        ASSERT_FALSE(transResult.isError()) << "Translation failed for IOVA 0x" << std::hex << mapping.first;
        
        const TranslationData& result = transResult.getValue();
        EXPECT_EQ(result.physicalAddress, mapping.second) << "PA mismatch for IOVA 0x" << std::hex << mapping.first;
        EXPECT_TRUE(result.permissions.read) << "Read permission missing";
        EXPECT_TRUE(result.permissions.write) << "Write permission missing";
    }
    
    // Test sequential access pattern (should benefit from prefetching)
    for (int i = 0; i < numPages; ++i) {
        IOVA iova = 0x100000 + (i * PAGE_SIZE);
        auto transResult = addressSpace.translatePage(iova, AccessType::Read);
        ASSERT_FALSE(transResult.isError()) << "Sequential access failed at index " << i;
    }
    
    // Test bulk unmapping
    std::vector<IOVA> iovasToUnmap;
    for (const auto& mapping : mappings) {
        iovasToUnmap.push_back(mapping.first);
    }
    
    auto unmapResult = addressSpace.unmapPages(iovasToUnmap);
    ASSERT_FALSE(unmapResult.isError()) << "Bulk unmapping failed";
    
    // Verify all mappings are gone
    for (IOVA iova : iovasToUnmap) {
        auto transResult = addressSpace.translatePage(iova, AccessType::Read);
        EXPECT_TRUE(transResult.isError()) << "Translation should fail for unmapped IOVA 0x" << std::hex << iova;
    }
}

// Test 4: Memory Pool Basic Functionality Test
TEST_F(OptimizationRegressionTest, MemoryPoolFunctionality) {
    MemoryPool<PageEntry> pool(10, 5);
    
    // Test basic acquire/release cycle
    PageEntry* entry1 = pool.acquire(0x1000, PagePermissions(true, false, false), SecurityState::NonSecure);
    ASSERT_NE(entry1, nullptr);
    EXPECT_EQ(entry1->physicalAddress, 0x1000);
    EXPECT_TRUE(entry1->permissions.read);
    EXPECT_FALSE(entry1->permissions.write);
    
    PageEntry* entry2 = pool.acquire(0x2000, PagePermissions(false, true, false), SecurityState::Secure);
    ASSERT_NE(entry2, nullptr);
    EXPECT_NE(entry1, entry2);  // Different objects
    
    // Release and reacquire
    pool.release(entry1);
    PageEntry* entry3 = pool.acquire(0x3000, PagePermissions(true, true, true), SecurityState::Realm);
    
    // entry3 should reuse the memory from entry1
    EXPECT_EQ(entry1, entry3);  // Same memory location
    EXPECT_EQ(entry3->physicalAddress, 0x3000);  // But new values
    EXPECT_TRUE(entry3->permissions.execute);
    EXPECT_EQ(entry3->securityState, SecurityState::Realm);
    
    pool.release(entry2);
    pool.release(entry3);
    
    // Pool statistics should be consistent
    EXPECT_GE(pool.getTotalCapacity(), 10);  // At least initial size
    EXPECT_EQ(pool.getUsedCount(), 0);       // Nothing in use
}

// Test 5: Hash Function Collision and Distribution Test
TEST_F(OptimizationRegressionTest, HashFunctionCollisionTest) {
    TLBCache cache(4096);  // Large cache to test hash distribution
    
    std::set<size_t> hashValues;
    std::vector<TLBEntry> entries;
    
    // Generate entries that could cause collisions with simple hash functions
    for (int s = 0; s < 100; ++s) {
        for (int p = 0; p < 10; ++p) {
            TLBEntry entry;
            entry.streamID = s;
            entry.pasid = p;
            // Use page-aligned addresses to test hash function's handling of zero lower bits
            entry.iova = 0x10000 + (s * PAGE_SIZE) + (p * PAGE_SIZE * 100);
            entry.physicalAddress = 0x40000000 + entry.iova;
            entry.permissions = PagePermissions(true, s % 2 == 0, false);
            entry.securityState = static_cast<SecurityState>(s % 3);
            entry.valid = true;
            
            cache.insert(entry);
            entries.push_back(entry);
            
            // Test that we can find this specific entry
            auto result = cache.lookupEntry(entry.streamID, entry.pasid, entry.iova, entry.securityState);
            ASSERT_FALSE(result.isError()) << "Failed to insert/lookup entry: stream=" << s << " pasid=" << p;
        }
    }
    
    // Verify all entries can still be found (tests hash collision handling)
    for (const auto& entry : entries) {
        auto result = cache.lookupEntry(entry.streamID, entry.pasid, entry.iova, entry.securityState);
        ASSERT_FALSE(result.isError()) << "Hash collision or corruption detected for entry: stream=" 
                                       << entry.streamID << " pasid=" << entry.pasid;
        
        const TLBEntry& retrieved = result.getValue();
        EXPECT_EQ(retrieved.streamID, entry.streamID);
        EXPECT_EQ(retrieved.pasid, entry.pasid);
        EXPECT_EQ(retrieved.iova, entry.iova);
        EXPECT_EQ(retrieved.physicalAddress, entry.physicalAddress);
    }
    
    // Cache should have reasonable hit rate (indicating good hash distribution)
    auto stats = cache.getAtomicStatistics();
    double hitRate = stats.hitRate;
    EXPECT_GT(hitRate, 0.8) << "Hash function may have poor distribution, hit rate: " << hitRate;
}

// Test 6: Integration Test - All Optimizations Together
TEST_F(OptimizationRegressionTest, IntegrationTest) {
    TLBCache cache(1024);
    AddressSpace addressSpace;
    MemoryPool<PageEntry> pool;
    
    // Setup address space with bulk operations
    std::vector<std::pair<IOVA, PA>> mappings;
    const int numPages = 500;
    
    for (int i = 0; i < numPages; ++i) {
        IOVA iova = 0x400000 + (i * PAGE_SIZE);
        PA pa = 0x800000 + (i * PAGE_SIZE);
        mappings.push_back(std::make_pair(iova, pa));
    }
    
    PagePermissions perms(true, true, false);
    auto mapResult = addressSpace.mapPages(mappings, perms);
    ASSERT_FALSE(mapResult.isError());
    
    // Populate TLB cache with translations
    for (int i = 0; i < numPages; i += 10) {  // Sample every 10th entry
        const auto& mapping = mappings[i];
        
        auto transResult = addressSpace.translatePage(mapping.first, AccessType::Read);
        ASSERT_FALSE(transResult.isError());
        
        const TranslationData& transData = transResult.getValue();
        TLBEntry entry;
        entry.streamID = i % 100;
        entry.pasid = i % 50;
        entry.iova = mapping.first;
        entry.physicalAddress = transData.physicalAddress;
        entry.permissions = transData.permissions;
        entry.securityState = transData.securityState;
        entry.valid = true;
        entry.timestamp = i;
        
        cache.insert(entry);
    }
    
    // Test mixed operations with all optimizations
    for (int i = 0; i < 50; ++i) {
        // TLB lookup (optimized hash function)
        int idx = i * 10;
        auto cacheResult = cache.lookupEntry(idx % 100, idx % 50, mappings[idx].first, SecurityState::NonSecure);
        EXPECT_FALSE(cacheResult.isError()) << "TLB lookup failed for index " << idx;
        
        // Address space translation (prefetching hints)
        auto transResult = addressSpace.translatePage(mappings[i].first, AccessType::Read);
        EXPECT_FALSE(transResult.isError()) << "Address space translation failed for index " << i;
        
        // Memory pool usage
        PageEntry* poolEntry = pool.acquire(mappings[i].second, perms, SecurityState::NonSecure);
        ASSERT_NE(poolEntry, nullptr);
        EXPECT_EQ(poolEntry->physicalAddress, mappings[i].second);
        pool.release(poolEntry);
    }
    
    // Test invalidation with secondary indices
    cache.invalidateStream(25);
    cache.invalidatePASID(50, 25);
    
    // Cleanup with bulk operations
    std::vector<IOVA> iovasToUnmap;
    for (const auto& mapping : mappings) {
        iovasToUnmap.push_back(mapping.first);
    }
    
    auto unmapResult = addressSpace.unmapPages(iovasToUnmap);
    EXPECT_FALSE(unmapResult.isError());
}

int main(int argc, char **argv) {
    ::testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}