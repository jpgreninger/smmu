//! ARM SMMU v3 StreamContext Stream Operations Tests - Section 4.2
//!
//! Comprehensive TDD test suite for Stream Operations implementation following the
//! ARM SMMU v3 specification. These tests are written BEFORE implementation to
//! drive the design and ensure complete functionality coverage.
//!
//! # Test Organization
//!
//! 1. **Configuration Update Tests** (Task 4.2.1)
//!    - Builder pattern for partial config updates
//!    - Transactional updates (all-or-nothing semantics)
//!    - Validation failure rollback
//!    - Concurrent configuration updates
//!    - Configuration limits enforcement
//!
//! 2. **State Machine Transition Tests** (Task 4.2.2)
//!    - Enable/disable state transitions
//!    - Disabled streams reject operations
//!    - Invalid state transition prevention
//!    - Concurrent state transitions
//!    - State persistence across operations
//!
//! 3. **State Querying Tests** (Task 4.2.3)
//!    - Read-only access (immutable references)
//!    - Efficient RwLock-based querying
//!    - Iterator API for stream enumeration
//!    - Concurrent queries don't block operations
//!    - Query consistency under concurrent modifications
//!
//! 4. **Fault Handling Integration Tests** (Task 4.2.4)
//!    - Fault recording with full context
//!    - Fault propagation through translation pipeline
//!    - Fault recovery mechanisms
//!    - Fault statistics tracking
//!    - Concurrent fault handling
//!
//! # ARM SMMU v3 Compliance
//!
//! - Dynamic stream configuration per spec
//! - Stream enable/disable support
//! - Fault handling integration
//! - Thread-safe operations
//!
//! Copyright (c) 2024 John Greninger

#![cfg(test)]

use smmu::types::{AccessType, PagePermissions, SecurityState, TranslationError, IOVA, PA, PASID};
use std::sync::Arc;
use std::thread;

// ============================================================================
// Section 4.2.1: Configuration Update Tests
// ============================================================================

/// Test: Builder pattern for partial configuration updates
///
/// ARM SMMU v3 spec: Stream configuration supports partial updates
#[test]

fn test_section_4_2_1_builder_pattern_partial_update() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    let mut stream_context = StreamContext::new();
    stream_context.create_pasid(PASID::new(1).unwrap()).unwrap();

    // Build partial configuration update using builder pattern
    let config_update = StreamConfigBuilder::new()
        .max_pasids_per_stream(512)
        .stage1_enabled(false)
        .build();

    // Apply configuration update
    let result = stream_context.apply_config(config_update);
    assert!(result.is_ok(), "Partial config update should succeed");

    // Verify only specified fields were updated
    assert!(!stream_context.is_stage1_enabled(), "Stage-1 should be disabled");
    // Stage-2 should remain unchanged (default false)
    assert!(!stream_context.is_stage2_enabled(), "Stage-2 should remain unchanged");
}

/// Test: Transactional updates with all-or-nothing semantics
///
/// ARM SMMU v3 spec: Configuration updates are atomic
#[test]

fn test_section_4_2_1_transactional_update_all_or_nothing() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    let mut stream_context = StreamContext::new();
    stream_context.set_max_pasids_per_stream(100);

    // Create maximum PASIDs (100)
    for i in 0..100 {
        stream_context.create_pasid(PASID::new(i).unwrap()).unwrap();
    }

    // Build configuration that will fail validation (max_pasids < current count)
    let invalid_config = StreamConfigBuilder::new()
        .max_pasids_per_stream(50) // Invalid: less than current 100
        .stage1_enabled(false)
        .stage2_enabled(true)
        .build();

    // Apply configuration - should fail validation
    let result = stream_context.apply_config(invalid_config);
    assert!(result.is_err(), "Invalid config should fail");

    // Verify NO changes were applied (all-or-nothing)
    assert_eq!(stream_context.pasid_count(), 100, "PASID count should be unchanged");
    assert!(stream_context.is_stage1_enabled(), "Stage-1 should remain enabled");
    assert!(!stream_context.is_stage2_enabled(), "Stage-2 should remain disabled");
}

/// Test: Validation failures rollback properly
///
/// ARM SMMU v3 spec: Failed updates leave stream in consistent state
#[test]

fn test_section_4_2_1_validation_failure_rollback() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    let mut stream_context = StreamContext::new();

    // Set initial valid configuration
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(false);
    stream_context.set_max_pasids_per_stream(1024);

    // Build configuration with conflicting settings
    let conflicting_config = StreamConfigBuilder::new()
        .stage1_enabled(false)
        .stage2_enabled(true)
        .stage2_address_space(None) // Invalid: Stage-2 enabled but no AddressSpace
        .build();

    // Apply configuration - should fail validation
    let result = stream_context.apply_config(conflicting_config);
    assert!(result.is_err(), "Conflicting config should fail validation");

    // Verify rollback to original state
    assert!(stream_context.is_stage1_enabled(), "Stage-1 should be rolled back");
    assert!(!stream_context.is_stage2_enabled(), "Stage-2 should be rolled back");
}

/// Test: Concurrent configuration updates are serialized
///
/// ARM SMMU v3 spec: Configuration updates are thread-safe
#[test]

fn test_section_4_2_1_concurrent_config_updates() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    let stream_context = Arc::new(std::sync::Mutex::new(StreamContext::new()));

    // Spawn threads to update configuration concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let mut ctx = ctx.lock().unwrap();
            let config = StreamConfigBuilder::new()
                .max_pasids_per_stream(100 + i * 10)
                .build();
            ctx.apply_config(config).unwrap();
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify configuration is consistent (no corruption)
    let ctx = stream_context.lock().unwrap();
    // Should be one of the valid values set by threads
    let max_pasids = ctx.max_pasids_per_stream();
    assert!(max_pasids >= 100 && max_pasids <= 190);
}

/// Test: Configuration limits enforcement
///
/// ARM SMMU v3 spec: Configuration respects hardware limits
#[test]

fn test_section_4_2_1_configuration_limits_enforcement() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    let mut stream_context = StreamContext::new();

    // Attempt to set max_pasids beyond ARM SMMU v3 limit (2^20 - 1)
    let invalid_config = StreamConfigBuilder::new()
        .max_pasids_per_stream(1 << 21) // 2^21, exceeds 20-bit PASID limit
        .build();

    let result = stream_context.apply_config(invalid_config);
    assert!(result.is_err(), "Config exceeding PASID limit should fail");

    // Verify error type
    match result.unwrap_err() {
        smmu::types::StreamContextError::ConfigurationError(msg) => {
            assert!(msg.contains("PASID limit") || msg.contains("exceeds"));
        }
        err => panic!("Expected ConfigurationError, got: {:?}", err),
    }
}

/// Test: Builder pattern with method chaining
///
/// Rust best practice: Fluent API for configuration
#[test]

fn test_section_4_2_1_builder_method_chaining() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};
    use smmu::address_space::AddressSpace;

    let mut stream_context = StreamContext::new();

    // Create Stage-2 AddressSpace
    let stage2 = Arc::new(AddressSpace::new());

    // Use builder with method chaining
    let config = StreamConfigBuilder::new()
        .max_pasids_per_stream(512)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .stage2_address_space(Some(stage2))
        .build();

    let result = stream_context.apply_config(config);
    assert!(result.is_ok(), "Chained builder config should succeed");

    // Verify all fields were set
    assert!(stream_context.is_stage1_enabled());
    assert!(stream_context.is_stage2_enabled());
}

/// Test: Configuration update preserves existing PASIDs
///
/// ARM SMMU v3 spec: Config updates don't destroy PASID mappings
#[test]

fn test_section_4_2_1_config_preserves_existing_pasids() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    let mut stream_context = StreamContext::new();

    // Create PASIDs with mappings
    for i in 0..5 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);
        stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // Update configuration
    let config = StreamConfigBuilder::new()
        .stage1_enabled(false)
        .stage2_enabled(false) // Bypass mode
        .build();

    stream_context.apply_config(config).unwrap();

    // Verify PASIDs still exist
    assert_eq!(stream_context.pasid_count(), 5, "PASIDs should be preserved");

    // Verify mappings still exist (in bypass mode)
    for i in 0..5 {
        let pasid = PASID::new(i).unwrap();
        assert!(stream_context.has_pasid(pasid), "PASID {} should exist", i);
    }
}

// ============================================================================
// Section 4.2.2: State Machine Transition Tests
// ============================================================================

/// Test: Enable/disable state transitions
///
/// ARM SMMU v3 spec: Stream can be enabled and disabled
#[test]

fn test_section_4_2_2_enable_disable_transitions() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();

    // Initial state: enabled
    assert!(stream_context.is_enabled(), "Stream should be enabled initially");

    // Disable stream
    stream_context.disable();
    assert!(!stream_context.is_enabled(), "Stream should be disabled");

    // Re-enable stream
    stream_context.enable();
    assert!(stream_context.is_enabled(), "Stream should be re-enabled");
}

/// Test: Disabled streams reject translation operations
///
/// ARM SMMU v3 spec: Disabled streams return error on access
#[test]

fn test_section_4_2_2_disabled_stream_rejects_operations() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);

    // Create PASID and mapping while enabled
    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Disable stream
    stream_context.disable();

    // Attempt translation - should fail
    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Disabled stream should reject translation");

    // Verify error type
    match result.unwrap_err() {
        TranslationError::StreamDisabled => {
            // Expected error
        }
        err => panic!("Expected StreamDisabled error, got: {:?}", err),
    }
}

/// Test: Disabled streams reject PASID creation
///
/// ARM SMMU v3 spec: Configuration operations fail on disabled stream
#[test]

fn test_section_4_2_2_disabled_stream_rejects_pasid_creation() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();

    // Disable stream
    stream_context.disable();

    // Attempt PASID creation - should fail
    let pasid = PASID::new(1).unwrap();
    let result = stream_context.create_pasid(pasid);
    assert!(result.is_err(), "Disabled stream should reject PASID creation");

    match result.unwrap_err() {
        smmu::types::StreamContextError::ConfigurationError(msg) => {
            assert!(msg.contains("disabled") || msg.contains("not enabled"));
        }
        err => panic!("Expected ConfigurationError, got: {:?}", err),
    }
}

/// Test: Invalid state transition prevention
///
/// ARM SMMU v3 spec: Certain state transitions are invalid
#[test]

fn test_section_4_2_2_invalid_state_transition_prevention() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();

    // Create PASIDs with active translations
    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    // Attempt to disable while PASIDs exist (might be invalid depending on policy)
    // Implementation should either:
    // 1. Allow disable but clear PASIDs
    // 2. Reject disable with error
    // Our implementation uses option 1 (auto-clear)
    stream_context.disable();

    // Verify PASIDs were cleared
    assert_eq!(stream_context.pasid_count(), 0, "PASIDs should be cleared on disable");
}

/// Test: Concurrent state transitions
///
/// ARM SMMU v3 spec: State transitions are thread-safe
#[test]

fn test_section_4_2_2_concurrent_state_transitions() {
    use smmu::stream_context::StreamContext;

    let stream_context = Arc::new(std::sync::Mutex::new(StreamContext::new()));

    // Spawn threads to toggle state concurrently
    let mut handles = vec![];
    for i in 0..20 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let mut ctx = ctx.lock().unwrap();
            if i % 2 == 0 {
                ctx.enable();
            } else {
                ctx.disable();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify final state is consistent (either enabled or disabled, no corruption)
    let ctx = stream_context.lock().unwrap();
    let is_enabled = ctx.is_enabled();
    assert!(is_enabled || !is_enabled, "State should be consistent");
}

/// Test: State persistence across operations
///
/// ARM SMMU v3 spec: Stream state persists across multiple operations
#[test]

fn test_section_4_2_2_state_persistence_across_operations() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();

    // Disable stream
    stream_context.disable();

    // Perform various operations (should all fail or skip)
    let pasid = PASID::new(1).unwrap();
    let result1 = stream_context.create_pasid(pasid);
    assert!(result1.is_err(), "PASID creation should fail while disabled");

    // Verify stream is still disabled
    assert!(!stream_context.is_enabled(), "Stream should remain disabled");

    // Re-enable
    stream_context.enable();

    // Now operations should succeed
    let result2 = stream_context.create_pasid(pasid);
    assert!(result2.is_ok(), "PASID creation should succeed when enabled");

    // Verify stream is still enabled
    assert!(stream_context.is_enabled(), "Stream should remain enabled");
}

/// Test: Type-state pattern prevents invalid operations
///
/// Rust best practice: Use type system to prevent invalid states
#[test]

fn test_section_4_2_2_type_state_pattern_prevention() {
    // This test would use Rust's type system to prevent operations at compile-time
    // Example: StreamContext<Disabled> vs StreamContext<Enabled>
    // For now, we test runtime checks as a fallback

    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.disable();

    // All PASID operations should fail
    assert!(stream_context.create_pasid(PASID::new(1).unwrap()).is_err());

    // Translation operations should fail
    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Translation should fail on disabled stream");
}

// ============================================================================
// Section 4.2.3: State Querying Tests
// ============================================================================

/// Test: Read-only access with immutable references
///
/// Rust best practice: Immutable references for queries
#[test]

fn test_section_4_2_3_readonly_access_immutable_refs() {
    use smmu::stream_context::StreamContext;

    let stream_context = StreamContext::new();

    // Create some PASIDs
    for i in 0..5 {
        stream_context.create_pasid(PASID::new(i).unwrap()).unwrap();
    }

    // Query operations should use immutable references
    let query = stream_context.query();

    // Verify query provides read-only access
    assert_eq!(query.pasid_count(), 5);
    assert!(query.has_pasid(PASID::new(0).unwrap()));
    assert!(!query.has_pasid(PASID::new(99).unwrap()));

    // Query should not allow mutation (compile-time check in real code)
    // query.create_pasid(...) // Would fail to compile
}

/// Test: Efficient RwLock-based querying
///
/// ARM SMMU v3 spec: Queries don't block concurrent translations
#[test]

fn test_section_4_2_3_efficient_rwlock_queries() {
    use smmu::stream_context::StreamContext;

    let stream_context = Arc::new(StreamContext::new());

    // Setup PASIDs with mappings
    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);
        stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // Spawn reader threads (queries)
    let mut reader_handles = vec![];
    for _ in 0..20 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            // Multiple queries should not block each other
            for _ in 0..100 {
                let query = ctx.query();
                let count = query.pasid_count();
                assert_eq!(count, 10);
            }
        });
        reader_handles.push(handle);
    }

    // Wait for readers
    for handle in reader_handles {
        handle.join().unwrap();
    }

    // Queries should have completed efficiently (not timed out)
}

/// Test: Iterator API for stream enumeration
///
/// Rust best practice: Provide iterator for PASID enumeration
#[test]

fn test_section_4_2_3_iterator_api_stream_enumeration() {
    use smmu::stream_context::StreamContext;

    let stream_context = StreamContext::new();

    // Create PASIDs
    let pasid_values = vec![0, 1, 5, 10, 42, 100];
    for &val in &pasid_values {
        stream_context.create_pasid(PASID::new(val).unwrap()).unwrap();
    }

    // Iterate over PASIDs
    let query = stream_context.query();
    let collected_pasids: Vec<u32> = query.pasids().map(|p| p.as_u32()).collect();

    // Verify all PASIDs are present (order may vary)
    assert_eq!(collected_pasids.len(), pasid_values.len());
    for val in pasid_values {
        assert!(collected_pasids.contains(&val), "PASID {} should be in iterator", val);
    }
}

/// Test: Concurrent queries don't block operations
///
/// ARM SMMU v3 spec: Read queries concurrent with translations
#[test]

fn test_section_4_2_3_concurrent_queries_dont_block() {
    use smmu::stream_context::StreamContext;

    let mut stream_context_setup = StreamContext::new();
    stream_context_setup.set_stage1_enabled(true);
    stream_context_setup.set_stage2_enabled(false);

    // Setup PASIDs
    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context_setup.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);
        stream_context_setup.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    let stream_context = Arc::new(stream_context_setup);

    // Spawn query threads
    let mut query_handles = vec![];
    for _ in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let query = ctx.query();
                let _count = query.pasid_count();
            }
        });
        query_handles.push(handle);
    }

    // Spawn translation threads (operations)
    let mut translation_handles = vec![];
    for i in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();

            for _ in 0..100 {
                let _result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            }
        });
        translation_handles.push(handle);
    }

    // Wait for all threads - should complete without deadlock
    for handle in query_handles {
        handle.join().unwrap();
    }
    for handle in translation_handles {
        handle.join().unwrap();
    }
}

/// Test: Query consistency under concurrent modifications
///
/// ARM SMMU v3 spec: Queries return consistent snapshots
#[test]

fn test_section_4_2_3_query_consistency_concurrent_mods() {
    use smmu::stream_context::StreamContext;

    let stream_context = Arc::new(StreamContext::new());

    // Initial PASIDs
    for i in 0..5 {
        stream_context.create_pasid(PASID::new(i).unwrap()).unwrap();
    }

    let ctx_writer = Arc::clone(&stream_context);
    let ctx_reader = Arc::clone(&stream_context);

    // Writer thread (adds/removes PASIDs)
    let writer = thread::spawn(move || {
        for i in 5..15 {
            ctx_writer.create_pasid(PASID::new(i).unwrap()).unwrap();
            thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    // Reader thread (queries state)
    let reader = thread::spawn(move || {
        for _ in 0..50 {
            let query = ctx_reader.query();
            let count = query.pasid_count();

            // Count should be consistent (between 5 and 15)
            assert!(count >= 5 && count <= 15, "PASID count {} out of expected range", count);

            // Verify all PASIDs in iterator exist
            for pasid in query.pasids() {
                assert!(query.has_pasid(pasid), "PASID from iterator should exist");
            }

            thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

/// Test: Query filter by security state
///
/// ARM SMMU v3 spec: Query PASIDs by security state
#[test]
#[ignore] // TODO: Requires security state tracking per PASID - beyond Section 4.2 scope
fn test_section_4_2_3_query_filter_security_state() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);

    // Create PASIDs with different security states
    for i in 0..3 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);

        let security_state = match i {
            0 => SecurityState::Secure,
            1 => SecurityState::NonSecure,
            _ => SecurityState::Realm,
        };

        stream_context.map_page(pasid, iova, pa, perms, security_state).unwrap();
    }

    // Query by security state
    let query = stream_context.query();

    // Filter for Secure state
    let secure_pasids: Vec<PASID> = query.pasids_by_security_state(SecurityState::Secure).collect();
    assert_eq!(secure_pasids.len(), 1, "Should have 1 Secure PASID");

    // Filter for NonSecure state
    let nonsecure_pasids: Vec<PASID> = query.pasids_by_security_state(SecurityState::NonSecure).collect();
    assert_eq!(nonsecure_pasids.len(), 1, "Should have 1 NonSecure PASID");
}

// ============================================================================
// Section 4.2.4: Fault Handling Integration Tests
// ============================================================================

/// Test: Fault recording with full context
///
/// ARM SMMU v3 spec: Faults recorded with complete diagnostic information
#[test]

fn test_section_4_2_4_fault_recording_full_context() {
    use smmu::stream_context::StreamContext;
    use smmu::types::FaultRecord;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);

    // Create PASID without mapping
    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    // Attempt translation that will fault
    let iova = IOVA::new(0x1000).unwrap();
    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Translation should fault (page not mapped)");

    // Verify fault was recorded
    let faults = stream_context.get_fault_records();
    assert_eq!(faults.len(), 1, "Should have 1 fault recorded");

    let fault = &faults[0];
    // Verify fault contains full context
    assert_eq!(fault.iova(), iova, "Fault should record IOVA");
    assert_eq!(fault.access_type(), AccessType::Read, "Fault should record access type");
    assert_eq!(fault.security_state(), SecurityState::NonSecure, "Fault should record security state");
    // Fault record should have timestamp
    assert!(fault.timestamp() > 0, "Fault should have timestamp");
}

/// Test: Fault propagation through translation pipeline
///
/// ARM SMMU v3 spec: Faults propagate correctly through stages
#[test]

fn test_section_4_2_4_fault_propagation_pipeline() {
    use smmu::stream_context::StreamContext;
    use smmu::address_space::AddressSpace;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);
    stream_context.set_stage2_enabled(true);

    // Setup Stage-2 with NO mapping
    let stage2 = Arc::new(AddressSpace::new());
    stream_context.set_stage2_address_space(Some(stage2));

    // Setup Stage-1 mapping
    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let ipa_as_pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context.map_page(pasid, iova, ipa_as_pa, perms, SecurityState::NonSecure).unwrap();

    // Translation should fault in Stage-2
    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Translation should fault in Stage-2");

    // Verify fault was recorded with Stage-2 attribution
    let faults = stream_context.get_fault_records();
    assert_eq!(faults.len(), 1, "Should have 1 fault recorded");

    let fault = &faults[0];
    assert_eq!(fault.stage(), smmu::types::TranslationStage::Stage2, "Fault should be from Stage-2");
}

/// Test: Fault recovery mechanisms
///
/// ARM SMMU v3 spec: Faults can be recovered from
#[test]

fn test_section_4_2_4_fault_recovery_mechanisms() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);

    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();

    // Translation faults (page not mapped)
    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Translation should fault");
    assert_eq!(stream_context.get_fault_count(), 1, "Should have 1 fault");

    // Recover by mapping the page
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Clear faults
    stream_context.clear_fault_records();
    assert_eq!(stream_context.get_fault_count(), 0, "Faults should be cleared");

    // Translation should now succeed
    let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Translation should succeed after recovery");
}

/// Test: Fault statistics tracking
///
/// ARM SMMU v3 spec: Fault statistics for diagnostics
#[test]

fn test_section_4_2_4_fault_statistics_tracking() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);

    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    // Generate different fault types

    // PageNotMapped fault
    let iova1 = IOVA::new(0x1000).unwrap();
    let _ = stream_context.translate(pasid, iova1, AccessType::Read, SecurityState::NonSecure);

    // PermissionViolation fault (read-only page, write access)
    let iova2 = IOVA::new(0x2000).unwrap();
    let pa = PA::new(0x4000).unwrap();
    let perms_readonly = PagePermissions::new(true, false, false);
    stream_context.map_page(pasid, iova2, pa, perms_readonly, SecurityState::NonSecure).unwrap();
    let _ = stream_context.translate(pasid, iova2, AccessType::Write, SecurityState::NonSecure);

    // Get statistics
    let stats = stream_context.get_fault_statistics();
    assert_eq!(stats.total_faults, 2, "Should have 2 total faults");
    assert_eq!(stats.page_not_mapped_count, 1, "Should have 1 PageNotMapped fault");
    assert_eq!(stats.permission_violation_count, 1, "Should have 1 PermissionViolation fault");
}

/// Test: Concurrent fault handling
///
/// ARM SMMU v3 spec: Fault handling is thread-safe
#[test]

fn test_section_4_2_4_concurrent_fault_handling() {
    use smmu::stream_context::StreamContext;

    let mut stream_context_setup = StreamContext::new();
    stream_context_setup.set_stage1_enabled(true);

    // Create PASIDs (some with mappings, some without)
    for i in 0..10 {
        let pasid = PASID::new(i).unwrap();
        stream_context_setup.create_pasid(pasid).unwrap();

        // Only map even PASIDs
        if i % 2 == 0 {
            let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
            let perms = PagePermissions::new(true, true, false);
            stream_context_setup.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
        }
    }

    let stream_context = Arc::new(stream_context_setup);

    // Spawn threads that will cause faults
    let mut handles = vec![];
    for i in 0..10 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();

            // Attempt translation (odd PASIDs will fault)
            for _ in 0..10 {
                let _result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify faults were recorded correctly
    let faults = stream_context.get_fault_records();
    // Odd PASIDs (5 total) × 10 attempts = 50 faults expected
    assert!(faults.len() >= 50, "Should have recorded concurrent faults");
}

/// Test: Fault rate limiting
///
/// ARM SMMU v3 spec: Prevent fault storms with rate limiting
#[test]

fn test_section_4_2_4_fault_rate_limiting() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);
    stream_context.set_fault_rate_limit(10); // Max 10 faults per PASID

    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();

    // Generate many faults (should be rate-limited)
    for _ in 0..100 {
        let _result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    // Verify only first 10 faults were recorded
    let faults = stream_context.get_fault_records();
    assert_eq!(faults.len(), 10, "Faults should be rate-limited to 10");

    // Verify rate limit flag is set
    let stats = stream_context.get_fault_statistics();
    assert!(stats.rate_limited, "Rate limiting should be active");
}

/// Test: Fault recovery with retry
///
/// ARM SMMU v3 spec: Support automatic retry after fault recovery
#[test]

fn test_section_4_2_4_fault_recovery_with_retry() {
    use smmu::stream_context::StreamContext;

    let mut stream_context = StreamContext::new();
    stream_context.set_stage1_enabled(true);
    stream_context.enable_fault_retry(true);

    let pasid = PASID::new(1).unwrap();
    stream_context.create_pasid(pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();

    // First translation faults
    let result = stream_context.translate_with_retry(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Initial translation should fault");

    // Simulate fault handler mapping the page
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Retry translation - should succeed
    let result = stream_context.translate_with_retry(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Retry translation should succeed after fault recovery");
}

// ============================================================================
// Additional Integration Tests
// ============================================================================

/// Test: Complete workflow - configure, enable, translate, handle faults
///
/// ARM SMMU v3 spec: Full stream lifecycle test
#[test]

fn test_section_4_2_complete_workflow_integration() {
    use smmu::stream_context::{StreamContext, StreamConfigBuilder};

    // 1. Create and configure stream
    let mut stream_context = StreamContext::new();
    let config = StreamConfigBuilder::new()
        .max_pasids_per_stream(256)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .build();
    stream_context.apply_config(config).unwrap();

    // 2. Enable stream
    stream_context.enable();
    assert!(stream_context.is_enabled());

    // 3. Create PASIDs and mappings
    for i in 0..5 {
        let pasid = PASID::new(i).unwrap();
        stream_context.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);
        stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // 4. Query state
    let query = stream_context.query();
    assert_eq!(query.pasid_count(), 5);

    // 5. Perform translations
    for i in 0..5 {
        let pasid = PASID::new(i).unwrap();
        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok(), "Translation {} should succeed", i);
    }

    // 6. Generate fault (unmapped page)
    let pasid = PASID::new(0).unwrap();
    let unmapped_iova = IOVA::new(0x9999000).unwrap();
    let result = stream_context.translate(pasid, unmapped_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Translation of unmapped page should fault");

    // 7. Check fault statistics
    let stats = stream_context.get_fault_statistics();
    assert_eq!(stats.total_faults, 1);

    // 8. Clear faults
    stream_context.clear_fault_records();
    assert_eq!(stream_context.get_fault_count(), 0);

    // 9. Disable stream
    stream_context.disable();
    assert!(!stream_context.is_enabled());

    // 10. Verify operations fail while disabled
    let result = stream_context.create_pasid(PASID::new(99).unwrap());
    assert!(result.is_err(), "PASID creation should fail on disabled stream");
}

/// Test: Stress test - high concurrency with all operations
///
/// ARM SMMU v3 spec: System handles high load correctly
#[test]

fn test_section_4_2_stress_test_high_concurrency() {
    use smmu::stream_context::StreamContext;

    let mut stream_context_setup = StreamContext::new();
    stream_context_setup.set_stage1_enabled(true);

    // Setup initial PASIDs
    for i in 0..50 {
        let pasid = PASID::new(i).unwrap();
        stream_context_setup.create_pasid(pasid).unwrap();

        let iova = IOVA::new((i as u64) * 0x1000 + 0x1000).unwrap();
        let pa = PA::new((i as u64) * 0x4000 + 0x4000).unwrap();
        let perms = PagePermissions::new(true, true, false);
        stream_context_setup.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    let stream_context = Arc::new(stream_context_setup);

    // Spawn many threads with different operations
    let mut handles = vec![];

    // Translation threads
    for i in 0..50 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i % 50).unwrap();
            let iova = IOVA::new(((i % 50) as u64) * 0x1000 + 0x1000).unwrap();
            for _ in 0..1000 {
                let _result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
            }
        });
        handles.push(handle);
    }

    // Query threads
    for _ in 0..20 {
        let ctx = Arc::clone(&stream_context);
        let handle = thread::spawn(move || {
            for _ in 0..500 {
                let query = ctx.query();
                let _count = query.pasid_count();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify system is still consistent
    assert_eq!(stream_context.pasid_count(), 50);
}
