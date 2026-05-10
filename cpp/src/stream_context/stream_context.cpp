// ARM SMMU v3 Stream Context Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/stream_context.h"
#include <algorithm>  // Required for std::find_if
#include <chrono>     // Required for timestamp generation

namespace smmu {

// §3.15 / §13.1.7 Rule 1: OSH required for all Device types and Normal Non-Cacheable.
static inline bool oshRequired(uint8_t memAttr) {
    return (memAttr <= 0x5u) || (memAttr == 0x8u) || (memAttr == 0xCu);
}

// Helper function to get current timestamp
static uint64_t getCurrentTimestamp() {
    auto now = std::chrono::steady_clock::now();
    auto duration = now.time_since_epoch();
    return std::chrono::duration_cast<std::chrono::microseconds>(duration).count();
}

// Constructor - initializes stream context with ARM SMMU v3 defaults
// §5.2 STE.Config==0b000 means all stages disabled at reset.
// BUG-AUDIT-2 fix: both stage1Enabled (internal) and currentConfiguration.stage1Enabled
// must be false at construction.  The previous split-brain (stage1Enabled=true while
// currentConfiguration.stage1Enabled=false) caused the two code paths in translate()
// to behave differently before any configuration was applied.
StreamContext::StreamContext()
    : stage1Enabled(false),    // ARM SMMU v3 §5.2: stage-1 disabled at reset (STE.Config==0b000)
      stage2Enabled(false),    // ARM SMMU v3: Stage-2 disabled until configured
      faultMode(FaultMode::Terminate),  // Default to immediate DMA termination
      s1Stalld_(false),        // GAP-NEW-G: STE.S1STALLD defaults to false (stall mode operates normally)
      streamEnabled(false),    // Stream disabled by default per ARM SMMU v3
      configurationChanged(false),  // Configuration initially unchanged
      maxPASIDsPerStream(1024),  // Default PASID limit per stream (configurable)
      ha(false),               // CD.HA: hardware access flag management disabled by default
      hd(false) {              // CD.HD: hardware dirty state management disabled by default

    // Initialize default configuration
    // BUG-CPP-DBGR-11 fix: all three fields must be false at reset (STE.Config==0b000)
    currentConfiguration.translationEnabled = false;  // Default: translation disabled
    currentConfiguration.stage1Enabled = false;       // Consistent with translationEnabled=false
    currentConfiguration.stage2Enabled = false;
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
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds ARM SMMU v3 specification limits
    }

    // Check if PASID already exists to prevent accidental overwrites
    if (pasidMap.find(pasid) != pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDAlreadyExists);  // PASID already exists - use addPASID to replace
    }

    // Check if adding this PASID would exceed the configured resource limit
    if (pasidMap.size() >= maxPASIDsPerStream) {
        return makeVoidError(SMMUError::PASIDLimitExceeded);  // Maximum PASIDs per stream limit reached
    }

    // Create new AddressSpace for this PASID
    // ARM SMMU v3: Each PASID gets independent Stage-1 address space
    std::shared_ptr<AddressSpace> addressSpace = std::make_shared<AddressSpace>();

    // Insert into PASID map with efficient O(1) average case performance
    pasidMap[pasid] = addressSpace;

    // BUG-07 integration fix: When Stage-2 is enabled, PASID 0 is the
    // hypervisor address space and doubles as the Stage-2 page table.
    // Link it to stage2AddressSpace so that performBothStagesTranslation
    // (which correctly calls getStage2AddressSpace()) finds it.
    if (pasid == 0 && stage2Enabled) {
        stage2AddressSpace = addressSpace;
    }

    // Update PASID count statistics
    streamStatistics.pasidCount = pasidMap.size();

    return makeVoidSuccess();  // Successful PASID creation
}

// Remove PASID and clean up associated resources
// ARM SMMU v3 spec: PASID removal invalidates all associated translations
VoidResult StreamContext::removePASID(PASID pasid) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within specification limits
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // Invalid PASID value
    }
    
    // Find PASID in map
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDNotFound);  // PASID does not exist
    }

    // If PASID 0 is being removed and stage2AddressSpace is aliased to it, clear it
    if (pasid == 0 && stage2Enabled && it->second == stage2AddressSpace) {
        stage2AddressSpace = nullptr;
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
VoidResult StreamContext::addPASID(PASID pasid, std::shared_ptr<AddressSpace> addressSpace) {
    std::lock_guard<std::mutex> lock(contextMutex);

    // BUG-CPP-M02 fix: return errors instead of silently ignoring invalid inputs.
    // ARM SMMU v3 spec: Validate PASID within 20-bit range (0xFFFFF).
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts.
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds ARM SMMU v3 specification limits
    }

    // BUG-CPP-M02 fix: null AddressSpace is a programming error; report it.
    if (!addressSpace) {
        return makeVoidError(SMMUError::InvalidConfiguration);  // Null AddressSpace not allowed
    }

    // Insert or replace existing PASID mapping.
    // ARM SMMU v3: Allows multiple PASIDs to share the same address space.
    pasidMap[pasid] = addressSpace;

    // BUG-NEW2-06 fix: keep stage2AddressSpace alias in sync when replacing
    // PASID-0 on a stage-2-enabled stream, mirroring createPASID() logic.
    if (pasid == 0 && stage2Enabled) {
        stage2AddressSpace = addressSpace;
    }

    // Update PASID count statistics.
    streamStatistics.pasidCount = pasidMap.size();

    // Note: If replacing an existing PASID, the old AddressSpace reference count
    // decrements and may trigger automatic cleanup via shared_ptr.
    return makeVoidSuccess();
}

// Map page within specific PASID address space
// ARM SMMU v3 spec: Per-PASID page mapping with isolation enforcement
VoidResult StreamContext::mapPage(PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions,
                                   SecurityState securityState, bool accessFlag) {
    std::lock_guard<std::mutex> lock(contextMutex);

    // Validate PASID within ARM SMMU v3 specification limits
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds specification limits
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
    VoidResult result = addressSpace->mapPage(iova, pa, permissions, securityState, accessFlag);
    if (result.isError()) {
        return result;  // Propagate AddressSpace error
    }

    return makeVoidSuccess();  // Successful page mapping
}

// Map a stage-1 page as Device memory type (§13.1.5: Device wins in two-stage combining).
VoidResult StreamContext::mapPageDevice(PASID pasid, IOVA iova, PA pa, const PagePermissions& permissions,
                                        SecurityState securityState) {
    std::lock_guard<std::mutex> lock(contextMutex);
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);
    }
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDNotFound);
    }
    std::shared_ptr<AddressSpace> addressSpace = it->second;
    if (!addressSpace) {
        return makeVoidError(SMMUError::InternalError);
    }
    return addressSpace->mapPageDevice(iova, pa, permissions, securityState);
}

// Unmap page from specific PASID address space
// ARM SMMU v3 spec: Per-PASID page unmapping with proper cleanup
// Map a stage-2 page as Device memory type (for S2PTW: STE.S2PTW=1 will fault on such pages).
// Auto-creates the stage-2 AddressSpace if it does not yet exist.
VoidResult StreamContext::mapStage2DevicePage(IOVA ipa, PA pa, const PagePermissions& permissions,
                                               SecurityState securityState) {
    std::lock_guard<std::mutex> lock(contextMutex);
    // BUG-CPP-3 fix (Approach B): Do NOT replace the existing stage2AddressSpace
    // when it is aliased to the PASID-0 stage-1 AS.  Replacing it would discard
    // any normal stage-2 pages that a subsequent setStreamStage2AddressSpace call
    // adds to the original AS.  Instead, simply ensure stage2AddressSpace is
    // non-null (create one only when completely absent) and map the device page
    // in-band on whichever AS currently serves as stage-2.  The alias short-circuit
    // guard in performBothStagesTranslation is removed separately so the real
    // stage-2 walk always runs.
    if (!stage2AddressSpace) {
        stage2AddressSpace = std::make_shared<AddressSpace>();
    }
    return stage2AddressSpace->mapPageDevice(ipa, pa, permissions, securityState);
}

VoidResult StreamContext::unmapPage(PASID pasid, IOVA iova) {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within ARM SMMU v3 specification limits
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
        return makeVoidError(SMMUError::InvalidPASID);  // PASID exceeds specification limits
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
TranslationResult StreamContext::translate(PASID pasid, IOVA iova, AccessType accessType,
                                           SecurityState securityState, bool isAtos) {
    std::lock_guard<std::mutex> lock(contextMutex);
    return translateUnlocked(pasid, iova, accessType, securityState, isAtos);
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

    // BUG-CPP-3 fix (Approach B): Before replacing the current stage-2 AS, carry
    // over any device-memory page entries so that S2PTW checks remain effective.
    // Scenario: mapStage2DevicePage() mapped a device page into the aliased AS
    // (pasid0AS), then setStreamStage2AddressSpace() is called with a new AS.
    // Without migration, the device-page entry would be silently discarded and
    // the S2PTW F_PERMISSION fault would never fire for that IPA.
    if (stage2AddressSpace && addressSpace) {
        auto devicePages = stage2AddressSpace->getDevicePageEntries();
        for (const auto& dp : devicePages) {
            // Re-map the device page into the replacement AS (ignore errors —
            // the entry may already exist if the caller added it explicitly).
            addressSpace->mapPageDevice(dp.first, dp.second.physicalAddress,
                                        dp.second.permissions, dp.second.securityState);
        }
    }

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

// Atomically set fault mode (read-modify-write under contextMutex)
VoidResult StreamContext::setFaultModeAtomic(FaultMode mode) {
    std::lock_guard<std::mutex> lock(contextMutex);
    faultMode = mode;
    currentConfiguration.faultMode = mode;
    return makeVoidSuccess();
}

// Configure maximum PASIDs per stream resource limit
void StreamContext::setMaxPASIDsPerStream(uint32_t maxPASIDs) {
    std::lock_guard<std::mutex> lock(contextMutex);
    maxPASIDsPerStream = maxPASIDs;
}

// Set the per-context input address size for the AddressSpace of the given PASID.
// Valid bit-widths: 32–52 (ARM §3.4.1, TCR.T0SZ encoding).
VoidResult StreamContext::setAddressSpaceInputSize(PASID pasid, uint8_t bits) {
    // Validate bit-width bounds
    if (bits < 32 || bits > 52) {
        return makeVoidError(SMMUError::InvalidAddress);
    }

    std::lock_guard<std::mutex> lock(contextMutex);
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return makeVoidError(SMMUError::PASIDNotFound);
    }

    it->second->setInputAddressSize(bits);
    return makeVoidSuccess();
}

// Enable or disable hardware Access Flag management (CD.HA, ARM §3.13)
void StreamContext::setHardwareAccessFlag(bool haEnabled) {
    std::lock_guard<std::mutex> lock(contextMutex);
    ha = haEnabled;
}

// Enable or disable hardware Dirty State management (CD.HD, ARM §3.13)
void StreamContext::setHardwareDirtyState(bool hdEnabled) {
    std::lock_guard<std::mutex> lock(contextMutex);
    hd = hdEnabled;
}

// Query whether hardware Access Flag management is enabled
bool StreamContext::isHardwareAccessFlagEnabled() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return ha;
}

// Query whether hardware Dirty State management is enabled
bool StreamContext::isHardwareDirtyStateEnabled() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return hd;
}

// Return the current fault mode for this stream context (FINDING-NEW-08)
FaultMode StreamContext::getFaultMode() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return faultMode;
}

// GAP-NEW-G: Return STE.S1STALLD for this stream context (ARM IHI0070G.b §5.2).
// When true, stall mode is suppressed — the stream aborts immediately on fault
// regardless of the faultMode setting.
bool StreamContext::isS1StallDisabled() const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return s1Stalld_;
}

// Query if specific PASID exists in this stream context
// ARM SMMU v3 spec: PASID existence check for management operations
bool StreamContext::hasPASID(PASID pasid) const {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // Validate PASID within specification limits
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
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

// Get AddressSpace for specific PASID.
// BUG-NEW-CPP-5 fix: return shared_ptr instead of raw pointer.  The shared_ptr
// keeps the AddressSpace alive for the duration of the caller's use even if a
// concurrent removeStreamPASID() erases the entry from pasidMap.  Previously
// the raw pointer could become a dangling pointer once the lock was released
// and removePASID() destroyed the AddressSpace.
std::shared_ptr<AddressSpace> StreamContext::getPASIDAddressSpace(PASID pasid) {
    std::lock_guard<std::mutex> lock(contextMutex);
    if (pasid > MAX_PASID) {
        return nullptr;
    }
    auto it = pasidMap.find(pasid);
    if (it == pasidMap.end()) {
        return nullptr;
    }
    return it->second;
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

        // BUG-CPP-10 fix: When PASID 0 was created with two-stage enabled,
        // stage2AddressSpace was set to alias the PASID-0 AddressSpace.
        // After clearing pasidMap the shared_ptr in stage2AddressSpace still
        // keeps that object alive, causing stage-2 translation to use a stale
        // address space after clearAllPASIDs().  Reset it here so the next
        // stage-2 operation starts with a clean (null) context.
        stage2AddressSpace.reset();

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
    s1Stalld_ = config.s1Stalld;  // GAP-NEW-G: propagate STE.S1STALLD
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

    // Bug NEW-3 fix: also detect and merge s1Stalld changes.
    if (newConfig.s1Stalld != currentConfiguration.s1Stalld) {
        mergedConfig.s1Stalld = newConfig.s1Stalld;
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
    // Bug NEW-3 fix: sync s1Stalld_ mirror (was omitted; now also detected in merge above).
    s1Stalld_ = mergedConfig.s1Stalld;
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
            if (pasid > MAX_PASID) {
                return makeSuccess(false);  // Invalid PASID configuration
            }
            
            // Validate AddressSpace is configured properly
            if (!pasidPair.second) {
                return makeSuccess(false);  // Null AddressSpace for configured PASID
            }
        }
    }
    
    // BUG-11 fix / ARM §5.2 STE.S1CDMax + SMMU_IDR1.SSIDSIZE:
    // "The allowable range is 0 to SMMU_IDR1.SSIDSIZE inclusive."
    // SSIDSIZE valid range 0–20; values > 20 are ILLEGAL per §5.2.
    // Values >= 32 additionally cause UB on `1u << s1cdMax` in smmu.cpp.
    if (config.s1cdMax > 20) {
        return makeSuccess(false);  // s1cdMax exceeds SSIDSIZE architectural maximum
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
    
    // ARM SMMU v3: Enabling stream with translation active requires at least one stage
    if (currentConfiguration.translationEnabled && !stage1Enabled && !stage2Enabled) {
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
    // Translation is active if stream is enabled, translation is enabled in config, and at least one stage is enabled.
    // ARM spec: STE.V=1 + STE.Config != 0b000 determines active state, not PASID population.
    return streamEnabled && currentConfiguration.translationEnabled && (stage1Enabled || stage2Enabled);
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
                                                     PASID pasid, StreamID /* streamID */) const {
    std::lock_guard<std::mutex> lock(contextMutex);
    
    // ARM SMMU v3: Validate PASID within specification limits
    if (pasid > MAX_PASID) {
        return makeSuccess(false);  // Invalid PASID
    }
    
    // ARM SMMU v3: ASID validation - uint16_t type already enforces 16-bit range
    
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
    // BUG-19 fix: call the unlocked variant — contextMutex is already held by
    // validateContextDescriptor(), so calling the public validateASIDConfiguration()
    // which also tries to acquire contextMutex would deadlock on the non-reentrant mutex.
    Result<bool> asidValid = validateASIDConfigurationUnlocked(contextDescriptor.asid, pasid,
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
// BUG-19 fix: public method acquires contextMutex, then delegates to the
// lock-free helper.  validateContextDescriptor() calls the helper directly
// because it already holds contextMutex.
Result<bool> StreamContext::validateASIDConfiguration(uint16_t asid, PASID pasid,
                                                     SecurityState securityState) const {
    std::lock_guard<std::mutex> lock(contextMutex);
    return validateASIDConfigurationUnlocked(asid, pasid, securityState);
}

// Lock-free implementation — caller must hold contextMutex.
Result<bool> StreamContext::validateASIDConfigurationUnlocked(uint16_t /* asid */, PASID pasid,
                                                             SecurityState securityState) const {
    // ARM SMMU v3: ASID 0 is reserved in some contexts but may be valid
    // Allow ASID 0 but validate it doesn't conflict with global translations

    // ARM SMMU v3: ASID validation - uint16_t type already enforces 16-bit range

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

    // ARM SMMU v3 §3.10: Four valid security states — NonSecure(0x00), Secure(0x01),
    // Realm(0x02), Root(0x03).  Any value above 0x03 is reserved/invalid.
    if (static_cast<uint8_t>(securityState) > 0x03) {
        return makeSuccess(false);  // Unknown security state (reserved)
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
    
    // ARM SMMU v3 §3.10: Four valid security states — NonSecure(0x00), Secure(0x01),
    // Realm(0x02), Root(0x03).  Any value above 0x03 is reserved/invalid.
    if (static_cast<uint8_t>(streamTableEntry.securityState) > 0x03) {
        return makeSuccess(false);  // Unknown security state (reserved)
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

// Unlocked internal helper methods to eliminate redundant mutex acquisitions
// These methods assume the caller already holds contextMutex

AddressSpace* StreamContext::getPASIDAddressSpaceUnlocked(PASID pasid) {
    // Validate PASID within specification limits
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
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

TranslationResult StreamContext::translateUnlocked(PASID pasid, IOVA iova, AccessType accessType, SecurityState securityState, bool isAtos) {
    // Update translation statistics
    streamStatistics.translationCount++;
    streamStatistics.lastAccessTimestamp = getCurrentTimestamp();

    // §5.2 GAP-2: STRW=EL2/EL3 suppresses AP[1] privilege checks.
    // Convert the caller's access type to its privileged variant when EL2 or EL3.
    // BUG-1 fix: §5.2 states STRW is IGNORED when stage-2 is enabled for Non-secure
    // streams (STE.Config==0b11x).  Guard the promotion with !stage2Enabled to match
    // the TLB fast path in smmu.cpp which already carries this guard.
    AccessType effectiveAccessType = accessType;
    if (!stage2Enabled &&
        (currentConfiguration.strw == StreamWorld::EL2 ||
         currentConfiguration.strw == StreamWorld::EL3)) {
        switch (accessType) {
            case AccessType::Read:      effectiveAccessType = AccessType::ReadPrivileged; break;
            case AccessType::Write:     effectiveAccessType = AccessType::WritePrivileged; break;
            case AccessType::Execute:   effectiveAccessType = AccessType::ExecutePrivileged; break;
            case AccessType::ReadWrite: effectiveAccessType = AccessType::ReadWritePrivileged; break;
            // Bug B fix: ReadExecute in an all-privileged stream promotes to
            // ReadExecutePrivileged (requires BOTH read AND execute, privileged).
            case AccessType::ReadExecute:  effectiveAccessType = AccessType::ReadExecutePrivileged; break;
            default: break;
        }
    }

    // Gap D fix: ARM IHI0070G.b §3.2 / §13.5 — STE.INSTCFG access-type override.
    // INSTCFG must be applied to the *effective* access type before permission checks
    // so that the page-table walk uses the overridden type.
    //   INSTCFG=3 (Force Instruction): treat Read/ReadPrivileged as Execute/ExecutePrivileged.
    //   INSTCFG=2 (Force Data):        treat Execute/ExecutePrivileged as Read/ReadPrivileged.
    //   INSTCFG=0 (Use incoming):      no change.
    //   INSTCFG=1 (Reserved):          no change (passthrough).
    // BUG-13.1.4-CPP-A fix: §13.1.4 — ATOS (GATOS) must NOT apply INSTCFG or PRIVCFG.
    if (!isAtos && currentConfiguration.instCfg == 3u) {
        if (effectiveAccessType == AccessType::Read) {
            effectiveAccessType = AccessType::Execute;
        } else if (effectiveAccessType == AccessType::ReadPrivileged) {
            effectiveAccessType = AccessType::ExecutePrivileged;
        }
        // NOTE: ReadWrite / ReadWritePrivileged are intentionally NOT converted by INSTCFG=1.
        // ARM §5.2: "INSTCFG only affects reads; writes are always considered Data."
        // A compound Read+Write access type cannot have its read component independently
        // forced to Execute without abandoning its write semantics.  Leave as-is.
    } else if (currentConfiguration.instCfg == 2u) {
        if (effectiveAccessType == AccessType::Execute) {
            effectiveAccessType = AccessType::Read;
        } else if (effectiveAccessType == AccessType::ExecutePrivileged) {
            effectiveAccessType = AccessType::ReadPrivileged;
        } else if (effectiveAccessType == AccessType::ReadExecute) {
            // Bug F fix: INSTCFG=2 (Force Data) demotes ReadExecute to Read.
            effectiveAccessType = AccessType::Read;
        } else if (effectiveAccessType == AccessType::ReadExecutePrivileged) {
            // Bug B+F fix: INSTCFG=2 demotes ReadExecutePrivileged to ReadPrivileged.
            effectiveAccessType = AccessType::ReadPrivileged;
        }
    }

    // NEW-4 fix: §3.3.4/§13.5 — STE.PRIVCFG override before permission checks.
    // PRIVCFG transforms the effective access type so that permission checks use
    // the overridden privilege level.
    // BUG-13.1.4-CPP-A fix: ATOS must also skip PRIVCFG per §13.1.4.
    if (!isAtos && currentConfiguration.privCfg == 2u) {
        // Force Unprivileged: strip Privileged suffix
        switch (effectiveAccessType) {
            case AccessType::ReadPrivileged:          effectiveAccessType = AccessType::Read;      break;
            case AccessType::WritePrivileged:         effectiveAccessType = AccessType::Write;     break;
            case AccessType::ExecutePrivileged:       effectiveAccessType = AccessType::Execute;   break;
            case AccessType::ReadWritePrivileged:     effectiveAccessType = AccessType::ReadWrite; break;
            // Bug B fix: demote ReadExecutePrivileged back to ReadExecute.
            case AccessType::ReadExecutePrivileged:   effectiveAccessType = AccessType::ReadExecute; break;
            default: break;
        }
    } else if (currentConfiguration.privCfg == 3u) {
        // Force Privileged: add Privileged suffix
        switch (effectiveAccessType) {
            case AccessType::Read:      effectiveAccessType = AccessType::ReadPrivileged;          break;
            case AccessType::Write:     effectiveAccessType = AccessType::WritePrivileged;         break;
            case AccessType::Execute:   effectiveAccessType = AccessType::ExecutePrivileged;       break;
            case AccessType::ReadWrite: effectiveAccessType = AccessType::ReadWritePrivileged;     break;
            // Bug B fix: ReadExecute promotes to ReadExecutePrivileged (keeps the read requirement).
            case AccessType::ReadExecute:  effectiveAccessType = AccessType::ReadExecutePrivileged; break;
            default: break;
        }
    }

    // GAP-1: Helper lambda to apply STE output-attribute overrides to a TranslationData.
    auto applyOutputAttrs = [&](TranslationData data) -> TranslationData {
        data.memType      = currentConfiguration.mtCfg ? currentConfiguration.memAttr : 0u;
        data.shareability = currentConfiguration.shCfg;
        data.allocHint    = currentConfiguration.allocCfg;
        data.instCfg      = currentConfiguration.instCfg;
        data.privCfg      = currentConfiguration.privCfg;
        data.nsCfgOut     = currentConfiguration.nsCfg;
        // BUG-13.1.7-CPP / BUG-AUDIT-164-CPP fix: §3.15/§13.1.7 Rule 1 — all Device
        // and Normal Non-Cacheable types require OSH regardless of STE.SHCfg.
        // mtCfg=true: STE memAttr override is authoritative (use oshRequired helper).
        // mtCfg=false: page-level attr governs (only binary 0x00/0xFF representable).
        {
            bool needsOsh = currentConfiguration.mtCfg
                ? oshRequired(currentConfiguration.memAttr)
                : (data.pageAttr == 0x00u);
            if (needsOsh) {
                data.shareability = 2u;  // OSH
            }
        }
        return data;
    };

    // ARM SMMU v3: Check if any translation stage is enabled
    if (!stage1Enabled && !stage2Enabled) {
        // §5.2 STE.Config==0b100 / FINDING-NEW-41: Bypass mode — identity mapping (PA == IOVA)
        // with full read/write/execute permissions.  PagePermissions() default-constructs all
        // bits false, which would silently deny every access; bypass must grant full RWX.
        PagePermissions bypassPerms;
        bypassPerms.read    = true;
        bypassPerms.write   = true;
        bypassPerms.execute = true;
        return makeSuccess(applyOutputAttrs(TranslationData(iova, bypassPerms, securityState)));
    }

    // ARM SMMU v3 §5.2 / §7.3.7: stream-enabled check must fire whenever any
    // translation stage is spec-configured, regardless of the translationEnabled flag.
    // Use currentConfiguration.stage1Enabled (the spec-level field) rather than the
    // internal stage1Enabled bool, which defaults true for backward-compat with
    // direct-StreamContext tests (see constructor comment).  A stream explicitly
    // configured with stage1Enabled=true must be rejected when streamEnabled=false,
    // even if translationEnabled=false — the extraneous && translationEnabled guard
    // allowed disabled-but-configured streams to translate via Stage-1.
    if ((currentConfiguration.stage1Enabled || currentConfiguration.stage2Enabled) && !streamEnabled) {
        // Translation is configured but stream is disabled - fail translation
        streamStatistics.faultCount++;  // Track fault
        return makeTranslationError(SMMUError::StreamDisabled);
    }

    // Validate PASID within ARM SMMU v3 specification limits
    // PASID 0 is valid and commonly used for kernel/hypervisor contexts per ARM SMMU v3 specification
    if (pasid > MAX_PASID) {
        streamStatistics.faultCount++;  // Track fault
        // Note: Fault will be recorded by SMMU controller with proper StreamID
        return makeTranslationError(SMMUError::InvalidPASID);
    }

    IPA intermediatePA = iova;  // Start with input address

    // ARM SMMU v3: Stage-1 translation (per-PASID address space)
    TranslationResult stage1Result = makeTranslationError(SMMUError::InternalError);
    if (stage1Enabled) {
        // Find PASID in map
        auto it = pasidMap.find(pasid);
        if (it == pasidMap.end()) {
            // PASID not found - return proper PASID error
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return makeTranslationError(SMMUError::PASIDNotFound);
        }

        // Optimization 5: Use raw pointer instead of copying shared_ptr
        AddressSpace* stage1AddressSpace = it->second.get();
        if (!stage1AddressSpace) {
            // Null AddressSpace - translation fault
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return makeTranslationError(SMMUError::InternalError);
        }

        // Perform Stage-1 translation (IOVA -> IPA)
        // NEW-GAP-J: pass ha and affd so translatePage can enforce the AF fault rule.
        // Use (ha || currentConfiguration.ha) so both the direct setHardwareAccessFlag()
        // path (private member) and the configureStream() path (currentConfiguration.ha)
        // correctly suppress AF faults when HTTU is enabled.
        stage1Result = stage1AddressSpace->translatePage(iova, effectiveAccessType, securityState,
                                                          (ha || currentConfiguration.ha),
                                                          currentConfiguration.affd);
        if (stage1Result.isError()) {
            // Stage-1 translation failed - propagate fault
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return stage1Result;
        }

        // §5.4 NEW-GAP-K: WXN/UWXN write-execute-never permission override.
        // Applied after stage-1 succeeds, before stage-2 translation.
        {
            bool isExec = (effectiveAccessType == AccessType::Execute ||
                           effectiveAccessType == AccessType::ExecutePrivileged ||
                           effectiveAccessType == AccessType::ReadExecute ||
                           effectiveAccessType == AccessType::ReadExecutePrivileged);
            if (isExec) {
                const PagePermissions& s1p = stage1Result.getValue().permissions;
                // WXN: writable page is execute-never for all execute accesses.
                if (currentConfiguration.wxn && s1p.write) {
                    streamStatistics.faultCount++;
                    return makeTranslationError(FaultType::PermissionFault);
                }
                // UWXN: privileged execute on an unprivileged-writable page is forbidden.
                // ARM §5.4: UWXN is IGNORED when all accesses are privileged, i.e.
                // when strw==EL2 or strw==EL3.  EL2_E2H is explicitly excluded from
                // this exemption per §3.3.4.
                bool allPrivilegedStream = (currentConfiguration.strw == StreamWorld::EL2 ||
                                            currentConfiguration.strw == StreamWorld::EL3);
                if (currentConfiguration.uwxn && !allPrivilegedStream &&
                    (effectiveAccessType == AccessType::ExecutePrivileged ||
                     effectiveAccessType == AccessType::ReadExecutePrivileged) &&
                    s1p.write && !s1p.privilegedOnly) {
                    streamStatistics.faultCount++;
                    return makeTranslationError(FaultType::PermissionFault);
                }
            }
        }

        // ARM §3.13: Update AF/dirty bits when hardware management enabled
        // BUG-NEW-CPP-B fix: §3.13.2 — check both the direct setter path (ha/hd
        // private members) and the configureStream() path (currentConfiguration.ha/hd)
        // so that HTTU updates fire regardless of how the stream was configured.
        if ((ha || currentConfiguration.ha) || (hd || currentConfiguration.hd)) {
            AddressSpace* as1 = getPASIDAddressSpaceUnlocked(pasid);
            if (as1) {
                as1->updateAccessFlags(iova, (ha || currentConfiguration.ha),
                                       (hd || currentConfiguration.hd), effectiveAccessType);
            }
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
        // Use Read access type for Stage-2 address lookup — permission intersection
        // is applied below; we don't want Stage-2 to independently reject a write
        // that Stage-1 already accepted (the intersection handles that).
        // BUG-NEW2-05 fix: use AccessType::Read for Stage-2 lookup so that Stage-2
        // does not independently deny writes before the Stage-1∩Stage-2 permission
        // intersection below.  The intersection applies the original accessType to
        // the combined permissions.
        // BUG-CPP-DBGR-10 fix: §3.10.2 — the Stage-2 PTW must use the Stage-1 output
        // security state (stage1Result output), not the incoming transaction's securityState.
        // For NS streams both are equal (NonSecure), but for Secure streams with NS-output
        // overrides this distinction is load-bearing.
        SecurityState ipaSecurityState = stage1Result.isOk()
            ? stage1Result.getValue().securityState
            : securityState;
        // NEW-GAP-J: pass s2ha and s2affd for stage-2 AF fault enforcement.
        TranslationResult stage2Result = stage2AddressSpace->translatePage(intermediatePA, AccessType::Read,
                                                                            ipaSecurityState,
                                                                            currentConfiguration.s2ha,
                                                                            currentConfiguration.s2affd);
        if (stage2Result.isError()) {
            // Stage-2 translation failed - propagate fault
            streamStatistics.faultCount++;  // Track fault
            // Note: Fault will be recorded by SMMU controller with proper StreamID
            return stage2Result;
        }

        // §5.2 NEW-GAP-L: S2PTW — stage-2 translation through a Device-type page is
        // a Permission fault. This prevents table walk from landing in Device MMIO.
        if (currentConfiguration.s2ptw &&
            stage2AddressSpace->getPageDeviceMemory(intermediatePA)) {
            streamStatistics.faultCount++;
            return makeTranslationError(FaultType::PermissionFault);
        }

        // ARM §3.3.1 / FINDING-NEW-38: effective permissions = Stage-1 ∩ Stage-2.
        // When Stage-1 is enabled, its permissions restrict what Stage-2 permits.
        PagePermissions intersected;
        if (stage1Enabled && stage1Result.isOk()) {
            const PagePermissions& s1perms = stage1Result.getValue().permissions;
            const PagePermissions& s2perms = stage2Result.getValue().permissions;
            intersected.read          = s1perms.read    && s2perms.read;
            intersected.write         = s1perms.write   && s2perms.write;
            intersected.execute       = s1perms.execute && s2perms.execute;
            intersected.privilegedOnly = s1perms.privilegedOnly || s2perms.privilegedOnly;
        } else {
            // Stage-2 only — no Stage-1 to intersect with
            intersected = stage2Result.getValue().permissions;
        }

        // Validate the intersected permissions against the effective access type
        bool permOk = false;
        switch (effectiveAccessType) {
            case AccessType::Read:                permOk = intersected.read && !intersected.privilegedOnly; break;
            case AccessType::Write:               permOk = intersected.write && !intersected.privilegedOnly; break;
            case AccessType::Execute:             permOk = intersected.execute && !intersected.privilegedOnly; break;
            case AccessType::ReadWrite:           permOk = intersected.read && intersected.write && !intersected.privilegedOnly; break;
            // Bug-6: ReadExecute combined access type.
            case AccessType::ReadExecute:             permOk = intersected.read && intersected.execute && !intersected.privilegedOnly; break;
            case AccessType::ReadPrivileged:          permOk = intersected.read; break;
            case AccessType::WritePrivileged:         permOk = intersected.write; break;
            case AccessType::ExecutePrivileged:       permOk = intersected.execute; break;
            case AccessType::ReadWritePrivileged:     permOk = intersected.read && intersected.write; break;
            // Bug B fix: ReadExecutePrivileged — privileged combined read+execute (no privilegedOnly restriction).
            case AccessType::ReadExecutePrivileged:   permOk = intersected.read && intersected.execute; break;
            default: permOk = false; break;
        }
        if (!permOk) {
            streamStatistics.faultCount++;
            return makeTranslationError(SMMUError::PagePermissionViolation);
        }

        // Stage-2 success - return final physical address with intersected permissions.
        // BUG-13.1.5-CPP fix: combine S1+S2 pageAttr per §13.1.5 strength ordering.
        // Device (0x00) is strongest and wins over Normal (0xFF) from either stage.
        TranslationData twoStageTd(stage2Result.getValue().physicalAddress,
                                   intersected,
                                   stage2Result.getValue().securityState);
        {
            uint8_t s1pa = (stage1Enabled && stage1Result.isOk())
                               ? stage1Result.getValue().pageAttr
                               : 0xFFu;
            uint8_t s2pa = stage2Result.getValue().pageAttr;
            twoStageTd.pageAttr = (s1pa == 0x00u || s2pa == 0x00u) ? 0x00u : s2pa;
        }

        // BUG-AUDIT-130 fix: ARM §3.13 / §3.13.4 — When S2HA=1, the SMMU must update
        // the stage-2 access flag on a successful translation, mirroring the stage-1 HA
        // update path above.  translatePage() is const and only gates AF faults; it does
        // not write AF.  The write must be done here after stage-2 translation succeeds.
        if (currentConfiguration.s2ha || currentConfiguration.s2affd) {
            stage2AddressSpace->updateAccessFlags(intermediatePA,
                                                  currentConfiguration.s2ha,
                                                  /*hd=*/false,  // S2HD always rejected (HTTU=0b01)
                                                  effectiveAccessType);
        }

        return makeSuccess(applyOutputAttrs(twoStageTd));
    }

    // Only Stage-1 enabled: return the Stage-1 result directly.
    if (stage1Enabled && stage1Result.isOk()) {
        TranslationData s1data(intermediatePA,
                               stage1Result.getValue().permissions,
                               stage1Result.getValue().securityState);
        // BUG-13.1.7-CPP fix: propagate per-page memory type so that applyOutputAttrs
        // can enforce ARM §13.1.7 Rule 1 (Device/NC → OSH) via data.pageAttr check.
        s1data.pageAttr = stage1Result.getValue().pageAttr;
        // BUG-8 fix: ARM §13.4.1 — for EL2 (non-VHE) and EL3 StreamWorld, AP[1]
        // is ignored (treated as 1).  The output PRIV attribute must reflect the
        // effective access privilege, not the raw page-descriptor AP[1] bit.
        // Clear privilegedOnly on the output TranslationData for these worlds.
        // EL2_E2H (VHE) is excluded — it maintains EL1-like privilege checks.
        // The guard !stage2Enabled matches the STRW promotion guard above (STRW
        // is ignored when stage-2 is active for Non-secure streams per §5.2).
        if (!stage2Enabled &&
            (currentConfiguration.strw == StreamWorld::EL2 ||
             currentConfiguration.strw == StreamWorld::EL3)) {
            s1data.permissions.privilegedOnly = false;
        }
        return makeSuccess(applyOutputAttrs(s1data));
    }

    // BUG-08 fix: Bypass / identity-mapping mode (neither stage enabled).
    // PagePermissions() default-constructs with all bits false, which would
    // silently deny every access.  In bypass mode the ARM SMMU v3 spec passes
    // the transaction through unchanged, so grant full read/write/execute access.
    PagePermissions bypassPermissions;
    bypassPermissions.read = true;
    bypassPermissions.write = true;
    bypassPermissions.execute = true;
    return makeSuccess(applyOutputAttrs(TranslationData(intermediatePA, bypassPermissions, securityState)));
}

} // namespace smmu