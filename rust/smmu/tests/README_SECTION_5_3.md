# ARM SMMU v3 Section 5.3 Test Suite

## Test Plan Summary: Event and Command Processing

This document describes the comprehensive test suite for Section 5.3 of the ARM SMMU v3 implementation, covering event queue, command queue, and PRI (Page Request Interface) queue management.

## Test Organization

### 1. Event Queue Tests (5.3.1)
**Coverage Areas:**
- Queue initialization and configuration
- Event submission (all event types)
- FIFO ordering guarantees
- Overflow handling and queue full detection
- Event filtering (by type, StreamID, PASID)
- Concurrent event submission (thread safety)
- Queue clearing and reset operations

**Test Count:** 9 tests
**Estimated Time:** 5 hours development

**Key Test Cases:**
- `test_section_5_3_1_event_queue_initialization` - Verify default 512+ entry capacity
- `test_section_5_3_1_submit_translation_fault_event` - Translation fault event handling
- `test_section_5_3_1_submit_permission_fault_event` - Permission fault event handling
- `test_section_5_3_1_event_queue_fifo_ordering` - FIFO ordering compliance
- `test_section_5_3_1_event_queue_overflow` - Graceful overflow handling
- `test_section_5_3_1_event_queue_clear` - Atomic queue clearing
- `test_section_5_3_1_concurrent_event_submission` - Thread-safe 10-thread concurrent access
- `test_section_5_3_1_event_filtering_by_type` - Filter by EventType
- `test_section_5_3_1_event_filtering_by_stream` - Filter by StreamID

**ARM SMMU v3 Compliance:**
- Event queue structure (Section 6.3 of ARM spec)
- Event record format and fields
- Queue size requirements (minimum 512 entries)
- Overflow behavior specification

### 2. Command Queue Tests (5.3.2)
**Coverage Areas:**
- Queue initialization and configuration
- Command submission (all 11 CommandType variants)
- FIFO command processing
- Command validation and rejection
- Queue overflow and full detection
- Concurrent command submission
- Command completion events
- Queue clearing operations

**Test Count:** 8 tests
**Estimated Time:** 5 hours development

**Key Test Cases:**
- `test_section_5_3_2_command_queue_initialization` - Verify default 256+ entry capacity
- `test_section_5_3_2_submit_sync_command` - SYNC command processing
- `test_section_5_3_2_submit_all_command_types` - All 11 command types
- `test_section_5_3_2_command_processing_fifo` - FIFO processing order
- `test_section_5_3_2_command_queue_overflow` - Queue full detection
- `test_section_5_3_2_command_validation` - Invalid command rejection
- `test_section_5_3_2_command_queue_clear` - Queue clearing
- `test_section_5_3_2_concurrent_command_submission` - 8-thread concurrent access

**Command Types Tested:**
1. `PrefetchConfig` - Configuration prefetch
2. `PrefetchAddr` - Address prefetch
3. `CfgiSte` - Stream Table Entry invalidation
4. `CfgiAll` - All configuration invalidation
5. `TlbiNhAll` - TLB invalidation non-secure hyp all
6. `TlbiEl2All` - TLB invalidation EL2 all
7. `TlbiS12Vmall` - TLB invalidation stage 1&2 VM all
8. `AtcInv` - Address Translation Cache invalidation
9. `PriResp` - Page Request Interface response
10. `Resume` - Resume processing
11. `Sync` - Synchronization barrier

**ARM SMMU v3 Compliance:**
- Command queue structure (Section 6.4 of ARM spec)
- Command format and encoding
- Queue size requirements (minimum 256 entries)
- Command processing order guarantees

### 3. PRI Queue Tests (5.3.3)
**Coverage Areas:**
- Queue initialization and configuration
- Page request submission
- Priority ordering for processing
- Last request flag handling
- Queue overflow handling
- Concurrent PRI submission
- Request lifecycle management
- Queue clearing operations

**Test Count:** 7 tests
**Estimated Time:** 4 hours development

**Key Test Cases:**
- `test_section_5_3_3_pri_queue_initialization` - Verify default 128+ entry capacity
- `test_section_5_3_3_submit_page_request` - Basic page request submission
- `test_section_5_3_3_pri_processing` - Priority-based processing
- `test_section_5_3_3_pri_queue_overflow` - Overflow handling
- `test_section_5_3_3_last_request_flag` - Request group completion
- `test_section_5_3_3_concurrent_pri_submission` - 5-thread concurrent access
- `test_section_5_3_3_pri_queue_clear` - Queue clearing

**ARM SMMU v3 Compliance:**
- PRI queue structure (Section 7 of ARM spec)
- Page request format and fields
- Queue size requirements (minimum 128 entries)
- Priority ordering requirements

### 4. Cache Invalidation Command Tests (5.3.4)
**Coverage Areas:**
- TLBI_NH_ALL command processing
- TLBI_EL2_ALL command processing
- TLBI_S12_VMALL command processing
- ATC_INV command processing
- Selective invalidation (StreamID/PASID targeting)
- Address range invalidation
- Batch invalidation operations
- Invalidation completion events

**Test Count:** 8 tests
**Estimated Time:** 5 hours development

**Key Test Cases:**
- `test_section_5_3_4_tlbi_nh_all_command` - Non-secure hyp TLB invalidation
- `test_section_5_3_4_tlbi_el2_all_command` - EL2 TLB invalidation
- `test_section_5_3_4_tlbi_s12_vmall_command` - Stage 1&2 VM TLB invalidation
- `test_section_5_3_4_atc_inv_command` - ATC invalidation with completion event
- `test_section_5_3_4_selective_invalidation` - Target specific stream/PASID
- `test_section_5_3_4_address_range_invalidation` - Invalidate address range
- `test_section_5_3_4_batch_invalidation` - Batch multiple invalidations
- `test_section_5_3_4_invalidation_completion_events` - Verify completion events

**ARM SMMU v3 Compliance:**
- Cache invalidation commands (Section 9 of ARM spec)
- Invalidation scope and targeting
- Completion event generation
- Cache coherency requirements

### 5. Queue Integration Tests (5.3.5)
**Coverage Areas:**
- Multiple queues operating concurrently
- Event generation from command processing
- Queue overflow isolation (no cascading failures)
- Queue statistics and monitoring
- Atomic queue reset operations
- Memory efficiency under sustained load

**Test Count:** 7 tests
**Estimated Time:** 4 hours development

**Key Test Cases:**
- `test_section_5_3_5_concurrent_queue_operations` - 3 queues, 50 entries each, concurrent
- `test_section_5_3_5_command_generates_events` - SYNC command generates completion event
- `test_section_5_3_5_queue_overflow_isolation` - Overflow in one queue doesn't affect others
- `test_section_5_3_5_queue_statistics` - Queue usage statistics API
- `test_section_5_3_5_queue_reset` - Atomic reset of all queues
- `test_section_5_3_5_memory_efficiency` - 1000 entries, 10 iterations, no leaks

**Integration Points:**
- Event queue ↔ Command queue (commands generate events)
- Command queue ↔ Cache system (invalidation commands)
- PRI queue ↔ Event queue (page request events)
- All queues ↔ Statistics system

### 6. Queue Performance Tests (5.3.6)
**Coverage Areas:**
- High-throughput event submission
- High-throughput command submission
- Low-latency queue operations
- Concurrent access scalability

**Test Count:** 3 tests
**Estimated Time:** 3 hours development

**Key Test Cases:**
- `test_section_5_3_6_event_queue_performance` - >100k events/sec target
- `test_section_5_3_6_command_queue_performance` - >50k commands/sec target
- `test_section_5_3_6_queue_latency` - <1μs submission latency target

**Performance Targets:**
- Event queue: 100,000+ submissions/second
- Command queue: 50,000+ submissions/second
- Submission latency: <1 microsecond average
- Concurrent scalability: Linear up to 10 threads

## Test Execution Strategy

### TDD Approach
All tests are written **before** implementation to drive design:

1. **Phase 1: Type Definitions** (2 hours)
   - Define `EventEntry`, `CommandEntry`, `PRIEntry` types
   - Define `EventType`, `CommandType` enums
   - Define `QueueConfig` configuration structure

2. **Phase 2: Event Queue Implementation** (8 hours)
   - Implement `VecDeque<EventEntry>` with `RwLock` for thread safety
   - Implement bounded queue with overflow handling
   - Implement event filtering and querying APIs
   - Run tests 5.3.1 (should pass)

3. **Phase 3: Command Queue Implementation** (10 hours)
   - Implement `VecDeque<CommandEntry>` with `RwLock`
   - Implement command validation and processing
   - Implement command completion event generation
   - Run tests 5.3.2 (should pass)

4. **Phase 4: PRI Queue Implementation** (8 hours)
   - Implement `VecDeque<PRIEntry>` or `BinaryHeap<PRIEntry>` for priority
   - Implement request lifecycle management
   - Implement concurrent access with `RwLock`
   - Run tests 5.3.3 (should pass)

5. **Phase 5: Cache Invalidation Commands** (8 hours)
   - Implement invalidation command handlers
   - Integrate with TLB cache system (Section 5.2)
   - Implement selective and batch invalidation
   - Run tests 5.3.4 (should pass)

6. **Phase 6: Integration** (6 hours)
   - Connect command processing to event generation
   - Implement queue statistics and monitoring
   - Implement atomic queue reset
   - Run tests 5.3.5 (should pass)

7. **Phase 7: Performance Optimization** (4 hours)
   - Profile queue operations
   - Optimize lock contention
   - Tune queue capacities
   - Run tests 5.3.6 (should meet targets)

**Total Estimated Time:** 46 hours (matches TASKS-RUST.md Section 5.3)

## Expected Test Results

### Initial State (Before Implementation)
All 42 tests should **FAIL** with compilation errors:
```
error[E0425]: cannot find function `submit_event` in type `SMMU`
error[E0425]: cannot find function `get_event_queue_size` in type `SMMU`
error[E0425]: cannot find function `submit_command` in type `SMMU`
...
```

This is **expected behavior** for TDD - tests drive the implementation.

### After Type Definitions (Phase 1)
Tests should compile but **FAIL** at runtime:
```
test test_section_5_3_1_event_queue_initialization ... FAILED
test test_section_5_3_2_command_queue_initialization ... FAILED
...
```

### After Event Queue Implementation (Phase 2)
Event queue tests (5.3.1) should **PASS**:
```
test test_section_5_3_1_event_queue_initialization ... ok
test test_section_5_3_1_submit_translation_fault_event ... ok
test test_section_5_3_1_event_queue_fifo_ordering ... ok
test test_section_5_3_1_concurrent_event_submission ... ok
...
```

### After Full Implementation (Phase 7)
All 42 tests should **PASS**:
```
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Coverage Requirements

### Code Coverage Target: >95%

**Covered Code Paths:**
- Event queue submission (all event types)
- Event queue retrieval (all filtering modes)
- Event queue overflow handling
- Command queue submission (all command types)
- Command queue processing (all command types)
- Command validation (all error conditions)
- PRI queue submission and processing
- Cache invalidation (all invalidation types)
- Concurrent access (all queues)
- Queue reset and clearing
- Statistics and monitoring APIs

**Coverage Metrics:**
- Line coverage: >95%
- Branch coverage: >90%
- Function coverage: 100% (all public APIs)
- Concurrency coverage: All `RwLock` paths tested

## Integration with Existing Codebase

### Dependencies
```rust
// From existing modules
use smmu::SMMU;                    // Section 5.1
use smmu::cache::TLBCache;        // Section 5.2
use smmu::types::{
    StreamID, PASID, IOVA,        // Section 2.1
    AccessType, SecurityState,     // Section 2.2
    // New types added in Section 5.3:
    EventEntry, EventType,
    CommandEntry, CommandType,
    PRIEntry, QueueConfig,
};
```

### Required APIs (To Be Implemented)

#### Event Queue APIs
```rust
impl SMMU {
    // Event queue management
    pub fn submit_event(&self, event: EventEntry) -> Result<(), SMMUError>;
    pub fn get_events(&self) -> Vec<EventEntry>;
    pub fn get_events_by_type(&self, event_type: EventType) -> Vec<EventEntry>;
    pub fn get_events_by_stream(&self, stream_id: StreamID) -> Vec<EventEntry>;
    pub fn has_events(&self) -> bool;
    pub fn get_event_queue_size(&self) -> u64;
    pub fn clear_event_queue(&self);
}
```

#### Command Queue APIs
```rust
impl SMMU {
    // Command queue management
    pub fn submit_command(&self, command: CommandEntry) -> Result<(), SMMUError>;
    pub fn process_command_queue(&self);
    pub fn is_command_queue_full(&self) -> bool;
    pub fn get_command_queue_size(&self) -> u64;
    pub fn clear_command_queue(&self);
}
```

#### PRI Queue APIs
```rust
impl SMMU {
    // PRI queue management
    pub fn submit_page_request(&self, request: PRIEntry) -> Result<(), SMMUError>;
    pub fn process_pri_queue(&self);
    pub fn get_pri_queue(&self) -> Vec<PRIEntry>;
    pub fn get_pri_queue_size(&self) -> u64;
    pub fn clear_pri_queue(&self);
}
```

#### Queue Statistics APIs
```rust
impl SMMU {
    // Statistics and monitoring
    pub fn get_queue_statistics(&self) -> QueueStatistics;
    pub fn reset_queues(&self);
}

pub struct QueueStatistics {
    pub fn event_queue_size(&self) -> u64;
    pub fn command_queue_size(&self) -> u64;
    pub fn pri_queue_size(&self) -> u64;
    pub fn event_queue_utilization(&self) -> f64;
    // ... more fields
}
```

#### Cache Statistics APIs (integration with Section 5.2)
```rust
impl SMMU {
    pub fn get_cache_statistics(&self) -> CacheStatistics;
}

pub struct CacheStatistics {
    pub fn invalidation_count(&self) -> u64;
    // ... existing cache stats
}
```

## Success Criteria

### Functional Requirements
- ✅ All 42 tests pass
- ✅ Zero compilation warnings
- ✅ No unsafe code (or minimal, well-documented unsafe)
- ✅ All panics handled (no unwrap() in production code)
- ✅ Full ARM SMMU v3 compliance for queue management

### Quality Requirements
- ✅ Code coverage >95%
- ✅ Documentation coverage 100% (all public APIs)
- ✅ clippy::pedantic passes with no warnings
- ✅ No memory leaks detected (verified by tests)
- ✅ Thread safety verified (Send + Sync traits)

### Performance Requirements
- ✅ Event queue: >100k submissions/second
- ✅ Command queue: >50k submissions/second
- ✅ Queue latency: <1μs average
- ✅ Memory usage: <1KB per queue entry
- ✅ Concurrent scalability: Linear up to 10 threads

### Integration Requirements
- ✅ Integrates with Section 5.1 (SMMU controller)
- ✅ Integrates with Section 5.2 (TLB cache)
- ✅ Integrates with Section 4.x (stream context)
- ✅ Integrates with Section 3.x (address space)
- ✅ No breaking changes to existing APIs

## Next Steps

1. **Immediate:** Add this test file to the regression suite:
   ```bash
   cd /home/jpgreninger/Work/smmu/rust/smmu
   cargo test test_section_5_3 --no-fail-fast
   ```

2. **Development:** Follow TDD phases 1-7 as outlined above

3. **Documentation:** Update TASKS-RUST.md with test status after each phase

4. **Review:** Use qa-engineer agent after each phase for ARM spec compliance

5. **Integration:** Ensure test suite runs in CI/CD pipeline

## Test Execution

### Run All Section 5.3 Tests
```bash
cargo test test_section_5_3
```

### Run Specific Test Categories
```bash
# Event queue tests only
cargo test test_section_5_3_1

# Command queue tests only
cargo test test_section_5_3_2

# PRI queue tests only
cargo test test_section_5_3_3

# Cache invalidation tests only
cargo test test_section_5_3_4

# Integration tests only
cargo test test_section_5_3_5

# Performance tests only
cargo test test_section_5_3_6
```

### Run with Output
```bash
cargo test test_section_5_3 -- --nocapture
```

### Run with Coverage
```bash
cargo tarpaulin --test test_queues_section_5_3 --out Html
```

## References

- **ARM Specification:** ARM IHI 0070G - Section 6 (Event and Command Queues)
- **ARM Specification:** ARM IHI 0070G - Section 7 (Page Request Interface)
- **ARM Specification:** ARM IHI 0070G - Section 9 (Cache Invalidation)
- **TASKS-RUST.md:** Section 5.3 (Event and Command Processing)
- **C++ Reference:** `/home/jpgreninger/Work/smmu/include/smmu/smmu.h` lines 71-96, 132-140
- **C++ Types:** `/home/jpgreninger/Work/smmu/include/smmu/types.h` lines 1088-1177

---

**Test Suite Status:** ✅ Complete and ready for TDD implementation
**ARM SMMU v3 Compliance:** ✅ Full specification adherence
**Coverage Target:** >95% code coverage
**Performance Target:** >100k events/sec, >50k commands/sec, <1μs latency
