// ARM SMMU v3 Main Controller Implementation
// Copyright (c) 2024 John Greninger
// Enhanced with Task 5.2: Translation Engine

#include "smmu/smmu.h"
#include <chrono>
#include <algorithm>

namespace smmu {

// ARM §3.5.1: Circular queue index helpers (FINDING-M-01)

// Compute LOG2SIZE: smallest k such that 2^k >= capacity (minimum 0).
static uint32_t computeLog2Size(size_t capacity) {
    if (capacity <= 1) return 0;
    uint32_t k = 0;
    size_t n = 1;
    while (n < capacity) {
        n <<= 1;
        ++k;
    }
    return k;
}

// Advance a circular index by 1 per ARM §3.5.1.
// The index cycles 0..2^(log2size+1)-1 then wraps to 0.
static uint32_t advanceQueueIndex(uint32_t idx, uint32_t log2size) {
    uint32_t modulus = 2u << log2size; // 2^(log2size+1)
    return (idx + 1) % modulus;
}

// Compute number of occupied entries.
static uint32_t queueOccupied(uint32_t prod, uint32_t cons, uint32_t log2size) {
    uint32_t modulus = 2u << log2size;
    return (prod - cons + modulus) % modulus;
}

// Default constructor - Initialize SMMU with default configuration
SMMU::SMMU()
    : faultHandler(std::shared_ptr<FaultHandler>(new FaultHandler())),
      tlbCache(std::unique_ptr<TLBCache>(new TLBCache(SMMUConfiguration::createDefault().getCacheConfiguration().tlbCacheSize))),
      configuration(SMMUConfiguration::createDefault()),
      globalFaultMode(FaultMode::Terminate),
      cachingEnabled(configuration.getCacheConfiguration().enableCaching),
      translationCount(0),
      cacheHits(0),
      cacheMisses(0),
      // Task 5.3: Initialize event and command processing queues using configuration
      maxEventQueueSize(configuration.getQueueConfiguration().eventQueueSize),
      maxCommandQueueSize(configuration.getQueueConfiguration().commandQueueSize),
      maxPRIQueueSize(configuration.getQueueConfiguration().priQueueSize),
      // FINDING-M-01: ARM §3.5.1 circular queue PROD/CONS indices
      cmdqLog2Size(computeLog2Size(configuration.getQueueConfiguration().commandQueueSize)),
      eventqLog2Size(computeLog2Size(configuration.getQueueConfiguration().eventQueueSize)),
      priqLog2Size(computeLog2Size(configuration.getQueueConfiguration().priQueueSize)),
      cmdqProd(0),
      cmdqCons(0),
      eventqProd(0),
      eventqCons(0),
      priqProd(0),
      priqCons(0),
      gerrorStatus(0),
      cr0_(0),
      smmuen_(false),
      gbpaAbort_(false),
      strtabLog2Size_(32),
      stagCounter_(0) {
    // Initialize empty stream map - streams will be added via configureStream
    // ARM SMMU v3 spec: Controller starts in disabled state with no streams configured

    // Task 5.3: Initialize empty queues for event and command processing
    eventQueue.clear();
    commandQueue.clear();
    priQueue.clear();
}

// Constructor with custom configuration
SMMU::SMMU(const SMMUConfiguration& config)
    : faultHandler(std::shared_ptr<FaultHandler>(new FaultHandler())),
      tlbCache(std::unique_ptr<TLBCache>(new TLBCache(config.getCacheConfiguration().tlbCacheSize))),
      configuration(config),
      globalFaultMode(FaultMode::Terminate),
      cachingEnabled(config.getCacheConfiguration().enableCaching),
      translationCount(0),
      cacheHits(0),
      cacheMisses(0),
      // Task 5.3: Initialize event and command processing queues using configuration
      maxEventQueueSize(config.getQueueConfiguration().eventQueueSize),
      maxCommandQueueSize(config.getQueueConfiguration().commandQueueSize),
      maxPRIQueueSize(config.getQueueConfiguration().priQueueSize),
      // FINDING-M-01: ARM §3.5.1 circular queue PROD/CONS indices
      cmdqLog2Size(computeLog2Size(config.getQueueConfiguration().commandQueueSize)),
      eventqLog2Size(computeLog2Size(config.getQueueConfiguration().eventQueueSize)),
      priqLog2Size(computeLog2Size(config.getQueueConfiguration().priQueueSize)),
      cmdqProd(0),
      cmdqCons(0),
      eventqProd(0),
      eventqCons(0),
      priqProd(0),
      priqCons(0),
      gerrorStatus(0),
      cr0_(0),
      smmuen_(false),
      gbpaAbort_(false),
      strtabLog2Size_(32),
      stagCounter_(0) {
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
    // Optimization 3: Cache timestamp once at start for reuse
    uint64_t currentTime = std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();

    // Optimization 4: Update translation statistics with relaxed memory ordering
    translationCount.fetch_add(1, std::memory_order_relaxed);

    // §6.3.9 SMMUEN=0: bypass or abort depending on SMMU_GBPA.ABORT (§3.11, §13.2).
    // FINDING-NEW-01 / FINDING-NEW-09: check SMMUEN before TLB / stream lookup.
    if (!smmuen_) {
        if (gbpaAbort_) {
            // GBPA.ABORT=1: abort all transactions — no identity mapping, no fault event.
            return makeTranslationError(SMMUError::GbpaAbort);
        }
        // GBPA.ABORT=0: bypass — identity mapping (PA == IOVA), no fault.
        // §3.4: OAS check for GBPA bypass. If IOVA >= OAS, abort silently (no event per spec).
        {
            uint64_t oasBits = configuration.getAddressConfiguration().maxPASize;
            if (oasBits < 64 && iova >= (static_cast<IOVA>(1) << oasBits)) {
                return makeTranslationError(SMMUError::InvalidAddress);
            }
        }
        PagePermissions allPerms;
        allPerms.read = true;
        allPerms.write = true;
        allPerms.execute = true;
        return makeTranslationSuccess(static_cast<PA>(iova), allPerms, securityState);
    }

    // BUG-15: StreamID is uint32_t and MAX_STREAM_ID is 0xFFFFFFFF, so
    // streamID > MAX_STREAM_ID is always false — check removed.

    // §6.3.4 / CT-04: SMMU_STRTAB_BASE_CFG.LOG2SIZE StreamID range check.
    // When strtabLog2Size_ < 32, reject StreamIDs >= 2^strtabLog2Size_.
    if (strtabLog2Size_ < 32u) {
        uint32_t limit = 1u << strtabLog2Size_;
        if (streamID >= limit) {
            generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidStreamID);
        }
    }

    // Task 5.2: Optimized fast path - Check TLB cache first for maximum performance
    if (cachingEnabled && tlbCache) {
        // Performance optimization: Use lookupEntry() which returns TLBEntry by value (thread-safe)
        IOVA pageAlignedIOVA = iova & ~PAGE_MASK;
        Result<TLBEntry> entryResult = tlbCache->lookupEntry(streamID, pasid, pageAlignedIOVA, securityState);

        if (entryResult.isOk()) {
            const TLBEntry& entry = entryResult.getValue();
            if (entry.valid) {
                // ARM §3.16 / FINDING-NEW-37: TLB entries are valid until an explicit
                // TLBI command.  No time-based eviction is performed here.
                // Cache hit - validate access permissions against requested access type
                // §3.12.2 / FINDING-NEW-25: On permission failure, fall through to the
                // slow path so that FaultMode::Stall is correctly applied.  The slow
                // path (performTwoStageTranslation) re-checks permissions via the page
                // table and applies the stall-mode queue if required.
                if (validateAccessPermissions(entry.permissions, accessType)) {
                    // TLBCache already recorded hit statistics
                    // No need for additional recordCacheHit() here

                    PA finalPA = entry.physicalAddress + (iova & PAGE_MASK);
                    TranslationData data(finalPA, entry.permissions, entry.securityState);
                    return TranslationResult(data);
                }
                // Permission failure: fall through to slow path for stall-mode handling.
            }
        }
        // Cache miss - TLBCache already recorded miss statistics
        // No need for additional recordCacheMiss() here
    }

    // BUG-01 fix: Use unique_lock so we can release the stripe lock before
    // calling StreamContext methods (which acquire contextMutex).  Holding
    // the stripe lock while waiting for contextMutex creates a lock-ordering
    // hazard.  Established invariant: stripe_lock is NEVER held when
    // contextMutex is acquired. The raw StreamContext* remains valid after
    // unlock because removeStream() also acquires the same stripe lock before
    // erasing from streamMap, so it cannot delete the object concurrently
    // while we hold the lock.
    size_t stripe = getStreamStripe(streamID);
    std::unique_lock<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        lock.unlock();
        // §7.3.3: StreamID not in stream table → C_BAD_STREAMID (0x02), not F_TRANSLATION.
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::BadStreamID;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = currentTime;
        recordFault(fault);
        generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
        return makeTranslationError(SMMUError::StreamNotConfigured);
    }

    StreamContext* streamContext = streamIt->second.get();

    // BUG-CPP-02 fix: Do NOT release the stripe lock before performTwoStageTranslation.
    // A concurrent reset() acquires ALL stripe locks before clearing streamMap; keeping
    // our stripe lock held prevents reset() from destroying the StreamContext while we
    // still hold a raw pointer to it.  The lock is released only after all accesses to
    // streamContext are complete (before handleTranslationFailure, which would re-acquire
    // the same stripe lock via determineContextSecurityState and deadlock).
    // Note: performTwoStageTranslation and the cache/stall result code below do NOT
    // attempt to re-acquire this stripe lock, so there is no self-deadlock risk here.

    // Task 5.2: Enhanced two-stage translation with comprehensive error handling
    TranslationResult result = performTwoStageTranslation(streamID, pasid, iova, accessType, securityState, streamContext, currentTime);

    // Task 5.2: Cache successful translations for future lookups
    if (result.isOk() && isTranslationCacheable(result) && cachingEnabled && tlbCache) {
        StreamConfig streamCfg = streamContext->getStreamConfiguration();
        cacheTranslationResult(streamID, pasid, iova, result, currentTime, streamCfg.asid, streamCfg.vmid);
    } else if (result.isError()) {
        // ARM §3.12.2: Check per-stream stall mode before standard fault handling.
        // If the stream is configured for stall, enqueue a StallRecord and return
        // Stalled instead of the original error — software must issue CMD_RESUME.
        bool inStallMode = (streamContext->getFaultMode() == FaultMode::Stall);
        if (inStallMode) {
            // §3.12.2 / FINDING-NEW-26: STAG=0 is reserved; skip it so that
            // EventEntry.stag is always non-zero for genuine stall events.
            // BUG-CPP-03 fix: Allocate STAG under stallQueueMutex_ and check for
            // wrap-around collisions with existing active entries.  After 65535 stalls
            // the 16-bit counter wraps and the new STAG could alias a live entry.
            // Keep incrementing (under the lock) until a free slot is found, with a
            // safety limit of 65535 iterations to avoid an infinite loop when the
            // queue is completely full.
            uint16_t stag = 0;
            StallRecord record(0, streamID, pasid, iova, accessType, securityState, currentTime);
            {
                std::lock_guard<std::mutex> slock(stallQueueMutex_);
                static constexpr int MAX_STAG_TRIES = 65535;
                int tries = 0;
                do {
                    stag = stagCounter_.fetch_add(1, std::memory_order_relaxed);
                    ++tries;
                } while ((stag == 0 || stallQueue_.count(stag) != 0) && tries < MAX_STAG_TRIES);
                record = StallRecord(stag, streamID, pasid, iova, accessType, securityState, currentTime);
                stallQueue_[stag] = record;
            }
            // ARM §7.3 / FINDING-NEW-13: Derive the correct EventType from the actual
            // error so the OS fault handler receives the right fault classification.
            // Mirrors the mapping in handleTranslationFailure().
            EventType stallEventType;
            switch (result.getError()) {
                case SMMUError::PagePermissionViolation:
                    stallEventType = EventType::F_PERMISSION;
                    break;
                case SMMUError::InvalidAddress:
                    stallEventType = EventType::F_ADDR_SIZE;
                    break;
                case SMMUError::PageNotMapped:
                default:
                    stallEventType = EventType::F_TRANSLATION;
                    break;
            }
            generateEvent(stallEventType, streamID, pasid, iova, securityState, /*isStall=*/true, stag);
            return makeTranslationError(SMMUError::Stalled);
        }
        // BUG-CPP-02 fix: Release the stripe lock before calling handleTranslationFailure.
        // That function may call determineContextSecurityState which tries to acquire the
        // same stripe lock, which would deadlock.  All accesses to streamContext are
        // complete at this point.
        lock.unlock();
        handleTranslationFailure(streamID, pasid, iova, accessType, securityState, result, currentTime);
        return result;
    }

    return result;
}

// Stream management - Create and configure new stream with StreamContext
VoidResult SMMU::configureStream(StreamID streamID, const StreamConfig& config) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    // Check if stream already exists
    if (streamMap.find(streamID) != streamMap.end()) {
        // ARM §3.11: Changing a stream table entry requires CMD_CFGI_STE + CMD_SYNC
        // first.  Reject direct reconfiguration; caller must removeStream first.
        return makeVoidError(SMMUError::StreamAlreadyConfigured);
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
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
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
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    bool configured = streamMap.find(streamID) != streamMap.end();
    return Result<bool>(configured);
}

// Stream lifecycle control via StreamContext
VoidResult SMMU::enableStream(StreamID streamID) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
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
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
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
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
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

    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    return streamIt->second->createPASID(pasid);
}

// Remove PASID from stream
VoidResult SMMU::removeStreamPASID(StreamID streamID, PASID pasid) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
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

// Address size configuration (ARM §3.4.1 — TCR.T0SZ)
// Sets the per-context input address size for (streamID, pasid).
// Valid bit-widths: 32–52.  Out-of-range values are rejected.
VoidResult SMMU::setStreamInputAddressSize(StreamID streamID, PASID pasid, uint8_t bits) {
    if (bits < 32 || bits > 52) {
        return makeVoidError(SMMUError::InvalidAddress);
    }

    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }

    return streamIt->second->setAddressSpaceInputSize(pasid, bits);
}

// Per-stream per-PASID page operations
VoidResult SMMU::mapPage(StreamID streamID, PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    return streamIt->second->mapPage(pasid, iova, pa, permissions, securityState);
}

VoidResult SMMU::unmapPage(StreamID streamID, PASID pasid, IOVA iova) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    
    // Unmap page from stream context
    VoidResult result = streamIt->second->unmapPage(pasid, iova);

    // ARM SMMU v3 spec §4.4: TLB maintenance is the caller's responsibility via
    // explicit TLBI commands (CMD_TLBI_*).  unmapPage() must NOT auto-invalidate
    // the TLB; doing so prevents VMID/ASID-targeted invalidation tests from
    // distinguishing a hit (entry still in TLB) from a miss (TLBI evicted it).

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

    // Lock all stripes in order to prevent deadlock when iterating all streams
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }

    globalFaultMode = mode;

    // Apply to all configured streams
    VoidResult firstError = makeVoidSuccess();
    for (auto& streamPair : streamMap) {
        VoidResult result = streamPair.second->setFaultModeAtomic(mode);
        if (result.isError() && !firstError.isError()) {
            firstError = result;
        }
    }

    return firstError;
}

// Global caching enable/disable - Enhanced with TLBCache integration (Task 5.2)
VoidResult SMMU::enableCaching(bool enable) {
    // Lock all stripes in order to prevent deadlock when modifying global state
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }

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
    // BUG-02 fix: streamMap requires all stripe locks held for a consistent read.
    std::vector<std::unique_lock<std::mutex>> locks;
    locks.reserve(NUM_STREAM_STRIPES);
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }
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
    // BUG-25 fix: reset local cache-hit/miss counters that were previously left stale.
    cacheHits.store(0, std::memory_order_relaxed);
    cacheMisses.store(0, std::memory_order_relaxed);
    faultHandler->resetStatistics();
    
    // Task 5.2: Reset TLB cache statistics
    if (tlbCache) {
        tlbCache->resetStatistics();
    }
}

void SMMU::reset() {
    // Complete system reset - Enhanced with TLBCache reset (Task 5.2)
    // BUG-28 fix: acquire all stripe locks before clearing streamMap to prevent
    // use-after-free when a concurrent translate() holds a raw StreamContext*
    // pointer to an entry that reset() would otherwise delete unprotected.
    {
        std::vector<std::unique_lock<std::mutex>> locks;
        for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
            locks.emplace_back(streamLockStripes[i]);
        }
        streamMap.clear();
    }
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

    // ARM §6.3.17: Reset global error register (FINDING-M-06)
    gerrorStatus = 0;

    // ARM §6.3.9: Reset returns SMMU to disabled state (SMMUEN=0, GBPA.ABORT=0).
    smmuen_ = false;
    gbpaAbort_ = false;

    // ARM §3.12.2: Clear stall queue on reset.
    {
        std::lock_guard<std::mutex> slock(stallQueueMutex_);
        stallQueue_.clear();
    }
    stagCounter_.store(0, std::memory_order_relaxed);
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

void SMMU::setStreamVMID(StreamID streamID, uint16_t vmid) {
    // ARM §5.2: STE.S2VMID — VMID for Stage-2 TLB tagging and targeted invalidation.
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return; // Stream not found — silently ignore
    }
    StreamConfig cfg = streamIt->second->getStreamConfiguration();
    cfg.vmid = vmid;
    streamIt->second->updateConfiguration(cfg);
}

void SMMU::setStreamASID(StreamID streamID, uint16_t asid) {
    // ARM §3.17: CD.ASID — ASID for Stage-1 TLB tagging and targeted invalidation.
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return; // Stream not found — silently ignore
    }
    StreamConfig cfg = streamIt->second->getStreamConfiguration();
    cfg.asid = asid;
    streamIt->second->updateConfiguration(cfg);
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
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime) {
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
        fault.timestamp = currentTime;

        recordFault(fault);
        return makeTranslationError(SMMUError::StreamNotConfigured);
    }

    // ARM SMMU v3 spec: Get stream configuration to determine translation stages
    // Issue 3 fix: Retrieve config once and pass to stage-specific methods
    StreamConfig config = streamContext->getStreamConfiguration();

    TranslationResult result = makeTranslationError(SMMUError::InternalError);

    // ARM SMMU v3 spec: Handle different stage combinations
    if (!config.translationEnabled) {
        // §5.2 STE.Config==0b000 (disabled): PASID check first — a non-zero PASID on
        // a stream with no stage-1 translation aborts with C_BAD_SUBSTREAMID (§3.9 / §7.3.9),
        // regardless of whether the stream is disabled or bypass.
        if (pasid != 0) {
            FaultRecord fault;
            fault.streamID = streamID;
            fault.pasid = pasid;
            fault.address = iova;
            fault.faultType = FaultType::BadSubstreamId;
            fault.accessType = accessType;
            fault.securityState = securityState;
            fault.timestamp = currentTime;
            recordFault(fault);
            generateEvent(EventType::C_BAD_SUBSTREAMID, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidPASID);
        }

        // §5.2 STE.Config==0b000 (disabled): all translation stages absent and bypassEnabled
        // is false — transaction aborts silently with no event recorded (§7.3.7 last line).
        if (!config.bypassEnabled) {
            return makeTranslationError(SMMUError::StreamDisabled);
        }

        // §5.2 STE.Config==0b100 (bypass): PASID=0 — identity mapping (PA == IOVA).
        // §3.4: OAS check for STE-level bypass. If IOVA >= OAS, abort with F_ADDR_SIZE
        // (stage-1 address size fault per ARM IHI0070G.b §7.3.14).
        {
            uint64_t oasBits = configuration.getAddressConfiguration().maxPASize;
            if (oasBits < 64 && iova >= (static_cast<IOVA>(1) << oasBits)) {
                FaultRecord bypassOasFault;
                bypassOasFault.streamID = streamID;
                bypassOasFault.pasid = pasid;
                bypassOasFault.address = iova;
                bypassOasFault.faultType = FaultType::AddressSizeFault;
                bypassOasFault.accessType = accessType;
                bypassOasFault.securityState = securityState;
                bypassOasFault.timestamp = currentTime;
                recordFault(bypassOasFault);
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState);
                return makeTranslationError(SMMUError::InvalidAddress);
            }
        }
        PagePermissions bypassPerms(true, true, true); // Full permissions in bypass
        TranslationData data(iova, bypassPerms, securityState);
        return TranslationResult(data);
    }

    // §3.9 / §5.2 STE.S1DSS: When stage-1 is enabled and the stream is
    // substream-capable (S1CDMax > 0), non-substream transactions (PASID==0)
    // are handled according to STE.S1DSS before the normal CD[0] lookup.
    if (config.stage1Enabled && config.s1cdMax > 0 && pasid == 0) {
        if (config.s1dss == 0x00u) {
            // §7.3.7: S1DSS==0b00 — non-substream transaction on substream-capable
            // stream aborts with F_STREAM_DISABLED (event 0x06).
            FaultRecord s1dssFault;
            s1dssFault.streamID = streamID;
            s1dssFault.pasid = pasid;
            s1dssFault.address = iova;
            s1dssFault.faultType = FaultType::StreamDisabled;
            s1dssFault.accessType = accessType;
            s1dssFault.securityState = securityState;
            s1dssFault.timestamp = currentTime;
            recordFault(s1dssFault);
            generateEvent(EventType::F_STREAM_DISABLED, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::StreamDisabled);
        }
        if (config.s1dss == 0x01u) {
            // §3.9 S1DSS==0b01: bypass stage-1 for non-substream transactions.
            // OAS check applies per §3.4 (same as STE bypass).
            uint64_t oasBits = configuration.getAddressConfiguration().maxPASize;
            if (oasBits < 64 && iova >= (static_cast<IOVA>(1) << oasBits)) {
                FaultRecord oasFault;
                oasFault.streamID = streamID;
                oasFault.pasid = pasid;
                oasFault.address = iova;
                oasFault.faultType = FaultType::AddressSizeFault;
                oasFault.accessType = accessType;
                oasFault.securityState = securityState;
                oasFault.timestamp = currentTime;
                recordFault(oasFault);
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState);
                return makeTranslationError(SMMUError::InvalidAddress);
            }
            PagePermissions s1dssPerms(true, true, true);
            TranslationData s1dssData(iova, s1dssPerms, securityState);
            return TranslationResult(s1dssData);
        }
        // s1dss == 0b10: use CD[0] — fall through to normal stage-1 translation.
    }

    // §5.4 / CT-14: CD.AA64 validation — AArch32 LPAE mode is unsupported.
    // §5.4 / CT-13: CD.T0SZ/T1SZ validation — valid range 0-39 for SMMUv3.0.
    if (config.stage1Enabled) {
        if (!config.aa64) {
            // AA64=0 (VMSAv8-32 LPAE) is unsupported — C_BAD_CD (event 0x0A).
            generateEvent(EventType::C_BAD_CD, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }
        if (config.t0sz > 39u || config.t1sz > 39u) {
            // T0SZ or T1SZ out of range — C_BAD_CD (event 0x0A).
            generateEvent(EventType::C_BAD_CD, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }
    }

    if (config.stage1Enabled && config.stage2Enabled) {
        // Two-stage translation: IOVA -> IPA -> PA
        result = performBothStagesTranslation(streamID, pasid, iova, accessType, securityState, streamContext, config, currentTime);
    } else if (config.stage1Enabled && !config.stage2Enabled) {
        // Stage-1 only: IOVA -> PA directly
        result = performStage1OnlyTranslation(streamID, pasid, iova, accessType, securityState, streamContext, currentTime);
    } else if (!config.stage1Enabled && config.stage2Enabled) {
        // ARM §3.9: Stage-2-only stream — stage 1 is absent. A non-zero PASID has
        // no stage-1 context to consume it; abort with C_BAD_SUBSTREAMID.
        if (pasid != 0) {
            FaultRecord fault;
            fault.streamID = streamID;
            fault.pasid = pasid;
            fault.address = iova;
            fault.faultType = FaultType::BadSubstreamId;
            fault.accessType = accessType;
            fault.securityState = securityState;
            fault.timestamp = currentTime;
            recordFault(fault);
            generateEvent(EventType::C_BAD_SUBSTREAMID, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidPASID);
        }
        // Stage-2 only: IPA -> PA (IOVA = IPA)
        result = performStage2OnlyTranslation(streamID, pasid, iova, accessType, securityState, streamContext, currentTime);
    } else {
        // No stages enabled but translation enabled - configuration error
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::TranslationFault;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = currentTime;

        recordFault(fault);
        return makeTranslationError(SMMUError::ConfigurationError);
    }

    return result;
}

// Task 5.2: Translation caching helper methods
bool SMMU::isTranslationCacheable(const TranslationResult& result) const {
    // ARM SMMU v3 spec: Only cache successful, completed translations.
    // BUG-27 fix: removed spurious physicalAddress!=0 guard; PA=0 is a valid,
    // cacheable translation result per the ARM SMMU v3 specification.
    return !result.isError();
}

void SMMU::cacheTranslationResult(StreamID streamID, PASID pasid, IOVA iova,
                                 const TranslationResult& result, uint64_t currentTime,
                                 uint16_t asid, uint16_t vmid) {
    if (!tlbCache || result.isError() || !cachingEnabled) {
        return; // Caching disabled or invalid result
    }

    const TranslationData& data = result.getValue();

    // ARM SMMU v3 spec: Only cache page-aligned translations for efficiency
    IOVA pageAlignedIOVA = iova & ~PAGE_MASK; // Page-align the IOVA
    PA pageAlignedPA = data.physicalAddress & ~PAGE_MASK; // Page-align the PA

    // BUG-27 fix: removed spurious pageAlignedPA==0 guard; PA=0 is cacheable.

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
    entry.timestamp = currentTime;
    entry.asid = asid;
    entry.vmid = vmid;

    // ARM SMMU v3 spec: Insert into TLB with LRU eviction if needed
    tlbCache->insert(entry);
}

TranslationResult SMMU::lookupTranslationCache(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    if (!tlbCache || !cachingEnabled) {
        return makeTranslationError(SMMUError::CacheOperationFailed); // Failed result - caching disabled
    }
    
    // ARM SMMU v3 spec: Perform optimized TLB lookup with page alignment
    IOVA pageAlignedIOVA = iova & ~PAGE_MASK; // Page-align the IOVA for lookup
    
    // BUG-26 fix: use lookupEntry() which returns TLBEntry by value, eliminating the
    // use-after-free that occurred when lookup() returned a raw TLBEntry* whose
    // backing list node could be destroyed by a concurrent insert/invalidate after
    // the stripe lock inside lookup() was released.
    Result<TLBEntry> lookupResult = tlbCache->lookupEntry(streamID, pasid, pageAlignedIOVA, securityState);
    if (lookupResult.isError() || !lookupResult.getValue().valid) {
        return makeTranslationError(SMMUError::CacheEntryNotFound); // Cache miss
    }

    const TLBEntry entry = lookupResult.getValue();

    // Security state validation
    if (entry.securityState != securityState) {
        return makeTranslationError(FaultType::SecurityFault);
    }

    // ARM §3.16 / FINDING-NEW-37: TLB entries are valid until an explicit TLBI.
    // No time-based eviction is performed; the entry is unconditionally valid.

    // Convert TLBEntry back to TranslationResult with page offset preservation
    PA finalPhysicalAddress = entry.physicalAddress + (iova & PAGE_MASK); // Add back page offset
    return makeTranslationSuccess(finalPhysicalAddress, entry.permissions, entry.securityState);
}

void SMMU::generateCacheKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState, uint64_t& cacheKey) const {
    // Generate a unique cache key combining StreamID, PASID, IOVA, and SecurityState
    // This is used internally by TLBCache, but provided for completeness
    // BUG-16 fix: previous layout had securityState<<20 overlapping with
    // pasid<<12 in bits [21:20].  Collision-free layout:
    //   bits [63:32] = streamID      (32 bits)
    //   bits [31:12] = pasid         (20 bits, max 0xFFFFF)
    //   bits [11:10] = securityState (2 bits; values 0, 1, 2)
    //   bits [9:0]   = iova low      (10 bits)
    cacheKey = (static_cast<uint64_t>(streamID) << 32) |
               (static_cast<uint64_t>(pasid) << 12) |
               (static_cast<uint64_t>(securityState) << 10) |
               (iova & 0x3FFULL);
}

// Task 5.2: Enhanced stage-specific translation methods
TranslationResult SMMU::performBothStagesTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                    AccessType accessType, SecurityState securityState, StreamContext* streamContext, const StreamConfig& config, uint64_t currentTime) {
    // ARM SMMU v3 spec: Two-stage translation IOVA -> IPA -> PA
    // This provides comprehensive coordination between Stage-1 and Stage-2 translations
    // with proper fault handling and permission intersection

    // Issue 3 fix: config is passed from performTwoStageTranslation, no redundant getStreamConfiguration()

    // Validate that both stages are properly configured
    if (!config.stage1Enabled || !config.stage2Enabled) {
        // Configuration error - both stages should be enabled for this method
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::BothStages, currentTime, 0, 0);
        return makeTranslationError(SMMUError::ConfigurationError);
    }

    // Issue 2 fix: Use getPASIDAddressSpaceUnlocked via the public getPASIDAddressSpace
    // since we don't hold contextMutex here. The stage methods use translate() which
    // acquires contextMutex internally, so we use the locked variant for safety.
    // Stage 1: IOVA -> IPA translation (using per-PASID address space)
    AddressSpace* stage1AddressSpace = streamContext->getPASIDAddressSpace(pasid);
    if (!stage1AddressSpace) {
        // PASID not configured - Stage-1 translation fault
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage1Only, currentTime, 0, 0);
        return makeTranslationError(SMMUError::PASIDNotFound);
    }
    
    // Perform Stage-1 translation: IOVA -> IPA
    TranslationResult stage1Result = stage1AddressSpace->translatePage(iova, accessType, securityState);
    if (stage1Result.isError()) {
        // Stage-1 translation failed - record fault with comprehensive syndrome
        // ARM SMMU v3 spec Section 7.3.2: Use level-specific fault classification
        FaultType faultType;
        if (stage1Result.getError() == SMMUError::PageNotMapped) {
            // Use level-specific fault classification (ARM SMMU v3 Section 7.3.2)
            faultType = classifyDetailedTranslationFault(iova, 1, false);
        } else {
            faultType = FaultType::AccessFault;
        }
        recordComprehensiveFault(streamID, pasid, iova, faultType,
                               accessType, securityState, FaultStage::Stage1Only, currentTime, 1, 0);
        return stage1Result;
    }

    // Stage 1 success - IPA is now in stage1Result.physicalAddress
    IPA intermediatePA = stage1Result.getValue().physicalAddress;

    // BUG-27 fix: removed spurious IPA==0 guard; IPA=0 is a valid intermediate
    // address — Stage-2 will look it up and fail normally if not mapped.

    // Stage 2: IPA -> PA translation using the stream's dedicated Stage-2 address space.
    // BUG-07 fix: Stage-2 has its own address space set via setStage2AddressSpace().
    // The previous code incorrectly used getPASIDAddressSpace(0) (a Stage-1 per-PASID
    // space) instead of the separate Stage-2 hypervisor address space.
    AddressSpace* stage2AddressSpace = streamContext->getStage2AddressSpace();
    if (!stage2AddressSpace) {
        // Stage-2 address space not configured - Stage-2 translation fault
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage2Only, currentTime, 0, 0);
        return makeTranslationError(SMMUError::AddressSpaceExhausted);
    }
    // ARM §5.2: When stage-2 AS is the same object as stage-1 AS (aliased via createStreamPASID
    // PASID-0 auto-link), the IPA from stage-1 cannot be looked up in stage-2 (IOVA→IPA mapping,
    // not IPA→PA).  Treat stage-1 result as the final translation (identity stage-2 semantics).
    if (stage2AddressSpace == stage1AddressSpace) {
        return stage1Result;
    }
    
    // Perform Stage-2 translation: IPA -> PA
    // ARM SMMU v3 spec: Stage-2 translates the IPA from Stage-1 to final PA
    TranslationResult stage2Result = stage2AddressSpace->translatePage(intermediatePA, accessType, securityState);
    if (stage2Result.isError()) {
        // ARM SMMU v3 spec Section 7.3.3: Stage-2 fault attribution
        FaultType stage2FaultType;
        if (stage2Result.getError() == SMMUError::PageNotMapped) {
            // Use level-specific fault classification
            stage2FaultType = classifyDetailedTranslationFault(intermediatePA, 1, false);
        } else {
            stage2FaultType = FaultType::Stage2PermissionFault;
        }

        // ARM SMMU v3 spec: Stage-2 faults use PASID 0 (hypervisor) and IPA as fault address
        recordComprehensiveFault(streamID, 0, intermediatePA, stage2FaultType,
                               accessType, securityState, FaultStage::Stage2Only, currentTime, 1, 0);
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
                               accessType, securityState, FaultStage::BothStages, currentTime, 2, 0);
        return makeTranslationError(SMMUError::PagePermissionViolation);
    }

    // ARM SMMU v3 spec: Validate security state consistency across both stages
    if (stage1Data.securityState != stage2Data.securityState) {
        // Security state inconsistency between stages
        recordComprehensiveFault(streamID, pasid, iova, FaultType::SecurityFault,
                               accessType, securityState, FaultStage::BothStages, currentTime, 0, 0);
        return makeTranslationError(SMMUError::InvalidSecurityState);
    }

    // ARM SMMU v3 spec: Final security state validation - use stage2 security state as reference
    if (!validateSecurityState(securityState, stage2Data.securityState)) {
        // Security state violation
        recordComprehensiveFault(streamID, pasid, iova, FaultType::SecurityFault,
                               accessType, securityState, FaultStage::BothStages, currentTime, 0, 0);
        return makeTranslationError(SMMUError::InvalidSecurityState);
    }
    
    // Create successful final translation result
    return makeTranslationSuccess(stage2Data.physicalAddress, finalPermissions, stage2Data.securityState);
}

TranslationResult SMMU::performStage1OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                    AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime) {
    // ARM SMMU v3 spec: Stage-1 only translation IOVA -> PA
    TranslationResult result = streamContext->translate(pasid, iova, accessType, securityState);

    // Record fault if translation failed
    if (result.isError()) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        // Classify fault type based on error code
        switch (result.getError()) {
            case SMMUError::PageNotMapped:
                fault.faultType = FaultType::TranslationFault;
                break;
            case SMMUError::PagePermissionViolation:
                fault.faultType = FaultType::PermissionFault;
                break;
            case SMMUError::InvalidSecurityState:
                fault.faultType = FaultType::SecurityFault;
                break;
            case SMMUError::InvalidAddress:
                // ARM §3.4.1: IOVA exceeded the per-context input address size.
                // BUG-CPP-04 fix: Emit F_ADDR_SIZE here (in the stage-specific path) so
                // that handleTranslationFailure does not need to emit it again.
                fault.faultType = FaultType::AddressSizeFault;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState);
                break;
            default:
                fault.faultType = FaultType::AccessFault;
                break;
        }
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = currentTime;

        recordFault(fault);
        return result;
    }

    // BUG-27 fix: removed spurious physicalAddress==0 guard; PA=0 is valid per spec.
    return result;
}

TranslationResult SMMU::performStage2OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                   AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime) {
    // ARM SMMU v3 spec: Stage-2 only translation IPA -> PA (IOVA treated as IPA)
    TranslationResult result = streamContext->translate(pasid, iova, accessType, securityState);

    // Record fault if translation failed
    if (result.isError()) {
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        // Classify fault type based on error code
        switch (result.getError()) {
            case SMMUError::PageNotMapped:
                fault.faultType = FaultType::TranslationFault;
                break;
            case SMMUError::PagePermissionViolation:
                fault.faultType = FaultType::PermissionFault;
                break;
            case SMMUError::InvalidSecurityState:
                fault.faultType = FaultType::SecurityFault;
                break;
            case SMMUError::InvalidAddress:
                // BUG-CPP-04 fix: Emit F_ADDR_SIZE here (in the stage-specific path) so
                // that handleTranslationFailure does not need to emit it again.
                fault.faultType = FaultType::AddressSizeFault;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState);
                break;
            default:
                fault.faultType = FaultType::AccessFault;
                break;
        }
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = currentTime;

        recordFault(fault);
        return result;
    }

    // BUG-27 fix: removed spurious physicalAddress==0 guard; PA=0 is valid per spec.
    return result;
}

// Task 5.2: Enhanced error handling and fault recovery methods
// Issue 5 fix: This method no longer re-records faults that were already recorded
// in stage-specific methods. It only performs fault recovery actions.
void SMMU::handleTranslationFailure(StreamID streamID, PASID pasid, IOVA iova,
                                   AccessType accessType, SecurityState securityState, TranslationResult& result, uint64_t currentTime) {
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
                // §7.3.14: F_ADDR_SIZE event already emitted by the translation path
                // (performTwoStageTranslation / stage-specific methods) before returning
                // this error.  BUG-CPP-04 fix: do NOT re-emit it here to avoid a
                // duplicate F_ADDR_SIZE event for a single fault.
                faultType = FaultType::AddressSizeFault;
                break;
            case SMMUError::InvalidSecurityState:
                faultType = FaultType::SecurityFault;
                break;
            case SMMUError::StreamDisabled:
                // §7.3.7 last line / §5.2 Config table: STE.Config==0b000 terminates without
                // recording an event. Abort is silent — no EventEntry is enqueued.
                faultType = FaultType::StreamDisabled;
                break;
            default:
                faultType = classifyTranslationFault(streamID, pasid, iova, accessType, securityState);
                break;
        }
    } else {
        // If result is successful, this shouldn't be called, but handle gracefully
        faultType = FaultType::TranslationFault;
    }

    // Issue 5 fix: Do NOT re-record the fault here. The fault was already recorded
    // in performTwoStageTranslation, performBothStagesTranslation, performStage1OnlyTranslation,
    // or performStage2OnlyTranslation. Only perform recovery actions.

    // ARM SMMU v3 spec: Implement fault recovery mechanisms based on fault type
    switch (faultType) {
        case FaultType::TranslationFault:
            // §7.3.13: F_TRANSLATION (0x10) must be generated for terminate-mode streams.
            // Stall-mode faults already generate the event in the stall path above.
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState);
            // Could implement page fault handling or demand paging here
            handleTranslationFaultRecovery(streamID, pasid, iova, securityState);
            break;

        case FaultType::PermissionFault:
            // §7.3.16: F_PERMISSION (0x13) must be generated for terminate-mode streams.
            // Stall-mode faults already generate the event in the stall path above.
            generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, securityState);
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

        case FaultType::SecurityFault: {
            // Security fault - log violation and notify security subsystem
            SecurityState expectedState = determineContextSecurityState(streamID, pasid);
            recordSecurityFault(streamID, pasid, iova, accessType, expectedState, securityState);
            break;
        }

        case FaultType::StreamDisabled:
            // §7.3.7: Stream is administratively disabled — event was generated above; no recovery needed
            break;

        case FaultType::BadStreamID:
            // §7.3.3: StreamID not in stream table — C_BAD_STREAMID event was generated in translate(); no recovery
            break;

        case FaultType::BadSubstreamId:
            // §7.3.9 / §3.9: Non-zero PASID on stage-2-only or bypass stream — C_BAD_SUBSTREAMID event
            // already generated in performTwoStageTranslation(); no additional recovery needed.
            break;

        // ARM SMMU v3 specific fault types - default handling (recovery only, no re-recording)
        case FaultType::ContextDescriptorFormatFault:
        case FaultType::TranslationTableFormatFault:
        case FaultType::Level0TranslationFault:
        case FaultType::Level1TranslationFault:
        case FaultType::Level2TranslationFault:
        case FaultType::Level3TranslationFault:
        case FaultType::AccessFlagFault:
            // §7.3.15 / FINDING-NEW-31: Access flag fault → F_ACCESS event (0x12).
            generateEvent(EventType::F_ACCESS, streamID, pasid, iova, securityState);
            break;

        case FaultType::DirtyBitFault:
        case FaultType::TLBConflictFault:
        case FaultType::ExternalAbort:
        case FaultType::SynchronousExternalAbort:
        case FaultType::AsynchronousExternalAbort:
        case FaultType::StreamTableFormatFault:
        case FaultType::ConfigurationCacheFault:
        case FaultType::Stage2TranslationFault:
        case FaultType::Stage2PermissionFault:
            // Default handling for ARM SMMU v3 specific faults
            // Recovery actions would be implemented here in a full implementation
            break;
    }

    (void)currentTime; // Available for future recovery logic that may need timing
}

FaultType SMMU::classifyTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) const {
    (void)pasid; // Suppress unused parameter warning - reserved for future PASID-aware fault classification
    (void)accessType; // Suppress unused parameter warning - reserved for future access-aware fault classification  
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware fault classification
    
    // ARM §7.3.13–7.3.16 / FINDING-NEW-43: Fault classification must be based on
    // actual error cause, not on arbitrary IOVA value heuristics.
    // IOVA=0 is a valid mapped address (not an "access fault").
    // High IOVAs are only an address-size fault when they exceed the configured
    // input address size — that is handled in handleTranslationFailure() via the
    // SMMUError::InvalidAddress case.  Here we return the generic default.

    // BUG-06 fix: streamMap must be accessed under the appropriate stripe lock.
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    (void)streamMap.find(streamID);  // access under lock for BUG-06 correctness

    // Default: TranslationFault — callers supply the real error code via result.getError().
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

    // BUG-03 fix: protect eventQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    while (!eventQueue.empty()) {
        const EventEntry& event = eventQueue.front();
        
        // ARM SMMU v3 spec: Process different event types
        switch (event.type) {
            case EventType::F_TRANSLATION:
            case EventType::F_PERMISSION:
            case EventType::F_ADDR_SIZE:
            case EventType::F_ACCESS:
            case EventType::F_WALK_EABT:
                // Fault events are already recorded by fault handler
                break;

            case EventType::F_UUT:
            case EventType::C_BAD_STREAMID:
            case EventType::F_STE_FETCH:
            case EventType::C_BAD_STE:
            case EventType::F_BAD_ATS_TREQ:
            case EventType::F_STREAM_DISABLED:
            case EventType::F_TRANSL_FORBIDDEN:
            case EventType::C_BAD_SUBSTREAMID:
            case EventType::F_CD_FETCH:
            case EventType::C_BAD_CD:
                // Configuration / setup errors
                break;

            case EventType::F_TLB_CONFLICT:
            case EventType::F_CFG_CONFLICT:
            case EventType::F_VMS_FETCH:
                // Implementation-specific conflict / fetch errors
                break;

            case EventType::E_PAGE_REQUEST:
                // Page Request Interface hint event (§7.3.19)
                break;

            case EventType::COMMAND_SYNC_COMPLETION:
                // IMPDEF: CMD_SYNC completion signalling
                break;

            case EventType::ATC_INVALIDATE_COMPLETION:
                // IMPDEF: ATC_INV completion signalling
                break;
        }
        
        // Remove processed event
        eventQueue.pop_front();
        // ARM §3.5.1: Advance consumer index on dequeue (FINDING-M-01)
        eventqCons = advanceQueueIndex(eventqCons, eventqLog2Size);
    }
}

Result<bool> SMMU::hasEvents() const {
    // BUG-03 fix: protect eventQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    try {
        return makeSuccess(!eventQueue.empty());
    } catch (...) {
        return makeError<bool>(SMMUError::InternalError);
    }
}

std::vector<EventEntry> SMMU::getEventQueue() const {
    // BUG-03 fix: protect eventQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    std::vector<EventEntry> events;
    events.reserve(eventQueue.size());
    for (const auto& event : eventQueue) {
        events.push_back(event);
    }
    return events;
}

void SMMU::clearEventQueue() {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    eventQueue.clear();
    // ARM §3.5.1: Reset PROD/CONS indices on clear (FINDING-M-01)
    eventqProd = 0;
    eventqCons = 0;
}

size_t SMMU::getEventQueueSize() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return eventQueue.size();
}

// Task 5.3: Command Queue Processing Simulation (Task 5.3.2)
VoidResult SMMU::submitCommand(const CommandEntry& command) {
    // BUG-03 fix: protect commandQueue with queueMutex (recursive to allow
    // processPRIQueue -> submitCommand re-entrant calls).
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    if (commandQueue.size() >= maxCommandQueueSize) {
        // §6.3.17: Command queue abort — set GERROR.CMDQ_ABT_ERR (bit 8).
        // No event is generated; hardware signals the error via GERROR only.
        gerrorStatus |= GERROR_CMDQ_ABT_ERR;
        return makeVoidError(SMMUError::CommandQueueFull);
    }
    CommandEntry timestampedCommand = command;
    timestampedCommand.timestamp = getCurrentTimestamp();
    commandQueue.push_back(timestampedCommand);
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-01)
    cmdqProd = advanceQueueIndex(cmdqProd, cmdqLog2Size);
    return makeVoidSuccess();
}

void SMMU::processCommandQueue() {
    // §4.1.2 / CT-33: When CR0.CMDQEN=0, command queue processing is disabled.
    if ((cr0_ & CR0_CMDQEN) == 0u) {
        return;
    }

    // ARM SMMU v3 spec: Process command queue with proper ordering.
    // BUG-03 fix: protect commandQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    while (!commandQueue.empty()) {
        // BUG-04 fix: copy command by value before pop_front() so that the
        // reference is not dangling when we inspect command.type afterwards.
        CommandEntry command = commandQueue.front();
        commandQueue.pop_front();
        // ARM §3.5.1: Advance consumer index on dequeue (FINDING-M-01)
        cmdqCons = advanceQueueIndex(cmdqCons, cmdqLog2Size);

        // Process the command based on type
        processCommand(command);

        // ARM SMMU v3 spec: Handle synchronization commands
        if (command.type == CommandType::SYNC) {
            // §4.8 / FINDING-NEW-33: CS=0b11 is Reserved → CERROR_ILL.
            // Suppress the completion event and record a configuration fault.
            if (command.cs == 3u) {  // 0b11 = 3: Reserved per §4.8
                FaultRecord illFault;
                illFault.streamID = command.streamID;
                illFault.pasid = command.pasid;
                illFault.faultType = FaultType::ConfigurationCacheFault;
                recordFault(illFault);
                break;
            }
            // §4.8 / FINDING-NEW-27: CS=0b00 (SIG_NONE) → no completion signal.
            if (command.cs != 0) {
                // FINDING-NEW-39: derive security state from the stream config rather than
                // hardcoding NonSecure, per ARM §4.8.
                SecurityState syncEventSecState = SecurityState::NonSecure;
                {
                    size_t syncStripe = getStreamStripe(command.streamID);
                    std::lock_guard<std::mutex> syncLock(streamLockStripes[syncStripe]);
                    auto syncIt = streamMap.find(command.streamID);
                    if (syncIt != streamMap.end()) {
                        syncEventSecState = syncIt->second->getStreamConfiguration().securityState;
                    }
                }
                generateEvent(EventType::COMMAND_SYNC_COMPLETION, command.streamID, command.pasid,
                              command.startAddress, syncEventSecState);
            }
            // BUG-CPP-05 fix: ARM §4.8 specifies CMD_SYNC as a barrier, not a stop.
            // After completing the sync and emitting the optional completion event,
            // processing must continue with the next command.  Only the CS=0b11
            // CERROR_ILL error path (above) should stop processing via break.
            continue;
        }
    }
}

Result<bool> SMMU::isCommandQueueFull() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    try {
        return makeSuccess(commandQueue.size() >= maxCommandQueueSize);
    } catch (...) {
        return makeError<bool>(SMMUError::InternalError);
    }
}

size_t SMMU::getCommandQueueSize() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return commandQueue.size();
}

std::vector<CommandEntry> SMMU::getCommandQueue() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    std::vector<CommandEntry> commands;
    commands.reserve(commandQueue.size());
    for (const auto& cmd : commandQueue) {
        commands.push_back(cmd);
    }
    return commands;
}

void SMMU::clearCommandQueue() {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    commandQueue.clear();
    // ARM §3.5.1: Reset PROD/CONS indices on clear (FINDING-M-01)
    cmdqProd = 0;
    cmdqCons = 0;
}

// Task 5.3: PRI Queue for Page Requests (Task 5.3.3)
void SMMU::submitPageRequest(const PRIEntry& request) {
    // BUG-03 fix: protect priQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    if (priQueue.size() >= maxPRIQueueSize) {
        priQueue.pop_front();
    }
    PRIEntry timestampedRequest = request;
    timestampedRequest.timestamp = getCurrentTimestamp();
    priQueue.push_back(timestampedRequest);
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-08)
    priqProd = advanceQueueIndex(priqProd, priqLog2Size);
    // §7.3.19 / FINDING-NEW-32: carry the request's security state, not a hardcoded NonSecure.
    generateEvent(EventType::E_PAGE_REQUEST, request.streamID, request.pasid, request.requestedAddress, request.securityState);
}

void SMMU::processPRIQueue() {
    // §CT-33: When CR0.PRIQEN=0, PRI queue processing is disabled.
    if ((cr0_ & CR0_PRIQEN) == 0u) {
        return;
    }

    // ARM SMMU v3 spec: Process Page Request Interface queue.
    // BUG-03 fix: protect priQueue with queueMutex. Uses recursive_mutex so
    // that the nested submitCommand() call can re-acquire the same lock.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
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
        // ARM §8.3: Echo PRGIndex back in the response command (FINDING-M-08)
        response.prgIndex = request.prgIndex;
        
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
    // BUG-03 fix: protect priQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    std::vector<PRIEntry> requests;
    requests.reserve(priQueue.size());
    for (const auto& request : priQueue) {
        requests.push_back(request);
    }
    return requests;
}

void SMMU::clearPRIQueue() {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    priQueue.clear();
}

size_t SMMU::getPRIQueueSize() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
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
            // ARM §4.3.2: CMD_CFGI_ALL (range==31) or CMD_CFGI_STE_RANGE (range<31).
            // NOTE: shifting uint32_t by 32 bits is UB, so range==31 is handled separately.
            if (command.range == 31) {
                // CMD_CFGI_ALL — full global STE-cache / TLB invalidation
                invalidateTranslationCache();
            } else {
                // CMD_CFGI_STE_RANGE — invalidate only streams matching the upper-bit prefix:
                // match condition: (sid >> (range+1)) == (command.streamID >> (range+1))
                uint32_t prefixBits = static_cast<uint32_t>(command.range) + 1u;
                StreamID cmdPrefix = command.streamID >> prefixBits;
                for (const auto& pair : streamMap) {
                    if ((pair.first >> prefixBits) == cmdPrefix) {
                        invalidateStreamCache(pair.first);
                    }
                }
            }
            break;
            
        case CommandType::CFGI_CD:
            // ARM §4.3.3: Invalidate cached CD for (StreamID, SSID/PASID).
            // Maps to PASID-scoped TLB invalidation in SW model.
            invalidatePASIDCache(command.streamID, command.pasid);
            break;

        case CommandType::CFGI_CD_ALL:
            // ARM §4.3.4: Invalidate all cached CDs for StreamID.
            // Maps to stream-wide TLB invalidation in SW model.
            invalidateStreamCache(command.streamID);
            break;

        case CommandType::TLBI_NH_ALL:
        case CommandType::TLBI_NH_ASID:
        case CommandType::TLBI_NH_VA:
        case CommandType::TLBI_NH_VAA:
        case CommandType::TLBI_EL2_ALL:
        case CommandType::TLBI_EL2_ASID:
        case CommandType::TLBI_EL2_VA:
        case CommandType::TLBI_EL2_VAA:
        case CommandType::TLBI_S12_VMALL:
        case CommandType::TLBI_S2_IPA:
        case CommandType::TLBI_NSNH_ALL:
            // TLB invalidation commands
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid, command.asid, command.vmid);
            break;
            
        case CommandType::ATC_INV: {
            // §4.5.1: ATC_INVALIDATE_COMPLETION is generated ONLY for CMD_ATC_INV.
            executeATCInvalidationCommand(command.streamID, command.pasid,
                                        command.startAddress, command.endAddress);
            // FINDING-NEW-39: derive security state from the stream config rather than
            // hardcoding NonSecure, per ARM §4.5.1 / §4.8.
            SecurityState atcEventSecState = SecurityState::NonSecure;
            {
                size_t atcStripe = getStreamStripe(command.streamID);
                std::lock_guard<std::mutex> atcLock(streamLockStripes[atcStripe]);
                auto atcIt = streamMap.find(command.streamID);
                if (atcIt != streamMap.end()) {
                    atcEventSecState = atcIt->second->getStreamConfiguration().securityState;
                }
            }
            generateEvent(EventType::ATC_INVALIDATE_COMPLETION, command.streamID, command.pasid,
                          command.startAddress, atcEventSecState);
            break;
        }

        // §4.1.1 / CT-30: Additional TLB invalidation commands
        case CommandType::TLBI_EL3_ALL:
        case CommandType::TLBI_EL3_VA:
        case CommandType::TLBI_S_EL2_ALL:
        case CommandType::TLBI_S_EL2_ASID:
        case CommandType::TLBI_S_EL2_VA:
        case CommandType::TLBI_S_EL2_VAA:
        case CommandType::TLBI_S_S12_VMALL:
        case CommandType::TLBI_S_S2_IPA:
        case CommandType::TLBI_SNH_ALL:
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid, command.asid, command.vmid);
            break;

        default:
            // Invalid invalidation command
            generateEvent(EventType::C_BAD_STE, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
            break;
    }
}

void SMMU::executeTLBInvalidationCommand(CommandType type, StreamID streamID, PASID pasid, uint16_t asid, uint16_t vmid) {
    // ARM SMMU v3 spec: Execute TLB-specific invalidation commands
    switch (type) {
        case CommandType::TLBI_NH_ALL:
        case CommandType::TLBI_NH_VA:
        case CommandType::TLBI_NH_VAA:
        case CommandType::TLBI_NSNH_ALL:
            // Non-secure Hyp / VA / VAA TLB invalidation — global flush
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_NH_ASID:
            // ARM §4.4: ASID-targeted invalidation (CMD_TLBI_NH_ASID, opcode 0x11)
            tlbCache->invalidateByASID(asid);
            break;

        case CommandType::TLBI_EL2_ALL:
        case CommandType::TLBI_EL2_VA:
        case CommandType::TLBI_EL2_VAA:
            // EL2 TLB invalidation — global flush
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_EL2_ASID:
            // ARM §4.4: ASID-targeted invalidation (CMD_TLBI_EL2_ASID, opcode 0x21)
            tlbCache->invalidateByASID(asid);
            break;

        case CommandType::TLBI_S12_VMALL:
        case CommandType::TLBI_S2_IPA:
            // ARM §4.4: VMID-targeted invalidation (CMD_TLBI_S12_VMALL 0x28, CMD_TLBI_S2_IPA 0x2A)
            tlbCache->invalidateByVMID(vmid);
            break;

        // §4.1.1 / CT-30: EL3 and Secure EL2 TLB invalidation
        case CommandType::TLBI_EL3_ALL:
        case CommandType::TLBI_EL3_VA:
        case CommandType::TLBI_SNH_ALL:
            // EL3 / Secure NH TLB invalidation — global flush in SW model
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_S_EL2_ALL:
        case CommandType::TLBI_S_EL2_VA:
        case CommandType::TLBI_S_EL2_VAA:
            // Secure EL2 TLB invalidation — global flush in SW model
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_S_EL2_ASID:
            // Secure EL2 ASID-targeted invalidation
            tlbCache->invalidateByASID(asid);
            break;

        case CommandType::TLBI_S_S12_VMALL:
        case CommandType::TLBI_S_S2_IPA:
            // Secure Stage-1+2 / Stage-2 IPA VMID-targeted invalidation
            tlbCache->invalidateByVMID(vmid);
            break;

        default:
            // Not a TLB invalidation command
            generateEvent(EventType::C_BAD_STE, streamID, pasid, 0, SecurityState::NonSecure);
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

            // BUG-14 fix: computing (endAddr + PAGE_SIZE - 1) overflows when
            // endAddr is near UINT64_MAX.  Saturate to the last aligned address
            // representable in 64 bits instead.
            IOVA alignedEndAddr;
            if (endAddr >= (UINT64_MAX - (PAGE_SIZE - 1))) {
                alignedEndAddr = UINT64_MAX & ~PAGE_MASK; // saturate
            } else {
                alignedEndAddr = (endAddr + PAGE_SIZE - 1) & ~PAGE_MASK;
            }

            // Invalidate each page in the range.
            // BUG-14 fix: detect unsigned wrap-around *before* incrementing so
            // we never execute the loop body with an overflowed address.
            while (currentAddr <= alignedEndAddr) {
                tlbCache->invalidate(streamID, pasid, currentAddr);
                if (currentAddr > UINT64_MAX - PAGE_SIZE) {
                    break; // Next increment would wrap — all pages covered
                }
                currentAddr += PAGE_SIZE;
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
            
        case CommandType::CFGI_STE: {
            // ARM §4.3.1 / FINDING-NEW-40: CMD_CFGI_STE with an unknown StreamID
            // (not present in the stream table) must generate C_BAD_STREAMID and
            // set GERROR.CMDQ_ERR, per §4.3.1.
            size_t cfgiStripe = getStreamStripe(command.streamID);
            {
                std::lock_guard<std::mutex> cfgiLock(streamLockStripes[cfgiStripe]);
                if (streamMap.find(command.streamID) == streamMap.end()) {
                    generateEvent(EventType::C_BAD_STREAMID, command.streamID, command.pasid,
                                  command.startAddress, SecurityState::NonSecure);
                    gerrorStatus |= GERROR_CMDQ_ERR;
                    break;
                }
            }
            // Known StreamID — proceed with normal STE cache invalidation
            executeInvalidationCommand(command);
            break;
        }

        case CommandType::CFGI_ALL:
        case CommandType::CFGI_CD:
        case CommandType::CFGI_CD_ALL:
        case CommandType::TLBI_NH_ALL:
        case CommandType::TLBI_NH_ASID:
        case CommandType::TLBI_NH_VA:
        case CommandType::TLBI_NH_VAA:
        case CommandType::TLBI_EL2_ALL:
        case CommandType::TLBI_EL2_ASID:
        case CommandType::TLBI_EL2_VA:
        case CommandType::TLBI_EL2_VAA:
        case CommandType::TLBI_S12_VMALL:
        case CommandType::TLBI_S2_IPA:
        case CommandType::TLBI_NSNH_ALL:
        case CommandType::ATC_INV:
            // Cache invalidation commands
            executeInvalidationCommand(command);
            break;

        case CommandType::PRI_RESP: {
            // ARM §8.3: Find and complete the pending PRIEntry with matching
            // streamID + prgIndex. Remove it from the PRI queue. (FINDING-M-08)
            // processCommandQueue() holds queueMutex via recursive_mutex, so
            // re-acquiring here is safe.
            std::lock_guard<std::recursive_mutex> priLock(queueMutex);
            for (auto it = priQueue.begin(); it != priQueue.end(); ++it) {
                if (it->streamID == command.streamID && it->prgIndex == command.prgIndex) {
                    priQueue.erase(it);
                    priqCons = advanceQueueIndex(priqCons, priqLog2Size);
                    break;
                }
            }
            // If not found: PRGIndex is invalid — per ARM §8.3, this is a
            // software error; no action taken (no event generated for simulation).
            break;
        }

        case CommandType::RESUME: {
            // ARM §4.6: CMD_RESUME — resume or abort a stalled transaction.
            // Three outcomes based on Ac (action) and Ab (abort) bits:
            //   Ac=1:           Retry  — transaction may be retried.
            //   Ac=0, Ab=0:     Terminate successfully (RAZ/WI from device perspective).
            //   Ac=0, Ab=1:     Abort with bus error.
            // Per §4.6: only retire the record if its StreamID matches.
            std::lock_guard<std::mutex> slock(stallQueueMutex_);
            auto it = stallQueue_.find(command.stag);
            if (it != stallQueue_.end() && it->second.streamID == command.streamID) {
                // Outcome is determined by Ac/Ab — in the SW model all three outcomes
                // simply retire the stall record (actual retry/abort is the caller's
                // responsibility via re-issuing or discarding the DMA transaction).
                stallQueue_.erase(it);
            }
            // If STAG not found or StreamID mismatch: no effect (§4.6).
            break;
        }

        case CommandType::STALL_TERM: {
            // ARM §4.7: CMD_STALL_TERM — abort ALL stalled transactions for StreamID.
            // Removes every StallRecord whose streamID matches command.streamID.
            std::lock_guard<std::mutex> slock(stallQueueMutex_);
            for (auto it = stallQueue_.begin(); it != stallQueue_.end(); ) {
                if (it->second.streamID == command.streamID) {
                    it = stallQueue_.erase(it);
                } else {
                    ++it;
                }
            }
            break;
        }
            
        case CommandType::SYNC:
            // Synchronization barrier
            // ARM SMMU v3 spec: Ensure command ordering and completion
            // Handled in processCommandQueue()
            break;

        // §4.1.1 / CT-30: Additional spec-defined command opcodes
        case CommandType::CFGI_VMS_PIDM:
            // Secure substream PIDM cache invalidation: invalidate CD cache for (SID, SSID).
            invalidatePASIDCache(command.streamID, command.pasid);
            break;

        case CommandType::TLBI_EL3_ALL:
        case CommandType::TLBI_EL3_VA:
        case CommandType::TLBI_S_EL2_ALL:
        case CommandType::TLBI_S_EL2_ASID:
        case CommandType::TLBI_S_EL2_VA:
        case CommandType::TLBI_S_EL2_VAA:
        case CommandType::TLBI_S_S12_VMALL:
        case CommandType::TLBI_S_S2_IPA:
        case CommandType::TLBI_SNH_ALL:
            // TLB invalidation commands: delegate to TLB invalidation handler.
            executeInvalidationCommand(command);
            break;

        case CommandType::DPTI_ALL:
        case CommandType::DPTI_PA:
            // Dirty page tracking invalidation — software model: no-op (log only).
            break;

        default:
            // Unknown command type — ARM §6.3.17: set CMDQ_ERR (FINDING-M-06)
            gerrorStatus |= GERROR_CMDQ_ERR;
            generateEvent(EventType::C_BAD_STE, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
            break;
    }
}

// ARM §6.3.17: Read SMMU_GERROR register (FINDING-M-06)
uint32_t SMMU::getGerror() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return gerrorStatus;
}

// ARM §6.3.18: Clear SMMU_GERROR bits by writing to SMMU_GERRORN (FINDING-M-06)
void SMMU::clearGerror(uint32_t bits) {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    gerrorStatus &= ~bits;
}

// ARM §6.3.9 SMMU_CR0.SMMUEN and §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01, FINDING-NEW-09)
// §6.3.9 SMMU_CR0 register (CT-33)
void SMMU::setCR0(uint32_t value) {
    cr0_ = value;
    // Mirror SMMUEN bit to smmuen_ for backward compatibility
    smmuen_ = ((cr0_ & CR0_SMMUEN) != 0u);
}

uint32_t SMMU::getCR0() const {
    return cr0_;
}

// §6.3.4 SMMU_STRTAB_BASE_CFG.LOG2SIZE (CT-04)
void SMMU::setStrtabLog2Size(uint8_t log2size) {
    strtabLog2Size_ = log2size;
}

uint8_t SMMU::getStrtabLog2Size() const {
    return strtabLog2Size_;
}

void SMMU::enable() {
    smmuen_ = true;
    cr0_ |= CR0_SMMUEN;
    // Also enable event and command queues by default when enable() is called
    // (for backward compatibility — callers of enable() expect all queues active)
    cr0_ |= CR0_EVENTQEN | CR0_CMDQEN | CR0_PRIQEN;
}

void SMMU::disable() {
    smmuen_ = false;
    cr0_ &= ~CR0_SMMUEN;
}

bool SMMU::isEnabled() const {
    return smmuen_;
}

void SMMU::setGbpaAbort(bool abort) {
    gbpaAbort_ = abort;
}

bool SMMU::isGbpaAbort() const {
    return gbpaAbort_;
}

// ARM §3.12.2: Stall queue management (FINDING-NEW-08)

std::vector<StallRecord> SMMU::getStalledTransactions() const {
    std::lock_guard<std::mutex> slock(stallQueueMutex_);
    std::vector<StallRecord> result;
    result.reserve(stallQueue_.size());
    for (const auto& pair : stallQueue_) {
        result.push_back(pair.second);
    }
    return result;
}

bool SMMU::abortStalledTransaction(uint16_t stag) {
    std::lock_guard<std::mutex> slock(stallQueueMutex_);
    auto it = stallQueue_.find(stag);
    if (it != stallQueue_.end()) {
        stallQueue_.erase(it);
        return true;
    }
    return false;
}

size_t SMMU::getStalledTransactionCount() const {
    std::lock_guard<std::mutex> slock(stallQueueMutex_);
    return stallQueue_.size();
}

void SMMU::generateEvent(EventType type, StreamID streamID, PASID pasid, IOVA address,
                         SecurityState securityState, bool isStall, uint16_t stag) {
    // §7.2.1 / CT-33: When CR0.EVENTQEN=0, events must not be recorded.
    // Exception: stall events must always be recorded (§3.5.3 stall semantics).
    if (!isStall && (cr0_ & CR0_EVENTQEN) == 0u) {
        return;
    }

    // ARM SMMU v3 spec: Generate event for event queue processing.
    // BUG-03 fix: protect eventQueue with queueMutex. Uses recursive_mutex so
    // that callers already holding queueMutex (e.g. processCommandQueue) can
    // safely call this without deadlocking.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    if (eventQueue.size() >= maxEventQueueSize) {
        if (!isStall) {
            // §3.5.3: Non-stall events may be discarded when queue is full.
            // Do NOT evict oldest — discard incoming event instead.
            return;
        }
        // Stall event: must not be lost even if it exceeds soft capacity.
        // Fall through to push_back without evicting the oldest entry.
    }

    // Create new event
    EventEntry event;
    event.type = type;
    event.streamID = streamID;
    event.pasid = pasid;
    event.address = address;
    event.securityState = securityState;
    event.timestamp = getCurrentTimestamp();
    event.stall = isStall;
    // §3.12.2 / FINDING-NEW-26: Carry the stall tag so software can correlate
    // the EventEntry to the matching CMD_RESUME command.  Zero when stall==false.
    event.stag = stag;

    // §7.3 / FINDING-NEW-28: errorCode has no spec-defined meaning; set to 0.
    // The event type (EventEntry.type) is the authoritative fault identifier.
    event.errorCode = 0;
    
    // Add to event queue
    eventQueue.push_back(event);
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-01)
    eventqProd = advanceQueueIndex(eventqProd, eventqLog2Size);
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
    
    // BUG-CPP-07 fix: Security state mismatches at translation time generate
    // F_PERMISSION (event code 0x13, permission fault) per ARM §7.3.16, not
    // C_BAD_STE which is for malformed STE configuration entries.
    generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, actualState);
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

        case SecurityState::Root:
            // Root (SMMUv3.3 RME §3.10) can access all PA spaces
            return true;

        default:
            return false;
    }
}

SecurityState SMMU::determineContextSecurityState(StreamID streamID, PASID pasid) const {
    (void)pasid; // Suppress unused parameter warning - reserved for future PASID-specific security state logic
    
    // ARM SMMU v3 spec: Determine security state for stream/PASID context
    // In this implementation, we default to NonSecure unless configured otherwise
    // A real implementation would consult stream configuration tables

    // BUG-02 fix: streamMap access requires the appropriate stripe lock.
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
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
    // ARM SMMU v3 §7.3 syndrome encoding: maps event record bits [127:96] to a
    // 32-bit value (bit N = event record bit 96+N).
    //
    // Bit layout per §7.3.13–7.3.16:
    //   [2]   InD  — instruction/data    (event bit [98])
    //   [3]   RnW  — read/write          (event bit [99])
    //   [7]   S2   — stage-2 fault       (event bit [103])
    //   [9:8] CLASS — access class       (event bits [105:104])
    //         00 = IN (input transaction)
    //         01 = TT (translation table walk)
    //         10 = CD (context descriptor fetch)
    //   [17:16] IMPL_DEF level           (event bits [113:112])
    uint32_t syndrome = 0;

    // Bit [2]: InD — instruction fetch (1) or data access (0).
    // Note: spec requires InD == 0 when RnW == 0.
    if (instructionFetch) {
        syndrome |= (1u << 2);
    }

    // Bit [3]: RnW — write (1) or read (0).
    if (writeAccess) {
        syndrome |= (1u << 3);
    }

    // Bit [7]: S2 — stage-2 fault indicator.
    if (stage == FaultStage::Stage2Only || stage == FaultStage::BothStages) {
        syndrome |= (1u << 7);
    }

    // Bits [9:8]: CLASS — access class.
    uint32_t classVal = 0x00u;  // default: IN (input transaction)
    switch (faultType) {
        case FaultType::ContextDescriptorFormatFault:
            classVal = 0x02u;  // CD
            break;
        case FaultType::TranslationTableFormatFault:
        case FaultType::StreamTableFormatFault:
            classVal = 0x01u;  // TT
            break;
        default:
            classVal = 0x00u;  // IN
            break;
    }
    syndrome |= ((classVal & 0x03u) << 8);

    // Bits [17:16]: IMPL_DEF — encode page-table level for diagnostics.
    syndrome |= ((static_cast<uint32_t>(level) & 0x03u) << 16);

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
                                   uint64_t currentTime, uint8_t faultLevel, uint16_t contextDescIndex) {
    // Generate comprehensive ARM SMMU v3 fault syndrome
    PrivilegeLevel privLevel = determinePrivilegeLevel(accessType, securityState);
    FaultSyndrome syndrome = generateFaultSyndrome(faultType, stage, accessType, securityState,
                                                  faultLevel, privLevel, contextDescIndex);

    // Create comprehensive fault record
    FaultRecord fault(streamID, pasid, iova, faultType, accessType, securityState, syndrome);
    fault.timestamp = currentTime;

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
    // Lock all stripes in order to prevent deadlock when updating global configuration
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }

    // Validate the configuration
    VoidResult validationResult = validateConfigurationUpdate(config);
    if (!validationResult.isOk()) {
        return validationResult;
    }
    
    // Store old configuration for potential rollback
    SMMUConfiguration oldConfig = configuration;
    
    try {
        configuration = config;

        // Apply cache/TLB settings while stripe locks are held.
        applyConfiguration();

    } catch (const std::exception&) {
        // Rollback on failure
        configuration = oldConfig;
        return makeVoidError(SMMUError::ConfigurationError);
    }

    // BUG-24 fix: release all stripe locks before acquiring queueMutex
    // (lock-order invariant: queueMutex must never be acquired while a stripe lock is held).
    locks.clear();

    // Apply queue size limits and trim queues under queueMutex.
    {
        std::lock_guard<std::recursive_mutex> qlock(queueMutex);
        const QueueConfiguration& queueConfig = configuration.getQueueConfiguration();
        maxEventQueueSize = queueConfig.eventQueueSize;
        maxCommandQueueSize = queueConfig.commandQueueSize;
        maxPRIQueueSize = queueConfig.priQueueSize;
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

    return makeVoidSuccess();
}

VoidResult SMMU::updateQueueConfiguration(const QueueConfiguration& queueConfig) {
    if (!queueConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-24 fix: update `configuration` under all stripe locks (protects the shared
    // configuration object), then release stripe locks before acquiring queueMutex.
    // Lock-order invariant: queueMutex must never be acquired while a stripe lock is held.
    {
        std::vector<std::unique_lock<std::mutex>> locks;
        for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
            locks.emplace_back(streamLockStripes[i]);
        }
        VoidResult result = configuration.setQueueConfiguration(queueConfig);
        if (result.isError()) {
            return result;
        }
    }

    // Stripe locks are now released — safe to acquire queueMutex.
    std::lock_guard<std::recursive_mutex> qlock(queueMutex);
    maxEventQueueSize = queueConfig.eventQueueSize;
    maxCommandQueueSize = queueConfig.commandQueueSize;
    maxPRIQueueSize = queueConfig.priQueueSize;
    while (eventQueue.size() > maxEventQueueSize) {
        eventQueue.pop_front();
    }
    while (commandQueue.size() > maxCommandQueueSize) {
        commandQueue.pop_front();
    }
    while (priQueue.size() > maxPRIQueueSize) {
        priQueue.pop_front();
    }
    return makeVoidSuccess();
}

VoidResult SMMU::updateCacheConfiguration(const CacheConfiguration& cacheConfig) {
    // Lock all stripes in order to prevent deadlock when updating cache configuration
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }

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
    // Lock all stripes in order to prevent deadlock when updating address configuration
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }

    if (!addressConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Update the configuration
    return configuration.setAddressConfiguration(addressConfig);
}

VoidResult SMMU::updateResourceLimits(const ResourceLimits& resourceLimits) {
    // Lock all stripes in order to prevent deadlock when updating resource limits
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
    }

    if (!resourceLimits.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    // Update the configuration
    return configuration.setResourceLimits(resourceLimits);
}

// Configuration helper methods
void SMMU::applyConfiguration() {
    // BUG-24 fix: this method is called while all streamLockStripes are held.
    // Per the lock-order invariant, queueMutex must not be acquired while any stripe
    // lock is held.  Queue size limits and trimming are therefore handled separately
    // by callers (updateConfiguration) under queueMutex after releasing stripe locks.
    // Only cache / TLB settings are applied here.
    const CacheConfiguration& cacheConfig = configuration.getCacheConfiguration();
    cachingEnabled = cacheConfig.enableCaching;

    // Update TLB cache size if changed
    if (tlbCache->getCapacity() != cacheConfig.tlbCacheSize) {
        tlbCache->setMaxSize(cacheConfig.tlbCacheSize);
    }
}

VoidResult SMMU::validateConfigurationUpdate(const SMMUConfiguration& config) const {
    if (!config.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-13 fix: remove the empty if-blocks that falsely implied "we'll trim
    // the queue" but did nothing.  Queue trimming is performed later by
    // applyConfiguration(), which is the correct place for mutation.
    // Reading queue sizes here requires queueMutex (BUG-03 invariant).
    (void)config.getQueueConfiguration(); // size checks happen in applyConfiguration

    return makeVoidSuccess();
}

// FINDING-M-01: ARM §3.5.1 Circular Queue PROD/CONS register accessor implementations

uint32_t SMMU::getCmdqProdIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return cmdqProd;
}

uint32_t SMMU::getCmdqConsIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return cmdqCons;
}

uint32_t SMMU::getEventqProdIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return eventqProd;
}

uint32_t SMMU::getEventqConsIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return eventqCons;
}

uint32_t SMMU::getPriqProdIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return priqProd;
}

uint32_t SMMU::getPriqConsIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return priqCons;
}

bool SMMU::isCmdqEmptyByIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return cmdqProd == cmdqCons;
}

bool SMMU::isEventqEmptyByIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return eventqProd == eventqCons;
}

uint32_t SMMU::getCmdqOccupiedEntries() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return queueOccupied(cmdqProd, cmdqCons, cmdqLog2Size);
}

uint32_t SMMU::getEventqOccupiedEntries() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return queueOccupied(eventqProd, eventqCons, eventqLog2Size);
}

uint32_t SMMU::getCmdqLog2Size() const {
    return cmdqLog2Size;
}

uint32_t SMMU::getEventqLog2Size() const {
    return eventqLog2Size;
}

} // namespace smmu