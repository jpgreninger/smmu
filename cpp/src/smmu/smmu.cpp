// ARM SMMU v3 Main Controller Implementation
// Copyright (c) 2024 John Greninger
// Enhanced with Task 5.2: Translation Engine

#include "smmu/smmu.h"
#include <chrono>
#include <algorithm>
#include <climits>

namespace smmu {

// GAP NEW-2: Thread-local storage for stage-2 fault context.
// Used to pass IPA and isStage2 flag from performBothStagesTranslation()
// to handleTranslationFailure() without changing intermediate function signatures.
// Each translate() call resets these at entry so stale state from a prior call
// on the same thread cannot leak into the next translation.
// Thread-local ensures concurrent translations on different threads are isolated.
namespace {
struct Stage2FaultContext {
    bool     isStage2 = false; ///< true when the fault occurred in stage-2
    uint64_t ipa      = 0;     ///< IPA (stage-1 output) at which stage-2 faulted
};
thread_local Stage2FaultContext tl_stage2FaultCtx;
} // anonymous namespace

// NEW-8 helper: convert S2PS 3-bit encoding to output PA bit-width.
// ARM IHI0070G.b §5.2: S2PS encoding 0=32b, 1=36b, 2=40b, 3=42b, 4=44b, 5=48b, 6=52b.
// Used by performBothStagesTranslation and performStage2OnlyTranslation to check
// that the stage-2 output PA lies within the allowed S2PS range (NEW-8 fix).
static uint8_t oasBitsFromS2PS(uint8_t s2ps) {
    static const uint8_t kS2PSToBits[] = {32u, 36u, 40u, 42u, 44u, 48u, 52u};
    uint8_t idx = (s2ps <= 6u) ? s2ps : 5u;
    return kS2PSToBits[idx];
}

// ARM §3.5.1: Circular queue index helpers (FINDING-M-01)

// Compute LOG2SIZE: smallest k such that 2^k >= capacity (minimum 0).
// BUG-6 fix: capacity=0 is not architecturally valid (ARM §3.5.1 specifies a
// minimum of 1 entry, i.e. LOG2SIZE=0 ⟺ 2^0=1 entry).  Clamp capacity=0 to
// 1 so that the caller receives LOG2SIZE=0 rather than treating zero-capacity
// as a special "no queue" sentinel.  Capacity=1 → return 0 (spec minimum).
static uint32_t computeLog2Size(size_t capacity) {
    if (capacity == 0) return 0; // clamp: 0 → 1-entry queue (spec minimum, LOG2SIZE=0)
    if (capacity == 1) return 0;
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
      cr0ack_(0),
      cr1_(0),
      gbpaConfig_(),
      strtabFmt_(static_cast<uint8_t>(StreamTableFormat::Linear)),
      strtabSplit_(6u),
      cmdqSyncMsiAttr_(0),
      cmdqSyncMsiAddr_(0),
      cmdqSyncMsiData_(0),
      cmdSyncLastSig_(static_cast<uint8_t>(CmdSyncSignalType::None)),
      // Default: 32 — matches max SIDSIZE per ARM §6.3.4 IDR1 (SIDSIZE bits [5:0], 0–32).
      // Per §6.3.25 the effective range is MIN(LOG2SIZE, SIDSIZE); values 0–32 are valid.
      strtabLog2Size_(32),
      // BUG-NEW3-05 fix: Start at 1; STAG=0 is reserved per ARM §3.12.2.
      stagCounter_(1),
      // GAP-NEW-E: STATUSR/IRQ_CTRL/CTRLACK registers initialize to 0.
      statusr_(0),
      irqCtrl_(0),
      irqCtrlAck_(0) {
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
      cr0ack_(0),
      cr1_(0),
      gbpaConfig_(),
      strtabFmt_(static_cast<uint8_t>(StreamTableFormat::Linear)),
      strtabSplit_(6u),
      cmdqSyncMsiAttr_(0),
      cmdqSyncMsiAddr_(0),
      cmdqSyncMsiData_(0),
      cmdSyncLastSig_(static_cast<uint8_t>(CmdSyncSignalType::None)),
      // Default: 32 — matches max SIDSIZE per ARM §6.3.4 IDR1 (SIDSIZE bits [5:0], 0–32).
      strtabLog2Size_(32),
      // BUG-NEW3-05 fix: Start at 1; STAG=0 is reserved per ARM §3.12.2.
      stagCounter_(1),
      // GAP-NEW-E: STATUSR/IRQ_CTRL/CTRLACK registers initialize to 0.
      statusr_(0),
      irqCtrl_(0),
      irqCtrlAck_(0) {
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
// NEW-12: extended with TransactionType parameter (defaulted to Ordinary for backward compat).
TranslationResult SMMU::translate(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType,
                                  SecurityState securityState, TransactionType transactionType) {
    // GAP NEW-2: Reset thread-local stage-2 fault context at the start of each translation.
    // This prevents stale context from a prior translation on the same thread from leaking.
    tl_stage2FaultCtx.isStage2 = false;
    tl_stage2FaultCtx.ipa      = 0;

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
        // NEW-13: ATS TR/TT get specific events even when SMMUEN=0 (§3.9.1.2/3.9.1.3).
        if (transactionType == TransactionType::AtsTranslationRequest) {
            // §7.3.6: F_BAD_ATS_TREQ for SMMUEN=0 requires CR2.REC_CFG_ATS=1.
            if ((cr2_.load(std::memory_order_acquire) & CR2_REC_CFG_ATS) != 0u) {
                generateEvent(EventType::F_BAD_ATS_TREQ, streamID, pasid, iova,
                              securityState, false, 0, accessType, false, 0);
            }
            return makeTranslationError(SMMUError::PageNotMapped);
        }
        if (transactionType == TransactionType::AtsTranslated) {
            // §7.3.8: F_TRANSL_FORBIDDEN for SMMUEN=0 is always permitted.
            generateEvent(EventType::F_TRANSL_FORBIDDEN, streamID, pasid, iova,
                          securityState, false, 0, accessType, false, 0);
            return makeTranslationError(SMMUError::PageNotMapped);
        }
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
        {
            TranslationData td(static_cast<PA>(iova), allPerms, securityState);
            // CONF-GAP-13: Apply GBPA output attributes to bypass result (§6.3.22).
            GbpaConfig gbpa;
            {
                std::lock_guard<std::recursive_mutex> glock(queueMutex);
                gbpa = gbpaConfig_;
            }
            if (gbpa.mtCfg) {
                td.memType = gbpa.memAttr;
            }
            td.shareability = gbpa.shCfg;
            td.allocHint    = gbpa.allocCfg;
            td.instCfg      = gbpa.instCfg;
            td.privCfg      = gbpa.privCfg;
            return TranslationResult(td);
        }
    }

    // CONF-GAP-3: 2-level stream table StreamID validation.
    // When format is TwoLevel, validate L1 index is in bounds.
    if (static_cast<StreamTableFormat>(strtabFmt_.load(std::memory_order_acquire)) == StreamTableFormat::TwoLevel) {
        if (!validateStreamID2Level(streamID)) {
            FaultRecord fault;
            fault.streamID = streamID;
            fault.pasid = pasid;
            fault.address = iova;
            fault.faultType = FaultType::BadStreamID;
            fault.accessType = accessType;
            fault.securityState = securityState;
            fault.timestamp = getCurrentTimestamp();
            recordFault(fault);
            {
                bool recordSid = (cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u;
                // NEW-15: ATS TR C_BAD_STREAMID also requires CR2.REC_CFG_ATS=1 (§3.9.1.2).
                if (recordSid && transactionType == TransactionType::AtsTranslationRequest) {
                    recordSid = (cr2_.load(std::memory_order_acquire) & CR2_REC_CFG_ATS) != 0u;
                }
                if (recordSid) {
                    generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
                }
            }
            return makeTranslationError(SMMUError::InvalidStreamID);
        }
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
                {
                    bool recordSid = (cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u;
                    // NEW-15: ATS TR C_BAD_STREAMID also requires CR2.REC_CFG_ATS=1 (§3.9.1.2).
                    if (recordSid && transactionType == TransactionType::AtsTranslationRequest) {
                        recordSid = (cr2_.load(std::memory_order_acquire) & CR2_REC_CFG_ATS) != 0u;
                    }
                    if (recordSid) {
                        generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
                    }
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
        {
            bool recordSid = (cr2_.load(std::memory_order_acquire) & CR2_RECINVSID) != 0u;
            // NEW-15: ATS TR C_BAD_STREAMID also requires CR2.REC_CFG_ATS=1 (§3.9.1.2).
            if (recordSid && transactionType == TransactionType::AtsTranslationRequest) {
                recordSid = (cr2_.load(std::memory_order_acquire) & CR2_REC_CFG_ATS) != 0u;
            }
            if (recordSid) {
                generateEvent(EventType::C_BAD_STREAMID, streamID, pasid, iova, securityState);
            }
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
                // Bug B fix: ReadExecute promotes to ReadExecutePrivileged (keeps read requirement).
                case AccessType::ReadExecute:  effectiveAccessType = AccessType::ReadExecutePrivileged; break;
                case AccessType::ReadPrivileged:
                case AccessType::WritePrivileged:
                case AccessType::ExecutePrivileged:
                case AccessType::ReadWritePrivileged:
                case AccessType::ReadExecutePrivileged:
                    break;
            }
        }
        // NEW-6 fix: Apply INSTCFG/PRIVCFG overrides after STRW promotion, mirroring
        // stream_context.cpp translateUnlocked() so TLB fast-path permission checks
        // use the same effective access type as the slow path.
        if (streamCfgForTlb.instCfg == 1u) {
            if (effectiveAccessType == AccessType::Read)
                effectiveAccessType = AccessType::Execute;
            else if (effectiveAccessType == AccessType::ReadPrivileged)
                effectiveAccessType = AccessType::ExecutePrivileged;
        } else if (streamCfgForTlb.instCfg == 2u) {
            if (effectiveAccessType == AccessType::Execute)
                effectiveAccessType = AccessType::Read;
            else if (effectiveAccessType == AccessType::ExecutePrivileged)
                effectiveAccessType = AccessType::ReadPrivileged;
            else if (effectiveAccessType == AccessType::ReadExecute)
                effectiveAccessType = AccessType::Read;
            else if (effectiveAccessType == AccessType::ReadExecutePrivileged)
                effectiveAccessType = AccessType::ReadPrivileged;
        }
        if (streamCfgForTlb.privCfg == 2u) {
            switch (effectiveAccessType) {
                case AccessType::ReadPrivileged:          effectiveAccessType = AccessType::Read;        break;
                case AccessType::WritePrivileged:         effectiveAccessType = AccessType::Write;       break;
                case AccessType::ExecutePrivileged:       effectiveAccessType = AccessType::Execute;     break;
                case AccessType::ReadWritePrivileged:     effectiveAccessType = AccessType::ReadWrite;   break;
                case AccessType::ReadExecutePrivileged:   effectiveAccessType = AccessType::ReadExecute; break;
                default: break;
            }
        } else if (streamCfgForTlb.privCfg == 3u) {
            switch (effectiveAccessType) {
                case AccessType::Read:        effectiveAccessType = AccessType::ReadPrivileged;          break;
                case AccessType::Write:       effectiveAccessType = AccessType::WritePrivileged;         break;
                case AccessType::Execute:     effectiveAccessType = AccessType::ExecutePrivileged;       break;
                case AccessType::ReadWrite:   effectiveAccessType = AccessType::ReadWritePrivileged;     break;
                case AccessType::ReadExecute: effectiveAccessType = AccessType::ReadExecutePrivileged;   break;
                default: break;
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
                    // BUG-8 fix: ARM §13.4.1 — for EL2 (non-VHE) and EL3 StreamWorld,
                    // AP[1] is ignored (treated as 1), so the output PRIV attribute must
                    // reflect the effective privilege level of the transaction, not the
                    // raw page-descriptor AP[1] bit stored in entry.permissions.
                    // Clear privilegedOnly so that the caller observes the correct output.
                    // EL2_E2H is explicitly excluded — VHE maintains EL1-like privilege
                    // checks and must NOT have privilegedOnly cleared.
                    if (!streamCfgForTlb.stage2Enabled &&
                        (streamCfgForTlb.strw == StreamWorld::EL2 ||
                         streamCfgForTlb.strw == StreamWorld::EL3)) {
                        data.permissions.privilegedOnly = false;
                    }
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

    // NEW-12: ARM §3.9 ATS Transaction Type enforcement.
    // These checks are performed after the stripe lock is released so that
    // generateEvent() can safely acquire queueMutex without creating a deadlock.

    // Check 1 — F_BAD_ATS_TREQ (§7.3.6, event 0x05): ATS Translation Request on a
    // stream that does not support ATS.
    if (transactionType == TransactionType::AtsTranslationRequest) {
        // NEW-19: STE.Config=0b000 (no translation, no bypass) → silent UR, no event (§3.9.1.2).
        if (!streamCfgSnapshot.translationEnabled && !streamCfgSnapshot.bypassEnabled) {
            return makeTranslationError(SMMUError::PageNotMapped);
        }
        // Other unsupported cases (EATS=0, bypass stream) → F_BAD_ATS_TREQ.
        bool atsSupported = (streamCfgSnapshot.eats != 0)
                            && streamCfgSnapshot.translationEnabled
                            && !streamCfgSnapshot.bypassEnabled;
        if (!atsSupported) {
            generateEvent(EventType::F_BAD_ATS_TREQ, streamID, pasid, iova,
                          securityState, false, 0, accessType, false, 0);
            return makeTranslationError(SMMUError::PageNotMapped);
        }
    }

    // Check 2 — F_TRANSL_FORBIDDEN (§7.3.8, event 0x07): ATS Translated transaction
    // with ATSCHK=1. The SMMU re-translates the address as Ordinary to verify the
    // presented translation is correct.  If re-translation fails, F_TRANSL_FORBIDDEN
    // is emitted.
    if (transactionType == TransactionType::AtsTranslated
        && (cr0_.load(std::memory_order_acquire) & CR0_ATSCHK) != 0u) {
        // Re-translate as Ordinary to validate the pre-translated address.
        TranslationResult recheck = performTwoStageTranslation(
            streamID, pasid, iova, accessType, securityState,
            streamContext, currentTime);
        if (recheck.isError()) {
            generateEvent(EventType::F_TRANSL_FORBIDDEN, streamID, pasid, iova,
                          securityState, false, 0, accessType, false, 0);
            return makeTranslationError(SMMUError::PageNotMapped);
        }
        // Re-translation succeeded — return the re-translated result as the
        // canonical answer (the re-translation IS the verification result).
        return recheck;
    }

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
            // CONF-GAP-25: ARM §3.17 — TLB entries must be tagged with the correct
            // ASID and VMID depending on the active translation stage(s).
            //   Stage-1 only:   asid = CD.ASID,  vmid = 0    (no stage-2 VMID)
            //   Stage-2 only:   asid = 0,         vmid = S2VMID
            //   Both stages:    asid = CD.ASID,  vmid = S2VMID
            uint16_t entryAsid = streamCfg.asid;
            uint16_t entryVmid = streamCfg.vmid;
            if (!streamCfg.stage2Enabled) {
                // Stage-1 only: no VMID tagging
                entryVmid = 0;
            }
            if (!streamCfg.stage1Enabled) {
                // Stage-2 only: no ASID tagging
                entryAsid = 0;
            }
            // GAP-NEW-S3: ARM IHI0070G.b §6.3.12 / §3.17.5 — STRW=EL2_E2H (VHE) is only
            // valid when CR2.E2H=1.  When CR2.E2H=0, STRW=EL2_E2H must be treated as
            // NS-EL2 (STRW=EL2): no ASID tagging on TLB entries.  ASID-based TLBI
            // commands (e.g. CMD_TLBI_NH_ASID) must therefore NOT match these entries,
            // which is achieved by tagging them with ASID=0 (same as plain NS-EL2).
            if (streamCfg.strw == StreamWorld::EL2_E2H &&
                (cr2_.load(std::memory_order_acquire) & CR2_E2H) == 0u) {
                entryAsid = 0;
            }
            cacheTranslationResult(streamID, pasid, iova, result, currentTime, entryAsid, entryVmid);
        }
    } else if (result.isError()) {
        // ARM §3.12.2: Check per-stream stall mode before standard fault handling.
        // If the stream is configured for stall, enqueue a StallRecord and return
        // Stalled instead of the original error — software must issue CMD_RESUME.
        // GAP-NEW-G: STE.S1STALLD=1 forces abort semantics — override stall mode.
        bool inStallMode = (streamContext->getFaultMode() == FaultMode::Stall &&
                            !streamContext->isS1StallDisabled());
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
                // BUG-5 fix: ARM §3.12.2 — the SMMU must not terminate a stalled
                // transaction while free STAG slots remain.  The previous code
                // checked only ONE candidate slot and immediately fell back to
                // terminate mode on any collision.
                //
                // Corrected algorithm:
                //   1. If ALL 65534 usable slots are occupied, fall back to
                //      terminate immediately (no free slot exists).
                //   2. Otherwise scan up to 65535 candidates: increment the
                //      monotonic counter, skip STAG=0 (reserved per §3.12.2),
                //      and return the first slot not present in stallQueue_.
                //
                // The entire scan runs under stallQueueMutex_ so that no
                // concurrent thread can steal a slot between the "free?" check
                // and the insert.
                if (stallQueue_.size() >= 65534u) {
                    // All usable slots occupied — terminate mode is the only option.
                    stagValid = false;
                }
                else {
                    // At least one free slot exists; scan for it.
                    for (uint32_t attempt = 0; attempt < 65535u; ++attempt) {
                        uint16_t candidate = stagCounter_.fetch_add(1, std::memory_order_acq_rel);
                        if (candidate == 0u) {
                            // Skip the reserved STAG=0 value.
                            candidate = stagCounter_.fetch_add(1, std::memory_order_acq_rel);
                        }
                        if (stallQueue_.count(candidate) == 0) {
                            stag = candidate;
                            stagValid = true;
                            record = StallRecord(stag, streamID, pasid, iova, accessType, securityState, currentTime);
                            stallQueue_[stag] = record;
                            break;
                        }
                    }
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
                case SMMUError::AccessFlagFaultError:
                    // §3.13.2 NEW-GAP-J: Access flag fault stall event → F_ACCESS.
                    stallEventType = EventType::F_ACCESS;
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
            // NEW-A fix: §7.3 — stall event InD/PnU must reflect post-STE-override access type.
            // Apply STRW→INSTCFG→PRIVCFG overrides to accessType (same order as stream_context.cpp
            // translateUnlocked() lines 1087-1157) using the streamCfgSnapshot captured at line 413.
            AccessType stallEventAccessType = accessType;
            // STRW promotion: only when stage-2 is disabled (§5.2 says STRW is ignored for two-stage).
            if (!streamCfgSnapshot.stage2Enabled &&
                (streamCfgSnapshot.strw == StreamWorld::EL2 || streamCfgSnapshot.strw == StreamWorld::EL3)) {
                switch (stallEventAccessType) {
                    case AccessType::Read:        stallEventAccessType = AccessType::ReadPrivileged;          break;
                    case AccessType::Write:       stallEventAccessType = AccessType::WritePrivileged;         break;
                    case AccessType::Execute:     stallEventAccessType = AccessType::ExecutePrivileged;       break;
                    case AccessType::ReadWrite:   stallEventAccessType = AccessType::ReadWritePrivileged;     break;
                    case AccessType::ReadExecute: stallEventAccessType = AccessType::ReadExecutePrivileged;   break;
                    default: break;
                }
            }
            // INSTCFG override.
            if (streamCfgSnapshot.instCfg == 1u) {
                if (stallEventAccessType == AccessType::Read)
                    stallEventAccessType = AccessType::Execute;
                else if (stallEventAccessType == AccessType::ReadPrivileged)
                    stallEventAccessType = AccessType::ExecutePrivileged;
            } else if (streamCfgSnapshot.instCfg == 2u) {
                if (stallEventAccessType == AccessType::Execute)
                    stallEventAccessType = AccessType::Read;
                else if (stallEventAccessType == AccessType::ExecutePrivileged)
                    stallEventAccessType = AccessType::ReadPrivileged;
                else if (stallEventAccessType == AccessType::ReadExecute)
                    stallEventAccessType = AccessType::Read;
                else if (stallEventAccessType == AccessType::ReadExecutePrivileged)
                    stallEventAccessType = AccessType::ReadPrivileged;
            }
            // PRIVCFG override.
            if (streamCfgSnapshot.privCfg == 2u) {
                switch (stallEventAccessType) {
                    case AccessType::ReadPrivileged:        stallEventAccessType = AccessType::Read;        break;
                    case AccessType::WritePrivileged:       stallEventAccessType = AccessType::Write;       break;
                    case AccessType::ExecutePrivileged:     stallEventAccessType = AccessType::Execute;     break;
                    case AccessType::ReadWritePrivileged:   stallEventAccessType = AccessType::ReadWrite;   break;
                    case AccessType::ReadExecutePrivileged: stallEventAccessType = AccessType::ReadExecute; break;
                    default: break;
                }
            } else if (streamCfgSnapshot.privCfg == 3u) {
                switch (stallEventAccessType) {
                    case AccessType::Read:        stallEventAccessType = AccessType::ReadPrivileged;          break;
                    case AccessType::Write:       stallEventAccessType = AccessType::WritePrivileged;         break;
                    case AccessType::Execute:     stallEventAccessType = AccessType::ExecutePrivileged;       break;
                    case AccessType::ReadWrite:   stallEventAccessType = AccessType::ReadWritePrivileged;     break;
                    case AccessType::ReadExecute: stallEventAccessType = AccessType::ReadExecutePrivileged;   break;
                    default: break;
                }
            }
            // CONF-GAP-20: pass stallEventAccessType so rnw/ind/pnu wire-format fields are populated.
            // GAP NEW-2: propagate stage-2 context (S2 flag, IPA) for stall events that
            // originated in stage-2.  tl_stage2FaultCtx was set by performBothStagesTranslation.
            generateEvent(stallEventType, streamID, pasid, iova, securityState, /*isStall=*/true, stag, stallEventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
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
    // CONF-GAP-16: ARM §5.2.2 STE validation — check for illegal combinations
    // BEFORE acquiring the stripe lock.  These checks examine only the config
    // parameter (no shared state), so they are safe to run unlocked.
    // generateEvent() acquires queueMutex (and then the stripe lock for MEV check)
    // which would deadlock if called while holding the stripe lock.

    // Validation 1: STRW=EL3 is forbidden for Non-secure streams (§5.2 Table 3-2).
    if (config.securityState == SecurityState::NonSecure &&
        config.strw == StreamWorld::EL3) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // Validation 2: Reserved STE.Config combinations (ARM §5.2 Table STE.Config).
    // Valid encodings: 0b000 (disabled), 0b100 (bypass), 0b101 (S1-only),
    //                  0b110 (S2-only), 0b111 (S1+S2).
    // 0b001/0b010/0b011 are reserved and "behave as 0b000" per spec.
    // The reserved encodings correspond to translationEnabled=true with no stage
    // selected — generate C_BAD_STE so callers detect the misconfigured STE.
    if (config.translationEnabled && !config.stage1Enabled && !config.stage2Enabled) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // Validation 3: Stage-2 S2TTB must lie within the OAS (§5.2, §3.4.3).
    // s2ps encoding: 0=32b, 1=36b, 2=40b, 3=42b, 4=44b, 5=48b, 6=52b.
    if (config.stage2Enabled && config.s2ttb != 0u) {
        static const uint8_t s2psToOasBits[] = {32, 36, 40, 42, 44, 48, 52};
        uint8_t s2psIdx = (config.s2ps <= 6u) ? config.s2ps : 5u;
        uint8_t oasBits = s2psToOasBits[s2psIdx];
        uint64_t oasLimit = (oasBits >= 64u) ? UINT64_MAX : (static_cast<uint64_t>(1u) << oasBits);
        if (config.s2ttb >= oasLimit) {
            generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
            return makeVoidError(SMMUError::InvalidConfiguration);
        }
    }

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
VoidResult SMMU::mapPage(StreamID streamID, PASID pasid, IOVA iova, PA pa,
                          const PagePermissions& permissions, SecurityState securityState,
                          bool accessFlag) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }

    return streamIt->second->mapPage(pasid, iova, pa, permissions, securityState, accessFlag);
}

// Map a stage-2 page as Device memory type (for S2PTW testing).
VoidResult SMMU::mapStage2DevicePage(StreamID streamID, IOVA ipa, PA pa,
                                      const PagePermissions& permissions,
                                      SecurityState securityState) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }

    return streamIt->second->mapStage2DevicePage(ipa, pa, permissions, securityState);
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
    // BUG-8 fix: use explicit relaxed store, not the implicit seq_cst assignment
    // operator.  cacheHits and cacheMisses already use memory_order_relaxed; the
    // inconsistency (seq_cst for translationCount, relaxed for the others) is a
    // code-quality defect — there is no ordering requirement between a statistics
    // reset and any other operation, so relaxed is correct and consistent here.
    translationCount.store(0, std::memory_order_relaxed);
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
    // BUG-5 fix: hold queueMutex when zeroing gerrorStatus and gerrorNStatus.
    // signalGerror() and clearGerror() both hold queueMutex when reading/writing
    // these atomics.  A concurrent clearGerror() computes
    //   activeBits = gerrorStatus XOR gerrorNStatus
    // as two separate reads; if reset() zeroes one between those two reads, the
    // XOR is computed against a stale value and clearGerror() toggles the wrong
    // bits.  Holding queueMutex here serialises reset() against those callers.
    {
        std::lock_guard<std::recursive_mutex> glock(queueMutex);
        gerrorStatus.store(0, std::memory_order_release);
        gerrorNStatus.store(0, std::memory_order_release);
    }

    // ARM §6.3.9: Reset returns SMMU to disabled state (SMMUEN=0, GBPA.ABORT=0).
    // BUG-NEW3-04 fix: ARM §6.3.9 — reset returns SMMU to disabled state (all CR0 bits clear).
    // Resetting only smmuen_ is insufficient; queue-enable bits must also be cleared.
    // BUG-CPP-NEW-1 fix: use release stores for these atomic members.
    smmuen_.store(false, std::memory_order_release);
    gbpaAbort_.store(false, std::memory_order_release);
    cr0_.store(0, std::memory_order_release);
    // CONF-GAP-9: Reset CR0ACK to 0 (mirrors CR0 reset value)
    cr0ack_.store(0, std::memory_order_release);
    // CONF-GAP-10: Reset CR1 to 0
    cr1_.store(0, std::memory_order_release);
    // CONF-GAP-13: Reset GBPA config to default (all zeros, no abort)
    {
        std::lock_guard<std::recursive_mutex> glock(queueMutex);
        gbpaConfig_ = GbpaConfig();
        priAutoFailures_.clear();
    }
    // CONF-GAP-3: Reset stream table format to linear with default split
    strtabFmt_.store(static_cast<uint8_t>(StreamTableFormat::Linear), std::memory_order_release);
    strtabSplit_.store(6u, std::memory_order_release);
    // CONF-GAP-18: Reset CMD_SYNC MSI registers to 0
    cmdqSyncMsiAttr_.store(0, std::memory_order_release);
    cmdqSyncMsiAddr_.store(0, std::memory_order_release);
    cmdqSyncMsiData_.store(0, std::memory_order_release);
    cmdSyncLastSig_.store(static_cast<uint8_t>(CmdSyncSignalType::None), std::memory_order_release);

    // BUG-R2-CPP-1 fix: restore strtabLog2Size_ to 32 and cr2_ to 0 on reset.
    // 32 is the maximum SIDSIZE per ARM §6.3.4 IDR1; LOG2SIZE is clamped at 32
    // to match the max SIDSIZE the model can advertise (per §6.3.25 effective
    // range = MIN(LOG2SIZE, SIDSIZE)).
    strtabLog2Size_.store(32u, std::memory_order_release);
    cr2_.store(0u, std::memory_order_release);

    // ARM §3.12.2: Clear stall queue and resume outcomes on reset.
    {
        std::lock_guard<std::mutex> slock(stallQueueMutex_);
        stallQueue_.clear();
        resumeOutcomes_.clear();
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
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, accessType);
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
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, accessType);
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

    // Gap C fix: ARM IHI0070G.b §3.4.1 / §5.4 — CD.T0SZ VA range enforcement.
    // T0SZ defines the TTBR0 input address range: valid IOVAs are [0, 2^(64-T0SZ)).
    // An IOVA at or above the range limit is a translation fault (F_TRANSLATION),
    // NOT an address-size fault (F_ADDR_SIZE, which is for OAS violations).
    // When t0sz==0 the effective VA space is the full 64-bit range — no check needed.
    //
    // GAP-E fix: ARM IHI0070G.b §3.4.1 — CD.TBI: when tbi==true, VA bits[63:56] are
    // masked (treated as a tag) before the T0SZ range check.  iova is preserved for
    // fault reporting; only effectiveIova is used for the range comparison.
    if (config.stage1Enabled && config.t0sz > 0u) {
        uint64_t effectiveIova = iova;
        if (config.tbi) {
            effectiveIova &= UINT64_C(0x00FFFFFFFFFFFFFF);
        }
        uint64_t vaLimit = UINT64_C(1) << (64u - static_cast<unsigned>(config.t0sz));
        if (effectiveIova >= vaLimit) {
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, accessType);
            // Return InvalidConfiguration so the outer handleTranslationFailure() switch
            // maps this to FaultType::StreamDisabled (a no-op) and does not re-emit
            // a second F_TRANSLATION event.  The event was already queued above.
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }
    }

    // NEW-7 fix: §5.4 — CD.EPD0=1: TTBR0 translation walk disabled → F_TRANSLATION.
    // SW model uses a single address space per PASID (no TTBR1), so EPD0 applies
    // to all stage-1 translations; EPD1 is architecturally for the upper VA half
    // (TTBR1) which this model does not implement separately.
    if (config.stage1Enabled && config.epd0) {
        generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                      false, 0, accessType);
        // Return InvalidConfiguration so handleTranslationFailure() treats this as
        // a no-op (StreamDisabled path) and does not re-emit a duplicate event.
        return makeTranslationError(SMMUError::InvalidConfiguration);
    }

    // GAP-E: Compute the effective IOVA for the page-table walk.
    // When TBI=1 the top byte is treated as a tag and is not part of the VA
    // used in the page-table walk (only the range check above used effectiveIova).
    // Use lookupIova for all address-space lookups; keep iova for fault reporting.
    uint64_t lookupIova = iova;
    if (config.stage1Enabled && config.tbi) {
        lookupIova &= UINT64_C(0x00FFFFFFFFFFFFFF);
    }

    if (config.stage1Enabled && config.stage2Enabled) {
        // Two-stage translation: IOVA -> IPA -> PA
        result = performBothStagesTranslation(streamID, pasid, lookupIova, accessType, securityState, streamContext, config, currentTime);
    } else if (config.stage1Enabled && !config.stage2Enabled) {
        // Stage-1 only: IOVA -> PA directly
        result = performStage1OnlyTranslation(streamID, pasid, lookupIova, accessType, securityState, streamContext, currentTime);
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
        // NEW-3 fix: §3.4 — S2T0SZ IPA input range check for stage-2-only streams.
        // In stage-2-only mode the IOVA is treated directly as the IPA.  An IPA at or
        // above 2^(64-S2T0SZ) exceeds the stage-2 input range → F_TRANSLATION.
        if (config.s2t0sz > 0u) {
            uint64_t ipaLimit = UINT64_C(1) << (64u - static_cast<unsigned>(config.s2t0sz));
            if (iova >= ipaLimit) {
                generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                              false, 0, accessType);
                // Return InvalidConfiguration so handleTranslationFailure() suppresses
                // duplicate event emission (StreamDisabled no-op path).
                return makeTranslationError(SMMUError::InvalidConfiguration);
            }
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
    // CONF-GAP-7: propagate IPA from two-stage translation result so that
    // TLBI_S2_IPA can perform selective IPA-based invalidation (§4.4).
    // For single-stage results data.ipa==0 (default), correctly marking the
    // entry as non-IPA-addressable.
    entry.ipa = data.ipa & ~static_cast<uint64_t>(PAGE_SIZE - 1u); // page-align

    // ARM SMMU v3 spec: Insert into TLB with LRU eviction if needed
    tlbCache->insert(entry);
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
    
    // NEW-2 fix: §13.4/§13 — STE overrides (INSTCFG, PRIVCFG) must be applied
    // before any permission check, including the stage-1 translatePage call.
    // Mirror the same override logic used by StreamContext::translateUnlocked()
    // (stream_context.cpp ~1100-1157).  Note: STRW promotion is NOT applied here
    // because performBothStagesTranslation is only called for two-stage streams
    // (stage2Enabled=true), and STRW is defined to be ignored when stage-2 is active.
    AccessType effectiveAccessType = accessType;
    // INSTCFG override (matches stream_context.cpp logic).
    if (config.instCfg == 1u) {
        if (effectiveAccessType == AccessType::Read)
            effectiveAccessType = AccessType::Execute;
        else if (effectiveAccessType == AccessType::ReadPrivileged)
            effectiveAccessType = AccessType::ExecutePrivileged;
    } else if (config.instCfg == 2u) {
        if (effectiveAccessType == AccessType::Execute)
            effectiveAccessType = AccessType::Read;
        else if (effectiveAccessType == AccessType::ExecutePrivileged)
            effectiveAccessType = AccessType::ReadPrivileged;
        else if (effectiveAccessType == AccessType::ReadExecute)
            effectiveAccessType = AccessType::Read;
        else if (effectiveAccessType == AccessType::ReadExecutePrivileged)
            effectiveAccessType = AccessType::ReadPrivileged;
    }
    // PRIVCFG override (matches stream_context.cpp logic).
    if (config.privCfg == 2u) {
        switch (effectiveAccessType) {
            case AccessType::ReadPrivileged:          effectiveAccessType = AccessType::Read;        break;
            case AccessType::WritePrivileged:         effectiveAccessType = AccessType::Write;       break;
            case AccessType::ExecutePrivileged:       effectiveAccessType = AccessType::Execute;     break;
            case AccessType::ReadWritePrivileged:     effectiveAccessType = AccessType::ReadWrite;   break;
            case AccessType::ReadExecutePrivileged:   effectiveAccessType = AccessType::ReadExecute; break;
            default: break;
        }
    } else if (config.privCfg == 3u) {
        switch (effectiveAccessType) {
            case AccessType::Read:        effectiveAccessType = AccessType::ReadPrivileged;          break;
            case AccessType::Write:       effectiveAccessType = AccessType::WritePrivileged;         break;
            case AccessType::Execute:     effectiveAccessType = AccessType::ExecutePrivileged;       break;
            case AccessType::ReadWrite:   effectiveAccessType = AccessType::ReadWritePrivileged;     break;
            case AccessType::ReadExecute: effectiveAccessType = AccessType::ReadExecutePrivileged;   break;
            default: break;
        }
    }

    // Perform Stage-1 translation: IOVA -> IPA
    // NEW-GAP-J: pass ha and affd so translatePage can enforce the AF fault rule.
    // NEW-2 fix: use effectiveAccessType (post INSTCFG/PRIVCFG) for the permission check.
    TranslationResult stage1Result = stage1AddressSpace->translatePage(iova, effectiveAccessType, securityState,
                                                                        config.ha, config.affd);
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
                // ARM IHI0070G.b §7.3.14 + §7.3 fault ordering step 3c:
                // F_ADDR_SIZE must be emitted on address-size faults in any
                // stage of translation, including two-stage.
                // BUG-CPP-TWOSTAGE-1 fix: emit the event here, matching the
                // pattern used in performStage1OnlyTranslation (line 1526) and
                // performStage2OnlyTranslation (line 1570).
                faultType = FaultType::AddressSizeFault;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType);
                break;
            case SMMUError::InvalidSecurityState:
                faultType = FaultType::SecurityFault;
                break;
            case SMMUError::AccessFlagFaultError:
                // §3.13.2 NEW-GAP-J: Access flag fault → F_ACCESS (0x12).
                faultType = FaultType::AccessFlagFault;
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

    // §5.4 NEW-GAP-K: WXN/UWXN write-execute-never permission override.
    // Applied after stage-1 succeeds, before stage-2 translation.
    // NEW-2 fix: use the effectiveAccessType computed above (INSTCFG/PRIVCFG already
    // applied), eliminating the duplicate wxnEffective computation that previously
    // re-derived the overrides from the raw accessType.
    {
        // Bug A fix: isExec now includes ReadExecute and ReadExecutePrivileged.
        bool isExec = (effectiveAccessType == AccessType::Execute ||
                       effectiveAccessType == AccessType::ExecutePrivileged ||
                       effectiveAccessType == AccessType::ReadExecute ||
                       effectiveAccessType == AccessType::ReadExecutePrivileged);
        if (isExec) {
            const PagePermissions& s1p = stage1Result.getValue().permissions;
            // WXN: writable page is execute-never for all execute accesses.
            if (config.wxn && s1p.write) {
                recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                                        accessType, securityState, FaultStage::Stage1Only, currentTime, 0, 0);
                return makeTranslationError(SMMUError::PagePermissionViolation);
            }
            // UWXN: privileged execute on an unprivileged-writable page is forbidden.
            if (config.uwxn &&
                (effectiveAccessType == AccessType::ExecutePrivileged ||
                 effectiveAccessType == AccessType::ReadExecutePrivileged) &&
                s1p.write && !s1p.privilegedOnly) {
                recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                                        accessType, securityState, FaultStage::Stage1Only, currentTime, 0, 0);
                return makeTranslationError(SMMUError::PagePermissionViolation);
            }
        }
    }

    // GAP-F fix: ARM IHI0070G.b §5.4/§3.4 — CD.IPS: stage-1 output IPA must lie
    // within the IPS output address range.  Violation → F_ADDR_SIZE (§7.3.14).
    // IPS uses the same 3-bit encoding as S2PS; reuse the oasBitsFromS2PS() helper.
    // When ips==6 (52-bit, the default) the check is skipped (no restriction).
    {
        uint8_t ipsBits = oasBitsFromS2PS(config.ips);
        if (ipsBits < 52u) {
            uint64_t ipaLimit = UINT64_C(1) << static_cast<unsigned>(ipsBits);
            if (intermediatePA >= ipaLimit) {
                recordComprehensiveFault(streamID, pasid, iova, FaultType::AddressSizeFault,
                                       accessType, securityState, FaultStage::Stage1Only, currentTime, 0, 0);
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType);
                // Return InvalidConfiguration to suppress a duplicate event in
                // handleTranslationFailure() (same pattern as S2PS OAS check).
                return makeTranslationError(SMMUError::InvalidConfiguration);
            }
        }
    }

    // NEW-3 fix: §3.4 — S2T0SZ IPA input range check for two-stage streams.
    // The IPA produced by stage-1 must lie within [0, 2^(64-S2T0SZ)).
    // An IPA at or above the limit is a translation fault (F_TRANSLATION).
    if (config.s2t0sz > 0u) {
        uint64_t ipaLimit = UINT64_C(1) << (64u - static_cast<unsigned>(config.s2t0sz));
        if (intermediatePA >= ipaLimit) {
            recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                                   accessType, securityState, FaultStage::Stage2Only, currentTime, 0, 0);
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, effectiveAccessType);
            // Return InvalidConfiguration to suppress a second event in handleTranslationFailure().
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }
    }

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
        // BUG-CPP-S1 fix: do NOT emit F_TRANSLATION here; handleTranslationFailure() will
        // emit it exactly once, per §3.12.2 (one fault per transaction).
        recordComprehensiveFault(streamID, pasid, iova, FaultType::TranslationFault,
                               accessType, securityState, FaultStage::Stage2Only, currentTime, 0, 0);
        // NEW-E fix: §7.3.13 — null stage-2 AS is a stage-2 translation fault; set
        // tl_stage2FaultCtx so handleTranslationFailure() emits S2=1 and IPA in the event.
        // intermediatePA is the IPA produced by stage-1 (assigned at line 1852).
        tl_stage2FaultCtx.isStage2 = true;
        tl_stage2FaultCtx.ipa      = intermediatePA;
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
    // NEW-GAP-J: pass s2ha and s2affd for stage-2 AF fault enforcement.
    TranslationResult stage2Result = stage2AddressSpace->translatePage(intermediatePA, AccessType::Read,
                                                                        stage1DataRef.securityState,
                                                                        config.s2ha, config.s2affd);
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
            case SMMUError::AccessFlagFaultError:
                // §7.3.15 / NEW-B fix: Stage-2 AF fault must produce AccessFlagFault
                // (→ F_ACCESS with s2=true), not Stage2PermissionFault.
                // Previously fell to default → Stage2PermissionFault → no F_ACCESS emitted.
                stage2FaultType = FaultType::AccessFlagFault;
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

        // GAP NEW-2: Record stage-2 fault context so handleTranslationFailure() and the
        // stall path can set S2=1 and IPA in the generated event (§7.3.13).
        tl_stage2FaultCtx.isStage2 = true;
        tl_stage2FaultCtx.ipa      = intermediatePA;

        return stage2Result;
    }

    // §5.2 NEW-GAP-L: S2PTW — translation through a Device-type stage-2 page is
    // a Permission fault. Prevents table walk from landing in Device MMIO regions.
    if (config.s2ptw && stage2AddressSpace->getPageDeviceMemory(intermediatePA)) {
        recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                                accessType, securityState, FaultStage::Stage2Only, currentTime, 0, 0);
        // NEW-D fix: §7.3.16 — S2PTW device fault is a stage-2 fault; set tl_stage2FaultCtx
        // so handleTranslationFailure() and the stall path emit S2=1 and the IPA in the event.
        tl_stage2FaultCtx.isStage2 = true;
        tl_stage2FaultCtx.ipa      = intermediatePA;
        return makeTranslationError(SMMUError::PagePermissionViolation);
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

    // ARM IHI0070G.b §3.10/§3.10.2: Stage-2 alone determines the final PA security
    // state. The translatePage() call at stage-2 (above) already enforces the
    // correct security state match — it rejects a lookup if the requested security
    // state does not match the mapped entry.  Adding a second validateSecurityState()
    // call here comparing the incoming transaction security state against the stage-2
    // output security state is architecturally incorrect: the stage-2 output is the
    // authoritative PA security state and is not required to equal the incoming NS
    // bit by §3.10.2.  The check has been removed.

    // NEW-8 / BUG-10 fix: §3.4/§7.3.14 — Stage-2 output PA must lie within the
    // S2PS output range.  This check MUST run before the permission intersection
    // check below (ARM §7.3 fault priority: F_ADDR_SIZE §7.3.14 > F_PERMISSION
    // §7.3.16).  When both conditions hold simultaneously the address-size fault
    // takes precedence.
    {
        uint8_t s2psBits = oasBitsFromS2PS(config.s2ps);
        if (s2psBits < 52u) {
            uint64_t paLimit = UINT64_C(1) << static_cast<unsigned>(s2psBits);
            PA outputPA = stage2Data.physicalAddress;
            if (outputPA >= paLimit) {
                recordComprehensiveFault(streamID, pasid, iova, FaultType::AddressSizeFault,
                                       accessType, securityState, FaultStage::Stage2Only, currentTime, 0, 0);
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType);
                return makeTranslationError(SMMUError::InvalidConfiguration);
            }
        }
    }

    // ARM SMMU v3 spec: Validate final permissions against requested access.
    // Runs AFTER the S2PS OAS check per §7.3 fault priority ordering.
    // NEW-3 fix: use effectiveAccessType (post INSTCFG/PRIVCFG) computed above,
    // so the same overrides applied to stage-1 are reflected here.
    if (!validateAccessPermissions(finalPermissions, effectiveAccessType)) {
        // Permission fault after two-stage translation - final permission check failed
        recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                               accessType, securityState, FaultStage::BothStages, currentTime, 2, 0);
        // NEW-F fix: §7.3.16 — S2=1 when stage-2 is the binding constraint.
        // Stage-2 is the binding constraint when stage-1 alone would have permitted the access.
        if (validateAccessPermissions(stage1Data.permissions, effectiveAccessType)) {
            tl_stage2FaultCtx.isStage2 = true;
            tl_stage2FaultCtx.ipa      = intermediatePA;
        }
        return makeTranslationError(SMMUError::PagePermissionViolation);
    }

    // Create successful final translation result.
    // CONF-GAP-7: tag the result with the IPA (stage-1 output) so it can be
    // stored in the TLBEntry.ipa field for selective TLBI_S2_IPA invalidation.
    TranslationData twoStageResult(stage2Data.physicalAddress, finalPermissions, stage2Data.securityState);
    twoStageResult.ipa = intermediatePA;
    return makeSuccess<TranslationData>(twoStageResult);
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
                // CONF-GAP-20: pass accessType so rnw/ind/pnu wire-format fields are set.
                fault.faultType = FaultType::AddressSizeFault;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, accessType);
                break;
            case SMMUError::AccessFlagFaultError:
                // §3.13.2 NEW-GAP-J: Access flag fault → F_ACCESS (0x12).
                fault.faultType = FaultType::AccessFlagFault;
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

    // GAP NEW-5 fix: ARM IHI0070G.b §3.4 — when stage-2 is bypassed (STE.Config=0b10x,
    // i.e., stage-1 active + stage-2 disabled), a PA output that exceeds OAS must be
    // SILENTLY TRUNCATED to OAS width.  No F_ADDR_SIZE event is generated (contrast with
    // STE.Config=0b100 full-bypass, which does generate F_ADDR_SIZE per §7.3.14).
    {
        uint64_t oasBits = configuration.getAddressConfiguration().maxPASize;
        if (oasBits < 64u && result.isOk()) {
            PA outputPA = result.getValue().physicalAddress;
            PA oasLimit = static_cast<PA>(1) << oasBits;
            if (outputPA >= oasLimit) {
                // Silently truncate to OAS width — ARM §3.4 stage-2-bypass semantics.
                TranslationData truncated = result.getValue();
                truncated.physicalAddress = outputPA & (oasLimit - 1u);
                return makeSuccess<TranslationData>(truncated);
            }
        }
    }

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
                // CONF-GAP-20: pass accessType so rnw/ind/pnu wire-format fields are set.
                fault.faultType = FaultType::AddressSizeFault;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, accessType);
                break;
            case SMMUError::AccessFlagFaultError:
                // §7.3.15 / NEW-C fix: set fault type but do NOT call generateEvent() here.
                // handleTranslationFailure() (fixed by NEW-A) owns the single event emission
                // per §7.2 / §3.12.2 — one fault event per transaction.
                // Previously this path called generateEvent(F_ACCESS) inline AND returned the
                // error, causing handleTranslationFailure to emit a second F_ACCESS.
                fault.faultType = FaultType::AccessFlagFault;
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

    // NEW-8 fix: §3.4/§7.3.14 — Stage-2 output PA must lie within the S2PS output range.
    // Applies to stage-2-only streams the same way as two-stage streams.
    {
        StreamConfig s2onlyCfg = streamContext->getStreamConfiguration();
        uint8_t s2psBits = oasBitsFromS2PS(s2onlyCfg.s2ps);
        if (s2psBits < 52u && result.isOk()) {
            uint64_t paLimit = UINT64_C(1) << static_cast<unsigned>(s2psBits);
            PA outputPA = result.getValue().physicalAddress;
            if (outputPA >= paLimit) {
                FaultRecord s2PaFault;
                s2PaFault.streamID = streamID;
                s2PaFault.pasid = pasid;
                s2PaFault.address = iova;
                s2PaFault.faultType = FaultType::AddressSizeFault;
                s2PaFault.accessType = accessType;
                s2PaFault.securityState = securityState;
                s2PaFault.timestamp = currentTime;
                recordFault(s2PaFault);
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, accessType);
                return makeTranslationError(SMMUError::InvalidConfiguration);
            }
        }
    }

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
            case SMMUError::AccessFlagFaultError:
                // §7.3.15 / NEW-A fix: AccessFlagFaultError must map to AccessFlagFault
                // so the recovery switch emits F_ACCESS (0x12), not F_TRANSLATION (0x10).
                // Previously fell to default → classifyTranslationFault() → TranslationFault.
                faultType = FaultType::AccessFlagFault;
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
            case SMMUError::InvalidPASID:
            case SMMUError::PASIDNotFound:
                // §7.3.9: C_BAD_SUBSTREAMID was already emitted in performTwoStageTranslation()
                // before returning this error.  Map to BadSubstreamId (no-op in recovery switch)
                // so no secondary F_TRANSLATION event is generated.
                faultType = FaultType::BadSubstreamId;
                break;
            case SMMUError::InvalidConfiguration:
                // BUG-NEW-C fix: §3.12.2 / §7.2 — "The SMMU does not record more than one
                // fault for each incoming transaction."  C_BAD_CD was already emitted by
                // translateUnlocked() for AA64=0 / T0SZ/T1SZ violations.  Map to
                // StreamDisabled (a no-op in the recovery switch below) so no secondary
                // F_TRANSLATION event is generated.
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

    // NEW-4 fix: §7.3 — EventEntry InD/PnU must reflect post-STE-override access type.
    // Look up the stream config to apply INSTCFG/PRIVCFG to the raw accessType so that
    // the generated event carries the correct ind/pnu values per the spec.
    AccessType eventAccessType = accessType;
    {
        size_t stripe = getStreamStripe(streamID);
        std::lock_guard<std::mutex> slock(streamLockStripes[stripe]);
        auto streamIt = streamMap.find(streamID);
        if (streamIt != streamMap.end() && streamIt->second) {
            const StreamConfig& sCfg = streamIt->second->getStreamConfiguration();
            // NEW-B fix: §7.3 — STRW promotion must precede INSTCFG/PRIVCFG overrides.
            // ARM §5.2.2: STRW=EL2/EL3 promotes unprivileged access types to privileged
            // equivalents; guard with !stage2Enabled (STRW is ignored for two-stage streams).
            if (!sCfg.stage2Enabled &&
                (sCfg.strw == StreamWorld::EL2 || sCfg.strw == StreamWorld::EL3)) {
                switch (eventAccessType) {
                    case AccessType::Read:        eventAccessType = AccessType::ReadPrivileged;          break;
                    case AccessType::Write:       eventAccessType = AccessType::WritePrivileged;         break;
                    case AccessType::Execute:     eventAccessType = AccessType::ExecutePrivileged;       break;
                    case AccessType::ReadWrite:   eventAccessType = AccessType::ReadWritePrivileged;     break;
                    case AccessType::ReadExecute: eventAccessType = AccessType::ReadExecutePrivileged;   break;
                    default: break;
                }
            }
            // INSTCFG override — mirrors stream_context.cpp translateUnlocked() logic.
            if (sCfg.instCfg == 1u) {
                if (eventAccessType == AccessType::Read)
                    eventAccessType = AccessType::Execute;
                else if (eventAccessType == AccessType::ReadPrivileged)
                    eventAccessType = AccessType::ExecutePrivileged;
            } else if (sCfg.instCfg == 2u) {
                if (eventAccessType == AccessType::Execute)
                    eventAccessType = AccessType::Read;
                else if (eventAccessType == AccessType::ExecutePrivileged)
                    eventAccessType = AccessType::ReadPrivileged;
                else if (eventAccessType == AccessType::ReadExecute)
                    eventAccessType = AccessType::Read;
                else if (eventAccessType == AccessType::ReadExecutePrivileged)
                    eventAccessType = AccessType::ReadPrivileged;
            }
            // PRIVCFG override — mirrors stream_context.cpp translateUnlocked() logic.
            if (sCfg.privCfg == 2u) {
                switch (eventAccessType) {
                    case AccessType::ReadPrivileged:          eventAccessType = AccessType::Read;        break;
                    case AccessType::WritePrivileged:         eventAccessType = AccessType::Write;       break;
                    case AccessType::ExecutePrivileged:       eventAccessType = AccessType::Execute;     break;
                    case AccessType::ReadWritePrivileged:     eventAccessType = AccessType::ReadWrite;   break;
                    case AccessType::ReadExecutePrivileged:   eventAccessType = AccessType::ReadExecute; break;
                    default: break;
                }
            } else if (sCfg.privCfg == 3u) {
                switch (eventAccessType) {
                    case AccessType::Read:        eventAccessType = AccessType::ReadPrivileged;          break;
                    case AccessType::Write:       eventAccessType = AccessType::WritePrivileged;         break;
                    case AccessType::Execute:     eventAccessType = AccessType::ExecutePrivileged;       break;
                    case AccessType::ReadWrite:   eventAccessType = AccessType::ReadWritePrivileged;     break;
                    case AccessType::ReadExecute: eventAccessType = AccessType::ReadExecutePrivileged;   break;
                    default: break;
                }
            }
        }
    }

    // ARM SMMU v3 spec: Implement fault recovery mechanisms based on fault type
    switch (faultType) {
        case FaultType::TranslationFault:
            // §7.3.13: F_TRANSLATION (0x10) must be generated for terminate-mode streams.
            // Stall-mode faults already generate the event in the stall path above.
            // CONF-GAP-20: pass eventAccessType so rnw/ind/pnu wire-format fields are set.
            // GAP NEW-2: propagate stage-2 context (S2 flag, IPA) from thread-local state
            // set by performBothStagesTranslation() when stage-2 translation failed.
            // NEW-4 fix: eventAccessType carries post-INSTCFG/PRIVCFG overrides.
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
            // Could implement page fault handling or demand paging here
            handleTranslationFaultRecovery(streamID, pasid, iova, securityState);
            break;

        case FaultType::PermissionFault:
            // §7.3.16: F_PERMISSION (0x13) must be generated for terminate-mode streams.
            // Stall-mode faults already generate the event in the stall path above.
            // CONF-GAP-20: pass eventAccessType so rnw/ind/pnu wire-format fields are set.
            // GAP NEW-2: propagate stage-2 context for permission faults that occurred at stage-2.
            // NEW-4 fix: eventAccessType carries post-INSTCFG/PRIVCFG overrides.
            generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
            // Could implement permission escalation or security logging
            handlePermissionFaultRecovery(streamID, pasid, iova, accessType, securityState);
            break;

        case FaultType::AddressSizeFault:
            // Could implement address range expansion or validation
            handleAddressSizeFaultRecovery(streamID, pasid, iova, securityState);
            break;

        case FaultType::AccessFault:
            // NEW-5 fix: Defensive fallback — this case is currently unreachable
            // (no spec-defined error code routes here), but emit F_TRANSLATION as a
            // safe fallback to ensure no event is silently dropped if this case ever
            // becomes reachable in the future.
            handleAccessFaultRecovery(streamID, pasid, iova, accessType, securityState);
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
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
            // NEW-4 fix: use eventAccessType (post-override).
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType);
            break;

        case FaultType::AccessFlagFault:
            // §7.3.15: Access flag fault → F_ACCESS (0x12).  This is the ONLY
            // fault type in this group that correctly emits F_ACCESS.
            // NEW-B fix: pass tl_stage2FaultCtx so that stage-2 AF faults carry
            // s2=true and the correct IPA in the generated EventEntry (§7.3.13).
            // NEW-4 fix: use eventAccessType (post-override).
            generateEvent(EventType::F_ACCESS, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
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
    // BUG-ANALYSIS-5 fix: also clear the stall-pending buffer on explicit
    // eventQueue clear so the two containers stay in sync.
    stallPending_.clear();
    // ARM §3.5.1: Reset PROD/CONS indices on clear (FINDING-M-01)
    eventqProd.store(0, std::memory_order_release);
    eventqCons.store(0, std::memory_order_release);
}

size_t SMMU::getEventQueueSize() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return eventQueue.size();
}

// NEW-10: ARM §6.3.96 SMMU_EVENTQ_CONS.OVACKFLG acknowledge.
// Copies the OVFLG bit (bit 31) of SMMU_EVENTQ_PROD into the OVACKFLG bit
// (bit 31) of SMMU_EVENTQ_CONS.  This makes the overflow condition inactive
// (OVFLG == OVACKFLG) without discarding any queued events or resetting the
// PROD/CONS circular indices.  Software must call this after draining the queue
// to clear the overflow indication per ARM IHI0070G.b §6.3.96.
void SMMU::acknowledgeEventQueueOverflow() {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    uint32_t prodVal = eventqProd.load(std::memory_order_relaxed);
    uint32_t consVal = eventqCons.load(std::memory_order_relaxed);
    // Extract current OVFLG (bit 31 of PROD)
    uint32_t ovflg = prodVal & (1u << 31);
    // Replace bit 31 of CONS with OVFLG — all other bits unchanged
    uint32_t newCons = (consVal & ~(1u << 31)) | ovflg;
    eventqCons.store(newCons, std::memory_order_release);
}

// NEW-11: ARM §7.3.2 F_UUT — Unsupported Upstream Transaction.
// Injects an F_UUT event into the event queue on behalf of a simulation harness.
// Gated by CR0.EVENTQEN (same as all other events): when EVENTQEN=0 the event is
// silently dropped.  eventClass=2 (IN) per NEW-1 encoding rules.
void SMMU::reportUnsupportedTransaction(StreamID streamID, PASID pasid,
                                        IOVA iova, AccessType accessType,
                                        SecurityState securityState) {
    // generateEvent() internally gates on CR0.EVENTQEN — no extra check needed here.
    generateEvent(EventType::F_UUT, streamID, pasid, iova,
                  securityState, /*isStall=*/false, /*stag=*/0,
                  accessType, /*isStage2=*/false, /*ipaValue=*/0);
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-NEW-D: IDR registers — capability bitmasks (ARM §6.3.1–6.3.8)
// ─────────────────────────────────────────────────────────────────────────────

uint32_t SMMU::getIDR0() const {
    // ARM IHI0070G.b §6.3.1 SMMU_IDR0 — verified bit positions:
    return (1u << 0)        // S2P: stage-2 translation present
         | (1u << 1)        // S1P: stage-1 translation present
         | (2u << 2)        // TTF[3:2] = 0b10 (=2): AArch64 stage-1 and stage-2
         | (2u << 6)         // HTTU[7:6] = 0b10 (=2): access flag + dirty state update (§6.3.1)
         | (1u << 5)        // BTM: broadcast TLB maintenance (receiveBroadcastTLBI() with CR2.PTM gating) — GAP-R07 §6.3.1
         | (1u << 9)        // Hyp: hypervisor stage-1 translation — mandatory for SMMUv3.2 with S1P+S2P (§6.3.1)
         | (1u << 10)       // ATS: PCIe ATS supported
         | (1u << 12)       // ASID16: 16-bit ASIDs supported
         | (1u << 14)       // SEV: stall model (WFE/SEV) supported
         | (1u << 15)       // ATOS: address translation operations (GATOS) supported
         | (1u << 16)       // PRI: page request interface supported
         | (1u << 17)       // VMW: VMID wildcard bits in CR0
         | (1u << 18)       // VMID16: 16-bit VMIDs supported
         | (1u << 23)       // ATSRECERR: extended ATS error recording (CR2.REC_CFG_ATS gating) — GAP-R04 §6.3.1/§2.5
         // BUG-2 fix: TERM_MODEL (bit 26) cleared — model implements both stall and
         // terminate behaviors and does NOT validate CD.A=1.  ARM IHI0070G.b §6.3.1:
         // when TERM_MODEL=1, C_BAD_CD must fire if CD.A=0.  Setting TERM_MODEL=0
         // (RAZ/WI IS supported) matches the model's actual behaviour.
         | (1u << 27);      // ST_LEVEL[0]: 2-level stream table supported
                            // STALL_MODEL[25:24] = 0b00: both stall and terminate models supported (§6.3.1)
}

uint32_t SMMU::getIDR1() const {
    // ARM IHI0070G.b §6.3.2 SMMU_IDR1:
    // bits[ 5: 0]: SIDSIZE  = 32 (32-bit StreamIDs)
    // bits[10: 6]: SSIDSIZE = 20 (20-bit SubstreamIDs)
    // bits[15:11]: PRIQS    = priqLog2Size
    // bits[20:16]: EVENTQS  = eventqLog2Size
    // bits[25:21]: CMDQS    = cmdqLog2Size
    return 0x20u                              // SIDSIZE = 32 in bits[5:0]
         | (0x14u << 6)                       // SSIDSIZE = 20 in bits[10:6]
         | (static_cast<uint32_t>(priqLog2Size)   << 11) // PRIQS
         | (static_cast<uint32_t>(eventqLog2Size) << 16) // EVENTQS
         | (static_cast<uint32_t>(cmdqLog2Size)   << 21) // CMDQS
         | (1u << 26)                         // ATTR_PERMS_OVR: INSTCFG+PRIVCFG overrides implemented (§6.3.2)
         | (1u << 27);                        // ATTR_TYPES_OVR: MTCFG/SHCFG/ALLOCCFG overrides implemented (§6.3.2)
}

uint32_t SMMU::getIDR2() const {
    // ARM IHI0070G.b §6.3.3 SMMU_IDR2:
    // Only field is BA_VATOS[9:0] (VATOS page base address offset).
    // This model does not implement VATOS — return 0.
    return 0u;
}

// IDR3: ARM IHI0070G.b §6.3.4 — capability bits for SMMUv3.2 features implemented.
uint32_t SMMU::getIDR3() const {
    return (1u << 2)   // HAD: hierarchical attribute disable supported
         | (1u << 4)   // XNX: execute-never differentiation for EL0/EL1 — mandatory for SMMUv3.1+ with S2P (§6.3.4)
         | (1u << 8)   // FWB: stage-2 force write-back attribute control supported
         | (1u << 10)  // RIL: range-based invalidation (RIL TLBI commands processed)
         | (1u << 11); // BBML[0]: basic bus master lock level 1 (BBML=0b01)
}

// IDR4: reserved — returns 0 for this model.
uint32_t SMMU::getIDR4() const { return 0u; }

uint32_t SMMU::getIDR5() const {
    // ARM IHI0070G.b §6.3.6 SMMU_IDR5:
    // bits[2:0]: OAS    = 5 (48-bit output address size)
    // bit  3:   RES0
    // bit  4:   GRAN4K  — 4KB granule supported
    // bit  5:   GRAN16K — 16KB granule supported
    // bit  6:   GRAN64K — 64KB granule supported
    return 5u           // OAS = 5 (48-bit) in bits[2:0]
         | (1u << 4)    // GRAN4K
         | (1u << 5)    // GRAN16K
         | (1u << 6)    // GRAN64K
         | (64u << 16); // STALL_MAX[31:16] = 64: max outstanding stall transactions (§6.3.6, non-zero when STALL_MODEL=0b00)
}

// AIDR: ARM IHI0070G.b §6.3.8 SMMU_AIDR: ArchMinorRev=2 (SMMUv3.2).
// This model implements SMMUv3.2-mandatory features: RIL, FWB, T0SZ, S2T0SZ.
uint32_t SMMU::getAIDR() const { return 0x02u; }

// IIDR: implementer-defined — returns 0.
uint32_t SMMU::getIIDR() const { return 0u; }

// ─────────────────────────────────────────────────────────────────────────────
// GAP-NEW-A: Fault injection — structure-fetch and walk external aborts
// ─────────────────────────────────────────────────────────────────────────────

// Inject F_STE_FETCH (§7.3.4) into the event queue, gated on CR0.EVENTQEN.
void SMMU::injectSteFetchAbort(StreamID streamID, SecurityState ss) {
    generateEvent(EventType::F_STE_FETCH, streamID, 0, 0,
                  ss, /*isStall=*/false, /*stag=*/0,
                  AccessType::Read, /*isStage2=*/false, /*ipaValue=*/0);
}

// Inject F_CD_FETCH (§7.3.10) into the event queue, gated on CR0.EVENTQEN.
void SMMU::injectCdFetchAbort(StreamID streamID, PASID pasid, SecurityState ss) {
    generateEvent(EventType::F_CD_FETCH, streamID, pasid, 0,
                  ss, /*isStall=*/false, /*stag=*/0,
                  AccessType::Read, /*isStage2=*/false, /*ipaValue=*/0);
}

// Inject F_WALK_EABT (§7.3.12) into the event queue, gated on CR0.EVENTQEN.
void SMMU::injectWalkEabt(StreamID streamID, PASID pasid, IOVA iova, SecurityState ss) {
    generateEvent(EventType::F_WALK_EABT, streamID, pasid, iova,
                  ss, /*isStall=*/false, /*stag=*/0,
                  AccessType::Read, /*isStage2=*/false, /*ipaValue=*/0);
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-NEW-E: STATUSR / IRQ_CTRL / CTRLACK registers (§6.3.45–6.3.47)
// ─────────────────────────────────────────────────────────────────────────────

// SMMU_STATUSR: bit 0 = DORMANT.  This SW model has no dormant state — always 0.
uint32_t SMMU::getStatusr() const {
    return statusr_.load(std::memory_order_acquire);
}

// SMMU_IRQ_CTRL write: store v and echo immediately to SMMU_IRQ_CTRLACK
// (synchronous handshake — hardware would echo after internal synchronization).
void SMMU::setIrqCtrl(uint32_t v) {
    irqCtrl_.store(v, std::memory_order_release);
    irqCtrlAck_.store(v, std::memory_order_release);
}

// SMMU_IRQ_CTRLACK read.
uint32_t SMMU::getIrqCtrlAck() const {
    return irqCtrlAck_.load(std::memory_order_acquire);
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-NEW-F: GATOS address translation wrapper (§9.1–9.9)
// ─────────────────────────────────────────────────────────────────────────────

// Returns a 64-bit GATOS_PAR value per ARM IHI0070G.b §6.3.40:
//   bit 0     = FAULT (1 = fault)
//   bits[9:8] = SH (shareability; 0b11 = Inner Shareable)
//   bit 11    = SIZE (0 = 4KB page — implicit)
//   bits[55:12] = ADDR (PA[55:12], page-aligned PA)
//   bits[63:56] = ATTR (0xFF = Normal WB/WA cacheable)
// The internal translate() call may produce side-effects (events, stalls) but
// those are acceptable — GATOS is a full translation that records faults.
// GAP-L: ARM §9.1.4 / §6.3.40 — GATOS_PAR FAULTCODE mapping.
// Returns the FAULTCODE byte corresponding to the given EventType.
uint64_t SMMU::mapEventTypeToGatosFaultCode(EventType t) {
    // ARM IHI0070G.b §9.1.5 Table: ATOS_PAR.FAULTCODE encodings
    switch (t) {
        case EventType::F_UUT:               return 0x01u;
        case EventType::C_BAD_STREAMID:      return 0x02u;
        case EventType::F_STE_FETCH:         return 0x03u;
        case EventType::C_BAD_STE:           return 0x04u;
        case EventType::F_BAD_ATS_TREQ:      return 0x05u;
        case EventType::F_STREAM_DISABLED:   return 0x06u;
        case EventType::F_TRANSL_FORBIDDEN:  return 0x07u;
        case EventType::C_BAD_SUBSTREAMID:   return 0x08u;
        case EventType::F_CD_FETCH:          return 0x09u;
        case EventType::C_BAD_CD:            return 0x0Au;
        case EventType::F_WALK_EABT:         return 0x0Bu;
        case EventType::F_TRANSLATION:       return 0x10u;
        case EventType::F_ADDR_SIZE:         return 0x11u;
        case EventType::F_ACCESS:            return 0x12u;
        case EventType::F_PERMISSION:        return 0x13u;
        case EventType::F_TLB_CONFLICT:      return 0x20u;
        case EventType::F_CFG_CONFLICT:      return 0x21u;
        case EventType::F_VMS_FETCH:         return 0x25u; // §9.1.5 / NEW-GAP-B
        default:                             return 0x10u; // fallback: F_TRANSLATION
    }
}

uint64_t SMMU::gatosTranslate(StreamID streamID, PASID pasid, IOVA iova,
                               AccessType accessType, SecurityState securityState) {
    // Snapshot the event queue size before translation so we can identify
    // any new event appended by the translation (GAP-L fix).
    size_t evtSizeBefore = 0u;
    {
        std::lock_guard<std::recursive_mutex> lk(queueMutex);
        evtSizeBefore = eventQueue.size();
    }
    TranslationResult result = translate(streamID, pasid, iova, accessType, securityState);
    if (result.isError()) {
        // GAP-L fix: ARM §9.1.4 / §6.3.40 — GATOS_PAR fault syndrome.
        // Determine FAULTCODE and REASON from the most recent event appended
        // during this translate() call, rather than hardcoding 0x10/0b00.
        uint64_t faultCode = 0x10u; // default: F_TRANSLATION
        uint64_t reason    = 0u;    // default: stage-1 fault
        uint64_t faddr     = 0u;    // default: no IPA (stage-1/config fault)
        {
            std::lock_guard<std::recursive_mutex> lk(queueMutex);
            if (eventQueue.size() > evtSizeBefore) {
                // Use the last event that was added by this translate() call.
                const EventEntry& ev = eventQueue.back();
                faultCode = mapEventTypeToGatosFaultCode(ev.type);
                if (ev.s2) {
                    // §9.1.4: REASON encodes the stage-2 translation context:
                    // 0b01 = stage-2 during CD fetch (eventClass=0 CD)
                    // 0b10 = stage-2 during TT walk  (eventClass=1 TTD)
                    // 0b11 = stage-2 on IPA input    (eventClass=2 IN)
                    reason = static_cast<uint64_t>(ev.eventClass) + 1u;
                    // §9.1.4 NEW-GAP-I: FADDR[55:12] = page-aligned IPA input to stage 2.
                    // This is the IPA that caused the stage-2 fault (CD address, descriptor
                    // address, or IPA output of stage-1, depending on REASON value).
                    faddr = ev.ipa & 0x00FFFFFFFFFFF000ULL;
                }
            }
        }
        // GATOS_PAR fault format (§6.3.40):
        //   bit 0       = FAULT=1
        //   bits[2:1]   = REASON
        //   bits[11:4]  = FAULTCODE
        //   bits[55:12] = FADDR (page-aligned IPA, non-zero only when REASON≠0b00)
        return 0x1ULL | (reason << 1) | (faultCode << 4) | faddr;
    }
    uint64_t pa         = result.getValue().physicalAddress;
    uint64_t addr_field = pa & 0x00FFFFFFFFFFF000ULL;              // PA bits[55:12]
    uint64_t sh         = static_cast<uint64_t>(3u) << 8;         // ISH (0b11=3) in bits[9:8]
    uint64_t attr       = static_cast<uint64_t>(0xFFu) << 56;     // Normal WB/WA in bits[63:56]
    return attr | sh | addr_field;
    // SIZE (bit 11) = 0 (4KB page — implicit)
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

    // ARM §6.3.17: Do not process commands when GERROR.CMDQ_ERR is active.
    // An error is active when GERROR[x] != GERRORN[x] (unacknowledged).
    // Software must write GERRORN via clearGerror() to acknowledge before
    // queue processing can resume.  BUG-03/SPEC-09.
    // BUG-2 fix: this check MUST be inside queueMutex to close the TOCTOU
    // window.  A concurrent signalGerror(GERROR_CMDQ_ERR) between the old
    // pre-lock check and the lock acquisition would be missed, allowing
    // commands to be consumed while the error is active.  Moving the check
    // inside the lock makes it atomic with respect to command dequeue.
    // The lock-ordering invariant (stripe locks must not be acquired while
    // queueMutex is held) is preserved — this check does not acquire any
    // stripe lock.
    if ((gerrorStatus.load(std::memory_order_relaxed) ^ gerrorNStatus.load(std::memory_order_relaxed)) & GERROR_CMDQ_ERR) {
        return;
    }
    while (!commandQueue.empty()) {
        // BUG-04 fix: copy command by value before pop_front() so that the
        // reference is not dangling when we inspect command.type afterwards.
        CommandEntry command = commandQueue.front();
        commandQueue.pop_front();
        // ARM §3.5.1: Advance consumer index on dequeue (FINDING-M-01).
        // BUG-1 fix: §6.3.28 — CMDQ_CONS.ERR bits [31:24] must persist until
        // software clears them.  A plain store(advanceQueueIndex()) zeros the
        // upper 12 bits on every dequeue.  Use a read-modify-write so that
        // only the RD field (bits [19:0]) is updated; bits [31:20] are
        // preserved verbatim.  advanceQueueIndex() operates on the RD portion
        // only (result is in range [0, 2^(log2size+1)-1], at most 20 bits).
        {
            uint32_t oldCons = cmdqCons.load(std::memory_order_relaxed);
            uint32_t newRD   = advanceQueueIndex(oldCons & 0xFFFFFu, cmdqLog2Size);
            cmdqCons.store((oldCons & ~static_cast<uint32_t>(0xFFFFFu)) | newRD,
                           std::memory_order_release);
        }

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
                // CONF-GAP-17: Write CERROR_ILL to CMDQ_CONS.ERR before signalling GERROR.
                writeCmdqConsErr(CERROR_ILL);
                // BUG-NEW-CPP-1 fix: use signalGerror() CAS loop instead of
                // the TOCTOU load-compare-fetch_xor pattern.
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // §4.8 / FINDING-NEW-27: CS=0b00 (SIG_NONE) → no completion signal.
            // CONF-GAP-18: Record the signal type for CS=1 (IRQ) and CS=2 (MSI).
            if (command.cs == 1u) {
                cmdSyncLastSig_.store(static_cast<uint8_t>(CmdSyncSignalType::Irq), std::memory_order_release);
            } else if (command.cs == 2u) {
                cmdSyncLastSig_.store(static_cast<uint8_t>(CmdSyncSignalType::Msi), std::memory_order_release);
            }
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

        // GAP-H fix: ARM IHI0070G.b §3.13.6 — PRIQ overflow auto-PRG_RESPONSE.
        // When the PRIQ is full, the SMMU must automatically generate a FAILURE
        // response for the overflowing page request group so the device is not
        // left stalled indefinitely waiting for a response that will never arrive.
        PRIAutoFailure autoFail(request.streamID, request.pasid,
                                request.prgIndex, getCurrentTimestamp());
        priAutoFailures_.push_back(autoFail);
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

// GAP-H: ARM §3.13.6 — return auto-generated FAILURE responses from PRIQ overflow.
std::vector<PRIAutoFailure> SMMU::getPRIAutoFailures() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    return priAutoFailures_;
}

void SMMU::clearPRIAutoFailures() {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    priAutoFailures_.clear();
}

// Task 5.3: Cache Invalidation Command Handling (Task 5.3.4)
void SMMU::executeInvalidationCommand(const CommandEntry& command) {
    // ARM SMMU v3 spec: Execute cache invalidation commands
    switch (command.type) {
        case CommandType::CFGI_STE:
            // ARM §4.3.1 (CONF-GAP-2 fix): CMD_CFGI_STE is a cache invalidation command.
            // If the StreamID is unknown there is nothing cached to invalidate — this is a
            // silent no-op.  §4.3.1 defines no error condition for an unknown StreamID;
            // C_BAD_STREAMID is raised only on the *translation* path (§7.3.3), not here.
            invalidateStreamCache(command.streamID);
            break;

        case CommandType::CFGI_ALL:
            // ARM §4.3.2: CMD_CFGI_ALL (range==31) or CMD_CFGI_STE_RANGE (range<31).
            // NOTE: shifting uint32_t by 32 bits is UB, so range==31 is handled separately.
            if (command.range == 31) {
                // CMD_CFGI_ALL — full global STE-cache / TLB invalidation
                invalidateTranslationCache();
            } else if (command.range > 31) {
                // BUG-3 fix: ARM §4.3.2 range field is 5 bits (0–31); values > 31 are
                // architecturally impossible.  Clamp to CFGI_ALL to avoid shifting
                // uint32_t by 33+ bits, which is undefined behaviour per C++11 §5.8.
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
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid, command.asid, command.vmid, command.startAddress, command.ril, command.tg, command.num, command.scale);
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
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid, command.asid, command.vmid, command.startAddress, command.ril, command.tg, command.num, command.scale);
            break;

        default:
            // Invalid invalidation command
            generateEvent(EventType::C_BAD_STE, command.streamID, command.pasid, command.startAddress, SecurityState::NonSecure);
            break;
    }
}

void SMMU::executeTLBInvalidationCommand(CommandType type, uint16_t asid, uint16_t vmid, IOVA va) {
    // Broadcast TLBIs carry no stream context (ARM §3.17)
    executeTLBInvalidationCommand(type, 0, 0, asid, vmid, va);
}

void SMMU::executeTLBInvalidationCommand(CommandType type, StreamID streamID, PASID pasid, uint16_t asid, uint16_t vmid, IOVA iova, bool ril, uint8_t tg, uint8_t num, uint8_t scale) {
    // CONF-GAP-11: CR2.PTM controls *broadcast* TLB maintenance participation
    // (§6.3.12): when PTM=0 the SMMU ignores OS-level broadcast TLB invalidations
    // propagated by hardware.  Command-queue TLBI commands are explicit software
    // commands sent directly to the SMMU — they are NOT broadcast operations and
    // MUST NOT be gated by PTM.  PTM only affects the receiveBroadcastTLBI() path.

    // CONF-GAP-8: Helper to compute range end for RIL TLBI.
    // Granule size in bytes: tg=0→4KB, tg=1→64KB, tg=2→16KB
    // Range covers (num+1) * (1 << (5*scale)) granules starting at iova.
    auto computeRILRangeEnd = [](IOVA start, uint8_t tg_, uint8_t num_, uint8_t scale_) -> IOVA {
        uint64_t granule;
        switch (tg_) {
            case 1:  granule = 64u * 1024u; break;
            case 2:  granule = 16u * 1024u; break;
            default: granule = 4u  * 1024u; break;
        }
        // scale_ clamped to 0-7 per spec; 5*scale_ max = 35 which fits uint64_t
        uint64_t scaleShift = (scale_ > 7u) ? 35u : (static_cast<uint64_t>(5u) * scale_);
        uint64_t blocks = static_cast<uint64_t>(num_) + 1u;
        uint64_t rangeBytes = blocks * (static_cast<uint64_t>(1u) << scaleShift) * granule;
        if (rangeBytes == 0 || start > UINT64_MAX - rangeBytes + 1u) {
            return UINT64_MAX; // saturate
        }
        return start + rangeBytes - 1u;
    };

    // CONF-GAP-12: VMID wildcard masking helper.
    auto getVmidMask = [this]() -> uint16_t {
        uint8_t vmw = static_cast<uint8_t>((cr0_.load(std::memory_order_acquire) >> CR0_VMW_SHIFT) & 7u);
        if (vmw >= 16u) return 0u; // mask all bits (wildcard = all VMIDs)
        return static_cast<uint16_t>((0xFFFFu << vmw) & 0xFFFFu);
    };

    // ARM SMMU v3 spec: Execute TLB-specific invalidation commands
    switch (type) {
        case CommandType::TLBI_NH_ALL:
        case CommandType::TLBI_NSNH_ALL:
            // Non-secure all TLB invalidation — global flush
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_NH_VA:
            // CONF-GAP-6: VA+ASID targeted invalidation (or RIL range)
            if (ril) {
                tlbCache->invalidateByVARange(iova, computeRILRangeEnd(iova, tg, num, scale), asid);
            } else {
                tlbCache->invalidateByVAAndASID(iova, asid);
            }
            break;

        case CommandType::TLBI_NH_VAA:
            // CONF-GAP-6: VAA = VA all-ASID invalidation (or RIL range, all ASIDs)
            if (ril) {
                // For VAA, invalidate the range for all ASIDs (pass 0 and use VA-only for each page)
                IOVA rangeEnd = computeRILRangeEnd(iova, tg, num, scale);
                // invalidateByVARange with wildcard asid: use invalidateByVA for the range
                // Approximate: scan page-by-page (small ranges expected)
                IOVA cur = iova & ~PAGE_MASK;
                while (cur <= rangeEnd) {
                    tlbCache->invalidateByVA(cur);
                    if (cur > UINT64_MAX - PAGE_SIZE) break;
                    cur += PAGE_SIZE;
                }
            } else {
                tlbCache->invalidateByVA(iova);
            }
            break;

        case CommandType::TLBI_NH_ASID:
            // ARM §4.4: ASID-targeted invalidation (CMD_TLBI_NH_ASID, opcode 0x11)
            tlbCache->invalidateByASID(asid);
            break;

        case CommandType::TLBI_EL2_ALL:
            // EL2 TLB invalidation — global flush
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_EL2_VA:
            // CONF-GAP-6: VA+ASID targeted invalidation
            if (ril) {
                tlbCache->invalidateByVARange(iova, computeRILRangeEnd(iova, tg, num, scale), asid);
            } else {
                tlbCache->invalidateByVAAndASID(iova, asid);
            }
            break;

        case CommandType::TLBI_EL2_VAA:
            // CONF-GAP-6: VAA EL2 — VA all-ASID invalidation
            if (ril) {
                IOVA rangeEnd = computeRILRangeEnd(iova, tg, num, scale);
                IOVA cur = iova & ~PAGE_MASK;
                while (cur <= rangeEnd) {
                    tlbCache->invalidateByVA(cur);
                    if (cur > UINT64_MAX - PAGE_SIZE) break;
                    cur += PAGE_SIZE;
                }
            } else {
                tlbCache->invalidateByVA(iova);
            }
            break;

        case CommandType::TLBI_EL2_ASID:
            // ARM §4.4: ASID-targeted invalidation (CMD_TLBI_EL2_ASID, opcode 0x21)
            tlbCache->invalidateByASID(asid);
            break;

        case CommandType::TLBI_S12_VMALL:
            // ARM §4.4: VMID-targeted invalidation — all entries for this VMID
            // CONF-GAP-12: apply VMW wildcard mask
            tlbCache->invalidateByVMIDWithMask(vmid, getVmidMask());
            break;

        case CommandType::TLBI_S2_IPA:
            // CONF-GAP-7: ARM §4.4 TLBI_S2_IPA — IPA-selective invalidation.
            // iova carries the IPA operand. Invalidate only TLB entries whose
            // stage-1 output (entry.ipa) falls within the specified IPA range.
            // This avoids over-invalidation of entries with different IPAs that
            // share the same VMID.
            if (ril) {
                tlbCache->invalidateByVMIDAndIPA(vmid, getVmidMask(),
                                                 iova, computeRILRangeEnd(iova, tg, num, scale));
            } else {
                // BUG-9 fix: use PAGE_MASK (from types.h) instead of the literal 0xFFFu.
                // Single-page IPA invalidation: align the IPA down to the 4KB page base
                // and compute the end of the page using PAGE_MASK (= PAGE_SIZE - 1 = 0xFFF).
                IOVA pageBase = iova & ~static_cast<IOVA>(PAGE_MASK);
                tlbCache->invalidateByVMIDAndIPA(vmid, getVmidMask(),
                                                 pageBase,
                                                 pageBase | static_cast<IOVA>(PAGE_MASK));
            }
            break;

        case CommandType::TLBI_S_S12_VMALL:
            // Secure Stage-1+2 VMID-targeted invalidation — apply VMW mask
            tlbCache->invalidateByVMIDWithMask(vmid, getVmidMask());
            break;

        case CommandType::TLBI_S_S2_IPA:
            // CONF-GAP-7: Secure Stage-2 IPA-selective invalidation (same logic as TLBI_S2_IPA).
            if (ril) {
                tlbCache->invalidateByVMIDAndIPA(vmid, getVmidMask(),
                                                 iova, computeRILRangeEnd(iova, tg, num, scale));
            } else {
                // BUG-9 fix: use PAGE_MASK instead of the literal 0xFFFu (consistent with
                // TLBI_S2_IPA fix above).
                IOVA pageBase = iova & ~static_cast<IOVA>(PAGE_MASK);
                tlbCache->invalidateByVMIDAndIPA(vmid, getVmidMask(),
                                                 pageBase,
                                                 pageBase | static_cast<IOVA>(PAGE_MASK));
            }
            break;

        // §4.1.1 / CT-30: EL3 and Secure EL2 TLB invalidation
        case CommandType::TLBI_EL3_ALL:
        case CommandType::TLBI_SNH_ALL:
            // EL3 / Secure NH TLB invalidation — global flush in SW model
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_EL3_VA:
            // EL3 VA targeted invalidation
            if (ril) {
                tlbCache->invalidateByVARange(iova, computeRILRangeEnd(iova, tg, num, scale), asid);
            } else {
                tlbCache->invalidateByVAAndASID(iova, asid);
            }
            break;

        case CommandType::TLBI_S_EL2_ALL:
            // Secure EL2 TLB invalidation — global flush in SW model
            invalidateTranslationCache();
            break;

        case CommandType::TLBI_S_EL2_VA:
            // Secure EL2 VA targeted invalidation
            if (ril) {
                tlbCache->invalidateByVARange(iova, computeRILRangeEnd(iova, tg, num, scale), asid);
            } else {
                tlbCache->invalidateByVAAndASID(iova, asid);
            }
            break;

        case CommandType::TLBI_S_EL2_VAA:
            // Secure EL2 VAA invalidation
            if (ril) {
                IOVA rangeEnd = computeRILRangeEnd(iova, tg, num, scale);
                IOVA cur = iova & ~PAGE_MASK;
                while (cur <= rangeEnd) {
                    tlbCache->invalidateByVA(cur);
                    if (cur > UINT64_MAX - PAGE_SIZE) break;
                    cur += PAGE_SIZE;
                }
            } else {
                tlbCache->invalidateByVA(iova);
            }
            break;

        case CommandType::TLBI_S_EL2_ASID:
            // Secure EL2 ASID-targeted invalidation
            tlbCache->invalidateByASID(asid);
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
            // ARM §4.3.1 (CONF-GAP-2): CMD_CFGI_STE is a cache invalidation command.
            // Unknown StreamID → silent no-op (nothing cached to evict).
            invalidateStreamCache(command.streamID);
            break;

        case CommandType::CFGI_ALL:
            // ARM §4.3.2: CMD_CFGI_ALL (range==31) or CMD_CFGI_STE_RANGE (range<31).
            // NOTE: shifting uint32_t by 32 bits is UB, so range==31 is handled separately.
            if (command.range == 31) {
                // CMD_CFGI_ALL — full global STE-cache / TLB invalidation
                invalidateTranslationCache();
            } else if (command.range > 31) {
                // BUG-3 fix: ARM §4.3.2 range field is 5 bits (0–31); values > 31 are
                // architecturally impossible.  Clamp to CFGI_ALL to avoid shifting
                // uint32_t by 33+ bits, which is undefined behaviour per C++11 §5.8.
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
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid, command.asid, command.vmid, command.startAddress, command.ril, command.tg, command.num, command.scale);
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
            executeTLBInvalidationCommand(command.type, command.streamID, command.pasid, command.asid, command.vmid, command.startAddress, command.ril, command.tg, command.num, command.scale);
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

        case CommandType::CFGI_STE:
            // CONF-GAP-2 fix / ARM §4.3.1: CMD_CFGI_STE is a pure cache invalidation.
            // Unknown StreamID → silent no-op (nothing cached to evict).
            // C_BAD_STREAMID is a *transaction-path* error (§7.3.3), not raised here.
            executeInvalidationCommandLocked(command, queueLock);
            break;

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
            // CONF-GAP-24: Record the outcome BEFORE erasing the stall record so
            // software can observe which disposition was chosen via getResumeOutcome().
            std::lock_guard<std::mutex> slock(stallQueueMutex_);
            auto it = stallQueue_.find(command.stag);
            if (it != stallQueue_.end() && it->second.streamID == command.streamID) {
                // Classify outcome per ARM §4.6 Ac/Ab field encoding.
                ResumeOutcome outcome;
                if (command.action) {
                    outcome = ResumeOutcome::Retry;
                } else if (command.abort) {
                    outcome = ResumeOutcome::Abort;
                } else {
                    outcome = ResumeOutcome::Terminate;
                }
                // Record outcome for one-shot query via getResumeOutcome().
                resumeOutcomes_[command.stag] = outcome;
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
            // §4.6.1 (CONF-GAP-4 fix): DPTI commands require IDR3.DPT=1.
            // This model does not implement DPT (IDR3.DPT=0), so DPTI_ALL and
            // DPTI_PA must result in CERROR_ILL per §4.6.1.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        default:
            // Unknown command type — ARM §6.3.17: set CMDQ_ERR (FINDING-M-06)
            // BUG-NEW-05 fix: do not generate C_BAD_STE — the spec defines no
            // event type for "unknown command opcode".  GERROR.CMDQ_ERR is the
            // correct signal to software.
            // CONF-GAP-17: Write CERROR_ILL to CMDQ_CONS.ERR for illegal commands.
            writeCmdqConsErr(CERROR_ILL);
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

// BUG-ANALYSIS-1 fix: acquire queueMutex before reading/modifying the GERROR pair.
// The previous CAS-loop implementation was lockless while clearGerror() held
// queueMutex, creating a TOCTOU race: clearGerror() could read gerrorStatus and
// gerrorNStatus, then signalGerror() could modify gerrorStatus between those two
// loads, causing clearGerror() to compute activeBits from a stale snapshot and
// subsequently toggle GERRORN for an already-inactive error — the CONSTRAINED
// UNPREDICTABLE behavior prohibited by ARM IHI0070G.b §6.3.20.
// Using the same mutex for both functions provides the necessary mutual exclusion
// without requiring any CAS retry logic.
// ARM IHI0070G.b §6.3.19: "SMMU does not toggle bit[x] if already active."
void SMMU::signalGerror(uint32_t bits) {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    uint32_t cur_gerror  = gerrorStatus.load(std::memory_order_relaxed);
    uint32_t cur_gerrorn = gerrorNStatus.load(std::memory_order_relaxed);
    // active bits: those where GERROR[x] != GERRORN[x]
    uint32_t active   = cur_gerror ^ cur_gerrorn;
    // Only toggle bits that are currently inactive
    uint32_t inactive = bits & ~active;
    if (inactive == 0) {
        return;  // all requested bits already active — no-op per ARM §6.3.19
    }
    gerrorStatus.store(cur_gerror ^ inactive, std::memory_order_relaxed);
}

// ARM §6.3.9 SMMU_CR0.SMMUEN and §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01, FINDING-NEW-09)
// §6.3.9 SMMU_CR0 register (CT-33)
void SMMU::setCR0(uint32_t value) {
    // BUG-CPP-DBGR-1 fix: §6.3.9 — derive smmuen_ from `value` directly (not by
    // re-reading cr0_ non-atomically), and use release stores for both so that
    // any thread that observes smmuen_==true is also guaranteed to see the updated cr0_.
    cr0_.store(value, std::memory_order_release);
    smmuen_.store((value & CR0_SMMUEN) != 0u, std::memory_order_release);
    // CONF-GAP-9: CR0ACK mirrors CR0 synchronously (software model of §6.3.10)
    cr0ack_.store(value, std::memory_order_release);
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

// CONF-GAP-9: SMMU_CR0ACK register (§6.3.10) — synchronous mirror of CR0.
uint32_t SMMU::getCR0ACK() const {
    return cr0ack_.load(std::memory_order_acquire);
}

void SMMU::setCR0ACK(uint32_t v) {
    cr0ack_.store(v, std::memory_order_release);
}

// CONF-GAP-10: SMMU_CR1 register (§6.3.11) — table/queue memory attributes.
void SMMU::setCR1(uint32_t value) {
    cr1_.store(value, std::memory_order_release);
}

uint32_t SMMU::getCR1() const {
    return cr1_.load(std::memory_order_acquire);
}

// CONF-GAP-13: GBPA full configuration (§6.3.22).
void SMMU::setGbpaConfig(const GbpaConfig& cfg) {
    {
        std::lock_guard<std::recursive_mutex> glock(queueMutex);
        gbpaConfig_ = cfg;
    }
    // Keep backward-compat gbpaAbort_ in sync with cfg.abort.
    gbpaAbort_.store(cfg.abort, std::memory_order_release);
}

GbpaConfig SMMU::getGbpaConfig() const {
    std::lock_guard<std::recursive_mutex> glock(queueMutex);
    return gbpaConfig_;
}

// CONF-GAP-3: 2-level stream table format (§3.3.1.2).
void SMMU::setStrtabFormat(StreamTableFormat fmt) {
    strtabFmt_.store(static_cast<uint8_t>(fmt), std::memory_order_release);
}

StreamTableFormat SMMU::getStrtabFormat() const {
    return static_cast<StreamTableFormat>(strtabFmt_.load(std::memory_order_acquire));
}

void SMMU::setStrtabSplit(uint8_t split) {
    // Bug-1 fix: acquire queueMutex to serialise against validateStreamID2Level()
    // which also holds queueMutex when reading strtabLog2Size_ and strtabSplit_.
    // Without this lock a concurrent reconfiguration racing with stream-ID validation
    // could produce a mixed snapshot (new split + old log2size) and underflow l1Bits.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // Only values 6, 8, 10 are architecturally defined (§6.3.25 STRTAB_BASE_CFG.SPLIT).
    // Reserved values are treated as 6 per the specification.
    if (split != 6u && split != 8u && split != 10u) {
        split = 6u;
    }
    strtabSplit_.store(split, std::memory_order_release);
}

uint8_t SMMU::getStrtabSplit() const {
    return strtabSplit_.load(std::memory_order_acquire);
}

bool SMMU::validateStreamID2Level(StreamID streamID) const {
    // 2-level validation: L1 index = streamID >> split, L2 index = streamID & ((1<<split)-1)
    // L1 table size = 2^(log2size - split), L2 table size = 2^split
    // BUG-7 fix / Bug-1 fix: acquire queueMutex before reading both strtabLog2Size_
    // and strtabSplit_.  A concurrent reconfiguration that changes both fields creates
    // a TOCTOU window: a mixed snapshot (new log2size + old split) can cause
    // l1Bits = log2sz - split to underflow.  Holding queueMutex here serialises
    // against setStrtabLog2Size() and setStrtabSplit(), both of which now also hold
    // queueMutex (Bug-1 fix: those two functions previously did not acquire the lock).
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    uint8_t log2sz = strtabLog2Size_.load(std::memory_order_acquire);
    uint8_t split  = strtabSplit_.load(std::memory_order_acquire);
    // Guard against inconsistent snapshot (new log2size + old split underflow).
    if (log2sz < split) {
        return false; // inconsistent snapshot — conservatively reject
    }
    if (split >= log2sz) {
        // All StreamIDs map to a single L2 table — only check upper log2sz bits
        uint64_t limit = (uint64_t)1u << log2sz;
        return static_cast<uint64_t>(streamID) < limit;
    }
    // L1 index range: 0 .. 2^(log2sz - split) - 1
    uint32_t l1Bits = log2sz - split;
    uint64_t l1Limit = (uint64_t)1u << l1Bits;
    uint32_t l1Index = streamID >> split;
    if (static_cast<uint64_t>(l1Index) >= l1Limit) {
        return false;
    }
    return true;
}

// CONF-GAP-17: CMDQ_CONS.ERR field accessor and writer (§6.3.17).
uint32_t SMMU::getCmdqConsErr() const {
    return (cmdqCons.load(std::memory_order_acquire) >> CMDQ_CONS_ERR_SHIFT) & 0xFFu;
}

void SMMU::writeCmdqConsErr(uint32_t errCode) {
    // Atomic CAS loop: update only the ERR[31:24] field without touching other bits.
    uint32_t expected = cmdqCons.load(std::memory_order_relaxed);
    uint32_t desired;
    do {
        desired = (expected & ~(0xFFu << CMDQ_CONS_ERR_SHIFT)) |
                  ((errCode & 0xFFu) << CMDQ_CONS_ERR_SHIFT);
    } while (!cmdqCons.compare_exchange_weak(expected, desired,
                                              std::memory_order_release,
                                              std::memory_order_relaxed));
}

// CONF-GAP-18: CMD_SYNC MSI signalling registers and signal type accessor.
void SMMU::setCmdqSyncMsiAttr(uint32_t v) {
    cmdqSyncMsiAttr_.store(v, std::memory_order_release);
}

uint32_t SMMU::getCmdqSyncMsiAttr() const {
    return cmdqSyncMsiAttr_.load(std::memory_order_acquire);
}

void SMMU::setCmdqSyncMsiAddr(uint64_t v) {
    cmdqSyncMsiAddr_.store(v, std::memory_order_release);
}

uint64_t SMMU::getCmdqSyncMsiAddr() const {
    return cmdqSyncMsiAddr_.load(std::memory_order_acquire);
}

void SMMU::setCmdqSyncMsiData(uint32_t v) {
    cmdqSyncMsiData_.store(v, std::memory_order_release);
}

uint32_t SMMU::getCmdqSyncMsiData() const {
    return cmdqSyncMsiData_.load(std::memory_order_acquire);
}

CmdSyncSignalType SMMU::getCmdSyncLastSignalType() const {
    return static_cast<CmdSyncSignalType>(cmdSyncLastSig_.load(std::memory_order_acquire));
}

// §6.3.4 SMMU_STRTAB_BASE_CFG.LOG2SIZE (CT-04)
void SMMU::setStrtabLog2Size(uint8_t log2size) {
    // Bug-1 fix: acquire queueMutex to serialise against validateStreamID2Level()
    // which also holds queueMutex when reading strtabLog2Size_ and strtabSplit_.
    // Without this lock a concurrent stream-ID validation racing with a LOG2SIZE
    // update could observe a mixed snapshot and underflow l1Bits.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // Clamp to 32 — matches max SIDSIZE per ARM §6.3.4 IDR1 (SIDSIZE field is
    // bits [5:0], range 0–32 inclusive).  Per §6.3.25 the effective LOG2SIZE used
    // for range checking is MIN(LOG2SIZE, SIDSIZE); values 33–63 are representable
    // in the 6-bit field but exceed the maximum SIDSIZE (32) and add no coverage.
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
    // CONF-GAP-23: ARM IHI0070G.b §6.3.9 requires PRIQEN (bit 1) to be set
    // alongside SMMUEN|EVENTQEN|CMDQEN when the SMMU is globally enabled.
    // acquire/release ordering ensures visibility to concurrent translate() readers.
    uint32_t newVal = cr0_.fetch_or(CR0_SMMUEN | CR0_PRIQEN | CR0_EVENTQEN | CR0_CMDQEN, std::memory_order_acq_rel)
                      | CR0_SMMUEN | CR0_PRIQEN | CR0_EVENTQEN | CR0_CMDQEN;
    smmuen_.store(true, std::memory_order_release);
    // CONF-GAP-9: sync CR0ACK to match updated CR0
    cr0ack_.store(newVal, std::memory_order_release);
}

void SMMU::disable() {
    // BUG-NEW-CPP-4 fix: use fetch_and (atomic read-modify-write) to clear only
    // CR0_SMMUEN without touching any other bits.  The previous load-then-store
    // pattern allowed a concurrent setCR0() write between the load and store to
    // be silently lost.
    uint32_t newVal = cr0_.fetch_and(~static_cast<uint32_t>(CR0_SMMUEN), std::memory_order_acq_rel)
                      & ~static_cast<uint32_t>(CR0_SMMUEN);
    smmuen_.store(false, std::memory_order_release);
    // CONF-GAP-9: sync CR0ACK to match updated CR0
    cr0ack_.store(newVal, std::memory_order_release);
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
    // CONF-GAP-13: keep gbpaConfig_.abort in sync for full GBPA config consistency.
    {
        std::lock_guard<std::recursive_mutex> glock(queueMutex);
        gbpaConfig_.abort = abort;
    }
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

// CONF-GAP-24: ARM §3.12.2 / §4.6 CMD_RESUME outcome observability.
// One-shot read: removes the entry from resumeOutcomes_ after returning it.
ResumeOutcome SMMU::getResumeOutcome(uint16_t stag) {
    std::lock_guard<std::mutex> slock(stallQueueMutex_);
    auto it = resumeOutcomes_.find(stag);
    if (it == resumeOutcomes_.end()) {
        return ResumeOutcome::None;
    }
    ResumeOutcome outcome = it->second;
    resumeOutcomes_.erase(it);
    return outcome;
}

void SMMU::clearResumeOutcomes() {
    std::lock_guard<std::mutex> slock(stallQueueMutex_);
    resumeOutcomes_.clear();
}

// CONF-GAP-11: §6.3.12 CR2.PTM — broadcast TLB maintenance.
// Checks PTM bit before executing the invalidation; NS-targeted commands
// (TLBI_NH_*, TLBI_NSNH_ALL, TLBI_EL2_*) are gated by PTM.  Secure and
// EL3 commands are not broadcast maintenance and always execute.
void SMMU::receiveBroadcastTLBI(CommandType type, uint16_t asid, uint16_t vmid, IOVA va) {
    bool isNsEL1Tlbi = (type == CommandType::TLBI_NH_ALL  ||
                        type == CommandType::TLBI_NH_VA   ||
                        type == CommandType::TLBI_NH_VAA  ||
                        type == CommandType::TLBI_NH_ASID ||
                        type == CommandType::TLBI_NSNH_ALL ||
                        type == CommandType::TLBI_EL2_ALL ||
                        type == CommandType::TLBI_EL2_VA  ||
                        type == CommandType::TLBI_EL2_VAA ||
                        type == CommandType::TLBI_EL2_ASID);
    if (isNsEL1Tlbi && (cr2_.load(std::memory_order_acquire) & CR2_PTM) == 0u) {
        return; // PTM=0: SMMU does not participate in this broadcast
    }
    executeTLBInvalidationCommand(type, asid, vmid, va);
}

void SMMU::generateEvent(EventType type, StreamID streamID, PASID pasid, IOVA address,
                         SecurityState securityState, bool isStall, uint16_t stag,
                         AccessType accessType, bool isStage2, uint64_t ipaValue) {
    // §7.2.1 / CT-33: When CR0.EVENTQEN=0, events must not be recorded.
    // NEW-9 fix: §3.5.3 — the EVENTQEN gate applies to ALL events including stall
    // events.  The previous exception for isStall was incorrect; the event queue is
    // not writable when disabled regardless of the event kind.
    // BUG-CPP-NEW-1 fix: use load() for the atomic cr0_.
    if ((cr0_.load(std::memory_order_acquire) & CR0_EVENTQEN) == 0u) {
        return;
    }

    // BUG-1 fix: snapshot mev flag BEFORE acquiring queueMutex.
    // Lock-order invariant: stripe_lock → queueMutex (stripe lock must come first).
    // The original code acquired queueMutex first, then a stripe lock inside the
    // MEV block — an ABBA deadlock with processCommandQueue() which holds queueMutex
    // and then acquires stripe locks on the CFGI_STE path.
    // Fix: acquire stripe lock, snapshot mev, release stripe lock; then acquire
    // queueMutex and use the snapshotted mev value.  If the stream was removed
    // concurrently (find returns end()), treat as mev=false (allow the event).
    bool mevEnabled = false;
    {
        size_t stripe = getStreamStripe(streamID);
        std::lock_guard<std::mutex> slock(streamLockStripes[stripe]);
        auto streamIt = streamMap.find(streamID);
        if (streamIt != streamMap.end() && streamIt->second) {
            mevEnabled = streamIt->second->getStreamConfiguration().mev;
        }
    }
    // Bug-5 note (TOCTOU — benign race, documented as acceptable):
    // A narrow TOCTOU window exists between releasing the stripe lock above and
    // acquiring queueMutex below.  A concurrent reconfigureStream() in that window
    // could flip the mev flag, causing one spurious duplicate event to be emitted
    // (mev flipped false→true) or one event to be wrongly suppressed (mev flipped
    // true→false).  In this software model this is a benign race: the impact is at
    // most one missed deduplication per reconfiguration event.  A future hardening
    // pass could eliminate the race by making mev an std::atomic<bool> member of
    // StreamContext, readable without a lock.

    // ARM SMMU v3 spec: Generate event for event queue processing.
    // BUG-03 fix: protect eventQueue with queueMutex. Uses recursive_mutex so
    // that callers already holding queueMutex (e.g. processCommandQueue) can
    // safely call this without deadlocking.
    //
    // LOCK-ORDER INVARIANT (Bug-2 documentation): The canonical lock order in
    // this SMMU is:
    //   stripe_lock(s) → queueMutex
    //
    // queueMutex is a std::recursive_mutex specifically because
    // processCommandQueue() holds queueMutex and then invokes code that calls
    // generateEvent() (e.g. CMD_TLBI → C_BAD_STE path), creating a re-entrant
    // call.  The recursive_mutex allows this safely.
    //
    // IMPORTANT: If queueMutex is ever changed to a non-recursive mutex, all
    // call paths that hold queueMutex and call generateEvent() (directly or
    // indirectly) must be refactored to call an internal generateEventLocked()
    // that skips the mutex acquisition.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);

    // CONF-GAP-14: STE.MEV event merging — suppress duplicate events when enabled.
    // When the stream has MEV=true, skip inserting this event if an identical
    // event (same type + streamID) already exists in the event queue.
    // MEV merging applies only within the event queue (not across queue drains).
    // mevEnabled was snapshotted above under the stripe lock (before queueMutex was
    // acquired) to preserve the stripe_lock → queueMutex lock order.
    if (mevEnabled) {
        for (const auto& existing : eventQueue) {
            if (existing.type == type && existing.streamID == streamID) {
                return; // suppress duplicate
            }
        }
    }

    // BUG-ANALYSIS-5 fix: Drain any previously-pending stall events into the
    // main queue first (FIFO order) before inserting the new event, provided
    // space is available.  This ensures stall events are delivered in order once
    // the queue drains, per ARM §7.4.
    while (!stallPending_.empty() && eventQueue.size() < maxEventQueueSize) {
        eventQueue.push_back(stallPending_.front());
        stallPending_.pop_front();
        eventqProd.store(advanceQueueIndex(eventqProd.load(std::memory_order_relaxed), eventqLog2Size), std::memory_order_release);
    }

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
        // BUG-ANALYSIS-5 fix: Stall event and queue is full — redirect to
        // stallPending_ instead of growing eventQueue beyond maxEventQueueSize.
        // ARM §7.4: stall fault records must not be discarded; they are reported
        // when the queue is next writable.  Stall-pending events do NOT trigger
        // OVFLG (stall faults never cause an overflow condition per §7.4).
        // Build the EventEntry and park it in the pending buffer.
        EventEntry pendingEvent;
        pendingEvent.type = type;
        pendingEvent.streamID = streamID;
        pendingEvent.pasid = pasid;
        pendingEvent.address = address;
        pendingEvent.securityState = securityState;
        pendingEvent.timestamp = getCurrentTimestamp();
        pendingEvent.stall = isStall;
        pendingEvent.stag = stag;
        pendingEvent.errorCode = 0;
        // §7.3 wire-format fields — must match the normal path derivations.
        // GAP-N fix: §7.3.9 — C_BAD_SUBSTREAMID SSV is always 1 (no SSV qualifier).
        if (type == EventType::C_BAD_SUBSTREAMID) {
            pendingEvent.ssv = true;
        } else {
            pendingEvent.ssv = (pasid != 0);
        }
        switch (type) {
            case EventType::C_BAD_STREAMID:
            case EventType::C_BAD_STE:
            case EventType::C_BAD_SUBSTREAMID:
            case EventType::C_BAD_CD:
            case EventType::F_CFG_CONFLICT:
                pendingEvent.eventClass = 0u;
                break;
            case EventType::F_TRANSLATION:
            case EventType::F_ADDR_SIZE:
            case EventType::F_PERMISSION:
            case EventType::F_ACCESS:
                // §7.3 NEW-1 encoding: CLASS=0b10 (IN) — fault on the input address.
                pendingEvent.eventClass = 2u;
                break;
            case EventType::F_UUT:
            case EventType::F_TRANSL_FORBIDDEN:
            case EventType::F_BAD_ATS_TREQ:
                // GAP-R05: §7.3.2/§7.3.7/§7.3.8 — CLASS field positions are RES0 for these
                // event types; their wire formats have no CLASS encoding.  Must be 0.
                pendingEvent.eventClass = 0u;
                break;
            default:
                pendingEvent.eventClass = 0u;
                break;
        }
        // RnW-GAP fix: ARM §7.3 — RnW=1 means Read (instruction or data),
        // RnW=0 means Write.  Previous code had this inverted.
        switch (accessType) {
            case AccessType::Write:
                // RnW=0 for writes (ARM §7.3).
                pendingEvent.rnw = false;
                pendingEvent.ind = false;
                pendingEvent.pnu = false;
                break;
            case AccessType::Execute:
                // Execute is a read (instruction fetch); RnW=1.
                pendingEvent.rnw = true;
                pendingEvent.ind = true;
                pendingEvent.pnu = false;
                break;
            case AccessType::ReadWrite:
                // Atomic RMW is write-class per ARM §3.24; RnW=0.
                pendingEvent.rnw = false;
                pendingEvent.ind = false;
                pendingEvent.pnu = false;
                break;
            case AccessType::ReadExecute:
                // Read component present, no write; RnW=1.  Has execute component: ind=1.
                pendingEvent.rnw = true;
                pendingEvent.ind = true;
                pendingEvent.pnu = false;
                break;
            case AccessType::ReadPrivileged:
                // Read, privileged; RnW=1.
                pendingEvent.rnw = true;
                pendingEvent.ind = false;
                pendingEvent.pnu = true;
                break;
            case AccessType::WritePrivileged:
                // Write, privileged; RnW=0.
                pendingEvent.rnw = false;
                pendingEvent.ind = false;
                pendingEvent.pnu = true;
                break;
            case AccessType::ExecutePrivileged:
                // Execute (instruction fetch), privileged; RnW=1.
                pendingEvent.rnw = true;
                pendingEvent.ind = true;
                pendingEvent.pnu = true;
                break;
            case AccessType::ReadWritePrivileged:
                // Atomic RMW, privileged; write-class; RnW=0.
                pendingEvent.rnw = false;
                pendingEvent.ind = false;
                pendingEvent.pnu = true;
                break;
            case AccessType::ReadExecutePrivileged:
                // Bug B new type: read+execute, privileged; RnW=1.
                pendingEvent.rnw = true;
                pendingEvent.ind = true;
                pendingEvent.pnu = true;
                break;
            case AccessType::Read:
            default:
                // Read (default); RnW=1.
                pendingEvent.rnw = true;
                pendingEvent.ind = false;
                pendingEvent.pnu = false;
                break;
        }
        pendingEvent.s2    = isStage2;
        pendingEvent.ipa   = isStage2 ? ipaValue : 0u;
        // §7.3: NSIPA=1 when S2=1 and the IPA is in Non-Secure PA space.
        // A stream is Non-Secure when securityState == SecurityState::NonSecure.
        pendingEvent.nsipa = (isStage2 && securityState == SecurityState::NonSecure);
        stallPending_.push_back(pendingEvent);
        return;
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

    // CONF-GAP-20: §7.3 event record wire format fields.
    // Populate access-type-derived fields from the AccessType parameter.
    // Note: accessType is not a parameter of generateEvent(); it is encoded in
    // the fault record / event type at the call site.  We derive the fields
    // here from the event type and PASID.
    //
    // RnW: true for writes (Write, WritePrivileged).
    // InD: true for instruction fetches (Execute, ExecutePrivileged).
    // PnU: true for privileged access types (*Privileged variants).
    // SSV: true when PASID != 0 (SubstreamID is valid).
    // eventClass: 2 (IN) for input-address faults (F_TRANSLATION/F_ADDR_SIZE/F_PERMISSION/F_ACCESS),
    //             0 for config faults (C_*) — CLASS field is undefined for configuration events.
    //
    // The AccessType is not available at this level; generateEvent() is called
    // from many call sites.  We populate what we can from the event type and pasid.
    // Fields that require the AccessType are populated by call sites that have it
    // (translate-path code sets rnw/ind/pnu before calling generateEvent via the
    // stallPending path; here we set reasonable defaults for generic events).
    // GAP-N fix: ARM IHI0070G.b §7.3.9 — for C_BAD_SUBSTREAMID "SubstreamID is
    // always valid (there is no SSV qualifier)".  SSV must be set to 1 regardless
    // of the PASID value.  All other event types follow the standard rule.
    if (type == EventType::C_BAD_SUBSTREAMID) {
        event.ssv = true;
    } else {
        event.ssv = (pasid != 0);
    }

    // GAP NEW-1 fix: ARM IHI0070G.b §7.3 CLASS field (2-bit) encoding.
    // The CLASS field is defined ONLY for F_* translation events:
    //   0b00 (0) = CD   — fault during CD fetch
    //   0b01 (1) = TTD  — fault during translation table descriptor fetch
    //   0b10 (2) = IN   — fault on the input address itself (SW model default for F_* data faults)
    //   0b11 (3) = Reserved
    // C_* (configuration) events do NOT have a CLASS field — it must be left as 0.
    // Previously C_* events erroneously set eventClass=1.
    switch (type) {
        case EventType::C_BAD_STREAMID:
        case EventType::C_BAD_STE:
        case EventType::C_BAD_SUBSTREAMID:
        case EventType::C_BAD_CD:
        case EventType::F_CFG_CONFLICT:
            // §7.3: CLASS field not defined for configuration events; leave as 0.
            event.eventClass = 0u;
            break;
        case EventType::F_TRANSLATION:
        case EventType::F_ADDR_SIZE:
        case EventType::F_PERMISSION:
        case EventType::F_ACCESS:
            // §7.3 NEW-1 encoding: CLASS=0b10 (IN) — fault is on the input address itself.
            event.eventClass = 2u;
            break;
        case EventType::F_UUT:
        case EventType::F_TRANSL_FORBIDDEN:
        case EventType::F_BAD_ATS_TREQ:
            // GAP-R05: §7.3.2/§7.3.7/§7.3.8 — CLASS field positions are RES0 for these
            // event types; their wire formats have no CLASS encoding.  Must be 0.
            event.eventClass = 0u;
            break;
        default:
            event.eventClass = 0u;
            break;
    }

    // CONF-GAP-20 + RnW-GAP fix: populate rnw/ind/pnu from the accessType parameter (§7.3).
    // ARM §7.3: RnW=1 means Read (instruction or data read), RnW=0 means Write.
    switch (accessType) {
        case AccessType::Write:
            // RnW=0 for writes (ARM §7.3).
            event.rnw = false;
            event.ind = false;
            event.pnu = false;
            break;
        case AccessType::Execute:
            // Execute is a read (instruction fetch); RnW=1.
            event.rnw = true;
            event.ind = true;
            event.pnu = false;
            break;
        case AccessType::ReadWrite:
            // Atomic RMW is write-class per ARM §3.24; RnW=0.
            event.rnw = false;
            event.ind = false;
            event.pnu = false;
            break;
        case AccessType::ReadExecute:
            // Read+execute: has read component (no write); RnW=1.  Has execute: ind=1.
            event.rnw = true;
            event.ind = true;
            event.pnu = false;
            break;
        case AccessType::ReadPrivileged:
            // Read, privileged; RnW=1.
            event.rnw = true;
            event.ind = false;
            event.pnu = true;
            break;
        case AccessType::WritePrivileged:
            // Write, privileged; RnW=0.
            event.rnw = false;
            event.ind = false;
            event.pnu = true;
            break;
        case AccessType::ExecutePrivileged:
            // Execute (instruction fetch), privileged; RnW=1.
            event.rnw = true;
            event.ind = true;
            event.pnu = true;
            break;
        case AccessType::ReadWritePrivileged:
            // Atomic RMW, privileged; write-class; RnW=0.
            event.rnw = false;
            event.ind = false;
            event.pnu = true;
            break;
        case AccessType::ReadExecutePrivileged:
            // Bug B new type: read+execute, privileged; RnW=1.
            event.rnw = true;
            event.ind = true;
            event.pnu = true;
            break;
        case AccessType::Read:
        default:
            // Read (default); RnW=1.
            event.rnw = true;
            event.ind = false;
            event.pnu = false;
            break;
    }
    // GAP NEW-2 fix: ARM IHI0070G.b §7.3.13 — S2 and IPA fields for two-stage faults.
    // When the fault occurred during stage-2 translation, S2=1 and IPA carries
    // the intermediate physical address (stage-1 output) that was looked up in stage-2.
    event.s2    = isStage2;
    event.ipa   = isStage2 ? ipaValue : 0u;
    // §7.3: NSIPA=1 when S2=1 and the IPA is in Non-Secure PA space.
    // A stream is Non-Secure when securityState == SecurityState::NonSecure.
    event.nsipa = (isStage2 && securityState == SecurityState::NonSecure);

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
    generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, actualState,
                  false, 0, accessType);
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
    // ARM IHI0070G.b §7.3.13–7.3.15:
    //   RnW (bit [3]) = 1 for Read accesses (Read-not-Write), 0 for Write accesses.
    //   InD (bit [2]) = 1 for instruction fetches, BUT ONLY when RnW == 1 (no write component).
    //   "InD == 0 when RnW == 0" is a spec-required constraint.
    // NEW-1 fix: corrected — prior code wrongly set RnW=1 for writes (inverted).
    bool writeAccess = (accessType == AccessType::Write             ||
                        accessType == AccessType::ReadWrite         ||
                        accessType == AccessType::WritePrivileged   ||
                        accessType == AccessType::ReadWritePrivileged);
    // InD is set for execute-class accesses only when there is no write component.
    // Per spec: InD must be 0 when RnW (writeAccess) is 1.
    // Bug NEW-1 fix: also set InD for ReadExecute and ReadExecutePrivileged.
    bool instructionFetch = (!writeAccess) &&
                            (accessType == AccessType::Execute ||
                             accessType == AccessType::ExecutePrivileged ||
                             accessType == AccessType::ReadExecute ||
                             accessType == AccessType::ReadExecutePrivileged);
    
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

    // Bit [3]: RnW — read (1) or write (0).
    // ARM §7.3: RnW=1 means Read (Read-not-Write), RnW=0 means Write.
    // NEW-1 fix: inverted from the buggy "if (writeAccess)" condition.
    if (!writeAccess) {
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
    // §3.6 / §13.1 (NEW-GAP-C fix): Derive privilege from caller-supplied AxPROT[1],
    // encoded as *Privileged AccessType variants.  Root→EL3 and Realm→EL2 remain
    // fixed by security state per the spec.  For Secure/NonSecure streams, inspect
    // the accessType: *Privileged variants indicate EL1 (privileged), unprivileged
    // variants indicate EL0 (ARM §13.1.3 default for unknown privilege is Unprivileged).
    switch (securityState) {
        case SecurityState::Root:
            return PrivilegeLevel::EL3;
        case SecurityState::Realm:
            return PrivilegeLevel::EL2;
        case SecurityState::Secure:
        case SecurityState::NonSecure:
        default:
            // AxPROT[1]=1 (privileged) is encoded as *Privileged AccessType variants.
            switch (accessType) {
                case AccessType::ReadPrivileged:
                case AccessType::WritePrivileged:
                case AccessType::ExecutePrivileged:
                case AccessType::ReadWritePrivileged:
                // Bug B: ReadExecutePrivileged is also a privileged variant.
                case AccessType::ReadExecutePrivileged:
                    return PrivilegeLevel::EL1;
                default:
                    return PrivilegeLevel::EL0;
            }
    }
}

AccessClassification SMMU::classifyAccess(AccessType accessType) const {
    // Classify access type for syndrome generation.
    // ARM IHI0070G.b §7.3: the InD field in fault syndrome is 1-bit
    // (0=Data, 1=Instruction). There is no "Unknown" encoding.
    switch (accessType) {
        case AccessType::Execute:
        case AccessType::ExecutePrivileged:
            return AccessClassification::InstructionFetch;
        // Bug-6: ReadExecute has an execute component — classify as InstructionFetch
        // so that ind=true is consistent with syndrome derivation.
        case AccessType::ReadExecute:
        // Bug B: ReadExecutePrivileged also has execute component.
        case AccessType::ReadExecutePrivileged:
            return AccessClassification::InstructionFetch;
        case AccessType::Read:
        case AccessType::Write:
        case AccessType::ReadWrite:
        case AccessType::ReadPrivileged:
        case AccessType::WritePrivileged:
        case AccessType::ReadWritePrivileged:
            return AccessClassification::DataAccess;
        default:
            return AccessClassification::DataAccess;
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