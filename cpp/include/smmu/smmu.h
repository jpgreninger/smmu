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
    size_t getCommandQueueSize() const;
    void clearCommandQueue();
    
    // PRI queue for page requests (Task 5.3.3)
    void submitPageRequest(const PRIEntry& request);
    void processPRIQueue();
    std::vector<PRIEntry> getPRIQueue() const;
    void clearPRIQueue();
    size_t getPRIQueueSize() const;
    
    // Cache invalidation command handling (Task 5.3.4)
    void executeInvalidationCommand(const CommandEntry& command);
    void executeTLBInvalidationCommand(CommandType type, StreamID streamID, PASID pasid);
    void executeATCInvalidationCommand(StreamID streamID, PASID pasid, IOVA startAddr, IOVA endAddr);
    
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
    
    // Thread safety protection for SMMU controller
    mutable std::mutex sMMUMutex;
    
    // Helper methods
    void recordFault(const FaultRecord& fault);
    void recordSecurityFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState expectedState, SecurityState actualState);
    bool validateSecurityState(SecurityState requestedState, SecurityState contextState) const;
    SecurityState determineContextSecurityState(StreamID streamID, PASID pasid) const;
    
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
                                 uint8_t faultLevel = 0, uint16_t contextDescIndex = 0);
    FaultType classifyDetailedTranslationFault(IOVA iova, uint8_t tableLevel, bool formatError = false) const;
    void recordCacheHit() const;
    void recordCacheMiss() const;
    
    // Enhanced translation helpers (Task 5.2)
    TranslationResult performTwoStageTranslation(StreamID streamID, PASID pasid, IOVA iova, 
                                               AccessType accessType, SecurityState securityState, StreamContext* streamContext);
    bool isTranslationCacheable(const TranslationResult& result) const;
    void cacheTranslationResult(StreamID streamID, PASID pasid, IOVA iova, 
                               const TranslationResult& result);
    TranslationResult lookupTranslationCache(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void generateCacheKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState, uint64_t& cacheKey) const;
    
    // Stage-specific translation methods (Task 5.2)
    TranslationResult performBothStagesTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext);
    TranslationResult performStage1OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext);
    TranslationResult performStage2OnlyTranslation(StreamID streamID, PASID pasid, IOVA iova,
                                                  AccessType accessType, SecurityState securityState, StreamContext* streamContext);
    bool validateAccessPermissions(const PagePermissions& permissions, AccessType accessType) const;
    
    // Enhanced error handling and fault recovery methods (Task 5.2)
    void handleTranslationFailure(StreamID streamID, PASID pasid, IOVA iova, 
                                 AccessType accessType, SecurityState securityState, TranslationResult& result);
    FaultType classifyTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) const;
    void handleTranslationFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void handlePermissionFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState);
    void handleAddressSizeFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState);
    void handleAccessFaultRecovery(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState);
    
    // Task 5.3: Helper methods for event and command processing
    void processCommand(const CommandEntry& command);
    void generateEvent(EventType type, StreamID streamID, PASID pasid, IOVA address, SecurityState securityState = SecurityState::NonSecure);
    uint64_t getCurrentTimestamp() const;
};

} // namespace smmu

#endif // SMMU_SMMU_H