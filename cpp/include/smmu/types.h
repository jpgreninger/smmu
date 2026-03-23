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
    StateTransitionError,

    // GBPA.ABORT — disabled-SMMU abort path (FINDING-NEW-01)
    /// @brief Transaction aborted by SMMU_GBPA.ABORT (§3.11, §13.2).
    ///        Returned when SMMUEN=0 and GBPA.ABORT=1; all transactions are
    ///        aborted instead of bypassed with an identity mapping.
    GbpaAbort,

    /// @brief Transaction stalled, awaiting CMD_RESUME (§3.12.2, §4.6).
    ///        Returned when FaultMode::Stall is active and a fault occurs.
    ///        The caller must issue CMD_RESUME with the STAG returned in the event.
    Stalled,

    /// @brief S1DSS==0b00 abort: non-substream (PASID==0) transaction on a
    ///        substream-capable stream (S1CDMax > 0) when STE.S1DSS==0b00.
    ///        §5.2 / §7.3.7: transaction aborts AND F_STREAM_DISABLED (0x06)
    ///        is recorded.  Distinct from SMMUError::StreamDisabled which is
    ///        the STE.Config==0b000 silent-abort with NO event.
    SubstreamDisabled,

    /// @brief Access Flag Fault: page has AF=0, HA=0, and AFFD=0 (§3.13.2, NEW-GAP-J).
    ///        The transaction must fault with F_ACCESS (EventType 0x12).
    AccessFlagFaultError
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
            } else {
                // BUG-18 fix: reset stale value when assigning from an error state
                value = T();
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
            } else {
                // BUG-18 fix: reset stale value when assigning from an error state
                value = T();
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
    Execute,
    /// @brief Read-write access - atomic read-modify-write operations
    ReadWrite,
    /// @brief Read and Execute access (code pages allowing both load and fetch)
    ReadExecute,
    /// @brief Read and Execute access in privileged mode (ignores privilegedOnly check)
    ReadExecutePrivileged,
    /// @brief Read access in privileged mode (ignores privilegedOnly check)
    ReadPrivileged,
    /// @brief Write access in privileged mode (ignores privilegedOnly check)
    WritePrivileged,
    /// @brief Execute access in privileged mode (ignores privilegedOnly check)
    ExecutePrivileged,
    /// @brief Read-write access in privileged mode (ignores privilegedOnly check)
    ReadWritePrivileged
};

/**
 * @enum SecurityState
 * @brief ARM SMMU v3 security state enumeration (§3.10)
 * @details Defines security context for memory transactions.
 *          SMMUv3.3 RME adds Root state (0b11) with highest privilege.
 *          2-bit encoding: NonSecure=0b01, Secure=0b00, Realm=0b10, Root=0b11.
 *          Used for access control and security fault detection.
 */
enum class SecurityState : uint8_t {
    /// @brief Non-secure state - normal world access (SEC_SID 0b00 = 0x00) per §3.10
    NonSecure = 0x00,
    /// @brief Secure state - secure world access (SEC_SID 0b01 = 0x01) per §3.10
    Secure    = 0x01,
    /// @brief Realm state - confidential computing access (SEC_SID 0b10 = 0x02) per §3.10
    Realm     = 0x02,
    /// @brief Root state - SMMUv3.3 RME highest privilege, can access all PA spaces (SEC_SID 0b11 = 0x03) per §3.10
    Root      = 0x03
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
    Stage2PermissionFault,

    /// @brief Stream disabled — STE.Config indicates disabled/abort stream (§7.3.7)
    /// Generates F_STREAM_DISABLED event (event code 0x06) per ARM IHI0070G.b §7.3.7.
    StreamDisabled,

    /// @brief StreamID not found in stream table (§7.3.3)
    /// Generates C_BAD_STREAMID event (event code 0x02) per ARM IHI0070G.b §7.3.3.
    BadStreamID,

    /// @brief StreamID is in-range but STE.V=0 — no valid STE (§7.3.5)
    /// Generates C_BAD_STE event (event code 0x04) per ARM IHI0070G.b §7.3.5.
    /// Distinct from BadStreamID (§7.3.3) which fires only when StreamID is
    /// outside the configured stream table range (>= 2^LOG2SIZE).
    BadSTE,

    /// @brief Non-zero SubstreamID/PASID on a stream with no stage-1 translation (§3.9, §7.3.9)
    /// Generates C_BAD_SUBSTREAMID event (event code 0x08) when a PASID is supplied
    /// to a stage-2-only or bypass stream that has no stage-1 context to consume it.
    BadSubstreamId
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
    /// @brief If true, page is restricted to privileged access modes only (ARM §5.2 PRIVCFG)
    bool privilegedOnly;

    /**
     * @brief Default constructor - no permissions
     * @details All permissions set to false for security.
     */
    PagePermissions() : read(false), write(false), execute(false), privilegedOnly(false) {
    }

    /**
     * @brief Constructor with explicit permissions
     * @param r Read permission
     * @param w Write permission
     * @param x Execute permission
     */
    PagePermissions(bool r, bool w, bool x) : read(r), write(w), execute(x), privilegedOnly(false) {
    }

    /**
     * @brief Constructor with explicit permissions including privileged-only flag
     * @param r Read permission
     * @param w Write permission
     * @param x Execute permission
     * @param priv If true, page requires privileged access mode
     */
    PagePermissions(bool r, bool w, bool x, bool priv) : read(r), write(w), execute(x), privilegedOnly(priv) {
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
    /// @brief 4-bit resolved memory type; 0 = from-translation, 0xF = Normal WB when MTCFG=1
    uint8_t memType;
    /// @brief 2-bit shareability: 0=NSH, 1=from-translation, 2=OSH, 3=ISH
    uint8_t shareability;
    /// @brief 4-bit allocation hint (ALLOCCFG pass-through)
    uint8_t allocHint;
    /// @brief 2-bit instruction/data hint: 0=from-translation, 1=data, 2=instruction
    uint8_t instCfg;
    /// @brief 2-bit privilege attribute: 0=from-translation, 1=unpriv, 2=priv
    uint8_t privCfg;
    /// @brief 2-bit resolved NS output attribute
    uint8_t nsCfgOut;
    /// @brief CONF-GAP-7: Stage-1 output (IPA) for two-stage translations.
    /// Set to the IPA produced by Stage-1 when both stages are enabled, so that
    /// TLB entries can be tagged with the IPA for selective TLBI_S2_IPA invalidation.
    /// Zero for single-stage translations (no IPA).
    uint64_t ipa;

    /**
     * @brief Default constructor
     * @details Physical address = 0, NonSecure state, no permissions.
     */
    TranslationData() : physicalAddress(0), securityState(SecurityState::NonSecure),
                        memType(0), shareability(0), allocHint(0), instCfg(0), privCfg(0), nsCfgOut(0),
                        ipa(0) {
    }

    /**
     * @brief Constructor with physical address only
     * @param pa Physical address
     * @details Security state defaults to NonSecure, no permissions.
     */
    TranslationData(PA pa) : physicalAddress(pa), securityState(SecurityState::NonSecure),
                             memType(0), shareability(0), allocHint(0), instCfg(0), privCfg(0), nsCfgOut(0),
                             ipa(0) {
    }

    /**
     * @brief Constructor with address and permissions
     * @param pa Physical address
     * @param perms Page permissions
     * @details Security state defaults to NonSecure.
     */
    TranslationData(PA pa, PagePermissions perms) : physicalAddress(pa), permissions(perms),
                                                    securityState(SecurityState::NonSecure),
                                                    memType(0), shareability(0), allocHint(0),
                                                    instCfg(0), privCfg(0), nsCfgOut(0), ipa(0) {
    }

    /**
     * @brief Constructor with full translation data
     * @param pa Physical address
     * @param perms Page permissions
     * @param secState Security state
     */
    TranslationData(PA pa, PagePermissions perms, SecurityState secState) : physicalAddress(pa),
                                                                            permissions(perms),
                                                                            securityState(secState),
                                                                            memType(0), shareability(0),
                                                                            allocHint(0), instCfg(0),
                                                                            privCfg(0), nsCfgOut(0),
                                                                            ipa(0) {
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
            
        case FaultType::StreamDisabled:
            return SMMUError::StreamDisabled;

        case FaultType::BadSubstreamId:
            return SMMUError::InvalidPASID;

        case FaultType::BadStreamID:
            // BUG-CPP-DBGR-12 fix: §7.3.3 C_BAD_STREAMID maps to InvalidStreamID.
            return SMMUError::InvalidStreamID;

        case FaultType::BadSTE:
            // §7.3.5 C_BAD_STE: in-range StreamID with STE.V=0 → StreamNotConfigured.
            return SMMUError::StreamNotConfigured;

        case FaultType::AccessFlagFault:
            return SMMUError::AccessFlagFaultError;

        case FaultType::AccessFault:
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
        case SMMUError::AccessFlagFaultError:
            return FaultType::AccessFlagFault;
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
    bool accessFlag;    // Hardware Access Flag (AF) — set on first access when CD.HA=1
    bool dirty;         // Hardware Dirty State — set on write when CD.HD=1
    bool deviceMemory;  // §S2PTW: page mapped as Device memory type; defaults to false

    PageEntry() : physicalAddress(0), valid(false), securityState(SecurityState::NonSecure),
                  accessFlag(false), dirty(false), deviceMemory(false) {
    }

    PageEntry(PA pa, PagePermissions perms) : physicalAddress(pa), permissions(perms), valid(true),
                                              securityState(SecurityState::NonSecure),
                                              accessFlag(false), dirty(false), deviceMemory(false) {
    }

    PageEntry(PA pa, PagePermissions perms, SecurityState secState) : physicalAddress(pa),
                                                                       permissions(perms),
                                                                       valid(true),
                                                                       securityState(secState),
                                                                       accessFlag(false),
                                                                       dirty(false),
                                                                       deviceMemory(false) {
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

/**
 * @enum StreamWorld
 * @brief ARM SMMU v3 STE.STRW stream world field (§5.2)
 * @details 2-bit field that selects the exception level associated with the stream.
 */
enum class StreamWorld : uint8_t {
    EL1_EL0 = 0x00, // §5.2 STRW=0b00: NS-EL1/EL0
    EL2     = 0x01, // §5.2 STRW=0b01: NS-EL2
    EL2_E2H = 0x02, // §5.2 STRW=0b10: NS-EL2 with VHE
    EL3     = 0x03  // §5.2 STRW=0b11: EL3/Secure
};

// Stream configuration structure
struct StreamConfig {
    bool translationEnabled;
    bool stage1Enabled;
    bool stage2Enabled;
    /// §5.2 STE.Config bypass mode: when true this stream uses STE.Config==0b100
    /// (bypass — identity PA==IOVA). When false and all translation stages are
    /// disabled, this stream uses STE.Config==0b000 (disabled — silent abort, no event).
    bool bypassEnabled;  ///< defaults to false (STE.Config==0b000 = disabled/abort)
    FaultMode faultMode;
    bool ha;  // Hardware Access Flag management enabled (CD.HA)
    bool hd;  // Hardware Dirty State management enabled (CD.HD)
    uint16_t asid;  // CD.ASID (ARM §3.17): ASID tag for Stage-1 TLB entries (§4.4 targeted invalidation)
    uint16_t vmid;  // STE.S2VMID (ARM §5.2): VMID tag for Stage-2 TLB entries (§4.4 targeted invalidation)
    /// ARM §5.2 STE.S1DSS: controls behavior when a non-substream transaction
    /// (PASID==0) arrives on a substream-capable stage-1 stream (s1cdMax > 0).
    ///   0b00 = abort with F_STREAM_DISABLED (§7.3.7)
    ///   0b01 = bypass stage-1 for this transaction (identity PA = IOVA)
    ///   0b10 = use CD[0] for translation (default — preserves existing behavior)
    uint8_t s1dss;  ///< defaults to 0b10 (use CD[0])
    /// ARM §5.2 STE.S1CDMax: number of SubstreamID bits supported by this stream.
    /// 0 = stream not substream-capable (s1dss ignored, PASID=0 always uses CD[0]).
    /// >0 = stream supports substreams; s1dss governs non-substream PASID=0 handling.
    uint8_t s1cdMax;  ///< defaults to 0 (not substream-capable)

    // §5.2 STE.STRW: stream world / exception level selection (CT-20)
    StreamWorld strw;  ///< defaults to EL1_EL0 (0b00)

    // §5.2 STE output attribute override fields (CT-19)
    uint8_t nsCfg;    ///< 2-bit NSCFG non-secure attribute override; default 0
    uint8_t shCfg;    ///< 2-bit SHCFG shareability override; default 0
    uint8_t allocCfg; ///< 4-bit ALLOCCFG allocation hint override; default 0
    uint8_t memAttr;  ///< 4-bit MemAttr memory type attribute; default 0
    uint8_t instCfg;  ///< 2-bit INSTCFG instruction/data override; default 0
    uint8_t privCfg;  ///< 2-bit PRIVCFG privilege attribute override; default 0
    bool    mtCfg;    ///< MTCFG memory type override enable flag; default false

    // §5.4 CD.T0SZ / CD.T1SZ (CT-13): valid range 0-39 for SMMUv3.0
    uint8_t t0sz;     ///< CD.T0SZ — bits to exclude from top of TTBR0 range; default 16
    uint8_t t1sz;     ///< CD.T1SZ — bits to exclude from top of TTBR1 range; default 16

    // §5.4 CD.AA64 (CT-14): 1=AArch64 stage-1 tables, 0=AArch32 LPAE (unsupported)
    bool aa64;        ///< CD.AA64 — true=AArch64 (default); false=AArch32 LPAE

    // §5.4 CD.EPD0/EPD1: disable TTBR0/TTBR1 translation table walk (NEW-7)
    bool epd0;  ///< §5.4 CD.EPD0: disable TTBR0 translation table walk; default false
    bool epd1;  ///< §5.4 CD.EPD1: disable TTBR1 translation table walk; default false

    // §5.2 Stage-2 STE translation parameters (CT-23)
    uint8_t  s2t0sz;  ///< 6-bit Stage-2 T0SZ; default 16
    uint8_t  s2tg;    ///< 2-bit Stage-2 granule (0=4KB, 1=64KB, 2=16KB); default 0
    uint8_t  s2sl0;   ///< 2-bit Stage-2 starting level (0=L2, 1=L1, 2=L0); default 1
    bool     s2aa64;  ///< 1-bit AArch64 stage-2 tables; default true
    uint8_t  s2ps;    ///< 3-bit Stage-2 physical address size (5=48-bit); default 5
    uint64_t s2ttb;   ///< Physical address of stage-2 root table; default 0

    // §3.10 / FINDING-NEW-39: Security state of this stream's STE.
    // Used to carry the correct security state into completion events
    // (ATC_INVALIDATE_COMPLETION, COMMAND_SYNC_COMPLETION) per ARM §4.5.1, §4.8.
    // Default: NonSecure (0x00).
    SecurityState securityState;  ///< Stream security state; defaults to NonSecure

    /// CONF-GAP-14: ARM §5.2 STE.MEV — merge (suppress duplicate) fault events.
    /// When true, a fault event of the same type for this stream already in the
    /// event queue will suppress a new identical event (merge/dedup).
    bool mev;  ///< STE.MEV; defaults to false (no merging)

    /// CONF-GAP-16: ARM §5.2 STE.S2S — Stage-2 Secure bit.
    /// When true, stage-2 translation uses Secure PA space.
    bool s2s;  ///< STE.S2S; defaults to false
    /// CONF-GAP-16: ARM §5.2 STE.EATS — Enhanced Address Translation Security.
    /// 2-bit field; 0=off, other values per spec.
    uint8_t eats;  ///< STE.EATS; defaults to 0

    /// GAP-E: ARM IHI0070G.b §3.4.1 / §5.4 CD.TBI — Top-Byte Ignore.
    /// When true, VA bits[63:56] are masked (zeroed) before the T0SZ range check.
    /// The top byte is treated as a tag and is not part of the effective address.
    /// Default false (top byte participates in range check).
    bool tbi;  ///< CD.TBI; defaults to false

    /// GAP-F: ARM IHI0070G.b §5.4 CD.IPS — stage-1 output IPA size (3-bit).
    /// Encodes the maximum IPA width from stage-1 translation.  Same encoding as S2PS:
    ///   0b000=32-bit, 0b001=36-bit, 0b010=40-bit, 0b011=42-bit,
    ///   0b100=44-bit, 0b101=48-bit, 0b110=52-bit.
    /// An IPA from stage-1 that exceeds 2^IPS → F_ADDR_SIZE (§7.3.14).
    /// Default 6 (52-bit, no effective restriction).
    uint8_t ips;  ///< CD.IPS 3-bit encoding; defaults to 6 (52-bit)

    /// GAP-NEW-G: ARM IHI0070G.b §5.2 STE.S1STALLD — Stage-1 Stall Disabled.
    /// When true, forces abort semantics even when faultMode == FaultMode::Stall.
    /// Translations for this stream will never enter stall mode; they abort
    /// immediately and generate an event as if FaultMode::Terminate were set.
    /// Default false (backward compatible: stall mode operates normally).
    bool s1Stalld;  ///< STE.S1STALLD; defaults to false

    /// NEW-GAP-J: ARM IHI0070G.b §5.4 CD.AFFD — Access Flag Fault Disable.
    /// When true, disables F_ACCESS generation for stage-1 (AF=0 does not fault).
    /// When false (default) and HA=false, AF=0 causes F_ACCESS (§3.13.2).
    bool affd;   ///< CD.AFFD; defaults to false

    /// NEW-GAP-J: ARM IHI0070G.b §5.2 STE.S2AFFD — Stage-2 Access Flag Fault Disable.
    /// When true, disables F_ACCESS generation for stage-2 (AF=0 does not fault).
    bool s2affd;  ///< STE.S2AFFD; defaults to false

    /// NEW-GAP-J: ARM IHI0070G.b §5.2 STE.S2HA — Stage-2 Hardware AF management.
    /// When true, hardware sets stage-2 AF=1 on first access (no F_ACCESS).
    bool s2ha;   ///< STE.S2HA; defaults to false

    /// NEW-GAP-J: ARM IHI0070G.b §5.2 STE.S2HD — Stage-2 Hardware Dirty management.
    bool s2hd;   ///< STE.S2HD; defaults to false

    /// NEW-GAP-K: ARM IHI0070G.b §5.4 CD.WXN — Write eXecute Never.
    /// When true, any writable page is also non-executable.
    /// Execute or ExecutePrivileged access to a write-permitted page → F_PERMISSION.
    bool wxn;    ///< CD.WXN; defaults to false

    /// NEW-GAP-K: ARM IHI0070G.b §5.4 CD.UWXN — Unprivileged Write eXecute Never.
    /// When true, privileged Execute to an unprivileged-writable page → F_PERMISSION.
    bool uwxn;   ///< CD.UWXN; defaults to false

    /// NEW-GAP-L: ARM IHI0070G.b §5.2 STE.S2PTW — Protected Table Walk.
    /// When true in a two-stage stream, translation through a Device-memory stage-2
    /// page → F_PERMISSION (prevents TTW from hitting device MMIO regions).
    bool s2ptw;  ///< STE.S2PTW; defaults to false

    StreamConfig() : translationEnabled(false), stage1Enabled(false),
                    stage2Enabled(false), bypassEnabled(false), faultMode(FaultMode::Terminate),
                    ha(false), hd(false), asid(0), vmid(0), s1dss(2), s1cdMax(0),
                    strw(StreamWorld::EL1_EL0),
                    nsCfg(0), shCfg(0), allocCfg(0), memAttr(0), instCfg(0), privCfg(0), mtCfg(false),
                    t0sz(16), t1sz(16), aa64(true),
                    epd0(false), epd1(false),
                    s2t0sz(16), s2tg(0), s2sl0(1), s2aa64(true), s2ps(5), s2ttb(0),
                    securityState(SecurityState::NonSecure),
                    mev(false), s2s(false), eats(0),
                    tbi(false), ips(6),
                    s1Stalld(false),
                    affd(false), s2affd(false), s2ha(false), s2hd(false),
                    wxn(false), uwxn(false),
                    s2ptw(false) {
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
        if (endAddress <= startAddress) {
            return 0;
        }
        uint64_t diff = endAddress - startAddress;
        // BUG-05 fix: adding 1 when diff == UINT64_MAX would overflow.
        // Return UINT64_MAX as a sentinel for the maximum possible range.
        return (diff < UINT64_MAX) ? (diff + 1) : UINT64_MAX;
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
    uint16_t asid;  // CD.ASID tag — used for CMD_TLBI_NH_ASID / CMD_TLBI_EL2_ASID (ARM §4.4)
    uint16_t vmid;  // STE.S2VMID tag — used for CMD_TLBI_S12_VMALL / CMD_TLBI_S2_IPA (ARM §4.4)
    /// CONF-GAP-7: ARM §4.4 TLBI_S2_IPA operand.
    /// For two-stage translation entries this is the stage-1 output address (IPA)
    /// used as the operand of CMD_TLBI_S2_IPA to perform selective IPA invalidation.
    /// Zero for single-stage entries (no IPA to compare against).
    uint64_t ipa;   // Stage-1 output (IPA) for two-stage entries; 0 for single-stage

    TLBEntry() : streamID(0), pasid(0), iova(0), physicalAddress(0),
                 securityState(SecurityState::NonSecure), valid(false), timestamp(0),
                 asid(0), vmid(0), ipa(0) {
    }

    TLBEntry(StreamID sid, PASID p, IOVA iva, PA pa, PagePermissions perms, SecurityState secState)
        : streamID(sid), pasid(p), iova(iva), physicalAddress(pa), permissions(perms),
          securityState(secState), valid(true), timestamp(0), asid(0), vmid(0), ipa(0) {
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

/// @brief CONF-GAP-13: GBPA output attribute fields (ARM §6.3.22).
/// Populated from SMMU_GBPA register fields; applied on bypass (SMMUEN=0, ABORT=0).
struct GbpaConfig {
    bool abort;       ///< ABORT bit — abort transaction when set
    uint8_t instCfg;  ///< 2-bit INSTCFG instruction/data override
    uint8_t privCfg;  ///< 2-bit PRIVCFG privilege attribute override
    bool mtCfg;       ///< MTCFG memory type override enable
    uint8_t memAttr;  ///< 4-bit MemAttr memory type attribute
    uint8_t shCfg;    ///< 2-bit SHCFG shareability override
    uint8_t allocCfg; ///< 4-bit ALLOCCFG allocation hint override

    GbpaConfig() : abort(false), instCfg(0), privCfg(0), mtCfg(false),
                   memAttr(0), shCfg(0), allocCfg(0) {
    }
};

/// @brief CONF-GAP-3: 2-level stream table format selection (ARM §3.3.1.2).
enum class StreamTableFormat {
    Linear  = 0, ///< Linear (flat) stream table — default
    TwoLevel = 1 ///< 2-level stream table using L1+L2 index split
};

/// @brief NEW-12: ARM §3.9 ATS Transaction Type.
/// Classifies the transaction as an ordinary (non-ATS) request, an ATS
/// Translation Request (TR), or an ATS Translated (TT) transaction.
enum class TransactionType : uint8_t {
    Ordinary              = 0, ///< Normal (non-ATS) upstream transaction
    AtsTranslationRequest = 1, ///< ATS TR: SMMU must perform full translation
    AtsTranslated         = 2  ///< ATS TT: upstream device provides pre-translated PA
};

/// @brief CONF-GAP-18: CMD_SYNC completion signal type (ARM §4.7.3).
enum class CmdSyncSignalType {
    None = 0, ///< SIG_NONE: no completion signal (CS=0b00)
    Irq  = 1, ///< SIG_IRQ: interrupt/MSI completion signal (CS=0b01)
    Msi  = 2  ///< SIG_MSI: MSI write completion signal (CS=0b10)
};

// ARM §6.3.17: CMDQ_CONS.ERR field (CONF-GAP-17).
static constexpr uint32_t CMDQ_CONS_ERR_SHIFT  = 24u;    ///< ERR field bit position
static constexpr uint32_t CERROR_NONE           = 0u;    ///< No error
static constexpr uint32_t CERROR_ILL            = 1u;    ///< Illegal command (CERROR_ILL)
static constexpr uint32_t CERROR_ABT            = 2u;    ///< Command queue memory abort
static constexpr uint32_t CERROR_ATC_INV_SYNC   = 3u;    ///< ATC invalidate sync error

// ARM §6.3.17: SMMU_GERROR bit constants — spec-correct bit positions.
// Set by hardware; cleared by software writing to SMMU_GERRORN.
static constexpr uint32_t GERROR_CMDQ_ERR           = (1u << 0); ///< bit 0: Command queue processing error
static constexpr uint32_t GERROR_EVENTQ_ABT_ERR     = (1u << 2); ///< bit 2: Event queue memory system abort
static constexpr uint32_t GERROR_PRIQ_ABT_ERR       = (1u << 3); ///< bit 3: PRI queue memory system abort
static constexpr uint32_t GERROR_MSI_CMDQ_ABT_ERR   = (1u << 4); ///< bit 4: MSI write abort for command queue
static constexpr uint32_t GERROR_MSI_EVENTQ_ABT_ERR = (1u << 5); ///< bit 5: MSI write abort for event queue
static constexpr uint32_t GERROR_MSI_PRIQ_ABT_ERR   = (1u << 6); ///< bit 6: MSI write abort for PRI queue
static constexpr uint32_t GERROR_MSI_GERROR_ABT_ERR = (1u << 7); ///< bit 7: MSI write abort for GERROR
static constexpr uint32_t GERROR_SFM_ERR            = (1u << 8); ///< bit 8: Service Fault Mapping error
static constexpr uint32_t GERROR_CMDQP_ERR          = (1u << 9); ///< bit 9: Command queue paused error
// Backward-compat aliases for renamed constants:
static constexpr uint32_t GERROR_CMDQ_ABT_ERR   = GERROR_MSI_CMDQ_ABT_ERR;  ///< alias: use GERROR_MSI_CMDQ_ABT_ERR
static constexpr uint32_t GERROR_MSI_ABT_ERR    = GERROR_MSI_EVENTQ_ABT_ERR; ///< alias: use GERROR_MSI_EVENTQ_ABT_ERR
static constexpr uint32_t GERROR_SFE            = GERROR_SFM_ERR;             ///< alias: use GERROR_SFM_ERR

// Task 5.3: Event and Command Processing - Command types for SMMU command queue
// Opcode values match ARM IHI0070G.b §4.1.1 exactly.
enum class CommandType {
    PREFETCH_CONFIG = 0x01, // CMD_PREFETCH_CONFIG
    PREFETCH_ADDR   = 0x02, // CMD_PREFETCH_ADDR
    CFGI_STE        = 0x03, // CMD_CFGI_STE — Stream Table Entry invalidation
    CFGI_ALL        = 0x04, // CMD_CFGI_ALL / CMD_CFGI_STE_RANGE (same opcode per §4.1.1)
    CFGI_CD         = 0x05, // CMD_CFGI_CD — Context Descriptor invalidation for (SID, SSID)
    CFGI_CD_ALL     = 0x06, // CMD_CFGI_CD_ALL — Context Descriptor invalidation for all SSIDs of SID
    TLBI_NH_ALL     = 0x10, // CMD_TLBI_NH_ALL — TLB invalidation non-secure hyp all
    TLBI_NH_ASID    = 0x11, // CMD_TLBI_NH_ASID
    TLBI_NH_VA      = 0x12, // CMD_TLBI_NH_VA
    TLBI_NH_VAA     = 0x13, // CMD_TLBI_NH_VAA
    TLBI_EL2_ALL    = 0x20, // CMD_TLBI_EL2_ALL — TLB invalidation EL2 all
    TLBI_EL2_ASID   = 0x21, // CMD_TLBI_EL2_ASID
    TLBI_EL2_VA     = 0x22, // CMD_TLBI_EL2_VA
    TLBI_EL2_VAA    = 0x23, // CMD_TLBI_EL2_VAA
    TLBI_S12_VMALL  = 0x28, // CMD_TLBI_S12_VMALL — TLB invalidation stage 1&2 VM all
    TLBI_S2_IPA     = 0x2A, // CMD_TLBI_S2_IPA
    TLBI_NSNH_ALL   = 0x30, // CMD_TLBI_NSNH_ALL
    ATC_INV         = 0x40, // CMD_ATC_INV — Address Translation Cache invalidation
    PRI_RESP        = 0x41, // CMD_PRI_RESP — Page Request Interface response
    RESUME          = 0x44, // CMD_RESUME — Resume stalled transaction
    STALL_TERM      = 0x45, // CMD_STALL_TERM — Terminate stalled transaction
    SYNC            = 0x46, // CMD_SYNC — Synchronization barrier
    // §4.1.1: Additional spec-defined command opcodes
    CFGI_VMS_PIDM   = 0x07, // §4.1.1: Secure substream PIDM cache invalidation
    TLBI_EL3_ALL    = 0x18, // §4.1.1: Invalidate all EL3 TLB entries
    TLBI_EL3_VA     = 0x1A, // §4.1.1: Invalidate EL3 TLB entries by VA
    TLBI_S_EL2_ALL  = 0x50, // §4.1.1: Invalidate all Secure EL2 TLB entries
    TLBI_S_EL2_ASID = 0x51, // §4.1.1: Invalidate Secure EL2 TLB by ASID
    TLBI_S_EL2_VA   = 0x52, // §4.1.1: Invalidate Secure EL2 TLB by VA
    TLBI_S_EL2_VAA  = 0x53, // §4.1.1: Invalidate Secure EL2 TLB by VA, all ASID
    TLBI_S_S12_VMALL = 0x58, // §4.1.1: Invalidate all Secure S12 TLB by VMID
    TLBI_S_S2_IPA   = 0x5A, // §4.1.1: Invalidate Secure S2 TLB by IPA
    TLBI_SNH_ALL    = 0x60, // §4.1.1: Invalidate all Secure NH TLB entries
    DPTI_ALL        = 0x70, // §4.1.1: Dirty page tracking invalidation, all
    DPTI_PA         = 0x73  // §4.1.1: Dirty page tracking invalidation by PA
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
    uint16_t prgIndex;  // ARM §8.3 PRGIndex — echoed back in CMD_PRI_RESP
    /// Range field for CMD_CFGI_ALL / CMD_CFGI_STE_RANGE (ARM §4.3.2).
    /// 31 → CMD_CFGI_ALL (full global invalidation).
    /// <31 → CMD_CFGI_STE_RANGE: match streams where (sid >> (range+1)) == (streamID >> (range+1)).
    uint8_t range;  // defaults to 31 (CMD_CFGI_ALL semantics)
    uint16_t stag;  ///< ARM §4.6: STAG field — identifies stalled transaction for RESUME/STALL_TERM
    bool action;    ///< ARM §4.6: Ac bit — true=retry, false=terminate or abort
    bool abort;     ///< ARM §4.6: Ab bit — true=abort with bus error, false=terminate successfully (only when action=false)
    uint16_t asid;  ///< ARM §4.4: ASID operand for CMD_TLBI_NH_ASID / CMD_TLBI_EL2_ASID
    uint16_t vmid;  ///< ARM §4.4: VMID operand for CMD_TLBI_S12_VMALL / CMD_TLBI_S2_IPA
    /// ARM §4.3.1 / §4.3.3: Leaf bit for CMD_CFGI_STE and CMD_CFGI_CD.
    /// When false (Leaf=0), both the target entry and any cached intermediate
    /// L1ST/L1CD descriptor structures are invalidated.
    /// When true (Leaf=1), only the target entry is invalidated; intermediate
    /// structures need not be invalidated.
    /// This software model does not cache intermediate table structures, so
    /// both values produce equivalent results (semantically a no-op here).
    bool leaf;  ///< defaults to false (Leaf=0 — full invalidation)
    /// ARM §4.8: CS (Completion Signalling) field for CMD_SYNC.
    /// 0b00 (SIG_NONE) — no completion signal; 0b01 (SIG_IRQ) — MSI/IRQ;
    /// 0b10 (SIG_MSI) — MSI write.  Ignored by other command types.
    uint8_t cs;  ///< defaults to 0 (SIG_NONE)
    /// BUG-14 fix / ARM §7.3.3: Security state of the command's originating context.
    /// C_BAD_STREAMID and similar command-error events must be recorded with the
    /// security state matching the StreamID's security domain (§7.3 / §7.3.3).
    /// Defaults to NonSecure; Secure commands should set this to SecurityState::Secure.
    SecurityState securityState;  ///< defaults to NonSecure

    /// CONF-GAP-8: ARM §4.4.1.1 RIL (Range Invalidation Leaf) fields.
    /// Used by VA-targeted TLBI commands when ril=true.
    /// startAddress serves as the VA operand; tg/num/scale define the range.
    uint8_t tg;    ///< RIL Translation Granule: 0=4KB, 1=64KB, 2=16KB; default 0
    uint8_t num;   ///< RIL number-1 of granule blocks (5-bit: 0-30); default 0
    uint8_t scale; ///< RIL log2 scale factor; effective range 0-39; values >39 treated as 39 per ARM §4.4.1.1; range=(num+1)*(1<<(5*scale)) granules; default 0
    uint8_t ttl;   ///< RIL target TLB level hint (2-bit: 0=any,1=L1,2=L2,3=L3); default 0
    bool    ril;   ///< RIL range invalidation leaf flag; true=use TG/NUM/SCALE range; default false

    CommandEntry() : type(CommandType::SYNC), streamID(0), pasid(0),
                    startAddress(0), endAddress(0), flags(0), timestamp(0),
                    prgIndex(0), range(31), stag(0), action(false), abort(false),
                    asid(0), vmid(0), leaf(false), cs(0),
                    securityState(SecurityState::NonSecure),
                    tg(0), num(0), scale(0), ttl(0), ril(false) {
    }

    CommandEntry(CommandType cmdType, StreamID sid, PASID p, IOVA start, IOVA end)
        : type(cmdType), streamID(sid), pasid(p), startAddress(start), endAddress(end),
          flags(0), timestamp(0), prgIndex(0), range(31), stag(0), action(false), abort(false),
          asid(0), vmid(0), leaf(false), cs(0),
          securityState(SecurityState::NonSecure),
          tg(0), num(0), scale(0), ttl(0), ril(false) {
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
    uint16_t prgIndex;  // ARM §8.2 PRGIndex — Page Request Group index
    /// §7.3.19 / FINDING-NEW-32: Security state of the page request.
    /// The generated E_PAGE_REQUEST event must carry this state, not a
    /// hardcoded NonSecure.
    SecurityState securityState;

    PRIEntry() : streamID(0), pasid(0), requestedAddress(0),
                accessType(AccessType::Read), isLastRequest(false), timestamp(0),
                prgIndex(0), securityState(SecurityState::NonSecure) {
    }

    PRIEntry(StreamID sid, PASID p, IOVA addr, AccessType access)
        : streamID(sid), pasid(p), requestedAddress(addr), accessType(access),
          isLastRequest(false), timestamp(0), prgIndex(0),
          securityState(SecurityState::NonSecure) {
    }
};

/// GAP-H: ARM IHI0070G.b §3.13.6 — auto-generated PRG_RESPONSE on PRIQ overflow.
/// When the PRIQ is full and a new page request cannot be enqueued, the SMMU
/// automatically generates a FAILURE response for the overflowing request group
/// so the device is not left waiting indefinitely.
struct PRIAutoFailure {
    StreamID streamID;    ///< Stream that issued the overflowing page request
    PASID    pasid;       ///< PASID of the overflowing page request
    uint16_t prgIndex;    ///< Page Request Group index echoed back to device
    uint64_t timestamp;   ///< Time the auto-failure was generated

    PRIAutoFailure() : streamID(0), pasid(0), prgIndex(0), timestamp(0) {
    }

    PRIAutoFailure(StreamID sid, PASID p, uint16_t prg, uint64_t ts)
        : streamID(sid), pasid(p), prgIndex(prg), timestamp(ts) {
    }
};

// ARM IHI0070G.b §7.3 Event types — exact event numbers as discriminants
enum class EventType : uint8_t {
    // ── Spec-defined events §7.3 ────────────────────────────────────────────
    F_UUT                = 0x01,  // §7.3.2  Unsupported Upstream Transaction
    C_BAD_STREAMID       = 0x02,  // §7.3.3  StreamID out of range
    F_STE_FETCH          = 0x03,  // §7.3.4  STE fetch external abort
    C_BAD_STE            = 0x04,  // §7.3.5  Used STE invalid
    F_BAD_ATS_TREQ       = 0x05,  // §7.3.6  ATS Translation Request disallowed
    F_STREAM_DISABLED    = 0x06,  // §7.3.7  Non-substream transaction with stream disabled
    F_TRANSL_FORBIDDEN   = 0x07,  // §7.3.8  ATS Translated transaction disallowed
    C_BAD_SUBSTREAMID    = 0x08,  // §7.3.9  SubstreamID present but invalid
    F_CD_FETCH           = 0x09,  // §7.3.10 CD fetch external abort
    C_BAD_CD             = 0x0A,  // §7.3.11 Fetched CD invalid
    F_WALK_EABT          = 0x0B,  // §7.3.12 External abort during table walk
    F_TRANSLATION        = 0x10,  // §7.3.13 Translation fault
    F_ADDR_SIZE          = 0x11,  // §7.3.14 Address size fault
    F_ACCESS             = 0x12,  // §7.3.15 Access flag fault
    F_PERMISSION         = 0x13,  // §7.3.16 Permission fault
    F_TLB_CONFLICT       = 0x20,  // §7.3.17 TLB conflict
    F_CFG_CONFLICT       = 0x21,  // §7.3.18 Configuration cache conflict
    E_PAGE_REQUEST       = 0x24,  // §7.3.19 Page request hint
    F_VMS_FETCH          = 0x25,  // §7.3.20 VMS fetch external abort
    // ── Implementation-defined (§7.3.21, 0xE0–0xEF range) ──────────────────
    COMMAND_SYNC_COMPLETION   = 0xE0,  // IMPDEF: CMD_SYNC completion signalling
    ATC_INVALIDATE_COMPLETION = 0xE1   // IMPDEF: ATC_INV completion signalling
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
    bool stall;   ///< §7.3: true when this event corresponds to a stalled transaction (§3.5.3)
    uint16_t stag; ///< §3.12.2: Stall Tag — identifies the stalled transaction group; 0 when stall==false

    // CONF-GAP-20: §7.3 event record wire format fields
    uint64_t ipa;        ///< §7.3: Intermediate Physical Address (for two-stage faults)
    uint8_t  eventClass; ///< §7.3: CLASS field (2-bit): 0b00=CD, 0b01=TTD, 0b10=IN (default for F_* input-address faults), 0b11=Reserved. C_* events leave this 0 (field not defined).
    bool     s2;         ///< §7.3: S2 flag — true if fault occurred during stage-2 translation
    bool     rnw;        ///< §7.3: RnW (Read-not-Write) — true=read, false=write (ARM §7.3: RnW=1=Read, RnW=0=Write)
    bool     ind;        ///< §7.3: InD — true=instruction (Execute), false=data
    bool     pnu;        ///< §7.3: PnU — true=privileged access, false=unprivileged
    bool     nsipa;      ///< §7.3: NSIPA — true if the IPA is non-secure
    bool     ssv;        ///< §7.3: SSV — SubstreamID Valid (true when PASID != 0)

    // §7.3.6: F_BAD_ATS_TREQ ATS-specific permission fields — bits [95:92].
    // These are the permissions from the original ATS Translation Request (pre-STE-override).
    // Only valid/meaningful when event type == F_BAD_ATS_TREQ; RES0 for all other events.
    bool ats_r;  ///< bit[95]: ATS TR requested Read permission
    bool ats_w;  ///< bit[94]: ATS TR requested Write permission (= !NW)
    bool ats_x;  ///< bit[93]: ATS TR requested Execute permission
    bool ats_p;  ///< bit[92]: ATS TR requested Privileged access

    uint16_t reason; ///< §7.3.2: F_UUT Reason field at bits [79:64] — IMPLEMENTATION DEFINED; always 0 for this SW model

    EventEntry() : type(EventType::F_TLB_CONFLICT), streamID(0), pasid(0),
                  address(0), securityState(SecurityState::NonSecure), errorCode(0), timestamp(0),
                  stall(false), stag(0),
                  ipa(0), eventClass(0), s2(false), rnw(false), ind(false),
                  pnu(false), nsipa(false), ssv(false),
                  ats_r(false), ats_w(false), ats_x(false), ats_p(false), reason(0) {
    }

    EventEntry(EventType eventType, StreamID sid, PASID p, IOVA addr)
        : type(eventType), streamID(sid), pasid(p), address(addr),
          securityState(SecurityState::NonSecure), errorCode(0), timestamp(0),
          stall(false), stag(0),
          ipa(0), eventClass(0), s2(false), rnw(false), ind(false),
          pnu(false), nsipa(false), ssv(false),
          ats_r(false), ats_w(false), ats_x(false), ats_p(false), reason(0) {
    }

    EventEntry(EventType eventType, StreamID sid, PASID p, IOVA addr, SecurityState secState)
        : type(eventType), streamID(sid), pasid(p), address(addr),
          securityState(secState), errorCode(0), timestamp(0),
          stall(false), stag(0),
          ipa(0), eventClass(0), s2(false), rnw(false), ind(false),
          pnu(false), nsipa(false), ssv(false),
          ats_r(false), ats_w(false), ats_x(false), ats_p(false), reason(0) {
    }
};

/// @brief CONF-GAP-24: ARM §3.12.2 CMD_RESUME outcome classification (§4.6).
/// Records the disposition chosen by software when resuming a stalled transaction.
///   Retry     — Ac=1:          transaction may be retried.
///   Terminate — Ac=0, Ab=0:   transaction terminates successfully (RAZ/WI from device).
///   Abort     — Ac=0, Ab=1:   transaction aborts with bus error.
///   None      — no outcome recorded (STAG not found or not yet resumed).
enum class ResumeOutcome : uint8_t {
    None      = 0, ///< No outcome recorded (default / not-yet-resumed)
    Retry     = 1, ///< Ac=1: retry the stalled transaction
    Terminate = 2, ///< Ac=0, Ab=0: terminate successfully
    Abort     = 3  ///< Ac=0, Ab=1: abort with bus error
};

/// @brief ARM §3.12.2: Record of a stalled transaction awaiting CMD_RESUME.
struct StallRecord {
    uint16_t stag;               ///< Stall Tag — identifies the stalled transaction group
    StreamID streamID;           ///< Stream ID of the stalled transaction
    PASID pasid;                 ///< PASID of the stalled transaction
    IOVA iova;                   ///< Input address of the stalled transaction
    AccessType accessType;       ///< Access type of the stalled transaction
    SecurityState securityState; ///< Security state of the stalled transaction
    uint64_t timestamp;          ///< When the stall was recorded

    StallRecord()
        : stag(0), streamID(0), pasid(0), iova(0),
          accessType(AccessType::Read), securityState(SecurityState::NonSecure),
          timestamp(0) {
    }

    StallRecord(uint16_t s, StreamID sid, PASID p, IOVA i, AccessType at,
                SecurityState ss, uint64_t ts)
        : stag(s), streamID(sid), pasid(p), iova(i), accessType(at),
          securityState(ss), timestamp(ts) {
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
    bool ha;                                // Hardware Access Flag management enabled (CD bit 43)
    bool hd;                                // Hardware Dirty State management enabled (CD bit 42)

    ContextDescriptor()
        : ttbr0(0), ttbr1(0), asid(0), securityState(SecurityState::NonSecure),
          ttbr0Valid(false), ttbr1Valid(false), globalTranslations(false),
          contextDescriptorIndex(0), ha(false), hd(false) {
    }

    ContextDescriptor(uint64_t ttbr0Addr, uint16_t asidValue, SecurityState secState)
        : ttbr0(ttbr0Addr), ttbr1(0), asid(asidValue), securityState(secState),
          ttbr0Valid(true), ttbr1Valid(false), globalTranslations(false),
          contextDescriptorIndex(0), ha(false), hd(false) {
    }

    ContextDescriptor(uint64_t ttbr0Addr, uint64_t ttbr1Addr, uint16_t asidValue,
                     const TranslationControlRegister& tcrValue,
                     const MemoryAttributeRegister& mairValue, SecurityState secState)
        : ttbr0(ttbr0Addr), ttbr1(ttbr1Addr), tcr(tcrValue), mair(mairValue),
          asid(asidValue), securityState(secState), ttbr0Valid(true), ttbr1Valid(true),
          globalTranslations(false), contextDescriptorIndex(0), ha(false), hd(false) {
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
    uint16_t vmid;   // STE.S2VMID (ARM §5.2 Word 2 bits 63:48): VMID for Stage-2 TLB tagging
    uint16_t asid;   // CD.ASID (ARM §5.4 Word 1 bits 31:16): ASID for Stage-1 TLB tagging
    /// ARM §5.2 STE.S1DSS: controls behavior when a non-substream transaction
    /// (PASID==0) arrives on a substream-capable stage-1 stream (s1cdMax > 0).
    ///   0b00 = abort with F_STREAM_DISABLED (§7.3.7)
    ///   0b01 = bypass stage-1 for this transaction (identity PA = IOVA)
    ///   0b10 = use CD[0] for translation (default — preserves existing behavior)
    uint8_t s1dss;  ///< defaults to 0b10 (use CD[0])
    /// ARM §5.2 STE.S1CDMax: number of SubstreamID bits supported by this stream.
    /// 0 = stream not substream-capable (s1dss ignored, PASID=0 always uses CD[0]).
    /// >0 = stream supports substreams; s1dss governs non-substream PASID=0 handling.
    uint8_t s1cdMax;  ///< defaults to 0 (not substream-capable)

    // §5.2 STE.STRW: stream world / exception level selection (CT-20)
    StreamWorld strw;  ///< defaults to EL1_EL0 (0b00)

    // §5.2 STE output attribute override fields (CT-19)
    uint8_t nsCfg;    ///< 2-bit NSCFG non-secure attribute override; default 0
    uint8_t shCfg;    ///< 2-bit SHCFG shareability override; default 0
    uint8_t allocCfg; ///< 4-bit ALLOCCFG allocation hint override; default 0
    uint8_t memAttr;  ///< 4-bit MemAttr memory type attribute; default 0
    uint8_t instCfg;  ///< 2-bit INSTCFG instruction/data override; default 0
    uint8_t privCfg;  ///< 2-bit PRIVCFG privilege attribute override; default 0
    bool    mtCfg;    ///< MTCFG memory type override enable flag; default false

    // §5.4 CD.T0SZ / CD.T1SZ (CT-13): valid range 0-39 for SMMUv3.0
    uint8_t t0sz;     ///< CD.T0SZ; default 16
    uint8_t t1sz;     ///< CD.T1SZ; default 16

    // §5.4 CD.AA64 (CT-14)
    bool aa64;        ///< CD.AA64; default true (AArch64)

    // §5.2 Stage-2 STE translation parameters (CT-23)
    uint8_t  s2t0sz;  ///< Stage-2 T0SZ; default 16
    uint8_t  s2tg;    ///< Stage-2 granule (0=4KB, 1=64KB, 2=16KB); default 0
    uint8_t  s2sl0;   ///< Stage-2 starting level; default 1
    bool     s2aa64;  ///< AArch64 stage-2 tables; default true
    uint8_t  s2ps;    ///< Stage-2 physical address size (5=48-bit); default 5
    uint64_t s2ttb;   ///< Physical address of stage-2 root table; default 0

    StreamTableEntry()
        : stage1Enabled(false), stage2Enabled(false), translationEnabled(false),
          contextDescriptorTableBase(0), contextDescriptorTableSize(0),
          securityState(SecurityState::NonSecure),
          stage1Granule(TranslationGranule::Size4KB),
          stage2Granule(TranslationGranule::Size4KB),
          faultMode(FaultMode::Terminate),
          privilegedExecuteNever(false), instructionFetchDisable(false),
          streamID(0), vmid(0), asid(0), s1dss(2), s1cdMax(0),
          strw(StreamWorld::EL1_EL0),
          nsCfg(0), shCfg(0), allocCfg(0), memAttr(0), instCfg(0), privCfg(0), mtCfg(false),
          t0sz(16), t1sz(16), aa64(true),
          s2t0sz(16), s2tg(0), s2sl0(1), s2aa64(true), s2ps(5), s2ttb(0) {
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
          streamID(sid), vmid(0), asid(0), s1dss(2), s1cdMax(0),
          strw(StreamWorld::EL1_EL0),
          nsCfg(0), shCfg(0), allocCfg(0), memAttr(0), instCfg(0), privCfg(0), mtCfg(false),
          t0sz(16), t1sz(16), aa64(true),
          s2t0sz(16), s2tg(0), s2sl0(1), s2aa64(true), s2ps(5), s2ttb(0) {
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