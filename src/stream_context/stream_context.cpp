// ARM SMMU v3 Stream Context Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/stream_context.h"
#include <algorithm>  // Required for std::find_if
#include <chrono>     // Required for timestamp generation

namespace smmu {

// Helper function to get current timestamp
static uint64_t getCurrentTimestamp() {
    auto now = std::chrono::steady_clock::now();
    auto duration = now.time_since_epoch();
    return std::chrono::duration_cast<std::chrono::microseconds>(duration).count();
}

// Constructor - initializes stream context with ARM SMMU v3 defaults
StreamContext::StreamContext() 
    : stage1Enabled(true),     // ARM SMMU v3: Stage-1 typically enabled by default
      stage2Enabled(false),    // ARM SMMU v3: Stage-2 disabled until configured
      faultMode(FaultMode::Terminate),  // Default to immediate DMA termination
      streamEnabled(false),    // Stream disabled by default per ARM SMMU v3
      configurationChanged(false) {  // Configuration initially unchanged
    
    // Initialize default configuration
    currentConfiguration.translationEnabled = false;  // Default: translation disabled
    currentConfiguration.stage1Enabled = stage1Enabled;
    currentConfiguration.stage2Enabled = stage2Enabled;
    currentConfiguration.faultMode = faultMode;
    
    // Initialize statistics with creation timestamp
    streamStatistics.creationTimestamp = getCurrentTimestamp();
    streamStatistics.lastAccessTimestamp = streamStatistics.creationTimestamp;
    
    // Empty PASID map - sparse allocation for efficient memory usage
    // Stage-2 AddressSpace remains null until explicitly configured
    // Fault handler initially null
}

// Destructor - automatic cleanup via RAII
StreamContext::~StreamContext() {
    // std::unordered_map and std::shared_ptr provide automatic cleanup
    // All PASID address spaces are cleaned up automatically
    // Stage-2 AddressSpace cleaned up when last reference is released
}

// Create new PASID with fresh AddressSpace
// ARM SMMU v3 spec: PASID creates isolated translation context
VoidResult StreamContext::createPASID(PASID pasid) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // ARM SMMU v3 spec: Validate PASID within 20-bit range (0xFFFFF)
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds ARM SMMU v3 specification limits or is reserved
    }
    
    // Check if PASID already exists to prevent accidental overwrites
    if (pasidMap.find(pasid) != pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDAlreadyExists);  // PASID already exists - use addPASID to replace
    }
    
    // Create new AddressSpace for this PASID
    // ARM SMMU v3: Each PASID gets independent Stage-1 address space
    std::shared_ptr<AddressSpace> addressSpace = std::make_shared<AddressSpace>();
    
    // Insert into PASID map with efficient O(1) average case performance
    pasidMap[pasid] = addressSpace;
    
    // Update PASID count statistics
    streamStatistics.pasidCount = pasidMap.size();
    
    return makeVoidSuccess();  // Successful PASID creation
}

// Remove PASID and clean up associated resources
// ARM SMMU v3 spec: PASID removal invalidates all associated translations
VoidResult StreamContext::removePASID(PASID pasid) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within specification limits
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // Invalid PASID value
    }
    
    // Find PASID in map
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDNotFound);  // PASID does not exist
    }
    
    // Remove from map - AddressSpace will be destroyed when last reference released
    // ARM SMMU v3: All translations for this PASID become invalid
    pasidMap.erase(it);
    
    // Update PASID count statistics
    streamStatistics.pasidCount = pasidMap.size();
    
    // Note: Actual TLB invalidation is coordinated at higher SMMU level
    // where StreamID context is available for complete cache coherence
    
    return makeVoidSuccess();  // Successful PASID removal
}

// Add PASID with existing AddressSpace (for sharing scenarios)
// ARM SMMU v3 spec: Supports shared address spaces across PASIDs
void StreamContext::addPASID(PASID pasid, std::shared_ptr<AddressSpace> addressSpace) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // ARM SMMU v3 spec: Validate PASID within 20-bit range
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return;  // Silently ignore invalid PASID to maintain interface consistency
    }
    
    // Validate AddressSpace pointer - null pointer indicates programming error
    if (!addressSpace) {
        return;  // Null AddressSpace not allowed - maintain translation integrity
    }
    
    // Insert or replace existing PASID mapping
    // ARM SMMU v3: Allows multiple PASIDs to share same address space
    pasidMap[pasid] = addressSpace;
    
    // Update PASID count statistics
    streamStatistics.pasidCount = pasidMap.size();
    
    // Note: If replacing existing PASID, old AddressSpace reference count
    // decrements and may trigger automatic cleanup via shared_ptr
}

// Map page within specific PASID address space
// ARM SMMU v3 spec: Per-PASID page mapping with isolation enforcement
VoidResult StreamContext::mapPage(PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within ARM SMMU v3 specification limits
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds specification limits or is reserved
    }
    
    // Find PASID in map
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDNotFound);  // PASID not found - must create PASID first
    }
    
    // Get AddressSpace for this PASID
    std::shared_ptr<AddressSpace> addressSpace = it->second;
    if (!addressSpace) {
        return makeVoidError(SMMUError::InternalError);  // Null AddressSpace indicates corrupted state
    }
    
    // Delegate to AddressSpace for actual mapping
    // ARM SMMU v3: Stage-1 translation managed by per-PASID AddressSpace
    VoidResult result = addressSpace->mapPage(iova, pa, permissions, securityState);
    if (result.isError()) {
        return result;  // Propagate AddressSpace error
    }
    
    return makeVoidSuccess();  // Successful page mapping
}

// Unmap page from specific PASID address space
// ARM SMMU v3 spec: Per-PASID page unmapping with proper cleanup
VoidResult StreamContext::unmapPage(PASID pasid, IOVA iova) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within ARM SMMU v3 specification limits
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds specification limits or is reserved
    }
    
    // Find PASID in map
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDNotFound);  // PASID not found
    }
    
    // Get AddressSpace for this PASID
    std::shared_ptr<AddressSpace> addressSpace = it->second;
    if (!addressSpace) {
        return makeVoidError(SMMUError::InternalError);  // Null AddressSpace indicates corrupted state
    }
    
    // Delegate to AddressSpace for actual unmapping
    // ARM SMMU v3: Stage-1 unmapping with proper cleanup
    VoidResult result = addressSpace->unmapPage(iova);
    if (result.isError()) {
        return result;  // Propagate AddressSpace error
    }
    
    // Note: TLB invalidation coordinated at higher SMMU level
    // where StreamID/PASID context available for cache coherence
    
    return makeVoidSuccess();
}

// Perform two-stage address translation with ARM SMMU v3 semantics
// ARM SMMU v3 spec: Stage-1 (per-PASID) + Stage-2 (shared) translation
TranslationResult StreamContext::translate(PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Update translation statistics
    streamStatistics.translationCount++;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
    
    // ARM SMMU v3: Check if any translation stage is enabled
    if (!stage1Enabled && !stage2Enabled) {
        // No translation enabled - return identity mapping (pass-through)
        return makeTranslationSuccess(iova, PagePermissions(), securityState);
    }
    
    // ARM SMMU v3: Check stream enabled state only for active translation contexts
    // Stream must be enabled if translation is configured and requested
    if ((stage1Enabled || stage2Enabled) && currentConfiguration.translationEnabled && !streamEnabled) {
        // Translation is configured but stream is disabled - fail translation
        streamStatistics.faultCount++;  // Track fault
        return makeTranslationError(SMMUError::StreamDisabled);
    }
    
    // Validate PASID within ARM SMMU v3 specification limits
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        streamStatistics.faultCount++;  // Track fault
        // Note: Fault will be recorded by SMMU controller with proper StreamID
        return makeTranslationError(SMMUError::InvalidPASID);
    }
    
    IPA intermediatePA = iova;  // Start with input address
    
    // ARM SMMU v3: Stage-1 translation (per-PASID address space)
    if (stage1Enabled) {
        // Find PASID in map
        auto it = pasidMap.find(pasid);
        if (it == pasidMap.end()) {
            // PASID not found - translation fault (page not mapped)
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return makeTranslationError(SMMUError::PageNotMapped);
        }
        
        // Get AddressSpace for this PASID
        std::shared_ptr<AddressSpace> stage1AddressSpace = it->second;
        if (!stage1AddressSpace) {
            // Null AddressSpace - translation fault
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return makeTranslationError(SMMUError::InternalError);
        }
        
        // Perform Stage-1 translation (IOVA -> IPA)
        TranslationResult stage1Result = stage1AddressSpace->translatePage(iova, accessType, securityState);
        if (stage1Result.isError()) {
            // Stage-1 translation failed - propagate fault
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return stage1Result;
        }
        
        // Use Stage-1 output as input to Stage-2
        intermediatePA = stage1Result.getValue().physicalAddress;
    }
    
    // ARM SMMU v3: Stage-2 translation (shared across stream)
    if (stage2Enabled) {
        // Validate Stage-2 AddressSpace is configured
        if (!stage2AddressSpace) {
            // Stage-2 enabled but not configured - no pages are mapped
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return makeTranslationError(SMMUError::PageNotMapped);
        }
        
        // Perform Stage-2 translation (IPA -> PA)
        TranslationResult stage2Result = stage2AddressSpace->translatePage(intermediatePA, accessType, securityState);
        if (stage2Result.isError()) {
            // Stage-2 translation failed - propagate fault
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return stage2Result;
        }
        
        // Stage-2 success - return final physical address
        return makeTranslationSuccess(stage2Result.getValue().physicalAddress, 
                                    stage2Result.getValue().permissions, 
                                    stage2Result.getValue().securityState);
    }
    
    // Only Stage-1 enabled case - intermediatePA already contains translated address
    if (stage1Enabled) {
        // Find the Stage-1 result to get permissions info
        auto it = pasidMap.find(pasid);
        if (it != pasidMap.end() && it->second) {
            TranslationResult stage1Result = it->second->translatePage(iova, accessType, securityState);
            if (stage1Result.isOk()) {
                return makeTranslationSuccess(intermediatePA, 
                                            stage1Result.getValue().permissions, 
                                            stage1Result.getValue().securityState);
            }
        }
    }
    
    // Identity mapping case - no permissions validation
    return makeTranslationSuccess(intermediatePA, PagePermissions(), securityState);
}

// Configure Stage-1 translation enable
// ARM SMMU v3 spec: Per-PASID address translation control
void StreamContext::setStage1Enabled(bool enabled) {
    std::lock_guard<std::mutex> lock(contextMutex);
    stage1Enabled = enabled;
    
    // Note: Actual hardware configuration would be coordinated at SMMU level
    // where StreamID context and hardware registers are accessible
}

// Configure Stage-2 translation enable  
// ARM SMMU v3 spec: Shared Stage-2 translation control
void StreamContext::setStage2Enabled(bool enabled) {
    std::lock_guard<std::mutex> lock(contextMutex);
    stage2Enabled = enabled;
    
    // Note: Stage-2 requires configured AddressSpace to be meaningful
    // Hardware configuration coordinated at SMMU controller level
}

// Set Stage-2 AddressSpace (shared across stream)
// ARM SMMU v3 spec: Stage-2 provides shared translation context
void StreamContext::setStage2AddressSpace(std::shared_ptr<AddressSpace> addressSpace) {
    std::lock_guard<std::mutex> lock(contextMutex);
    stage2AddressSpace = addressSpace;
    
    // ARM SMMU v3: Stage-2 AddressSpace can be shared across multiple streams
    // for efficient memory usage and consistent address translation
}

// Configure fault handling mode
// ARM SMMU v3 spec: Fault response behavior configuration
void StreamContext::setFaultMode(FaultMode mode) {
    std::lock_guard<std::mutex> lock(contextMutex);
    faultMode = mode;
    
    // ARM SMMU v3 supports:
    // - Terminate: Abort DMA transaction immediately
    // - Stall: Queue fault for OS/hypervisor handling
}

// Query if specific PASID exists in this stream context
// ARM SMMU v3 spec: PASID existence check for management operations
bool StreamContext::hasPASID(PASID pasid) const {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within specification limits
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return false;  // Invalid PASID cannot exist
    }
    
    // Check if PASID exists in map
    return pasidMap.find(pasid) != pasidMap.end();
}

// Query Stage-1 translation enable status
// ARM SMMU v3 spec: Per-PASID translation status
bool StreamContext::isStage1Enabled() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return stage1Enabled;
}

// Query Stage-2 translation enable status
// ARM SMMU v3 spec: Shared Stage-2 translation status
bool StreamContext::isStage2Enabled() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return stage2Enabled;
}

// Get count of configured PASIDs
// ARM SMMU v3 spec: Resource utilization and management
size_t StreamContext::getPASIDCount() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return pasidMap.size();
}

// Get AddressSpace for specific PASID (raw pointer for performance)
// ARM SMMU v3 spec: Direct access to PASID address space for efficiency
AddressSpace* StreamContext::getPASIDAddressSpace(PASID pasid) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within specification limits
    // PASID 0 is reserved and invalid per ARM SMMU v3 specification
    if (pasid == 0 || pasid > MAX_PASID) {
        return nullptr;  // Invalid PASID
    }
    
    // Find PASID in map
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return nullptr;  // PASID not found
    }
    
    // Return raw pointer from shared_ptr for caller efficiency
    // Caller must not store this pointer beyond current operation scope
    return it->second.get();
}

// Get Stage-2 AddressSpace for two-stage translation coordination
// ARM SMMU v3 spec: Direct access to Stage-2 address space for SMMU coordination
AddressSpace* StreamContext::getStage2AddressSpace() {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Return raw pointer from shared_ptr for caller efficiency
    // ARM SMMU v3: Stage-2 address space shared across stream contexts
    // Caller must not store this pointer beyond current operation scope
    return stage2AddressSpace.get();
}

// Clear all PASIDs and associated address spaces
// ARM SMMU v3 spec: Complete stream context invalidation
VoidResult StreamContext::clearAllPASIDs() {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    try {
        // Clear entire PASID map
        // ARM SMMU v3: All translations for this stream become invalid
        pasidMap.clear();
        
        // Update PASID count statistics
        streamStatistics.pasidCount = 0;
        
        // Note: AddressSpace objects automatically cleaned up via shared_ptr
        // when last reference is released - RAII ensures proper cleanup
        //
        // Actual TLB invalidation coordinated at SMMU level where StreamID
        // context is available for complete cache coherence operations
        
        return makeVoidSuccess();
    } catch (...) {
        return makeVoidError(SMMUError::InternalError);
    }
}

// ============================================================================
// Task 4.2: Stream Configuration Update Methods
// ============================================================================

// Update complete stream configuration
// ARM SMMU v3 spec: Complete configuration replacement with validation
VoidResult StreamContext::updateConfiguration(const StreamConfig& config) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate configuration before applying
    Result<bool> validResult = isConfigurationValid(config);
    if (validResult.isError() || !validResult.getValue()) {
        return makeVoidError(SMMUError::InvalidConfiguration);  // Invalid configuration rejected
    }
    
    // Apply complete configuration replacement
    currentConfiguration = config;
    
    // Update internal state flags to match configuration
    stage1Enabled = config.stage1Enabled;
    stage2Enabled = config.stage2Enabled;
    faultMode = config.faultMode;
    // Note: streamEnabled state is independent and managed by enableStream/disableStream methods
    
    // Mark configuration as changed
    configurationChanged = true;
    streamStatistics.configurationUpdateCount++;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
    
    return makeVoidSuccess();  // Configuration successfully updated
}

// Apply selective configuration changes
// ARM SMMU v3 spec: Incremental configuration updates with validation
VoidResult StreamContext::applyConfigurationChanges(const StreamConfig& newConfig) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Create merged configuration
    StreamConfig mergedConfig = currentConfiguration;
    
    // Apply selective changes - only update if different
    bool hasChanges = false;
    
    if (newConfig.translationEnabled != currentConfiguration.translationEnabled) {
        mergedConfig.translationEnabled = newConfig.translationEnabled;
        hasChanges = true;
    }
    
    if (newConfig.stage1Enabled != currentConfiguration.stage1Enabled) {
        mergedConfig.stage1Enabled = newConfig.stage1Enabled;
        hasChanges = true;
    }
    
    if (newConfig.stage2Enabled != currentConfiguration.stage2Enabled) {
        mergedConfig.stage2Enabled = newConfig.stage2Enabled;
        hasChanges = true;
    }
    
    if (newConfig.faultMode != currentConfiguration.faultMode) {
        mergedConfig.faultMode = newConfig.faultMode;
        hasChanges = true;
    }
    
    // No changes detected
    if (!hasChanges) {
        return makeVoidSuccess();  // Success - no changes needed
    }
    
    // Validate merged configuration
    Result<bool> validResult = isConfigurationValid(mergedConfig);
    if (validResult.isError() || !validResult.getValue()) {
        return makeVoidError(SMMUError::InvalidConfiguration);  // Invalid merged configuration rejected
    }
    
    // Apply merged configuration directly (already locked)
    currentConfiguration = mergedConfig;
    stage1Enabled = mergedConfig.stage1Enabled;
    stage2Enabled = mergedConfig.stage2Enabled;
    faultMode = mergedConfig.faultMode;
    // Note: streamEnabled state is independent and managed by enableStream/disableStream methods
    configurationChanged = true;
    streamStatistics.configurationUpdateCount++;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
    
    return makeVoidSuccess();
}

// Validate configuration before applying
// ARM SMMU v3 spec: Configuration validation rules
Result<bool> StreamContext::isConfigurationValid(const StreamConfig& config) const {
    // Note: This method is called from within updateConfiguration which already holds contextMutex
    // Therefore, we don't acquire the lock here to avoid deadlock
    
    // ARM SMMU v3: Translation requires at least one stage enabled
    if (config.translationEnabled && !config.stage1Enabled && !config.stage2Enabled) {
        return makeSuccess(false);  // Translation enabled but no stages enabled
    }
    
    // ARM SMMU v3: Stage-2 configuration validation - Check structural validity, not resource availability
    // Note: Stage-2 AddressSpace availability is checked at translation time, not configuration time
    // This allows Stage-2 configuration to be set up before AddressSpace is assigned
    if (config.stage2Enabled && config.translationEnabled && !config.stage1Enabled) {
        // Stage-2 only configuration is structurally valid - AddressSpace will be validated at translation time
        // ARM SMMU v3 spec allows configuration setup before resource allocation
    }
    
    // ARM SMMU v3: Fault mode must be valid
    if (config.faultMode != FaultMode::Terminate && config.faultMode != FaultMode::Stall) {
        return makeSuccess(false);  // Invalid fault mode
    }
    
    // ARM SMMU v3: Enhanced validation for context descriptor consistency
    // If Stage-1 is enabled and we have PASIDs configured, validate each PASID's context
    if (config.stage1Enabled && config.translationEnabled) {
        for (const auto& pasidPair : pasidMap) {
            PASID pasid = pasidPair.first;
            
            // Validate PASID is within ARM SMMU v3 specification limits
            if (pasid == 0 || pasid > MAX_PASID) {
                return makeSuccess(false);  // Invalid PASID configuration
            }
            
            // Validate AddressSpace is configured properly
            if (!pasidPair.second) {
                return makeSuccess(false);  // Null AddressSpace for configured PASID
            }
        }
    }
    
    return makeSuccess(true);  // Configuration is valid
}

// ============================================================================
// Task 4.2: Stream Enable/Disable Functionality
// ============================================================================

// Enable stream for translation operations
// ARM SMMU v3 spec: Stream enable with configuration validation
VoidResult StreamContext::enableStream() {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate current configuration is suitable for enabling
    Result<bool> validResult = isConfigurationValid(currentConfiguration);
    if (validResult.isError() || !validResult.getValue()) {
        return makeVoidError(SMMUError::InvalidConfiguration);  // Cannot enable with invalid configuration
    }
    
    // ARM SMMU v3: Enabling stream requires at least one translation stage
    if (!stage1Enabled && !stage2Enabled) {
        return makeVoidError(SMMUError::ConfigurationError);  // No translation stages enabled
    }
    
    // Enable stream - ARM SMMU v3: Stream enabled/disabled is independent of translation configuration  
    streamEnabled = true;
    // Note: currentConfiguration.translationEnabled remains unchanged - configuration vs stream state are separate
    configurationChanged = true;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
    
    return makeVoidSuccess();  // Stream successfully enabled
}

// Disable stream and halt operations
// ARM SMMU v3 spec: Stream disable with immediate effect
VoidResult StreamContext::disableStream() {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Disable stream - ARM SMMU v3: Stream enabled/disabled is independent of translation configuration
    streamEnabled = false;
    // Note: currentConfiguration.translationEnabled remains unchanged - configuration vs stream state are separate
    configurationChanged = true;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
    
    // ARM SMMU v3: Stream disable always succeeds
    // Hardware would coordinate TLB invalidation at SMMU level
    
    return makeVoidSuccess();  // Stream successfully disabled
}

// Query current stream enable state
// ARM SMMU v3 spec: Stream state query
Result<bool> StreamContext::isStreamEnabled() const {
    try {
        std::lock_guard<std::mutex> lock(contextMutex);
        return Result<bool>(streamEnabled);
    } catch (...) {
        return makeError<bool>(SMMUError::InternalError);
    }
}

// ============================================================================
// Task 4.2: Stream State Querying Capabilities
// ============================================================================

// Get complete current configuration
// ARM SMMU v3 spec: Configuration state query
StreamConfig StreamContext::getStreamConfiguration() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return currentConfiguration;
}

// Get stream usage statistics
// ARM SMMU v3 spec: Performance and usage monitoring
StreamStatistics StreamContext::getStreamStatistics() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return streamStatistics;
}

// Get comprehensive stream state information
// ARM SMMU v3 spec: Complete stream state (alias for getStreamConfiguration)
StreamConfig StreamContext::getStreamState() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return currentConfiguration;
}

// Check if translation is currently active
// ARM SMMU v3 spec: Translation state query
bool StreamContext::isTranslationActive() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    // Translation is active if stream is enabled, translation is enabled in config, at least one stage is enabled, and has PASIDs configured
    return streamEnabled && currentConfiguration.translationEnabled && (stage1Enabled || stage2Enabled) && !pasidMap.empty();
}

// Check if configuration has been modified
// ARM SMMU v3 spec: Configuration change detection
bool StreamContext::hasConfigurationChanged() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return configurationChanged;
}

// ============================================================================
// Task 4.2: Stream Fault Handling Integration
// ============================================================================

// Set fault handler for this stream
// ARM SMMU v3 spec: Fault handling integration
VoidResult StreamContext::setFaultHandler(std::shared_ptr<FaultHandler> handler) {
    // Allow nullptr to clear the fault handler
    try {
        std::lock_guard<std::mutex> lock(contextMutex);
        faultHandler = handler;  // Can be nullptr to clear the handler
        streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
        return makeVoidSuccess();
    } catch (...) {
        return makeVoidError(SMMUError::InternalError);
    }
}

// Get current fault handler
// ARM SMMU v3 spec: Fault handler query
std::shared_ptr<FaultHandler> StreamContext::getFaultHandler() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return faultHandler;
}

// Record fault through fault handler
// ARM SMMU v3 spec: Fault recording and propagation
VoidResult StreamContext::recordFault(const FaultRecord& fault) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Check if fault handler is configured
    if (!faultHandler) {
        return makeVoidError(SMMUError::FaultHandlingError);  // No fault handler configured
    }
    
    // Record fault through handler
    faultHandler->recordFault(fault);
    
    // Update fault statistics after successful recording
    streamStatistics.faultCount++;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
    
    return makeVoidSuccess();  // Fault successfully recorded
}

// Check if fault handler is configured
// ARM SMMU v3 spec: Fault handler presence check
bool StreamContext::hasFaultHandler() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return faultHandler != nullptr;
}

// Clear faults specific to this stream
// ARM SMMU v3 spec: Stream-specific fault management
void StreamContext::clearStreamFaults() {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Requires fault handler to be configured
    if (!faultHandler) {
        return;  // No fault handler - nothing to clear
    }
    
    // Use fault handler's clear functionality
    // ARM SMMU v3: Stream-specific fault clearing coordinated through handler
    faultHandler->clearFaults();
    
    // Update access timestamp
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();
}

// Note: Fault recording moved to SMMU controller for proper StreamID handling per ARM SMMU v3 spec

// ============================================================================
// ARM SMMU v3 Context Descriptor Validation Methods
// ============================================================================

// Validate context descriptor format and content
// ARM SMMU v3 spec: Comprehensive CD format validation
Result<bool> StreamContext::validateContextDescriptor(const ContextDescriptor& contextDescriptor, 
                                                     PASID pasid, StreamID streamID) const {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // ARM SMMU v3: Validate PASID within specification limits
    if (pasid == 0 || pasid > MAX_PASID) {
        return makeSuccess(false);  // Invalid PASID
    }
    
    // ARM SMMU v3: Validate ASID within 16-bit range
    if (contextDescriptor.asid > 0xFFFF) {
        return makeSuccess(false);  // ASID exceeds 16-bit range
    }
    
    // ARM SMMU v3: At least one TTBR must be valid if CD is valid
    if (!contextDescriptor.ttbr0Valid && !contextDescriptor.ttbr1Valid) {
        return makeSuccess(false);  // No valid translation table base registers
    }
    
    // ARM SMMU v3: Validate TTBR0 alignment and range if valid
    if (contextDescriptor.ttbr0Valid) {
        Result<bool> ttbr0Valid = validateTranslationTableBase(contextDescriptor.ttbr0, 
                                                              contextDescriptor.tcr.granuleSize,
                                                              contextDescriptor.tcr.inputAddressSize);
        if (ttbr0Valid.isError() || !ttbr0Valid.getValue()) {
            return makeSuccess(false);  // Invalid TTBR0 configuration
        }
    }
    
    // ARM SMMU v3: Validate TTBR1 alignment and range if valid  
    if (contextDescriptor.ttbr1Valid) {
        Result<bool> ttbr1Valid = validateTranslationTableBase(contextDescriptor.ttbr1,
                                                              contextDescriptor.tcr.granuleSize,
                                                              contextDescriptor.tcr.inputAddressSize);
        if (ttbr1Valid.isError() || !ttbr1Valid.getValue()) {
            return makeSuccess(false);  // Invalid TTBR1 configuration
        }
    }
    
    // ARM SMMU v3: Validate ASID configuration for conflicts
    Result<bool> asidValid = validateASIDConfiguration(contextDescriptor.asid, pasid, 
                                                      contextDescriptor.securityState);
    if (asidValid.isError() || !asidValid.getValue()) {
        return makeSuccess(false);  // ASID configuration conflict detected
    }
    
    // ARM SMMU v3: Validate address space size consistency
    if (contextDescriptor.tcr.inputAddressSize == contextDescriptor.tcr.outputAddressSize) {
        // Input and output sizes can be the same, this is valid
    } else {
        // Validate output address size is not smaller than input
        if (contextDescriptor.tcr.outputAddressSize == AddressSpaceSize::Size32Bit && 
            contextDescriptor.tcr.inputAddressSize != AddressSpaceSize::Size32Bit) {
            return makeSuccess(false);  // Output address space cannot be smaller than input
        }
    }
    
    // ARM SMMU v3: Validate translation granule size is supported
    if (contextDescriptor.tcr.granuleSize != TranslationGranule::Size4KB &&
        contextDescriptor.tcr.granuleSize != TranslationGranule::Size16KB &&
        contextDescriptor.tcr.granuleSize != TranslationGranule::Size64KB) {
        return makeSuccess(false);  // Unsupported translation granule size
    }
    
    return makeSuccess(true);  // Context descriptor is valid
}

// Validate translation table base address alignment and range
// ARM SMMU v3 spec: TTBR alignment and address range validation
Result<bool> StreamContext::validateTranslationTableBase(uint64_t ttbr, TranslationGranule granuleSize,
                                                        AddressSpaceSize addressSize) const {
    // ARM SMMU v3: TTBR cannot be zero (null pointer)
    if (ttbr == 0) {
        return makeSuccess(false);  // Null translation table base address
    }
    
    // ARM SMMU v3: Validate alignment based on granule size
    uint64_t alignmentMask = 0;
    switch (granuleSize) {
        case TranslationGranule::Size4KB:
            alignmentMask = 0xFFF;  // 4KB alignment (12 bits)
            break;
        case TranslationGranule::Size16KB:
            alignmentMask = 0x3FFF; // 16KB alignment (14 bits)
            break;
        case TranslationGranule::Size64KB:
            alignmentMask = 0xFFFF; // 64KB alignment (16 bits)
            break;
        default:
            return makeSuccess(false);  // Invalid granule size
    }
    
    // Check alignment - TTBR must be aligned to granule size
    if ((ttbr & alignmentMask) != 0) {
        return makeSuccess(false);  // TTBR not properly aligned
    }
    
    // ARM SMMU v3: Validate address is within supported range
    uint64_t maxAddress = 0;
    switch (addressSize) {
        case AddressSpaceSize::Size32Bit:
            maxAddress = 0xFFFFFFFFULL;        // 32-bit: 4GB
            break;
        case AddressSpaceSize::Size48Bit:
            maxAddress = 0xFFFFFFFFFFFFULL;    // 48-bit: 256TB
            break;
        case AddressSpaceSize::Size52Bit:
            maxAddress = 0xFFFFFFFFFFFFFULL;   // 52-bit: 4PB
            break;
        default:
            return makeSuccess(false);  // Invalid address size
    }
    
    // Check TTBR is within supported address range
    if (ttbr > maxAddress) {
        return makeSuccess(false);  // TTBR exceeds supported address range
    }
    
    return makeSuccess(true);  // TTBR is valid
}

// Validate ASID configuration and detect conflicts
// ARM SMMU v3 spec: ASID validation and conflict detection
Result<bool> StreamContext::validateASIDConfiguration(uint16_t asid, PASID pasid, 
                                                     SecurityState securityState) const {
    // ARM SMMU v3: ASID 0 is reserved in some contexts but may be valid
    // Allow ASID 0 but validate it doesn't conflict with global translations
    
    // ARM SMMU v3: Validate ASID within 16-bit range (already done in caller but double-check)
    if (asid > 0xFFFF) {
        return makeSuccess(false);  // ASID exceeds 16-bit range
    }
    
    // ARM SMMU v3: Check for ASID conflicts across different PASIDs in same security state
    // This is a simplified conflict detection - full implementation would require
    // global ASID tracking across all streams in the SMMU
    for (const auto& pasidPair : pasidMap) {
        PASID existingPasid = pasidPair.first;
        
        // Skip self-comparison
        if (existingPasid == pasid) {
            continue;
        }
        
        // For this simplified implementation, we assume no ASID conflicts
        // within the same stream context, but in a full implementation,
        // this would check against a global ASID allocation table
        
        // ARM SMMU v3: Different security states can reuse same ASID
        // Same security state within same stream should not reuse ASID
        // This check would be more comprehensive in full SMMU implementation
    }
    
    // ARM SMMU v3: Validate security state is valid
    if (securityState != SecurityState::NonSecure && 
        securityState != SecurityState::Secure &&
        securityState != SecurityState::Realm) {
        return makeSuccess(false);  // Invalid security state
    }
    
    return makeSuccess(true);  // ASID configuration is valid
}

// Validate stream table entry configuration
// ARM SMMU v3 spec: STE format and configuration validation
Result<bool> StreamContext::validateStreamTableEntry(const StreamTableEntry& streamTableEntry) const {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // ARM SMMU v3: Translation enabled requires at least one stage enabled
    if (streamTableEntry.translationEnabled && 
        !streamTableEntry.stage1Enabled && !streamTableEntry.stage2Enabled) {
        return makeSuccess(false);  // Translation enabled but no stages enabled
    }
    
    // ARM SMMU v3: If Stage-1 enabled, CD table base must be valid
    if (streamTableEntry.stage1Enabled && streamTableEntry.translationEnabled) {
        if (streamTableEntry.contextDescriptorTableBase == 0) {
            return makeSuccess(false);  // Stage-1 enabled but no CD table base
        }
        
        // ARM SMMU v3: CD table must be aligned to 64-byte boundary
        if ((streamTableEntry.contextDescriptorTableBase & 0x3F) != 0) {
            return makeSuccess(false);  // CD table base not 64-byte aligned
        }
        
        // ARM SMMU v3: CD table size must be non-zero
        if (streamTableEntry.contextDescriptorTableSize == 0) {
            return makeSuccess(false);  // Invalid CD table size
        }
    }
    
    // ARM SMMU v3: Validate fault mode
    if (streamTableEntry.faultMode != FaultMode::Terminate && 
        streamTableEntry.faultMode != FaultMode::Stall) {
        return makeSuccess(false);  // Invalid fault mode
    }
    
    // ARM SMMU v3: Validate security state
    if (streamTableEntry.securityState != SecurityState::NonSecure &&
        streamTableEntry.securityState != SecurityState::Secure &&
        streamTableEntry.securityState != SecurityState::Realm) {
        return makeSuccess(false);  // Invalid security state
    }
    
    // ARM SMMU v3: Validate granule sizes are supported
    if (streamTableEntry.stage1Granule != TranslationGranule::Size4KB &&
        streamTableEntry.stage1Granule != TranslationGranule::Size16KB &&
        streamTableEntry.stage1Granule != TranslationGranule::Size64KB) {
        return makeSuccess(false);  // Invalid Stage-1 granule size
    }
    
    if (streamTableEntry.stage2Granule != TranslationGranule::Size4KB &&
        streamTableEntry.stage2Granule != TranslationGranule::Size16KB &&
        streamTableEntry.stage2Granule != TranslationGranule::Size64KB) {
        return makeSuccess(false);  // Invalid Stage-2 granule size
    }
    
    return makeSuccess(true);  // Stream table entry is valid
}

// Generate context descriptor format fault syndrome
// ARM SMMU v3 spec: Comprehensive fault syndrome generation for CD format faults
FaultSyndrome StreamContext::generateContextDescriptorFaultSyndrome(
    const ContextDescriptor& contextDescriptor, 
    PASID pasid, uint32_t errorCode) const {
    
    // ARM SMMU v3: Build fault syndrome register value
    uint32_t syndromeValue = 0;
    
    // ARM SMMU v3: Set fault type in syndrome register (bits [7:0])
    syndromeValue |= static_cast<uint32_t>(FaultType::ContextDescriptorFormatFault) & 0xFF;
    
    // ARM SMMU v3: Set PASID in syndrome register (bits [27:8] for 20-bit PASID)
    syndromeValue |= ((pasid & 0xFFFFF) << 8);
    
    // ARM SMMU v3: Set error code in upper bits (bits [31:28])
    syndromeValue |= ((errorCode & 0xF) << 28);
    
    // ARM SMMU v3: Create comprehensive fault syndrome
    FaultSyndrome syndrome(
        syndromeValue,                           // Syndrome register value
        FaultStage::Stage1Only,                  // CD faults are Stage-1 related
        0,                                       // Context descriptor level
        PrivilegeLevel::Unknown,                 // Privilege level unknown for CD fault
        AccessClassification::Unknown,           // Access class unknown for CD fault
        false,                                   // Not a write access fault
        static_cast<uint16_t>(contextDescriptor.contextDescriptorIndex) // CD index
    );
    
    return syndrome;
}

} // namespace smmu