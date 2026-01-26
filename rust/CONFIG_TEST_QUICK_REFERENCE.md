# Configuration Test Suite Quick Reference

## Test File Locations

### Integration Tests (External)
```
/home/jpgreninger/Work/smmu/rust/smmu/tests/
├── config_tests.rs                      # 200+ existing tests (PASSING)
└── config_tests_new_features.rs         # 200+ new tests (FAILING - not implemented)
```

### Unit Tests (Inline)
```
/home/jpgreninger/Work/smmu/rust/smmu/src/types/config.rs
└── mod tests { ... }                    # 40+ inline unit tests
```

## Running Tests

### All Config Tests (Existing + New)
```bash
cd /home/jpgreninger/Work/smmu/rust
cargo test config
```

### Only Passing Tests (Existing)
```bash
cargo test --test config_tests
```

### Only New Feature Tests (Will Fail)
```bash
cargo test --test config_tests_new_features
```

### Include Ignored Tests
```bash
cargo test config -- --include-ignored
```

### List All Tests
```bash
cargo test --test config_tests_new_features -- --list
cargo test --test config_tests -- --list
```

### Run Specific Test
```bash
cargo test test_resource_limits_default -- --exact
```

## Test Structure by Component

### 1. ResourceLimits (40+ tests)
```
test_resource_limits_default                              # Default construction
test_resource_limits_constants                            # Constant values
test_resource_limits_builder_*                            # Builder pattern (7 tests)
test_resource_limits_validation_*                         # Validation (12 tests)
test_resource_limits_min_boundaries                       # Min values
test_resource_limits_max_boundaries                       # Max values
test_resource_limits_clone                                # Clone trait
test_resource_limits_debug                                # Debug trait
test_resource_limits_equality                             # PartialEq
test_resource_limits_inequality                           # PartialEq negative
test_resource_limits_memory_sizes                         # Common memory values
test_resource_limits_thread_counts                        # Common thread counts
test_resource_limits_timeout_values                       # Common timeout values
test_resource_limits_all_fields_custom                    # Full customization
test_resource_limits_tracking_enabled_disabled            # Boolean fields
test_resource_limits_validate_directly                    # Direct construction
test_resource_limits_validate_invalid_*                   # Invalid values (3 tests)
test_resource_limits_builder_fluent_api                   # Method chaining
test_resource_limits_structural_equality                  # Struct comparison
test_resource_limits_each_field_different                 # Field uniqueness (4 tests)
test_resource_limits_timeout_as_duration                  # Helper methods
test_resource_limits_memory_in_bytes                      # Helper methods
test_resource_limits_hardware_concurrency                 # Platform detection
test_resource_limits_partial_override                     # Builder defaults
test_resource_limits_builder_multiple_builds              # Builder reuse
```

### 2. Extended SMMUConfig Methods (40+ tests)
```
test_smmu_config_with_resource_limits                     # ResourceLimits integration
test_smmu_config_server_profile                           # Factory method
test_smmu_config_embedded_profile                         # Factory method
test_smmu_config_development_profile                      # Factory method
test_smmu_config_update_queue_sizes                       # Update method
test_smmu_config_update_queue_sizes_invalid               # Update validation
test_smmu_config_update_cache_settings                    # Update method
test_smmu_config_update_cache_settings_invalid            # Update validation
test_smmu_config_update_address_limits                    # Update method
test_smmu_config_update_address_limits_invalid            # Update validation
test_smmu_config_update_resource_limits                   # Update method
test_smmu_config_update_resource_limits_invalid           # Update validation
test_smmu_config_merge                                    # Config merging
test_smmu_config_merge_partial                            # Partial merge
test_smmu_config_merge_invalid                            # Merge validation
test_smmu_config_reset                                    # Reset to defaults
test_smmu_config_validate_detailed                        # Detailed validation
test_smmu_config_validate_detailed_with_errors            # Error reporting
test_smmu_config_validate_detailed_with_warnings          # Warning reporting
test_smmu_config_to_string                                # Serialization
test_smmu_config_from_string                              # Deserialization
test_smmu_config_from_string_invalid                      # Parse errors
test_smmu_config_from_string_partial                      # Partial parsing
test_smmu_config_roundtrip_string                         # Serialize + deserialize
test_smmu_config_from_string_with_comments                # Comment handling
test_smmu_config_all_profiles_valid                       # Profile validation
test_smmu_config_all_profiles_detailed_validation         # Profile details
test_smmu_config_profile_characteristics                  # Profile comparison
test_smmu_config_update_methods_thread_safe               # Concurrency
test_smmu_config_builder_with_resource_limits             # Builder extension
test_smmu_config_partial_update_preserves_other_fields    # Update isolation
test_smmu_config_chained_updates                          # Multiple updates
test_smmu_config_update_rollback_on_error                 # Error handling
test_smmu_config_merge_preserves_valid_fields             # Merge behavior
```

### 3. ConfigurationError (4 tests in new file + inline)
```
test_configuration_error_types                            # Enum variants
test_configuration_error_display                          # Display trait
test_configuration_error_debug                            # Debug trait
test_configuration_error_from_validation_error            # Conversion
test_configuration_error_construction                     # Constructor
test_configuration_error_types_exist                      # Type existence
```

### 4. ValidationResult (7 tests in new file + inline)
```
test_validation_result_default                            # Default construction
test_validation_result_success                            # Success state
test_validation_result_with_error                         # Error state
test_validation_result_with_warning                       # Warning state
test_validation_result_multiple_errors                    # Error accumulation
test_validation_result_multiple_warnings                  # Warning accumulation
test_validation_result_merge                              # Result merging
test_validation_result_add_warning                        # Add warning method
```

### 5. ConfigConstants (3 tests in new file + inline)
```
test_config_constants_default_file                        # File paths
test_config_constants_backup_file                         # Backup path
test_config_constants_version                             # Version string
test_config_constants_env_vars                            # Environment variables
```

## Test Patterns Used

### 1. Default Construction Pattern
```rust
#[test]
fn test_structure_default() {
    let instance = Structure::default();
    assert_eq!(instance.field, Structure::DEFAULT_FIELD);
    instance.validate().expect("default should be valid");
}
```

### 2. Builder Pattern
```rust
#[test]
fn test_structure_builder() {
    let instance = Structure::builder()
        .field1(value1)
        .field2(value2)
        .build()
        .expect("build should succeed");

    assert_eq!(instance.field1, value1);
}
```

### 3. Validation Pattern
```rust
#[test]
fn test_structure_validation_invalid() {
    let result = Structure::builder()
        .field(invalid_value)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("expected substring"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}
```

### 4. Boundary Testing Pattern
```rust
#[test]
fn test_structure_min_boundary() {
    let instance = Structure::builder()
        .field(Structure::MIN_FIELD)
        .build()
        .expect("min value should be valid");

    assert_eq!(instance.field, Structure::MIN_FIELD);
}

#[test]
fn test_structure_max_boundary() {
    let instance = Structure::builder()
        .field(Structure::MAX_FIELD)
        .build()
        .expect("max value should be valid");

    assert_eq!(instance.field, Structure::MAX_FIELD);
}
```

### 5. Equality Testing Pattern
```rust
#[test]
fn test_structure_equality() {
    let instance1 = Structure::default();
    let instance2 = Structure::default();
    assert_eq!(instance1, instance2);
}

#[test]
fn test_structure_inequality() {
    let instance1 = Structure::builder().field(value1).build().unwrap();
    let instance2 = Structure::builder().field(value2).build().unwrap();
    assert_ne!(instance1, instance2);
}
```

### 6. Range Testing Pattern
```rust
#[test]
fn test_structure_value_range() {
    for value in [val1, val2, val3, ...] {
        let instance = Structure::builder()
            .field(value)
            .build()
            .expect(&format!("value {} should be valid", value));

        assert_eq!(instance.field, value);
    }
}
```

## Constants Reference

### ResourceLimits Constants
```rust
MIN_MEMORY_USAGE: u64       = 1_048_576              // 1MB
MAX_MEMORY_USAGE: u64       = 68_719_476_736         // 64GB
MIN_THREAD_COUNT: u32       = 1
MAX_THREAD_COUNT: u32       = 256
MIN_TIMEOUT_MS: u32         = 10
MAX_TIMEOUT_MS: u32         = 300_000                // 5 minutes
DEFAULT_MAX_MEMORY_USAGE: u64 = 1_073_741_824       // 1GB
DEFAULT_MAX_THREAD_COUNT: u32 = 8
DEFAULT_TIMEOUT_MS: u32     = 1000                   // 1 second
```

### QueueConfig Constants (Existing)
```rust
MIN_QUEUE_SIZE: usize       = 16
MAX_QUEUE_SIZE: usize       = 65536
DEFAULT_EVENT_QUEUE_SIZE: usize = 512
DEFAULT_COMMAND_QUEUE_SIZE: usize = 256
DEFAULT_PRI_QUEUE_SIZE: usize = 128
```

### CacheConfig Constants (Existing)
```rust
MIN_CACHE_SIZE: usize       = 64
MAX_CACHE_SIZE: usize       = 1_048_576
MIN_CACHE_AGE_MS: u32       = 100
MAX_CACHE_AGE_MS: u32       = 3_600_000
DEFAULT_TLB_CACHE_SIZE: usize = 1024
DEFAULT_CACHE_MAX_AGE_MS: u32 = 5000
```

### AddressConfig Constants (Existing)
```rust
MIN_IOVA_BITS: u8           = 32
MAX_IOVA_BITS: u8           = 52
MIN_PA_BITS: u8             = 32
MAX_PA_BITS: u8             = 52
MIN_STREAM_COUNT: u32       = 1
MAX_STREAM_COUNT: u32       = 1_048_576
MIN_PASID_COUNT: u32        = 1
MAX_PASID_COUNT: u32        = 1_048_576
DEFAULT_IOVA_BITS: u8       = 48
DEFAULT_PA_BITS: u8         = 52
DEFAULT_STREAM_COUNT: u32   = 65536
DEFAULT_PASID_COUNT: u32    = 1_048_576
```

## Implementation Checklist

### ResourceLimits
- [ ] Struct definition with 4 fields
- [ ] 9 constants (MIN/MAX/DEFAULT for each field)
- [ ] Default trait
- [ ] Clone, Debug, PartialEq, Eq traits
- [ ] ResourceLimitsBuilder struct
- [ ] Builder methods (4 setters + build)
- [ ] validate() method
- [ ] Helper methods (timeout, memory conversions)

### Extended SMMUConfig
- [ ] Add resource_limits field
- [ ] 3 profile factory methods
- [ ] 4 update methods
- [ ] merge() method
- [ ] reset() method
- [ ] to_string() method
- [ ] from_string() method
- [ ] validate_detailed() method
- [ ] Update builder to include resource_limits

### ConfigurationError
- [ ] ConfigurationErrorType enum (7 variants)
- [ ] ConfigurationError struct (3 fields)
- [ ] new() constructor
- [ ] Display trait
- [ ] From<ValidationError> trait

### ValidationResult
- [ ] Struct with 3 fields (is_valid, errors, warnings)
- [ ] Default trait
- [ ] success() constructor
- [ ] with_error() constructor
- [ ] add_error() method
- [ ] add_warning() method
- [ ] merge() method

### ConfigConstants
- [ ] Struct or module definition
- [ ] 7 string constants
- [ ] File paths (2)
- [ ] Version (1)
- [ ] Environment variables (4)

## Test Execution Examples

### Run All Tests and See Summary
```bash
cargo test config 2>&1 | grep -E "test result|running"
```

### Run Tests with Output
```bash
cargo test config -- --nocapture
```

### Run Tests in Parallel
```bash
cargo test config -- --test-threads=8
```

### Run Tests Serially (for debugging)
```bash
cargo test config -- --test-threads=1
```

### Show Ignored Tests
```bash
cargo test config -- --ignored --list
```

### Time Test Execution
```bash
time cargo test config --release
```

## Coverage Analysis

### Generate Coverage Report
```bash
cd /home/jpgreninger/Work/smmu/rust
cargo llvm-cov --test config_tests --html
cargo llvm-cov --test config_tests_new_features --html
```

### Check Coverage Percentage
```bash
cargo llvm-cov --test config_tests --summary-only
```

## Debugging Failed Tests

### Run Single Test with Backtrace
```bash
RUST_BACKTRACE=1 cargo test test_resource_limits_default -- --exact
```

### Run Tests with Full Error Messages
```bash
cargo test config -- --nocapture --test-threads=1
```

### Check Test Output
```bash
cargo test config 2>&1 | tee test_output.log
```

## Status Summary

| Component | Tests | Status | File |
|-----------|-------|--------|------|
| StreamConfig | 40+ | ✅ PASSING | config_tests.rs |
| QueueConfig | 40+ | ✅ PASSING | config_tests.rs |
| CacheConfig | 40+ | ✅ PASSING | config_tests.rs |
| AddressConfig | 40+ | ✅ PASSING | config_tests.rs |
| SMMUConfig (basic) | 40+ | ✅ PASSING | config_tests.rs |
| ResourceLimits | 40+ | ❌ FAILING | config_tests_new_features.rs |
| SMMUConfig (extended) | 40+ | ❌ FAILING | config_tests_new_features.rs |
| ConfigurationError | 4+ | ❌ FAILING | config_tests_new_features.rs |
| ValidationResult | 7+ | ❌ FAILING | config_tests_new_features.rs |
| ConfigConstants | 3+ | ❌ FAILING | config_tests_new_features.rs |

**Total**: 200+ passing, 200+ failing (intentionally - not implemented yet)
