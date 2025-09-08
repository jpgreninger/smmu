// ARM SMMU v3 TLB Cache
// Copyright (c) 2024 John Greninger

#ifndef SMMU_TLB_CACHE_H
#define SMMU_TLB_CACHE_H

#include "smmu/types.h"
#include <unordered_map>
#include <list>
#include <utility>
#include <cstddef>
#include <mutex>
#include <atomic>

namespace smmu {

// Cache entry structure
struct CacheEntry {
    IOVA iova;
    PA physicalAddress;
    PagePermissions permissions;
    SecurityState securityState;
    uint64_t timestamp;
    
    CacheEntry() : iova(0), physicalAddress(0), securityState(SecurityState::NonSecure), timestamp(0) {
    }
    
    CacheEntry(IOVA va, PA pa, PagePermissions perms, uint64_t ts) 
        : iova(va), physicalAddress(pa), permissions(perms), securityState(SecurityState::NonSecure), timestamp(ts) {
    }
    
    CacheEntry(IOVA va, PA pa, PagePermissions perms, SecurityState secState, uint64_t ts) 
        : iova(va), physicalAddress(pa), permissions(perms), securityState(secState), timestamp(ts) {
    }
};

// Cache key structure for multi-level indexing
struct CacheKey {
    StreamID streamID;
    PASID pasid;
    IOVA iova;
    SecurityState securityState;
    
    bool operator==(const CacheKey& other) const {
        return streamID == other.streamID && pasid == other.pasid && 
               iova == other.iova && securityState == other.securityState;
    }
};

// Hash function for CacheKey
struct CacheKeyHash {
    std::size_t operator()(const CacheKey& key) const {
        std::size_t h1 = std::hash<uint32_t>{}(key.streamID);
        std::size_t h2 = std::hash<uint32_t>{}(key.pasid);
        std::size_t h3 = std::hash<uint64_t>{}(key.iova);
        std::size_t h4 = std::hash<int>{}(static_cast<int>(key.securityState));
        return h1 ^ (h2 << 1) ^ (h3 << 2) ^ (h4 << 3);
    }
};

class TLBCache {
public:
    explicit TLBCache(size_t maxSize = 1024);
    ~TLBCache();
    
    // Cache operations - Result<T> error handling pattern
    Result<TLBEntry> lookupEntry(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState = SecurityState::NonSecure);
    void insert(const TLBEntry& entry);
    Result<CacheEntry> lookupCacheEntry(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState = SecurityState::NonSecure);
    void insert(StreamID streamID, PASID pasid, const CacheEntry& entry);
    
    // Legacy interfaces for backward compatibility - deprecated
    TLBEntry* lookup(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState = SecurityState::NonSecure);
    bool lookup(StreamID streamID, PASID pasid, IOVA iova, CacheEntry& entry);
    void remove(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState = SecurityState::NonSecure);
    
    // Invalidation operations
    void invalidate(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState = SecurityState::NonSecure);
    void invalidateBySecurityState(SecurityState securityState);
    void invalidateByStream(StreamID streamID);
    void invalidateByPASID(StreamID streamID, PASID pasid);
    void invalidateAll();
    void invalidateStream(StreamID streamID);  // Alias
    void invalidatePASID(StreamID streamID, PASID pasid);  // Alias
    void invalidatePage(StreamID streamID, PASID pasid, IOVA iova);  // Alias
    void clear();  // Clear all entries
    
    // Statistics
    uint64_t getHitCount() const;
    uint64_t getMissCount() const;
    uint64_t getTotalLookups() const;
    double getHitRate() const;
    size_t getSize() const;
    size_t getCapacity() const;
    size_t getMaxSize() const;  // Alias for getCapacity
    void resetStatistics();
    void reset();  // Complete reset
    
    // Thread-safe statistics snapshot
    struct CacheStatistics {
        uint64_t hitCount;
        uint64_t missCount;
        uint64_t totalLookups;
        double hitRate;
        size_t currentSize;
        size_t maxSize;
    };
    CacheStatistics getAtomicStatistics() const;
    
    // Configuration
    void setMaxSize(size_t maxSize);
    
private:
    // Cache storage using LRU policy with TLBEntry
    using TLBCacheMap = std::unordered_map<CacheKey, std::list<std::pair<CacheKey, TLBEntry>>::iterator, CacheKeyHash>;
    using TLBCacheList = std::list<std::pair<CacheKey, TLBEntry>>;
    
    // Legacy cache storage for backward compatibility
    using CacheMap = std::unordered_map<CacheKey, std::list<std::pair<CacheKey, CacheEntry>>::iterator, CacheKeyHash>;
    using CacheList = std::list<std::pair<CacheKey, CacheEntry>>;
    
    TLBCacheMap tlbCacheMap;
    TLBCacheList tlbCacheList;
    size_t maxSize;
    
    // Statistics - atomic for thread safety
    mutable std::atomic<uint64_t> hitCount;
    mutable std::atomic<uint64_t> missCount;
    
    // Thread safety
    mutable std::mutex cacheMutex;
    
    // Helper methods
    void evictLRU();
    void moveToFront(typename TLBCacheList::iterator it);
    uint64_t getCurrentTimestamp() const;
    CacheKey makeKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState = SecurityState::NonSecure) const;
};

} // namespace smmu

#endif // SMMU_TLB_CACHE_H