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

// §3.15 / §13.1.7 Rule 1: OSH is required for all Device memory types and Normal
// Non-Cacheable.  Device encodes as 0x0–0x3 (and reserved aliases 0x4, 0x8, 0xC);
// Normal-iNC-oNC encodes as 0x5.  Normal WB/WA/WT (inner or outer cacheable) does not.
static inline bool oshRequired(uint8_t memAttr) {
    return (memAttr <= 0x5u) || (memAttr == 0x8u) || (memAttr == 0xCu);
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
      // BUG-NEW-11: STALL_MODEL defaults to 0b00 (stall+terminate both supported).
      stallModel_(0),
      // BUG-NEW-16: IDR0.Hyp defaults to true (hypervisor extension supported).
      hypSupported_(true),
      // BUG-NEW-39: IDR0.S2P defaults to true (stage-2 translation supported).
      s2pSupported_(true),
      // BUG-AUDIT-NEW-03: IDR0.S1P defaults to true (stage-1 translation supported).
      s1pSupported_(true),
      // BUG-NEW-G: IDR0.PRI defaults to true (page request interface supported).
      priSupported_(true),
      // BUG-AUDIT-01: IDR0.NS1ATS defaults to false (NS1ATS not advertised by default).
      ns1atsSupported_(false),
      // BUG-AUDIT-55: IDR0.SEV defaults to false (WFE/SEV mechanism not implemented).
      sevSupported_(false),
      // GAP-NEW-E: STATUSR/IRQ_CTRL/CTRLACK registers initialize to 0.
      statusr_(0),
      irqCtrl_(0),
      irqCtrlAck_(0),
      // BUG-AUDIT-8 fix: atomic event timestamp counter, initialized to 0.
      eventTimestampCounter_(0) {
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
      // BUG-NEW-11: STALL_MODEL defaults to 0b00 (stall+terminate both supported).
      stallModel_(0),
      // BUG-NEW-16: IDR0.Hyp defaults to true (hypervisor extension supported).
      hypSupported_(true),
      // BUG-NEW-39: IDR0.S2P defaults to true (stage-2 translation supported).
      s2pSupported_(true),
      // BUG-AUDIT-NEW-03: IDR0.S1P defaults to true (stage-1 translation supported).
      s1pSupported_(true),
      // BUG-NEW-G: IDR0.PRI defaults to true (page request interface supported).
      priSupported_(true),
      // BUG-AUDIT-01: IDR0.NS1ATS defaults to false (NS1ATS not advertised by default).
      ns1atsSupported_(false),
      // BUG-AUDIT-55: IDR0.SEV defaults to false (WFE/SEV mechanism not implemented).
      sevSupported_(false),
      // GAP-NEW-E: STATUSR/IRQ_CTRL/CTRLACK registers initialize to 0.
      statusr_(0),
      irqCtrl_(0),
      irqCtrlAck_(0),
      // BUG-AUDIT-8 fix: atomic event timestamp counter, initialized to 0.
      eventTimestampCounter_(0) {
    // Validate the provided configuration
    if (!config.isValid()) {
        // Fall back to default configuration if invalid
        configuration = SMMUConfiguration::createDefault();
        // BUG-CPP-1 fix: recompute the six queue-sizing members from the fallback
        // configuration so they remain consistent with this->configuration.
        // The initializer list already set them from the invalid `config`; they must
        // now reflect the default queue sizes.
        maxEventQueueSize   = configuration.getQueueConfiguration().eventQueueSize;
        maxCommandQueueSize = configuration.getQueueConfiguration().commandQueueSize;
        maxPRIQueueSize     = configuration.getQueueConfiguration().priQueueSize;
        cmdqLog2Size  = computeLog2Size(configuration.getQueueConfiguration().commandQueueSize);
        eventqLog2Size = computeLog2Size(configuration.getQueueConfiguration().eventQueueSize);
        priqLog2Size  = computeLog2Size(configuration.getQueueConfiguration().priQueueSize);
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
                                  SecurityState securityState, TransactionType transactionType,
                                  bool ssv) {
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
            // BUG-AUDIT-156-CPP fix: §3.9.1.2 table row for SMMUEN=0 is unconditional —
            // F_BAD_ATS_TREQ fires regardless of CR2.REC_CFG_ATS. REC_CFG_ATS only gates
            // config-error events (§3.9.1.2 lines 2157-2158), not admission-failure events.
            generateEvent(EventType::F_BAD_ATS_TREQ, streamID, pasid, iova,
                          securityState, false, 0, accessType, false, 0);
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
            // BUG-13.2-CPP / BUG-AUDIT-164-CPP fix: §3.15/§13.1.7 Rule 1 — all Device and
            // Normal Non-Cacheable memory types require OSH regardless of GBPA.SHCfg.
            if (oshRequired(td.memType)) {
                td.shareability = 2u;
            }
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
        // SPEC-20 fix (§7.3.3 vs §7.3.5): The StreamID passed the LOG2SIZE bounds
        // check above, so it is WITHIN the configured table range.  A missing entry
        // in streamMap is equivalent to STE.V=0 — the STE is present in range but
        // not valid.  Per ARM IHI0070G.b §7.3.5 this must generate C_BAD_STE (0x04),
        // NOT C_BAD_STREAMID (0x02).
        //
        // §7.3.3 C_BAD_STREAMID only fires when the StreamID is OUTSIDE the table
        // range (>= 2^LOG2SIZE), which is handled by the bounds check above.
        //
        // §6.3.12 RECINVSID gates C_BAD_STREAMID recording only; C_BAD_STE is always
        // recorded regardless of RECINVSID.
        FaultRecord fault;
        fault.streamID = streamID;
        fault.pasid = pasid;
        fault.address = iova;
        // BUG-CPP-1 fix: §7.3.5 C_BAD_STE fires when StreamID is in-range but
        // STE.V=0 (no configured stream).  §7.3.3 C_BAD_STREAMID is reserved for
        // StreamIDs that are OUTSIDE the table range (handled earlier in this
        // function).  Use FaultType::BadSTE so the FaultRecord is consistent with
        // the EventType::C_BAD_STE event generated on the next line.
        fault.faultType = FaultType::BadSTE;
        fault.accessType = accessType;
        fault.securityState = securityState;
        fault.timestamp = currentTime;
        recordFault(fault);
        generateEvent(EventType::C_BAD_STE, streamID, pasid, iova, securityState);
        return makeTranslationError(SMMUError::StreamNotConfigured);
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
    // BUG-NEW-CPP-D fix: §7.3.7 — Config=0b000 abort-mode streams must not serve
    // TLB hits. Skip the fast path entirely when no stage or bypass is enabled so
    // stale cache entries never short-circuit the slow path's terminate logic.
    if (cachingEnabled.load(std::memory_order_acquire) && tlbCache &&
        (streamCfgSnapshot.stage1Enabled || streamCfgSnapshot.stage2Enabled || streamCfgSnapshot.bypassEnabled)) {
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
        // BUG-13.1.4-CPP-A fix: §13.1.4 — ATOS (GatosTranslation) must NOT override access type.
        if (transactionType != TransactionType::GatosTranslation) {
            if (streamCfgForTlb.instCfg == 3u) {
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
                    // BUG-13.1.7-CPP fix: ARM §13.1.7 Rule 1 — Device and Non-Cacheable
                    // memory types must always use Outer Shareable (OSH=2) regardless of
                    // STE.SHCfg.  When mtCfg=true, the STE override is authoritative;
                    // when mtCfg=false, the page-level attribute (cached in TLBEntry) is used.
                    {
                        // BUG-AUDIT-164-CPP fix: §3.15/§13.1.7 Rule 1 applies to all
                        // Device and Normal-NC types, not just Device-nGnRnE (0x00).
                        bool needsOsh = streamCfgForTlb.mtCfg
                            ? oshRequired(streamCfgForTlb.memAttr)
                            : (entry.pageAttr == 0x00u);
                        if (needsOsh) {
                            data.shareability = 2u;  // OSH
                        }
                    }
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
        // BUG-AUDIT-158-CPP fix: §3.9.1.2 footnote — effective EATS==0 when
        // EATS==0b1x (2 or 3) AND CR0_ATSCHK==0. Only eats==1 (full ATS) is
        // unconditionally supported; eats==2/3 require ATSCHK==1.
        const bool atschkSet = (cr0_.load(std::memory_order_acquire) & CR0_ATSCHK) != 0u;
        bool atsSupported = streamCfgSnapshot.translationEnabled
                            && !streamCfgSnapshot.bypassEnabled
                            && (streamCfgSnapshot.eats == 1
                                || (streamCfgSnapshot.eats >= 2 && atschkSet));
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
        // NEW-BUG-3 fix: ARM §3.9.1.3 — bypass streams cannot be re-validated by
        // ATSCHK because there is no stage-1 or stage-2 table to check against.
        // For STE.Config==Bypass (bypassEnabled=true), the SMMU must unconditionally
        // return F_TRANSL_FORBIDDEN regardless of any translation result.
        if (streamCfgSnapshot.bypassEnabled) {
            generateEvent(EventType::F_TRANSL_FORBIDDEN, streamID, pasid, iova,
                          securityState, false, 0, accessType, false, 0);
            return makeTranslationError(SMMUError::PageNotMapped);
        }
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

    // BUG-AUDIT-109/112 fix: §5.2 STE.S1CDMax line 6691 — "If this field is 0, then any
    // transaction using this STE that is presented with SSV=1 is terminated with an abort,
    // and a C_BAD_SUBSTREAMID event is generated."
    // This covers PASID==0 (BUG-AUDIT-112) and PASID!=0 (BUG-AUDIT-109) equally.
    // Placed here (in translate(), where ssv is in scope) because performTwoStageTranslation()
    // does not receive ssv.
    if (ssv && streamCfgSnapshot.stage1Enabled && streamCfgSnapshot.s1cdMax == 0u) {
        FaultRecord cBadSubFault;
        cBadSubFault.streamID     = streamID;
        cBadSubFault.pasid        = pasid;
        cBadSubFault.address      = iova;
        cBadSubFault.faultType    = FaultType::BadSubstreamId;
        cBadSubFault.accessType   = accessType;
        cBadSubFault.securityState = securityState;
        cBadSubFault.timestamp    = currentTime;
        recordFault(cBadSubFault);
        generateEvent(EventType::C_BAD_SUBSTREAMID, streamID, pasid, iova, securityState);
        return makeTranslationError(SMMUError::InvalidPASID);
    }

    // BUG-AUDIT-111 fix: ARM §3.3.2 line 1354 — "Transactions provided with a SubstreamID
    // are terminated when stage 1 translation is not enabled."
    // Applies to bypass streams (translationEnabled=false) and stage-2-only streams
    // (stage1Enabled=false, stage2Enabled=true).  When SSV=1 on any stream that has no
    // stage-1 context, abort with C_BAD_SUBSTREAMID.
    // Note: the stage-1-enabled + s1cdMax==0 case is already handled above.
    if (ssv && !streamCfgSnapshot.stage1Enabled) {
        FaultRecord cBadSubFault;
        cBadSubFault.streamID     = streamID;
        cBadSubFault.pasid        = pasid;
        cBadSubFault.address      = iova;
        cBadSubFault.faultType    = FaultType::BadSubstreamId;
        cBadSubFault.accessType   = accessType;
        cBadSubFault.securityState = securityState;
        cBadSubFault.timestamp    = currentTime;
        recordFault(cBadSubFault);
        generateEvent(EventType::C_BAD_SUBSTREAMID, streamID, pasid, iova, securityState);
        return makeTranslationError(SMMUError::InvalidPASID);
    }

    // BUG-E fix: §3.9 / §7.3.7 — S1DSS==0b10 + SSV==1 + PASID==0 → F_STREAM_DISABLED.
    // "When STE.S1DSS==0b10, transactions that arrive with SubstreamID 0 [and SSV=1]
    // are aborted and an event recorded." (ARM §3.9 table note on S1DSS==0b10.)
    // This check is placed in the outer translate() so ssv is in scope.
    //
    // NEW-FINDING-1 fix (§5.2 STE.S1DSS line 6725):
    // "For ATS Translation Requests, if the cases described in 0b00 and 0b10 lead to
    //  termination, the Translation Request is terminated with a CA and no event is
    //  recorded."
    // ATS Translation Requests must abort (CA) but must NOT record F_STREAM_DISABLED.
    if (ssv && pasid == 0 &&
        streamCfgSnapshot.stage1Enabled &&
        streamCfgSnapshot.s1cdMax > 0 &&
        streamCfgSnapshot.s1dss == 0x02u) {
        // Only record the fault/event for non-ATS transactions (§5.2 STE.S1DSS line 6725).
        if (transactionType != TransactionType::AtsTranslationRequest) {
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
        }
        return makeTranslationError(SMMUError::SubstreamDisabled);
    }

    // NEW-A fix: §5.2 STE.S1DSS line 6725 — For ATS Translation Requests, S1DSS==0b00
    // termination must NOT record an event.  "The Translation Request is terminated with
    // a CA and no event is recorded." (ARM §5.2).
    // This pre-check intercepts ATS TR before performTwoStageTranslation() (which lacks
    // transactionType) so the ATS abort is silent.  Ordinary transactions fall through to
    // performTwoStageTranslation() where the S1DSS==0b00 block records F_STREAM_DISABLED.
    if (transactionType == TransactionType::AtsTranslationRequest &&
        pasid == 0 &&
        streamCfgSnapshot.stage1Enabled &&
        streamCfgSnapshot.s1cdMax > 0 &&
        (streamCfgSnapshot.s1dss == 0x00u || streamCfgSnapshot.s1dss == 0x03u)) {
        // BUG-AUDIT-110: S1DSS==0b11 is Reserved and behaves as 0b00 per §5.2 line 6716.
        // For ATS TRs, both 0b00 and 0b11 abort silently (no event per §5.2 line 6725).
        return makeTranslationError(SMMUError::SubstreamDisabled);
    }

    // Task 5.2: Enhanced two-stage translation with comprehensive error handling
    TranslationResult result = performTwoStageTranslation(streamID, pasid, iova, accessType, securityState, streamContext, currentTime, transactionType);

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
                // Stage-1 only (ARM §3.17):
                // - Secure stage-1-only: VMID=0 (no stage-2, no VMID tagging)
                // - NS-EL1 stage-1-only: retain STE.S2VMID so VMID-targeted TLBIs
                //   (CMD_TLBI_S12_VMALL etc.) correctly match these entries.
                if (streamCfg.securityState == SecurityState::Secure) {
                    entryVmid = 0;
                }
                // else: entryVmid already set to streamCfg.vmid (STE.S2VMID) above
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
            // BUG-AUDIT-152-CPP fix: §3.3.3 lines 1479 and 1491 — NS-EL2 (without E2H)
            // and EL3 StreamWorlds have no ASID tag. Zero entryAsid for these regimes
            // regardless of whether stage 1 or stage 2 is enabled.
            // BUG-AUDIT-166-CPP fix: §3.17.1 — EL2/EL3 streams are not in the EL1&0
            // translation regime and carry no VMID tag. Zero entryVmid for these regimes
            // regardless of whether stage 2 is enabled (entryVmid=streamCfg.vmid above
            // is only correct for EL1_EL0 two-stage streams).
            if (streamCfg.strw == StreamWorld::EL2 || streamCfg.strw == StreamWorld::EL3) {
                entryAsid = 0;
                entryVmid = 0;
            }
            cacheTranslationResult(streamID, pasid, iova, result, currentTime, entryAsid, entryVmid, streamCfg.strw);
        }
    } else if (result.isError()) {
        // BUG-CPP-5 fix: §7.3 — compute the effective (post-STE-override) access type
        // from the streamCfgSnapshot captured before the stripe lock was released.
        // This is the same STRW→INSTCFG→PRIVCFG chain used in performTwoStageTranslation().
        // Passing it to handleTranslationFailure() eliminates the TOCTOU race where a
        // concurrent configureStream() could change the override between translation
        // completion and the config re-fetch inside handleTranslationFailure().
        AccessType translateEffectiveAccessType = accessType;
        if (!streamCfgSnapshot.stage2Enabled &&
            (streamCfgSnapshot.strw == StreamWorld::EL2 || streamCfgSnapshot.strw == StreamWorld::EL3)) {
            switch (translateEffectiveAccessType) {
                case AccessType::Read:        translateEffectiveAccessType = AccessType::ReadPrivileged;          break;
                case AccessType::Write:       translateEffectiveAccessType = AccessType::WritePrivileged;         break;
                case AccessType::Execute:     translateEffectiveAccessType = AccessType::ExecutePrivileged;       break;
                case AccessType::ReadWrite:   translateEffectiveAccessType = AccessType::ReadWritePrivileged;     break;
                case AccessType::ReadExecute: translateEffectiveAccessType = AccessType::ReadExecutePrivileged;   break;
                default: break;
            }
        }
        if (streamCfgSnapshot.instCfg == 3u) {
            if (translateEffectiveAccessType == AccessType::Read)
                translateEffectiveAccessType = AccessType::Execute;
            else if (translateEffectiveAccessType == AccessType::ReadPrivileged)
                translateEffectiveAccessType = AccessType::ExecutePrivileged;
        } else if (streamCfgSnapshot.instCfg == 2u) {
            if (translateEffectiveAccessType == AccessType::Execute)
                translateEffectiveAccessType = AccessType::Read;
            else if (translateEffectiveAccessType == AccessType::ExecutePrivileged)
                translateEffectiveAccessType = AccessType::ReadPrivileged;
            else if (translateEffectiveAccessType == AccessType::ReadExecute)
                translateEffectiveAccessType = AccessType::Read;
            else if (translateEffectiveAccessType == AccessType::ReadExecutePrivileged)
                translateEffectiveAccessType = AccessType::ReadPrivileged;
        }
        if (streamCfgSnapshot.privCfg == 2u) {
            switch (translateEffectiveAccessType) {
                case AccessType::ReadPrivileged:          translateEffectiveAccessType = AccessType::Read;        break;
                case AccessType::WritePrivileged:         translateEffectiveAccessType = AccessType::Write;       break;
                case AccessType::ExecutePrivileged:       translateEffectiveAccessType = AccessType::Execute;     break;
                case AccessType::ReadWritePrivileged:     translateEffectiveAccessType = AccessType::ReadWrite;   break;
                case AccessType::ReadExecutePrivileged:   translateEffectiveAccessType = AccessType::ReadExecute; break;
                default: break;
            }
        } else if (streamCfgSnapshot.privCfg == 3u) {
            switch (translateEffectiveAccessType) {
                case AccessType::Read:        translateEffectiveAccessType = AccessType::ReadPrivileged;          break;
                case AccessType::Write:       translateEffectiveAccessType = AccessType::WritePrivileged;         break;
                case AccessType::Execute:     translateEffectiveAccessType = AccessType::ExecutePrivileged;       break;
                case AccessType::ReadWrite:   translateEffectiveAccessType = AccessType::ReadWritePrivileged;     break;
                case AccessType::ReadExecute: translateEffectiveAccessType = AccessType::ReadExecutePrivileged;   break;
                default: break;
            }
        }

        // ARM §3.12.2: Check per-stream stall mode before standard fault handling.
        // If the stream is configured for stall, enqueue a StallRecord and return
        // Stalled instead of the original error — software must issue CMD_RESUME.
        // GAP-NEW-G: STE.S1STALLD=1 forces abort semantics — override stall mode.
        // QA-AUDIT-FIX-1: §5.5 STE.S2S governs stall for ALL stage-2 faults,
        // including faults in the stage-2 walk of two-stage streams (stage-1 + stage-2).
        // The previous !stage1Enabled guard was wrong: it caused two-stage streams to
        // use FaultMode instead of S2S for their stage-2 faults.
        bool isStage2Fault = tl_stage2FaultCtx.isStage2;
        bool inStallMode = isStage2Fault
            ? streamCfgSnapshot.s2s
            : (streamContext->getFaultMode() == FaultMode::Stall &&
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
                handleTranslationFailure(streamID, pasid, iova, accessType, translateEffectiveAccessType,
                                         securityState, result, currentTime, transactionType);
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
            if (streamCfgSnapshot.instCfg == 3u) {
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
        // QA-AUDIT-FIX-1 / BUG-QA-13 fix: §5.5 STE.S2R=0 && STE.S2S=0 → suppress
        // stage-2 fault events.  Now applies to ALL stage-2 faults (isStage2Fault),
        // not just stage-2-only streams.  Configuration faults are always recorded.
        if (isStage2Fault && !streamCfgSnapshot.s2R && !streamCfgSnapshot.s2s
                && !isConfigFault) {
            streamContext = nullptr;
            return result;
        }
        // BUG-NEW-CPP-3 fix: the stripe lock was already released before
        // performTwoStageTranslation(); do NOT call lock.unlock() again.
        // handleTranslationFailure() may call determineContextSecurityState() which
        // acquires the stripe lock — this is safe because we no longer hold it.
        streamContext = nullptr; // Defensive: no further accesses via this pointer
        handleTranslationFailure(streamID, pasid, iova, accessType, translateEffectiveAccessType,
                                 securityState, result, currentTime, transactionType);
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

    // Validation 1: STRW checks per ARM §5.2 SteIllegal() pseudocode.
    // STRW is only meaningful when stage-1 is active and stage-2 is NOT enabled.
    // ARM §5.2 IgnoreSTESTRW(): Config==0b11x (stage-2 enabled) → always ignore STRW.
    // BUG-AUDIT-113 fix: add ||config.stage2Enabled to correctly ignore STRW for Config=0b111.
    {
        bool strwUnused = !config.stage1Enabled || config.stage2Enabled;
        if (!strwUnused) {
            // BUG-CPP-3(a) fix: ARM §5.2 bit[0]=1 pattern ('x1'): STRW encodings
            // 0b01 (EL2) and 0b11 (EL3) are ILLEGAL for NonSecure/Realm streams.
            // The previous check only tested STRW==EL3; EL2 (0b01) was silently
            // accepted.  Now we check bit[0] so both EL2 and EL3 are rejected.
            if (config.securityState == SecurityState::NonSecure &&
                (static_cast<uint8_t>(config.strw) & 0x01u) != 0u) {
                generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
                return makeVoidError(SMMUError::InvalidConfiguration);
            }
            // BUG-CPP-3(b) fix: ARM §5.2 SteIllegal() pseudocode — STRW=EL3 (0b11)
            // is ILLEGAL for Secure streams.  This check was entirely missing before.
            if (config.securityState == SecurityState::Secure &&
                config.strw == StreamWorld::EL3) {
                generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
                return makeVoidError(SMMUError::InvalidConfiguration);
            }
        }
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

    // BUG-QA-12 fix: §5.5 — STALL_MODEL==0b01 (terminate-only) AND STE.S2S==1 → C_BAD_STE.
    // Checked before the stripe lock is acquired for the same reason as the STRW checks above
    // (generateEvent() acquires queueMutex which would deadlock with the stripe lock).
    // QA-AUDIT-FIX-3: guard by stage2Enabled — bypass and stage1-only streams never
    // encounter stage-2 faults, so S2S is irrelevant and must not trigger C_BAD_STE.
    if (config.stage2Enabled && stallModel_.load(std::memory_order_acquire) == 0x01u && config.s2s) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-C1 fix: §5.5 — STALL_MODEL==0b10 (forced-stall) + stage2Enabled + s2s==false → C_BAD_STE.
    // When forced-stall is required, every stage-2 stream must have S2S=1.
    if (config.stage2Enabled && stallModel_.load(std::memory_order_acquire) == 0x02u && !config.s2s) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-C2 fix: §5.5 — STALL_MODEL==0b10 (forced-stall) + stage1Enabled + faultMode==Terminate (CD.S=0) → C_BAD_CD.
    // When forced-stall is required, stage-1 streams must have CD.S=1 (FaultMode::Stall).
    if (config.stage1Enabled && stallModel_.load(std::memory_order_acquire) == 0x02u && config.faultMode != FaultMode::Stall) {
        generateEvent(EventType::C_BAD_CD, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // NEW-B fix: §5.5 CdIllegal() — STALL_MODEL==0b01 (terminate-only) + stage1Enabled + CD.S==1 → C_BAD_CD.
    // When terminate-only mode is set, stall (CD.S==1) on a stage-1 stream is illegal.
    // ARM §5.5 CdIllegal() pseudocode: "if stall_model == '01' && CD.S == '1' then return TRUE".
    if (config.stage1Enabled && stallModel_.load(std::memory_order_acquire) == 0x01u
            && config.faultMode == FaultMode::Stall) {
        generateEvent(EventType::C_BAD_CD, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-C3 fix: §5.5 — STALL_MODEL!=0b00 + s1Stalld==true → C_BAD_STE.
    // When stall is required by the model, S1STALLD=1 (which disables stage-1 stall) is contradictory.
    if (config.stage1Enabled && stallModel_.load(std::memory_order_acquire) != 0x00u && config.s1Stalld) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-36 fix: ARM IHI0070G.b §5.2 SteIllegal() line 8354 —
    // "if STE.S2HD == '1' && SMMU_IDR0.HTTU == '01' then return TRUE"
    // HTTU is hardcoded to 0b01 (access flag only, dirty state NOT supported).
    // S2HD requests hardware dirty-state management which this SMMU does not implement.
    // S2HD is only meaningful when stage-2 is enabled (ignored otherwise).
    if (config.stage2Enabled && config.s2hd) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-40 fix: ARM IHI0070G.b §5.5 CdIllegal() line 9797 —
    // "CD.HD == '1' && SMMU_IDR0.HTTU == '01' → return TRUE (CdIllegal)"
    // HTTU is hardcoded to 0b01 (access flag only, dirty state NOT supported).
    // CD.HD requests hardware dirty-state management which this SMMU does not implement.
    // CD.HD is only meaningful when stage-1 is enabled (CD is irrelevant otherwise).
    if (config.stage1Enabled && config.hd) {
        generateEvent(EventType::C_BAD_CD, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-42 fix: ARM §5.2 SteIllegal() — using_vmsa32 (S2AA64==0) AND TTF[0]==0 → SteIllegal.
    // TTF is hardcoded 0b10 (AArch64-only) so TTF[0]=0 always. Any stage-2 stream with
    // s2aa64==false is therefore SteIllegal → C_BAD_STE.
    if (config.stage2Enabled && !config.s2aa64) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-43 fix: ARM §5.2 SteIllegal() — Config requires S1P/S2P capability.
    // Line ~8220: if STE.Config=='1x1' && SMMU_IDR0.S1P=='0' → SteIllegal → C_BAD_STE.
    // Lines ~8224-8227: if STE.Config=='11x' && SMMU_IDR0.S2P=='0' → SteIllegal → C_BAD_STE.
    if (config.stage1Enabled && !s1pSupported_.load(std::memory_order_acquire)) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    if (config.stage2Enabled && !s2pSupported_.load(std::memory_order_acquire)) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-44 fix: ARM IHI0070G.b §5.2 SteIllegal() —
    // "if UInt(STE.S1CDMax) > UInt(SMMU_IDR1.SSIDSIZE) then return TRUE"
    // SSIDSIZE is hardcoded to 20 (IDR1[10:6]=20 per getIDR1()).
    // When stage-1 is enabled and s1cdMax exceeds 20, the STE is illegal → C_BAD_STE.
    if (config.stage1Enabled && config.s1cdMax > 20u) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-45 fix: ARM IHI0070G.b §5.2 STES2T0SZInvalid() —
    // For STT=0 (4-level), 4KB granule (s2tg=0), IAS=48, the valid S2T0SZ range
    // is [16, 39].  Values outside this range → SteIllegal → C_BAD_STE.
    // Guard is applied only when stage-2 is enabled; s2t0sz is irrelevant otherwise.
    // BUG-AUDIT-57: s2t0sz==0 is architecturally the maximum IPA range (2^64 per §5.4
    // S2T0SZ encoding), used here as a model sentinel meaning "no IPA range restriction."
    // All `if (s2t0sz > 0u)` guards in translation paths preserve this convention.
    // Only non-zero values outside [16, 39] are architecturally invalid.
    if (config.stage2Enabled && config.s2t0sz != 0u &&
            (config.s2t0sz < 16u || config.s2t0sz > 39u)) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-AUDIT-58 fix: ARM IHI0070G.b §5.2 SteIllegal() —
    // "if !using_vmsa32 && !GranuleSupported(s2tg) then return TRUE"
    // IDR5 advertises GRAN4K=1, GRAN16K=0, GRAN64K=0.
    // S2TG encoding: 0b00=4KB (valid), 0b01=64KB (invalid), 0b10=16KB (invalid), 0b11=Reserved.
    // Any non-zero s2tg when stage2 is enabled → C_BAD_STE.
    if (config.stage2Enabled && config.s2tg != 0u) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-NEW-F fix: §5.5 CdIllegal() line 9748 — S1STALLD==1 AND CD.S==1 → C_BAD_CD.
    // The spec pseudocode declares a CD ILLEGAL when STE.S1STALLD==1 AND CD.S==1,
    // regardless of STALL_MODEL value.  The BUG-C3 check above already rejects
    // STALL_MODEL!=0b00 + s1Stalld (→ C_BAD_STE), so the only remaining case that
    // reaches here is STALL_MODEL==0b00 with s1Stalld==true AND faultMode==Stall (CD.S==1).
    if (config.stage1Enabled && config.s1Stalld && config.faultMode == FaultMode::Stall) {
        generateEvent(EventType::C_BAD_CD, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
    }

    // BUG-NEW-J fix: ARM §5.2 SteIllegal() — EATS field validity (lines 8235–8257).
    // EATS==0b10 (split-stage ATS): requires two-stage translation (stage1+stage2),
    //   and S2S must be 0.  Any other combination with EATS==0b10 → C_BAD_STE.
    // EATS==0b01 (stage-2 ATS): illegal when S2S==1 and stage2 is enabled → C_BAD_STE.
    bool twoStage = config.stage1Enabled && config.stage2Enabled;
    if (config.eats == 2u) {
        if (!twoStage || config.s2s) {
            generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
            return makeVoidError(SMMUError::InvalidConfiguration);
        }
        // BUG-AUDIT-01 fix: ARM §5.2 SteIllegal() — EATS==0b10 is also illegal when
        // IDR0.NS1ATS==1 (even for a legal two-stage, S2S=0 configuration).
        if (ns1atsSupported_.load(std::memory_order_acquire)) {
            generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
            return makeVoidError(SMMUError::InvalidConfiguration);
        }
    }
    if (config.eats == 1u && config.s2s && config.stage2Enabled) {
        generateEvent(EventType::C_BAD_STE, streamID, 0, 0, config.securityState);
        return makeVoidError(SMMUError::InvalidConfiguration);
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

// Map a stage-1 page as Device memory type (§13.1.5: Device wins in two-stage attr combining).
VoidResult SMMU::mapPageDevice(StreamID streamID, PASID pasid, IOVA iova, PA pa,
                                const PagePermissions& permissions, SecurityState securityState) {
    size_t stripe = getStreamStripe(streamID);
    std::lock_guard<std::mutex> lock(streamLockStripes[stripe]);
    auto streamIt = streamMap.find(streamID);
    if (streamIt == streamMap.end()) {
        return makeVoidError(SMMUError::StreamNotFound);
    }
    return streamIt->second->mapPageDevice(pasid, iova, pa, permissions, securityState);
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
    // BUG-NEW-11: Reset STALL_MODEL to 0b00 (default: stall+terminate supported).
    stallModel_.store(0u, std::memory_order_release);
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
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime,
                                                  TransactionType transactionType) {
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
        // BUG-QA-11 fix: §5.2/§13.3 — apply STE output attribute overrides to bypass
        // transactions.  Rust correctly calls apply_output_attrs() via
        // StreamContext::translate_bypass(); C++ previously omitted this step.
        data.memType      = config.mtCfg ? config.memAttr : 0u;
        data.shareability = config.shCfg;
        data.allocHint    = config.allocCfg;
        data.instCfg      = config.instCfg;
        data.privCfg      = config.privCfg;
        data.nsCfgOut     = config.nsCfg;
        return TranslationResult(data);
    }

    // BUG-5 fix: ARM IHI0070G.b §7.3.14 — EventEntry.RnW/InD/PnU must reflect the
    // *post-STE-override* effective access type, not the raw incoming accessType.
    // Compute effectiveAccessType here, BEFORE the S1DSS block, so that the
    // S1DSS==0x01 OAS-overflow path uses the correct override-adjusted type when
    // calling generateEvent(). This is the same three-step override chain that was
    // previously computed (as the NEW-02 fix) only after the S1DSS block; it is now
    // moved earlier so all early-exit paths — S1DSS OAS, T0SZ, EPD0 — see it.
    //
    // Step 1: STRW promotion (STRW=EL2/EL3 → promote to Privileged variants).
    //         Per §5.2, STRW is ignored when stage-2 is enabled for Non-Secure streams.
    AccessType effectiveAccessType = accessType;
    if (!config.stage2Enabled &&
        (config.strw == StreamWorld::EL2 || config.strw == StreamWorld::EL3)) {
        switch (accessType) {
            case AccessType::Read:        effectiveAccessType = AccessType::ReadPrivileged;          break;
            case AccessType::Write:       effectiveAccessType = AccessType::WritePrivileged;         break;
            case AccessType::Execute:     effectiveAccessType = AccessType::ExecutePrivileged;       break;
            case AccessType::ReadWrite:   effectiveAccessType = AccessType::ReadWritePrivileged;     break;
            case AccessType::ReadExecute: effectiveAccessType = AccessType::ReadExecutePrivileged;   break;
            case AccessType::ReadPrivileged:
            case AccessType::WritePrivileged:
            case AccessType::ExecutePrivileged:
            case AccessType::ReadWritePrivileged:
            case AccessType::ReadExecutePrivileged:
                break;
        }
    }
    // Step 2: INSTCFG override (instCfg=3: Read→Execute; instCfg=2: Execute→Read).
    // BUG-13.1.4-CPP-A fix: §13.1.4 — ATOS (GATOS) must NOT apply INSTCFG or PRIVCFG overrides.
    if (transactionType != TransactionType::GatosTranslation) {
        if (config.instCfg == 3u) {
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
    }
    // Step 3: PRIVCFG override (privCfg=2: demote Privileged; privCfg=3: promote).
    // Skipped for GATOS transactions per §13.1.4.
    if (transactionType != TransactionType::GatosTranslation) {
        if (config.privCfg == 2u) {
            switch (effectiveAccessType) {
                case AccessType::ReadPrivileged:        effectiveAccessType = AccessType::Read;        break;
                case AccessType::WritePrivileged:       effectiveAccessType = AccessType::Write;       break;
                case AccessType::ExecutePrivileged:     effectiveAccessType = AccessType::Execute;     break;
                case AccessType::ReadWritePrivileged:   effectiveAccessType = AccessType::ReadWrite;   break;
                case AccessType::ReadExecutePrivileged: effectiveAccessType = AccessType::ReadExecute; break;
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
    }

    // §3.9 / §5.2 STE.S1DSS: When stage-1 is enabled and the stream is
    // substream-capable (S1CDMax > 0), non-substream transactions (PASID==0)
    // are handled according to STE.S1DSS before the normal CD[0] lookup.
    if (config.stage1Enabled && config.s1cdMax > 0 && pasid == 0) {
        if (config.s1dss == 0x00u || config.s1dss == 0x03u) {
            // §7.3.7: S1DSS==0b00 — non-substream transaction on substream-capable
            // stream aborts with F_STREAM_DISABLED (event 0x06).
            // BUG-AUDIT-110 fix: §5.2 STE.S1DSS line 6716 — S1DSS==0b11 is Reserved
            // and "behaves as 0b00".  Treat it identically: abort with F_STREAM_DISABLED.
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
                // BUG-5 fix: §7.3.14 — use effectiveAccessType so that PnU/InD/RnW
                // reflect the post-STE-override value (STRW/INSTCFG/PRIVCFG applied).
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType);
                return makeTranslationError(SMMUError::InvalidAddress);
            }
            // BUG-CPP-4 fix: §3.9 / §3.3.1.2 — when stage-2 is enabled, the IOVA
            // must be passed to stage-2 as the IPA rather than returning identity.
            // "If bypassing stage 1 (S1DSS==0b01), the input address is passed
            // directly to stage 2 as the IPA."
            // Use the stage-2 address space directly, mirroring the
            // performBothStagesTranslation() stage-2 path (iova is the IPA).
            if (config.stage2Enabled) {
                AddressSpace* s1dssS2AS = streamContext->getStage2AddressSpace();
                if (!s1dssS2AS) {
                    // Stage-2 not configured — translation fault.
                    tl_stage2FaultCtx.isStage2 = true;
                    tl_stage2FaultCtx.ipa      = iova;
                    FaultRecord s2Fault;
                    s2Fault.streamID    = streamID;
                    s2Fault.pasid       = pasid;
                    s2Fault.address     = iova;
                    s2Fault.faultType   = FaultType::TranslationFault;
                    s2Fault.accessType  = accessType;
                    s2Fault.securityState = securityState;
                    s2Fault.timestamp   = currentTime;
                    recordFault(s2Fault);
                    return makeTranslationError(SMMUError::PageNotMapped);
                }
                // S2T0SZ IPA range check (same as stage-2-only path).
                if (config.s2t0sz > 0u) {
                    uint64_t ipaLimit = UINT64_C(1) << (64u - static_cast<unsigned>(config.s2t0sz));
                    if (iova >= ipaLimit) {
                        tl_stage2FaultCtx.isStage2 = true;
                        tl_stage2FaultCtx.ipa      = iova;
                        generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova,
                                      securityState, false, 0, effectiveAccessType, true, iova);
                        return makeTranslationError(SMMUError::InvalidConfiguration);
                    }
                }
                // Translate iova as IPA through stage-2.
                TranslationResult s2Result = s1dssS2AS->translatePage(
                    iova, AccessType::Read, securityState, config.s2ha, config.s2affd);
                if (s2Result.isError()) {
                    tl_stage2FaultCtx.isStage2 = true;
                    tl_stage2FaultCtx.ipa      = iova;
                    FaultRecord s2Fault;
                    s2Fault.streamID    = streamID;
                    s2Fault.pasid       = pasid;
                    s2Fault.address     = iova;
                    s2Fault.faultType   = FaultType::TranslationFault;
                    s2Fault.accessType  = accessType;
                    s2Fault.securityState = securityState;
                    s2Fault.timestamp   = currentTime;
                    recordFault(s2Fault);
                    return s2Result;
                }
                // Validate effective access permissions against stage-2 result.
                const PagePermissions& s2perms = s2Result.getValue().permissions;
                bool permOk = false;
                switch (effectiveAccessType) {
                    case AccessType::Read:         permOk = s2perms.read && !s2perms.privilegedOnly; break;
                    case AccessType::Write:        permOk = s2perms.write && !s2perms.privilegedOnly; break;
                    case AccessType::Execute:      permOk = s2perms.execute && !s2perms.privilegedOnly; break;
                    case AccessType::ReadWrite:    permOk = s2perms.read && s2perms.write && !s2perms.privilegedOnly; break;
                    case AccessType::ReadExecute:  permOk = s2perms.read && s2perms.execute && !s2perms.privilegedOnly; break;
                    case AccessType::ReadPrivileged:      permOk = s2perms.read; break;
                    case AccessType::WritePrivileged:     permOk = s2perms.write; break;
                    case AccessType::ExecutePrivileged:   permOk = s2perms.execute; break;
                    case AccessType::ReadWritePrivileged: permOk = s2perms.read && s2perms.write; break;
                    case AccessType::ReadExecutePrivileged: permOk = s2perms.read && s2perms.execute; break;
                    default: permOk = false; break;
                }
                if (!permOk) {
                    tl_stage2FaultCtx.isStage2 = true;
                    tl_stage2FaultCtx.ipa      = iova;
                    FaultRecord permFault;
                    permFault.streamID    = streamID;
                    permFault.pasid       = pasid;
                    permFault.address     = iova;
                    permFault.faultType   = FaultType::PermissionFault;
                    permFault.accessType  = accessType;
                    permFault.securityState = securityState;
                    permFault.timestamp   = currentTime;
                    recordFault(permFault);
                    return makeTranslationError(SMMUError::PagePermissionViolation);
                }
                // translatePage() already includes the page offset in the returned PA.
                PA finalPA = s2Result.getValue().physicalAddress;
                TranslationData s1dssOutData(finalPA, s2perms, s2Result.getValue().securityState);
                return makeSuccess<TranslationData>(s1dssOutData);
            }
            // stage2Enabled=false: identity return (IOVA passed through as PA).
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
    // §5.4 / CT-13: CD.T0SZ/T1SZ validation — valid range [16,39] for SMMUv3.2; 0=sentinel (no restriction).
    if (config.stage1Enabled) {
        if (!config.aa64) {
            // AA64=0 (VMSAv8-32 LPAE) is unsupported — C_BAD_CD (event 0x0A).
            generateEvent(EventType::C_BAD_CD, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }
        if ((config.t0sz != 0u && config.t0sz < 16u) || config.t0sz > 39u ||
                (config.t1sz != 0u && config.t1sz < 16u) || config.t1sz > 39u) {
            // T0SZ or T1SZ out of valid range [16,39] (0=sentinel, no restriction) — C_BAD_CD.
            // §5.4 CDTxSZInvalid(): txsz_min=16, txsz_max=39 for STT=0, VAX=0, IAS=48 (SMMUv3.2).
            generateEvent(EventType::C_BAD_CD, streamID, pasid, iova, securityState);
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }
    }

    // BUG-5 fix: effectiveAccessType was computed here (NEW-02 fix comment).
    // It has been moved above the S1DSS block so that the S1DSS==0x01 OAS-overflow
    // path also sees the correct post-STE-override type. effectiveAccessType is
    // already available at this point.

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
            // NEW-02 fix: use effectiveAccessType so PnU/InD/RnW reflect STE overrides.
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, effectiveAccessType);
            // Return InvalidConfiguration so the outer handleTranslationFailure() switch
            // maps this to FaultType::StreamDisabled (a no-op) and does not re-emit
            // a second F_TRANSLATION event.  The event was already queued above.
            return makeTranslationError(SMMUError::InvalidConfiguration);
        }

        // BUG-AUDIT-153-CPP fix: §3.4.1 — Canonical VA sign-extension check.
        // "Input range checks ... fail unless bits VA[AddrTop:N-1] are identical."
        // AddrTop = 55 when TBI=1 (VA[63:56] treated as tag), else 63.
        // N = 64 - T0SZ.  VA[AddrTop:N-1] must all equal VA[N-1] (the sign bit).
        // A VA within the T0SZ magnitude window but with non-uniform high bits is
        // non-canonical and must generate F_TRANSLATION (not a silent success).
        {
            unsigned nBits  = 64u - static_cast<unsigned>(config.t0sz);
            unsigned addrTop = config.tbi ? 55u : 63u;
            if (addrTop >= nBits) {
                unsigned extWidth = addrTop - nBits + 2u; // bits in [AddrTop:N-1]
                uint64_t upper = effectiveIova >> (nBits - 1u);
                uint64_t mask  = (extWidth >= 64u) ? UINT64_MAX : ((UINT64_C(1) << extWidth) - 1u);
                bool nonCanonical = (upper != 0u) && (upper != mask);
                if (nonCanonical) {
                    generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                                  false, 0, effectiveAccessType);
                    return makeTranslationError(SMMUError::InvalidConfiguration);
                }
            }
        }
    }

    // NEW-7 fix: §5.4 — CD.EPD0=1: TTBR0 translation walk disabled → F_TRANSLATION.
    // SW model uses a single address space per PASID (no TTBR1), so EPD0 applies
    // to all stage-1 translations; EPD1 is architecturally for the upper VA half
    // (TTBR1) which this model does not implement separately.
    if (config.stage1Enabled && config.epd0) {
        // NEW-02 fix: use effectiveAccessType so PnU/InD/RnW reflect STE overrides.
        generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                      false, 0, effectiveAccessType);
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
        result = performBothStagesTranslation(streamID, pasid, lookupIova, effectiveAccessType, securityState, streamContext, config, currentTime);
    } else if (config.stage1Enabled && !config.stage2Enabled) {
        // Stage-1 only: IOVA -> PA directly
        result = performStage1OnlyTranslation(streamID, pasid, lookupIova, effectiveAccessType, securityState, streamContext, currentTime,
                                              transactionType == TransactionType::GatosTranslation);
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
                // QA-ADD fix: §7.3.13 — for S2-only streams, iova IS the IPA; all faults are
                // stage-2 faults. Set tl_stage2FaultCtx and pass s2=true/ipa=iova.
                tl_stage2FaultCtx.isStage2 = true;
                tl_stage2FaultCtx.ipa      = iova;
                generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                              false, 0, accessType, true, iova);
                // Return InvalidConfiguration so handleTranslationFailure() suppresses
                // duplicate event emission (StreamDisabled no-op path).
                return makeTranslationError(SMMUError::InvalidConfiguration);
            }
        }
        // Stage-2 only: IPA -> PA (IOVA = IPA)
        result = performStage2OnlyTranslation(streamID, pasid, iova, effectiveAccessType, securityState, streamContext, currentTime);
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
                                 uint16_t asid, uint16_t vmid, StreamWorld strw) {
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
    // BUG-QA-14: tag TLB entry with stream world for NSNH_ALL scoped invalidation.
    entry.strw = strw;
    // CONF-GAP-7: propagate IPA from two-stage translation result so that
    // TLBI_S2_IPA can perform selective IPA-based invalidation (§4.4).
    // For single-stage results data.ipa==0 (default), correctly marking the
    // entry as non-IPA-addressable.
    entry.ipa = data.ipa & ~static_cast<uint64_t>(PAGE_SIZE - 1u); // page-align
    // BUG-13.1.7-CPP fix: propagate page-level memory type so the TLB fast path
    // can enforce ARM §13.1.7 Rule 1 (Device/NC memory must use OSH) on cache hits.
    entry.pageAttr = data.pageAttr;

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
    
    // BUG-CPP-1 fix: effectiveAccessType (STRW->INSTCFG->PRIVCFG) is now computed
    // once in performTwoStageTranslation() and passed in as the accessType parameter.
    // The local re-derivation that previously duplicated that chain has been removed.
    // Use accessType directly as the already-effective access type throughout.
    const AccessType effectiveAccessType = accessType;

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
                // BUG-CPP-8 fix: §7.3.16 — security-state mismatches generate F_PERMISSION.
                // Use PermissionFault so handleTranslationFailure() routes through the
                // PermissionFault case, which propagates tl_stage2FaultCtx (s2/ipa) into
                // the generated event, correctly setting s2=true/ipa for stage-2 security
                // faults.
                faultType = FaultType::PermissionFault;
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
            // ARM §5.4: UWXN is IGNORED when all accesses are privileged, i.e.
            // when strw==EL2 or strw==EL3.  EL2_E2H is explicitly excluded from
            // this exemption per §3.3.4.
            {
                bool allPrivilegedStream = (config.strw == StreamWorld::EL2 ||
                                            config.strw == StreamWorld::EL3);
                if (config.uwxn && !allPrivilegedStream &&
                    (effectiveAccessType == AccessType::ExecutePrivileged ||
                     effectiveAccessType == AccessType::ReadExecutePrivileged) &&
                    s1p.write && !s1p.privilegedOnly) {
                    recordComprehensiveFault(streamID, pasid, iova, FaultType::PermissionFault,
                                            accessType, securityState, FaultStage::Stage1Only, currentTime, 0, 0);
                    return makeTranslationError(SMMUError::PagePermissionViolation);
                }
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
            // BUG-5PM-1 fix: §7.3.13 — S2T0SZ IPA range check is a stage-2 fault.
            // Set tl_stage2FaultCtx so stall path also carries correct S2/IPA.
            tl_stage2FaultCtx.isStage2 = true;
            tl_stage2FaultCtx.ipa      = intermediatePA;
            generateEvent(EventType::F_TRANSLATION, streamID, pasid, iova, securityState,
                          false, 0, effectiveAccessType, true, intermediatePA);
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
    // PASID-0 auto-link), the mappings in the AS are IOVA→PA (stage-1 format), not IPA→PA.
    // Attempting a stage-2 IPA lookup in the aliased AS would fail immediately with F_TRANSLATION.
    // Treat stage-1 result as the final translation (identity stage-2 semantics).
    //
    // BUG-CPP-3 fix: Suppress the alias short-circuit when the aliased AS has device-memory
    // pages registered in-band (via mapStage2DevicePage).  In that case the aliased AS doubles
    // as both the stage-1 AS and the stage-2 device-page registry, and the IPA from stage-1
    // must be checked against the device page to trigger the S2PTW F_PERMISSION fault.
    // We check whether the IPA produced by stage-1 is a device page; if so, bypass the
    // alias guard and proceed to the normal stage-2 S2PTW check below.
    bool aliasedS2HasDevicePage =
        (stage2AddressSpace == stage1AddressSpace.get()) &&
        stage2AddressSpace->getPageDeviceMemory(intermediatePA);
    if (stage2AddressSpace == stage1AddressSpace.get() && !aliasedS2HasDevicePage) {
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
                // BUG-CPP-3 fix: §7.3.14 — F_ADDR_SIZE must be emitted when stage-2
                // translatePage returns InvalidAddress (IPA >= 2^inputAddressSizeBits).
                // Unlike the other stage-2 error cases (which fall through to the shared
                // recordComprehensiveFault + tl_stage2FaultCtx block and return stage2Result
                // so handleTranslationFailure emits the event), this case mirrors the
                // S2PS OAS check pattern (lines above): emit the event inline here and
                // return InvalidConfiguration so handleTranslationFailure suppresses
                // the duplicate.  Without this, handleTranslationFailure maps
                // InvalidAddress → AddressSizeFault but its switch case calls only
                // handleAddressSizeFaultRecovery() — emitting no event at all.
                recordComprehensiveFault(streamID, pasid, intermediatePA, FaultType::AddressSizeFault,
                                       accessType, securityState, FaultStage::Stage2Only, currentTime, 1, 0);
                tl_stage2FaultCtx.isStage2 = true;
                tl_stage2FaultCtx.ipa      = intermediatePA;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType, true, intermediatePA);
                // Return InvalidConfiguration to suppress a second event in handleTranslationFailure().
                return makeTranslationError(SMMUError::InvalidConfiguration);
            case SMMUError::InvalidSecurityState:
                // BUG-CPP-8 fix: §7.3.16 — security-state mismatch at stage-2 must emit
                // F_PERMISSION with s2=true and ipa set to intermediatePA.  tl_stage2FaultCtx
                // is set below (lines 2286-2287) before returning, so handleTranslationFailure()
                // PermissionFault case will propagate it correctly.
                stage2FaultType = FaultType::PermissionFault;
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
                // BUG-5PM-2 fix: §7.3.14 — S2PS OAS check is a stage-2 fault.
                // Set tl_stage2FaultCtx so stall path also carries correct S2/IPA.
                tl_stage2FaultCtx.isStage2 = true;
                tl_stage2FaultCtx.ipa      = intermediatePA;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType, true, intermediatePA);
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

    // BUG-AUDIT-130 fix: ARM §3.13 / §3.13.4 — When S2HA=1, the SMMU must update
    // the stage-2 access flag on a successful translation.  translatePage() is const;
    // the AF write must be done here after stage-2 translation succeeds and before
    // the result is returned.  This mirrors the stage-1 HA path in
    // StreamContext::translateUnlocked() (lines 1316-1321).
    if ((config.s2ha || config.s2affd) && stage2AddressSpace) {
        stage2AddressSpace->updateAccessFlags(intermediatePA,
                                              config.s2ha,
                                              /*hd=*/false,  // S2HD always rejected (HTTU=0b01)
                                              effectiveAccessType);
    }

    // Create successful final translation result.
    // CONF-GAP-7: tag the result with the IPA (stage-1 output) so it can be
    // stored in the TLBEntry.ipa field for selective TLBI_S2_IPA invalidation.
    // BUG-AUDIT-49 fix: propagate the stage-2 page's memory-type attribute so
    // gatosTranslate() can report the correct GATOS_PAR ATTR/SH (§9.1.4 / §6.3.40).
    // BUG-13.1.5-CPP fix: §13.1.5 strength ordering — Device (0x00) wins over Normal (0xFF).
    // When combining S1+S2 memory attributes, Device memory is the strongest restriction.
    auto combinePageAttr = [](uint8_t s1Attr, uint8_t s2Attr) -> uint8_t {
        // Device-nGnRnE (0x00) is the strongest; it wins over any Normal attribute.
        if (s1Attr == 0x00u || s2Attr == 0x00u) return 0x00u;
        // For Normal memory types, S2 takes precedence (outer stage dominates).
        return s2Attr;
    };
    TranslationData twoStageResult(stage2Data.physicalAddress, finalPermissions, stage2Data.securityState);
    twoStageResult.ipa      = intermediatePA;
    twoStageResult.pageAttr = combinePageAttr(stage1Data.pageAttr, stage2Data.pageAttr);
    return makeSuccess<TranslationData>(twoStageResult);
}

TranslationResult SMMU::performStage1OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                    AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime,
                                                    bool isAtos) {
    // ARM SMMU v3 spec: Stage-1 only translation IOVA -> PA

    // BUG-CPP-2 fix: effectiveAccessType (STRW->INSTCFG->PRIVCFG) is now computed
    // once in performTwoStageTranslation() and passed in as the accessType parameter.
    // The local re-derivation that previously duplicated that chain has been removed.
    // Use accessType directly as the already-effective access type throughout.
    const AccessType effectiveAccessType = accessType;

    TranslationResult result = streamContext->translate(pasid, iova, effectiveAccessType, securityState, isAtos);

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
                // BUG-CPP-8 fix: §7.3.16 — security-state mismatch generates F_PERMISSION.
                // Use PermissionFault so handleTranslationFailure() PermissionFault case
                // handles the event; for stage-1-only streams tl_stage2FaultCtx is not set
                // (isStage2=false) so the event correctly has s2=false.
                fault.faultType = FaultType::PermissionFault;
                break;
            case SMMUError::InvalidAddress:
                // ARM §3.4.1: IOVA exceeded the per-context input address size.
                // BUG-CPP-04 fix: Emit F_ADDR_SIZE here (in the stage-specific path) so
                // that handleTranslationFailure does not need to emit it again.
                // BUG-5PM-3 fix: use effectiveAccessType (post STRW/INSTCFG/PRIVCFG) so
                // InD/PnU wire-format fields reflect the STE overrides.
                fault.faultType = FaultType::AddressSizeFault;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType);
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

    // BUG-CPP-2 fix: effectiveAccessType (STRW->INSTCFG->PRIVCFG) is now computed
    // once in performTwoStageTranslation() and passed in as the accessType parameter.
    // The local re-derivation that previously duplicated that chain has been removed.
    // Use accessType directly as the already-effective access type throughout.
    const AccessType effectiveAccessType = accessType;

    TranslationResult result = streamContext->translate(pasid, iova, effectiveAccessType, securityState);

    // Record fault if translation failed
    if (result.isError()) {
        // BUG-NEW-CPP-A fix: §7.3.13 — for S2-only streams, iova IS the IPA; all faults
        // are stage-2 faults. Set tl_stage2FaultCtx on every error path so generateEvent()
        // sees S2=true and IPA=iova regardless of the error type.
        tl_stage2FaultCtx.isStage2 = true;
        tl_stage2FaultCtx.ipa      = iova;
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
                // BUG-CPP-8 fix: §7.3.16 — security-state mismatch at stage-2 must emit
                // F_PERMISSION with s2=true and ipa=iova (the IPA for S2-only streams).
                // tl_stage2FaultCtx is set above (BUG-NEW-CPP-A fix) before the switch,
                // so handleTranslationFailure() PermissionFault case will propagate it.
                fault.faultType = FaultType::PermissionFault;
                break;
            case SMMUError::InvalidAddress:
                // BUG-CPP-04 fix: Emit F_ADDR_SIZE here (in the stage-specific path) so
                // that handleTranslationFailure does not need to emit it again.
                // BUG-5PM-4 fix: use effectiveAccessType (post INSTCFG/PRIVCFG) so InD/PnU
                // reflect STE overrides.
                // QA-ADD fix: §7.3.13 — for S2-only streams, iova IS the IPA; all faults
                // are stage-2 faults. Set tl_stage2FaultCtx and pass s2=true/ipa=iova.
                fault.faultType = FaultType::AddressSizeFault;
                tl_stage2FaultCtx.isStage2 = true;
                tl_stage2FaultCtx.ipa      = iova;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType, true, iova);
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
                // BUG-5PM-4 fix: use effectiveAccessType for InD/PnU fields.
                // QA-ADD fix: §7.3.13 — S2-only stream: iova IS the IPA; all faults are stage-2.
                tl_stage2FaultCtx.isStage2 = true;
                tl_stage2FaultCtx.ipa      = iova;
                generateEvent(EventType::F_ADDR_SIZE, streamID, pasid, iova, securityState,
                              false, 0, effectiveAccessType, true, iova);
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
                                   AccessType accessType, AccessType effectiveAccessType,
                                   SecurityState securityState, TranslationResult& result, uint64_t currentTime,
                                   TransactionType transactionType) {
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
                // BUG-CPP-8 fix: §7.3.16 — security-state mismatches generate F_PERMISSION.
                // Route through PermissionFault so the switch case below calls
                // generateEvent(F_PERMISSION, ..., tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa)
                // which correctly sets s2=true and ipa for two-stage security faults.
                faultType = FaultType::PermissionFault;
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

    // BUG-CPP-5 fix: §7.3 — EventEntry InD/PnU must reflect post-STE-override access type
    // at the time of the translation, not at the time this recovery function executes.
    // The previous implementation re-fetched the StreamConfig under a stripe lock here,
    // creating a TOCTOU race: a concurrent configureStream() between translation completion
    // and this re-fetch could change INSTCFG/PRIVCFG, causing the event to carry overrides
    // that were NOT applied during the actual translation.
    // Fix: accept the already-computed effective access type as a parameter (computed in
    // translate() from the streamCfgSnapshot taken before the stripe lock was released),
    // and use it directly — no re-fetch, no lock, no race.
    const AccessType eventAccessType = effectiveAccessType;

    // BUG-AUDIT-155-CPP fix: §3.9.1.2 — ATS TR that encounters Address Size / Access /
    // Translation fault must return Success R==W==0 with NO event recorded.
    if (transactionType == TransactionType::AtsTranslationRequest) {
        switch (faultType) {
            case FaultType::TranslationFault:
            case FaultType::PermissionFault:
            case FaultType::AddressSizeFault:
            case FaultType::AccessFault:
            case FaultType::AccessFlagFault:
                // Spec §3.9.1.2: suppress event, return Success with PA=0 (R==W==0).
                result = makeTranslationSuccess(0);
                return;
            default:
                break;  // config faults and others fall through to normal handling
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

        case FaultType::StreamDisabled:
            // §7.3.7: Stream is administratively disabled — event was generated above; no recovery needed
            break;

        case FaultType::BadStreamID:
            // §7.3.3: StreamID not in stream table — C_BAD_STREAMID event was generated in translate(); no recovery
            break;

        case FaultType::BadSTE:
            // §7.3.5: In-range StreamID with STE.V=0 — C_BAD_STE event was generated in translate(); no recovery
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
            // Default handling for ARM SMMU v3 specific faults
            // Recovery actions would be implemented here in a full implementation
            break;

        case FaultType::Stage2PermissionFault:
            // BUG-NEW-1 fix: §7.3 — Stage2PermissionFault must emit F_PERMISSION, not
            // silently drop the event.  This case is reached via the default: arm of
            // the stage-2 error classification switch in performBothStagesTranslation()
            // when translatePage() returns an unrecognized SMMUError.  Before this fix,
            // the case fell through to the no-op block, silently dropping the event.
            generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
            break;

        case FaultType::SecurityFault:
            // BUG-NEW-5 fix: §7.3.16 — SecurityFault must emit F_PERMISSION.
            // FaultType::SecurityFault is a valid enum value defined in types.h, but
            // handleTranslationFailure() previously had no case for it and no default:,
            // so any path that classified a fault as SecurityFault would silently drop
            // the event.  Map to F_PERMISSION matching the InvalidSecurityState →
            // PermissionFault mapping in the SMMUError switch above.
            generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
            break;

        default:
            // BUG-NEW-5 fix: defensive fallback — any future FaultType value that is
            // not explicitly handled must not silently drop the event.  Emit F_PERMISSION
            // as a safe default to ensure the event queue is never silently starved.
            generateEvent(EventType::F_PERMISSION, streamID, pasid, iova, securityState,
                          false, 0, eventAccessType,
                          tl_stage2FaultCtx.isStage2, tl_stage2FaultCtx.ipa);
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
        // BUG-CPP-1 fix: §6.3.17 EVENTQ_CONS.OVACKFLG (bit 31) is software-written;
        // the SMMU must never modify it.  Preserve bit 31 via RMW rather than storing
        // the raw advanceQueueIndex result (which drops bit 31 since modulus <= 2^20).
        {
            uint32_t oldCons = eventqCons.load(std::memory_order_relaxed);
            uint32_t newRD = advanceQueueIndex(oldCons & ~(1u << 31), eventqLog2Size);
            eventqCons.store((oldCons & (1u << 31)) | newRD, std::memory_order_release);
        }
    }
    // BUG-AUDIT-154-CPP fix: ARM §3.5.3 — drain stallPending_ into eventQueue
    // after the consumption loop completes and space has been freed.  Stall events
    // must be recorded "when entries are consumed from the Event queue and space
    // next becomes available" per §3.5.3 line 1824.  The drain runs post-loop so
    // promoted stall events are not immediately re-consumed by the same call.
    while (!stallPending_.empty() && eventQueue.size() < maxEventQueueSize) {
        eventQueue.push_back(stallPending_.front());
        stallPending_.pop_front();
        // BUG-NEW-D fix: preserve OVFLG (bit 31) across the PROD advance.
        {
            uint32_t oldProd = eventqProd.load(std::memory_order_relaxed);
            uint32_t newProd = advanceQueueIndex(oldProd, eventqLog2Size) | (oldProd & (1u << 31));
            eventqProd.store(newProd, std::memory_order_release);
        }
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
    // BUG-CPP-1/2 fix: getEventQueue() is a pure non-destructive snapshot.
    // ARM §3.5.1: CONS.RD must only advance when an entry is actually consumed
    // (removed from the queue).  getEventQueue() copies entries without removing
    // them; therefore it must NOT advance eventqCons.  Only processEventQueue()
    // (which removes entries) and clearEventQueue() (which resets the queue) may
    // modify eventqCons.  The previous BUG-AUDIT-4 "fix" incorrectly added CONS
    // advancement here, causing double-advancement when both getEventQueue() and
    // processEventQueue() were called on the same entries.
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
    // BUG-NEW-39: S2P (bit 0) is now conditional on s2pSupported_ so that
    // setS2PSupported(false) can disable stage-2 TLBI commands for testing.
    return (s2pSupported_.load(std::memory_order_acquire) ? (1u << 0) : 0u) // S2P: configurable
         | (s1pSupported_.load(std::memory_order_acquire) ? (1u << 1) : 0u) // S1P: configurable (BUG-AUDIT-NEW-03 fix §6.3.1 — stage-1 present)
         | (2u << 2)        // TTF[3:2] = 0b10 (=2): AArch64 stage-1 and stage-2
         | (1u << 6)         // HTTU[7:6] = 0b01 (=1): access flag only (BUG-AUDIT-35 fix §6.3.1 — dirty state not implemented)
         | (1u << 5)        // BTM: broadcast TLB maintenance (receiveBroadcastTLBI() with CR2.PTM gating) — GAP-R07 §6.3.1
         | ((hypSupported_.load(std::memory_order_acquire) && s1pSupported_.load(std::memory_order_acquire) && s2pSupported_.load(std::memory_order_acquire)) ? (1u << 9) : 0u) // Hyp: gated on Hyp+S1P+S2P (BUG-AUDIT-41 fix §6.3.1)
         | (1u << 8)         // DORMHINT: dormancy signaling supported (BUG-AUDIT-53 fix §6.3.1 — shutdown()/STATUSR.DORMANT implemented; bit must be 1 so software knows DORMANT is meaningful)
         | (1u << 10)       // ATS: PCIe ATS supported
         | ((ns1atsSupported_.load(std::memory_order_acquire) && s1pSupported_.load(std::memory_order_acquire) && s2pSupported_.load(std::memory_order_acquire)) ? (1u << 11) : 0u) // NS1ATS: gated on S1P+S2P (BUG-AUDIT-20 fix §6.3.1 — RES0 when ATS==0 OR S1P==0 OR S2P==0)
         | (1u << 12)       // ASID16: 16-bit ASIDs supported
         | (sevSupported_.load(std::memory_order_acquire) ? (1u << 14) : 0u)  // SEV: gated (BUG-AUDIT-55 fix §6.3.1 — WFE/SEV not implemented; default false)
         | (1u << 15)       // ATOS: address translation operations (GATOS) supported
         | (priSupported_.load(std::memory_order_acquire) ? (1u << 16) : 0u)  // PRI: configurable (BUG-NEW-G fix §4.5.2)
         // NOTE: AUDIT-NEW-03: ARM §6.3.1 requires PRI to be RES0 when ATS==0. ATS is currently hardcoded to 1;
         // if ATS is ever made configurable, add '&& ats_enabled' guard here.
         | (s2pSupported_.load(std::memory_order_acquire) ? (1u << 17) : 0u)  // VMW: gated on S2P (BUG-A fix §6.3.1)
         | (1u << 18)       // VMID16: 16-bit VMIDs supported
         | (1u << 19)       // CD2L: 2-level CD table supported (BUG-AUDIT-32 fix §6.3.1 — model uses flat PASID map, no s1cdMax limit)
         | (1u << 23)       // ATSRECERR: extended ATS error recording (CR2.REC_CFG_ATS gating) — GAP-R04 §6.3.1/§2.5 (BUG-AUDIT-56: latent guard — RES0 when ATS==0; ATS currently hardcoded 1 so bit is always set; add && ats_enabled if ATS ever made configurable)
         // BUG-2 fix: TERM_MODEL (bit 26) cleared — model implements both stall and
         // terminate behaviors and does NOT validate CD.A=1.  ARM IHI0070G.b §6.3.1:
         // when TERM_MODEL=1, C_BAD_CD must fire if CD.A=0.  Setting TERM_MODEL=0
         // (RAZ/WI IS supported) matches the model's actual behaviour.
         | (1u << 27)       // ST_LEVEL[0]: 2-level stream table supported
         // BUG-QA-1: §6.3.1 STALL_MODEL[25:24] must reflect the runtime stallModel_
         // value rather than being hardcoded to 0b00.
         | ((static_cast<uint32_t>(stallModel_.load(std::memory_order_acquire)) & 0x3u) << 24);
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
    // BUG-AUDIT-02 fix: IDR3.XNX (bit 4) is RES0 when IDR0.S2P==0 (ARM §6.3.4).
    // Gate bit 4 on s2pSupported_ so that setS2PSupported(false) correctly clears XNX.
    // BUG-AUDIT-37 fix: IDR3.HAD (bit 2) is RES0 when IDR0.S1P==0 (ARM §6.3.4 line 11425).
    // "When SMMU_IDR0.S1P == 0, SMMU_IDR3.HAD == 0"
    return (s1pSupported_.load(std::memory_order_acquire) ? (1u << 2) : 0u) // HAD: gated on S1P (BUG-AUDIT-37 fix §6.3.4)
         | (s2pSupported_.load(std::memory_order_acquire) ? (1u << 4) : 0u) // XNX: gated on S2P (BUG-AUDIT-02 fix §6.3.4)
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
    // bits[31:16]: STALL_MAX — RES0 when IDR0.STALL_MODEL==0b01 (terminate-only) per §6.3.6.
    // BUG-QA-2: was hardcoded to 64; now conditional on stallModel_.
    uint32_t stall_max = (stallModel_.load(std::memory_order_acquire) == 0x01u) ? 0u : 64u;
    // BUG-AUDIT-52 fix: ARM §6.3.6 — only the 4KB granule is implemented.
    // GRAN16K (bit[5]) and GRAN64K (bit[6]) must NOT be set unless the corresponding
    // granule size is actually supported.  Claiming them without support would allow
    // software to configure unsupported granule sizes.
    return 5u           // OAS = 5 (48-bit) in bits[2:0]
         | (1u << 4)    // GRAN4K — only 4KB granule is implemented
         | (stall_max << 16); // STALL_MAX[31:16]
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
// NEW-AUDIT-05 fix: isStage2 and eventClass parameters allow the caller to express
// all four F_WALK_EABT contexts defined by ARM §7.3.12:
//   1. S1 walk only:              isStage2=false, eventClass=1 (CLASS=TT)
//   2. S2 TT descriptor walk:     isStage2=true,  eventClass=1 (CLASS=TT)
//   3. S2 IPA input walk:         isStage2=true,  eventClass=2 (CLASS=IN)
//   4. Two-stage S2 CD fetch:     isStage2=true,  eventClass=0 (CLASS=CD) — stage-2 abort during CD fetch
// generateEvent() hard-codes eventClass=1 for F_WALK_EABT in its switch; override
// it after the call by mutating the last event in the queue under queueMutex.
void SMMU::injectWalkEabt(StreamID streamID, PASID pasid, IOVA iova,
                           bool isStage2, uint8_t eventClass, SecurityState ss) {
    generateEvent(EventType::F_WALK_EABT, streamID, pasid, iova,
                  ss, /*isStall=*/false, /*stag=*/0,
                  AccessType::Read, isStage2, /*ipaValue=*/0);
    // Override eventClass: generateEvent() always sets CLASS=TT (1) for F_WALK_EABT.
    // Apply the caller-specified eventClass so two-stage cases (CLASS=IN, eventClass=2)
    // are correctly expressed.  Mutate the last enqueued event under queueMutex.
    if (eventClass != 1u) {
        std::lock_guard<std::recursive_mutex> lock(queueMutex);
        if (!eventQueue.empty() && eventQueue.back().type == EventType::F_WALK_EABT) {
            eventQueue.back().eventClass = eventClass;
        }
    }
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
// BUG-NEW-11: STALL_MODEL configuration (§4.7.1/§4.7.2)
// ─────────────────────────────────────────────────────────────────────────────

// Set IDR0.STALL_MODEL.  0b00 = stall+terminate; 0b01 = terminate-only.
// BUG-AUDIT-46 fix: ARM IHI0070G.b §6.3.1 IDR0[25:24] — 0b11 is Reserved.
// Silently ignore any value > 0b10 to prevent a reserved encoding from being
// stored in stallModel_ and subsequently exposed through IDR0 or STALL_MAX.
void SMMU::setStallModel(uint8_t model) {
    if (model > 0x02u) {
        return;  // 0b11 is Reserved per ARM §6.3.1 IDR0 STALL_MODEL[25:24]
    }
    stallModel_.store(model, std::memory_order_release);
}

uint8_t SMMU::getStallModel() const {
    return stallModel_.load(std::memory_order_acquire);
}

// BUG-NEW-16 fix: IDR0.Hyp is now configurable so the CMD_TLBI_EL2_ALL guard
// (§4.4.2.7) is exercisable in tests.
void SMMU::setHypSupported(bool enabled) {
    hypSupported_.store(enabled, std::memory_order_release);
}

bool SMMU::isHypSupported() const {
    return hypSupported_.load(std::memory_order_acquire);
}

// BUG-NEW-39 fix: IDR0.S2P is now configurable so the CERROR_ILL guard for
// TLBI_S12_VMALL / TLBI_S2_IPA (§4.4.3.1/§4.4.3.2) is exercisable in tests.
// BUG-AUDIT-149-CPP fix: §3.3 line 1429 — SMMU must support at least one stage.
// If disabling S2P would leave S1P also false, the request is silently ignored.
void SMMU::setS2PSupported(bool enabled) {
    if (!enabled && !s1pSupported_.load(std::memory_order_acquire)) {
        return; // §3.3 line 1429: at least one stage required; refuse to clear the last one.
    }
    s2pSupported_.store(enabled, std::memory_order_release);
}

bool SMMU::isS2PSupported() const {
    return s2pSupported_.load(std::memory_order_acquire);
}

// BUG-AUDIT-NEW-03 fix: IDR0.S1P is now configurable so the CERROR_ILL guard for
// TLBI_NH_ALL/ASID/VA/VAA (§4.4.2.1-4) and CFGI_CD/ALL (§4.3.3/4.3.4) is exercisable.
// BUG-AUDIT-149-CPP fix: §3.3 line 1429 — SMMU must support at least one stage.
// If disabling S1P would leave S2P also false, the request is silently ignored.
void SMMU::setS1PSupported(bool enabled) {
    if (!enabled && !s2pSupported_.load(std::memory_order_acquire)) {
        return; // §3.3 line 1429: at least one stage required; refuse to clear the last one.
    }
    s1pSupported_.store(enabled, std::memory_order_release);
}

bool SMMU::isS1PSupported() const {
    return s1pSupported_.load(std::memory_order_acquire);
}

// BUG-NEW-G fix: IDR0.PRI (bit 16) is now configurable so the CERROR_ILL guard
// for CMD_PRI_RESP (ARM §4.5.2) can be tested with PRI==0.
// BUG-AUDIT-148-CPP fix: when transitioning to false (PRI structurally absent),
// clear any in-flight priQueue entries and reset PROD/CONS indices to 0.
void SMMU::setPRISupported(bool enabled) {
    priSupported_.store(enabled, std::memory_order_release);
    if (!enabled) {
        std::lock_guard<std::recursive_mutex> lock(queueMutex);
        priQueue.clear();
        priqProd.store(0, std::memory_order_release);
        priqCons.store(0, std::memory_order_release);
    }
}

// BUG-AUDIT-01 fix: IDR0.NS1ATS (bit 11) is now configurable so the C_BAD_STE
// guard for EATS==0b10+NS1ATS==1 (ARM §5.2 SteIllegal()) is exercisable in tests.
void SMMU::setNS1ATSSupported(bool supported) {
    ns1atsSupported_.store(supported, std::memory_order_release);
}

// BUG-AUDIT-55 fix: IDR0.SEV (bit 14) is now gated on sevSupported_ (§4.7.3 / §6.3.1).
// CMD_SYNC CS=0b10 (SIG_SEV) WFE-wake is not implemented; default false prevents
// software from relying on SEV=1 for WFE-based synchronization.
void SMMU::setSevSupported(bool enabled) {
    sevSupported_.store(enabled, std::memory_order_release);
}

// BUG-NEW-H fix: ARM §8.1 — PRIQ_CONS.OVACKFLG (bit 31) is written by software to
// acknowledge a PRI queue overflow.  Provides C++ API parity with Rust set_priq_cons_ovackflg().
void SMMU::setPriqConsOvackflg(uint8_t value) {
    // Atomically set or clear bit 31 (OVACKFLG) while preserving the rest of PRIQ_CONS.
    uint32_t expected = priqCons.load(std::memory_order_acquire);
    uint32_t desired;
    do {
        if (value != 0u) {
            desired = expected | (1u << 31u);
        } else {
            desired = expected & ~(1u << 31u);
        }
    } while (!priqCons.compare_exchange_weak(expected, desired,
                                              std::memory_order_acq_rel,
                                              std::memory_order_acquire));
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
        // BUG-AUDIT-38 fix: three event types are Reserved in the GATOS/ATOS context
        // per ARM IHI0070G.b §9.1.5 Table — they cannot be generated through the
        // GATOS path and must map to 0xFD (INTERNAL_ERR) to signal an unexpected
        // fault code rather than a misleading spec-defined one:
        //   F_UUT (0x01): cannot arise from GATOS — device-tree sourced fault.
        //   F_BAD_ATS_TREQ (0x05): cannot arise — GATOS is not an ATS TLB request.
        //   F_TRANSL_FORBIDDEN (0x07): cannot arise — GATOS is not an ATS translated txn.
        case EventType::F_UUT:               return 0xFDu; // Reserved in GATOS context (BUG-AUDIT-38)
        case EventType::C_BAD_STREAMID:      return 0x02u;
        case EventType::F_STE_FETCH:         return 0x03u;
        case EventType::C_BAD_STE:           return 0x04u;
        case EventType::F_BAD_ATS_TREQ:      return 0xFDu; // Reserved in GATOS context (BUG-AUDIT-38)
        case EventType::F_STREAM_DISABLED:   return 0x06u;
        case EventType::F_TRANSL_FORBIDDEN:  return 0xFDu; // Reserved in GATOS context (BUG-AUDIT-38)
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
    // BUG-AUDIT-39 fix: ARM IHI0070G.b §9.1 — GATOS requires SMMU_CR0.SMMUEN==1.
    // When SMMUEN==0 the SMMU is globally disabled; return a fault PAR immediately
    // with FAULT=1 and FAULTCODE=0xFD (INTERNAL_ERR) rather than attempting
    // translation through a disabled SMMU.
    // BUG-AUDIT-62 fix: use authoritative cr0_ register instead of shadow smmuen_
    // (consistent with BUG-CPP-3 fix rationale — eliminate split-brain hazard).
    if ((cr0_.load(std::memory_order_acquire) & CR0_SMMUEN) == 0u) {
        // GATOS fault PAR: FAULT=1, REASON=0b00, FAULTCODE=0xFD, FADDR=0.
        return 0x1ULL | (static_cast<uint64_t>(0xFDu) << 4);
    }

    // BUG-AUDIT-64 fix: ARM IHI0070G.b §9.1.3 line 27699 — GATOS for a stream with
    // bypass (Config=0b100) or disabled/abort (Config=0b0xx) must return INV_STAGE:
    // FAULT=1, FAULTCODE=0xFE. Absent (in-range) streams are also INV_STAGE.
    // Out-of-range StreamIDs (strtab range check) must fall through to translate()
    // so they produce C_BAD_STREAMID (FAULTCODE=0x02) per §7.3.3/§9.1.5.
    // The pre-check block is scoped so the stripe lock is released before translate().
    {
        // Only apply the INV_STAGE pre-check when the StreamID is within range.
        // Out-of-range StreamIDs are handled by translate() which emits C_BAD_STREAMID.
        uint8_t log2sz = strtabLog2Size_.load(std::memory_order_acquire);
        bool inRange = (log2sz >= 32u) ||
                       (static_cast<uint64_t>(streamID) < ((uint64_t)1u << log2sz));
        if (inRange) {
            size_t stripe = getStreamStripe(streamID);
            std::lock_guard<std::mutex> streamLk(streamLockStripes[stripe]);
            auto streamIt = streamMap.find(streamID);
            if (streamIt == streamMap.end()) {
                // In-range absent stream: no STE — INV_STAGE per §9.1.3.
                return 0x1ULL | (static_cast<uint64_t>(0xFEu) << 4);
            }
            StreamConfig cfg = streamIt->second->getStreamConfiguration();
            if (!cfg.stage1Enabled && !cfg.stage2Enabled) {
                // Bypass (bypassEnabled=true) or disabled/abort: INV_STAGE per §9.1.3.
                return 0x1ULL | (static_cast<uint64_t>(0xFEu) << 4);
            }
        }
    }

    // Snapshot the event queue size before translation so we can identify
    // any new event appended by the translation (GAP-L fix).
    size_t evtSizeBefore = 0u;
    {
        std::lock_guard<std::recursive_mutex> lk(queueMutex);
        evtSizeBefore = eventQueue.size();
    }
    // BUG-13.1.4-CPP-A fix: §13.1.4 — ATOS must not apply INSTCFG/PRIVCFG overrides.
    // Pass GatosTranslation so performTwoStageTranslation() skips those override steps.
    TranslationResult result = translate(streamID, pasid, iova, accessType, securityState,
                                         TransactionType::GatosTranslation);
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
    // BUG-AUDIT-49 fix: ARM §9.1.4 / §6.3.40 — use per-page memory-type attribute.
    // pageAttr=0x00 → Device-nGnRnE: SH must be OSH (0b10) per §9.1.4.
    // pageAttr=0xFF → Normal WB/WA:  SH is ISH (0b11).
    uint8_t  pageAttr   = result.getValue().pageAttr;
    uint64_t sh         = static_cast<uint64_t>(pageAttr == 0x00u ? 2u : 3u) << 8;
    uint64_t attr       = static_cast<uint64_t>(pageAttr) << 56;
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

    // BUG-CPP-1 fix: Acquire the processing serialization mutex before entering
    // the command loop.  This guarantees that at most one thread executes the
    // loop at a time.  Without this, the CMD_SYNC handler's temporary release of
    // queueMutex (required to avoid the ABBA stripe→queueMutex deadlock) opens a
    // window where a second concurrent call to processCommandQueue() can enter
    // the loop, dequeue commands, and advance CONS.RD — violating the ARM §3.5.1
    // requirement that commands are consumed in strict submission order by a
    // single logical consumer.
    //
    // Lock order: cmdqProcessingMutex_ is acquired first (here), then queueMutex
    // is acquired inside the loop body.  This order is never reversed, so no new
    // deadlock is introduced.
    std::lock_guard<std::mutex> processingLock(cmdqProcessingMutex_);

    // ARM SMMU v3 spec: Process command queue with proper ordering.
    // BUG-03 fix: protect commandQueue with queueMutex.
    // BUG-CPP-C01 fix: Use unique_lock so we can temporarily release queueMutex
    // before acquiring a stream stripe lock inside the CMD_SYNC handler.
    // Lock ordering invariant: stripe lock must NEVER be acquired while
    // queueMutex is held (translate() acquires stripe -> queueMutex via
    // generateEvent(); holding both in the opposite order causes an ABBA
    // deadlock).  Releasing queueMutex before taking the stripe lock, then
    // re-acquiring queueMutex to continue the loop, preserves the invariant.
    // With cmdqProcessingMutex_ now serializing the entire function, the
    // unlock/re-lock of queueMutex remains correct and the concurrent-entry
    // race window is closed.
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
        // BUG-CPP-1 fix: Peek at front WITHOUT popping — pop only on success.
        // ARM §7.1: on error, commandQueue.front() must remain the erroneous
        // command so that CONS.RD and queue-front stay in sync after the break.
        // Previously the code called pop_front() here (before processCommand),
        // which desynced the queue from CONS.RD when an error caused a break.
        CommandEntry command = commandQueue.front();

        // Process the command based on type (invalidations, PRI_RESP, RESUME, etc.).
        processCommand(command, lock);

        // BUG-NEW-23 fix: CMD_SYNC validation and completion is now fully
        // handled inside processCommand() (SYNC case).  processCommandQueue()
        // relies on the GERROR_CMDQ_ERR check below to detect the CS=3 error
        // path, which processCommand() signals via signalGerror(GERROR_CMDQ_ERR).
        // No additional CMD_SYNC special-casing is needed here.

        // ARM §6.3.17: Commands must not be processed while GERROR.CMDQ_ERR is active.
        // If processCommand() signalled CMDQ_ERR, halt queue processing immediately.
        // Active = GERROR[x] != GERRORN[x] (unacknowledged).  BUG-03/SPEC-09.
        // BUG-CPP-5 fix: on error, do NOT advance CONS.RD — leave it pointing at
        // the erroneous command so software can identify which command failed.
        if ((gerrorStatus.load(std::memory_order_relaxed) ^ gerrorNStatus.load(std::memory_order_relaxed)) & GERROR_CMDQ_ERR) {
            break;
        }

        // SUCCESS path: remove the command from the deque and advance CONS.RD.
        // BUG-CPP-1 fix: pop_front() moved here from the top of the loop so that
        // erroneous commands (CS=3 and GERROR_CMDQ_ERR paths above) are NOT
        // removed from commandQueue on error — they stay at front so that
        // CONS.RD and commandQueue.front() remain in sync (ARM §7.1).
        commandQueue.pop_front();

        // ARM §3.5.1: Advance consumer index only for successfully processed
        // commands (FINDING-M-01).
        // BUG-CPP-5 fix: this block is now reached only when no error was detected,
        // ensuring CONS.RD is not advanced past an erroneous command (ARM §7.1).
        // BUG-1 fix: §6.3.28 — CMDQ_CONS.ERR bits [30:24] must persist until
        // software clears them.  A plain store(advanceQueueIndex()) zeros the
        // upper bits on every dequeue.  Use a read-modify-write so that only
        // the RD field (bits [19:0]) is updated.
        // OPEN-1 fix (§3.5.1/3.5.3): narrow the preservation mask to only the
        // ERR field bits [30:24] (mask 0x7F000000u).  The earlier mask
        // ~0xFFFFFu = 0xFFF00000u incorrectly preserved bit 31 (RES0 per
        // ARM §6.3.28) as well as bits [23:20] (also RES0).  CMDQ_CONS bit 31
        // is NOT an OVFLG/OVACKFLG — it must always be 0.
        {
            uint32_t oldCons = cmdqCons.load(std::memory_order_relaxed);
            uint32_t newRD   = advanceQueueIndex(oldCons & 0xFFFFFu, cmdqLog2Size);
            cmdqCons.store((oldCons & 0x7F000000u) | newRD,
                           std::memory_order_release);
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
    // NEW-BUG-B fix + BUG-NEW-15 fix: effective PRIQEN = CR0.PRIQEN AND CR0.SMMUEN.
    // §8.2: when effective PRIQEN==0, all incoming PPRs cause an automatic PRG
    // Response with ResponseCode==0b1111 ('Response Failure') and are discarded.
    // Only the Last=1 PPR of a PRG requires a response (PRIv2 protocol).
    // Lock is acquired first so priAutoFailures_ is protected.
    // BUG-03 fix: protect priQueue with queueMutex.
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // BUG-AUDIT-148-CPP fix: §3.1 line 1220 — PRI queue only exists on PRI-supporting SMMUs.
    // When priSupported_==false (IDR0.PRI==0), the queue is structurally absent.
    // Silent drop — no auto-failure, no enqueue.
    if (!priSupported_.load(std::memory_order_acquire)) {
        return;
    }
    {
        uint32_t cr0val = cr0_.load(std::memory_order_acquire);
        bool priqen = (cr0val & CR0_PRIQEN) != 0u;
        bool smmuen = (cr0val & CR0_SMMUEN) != 0u;
        // BUG-NEW-31 fix / NEW-BUG-B fix / BUG-NEW-15 fix: ARM §8.2 — effective
        // PRIQEN = CR0.PRIQEN AND CR0.SMMUEN.  When effective PRIQEN==0 (i.e.
        // EITHER bit is clear), all incoming PPRs cause an automatic PRG Response
        // with ResponseCode==0b1111 ('Response Failure') and are discarded.
        // The previous XOR check only fired when exactly one bit differed; the
        // correct OR semantics ensure both-zero also triggers auto-failure.
        if (!priqen || !smmuen) {
            // §8.2: ResponseCode=0b1111 unconditionally (not PASID-conditional).
            if (request.isLastRequest) {
                priAutoFailures_.push_back(PRIAutoFailure(
                    request.streamID, request.pasid, request.prgIndex,
                    getCurrentTimestamp(), 0xFu, false));
            }
            return;
        }
    }

    // BUG-SEC8-SECURE-PRI-CPP fix: ARM §8.2 — PRI requests from Secure streams
    // must be discarded with ResponseCode=0b1111 (Failure).  The SMMU must not
    // enqueue PPRs from Secure streams into the PRI queue.
    if (request.securityState == SecurityState::Secure) {
        if (request.isLastRequest) {
            priAutoFailures_.push_back(PRIAutoFailure(
                request.streamID, request.pasid, request.prgIndex,
                getCurrentTimestamp(), 0xFu, false));
        }
        return;
    }

    // BUG-QA-5: §8.1 — check overflow-active state BEFORE checking queue capacity.
    // Overflow is "active" when PRIQ_PROD.OVFLG (bit 31) differs from
    // PRIQ_CONS.OVACKFLG (bit 31).  While active, new PPRs are inhibited even if
    // the software consumer has drained the queue and physical space is available.
    // Software must acknowledge the overflow (by writing OVACKFLG=OVFLG) before
    // new entries are accepted.
    {
        uint32_t priqProdVal = priqProd.load(std::memory_order_relaxed);
        uint32_t priqConsVal = priqCons.load(std::memory_order_relaxed);
        bool overflowActive = (((priqProdVal >> 31) & 1u) != ((priqConsVal >> 31) & 1u));
        if (overflowActive) {
            // Inhibit the new entry — treat as if the queue were still full.
            if (request.isLastRequest) {
                // Determine responseCode/includePASID the same way as below.
                bool hasPasid = false;
                {
                    size_t stripe = getStreamStripe(request.streamID);
                    std::lock_guard<std::mutex> stripeLock(streamLockStripes[stripe]);
                    auto it = streamMap.find(request.streamID);
                    if (it != streamMap.end() && it->second) {
                        StreamConfig cfg = it->second->getStreamConfiguration();
                        hasPasid = (cfg.s1cdMax > 0 && request.pasid != 0);
                    }
                }
                uint8_t rc       = hasPasid ? 0xFu : 0u;
                bool    inclPasid = false;
                PRIAutoFailure autoFail(request.streamID, request.pasid,
                                        request.prgIndex, getCurrentTimestamp(),
                                        rc, inclPasid);
                priAutoFailures_.push_back(autoFail);
            }
            return;
        }
    }

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
        // BUG-NEW-12 fix: §8.1 — an auto-failure PRG_RESPONSE is required ONLY
        // when the overflowing PPR has isLastRequest=true (it is the last PPR in
        // a Page Request Group and the device is waiting for a response).
        // When isLastRequest=false the PPR must be silently discarded with no
        // auto-response — the device does not expect a response for non-last PPRs.
        if (request.isLastRequest) {
            // BUG-QA-4: §8.1 — determine responseCode and includePASID.
            // Determine whether the PPR carries a valid PASID.  A PPR "has no PASID"
            // when the stream is not substream-capable (s1cdMax==0) or the PASID is 0
            // on a stream that has not been configured as substream-capable.
            // IDR3.PPS is 0 in this implementation (not set in getIDR3()), so for
            // PPRs that do carry a PASID the conservative fallback is Failure (0xF).
            uint8_t  responseCode = 0xFu;  // default: Failure
            bool     inclPasid    = false;
            // Check whether the overflowing PPR has a PASID by inspecting the
            // stream configuration (if the stream exists and is substream-capable).
            bool hasPasid = false;
            {
                size_t stripe = getStreamStripe(request.streamID);
                std::lock_guard<std::mutex> stripeLock(streamLockStripes[stripe]);
                auto it = streamMap.find(request.streamID);
                if (it != streamMap.end() && it->second) {
                    StreamConfig cfg = it->second->getStreamConfiguration();
                    // PPR has a PASID only if the stream is substream-capable
                    // (s1cdMax > 0) and the PASID is non-zero.
                    hasPasid = (cfg.s1cdMax > 0 && request.pasid != 0);
                }
            }
            if (!hasPasid) {
                // ARM §8.1: no PASID → responseCode=0 (Success), no PASID in response.
                responseCode = 0u;
                inclPasid    = false;
            }
            // else: PASID present, IDR3.PPS=0, no STE.PPAR field → conservative Failure.
            PRIAutoFailure autoFail(request.streamID, request.pasid,
                                    request.prgIndex, getCurrentTimestamp(),
                                    responseCode, inclPasid);
            priAutoFailures_.push_back(autoFail);
        }
        return;
    }

    PRIEntry timestampedRequest = request;
    timestampedRequest.timestamp = getCurrentTimestamp();
    priQueue.push_back(timestampedRequest);
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-08).
    // BUG-NEW-D fix: preserve OVFLG (bit 31) across the PRIQ PROD advance.
    {
        uint32_t oldProd = priqProd.load(std::memory_order_relaxed);
        uint32_t newProd = advanceQueueIndex(oldProd, priqLog2Size) | (oldProd & (1u << 31));
        priqProd.store(newProd, std::memory_order_release);
    }
    // §7.3.19 / FINDING-NEW-32: carry the request's security state, not a hardcoded NonSecure.
    // Only generate the E_PAGE_REQUEST event when the entry was actually enqueued.
    generateEvent(EventType::E_PAGE_REQUEST, request.streamID, request.pasid, request.requestedAddress, request.securityState);

    // BUG-NEW-9: §7.3.19 — populate E_PAGE_REQUEST permission fields on the event
    // just enqueued.  queueMutex is already held (recursive_mutex) so eventQueue is
    // stable.  The event may have been parked in stallPending_ if the queue was full,
    // but E_PAGE_REQUEST is a non-stall event so it goes to eventQueue.back() when
    // enqueued successfully (EVENTQEN=1 and queue not full).
    if ((cr0_.load(std::memory_order_acquire) & CR0_EVENTQEN) != 0u &&
        !eventQueue.empty() &&
        eventQueue.back().type == EventType::E_PAGE_REQUEST &&
        eventQueue.back().streamID == request.streamID &&
        eventQueue.back().pasid == request.pasid) {
        EventEntry& ev = eventQueue.back();
        const AccessType acc = request.accessType;
        // BUG-QA-6: §7.3.19 — use decomposed isPrivileged/canRead/canWrite/canExecute
        // helpers to set permission bits correctly.
        // Privileged access types: ReadPrivileged, WritePrivileged, ExecutePrivileged,
        // ReadWritePrivileged, ReadExecutePrivileged.
        // Non-privileged: Read, Write, Execute, ReadWrite, ReadExecute.
        bool isPriv =
            (acc == AccessType::ReadPrivileged     ||
             acc == AccessType::WritePrivileged     ||
             acc == AccessType::ExecutePrivileged   ||
             acc == AccessType::ReadWritePrivileged ||
             acc == AccessType::ReadExecutePrivileged);
        bool canRead =
            (acc == AccessType::Read               ||
             acc == AccessType::ReadWrite           ||
             acc == AccessType::ReadExecute         ||
             acc == AccessType::ReadPrivileged      ||
             acc == AccessType::ReadWritePrivileged ||
             acc == AccessType::ReadExecutePrivileged);
        bool canWrite =
            (acc == AccessType::Write              ||
             acc == AccessType::ReadWrite           ||
             acc == AccessType::WritePrivileged     ||
             acc == AccessType::ReadWritePrivileged);
        bool canExecute =
            (acc == AccessType::Execute            ||
             acc == AccessType::ReadExecute         ||
             acc == AccessType::ExecutePrivileged   ||
             acc == AccessType::ReadExecutePrivileged);
        ev.ur = (!isPriv && canRead);
        ev.uw = (!isPriv && canWrite);
        ev.ux = (!isPriv && canExecute);
        ev.pr = (isPriv  && canRead);
        ev.pw = (isPriv  && canWrite);
        ev.px = (isPriv  && canExecute);
        ev.span = 0u; // single page (default)
    }
}

void SMMU::processPRIQueue() {
    // §CT-33 / BUG-NEW-25 fix: Effective PRIQEN = CR0.PRIQEN AND CR0.SMMUEN.
    // When SMMUEN=0 the SMMU is globally disabled and PRI processing must also
    // be disabled, even if PRIQEN remains set after disable() (ARM §6.3.9).
    // BUG-CPP-NEW-1 fix: use load() for the atomic cr0_.
    {
        uint32_t cr0val = cr0_.load(std::memory_order_acquire);
        if (!(cr0val & CR0_PRIQEN) || !(cr0val & CR0_SMMUEN)) {
            return;
        }
    }

    // BUG-NEW-3 fix: ARM §3.5.1 — software is the producer of the Command queue;
    // the SMMU is the consumer.  processPRIQueue() must NOT submit CMD_PRI_RESP
    // commands to the SMMU's own command queue — that is architecturally inverted.
    // ARM §8.2: the auto-PRG-Response path refers to direct bus-level PCIe
    // responses, not CMD_PRI_RESP commands enqueued back into the command queue.
    //
    // BUG-NEW-4 fix: ARM §8.1 — PRIQ_CONS is software-written; the SMMU hardware
    // must never write it.  Only software (via CMD_PRI_RESP command processing)
    // may advance priqCons.  processPRIQueue() must not touch priqCons at all.
    //
    // BUG-NEW-19 fix: ARM §8.1 — while the queue is in its normal (non-overflow)
    // state, software consumes Last=1 (PRG-terminating) entries via CMD_PRI_RESP.
    // processPRIQueue() must NOT drain Last=1 entries because CMD_PRI_RESP must
    // be able to match the head entry.  Non-Last (intermediate) PPRs are not
    // individually matched by CMD_PRI_RESP and may be retired by processPRIQueue()
    // to reclaim queue space (ARM §8.1 PRG sequencing).
    //
    // BUG-QA-5 fix: ARM §8.1 — when PRIQ OVERFLOW is active (PRIQ_PROD.OVFLG bit
    // differs from PRIQ_CONS.OVACKFLG bit), the queue is "stuck" and cannot accept
    // new PPRs until software clears it.  In this overflow state, processPRIQueue()
    // drains ALL entries from priQueue (both Last and non-Last) so that the queue
    // is empty and software can then write PRIQ_CONS.OVACKFLG to acknowledge the
    // overflow and resume normal operation.  priqCons is intentionally NOT advanced
    // here (software writes it).
    {
        std::lock_guard<std::recursive_mutex> lock(queueMutex);
        // BUG-AUDIT-148-CPP fix: §3.1 line 1220 — no-op when PRI not supported.
        if (!priSupported_.load(std::memory_order_acquire)) {
            return;
        }
        uint32_t priqProdVal = priqProd.load(std::memory_order_relaxed);
        uint32_t priqConsVal = priqCons.load(std::memory_order_relaxed);
        bool overflowActive = (((priqProdVal >> 31) & 1u) != ((priqConsVal >> 31) & 1u));
        if (overflowActive) {
            // Drain ALL entries (Last and non-Last).  priqCons is NOT advanced —
            // that is software's responsibility via PRIQ_CONS.OVACKFLG write.
            while (!priQueue.empty()) {
                priQueue.pop_front();
            }
        } else {
            // Non-overflow: drain only intermediate (non-Last) PPRs.
            // Last=1 PPRs must remain at the head so CMD_PRI_RESP can match
            // them (ARM §4.5.2).  priqCons is NOT advanced here.
            while (!priQueue.empty() && !priQueue.front().isLastRequest) {
                priQueue.pop_front();
            }
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
            
        case CommandType::CFGI_CD: {
            // ARM §4.3.3: "This command raises CERROR_ILL when stage 1 is not implemented."
            // BUG-AUDIT-NEW-02 fix: §4.3.3 line 5362 clarified at spec line 6605:
            // "If stage 1 is not implemented (SMMU_IDR0.S1P == 0)."
            // The guard is the SMMU-global IDR0.S1P capability, not per-stream config.
            // A stream that uses bypass/stage-2 but the SMMU supports stage-1 globally
            // → silent no-op (the stream has no CD; invalidation is a no-op).
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // Maps to PASID-scoped TLB invalidation in SW model.
            invalidatePASIDCache(command.streamID, command.pasid);
            break;
        }

        case CommandType::CFGI_CD_ALL: {
            // ARM §4.3.4: "This command raises CERROR_ILL when stage 1 is not implemented."
            // BUG-AUDIT-NEW-02 fix: same global IDR0.S1P guard as CFGI_CD.
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // Maps to stream-wide TLB invalidation in SW model.
            invalidateStreamCache(command.streamID);
            break;
        }

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

        default:
            // BUG-AUDIT-72 fix: §4.1.7/§7.1 — unrecognized invalidation command must signal
            // CERROR_ILL + GERROR.CMDQ_ERR, not a stream-level C_BAD_STE event.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
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
    // Granule size in bytes per ARM §4.4.1.1 table:
    //   tg=0b00 (0) → not a range operation (treated as 4KB fallback)
    //   tg=0b01 (1) → 4KB  (4096 bytes)
    //   tg=0b10 (2) → 16KB (16384 bytes)
    //   tg=0b11 (3) → 64KB (65536 bytes)
    // ARM §4.4.1.1: Range covers (NUM+1) * 2^SCALE granules starting at iova.
    // SCALE is used directly as the exponent — NOT multiplied by 5.
    // (The 5*SCALE encoding is a PE TLBI instruction artifact, not SMMU command encoding.)
    // BUG-AUDIT-1 fix: replace '5u * effectiveScale' with effectiveScale directly.
    // NEW-BUG-1 fix: correct TG encoding per §4.4.1.1 — tg=1→4KB, tg=2→16KB, tg=3→64KB.
    // Max SCALE=39 → max shift=39 bits, safely within uint64_t (no UB).
    auto computeRILRangeEnd = [](IOVA start, uint8_t tg_, uint8_t num_, uint8_t scale_) -> IOVA {
        uint64_t granule;
        switch (tg_) {
            case 1:  granule = 4u   * 1024u; break;  // §4.4.1.1: TG=0b01 = 4KB
            case 2:  granule = 16u  * 1024u; break;  // §4.4.1.1: TG=0b10 = 16KB
            case 3:  granule = 64u  * 1024u; break;  // §4.4.1.1: TG=0b11 = 64KB
            default: granule = 4u   * 1024u; break;  // tg=0: treat as 4KB (non-range fallback)
        }
        // ARM §4.4.1.1: effective SCALE range is 0-39; values > 39 treated as 39.
        // Using SCALE directly as the shift: max shift = 39, well within uint64_t.
        uint32_t effectiveScale = (scale_ > 39u) ? 39u : static_cast<uint32_t>(scale_);
        uint64_t blocks = static_cast<uint64_t>(num_) + 1u;
        uint64_t rangeBytes = blocks * (static_cast<uint64_t>(1u) << effectiveScale) * granule;
        if (rangeBytes == 0 || start > UINT64_MAX - rangeBytes + 1u) {
            return UINT64_MAX; // saturate when range overflows address space
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
            // QA-AUDIT-FIX-2: ARM §4.4.2.1 CMD_TLBI_NH_ALL — Non-Hyp all, VMID-scoped.
            // Only evicts EL1_EL0 entries belonging to the command's VMID operand.
            // Preserves EL2, EL2_E2H, and EL1_EL0 entries of other VMIDs.
            // BUG-AUDIT-169-CPP fix: apply VMW wildcard mask (§3.17.6).
            if (tlbCache) {
                tlbCache->invalidateNonHypEntriesByVMID(vmid, getVmidMask());
            }
            break;

        case CommandType::TLBI_NSNH_ALL:
            // BUG-QA-14 fix: ARM §4.4.4.1 CMD_TLBI_NSNH_ALL — Non-Secure Non-Hyp all.
            // Evicts ALL EL1_EL0 entries across all VMIDs (VMID-agnostic per §4.4.4.1).
            if (tlbCache) {
                tlbCache->invalidateNonHypEntries();
            }
            break;

        case CommandType::TLBI_NH_VA:
            // BUG-NEW-37 fix: ARM §4.4.2.3 CMD_TLBI_NH_VA — must be VMID-scoped.
            // The previous calls to invalidateByVARange / invalidateByVAAndASID ignored
            // VMID and could evict entries from other VMIDs sharing the same VA+ASID.
            // BUG-AUDIT-169-CPP fix: apply VMW wildcard mask (§3.17.6).
            if (ril) {
                tlbCache->invalidateByVMIDAndVARange(vmid,
                    iova, computeRILRangeEnd(iova, tg, num, scale), asid, getVmidMask());
            } else {
                tlbCache->invalidateByVMIDAndVAAndASID(vmid, iova, asid, getVmidMask());
            }
            break;

        case CommandType::TLBI_NH_VAA:
            // BUG-NEW-37 fix: ARM §4.4.2.4 CMD_TLBI_NH_VAA — must be VMID-scoped.
            // The previous calls to invalidateByVA (in both RIL and non-RIL paths)
            // ignored VMID and could evict entries from other VMIDs sharing the same VA.
            // BUG-AUDIT-169-CPP fix: apply VMW wildcard mask (§3.17.6).
            if (ril) {
                // BUG-AUDIT-34 fix: use TG-derived granule size per ARM §4.4.1.1
                uint64_t granuleSize;
                switch (tg) {
                    case 2:  granuleSize = 16384u; break;
                    case 3:  granuleSize = 65536u; break;
                    default: granuleSize =  4096u; break;
                }
                IOVA rangeEnd = computeRILRangeEnd(iova, tg, num, scale);
                IOVA cur = iova & ~(granuleSize - 1u);
                while (cur <= rangeEnd) {
                    tlbCache->invalidateByVMIDAndVA(vmid, cur, getVmidMask());
                    if (cur > UINT64_MAX - granuleSize) break;
                    cur += granuleSize;
                }
            } else {
                tlbCache->invalidateByVMIDAndVA(vmid, iova, getVmidMask());
            }
            break;

        case CommandType::TLBI_NH_ASID:
            // BUG-CPP-2 fix: ARM §4.4.2.2 CMD_TLBI_NH_ASID — NS/Realm queue — invalidate
            // by VMID AND ASID (joint match).  Previously called invalidateByASID(asid)
            // which ignored VMID, over-invalidating entries from other VMIDs that share
            // the same ASID value.
            // BUG-AUDIT-169-CPP fix: apply VMW wildcard mask (§3.17.6).
            tlbCache->invalidateByVMIDAndASID(vmid, asid, getVmidMask());
            break;

        case CommandType::TLBI_EL2_ALL:
            // BUG-NEW-18 fix: ARM §4.4.2.7 CMD_TLBI_EL2_ALL — evict only
            // EL2 and EL2_E2H tagged TLB entries; preserve EL1_EL0 entries.
            // The previous call to invalidateTranslationCache() over-invalidated
            // by evicting all entries regardless of StreamWorld tag.
            tlbCache->invalidateEL2Entries();
            break;

        case CommandType::TLBI_EL2_VA:
            // BUG-NEW-E fix: ARM §4.4.2.8 CMD_TLBI_EL2_VA — evict only EL2/EL2_E2H
            // entries by VA+ASID.  Previous calls to invalidateByVARange / invalidateByVAAndASID
            // over-invalidated by evicting ALL entries at that VA/ASID regardless of StreamWorld,
            // including EL1_EL0 entries that must be preserved.
            if (ril) {
                tlbCache->invalidateEL2ByVARange(iova, computeRILRangeEnd(iova, tg, num, scale), asid);
            } else {
                tlbCache->invalidateEL2ByVAAndASID(iova, asid);
            }
            break;

        case CommandType::TLBI_EL2_VAA:
            // BUG-NEW-E fix: ARM §4.4.2.9 CMD_TLBI_EL2_VAA — evict only EL2/EL2_E2H
            // entries by VA (any ASID).  Previous calls to invalidateByVA over-invalidated
            // by evicting ALL entries at that VA regardless of StreamWorld.
            if (ril) {
                // BUG-AUDIT-34 fix: use TG-derived granule size per ARM §4.4.1.1
                uint64_t granuleSize;
                switch (tg) {
                    case 2:  granuleSize = 16384u; break;
                    case 3:  granuleSize = 65536u; break;
                    default: granuleSize =  4096u; break;
                }
                IOVA rangeEnd = computeRILRangeEnd(iova, tg, num, scale);
                IOVA cur = iova & ~(granuleSize - 1u);
                while (cur <= rangeEnd) {
                    tlbCache->invalidateEL2ByVA(cur);
                    if (cur > UINT64_MAX - granuleSize) break;
                    cur += granuleSize;
                }
            } else {
                tlbCache->invalidateEL2ByVA(iova);
            }
            break;

        case CommandType::TLBI_EL2_ASID:
            // BUG-NEW-D fix: ARM §4.4.2.10 CMD_TLBI_EL2_ASID — only EL2_E2H entries
            // with matching ASID should be evicted.  Previous call to invalidateByASID()
            // over-invalidated by evicting ALL entries with that ASID regardless of
            // StreamWorld, including EL1_EL0 entries that must be preserved.
            tlbCache->invalidateEL2E2HByASID(asid);
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

        // BUG-NEW-33 fix: TLBI_EL3_ALL, TLBI_EL3_VA, TLBI_S_EL2_ALL, TLBI_S_EL2_VA,
        // TLBI_S_EL2_VAA, TLBI_S_EL2_ASID, TLBI_S_S12_VMALL, TLBI_S_S2_IPA, and
        // TLBI_SNH_ALL are all intercepted by processCommand() with CERROR_ILL and
        // never reach this function.  Their former cases have been removed to eliminate
        // the dead TLB mutation paths that were a maintenance hazard.

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

        case CommandType::CFGI_CD: {
            // ARM §4.3.3: "This command raises CERROR_ILL when stage 1 is not implemented."
            // BUG-AUDIT-NEW-02 fix: §4.3.3 line 5362 clarified at spec line 6605:
            // "If stage 1 is not implemented (SMMU_IDR0.S1P == 0)."
            // The guard is the SMMU-global IDR0.S1P capability, not per-stream config.
            // A stream that uses bypass/stage-2 but the SMMU supports stage-1 globally
            // → silent no-op (the stream has no CD; invalidation is a no-op).
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // Maps to PASID-scoped TLB invalidation in SW model.
            invalidatePASIDCache(command.streamID, command.pasid);
            break;
        }

        case CommandType::CFGI_CD_ALL: {
            // ARM §4.3.4: "This command raises CERROR_ILL when stage 1 is not implemented."
            // BUG-AUDIT-NEW-02 fix: same global IDR0.S1P guard as CFGI_CD.
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // Maps to stream-wide TLB invalidation in SW model.
            invalidateStreamCache(command.streamID);
            break;
        }

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

        default:
            // BUG-AUDIT-72 fix: §4.1.7/§7.1 — unrecognized invalidation command must signal
            // CERROR_ILL + GERROR.CMDQ_ERR, not a stream-level C_BAD_STE event.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
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
            // BUG-NEW-25 fix: ARM §4.2.1/§4.1.6 — SSec=1 on NS queue is ILLEGAL → CERROR_ILL.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // No prefetch side effects in SW model.
            break;

        case CommandType::PREFETCH_ADDR:
            // BUG-NEW-25 fix: ARM §4.2.2/§4.1.6 — SSec=1 on NS queue is ILLEGAL → CERROR_ILL.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // No prefetch side effects in SW model.
            break;

        case CommandType::CFGI_STE:
            // CONF-GAP-2 fix / ARM §4.3.1: CMD_CFGI_STE is a pure cache invalidation.
            // Unknown StreamID → silent no-op (nothing cached to evict).
            // C_BAD_STREAMID is a *transaction-path* error (§7.3.3), not raised here.
            // BUG-NEW-15 fix: ARM §4.1.6 — any NS-queue command with SSec=1 is illegal.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_EL2_ALL:
            // NEW-BUG-A fix: ARM §4.4.2.7 — "If SMMU_IDR0.Hyp==0, CERROR_ILL."
            // CMD_TLBI_EL2_ALL requires hypervisor extension support (IDR0.Hyp=1).
            // Must not be in the shared fall-through group so the guard can fire.
            if ((getIDR0() & (1u << 9u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::CFGI_ALL:
        case CommandType::CFGI_CD:
        case CommandType::CFGI_CD_ALL:
            // BUG-NEW-15 fix: ARM §4.1.6 — SSec=1 on the NS command queue is illegal.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_NH_ALL:
            // BUG-AUDIT-NEW-03 fix: ARM §4.4.2.1 — NH_ALL raises CERROR_ILL on a
            // stage-1-absent SMMU (IDR0.S1P==0); no stage-1 entries to invalidate.
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding;
            // setting RES0 bits is CONSTRAINED UNPREDICTABLE, not CERROR_ILL.
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_NH_ASID:
            // BUG-AUDIT-NEW-03 fix: ARM §4.4.2.2 — NH_ASID raises CERROR_ILL on a
            // stage-1-absent SMMU (IDR0.S1P==0).
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding;
            // setting RES0 bits is CONSTRAINED UNPREDICTABLE, not CERROR_ILL.
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_NH_VA:
            // BUG-AUDIT-NEW-03 fix: ARM §4.4.2.3 — NH_VA raises CERROR_ILL on a
            // stage-1-absent SMMU (IDR0.S1P==0).
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding;
            // setting RES0 bits is CONSTRAINED UNPREDICTABLE, not CERROR_ILL.
            // BUG-NEW-B fix: ARM §4.4 — Reserved RIL combination when TG!=0 AND
            // NUM==0 AND SCALE==0 AND TTL==0b00 → CERROR_ILL.  Applies to ALL
            // non-zero TG values (4KB=TG1, 16KB=TG2, 64KB=TG3); was incorrectly
            // only checking TG==1 (4KB), missing TG=2 (16KB) and TG=3 (64KB).
            if (command.ril && command.tg != 0u && command.num == 0u
                    && command.scale == 0u && command.ttl == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_NH_VAA:
            // BUG-AUDIT-NEW-03 fix: ARM §4.4.2.4 — NH_VAA raises CERROR_ILL on a
            // stage-1-absent SMMU (IDR0.S1P==0).
            if ((getIDR0() & (1u << 1u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding;
            // setting RES0 bits is CONSTRAINED UNPREDICTABLE, not CERROR_ILL.
            // BUG-NEW-B fix: ARM §4.4 — Reserved RIL combination (TG!=0, NUM==0,
            // SCALE==0, TTL==0b00) → CERROR_ILL. See TLBI_NH_VA case for rationale.
            if (command.ril && command.tg != 0u && command.num == 0u
                    && command.scale == 0u && command.ttl == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_S12_VMALL:
            // BUG-NEW-39 fix: ARM §4.4.3.1 — "If SMMU_IDR0.S2P==0, CERROR_ILL."
            // Stage-2 VMALL requires stage-2 support (IDR0.S2P=1).
            if ((getIDR0() & (1u << 0u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding;
            // setting RES0 bits is CONSTRAINED UNPREDICTABLE, not CERROR_ILL.
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_S2_IPA:
            // BUG-NEW-39 fix: ARM §4.4.3.2 — "If SMMU_IDR0.S2P==0, CERROR_ILL."
            // Stage-2 IPA invalidation requires stage-2 support (IDR0.S2P=1).
            if ((getIDR0() & (1u << 0u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-AUDIT-NEW-01 fix: ARM §4.4.1.1 — Reserved RIL combination (TG!=0,
            // NUM==0, SCALE==0, TTL==0) → CERROR_ILL. Applies to all address-based
            // invalidation commands including S2_IPA (§4.4.1.1 "for VA and IPA").
            if (command.ril && command.tg != 0u && command.num == 0u
                    && command.scale == 0u && command.ttl == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding.
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_NSNH_ALL:
            // BUG-D fix: ARM §4.1.6 — SSec is RES0 in this command's encoding;
            // setting RES0 bits is CONSTRAINED UNPREDICTABLE, not CERROR_ILL.
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::ATC_INV:
            // BUG-NEW-38 fix: ARM §4.1.6 — SSec=1 on the NS command queue is illegal.
            // The SSec guard MUST come before the queueLock.unlock() call inside
            // executeInvalidationCommandLocked to avoid releasing the lock under an error.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_EL2_VA:
            // BUG-NEW-24 fix: ARM §4.4.2.8 — "If SMMU_IDR0.Hyp==0, CERROR_ILL."
            // Same pattern as TLBI_EL2_ALL (§4.4.2.7): hypervisor extension required.
            if ((getIDR0() & (1u << 9u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-B fix: ARM §4.4 — Reserved RIL combination (TG!=0, NUM==0,
            // SCALE==0, TTL==0b00) → CERROR_ILL. See TLBI_NH_VA case for rationale.
            if (command.ril && command.tg != 0u && command.num == 0u
                    && command.scale == 0u && command.ttl == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_EL2_VAA:
            // BUG-NEW-24 fix: ARM §4.4.2.9 — "If SMMU_IDR0.Hyp==0, CERROR_ILL."
            // Same pattern as TLBI_EL2_ALL (§4.4.2.7): hypervisor extension required.
            if ((getIDR0() & (1u << 9u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-B fix: ARM §4.4 — Reserved RIL combination (TG!=0, NUM==0,
            // SCALE==0, TTL==0b00) → CERROR_ILL. See TLBI_NH_VA case for rationale.
            if (command.ril && command.tg != 0u && command.num == 0u
                    && command.scale == 0u && command.ttl == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::TLBI_EL2_ASID:
            // BUG-NEW-24 fix: ARM §4.4.2.10 — "If SMMU_IDR0.Hyp==0, CERROR_ILL."
            // Same pattern as TLBI_EL2_ALL (§4.4.2.7): hypervisor extension required.
            if ((getIDR0() & (1u << 9u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            executeInvalidationCommandLocked(command, queueLock);
            break;

        case CommandType::PRI_RESP: {
            // BUG-AUDIT-03 fix: ARM §4.5.2 lines 6075-6079 — CMD_PRI_RESP must be
            // silently ignored when SMMU_CR0.SMMUEN==0.  No error, no side-effects.
            if ((cr0_.load(std::memory_order_acquire) & CR0_SMMUEN) == 0u) {
                break;
            }
            // BUG-NEW-A fix: ARM §4.5.2 — Resp==0b11 is Reserved/ILLEGAL → CERROR_ILL.
            // The 2-bit Resp field is encoded in bits[1:0] of the flags field.
            // Any Resp value of 0b11 must raise CERROR_ILL and GERROR_CMDQ_ERR before
            // any further processing of the PRI response.
            if ((command.flags & 0x03u) == 0x03u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-G fix: ARM §4.5.2 — CMD_PRI_RESP is ILLEGAL when IDR0.PRI==0.
            if ((getIDR0() & (1u << 16u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-13 fix: §3.5.1/§4.5.2 — CMD_PRI_RESP must only retire the
            // HEAD entry of the PRI queue and must match on streamID, prgIndex, AND
            // PASID.  A linear scan of all queue entries was incorrect: it could
            // retire an interior entry (skipping the head) and did not check PASID.
            //
            // Correct behaviour per ARM §4.5.2:
            //   1. Check only the HEAD of the queue (priQueue.front()).
            //   2. Match on streamID, prgIndex, AND pasid.
            //   3. If the head matches: retire it (pop_front, advance PRIQ_CONS).
            //   4. If the head does not match: no-op (software error per §4.5.2).
            std::lock_guard<std::recursive_mutex> priLock(queueMutex);
            if (!priQueue.empty()) {
                const PRIEntry& head = priQueue.front();
                if (head.streamID == command.streamID &&
                    head.prgIndex  == command.prgIndex  &&
                    head.pasid     == command.pasid) {
                    priQueue.pop_front();
                    // BUG-CPP-3 fix: §8.1 PRIQ_CONS.OVACKFLG (bit 31) is software-written;
                    // preserve bit 31 via RMW when advancing the consumer index here.
                    {
                        uint32_t oldCons = priqCons.load(std::memory_order_relaxed);
                        uint32_t newRD = advanceQueueIndex(oldCons & ~(1u << 31), priqLog2Size);
                        priqCons.store((oldCons & (1u << 31)) | newRD, std::memory_order_release);
                    }
                }
                // If head does not match (wrong streamID/prgIndex/PASID): no-op.
                // ARM §4.5.2: it is a software error to respond out-of-order.
            }
            // If queue empty: no-op (software sent a spurious CMD_PRI_RESP).
            break;
        }

        case CommandType::RESUME: {
            // BUG-NEW-15 fix: §4.1.6 — SSec=1 on the NS command queue is ILLEGAL.
            // Any CMD_RESUME with ssec=true must raise CERROR_ILL.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-11 fix: §4.7.1 — when IDR0.STALL_MODEL==0b01 (terminate-only),
            // CMD_RESUME is not supported and must raise CERROR_ILL (ARM §4.7.1).
            if (stallModel_.load(std::memory_order_acquire) == 0x01u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // ARM §4.6: CMD_RESUME — resume or abort a stalled transaction.
            // Three outcomes based on Ac (action) and Ab (abort) bits:
            //   Ac=1:           Retry  — transaction may be retried.
            //   Ac=0, Ab=0:     Terminate successfully (RAZ/WI from device perspective).
            //   Ac=0, Ab=1:     Abort with bus error.
            // Per §4.6: only retire the record if its StreamID matches.
            // CONF-GAP-24: Record the outcome BEFORE erasing the stall record so
            // software can observe which disposition was chosen via getResumeOutcome().
            {
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
            }
            break;
        }

        case CommandType::STALL_TERM: {
            // BUG-NEW-15 fix: §4.1.6 — SSec=1 on the NS command queue is ILLEGAL.
            // Any CMD_STALL_TERM with ssec=true must raise CERROR_ILL.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-11 fix: §4.7.2 — when IDR0.STALL_MODEL==0b01 (terminate-only),
            // CMD_STALL_TERM is not supported and must raise CERROR_ILL (ARM §4.7.2).
            if (stallModel_.load(std::memory_order_acquire) == 0x01u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // ARM §4.7: CMD_STALL_TERM — abort ALL stalled transactions for StreamID.
            // Removes every StallRecord whose streamID matches command.streamID.
            {
                std::lock_guard<std::mutex> slock(stallQueueMutex_);
                for (auto it = stallQueue_.begin(); it != stallQueue_.end(); ) {
                    if (it->second.streamID == command.streamID) {
                        it = stallQueue_.erase(it);
                    } else {
                        ++it;
                    }
                }
            }
            break;
        }

        case CommandType::SYNC:
            // BUG-NEW-23 fix: CMD_SYNC validation and completion fully handled
            // here in processCommand() instead of being split across
            // processCommandQueue().  processCommandQueue() detects the CS=3
            // error path via the GERROR_CMDQ_ERR check after this call returns.
            //
            // §4.8 / FINDING-NEW-33: CS=0b11 is Reserved → CERROR_ILL.
            // CONF-GAP-17: Write CERROR_ILL to CMDQ_CONS.ERR before signalling GERROR.
            // BUG-CPP-5 / BUG-NEW-CPP-1 fixes apply here as well.
            if (command.cs == 3u) {  // 0b11 = 3: Reserved per §4.8
                FaultRecord illFault;
                illFault.streamID = command.streamID;
                illFault.pasid    = command.pasid;
                illFault.faultType = FaultType::ConfigurationCacheFault;
                recordFault(illFault);
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                // processCommandQueue() will detect GERROR_CMDQ_ERR and break
                // without advancing CONS.RD, keeping it at this command.
                break;
            }
            // §4.8 / FINDING-NEW-27: CS=0b00 (SIG_NONE) → no completion signal.
            // CONF-GAP-18: Record the signal type for CS=1 (IRQ) and CS=2 (SEV).
            // BUG-QA-7 fix: CS=2 is SIG_SEV (PE-level wakeup), NOT SIG_MSI.
            if (command.cs == 1u) {
                cmdSyncLastSig_.store(static_cast<uint8_t>(CmdSyncSignalType::Irq), std::memory_order_release);
                // BUG-NEW-2 fix: completion event streamID=0 (RES0 operand in CMD_SYNC).
                // BUG-NEW-26 fix: CMD_SYNC has no architectural StreamID operand (§4.7.3/§4.8).
                // The completion event security state must always be NonSecure — the command
                // queue operates in the NS domain and the stream_id field is meaningless here.
                generateEvent(EventType::COMMAND_SYNC_COMPLETION, 0u, command.pasid,
                              command.startAddress, SecurityState::NonSecure);
            } else if (command.cs == 2u) {
                // BUG-AUDIT-65 fix: §4.7.3 — CS=2 (SIG_SEV) only active when IDR0.SEV=1.
                // When sevSupported_=false, treat SIG_SEV as SIG_NONE (no signal, no event).
                if (sevSupported_.load(std::memory_order_acquire)) {
                    cmdSyncLastSig_.store(static_cast<uint8_t>(CmdSyncSignalType::Sev), std::memory_order_release);
                    generateEvent(EventType::COMMAND_SYNC_COMPLETION, 0u, command.pasid,
                                  command.startAddress, SecurityState::NonSecure);
                }
            }
            // BUG-CPP-05 fix: ARM §4.8 CMD_SYNC is a barrier, not a stop.
            // Fall through (no break) so processCommandQueue() advances CONS.RD.
            break;

        // §4.1.1 / CT-30: Additional spec-defined command opcodes
        case CommandType::CFGI_VMS_PIDM:
            // BUG-NEW-32 fix: ARM §4.1.6 — SSec=1 on the NS command queue is ILLEGAL.
            if (command.ssec) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            // BUG-NEW-32 fix: ARM §4.3.5 — CMD_CFGI_VMS_PIDM requires IDR3.MPAM==1.
            // BUG-G fix: §6.3.4 — IDR3.MPAM is at bit [7] not bit [6]. Bit [6] is RES0.
            // This model reports IDR3.MPAM=0, so this command is always CERROR_ILL.
            if ((getIDR3() & (1u << 7u)) == 0u) {
                writeCmdqConsErr(CERROR_ILL);
                signalGerror(GERROR_CMDQ_ERR);
                break;
            }
            invalidatePASIDCache(command.streamID, command.pasid);
            break;

        case CommandType::TLBI_EL3_ALL:
        case CommandType::TLBI_EL3_VA:
            // BUG-QA-9 fix: ARM §4.4.2.5 — CMD_TLBI_EL3_ALL causes CERROR_ILL:
            //   (1) when used on the Non-secure Command queue, OR
            //   (2) when IDR0.RME_IMPL==1 (EL3 StreamWorld unsupported per RME extension).
            // ARM §4.4.2.6 — CMD_TLBI_EL3_VA carries the same two conditions.
            // This model implements only the NS queue and reports RME_IMPL=0, so condition
            // (1) is always met. CERROR_ILL unconditional.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_SNH_ALL:
            // BUG-NEW-28 fix: ARM §4.4.4.2 — "This command causes CERROR_ILL when used
            // on the Non-secure Command queue." This model implements only the NS command
            // queue, so CERROR_ILL always fires.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_S_EL2_ALL:
            // BUG-NEW-29 fix: ARM §4.4.2.11 — CERROR_ILL on NS queue.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_S_EL2_VA:
            // BUG-NEW-29 fix: ARM §4.4.2.12 — CERROR_ILL on NS queue.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_S_EL2_VAA:
            // BUG-NEW-29 fix: ARM §4.4.2.13 — CERROR_ILL on NS queue.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_S_EL2_ASID:
            // BUG-NEW-29 fix: ARM §4.4.2.14 — CERROR_ILL on NS queue.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_S_S2_IPA:
            // BUG-NEW-30 fix: ARM §4.4.3.3 — CERROR_ILL on NS queue.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
            break;

        case CommandType::TLBI_S_S12_VMALL:
            // BUG-NEW-30 fix: ARM §4.4.3.4 — CERROR_ILL on NS queue.
            writeCmdqConsErr(CERROR_ILL);
            signalGerror(GERROR_CMDQ_ERR);
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
    // BUG-AUDIT-60/67 fix: ARM IHI0070G.b §6.3.12 — CR2 is RO when SMMUEN=1 in CR0 or CR0ACK.
    // SMMUv3.2 mandates that writes to CR2 while SMMUEN=1 are silently ignored.
    if (((cr0_.load(std::memory_order_acquire) | cr0ack_.load(std::memory_order_acquire)) & CR0_SMMUEN) != 0u) {
        return;
    }
    const uint32_t oldCR2 = cr2_.load(std::memory_order_acquire);
    cr2_.store(value, std::memory_order_release);
    // BUG-AUDIT-168 fix: ARM §3.17.5 — a change to CR2.E2H requires invalidation
    // of all EL2 and EL2-E2H TLB entries.  The OS is responsible for issuing an
    // ALLE2 (or equivalent) before toggling E2H, but the simulation model must
    // also defensively flush stale EL2/EL2_E2H entries when E2H transitions to
    // correctly model the required behavior across disable/enable cycles.
    if (((oldCR2 ^ value) & CR2_E2H) != 0u) {
        if (tlbCache) {
            tlbCache->invalidateEL2Entries();
        }
    }
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
    // BUG-AUDIT-66/68 fix: ARM IHI0070G.b §6.3.11 — TABLE_* and QUEUE_* have independent guards.
    // TABLE_* fields (bits[11:6]): RO when SMMUEN=1 in CR0 or CR0ACK.
    // QUEUE_* fields (bits[5:0]):  RO when any queue-enable is 1 in CR0 or CR0ACK.
    const uint32_t cr0  = cr0_.load(std::memory_order_acquire);
    const uint32_t ack  = cr0ack_.load(std::memory_order_acquire);
    const uint32_t queueGuard = CR0_CMDQEN | CR0_EVENTQEN | CR0_PRIQEN;
    const uint32_t tableMask  = 0xFC0u;  // CR1 bits[11:6]: TABLE_SH/OC/IC
    const uint32_t queueMask  = 0x03Fu;  // CR1 bits[5:0]:  QUEUE_SH/OC/IC
    uint32_t current = cr1_.load(std::memory_order_acquire);
    uint32_t newVal  = current;
    if (((cr0 | ack) & CR0_SMMUEN) == 0u) {
        newVal = (newVal & ~tableMask) | (value & tableMask);
    }
    if (((cr0 | ack) & queueGuard) == 0u) {
        newVal = (newVal & ~queueMask) | (value & queueMask);
    }
    cr1_.store(newVal, std::memory_order_release);
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
    // BUG-AUDIT-63/67 fix: ARM IHI0070G.b §6.3.24/§6.3.25 line 13807 — STRTAB_BASE_CFG
    // is RO when SMMUEN=1 in CR0 or CR0ACK. Writes while SMMUEN=1 must be silently ignored.
    if (((cr0_.load(std::memory_order_acquire) | cr0ack_.load(std::memory_order_acquire)) & CR0_SMMUEN) != 0u) {
        return;
    }
    strtabFmt_.store(static_cast<uint8_t>(fmt), std::memory_order_release);
}

StreamTableFormat SMMU::getStrtabFormat() const {
    return static_cast<StreamTableFormat>(strtabFmt_.load(std::memory_order_acquire));
}

void SMMU::setStrtabSplit(uint8_t split) {
    // BUG-AUDIT-63/67 fix: ARM IHI0070G.b §6.3.25 line 13807 — STRTAB_BASE_CFG is RO
    // when SMMUEN=1 in CR0 or CR0ACK. Writes while SMMUEN=1 must be silently ignored.
    // Guard placed BEFORE lock acquisition to avoid acquiring queueMutex unnecessarily.
    if (((cr0_.load(std::memory_order_acquire) | cr0ack_.load(std::memory_order_acquire)) & CR0_SMMUEN) != 0u) {
        return;
    }
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
// NOTE-3 fix: ARM §6.3.28 CMDQ_CONS.ERR = bits[30:24] (7 bits).
// Correct mask is 0x7F (7 bits), not 0xFF (8 bits).
uint32_t SMMU::getCmdqConsErr() const {
    return (cmdqCons.load(std::memory_order_acquire) >> CMDQ_CONS_ERR_SHIFT) & 0x7Fu;
}

void SMMU::writeCmdqConsErr(uint32_t errCode) {
    // Atomic CAS loop: update only the ERR[30:24] field without touching other bits.
    // NOTE-3 fix: use 0x7F (7-bit) mask — bits[30:24] only, never bit 31.
    uint32_t expected = cmdqCons.load(std::memory_order_relaxed);
    uint32_t desired;
    do {
        desired = (expected & ~(0x7Fu << CMDQ_CONS_ERR_SHIFT)) |
                  ((errCode & 0x7Fu) << CMDQ_CONS_ERR_SHIFT);
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
    // BUG-AUDIT-63/67 fix: ARM IHI0070G.b §6.3.25 line 13807 — STRTAB_BASE_CFG is RO
    // when SMMUEN=1 in CR0 or CR0ACK. Writes while SMMUEN=1 must be silently ignored.
    // Guard placed BEFORE lock acquisition to avoid acquiring queueMutex unnecessarily.
    if (((cr0_.load(std::memory_order_acquire) | cr0ack_.load(std::memory_order_acquire)) & CR0_SMMUEN) != 0u) {
        return;
    }
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
    // BUG-AUDIT-71 fix: PRIQEN is RES0 when PRI is not supported; only set it
    // when priSupported_ is true (ARM IHI0070G.b §6.3.9, §3.16).  When PRI is
    // not supported we must also clear any pre-existing PRIQEN bit that may
    // have been left in cr0_ by a prior enable() call before PRI was disabled,
    // since fetch_or alone can never clear a bit already set in cr0_.
    const bool priSup = priSupported_.load(std::memory_order_acquire);
    const uint32_t priqenBit = priSup ? CR0_PRIQEN : 0u;
    cr0_.fetch_or(CR0_SMMUEN | priqenBit | CR0_EVENTQEN | CR0_CMDQEN, std::memory_order_acq_rel);
    if (!priSup) {
        cr0_.fetch_and(~static_cast<uint32_t>(CR0_PRIQEN), std::memory_order_acq_rel);
    }
    uint32_t newVal = cr0_.load(std::memory_order_acquire);
    smmuen_.store(true, std::memory_order_release);
    // CONF-GAP-9: sync CR0ACK to match updated CR0
    cr0ack_.store(newVal, std::memory_order_release);
    // BUG-DORMANT-CPP fix: Clear STATUSR.DORMANT (bit 0) when SMMU is re-enabled.
    // The SMMU is no longer dormant once SMMUEN=1.
    statusr_.store(0u, std::memory_order_release);
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
    // BUG-DORMANT-CPP fix: ARM §3.19 / §6.3.47 — STATUSR.DORMANT (bit 0) must be 1
    // when the SMMU has entered the dormant state.  IDR0.DORMHINT=1 advertises that
    // this SMMU implements dormancy; disable() is the entry point to the dormant state
    // (SMMUEN cleared → SMMU no longer servicing transactions).
    statusr_.store(1u, std::memory_order_release);
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
    if (isNsEL1Tlbi && (cr2_.load(std::memory_order_acquire) & CR2_PTM) != 0u) {
        return; // BUG-AUDIT-54 fix: CR2.PTM=1 → Private TLB Maintenance, SMMU does not participate (ARM §6.3.12)
    }
    // BUG-AUDIT-167-CPP fix: §3.17 line 3466 — if SMMU does not support stage 2
    // (IDR0.S2P==0), a VMID operand in a broadcast TLBI is treated as 0.
    if (!s2pSupported_.load(std::memory_order_acquire)) {
        vmid = 0;
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
    // BUG-NEW-20 fix: snapshot s1cdMax alongside mevEnabled under the stripe lock
    // so that SSV is derived from stream capability (s1cdMax > 0) rather than
    // from PASID value.  ARM §7.3.20: SSV=1 when a SubstreamID was presented,
    // meaning the stream is substream-capable; PASID=0 on such a stream is a
    // valid substream presentation and must carry SSV=1.
    uint8_t streamS1cdMax = 0u;
    {
        size_t stripe = getStreamStripe(streamID);
        std::lock_guard<std::mutex> slock(streamLockStripes[stripe]);
        auto streamIt = streamMap.find(streamID);
        if (streamIt != streamMap.end() && streamIt->second) {
            const StreamConfig& sc = streamIt->second->getStreamConfiguration();
            mevEnabled = sc.mev;
            streamS1cdMax = sc.s1cdMax;
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
    // §7.3.1 BUG-7.3.1-CPP fix: stall events carry a unique STAG and must never
    // be merged by MEV dedup logic.  Gate the entire MEV block on !isStall, and
    // also skip any existing stall event in the inner loop so that a stall entry
    // already in the queue does not suppress a new non-stall event of the same type.
    if (mevEnabled && !isStall) {
        for (const auto& existing : eventQueue) {
            if (!existing.stall && existing.type == type && existing.streamID == streamID && existing.pasid == pasid) {
                return; // suppress duplicate non-stall event
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
        // BUG-NEW-D fix: preserve OVFLG (bit 31) across the PROD advance.
        // advanceQueueIndex() computes (idx+1) % modulus where modulus <= 2^20,
        // stripping bit 31.  Use a read-modify-write so OVFLG is not lost.
        {
            uint32_t oldProd = eventqProd.load(std::memory_order_relaxed);
            uint32_t newProd = advanceQueueIndex(oldProd, eventqLog2Size) | (oldProd & (1u << 31));
            eventqProd.store(newProd, std::memory_order_release);
        }
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
        // BUG-CPP-2 fix: §7.3.3/§7.3.5/§7.3.7/§7.3.11 — InputAddr and SubstreamID are
        // RES0 for C_BAD_STREAMID, C_BAD_STE, C_BAD_CD, and F_STREAM_DISABLED.
        // §7.3 catch-all: "Portions not explicitly defined are RES0."
        // Note: C_BAD_SUBSTREAMID (0x08) DOES define InputAddr (§7.3.9) — do not zero it.
        if (type == EventType::C_BAD_STREAMID || type == EventType::C_BAD_STE ||
            type == EventType::C_BAD_CD       || type == EventType::F_STREAM_DISABLED) {
            pendingEvent.address = 0;
            pendingEvent.pasid   = 0;
        }
        pendingEvent.securityState = securityState;
        pendingEvent.timestamp = getCurrentTimestamp();
        pendingEvent.stall = isStall;
        pendingEvent.stag = stag;
        pendingEvent.errorCode = 0;
        // §7.3 wire-format fields — must match the normal path derivations.
        // GAP-N fix: §7.3.9 — C_BAD_SUBSTREAMID SSV is always 1 (no SSV qualifier).
        // BUG-NEW-20 fix: ARM §7.3.20 — SSV=1 when the stream is substream-capable
        // (streamS1cdMax > 0), regardless of the PASID value.
        if (type == EventType::C_BAD_SUBSTREAMID) {
            pendingEvent.ssv = true;
        } else {
            pendingEvent.ssv = (streamS1cdMax > 0u);
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
            case EventType::F_WALK_EABT:
                // BUG-QA-8 fix: ARM §7.3.12 — F_WALK_EABT CLASS=0b01 (TT).
                // The fault originates from fetching a translation table descriptor
                // (TT = Translation Table), not from the input address (IN) or CD.
                pendingEvent.eventClass = 1u;
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
        // BUG-CPP-5 fix: §7.3.1 — RnW, InD, PnU are RES0 for configuration-class events.
        // Override any non-zero values set by the access-type switch above.
        if (type == EventType::C_BAD_STREAMID || type == EventType::C_BAD_STE    ||
            type == EventType::C_BAD_SUBSTREAMID || type == EventType::C_BAD_CD  ||
            type == EventType::F_CFG_CONFLICT) {
            pendingEvent.rnw = false;
            pendingEvent.ind = false;
            pendingEvent.pnu = false;
        }
        // BUG-CPP-1a fix: §7.3.6 — F_BAD_ATS_TREQ: bits 100/99/98 (RnW/InD/PnU) are RES0.
        // The ATS-specific R/W/X/P fields live at bits 95-92 (separate positions).
        if (type == EventType::F_BAD_ATS_TREQ) {
            pendingEvent.rnw = false;
            pendingEvent.ind = false;
            pendingEvent.pnu = false;
        }
        // BUG-CPP-1b fix: §7.3.8 — F_TRANSL_FORBIDDEN: RnW (bit 100) IS defined;
        // InD (bit 99) and PnU (bit 98) are RES0.
        if (type == EventType::F_TRANSL_FORBIDDEN) {
            pendingEvent.ind = false;
            pendingEvent.pnu = false;
        }
        // BUG-CPP-3 fix: §7.3.21 — COMMAND_SYNC_COMPLETION has no defined wire format;
        // all fields above StreamID are RES0 per §7.3 catch-all.
        // BUG-CPP-4 fix: §7.3.7 — F_STREAM_DISABLED: all fields above StreamID are RES0.
        if (type == EventType::COMMAND_SYNC_COMPLETION ||
            type == EventType::F_STREAM_DISABLED) {
            pendingEvent.rnw = false;
            pendingEvent.ind = false;
            pendingEvent.pnu = false;
        }
        // §7.3.6: F_BAD_ATS_TREQ — populate ATS-specific R/W/X/P fields at bits [95:92].
        // These carry the ATS TR's requested permissions BEFORE STE.{INSTCFG/PRIVCFG} overrides.
        // Derived from the raw (pre-override) access type passed to generateEvent().
        if (type == EventType::F_BAD_ATS_TREQ) {
            // R=1 when the request has a read component (reads and instruction fetches).
            pendingEvent.ats_r = (accessType != AccessType::Write &&
                                  accessType != AccessType::WritePrivileged &&
                                  accessType != AccessType::ReadWrite &&
                                  accessType != AccessType::ReadWritePrivileged);
            // W=1 when the request has a write component (!NW per spec).
            pendingEvent.ats_w = (accessType == AccessType::Write ||
                                  accessType == AccessType::WritePrivileged ||
                                  accessType == AccessType::ReadWrite ||
                                  accessType == AccessType::ReadWritePrivileged);
            // X=1 when the request is an instruction fetch.
            pendingEvent.ats_x = (accessType == AccessType::Execute ||
                                  accessType == AccessType::ExecutePrivileged ||
                                  accessType == AccessType::ReadExecute ||
                                  accessType == AccessType::ReadExecutePrivileged);
            // P=1 when the request is privileged.
            pendingEvent.ats_p = (accessType == AccessType::ReadPrivileged ||
                                  accessType == AccessType::WritePrivileged ||
                                  accessType == AccessType::ExecutePrivileged ||
                                  accessType == AccessType::ReadWritePrivileged ||
                                  accessType == AccessType::ReadExecutePrivileged);
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
    // BUG-CPP-2 fix: §7.3.3/§7.3.5/§7.3.7/§7.3.11 — InputAddr and SubstreamID are
    // RES0 for C_BAD_STREAMID, C_BAD_STE, C_BAD_CD, and F_STREAM_DISABLED.
    // §7.3 catch-all: "Portions not explicitly defined are RES0."
    // Note: C_BAD_SUBSTREAMID (0x08) DOES define InputAddr (§7.3.9) — do not zero it.
    if (type == EventType::C_BAD_STREAMID || type == EventType::C_BAD_STE ||
        type == EventType::C_BAD_CD       || type == EventType::F_STREAM_DISABLED) {
        event.address = 0;
        event.pasid   = 0;
    }
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
    // BUG-NEW-20 fix: ARM §7.3.20 — SSV=1 when the stream is substream-capable
    // (streamS1cdMax > 0), regardless of the PASID value.
    if (type == EventType::C_BAD_SUBSTREAMID) {
        event.ssv = true;
    } else {
        event.ssv = (streamS1cdMax > 0u);
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
        case EventType::F_WALK_EABT:
            // BUG-QA-8 fix: ARM §7.3.12 — F_WALK_EABT CLASS=0b01 (TT).
            // The fault originates from fetching a translation table descriptor.
            event.eventClass = 1u;
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
    // BUG-CPP-5 fix: §7.3.1 — RnW, InD, PnU are RES0 for configuration-class events.
    // Override any non-zero values set by the access-type switch above.
    if (type == EventType::C_BAD_STREAMID || type == EventType::C_BAD_STE    ||
        type == EventType::C_BAD_SUBSTREAMID || type == EventType::C_BAD_CD  ||
        type == EventType::F_CFG_CONFLICT) {
        event.rnw = false;
        event.ind = false;
        event.pnu = false;
    }
    // BUG-CPP-1a fix: §7.3.6 — F_BAD_ATS_TREQ: bits 100/99/98 (RnW/InD/PnU) are RES0.
    // The ATS-specific R/W/X/P fields live at bits 95-92 (separate positions).
    if (type == EventType::F_BAD_ATS_TREQ) {
        event.rnw = false;
        event.ind = false;
        event.pnu = false;
    }
    // BUG-CPP-1b fix: §7.3.8 — F_TRANSL_FORBIDDEN: RnW (bit 100) IS defined;
    // InD (bit 99) and PnU (bit 98) are RES0.
    if (type == EventType::F_TRANSL_FORBIDDEN) {
        event.ind = false;
        event.pnu = false;
    }
    // BUG-CPP-3 fix: §7.3.21 — COMMAND_SYNC_COMPLETION has no defined wire format;
    // all fields above StreamID are RES0 per §7.3 catch-all.
    // BUG-CPP-4 fix: §7.3.7 — F_STREAM_DISABLED: all fields above StreamID are RES0.
    if (type == EventType::COMMAND_SYNC_COMPLETION ||
        type == EventType::F_STREAM_DISABLED) {
        event.rnw = false;
        event.ind = false;
        event.pnu = false;
    }
    // §7.3.6: F_BAD_ATS_TREQ — populate ATS-specific R/W/X/P fields at bits [95:92].
    // These carry the ATS TR's requested permissions BEFORE STE.{INSTCFG/PRIVCFG} overrides.
    // Derived from the raw (pre-override) access type passed to generateEvent().
    if (type == EventType::F_BAD_ATS_TREQ) {
        // R=1 when the request has a read component (reads and instruction fetches).
        event.ats_r = (accessType != AccessType::Write &&
                       accessType != AccessType::WritePrivileged &&
                       accessType != AccessType::ReadWrite &&
                       accessType != AccessType::ReadWritePrivileged);
        // W=1 when the request has a write component (!NW per spec).
        event.ats_w = (accessType == AccessType::Write ||
                       accessType == AccessType::WritePrivileged ||
                       accessType == AccessType::ReadWrite ||
                       accessType == AccessType::ReadWritePrivileged);
        // X=1 when the request is an instruction fetch.
        event.ats_x = (accessType == AccessType::Execute ||
                       accessType == AccessType::ExecutePrivileged ||
                       accessType == AccessType::ReadExecute ||
                       accessType == AccessType::ReadExecutePrivileged);
        // P=1 when the request is privileged.
        event.ats_p = (accessType == AccessType::ReadPrivileged ||
                       accessType == AccessType::WritePrivileged ||
                       accessType == AccessType::ExecutePrivileged ||
                       accessType == AccessType::ReadWritePrivileged ||
                       accessType == AccessType::ReadExecutePrivileged);
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
    // ARM §3.5.1: Advance producer index on enqueue (FINDING-M-01).
    // BUG-NEW-D fix: preserve OVFLG (bit 31) across the PROD advance so that
    // the overflow indication is not silently cleared before software acknowledges.
    {
        uint32_t oldProd = eventqProd.load(std::memory_order_relaxed);
        uint32_t newProd = advanceQueueIndex(oldProd, eventqLog2Size) | (oldProd & (1u << 31));
        eventqProd.store(newProd, std::memory_order_release);
    }
}

uint64_t SMMU::getCurrentTimestamp() const {
    // BUG-AUDIT-8 fix: Replace expensive vDSO syscall (std::chrono::steady_clock::now(),
    // ~20-50 ns) with a cheap atomic increment (~1-2 ns, no lock interaction).
    // This matches the Rust implementation and eliminates potential lock contention
    // from holding queueMutex while making an OS call.
    // Each call increments the counter and returns the new value, guaranteeing
    // strictly increasing timestamps for consecutive events.
    return eventTimestampCounter_.fetch_add(1u, std::memory_order_relaxed) + 1u;
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
    // ARM §6.3.28: CMDQ_CONS has ERR at bits[30:24] and RD at bits[19:0].
    // Queue empty iff PROD.WR == CONS.RD — compare only the RD portion.
    return (cmdqProd.load(std::memory_order_relaxed) & 0xFFFFFu) ==
           (cmdqCons.load(std::memory_order_relaxed) & 0xFFFFFu);
}

bool SMMU::isEventqEmptyByIndex() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // BUG-NEW-A fix: ARM §3.5.1 — empty = PROD.WR == CONS.RD (index bits only).
    // EVENTQ_PROD bit[31]=OVFLG and EVENTQ_CONS bit[31]=OVACKFLG are status flags
    // that must not participate in the empty check.  Mask both before comparing.
    return (eventqProd.load(std::memory_order_relaxed) & ~(1u << 31)) ==
           (eventqCons.load(std::memory_order_relaxed) & ~(1u << 31));
}

uint32_t SMMU::getCmdqOccupiedEntries() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // ARM §6.3.28: mask the ERR field (bits[30:24]) from CONS before computing
    // occupied entries — only the RD field (bits[19:0]) is a queue pointer.
    return queueOccupied(cmdqProd.load(std::memory_order_relaxed),
                         cmdqCons.load(std::memory_order_relaxed) & 0xFFFFFu,
                         cmdqLog2Size);
}

uint32_t SMMU::getEventqOccupiedEntries() const {
    std::lock_guard<std::recursive_mutex> lock(queueMutex);
    // BUG-NEW-A fix: mask OVFLG (bit 31) from both PROD and CONS before computing
    // occupied count.  queueOccupied() computes (prod - cons + modulus) % modulus
    // using only the circular index bits; bit 31 must not contribute to the difference.
    return queueOccupied(
        eventqProd.load(std::memory_order_relaxed) & ~(1u << 31),
        eventqCons.load(std::memory_order_relaxed) & ~(1u << 31),
        eventqLog2Size);
}

uint32_t SMMU::getCmdqLog2Size() const {
    return cmdqLog2Size;
}

uint32_t SMMU::getEventqLog2Size() const {
    return eventqLog2Size;
}

} // namespace smmu