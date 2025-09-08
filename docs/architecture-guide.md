# ARM SMMU v3 Architecture Guide

This document provides a comprehensive overview of the ARM SMMU v3 implementation architecture, covering system design, component interactions, data flow, and design patterns.

## Table of Contents

1. [System Overview](#system-overview)
2. [Core Architecture](#core-architecture)
3. [Component Hierarchy](#component-hierarchy)
4. [Data Flow](#data-flow)
5. [Design Patterns](#design-patterns)
6. [Memory Management](#memory-management)
7. [Thread Safety](#thread-safety)
8. [Performance Optimizations](#performance-optimizations)
9. [Error Handling Strategy](#error-handling-strategy)
10. [Extensibility Points](#extensibility-points)

---

## System Overview

The ARM SMMU v3 implementation is designed as a comprehensive software model that faithfully represents the hardware SMMU behavior while providing a clean, efficient C++11 interface for development and testing environments.

### Key Architectural Principles

- **Specification Compliance**: Strict adherence to ARM SMMU v3 architecture specification
- **Performance Efficiency**: Optimized for minimal overhead and high throughput
- **Memory Efficiency**: Sparse data structures to handle large address spaces
- **Thread Safety**: Complete thread-safe operation for multi-threaded environments
- **C++11 Purity**: No external dependencies beyond C++11 standard library
- **Testability**: Designed for comprehensive unit and integration testing

### System Boundaries

```
┌─────────────────────────────────────────────────────────────┐
│                    Client Applications                       │
├─────────────────────────────────────────────────────────────┤
│                     SMMU Public API                         │
├─────────────────────────────────────────────────────────────┤
│  Stream Management  │  Translation Engine  │  Cache System  │
├─────────────────────┼─────────────────────┼─────────────────┤
│   Address Spaces    │    Fault Handling   │   Event Queue   │
├─────────────────────────────────────────────────────────────┤
│                    Core Types & Utilities                   │
└─────────────────────────────────────────────────────────────┘
```

---

## Core Architecture

### Layered Architecture

The system is organized in distinct layers, each with specific responsibilities:

#### Layer 1: Core Types Foundation
- **Location**: `src/types/`, `include/smmu/types.h`
- **Purpose**: Fundamental data types, enums, error codes, and result system
- **Components**: 
  - Type definitions (StreamID, PASID, IOVA, PA)
  - Error enumeration and result handling
  - Core data structures (PageEntry, FaultRecord)
  - ARM SMMU v3 specification types

#### Layer 2: Address Space Management  
- **Location**: `src/address_space/`, `include/smmu/address_space.h`
- **Purpose**: Virtual-to-physical address translation and page table management
- **Key Features**:
  - Sparse page table representation using `std::unordered_map`
  - Efficient bulk operations for contiguous memory regions
  - Permission validation and access control
  - Cache invalidation support

#### Layer 3: Stream Context Management
- **Location**: `src/stream_context/`, `include/smmu/stream_context.h`  
- **Purpose**: Per-stream state management and PASID address space coordination
- **Key Features**:
  - PASID lifecycle management
  - Two-stage translation coordination
  - Per-stream configuration and statistics
  - Thread-safe state transitions

#### Layer 4: SMMU Controller
- **Location**: `src/smmu/`, `include/smmu/smmu.h`
- **Purpose**: Main orchestration and public API implementation
- **Key Features**:
  - High-level translation API
  - Stream lifecycle management
  - Global fault handling coordination
  - Performance statistics and monitoring

#### Layer 5: Support Systems
- **Fault Handling**: `src/fault/`, `include/smmu/fault_handler.h`
- **TLB Caching**: `src/cache/`, `include/smmu/tlb_cache.h`
- **Configuration**: `src/configuration/`, `include/smmu/configuration.h`

### Component Interaction Model

```mermaid
graph TD
    A[Client Application] -->|translate()| B[SMMU Controller]
    B -->|lookup stream| C[Stream Context Manager]
    C -->|get PASID space| D[AddressSpace]
    D -->|translate page| E[Page Table]
    
    B -->|cache lookup| F[TLB Cache]
    F -->|miss| D
    F -->|hit| G[Return Result]
    
    D -->|fault| H[Fault Handler]
    H -->|record| I[Event Queue]
    
    B -->|statistics| J[Performance Monitor]
    
    subgraph "Core Data Layer"
        E
        K[Configuration Store]
        I
    end
    
    subgraph "Management Layer"  
        C
        H
        J
    end
    
    subgraph "Optimization Layer"
        F
    end
```

---

## Component Hierarchy

### SMMU Controller (Central Orchestrator)

```cpp
class SMMU {
private:
    // Core components
    std::unordered_map<StreamID, std::unique_ptr<StreamContext>> streamContexts;
    std::unique_ptr<FaultHandler> faultHandler;
    std::unique_ptr<TLBCache> tlbCache;
    
    // System configuration
    Configuration configuration;
    FaultMode globalFaultMode;
    bool cachingEnabled;
    
    // Event and command queues
    std::queue<EventEntry> eventQueue;
    std::queue<CommandEntry> commandQueue;
    std::queue<PRIEntry> priQueue;
    
    // Thread safety
    mutable std::mutex sMMUMutex;
    
    // Performance counters
    std::atomic<uint64_t> translationCount;
    std::atomic<uint64_t> cacheHits;
    std::atomic<uint64_t> cacheMisses;
};
```

**Responsibilities**:
- Public API implementation and validation
- Stream lifecycle management and coordination
- Global policy enforcement (fault modes, caching)
- Performance monitoring and statistics collection
- Event queue management and processing
- Thread synchronization across all components

### StreamContext (Per-Stream State Management)

```cpp
class StreamContext {
private:
    // PASID address spaces
    std::unordered_map<PASID, std::shared_ptr<AddressSpace>> pasidMap;
    
    // Translation configuration
    std::shared_ptr<AddressSpace> stage2AddressSpace;
    bool stage1Enabled;
    bool stage2Enabled;
    FaultMode faultMode;
    
    // Stream state
    StreamConfig currentConfiguration;
    StreamStatistics streamStatistics;
    bool streamEnabled;
    bool configurationChanged;
    
    // Fault handling
    std::shared_ptr<FaultHandler> faultHandler;
    
    // Thread safety
    mutable std::mutex contextMutex;
};
```

**Responsibilities**:
- PASID address space lifecycle management
- Two-stage translation coordination
- Per-stream configuration validation and application
- Stream-specific statistics collection
- Local fault handling and escalation

### AddressSpace (Translation Engine)

```cpp
class AddressSpace {
private:
    // Sparse page table - key optimization for large address spaces
    std::unordered_map<uint64_t, PageEntry> pageTable;
    
    // Helper methods
    uint64_t pageNumber(IOVA iova) const;
    VoidResult checkPermissions(const PageEntry& entry, AccessType access) const;
};
```

**Responsibilities**:
- Virtual-to-physical address translation
- Page table management with sparse representation
- Permission validation and access control
- Bulk operations for performance optimization
- Cache invalidation coordination

---

## Data Flow

### Primary Translation Flow

The core translation operation follows this sequence:

```
1. Client Request
   translate(streamID, pasid, iova, accessType)
   ↓

2. SMMU Controller Validation
   - Validate parameters
   - Check stream configuration
   - Verify stream is enabled
   ↓

3. Cache Lookup (if enabled)
   TLBCache::lookup(streamID, pasid, iova)
   ↓ (cache miss)

4. Stream Context Resolution
   streamContexts[streamID]->translate(pasid, iova, accessType)
   ↓

5. PASID Address Space Selection
   pasidMap[pasid]->translatePage(iova, accessType)
   ↓

6. Page Table Translation
   - Calculate page number: iova >> PAGE_SHIFT
   - Lookup in sparse pageTable map
   - Validate permissions against access type
   ↓

7. Two-Stage Translation (if enabled)
   - Stage 1: PASID address space translation
   - Stage 2: stage2AddressSpace translation
   ↓

8. Result Processing
   - Cache successful translation (if enabled)
   - Update performance counters
   - Return TranslationResult
```

### Fault Handling Flow

When translation fails, the system follows this fault handling sequence:

```
1. Translation Failure Detection
   - Page not mapped
   - Permission violation
   - Address size mismatch
   ↓

2. Fault Classification
   FaultHandler::classifyFault(iova, accessType, stage)
   ↓

3. Syndrome Generation
   FaultHandler::generateFaultSyndrome(faultType, context)
   ↓

4. Fault Mode Processing
   if (faultMode == Terminate):
       Record fault event and return error
   else if (faultMode == Stall):
       Queue PRI request and stall transaction
   ↓

5. Event Queue Management
   eventQueue.push(FaultRecord)
   Notify client of pending events
```

### Configuration Update Flow

Configuration changes propagate through the system as follows:

```
1. Configuration Request
   SMMU::configureStream(streamID, config)
   ↓

2. Validation
   StreamContext::validateConfiguration(config)
   ↓

3. Cache Invalidation  
   TLBCache::invalidateStream(streamID)
   ↓

4. State Update
   StreamContext::applyConfiguration(config)
   ↓

5. Notification
   Mark configuration as changed
   Update statistics
```

---

## Design Patterns

### Factory Pattern - Address Space Creation

```cpp
class StreamContext {
    VoidResult createPASID(PASID pasid) {
        // Factory creates new AddressSpace instances
        auto addressSpace = std::make_shared<AddressSpace>();
        pasidMap[pasid] = addressSpace;
        return makeVoidSuccess();
    }
};
```

### RAII Pattern - Resource Management

```cpp
class SMMU {
    // Automatic cleanup through smart pointers
    std::unordered_map<StreamID, std::unique_ptr<StreamContext>> streamContexts;
    std::unique_ptr<FaultHandler> faultHandler;
    std::unique_ptr<TLBCache> tlbCache;
};
```

### Template Specialization Pattern - Result System

```cpp
template<typename T>
class Result {
    // Generic result handling
};

// Specialized for void operations  
using VoidResult = Result<Unit>;

// Specialized for translations
using TranslationResult = Result<TranslationData>;
```

### Observer Pattern - Event System

```cpp
class SMMU {
    std::queue<EventEntry> eventQueue;
    
    // Observers can register for event notifications
    void processEventQueue() {
        while (!eventQueue.empty()) {
            // Notify registered observers
            EventEntry event = eventQueue.front();
            eventQueue.pop();
            // Process event...
        }
    }
};
```

### Strategy Pattern - Fault Handling

```cpp
enum class FaultMode {
    Terminate,  // Strategy: immediate termination
    Stall       // Strategy: stall and recover
};

class FaultHandler {
    void handleFault(FaultRecord fault, FaultMode mode) {
        switch (mode) {
            case FaultMode::Terminate:
                executeTerminateStrategy(fault);
                break;
            case FaultMode::Stall:
                executeStallStrategy(fault);
                break;
        }
    }
};
```

---

## Memory Management

### Sparse Data Structures

The implementation uses sparse data structures to efficiently handle large address spaces:

```cpp
class AddressSpace {
private:
    // Instead of dense arrays, use sparse hash maps
    std::unordered_map<uint64_t, PageEntry> pageTable;
    
    // Page number calculation for sparse indexing
    uint64_t pageNumber(IOVA iova) const {
        return iova >> 12;  // 4KB pages
    }
};
```

**Benefits**:
- **Memory Efficiency**: Only allocated pages consume memory
- **Scalability**: Handles 48/52-bit address spaces efficiently  
- **Performance**: O(1) average lookup time
- **Flexibility**: Easy to add/remove mappings

### Smart Pointer Strategy

```cpp
// Shared ownership for address spaces that may be shared between PASIDs
std::unordered_map<PASID, std::shared_ptr<AddressSpace>> pasidMap;

// Unique ownership for per-stream contexts
std::unordered_map<StreamID, std::unique_ptr<StreamContext>> streamContexts;
```

### Move Semantics

```cpp
template<typename T>
class Result {
public:
    Result(T&& value) : isSuccess(true), value(std::move(value)) {}
    
    T&& moveValue() {
        return std::move(value);
    }
};
```

---

## Thread Safety

### Synchronization Strategy

The implementation provides complete thread safety through a hierarchical locking strategy:

#### Global Level (SMMU Controller)
```cpp
class SMMU {
private:
    mutable std::mutex sMMUMutex;  // Protects global state
    
public:
    TranslationResult translate(StreamID streamID, PASID pasid, 
                               IOVA iova, AccessType access) {
        std::lock_guard<std::mutex> lock(sMMUMutex);
        // ... translation logic
    }
};
```

#### Stream Level (StreamContext)
```cpp
class StreamContext {
private:
    mutable std::mutex contextMutex;  // Protects per-stream state
    
public:
    TranslationResult translate(PASID pasid, IOVA iova, AccessType access) {
        std::lock_guard<std::mutex> lock(contextMutex);
        // ... context-specific translation
    }
};
```

#### Performance Counters
```cpp
class SMMU {
private:
    // Lock-free atomic counters for high-frequency updates
    std::atomic<uint64_t> translationCount;
    std::atomic<uint64_t> cacheHits;
    std::atomic<uint64_t> cacheMisses;
};
```

### Lock Ordering

To prevent deadlocks, locks are acquired in consistent order:
1. SMMU global mutex (if needed)
2. StreamContext mutex (if needed)
3. AddressSpace operations (inherently thread-safe with sparse maps)

---

## Performance Optimizations

### TLB Caching System

```cpp
class TLBCache {
private:
    struct TLBEntry {
        StreamID streamID;
        PASID pasid;
        IOVA iova;
        PA physicalAddress;
        PagePermissions permissions;
        SecurityState securityState;
        bool valid;
        uint64_t timestamp;  // For LRU eviction
    };
    
    std::unordered_map<uint64_t, TLBEntry> cache;
    size_t maxSize;
    
    uint64_t generateCacheKey(StreamID streamID, PASID pasid, IOVA iova) const;
};
```

**Cache Strategies**:
- **Key Generation**: Combines StreamID, PASID, and page number
- **Eviction Policy**: LRU (Least Recently Used)
- **Invalidation**: Selective by stream, PASID, or address range
- **Coherency**: Automatic invalidation on configuration changes

### Bulk Operations

```cpp
class AddressSpace {
public:
    // Optimize contiguous mappings
    VoidResult mapRange(IOVA startIova, PA startPhysical, size_t size,
                       PagePermissions permissions) {
        size_t pageCount = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        
        // Reserve space to avoid rehashing
        pageTable.reserve(pageTable.size() + pageCount);
        
        for (size_t i = 0; i < pageCount; ++i) {
            uint64_t pageNum = pageNumber(startIova + i * PAGE_SIZE);
            pageTable[pageNum] = PageEntry(startPhysical + i * PAGE_SIZE, 
                                         permissions);
        }
        return makeVoidSuccess();
    }
};
```

### Memory Pool Optimization

For high-frequency allocations, the system could employ memory pools:

```cpp
template<typename T>
class ObjectPool {
private:
    std::stack<std::unique_ptr<T>> available;
    std::mutex poolMutex;
    
public:
    std::unique_ptr<T> acquire() {
        std::lock_guard<std::mutex> lock(poolMutex);
        if (available.empty()) {
            return std::make_unique<T>();
        } else {
            auto obj = std::move(const_cast<std::unique_ptr<T>&>(available.top()));
            available.pop();
            return obj;
        }
    }
};
```

---

## Error Handling Strategy

### Result-Based Error Handling

The implementation avoids exceptions in favor of explicit error handling:

```cpp
// Instead of throwing exceptions
TranslationResult translate(...) {
    if (validation_fails) {
        return makeTranslationError(SMMUError::InvalidAddress);
    }
    
    // Normal processing
    return makeTranslationSuccess(physicalAddress, permissions);
}
```

### Error Propagation

```cpp
VoidResult SMMU::mapPage(...) {
    // Validate parameters
    if (!isValidAddress(iova)) {
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Delegate to stream context
    auto result = getStreamContext(streamID);
    if (!result) {
        return makeVoidError(result.getError());
    }
    
    return result.getValue()->mapPage(pasid, iova, physicalAddress, permissions);
}
```

### Fault Recovery

```cpp
class FaultHandler {
    VoidResult handleTranslationFault(const FaultRecord& fault) {
        switch (fault.faultType) {
            case FaultType::TranslationFault:
                return handlePageFault(fault);
            case FaultType::PermissionFault:
                return handlePermissionViolation(fault);
            default:
                return makeVoidError(SMMUError::UnknownFaultType);
        }
    }
};
```

---

## Extensibility Points

### Custom Fault Handlers

```cpp
class CustomFaultHandler : public FaultHandler {
public:
    VoidResult handleFault(const FaultRecord& fault) override {
        // Custom fault handling logic
        if (isCustomFaultType(fault)) {
            return handleCustomFault(fault);
        }
        return FaultHandler::handleFault(fault);  // Delegate to base
    }
};
```

### Pluggable Cache Policies

```cpp
class CachePolicy {
public:
    virtual ~CachePolicy() = default;
    virtual bool shouldCache(const TranslationData& data) const = 0;
    virtual TLBEntry* selectEvictionCandidate() = 0;
};

class TLBCache {
private:
    std::unique_ptr<CachePolicy> policy;
    
public:
    void setCachePolicy(std::unique_ptr<CachePolicy> newPolicy) {
        policy = std::move(newPolicy);
    }
};
```

### Custom Address Space Implementations

```cpp
class HardwareAddressSpace : public AddressSpace {
public:
    TranslationResult translatePage(IOVA iova, AccessType access) override {
        // Hardware-specific translation logic
        return performHardwareTranslation(iova, access);
    }
};
```

### Performance Monitoring Extensions

```cpp
class PerformanceMonitor {
public:
    virtual ~PerformanceMonitor() = default;
    virtual void recordTranslation(StreamID streamID, uint64_t latency) = 0;
    virtual void recordCacheHit(StreamID streamID) = 0;
    virtual void recordFault(const FaultRecord& fault) = 0;
};
```

This architecture provides a solid foundation for ARM SMMU v3 implementation while maintaining flexibility for future enhancements and customizations.