# ARM SMMU v3 API Documentation

This document provides comprehensive API documentation for the ARM SMMU v3 C++11 implementation, covering all public classes, methods, types, and interfaces.

## Table of Contents

1. [Core Types and Enums](#core-types-and-enums)
2. [Result System](#result-system)
3. [SMMU Main Controller](#smmu-main-controller)
4. [AddressSpace Class](#addressspace-class)
5. [StreamContext Class](#streamcontext-class)
6. [Configuration Types](#configuration-types)
7. [Event and Command System](#event-and-command-system)
8. [Error Handling](#error-handling)
9. [Usage Examples](#usage-examples)

---

## Core Types and Enums

### Basic Address Types

```cpp
namespace smmu {
    using StreamID = uint32_t;    // Stream identifier (0 to MAX_STREAM_ID)
    using PASID = uint32_t;       // Process Address Space ID
    using IOVA = uint64_t;        // Input/Virtual Address
    using IPA = uint64_t;         // Intermediate Physical Address
    using PA = uint64_t;          // Physical Address
}
```

### Access and Security Types

```cpp
enum class AccessType {
    Read,           // Read access request
    Write,          // Write access request  
    Execute         // Execute/instruction fetch access
};

enum class SecurityState {
    NonSecure,      // Non-secure access
    Secure,         // Secure access
    Realm           // Realm Management Extension access
};

enum class TranslationStage {
    Stage1Only,     // Stage 1 translation only
    Stage2Only,     // Stage 2 translation only
    BothStages,     // Two-stage translation
    Disabled        // Translation disabled
};
```

### Fault and Permission Types

```cpp
enum class FaultType {
    TranslationFault,                    // Page not mapped
    PermissionFault,                     // Access permission violation
    AddressSizeFault,                    // Address size mismatch
    AccessFault,                         // Access flag fault
    SecurityFault,                       // Security violation
    ContextDescriptorFormatFault,        // Invalid context descriptor
    TranslationTableFormatFault,         // Invalid translation table
    Level0TranslationFault,              // Level 0 translation fault
    Level1TranslationFault,              // Level 1 translation fault
    Level2TranslationFault,              // Level 2 translation fault
    Level3TranslationFault,              // Level 3 translation fault
    AccessFlagFault,                     // Access flag not set
    DirtyBitFault,                       // Dirty bit fault
    TLBConflictFault,                    // TLB conflict
    ExternalAbort,                       // External abort
    SynchronousExternalAbort,            // Synchronous external abort
    AsynchronousExternalAbort,           // Asynchronous external abort
    StreamTableFormatFault,              // Stream table format error
    ConfigurationCacheFault,             // Configuration cache fault
    Stage2TranslationFault,              // Stage 2 translation fault
    Stage2PermissionFault                // Stage 2 permission fault
};

enum class FaultMode {
    Terminate,      // Terminate faulting transaction
    Stall           // Stall and wait for software intervention
};

struct PagePermissions {
    bool read;      // Read permission
    bool write;     // Write permission  
    bool execute;   // Execute permission
    
    PagePermissions(bool r = false, bool w = false, bool x = false)
        : read(r), write(w), execute(x) {}
};
```

---

## Result System

The SMMU implementation uses a `Result<T>` type for error handling instead of exceptions, following modern C++ best practices.

### Result Template Class

```cpp
template<typename T>
class Result {
public:
    // Constructors
    Result(const T& value);                    // Success result
    Result(T&& value);                         // Move success result
    Result(SMMUError error);                   // Error result
    
    // Status checking
    bool isOk() const;                         // Check if successful
    bool isError() const;                      // Check if error
    operator bool() const;                     // Convert to bool (true if success)
    
    // Value access
    const T& getValue() const;                 // Get value (throws if error)
    T&& moveValue();                           // Move value out
    const T& getValueOr(const T& defaultValue) const;  // Get value or default
    
    // Error access
    SMMUError getError() const;                // Get error code
    
    // Factory methods
    static Result<T> makeSuccess(const T& value);
    static Result<T> makeSuccess(T&& value);
    static Result<T> makeError(SMMUError error);
};

// Specialized void result
using VoidResult = Result<Unit>;
VoidResult makeVoidSuccess();
VoidResult makeVoidError(SMMUError error);
```

### Translation Result

```cpp
using TranslationResult = Result<TranslationData>;

struct TranslationData {
    PA physicalAddress;                // Translated physical address
    PagePermissions permissions;       // Access permissions
    SecurityState securityState;       // Security state
    
    TranslationData(PA pa, PagePermissions perms, SecurityState security);
};

// Helper functions
TranslationResult makeTranslationSuccess(PA physicalAddress, 
                                       PagePermissions permissions,
                                       SecurityState securityState = SecurityState::NonSecure);
TranslationResult makeTranslationError(SMMUError error);
TranslationResult makeTranslationError(FaultType faultType);
```

---

## SMMU Main Controller

The `SMMU` class is the primary interface for the ARM SMMU v3 implementation.

### Core Translation API

```cpp
class SMMU {
public:
    // Constructor and Destructor
    SMMU();
    ~SMMU();
    
    /**
     * @brief Perform address translation
     * @param streamID Stream identifier
     * @param pasid Process Address Space ID
     * @param iova Input Virtual Address to translate
     * @param access Type of access (Read/Write/Execute)
     * @return TranslationResult containing physical address or error
     */
    TranslationResult translate(StreamID streamID, PASID pasid, 
                               IOVA iova, AccessType access);
};
```

### Stream Management

```cpp
public:
    /**
     * @brief Configure a stream with specified parameters
     * @param streamID Stream to configure
     * @param config Stream configuration parameters
     * @return VoidResult indicating success or error
     */
    VoidResult configureStream(StreamID streamID, const StreamConfig& config);
    
    /**
     * @brief Remove stream configuration
     * @param streamID Stream to remove
     * @return VoidResult indicating success or error
     */
    VoidResult removeStream(StreamID streamID);
    
    /**
     * @brief Check if stream is configured
     * @param streamID Stream to check
     * @return bool true if configured
     */
    bool isStreamConfigured(StreamID streamID) const;
    
    /**
     * @brief Enable translation for a stream
     * @param streamID Stream to enable
     * @return VoidResult indicating success or error
     */
    VoidResult enableStream(StreamID streamID);
    
    /**
     * @brief Disable translation for a stream
     * @param streamID Stream to disable
     * @return VoidResult indicating success or error
     */
    VoidResult disableStream(StreamID streamID);
    
    /**
     * @brief Check if stream translation is enabled
     * @param streamID Stream to check
     * @return bool true if enabled
     */
    bool isStreamEnabled(StreamID streamID) const;
```

### PASID Management

```cpp
public:
    /**
     * @brief Create new PASID for a stream
     * @param streamID Target stream
     * @param pasid PASID to create
     * @return VoidResult indicating success or error
     */
    VoidResult createStreamPASID(StreamID streamID, PASID pasid);
    
    /**
     * @brief Remove PASID from a stream
     * @param streamID Target stream
     * @param pasid PASID to remove
     * @return VoidResult indicating success or error
     */
    VoidResult removeStreamPASID(StreamID streamID, PASID pasid);
```

### Page Table Management

```cpp
public:
    /**
     * @brief Map virtual page to physical address
     * @param streamID Target stream
     * @param pasid Process Address Space ID
     * @param iova Input Virtual Address
     * @param physicalAddress Physical address to map to
     * @param permissions Access permissions for the page
     * @param securityState Security state for the mapping
     * @return VoidResult indicating success or error
     */
    VoidResult mapPage(StreamID streamID, PASID pasid, IOVA iova, 
                      PA physicalAddress, PagePermissions permissions,
                      SecurityState securityState = SecurityState::NonSecure);
    
    /**
     * @brief Unmap virtual page
     * @param streamID Target stream
     * @param pasid Process Address Space ID
     * @param iova Input Virtual Address to unmap
     * @return VoidResult indicating success or error
     */
    VoidResult unmapPage(StreamID streamID, PASID pasid, IOVA iova);
```

### Event and Fault Management

```cpp
public:
    /**
     * @brief Get pending fault events
     * @return Vector of FaultRecord entries
     */
    std::vector<FaultRecord> getEvents();
    
    /**
     * @brief Clear all pending events
     */
    void clearEvents();
    
    /**
     * @brief Set global fault handling mode
     * @param mode Fault mode (Terminate or Stall)
     */
    void setGlobalFaultMode(FaultMode mode);
    
    /**
     * @brief Check if there are pending events
     * @return bool true if events are pending
     */
    bool hasEvents() const;
    
    /**
     * @brief Get event queue size
     * @return size_t current number of events
     */
    size_t getEventQueueSize() const;
```

### Cache Management

```cpp
public:
    /**
     * @brief Enable or disable TLB caching
     * @param enabled true to enable caching
     */
    void enableCaching(bool enabled);
    
    /**
     * @brief Invalidate all translation cache entries
     */
    void invalidateTranslationCache();
    
    /**
     * @brief Invalidate cache entries for specific stream
     * @param streamID Stream to invalidate
     */
    void invalidateStreamCache(StreamID streamID);
    
    /**
     * @brief Invalidate cache entries for specific PASID
     * @param streamID Target stream
     * @param pasid PASID to invalidate
     */
    void invalidatePASIDCache(StreamID streamID, PASID pasid);
```

### Statistics and Performance

```cpp
public:
    /**
     * @brief Get total number of configured streams
     * @return uint32_t stream count
     */
    uint32_t getStreamCount() const;
    
    /**
     * @brief Get total translation count
     * @return uint64_t total translations performed
     */
    uint64_t getTotalTranslations() const;
    
    /**
     * @brief Get total fault count
     * @return uint64_t total faults recorded
     */
    uint64_t getTotalFaults() const;
    
    /**
     * @brief Get translation count for specific stream
     * @param streamID Target stream
     * @return uint64_t translation count
     */
    uint64_t getTranslationCount(StreamID streamID) const;
    
    /**
     * @brief Get cache hit count
     * @return uint64_t cache hits
     */
    uint64_t getCacheHitCount() const;
    
    /**
     * @brief Get cache miss count  
     * @return uint64_t cache misses
     */
    uint64_t getCacheMissCount() const;
    
    /**
     * @brief Get comprehensive cache statistics
     * @return CacheStatistics structure
     */
    CacheStatistics getCacheStatistics() const;
    
    /**
     * @brief Reset all statistics counters
     */
    void resetStatistics();
```

---

## AddressSpace Class

The `AddressSpace` class manages virtual-to-physical address mappings for a single address space.

### Core Translation Methods

```cpp
class AddressSpace {
public:
    // Constructor and Destructor
    AddressSpace();
    ~AddressSpace();
    
    /**
     * @brief Map virtual page to physical address
     * @param iova Input Virtual Address (page-aligned)
     * @param physicalAddress Physical address to map to
     * @param permissions Access permissions
     * @param securityState Security state of mapping
     * @return VoidResult indicating success or error
     */
    VoidResult mapPage(IOVA iova, PA physicalAddress, 
                      PagePermissions permissions,
                      SecurityState securityState = SecurityState::NonSecure);
    
    /**
     * @brief Remove virtual page mapping
     * @param iova Input Virtual Address to unmap
     * @return VoidResult indicating success or error
     */
    VoidResult unmapPage(IOVA iova);
    
    /**
     * @brief Translate virtual address to physical
     * @param iova Input Virtual Address
     * @param access Type of access being performed
     * @return TranslationResult with physical address or fault
     */
    TranslationResult translatePage(IOVA iova, AccessType access);
```

### Bulk Operations

```cpp
public:
    /**
     * @brief Map contiguous virtual range to physical range
     * @param startIova Start of virtual range
     * @param startPhysical Start of physical range
     * @param size Size of range in bytes
     * @param permissions Access permissions
     * @param securityState Security state
     * @return VoidResult indicating success or error
     */
    VoidResult mapRange(IOVA startIova, PA startPhysical, size_t size,
                       PagePermissions permissions,
                       SecurityState securityState = SecurityState::NonSecure);
    
    /**
     * @brief Unmap contiguous virtual range
     * @param startIova Start of virtual range
     * @param size Size of range in bytes
     * @return VoidResult indicating success or error
     */
    VoidResult unmapRange(IOVA startIova, size_t size);
    
    /**
     * @brief Map multiple individual pages
     * @param mappings Vector of page mappings
     * @return VoidResult indicating success or error
     */
    VoidResult mapPages(const std::vector<PageEntry>& mappings);
    
    /**
     * @brief Unmap multiple individual pages
     * @param addresses Vector of virtual addresses to unmap
     * @return VoidResult indicating success or error
     */
    VoidResult unmapPages(const std::vector<IOVA>& addresses);
```

### Query Methods

```cpp
public:
    /**
     * @brief Check if virtual page is mapped
     * @param iova Input Virtual Address
     * @return bool true if mapped
     */
    bool isPageMapped(IOVA iova) const;
    
    /**
     * @brief Get permissions for mapped page
     * @param iova Input Virtual Address
     * @return Result<PagePermissions> permissions or error
     */
    Result<PagePermissions> getPagePermissions(IOVA iova) const;
    
    /**
     * @brief Get total number of mapped pages
     * @return size_t page count
     */
    size_t getPageCount() const;
    
    /**
     * @brief Get all mapped address ranges
     * @return std::vector<AddressRange> list of mapped ranges
     */
    std::vector<AddressRange> getMappedRanges() const;
    
    /**
     * @brief Get total address space size covered
     * @return uint64_t size in bytes
     */
    uint64_t getAddressSpaceSize() const;
    
    /**
     * @brief Check for overlapping mappings
     * @return bool true if overlaps exist
     */
    bool hasOverlappingMappings() const;
```

### Cache Management

```cpp
public:
    /**
     * @brief Clear all mappings
     */
    void clear();
    
    /**
     * @brief Invalidate cache for address range
     * @param startIova Start address
     * @param size Range size
     */
    void invalidateRange(IOVA startIova, size_t size);
    
    /**
     * @brief Invalidate all cached translations
     */
    void invalidateAll();
```

---

## StreamContext Class

The `StreamContext` class manages per-stream state and PASID address spaces.

### PASID Management

```cpp
class StreamContext {
public:
    // Constructor and Destructor
    StreamContext();
    ~StreamContext();
    
    /**
     * @brief Create new PASID with dedicated address space
     * @param pasid PASID to create
     * @return VoidResult indicating success or error
     */
    VoidResult createPASID(PASID pasid);
    
    /**
     * @brief Remove PASID and its address space
     * @param pasid PASID to remove
     * @return VoidResult indicating success or error
     */
    VoidResult removePASID(PASID pasid);
    
    /**
     * @brief Add existing PASID with shared address space
     * @param pasid PASID to add
     * @param addressSpace Shared address space
     * @return VoidResult indicating success or error
     */
    VoidResult addPASID(PASID pasid, std::shared_ptr<AddressSpace> addressSpace);
    
    /**
     * @brief Check if PASID exists
     * @param pasid PASID to check
     * @return bool true if exists
     */
    bool hasPASID(PASID pasid) const;
    
    /**
     * @brief Get number of configured PASIDs
     * @return uint32_t PASID count
     */
    uint32_t getPASIDCount() const;
```

### Translation Operations

```cpp
public:
    /**
     * @brief Translate virtual address for specific PASID
     * @param pasid Target PASID
     * @param iova Input Virtual Address
     * @param access Access type
     * @return TranslationResult with physical address or fault
     */
    TranslationResult translate(PASID pasid, IOVA iova, AccessType access);
    
    /**
     * @brief Map page in PASID address space
     * @param pasid Target PASID
     * @param iova Virtual address
     * @param physicalAddress Physical address
     * @param permissions Access permissions
     * @param securityState Security state
     * @return VoidResult indicating success or error
     */
    VoidResult mapPage(PASID pasid, IOVA iova, PA physicalAddress,
                      PagePermissions permissions,
                      SecurityState securityState = SecurityState::NonSecure);
    
    /**
     * @brief Unmap page from PASID address space
     * @param pasid Target PASID  
     * @param iova Virtual address to unmap
     * @return VoidResult indicating success or error
     */
    VoidResult unmapPage(PASID pasid, IOVA iova);
```

### Configuration Management

```cpp
public:
    /**
     * @brief Enable/disable Stage 1 translation
     * @param enabled true to enable Stage 1
     */
    void setStage1Enabled(bool enabled);
    
    /**
     * @brief Enable/disable Stage 2 translation
     * @param enabled true to enable Stage 2
     */
    void setStage2Enabled(bool enabled);
    
    /**
     * @brief Set Stage 2 address space
     * @param addressSpace Stage 2 address space
     */
    void setStage2AddressSpace(std::shared_ptr<AddressSpace> addressSpace);
    
    /**
     * @brief Set fault handling mode
     * @param mode Fault mode (Terminate or Stall)
     */
    void setFaultMode(FaultMode mode);
    
    /**
     * @brief Check if Stage 1 is enabled
     * @return bool true if enabled
     */
    bool isStage1Enabled() const;
    
    /**
     * @brief Check if Stage 2 is enabled
     * @return bool true if enabled
     */
    bool isStage2Enabled() const;
```

### Address Space Access

```cpp
public:
    /**
     * @brief Get PASID address space
     * @param pasid Target PASID
     * @return Result<std::shared_ptr<AddressSpace>> address space or error
     */
    Result<std::shared_ptr<AddressSpace>> getPASIDAddressSpace(PASID pasid);
    
    /**
     * @brief Get Stage 2 address space
     * @return std::shared_ptr<AddressSpace> Stage 2 address space
     */
    std::shared_ptr<AddressSpace> getStage2AddressSpace() const;
    
    /**
     * @brief Clear all PASIDs
     */
    void clearAllPASIDs();
```

---

## Configuration Types

### Stream Configuration

```cpp
struct StreamConfig {
    bool translationEnabled;        // Enable address translation
    bool stage1Enabled;            // Enable Stage 1 translation
    bool stage2Enabled;            // Enable Stage 2 translation
    FaultMode faultMode;           // Fault handling mode
    
    StreamConfig(bool translation = true, bool s1 = true, bool s2 = false,
                FaultMode fault = FaultMode::Terminate);
};
```

### Address Range

```cpp
struct AddressRange {
    IOVA startAddress;             // Start of address range
    IOVA endAddress;               // End of address range
    
    AddressRange(IOVA start, IOVA end);
    
    /**
     * @brief Get size of address range
     * @return uint64_t size in bytes
     */
    uint64_t size() const;
    
    /**
     * @brief Check if address is in range
     * @param address Address to check
     * @return bool true if contained
     */
    bool contains(IOVA address) const;
    
    /**
     * @brief Check if ranges overlap
     * @param other Other address range
     * @return bool true if overlapping
     */
    bool overlaps(const AddressRange& other) const;
};
```

### Page Entry

```cpp
struct PageEntry {
    PA physicalAddress;            // Physical address
    PagePermissions permissions;   // Access permissions
    bool valid;                    // Entry is valid
    SecurityState securityState;   // Security state
    
    PageEntry();
    PageEntry(PA pa, PagePermissions perms, 
             SecurityState security = SecurityState::NonSecure);
};
```

---

## Event and Command System

### Fault Records

```cpp
struct FaultRecord {
    StreamID streamID;             // Faulting stream
    PASID pasid;                  // Faulting PASID
    IOVA address;                 // Faulting address
    FaultType faultType;          // Type of fault
    AccessType accessType;        // Access that caused fault
    SecurityState securityState;  // Security state
    FaultSyndrome syndrome;       // Detailed fault information
    uint64_t timestamp;           // Fault timestamp
    
    FaultRecord();
    FaultRecord(StreamID sid, PASID p, IOVA addr, FaultType type,
               AccessType access, SecurityState security);
};
```

### Fault Syndrome

```cpp
struct FaultSyndrome {
    uint32_t syndromeRegister;         // Raw syndrome register value
    FaultStage faultingStage;          // Stage that faulted
    uint8_t faultLevel;                // Translation table level
    PrivilegeLevel privilegeLevel;     // Privilege level of access
    AccessClassification accessClass;  // Access classification
    bool writeNotRead;                 // Write (true) or read (false)
    bool validSyndrome;                // Syndrome is valid
    uint16_t contextDescriptorIndex;   // Context descriptor index
    
    FaultSyndrome();
    FaultSyndrome(uint32_t syndrome, FaultStage stage);
};
```

### Command System

```cpp
enum class CommandType {
    PREFETCH_CONFIG,               // Prefetch configuration
    PREFETCH_ADDR,                // Prefetch address
    CFGI_STE,                     // Invalidate stream table entry
    CFGI_ALL,                     // Invalidate all configurations
    TLBI_NH_ALL,                  // Invalidate all non-hypervisor TLBs
    TLBI_EL2_ALL,                 // Invalidate all EL2 TLBs
    TLBI_S12_VMALL,               // Invalidate Stage 1&2 VM TLBs
    ATC_INV,                      // ATC invalidate
    PRI_RESP,                     // PRI response
    RESUME,                       // Resume stalled transaction
    SYNC                          // Synchronization barrier
};

struct CommandEntry {
    CommandType type;              // Command type
    StreamID streamID;            // Target stream (if applicable)
    PASID pasid;                  // Target PASID (if applicable)
    IOVA startAddress;            // Start address for range commands
    IOVA endAddress;              // End address for range commands
    uint32_t flags;               // Command-specific flags
    uint64_t timestamp;           // Command timestamp
    
    CommandEntry();
    CommandEntry(CommandType cmdType, StreamID sid = 0);
};
```

---

## Error Handling

### SMMU Error Codes

```cpp
enum class SMMUError {
    // Success
    Success = 0,
    
    // Stream errors
    InvalidStreamID,               // Stream ID out of range
    StreamNotConfigured,           // Stream not configured
    StreamAlreadyConfigured,       // Stream already configured
    StreamDisabled,                // Stream is disabled
    StreamNotFound,                // Stream not found
    StreamConfigurationError,      // Stream configuration error
    
    // PASID errors  
    InvalidPASID,                  // PASID out of range
    PASIDNotFound,                 // PASID not found
    PASIDAlreadyExists,           // PASID already exists
    PASIDLimitExceeded,           // Too many PASIDs
    PASIDPermissionDenied,        // PASID access denied
    
    // Address and permission errors
    InvalidAddress,                // Invalid address
    InvalidPermissions,            // Invalid permissions
    PageNotMapped,                 // Page not mapped
    PageAlreadyMapped,             // Page already mapped
    PagePermissionViolation,       // Permission violation
    AddressSpaceExhausted,         // Address space full
    
    // Translation errors
    TranslationTableError,         // Translation table error
    InvalidSecurityState,          // Invalid security state
    
    // Cache errors
    CacheOperationFailed,          // Cache operation failed
    CacheEntryNotFound,            // Cache entry not found
    CacheEvictionFailed,           // Cache eviction failed
    InvalidCacheOperation,         // Invalid cache operation
    
    // Fault handling errors
    FaultHandlingError,            // Fault handling error
    FaultRecordCorrupted,          // Fault record corrupted
    FaultQueueFull,                // Fault queue full
    UnknownFaultType,              // Unknown fault type
    
    // Queue errors
    CommandQueueFull,              // Command queue full
    EventQueueFull,                // Event queue full
    PRIQueueFull,                  // PRI queue full
    InvalidCommandType,            // Invalid command type
    CommandProcessingFailed,       // Command processing failed
    
    // System errors
    ResourceExhausted,             // System resources exhausted
    InternalError,                 // Internal error
    NotImplemented,                // Feature not implemented
    HardwareError,                 // Hardware error
    ConfigurationError,            // Configuration error
    ParseError,                    // Parse error
    SpecViolation,                 // Specification violation
    UnsupportedFeature,            // Unsupported feature
    InvalidConfiguration,          // Invalid configuration
    StateTransitionError           // State transition error
};
```

### Error Conversion Functions

```cpp
/**
 * @brief Convert fault type to SMMU error code
 * @param faultType Fault type to convert
 * @return SMMUError corresponding error code
 */
SMMUError faultTypeToSMMUError(FaultType faultType);

/**
 * @brief Convert SMMU error to fault type
 * @param error SMMU error to convert
 * @return FaultType corresponding fault type
 */
FaultType smmUErrorToFaultType(SMMUError error);
```

---

## Usage Examples

### Basic SMMU Setup and Translation

```cpp
#include "smmu/smmu.h"
#include "smmu/types.h"

int main() {
    // Create SMMU instance
    smmu::SMMU smmuController;
    
    // Configure a stream
    smmu::StreamConfig config(true, true, false, smmu::FaultMode::Terminate);
    auto result = smmuController.configureStream(100, config);
    if (!result) {
        std::cerr << "Failed to configure stream: " << static_cast<int>(result.getError()) << std::endl;
        return 1;
    }
    
    // Create PASID for the stream
    result = smmuController.createStreamPASID(100, 1);
    if (!result) {
        std::cerr << "Failed to create PASID" << std::endl;
        return 1;
    }
    
    // Map a page
    smmu::PagePermissions perms(true, true, false); // Read/Write, no Execute
    result = smmuController.mapPage(100, 1, 0x1000, 0x2000, perms);
    if (!result) {
        std::cerr << "Failed to map page" << std::endl;
        return 1;
    }
    
    // Enable the stream
    result = smmuController.enableStream(100);
    if (!result) {
        std::cerr << "Failed to enable stream" << std::endl;
        return 1;
    }
    
    // Perform translation
    auto translation = smmuController.translate(100, 1, 0x1000, smmu::AccessType::Read);
    if (translation) {
        std::cout << "Translation successful: 0x" << std::hex 
                  << translation.getValue().physicalAddress << std::endl;
    } else {
        std::cerr << "Translation failed: " << static_cast<int>(translation.getError()) << std::endl;
    }
    
    return 0;
}
```

### Advanced Address Space Management

```cpp
#include "smmu/address_space.h"

void setupComplexAddressSpace() {
    smmu::AddressSpace addressSpace;
    
    // Map individual pages
    smmu::PagePermissions readOnlyPerms(true, false, false);
    smmu::PagePermissions readWritePerms(true, true, false);
    smmu::PagePermissions executePerms(false, false, true);
    
    // Map code section (executable)
    auto result = addressSpace.mapRange(0x10000, 0x20000, 0x4000, executePerms);
    if (!result) {
        std::cerr << "Failed to map code section" << std::endl;
        return;
    }
    
    // Map data section (read/write)
    result = addressSpace.mapRange(0x14000, 0x24000, 0x2000, readWritePerms);
    if (!result) {
        std::cerr << "Failed to map data section" << std::endl;
        return;
    }
    
    // Map read-only section
    result = addressSpace.mapRange(0x16000, 0x26000, 0x1000, readOnlyPerms);
    if (!result) {
        std::cerr << "Failed to map readonly section" << std::endl;
        return;
    }
    
    // Query mappings
    auto ranges = addressSpace.getMappedRanges();
    std::cout << "Mapped " << ranges.size() << " address ranges" << std::endl;
    
    // Test translation
    auto translation = addressSpace.translatePage(0x10000, smmu::AccessType::Execute);
    if (translation) {
        std::cout << "Execute access allowed at 0x10000" << std::endl;
    }
    
    // Test invalid access
    translation = addressSpace.translatePage(0x10000, smmu::AccessType::Write);
    if (!translation) {
        std::cout << "Write access denied at 0x10000 (expected)" << std::endl;
    }
}
```

### Multi-PASID Stream Context

```cpp
#include "smmu/stream_context.h"

void setupMultiPASIDContext() {
    smmu::StreamContext streamContext;
    
    // Enable two-stage translation
    streamContext.setStage1Enabled(true);
    streamContext.setStage2Enabled(true);
    streamContext.setFaultMode(smmu::FaultMode::Stall);
    
    // Create multiple PASIDs
    for (smmu::PASID pasid = 1; pasid <= 4; ++pasid) {
        auto result = streamContext.createPASID(pasid);
        if (!result) {
            std::cerr << "Failed to create PASID " << pasid << std::endl;
            continue;
        }
        
        // Map different regions for each PASID
        smmu::IOVA baseAddr = 0x100000 * pasid;
        smmu::PA physAddr = 0x200000 * pasid;
        smmu::PagePermissions perms(true, true, pasid == 1); // Only PASID 1 gets execute
        
        result = streamContext.mapPage(pasid, baseAddr, physAddr, perms);
        if (!result) {
            std::cerr << "Failed to map page for PASID " << pasid << std::endl;
        }
    }
    
    // Test translations for different PASIDs
    for (smmu::PASID pasid = 1; pasid <= 4; ++pasid) {
        smmu::IOVA testAddr = 0x100000 * pasid;
        auto translation = streamContext.translate(pasid, testAddr, smmu::AccessType::Read);
        
        if (translation) {
            std::cout << "PASID " << pasid << " translation: 0x" << std::hex 
                      << testAddr << " -> 0x" << translation.getValue().physicalAddress 
                      << std::endl;
        }
    }
}
```

### Fault Handling and Event Processing

```cpp
#include "smmu/smmu.h"

void handleFaultsAndEvents() {
    smmu::SMMU smmuController;
    
    // Set up stream that will generate faults
    smmu::StreamConfig config(true, true, false, smmu::FaultMode::Terminate);
    smmuController.configureStream(200, config);
    smmuController.createStreamPASID(200, 1);
    smmuController.enableStream(200);
    
    // Don't map any pages - this will cause translation faults
    
    // Attempt translation that will fault
    auto translation = smmuController.translate(200, 1, 0x5000, smmu::AccessType::Read);
    if (!translation) {
        std::cout << "Expected translation fault occurred" << std::endl;
    }
    
    // Check for events
    if (smmuController.hasEvents()) {
        auto events = smmuController.getEvents();
        std::cout << "Received " << events.size() << " fault events" << std::endl;
        
        for (const auto& event : events) {
            std::cout << "Fault - Stream: " << event.streamID 
                      << ", PASID: " << event.pasid
                      << ", Address: 0x" << std::hex << event.address
                      << ", Type: " << static_cast<int>(event.faultType)
                      << std::endl;
        }
        
        // Clear events after processing
        smmuController.clearEvents();
    }
    
    // Get statistics
    auto stats = smmuController.getCacheStatistics();
    std::cout << "Cache hits: " << stats.hitCount 
              << ", misses: " << stats.missCount
              << ", hit rate: " << stats.hitRate << "%" << std::endl;
}
```

This comprehensive API documentation provides complete coverage of the ARM SMMU v3 implementation's public interface, including detailed method descriptions, usage examples, and best practices for integration.