# StreamContext Section 4.1 Test Suite Documentation

## Overview

This document describes the comprehensive TDD test suite for **Section 4.1: StreamContext Core** implementation in Rust. The tests are written **BEFORE** implementation to drive design and ensure complete ARM SMMU v3 specification compliance.

## Test Organization

### File Location
- **Test File**: `tests/test_stream_context_section_4_1.rs`
- **Implementation**: `src/stream_context/mod.rs` (to be created)
- **Error Types**: `src/types/mod.rs` (StreamContextError to be added)

### Test Categories (6 sections, 36 tests)

#### Section 4.1.1: PASID Lifecycle Tests (8 tests)
Tests for PASID creation, removal, and management operations.

| Test Name | Description | ARM SMMU v3 Requirement |
|-----------|-------------|------------------------|
| `test_create_pasid_success` | Create new PASID with fresh AddressSpace | Each PASID creates isolated translation context |
| `test_create_duplicate_pasid_error` | Duplicate PASID creation returns error | PASID must be unique within stream context |
| `test_remove_pasid_success` | Remove existing PASID and verify cleanup | PASID removal invalidates all translations |
| `test_remove_nonexistent_pasid_error` | Remove non-existent PASID returns error | Error handling for invalid operations |
| `test_pasid_count_limit_enforcement` | Maximum PASIDs per stream enforcement | Configurable resource limits |
| `test_clear_all_pasids` | Clear all PASIDs operation | Complete stream context invalidation |
| `test_pasid_zero_is_valid` | PASID 0 support verification | PASID 0 is valid for kernel/hypervisor |
| `test_invalid_pasid_rejected` | Invalid PASID values rejected | PASID is 20-bit (0 to 1,048,575) |

**Key Features Tested**:
- PASID creation with `create_pasid()`
- PASID removal with `remove_pasid()`
- PASID existence checking with `has_pasid()`
- PASID count tracking with `pasid_count()`
- PASID limit enforcement with `set_max_pasids_per_stream()`
- Clear all with `clear_all_pasids()`
- PASID 0 validity (ARM SMMU v3 requirement)

#### Section 4.1.2: Isolation Validation Tests (4 tests)
Tests for PASID isolation and security enforcement.

| Test Name | Description | ARM SMMU v3 Requirement |
|-----------|-------------|------------------------|
| `test_pasid_zero_isolation` | PASID 0 isolation from other PASIDs | Each PASID has independent address space |
| `test_cross_pasid_access_prevention` | One PASID cannot access another's space | PASID isolation enforcement |
| `test_security_state_isolation` | Security state isolation verification | Secure/NonSecure/Realm isolation |
| `test_concurrent_pasid_access` | Concurrent PASID access safety | Thread-safe concurrent operations |

**Key Features Tested**:
- Independent address spaces per PASID
- Cross-PASID access prevention
- Security state boundaries (Secure/NonSecure/Realm)
- Thread-safe concurrent access with Arc wrapping
- Proper isolation at translation time

#### Section 4.1.3: Configuration Validation Tests (6 tests)
Tests for stage configuration and validation.

| Test Name | Description | ARM SMMU v3 Requirement |
|-----------|-------------|------------------------|
| `test_stage1_only_configuration` | Stage-1 enabled, Stage-2 disabled | Per-PASID translation only |
| `test_stage2_only_configuration` | Stage-1 disabled, Stage-2 enabled | Shared Stage-2 translation |
| `test_two_stage_configuration` | Both stages enabled | Two-stage translation (IOVA→IPA→PA) |
| `test_bypass_mode_both_stages_disabled` | Both stages disabled | Identity mapping (bypass) |
| `test_invalid_configuration_detection` | Invalid config detection | Configuration validation |
| `test_configuration_state_transitions` | Dynamic reconfiguration | State transition support |

**Key Features Tested**:
- Stage-1 enable/disable with `set_stage1_enabled()`
- Stage-2 enable/disable with `set_stage2_enabled()`
- Stage-2 AddressSpace assignment with `set_stage2_address_space()`
- Configuration queries with `is_stage1_enabled()`, `is_stage2_enabled()`
- Invalid configuration error handling
- Dynamic configuration changes

#### Section 4.1.4: Stage-1/Stage-2 Translation Tests (4 tests)
Tests for translation behavior across different stage configurations.

| Test Name | Description | ARM SMMU v3 Requirement |
|-----------|-------------|------------------------|
| `test_stage1_translation_multiple_pasids` | Stage-1 translation with multiple PASIDs | Independent per-PASID translation |
| `test_stage2_shared_across_pasids` | Stage-2 shared across PASIDs | Shared Stage-2 AddressSpace |
| `test_two_stage_translation_path` | Full two-stage translation flow | IOVA→IPA→PA translation path |
| `test_stage_enable_disable_operations` | Dynamic stage enable/disable | Configuration changes during runtime |

**Key Features Tested**:
- Stage-1 per-PASID translation
- Stage-2 shared translation across PASIDs
- Two-stage translation chain (Stage-1 output → Stage-2 input)
- Dynamic stage configuration changes
- Translation correctness across configurations

#### Section 4.1.5: Thread Safety Tests (3 tests)
Tests for concurrent operations and thread safety.

| Test Name | Description | Rust Requirement |
|-----------|-------------|------------------|
| `test_concurrent_pasid_creation` | Concurrent PASID creation | Thread-safe with Arc/RwLock or Mutex |
| `test_concurrent_translation_requests` | Concurrent translation operations | Concurrent read access support |
| `test_concurrent_configuration_updates` | Concurrent configuration changes | Thread-safe configuration updates |

**Key Features Tested**:
- Thread-safe PASID creation (20 concurrent threads)
- Thread-safe translation (10 PASIDs × 100 translations each)
- Thread-safe configuration updates (10 concurrent toggles)
- Arc wrapping for shared ownership
- Mutex/RwLock for interior mutability
- No data races or deadlocks

#### Section 4.1.6: Integration with AddressSpace Tests (4 tests)
Tests for proper integration with AddressSpace module.

| Test Name | Description | Integration Requirement |
|-----------|-------------|------------------------|
| `test_map_unmap_through_stream_context` | Map/unmap delegation to AddressSpace | Proper API delegation |
| `test_translation_through_stream_context` | Translation with all access types | Full translation flow |
| `test_address_space_arc_reference_counting` | Arc reference counting verification | Shared ownership semantics |
| `test_shared_address_space_multiple_pasids` | Shared AddressSpace across PASIDs | Multiple PASIDs sharing same space |

**Key Features Tested**:
- `map_page()` delegation to underlying AddressSpace
- `unmap_page()` delegation to underlying AddressSpace
- `translate()` delegation with proper error propagation
- Arc reference counting (AddressSpace lifetime)
- Shared AddressSpace support via `add_pasid()`
- Permission checking (Read/Write/Execute)

### Additional Edge Case Tests (7 tests)
Critical error handling and edge case validation.

| Test Name | Description | ARM SMMU v3 Requirement |
|-----------|-------------|------------------------|
| `test_translation_nonexistent_pasid` | Translation with invalid PASID | PASIDNotFound error |
| `test_permission_violation_detection` | Permission violation on write | PermissionViolation error |
| `test_stage2_fault_propagation` | Stage-2 fault propagation | Correct fault attribution |
| `test_drop_cleanup` | RAII cleanup verification | Proper resource cleanup |

**Key Features Tested**:
- PASIDNotFound error for invalid PASIDs
- PermissionViolation for access type violations
- Stage-2 fault detection and propagation
- RAII cleanup (Drop trait implementation)

## API Requirements

Based on the test suite, the StreamContext implementation must provide:

### Construction and Lifecycle
```rust
impl StreamContext {
    pub fn new() -> Self;
}

impl Drop for StreamContext {
    fn drop(&mut self);
}
```

### PASID Management
```rust
pub fn create_pasid(&self, pasid: PASID) -> Result<(), StreamContextError>;
pub fn remove_pasid(&self, pasid: PASID) -> Result<(), StreamContextError>;
pub fn add_pasid(&self, pasid: PASID, address_space: Arc<AddressSpace>);
pub fn has_pasid(&self, pasid: PASID) -> bool;
pub fn pasid_count(&self) -> usize;
pub fn clear_all_pasids(&self) -> Result<(), StreamContextError>;
pub fn set_max_pasids_per_stream(&mut self, max: u32);
pub fn get_pasid_address_space(&self, pasid: PASID) -> Option<Arc<AddressSpace>>;
```

### Stage Configuration
```rust
pub fn set_stage1_enabled(&mut self, enabled: bool);
pub fn set_stage2_enabled(&mut self, enabled: bool);
pub fn is_stage1_enabled(&self) -> bool;
pub fn is_stage2_enabled(&self) -> bool;
pub fn set_stage2_address_space(&mut self, address_space: Option<Arc<AddressSpace>>);
```

### Page Mapping Operations
```rust
pub fn map_page(
    &self,
    pasid: PASID,
    iova: IOVA,
    pa: PA,
    permissions: PagePermissions,
    security_state: SecurityState,
) -> Result<(), StreamContextError>;

pub fn unmap_page(&self, pasid: PASID, iova: IOVA) -> Result<(), StreamContextError>;
```

### Translation Operations
```rust
pub fn translate(
    &self,
    pasid: PASID,
    iova: IOVA,
    access_type: AccessType,
    security_state: SecurityState,
) -> Result<TranslationData, TranslationError>;
```

## Error Types Required

The implementation requires a `StreamContextError` enum in `src/types/mod.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StreamContextError {
    #[error("PASID already exists")]
    PASIDAlreadyExists,

    #[error("PASID not found")]
    PASIDNotFound,

    #[error("PASID limit exceeded")]
    PASIDLimitExceeded,

    #[error("Invalid PASID value")]
    InvalidPASID,

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Internal error")]
    InternalError,
}
```

Also requires extending `TranslationError` enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TranslationError {
    #[error("Page not mapped")]
    PageNotMapped,

    #[error("Permission violation")]
    PermissionViolation,

    #[error("PASID not found")]
    PASIDNotFound,

    #[error("Security violation")]
    SecurityViolation,

    // ... existing variants
}
```

## Data Structure Requirements

### Internal Structure
```rust
pub struct StreamContext {
    // PASID to AddressSpace mapping (HashMap with Arc for shared ownership)
    pasid_map: HashMap<u32, Arc<AddressSpace>>,

    // Stage-2 AddressSpace (shared across PASIDs)
    stage2_address_space: Option<Arc<AddressSpace>>,

    // Configuration flags
    stage1_enabled: bool,
    stage2_enabled: bool,

    // Resource limits
    max_pasids_per_stream: u32,

    // Thread safety (consider RwLock or DashMap)
    // Option 1: Wrap entire struct in Arc<RwLock<StreamContext>>
    // Option 2: Use RwLock<HashMap<...>> for pasid_map
    // Option 3: Use DashMap for lock-free concurrent access
}
```

### Thread Safety Considerations

**Option 1: External synchronization (simplest)**
```rust
pub struct StreamContext {
    pasid_map: HashMap<u32, Arc<AddressSpace>>,
    // ... other fields
}

// Users wrap in Arc<RwLock<StreamContext>> or Arc<Mutex<StreamContext>>
let ctx = Arc::new(RwLock::new(StreamContext::new()));
```

**Option 2: Internal synchronization (more ergonomic)**
```rust
pub struct StreamContext {
    pasid_map: Arc<RwLock<HashMap<u32, Arc<AddressSpace>>>>,
    stage2_address_space: Arc<RwLock<Option<Arc<AddressSpace>>>>,
    stage1_enabled: AtomicBool,
    stage2_enabled: AtomicBool,
    max_pasids_per_stream: AtomicU32,
}
```

**Option 3: Lock-free with DashMap (highest performance)**
```rust
use dashmap::DashMap;

pub struct StreamContext {
    pasid_map: Arc<DashMap<u32, Arc<AddressSpace>>>,
    stage2_address_space: Arc<RwLock<Option<Arc<AddressSpace>>>>,
    stage1_enabled: AtomicBool,
    stage2_enabled: AtomicBool,
    max_pasids_per_stream: AtomicU32,
}
```

**Recommendation**: Start with Option 1 (external synchronization) for simplicity, then optimize to Option 3 (DashMap) if profiling shows lock contention.

## ARM SMMU v3 Specification Compliance

### PASID Requirements
- ✅ **PASID 0 Support**: PASID 0 is valid and commonly used for kernel/hypervisor contexts
- ✅ **PASID Range**: 20-bit PASID (0 to 1,048,575)
- ✅ **PASID Isolation**: Each PASID has independent address space
- ✅ **PASID Limits**: Configurable maximum PASIDs per stream

### Translation Stages
- ✅ **Stage-1 Only**: Per-PASID translation (IOVA→PA)
- ✅ **Stage-2 Only**: Shared translation across PASIDs (IPA→PA)
- ✅ **Two-Stage**: Full translation chain (IOVA→IPA→PA)
- ✅ **Bypass Mode**: Identity mapping when both stages disabled

### Security State Isolation
- ✅ **Secure State**: Isolated address space
- ✅ **NonSecure State**: Isolated address space
- ✅ **Realm State**: ARM CCA Realm support

### Fault Handling
- ✅ **PASIDNotFound**: Translation with non-existent PASID
- ✅ **PageNotMapped**: Translation fault (Stage-1 or Stage-2)
- ✅ **PermissionViolation**: Access type violation
- ✅ **SecurityViolation**: Security state mismatch

## Test Execution

### Running All Tests
```bash
cd rust/smmu
cargo test --test test_stream_context_section_4_1
```

### Running Specific Test Categories
```bash
# PASID Lifecycle Tests
cargo test --test test_stream_context_section_4_1 test_create_pasid
cargo test --test test_stream_context_section_4_1 test_remove_pasid

# Isolation Tests
cargo test --test test_stream_context_section_4_1 test_pasid_zero_isolation
cargo test --test test_stream_context_section_4_1 test_cross_pasid

# Configuration Tests
cargo test --test test_stream_context_section_4_1 test_stage1_only
cargo test --test test_stream_context_section_4_1 test_stage2_only

# Thread Safety Tests
cargo test --test test_stream_context_section_4_1 test_concurrent
```

### Running with Output
```bash
cargo test --test test_stream_context_section_4_1 -- --nocapture
```

### Running Ignored Tests
Once implementation is complete, remove `#[ignore]` attributes:
```bash
cargo test --test test_stream_context_section_4_1 -- --ignored
```

## Implementation Workflow

### Step 1: Create StreamContext Module
```bash
touch src/stream_context/mod.rs
```

Add to `src/lib.rs`:
```rust
pub mod stream_context;
```

### Step 2: Add Error Types
Update `src/types/mod.rs` to include:
- `StreamContextError` enum
- Extended `TranslationError` enum

### Step 3: Implement Basic Structure
Start with simplest implementation:
```rust
pub struct StreamContext {
    pasid_map: HashMap<u32, Arc<AddressSpace>>,
    stage2_address_space: Option<Arc<AddressSpace>>,
    stage1_enabled: bool,
    stage2_enabled: bool,
    max_pasids_per_stream: u32,
}

impl StreamContext {
    pub fn new() -> Self {
        Self {
            pasid_map: HashMap::new(),
            stage2_address_space: None,
            stage1_enabled: true,
            stage2_enabled: false,
            max_pasids_per_stream: 1024,
        }
    }
}
```

### Step 4: Implement PASID Operations
Implement in order:
1. `create_pasid()` - basic creation
2. `has_pasid()` - existence check
3. `pasid_count()` - count tracking
4. `remove_pasid()` - removal
5. `clear_all_pasids()` - bulk removal
6. `set_max_pasids_per_stream()` - limit configuration

### Step 5: Implement Stage Configuration
1. `set_stage1_enabled()` / `is_stage1_enabled()`
2. `set_stage2_enabled()` / `is_stage2_enabled()`
3. `set_stage2_address_space()`

### Step 6: Implement Page Operations
1. `map_page()` - delegate to AddressSpace
2. `unmap_page()` - delegate to AddressSpace

### Step 7: Implement Translation
1. Stage-1 only translation
2. Stage-2 only translation
3. Two-stage translation
4. Bypass mode

### Step 8: Add Thread Safety
Wrap in Arc/RwLock or use internal synchronization

### Step 9: Remove #[ignore] Attributes
Once all tests pass, remove `#[ignore]` from all tests

## Test Coverage Metrics

### Expected Coverage
- **Line Coverage**: >95%
- **Branch Coverage**: >90%
- **Function Coverage**: 100%

### Coverage Analysis
```bash
cargo llvm-cov --test test_stream_context_section_4_1 --html
```

## Performance Benchmarks

While not included in this test suite, consider adding benchmarks for:
- PASID creation/removal throughput
- Translation latency (target: <135ns)
- Concurrent translation throughput
- Memory usage per PASID

## References

- **ARM SMMU v3 Specification**: IHI0070G (Section 5: Context Descriptors, Section 6: Translation)
- **C++ Implementation**: `/home/jpgreninger/Work/smmu/include/smmu/stream_context.h`
- **C++ Tests**: `/home/jpgreninger/Work/smmu/tests/unit/test_stream_context*.cpp`
- **TASKS-RUST.md**: Section 4.1 (StreamContext Core)

## Success Criteria

- ✅ All 36 tests pass (0 failures)
- ✅ Zero unsafe code
- ✅ 100% ARM SMMU v3 specification compliance
- ✅ >95% code coverage
- ✅ Thread-safe concurrent operations
- ✅ Proper RAII cleanup
- ✅ Comprehensive error handling
- ✅ Feature parity with C++ implementation

---

**Status**: ⏳ Tests written, implementation pending
**Estimated Implementation Time**: 24-30 hours
**Priority**: P1 (High) - Core functionality
**Dependencies**: AddressSpace module (Section 3.1, complete)
