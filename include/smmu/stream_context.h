// ARM SMMU v3 Stream Context
// Copyright (c) 2024 John Greninger

#ifndef SMMU_STREAM_CONTEXT_H
#define SMMU_STREAM_CONTEXT_H

#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/fault_handler.h"
#include <unordered_map>
#include <memory>
#include <cstddef>
#include <mutex>

namespace smmu {

class StreamContext {
public:
    StreamContext();
    ~StreamContext();
    
    // PASID management
    VoidResult createPASID(PASID pasid);
    VoidResult removePASID(PASID pasid);
    void addPASID(PASID pasid, std::shared_ptr<AddressSpace> addressSpace);
    
    // Page mapping operations
    VoidResult mapPage(PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState = SecurityState::NonSecure);
    VoidResult unmapPage(PASID pasid, IOVA iova);
    
    // Translation operations
    TranslationResult translate(PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState = SecurityState::NonSecure);
    
    // Configuration
    void setStage1Enabled(bool enabled);
    void setStage2Enabled(bool enabled);
    void setStage2AddressSpace(std::shared_ptr<AddressSpace> addressSpace);
    void setFaultMode(FaultMode mode);
    
    // Query operations
    bool hasPASID(PASID pasid) const;
    bool isStage1Enabled() const;
    bool isStage2Enabled() const;
    size_t getPASIDCount() const;
    AddressSpace* getPASIDAddressSpace(PASID pasid);
    AddressSpace* getStage2AddressSpace();
    
    // Management operations
    VoidResult clearAllPASIDs();  // Returns VoidResult - error on PASID map corruption or thread safety issues
    
    // Task 4.2: Stream Configuration Update Methods
    /**
     * Update complete stream configuration
     * @param config New configuration to apply
     * @return VoidResult indicating success or specific error
     */
    VoidResult updateConfiguration(const StreamConfig& config);
    
    /**
     * Apply selective configuration changes
     * @param newConfig New configuration with changes to apply
     * @return VoidResult indicating success or specific error
     */
    VoidResult applyConfigurationChanges(const StreamConfig& newConfig);
    
    /**
     * Validate configuration before applying
     * @param config Configuration to validate
     * @return true if configuration is valid
     */
    Result<bool> isConfigurationValid(const StreamConfig& config) const;
    
    /**
     * Validate context descriptor format and content
     * @param contextDescriptor Context descriptor to validate
     * @param pasid PASID associated with context descriptor
     * @param streamID Stream ID for error reporting
     * @return true if context descriptor is valid
     */
    Result<bool> validateContextDescriptor(const ContextDescriptor& contextDescriptor, 
                                          PASID pasid, StreamID streamID) const;
    
    /**
     * Validate translation table base address alignment and range
     * @param ttbr Translation table base register value
     * @param granuleSize Translation granule size
     * @param addressSize Address space size
     * @return true if TTBR is valid
     */
    Result<bool> validateTranslationTableBase(uint64_t ttbr, TranslationGranule granuleSize,
                                             AddressSpaceSize addressSize) const;
    
    /**
     * Validate ASID configuration and detect conflicts
     * @param asid ASID value to validate
     * @param pasid PASID associated with ASID
     * @param securityState Security state context
     * @return true if ASID configuration is valid
     */
    Result<bool> validateASIDConfiguration(uint16_t asid, PASID pasid, 
                                          SecurityState securityState) const;
    
    /**
     * Validate stream table entry configuration
     * @param streamTableEntry STE to validate
     * @return true if STE is valid
     */
    Result<bool> validateStreamTableEntry(const StreamTableEntry& streamTableEntry) const;
    
    /**
     * Generate context descriptor format fault syndrome
     * @param contextDescriptor Faulting context descriptor
     * @param pasid Associated PASID
     * @param errorCode Specific error code
     * @return Comprehensive fault syndrome
     */
    FaultSyndrome generateContextDescriptorFaultSyndrome(
        const ContextDescriptor& contextDescriptor, 
        PASID pasid, uint32_t errorCode) const;
    
    // Task 4.2: Stream Enable/Disable Functionality
    /**
     * Enable stream for translation operations
     * @return VoidResult indicating success or specific error
     */
    VoidResult enableStream();
    
    /**
     * Disable stream and halt operations
     * @return VoidResult indicating success or specific error
     */
    VoidResult disableStream();
    
    /**
     * Query current stream enable state
     * @return Result<bool> indicating stream state - error on configuration corruption or thread safety issues
     */
    Result<bool> isStreamEnabled() const;
    
    // Task 4.2: Stream State Querying Capabilities
    /**
     * Get complete current configuration
     * @return Current stream configuration
     */
    StreamConfig getStreamConfiguration() const;
    
    /**
     * Get stream usage statistics
     * @return Current stream statistics
     */
    StreamStatistics getStreamStatistics() const;
    
    /**
     * Get comprehensive stream state information
     * @return Current stream configuration (alias for getStreamConfiguration)
     */
    StreamConfig getStreamState() const;
    
    /**
     * Check if translation is currently active
     * @return true if translation operations are active
     */
    bool isTranslationActive() const;
    
    /**
     * Check if configuration has been modified
     * @return true if configuration has been changed since creation
     */
    bool hasConfigurationChanged() const;
    
    // Task 4.2: Stream Fault Handling Integration
    /**
     * Set fault handler for this stream
     * @param handler Shared pointer to fault handler
     * @return VoidResult indicating success or specific error on handler validation failure
     */
    VoidResult setFaultHandler(std::shared_ptr<FaultHandler> handler);
    
    /**
     * Get current fault handler
     * @return Shared pointer to current fault handler (may be null)
     */
    std::shared_ptr<FaultHandler> getFaultHandler() const;
    
    /**
     * Record fault through fault handler
     * @param fault Fault record to record
     * @return VoidResult indicating success or specific error
     */
    VoidResult recordFault(const FaultRecord& fault);
    
    /**
     * Check if fault handler is configured
     * @return true if fault handler is set
     */
    bool hasFaultHandler() const;
    
    /**
     * Clear faults specific to this stream
     * Requires fault handler to be configured
     */
    void clearStreamFaults();
    
private:
    // PASID to AddressSpace mapping for Stage-1
    std::unordered_map<PASID, std::shared_ptr<AddressSpace>> pasidMap;
    
    // Stage-2 AddressSpace (potentially shared across streams)
    std::shared_ptr<AddressSpace> stage2AddressSpace;
    
    // Configuration flags
    bool stage1Enabled;
    bool stage2Enabled;
    FaultMode faultMode;
    
    // Task 4.2: Stream Operations Support Members
    StreamConfig currentConfiguration;
    StreamStatistics streamStatistics;
    bool streamEnabled;
    bool configurationChanged;
    std::shared_ptr<FaultHandler> faultHandler;
    
    // Thread safety synchronization
    mutable std::mutex contextMutex;
    
    // Helper methods
    // Note: Fault recording moved to SMMU controller for proper StreamID handling
};

} // namespace smmu

#endif // SMMU_STREAM_CONTEXT_H