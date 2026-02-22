// ARM SMMU v3 Main Controller
// Copyright (c) 2024 John Greninger

#ifndef SMMU_SMMU_H
#define SMMU_SMMU_H

#include "smmu/types.h"
#include "smmu/stream_context.h"
#include "smmu/fault_handler.h"
#include "smmu/tlb_cache.h"
#include "smmu/configuration.h"
#include <unordered_map>
#include <vector>
#include <memory>
#include <deque>
#include <cstddef>
#include <atomic>
#include <mutex>

namespace smmu {

class SMMU {
public:
    // Default constructor with default configuration
    SMMU();
    
    // Constructor with custom configuration
    explicit SMMU(const SMMUConfiguration& config);
    
    ~SMMU();
    
    // Main translation API
    TranslationResult translate(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState = SecurityState::NonSecure);
    
    // Stream management
    VoidResult configureStream(StreamID streamID, const StreamConfig& config);
    VoidResult removeStream(StreamID streamID);
    Result<bool> isStreamConfigured(StreamID streamID) const; // Returns Result<bool> - error on invalid StreamID or system failure
    VoidResult enableStream(StreamID streamID);
    VoidResult disableStream(StreamID streamID);
    Result<bool> isStreamEnabled(StreamID streamID) const;   // Returns Result<bool> - error on invalid StreamID or unconfigured stream
    
    // PASID management for streams
    VoidResult createStreamPASID(StreamID streamID, PASID pasid);
    VoidResult removeStreamPASID(StreamID streamID, PASID pasid);

    // Address size configuration (ARM §3.4.1)
    // Sets the input address size (in bits) for the context descriptor of the
    // given (streamID, pasid) pair.  Valid range: 32–52.
    // IOVAs >= (1ULL << bits) will produce F_ADDR_SIZE during translation.
    VoidResult setStreamInputAddressSize(StreamID streamID, PASID pasid, uint8_t bits);
    
    // Page mapping operations
    VoidResult mapPage(StreamID streamID, PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState = SecurityState::NonSecure);
    VoidResult unmapPage(StreamID streamID, PASID pasid, IOVA iova);
    
    // Event management
    Result<std::vector<FaultRecord>> getEvents();  // Returns Result - error on event queue corruption or system failure
    VoidResult clearEvents();                       // Returns VoidResult - error on event queue corruption or thread safety issues
    
    // Configuration
    VoidResult setGlobalFaultMode(FaultMode mode);  // Returns VoidResult - error on invalid mode or thread safety issues
    VoidResult enableCaching(bool enable);          // Returns VoidResult - error on cache system failure or invalid state
    
    // Configuration management
    const SMMUConfiguration& getConfiguration() const;
    VoidResult updateConfiguration(const SMMUConfiguration& config);
    VoidResult updateQueueConfiguration(const QueueConfiguration& queueConfig);
    VoidResult updateCacheConfiguration(const CacheConfiguration& cacheConfig);
    VoidResult updateAddressConfiguration(const AddressConfiguration& addressConfig);
    VoidResult updateResourceLimits(const ResourceLimits& resourceLimits);
    
    // Cache management operations (Task 5.2)
    void invalidateTranslationCache();
    void invalidateStreamCache(StreamID streamID);
    void invalidatePASIDCache(StreamID streamID, PASID pasid);
    void setStreamVMID(StreamID streamID, uint16_t vmid);  ///< Set STE.S2VMID for a stream (ARM §5.2)
    void setStreamASID(StreamID streamID, uint16_t asid);  ///< Set CD.ASID for a stream (ARM §3.17)
    
    // Task 5.3: Event and Command Processing
    // Event queue management (Task 5.3.1)
    void processEventQueue();
    Result<bool> hasEvents() const;
    std::vector<EventEntry> getEventQueue() const;
    void clearEventQueue();
    size_t getEventQueueSize() const;
    
    // Command queue processing simulation (Task 5.3.2)
    VoidResult submitCommand(const CommandEntry& command);
    void processCommandQueue();
    Result<bool> isCommandQueueFull() const;
    std::vector<CommandEntry> getCommandQueue() const;
    size_t getCommandQueueSize() const;
    void clearCommandQueue();
    
    // PRI queue for page requests (Task 5.3.3)
    void submitPageRequest(const PRIEntry& request);
    void processPRIQueue();
    std::vector<PRIEntry> getPRIQueue() const;
    void clearPRIQueue();
    size_t getPRIQueueSize() const;
    
    // ARM §3.5.1: Circular queue PROD/CONS register accessors (FINDING-M-01)
    uint32_t getCmdqProdIndex() const;
    uint32_t getCmdqConsIndex() const;
    uint32_t getEventqProdIndex() const;
    uint32_t getEventqConsIndex() const;
    uint32_t getPriqProdIndex() const;
    uint32_t getPriqConsIndex() const;

    bool isCmdqEmptyByIndex() const;
    bool isEventqEmptyByIndex() const;
    uint32_t getCmdqOccupiedEntries() const;
    uint32_t getEventqOccupiedEntries() const;

    uint32_t getCmdqLog2Size() const;
    uint32_t getEventqLog2Size() const;

    // Cache invalidation command handling (Task 5.3.4)
    void executeInvalidationCommand(const CommandEntry& command);
    void executeTLBInvalidationCommand(CommandType type, StreamID streamID, PASID pasid, uint16_t asid, uint16_t vmid);
    void executeATCInvalidationCommand(StreamID streamID, PASID pasid, IOVA startAddr, IOVA endAddr);
    
    // ARM §6.3.17: SMMU_GERROR / SMMU_GERRORN register model (FINDING-M-06)
    uint32_t getGerror() const;
    void clearGerror(uint32_t bits);

    // ARM §6.3.9 SMMU_CR0.SMMUEN / §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01, FINDING-NEW-09)
    /// Enable the SMMU globally (SMMUEN=1).  When enabled, translations go
    /// through the full stream/page-table path.
    void enable();
    /// Disable the SMMU globally (SMMUEN=0).  When disabled, transactions
    /// bypass (identity PA) or abort depending on GBPA.ABORT.
    void disable();
    /// Returns true when SMMUEN=1 (SMMU globally enabled).
    bool isEnabled() const;
    /// Set SMMU_GBPA.ABORT.  When true and SMMUEN=0, all transactions abort
    /// with SMMUError::GbpaAbort instead of bypassing.
    void setGbpaAbort(bool abort);
    /// Returns the current value of SMMU_GBPA.ABORT.
    bool isGbpaAbort() const;

    // ARM §3.12.2: Stall queue management (FINDING-NEW-08)
    /// Returns a snapshot of all currently stalled transactions.
    std::vector<StallRecord> getStalledTransactions() const;
    /// Forcibly abort a stalled transaction by STAG (removes from stall queue).
    /// Returns true if the STAG was found and removed; false if not found.
    bool abortStalledTransaction(uint16_t stag);
    /// Returns the number of currently stalled transactions.
    size_t getStalledTransactionCount() const;

    // Statistics and debugging
    size_t getStreamCount() const;
    uint64_t getTotalTranslations() const;
    uint64_t getTotalFaults() const;
    uint64_t getTranslationCount() const;
    uint64_t getCacheHitCount() const;
    uint64_t getCacheMissCount() const;
    CacheStatistics getCacheStatistics() const;
    void resetStatistics();
    void reset();
    
private:
    // StreamID to StreamContext mapping
    std::unordered_map<StreamID, std::unique_ptr<StreamContext>> streamMap;
    
    // Event handling
    std::shared_ptr<FaultHandler> faultHandler;
    
    // TLB Cache system (Task 5.2)
    std::unique_ptr<TLBCache> tlbCache;
    
    // SMMU Configuration
    SMMUConfiguration configuration;
    
    // Global configuration
    FaultMode globalFaultMode;
    bool cachingEnabled;
    
    // Statistics - Thread-safe atomic counters
    mutable std::atomic<uint64_t> translationCount;
    mutable std::atomic<uint64_t> cacheHits;
    mutable std::atomic<uint64_t> cacheMisses;
    
    // Task 5.3: Event and Command Processing private members
    std::deque<EventEntry> eventQueue;
    std::deque<CommandEntry> commandQueue;
    std::deque<PRIEntry> priQueue;

    size_t maxEventQueueSize;
    size_t maxCommandQueueSize;
    size_t maxPRIQueueSize;

    // ARM §3.5.1: Circular queue PROD/CONS indices
    // LOG2SIZE values (computed from max queue size)
    uint32_t cmdqLog2Size;     // log2(commandQueue capacity)
    uint32_t eventqLog2Size;   // log2(eventQueue capacity)
    uint32_t priqLog2Size;     // log2(priQueue capacity)

    // PROD/CONS index pairs per queue (declaration order must match constructor init list)
    uint32_t cmdqProd;         // CMDQ_PROD register equivalent
    uint32_t cmdqCons;         // CMDQ_CONS register equivalent
    uint32_t eventqProd;       // EVENTQ_PROD register equivalent
    uint32_t eventqCons;       // EVENTQ_CONS register equivalent
    uint32_t priqProd;         // PRIQ_PROD register equivalent
    uint32_t priqCons;         // PRIQ_CONS register equivalent

    // ARM §6.3.17: SMMU_GERROR register (FINDING-M-06)
    uint32_t gerrorStatus;     // global error flags; cleared by clearGerror()

    // ARM §6.3.9 SMMU_CR0.SMMUEN (FINDING-NEW-09) and §3.11 SMMU_GBPA.ABORT (FINDING-NEW-01)
    bool smmuen_;              // SMMUEN bit — false (disabled) at reset
    bool gbpaAbort_;           // GBPA.ABORT bit — false (bypass) at reset

    // ARM §3.12.2: Stall queue for stalled transactions (FINDING-NEW-08)
    std::unordered_map<uint16_t, StallRecord> stallQueue_;   ///< STAG -> StallRecord map
    std::atomic<uint16_t> stagCounter_;                       ///< Monotonically incrementing STAG generator
    mutable std::mutex stallQueueMutex_;                      ///< Protects stallQueue_

    // Thread safety protection for SMMU controller - lock striping for scalability
    static constexpr size_t NUM_STREAM_STRIPES = 16;
    mutable std::array<std::mutex, NUM_STREAM_STRIPES> streamLockStripes;

    // BUG-03 fix: Dedicated mutex protecting all queue operations (eventQueue,
    // commandQueue, priQueue). A recursive_mutex is required because
    // processPRIQueue() calls submitCommand() while already holding the lock.
    // Lock order invariant: queueMutex must never be acquired while a
    // streamLockStripe is held.
    mutable std::recursive_mutex queueMutex;
    
    // Helper methods
    void recordFault(const FaultRecord& fault);
    void recordSecurityFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState expectedState, SecurityState actualState);
    bool validateSecurityState(SecurityState requestedState, SecurityState contextState) const;
    SecurityState determineContextSecurityState(StreamID streamID, PASID pasid) const;

    // Lock striping helper for stream map access
    size_t getStreamStripe(StreamID streamID) const {
        return streamID % NUM_STREAM_STRIPES;
    }
    
    // Configuration helper methods
    void applyConfiguration();
    VoidResult validateConfigurationUpdate(const SMMUConfiguration& config) const;
    
    // ARM SMMU v3 comprehensive fault syndrome generation methods
    FaultSyndrome generateFaultSyndrome(FaultType faultType, FaultStage stage, AccessType accessType, 
                                       SecurityState securityState, uint8_t faultLevel = 0, 
                                       PrivilegeLevel privLevel = PrivilegeLevel::EL1,
                                       uint16_t contextDescIndex = 0) const;
    uint32_t encodeFaultSyndromeRegister(FaultType faultType, FaultStage stage, uint8_t level, 
                                        bool writeAccess, bool instructionFetch) const;
    FaultStage determineFaultStage(const StreamConfig& config, FaultType faultType) const;
    PrivilegeLevel determinePrivilegeLevel(AccessType accessType, SecurityState securityState) const;
    AccessClassification classifyAccess(AccessType accessType) const;
    void recordComprehensiveFault(StreamID streamID, PASID pasid, IOVA iova, FaultType faultType,
                                 AccessType accessType, SecurityState securityState, FaultStage stage,
                                 uint64_t currentTime, uint8_t faultLevel = 0, uint16_t contextDescIndex = 0);
    FaultType classifyDetailedTranslationFault(IOVA iova, uint8_t tableLevel, bool formatError = false) const;
    void recordCacheHit() const;
    void recordCacheMiss() const;
    
    // Enhanced translation helpers (Task 5.2)
    TranslationResult performTwoStageTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                               AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime);
    bool isTranslationCacheable(const TranslationResult& result) const;
    void cacheTranslationResult(StreamID streamID, PASID pasid, IOVA iova,
                               const TranslationResult& result, uint64_t currentTime,
                               uint16_t asid, uint16_t vmid);
    TranslationResult lookupTranslationCache(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void generateCacheKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState, uint64_t& cacheKey) const;

    // Stage-specific translation methods (Task 5.2)
    TranslationResult performBothStagesTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, const StreamConfig& config, uint64_t currentTime);
    TranslationResult performStage1OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime);
    TranslationResult performStage2OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext, uint64_t currentTime);

    // Optimization 6: Inline validateAccessPermissions for performance
    inline bool validateAccessPermissions(const PagePermissions& permissions, AccessType accessType) const {
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

    // Enhanced error handling and fault recovery methods (Task 5.2)
    void handleTranslationFailure(StreamID streamID, PASID pasid, IOVA iova,
                                 AccessType accessType, SecurityState securityState, TranslationResult& result, uint64_t currentTime);
    FaultType classifyTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) const;
    void handleTranslationFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void handlePermissionFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState);
    void handleAddressSizeFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void handleAccessFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState);
    
    // Task 5.3: Helper methods for event and command processing
    void processCommand(const CommandEntry& command);
    void generateEvent(EventType type, StreamID streamID, PASID pasid, IOVA address,
                       SecurityState securityState = SecurityState::NonSecure, bool isStall = false,
                       uint16_t stag = 0);
    uint64_t getCurrentTimestamp() const;
};

} // namespace smmu

#endif // SMMU_SMMU_H