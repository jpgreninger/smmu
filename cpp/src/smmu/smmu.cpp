// ARM SMMU v3 Main Controller Implementation
// Copyright (c) 2024 John Greninger
// Enhanced with Task 5.2: Translation Engine

#include "smmu/smmu.h"
#include <chrono>
#include <algorithm>
#include <climits>

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
// BUG-CPP-2 fix: clamp log2size to 19 (IDR1.CMDQS max per ARM §3.5.1) before
// the shift to prevent "2u << log2size" UB when log2size >= 31.
static uint32_t advanceQueueIndex(uint32_t idx, uint32_t log2size) {
    if (log2size > 19u) log2size = 19u; // ARM §3.5.1: spec max is 0 <= n <= 19
    uint32_t modulus = 2u << log2size; // 2^(log2size+1)
    return (idx + 1) % modulus;
}

// Compute number of occupied entries.
// BUG-CPP-2 fix: clamp log2size to 19 before the shift (same rationale as
// advanceQueueIndex above).
static uint32_t queueOccupied(uint32_t prod, uint32_t cons, uint32_t log2size) {
    if (log2size > 19u) log2size = 19u; // ARM §3.5.1: spec max is 0 <= n <= 19
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
      gerrorNStatus(0),
      cr0_(0),
      cr2_(0),
      smmuen_(false),
      gbpaAbort_(false),
      strtabLog2Size_(32),
      // BUG-NEW3-05 fix: Start at 1; STAG=0 is reserved per ARM §3.12.2.
      stagCounter_(1) {
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
      gerrorNStatus(0),
      cr0_(0),
      cr2_(0),
      smmuen_(false),
      gbpaAbort_(false),
      strtabLog2Size_(32),
      // BUG-NEW3-05 fix: Start at 1; STAG=0 is reserved per ARM §3.12.2.
      stagCounter_(1) {
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
    // BUG-CPP-3 fix: read SMMUEN from the authoritative cr0_ register bit rather
    // than the shadow smmuen_ bool to eliminate split-brain scenarios.
    // BUG-CPP-NEW-1 fix: use acquire load so that enable()/disable() writes
    // (which use release stores) are visible to this translate() reader.
    if ((cr0_.load(std::memory_order_acquire) & CR0_SMMUEN) == 0u) {
        if (gbpaAbort_.load(std::memory_order_acquire)) {
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
    // BUG-CPP-01 fix: use 64-bit shift to avoid UB when log2size approaches 32;
    // compare against uint64_t to safely accommodate all uint32_t StreamID values.
    // BUG-NEW-CPP-1 fix: load once with acquire ordering so that a concurrent
    // setStrtabLog2Size() write (release store) is visible to this reader.
    {
        uint8_t log2sz = strtabLog2Size_.load(std::memory_order_acquire);
        if (log2sz < 32u) {
            uint64_t limit = (uint64_t)1u << log2sz;
            if (static_cast<uint64_t>(streamID) >= limit) {
                // BUG-NEW-01 fix: record fault so getTotalFaultCount() stays consistent.
                // recordFault() is internal accounting and is always updated regardless of RECINVSID.
                FaultRecord fault;
                fault.streamID = streamID;
                fault.pasid = pasid;
                fault.address = iova;
                fault.faultType = FaultType::BadStreamID;
                fault.accessType = accessType;
                fault.securityState = securityState;
                fault.timestamp = currentTime;
                recordFault(fault);
                // §6.3.12 SMMU_CR2.RECINVSID: only write the C_BAD_STREAMID event to the
                // event queue when RECINVSID==1.  The fault record and translation error are
                // always produced unconditionally regardless of the RECINVSID setting.
                if ((cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u) {
                    generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
                }
                return makeTranslationError(SMMUError::InvalidStreamID);
            }
        }
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
        // recordFault() is internal accounting and is always updated regardless of RECINVSID.
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        fault.faultType = FaultType::BadStreamID;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = currentTime;
        recordFault(fault);
        // §6.3.12 SMMU_CR2.RECINVSID: only write the C_BAD_STREAMID event to the
        // event queue when RECINVSID==1.  The fault record and translation error are
        // always produced unconditionally regardless of the RECINVSID setting.
        if ((cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u) {
            generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
        }
        // BUG-CPP-DBGR-12 fix: §7.3.3 C_BAD_STREAMID maps to InvalidStreamID,
        // not StreamNotConfigured (which would imply the stream exists but is disabled).
        return makeTranslationError(SMMUError::InvalidStreamID);
    }

    // BUG-NEW-CPP-3 fix: take a shared_ptr copy of the StreamContext while the stripe
    // lock is held.  This keeps the StreamContext alive even after the stripe lock is
    // released (a concurrent reset() can clear streamMap, but the shared_ptr ref-count
    // prevents destruction while we still hold a reference).  We release the stripe lock
    // before calling performTwoStageTranslation() so that the generateEvent() calls
    // inside that function can acquire queueMutex without creating the ABBA deadlock
    // with processCommandQueue() which holds queueMutex and then acquires stripe locks.
    std::shared_ptr<StreamContext> streamContextPtr = streamIt->second;
    StreamContext* streamContext = streamContextPtr.get();

    // TLB fast path — now performed after streamContext is obtained so that the stream's
    // STE output-attribute overrides (memType, shareability, allocHint, instCfg, privCfg,
    // nsCfgOut) can be applied to the TranslationData on a cache hit.
    // BUG-CPP-DBGR-3 fix: the previous early lockless TLB path constructed
    // TranslationData(finalPA, permissions, securityState) which zero-initialises
    // the six output-attribute fields.  By moving the TLB check to here we have
    // the StreamConfig in hand and can re-apply the attributes the same way the
    // slow path's applyOutputAttrs lambda does in StreamContext::translateUnlocked().
    //
    // Bug 1 fix: STRW-aware TLB check (ARM §3.3.4 / §13.4.1).
    // STRW==EL2 (non-VHE) and STRW==EL3 suppress privilege checks by treating
    // AP[1] as 1.  EL2_E2H (VHE) maintains privileged/non-privileged checks
    // like EL1 and therefore must NOT be included here.
    // BUG-R2-CPP-4 fix: snapshot StreamConfig under the stripe lock so the same
    // consistent value is used for both the TLB fast path (below) and the
    // post-translation TLB insert guard (after lock.unlock()).  The previous
    // code called getStreamConfiguration() a second time after lock.unlock(),
    // racing with concurrent configureStream()/removeStream()+configureStream().
    StreamConfig streamCfgSnapshot = streamContext->getStreamConfiguration();

    // BUG-NEW-CPP-2 fix: acquire load pairs with the release stores in enableCaching(),
    // reset(), and applyConfiguration() so concurrent writes are safely observed.
    if (cachingEnabled.load(std::memory_order_acquire) && tlbCache) {
        const StreamConfig& streamCfgForTlb = streamCfgSnapshot;
        // Derive effective access type for the privilege-aware permission check.
        // BUG-CPP-DBGR-5 fix: §5.2 says STRW is IGNORED when stage-2 is enabled for
        // Non-secure streams.  Only apply STRW promotion for single-stage (stage-1-only) streams.
        AccessType effectiveAccessType = accessType;
        if (!streamCfgForTlb.stage2Enabled &&
            (streamCfgForTlb.strw == StreamWorld::EL2 || streamCfgForTlb.strw == StreamWorld::EL3)) {
            switch (accessType) {
                case AccessType::Read:      effectiveAccessType = AccessType::ReadPrivileged;      break;
                case AccessType::Write:     effectiveAccessType = AccessType::WritePrivileged;     break;
                case AccessType::Execute:   effectiveAccessType = AccessType::ExecutePrivileged;   break;
                case AccessType::ReadWrite: effectiveAccessType = AccessType::ReadWritePrivileged; break;
                case AccessType::ReadPrivileged:
                case AccessType::WritePrivileged:
                case AccessType::ExecutePrivileged:
                case AccessType::ReadWritePrivileged:
                    break;
            }
        }
        IOVA pageAlignedIOVA = iova & ~PAGE_MASK;
        Result<TLBEntry> entryResult = tlbCache->lookupEntry(streamID, pasid, pageAlignedIOVA, securityState);
        if (entryResult.isOk()) {
            const TLBEntry& entry = entryResult.getValue();
            if (entry.valid) {
                // ARM §3.16 / FINDING-NEW-37: TLB entries are valid until an explicit
                // TLBI command.  No time-based eviction is performed here.
                // Cache hit - validate access permissions against effective access type.
                // §3.12.2 / FINDING-NEW-25: On permission failure, fall through to the
                // slow path so that FaultMode::Stall is correctly applied.  The slow
                // path (performTwoStageTranslation) re-checks permissions via the page
                // table and applies the stall-mode queue if required.
                if (validateAccessPermissions(entry.permissions, effectiveAccessType)) {
                    PA finalPA = entry.physicalAddress + (iova & PAGE_MASK);
                    TranslationData data(finalPA, entry.permissions, entry.securityState);
                    // BUG-CPP-DBGR-3 fix: apply STE output-attribute overrides the same
                    // way StreamContext::translateUnlocked()'s applyOutputAttrs lambda
                    // does on the slow path.  Without this the six fields are always
                    // zero-initialised on every TLB cache hit.
                    data.memType      = streamCfgForTlb.mtCfg ? streamCfgForTlb.memAttr : 0u;
                    data.shareability = streamCfgForTlb.shCfg;
                    data.allocHint    = streamCfgForTlb.allocCfg;
                    data.instCfg      = streamCfgForTlb.instCfg;
                    data.privCfg      = streamCfgForTlb.privCfg;
                    data.nsCfgOut     = streamCfgForTlb.nsCfg;
                    return TranslationResult(data);
                }
                // Permission failure: fall through to slow path for stall-mode handling.
            }
        }
        // Cache miss — fall through to the full translation slow path.
    }

    // BUG-NEW-CPP-3 fix: Release the stripe lock BEFORE calling performTwoStageTranslation().
    // performTwoStageTranslation() (and its callees) call generateEvent(), which acquires
    // queueMutex.  Holding the stripe lock across that call creates an ABBA deadlock with
    // processCommandQueue(), which holds queueMutex and then acquires stripe locks.
    // The streamContextPtr shared_ptr (captured above) keeps the StreamContext alive after
    // the stripe lock is released, so accessing streamContext via the raw pointer is safe.
    lock.unlock();

    // Task 5.2: Enhanced two-stage translation with comprehensive error handling
    TranslationResult result = performTwoStageTranslation(streamID, pasid, iova, accessType, securityState, streamContext, currentTime);

    // Task 5.2: Cache successful translations for future lookups.
    // BUG-CPP-NEW-3 fix: ARM §3.9/§14.5 — S1DSS==0x01 bypass results must NEVER be
    // cached in the TLB.  The bypass path in performTwoStageTranslation returns early
    // with an identity result (PA == IOVA) when s1dss == 0x01 and pasid == 0.  Caching
    // this result would allow a stale TLB hit to survive stream reconfiguration.
    // Guard: skip TLB insert when s1dss == 0x01 AND pasid == 0 (the bypass condition).
    // s1dss == 0x02 ("use CD[0]") is NOT a bypass and MUST still be cached normally.
    // Only applies when the stream is substream-capable (s1cdMax > 0).
    if (result.isOk() && isTranslationCacheable(result) && cachingEnabled.load(std::memory_order_acquire) && tlbCache) {
        // BUG-R2-CPP-4 fix: use the pre-lock-release snapshot instead of a
        // post-lock getStreamConfiguration() call (which would race with
        // concurrent reconfigureStream()).
        const StreamConfig& streamCfg = streamCfgSnapshot;
        bool s1dssIsBypass = (streamCfg.s1cdMax > 0 && pasid == 0 && streamCfg.s1dss == 0x01u);
        if (!s1dssIsBypass) {
            cacheTranslationResult(streamID, pasid, iova, result, currentTime, streamCfg.asid, streamCfg.vmid);
        }
    } else if (result.isError()) {
        // ARM §3.12.2: Check per-stream stall mode before standard fault handling.
        // If the stream is configured for stall, enqueue a StallRecord and return
        // Stalled instead of the original error — software must issue CMD_RESUME.
        bool inStallMode = (streamContext->getFaultMode() == FaultMode::Stall);
        // BUG-NEW-02 fix: ARM §3.12.2 — configuration-class faults must always
        // abort immediately and must never be stalled.  Only F_TRANSLATION,
        // F_PERMISSION, and F_ADDR_SIZE are eligible for stall mode.
        // BUG-CPP-4 fix: Add PASIDNotFound (C_BAD_SUBSTREAMID) and
        // AddressSpaceExhausted (C_BAD_STE) to the isConfigFault guard.
        // ARM §3.12.2 and §7.3 intro: configuration errors always abort and
        // must NEVER stall.  Only F_TRANSLATION, F_PERMISSION, F_ADDR_SIZE,
        // and F_ACCESS (data faults) are eligible for stall mode.
        // PASIDNotFound maps to C_BAD_SUBSTREAMID (configuration fault class).
        // AddressSpaceExhausted maps to C_BAD_STE (configuration fault class).
        // BUG-CPP-DBGR-12 fix: add InvalidStreamID to the config-fault guard.
        // The streamMap-miss path now returns InvalidStreamID (§7.3.3 C_BAD_STREAMID)
        // instead of StreamNotConfigured.  Both are configuration faults that must
        // never stall — keep StreamNotConfigured for other callers that still use it.
        bool isConfigFault = (result.getError() == SMMUError::InvalidPASID           ||
                              result.getError() == SMMUError::StreamDisabled          ||
                              // BUG-CPP-F2 fix: SubstreamDisabled (S1DSS==0b00) is
                              // also a config fault — it must never enter stall mode.
                              result.getError() == SMMUError::SubstreamDisabled       ||
                              result.getError() == SMMUError::ConfigurationError      ||
                              result.getError() == SMMUError::InvalidConfiguration    ||
                              result.getError() == SMMUError::StreamNotConfigured     ||
                              result.getError() == SMMUError::InvalidStreamID         ||
                              result.getError() == SMMUError::PASIDNotFound           ||
                              result.getError() == SMMUError::AddressSpaceExhausted);
        if (inStallMode && !isConfigFault) {
            // §3.12.2 / FINDING-NEW-26: STAG=0 is reserved; skip it so that
            // EventEntry.stag is always non-zero for genuine stall events.
            // BUG-CPP-03 fix: Allocate STAG under stallQueueMutex_ and check for
            // wrap-around collisions with existing active entries.  After 65535 stalls
            // the 16-bit counter wraps and the new STAG could alias a live entry.
            // Keep incrementing (under the lock) until a free slot is found, with a
            // safety limit of 65535 iterations to avoid an infinite loop when the
            // queue is completely full.
            uint16_t stag = 0;
            bool stagValid = false;
            StallRecord record(0, streamID, pasid, iova, accessType, securityState, currentTime);
            {
                std::lock_guard<std::mutex> slock(stallQueueMutex_);
                // Skip STAG==0 (reserved), then attempt a single allocation.
                // Burning up to 65535 counter values on queue-full wasted the
                // 16-bit STAG counter space and accelerated wrap-around.
                // A single attempt is sufficient: if the slot is occupied the
                // stall queue is full and we fall back to terminate mode below.
                stag = stagCounter_.fetch_add(1, std::memory_order_acq_rel);
                while (stag == 0) {
                    stag = stagCounter_.fetch_add(1, std::memory_order_acq_rel);
                }
                // BUG-NEW2-03 fix: only write to stallQueue_ when a valid unique
                // non-zero slot was found.  If the queue is exhausted, fall back
                // to terminate-mode fault handling instead of corrupting the queue.
                if (stag != 0 && stallQueue_.count(stag) == 0) {
                    stagValid = true;
                    record = StallRecord(stag, streamID, pasid, iova, accessType, securityState, currentTime);
                    stallQueue_[stag] = record;
                }
            }
            if (!stagValid) {
                // Stall queue exhausted — fall back to terminate-mode fault handling.
                // BUG-NEW-CPP-3 fix: the stripe lock was already released before
                // performTwoStageTranslation(); do NOT call lock.unlock() again.
                streamContext = nullptr; // Defensive: no further accesses via this pointer
                handleTranslationFailure(streamID, pasid, iova, accessType, securityState, result, currentTime);
                return result;
            }
            // ARM §7.3 / FINDING-NEW-13: Derive the correct EventType from the actual
            // error so the OS fault handler receives the right fault classification.
            // Mirrors the mapping in handleTranslationFailure().
            EventType stallEventType;
            switch (result.getError()) {
                case SMMUError::PagePermissionViolation:
                    stallEventType = EventType::F_PERMISSION;
                    break;
                case SMMUError::InvalidSecurityState:
                    // ARM §7.3.16: security violations classify as F_PERMISSION,
                    // matching the non-stall path in handleTranslationFailure().
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
            // BUG-NEW-CPP-3 fix: the stripe lock was already released before
            // performTwoStageTranslation(); no need to release it again here.
            // generateEvent() acquires queueMutex — this is safe because the
            // stripe lock is no longer held, eliminating the ABBA deadlock.
            streamContext = nullptr; // Defensive: no further accesses via this pointer
            generateEvent(stallEventType, streamID, pasid, iova, securityState, /*isStall=*/true, stag);
            return makeTranslationError(SMMUError::Stalled);
        }
        // BUG-NEW-CPP-3 fix: the stripe lock was already released before
        // performTwoStageTranslation(); do NOT call lock.unlock() again.
        // handleTranslationFailure() may call determineContextSecurityState() which
        // acquires the stripe lock — this is safe because we no longer hold it.
        streamContext = nullptr; // Defensive: no further accesses via this pointer
        handleTranslationFailure(streamID, pasid, iova, accessType, securityState, result, currentTime);
        return result;
    }

    // BUG-CPP-C fix: null out the raw streamContext pointer before returning on the
    // success path.  The stripe lock `lock` is still held here (RAII releases it on
    // function exit), so streamContext is technically still valid — but nulling it
    // makes it explicit that no further dereferences should occur after this point
    // and prevents any future code from accidentally accessing the pointer after the
    // lock releases.
    streamContext = nullptr;
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
        // Create new StreamContext.
        // BUG-NEW-CPP-3 fix: use shared_ptr so translate() can take a reference-counted
        // copy under the stripe lock, then release the lock before performTwoStageTranslation().
        std::shared_ptr<StreamContext> streamContext(new StreamContext());

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
        streamMap[streamID] = streamContext;
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

    // ARM IHI0070G.b §7.3.3 / §3.11: Invalidate all TLB entries for this stream
    // before erasing it from the stream map.  Without this, the TLB fast-path in
    // translate() (which runs before the streamMap existence check) can serve a
    // stale hit and return a successful translation for a removed stream instead
    // of the required C_BAD_STREAMID configuration fault.
    invalidateStreamCache(streamID);

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

    // BUG-R2-CPP-3 fix: use release store to pair with acquire loads and
    // eliminate the C++11 data race with concurrent reset() writes.
    globalFaultMode.store(mode, std::memory_order_release);

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

    // BUG-NEW-CPP-2 fix: release store so that concurrent translate() readers
    // that use acquire loads observe the new value without a data race.
    cachingEnabled.store(enable, std::memory_order_release);
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
    // BUG-R2-CPP-3 fix: release store for atomic<FaultMode> globalFaultMode.
    globalFaultMode.store(FaultMode::Terminate, std::memory_order_release);
    // BUG-NEW-CPP-2 fix: release store for atomic<bool> cachingEnabled.
    cachingEnabled.store(true, std::memory_order_release);
    
    // Task 5.2: Reset TLB cache
    if (tlbCache) {
        tlbCache->reset();
    }
    
    // Task 5.3: Reset event and command processing queues
    clearEventQueue();
    clearCommandQueue();
    clearPRIQueue();

    // ARM §6.3.17/6.3.18: Reset global error registers (BUG-03/SPEC-09)
    // Both GERROR and GERRORN are reset to 0 at power-on reset.
    // BUG-CPP-NEW-1 fix: use release store so that subsequent acquire loads in
    // translate() observe the zeroed values.
    gerrorStatus.store(0, std::memory_order_release);
    gerrorNStatus.store(0, std::memory_order_release);

    // ARM §6.3.9: Reset returns SMMU to disabled state (SMMUEN=0, GBPA.ABORT=0).
    // BUG-NEW3-04 fix: ARM §6.3.9 — reset returns SMMU to disabled state (all CR0 bits clear).
    // Resetting only smmuen_ is insufficient; queue-enable bits must also be cleared.
    // BUG-CPP-NEW-1 fix: use release stores for these atomic members.
    smmuen_.store(false, std::memory_order_release);
    gbpaAbort_.store(false, std::memory_order_release);
    cr0_.store(0, std::memory_order_release);

    // BUG-R2-CPP-1 fix: restore strtabLog2Size_ to 32 (accept all 32-bit
    // StreamIDs) and cr2_ to 0 (RECINVSID=0 — events suppressed) per ARM
    // IHI0070G.b §6.3.4 and §6.3.12 reset-value requirements.
    strtabLog2Size_.store(32u, std::memory_order_release);
    cr2_.store(0u, std::memory_order_release);

    // ARM §3.12.2: Clear stall queue on reset.
    {
        std::lock_guard<std::mutex> slock(stallQueueMutex_);
        stallQueue_.clear();
    }
    // BUG-NEW3-05 fix: Start at 1; STAG=0 is reserved per ARM §3.12.2.
    stagCounter_.store(1, std::memory_order_relaxed);
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

VoidResult SMMU::setStreamStage2AddressSpace(StreamID streamID,
                                              std::shared_ptr<AddressSpace> stage2AS) {
    // ARM IHI0070G.b §5.2: Associate a hypervisor-managed Stage-2 address space
    // with the stream identified by streamID.
    if (!stage2AS) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotConfigured);
    }

    streamIt->second->setStage2AddressSpace(stage2AS);
    return makeVoidSuccess();
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
            // BUG-CPP-F2 fix: return SubstreamDisabled (not StreamDisabled) so
            // callers can distinguish S1DSS==0b00 (abort + event) from
            // STE.Config==0b000 (silent abort, no event).  §5.2 / §7.3.7.
            return makeTranslationError(SMMUError::SubstreamDisabled);
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

    // §7.3.9 / §3.10: C_BAD_SUBSTREAMID when SubstreamID (PASID) >= 2^STE.S1CDMax.
    // This must be checked BEFORE the AA64/T0SZ validation.
    // BUG-11 fix: guard the shift — isConfigurationValid() now rejects s1cdMax > 20,
    // but this defensive limit prevents UB if a value >= 32 ever reaches here.
    const uint32_t s1cdMaxLimit = (config.s1cdMax < 32u) ? (1u << config.s1cdMax) : UINT32_MAX;
    if (config.stage1Enabled && config.s1cdMax > 0 && pasid != 0 &&
        pasid >= s1cdMaxLimit) {
        FaultRecord substreamFault;
        substreamFault.streamID = streamID;
        substreamFault.pasid = pasid;
        substreamFault.address = iova;
        substreamFault.faultType = FaultType::BadSubstreamId;
        substreamFault.accessType = accessType;
        substreamFault.securityState = securityState;
        substreamFault.timestamp = currentTime;
        recordFault(substreamFault);
        generateEvent(EventType::C_BAD_SUBSTREAMID, streamID, pasid, iova, securityState);
        return makeTranslationError(SMMUError::InvalidPASID);
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
    if (!tlbCache || result.isError() || !cachingEnabled.load(std::memory_order_acquire)) {
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
    if (!tlbCache || !cachingEnabled.load(std::memory_order_acquire)) {
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
    // Generate a unique cache key combining StreamID, PASID, IOVA, and SecurityState.
    //
    // BUG-5 fix: the previous XOR layout placed (pasid & 0xFFFFF) in bits [31:12]
    // and (iova >> 12) also in overlapping bits [31:12], causing XOR cancellation.
    // For example, (PASID=1, IOVA=0x1000) and (PASID=0, IOVA=0x1001000) produced
    // identical keys because (1<<12)^(0x1) == (0<<12)^(0x1001) == 0x1001.
    //
    // The fix uses the Boost hash_combine pattern which mixes bits multiplicatively,
    // ensuring no component can silently cancel another through XOR.  Each call to
    // hashCombine avalanches all bits so that a single-bit difference in any input
    // propagates throughout the output.
    //
    // NOTE: generateCacheKey() is a utility method; the active TLB cache path uses
    // CacheKey structs with a full operator== comparison of (streamID, pasid, iova,
    // securityState).  This function is provided for potential use by secondary
    // lookup tables or diagnostic tools.
    auto hashCombine = [](uint64_t seed, uint64_t val) -> uint64_t {
        // 0x9e3779b97f4a7c15 is the 64-bit fractional part of the golden ratio,
        // chosen for good avalanche behavior (equivalent to Boost hash_combine).
        return seed ^ (val + 0x9e3779b97f4a7c15ULL + (seed << 6) + (seed >> 2));
    };
    uint64_t key = static_cast<uint64_t>(streamID);
    key = hashCombine(key, static_cast<uint64_t>(pasid));
    key = hashCombine(key, static_cast<uint64_t>(securityState));
    key = hashCombine(key, iova >> 12);
    cacheKey = key;
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
    // BUG-NEW-CPP-5 fix: getPASIDAddressSpace() now returns shared_ptr so the
    // AddressSpace stays alive even if a concurrent removeStreamPASID() destroys
    // the pasidMap entry between this call and the translatePage() call below.
    // Stage 1: IOVA -> IPA translation (using per-PASID address space)
    std::shared_ptr<AddressSpace> stage1AddressSpace = streamContext->getPASIDAddressSpace(pasid);
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
        // BUG-CPP-DBGR-6 fix: §7.3.14/§7.3.16 — classify each error correctly
        // instead of falling back to the generic AccessFault for all non-PageNotMapped errors.
        FaultType faultType;
        switch (stage1Result.getError()) {
            case SMMUError::PageNotMapped:
                faultType = classifyDetailedTranslationFault(iova, 1, false);
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
                faultType = FaultType::AccessFault;
                break;
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
        // §7.3.13: Stage-2 address space not configured — this is a translation fault
        // (F_TRANSLATION), not a configuration fault (C_BAD_STE / AddressSpaceExhausted).
        // BUG-CPP-DBGR-9 fix: return PageNotMapped so translate() routes to F_TRANSLATION;
        // AddressSpaceExhausted was incorrectly treated as a config fault by the stall guard.
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage2Only, currentTime, 0, 0);
        generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState);
        return makeTranslationError(SMMUError::PageNotMapped);
    }
    // ARM §5.2: When stage-2 AS is the same object as stage-1 AS (aliased via createStreamPASID
    // PASID-0 auto-link), the IPA from stage-1 cannot be looked up in stage-2 (IOVA→IPA mapping,
    // not IPA→PA).  Treat stage-1 result as the final translation (identity stage-2 semantics).
    if (stage2AddressSpace == stage1AddressSpace.get()) {
        return stage1Result;
    }
    
    // Perform Stage-2 translation: IPA -> PA
    // ARM IHI0070G.b §3.3.2, §3.12.1, §3.13.5, Ch.15, §16.5 (BUG-CPP-1 fix):
    // The Stage-2 translatePage lookup for the IPA->PA mapping must use
    // AccessType::Read regardless of the incoming accessType.  This mirrors the
    // correct pattern established in StreamContext::translateUnlocked
    // (BUG-NEW2-05 fix).  The real accessType is applied at the permission-
    // intersection step below (validateAccessPermissions), not here.
    // Using the incoming accessType here would cause a spurious PermissionFault
    // when a hypervisor maps Stage-1 page-table pages as read-only in Stage-2
    // and a guest WRITE triggers a Stage-2 PTW (page-table walk) lookup.
    // BUG-CPP-NEW-5 fix: ARM IHI0070G.b §3.3.2 / §3.10:
    // The IPA security state is carried by the stage-1 output (stage1Data.securityState),
    // not the incoming transaction's NS bit (securityState).  STE.NSCFG applies only when
    // stage-1 is bypassed.  When stage-1 has completed a full translation, its output
    // security state is the authoritative IPA security state for the stage-2 PTW lookup.
    // For Non-secure streams both values are equal (NonSecure), so this is a no-op in the
    // common case.  For Secure streams with NS-output overrides this is load-bearing.
    const TranslationData& stage1DataRef = stage1Result.getValue();
    TranslationResult stage2Result = stage2AddressSpace->translatePage(intermediatePA, AccessType::Read, stage1DataRef.securityState);
    if (stage2Result.isError()) {
        // ARM SMMU v3 spec Section 7.3.3: Stage-2 fault attribution
        // BUG-CPP-DBGR-6 fix: §7.3.14/§7.3.16 — classify each Stage-2 error correctly.
        // Stage2PermissionFault was incorrectly used as the fallback for all non-PageNotMapped
        // errors (including PermissionViolation, which must produce F_PERMISSION).
        FaultType stage2FaultType;
        switch (stage2Result.getError()) {
            case SMMUError::PageNotMapped:
                stage2FaultType = classifyDetailedTranslationFault(intermediatePA, 1, false);
                break;
            case SMMUError::PagePermissionViolation:
                stage2FaultType = FaultType::PermissionFault;
                break;
            case SMMUError::InvalidAddress:
                stage2FaultType = FaultType::AddressSizeFault;
                break;
            case SMMUError::InvalidSecurityState:
                stage2FaultType = FaultType::SecurityFault;
                break;
            default:
                stage2FaultType = FaultType::Stage2PermissionFault;
                break;
        }

        // BUG-NEW-05 fix: ARM §7.3 requires that fault records carry the originating
        // SubstreamID (PASID) of the transaction that caused the fault.  The previous
        // comment was incorrect — Stage-2 faults must use the guest PASID, not 0.
        // Using pasid (the parameter to this function) preserves the originating PASID.
        recordComprehensiveFault(streamID, pasid, intermediatePA, stage2FaultType,
                               accessType, securityState, FaultStage::Stage2Only, currentTime, 1, 0);
        return stage2Result;
    }

    // Both stages successful - create final translation result
    // Note: stage1DataRef was introduced above for the stage-2 PTW security state.
    const TranslationData& stage1Data = stage1DataRef;
    const TranslationData& stage2Data = stage2Result.getValue();

    // ARM SMMU v3 spec: Final permissions are intersection of Stage-1 and Stage-2 permissions
    // This ensures that access is only allowed if both stages permit it
    PagePermissions finalPermissions;
    finalPermissions.read          = stage1Data.permissions.read    && stage2Data.permissions.read;
    finalPermissions.write         = stage1Data.permissions.write   && stage2Data.permissions.write;
    finalPermissions.execute       = stage1Data.permissions.execute && stage2Data.permissions.execute;
    finalPermissions.privilegedOnly = stage1Data.permissions.privilegedOnly || stage2Data.permissions.privilegedOnly;

    // ARM SMMU v3 spec: Validate final permissions against requested access
    if (!validateAccessPermissions(finalPermissions, accessType)) {
        // Permission fault after two-stage translation - final permission check failed
        recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                               accessType, securityState, FaultStage::BothStages, currentTime, 2, 0);
        return makeTranslationError(SMMUError::PagePermissionViolation);
    }

    // ARM IHI0070G.b §3.10/§3.10.2: Stage-2 alone determines the final PA security
    // state. The translatePage() call at stage-2 (above) already enforces the
    // correct security state match — it rejects a lookup if the requested security
    // state does not match the mapped entry.  Adding a second validateSecurityState()
    // call here comparing the incoming transaction security state against the stage-2
    // output security state is architecturally incorrect: the stage-2 output is the
    // authoritative PA security state and is not required to equal the incoming NS
    // bit by §3.10.2.  The check has been removed.

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
            case SMMUError::SubstreamDisabled:
                // BUG-CPP-F2 fix: S1DSS==0b00 abort — event (F_STREAM_DISABLED) was
                // already generated in translateUnlocked() before returning this error.
                // Map to StreamDisabled FaultType so the switch below takes the
                // no-op case and does not re-emit a duplicate event.
                faultType = FaultType::StreamDisabled;
                break;
            case SMMUError::InvalidStreamID:
                // BUG-CPP-DBGR-12 fix: §7.3.3 — C_BAD_STREAMID event was already generated
                // in translate() before reaching here.  Map to BadStreamID so the switch
                // below takes the explicit no-op case and does not emit a wrong event.
                faultType = FaultType::BadStreamID;
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
            // Security fault - log violation and notify security subsystem.
            // BUG-CPP-F1 fix: use the securityState parameter captured at
            // translation time rather than re-deriving it via
            // determineContextSecurityState().  The re-derive does a second
            // streamMap lookup that may return SecurityState::NonSecure as a
            // default if the stream was removed between translate() and here,
            // producing a stale/wrong expectedState in the fault record
            // (§7.3 fault record accuracy).  The caller already holds the
            // correct security state from when the stream was held.
            recordSecurityFault(streamID, pasid, iova, accessType, securityState, securityState);
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

        // BUG-CPP-1 fix: Each fault type must emit its own correct event code per
        // ARM IHI0070G.b.  The previous fall-through caused all six fault types to
        // emit F_ACCESS (0x12), which is only correct for AccessFlagFault.

        case FaultType::ContextDescriptorFormatFault:
            // §7.3.11: Context Descriptor format error → C_BAD_CD (0x0A).
            // Event may already have been emitted by the CD-validation path;
            // emit here only for paths that reach this case without a prior event.
            generateEvent(EventType::C_BAD_CD, streamID, pasid, iova, securityState);
            break;

        case FaultType::TranslationTableFormatFault:
        case FaultType::Level0TranslationFault:
        case FaultType::Level1TranslationFault:
        case FaultType::Level2TranslationFault:
        case FaultType::Level3TranslationFault:
            // §7.3.13: Translation table format error and level-N translation faults
            // → F_TRANSLATION (0x10), not F_ACCESS (0x12).
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState);
            break;

        case FaultType::AccessFlagFault:
            // §7.3.15: Access flag fault → F_ACCESS (0x12).  This is the ONLY
            // fault type in this group that correctly emits F_ACCESS.
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
    (void)streamID;    // reserved for future stream-aware fault classification
    (void)pasid;       // reserved for future PASID-aware fault classification
    (void)iova;        // reserved for future address-aware fault classification
    (void)accessType;  // reserved for future access-aware fault classification
    (void)securityState; // reserved for future security-aware fault classification
    
    // ARM §7.3.13–7.3.16 / FINDING-NEW-43: Fault classification must be based on
    // actual error cause, not on arbitrary IOVA value heuristics.
    // IOVA=0 is a valid mapped address (not an "access fault").
    // High IOVAs are only an address-size fault when they exceed the configured
    // input address size — that is handled in handleTranslationFailure() via the
    // SMMUError::InvalidAddress case.  Here we return the generic default.

    // Default: TranslationFault — callers supply the real error code via result.getError().
    // Note: the previous lock acquisition + discarded streamMap.find() here were dead
    // code with no effect on the return value; removed to eliminate spurious stripe-lock
    // contention on the hot translation path (BUG-08).
    return FaultType::TranslationFault;
}

void SMMU::handleTranslationFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    // ARM SMMU v3 spec: Translation fault recovery mechanisms
    // In a full implementation, this could trigger:
    // - Demand paging from storage
    // - Page table updates
    // - Memory allocation

    // BUG-CPP-M03 fix: pass the actual securityState so that the TLB entry
    // (tagged with the correct security state at insertion time) is properly
    // evicted.  Previously the NonSecure default was used, leaving Secure-tagged
    // entries in the cache after a fault on a Secure stream.
    if (tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK, securityState);
    }
}

void SMMU::handlePermissionFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) {
    (void)accessType; // Reserved for future access-type-specific recovery logic
    // ARM SMMU v3 spec: Permission fault recovery mechanisms
    // This could implement:
    // - Security policy checks
    // - Permission escalation requests
    // - Access logging for security audit

    // BUG-CPP-M03 fix: pass the actual securityState so that a Secure TLB entry
    // is correctly evicted rather than leaving it in the cache.
    if (tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK, securityState);
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
    (void)accessType; // Reserved for future access-type-specific recovery logic
    // ARM SMMU v3 spec: Access fault recovery mechanisms
    // This could implement:
    // - Retry logic with backoff
    // - Alternative access paths
    // - Hardware fault recovery

    // BUG-CPP-M03 fix: pass the actual securityState so that a Secure TLB entry
    // is correctly evicted when an access fault occurs on a Secure stream.
    if (tlbCache) {
        tlbCache->invalidate(streamID, pasid, iova & ~PAGE_MASK, securityState);
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
        eventqCons.store(advanceQueueIndex(eventqCons.load(std::memory_order_relaxed), eventqLog2Size), std::memory_order_release);
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
    eventqProd.store(0, std::memory_order_release);
    eventqCons.store(0, std::memory_order_release);
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
        // ARM §3.5.1 / §6.3.17: Command queue full is a software-producer concern
        // handled by PROD/CONS index comparison.  No GERROR bit is defined for this
        // condition: GERROR.MSI_CMDQ_ABT_ERR (bit[4]) is for CMD_SYNC MSI write
        // aborts only, and GERROR.CMDQ_ERR (bit[0]) is for hardware command
        // consumption errors — neither applies to a simple queue-full rejection.
        return makeVoidError(SMMUError::CommandQueueFull);
    }
    CommandEntry timestampedCommand = command;
    timestampedCommand.timestamp = getCurrentTimestamp();
    commandQueue.push_back(timestampedCommand);
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-01)
    cmdqProd.store(advanceQueueIndex(cmdqProd.load(std::memory_order_relaxed), cmdqLog2Size), std::memory_order_release);
    return makeVoidSuccess();
}

void SMMU::processCommandQueue() {
    // §4.1.2 / CT-33: When CR0.CMDQEN=0, command queue processing is disabled.
    // BUG-CPP-NEW-1 fix: use load() for the atomic cr0_.
    if ((cr0_.load(std::memory_order_acquire) & CR0_CMDQEN) == 0u) {
        return;
    }

    // ARM §6.3.17: Do not process commands when GERROR.CMDQ_ERR is active.
    // An error is active when GERROR[x] != GERRORN[x] (unacknowledged).
    // Software must write GERRORN via clearGerror() to acknowledge before
    // queue processing can resume.  BUG-03/SPEC-09.
    // BUG-CPP-NEW-1 fix: use load(relaxed) — this function runs under queueMutex
    // which provides the required memory ordering between writer and this reader.
    if ((gerrorStatus.load(std::memory_order_relaxed) ^ gerrorNStatus.load(std::memory_order_relaxed)) & GERROR_CMDQ_ERR) {
        return;
    }

    // ARM SMMU v3 spec: Process command queue with proper ordering.
    // BUG-03 fix: protect commandQueue with queueMutex.
    // BUG-CPP-C01 fix: Use unique_lock so we can temporarily release queueMutex
    // before acquiring a stream stripe lock inside the CMD_SYNC handler.
    // Lock ordering invariant: stripe lock must NEVER be acquired while
    // queueMutex is held (translate() acquires stripe -> queueMutex via
    // generateEvent(); holding both in the opposite order causes an ABBA
    // deadlock).  Releasing queueMutex before taking the stripe lock, then
    // re-acquiring queueMutex to continue the loop, preserves the invariant.
    std::unique_lock<std::recursive_mutex> lock(queueMutex);
    while (!commandQueue.empty()) {
        // BUG-04 fix: copy command by value before pop_front() so that the
        // reference is not dangling when we inspect command.type afterwards.
        CommandEntry command = commandQueue.front();
        commandQueue.pop_front();
        // ARM §3.5.1: Advance consumer index on dequeue (FINDING-M-01)
        cmdqCons.store(advanceQueueIndex(cmdqCons.load(std::memory_order_relaxed), cmdqLog2Size), std::memory_order_release);

        // Process the command based on type
        processCommand(command, lock);

        // ARM §6.3.17: Commands must not be processed while GERROR.CMDQ_ERR is active.
        // If processCommand() signalled CMDQ_ERR, halt queue processing immediately.
        // Active = GERROR[x] != GERRORN[x] (unacknowledged).  BUG-03/SPEC-09.
        if ((gerrorStatus.load(std::memory_order_relaxed) ^ gerrorNStatus.load(std::memory_order_relaxed)) & GERROR_CMDQ_ERR) {
            break;
        }

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
                // BUG-NEW-04 fix: set GERROR.CMDQ_ERR so software can detect
                // the CERROR_ILL halt, per ARM §4.8 / §6.3.17.
                // BUG-NEW-CPP-1 fix: use signalGerror() CAS loop instead of
                // the TOCTOU load-compare-fetch_xor pattern.
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // §4.8 / FINDING-NEW-27: CS=0b00 (SIG_NONE) → no completion signal.
            if (command.cs != 0) {
                // FINDING-NEW-39: derive security state from the stream config rather than
                // hardcoding NonSecure, per ARM §4.8.
                // BUG-CPP-C01 fix: release queueMutex before acquiring the stripe lock to
                // prevent the ABBA deadlock: translate() holds stripe->queueMutex whereas
                // the old code held queueMutex->stripe here.
                SecurityState syncEventSecState = SecurityState::NonSecure;
                {
                    size_t syncStripe = getStreamStripe(command.streamID);
                    lock.unlock();
                    {
                        std::lock_guard<std::mutex> syncLock(streamLockStripes[syncStripe]);
                        auto syncIt = streamMap.find(command.streamID);
                        if (syncIt != streamMap.end()) {
                            syncEventSecState = syncIt->second->getStreamConfiguration().securityState;
                        }
                    }
                    lock.lock();
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
    cmdqProd.store(0, std::memory_order_release);
    cmdqCons.store(0, std::memory_order_release);
}

// Task 5.3: PRI Queue for Page Requests (Task 5.3.3)
void SMMU::submitPageRequest(const PRIEntry& request) {
    // BUG-03 fix: protect priQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);

    if (priQueue.size() >= maxPRIQueueSize) {
        // BUG-NEW-02 fix: ARM §8.1 specifies that PRIQ_PROD.OVFLG is toggled only
        // when transitioning from the non-overflow state to the overflow state.
        // Overflow is "active" when bit 31 of priqProd differs from bit 31 of
        // priqCons (analogous to the EVENTQ OVFLG / OVACKFLG mechanism).
        // While overflow is active, new entries are inhibited — the oldest entries
        // must NOT be evicted.  The old pop_front() eviction was incorrect.
        uint32_t priqProdVal = priqProd.load(std::memory_order_relaxed);
        uint32_t priqConsVal = priqCons.load(std::memory_order_relaxed);
        if (((priqProdVal >> 31) & 1u) == ((priqConsVal >> 31) & 1u)) {
            // Not yet overflowed: transition to overflow state by toggling OVFLG.
            priqProd.store(priqProdVal ^ (1u << 31), std::memory_order_release);
        }
        // If already overflowed (OVFLG != OVACKFLG bit in priqCons), leave
        // OVFLG unchanged and inhibit the new entry (do not push_back).
        return;
    }

    PRIEntry timestampedRequest = request;
    timestampedRequest.timestamp = getCurrentTimestamp();
    priQueue.push_back(timestampedRequest);
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-08)
    priqProd.store(advanceQueueIndex(priqProd.load(std::memory_order_relaxed), priqLog2Size), std::memory_order_release);
    // §7.3.19 / FINDING-NEW-32: carry the request's security state, not a hardcoded NonSecure.
    // Only generate the E_PAGE_REQUEST event when the entry was actually enqueued.
    generateEvent(EventType::E_PAGE_REQUEST, request.streamID, request.pasid, request.requestedAddress, request.securityState);
}

void SMMU::processPRIQueue() {
    // §CT-33: When CR0.PRIQEN=0, PRI queue processing is disabled.
    // BUG-CPP-NEW-1 fix: use load() for the atomic cr0_.
    if ((cr0_.load(std::memory_order_acquire) & CR0_PRIQEN) == 0u) {
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
            // BUG-NEW-03 fix: advance consumer index to match the dequeue.
            priqCons.store(advanceQueueIndex(priqCons.load(std::memory_order_relaxed), priqLog2Size), std::memory_order_release);
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
    // BUG-NEW2-02 fix: reset PROD/CONS indices to match the empty queue,
    // mirroring clearCommandQueue() and clearEventQueue() per ARM §3.5.1.
    priqProd.store(0, std::memory_order_release);
    priqCons.store(0, std::memory_order_release);
}

size_t SMMU::getPRIQueueSize() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return priQueue.size();
}

// Task 5.3: Cache Invalidation Command Handling (Task 5.3.4)
void SMMU::executeInvalidationCommand(const CommandEntry& command) {
    // ARM SMMU v3 spec: Execute cache invalidation commands
    switch (command.type) {
        case CommandType::CFGI_STE: {
            // BUG-NEW2-04 fix: check stream existence before invalidating.
            // ARM §4.3.1: CMD_CFGI_STE with an unknown StreamID must generate
            // C_BAD_STREAMID and set GERROR.CMDQ_ERR.
            size_t cfgiStripe = getStreamStripe(command.streamID);
            bool streamFound = false;
            {
                std::lock_guard<std::mutex> stripeLock(streamLockStripes[cfgiStripe]);
                streamFound = (streamMap.find(command.streamID) != streamMap.end());
            }
            if (!streamFound) {
                // BUG-14 fix / ARM §7.3.3: use command.securityState, not hardcoded
                // NonSecure.  §7.3 requires the event to be recorded in the Event queue
                // matching the security state of the StreamID causing the event.
                // §6.3.12 SMMU_CR2.RECINVSID: only record the C_BAD_STREAMID event when
                // RECINVSID==1.  GERROR.CMDQ_ERR is always toggled unconditionally.
                if ((cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u) {
                    generateEvent(EventType::C_BAD_STREAMID, command.streamID, command.pasid,
                                  command.startAddress, command.securityState);
                }
                // BUG-CPP-DBGR-2 fix: wrap the GERROR XOR-toggle in queueMutex.
                // BUG-NEW-CPP-1 fix: use signalGerror() CAS loop instead of
                // the TOCTOU load-compare-fetch_xor pattern.  signalGerror() is
                // safe to call with or without queueMutex held; the CAS loop
                // handles concurrent modifications from clearGerror().
                {
                    std::lock_guard<std::recursive_mutex> qLock(queueMutex);
                    signalGerror(GERROR_CMDQ_ERR);
                }
                break;
            }
            invalidateStreamCache(command.streamID);
            break;
        }

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

                // BUG-CPP-M01 fix: acquire all stripe locks in index order before
                // iterating streamMap.  Without the locks, a concurrent
                // configureStream() or removeStream() can modify the map during
                // iteration causing iterator invalidation (undefined behaviour).
                // This matches the pattern used by getStreamCount() and
                // setGlobalFaultMode().
                std::vector<std::unique_lock<std::mutex>> rangeLocks;
                rangeLocks.reserve(NUM_STREAM_STRIPES);
                for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
                    rangeLocks.emplace_back(streamLockStripes[i]);
                }
                for (const auto& pair : streamMap) {
                    if ((pair.first >> prefixBits) == cmdPrefix) {
                        invalidateStreamCache(pair.first);
                    }
                }
                // rangeLocks released here (RAII)
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
            // BUG-NEW3-01 fix: derive security state BEFORE calling
            // executeATCInvalidationCommand so TLB entries are invalidated
            // with the correct security state (Secure/Realm/Root), not just
            // the default NonSecure.
            SecurityState atcSecState = SecurityState::NonSecure;
            {
                size_t atcStripe = getStreamStripe(command.streamID);
                std::lock_guard<std::mutex> atcLock(streamLockStripes[atcStripe]);
                auto atcIt = streamMap.find(command.streamID);
                if (atcIt != streamMap.end()) {
                    atcSecState = atcIt->second->getStreamConfiguration().securityState;
                }
            }
            // §4.5.1: ATC_INVALIDATE_COMPLETION is generated ONLY for CMD_ATC_INV.
            executeATCInvalidationCommand(command.streamID, command.pasid,
                                        command.startAddress, command.endAddress, atcSecState);
            generateEvent(EventType::ATC_INVALIDATE_COMPLETION, command.streamID, command.pasid,
                          command.startAddress, atcSecState);
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

void SMMU::executeATCInvalidationCommand(StreamID streamID, PASID pasid, IOVA startAddr, IOVA endAddr, SecurityState securityState) {
    // ARM SMMU v3 spec: Execute Address Translation Cache invalidation

    if (tlbCache) {
        if (startAddr == 0 && endAddr == 0) {
            // Global invalidation for stream/PASID — these helpers invalidate all
            // entries for the stream/PASID regardless of security state.
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
            // BUG-NEW3-01 fix: pass the caller-supplied security state so that
            // Secure/Realm/Root TLB entries are correctly targeted.
            while (currentAddr <= alignedEndAddr) {
                tlbCache->invalidate(streamID, pasid, currentAddr, securityState);
                if (currentAddr > UINT64_MAX - PAGE_SIZE) {
                    break; // Next increment would wrap — all pages covered
                }
                currentAddr += PAGE_SIZE;
            }
        }
    }
}

// BUG-NEW-09 fix: executeInvalidationCommandLocked is the internal version of
// executeInvalidationCommand used when queueMutex is already held by
// processCommandQueue.  For code paths that acquire stripe locks (CFGI_STE_RANGE,
// ATC_INV), it releases queueMutex first to maintain the lock ordering invariant:
// stripe_lock must never be acquired while queueMutex is held.
void SMMU::executeInvalidationCommandLocked(const CommandEntry& command, std::unique_lock<std::recursive_mutex>& queueLock) {
    // ARM SMMU v3 spec: Execute cache invalidation commands (called with queueMutex held)
    switch (command.type) {
        case CommandType::CFGI_STE:
            // Stream Table Entry invalidation — invalidate specific stream.
            // PRE-CONDITION: callers must have already verified that command.streamID
            // is a known StreamID (generating C_BAD_STREAMID + GERROR_CMDQ_ERR if
            // not found) before calling this function.  ARM §4.3.1 does not define
            // any error condition for a CFGI_STE targeting a non-existent stream
            // (it is effectively a no-op); the stream-existence check is a model-
            // internal safety gate applied upstream in processCommand().
            invalidateStreamCache(command.streamID);
            break;

        case CommandType::CFGI_ALL:
            // ARM §4.3.2: CMD_CFGI_ALL (range==31) or CMD_CFGI_STE_RANGE (range<31).
            // NOTE: shifting uint32_t by 32 bits is UB, so range==31 is handled separately.
            if (command.range == 31) {
                // CMD_CFGI_ALL — full global STE-cache / TLB invalidation
                invalidateTranslationCache();
            } else {
                // CMD_CFGI_STE_RANGE — invalidate only streams matching the upper-bit prefix.
                uint32_t prefixBits = static_cast<uint32_t>(command.range) + 1u;
                StreamID cmdPrefix = command.streamID >> prefixBits;

                // BUG-NEW-09 fix: release queueMutex before acquiring all stripe locks.
                // Lock ordering invariant: stripe locks must never be acquired while
                // queueMutex is held (ABBA deadlock with translate()).
                queueLock.unlock();
                {
                    std::vector<std::unique_lock<std::mutex>> rangeLocks;
                    rangeLocks.reserve(NUM_STREAM_STRIPES);
                    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
                        rangeLocks.emplace_back(streamLockStripes[i]);
                    }
                    for (const auto& pair : streamMap) {
                        if ((pair.first >> prefixBits) == cmdPrefix) {
                            invalidateStreamCache(pair.first);
                        }
                    }
                } // rangeLocks released here (RAII)
                queueLock.lock();
            }
            break;

        case CommandType::CFGI_CD:
            // ARM §4.3.3: Invalidate cached CD for (StreamID, SSID/PASID).
            invalidatePASIDCache(command.streamID, command.pasid);
            break;

        case CommandType::CFGI_CD_ALL:
            // ARM §4.3.4: Invalidate all cached CDs for StreamID.
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
            // BUG-NEW3-01 fix: derive security state BEFORE calling
            // executeATCInvalidationCommand so TLB entries are invalidated with
            // the correct security state (Secure/Realm/Root).
            // Release queueMutex before acquiring the stripe lock to maintain the
            // lock ordering invariant: stripe_lock must never be acquired while
            // queueMutex is held.
            SecurityState atcSecState = SecurityState::NonSecure;
            {
                size_t atcStripe = getStreamStripe(command.streamID);
                queueLock.unlock();
                {
                    std::lock_guard<std::mutex> atcLock(streamLockStripes[atcStripe]);
                    auto atcIt = streamMap.find(command.streamID);
                    if (atcIt != streamMap.end()) {
                        atcSecState = atcIt->second->getStreamConfiguration().securityState;
                    }
                }
                queueLock.lock();
            }
            // §4.5.1: ATC_INVALIDATE_COMPLETION is generated ONLY for CMD_ATC_INV.
            executeATCInvalidationCommand(command.streamID, command.pasid,
                                        command.startAddress, command.endAddress, atcSecState);
            generateEvent(EventType::ATC_INVALIDATE_COMPLETION, command.streamID, command.pasid,
                          command.startAddress, atcSecState);
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

// Task 5.3: Helper Methods
// BUG-NEW-08 fix: processCommand accepts queueLock so it can temporarily release
// queueMutex before acquiring stripe locks (CFGI_STE case), preventing the ABBA
// deadlock with translate() which holds stripe_lock -> queueMutex.
void SMMU::processCommand(const CommandEntry& command, std::unique_lock<std::recursive_mutex>& queueLock) {
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
            // BUG-NEW-08 fix: release queueMutex before acquiring the stripe lock.
            // Lock ordering invariant: stripe_lock must never be acquired while
            // queueMutex is held (translate() holds stripe_lock and then acquires
            // queueMutex via generateEvent(), creating an ABBA deadlock otherwise).
            bool streamFound = false;
            {
                size_t cfgiStripe = getStreamStripe(command.streamID);
                queueLock.unlock();
                {
                    std::lock_guard<std::mutex> cfgiLock(streamLockStripes[cfgiStripe]);
                    streamFound = (streamMap.find(command.streamID) != streamMap.end());
                }
                queueLock.lock();
            }
            if (!streamFound) {
                // BUG-14 fix / ARM §7.3.3: use command.securityState (second call site).
                // §6.3.12 SMMU_CR2.RECINVSID: only record the C_BAD_STREAMID event when
                // RECINVSID==1.  GERROR.CMDQ_ERR is always toggled unconditionally.
                if ((cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u) {
                    generateEvent(EventType::C_BAD_STREAMID, command.streamID, command.pasid,
                                  command.startAddress, command.securityState);
                }
                // BUG-NEW-CPP-1 fix: use signalGerror() CAS loop instead of
                // the TOCTOU load-compare-fetch_xor pattern.
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // Known StreamID — proceed with normal STE cache invalidation.
            executeInvalidationCommandLocked(command, queueLock);
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
            executeInvalidationCommandLocked(command, queueLock);
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
                    priqCons.store(advanceQueueIndex(priqCons.load(std::memory_order_relaxed), priqLog2Size), std::memory_order_release);
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
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::DPTI_ALL:
        case CommandType::DPTI_PA:
            // Dirty page tracking invalidation — software model: no-op (log only).
            break;

        default:
            // Unknown command type — ARM §6.3.17: set CMDQ_ERR (FINDING-M-06)
            // BUG-NEW-05 fix: do not generate C_BAD_STE — the spec defines no
            // event type for "unknown command opcode".  GERROR.CMDQ_ERR is the
            // correct signal to software.
            // BUG-NEW-CPP-1 fix: use signalGerror() CAS loop instead of
            // the TOCTOU load-compare-fetch_xor pattern.
            signalGerror(GERROR_CMDQ_ERR);
            break;
    }
}

// ARM §6.3.17: Read active (unacknowledged) SMMU_GERROR bits.
// Active errors are those where GERROR[x] != GERRORN[x].
// Returns GERROR XOR GERRORN so that acknowledged errors appear cleared.
// BUG-03/SPEC-09: ARM IHI0070G.b §6.3.19/6.3.20.
uint32_t SMMU::getGerror() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return gerrorStatus ^ gerrorNStatus;
}

// ARM §6.3.18: Read SMMU_GERRORN register (software acknowledgement register).
// Returns the raw GERRORN value — useful for spec-compliance verification.
uint32_t SMMU::getGerrorN() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return gerrorNStatus;
}

// ARM §6.3.18: Software writes SMMU_GERRORN to acknowledge SMMU_GERROR bits.
// BUG-03/SPEC-09: Only toggle GERRORN[x] for bits that are currently active
// (GERROR[x] != GERRORN[x]).  Toggling GERRORN[x] to match GERROR[x] marks
// the error as inactive.  Writing a bit that is already inactive is a no-op.
// ARM IHI0070G.b §6.3.18: "Software writes GERRORN to acknowledge GERROR."
void SMMU::clearGerror(uint32_t bits) {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // Restrict the toggle to bits that are currently active.
    uint32_t activeBits = gerrorStatus ^ gerrorNStatus;
    gerrorNStatus ^= (bits & activeBits);
}

// BUG-NEW-CPP-1 fix: CAS-loop GERROR toggle.
// Atomically toggles the GERROR bits specified by `bits` only when they are
// currently inactive (GERROR[x] == GERRORN[x]).  Uses a CAS retry loop so that
// a concurrent clearGerror() cannot change gerrorNStatus between the comparison
// and the fetch_xor, eliminating the TOCTOU window in the previous pattern.
// ARM IHI0070G.b §6.3.19: "SMMU does not toggle bit[x] if already active."
// Must be called while queueMutex is held (or not needed — the CAS loop is
// safe with or without the external lock).
void SMMU::signalGerror(uint32_t bits) {
    while (true) {
        uint32_t cur_gerror  = gerrorStatus.load(std::memory_order_acquire);
        uint32_t cur_gerrorn = gerrorNStatus.load(std::memory_order_acquire);
        // Re-read gerrorNStatus to detect concurrent clearGerror() modifications.
        uint32_t reread_gerrorn = gerrorNStatus.load(std::memory_order_acquire);
        if (reread_gerrorn != cur_gerrorn) {
            continue;  // gerrorNStatus changed between reads — retry
        }
        uint32_t reread_gerror = gerrorStatus.load(std::memory_order_acquire);
        if (reread_gerror != cur_gerror) {
            continue;  // gerrorStatus changed between reads — retry
        }
        // active bits: those where GERROR[x] != GERRORN[x]
        uint32_t active   = cur_gerror ^ cur_gerrorn;
        // Only toggle bits that are currently inactive
        uint32_t inactive = bits & ~active;
        if (inactive == 0) {
            break;  // all requested bits already active — no-op per ARM §6.3.19
        }
        uint32_t new_gerror = cur_gerror ^ inactive;
        if (gerrorStatus.compare_exchange_weak(cur_gerror, new_gerror,
                std::memory_order_acq_rel, std::memory_order_acquire)) {
            break;
        }
        // CAS failed — another thread modified gerrorStatus; retry
    }
}

// ARM §6.3.9 SMMU_CR0.SMMUEN and §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01, FINDING-NEW-09)
// §6.3.9 SMMU_CR0 register (CT-33)
void SMMU::setCR0(uint32_t value) {
    // BUG-CPP-DBGR-1 fix: §6.3.9 — derive smmuen_ from `value` directly (not by
    // re-reading cr0_ non-atomically), and use release stores for both so that
    // any thread that observes smmuen_==true is also guaranteed to see the updated cr0_.
    cr0_.store(value, std::memory_order_release);
    smmuen_.store((value & CR0_SMMUEN) != 0u, std::memory_order_release);
}

uint32_t SMMU::getCR0() const {
    // BUG-R2-CPP-2 fix: use explicit acquire load to match the release stores
    // in setCR0()/enable()/disable()/reset() and to be consistent with all
    // other atomic reads in this class.  The previous implicit conversion used
    // seq_cst, which is semantically correct but unnecessarily expensive.
    return cr0_.load(std::memory_order_acquire);
}

// §6.3.12 SMMU_CR2 register (RECINVSID).
// RECINVSID (bit 1): gates C_BAD_STREAMID event recording in the event queue.
// Reset value is 0 (events suppressed) per ARM IHI0070G.b §6.3.12.
void SMMU::setCR2(uint32_t value) {
    cr2_.store(value, std::memory_order_release);
}

uint32_t SMMU::getCR2() const {
    return cr2_.load(std::memory_order_acquire);
}

// §6.3.4 SMMU_STRTAB_BASE_CFG.LOG2SIZE (CT-04)
void SMMU::setStrtabLog2Size(uint8_t log2size) {
    // BUG-CPP-01 fix: clamp to 32 — values > 32 would produce UB shifts and
    // have no valid meaning (StreamID is 32 bits, so 2^32 covers the full range).
    if (log2size > 32u) {
        log2size = 32u;
    }
    // BUG-NEW-CPP-1 fix: use release store so that concurrent translate() readers
    // that use acquire loads observe the updated value without a data race.
    strtabLog2Size_.store(log2size, std::memory_order_release);
}

uint8_t SMMU::getStrtabLog2Size() const {
    // BUG-NEW-CPP-1 fix: acquire load pairs with the release store in setStrtabLog2Size().
    return strtabLog2Size_.load(std::memory_order_acquire);
}

void SMMU::enable() {
    // BUG-NEW-CPP-4 fix: use fetch_or (atomic read-modify-write) instead of
    // the previous non-atomic load-then-store.  The old pattern allowed a
    // concurrent setCR0() write between the load and store to be silently lost.
    // Sets SMMUEN | EVENTQEN | CMDQEN atomically (PRIQEN deliberately excluded:
    // the old implementation incorrectly added PRIQEN, causing any bits set via
    // setCR0() to be permanently augmented after an enable()+disable() cycle).
    // acquire/release ordering ensures visibility to concurrent translate() readers.
    cr0_.fetch_or(CR0_SMMUEN | CR0_EVENTQEN | CR0_CMDQEN, std::memory_order_acq_rel);
    smmuen_.store(true, std::memory_order_release);
}

void SMMU::disable() {
    // BUG-NEW-CPP-4 fix: use fetch_and (atomic read-modify-write) to clear only
    // CR0_SMMUEN without touching any other bits.  The previous load-then-store
    // pattern allowed a concurrent setCR0() write between the load and store to
    // be silently lost.
    cr0_.fetch_and(~static_cast<uint32_t>(CR0_SMMUEN), std::memory_order_acq_rel);
    smmuen_.store(false, std::memory_order_release);
}

bool SMMU::isEnabled() const {
    // BUG-CPP-3 fix: ARM IHI0070G.b §6.3.9 — SMMU_CR0.SMMUEN (bit 0) is the
    // single authoritative source for the global enable state.  Reading smmuen_
    // instead of cr0_ created a potential split-brain: the shadow bool and the
    // register could diverge in concurrent code.  Always derive from cr0_.
    // BUG-CPP-B fix: use explicit acquire ordering to pair with the release
    // stores in enable(), disable(), and setCR0(), establishing a
    // happens-before relationship for all concurrent readers.
    return (cr0_.load(std::memory_order_acquire) & CR0_SMMUEN) != 0u;
}

void SMMU::setGbpaAbort(bool abort) {
    // BUG-CPP-A fix: use explicit release ordering so that any preceding writes
    // are visible to threads that subsequently acquire-load gbpaAbort_.
    gbpaAbort_.store(abort, std::memory_order_release);
}

bool SMMU::isGbpaAbort() const {
    // BUG-CPP-A fix: use explicit acquire ordering to pair with the release
    // store in setGbpaAbort(), establishing a happens-before relationship.
    return gbpaAbort_.load(std::memory_order_acquire);
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
    // BUG-CPP-NEW-1 fix: use load() for the atomic cr0_.
    if (!isStall && (cr0_.load(std::memory_order_acquire) & CR0_EVENTQEN) == 0u) {
        return;
    }

    // ARM SMMU v3 spec: Generate event for event queue processing.
    // BUG-03 fix: protect eventQueue with queueMutex. Uses recursive_mutex so
    // that callers already holding queueMutex (e.g. processCommandQueue) can
    // safely call this without deadlocking.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    if (eventQueue.size() >= maxEventQueueSize) {
        if (!isStall) {
            // §3.5.3 / ARM §7.4: Non-stall events may be discarded when queue is full.
            // Toggle EVENTQ_PROD.OVFLG (bit 31) so software can detect overflow by
            // comparing against its saved OVACKFLG in SMMU_EVENTQ_CONS.
            // GERROR_EVENTQ_ABT_ERR is NOT set here — that bit signals a memory system
            // abort on the event queue write, not a software-visible queue-full condition.
            //
            // BUG-NEW-01 fix: Only toggle OVFLG when transitioning from the non-overflow
            // state to the overflow state.  Overflow is "active" when the OVFLG bit
            // (bit 31 of eventqProd) differs from the OVACKFLG bit (bit 31 of
            // eventqCons).  If overflow is already active, further dropped events must
            // NOT toggle OVFLG again — doing so would clear the bit and hide the
            // overflow condition from software (ARM §7.4).
            {
                uint32_t eqProdVal = eventqProd.load(std::memory_order_relaxed);
                uint32_t eqConsVal = eventqCons.load(std::memory_order_relaxed);
                if (((eqProdVal >> 31) & 1u) == ((eqConsVal >> 31) & 1u)) {
                    // Not yet overflowed: transition to overflow state by toggling OVFLG.
                    eventqProd.store(eqProdVal ^ (1u << 31), std::memory_order_release);
                }
            }
            // If already overflowed (OVFLG != OVACKFLG), leave OVFLG unchanged.
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
    eventqProd.store(advanceQueueIndex(eventqProd.load(std::memory_order_relaxed), eventqLog2Size), std::memory_order_release);
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
    // ARM SMMU v3 spec §3.10: Security state validation logic.
    // Each security domain is strictly isolated; a requestor may only access
    // resources mapped in its own security domain.
    // BUG-CPP-08 fix: Secure requestors must NOT access NonSecure mappings —
    // allowing it violates TrustZone isolation.  Only Root may cross boundaries.

    switch (requestedState) {
        case SecurityState::NonSecure:
            return contextState == SecurityState::NonSecure;

        case SecurityState::Secure:
            // §3.10: Secure transactions may only target Secure PA space.
            return (contextState == SecurityState::Secure);

        case SecurityState::Realm:
            return contextState == SecurityState::Realm;

        case SecurityState::Root:
            // Root (SMMUv3.3 RME §3.10) can access all PA spaces.
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

    // BUG-NEW-07 fix: return the stream's actual configured security state
    // rather than always returning NonSecure.
    return streamIt->second->getStreamConfiguration().securityState;
}

// ARM SMMU v3 Comprehensive Fault Syndrome Generation Methods

FaultSyndrome SMMU::generateFaultSyndrome(FaultType faultType, FaultStage stage, AccessType accessType, 
                                         SecurityState securityState, uint8_t faultLevel, 
                                         PrivilegeLevel privLevel, uint16_t contextDescIndex) const {
    (void)securityState; // Suppress unused parameter warning - reserved for future security-aware fault syndrome generation
    
    // Generate ARM SMMU v3 compliant fault syndrome.
    //
    // BUG-CPP-2 fix: The original code only checked the single-variant Write and
    // Execute types, leaving ReadWrite, WritePrivileged, ReadWritePrivileged, and
    // ExecutePrivileged with wrong RnW/InD bits.
    //
    // ARM IHI0070G.b §7.3.13–7.3.15, line 35325:
    //   RnW (bit [3]) = 1 for any access that contains a write component.
    //   InD (bit [2]) = 1 for instruction fetches, BUT ONLY when RnW == 0.
    //   "InD == 0 when RnW == 0" is a spec-required constraint.
    bool writeAccess = (accessType == AccessType::Write             ||
                        accessType == AccessType::ReadWrite         ||
                        accessType == AccessType::WritePrivileged   ||
                        accessType == AccessType::ReadWritePrivileged);
    // InD is set for execute-class accesses only when there is no write component.
    // Per spec: InD must be 0 when RnW (writeAccess) is 1.
    bool instructionFetch = (!writeAccess) &&
                            (accessType == AccessType::Execute ||
                             accessType == AccessType::ExecutePrivileged);
    
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
    // Suppress unused-parameter warning: accessType is retained in the signature
    // for API stability and potential future use (e.g. AxPROT[1] decoding).
    (void)accessType;
    // TODO(BUG-CPP-09): Privilege level should be a transaction attribute supplied
    // by the initiating master (ARM SMMU v3 §3.6 STREAM_WORLD / AxPROT[1]).
    // This is a heuristic approximation only; accurate privilege level requires the
    // initiating master to supply it as part of the transaction descriptor.
    //
    // BUG-CPP-5 fix: Use security state only; Root→EL3 and Realm→EL2 remain
    // correct (those states canonically imply specific exception levels).
    // Secure and NonSecure default to EL0 per ARM §13.1.3 (spec default for
    // unknown privilege is Unprivileged).
    switch (securityState) {
        case SecurityState::Root:
            return PrivilegeLevel::EL3;
        case SecurityState::Realm:
            return PrivilegeLevel::EL2;
        case SecurityState::Secure:
        case SecurityState::NonSecure:
        default:
            // BUG-CPP-5 fix: ARM §13.1.3 states the spec default for missing
            // PRIV is Unprivileged (EL0), regardless of security state.
            // The previous heuristic (EL1 for data, EL0 for execute) was wrong:
            // Secure/NonSecure do not imply a specific exception level; a
            // Secure-EL2 hypervisor running DMA would receive incorrect EL1 bits
            // in the fault syndrome.  Return EL0 (Unprivileged) for ALL access
            // types to conform to §13.1.3.
            return PrivilegeLevel::EL0;
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
            // BUG-CPP-F fix: Derive threshold from configured maxPASize (ARM §3.4 / §STE.S2PS)
            // instead of hardcoded 48-bit constant. Valid addresses on systems with 52-bit OAS
            // (maxPASize = 52 bits) would be incorrectly classified as AddressSizeFault otherwise.
            {
                const uint64_t oasBits = configuration.getAddressConfiguration().maxPASize;
                const uint64_t maxValidAddr = (oasBits < 64u)
                    ? ((static_cast<uint64_t>(1) << oasBits) - 1u)
                    : UINT64_MAX;
                if (iova > maxValidAddr) {
                    return FaultType::AddressSizeFault;
                }
            }
            return FaultType::TranslationFault;
    }
}

// Configuration management methods

const SMMUConfiguration& SMMU::getConfiguration() const {
    return configuration;
}

VoidResult SMMU::updateConfiguration(const SMMUConfiguration& config) {
    // BUG-CPP-E fix: validate before acquiring stripe locks to avoid latent
    // deadlock hazard and reduce lock hold time during expensive validation.
    // validateConfigurationUpdate() is a const operation on the passed-in
    // config object and requires no stripe lock protection.
    VoidResult validationResult = validateConfigurationUpdate(config);
    if (!validationResult.isOk()) {
        return validationResult;
    }

    // Lock all stripes to atomically swap the configuration
    std::vector<std::unique_lock<std::mutex>> locks;
    for (size_t i = 0; i < NUM_STREAM_STRIPES; ++i) {
        locks.emplace_back(streamLockStripes[i]);
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
        // BUG-NEW-CPP-2 fix: release store for atomic<bool> cachingEnabled.
        cachingEnabled.store(cacheConfig.enableCaching, std::memory_order_release);
        
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
    // BUG-NEW-CPP-2 fix: release store for atomic<bool> cachingEnabled.
    cachingEnabled.store(cacheConfig.enableCaching, std::memory_order_release);

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
    // BUG-NEW-CPP-2 fix: use acquire load now that cmdqProd is std::atomic<uint32_t>.
    return cmdqProd.load(std::memory_order_acquire);
}

uint32_t SMMU::getCmdqConsIndex() const {
    return cmdqCons.load(std::memory_order_acquire);
}

uint32_t SMMU::getEventqProdIndex() const {
    return eventqProd.load(std::memory_order_acquire);
}

uint32_t SMMU::getEventqConsIndex() const {
    return eventqCons.load(std::memory_order_acquire);
}

uint32_t SMMU::getPriqProdIndex() const {
    return priqProd.load(std::memory_order_acquire);
}

uint32_t SMMU::getPriqConsIndex() const {
    return priqCons.load(std::memory_order_acquire);
}

bool SMMU::isCmdqEmptyByIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return cmdqProd.load(std::memory_order_relaxed) == cmdqCons.load(std::memory_order_relaxed);
}

bool SMMU::isEventqEmptyByIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return eventqProd.load(std::memory_order_relaxed) == eventqCons.load(std::memory_order_relaxed);
}

uint32_t SMMU::getCmdqOccupiedEntries() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return queueOccupied(cmdqProd.load(std::memory_order_relaxed), cmdqCons.load(std::memory_order_relaxed), cmdqLog2Size);
}

uint32_t SMMU::getEventqOccupiedEntries() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return queueOccupied(eventqProd.load(std::memory_order_relaxed), eventqCons.load(std::memory_order_relaxed), eventqLog2Size);
}

uint32_t SMMU::getCmdqLog2Size() const {
    return cmdqLog2Size;
}

uint32_t SMMU::getEventqLog2Size() const {
    return eventqLog2Size;
}

} // namespace smmu