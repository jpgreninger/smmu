# ARM SMMU v3 Architecture Diagrams

This document contains comprehensive Mermaid diagrams illustrating the architecture and data flows of the ARM SMMU v3 Rust implementation.

## Table of Contents

1. [Translation Flow](#1-translation-flow)
2. [Fault Handling Flow](#2-fault-handling-flow)
3. [Cache Architecture](#3-cache-architecture)
4. [Stream/PASID Hierarchy](#4-streampasid-hierarchy)

---

## 1. Translation Flow

The translation flow diagram shows the complete path from a translation request through stream lookup, PASID resolution, address space translation, cache lookups, and fault handling.

```mermaid
flowchart TD
    Start([Translation Request]) --> ValidateInput[Validate Input<br/>StreamID, PASID, IOVA, AccessType]
    ValidateInput --> |Valid| LookupStream[Lookup Stream Context<br/>in DashMap]
    ValidateInput --> |Invalid| ReturnError1[Return TranslationError]

    LookupStream --> |Stream Exists| CheckEnabled{Stream<br/>Translation<br/>Enabled?}
    LookupStream --> |Stream Not Found| ReturnError2[Return StreamNotFound]

    CheckEnabled --> |No| ReturnError3[Return TranslationDisabled]
    CheckEnabled --> |Yes| LookupPASID[Lookup PASID Context<br/>in Stream]

    LookupPASID --> |PASID Exists| CheckCache{TLB Cache<br/>Hit?}
    LookupPASID --> |PASID Not Found| CreateDefault{Create Default<br/>PASID 0?}

    CreateDefault --> |Yes| CheckCache
    CreateDefault --> |No| ReturnError4[Return PASIDNotFound]

    CheckCache --> |Hit| UpdateStats1[Update TLB Hit Stats<br/>Increment Counter]
    CheckCache --> |Miss| UpdateStats2[Update TLB Miss Stats<br/>Increment Counter]

    UpdateStats1 --> ValidateCached{Cached Entry<br/>Valid?}
    UpdateStats2 --> PageTableWalk[Page Table Walk<br/>in AddressSpace]

    ValidateCached --> |Valid| CheckPermissions1{Check<br/>Permissions}
    ValidateCached --> |Invalid| PageTableWalk

    PageTableWalk --> |Page Found| CheckPermissions2{Check<br/>Permissions<br/>for AccessType}
    PageTableWalk --> |Page Not Found| FaultPageNotMapped[Generate PageNotMapped Fault]

    CheckPermissions1 --> |Granted| ReturnSuccess1[Return TranslationResult<br/>with PA from Cache]
    CheckPermissions1 --> |Denied| FaultPermission1[Generate Permission<br/>Violation Fault]

    CheckPermissions2 --> |Granted| UpdateCache[Update TLB Cache<br/>Store Translation]
    CheckPermissions2 --> |Denied| FaultPermission2[Generate Permission<br/>Violation Fault]

    UpdateCache --> ReturnSuccess2[Return TranslationResult<br/>with PA, Attributes]

    FaultPageNotMapped --> RecordFault1[Record Fault<br/>to Fault Queue]
    FaultPermission1 --> RecordFault2[Record Fault<br/>to Fault Queue]
    FaultPermission2 --> RecordFault3[Record Fault<br/>to Fault Queue]

    RecordFault1 --> GenerateEvent1[Generate Event Entry<br/>if Enabled]
    RecordFault2 --> GenerateEvent2[Generate Event Entry<br/>if Enabled]
    RecordFault3 --> GenerateEvent3[Generate Event Entry<br/>if Enabled]

    GenerateEvent1 --> ReturnFault1[Return TranslationResult<br/>with Fault]
    GenerateEvent2 --> ReturnFault2[Return TranslationResult<br/>with Fault]
    GenerateEvent3 --> ReturnFault3[Return TranslationResult<br/>with Fault]

    ReturnSuccess1 --> End([Translation Complete])
    ReturnSuccess2 --> End
    ReturnError1 --> End
    ReturnError2 --> End
    ReturnError3 --> End
    ReturnError4 --> End
    ReturnFault1 --> End
    ReturnFault2 --> End
    ReturnFault3 --> End

    style Start fill:#e1f5e1
    style End fill:#e1f5e1
    style CheckCache fill:#fff4e6
    style CheckPermissions1 fill:#fff4e6
    style CheckPermissions2 fill:#fff4e6
    style FaultPageNotMapped fill:#ffe6e6
    style FaultPermission1 fill:#ffe6e6
    style FaultPermission2 fill:#ffe6e6
    style ReturnSuccess1 fill:#e6f3ff
    style ReturnSuccess2 fill:#e6f3ff
```

### Key Stages

1. **Input Validation**: Validate StreamID, PASID, IOVA, and AccessType
2. **Stream Lookup**: Find stream context in concurrent DashMap
3. **PASID Resolution**: Locate PASID context within stream
4. **Cache Check**: Query TLB for cached translation
5. **Page Table Walk**: Walk page tables if cache miss
6. **Permission Check**: Verify access permissions
7. **Fault Handling**: Generate and record faults if errors occur
8. **Cache Update**: Update TLB on successful translation
9. **Result Return**: Return physical address or fault information

---

## 2. Fault Handling Flow

The fault handling flow diagram illustrates fault detection, classification, recording, event generation, and recovery mechanisms.

```mermaid
flowchart TD
    Start([Fault Detected]) --> ClassifyFault{Classify<br/>Fault Type}

    ClassifyFault --> |Page Not Mapped| FaultType1[PageNotMapped]
    ClassifyFault --> |Permission Violation| FaultType2[PermissionViolation]
    ClassifyFault --> |Address Size| FaultType3[AddressSizeFault]
    ClassifyFault --> |Access Violation| FaultType4[AccessFlagFault]
    ClassifyFault --> |Translation Error| FaultType5[TranslationFault]
    ClassifyFault --> |TLB Conflict| FaultType6[TLBConflict]
    ClassifyFault --> |External Abort| FaultType7[ExternalAbort]
    ClassifyFault --> |Other| FaultType8[Other Fault Types]

    FaultType1 --> CreateRecord1[Create FaultRecord<br/>with Context]
    FaultType2 --> CreateRecord1
    FaultType3 --> CreateRecord1
    FaultType4 --> CreateRecord1
    FaultType5 --> CreateRecord1
    FaultType6 --> CreateRecord1
    FaultType7 --> CreateRecord1
    FaultType8 --> CreateRecord1

    CreateRecord1 --> PopulateContext[Populate Fault Context:<br/>- StreamID<br/>- PASID<br/>- IOVA<br/>- AccessType<br/>- Timestamp<br/>- Severity]

    PopulateContext --> CheckQueueSpace{Fault Queue<br/>Has Space?}

    CheckQueueSpace --> |Yes| AcquireLock[Acquire Fault Queue<br/>Mutex Lock]
    CheckQueueSpace --> |No| HandleOverflow[Handle Queue Overflow<br/>Drop Oldest or Error]

    HandleOverflow --> AcquireLock

    AcquireLock --> AppendQueue[Append Fault to<br/>Fault Queue]

    AppendQueue --> ReleaseLock[Release Mutex Lock]

    ReleaseLock --> CheckEventGen{Event<br/>Generation<br/>Enabled?}

    CheckEventGen --> |Yes| CreateEvent[Create EventEntry<br/>from FaultRecord]
    CheckEventGen --> |No| UpdateStats

    CreateEvent --> CheckEventFilter{Passes<br/>Event<br/>Filter?}

    CheckEventFilter --> |Yes| AcquireEventLock[Acquire Event Queue<br/>Mutex Lock]
    CheckEventFilter --> |No| UpdateStats

    AcquireEventLock --> AppendEvent[Append to Event Queue]

    AppendEvent --> ReleaseEventLock[Release Mutex Lock]

    ReleaseEventLock --> CheckInterrupt{Interrupt<br/>Enabled?}

    CheckInterrupt --> |Yes| RaiseInterrupt[Raise Interrupt to Host<br/>MSI/Wire/Signal]
    CheckInterrupt --> |No| UpdateStats

    RaiseInterrupt --> UpdateStats[Update Fault Statistics:<br/>- Total Faults<br/>- By Type<br/>- By Stream<br/>- By Severity]

    UpdateStats --> CheckRecovery{Fault<br/>Recoverable?}

    CheckRecovery --> |Yes| AttemptRecovery[Attempt Recovery:<br/>- Retry Translation<br/>- Use Alternate Path<br/>- Apply Workaround]
    CheckRecovery --> |No| LogFault[Log Non-Recoverable<br/>Fault]

    AttemptRecovery --> CheckSuccess{Recovery<br/>Successful?}

    CheckSuccess --> |Yes| ReturnSuccess[Return Success<br/>with Warning]
    CheckSuccess --> |No| LogFault

    LogFault --> ReturnError[Return Error<br/>to Caller]

    ReturnSuccess --> End([Fault Handling Complete])
    ReturnError --> End

    style Start fill:#ffe6e6
    style End fill:#e1f5e1
    style ClassifyFault fill:#fff4e6
    style CheckEventGen fill:#fff4e6
    style CheckRecovery fill:#fff4e6
    style ReturnSuccess fill:#e6f3ff
    style ReturnError fill:#ffe6e6
```

### Fault Types

The ARM SMMU v3 specification defines 15+ fault types:
- **Translation Faults**: Page not mapped, invalid translation
- **Permission Faults**: Read/write/execute violations
- **Access Faults**: Access flag not set, dirty bit issues
- **Address Size Faults**: IOVA/IPA/PA size violations
- **External Aborts**: Memory system errors
- **TLB Conflicts**: Cache coherency issues
- **Configuration Faults**: Invalid stream/PASID configuration

### Fault Recovery

- **Recoverable**: Can retry or use alternate path
- **Non-Recoverable**: Fatal errors requiring system intervention
- **Recovery Mechanisms**: Automatic retry, fallback paths, workarounds

---

## 3. Cache Architecture

The cache architecture diagram shows the TLB (Translation Lookaside Buffer) structure, lookup process, invalidation, and coherency mechanisms.

```mermaid
flowchart TD
    subgraph TLB["TLB (Translation Lookaside Buffer)"]
        direction TB
        TLBStructure[TLB Structure<br/>LRU HashMap<br/>Key: StreamID, PASID, IOVA Page<br/>Value: CachedTranslation]

        subgraph CacheEntry["Cache Entry Structure"]
            EntryFields[Fields:<br/>- Physical Address PA<br/>- Permissions Read/Write/Execute<br/>- Security State<br/>- Timestamp<br/>- Valid Bit<br/>- Dirty Bit]
        end

        TLBStructure -.-> CacheEntry
    end

    Start([Cache Operation]) --> OpType{Operation<br/>Type?}

    OpType --> |Lookup| LookupStart[Start TLB Lookup]
    OpType --> |Insert| InsertStart[Start Cache Insert]
    OpType --> |Invalidate| InvalidateStart[Start Invalidation]

    %% Lookup Path
    LookupStart --> ComputeKey[Compute Cache Key<br/>Hash StreamID, PASID, IOVA Page]
    ComputeKey --> CheckPresent{Entry<br/>Present?}

    CheckPresent --> |Yes| CheckValid{Entry<br/>Valid?}
    CheckPresent --> |No| CacheMiss[Cache Miss<br/>Return None]

    CheckValid --> |Yes| CheckAge{Entry<br/>Fresh?}
    CheckValid --> |No| CacheMiss

    CheckAge --> |Fresh| UpdateLRU1[Update LRU Position<br/>Mark Recently Used]
    CheckAge --> |Stale| EvictStale[Evict Stale Entry]

    UpdateLRU1 --> IncrementHits[Increment TLB Hit<br/>Counter]
    EvictStale --> CacheMiss

    IncrementHits --> ReturnCached[Return Cached<br/>Translation]
    CacheMiss --> IncrementMisses[Increment TLB Miss<br/>Counter]

    %% Insert Path
    InsertStart --> CheckCapacity{Cache<br/>At Capacity?}

    CheckCapacity --> |No| InsertEntry[Insert New Entry<br/>in HashMap]
    CheckCapacity --> |Yes| EvictLRU[Evict LRU Entry<br/>Make Space]

    EvictLRU --> InsertEntry

    InsertEntry --> UpdateLRU2[Update LRU List<br/>Mark Most Recent]
    UpdateLRU2 --> IncrementInserts[Increment Insert<br/>Counter]

    %% Invalidate Path
    InvalidateStart --> InvalidateType{Invalidation<br/>Scope?}

    InvalidateType --> |All| InvalidateAll[Invalidate All Entries<br/>Clear Entire Cache]
    InvalidateType --> |By Stream| InvalidateStream[Invalidate Stream Entries<br/>Remove Matching StreamID]
    InvalidateType --> |By PASID| InvalidatePASID[Invalidate PASID Entries<br/>Remove Matching PASID]
    InvalidateType --> |By Address| InvalidateAddr[Invalidate Address Range<br/>Remove Overlapping]
    InvalidateType --> |Single| InvalidateSingle[Invalidate Single Entry<br/>Remove Specific Key]

    InvalidateAll --> ClearCache[Clear HashMap<br/>Reset LRU]
    InvalidateStream --> FilterEntries1[Filter & Remove<br/>Matching Entries]
    InvalidatePASID --> FilterEntries2[Filter & Remove<br/>Matching Entries]
    InvalidateAddr --> FilterEntries3[Filter & Remove<br/>Overlapping Entries]
    InvalidateSingle --> RemoveEntry[Remove Single Entry<br/>from HashMap]

    ClearCache --> IncrementInv1[Increment Invalidation<br/>Counter]
    FilterEntries1 --> IncrementInv2[Increment Invalidation<br/>Counter]
    FilterEntries2 --> IncrementInv3[Increment Invalidation<br/>Counter]
    FilterEntries3 --> IncrementInv4[Increment Invalidation<br/>Counter]
    RemoveEntry --> IncrementInv5[Increment Invalidation<br/>Counter]

    ReturnCached --> End([Operation Complete])
    IncrementMisses --> End
    IncrementInserts --> End
    IncrementInv1 --> End
    IncrementInv2 --> End
    IncrementInv3 --> End
    IncrementInv4 --> End
    IncrementInv5 --> End

    style TLB fill:#e6f3ff
    style CacheEntry fill:#f0f8ff
    style ReturnCached fill:#e1f5e1
    style CacheMiss fill:#fff4e6
    style InvalidateAll fill:#ffe6e6
```

### Cache Features

1. **LRU Eviction**: Least Recently Used entries evicted when cache is full
2. **Hash-Based Lookup**: O(1) average lookup time
3. **Configurable Size**: Tunable cache size (default: 1024 entries)
4. **Fine-Grained Invalidation**: By stream, PASID, address range, or all
5. **Statistics Tracking**: Hits, misses, evictions, invalidations
6. **Coherency**: Automatic invalidation on page table updates

### Performance Characteristics

- **Hit Latency**: ~50-100ns (hash lookup + validation)
- **Miss Latency**: ~500-1000ns (page table walk)
- **Hit Rate**: >95% for typical workloads
- **Invalidation**: O(n) for filtered, O(1) for single

---

## 4. Stream/PASID Hierarchy

The hierarchy diagram illustrates the relationship between SMMU, streams, PASIDs, and address spaces, showing ownership and lifecycle management.

```mermaid
flowchart TD
    subgraph SMMU_Level["SMMU Controller (Top Level)"]
        direction TB
        SMMU[SMMU Instance<br/>Global Configuration<br/>Event/Command/PRI Queues<br/>Statistics & Monitoring]

        StreamTable[Stream Table<br/>DashMap StreamID → Arc RwLock StreamContext<br/>Lock-Free Concurrent Access<br/>Supports 4+ Billion Streams]

        SMMU --> StreamTable

        subgraph GlobalResources["Global Resources"]
            EventQ[Event Queue<br/>Mutex VecDeque EventEntry]
            CmdQ[Command Queue<br/>Mutex VecDeque CommandEntry]
            PRIQ[PRI Queue<br/>Mutex VecDeque PRIEntry]
            FaultQ[Fault Queue<br/>Mutex Vec FaultRecord]
        end

        SMMU --> GlobalResources
    end

    subgraph Stream_Level["Stream Context Level (Per Device)"]
        direction TB
        SC1[StreamContext 1<br/>StreamID: 0x0001<br/>Device A]
        SC2[StreamContext 2<br/>StreamID: 0x0002<br/>Device B]
        SC3[StreamContext N<br/>StreamID: 0xFFFF<br/>Device N]

        StreamTable --> SC1
        StreamTable --> SC2
        StreamTable --> SC3

        subgraph StreamConfig1["Stream Configuration"]
            Config1[Stream Config:<br/>- Translation Enabled<br/>- Stage 1/2 Mode<br/>- Security State<br/>- PASID Enabled<br/>- Max PASIDs]
        end

        SC1 --> Config1

        subgraph PASIDTable1["PASID Table (Per Stream)"]
            direction TB
            PT1[PASID Table<br/>HashMap PASID → Shared AddressSpace<br/>Up to 1048576 PASIDs<br/>PASID 0 Always Present]

            PASID0[PASID 0<br/>Default Context<br/>Always Active]
            PASID1[PASID 1<br/>Process 1]
            PASID2[PASID 2<br/>Process 2]
            PASIDN[PASID N<br/>Process N]

            PT1 --> PASID0
            PT1 --> PASID1
            PT1 --> PASID2
            PT1 --> PASIDN
        end

        SC1 --> PASIDTable1
    end

    subgraph AddressSpace_Level["Address Space Level (Per PASID)"]
        direction TB
        AS0[AddressSpace 0<br/>PASID 0 Default<br/>Shared by Multiple Streams]
        AS1[AddressSpace 1<br/>PASID 1 Process 1<br/>Isolated Private Space]
        AS2[AddressSpace 2<br/>PASID 2 Process 2<br/>Isolated Private Space]

        PASID0 --> AS0
        PASID1 --> AS1
        PASID2 --> AS2

        subgraph PageTable["Page Table Structure"]
            direction TB
            PTStruct[Page Table:<br/>- HashMap IOVA Page → PageEntry<br/>- Sparse Representation<br/>- O log n Lookup<br/>- Lazy Allocation]

            subgraph PageEntry["Page Entry"]
                PEFields[Page Entry Fields:<br/>- Physical Address PA<br/>- Permissions RWX<br/>- Security State<br/>- Attributes<br/>- Valid/Dirty Bits]
            end

            PTStruct --> PageEntry
        end

        AS1 --> PageTable

        subgraph ASStats["Address Space Statistics"]
            Stats[Statistics:<br/>- Page Count<br/>- Translation Count<br/>- Fault Count<br/>- Cache Hits/Misses]
        end

        AS1 --> ASStats
    end

    subgraph TwoStage["Two-Stage Translation (Optional)"]
        direction TB
        Stage1[Stage 1: VA → IPA<br/>Guest Virtual Address<br/>to Intermediate PA]
        Stage2[Stage 2: IPA → PA<br/>Intermediate PA<br/>to Physical Address]

        Stage1 --> Stage2

        AS2 -.->|Stage 1| Stage1
        Stage2 -.->|Stage 2| AS0
    end

    subgraph Sharing["Address Space Sharing"]
        direction TB
        Shared[Shared Address Space<br/>Multiple PASIDs → Same AS<br/>Arc Shared Ptr Reference Counted]

        PASID0 -.->|Shares| Shared
        SC2 -.->|Can Share| Shared
        SC3 -.->|Can Share| Shared
    end

    style SMMU fill:#e6f3ff
    style StreamTable fill:#f0f8ff
    style SC1 fill:#e1f5e1
    style PT1 fill:#f5f5dc
    style AS1 fill:#fff4e6
    style PageTable fill:#ffe6f0
    style Shared fill:#f0e6ff
```

### Hierarchy Levels

#### Level 1: SMMU Controller (Global)
- **Singleton**: One SMMU instance per system/simulation
- **Responsibilities**:
  - Stream table management
  - Global event/command/PRI queues
  - Statistics and monitoring
  - Configuration management
- **Concurrency**: DashMap for lock-free stream access

#### Level 2: Stream Context (Per Device)
- **Identity**: Unique StreamID (0 to 2^32-1)
- **Represents**: Physical device or virtual function
- **Contains**:
  - PASID table (up to 1,048,576 PASIDs)
  - Stream configuration
  - Per-stream statistics
- **Lifecycle**: Created on first use, persists until explicit deletion
- **Concurrency**: Arc<RwLock<>> for shared access

#### Level 3: PASID Context (Per Process)
- **Identity**: PASID (0 to 1,048,575)
- **Represents**: Process address space or context
- **PASID 0**: Always present, default context
- **Contains**: Reference to AddressSpace (can be shared)
- **Lifecycle**: Created on demand, reference counted
- **Sharing**: Multiple PASIDs can share one AddressSpace

#### Level 4: Address Space (Per Context)
- **Identity**: Unique per process or shared
- **Contains**:
  - Page table (IOVA → PA mappings)
  - Security state
  - Permission settings
  - Statistics
- **Structure**: Sparse hash map for memory efficiency
- **Concurrency**: Interior mutability via RwLock
- **Sharing**: Arc<> for reference counting

### Key Relationships

1. **One-to-Many**: SMMU → Streams → PASIDs → Pages
2. **Many-to-One**: PASIDs can share AddressSpace (Arc reference counting)
3. **Independent**: Streams are independent, no cross-stream dependencies
4. **Hierarchical**: Cannot have PASID without Stream, cannot have Page without AddressSpace

### Memory Management

- **Smart Pointers**: Arc for sharing, RwLock for mutability
- **Reference Counting**: Automatic cleanup when last reference dropped
- **Lazy Allocation**: PASIDs and pages allocated on first use
- **Sparse Structures**: Only allocated pages consume memory

---

## Diagram Usage in Documentation

These diagrams are embedded in the following documentation files:

1. **DESIGN.md**: Architecture overview and detailed design
2. **README.md**: Quick reference for users
3. **GUIDE.md**: Tutorial and user guide with flow explanations
4. **ARCHITECTURE_DIAGRAMS.md**: This file (canonical source)

## Rendering

These diagrams use Mermaid syntax and can be rendered by:

- **GitHub**: Automatic rendering in Markdown files
- **docs.rs**: Rendered in published documentation
- **IDEs**: Markdown preview with Mermaid support (VS Code, IntelliJ)
- **CLI**: `mermaid-cli` for SVG/PNG export

## Updating Diagrams

When updating architecture:

1. Update diagrams in this file (canonical source)
2. Copy updated diagrams to DESIGN.md
3. Update related sections in GUIDE.md if flows change
4. Regenerate documentation: `cargo doc`

---

**Version**: 1.0
**Date**: February 8, 2026
**Status**: Complete - All four diagrams implemented
