// ARM SMMU v3 Core Types
// Copyright (c) 2024 John Greninger

#ifndef SMMU_TYPES_H
#define SMMU_TYPES_H

#include <cstdint>
#include <cstddef>
#include <utility>
#include <type_traits>

namespace smmu {

// ARM SMMU v3 comprehensive error enumeration for Result<T> template
enum class SMMUError {
    // Success state (used internally)
    Success,
    
    // General operation errors
    InvalidStreamID,                // StreamID exceeds maximum allowed value
    InvalidPASID,                   // PASID exceeds maximum allowed value or invalid format
    InvalidAddress,                 // Address is invalid or out of supported range
    InvalidPermissions,             // Page permissions are invalid or inconsistent
    InvalidSecurityState,           // Security state transition not allowed
    
    // Stream management errors
    StreamNotConfigured,            // Stream has not been configured
    StreamAlreadyConfigured,        // Attempt to configure already configured stream
    StreamDisabled,                 // Operation attempted on disabled stream
    StreamNotFound,                 // StreamID not found in stream map
    StreamConfigurationError,       // Stream configuration parameters are invalid
    
    // PASID management errors
    PASIDNotFound,                  // PASID not found in stream context
    PASIDAlreadyExists,            // PASID already exists in stream context
    PASIDLimitExceeded,            // Maximum number of PASIDs per stream exceeded
    PASIDPermissionDenied,         // PASID operation not permitted in current state
    
    // Address space and translation errors
    PageNotMapped,                 // Requested page is not mapped in address space
    PageAlreadyMapped,             // Attempt to map already mapped page
    TranslationTableError,         // Translation table structure is invalid
    AddressSpaceExhausted,         // No more address space available
    PagePermissionViolation,       // Access type violates page permissions
    
    // Cache and TLB errors
    CacheOperationFailed,          // TLB cache operation failed
    CacheEntryNotFound,            // TLB cache entry not found
    CacheEvictionFailed,           // Failed to evict entry from cache
    InvalidCacheOperation,         // Cache operation parameters are invalid
    
    // Fault handling errors
    FaultHandlingError,            // General fault handling error
    FaultRecordCorrupted,          // Fault record data is corrupted
    FaultQueueFull,                // Fault event queue is full
    UnknownFaultType,              // Fault type is not recognized
    
    // Command and event processing errors (Task 5.3)
    CommandQueueFull,              // Command queue is at capacity
    EventQueueFull,                // Event queue is at capacity
    PRIQueueFull,                  // Page Request Interface queue is full
    InvalidCommandType,            // Command type is not supported
    CommandProcessingFailed,       // Command processing encountered error
    
    // System-level errors
    ResourceExhausted,             // System resources exhausted
    InternalError,                 // Internal SMMU implementation error
    NotImplemented,                // Feature not yet implemented
    HardwareError,                 // Hardware-level error detected
    ConfigurationError,            // System configuration error
    ParseError,                    // Configuration or data parsing error
    
    // ARM SMMU v3 specification compliance errors
    SpecViolation,                 // Operation violates ARM SMMU v3 specification
    UnsupportedFeature,            // Requested feature not supported by implementation
    InvalidConfiguration,          // Configuration violates specification constraints
    StateTransitionError           // Invalid state machine transition
};

// Result<T> template class for consistent error handling across ARM SMMU v3 implementation
// Provides type-safe success/error handling without exceptions, compatible with C++11
template<typename T>
class Result {
private:
    bool isSuccess;
    SMMUError errorCode;
    T value;  // Only valid when isSuccess == true
    
public:
    // Default constructor creates an error result for backward compatibility
    Result() : isSuccess(false), errorCode(SMMUError::InternalError), value() {
    }
    
    // Constructor for success case - stores value and marks as successful
    explicit Result(const T& val) 
        : isSuccess(true), errorCode(SMMUError::Success), value(val) {
    }
    
    // Constructor for success case with move semantics (C++11)
    explicit Result(T&& val) 
        : isSuccess(true), errorCode(SMMUError::Success), value(std::move(val)) {
    }
    
    // Constructor for error case - stores error code and marks as failed
    explicit Result(SMMUError error) 
        : isSuccess(false), errorCode(error), value() {
    }
    
    // Copy constructor
    Result(const Result& other) 
        : isSuccess(other.isSuccess), errorCode(other.errorCode), value(other.value) {
    }
    
    // Assignment operator with self-assignment protection
    Result& operator=(const Result& other) {
        if (this != &other) {
            isSuccess = other.isSuccess;
            errorCode = other.errorCode;
            if (other.isSuccess) {
                value = other.value;
            }
        }
        return *this;
    }
    
    // Move constructor (C++11)
    Result(Result&& other) 
        : isSuccess(other.isSuccess), errorCode(other.errorCode), value(std::move(other.value)) {
        other.isSuccess = false;
        other.errorCode = SMMUError::InternalError;
    }
    
    // Move assignment operator (C++11)
    Result& operator=(Result&& other) {
        if (this != &other) {
            isSuccess = other.isSuccess;
            errorCode = other.errorCode;
            if (other.isSuccess) {
                value = std::move(other.value);
            }
            other.isSuccess = false;
            other.errorCode = SMMUError::InternalError;
        }
        return *this;
    }
    
    // Check if result represents success
    bool isOk() const {
        return isSuccess;
    }
    
    // Check if result represents error
    bool isError() const {
        return !isSuccess;
    }
    
    // Get the error code (only valid when isError() == true)
    SMMUError getError() const {
        return errorCode;
    }
    
    // Get the success value (only valid when isOk() == true)
    // Caller must check isOk() before calling this method
    const T& getValue() const {
        return value;
    }
    
    // Get the success value with move semantics (only valid when isOk() == true)  
    // Caller must check isOk() before calling this method
    T&& moveValue() {
        return std::move(value);
    }
    
    // Safe value extraction - returns value if success, default value if error
    T getValueOr(const T& defaultValue) const {
        return isSuccess ? value : defaultValue;
    }
    
    // Conversion operators for compatibility with existing boolean patterns
    explicit operator bool() const {
        return isSuccess;
    }
    
};

// Unit type for Result<void> - represents successful void operations
struct Unit {
    // Empty struct to represent "no value" for void operations
};

// Convenience factory functions for Result<T> creation
template<typename T>
Result<T> makeSuccess(const T& value) {
    return Result<T>(value);
}

template<typename T>
Result<T> makeSuccess(T&& value) {
    return Result<T>(std::move(value));
}

template<typename T>
Result<T> makeError(SMMUError error) {
    return Result<T>(error);
}

// Special convenience functions for Result<void> operations
inline Result<Unit> makeVoidSuccess() {
    return Result<Unit>(Unit());
}

inline Result<Unit> makeVoidError(SMMUError error) {
    return Result<Unit>(error);
}

// Type alias for void operations - cleaner than Result<Unit>
using VoidResult = Result<Unit>;

// Core identifier types
using StreamID = uint32_t;
using PASID = uint32_t;
using IOVA = uint64_t;  // Input Output Virtual Address
using IPA = uint64_t;   // Intermediate Physical Address  
using PA = uint64_t;    // Physical Address

// Access type enumeration
enum class AccessType {
    Read,
    Write, 
    Execute
};

// Security state enumeration
enum class SecurityState {
    NonSecure,
    Secure,
    Realm
};

// Translation stage enumeration
enum class TranslationStage {
    Stage1Only,
    Stage2Only, 
    BothStages,
    Disabled
};

// ARM SMMU v3 Translation Stage enumeration for fault reporting
enum class FaultStage {
    Stage1Only,     // Fault in Stage-1 translation only
    Stage2Only,     // Fault in Stage-2 translation only
    BothStages,     // Fault involving both stages
    Unknown         // Stage could not be determined
};

// ARM SMMU v3 Privilege Level enumeration
enum class PrivilegeLevel {
    EL0,    // Exception Level 0 (User)
    EL1,    // Exception Level 1 (Kernel)
    EL2,    // Exception Level 2 (Hypervisor)
    EL3,    // Exception Level 3 (Secure Monitor)
    Unknown // Privilege level unknown
};

// ARM SMMU v3 Access Classification
enum class AccessClassification {
    InstructionFetch,   // Instruction fetch access
    DataAccess,         // Data access
    Unknown            // Classification unknown
};

// ARM SMMU v3 comprehensive fault type enumeration
enum class FaultType {
    // Basic fault types
    TranslationFault,           // Page not found in translation table
    PermissionFault,           // Access permission violation
    AddressSizeFault,          // Address size exceeds supported range
    AccessFault,               // General access fault
    SecurityFault,             // Security state violation
    
    // ARM SMMU v3 specific fault types
    ContextDescriptorFormatFault,  // Context descriptor format error
    TranslationTableFormatFault,   // Translation table format error
    Level0TranslationFault,        // Level 0 translation table fault
    Level1TranslationFault,        // Level 1 translation table fault
    Level2TranslationFault,        // Level 2 translation table fault
    Level3TranslationFault,        // Level 3 translation table fault
    AccessFlagFault,               // Access flag not set
    DirtyBitFault,                // Dirty bit management fault
    TLBConflictFault,             // TLB conflict resolution fault
    ExternalAbort,                // External memory abort
    SynchronousExternalAbort,     // Synchronous external abort
    AsynchronousExternalAbort,    // Asynchronous external abort
    StreamTableFormatFault,       // Stream table entry format fault
    ConfigurationCacheFault,      // Configuration cache fault
    
    // Stage-2 specific fault types
    Stage2TranslationFault,       // Stage-2 translation table fault
    Stage2PermissionFault         // Stage-2 permission fault
};

// ARM SMMU v3 Fault Syndrome structure for detailed fault information
struct FaultSyndrome {
    uint32_t syndromeRegister;      // ARM SMMU v3 fault syndrome register value
    FaultStage faultingStage;       // Which translation stage faulted
    uint8_t faultLevel;             // Translation table level (0-3)
    PrivilegeLevel privilegeLevel;  // Exception level of faulting access
    AccessClassification accessClass; // Instruction fetch vs data access
    bool writeNotRead;              // True for write access, false for read
    bool validSyndrome;             // True if syndrome information is valid
    uint16_t contextDescriptorIndex; // Index of faulting context descriptor
    
    // Constructor for complete syndrome
    FaultSyndrome(uint32_t syndrome, FaultStage stage, uint8_t level, 
                  PrivilegeLevel privLevel, AccessClassification accessType,
                  bool isWrite, uint16_t cdIndex = 0)
        : syndromeRegister(syndrome), faultingStage(stage), faultLevel(level),
          privilegeLevel(privLevel), accessClass(accessType), writeNotRead(isWrite),
          validSyndrome(true), contextDescriptorIndex(cdIndex) {
    }
    
    // Default constructor for invalid syndrome
    FaultSyndrome() 
        : syndromeRegister(0), faultingStage(FaultStage::Unknown), faultLevel(0),
          privilegeLevel(PrivilegeLevel::Unknown), accessClass(AccessClassification::Unknown),
          writeNotRead(false), validSyndrome(false), contextDescriptorIndex(0) {
    }
};

// Fault mode enumeration
enum class FaultMode {
    Terminate,  // Abort DMA immediately
    Stall      // Queue for OS handling
};

// Page permissions - defined before TranslationResult
struct PagePermissions {
    bool read;
    bool write;
    bool execute;
    
    PagePermissions() : read(false), write(false), execute(false) {
    }
    
    PagePermissions(bool r, bool w, bool x) : read(r), write(w), execute(x) {
    }
};

// Translation data structure for successful translations
struct TranslationData {
    PA physicalAddress;
    PagePermissions permissions;
    SecurityState securityState;
    
    TranslationData() : physicalAddress(0), securityState(SecurityState::NonSecure) {
    }
    
    TranslationData(PA pa) : physicalAddress(pa), securityState(SecurityState::NonSecure) {
    }
    
    TranslationData(PA pa, PagePermissions perms) : physicalAddress(pa), permissions(perms), securityState(SecurityState::NonSecure) {
    }
    
    TranslationData(PA pa, PagePermissions perms, SecurityState secState) : physicalAddress(pa), permissions(perms), securityState(secState) {
    }
};

// Simple TranslationResult using alias - cleaner approach for QA.3 Task 1
using TranslationResult = Result<TranslationData>;

// Helper function to map FaultType to SMMUError for backward compatibility
inline SMMUError faultTypeToSMMUError(FaultType faultType) {
    switch (faultType) {
        case FaultType::TranslationFault:
        case FaultType::Level0TranslationFault:
        case FaultType::Level1TranslationFault:
        case FaultType::Level2TranslationFault:
        case FaultType::Level3TranslationFault:
        case FaultType::Stage2TranslationFault:
            return SMMUError::PageNotMapped;
            
        case FaultType::PermissionFault:
        case FaultType::Stage2PermissionFault:
            return SMMUError::PagePermissionViolation;
            
        case FaultType::AddressSizeFault:
            return SMMUError::InvalidAddress;
            
        case FaultType::SecurityFault:
            return SMMUError::InvalidSecurityState;
            
        case FaultType::ContextDescriptorFormatFault:
        case FaultType::TranslationTableFormatFault:
        case FaultType::StreamTableFormatFault:
            return SMMUError::TranslationTableError;
            
        case FaultType::ConfigurationCacheFault:
            return SMMUError::CacheOperationFailed;
            
        case FaultType::AccessFault:
        case FaultType::AccessFlagFault:
        case FaultType::DirtyBitFault:
        case FaultType::TLBConflictFault:
        case FaultType::ExternalAbort:
        case FaultType::SynchronousExternalAbort:
        case FaultType::AsynchronousExternalAbort:
        default:
            return SMMUError::InternalError;
    }
}

// Convenience factory functions for TranslationResult
inline TranslationResult makeTranslationSuccess(PA physicalAddress) {
    return makeSuccess(TranslationData(physicalAddress));
}

inline TranslationResult makeTranslationSuccess(PA physicalAddress, PagePermissions permissions) {
    return makeSuccess(TranslationData(physicalAddress, permissions));
}

inline TranslationResult makeTranslationSuccess(PA physicalAddress, PagePermissions permissions, SecurityState securityState) {
    return makeSuccess(TranslationData(physicalAddress, permissions, securityState));
}

inline TranslationResult makeTranslationError(SMMUError error) {
    return makeError<TranslationData>(error);
}

inline TranslationResult makeTranslationError(FaultType faultType) {
    return makeError<TranslationData>(faultTypeToSMMUError(faultType));
}

// BACKWARD COMPATIBILITY HELPERS
// These helper functions provide backward compatibility for existing code
// that accessed TranslationResult fields directly. New code should use Result<T> methods.

// Helper function to check success status (replaces .success field access)
inline bool isTranslationSuccess(const TranslationResult& result) {
    return result.isOk();
}

// Helper function to get physical address from successful translation
inline PA getPhysicalAddress(const TranslationResult& result) {
    return result.isOk() ? result.getValue().physicalAddress : 0;
}

// Helper function to get permissions from successful translation
inline PagePermissions getPermissions(const TranslationResult& result) {
    return result.isOk() ? result.getValue().permissions : PagePermissions();
}

// Helper function to get security state from successful translation
inline SecurityState getSecurityState(const TranslationResult& result) {
    return result.isOk() ? result.getValue().securityState : SecurityState::NonSecure;
}

// Helper function to convert SMMUError back to FaultType for backward compatibility
inline FaultType smmUErrorToFaultType(SMMUError error) {
    switch (error) {
        case SMMUError::PageNotMapped:
            return FaultType::TranslationFault;
        case SMMUError::PagePermissionViolation:
            return FaultType::PermissionFault;
        case SMMUError::InvalidAddress:
            return FaultType::AddressSizeFault;
        case SMMUError::InvalidSecurityState:
            return FaultType::SecurityFault;
        case SMMUError::TranslationTableError:
            return FaultType::TranslationTableFormatFault;
        case SMMUError::CacheOperationFailed:
            return FaultType::ConfigurationCacheFault;
        case SMMUError::StreamNotConfigured:
        case SMMUError::PASIDNotFound:
        case SMMUError::InvalidStreamID:
        case SMMUError::InvalidPASID:
        default:
            return FaultType::AccessFault;
    }
}

// Helper function to get fault type from failed translation
inline FaultType getFaultType(const TranslationResult& result) {
    return result.isError() ? smmUErrorToFaultType(result.getError()) : FaultType::AccessFault;
}

// Page entry structure
struct PageEntry {
    PA physicalAddress;
    PagePermissions permissions;
    bool valid;
    SecurityState securityState;
    
    PageEntry() : physicalAddress(0), valid(false), securityState(SecurityState::NonSecure) {
    }
    
    PageEntry(PA pa, PagePermissions perms) : physicalAddress(pa), permissions(perms), valid(true), securityState(SecurityState::NonSecure) {
    }
    
    PageEntry(PA pa, PagePermissions perms, SecurityState secState) : physicalAddress(pa), permissions(perms), valid(true), securityState(secState) {
    }
};

// ARM SMMU v3 comprehensive fault record structure
struct FaultRecord {
    StreamID streamID;          // Source stream identifier
    PASID pasid;               // Process Address Space ID
    IOVA address;              // Faulting virtual address
    FaultType faultType;       // Detailed fault type classification
    AccessType accessType;     // Access type (Read/Write/Execute)
    SecurityState securityState; // Security state context
    FaultSyndrome syndrome;    // Detailed ARM SMMU v3 fault syndrome
    uint64_t timestamp;        // Fault occurrence timestamp
    
    // Default constructor with basic fault information
    FaultRecord() : streamID(0), pasid(0), address(0), faultType(FaultType::TranslationFault), 
                   accessType(AccessType::Read), securityState(SecurityState::NonSecure), 
                   syndrome(), timestamp(0) {
    }
    
    // Constructor with basic fault information (backward compatibility)
    FaultRecord(StreamID sid, PASID p, IOVA addr, FaultType ft, AccessType at, SecurityState secState) 
        : streamID(sid), pasid(p), address(addr), faultType(ft), accessType(at), 
          securityState(secState), syndrome(), timestamp(0) {
    }
    
    // Constructor with comprehensive ARM SMMU v3 fault syndrome
    FaultRecord(StreamID sid, PASID p, IOVA addr, FaultType ft, AccessType at, 
                SecurityState secState, const FaultSyndrome& faultSyndrome)
        : streamID(sid), pasid(p), address(addr), faultType(ft), accessType(at), 
          securityState(secState), syndrome(faultSyndrome), timestamp(0) {
    }
};

// Stream configuration structure
struct StreamConfig {
    bool translationEnabled;
    bool stage1Enabled;
    bool stage2Enabled;
    FaultMode faultMode;
    
    StreamConfig() : translationEnabled(false), stage1Enabled(false), 
                    stage2Enabled(false), faultMode(FaultMode::Terminate) {
    }
};

// Address range structure
struct AddressRange {
    IOVA startAddress;
    IOVA endAddress;
    
    AddressRange() : startAddress(0), endAddress(0) {
    }
    
    AddressRange(IOVA start, IOVA end) : startAddress(start), endAddress(end) {
    }
    
    uint64_t size() const {
        return (endAddress > startAddress) ? (endAddress - startAddress + 1) : 0;
    }
    
    bool contains(IOVA address) const {
        return address >= startAddress && address <= endAddress;
    }
    
    bool overlaps(const AddressRange& other) const {
        return !(endAddress < other.startAddress || startAddress > other.endAddress);
    }
};

// TLB Cache entry structure
struct TLBEntry {
    StreamID streamID;
    PASID pasid;
    IOVA iova;
    PA physicalAddress;
    PagePermissions permissions;
    SecurityState securityState;
    bool valid;
    uint64_t timestamp;
    
    TLBEntry() : streamID(0), pasid(0), iova(0), physicalAddress(0), 
                 securityState(SecurityState::NonSecure), valid(false), timestamp(0) {
    }
    
    TLBEntry(StreamID sid, PASID p, IOVA iva, PA pa, PagePermissions perms, SecurityState secState) 
        : streamID(sid), pasid(p), iova(iva), physicalAddress(pa), permissions(perms), securityState(secState), valid(true), timestamp(0) {
    }
};

// Stream statistics structure
struct StreamStatistics {
    uint64_t translationCount;
    uint64_t faultCount;
    uint64_t pasidCount;
    uint64_t configurationUpdateCount;
    uint64_t lastAccessTimestamp;
    uint64_t creationTimestamp;
    
    StreamStatistics() : translationCount(0), faultCount(0), pasidCount(0),
                        configurationUpdateCount(0), lastAccessTimestamp(0), 
                        creationTimestamp(0) {
    }
};

// Cache statistics structure (Task 5.2)
struct CacheStatistics {
    uint64_t hitCount;
    uint64_t missCount;
    uint64_t totalLookups;
    uint64_t evictionCount;
    size_t currentSize;
    size_t maxSize;
    double hitRate;
    
    CacheStatistics() : hitCount(0), missCount(0), totalLookups(0), evictionCount(0),
                       currentSize(0), maxSize(0), hitRate(0.0) {
    }
    
    void calculateHitRate() {
        if (totalLookups > 0) {
            hitRate = static_cast<double>(hitCount) / static_cast<double>(totalLookups);
        } else {
            hitRate = 0.0;
        }
    }
};

// Task 5.3: Event and Command Processing - Command types for SMMU command queue
enum class CommandType {
    PREFETCH_CONFIG,
    PREFETCH_ADDR,
    CFGI_STE,       // Stream Table Entry invalidation
    CFGI_ALL,       // All configuration invalidation
    TLBI_NH_ALL,    // TLB invalidation non-secure hyp all
    TLBI_EL2_ALL,   // TLB invalidation EL2 all
    TLBI_S12_VMALL, // TLB invalidation stage 1&2 VM all
    ATC_INV,        // Address Translation Cache invalidation
    PRI_RESP,       // Page Request Interface response
    RESUME,         // Resume processing
    SYNC            // Synchronization barrier
};

// Task 5.3: Command queue entry
struct CommandEntry {
    CommandType type;
    StreamID streamID;
    PASID pasid;
    IOVA startAddress;
    IOVA endAddress;
    uint32_t flags;
    uint64_t timestamp;
    
    CommandEntry() : type(CommandType::SYNC), streamID(0), pasid(0), 
                    startAddress(0), endAddress(0), flags(0), timestamp(0) {
    }
    
    CommandEntry(CommandType cmdType, StreamID sid, PASID p, IOVA start, IOVA end) 
        : type(cmdType), streamID(sid), pasid(p), startAddress(start), endAddress(end), 
          flags(0), timestamp(0) {
    }
};

// Task 5.3: Page Request Interface entry
struct PRIEntry {
    StreamID streamID;
    PASID pasid;
    IOVA requestedAddress;
    AccessType accessType;
    bool isLastRequest;
    uint64_t timestamp;
    
    PRIEntry() : streamID(0), pasid(0), requestedAddress(0), 
                accessType(AccessType::Read), isLastRequest(false), timestamp(0) {
    }
    
    PRIEntry(StreamID sid, PASID p, IOVA addr, AccessType access) 
        : streamID(sid), pasid(p), requestedAddress(addr), accessType(access), 
          isLastRequest(false), timestamp(0) {
    }
};

// Task 5.3: Event types beyond just faults
enum class EventType {
    TRANSLATION_FAULT,
    PERMISSION_FAULT,
    COMMAND_SYNC_COMPLETION,
    PRI_PAGE_REQUEST,
    ATC_INVALIDATE_COMPLETION,
    CONFIGURATION_ERROR,
    INTERNAL_ERROR
};

// Task 5.3: Enhanced event entry
struct EventEntry {
    EventType type;
    StreamID streamID;
    PASID pasid;
    IOVA address;
    SecurityState securityState;
    uint32_t errorCode;
    uint64_t timestamp;
    
    EventEntry() : type(EventType::INTERNAL_ERROR), streamID(0), pasid(0),
                  address(0), securityState(SecurityState::NonSecure), errorCode(0), timestamp(0) {
    }
    
    EventEntry(EventType eventType, StreamID sid, PASID p, IOVA addr) 
        : type(eventType), streamID(sid), pasid(p), address(addr), 
          securityState(SecurityState::NonSecure), errorCode(0), timestamp(0) {
    }
    
    EventEntry(EventType eventType, StreamID sid, PASID p, IOVA addr, SecurityState secState) 
        : type(eventType), streamID(sid), pasid(p), address(addr), 
          securityState(secState), errorCode(0), timestamp(0) {
    }
};

// ARM SMMU v3 Address Space Size enumeration
enum class AddressSpaceSize {
    Size32Bit,      // 32-bit address space (4GB)
    Size48Bit,      // 48-bit address space (256TB)  
    Size52Bit       // 52-bit address space (4PB)
};

// ARM SMMU v3 Translation Granule Size enumeration
enum class TranslationGranule {
    Size4KB,        // 4KB page granule
    Size16KB,       // 16KB page granule
    Size64KB        // 64KB page granule
};

// ARM SMMU v3 Translation Control Register structure
struct TranslationControlRegister {
    AddressSpaceSize inputAddressSize;      // T0SZ/T1SZ input address size
    AddressSpaceSize outputAddressSize;     // Output address size
    TranslationGranule granuleSize;         // Translation granule size
    bool shareabilityInner;                 // Inner shareability attribute
    bool shareabilityOuter;                 // Outer shareability attribute
    uint8_t cachePolicyInner;               // Inner cache policy (IRGN)
    uint8_t cachePolicyOuter;               // Outer cache policy (ORGN)
    bool walkCacheDisable;                  // Disable page table walks caching
    bool hierarchicalPermDisable;           // Disable hierarchical permission checks
    
    TranslationControlRegister() 
        : inputAddressSize(AddressSpaceSize::Size48Bit),
          outputAddressSize(AddressSpaceSize::Size48Bit),
          granuleSize(TranslationGranule::Size4KB),
          shareabilityInner(false), shareabilityOuter(false),
          cachePolicyInner(0), cachePolicyOuter(0),
          walkCacheDisable(false), hierarchicalPermDisable(false) {
    }
    
    TranslationControlRegister(AddressSpaceSize inSize, AddressSpaceSize outSize, 
                              TranslationGranule granule)
        : inputAddressSize(inSize), outputAddressSize(outSize), granuleSize(granule),
          shareabilityInner(false), shareabilityOuter(false),
          cachePolicyInner(0), cachePolicyOuter(0),
          walkCacheDisable(false), hierarchicalPermDisable(false) {
    }
};

// ARM SMMU v3 Memory Attribute Indirection Register structure
struct MemoryAttributeRegister {
    uint64_t mairValue;                     // Complete MAIR register value
    uint8_t attr0;                          // Memory attribute 0
    uint8_t attr1;                          // Memory attribute 1
    uint8_t attr2;                          // Memory attribute 2
    uint8_t attr3;                          // Memory attribute 3
    uint8_t attr4;                          // Memory attribute 4
    uint8_t attr5;                          // Memory attribute 5
    uint8_t attr6;                          // Memory attribute 6
    uint8_t attr7;                          // Memory attribute 7
    
    MemoryAttributeRegister() 
        : mairValue(0), attr0(0), attr1(0), attr2(0), attr3(0),
          attr4(0), attr5(0), attr6(0), attr7(0) {
    }
    
    explicit MemoryAttributeRegister(uint64_t mair)
        : mairValue(mair),
          attr0(static_cast<uint8_t>(mair & 0xFF)),
          attr1(static_cast<uint8_t>((mair >> 8) & 0xFF)),
          attr2(static_cast<uint8_t>((mair >> 16) & 0xFF)),
          attr3(static_cast<uint8_t>((mair >> 24) & 0xFF)),
          attr4(static_cast<uint8_t>((mair >> 32) & 0xFF)),
          attr5(static_cast<uint8_t>((mair >> 40) & 0xFF)),
          attr6(static_cast<uint8_t>((mair >> 48) & 0xFF)),
          attr7(static_cast<uint8_t>((mair >> 56) & 0xFF)) {
    }
};

// ARM SMMU v3 Context Descriptor validation structure
struct ContextDescriptor {
    uint64_t ttbr0;                         // Translation Table Base Register 0
    uint64_t ttbr1;                         // Translation Table Base Register 1
    TranslationControlRegister tcr;         // Translation Control Register
    MemoryAttributeRegister mair;           // Memory Attribute Indirection Register
    uint16_t asid;                          // Address Space Identifier
    SecurityState securityState;            // Security state context
    bool ttbr0Valid;                        // TTBR0 is valid and configured
    bool ttbr1Valid;                        // TTBR1 is valid and configured
    bool globalTranslations;                // Global vs non-global translations
    uint8_t contextDescriptorIndex;         // CD index within CD table
    
    ContextDescriptor()
        : ttbr0(0), ttbr1(0), asid(0), securityState(SecurityState::NonSecure),
          ttbr0Valid(false), ttbr1Valid(false), globalTranslations(false),
          contextDescriptorIndex(0) {
    }
    
    ContextDescriptor(uint64_t ttbr0Addr, uint16_t asidValue, SecurityState secState)
        : ttbr0(ttbr0Addr), ttbr1(0), asid(asidValue), securityState(secState),
          ttbr0Valid(true), ttbr1Valid(false), globalTranslations(false),
          contextDescriptorIndex(0) {
    }
    
    ContextDescriptor(uint64_t ttbr0Addr, uint64_t ttbr1Addr, uint16_t asidValue,
                     const TranslationControlRegister& tcrValue,
                     const MemoryAttributeRegister& mairValue, SecurityState secState)
        : ttbr0(ttbr0Addr), ttbr1(ttbr1Addr), tcr(tcrValue), mair(mairValue),
          asid(asidValue), securityState(secState), ttbr0Valid(true), ttbr1Valid(true),
          globalTranslations(false), contextDescriptorIndex(0) {
    }
};

// ARM SMMU v3 Stream Table Entry configuration structure
struct StreamTableEntry {
    bool stage1Enabled;                     // Stage-1 translation enabled
    bool stage2Enabled;                     // Stage-2 translation enabled
    bool translationEnabled;                // Any translation enabled
    uint64_t contextDescriptorTableBase;    // CD table base address
    uint32_t contextDescriptorTableSize;    // CD table size (number of entries)
    SecurityState securityState;            // Stream security state
    TranslationGranule stage1Granule;       // Stage-1 translation granule
    TranslationGranule stage2Granule;       // Stage-2 translation granule
    FaultMode faultMode;                    // Fault handling mode
    bool privilegedExecuteNever;            // Privileged execute never
    bool instructionFetchDisable;           // Instruction fetch disable
    uint32_t streamID;                      // Associated Stream ID
    
    StreamTableEntry()
        : stage1Enabled(false), stage2Enabled(false), translationEnabled(false),
          contextDescriptorTableBase(0), contextDescriptorTableSize(0),
          securityState(SecurityState::NonSecure),
          stage1Granule(TranslationGranule::Size4KB),
          stage2Granule(TranslationGranule::Size4KB),
          faultMode(FaultMode::Terminate),
          privilegedExecuteNever(false), instructionFetchDisable(false),
          streamID(0) {
    }
    
    StreamTableEntry(uint32_t sid, bool s1Enabled, bool s2Enabled, 
                    uint64_t cdTableBase, SecurityState secState)
        : stage1Enabled(s1Enabled), stage2Enabled(s2Enabled),
          translationEnabled(s1Enabled || s2Enabled),
          contextDescriptorTableBase(cdTableBase), contextDescriptorTableSize(1),
          securityState(secState),
          stage1Granule(TranslationGranule::Size4KB),
          stage2Granule(TranslationGranule::Size4KB),
          faultMode(FaultMode::Terminate),
          privilegedExecuteNever(false), instructionFetchDisable(false),
          streamID(sid) {
    }
};

// Configuration constants
constexpr uint32_t MAX_STREAM_ID = 0xFFFFFFFF;
constexpr uint32_t MAX_PASID = 0xFFFFF;  // 20-bit PASID space
constexpr uint64_t PAGE_SIZE = 4096;     // 4KB pages
constexpr uint64_t PAGE_MASK = PAGE_SIZE - 1;

// Task 5.3: Queue size constants
constexpr size_t DEFAULT_EVENT_QUEUE_SIZE = 512;
constexpr size_t DEFAULT_COMMAND_QUEUE_SIZE = 256;
constexpr size_t DEFAULT_PRI_QUEUE_SIZE = 128;

} // namespace smmu

#endif // SMMU_TYPES_H