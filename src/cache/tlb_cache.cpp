// ARM SMMU v3 TLB Cache Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/tlb_cache.h"
#include <chrono>
#include <algorithm>

namespace smmu {

// Constructor
TLBCache::TLBCache(size_t maxSize) 
    : maxSize(maxSize), hitCount(0), missCount(0) {
}

// Destructor
TLBCache::~TLBCache() {
    clear();
}

// Cache operations - Result<T> error handling pattern
Result<TLBEntry> TLBCache::lookupEntry(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    
    // Validate input parameters
    if (streamID > MAX_STREAM_ID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<TLBEntry>(SMMUError::InvalidStreamID);
    }
    
    if (pasid > MAX_PASID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<TLBEntry>(SMMUError::InvalidPASID);
    }
    
    CacheKey key = makeKey(streamID, pasid, iova, securityState);
    auto it = tlbCacheMap.find(key);
    
    if (it == tlbCacheMap.end()) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<TLBEntry>(SMMUError::CacheEntryNotFound);
    }
    
    // Move to front (LRU)
    moveToFront(it->second);
    hitCount.fetch_add(1, std::memory_order_relaxed);
    
    // Create a copy of the TLBEntry to avoid reference issues
    TLBEntry entryCopy = it->second->second;
    return Result<TLBEntry>(entryCopy);
}

Result<CacheEntry> TLBCache::lookupCacheEntry(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    // Validate input parameters without updating statistics yet
    if (streamID > MAX_STREAM_ID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<CacheEntry>(SMMUError::InvalidStreamID);
    }
    
    if (pasid > MAX_PASID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<CacheEntry>(SMMUError::InvalidPASID);
    }
    
    // Use lookupEntry which handles its own statistics - avoid double counting
    Result<TLBEntry> tlbResult = lookupEntry(streamID, pasid, iova, securityState);
    if (tlbResult.isError()) {
        // Don't count miss again - lookupEntry already did
        return makeError<CacheEntry>(tlbResult.getError());
    }
    
    const TLBEntry& tlbEntry = tlbResult.getValue();
    CacheEntry entry;
    entry.iova = tlbEntry.iova;
    entry.physicalAddress = tlbEntry.physicalAddress;
    entry.permissions = tlbEntry.permissions;
    entry.securityState = tlbEntry.securityState;
    entry.timestamp = tlbEntry.timestamp;
    
    // Return a copy of the CacheEntry
    return Result<CacheEntry>(std::move(entry));
}

// Legacy interfaces for backward compatibility - deprecated
TLBEntry* TLBCache::lookup(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    CacheKey key = makeKey(streamID, pasid, iova, securityState);
    auto it = tlbCacheMap.find(key);
    
    if (it == tlbCacheMap.end()) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return nullptr;
    }
    
    // Move to front (LRU)
    moveToFront(it->second);
    hitCount.fetch_add(1, std::memory_order_relaxed);
    
    return &(it->second->second);
}

void TLBCache::insert(const TLBEntry& entry) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    CacheKey key = makeKey(entry.streamID, entry.pasid, entry.iova, entry.securityState);
    
    // Check if entry already exists
    auto it = tlbCacheMap.find(key);
    if (it != tlbCacheMap.end()) {
        // Update existing entry
        it->second->second = entry;
        moveToFront(it->second);
        return;
    }
    
    // Check if cache is full
    if (tlbCacheList.size() >= maxSize) {
        evictLRU();
    }
    
    // Insert new entry
    tlbCacheList.push_front(std::make_pair(key, entry));
    tlbCacheMap[key] = tlbCacheList.begin();
}

bool TLBCache::lookup(StreamID streamID, PASID pasid, IOVA iova, CacheEntry& entry) {
    TLBEntry* tlbEntry = lookup(streamID, pasid, iova);
    if (tlbEntry) {
        entry.iova = tlbEntry->iova;
        entry.physicalAddress = tlbEntry->physicalAddress;
        entry.permissions = tlbEntry->permissions;
        entry.securityState = tlbEntry->securityState;
        entry.timestamp = tlbEntry->timestamp;
        return true;
    }
    return false;
}

void TLBCache::insert(StreamID streamID, PASID pasid, const CacheEntry& entry) {
    TLBEntry tlbEntry;
    tlbEntry.streamID = streamID;
    tlbEntry.pasid = pasid;
    tlbEntry.iova = entry.iova;
    tlbEntry.physicalAddress = entry.physicalAddress;
    tlbEntry.permissions = entry.permissions;
    tlbEntry.valid = true;
    tlbEntry.timestamp = entry.timestamp;
    
    insert(tlbEntry);
}

void TLBCache::remove(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    CacheKey key = makeKey(streamID, pasid, iova, securityState);
    auto it = tlbCacheMap.find(key);
    
    if (it != tlbCacheMap.end()) {
        tlbCacheList.erase(it->second);
        tlbCacheMap.erase(it);
    }
}

// Invalidation operations
void TLBCache::invalidate(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    CacheKey key = makeKey(streamID, pasid, iova, securityState);
    auto it = tlbCacheMap.find(key);
    
    if (it != tlbCacheMap.end()) {
        tlbCacheList.erase(it->second);
        tlbCacheMap.erase(it);
    }
}

void TLBCache::invalidateByStream(StreamID streamID) {
    invalidateStream(streamID);
}

void TLBCache::invalidateByPASID(StreamID streamID, PASID pasid) {
    invalidatePASID(streamID, pasid);
}

void TLBCache::invalidateAll() {
    clear();
}

void TLBCache::invalidateBySecurityState(SecurityState securityState) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    auto it = tlbCacheList.begin();
    while (it != tlbCacheList.end()) {
        if (it->first.securityState == securityState) {
            tlbCacheMap.erase(it->first);
            it = tlbCacheList.erase(it);
        } else {
            ++it;
        }
    }
}

void TLBCache::invalidateStream(StreamID streamID) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    auto it = tlbCacheList.begin();
    while (it != tlbCacheList.end()) {
        if (it->first.streamID == streamID) {
            tlbCacheMap.erase(it->first);
            it = tlbCacheList.erase(it);
        } else {
            ++it;
        }
    }
}

void TLBCache::invalidatePASID(StreamID streamID, PASID pasid) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    auto it = tlbCacheList.begin();
    while (it != tlbCacheList.end()) {
        if (it->first.streamID == streamID && it->first.pasid == pasid) {
            tlbCacheMap.erase(it->first);
            it = tlbCacheList.erase(it);
        } else {
            ++it;
        }
    }
}

void TLBCache::invalidatePage(StreamID streamID, PASID pasid, IOVA iova) {
    invalidate(streamID, pasid, iova);
}

void TLBCache::clear() {
    std::lock_guard<std::mutex> lock(cacheMutex);
    tlbCacheMap.clear();
    tlbCacheList.clear();
}

// Statistics
uint64_t TLBCache::getHitCount() const {
    return hitCount.load(std::memory_order_relaxed);
}

uint64_t TLBCache::getMissCount() const {
    return missCount.load(std::memory_order_relaxed);
}

uint64_t TLBCache::getTotalLookups() const {
    return hitCount.load(std::memory_order_relaxed) + missCount.load(std::memory_order_relaxed);
}

double TLBCache::getHitRate() const {
    uint64_t hits = hitCount.load(std::memory_order_relaxed);
    uint64_t misses = missCount.load(std::memory_order_relaxed);
    uint64_t total = hits + misses;
    return total > 0 ? static_cast<double>(hits) / static_cast<double>(total) : 0.0;
}

size_t TLBCache::getSize() const {
    std::lock_guard<std::mutex> lock(cacheMutex);
    return tlbCacheList.size();
}

size_t TLBCache::getCapacity() const {
    std::lock_guard<std::mutex> lock(cacheMutex);
    return maxSize;
}

size_t TLBCache::getMaxSize() const {
    return getCapacity();
}

void TLBCache::resetStatistics() {
    hitCount.store(0, std::memory_order_relaxed);
    missCount.store(0, std::memory_order_relaxed);
}

void TLBCache::reset() {
    std::lock_guard<std::mutex> lock(cacheMutex);
    tlbCacheMap.clear();
    tlbCacheList.clear();
    hitCount.store(0, std::memory_order_relaxed);
    missCount.store(0, std::memory_order_relaxed);
}

// Configuration
void TLBCache::setMaxSize(size_t newMaxSize) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    maxSize = newMaxSize;
    
    // Evict entries if current size exceeds new limit
    while (tlbCacheList.size() > maxSize) {
        evictLRU();
    }
}

// Helper methods (Note: These are called from already-locked contexts)
void TLBCache::evictLRU() {
    if (!tlbCacheList.empty()) {
        auto last = tlbCacheList.end();
        --last;
        tlbCacheMap.erase(last->first);
        tlbCacheList.erase(last);
    }
}

void TLBCache::moveToFront(typename TLBCacheList::iterator it) {
    if (it != tlbCacheList.begin()) {
        auto entry = *it;
        tlbCacheList.erase(it);
        tlbCacheList.push_front(entry);
        tlbCacheMap[entry.first] = tlbCacheList.begin();
    }
}

uint64_t TLBCache::getCurrentTimestamp() const {
    return std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
}

CacheKey TLBCache::makeKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) const {
    CacheKey key;
    key.streamID = streamID;
    key.pasid = pasid;
    key.iova = iova;
    key.securityState = securityState;
    return key;
}

// Thread-safe atomic statistics snapshot
TLBCache::CacheStatistics TLBCache::getAtomicStatistics() const {
    CacheStatistics stats;
    
    // Use a loop to ensure consistent snapshot of hit/miss counters
    // This addresses the race condition where counters can be modified between individual reads
    uint64_t currentHits, currentMisses;
    do {
        currentHits = hitCount.load(std::memory_order_relaxed);
        currentMisses = missCount.load(std::memory_order_relaxed);
        // Verify that the values haven't changed during our reads
        // If they have, retry until we get a consistent snapshot
    } while (currentHits != hitCount.load(std::memory_order_relaxed) || 
             currentMisses != missCount.load(std::memory_order_relaxed));
    
    stats.hitCount = currentHits;
    stats.missCount = currentMisses;
    stats.totalLookups = stats.hitCount + stats.missCount;
    stats.hitRate = stats.totalLookups > 0 ? 
        static_cast<double>(stats.hitCount) / static_cast<double>(stats.totalLookups) : 0.0;
    
    // For size and maxSize, we need the mutex briefly
    {
        std::lock_guard<std::mutex> lock(cacheMutex);
        stats.currentSize = tlbCacheList.size();
        stats.maxSize = maxSize;
    }
    
    return stats;
}

}