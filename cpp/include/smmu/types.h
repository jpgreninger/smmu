/**
 * @file types.h
 * @brief ARM SMMU v3 Core Types and Data Structures
 * @details This file contains all fundamental types, enums, and data structures
 *          used throughout the ARM SMMU v3 implementation. It follows the
 *          ARM SMMU v3 specification (ARM IHI 0070G) and provides C++11-compliant
 *          error handling through the Result<T> template.
 *
 * Key Features:
 * - Type-safe error handling without exceptions
 * - ARM SMMU v3 specification compliant data structures
 * - Thread-safe atomic operations support
 * - Zero-overhead abstractions
 *
 * Copyright (c) 2024 John Greninger
 */

#ifndef SMMU_TYPES_H
#define SMMU_TYPES_H

#include <cstdint>
#include <cstddef>
#include <utility>
#include <type_traits>

namespace smmu {

/**
 * @enum SMMUError
 * @brief Comprehensive error enumeration for ARM SMMU v3 operations
 * @details Provides detailed error classification following ARM SMMU v3
 *          specification error categories. Used with Result<T> template
 *          for type-safe error handling without exceptions.
 *
 * @note All error codes are specification-compliant and map to appropriate
 *       ARM SMMU v3 fault syndrome values where applicable.
 */
enum class SMMUError {
    /// @brief Success state (used internally by Result<T>)
    Success,
    
    // General operation errors
    /// @brief StreamID exceeds maximum allowed value (MAX_STREAM_ID)
    InvalidStreamID,
    /// @brief PASID exceeds maximum allowed value (MAX_PASID) or invalid format
    InvalidPASID,
    /// @brief Address is invalid or out of supported range
    InvalidAddress,
    /// @brief Page permissions are invalid or inconsistent
    InvalidPermissions,
    /// @brief Security state transition not allowed by ARM SMMU v3 specification
    InvalidSecurityState,
    
    // Stream management errors
    /// @brief Stream has not been configured (missing Stream Table Entry)
    StreamNotConfigured,
    /// @brief Attempt to configure already configured stream
    StreamAlreadyConfigured,
    /// @brief Operation attempted on disabled stream
    StreamDisabled,
    /// @brief StreamID not found in stream map
    StreamNotFound,
    /// @brief Stream configuration parameters are invalid
    StreamConfigurationError,
    
    // PASID management errors
    /// @brief PASID not found in stream context (no Context Descriptor)
    PASIDNotFound,
    /// @brief PASID already exists in stream context
    PASIDAlreadyExists,
    /// @brief Maximum number of PASIDs per stream exceeded
    PASIDLimitExceeded,
    /// @brief PASID operation not permitted in current state
    PASIDPermissionDenied,
    
    // Address space and translation errors
    /// @brief Requested page is not mapped in address space (Translation Fault)
    PageNotMapped,
    /// @brief Attempt to map already mapped page
    PageAlreadyMapped,
    /// @brief Translation table structure is invalid (Table Format Fault)
    TranslationTableError,
    /// @brief No more address space available
    AddressSpaceExhausted,
    /// @brief Access type violates page permissions (Permission Fault)
    PagePermissionViolation,
    
    // Cache and TLB errors
    /// @brief TLB cache operation failed
    CacheOperationFailed,
    /// @brief TLB cache entry not found
    CacheEntryNotFound,
    /// @brief Failed to evict entry from cache
    CacheEvictionFailed,
    /// @brief Cache operation parameters are invalid
    InvalidCacheOperation,
    
    // Fault handling errors
    /// @brief General fault handling error
    FaultHandlingError,
    /// @brief Fault record data is corrupted
    FaultRecordCorrupted,
    /// @brief Fault event queue is full
    FaultQueueFull,
    /// @brief Fault type is not recognized
    UnknownFaultType,
    
    // Command and event processing errors (ARM SMMU v3 specification)
    /// @brief Command queue is at capacity
    CommandQueueFull,
    /// @brief Event queue is at capacity
    EventQueueFull,
    /// @brief Page Request Interface queue is full
    PRIQueueFull,
    /// @brief Command type is not supported
    InvalidCommandType,
    /// @brief Command processing encountered error
    CommandProcessingFailed,
    
    // System-level errors
    /// @brief System resources exhausted
    ResourceExhausted,
    /// @brief Internal SMMU implementation error
    InternalError,
    /// @brief Feature not yet implemented
    NotImplemented,
    /// @brief Hardware-level error detected
    HardwareError,
    /// @brief System configuration error
    ConfigurationError,
    /// @brief Configuration or data parsing error
    ParseError,
    
    // ARM SMMU v3 specification compliance errors
    /// @brief Operation violates ARM SMMU v3 specification
    SpecViolation,
    /// @brief Requested feature not supported by implementation
    UnsupportedFeature,
    /// @brief Configuration violates specification constraints
    InvalidConfiguration,
    /// @brief Invalid state machine transition
    StateTransitionError
};

/**
 * @class Result
 * @brief Type-safe error handling template for ARM SMMU v3 implementation
 * @tparam T The type of value returned on success
 * @details Provides consistent success/error handling without exceptions,
 *          compatible with C++11. Inspired by Rust's Result<T, E> type.
 *
 * Usage patterns:
 * @code
 * Result<PA> result = translateAddress(streamID, pasid, iova);
 * if (result.isOk()) {
 *     PA physicalAddr = result.getValue();
 *     // Use physicalAddr...
 * } else {
 *     SMMUError error = result.getError();
 *     // Handle error...
 * }
 * @endcode
 *
 * Performance Characteristics:
 * - Zero-cost abstractions when optimized
 * - Move semantics support (C++11)
 * - Cache-friendly memory layout
 * - O(1) all operations
 *
 * Thread Safety:
 * - Thread-safe for read operations after construction
 * - Individual Result<T> objects are not thread-safe for concurrent write
 */
template<typename T>
class Result {
private:
    bool isSuccess;
    SMMUError errorCode;
    T value;  // Only valid when isSuccess == true
    
public:
    /**
     * @brief Default constructor creates an error result
     * @details For backward compatibility, creates InternalError state.
     *          Prefer explicit error construction.
     */
    Result() : isSuccess(false), errorCode(SMMUError::InternalError), value() {
    }
    
    /**
     * @brief Constructor for success case with value copy
     * @param val The success value to store
     * @details Stores value and marks result as successful.
     *          Uses copy semantics.
     */
    explicit Result(const T& val) 
        : isSuccess(true), errorCode(SMMUError::Success), value(val) {
    }
    
    /**
     * @brief Constructor for success case with move semantics (C++11)
     * @param val The success value to move
     * @details Stores value using move semantics for efficiency.
     *          Optimal for expensive-to-copy types.
     */
    explicit Result(T&& val) 
        : isSuccess(true), errorCode(SMMUError::Success), value(std::move(val)) {
    }
    
    /**
     * @brief Constructor for error case
     * @param error The error code to store
     * @details Stores error code and marks result as failed.
     *          Value remains in default-constructed state.
     */
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
    
    /**
     * @brief Check if result represents success
     * @return true if the operation succeeded
     * @details O(1) operation, safe to call multiple times.
     *          Must return true before calling getValue().
     */
    bool isOk() const {
        return isSuccess;
    }
    
    /**
     * @brief Check if result represents error
     * @return true if the operation failed
     * @details O(1) operation, equivalent to !isOk().
     *          Must return true before calling getError().
     */
    bool isError() const {
        return !isSuccess;
    }
    
    /**
     * @brief Get the error code
     * @return The error code
     * @pre isError() must return true
     * @details Returns the stored error code. Behavior is undefined
     *          if called when isOk() returns true.
     */
    SMMUError getError() const {
        return errorCode;
    }
    
    /**
     * @brief Get the success value by const reference
     * @return Const reference to the success value
     * @pre isOk() must return true
     * @details Returns the stored success value. Behavior is undefined
     *          if called when isError() returns true.
     */
    const T& getValue() const {
        return value;
    }
    
    /**
     * @brief Get the success value with move semantics
     * @return Rvalue reference to the success value
     * @pre isOk() must return true
     * @details Returns the stored success value using move semantics.
     *          Behavior is undefined if called when isError() returns true.
     *          Value becomes unspecified after this call.
     */
    T&& moveValue() {
        return std::move(value);
    }
    
    /**
     * @brief Safe value extraction with default fallback
     * @param defaultValue Value to return if result is error
     * @return The success value or defaultValue
     * @details Returns the success value if available, otherwise
     *          returns the provided default value. Always safe to call.
     */
    T getValueOr(const T& defaultValue) const {
        return isSuccess ? value : defaultValue;
    }
    
    /**
     * @brief Explicit boolean conversion operator
     * @return true if the operation succeeded
     * @details Enables usage in boolean contexts: if (result) { ... }
     *          Equivalent to isOk().
     */
    explicit operator bool() const {
        return isSuccess;
    }
    
};

/**
 * @struct Unit
 * @brief Unit type for Result<void> operations
 * @details Empty struct representing "no value" for void operations.
 *          Used as the value type in VoidResult = Result<Unit>.
 *          Follows functional programming patterns.
 */
struct Unit {
    /// @brief Default constructor (no operation required)
    Unit() {}
};

/**
 * @brief Factory function to create successful Result<T> with copy semantics
 * @tparam T Type of the success value
 * @param value The value to wrap in a successful Result
 * @return Result<T> containing the value
 * @details Convenience function for creating successful results.
 *          Equivalent to Result<T>(value).
 */
template<typename T>
Result<T> makeSuccess(const T& value) {
    return Result<T>(value);
}

/**
 * @brief Factory function to create successful Result<T> with move semantics
 * @tparam T Type of the success value
 * @param value The value to move into a successful Result
 * @return Result<T> containing the moved value
 * @details Convenience function for creating successful results with move.
 *          Optimal for expensive-to-copy types.
 */
template<typename T>
Result<T> makeSuccess(T&& value) {
    return Result<T>(std::move(value));
}

/**
 * @brief Factory function to create error Result<T>
 * @tparam T Type that would have been returned on success
 * @param error The error code
 * @return Result<T> containing the error
 * @details Convenience function for creating error results.
 *          Equivalent to Result<T>(error).
 */
template<typename T>
Result<T> makeError(SMMUError error) {
    return Result<T>(error);
}

/**
 * @brief Factory function to create successful void Result
 * @return VoidResult indicating successful void operation
 * @details Convenience function for void operations that succeed.
 *          Equivalent to Result<Unit>(Unit()).
 */
inline Result<Unit> makeVoidSuccess() {
    return Result<Unit>(Unit());
}

/**
 * @brief Factory function to create error void Result
 * @param error The error code
 * @return VoidResult indicating failed void operation
 * @details Convenience function for void operations that fail.
 *          Equivalent to Result<Unit>(error).
 */
inline Result<Unit> makeVoidError(SMMUError error) {
    return Result<Unit>(error);
}

/**
 * @typedef VoidResult
 * @brief Type alias for void operations - cleaner than Result<Unit>
 * @details Used for functions that perform operations without returning
 *          a specific value, but need to indicate success/failure.
 *          Common in ARM SMMU v3 configuration operations.
 */
using VoidResult = Result<Unit>;

///@{
/// @name Core ARM SMMU v3 Identifier Types
/// @details Fundamental types used throughout ARM SMMU v3 implementation

/// @brief Stream ID type - identifies a stream of transactions
/// @details 32-bit identifier for transaction streams. Range: [0, MAX_STREAM_ID]
using StreamID = uint32_t;

/// @brief Process Address Space ID - identifies process context within stream
/// @details 20-bit identifier as per ARM SMMU v3 spec. Range: [0, MAX_PASID]
using PASID = uint32_t;

/// @brief Input Output Virtual Address - virtual address from device
/// @details 64-bit virtual address used by devices for memory access
using IOVA = uint64_t;

/// @brief Intermediate Physical Address - Stage 1 translation output
/// @details 64-bit intermediate address in two-stage translation
using IPA = uint64_t;

/// @brief Physical Address - final translated address
/// @details 64-bit physical address for memory access
using PA = uint64_t;

///@}

/**
 * @enum AccessType
 * @brief Memory access type enumeration
 * @details Defines the type of memory access being performed.
 *          Used for permission checking and fault classification.
 *          Corresponds to ARM SMMU v3 access type encoding.
 */
enum class AccessType {
    /// @brief Read access - loads, non-executable fetches
    Read,
    /// @brief Write access - stores, atomic operations
    Write,
    /// @brief Execute access - instruction fetches
    Execute
};

/**
 * @enum SecurityState
 * @brief ARM security state enumeration
 * @details Defines security context for memory transactions.
 *          Follows ARMv8-A security model with Realm support.
 *          Used for access control and security fault detection.
 */
enum class SecurityState {
    /// @brief Non-secure state - normal world access
    NonSecure,
    /// @brief Secure state - secure world access
    Secure,
    /// @brief Realm state - confidential computing access
    Realm
};

/**
 * @enum TranslationStage
 * @brief ARM SMMU v3 translation stage configuration
 * @details Defines which translation stages are enabled.
 *          Stage 1: Device virtual to intermediate physical
 *          Stage 2: Intermediate physical to physical
 */
enum class TranslationStage {
    /// @brief Stage 1 translation only (device virtual to physical)
    Stage1Only,
    /// @brief Stage 2 translation only (intermediate to physical)
    Stage2Only,
    /// @brief Both stages enabled (device virtual → intermediate → physical)
    BothStages,
    /// @brief Translation disabled (bypass mode)
    Disabled
};

/**
 * @enum FaultStage
 * @brief ARM SMMU v3 fault stage identification
 * @details Identifies which translation stage caused a fault.
 *          Used in fault syndrome generation and fault classification.
 *          Critical for proper fault handling and recovery.
 */
enum class FaultStage {
    /// @brief Fault in Stage 1 translation only
    Stage1Only,
    /// @brief Fault in Stage 2 translation only  
    Stage2Only,
    /// @brief Fault involving both stages
    BothStages,
    /// @brief Stage could not be determined
    Unknown
};

/**
 * @enum PrivilegeLevel
 * @brief ARM Exception Level enumeration
 * @details Defines privilege level for access classification.
 *          Maps to ARMv8-A exception level model.
 *          Used in fault syndrome generation.
 */
enum class PrivilegeLevel {
    /// @brief Exception Level 0 (User mode)
    EL0,
    /// @brief Exception Level 1 (Kernel mode)
    EL1,
    /// @brief Exception Level 2 (Hypervisor mode)
    EL2,
    /// @brief Exception Level 3 (Secure Monitor)
    EL3,
    /// @brief Privilege level unknown or not applicable
    Unknown
};

/**
 * @enum AccessClassification
 * @brief ARM SMMU v3 access classification
 * @details Classifies memory access for fault syndrome generation.
 *          Used to distinguish instruction fetches from data accesses.
 *          Important for security and permission enforcement.
 */
enum class AccessClassification {
    /// @brief Instruction fetch access
    InstructionFetch,
    /// @brief Data access (read/write)
    DataAccess,
    /// @brief Classification unknown or not applicable
    Unknown
};

/**
 * @enum FaultType
 * @brief Comprehensive ARM SMMU v3 fault type enumeration
 * @details Detailed fault classification following ARM SMMU v3 specification.
 *          Each fault type maps to specific ARM fault syndrome encoding.
 *          Used for fault handling, recovery, and syndrome generation.
 * 
 * @note Fault types are organized by category:
 *       - Basic faults: Common translation and permission issues
 *       - ARM SMMU v3 specific: Hardware-specific fault conditions
 *       - Stage-2 specific: Virtualization-related faults
 */
enum class FaultType {
    // Basic fault types
    /// @brief Page not found in translation table
    TranslationFault,
    /// @brief Access permission violation (read/write/execute)
    PermissionFault,
    /// @brief Address size exceeds supported range
    AddressSizeFault,
    /// @brief General access fault
    AccessFault,
    /// @brief Security state violation
    SecurityFault,
    
    // ARM SMMU v3 specific fault types
    /// @brief Context descriptor format error (invalid CD format)
    ContextDescriptorFormatFault,
    /// @brief Translation table format error (invalid table entry)
    TranslationTableFormatFault,
    /// @brief Level 0 translation table fault
    Level0TranslationFault,
    /// @brief Level 1 translation table fault
    Level1TranslationFault,
    /// @brief Level 2 translation table fault
    Level2TranslationFault,
    /// @brief Level 3 translation table fault
    Level3TranslationFault,
    /// @brief Access flag not set (hardware management)
    AccessFlagFault,
    /// @brief Dirty bit management fault
    DirtyBitFault,
    /// @brief TLB conflict resolution fault
    TLBConflictFault,
    /// @brief External memory abort
    ExternalAbort,
    /// @brief Synchronous external abort
    SynchronousExternalAbort,
    /// @brief Asynchronous external abort
    AsynchronousExternalAbort,
    /// @brief Stream table entry format fault
    StreamTableFormatFault,
    /// @brief Configuration cache fault
    ConfigurationCacheFault,
    
    // Stage-2 specific fault types
    /// @brief Stage-2 translation table fault (IPA → PA)
    Stage2TranslationFault,
    /// @brief Stage-2 permission fault (hypervisor permissions)
    Stage2PermissionFault
};

/**
 * @struct FaultSyndrome
 * @brief ARM SMMU v3 fault syndrome structure
 * @details Contains detailed fault information following ARM SMMU v3
 *          fault syndrome register format. Used for comprehensive
 *          fault reporting and debugging.
 * 
 * Performance Characteristics:
 * - Lightweight structure (cache-friendly)
 * - Pre-computed syndrome values for efficiency
 * - O(1) construction and access
 */
struct FaultSyndrome {
    /// @brief ARM SMMU v3 fault syndrome register value
    uint32_t syndromeRegister;
    /// @brief Which translation stage faulted
    FaultStage faultingStage;
    /// @brief Translation table level (0-3)
    uint8_t faultLevel;
    /// @brief Exception level of faulting access
    PrivilegeLevel privilegeLevel;
    /// @brief Instruction fetch vs data access classification
    AccessClassification accessClass;
    /// @brief True for write access, false for read
    bool writeNotRead;
    /// @brief True if syndrome information is valid
    bool validSyndrome;
    /// @brief Index of faulting context descriptor
    uint16_t contextDescriptorIndex;
    
    /**
     * @brief Constructor for complete syndrome
     * @param syndrome ARM SMMU v3 fault syndrome register value
     * @param stage Which translation stage faulted
     * @param level Translation table level (0-3)
     * @param privLevel Exception level of faulting access
     * @param accessType Instruction fetch vs data access
     * @param isWrite True for write access, false for read
     * @param cdIndex Context descriptor index (default: 0)
     */
    FaultSyndrome(uint32_t syndrome, FaultStage stage, uint8_t level, 
                  PrivilegeLevel privLevel, AccessClassification accessType,
                  bool isWrite, uint16_t cdIndex = 0)
        : syndromeRegister(syndrome), faultingStage(stage), faultLevel(level),
          privilegeLevel(privLevel), accessClass(accessType), writeNotRead(isWrite),
          validSyndrome(true), contextDescriptorIndex(cdIndex) {
    }
    
    /**
     * @brief Default constructor for invalid syndrome
     * @details Creates an invalid syndrome with all fields zeroed.
     *          validSyndrome field will be false.
     */
    FaultSyndrome() 
        : syndromeRegister(0), faultingStage(FaultStage::Unknown), faultLevel(0),
          privilegeLevel(PrivilegeLevel::Unknown), accessClass(AccessClassification::Unknown),
          writeNotRead(false), validSyndrome(false), contextDescriptorIndex(0) {
    }
};

/**
 * @enum FaultMode
 * @brief ARM SMMU v3 fault handling mode
 * @details Defines how the SMMU should handle faults.
 *          Follows ARM SMMU v3 specification fault handling model.
 */
enum class FaultMode {
    /// @brief Abort DMA immediately (terminate transaction)
    Terminate,
    /// @brief Queue fault for OS handling (stall transaction)
    Stall
};

/**
 * @struct PagePermissions
 * @brief Page access permissions structure
 * @details Defines read/write/execute permissions for memory pages.
 *          Used in translation results and permission checking.
 *          Maps to ARM architecture memory permissions.
 * 
 * Performance Characteristics:
 * - Compact representation (3 bits logical)
 * - Fast permission checking operations
 * - Cache-friendly structure
 */
struct PagePermissions {
    /// @brief Read permission allowed
    bool read;
    /// @brief Write permission allowed
    bool write;
    /// @brief Execute permission allowed
    bool execute;
    
    /**
     * @brief Default constructor - no permissions
     * @details All permissions set to false for security.
     */
    PagePermissions() : read(false), write(false), execute(false) {
    }
    
    /**
     * @brief Constructor with explicit permissions
     * @param r Read permission
     * @param w Write permission
     * @param x Execute permission
     */
    PagePermissions(bool r, bool w, bool x) : read(r), write(w), execute(x) {
    }
};

/**
 * @struct TranslationData
 * @brief Translation result data structure
 * @details Contains successful translation output including physical address,
 *          permissions, and security state. Used as the value type in
 *          TranslationResult = Result<TranslationData>.
 * 
 * Performance Characteristics:
 * - Compact structure (cache-friendly)
 * - Move-optimized constructors
 * - Zero-cost success path
 */
struct TranslationData {
    /// @brief Physical address (translation result)
    PA physicalAddress;
    /// @brief Page permissions for the translated address
    PagePermissions permissions;
    /// @brief Security state of the translated address
    SecurityState securityState;
    
    /**
     * @brief Default constructor
     * @details Physical address = 0, NonSecure state, no permissions.
     */
    TranslationData() : physicalAddress(0), securityState(SecurityState::NonSecure) {
    }
    
    /**
     * @brief Constructor with physical address only
     * @param pa Physical address
     * @details Security state defaults to NonSecure, no permissions.
     */
    TranslationData(PA pa) : physicalAddress(pa), securityState(SecurityState::NonSecure) {
    }
    
    /**
     * @brief Constructor with address and permissions
     * @param pa Physical address
     * @param perms Page permissions
     * @details Security state defaults to NonSecure.
     */
    TranslationData(PA pa, PagePermissions perms) : physicalAddress(pa), permissions(perms), securityState(SecurityState::NonSecure) {
    }
    
    /**
     * @brief Constructor with full translation data
     * @param pa Physical address
     * @param perms Page permissions
     * @param secState Security state
     */
    TranslationData(PA pa, PagePermissions perms, SecurityState secState) : physicalAddress(pa), permissions(perms), securityState(secState) {
    }
};

/**
 * @typedef TranslationResult
 * @brief Type alias for translation operation results
 * @details Result<TranslationData> - either successful translation with
 *          physical address and permissions, or error with detailed error code.
 *          Primary return type for all translation operations.
 * 
 * Usage:
 * @code
 * TranslationResult result = smmu.translate(streamID, pasid, iova, AccessType::Read);
 * if (result.isOk()) {
 *     PA physAddr = result.getValue().physicalAddress;
 *     PagePermissions perms = result.getValue().permissions;
 * } else {
 *     handleTranslationError(result.getError());
 * }
 * @endcode
 */
using TranslationResult = Result<TranslationData>;

/**
 * @brief Map FaultType to SMMUError for backward compatibility
 * @param faultType The fault type to convert
 * @return Corresponding SMMUError
 * @details Provides mapping between fault types and error codes
 *          for consistent error handling across the implementation.
 */
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

///@{
/// @name TranslationResult Factory Functions
/// @details Convenience functions for creating TranslationResult objects

/**
 * @brief Create successful translation with physical address only
 * @param physicalAddress Physical address result
 * @return TranslationResult containing the translation data
 */
inline TranslationResult makeTranslationSuccess(PA physicalAddress) {
    return makeSuccess(TranslationData(physicalAddress));
}

/**
 * @brief Create successful translation with address and permissions
 * @param physicalAddress Physical address result
 * @param permissions Page permissions
 * @return TranslationResult containing the translation data
 */
inline TranslationResult makeTranslationSuccess(PA physicalAddress, PagePermissions permissions) {
    return makeSuccess(TranslationData(physicalAddress, permissions));
}

/**
 * @brief Create successful translation with complete data
 * @param physicalAddress Physical address result
 * @param permissions Page permissions
 * @param securityState Security state
 * @return TranslationResult containing the translation data
 */
inline TranslationResult makeTranslationSuccess(PA physicalAddress, PagePermissions permissions, SecurityState securityState) {
    return makeSuccess(TranslationData(physicalAddress, permissions, securityState));
}

/**
 * @brief Create translation error with SMMUError
 * @param error The error code
 * @return TranslationResult containing the error
 */
inline TranslationResult makeTranslationError(SMMUError error) {
    return makeError<TranslationData>(error);
}

/**
 * @brief Create translation error with FaultType
 * @param faultType The fault type to convert
 * @return TranslationResult containing the converted error
 */
inline TranslationResult makeTranslationError(FaultType faultType) {
    return makeError<TranslationData>(faultTypeToSMMUError(faultType));
}

///@}

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

///@{
/// @name ARM SMMU v3 Configuration Constants
/// @details Fundamental constants from ARM SMMU v3 specification

/// @brief Maximum Stream ID value (32-bit)
/// @details As per ARM SMMU v3 specification
constexpr uint32_t MAX_STREAM_ID = 0xFFFFFFFF;

/// @brief Maximum PASID value (20-bit PASID space)
/// @details ARM SMMU v3 supports 20-bit PASIDs = 1,048,576 values
constexpr uint32_t MAX_PASID = 0xFFFFF;

/// @brief Standard page size (4KB pages)
/// @details Default translation granule size
constexpr uint64_t PAGE_SIZE = 4096;

/// @brief Page alignment mask
/// @details Used for page-aligned address calculations
constexpr uint64_t PAGE_MASK = PAGE_SIZE - 1;

/// @brief Maximum supported virtual address space (52-bit)
/// @details ARM SMMU v3 specification supports up to 52-bit address spaces
constexpr uint64_t MAX_VIRTUAL_ADDRESS = 0x000FFFFFFFFFFFFFULL;

/// @brief Maximum supported physical address space (52-bit) 
/// @details ARM SMMU v3 specification supports up to 52-bit physical addresses
constexpr uint64_t MAX_PHYSICAL_ADDRESS = 0x000FFFFFFFFFFFFFULL;

///@}

///@{
/// @name Queue Size Constants
/// @details Default queue sizes for ARM SMMU v3 event processing

/// @brief Default event queue size (512 entries)
constexpr size_t DEFAULT_EVENT_QUEUE_SIZE = 512;

/// @brief Default command queue size (256 entries)
constexpr size_t DEFAULT_COMMAND_QUEUE_SIZE = 256;

/// @brief Default PRI queue size (128 entries)
constexpr size_t DEFAULT_PRI_QUEUE_SIZE = 128;

///@}

} // namespace smmu

/**
 * @example translation_example.cpp
 * Basic translation operation using ARM SMMU v3 types:
 * @code
 * #include "smmu/types.h"
 * 
 * using namespace smmu;
 * 
 * TranslationResult translateAddress(IOVA iova) {
 *     // Simulate translation logic
 *     if (iova == 0) {
 *         return makeTranslationError(SMMUError::InvalidAddress);
 *     }
 *     
 *     PA physicalAddr = 0x1000 + (iova & PAGE_MASK);
 *     PagePermissions perms(true, true, false); // RW-
 *     return makeTranslationSuccess(physicalAddr, perms);
 * }
 * 
 * void handleTranslation() {
 *     TranslationResult result = translateAddress(0x2000);
 *     
 *     if (result.isOk()) {
 *         PA physAddr = result.getValue().physicalAddress;
 *         printf("Translation succeeded: 0x%lx\n", physAddr);
 *     } else {
 *         SMMUError error = result.getError();
 *         printf("Translation failed: %d\n", static_cast<int>(error));
 *     }
 * }
 * @endcode
 */

#endif // SMMU_TYPES_H