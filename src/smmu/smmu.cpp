// ARM SMMU v3 Main Controller Implementation
// Copyright (c) 2024 John Greninger
// Enhanced with Task 5.2: Translation Engine

#include "smmu/smmu.h"
#include <chrono>
#include <algorithm>

namespace smmu {

// Default constructor - Initialize SMMU with default configuration
SMMU::SMMU() 
    : configuration(SMMUConfiguration::createDefault()),
      faultHandler(std::shared_ptr<FaultHandler>(new FaultHandler())),
      tlbCache(std::unique_ptr<TLBCache>(new TLBCache(configuration.getCacheConfiguration().tlbCacheSize))),
      globalFaultMode(FaultMode::Terminate),
      cachingEnabled(configuration.getCacheConfiguration().enableCaching),
      translationCount(0),
      cacheHits(0),
      cacheMisses(0),
      // Task 5.3: Initialize event and command processing queues using configuration
      maxEventQueueSize(configuration.getQueueConfiguration().eventQueueSize),
      maxCommandQueueSize(configuration.getQueueConfiguration().commandQueueSize),
      maxPRIQueueSize(configuration.getQueueConfiguration().priQueueSize) {
    // Initialize empty stream map - streams will be added via configureStream
    // ARM SMMU v3 spec: Controller starts in disabled state with no streams configured
    
    // Task 5.3: Initialize empty queues for event and command processing
    eventQueue.clear();
    commandQueue.clear();
    priQueue.clear();
}

// Constructor with custom configuration
SMMU::SMMU(const SMMUConfiguration& config)
    : configuration(config),
      faultHandler(std::shared_ptr<FaultHandler>(new FaultHandler())),
      tlbCache(std::unique_ptr<TLBCache>(new TLBCache(config.getCacheConfiguration().tlbCacheSize))),
      globalFaultMode(FaultMode::Terminate),
      cachingEnabled(config.getCacheConfiguration().enableCaching),
      translationCount(0),
      cacheHits(0),
      cacheMisses(0),
      // Task 5.3: Initialize event and command processing queues using configuration
      maxEventQueueSize(config.getQueueConfiguration().eventQueueSize),
      maxCommandQueueSize(config.getQueueConfiguration().commandQueueSize),
      maxPRIQueueSize(config.getQueueConfiguration().priQueueSize) {
    // Validate the provided configuration
    if (!config.isValid()) {
        // Fall back to default configuration if invalid
        configuration = SMMUConfiguration::createDefault();
    }
    
    // Initialize empty stream map - streams will be added via configureStream
    // ARM SMMU v3 spec: Controller starts in disabled state with no streams configured
    
    // Task 5.3: Initialize empty queues for event and command processing
    eventQueue.clear();
    commandQueue.clear();
    priQueue.clear();
}

// Destructor - RAII cleanup
SMMU::~SMMU() {
    // Clear all streams (unique_ptr will handle cleanup automatically)
    streamMap.clear();
    // faultHandler shared_ptr will handle cleanup automatically
}

// Main translate() API - Enhanced with Task 5.2: Two-stage translation and TLBCache integration
TranslationResult SMMU::translate(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) {
    // Update translation statistics (atomic operation for thread safety)
    translationCount.fetch_add(1);
    
    // ARM SMMU v3 spec: Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        // No need to record cache miss here - TLBCache handles its own statistics
        return makeTranslationError(SMMUError::InvalidStreamID);
    }
    
    // Task 5.2: Optimized fast path - Check TLB cache first for maximum performance
    if (cachingEnabled && tlbCache) {
        // Performance optimization: Direct TLB lookup without intermediate method call overhead
        IOVA pageAlignedIOVA = iova & ~PAGE_MASK;
        TLBEntry* entry = tlbCache->lookup(streamID, pasid, pageAlignedIOVA, securityState);
        
        if (entry && entry->valid) {
            // Security validation: Ensure TLB entry SecurityState matches request
            if (entry->securityState != securityState) {
                // Security state mismatch - invalidate entry and continue to full translation
                tlbCache->invalidate(streamID, pasid, pageAlignedIOVA, securityState);
            } else {
                // Fast path: Validate cache entry age inline for speed
                uint64_t currentTime = std::chrono::duration_cast<std::chrono::microseconds>(
                    std::chrono::steady_clock::now().time_since_epoch()).count();
                
                const uint64_t MAX_CACHE_AGE_US = 1000000; // 1 second max age
                if (currentTime - entry->timestamp <= MAX_CACHE_AGE_US) {
                    // Cache hit - validate access permissions against requested access type
                    if (!validateAccessPermissions(entry->permissions, accessType)) {
                        // Permission fault - record fault and return error
                        FaultRecord fault;
                        fault.streamID = streamID;
                        fault.pasid = pasid;
                        fault.address = iova;
                        fault.faultType = FaultType::PermissionFault;
                        fault.accessType = accessType;
                        fault.securityState = securityState;
                        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
                            std::chrono::steady_clock::now().time_since_epoch()).count();
                        
                        recordFault(fault);
                        return makeTranslationError(SMMUError::PagePermissionViolation);
                    }
                    
                    // TLBCache already recorded hit statistics
                    // No need for additional recordCacheHit() here
                    
                    PA finalPA = entry->physicalAddress + (iova & PAGE_MASK);
                    TranslationData data(finalPA, entry->permissions, entry->securityState);
                    return TranslationResult(data);
                } else {
                    // Entry expired - invalidate and continue to full translation
                    tlbCache->invalidate(streamID, pasid, pageAlignedIOVA, securityState);
                }
            }
        }
        // Cache miss - TLBCache already recorded miss statistics
        // No need for additional recordCacheMiss() here
    }
    
    // Check if stream is configured (protect streamMap access)
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        // Stream not configured - record translation fault
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        // No need to record cache miss here - TLBCache handles its own statistics
        return makeTranslationError(SMMUError::StreamNotConfigured);
    }
    
    StreamContext* streamContext = streamIt->second.get();
    
    // Task 5.2: Enhanced two-stage translation with comprehensive error handling
    TranslationResult result = performTwoStageTranslation(streamID, pasid, iova, accessType, securityState, streamContext);
    
    // Task 5.2: Cache successful translations for future lookups
    if (result.isOk() && isTranslationCacheable(result) && cachingEnabled && tlbCache) {
        cacheTranslationResult(streamID, pasid, iova, result);
        // No need to record cache hit here - this is cache storage, not a hit
    } else if (result.isError()) {
        // Task 5.2: Enhanced fault handling and recovery mechanisms
        // ARM SMMU v3 spec: Comprehensive fault classification and recovery
        handleTranslationFailure(streamID, pasid, iova, accessType, securityState, result);
        
        // ARM SMMU v3 spec: Fault recovery based on global fault mode
        if (globalFaultMode == FaultMode::Stall) {
            // In stall mode, we could implement retry logic here
            // For now, we just ensure the fault is properly recorded
            FaultType faultType = classifyTranslationFault(streamID, pasid, iova, accessType, securityState);
            (void)faultType; // Suppress unused variable warning - used for future fault recovery logic
        }
    }
    
    return result;
}

// Stream management - Create and configure new stream with StreamContext
VoidResult SMMU::configureStream(StreamID streamID, const StreamConfig& config) {
    // ARM SMMU v3 spec: Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    // Check if stream already exists
    if (streamMap.find(streamID) != streamMap.end()) {
        // Update existing stream configuration
        StreamContext* streamContext = streamMap[streamID].get();
        VoidResult updateResult = streamContext->updateConfiguration(config);
        if (updateResult.isError()) {
            return updateResult;
        }
        
        // Note: Stream enable/disable is managed separately from configuration
        // ARM SMMU v3 spec: Configuration and stream enabling are separate operations
    } else {
        // Create new StreamContext
        std::unique_ptr<StreamContext> streamContext(new StreamContext());
        
        // Configure the stream context with provided configuration
        VoidResult configResult = streamContext->updateConfiguration(config);
        if (configResult.isError()) {
            return configResult;
        }
        
        // Note: Stream enable/disable is managed separately from configuration
        // ARM SMMU v3 spec: Configuration and stream enabling are separate operations
        
        // Set fault handler for the stream
        streamContext->setFaultHandler(faultHandler);
        
        // Add to stream map
        streamMap[streamID] = std::move(streamContext);
    }
    
    return makeVoidSuccess();
}

// Clean stream removal with proper cleanup
VoidResult SMMU::removeStream(StreamID streamID) {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    // Disable stream before removal
    VoidResult disableResult = streamIt->second->disableStream();
    (void)disableResult; // Suppress unused variable warning - continue even if disable fails
    
    // Clear all PASIDs for this stream
    streamIt->second->clearAllPASIDs();
    
    // Remove from map (unique_ptr will handle cleanup)
    streamMap.erase(streamIt);
    
    return makeVoidSuccess();
}

// Check stream existence and configuration
Result<bool> SMMU::isStreamConfigured(StreamID streamID) const {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeError<bool>(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    bool configured = streamMap.find(streamID) != streamMap.end();
    return Result<bool>(configured);
}

// Stream lifecycle control via StreamContext
VoidResult SMMU::enableStream(StreamID streamID) {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    // Delegate to StreamContext
    VoidResult result = streamIt->second->enableStream();
    if (result.isError()) {
        return result;
    }
    
    return makeVoidSuccess();
}

VoidResult SMMU::disableStream(StreamID streamID) {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    // Delegate to StreamContext
    VoidResult result = streamIt->second->disableStream();
    if (result.isError()) {
        return result;
    }
    
    return makeVoidSuccess();
}

// Query stream operational state
Result<bool> SMMU::isStreamEnabled(StreamID streamID) const {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeError<bool>(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeError<bool>(SMMUError::StreamNotConfigured);
    }
    
    Result<bool> enabledResult = streamIt->second->isStreamEnabled();
    return enabledResult;
}

// PASID management for streams - Create PASID within specific stream
VoidResult SMMU::createStreamPASID(StreamID streamID, PASID pasid) {
    // ARM SMMU v3 spec: Validate PASID bounds
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);
    }
    
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    return streamIt->second->createPASID(pasid);
}

// Remove PASID from stream
VoidResult SMMU::removeStreamPASID(StreamID streamID, PASID pasid) {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    // Remove PASID from stream context
    VoidResult result = streamIt->second->removePASID(pasid);
    
    // ARM SMMU v3 spec: Invalidate all TLB cache entries for removed PASID
    // This ensures subsequent translations to this PASID will fail properly
    if (result.isOk()) {
        invalidatePASIDCache(streamID, pasid);
    }
    
    return result;
}

// Per-stream per-PASID page operations
VoidResult SMMU::mapPage(StreamID streamID, PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState) {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    return streamIt->second->mapPage(pasid, iova, pa, permissions, securityState);
}

VoidResult SMMU::unmapPage(StreamID streamID, PASID pasid, IOVA iova) {
    // Validate StreamID bounds
    if (streamID > MAX_STREAM_ID) {
        return makeVoidError(SMMUError::InvalidStreamID);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    // Unmap page from stream context
    VoidResult result = streamIt->second->unmapPage(pasid, iova);
    
    // ARM SMMU v3 spec: Invalidate TLB cache entry for unmapped page
    // This ensures subsequent translations to this page will fail properly
    if (result.isOk() && tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK);
    }
    
    return result;
}

// System-wide fault event handling
Result<std::vector<FaultRecord>> SMMU::getEvents() {
    if (!faultHandler) {
        return makeError<std::vector<FaultRecord>>(SMMUError::FaultHandlingError);
    }
    
    try {
        std::vector<FaultRecord> events = faultHandler->getEvents();
        return makeSuccess(std::move(events));
    } catch (...) {
        return makeError<std::vector<FaultRecord>>(SMMUError::InternalError);
    }
}

VoidResult SMMU::clearEvents() {
    if (!faultHandler) {
        return makeVoidError(SMMUError::FaultHandlingError);
    }
    
    try {
        faultHandler->clearEvents();
        return makeVoidSuccess();
    } catch (...) {
        return makeVoidError(SMMUError::InternalError);
    }
}

// Global configuration management - System-wide fault handling policy
VoidResult SMMU::setGlobalFaultMode(FaultMode mode) {
    // Validate fault mode
    if (mode != FaultMode::Terminate && mode != FaultMode::Stall) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    std::lock_guard<std::mutex> lock(sMMUMutex);
    globalFaultMode = mode;
    
    // Apply to all configured streams
    for (auto& streamPair : streamMap) {
        StreamConfig config = streamPair.second->getStreamConfiguration();
        config.faultMode = mode;
        VoidResult result = streamPair.second->updateConfiguration(config);
        if (result.isError()) {
            // Continue updating other streams but return error
            return result;
        }
    }
    
    return makeVoidSuccess();
}

// Global caching enable/disable - Enhanced with TLBCache integration (Task 5.2)
VoidResult SMMU::enableCaching(bool enable) {
    std::lock_guard<std::mutex> lock(sMMUMutex);
    
    cachingEnabled = enable;
    // ARM SMMU v3 spec: Caching policy affects TLB behavior
    if (!enable && tlbCache) {
        try {
            // If disabling caching, clear the cache to ensure consistency
            tlbCache->clear();
        } catch (...) {
            return makeVoidError(SMMUError::CacheOperationFailed);
        }
    }
    
    return makeVoidSuccess();
}

// Statistics and monitoring - System-wide monitoring
size_t SMMU::getStreamCount() const {
    return streamMap.size();
}

uint64_t SMMU::getTotalTranslations() const {
    return translationCount;
}

uint64_t SMMU::getTotalFaults() const {
    return faultHandler->getTotalFaultCount();
}

uint64_t SMMU::getTranslationCount() const {
    return translationCount;
}

uint64_t SMMU::getCacheHitCount() const {
    if (tlbCache) {
        return tlbCache->getHitCount();
    }
    return 0;
}

uint64_t SMMU::getCacheMissCount() const {
    if (tlbCache) {
        return tlbCache->getMissCount();
    }
    return 0;
}

// System state management - Enhanced with TLBCache statistics (Task 5.2)
void SMMU::resetStatistics() {
    translationCount = 0;
    // Remove local cache statistics - delegate to TLBCache
    faultHandler->resetStatistics();
    
    // Task 5.2: Reset TLB cache statistics
    if (tlbCache) {
        tlbCache->resetStatistics();
    }
}

void SMMU::reset() {
    // Complete system reset - Enhanced with TLBCache reset (Task 5.2)
    streamMap.clear();
    resetStatistics();
    faultHandler->reset();
    globalFaultMode = FaultMode::Terminate;
    cachingEnabled = true;
    
    // Task 5.2: Reset TLB cache
    if (tlbCache) {
        tlbCache->reset();
    }
    
    // Task 5.3: Reset event and command processing queues
    clearEventQueue();
    clearCommandQueue();
    clearPRIQueue();
}

// Helper methods
void SMMU::recordFault(const FaultRecord& fault) {
    faultHandler->recordFault(fault);
}

void SMMU::recordCacheHit() const {
    cacheHits.fetch_add(1);
}

void SMMU::recordCacheMiss() const {
    cacheMisses.fetch_add(1);
}

// Task 5.2: Enhanced cache management operations with comprehensive functionality
void SMMU::invalidateTranslationCache() {
    // ARM SMMU v3 spec: Global TLB invalidation with enhanced cleanup
    if (tlbCache) {
        tlbCache->invalidateAll();
        
        // Reset cache statistics for consistency
        // Note: TLBCache statistics are preserved across invalidation for debugging
        
        // ARM SMMU v3 spec: After global invalidation, ensure coherency
        // All subsequent translations will miss and go through full translation
        
        // Performance optimization: Clear internal cache statistics if needed
        // (keeping them for debugging purposes in this implementation)
    }
    
    // ARM SMMU v3 spec: Global invalidation affects all streams and PASIDs
    // No additional stream-specific cleanup needed
}

void SMMU::invalidateStreamCache(StreamID streamID) {
    // ARM SMMU v3 spec: Stream-specific TLB invalidation with validation
    if (tlbCache) {
        // ARM SMMU v3 spec: Validate StreamID before invalidation
        if (streamID <= MAX_STREAM_ID) {
            tlbCache->invalidateStream(streamID);
            
            // Performance optimization: Could update per-stream statistics here
            // For now, rely on global statistics
        }
    }
    
    // ARM SMMU v3 spec: Stream invalidation affects all PASIDs within the stream
    // The TLBCache implementation handles this automatically
}

void SMMU::invalidatePASIDCache(StreamID streamID, PASID pasid) {
    // ARM SMMU v3 spec: PASID-specific TLB invalidation with comprehensive validation
    if (tlbCache) {
        // ARM SMMU v3 spec: Validate both StreamID and PASID bounds
        if (streamID <= MAX_STREAM_ID && pasid <= MAX_PASID) {
            tlbCache->invalidatePASID(streamID, pasid);
            
            // ARM SMMU v3 spec: PASID invalidation is surgical - only affects specific context
            // This is the most efficient invalidation operation
        }
    }
    
    // Performance optimization: Could track per-PASID invalidation statistics
    // for cache tuning and debugging purposes
}

// Task 5.2: Enhanced cache statistics with performance monitoring
CacheStatistics SMMU::getCacheStatistics() const {
    CacheStatistics stats;
    
    if (tlbCache) {
        // Performance optimization: Get all cache statistics in one call to avoid multiple method calls
        stats.hitCount = tlbCache->getHitCount();
        stats.missCount = tlbCache->getMissCount();
        stats.totalLookups = tlbCache->getTotalLookups();
        stats.currentSize = tlbCache->getSize();
        stats.maxSize = tlbCache->getCapacity();
        stats.evictionCount = 0; // TLBCache doesn't expose eviction count yet
        
        // Calculate hit rate with enhanced precision
        stats.calculateHitRate();
        
        // ARM SMMU v3 spec: Additional performance metrics
        if (stats.totalLookups > 0) {
            // Calculate cache efficiency ratio
            double efficiency = (stats.currentSize > 0) ? 
                (static_cast<double>(stats.hitCount) / static_cast<double>(stats.currentSize)) : 0.0;
            (void)efficiency; // Suppress unused variable warning - used for future cache tuning logic
        }
    } else {
        // Cache disabled - all zeros
        stats.hitCount = cacheHits;
        stats.missCount = cacheMisses;
        stats.totalLookups = cacheHits + cacheMisses;
        stats.calculateHitRate();
    }
    
    return stats;
}

// Task 5.2: Enhanced two-stage translation logic with sophisticated coordination
TranslationResult SMMU::performTwoStageTranslation(StreamID streamID, PASID pasid, IOVA iova, 
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext) {
    // ARM SMMU v3 spec: Enhanced two-stage translation coordination
    // This method provides sophisticated coordination between Stage-1 and Stage-2 translations
    // with comprehensive error handling and performance optimization
    
    if (!streamContext) {
        // Defensive programming - should not happen if called correctly
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        return makeTranslationError(SMMUError::StreamNotConfigured);
    }
    
    // ARM SMMU v3 spec: Get stream configuration to determine translation stages
    StreamConfig config = streamContext->getStreamConfiguration();
    
    TranslationResult result = makeTranslationError(SMMUError::InternalError);
    
    // ARM SMMU v3 spec: Handle different stage combinations
    if (!config.translationEnabled) {
        // Translation disabled - bypass mode (IOVA = PA)
        PagePermissions bypassPerms(true, true, true); // Full permissions in bypass
        TranslationData data(iova, bypassPerms, securityState);
        return TranslationResult(data);
    }
    
    if (config.stage1Enabled && config.stage2Enabled) {
        // Two-stage translation: IOVA -> IPA -> PA
        result = performBothStagesTranslation(streamID, pasid, iova, accessType, securityState, streamContext);
    } else if (config.stage1Enabled && !config.stage2Enabled) {
        // Stage-1 only: IOVA -> PA directly
        result = performStage1OnlyTranslation(streamID, pasid, iova, accessType, securityState, streamContext);
    } else if (!config.stage1Enabled && config.stage2Enabled) {
        // Stage-2 only: IPA -> PA (IOVA = IPA)
        result = performStage2OnlyTranslation(streamID, pasid, iova, accessType, securityState, streamContext);
    } else {
        // No stages enabled but translation enabled - configuration error
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        return makeTranslationError(SMMUError::ConfigurationError);
    }
    
    // ARM SMMU v3 spec: Enhanced validation of translation results
    if (result.isOk()) {
        // Validate translated address alignment and sanity
        TranslationData data = result.getValue();
        if (data.physicalAddress == 0 && iova != 0) {
            // Suspicious translation to null address
            
            FaultRecord fault;
            fault.streamID = streamID;
            fault.pasid = pasid;
            fault.address = iova;
            fault.faultType = FaultType::TranslationFault;
            fault.accessType = accessType;
            fault.securityState = securityState;
            fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
                std::chrono::steady_clock::now().time_since_epoch()).count();
            
            recordFault(fault);
            return makeTranslationError(SMMUError::TranslationTableError);
        } else {
            // Validate access permissions against requested access type
            if (!validateAccessPermissions(data.permissions, accessType)) {
                
                FaultRecord fault;
                fault.streamID = streamID;
                fault.pasid = pasid;
                fault.address = iova;
                fault.faultType = FaultType::PermissionFault;
                fault.accessType = accessType;
                fault.securityState = securityState;
                fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
                    std::chrono::steady_clock::now().time_since_epoch()).count();
                
                recordFault(fault);
                return makeTranslationError(SMMUError::PagePermissionViolation);
            }
        }
    }
    
    return result;
}

// Task 5.2: Translation caching helper methods
bool SMMU::isTranslationCacheable(const TranslationResult& result) const {
    // ARM SMMU v3 spec: Only cache successful, completed translations
    // Avoid caching translations that might be context-sensitive or temporary
    if (result.isError()) {
        return false;
    }
    TranslationData data = result.getValue();
    return data.physicalAddress != 0;
}

void SMMU::cacheTranslationResult(StreamID streamID, PASID pasid, IOVA iova, 
                                 const TranslationResult& result) {
    if (!tlbCache || result.isError() || !cachingEnabled) {
        return; // Caching disabled or invalid result
    }
    
    TranslationData data = result.getValue();
    
    // ARM SMMU v3 spec: Only cache page-aligned translations for efficiency
    IOVA pageAlignedIOVA = iova & ~PAGE_MASK; // Page-align the IOVA
    PA pageAlignedPA = data.physicalAddress & ~PAGE_MASK; // Page-align the PA
    
    // Validate that the translation is cacheable
    if (pageAlignedPA == 0 && pageAlignedIOVA != 0) {
        // Don't cache suspicious null translations
        return;
    }
    
    // We already know this is a cache miss from the main translate() method
    // No need to lookup again - just insert the new entry
    
    // Convert TranslationResult to TLBEntry for caching
    TLBEntry entry;
    entry.streamID = streamID;
    entry.pasid = pasid;
    entry.iova = pageAlignedIOVA; // Store page-aligned IOVA
    entry.physicalAddress = pageAlignedPA; // Store page-aligned PA
    entry.permissions = data.permissions;
    entry.securityState = data.securityState;
    entry.valid = true;
    entry.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
    
    // ARM SMMU v3 spec: Insert into TLB with LRU eviction if needed
    tlbCache->insert(entry);
}

TranslationResult SMMU::lookupTranslationCache(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    if (!tlbCache || !cachingEnabled) {
        return makeTranslationError(SMMUError::CacheOperationFailed); // Failed result - caching disabled
    }
    
    // ARM SMMU v3 spec: Perform optimized TLB lookup with page alignment
    IOVA pageAlignedIOVA = iova & ~PAGE_MASK; // Page-align the IOVA for lookup
    
    TLBEntry* entry = tlbCache->lookup(streamID, pasid, pageAlignedIOVA, securityState);
    if (!entry || !entry->valid) {
        return makeTranslationError(SMMUError::CacheEntryNotFound); // Cache miss
    }
    
    // Security state validation
    if (entry->securityState != securityState) {
        return makeTranslationError(FaultType::SecurityFault);
    }
    
    // ARM SMMU v3 spec: Validate cache entry freshness and coherency
    uint64_t currentTime = std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
    
    // Check if the cache entry is still valid (simple aging mechanism)
    const uint64_t MAX_CACHE_AGE_US = 1000000; // 1 second max age
    if (currentTime - entry->timestamp > MAX_CACHE_AGE_US) {
        // Entry is too old, invalidate and return cache miss
        tlbCache->invalidate(streamID, pasid, pageAlignedIOVA, securityState);
        return makeTranslationError(SMMUError::CacheEntryNotFound); // Cache miss due to age
    }
    
    // Convert TLBEntry back to TranslationResult with page offset preservation
    PA finalPhysicalAddress = entry->physicalAddress + (iova & PAGE_MASK); // Add back page offset
    return makeTranslationSuccess(finalPhysicalAddress, entry->permissions, entry->securityState);
}

void SMMU::generateCacheKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState, uint64_t& cacheKey) const {
    // Generate a unique cache key combining StreamID, PASID, IOVA, and SecurityState
    // This is used internally by TLBCache, but provided for completeness
    cacheKey = (static_cast<uint64_t>(streamID) << 32) | 
               (static_cast<uint64_t>(pasid) << 12) | 
               (static_cast<uint64_t>(securityState) << 20) |
               (iova & 0xFFFULL); // Use page-aligned portion
}

// Task 5.2: Enhanced stage-specific translation methods
TranslationResult SMMU::performBothStagesTranslation(StreamID streamID, PASID pasid, IOVA iova, 
                                                    AccessType accessType, SecurityState securityState, StreamContext* streamContext) {
    // ARM SMMU v3 spec: Two-stage translation IOVA -> IPA -> PA
    // This provides comprehensive coordination between Stage-1 and Stage-2 translations
    // with proper fault handling and permission intersection
    
    // Get stream configuration to validate stage setup
    StreamConfig config = streamContext->getStreamConfiguration();
    
    // Validate that both stages are properly configured
    if (!config.stage1Enabled || !config.stage2Enabled) {
        // Configuration error - both stages should be enabled for this method
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::BothStages, 0, 0);
        return makeTranslationError(SMMUError::ConfigurationError);
    }
    
    // Stage 1: IOVA -> IPA translation (using per-PASID address space)
    AddressSpace* stage1AddressSpace = streamContext->getPASIDAddressSpace(pasid);
    if (!stage1AddressSpace) {
        // PASID not configured - Stage-1 translation fault
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage1Only, 0, 0);
        return makeTranslationError(SMMUError::PASIDNotFound);
    }
    
    // Perform Stage-1 translation: IOVA -> IPA
    TranslationResult stage1Result = stage1AddressSpace->translatePage(iova, accessType, securityState);
    if (stage1Result.isError()) {
        // Stage-1 translation failed - record fault with comprehensive syndrome
        // Convert SMMUError back to FaultType for fault recording
        FaultType faultType = (stage1Result.getError() == SMMUError::PageNotMapped) ? FaultType::TranslationFault : FaultType::AccessFault;
        recordComprehensiveFault(streamID, pasid, iova, faultType,
                               accessType, securityState, FaultStage::Stage1Only, 1, 0);
        return stage1Result;
    }
    
    // Stage 1 success - IPA is now in stage1Result.physicalAddress
    IPA intermediatePA = stage1Result.getValue().physicalAddress;
    
    // ARM SMMU v3 spec: Validate IPA from Stage-1 before Stage-2 translation
    if (intermediatePA == 0 && iova != 0) {
        // Invalid IPA produced by Stage-1
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage1Only, 1, 0);
        return makeTranslationError(SMMUError::TranslationTableError);
    }
    
    // Stage 2: IPA -> PA translation (using stream's Stage-2 address space)
    // ARM SMMU v3 spec: Stage-2 uses shared address space across stream
    AddressSpace* stage2AddressSpace = streamContext->getStage2AddressSpace();
    if (!stage2AddressSpace) {
        // Stage-2 address space not configured - Stage-2 translation fault
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage2Only, 0, 0);
        return makeTranslationError(SMMUError::AddressSpaceExhausted);
    }
    
    // Perform Stage-2 translation: IPA -> PA
    // ARM SMMU v3 spec: Stage-2 translates the IPA from Stage-1 to final PA
    TranslationResult stage2Result = stage2AddressSpace->translatePage(intermediatePA, accessType, securityState);
    if (stage2Result.isError()) {
        // Stage-2 translation failed - record fault with comprehensive syndrome
        FaultType stage2FaultType = (stage2Result.getError() == SMMUError::PageNotMapped) ? 
                                   FaultType::Stage2TranslationFault : FaultType::Stage2PermissionFault;
        
        recordComprehensiveFault(streamID, pasid, iova, stage2FaultType,
                               accessType, securityState, FaultStage::Stage2Only, 2, 0);
        return stage2Result;
    }
    
    // Both stages successful - create final translation result
    const TranslationData& stage1Data = stage1Result.getValue();
    const TranslationData& stage2Data = stage2Result.getValue();
    
    // ARM SMMU v3 spec: Final permissions are intersection of Stage-1 and Stage-2 permissions
    // This ensures that access is only allowed if both stages permit it
    PagePermissions finalPermissions;
    finalPermissions.read = stage1Data.permissions.read && stage2Data.permissions.read;
    finalPermissions.write = stage1Data.permissions.write && stage2Data.permissions.write;
    finalPermissions.execute = stage1Data.permissions.execute && stage2Data.permissions.execute;
    
    // ARM SMMU v3 spec: Validate final permissions against requested access
    if (!validateAccessPermissions(finalPermissions, accessType)) {
        // Permission fault after two-stage translation - final permission check failed
        recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                               accessType, securityState, FaultStage::BothStages, 2, 0);
        return makeTranslationError(SMMUError::PagePermissionViolation);
    }
    
    // ARM SMMU v3 spec: Validate security state consistency across both stages
    if (stage1Data.securityState != stage2Data.securityState) {
        // Security state inconsistency between stages
        recordComprehensiveFault(streamID, pasid, iova, FaultType::SecurityFault,
                               accessType, securityState, FaultStage::BothStages, 0, 0);
        return makeTranslationError(SMMUError::InvalidSecurityState);
    }
    
    // ARM SMMU v3 spec: Final security state validation - use stage2 security state as reference
    if (!validateSecurityState(securityState, stage2Data.securityState)) {
        // Security state violation
        recordComprehensiveFault(streamID, pasid, iova, FaultType::SecurityFault,
                               accessType, securityState, FaultStage::BothStages, 0, 0);
        return makeTranslationError(SMMUError::InvalidSecurityState);
    }
    
    // Create successful final translation result
    return makeTranslationSuccess(stage2Data.physicalAddress, finalPermissions, stage2Data.securityState);
}

TranslationResult SMMU::performStage1OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova, 
                                                    AccessType accessType, SecurityState securityState, StreamContext* streamContext) {
    // ARM SMMU v3 spec: Stage-1 only translation IOVA -> PA
    TranslationResult result = streamContext->translate(pasid, iova, accessType, securityState);
    
    // Record fault if translation failed
    if (result.isError()) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = (result.getError() == SMMUError::PageNotMapped) ? FaultType::TranslationFault : FaultType::AccessFault;
        fault.accessType = accessType;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        return result;
    }
    
    // Enhanced validation for Stage-1 only mode
    const TranslationData& translationData = result.getValue();
    if (translationData.physicalAddress == 0 && iova != 0) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        return makeTranslationError(SMMUError::PageNotMapped);
    }
    
    return result;
}

TranslationResult SMMU::performStage2OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova, 
                                                   AccessType accessType, SecurityState securityState, StreamContext* streamContext) {
    // ARM SMMU v3 spec: Stage-2 only translation IPA -> PA (IOVA treated as IPA)
    TranslationResult result = streamContext->translate(pasid, iova, accessType, securityState);
    
    // Record fault if translation failed
    if (result.isError()) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = (result.getError() == SMMUError::PageNotMapped) ? FaultType::TranslationFault : FaultType::AccessFault;
        fault.accessType = accessType;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        return result;
    }
    
    // Enhanced validation for Stage-2 only mode
    const TranslationData& translationData = result.getValue();
    if (translationData.physicalAddress == 0 && iova != 0) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        
        recordFault(fault);
        return makeTranslationError(SMMUError::PageNotMapped);
    }
    
    return result;
}

bool SMMU::validateAccessPermissions(const PagePermissions& permissions, AccessType accessType) const {
    // ARM SMMU v3 spec: Validate access permissions against requested operation
    switch (accessType) {
        case AccessType::Read:
            return permissions.read;
        case AccessType::Write:
            return permissions.write;
        case AccessType::Execute:
            return permissions.execute;
        default:
            return false; // Unknown access type
    }
}

// Task 5.2: Enhanced error handling and fault recovery methods
void SMMU::handleTranslationFailure(StreamID streamID, PASID pasid, IOVA iova, 
                                   AccessType accessType, SecurityState securityState, TranslationResult& result) {
    // ARM SMMU v3 spec: Comprehensive fault handling and recovery
    
    // Determine fault type from the Result error code
    FaultType faultType;
    if (result.isError()) {
        switch (result.getError()) {
            case SMMUError::PageNotMapped:
                faultType = FaultType::TranslationFault;
                break;
            case SMMUError::PagePermissionViolation:
                faultType = FaultType::PermissionFault;
                break;
            case SMMUError::InvalidAddress:
                faultType = FaultType::AddressSizeFault;
                break;
            case SMMUError::InvalidSecurityState:
                faultType = FaultType::SecurityFault;
                break;
            default:
                faultType = classifyTranslationFault(streamID, pasid, iova, accessType, securityState);
                break;
        }
    } else {
        // If result is successful, this shouldn't be called, but handle gracefully
        faultType = FaultType::TranslationFault;
    }
    
    // Create comprehensive fault record
    FaultRecord fault;
    fault.streamID = streamID;
    fault.pasid = pasid;
    fault.address = iova;
    fault.faultType = faultType;
    fault.accessType = accessType;
    fault.securityState = securityState;
    fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
    
    // ARM SMMU v3 spec: Record fault with proper context
    recordFault(fault);
    
    // ARM SMMU v3 spec: Implement fault recovery mechanisms based on fault type
    switch (faultType) {
        case FaultType::TranslationFault:
            // Could implement page fault handling or demand paging here
            handleTranslationFaultRecovery(streamID, pasid, iova, securityState);
            break;
            
        case FaultType::PermissionFault:
            // Could implement permission escalation or security logging
            handlePermissionFaultRecovery(streamID, pasid, iova, accessType, securityState);
            break;
            
        case FaultType::AddressSizeFault:
            // Could implement address range expansion or validation
            handleAddressSizeFaultRecovery(streamID, pasid, iova, securityState);
            break;
            
        case FaultType::AccessFault:
            // Could implement access retry or alternative path handling
            handleAccessFaultRecovery(streamID, pasid, iova, accessType, securityState);
            break;
            
        case FaultType::SecurityFault:
            // Security fault - log violation and notify security subsystem
            recordSecurityFault(streamID, pasid, iova, accessType, securityState, securityState);
            break;
    }
}

FaultType SMMU::classifyTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) const {
    (void)pasid; // Suppress unused parameter warning - reserved for future PASID-aware fault classification
    (void)accessType; // Suppress unused parameter warning - reserved for future access-aware fault classification  
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware fault classification
    
    // ARM SMMU v3 spec: Intelligent fault classification based on context
    
    // Check if stream exists
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return FaultType::TranslationFault; // Stream not configured
    }
    
    // Check if IOVA is within reasonable bounds (basic address size check)
    const uint64_t MAX_REASONABLE_IOVA = 0x0001000000000000ULL; // 48-bit address space
    if (iova > MAX_REASONABLE_IOVA) {
        return FaultType::AddressSizeFault;
    }
    
    // Check for null pointer dereference
    if (iova == 0) {
        return FaultType::AccessFault; // Null pointer access
    }
    
    // Default classification based on the failure context
    return FaultType::TranslationFault;
}

void SMMU::handleTranslationFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware recovery
    
    // ARM SMMU v3 spec: Translation fault recovery mechanisms
    // In a full implementation, this could trigger:
    // - Demand paging from storage
    // - Page table updates
    // - Memory allocation
    
    // For now, we invalidate any stale cache entries to ensure consistency
    if (tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK);
    }
}

void SMMU::handlePermissionFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) {
    (void)accessType; // Suppress unused parameter warning - reserved for future access-type-specific recovery
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware recovery
    // ARM SMMU v3 spec: Permission fault recovery mechanisms
    // This could implement:
    // - Security policy checks
    // - Permission escalation requests
    // - Access logging for security audit
    
    // For now, just ensure the translation is not cached with wrong permissions
    if (tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK);
    }
}

void SMMU::handleAddressSizeFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    (void)streamID; // Suppress unused parameter warning - reserved for future stream-specific recovery
    (void)pasid; // Suppress unused parameter warning - reserved for future PASID-specific recovery
    (void)iova; // Suppress unused parameter warning - reserved for future address-specific recovery
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware recovery
    // ARM SMMU v3 spec: Address size fault recovery
    // This could implement:
    // - Address space expansion
    // - Range validation updates
    // - Memory layout reconfiguration
    
    // For now, log the problematic address range
    // In a full implementation, this might update address space limits
}

void SMMU::handleAccessFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) {
    (void)accessType; // Suppress unused parameter warning - reserved for future access-type-specific recovery
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware recovery
    // ARM SMMU v3 spec: Access fault recovery mechanisms
    // This could implement:
    // - Retry logic with backoff
    // - Alternative access paths
    // - Hardware fault recovery
    
    // For now, we clear any potentially corrupted cache state
    if (tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK);
    }
}

// Task 5.3: Event Queue Management (Task 5.3.1)
void SMMU::processEventQueue() {
    // ARM SMMU v3 spec: Process event queue with proper prioritization
    // Events are processed in FIFO order with exception handling
    
    while (!eventQueue.empty()) {
        const EventEntry& event = eventQueue.front();
        
        // ARM SMMU v3 spec: Process different event types
        switch (event.type) {
            case EventType::TRANSLATION_FAULT:
            case EventType::PERMISSION_FAULT:
                // Fault events are already recorded by fault handler
                // Could implement additional fault-specific processing here
                break;
                
            case EventType::COMMAND_SYNC_COMPLETION:
                // Command synchronization completion event
                // ARM SMMU v3 spec: Notify completion of synchronization commands
                break;
                
            case EventType::PRI_PAGE_REQUEST:
                // Page Request Interface event
                // Could trigger page allocation or OS notification
                break;
                
            case EventType::ATC_INVALIDATE_COMPLETION:
                // Address Translation Cache invalidation completion
                // ARM SMMU v3 spec: Confirm cache invalidation operations
                break;
                
            case EventType::CONFIGURATION_ERROR:
                // Configuration error event
                // ARM SMMU v3 spec: Handle configuration validation failures
                break;
                
            case EventType::INTERNAL_ERROR:
                // Internal system error
                // ARM SMMU v3 spec: Handle internal hardware/software errors
                break;
        }
        
        // Remove processed event
        eventQueue.pop_front();
    }
}

Result<bool> SMMU::hasEvents() const {
    // ARM SMMU v3 spec: Check event queue state with error handling
    try {
        return makeSuccess(!eventQueue.empty());
    } catch (...) {
        // Event queue access failed
        return makeError<bool>(SMMUError::InternalError);
    }
}

std::vector<EventEntry> SMMU::getEventQueue() const {
    // ARM SMMU v3 spec: Return copy of event queue for external processing
    std::vector<EventEntry> events;
    events.reserve(eventQueue.size());
    
    for (const auto& event : eventQueue) {
        events.push_back(event);
    }
    
    return events;
}

void SMMU::clearEventQueue() {
    eventQueue.clear();
}

size_t SMMU::getEventQueueSize() const {
    return eventQueue.size();
}

// Task 5.3: Command Queue Processing Simulation (Task 5.3.2)
VoidResult SMMU::submitCommand(const CommandEntry& command) {
    // ARM SMMU v3 spec: Validate command queue capacity
    if (commandQueue.size() >= maxCommandQueueSize) {
        // Command queue full - cannot accept new commands
        generateEvent(EventType::INTERNAL_ERROR, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
        return makeVoidError(SMMUError::CommandQueueFull);
    }
    
    // Add timestamp to command
    CommandEntry timestampedCommand = command;
    timestampedCommand.timestamp = getCurrentTimestamp();
    
    // ARM SMMU v3 spec: Enqueue command for processing
    commandQueue.push_back(timestampedCommand);
    
    return makeVoidSuccess();
}

void SMMU::processCommandQueue() {
    // ARM SMMU v3 spec: Process command queue with proper ordering
    // Commands are processed in FIFO order with synchronization support
    
    while (!commandQueue.empty()) {
        const CommandEntry& command = commandQueue.front();
        
        // Process the command based on type
        processCommand(command);
        
        // Remove processed command
        commandQueue.pop_front();
        
        // ARM SMMU v3 spec: Handle synchronization commands
        if (command.type == CommandType::SYNC) {
            // Synchronization barrier - ensure all previous commands completed
            generateEvent(EventType::COMMAND_SYNC_COMPLETION, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
            break; // Synchronization point reached
        }
    }
}

Result<bool> SMMU::isCommandQueueFull() const {
    // ARM SMMU v3 spec: Check command queue capacity with error handling
    try {
        return makeSuccess(commandQueue.size() >= maxCommandQueueSize);
    } catch (...) {
        // Command queue access failed
        return makeError<bool>(SMMUError::InternalError);
    }
}

size_t SMMU::getCommandQueueSize() const {
    return commandQueue.size();
}

void SMMU::clearCommandQueue() {
    commandQueue.clear();
}

// Task 5.3: PRI Queue for Page Requests (Task 5.3.3)
void SMMU::submitPageRequest(const PRIEntry& request) {
    // ARM SMMU v3 spec: Validate PRI queue capacity
    if (priQueue.size() >= maxPRIQueueSize) {
        // PRI queue full - drop oldest request (simple overflow handling)
        priQueue.pop_front();
    }
    
    // Add timestamp to request
    PRIEntry timestampedRequest = request;
    timestampedRequest.timestamp = getCurrentTimestamp();
    
    // ARM SMMU v3 spec: Enqueue page request for processing
    priQueue.push_back(timestampedRequest);
    
    // Generate event for page request
    generateEvent(EventType::PRI_PAGE_REQUEST, request.streamID, request.pasid, request.requestedAddress, SecurityState::NonSecure);
}

void SMMU::processPRIQueue() {
    // ARM SMMU v3 spec: Process Page Request Interface queue
    // Page requests are processed to trigger page allocation or OS notification
    
    while (!priQueue.empty()) {
        const PRIEntry& request = priQueue.front();
        
        // ARM SMMU v3 spec: Process page request
        // In a full implementation, this would:
        // - Notify the OS about page faults
        // - Trigger demand paging mechanisms
        // - Handle page allocation requests
        
        // For simulation, we generate a command response
        CommandEntry response;
        response.type = CommandType::PRI_RESP;
        response.streamID = request.streamID;
        response.pasid = request.pasid;
        response.startAddress = request.requestedAddress;
        response.endAddress = request.requestedAddress;
        response.timestamp = getCurrentTimestamp();
        
        // Submit response command
        if (submitCommand(response)) {
            // Successfully submitted response
            priQueue.pop_front();
        } else {
            // Command queue full - retry later
            break;
        }
    }
}

std::vector<PRIEntry> SMMU::getPRIQueue() const {
    // ARM SMMU v3 spec: Return copy of PRI queue for external processing
    std::vector<PRIEntry> requests;
    requests.reserve(priQueue.size());
    
    for (const auto& request : priQueue) {
        requests.push_back(request);
    }
    
    return requests;
}

void SMMU::clearPRIQueue() {
    priQueue.clear();
}

size_t SMMU::getPRIQueueSize() const {
    return priQueue.size();
}

// Task 5.3: Cache Invalidation Command Handling (Task 5.3.4)
void SMMU::executeInvalidationCommand(const CommandEntry& command) {
    // ARM SMMU v3 spec: Execute cache invalidation commands
    switch (command.type) {
        case CommandType::CFGI_STE:
            // Stream Table Entry invalidation - invalidate specific stream
            invalidateStreamCache(command.streamID);
            break;
            
        case CommandType::CFGI_ALL:
            // All configuration invalidation - full cache invalidation
            invalidateTranslationCache();
            break;
            
        case CommandType::TLBI_NH_ALL:
        case CommandType::TLBI_EL2_ALL:
        case CommandType::TLBI_S12_VMALL:
            // TLB invalidation commands
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid);
            break;
            
        case CommandType::ATC_INV:
            // Address Translation Cache invalidation
            executeATCInvalidationCommand(command.streamID, command.pasid, 
                                        command.startAddress, command.endAddress);
            break;
            
        default:
            // Invalid invalidation command
            generateEvent(EventType::CONFIGURATION_ERROR, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
            break;
    }
    
    // Generate completion event for invalidation commands
    generateEvent(EventType::ATC_INVALIDATE_COMPLETION, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
}

void SMMU::executeTLBInvalidationCommand(CommandType type, StreamID streamID, PASID pasid) {
    // ARM SMMU v3 spec: Execute TLB-specific invalidation commands
    switch (type) {
        case CommandType::TLBI_NH_ALL:
            // TLB invalidation non-secure hyp all
            invalidateTranslationCache();
            break;
            
        case CommandType::TLBI_EL2_ALL:
            // TLB invalidation EL2 all
            invalidateTranslationCache();
            break;
            
        case CommandType::TLBI_S12_VMALL:
            // TLB invalidation stage 1&2 VM all
            if (streamID != 0) {
                invalidateStreamCache(streamID);
            } else {
                invalidateTranslationCache();
            }
            break;
            
        default:
            // Not a TLB invalidation command
            generateEvent(EventType::CONFIGURATION_ERROR, streamID, pasid, 0, SecurityState::NonSecure);
            break;
    }
}

void SMMU::executeATCInvalidationCommand(StreamID streamID, PASID pasid, IOVA startAddr, IOVA endAddr) {
    // ARM SMMU v3 spec: Execute Address Translation Cache invalidation
    
    if (tlbCache) {
        if (startAddr == 0 && endAddr == 0) {
            // Global invalidation for stream/PASID
            if (pasid != 0) {
                invalidatePASIDCache(streamID, pasid);
            } else {
                invalidateStreamCache(streamID);
            }
        } else {
            // Range-specific invalidation
            // ARM SMMU v3 spec: Invalidate specific address range
            IOVA currentAddr = startAddr & ~PAGE_MASK; // Page-align start
            IOVA alignedEndAddr = (endAddr + PAGE_SIZE - 1) & ~PAGE_MASK; // Page-align end
            
            // Invalidate each page in the range
            while (currentAddr <= alignedEndAddr && currentAddr >= startAddr) {
                tlbCache->invalidate(streamID, pasid, currentAddr);
                currentAddr += PAGE_SIZE;
                
                // Prevent infinite loop on address overflow
                if (currentAddr < startAddr) {
                    break;
                }
            }
        }
    }
}

// Task 5.3: Helper Methods
void SMMU::processCommand(const CommandEntry& command) {
    // ARM SMMU v3 spec: Process individual command based on type
    switch (command.type) {
        case CommandType::PREFETCH_CONFIG:
            // Configuration prefetch - ARM SMMU v3 optimization
            // Could implement stream table entry prefetching
            break;
            
        case CommandType::PREFETCH_ADDR:
            // Address prefetch - ARM SMMU v3 optimization
            // Could implement translation prefetching for specific addresses
            break;
            
        case CommandType::CFGI_STE:
        case CommandType::CFGI_ALL:
        case CommandType::TLBI_NH_ALL:
        case CommandType::TLBI_EL2_ALL:
        case CommandType::TLBI_S12_VMALL:
        case CommandType::ATC_INV:
            // Cache invalidation commands
            executeInvalidationCommand(command);
            break;
            
        case CommandType::PRI_RESP:
            // Page Request Interface response
            // ARM SMMU v3 spec: Handle PRI response completion
            // Response processing is handled by PRI queue mechanism
            break;
            
        case CommandType::RESUME:
            // Resume processing command
            // ARM SMMU v3 spec: Resume stalled transactions
            // Could implement transaction restart logic
            break;
            
        case CommandType::SYNC:
            // Synchronization barrier
            // ARM SMMU v3 spec: Ensure command ordering and completion
            // Handled in processCommandQueue()
            break;
            
        default:
            // Unknown command type
            generateEvent(EventType::CONFIGURATION_ERROR, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
            break;
    }
}

void SMMU::generateEvent(EventType type, StreamID streamID, PASID pasid, IOVA address, SecurityState securityState) {
    // ARM SMMU v3 spec: Generate event for event queue processing
    
    // Check event queue capacity
    if (eventQueue.size() >= maxEventQueueSize) {
        // Event queue full - drop oldest event
        eventQueue.pop_front();
    }
    
    // Create new event
    EventEntry event;
    event.type = type;
    event.streamID = streamID;
    event.pasid = pasid;
    event.address = address;
    event.securityState = securityState;
    event.timestamp = getCurrentTimestamp();
    
    // ARM SMMU v3 spec: Set appropriate error codes
    switch (type) {
        case EventType::TRANSLATION_FAULT:
            event.errorCode = 0x01; // Translation fault error code
            break;
        case EventType::PERMISSION_FAULT:
            event.errorCode = 0x02; // Permission fault error code
            break;
        case EventType::CONFIGURATION_ERROR:
            event.errorCode = 0x10; // Configuration error code
            break;
        case EventType::INTERNAL_ERROR:
            event.errorCode = 0xFF; // Internal error code
            break;
        default:
            event.errorCode = 0x00; // Success/info event
            break;
    }
    
    // Add to event queue
    eventQueue.push_back(event);
}

uint64_t SMMU::getCurrentTimestamp() const {
    // ARM SMMU v3 spec: Generate timestamp for event ordering and aging
    return std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
}

// SecurityState helper methods
void SMMU::recordSecurityFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState expectedState, SecurityState actualState) {
    (void)expectedState; // Suppress unused parameter warning - reserved for future enhanced security logging
    
    // Create specialized security fault record
    FaultRecord fault;
    fault.streamID = streamID;
    fault.pasid = pasid;
    fault.address = iova;
    fault.faultType = FaultType::SecurityFault;
    fault.accessType = accessType;
    fault.securityState = actualState;  // Record the actual (violating) state
    fault.timestamp = getCurrentTimestamp();
    
    recordFault(fault);
    
    // Generate security event for monitoring
    generateEvent(EventType::CONFIGURATION_ERROR, streamID, pasid, iova, actualState);
}

bool SMMU::validateSecurityState(SecurityState requestedState, SecurityState contextState) const {
    // ARM SMMU v3 spec: Security state validation logic
    // NonSecure can only access NonSecure resources
    // Secure can access both Secure and NonSecure resources
    // Realm has its own isolated context
    
    switch (requestedState) {
        case SecurityState::NonSecure:
            return contextState == SecurityState::NonSecure;
            
        case SecurityState::Secure:
            return (contextState == SecurityState::Secure || contextState == SecurityState::NonSecure);
            
        case SecurityState::Realm:
            return contextState == SecurityState::Realm;
            
        default:
            return false;
    }
}

SecurityState SMMU::determineContextSecurityState(StreamID streamID, PASID pasid) const {
    (void)pasid; // Suppress unused parameter warning - reserved for future PASID-specific security state logic
    
    // ARM SMMU v3 spec: Determine security state for stream/PASID context
    // In this implementation, we default to NonSecure unless configured otherwise
    // A real implementation would consult stream configuration tables
    
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return SecurityState::NonSecure;  // Default for unconfigured streams
    }
    
    // For now, return NonSecure as default
    // In a complete implementation, this would check stream table entries
    // and PASID configuration to determine the appropriate security state
    return SecurityState::NonSecure;
}

// ARM SMMU v3 Comprehensive Fault Syndrome Generation Methods

FaultSyndrome SMMU::generateFaultSyndrome(FaultType faultType, FaultStage stage, AccessType accessType, 
                                         SecurityState securityState, uint8_t faultLevel, 
                                         PrivilegeLevel privLevel, uint16_t contextDescIndex) const {
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware fault syndrome generation
    
    // Generate ARM SMMU v3 compliant fault syndrome
    bool writeAccess = (accessType == AccessType::Write);
    bool instructionFetch = (accessType == AccessType::Execute);
    
    // Encode the syndrome register according to ARM SMMU v3 specification
    uint32_t syndromeRegister = encodeFaultSyndromeRegister(faultType, stage, faultLevel, writeAccess, instructionFetch);
    
    // Classify the access type
    AccessClassification accessClass = classifyAccess(accessType);
    
    // Create comprehensive fault syndrome
    return FaultSyndrome(syndromeRegister, stage, faultLevel, privLevel, accessClass, writeAccess, contextDescIndex);
}

uint32_t SMMU::encodeFaultSyndromeRegister(FaultType faultType, FaultStage stage, uint8_t level, 
                                          bool writeAccess, bool instructionFetch) const {
    // ARM SMMU v3 fault syndrome register encoding
    uint32_t syndrome = 0;
    
    // Bits [5:0] - Fault Status Code (FSC)
    uint32_t fsc = 0;
    switch (faultType) {
        case FaultType::TranslationFault:
        case FaultType::Level0TranslationFault:
        case FaultType::Level1TranslationFault:
        case FaultType::Level2TranslationFault:
        case FaultType::Level3TranslationFault:
            fsc = 0x04 | (level & 0x03);  // Translation fault at level
            break;
        case FaultType::PermissionFault:
            fsc = 0x0C | (level & 0x03);  // Permission fault at level
            break;
        case FaultType::AddressSizeFault:
            fsc = 0x00;                   // Address size fault
            break;
        case FaultType::AccessFlagFault:
            fsc = 0x08 | (level & 0x03);  // Access flag fault at level
            break;
        case FaultType::DirtyBitFault:
            fsc = 0x30;                   // Dirty bit fault
            break;
        case FaultType::ExternalAbort:
        case FaultType::SynchronousExternalAbort:
            fsc = 0x10;                   // Synchronous external abort
            break;
        case FaultType::AsynchronousExternalAbort:
            fsc = 0x11;                   // Asynchronous external abort
            break;
        case FaultType::TLBConflictFault:
            fsc = 0x30;                   // TLB conflict resolution fault
            break;
        case FaultType::ContextDescriptorFormatFault:
        case FaultType::TranslationTableFormatFault:
        case FaultType::StreamTableFormatFault:
            fsc = 0x0A;                   // Format fault
            break;
        case FaultType::SecurityFault:
            fsc = 0x20;                   // Security fault
            break;
        default:
            fsc = 0x02;                   // Debug fault (catch-all)
            break;
    }
    syndrome |= (fsc & 0x3F);
    
    // Bit 6 - Write not Read (WnR)
    if (writeAccess) {
        syndrome |= (1 << 6);
    }
    
    // Bit 7 - Stage 2 fault (S2)
    if (stage == FaultStage::Stage2Only || stage == FaultStage::BothStages) {
        syndrome |= (1 << 7);
    }
    
    // Bit 8 - Instruction fetch (INST)
    if (instructionFetch) {
        syndrome |= (1 << 8);
    }
    
    // Bits [15:9] - Reserved (set to 0)
    
    // Bits [23:16] - Implementation defined
    syndrome |= (0x42 << 16);  // ARM SMMU v3 implementation signature
    
    // Bits [31:24] - Reserved (set to 0)
    
    return syndrome;
}

FaultStage SMMU::determineFaultStage(const StreamConfig& config, FaultType faultType) const {
    // Determine which translation stage caused the fault
    if (config.stage1Enabled && config.stage2Enabled) {
        // Both stages enabled - classify based on fault type
        switch (faultType) {
            case FaultType::ContextDescriptorFormatFault:
                return FaultStage::Stage1Only;
            case FaultType::Level0TranslationFault:
            case FaultType::Level1TranslationFault:
            case FaultType::Level2TranslationFault:
            case FaultType::Level3TranslationFault:
                // Could be either stage - default to Stage1
                return FaultStage::Stage1Only;
            default:
                return FaultStage::BothStages;
        }
    } else if (config.stage1Enabled) {
        return FaultStage::Stage1Only;
    } else if (config.stage2Enabled) {
        return FaultStage::Stage2Only;
    } else {
        return FaultStage::Unknown;
    }
}

PrivilegeLevel SMMU::determinePrivilegeLevel(AccessType accessType, SecurityState securityState) const {
    // Determine privilege level based on security state and access pattern
    if (securityState == SecurityState::Secure) {
        return PrivilegeLevel::EL3;  // Secure monitor level
    } else if (securityState == SecurityState::Realm) {
        return PrivilegeLevel::EL2;  // Realm management level
    } else {
        // NonSecure state - determine based on access type
        if (accessType == AccessType::Execute) {
            return PrivilegeLevel::EL0;  // User level execution
        } else {
            return PrivilegeLevel::EL1;  // Kernel level access
        }
    }
}

AccessClassification SMMU::classifyAccess(AccessType accessType) const {
    // Classify access type for syndrome generation
    switch (accessType) {
        case AccessType::Execute:
            return AccessClassification::InstructionFetch;
        case AccessType::Read:
        case AccessType::Write:
            return AccessClassification::DataAccess;
        default:
            return AccessClassification::Unknown;
    }
}

void SMMU::recordComprehensiveFault(StreamID streamID, PASID pasid, IOVA iova, FaultType faultType, 
                                   AccessType accessType, SecurityState securityState, FaultStage stage,
                                   uint8_t faultLevel, uint16_t contextDescIndex) {
    // Generate comprehensive ARM SMMU v3 fault syndrome
    PrivilegeLevel privLevel = determinePrivilegeLevel(accessType, securityState);
    FaultSyndrome syndrome = generateFaultSyndrome(faultType, stage, accessType, securityState, 
                                                  faultLevel, privLevel, contextDescIndex);
    
    // Create comprehensive fault record
    FaultRecord fault(streamID, pasid, iova, faultType, accessType, securityState, syndrome);
    fault.timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
    
    // Record the fault through the fault handler
    recordFault(fault);
}

FaultType SMMU::classifyDetailedTranslationFault(IOVA iova, uint8_t tableLevel, bool formatError) const {
    // Classify detailed translation faults based on context
    if (formatError) {
        return FaultType::TranslationTableFormatFault;
    }
    
    // Classify by translation table level
    switch (tableLevel) {
        case 0:
            return FaultType::Level0TranslationFault;
        case 1:
            return FaultType::Level1TranslationFault;
        case 2:
            return FaultType::Level2TranslationFault;
        case 3:
            return FaultType::Level3TranslationFault;
        default:
            // Check for address size constraints
            const uint64_t MAX_48BIT_ADDRESS = 0x0000FFFFFFFFFFFFULL;
            if (iova > MAX_48BIT_ADDRESS) {
                return FaultType::AddressSizeFault;
            }
            return FaultType::TranslationFault;
    }
}

// Configuration management methods

const SMMUConfiguration& SMMU::getConfiguration() const {
    return configuration;
}

VoidResult SMMU::updateConfiguration(const SMMUConfiguration& config) {
    std::lock_guard<std::mutex> lock(sMMUMutex);
    
    // Validate the configuration
    VoidResult validationResult = validateConfigurationUpdate(config);
    if (!validationResult.isOk()) {
        return validationResult;
    }
    
    // Store old configuration for potential rollback
    SMMUConfiguration oldConfig = configuration;
    
    try {
        configuration = config;
        
        // Apply the new configuration
        applyConfiguration();
        
        return makeVoidSuccess();
        
    } catch (const std::exception&) {
        // Rollback on failure
        configuration = oldConfig;
        return makeVoidError(SMMUError::ConfigurationError);
    }
}

VoidResult SMMU::updateQueueConfiguration(const QueueConfiguration& queueConfig) {
    std::lock_guard<std::mutex> lock(sMMUMutex);
    
    if (!queueConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Update the configuration and apply changes
    VoidResult result = configuration.setQueueConfiguration(queueConfig);
    if (result.isOk()) {
        maxEventQueueSize = queueConfig.eventQueueSize;
        maxCommandQueueSize = queueConfig.commandQueueSize;
        maxPRIQueueSize = queueConfig.priQueueSize;
        
        // Trim queues if they exceed new limits
        while (eventQueue.size() > maxEventQueueSize) {
            eventQueue.pop_front();
        }
        while (commandQueue.size() > maxCommandQueueSize) {
            commandQueue.pop_front();
        }
        while (priQueue.size() > maxPRIQueueSize) {
            priQueue.pop_front();
        }
    }
    
    return result;
}

VoidResult SMMU::updateCacheConfiguration(const CacheConfiguration& cacheConfig) {
    std::lock_guard<std::mutex> lock(sMMUMutex);
    
    if (!cacheConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Update the configuration
    VoidResult result = configuration.setCacheConfiguration(cacheConfig);
    if (result.isOk()) {
        // Update cache settings
        cachingEnabled = cacheConfig.enableCaching;
        
        // Update TLB cache size if changed
        if (tlbCache->getCapacity() != cacheConfig.tlbCacheSize) {
            tlbCache->setMaxSize(cacheConfig.tlbCacheSize);
        }
    }
    
    return result;
}

VoidResult SMMU::updateAddressConfiguration(const AddressConfiguration& addressConfig) {
    std::lock_guard<std::mutex> lock(sMMUMutex);
    
    if (!addressConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Update the configuration
    return configuration.setAddressConfiguration(addressConfig);
}

VoidResult SMMU::updateResourceLimits(const ResourceLimits& resourceLimits) {
    std::lock_guard<std::mutex> lock(sMMUMutex);
    
    if (!resourceLimits.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Update the configuration
    return configuration.setResourceLimits(resourceLimits);
}

// Configuration helper methods
void SMMU::applyConfiguration() {
    // Apply queue configuration
    const QueueConfiguration& queueConfig = configuration.getQueueConfiguration();
    maxEventQueueSize = queueConfig.eventQueueSize;
    maxCommandQueueSize = queueConfig.commandQueueSize;
    maxPRIQueueSize = queueConfig.priQueueSize;
    
    // Apply cache configuration
    const CacheConfiguration& cacheConfig = configuration.getCacheConfiguration();
    cachingEnabled = cacheConfig.enableCaching;
    
    // Update TLB cache size if changed
    if (tlbCache->getCapacity() != cacheConfig.tlbCacheSize) {
        tlbCache->setMaxSize(cacheConfig.tlbCacheSize);
    }
    
    // Trim queues if they exceed new limits
    while (eventQueue.size() > maxEventQueueSize) {
        eventQueue.pop_front();
    }
    while (commandQueue.size() > maxCommandQueueSize) {
        commandQueue.pop_front();
    }
    while (priQueue.size() > maxPRIQueueSize) {
        priQueue.pop_front();
    }
}

VoidResult SMMU::validateConfigurationUpdate(const SMMUConfiguration& config) const {
    if (!config.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Additional validation specific to current SMMU state
    const QueueConfiguration& queueConfig = config.getQueueConfiguration();
    
    // Check if new queue sizes would cause data loss
    if (queueConfig.eventQueueSize < eventQueue.size()) {
        // This is a warning, not an error - we'll trim the queue
    }
    if (queueConfig.commandQueueSize < commandQueue.size()) {
        // This is a warning, not an error - we'll trim the queue
    }
    if (queueConfig.priQueueSize < priQueue.size()) {
        // This is a warning, not an error - we'll trim the queue
    }
    
    return makeVoidSuccess();
}

} // namespace smmu