// ARM SMMU v3 TLB Cache Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/tlb_cache.h"
#include <chrono>
#include <algorithm>
#include <cstdio>
#include <vector>

namespace smmu {

// Constructor
TLBCache::TLBCache(size_t maxSize) 
    : maxSize(maxSize > 0 ? maxSize : 1024), hitCount(0), missCount(0) {
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
    auto listIt = tlbCacheList.begin();
    tlbCacheMap[key] = listIt;
    
    // Add to secondary indices for fast invalidation
    addToSecondaryIndices(key, listIt);
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
        // Remove from secondary indices first
        removeFromSecondaryIndices(key, it->second);
        
        // Remove from primary structures
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
    
    // Use secondary index for O(k) performance instead of O(n)
    auto range = securityIndex.equal_range(securityState);
    std::vector<typename TLBCacheList::iterator> toRemove;
    
    // Collect iterators to remove (can't modify while iterating)
    for (auto secIt = range.first; secIt != range.second; ++secIt) {
        toRemove.push_back(secIt->second);
    }
    
    // Remove entries from all structures
    for (auto listIt : toRemove) {
        // Remove from secondary indices
        removeFromSecondaryIndices(listIt->first, listIt);
        
        // Remove from primary structures
        tlbCacheMap.erase(listIt->first);
        tlbCacheList.erase(listIt);
    }
}

void TLBCache::invalidateStream(StreamID streamID) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    
    // Use secondary index for O(k) performance instead of O(n)
    auto range = streamIndex.equal_range(streamID);
    std::vector<typename TLBCacheList::iterator> toRemove;
    
    // Collect iterators to remove (can't modify while iterating)
    for (auto streamIt = range.first; streamIt != range.second; ++streamIt) {
        toRemove.push_back(streamIt->second);
    }
    
    // Remove entries from all structures
    for (auto listIt : toRemove) {
        // Remove from secondary indices
        removeFromSecondaryIndices(listIt->first, listIt);
        
        // Remove from primary structures
        tlbCacheMap.erase(listIt->first);
        tlbCacheList.erase(listIt);
    }
}

void TLBCache::invalidatePASID(StreamID streamID, PASID pasid) {
    std::lock_guard<std::mutex> lock(cacheMutex);
    
    // Use secondary index for O(k) performance instead of O(n)
    StreamPASIDKey pasidKey;
    pasidKey.streamID = streamID;
    pasidKey.pasid = pasid;
    
    auto range = pasidIndex.equal_range(pasidKey);
    std::vector<typename TLBCacheList::iterator> toRemove;
    
    // Collect iterators to remove (can't modify while iterating)
    for (auto pasidIt = range.first; pasidIt != range.second; ++pasidIt) {
        toRemove.push_back(pasidIt->second);
    }
    
    // Remove entries from all structures
    for (auto listIt : toRemove) {
        // Remove from secondary indices
        removeFromSecondaryIndices(listIt->first, listIt);
        
        // Remove from primary structures
        tlbCacheMap.erase(listIt->first);
        tlbCacheList.erase(listIt);
    }
}

void TLBCache::invalidatePage(StreamID streamID, PASID pasid, IOVA iova) {
    invalidate(streamID, pasid, iova);
}

void TLBCache::clear() {
    std::lock_guard<std::mutex> lock(cacheMutex);
    tlbCacheMap.clear();
    tlbCacheList.clear();
    
    // Clear all secondary indices
    streamIndex.clear();
    pasidIndex.clear();
    securityIndex.clear();
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
    
    // Clear all secondary indices
    streamIndex.clear();
    pasidIndex.clear();
    securityIndex.clear();
    
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
        
        // Remove from secondary indices before erasing
        removeFromSecondaryIndices(last->first, last);
        
        // Remove from primary index and list
        tlbCacheMap.erase(last->first);
        tlbCacheList.erase(last);
    }
}

void TLBCache::moveToFront(typename TLBCacheList::iterator it) {
    if (it != tlbCacheList.begin()) {
        auto entry = *it;
        
        // Remove from secondary indices with old iterator
        removeFromSecondaryIndices(entry.first, it);
        
        // Move to front in list
        tlbCacheList.erase(it);
        tlbCacheList.push_front(entry);
        auto newIt = tlbCacheList.begin();
        tlbCacheMap[entry.first] = newIt;
        
        // Add back to secondary indices with new iterator
        addToSecondaryIndices(entry.first, newIt);
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

// Add entry to all secondary indices for fast invalidation
void TLBCache::addToSecondaryIndices(const CacheKey& key, typename TLBCacheList::iterator it) {
    // Add to StreamID index
    streamIndex.insert(std::make_pair(key.streamID, it));
    
    // Add to StreamID+PASID compound index
    StreamPASIDKey pasidKey;
    pasidKey.streamID = key.streamID;
    pasidKey.pasid = key.pasid;
    pasidIndex.insert(std::make_pair(pasidKey, it));
    
    // Add to SecurityState index
    securityIndex.insert(std::make_pair(key.securityState, it));
}

// Remove entry from all secondary indices
void TLBCache::removeFromSecondaryIndices(const CacheKey& key, typename TLBCacheList::iterator it) {
    // Remove from StreamID index
    auto streamRange = streamIndex.equal_range(key.streamID);
    for (auto streamIt = streamRange.first; streamIt != streamRange.second; ++streamIt) {
        if (streamIt->second == it) {
            streamIndex.erase(streamIt);
            break;
        }
    }
    
    // Remove from StreamID+PASID compound index
    StreamPASIDKey pasidKey;
    pasidKey.streamID = key.streamID;
    pasidKey.pasid = key.pasid;
    auto pasidRange = pasidIndex.equal_range(pasidKey);
    for (auto pasidIt = pasidRange.first; pasidIt != pasidRange.second; ++pasidIt) {
        if (pasidIt->second == it) {
            pasidIndex.erase(pasidIt);
            break;
        }
    }
    
    // Remove from SecurityState index
    auto securityRange = securityIndex.equal_range(key.securityState);
    for (auto secIt = securityRange.first; secIt != securityRange.second; ++secIt) {
        if (secIt->second == it) {
            securityIndex.erase(secIt);
            break;
        }
    }
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