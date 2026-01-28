# Section 5.1 Test Suite - SMMU Core Implementation

This test suite provides comprehensive TDD tests for the SMMU central controller implementation, written BEFORE implementation to drive the design.

## Test Organization

### 5.1.1 Initialization and Shutdown Tests (6 tests)

Tests for proper SMMU lifecycle management:

1. `test_section_5_1_1_new_default_config` - Default initialization
2. `test_section_5_1_1_new_with_custom_config` - Custom configuration
3. `test_section_5_1_1_proper_initialization` - Internal state initialization
4. `test_section_5_1_1_shutdown_releases_resources` - Resource cleanup on shutdown
5. `test_section_5_1_1_drop_cleanup` - Drop trait cleanup
6. `test_section_5_1_1_no_memory_leaks` - Arc reference counting verification

### 5.1.2 Thread-Safe State Management Tests (6 tests)

Tests for concurrent access patterns:

1. `test_section_5_1_2_concurrent_access_with_arc` - Arc<SMMU> sharing
2. `test_section_5_1_2_interior_mutability` - RwLock/Mutex patterns
3. `test_section_5_1_2_stream_mapping` - StreamID → StreamContext mapping
4. `test_section_5_1_2_global_config_access` - Read-heavy configuration access
5. `test_section_5_1_2_concurrent_stream_configuration` - Concurrent stream setup
6. `test_section_5_1_2_no_deadlock` - Deadlock prevention

### 5.1.3 Stream Management Tests (6 tests)

Tests for stream lifecycle operations:

1. `test_section_5_1_3_configure_stream_creates_context` - Stream creation
2. `test_section_5_1_3_remove_stream_cleanup` - Stream removal and cleanup
3. `test_section_5_1_3_get_stream_returns_context` - Stream retrieval
4. `test_section_5_1_3_stream_isolation` - Stream independence
5. `test_section_5_1_3_max_stream_count_enforcement` - Resource limits
6. `test_section_5_1_3_concurrent_stream_creation_removal` - Concurrent lifecycle

### 5.1.4 Concurrent Access Tests (8 tests)

Tests for high-concurrency scenarios:

1. `test_section_5_1_4_concurrent_translations` - 100+ concurrent translation requests
2. `test_section_5_1_4_concurrent_stream_config` - 50 concurrent stream configurations
3. `test_section_5_1_4_concurrent_create_delete` - Concurrent create/delete cycles
4. `test_section_5_1_4_mixed_read_write_operations` - Reader/writer patterns
5. `test_section_5_1_4_send_sync_bounds` - Compile-time thread safety
6. `test_section_5_1_4_no_deadlock_high_load` - 100 threads heavy load
7. Additional stress tests as needed

### 5.1.5 Resource Management Tests (5 tests)

Tests for resource lifecycle and cleanup:

1. `test_section_5_1_5_arc_reference_counting` - Arc refcount tracking
2. `test_section_5_1_5_cleanup_on_removal` - Proper cleanup semantics
3. `test_section_5_1_5_no_dangling_refs_after_shutdown` - Shutdown safety
4. `test_section_5_1_5_memory_bounds_many_streams` - 1000 stream test
5. `test_section_5_1_5_resource_limits_enforcement` - Limit enforcement

### Integration Tests (1 test)

1. `test_section_5_1_integration_basic_translation` - End-to-end translation flow

## Total Tests: 32 comprehensive tests

## Expected SMMU API

The tests define the following API that must be implemented:

```rust
pub struct SMMU {
    // Internal state (Arc<RwLock<SMMUInner>> or DashMap)
}

impl SMMU {
    // Initialization
    pub fn new() -> Self;
    pub fn new_with_config(config: SMMUConfig) -> Self;

    // Stream management
    pub fn configure_stream(&mut self, stream_id: StreamID, config: StreamConfig)
        -> Result<(), SMMUError>;
    pub fn configure_stream_concurrent(&self, stream_id: StreamID, config: StreamConfig)
        -> Result<(), SMMUError>;
    pub fn remove_stream(&mut self, stream_id: StreamID) -> Result<(), SMMUError>;
    pub fn remove_stream_concurrent(&self, stream_id: StreamID) -> Result<(), SMMUError>;
    pub fn get_stream(&self, stream_id: StreamID) -> Option<Arc<StreamContext>>;
    pub fn stream_count(&self) -> usize;
    pub fn max_streams(&self) -> usize;

    // Translation
    pub fn translate(&self, stream_id: StreamID, pasid: PASID, iova: IOVA, access_type: AccessType)
        -> TranslationResult;

    // Lifecycle
    pub fn shutdown(&mut self);
    pub fn is_shutdown(&self) -> bool;

    // Events
    pub fn get_event_count(&self) -> usize;
}

// Error type (add to types/mod.rs)
pub enum SMMUError {
    StreamNotFound,
    StreamAlreadyExists,
    StreamLimitExceeded { current: usize, limit: usize },
    InvalidConfiguration,
    ShutdownInProgress,
    AlreadyShutdown,
    // ...
}
```

## Required Type Extensions

Add to `types/mod.rs`:

```rust
pub enum SMMUError {
    StreamNotFound,
    StreamAlreadyExists,
    StreamLimitExceeded { current: usize, limit: usize },
    InvalidConfiguration,
    ShutdownInProgress,
    AlreadyShutdown,
}

// Extend SMMUConfig with:
impl SMMUConfig {
    pub fn with_max_streams(mut self, max: usize) -> Self;
}
```

## Thread Safety Requirements

1. **SMMU must be Send + Sync** - Can be shared across threads
2. **StreamContext must be Send + Sync** - Already implemented in Section 4.1
3. **Interior mutability** - Use DashMap or Arc<RwLock<>> for stream map
4. **Lock-free where possible** - DashMap preferred for stream map
5. **Read-heavy optimization** - RwLock for global configuration

## Implementation Strategy

### Recommended Architecture

```rust
pub struct SMMU {
    // Stream map: StreamID → Arc<StreamContext>
    // Option 1: DashMap<u16, Arc<StreamContext>> (lock-free, preferred)
    // Option 2: Arc<RwLock<HashMap<u16, Arc<StreamContext>>>> (simpler)
    streams: DashMap<u16, Arc<StreamContext>>,

    // Global configuration (read-heavy)
    config: Arc<RwLock<SMMUConfig>>,

    // Shutdown state (atomic)
    shutdown: AtomicBool,

    // Event queue (optional for Section 5.1)
    events: Arc<RwLock<Vec<Event>>>,
}
```

### DashMap vs HashMap<RwLock>

**DashMap (Recommended):**
- Lock-free sharded concurrent hashmap
- Better performance under high concurrency
- No write lock needed for insertions
- Already used in StreamContext

**HashMap with RwLock (Simpler):**
- Easier to understand
- Single lock for all operations
- May have contention under heavy write load
- Acceptable for initial implementation

## Running the Tests

```bash
# Run all Section 5.1 tests
cargo test --test test_smmu_section_5_1

# Run specific test category
cargo test --test test_smmu_section_5_1 test_section_5_1_1
cargo test --test test_smmu_section_5_1 test_section_5_1_2
cargo test --test test_smmu_section_5_1 test_section_5_1_3
cargo test --test test_smmu_section_5_1 test_section_5_1_4
cargo test --test test_smmu_section_5_1 test_section_5_1_5

# Run with output
cargo test --test test_smmu_section_5_1 -- --nocapture

# Run concurrency tests only
cargo test --test test_smmu_section_5_1 concurrent
```

## Expected Test Results (Before Implementation)

All tests should **FAIL** with compilation errors because:

1. `SMMU::new_with_config()` not implemented
2. `SMMU::configure_stream()` not implemented
3. `SMMU::remove_stream()` not implemented
4. `SMMU::get_stream()` not implemented
5. `SMMU::stream_count()` not implemented
6. `SMMU::max_streams()` not implemented
7. `SMMU::shutdown()` not implemented
8. `SMMU::is_shutdown()` not implemented
9. `SMMU::translate()` not implemented
10. `SMMU::get_event_count()` not implemented
11. `SMMU::configure_stream_concurrent()` not implemented
12. `SMMU::remove_stream_concurrent()` not implemented
13. `SMMUError` type not defined
14. `SMMUConfig::with_max_streams()` not implemented

## Coverage Goals

- **Minimum**: 95% code coverage for SMMU implementation
- **Error paths**: 100% coverage of all error types
- **Concurrency**: All concurrent operations tested
- **Resource cleanup**: All RAII paths validated

## ARM SMMU v3 Compliance

These tests verify:

1. **StreamID management** - Per ARM SMMU v3 specification
2. **Resource limits** - Configurable stream limits
3. **Thread safety** - Safe concurrent access patterns
4. **Lifecycle management** - Proper initialization and cleanup
5. **Fault isolation** - Streams don't interfere with each other

## Success Criteria

When implementation is complete:

1. ✅ All 32 tests passing
2. ✅ Zero unsafe code violations
3. ✅ No Clippy warnings (pedantic mode)
4. ✅ >95% code coverage
5. ✅ No memory leaks (verified by Arc refcounts)
6. ✅ No deadlocks under concurrent load
7. ✅ Send + Sync trait bounds satisfied
8. ✅ 100% ARM SMMU v3 specification compliance

## Next Steps

1. **rust-engineer**: Implement SMMU structure and methods in `src/smmu/mod.rs`
2. **rust-engineer**: Add SMMUError type to `src/types/mod.rs`
3. **rust-engineer**: Extend SMMUConfig with builder methods
4. **debugger**: Fix any compilation errors
5. **test-automator**: Run tests and verify they pass
6. **qa-engineer**: Review implementation against ARM SMMU v3 spec
7. **test-automator**: Integrate tests into regression suite

## Dependencies

- Section 4.1 ✅ Complete - StreamContext with DashMap
- Section 4.2 ✅ Complete - StreamContext operations
- Section 3.1 ✅ Complete - AddressSpace implementation
- Section 2.2 ✅ Complete - Core structures (SMMUConfig, etc.)
- Section 2.1 ✅ Complete - Core types (StreamID, PASID, etc.)

## Estimated Time

- **Test writing**: 4 hours (COMPLETE)
- **Implementation**: 24 hours (6 hours per subsection)
- **Testing/debugging**: 8 hours
- **QA review**: 4 hours
- **Total**: 40 hours for Section 5.1

## Notes

- Tests use `#[ignore]` attributes where implementation not yet available
- Tests verify both `&mut self` and `&self` APIs for different use cases
- Concurrent methods (`configure_stream_concurrent`) use `&self` for Arc<SMMU>
- Non-concurrent methods use `&mut self` for simpler single-threaded usage
- All tests follow TDD best practices (written before implementation)
