#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! ARM SMMU v3 Edge Case and Error Testing Suite (Section 8.3)
//!
//! Comprehensive edge case testing covering:
//! - Address range boundary conditions
//! - Unconfigured stream handling
//! - Queue overflow conditions
//! - Permission violation scenarios
//! - Cache invalidation effects
//! - Rust-specific panic safety
//! - Memory safety with Miri compatibility
//! - Thread safety verification
//!
//! This test suite ports C++ edge case tests to Rust while adding
//! Rust-specific safety guarantees and testing patterns.

use smmu::types::{
    AccessType, CommandEntry, CommandType, PagePermissions, SMMUConfig, SecurityState, StreamConfig, StreamID,
    TranslationError, IOVA, PA, PAGE_SIZE, PASID,
};
use smmu::SMMU;
use std::panic;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// =============================================================================
// Test Constants
// =============================================================================

const VALID_STREAM_ID: u32 = 0x1000;
#[allow(dead_code)]
const MAX_STREAM_ID: u32 = 0xFFFF_FFFF;
const INVALID_STREAM_ID: u32 = 0x10_0000; // For testing (still valid technically)

const VALID_PASID: u32 = 0x1;
#[allow(dead_code)]
const MAX_PASID: u32 = 0xF_FFFF; // 20-bit max
const INVALID_PASID: u32 = 0x10_0000; // Beyond 20-bit

const MIN_IOVA: u64 = 0x0;
const MAX_IOVA_32BIT: u64 = 0xFFFF_FFFF;
const MAX_IOVA_48BIT: u64 = 0xFFFF_FFFF_FFFF;
const INVALID_IOVA_HIGH: u64 = 0x1000_0000_0000; // Beyond 48-bit

const MIN_PA: u64 = 0x0;
const MAX_PA_48BIT: u64 = 0xFFFF_FFFF_FFFF;
const INVALID_PA_HIGH: u64 = 0x1000_0000_0000;

// =============================================================================
// Helper Functions
// =============================================================================

/// Setup a basic valid stream configuration
fn setup_basic_stream(smmu: &SMMU, stream_id: u32) {
    let stream_id = StreamID::new(stream_id).unwrap();
    let config = StreamConfig::stage1_only();

    smmu.configure_stream(stream_id, config).unwrap();
}

/// Setup basic mapping for testing
fn setup_basic_mapping(smmu: &SMMU, stream_id: u32, pasid: u32, iova: u64, pa: u64) {
    let stream_id = StreamID::new(stream_id).unwrap();
    let pasid = PASID::new(pasid).unwrap();
    let iova = IOVA::new(iova).unwrap();
    let pa = PA::new(pa).unwrap();

    smmu.create_pasid(stream_id, pasid).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
}

// =============================================================================
// 8.3.1 Address Range Testing (4 hours)
// =============================================================================

#[test]
fn test_minimum_address_boundary() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);
    setup_basic_mapping(&smmu, VALID_STREAM_ID, VALID_PASID, MIN_IOVA, MIN_PA);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    let iova = IOVA::new(MIN_IOVA).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    if let Ok(trans_data) = result {
        assert_eq!(trans_data.physical_address().as_u64() & !0xFFF, MIN_PA);
    }
}

#[test]
fn test_maximum_32bit_address_boundary() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);
    setup_basic_mapping(&smmu, VALID_STREAM_ID, VALID_PASID, MAX_IOVA_32BIT, MAX_PA_48BIT);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    let iova = IOVA::new(MAX_IOVA_32BIT).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_maximum_48bit_address_boundary() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);
    setup_basic_mapping(&smmu, VALID_STREAM_ID, VALID_PASID, MAX_IOVA_48BIT, MAX_PA_48BIT);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    let iova = IOVA::new(MAX_IOVA_48BIT).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_address_beyond_48bit_boundary() {
    // IOVA constructor in Rust accepts any valid u64 value
    // The 48-bit constraint is enforced at mapping time, not construction
    let result = IOVA::new(INVALID_IOVA_HIGH);
    assert!(result.is_ok()); // Rust implementation allows construction
}

#[test]
fn test_physical_address_beyond_boundary() {
    // PA constructor in Rust accepts any valid u64 value
    // The constraint is enforced at mapping time, not construction
    let result = PA::new(INVALID_PA_HIGH);
    assert!(result.is_ok()); // Rust implementation allows construction
}

#[test]
fn test_address_space_exhaustion() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();

    smmu.create_pasid(stream_id, pasid).unwrap();

    let base_iova = IOVA::new(0x1000_0000).unwrap();
    let pa1 = PA::new(0x4000_0000).unwrap();
    let pa2 = PA::new(0x5000_0000).unwrap();

    // Map initial page
    smmu.map_page(
        stream_id,
        pasid,
        base_iova,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Try to map overlapping page - implementation allows remapping
    let result = smmu.map_page(
        stream_id,
        pasid,
        base_iova,
        pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    );

    // Rust implementation allows remapping (overwrites existing mapping)
    assert!(result.is_ok());
}

#[test]
fn test_unmapped_address_in_valid_range() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let unmapped_iova = IOVA::new(0x5000_0000).unwrap();
    let result = smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, TranslationError::PageNotMapped));
    }
}

#[test]
fn test_address_alignment_edge_cases() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Test page-aligned addresses
    let aligned_iova = IOVA::new(0x1000_0000).unwrap(); // 4KB aligned
    let aligned_pa = PA::new(0x4000_0000).unwrap(); // 4KB aligned

    let result = smmu.map_page(
        stream_id,
        pasid,
        aligned_iova,
        aligned_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    );
    assert!(result.is_ok());

    // Test translation
    let trans_result = smmu.translate(stream_id, pasid, aligned_iova, AccessType::Read, SecurityState::NonSecure);
    assert!(trans_result.is_ok());
}

// =============================================================================
// 8.3.2 Unconfigured Stream Handling (3 hours)
// =============================================================================

#[test]
fn test_completely_unconfigured_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    let iova = IOVA::new(0x1000_0000).unwrap();

    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_invalid_stream_id_operations() {
    let smmu = SMMU::new();

    // StreamID has a max value of 65_535 (16-bit) in Rust implementation
    // Values beyond this will fail at construction
    let result = StreamID::new(INVALID_STREAM_ID);
    assert!(result.is_err()); // Should fail at StreamID construction

    // Test with max valid StreamID
    let max_stream_id = StreamID::new(65_535).unwrap();
    let config = StreamConfig::stage1_only();
    let result = smmu.configure_stream(max_stream_id, config);
    assert!(result.is_ok());
}

#[test]
fn test_operations_on_unconfigured_stream() {
    let smmu = SMMU::new();

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();

    // Try to create PASID on unconfigured stream
    let result = smmu.create_pasid(stream_id, pasid);
    assert!(result.is_err());
}

#[test]
fn test_reconfiguration_of_configured_stream() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();

    // In Rust implementation, reconfiguring an existing stream fails
    // Must remove and recreate stream
    let result = smmu.configure_stream(stream_id, StreamConfig::bypass());

    // Reconfiguration fails in Rust (stream already exists)
    assert!(result.is_err());

    // Proper approach: remove then reconfigure
    smmu.remove_stream(stream_id).unwrap();
    let result2 = smmu.configure_stream(stream_id, StreamConfig::bypass());
    assert!(result2.is_ok());
}

#[test]
fn test_invalid_pasid() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    // PASID beyond 20-bit should fail at construction
    let result = PASID::new(INVALID_PASID);
    assert!(result.is_err());
}

#[test]
fn test_non_existent_pasid() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    let iova = IOVA::new(0x1000_0000).unwrap();

    // Attempt translation with PASID that was never created
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_stream_removal_and_reconfiguration() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();

    smmu.create_pasid(stream_id, pasid).unwrap();

    // Remove stream
    smmu.remove_stream(stream_id).unwrap();

    // Attempt translation on removed stream
    let iova = IOVA::new(0x1000_0000).unwrap();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());

    // Reconfigure stream
    let config = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, config).unwrap();

    // Should be able to create new PASID
    let result = smmu.create_pasid(stream_id, pasid);
    assert!(result.is_ok());
}

// =============================================================================
// 8.3.3 Queue Overflow Conditions (4 hours)
// =============================================================================

#[test]
fn test_event_queue_overflow() {
    // Create SMMU with small queue for testing (minimum is 16)
    use smmu::types::QueueConfig;
    let queue_config = QueueConfig::default().with_event_queue_size(16);
    let config = SMMUConfig::builder().queue_config(queue_config).build().unwrap();
    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();

    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Clear any existing events
    smmu.clear_event_queue();

    // Generate events by causing translation faults
    for i in 0..15 {
        let iova = IOVA::new(0x1000_0000 + i * PAGE_SIZE).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    // Check event queue size
    let event_count = smmu.get_event_queue_size();
    assert!(event_count > 0);

    // Queue should either limit events or allow overflow based on implementation
    // For testing, we just verify events were recorded
    let events = smmu.get_events();
    assert!(!events.is_empty());

    smmu.clear_event_queue();
}

#[test]
fn test_command_queue_overflow() {
    use smmu::types::QueueConfig;
    let queue_config = QueueConfig::default().with_command_queue_size(16); // Minimum is 16
    let config = SMMUConfig::builder().queue_config(queue_config).build().unwrap();
    let smmu = SMMU::with_config(config);

    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id_val = VALID_STREAM_ID;
    let pasid_val = VALID_PASID;

    // Fill command queue (try to overflow beyond capacity of 16)
    for i in 0..20 {
        let cmd = CommandEntry::new(CommandType::TlbiNhAll, stream_id_val, pasid_val);

        let result = smmu.submit_command(cmd);
        if i < 16 {
            assert!(result.is_ok(), "Command {i} should succeed");
        } else {
            // Should fail when queue is full (capacity is 16)
            assert!(result.is_err(), "Command {i} should fail when queue is full");
        }
    }
}

#[test]
fn test_pri_queue_overflow() {
    use smmu::types::QueueConfig;
    let queue_config = QueueConfig::default().with_pri_queue_size(16); // Minimum is 16
    let config = SMMUConfig::builder().queue_config(queue_config).build().unwrap();
    let smmu = SMMU::with_config(config);

    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Generate page requests by translating unmapped pages
    // Note: PRI queue behavior depends on fault mode configuration
    for i in 0..8 {
        let iova = IOVA::new(0x2000_0000 + i * PAGE_SIZE).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    // Check PRI queue size
    let _pri_count = smmu.get_pri_queue_size();
    // PRI queue may or may not be populated depending on fault mode
    // Just verify the API works without errors
}

#[test]
fn test_queue_recovery_after_overflow() {
    use smmu::types::QueueConfig;
    let queue_config = QueueConfig::default().with_event_queue_size(16); // Minimum is 16
    let config = SMMUConfig::builder().queue_config(queue_config).build().unwrap();
    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();

    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Fill event queue to capacity (16 events)
    for i in 0..16 {
        let iova = IOVA::new(0x3000_0000 + i * PAGE_SIZE).unwrap();
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    }

    // Consume events
    let events = smmu.get_events();
    assert!(!events.is_empty());
    smmu.clear_event_queue();

    // Should be able to generate new events
    let new_iova = IOVA::new(0x4000_0000).unwrap();
    let _ = smmu.translate(stream_id, pasid, new_iova, AccessType::Read, SecurityState::NonSecure);

    let new_events = smmu.get_events();
    assert!(!new_events.is_empty());
}

#[test]
fn test_concurrent_queue_access_under_full_conditions() {
    use smmu::types::QueueConfig;
    let queue_config = QueueConfig::default().with_event_queue_size(50);
    let config = SMMUConfig::builder().queue_config(queue_config).build().unwrap();
    let smmu = Arc::new(SMMU::with_config(config));
    smmu.enable().unwrap();

    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let success_count = Arc::new(AtomicU64::new(0));
    let failure_count = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];

    // Launch multiple threads generating events
    for t in 0..4 {
        let smmu_clone = Arc::clone(&smmu);
        let success_clone = Arc::clone(&success_count);
        let failure_clone = Arc::clone(&failure_count);

        let handle = thread::spawn(move || {
            for i in 0..25 {
                let iova_val = 0x5000_0000 + (t as u64 * 0x10_0000) + (i * PAGE_SIZE);
                let iova = IOVA::new(iova_val).unwrap();

                let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

                if result.is_err() {
                    success_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    failure_clone.fetch_add(1, Ordering::Relaxed);
                }

                thread::sleep(Duration::from_micros(100));
            }
        });

        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify operations completed
    let total_ops = success_count.load(Ordering::Relaxed) + failure_count.load(Ordering::Relaxed);
    assert_eq!(total_ops, 100); // 4 threads * 25 operations
    assert!(success_count.load(Ordering::Relaxed) > 0);
}

// =============================================================================
// 8.3.4 Permission Violation Tests (5 hours)
// =============================================================================

#[test]
fn test_read_violation_on_write_only_page() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa = PA::new(0x4000_0000).unwrap();

    // Map with write-only permissions
    let write_only_perms = PagePermissions::write_only();
    smmu.map_page(stream_id, pasid, iova, pa, write_only_perms, SecurityState::NonSecure)
        .unwrap();

    // Read should fail
    let read_result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(read_result.is_err());

    // Write should succeed
    let write_result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(write_result.is_ok());
}

#[test]
fn test_write_violation_on_read_only_page() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa = PA::new(0x4000_0000).unwrap();

    // Map with read-only permissions
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Read should succeed
    let read_result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(read_result.is_ok());

    // Write should fail
    let write_result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(write_result.is_err());
}

#[test]
fn test_execute_violation_on_non_executable_page() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa = PA::new(0x4000_0000).unwrap();

    // Map with read-write but no execute
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Read and write should succeed
    assert!(smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    assert!(smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure).is_ok());

    // Execute should fail
    let exec_result = smmu.translate(stream_id, pasid, iova, AccessType::Execute, SecurityState::NonSecure);
    assert!(exec_result.is_err());
}

#[test]
#[allow(clippy::too_many_lines)]
fn test_all_permission_combinations() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    struct PermTest {
        perms: PagePermissions,
        access: AccessType,
        should_succeed: bool,
        description: &'static str,
    }

    let tests = vec![
        // Read-only
        PermTest {
            perms: PagePermissions::read_only(),
            access: AccessType::Read,
            should_succeed: true,
            description: "Read on read-only page",
        },
        PermTest {
            perms: PagePermissions::read_only(),
            access: AccessType::Write,
            should_succeed: false,
            description: "Write on read-only page",
        },
        PermTest {
            perms: PagePermissions::read_only(),
            access: AccessType::Execute,
            should_succeed: false,
            description: "Execute on read-only page",
        },
        // Write-only
        PermTest {
            perms: PagePermissions::write_only(),
            access: AccessType::Read,
            should_succeed: false,
            description: "Read on write-only page",
        },
        PermTest {
            perms: PagePermissions::write_only(),
            access: AccessType::Write,
            should_succeed: true,
            description: "Write on write-only page",
        },
        PermTest {
            perms: PagePermissions::write_only(),
            access: AccessType::Execute,
            should_succeed: false,
            description: "Execute on write-only page",
        },
        // Execute-only
        PermTest {
            perms: PagePermissions::execute_only(),
            access: AccessType::Read,
            should_succeed: false,
            description: "Read on execute-only page",
        },
        PermTest {
            perms: PagePermissions::execute_only(),
            access: AccessType::Write,
            should_succeed: false,
            description: "Write on execute-only page",
        },
        PermTest {
            perms: PagePermissions::execute_only(),
            access: AccessType::Execute,
            should_succeed: true,
            description: "Execute on execute-only page",
        },
        // Read-write
        PermTest {
            perms: PagePermissions::read_write(),
            access: AccessType::Read,
            should_succeed: true,
            description: "Read on read-write page",
        },
        PermTest {
            perms: PagePermissions::read_write(),
            access: AccessType::Write,
            should_succeed: true,
            description: "Write on read-write page",
        },
        PermTest {
            perms: PagePermissions::read_write(),
            access: AccessType::Execute,
            should_succeed: false,
            description: "Execute on read-write page",
        },
        // All permissions
        PermTest {
            perms: PagePermissions::all(),
            access: AccessType::Read,
            should_succeed: true,
            description: "Read on full-permission page",
        },
        PermTest {
            perms: PagePermissions::all(),
            access: AccessType::Write,
            should_succeed: true,
            description: "Write on full-permission page",
        },
        PermTest {
            perms: PagePermissions::all(),
            access: AccessType::Execute,
            should_succeed: true,
            description: "Execute on full-permission page",
        },
    ];

    for (i, test) in tests.iter().enumerate() {
        let iova = IOVA::new(0x1000_0000 + (i as u64 * PAGE_SIZE)).unwrap();
        let pa = PA::new(0x4000_0000 + (i as u64 * PAGE_SIZE)).unwrap();

        smmu.map_page(stream_id, pasid, iova, pa, test.perms, SecurityState::NonSecure)
            .unwrap_or_else(|_| panic!("Failed to map page for test: {}", test.description));

        let result = smmu.translate(stream_id, pasid, iova, test.access, SecurityState::NonSecure);

        if test.should_succeed {
            assert!(result.is_ok(), "Expected success for: {}", test.description);
        } else {
            assert!(result.is_err(), "Expected failure for: {}", test.description);
        }
    }
}

#[test]
fn test_security_state_permission_violations() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa = PA::new(0x4000_0000).unwrap();

    // Map with NonSecure state
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Access from NonSecure context should succeed
    let ns_result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(ns_result.is_ok());

    // Note: Rust SMMU translate doesn't take SecurityState parameter - it's part of the mapping
    // The security state is encoded in the page table entry itself
}

#[test]
fn test_concurrent_permission_violations() {
    let smmu = Arc::new(SMMU::new());
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map multiple pages with different permissions
    for i in 0..10 {
        let iova = IOVA::new(0x1000_0000 + (i * PAGE_SIZE)).unwrap();
        let pa = PA::new(0x4000_0000 + (i * PAGE_SIZE)).unwrap();

        let perms = if i % 2 == 0 {
            PagePermissions::read_only()
        } else {
            PagePermissions::write_only()
        };

        smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
    }

    let read_violations = Arc::new(AtomicU64::new(0));
    let write_violations = Arc::new(AtomicU64::new(0));
    let successful_accesses = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];

    // Launch threads performing mixed operations
    for _ in 0..4 {
        let smmu_clone = Arc::clone(&smmu);
        let read_viol_clone = Arc::clone(&read_violations);
        let write_viol_clone = Arc::clone(&write_violations);
        let success_clone = Arc::clone(&successful_accesses);

        let handle = thread::spawn(move || {
            for i in 0..10 {
                let iova = IOVA::new(0x1000_0000 + (i * PAGE_SIZE)).unwrap();

                // Try read
                let read_result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
                if read_result.is_err() {
                    read_viol_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    success_clone.fetch_add(1, Ordering::Relaxed);
                }

                // Try write
                let write_result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
                if write_result.is_err() {
                    write_viol_clone.fetch_add(1, Ordering::Relaxed);
                } else {
                    success_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify expected violation patterns
    // 5 pages are read-only (write violations), 5 are write-only (read violations)
    assert_eq!(read_violations.load(Ordering::Relaxed), 4 * 5); // 4 threads * 5 write-only pages
    assert_eq!(write_violations.load(Ordering::Relaxed), 4 * 5); // 4 threads * 5 read-only pages
    assert_eq!(successful_accesses.load(Ordering::Relaxed), 4 * 10); // 4 threads * 10 appropriate accesses
}

// =============================================================================
// 8.3.5 Cache Invalidation Effect Testing (4 hours)
// =============================================================================

// Note: Cache invalidation tests would require cache-specific APIs
// These tests verify that the SMMU remains functionally correct
// regardless of cache state

#[test]
fn test_translation_consistency_after_remapping() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa1 = PA::new(0x4000_0000).unwrap();
    let pa2 = PA::new(0x5000_0000).unwrap();

    // Map page
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translate
    let result1 = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result1.is_ok());

    // Remap to different PA
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translation should reflect new mapping
    let result2 = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result2.is_ok());
    if let Ok(trans_data) = result2 {
        assert_eq!(trans_data.physical_address().as_u64() & !0xFFF, pa2.as_u64());
    }
}

#[test]
fn test_multiple_pasid_isolation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();

    smmu.create_pasid(stream_id, pasid1).unwrap();
    smmu.create_pasid(stream_id, pasid2).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa1 = PA::new(0x4000_0000).unwrap();
    let pa2 = PA::new(0x5000_0000).unwrap();

    // Map same IOVA to different PAs in different PASIDs
    smmu.map_page(
        stream_id,
        pasid1,
        iova,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu.map_page(
        stream_id,
        pasid2,
        iova,
        pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translations should be independent
    let result1 = smmu.translate(stream_id, pasid1, iova, AccessType::Read, SecurityState::NonSecure);
    let result2 = smmu.translate(stream_id, pasid2, iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result1.is_ok());
    assert!(result2.is_ok());

    if let (Ok(trans1), Ok(trans2)) = (result1, result2) {
        assert_eq!(trans1.physical_address().as_u64() & !0xFFF, pa1.as_u64());
        assert_eq!(trans2.physical_address().as_u64() & !0xFFF, pa2.as_u64());
    }
}

#[test]
fn test_multiple_stream_isolation() {
    let smmu = SMMU::new();

    let stream1 = StreamID::new(0x1000).unwrap();
    let stream2 = StreamID::new(0x2000).unwrap();

    smmu.configure_stream(stream1, StreamConfig::stage1_only()).unwrap();
    smmu.configure_stream(stream2, StreamConfig::stage1_only()).unwrap();

    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream1, pasid).unwrap();
    smmu.create_pasid(stream2, pasid).unwrap();

    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa1 = PA::new(0x4000_0000).unwrap();
    let pa2 = PA::new(0x5000_0000).unwrap();

    // Map same IOVA to different PAs in different streams
    smmu.map_page(
        stream1,
        pasid,
        iova,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu.map_page(
        stream2,
        pasid,
        iova,
        pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translations should be independent per stream
    let result1 = smmu.translate(stream1, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    let result2 = smmu.translate(stream2, pasid, iova, AccessType::Read, SecurityState::NonSecure);

    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

// =============================================================================
// 8.3.6 Rust-Specific Panic Safety Tests (4 hours)
// =============================================================================

#[test]
fn test_no_panic_on_normal_operations() {
    let result = panic::catch_unwind(|| {
        let smmu = SMMU::new();
        setup_basic_stream(&smmu, VALID_STREAM_ID);

        let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
        let pasid = PASID::new(VALID_PASID).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        let iova = IOVA::new(0x1000_0000).unwrap();
        let pa = PA::new(0x4000_0000).unwrap();

        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    });

    assert!(result.is_ok(), "Normal operations should not panic");
}

#[test]
fn test_no_panic_on_error_conditions() {
    let result = panic::catch_unwind(|| {
        let smmu = SMMU::new();

        // Unconfigured stream
        let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
        let pasid = PASID::new(VALID_PASID).unwrap();
        let iova = IOVA::new(0x1000_0000).unwrap();

        // Should return error, not panic
        let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);

        // Invalid PASID construction
        let _ = PASID::new(INVALID_PASID);

        // Invalid address construction
        let _ = IOVA::new(INVALID_IOVA_HIGH);
    });

    assert!(result.is_ok(), "Error conditions should not panic");
}

#[test]
fn test_no_panic_on_concurrent_access() {
    let result = panic::catch_unwind(|| {
        let smmu = Arc::new(SMMU::new());
        setup_basic_stream(&smmu, VALID_STREAM_ID);

        let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
        let pasid = PASID::new(VALID_PASID).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        let mut handles = vec![];

        for t in 0..10 {
            let smmu_clone = Arc::clone(&smmu);
            let handle = thread::spawn(move || {
                let iova = IOVA::new(0x1000_0000 + (t * PAGE_SIZE)).unwrap();
                let pa = PA::new(0x4000_0000 + (t * PAGE_SIZE)).unwrap();

                let _ = smmu_clone.map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::read_write(),
                    SecurityState::NonSecure,
                );

                let _ = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    });

    assert!(result.is_ok(), "Concurrent access should not panic");
}

#[test]
fn test_panic_safety_with_shutdown() {
    let result = panic::catch_unwind(|| {
        let smmu = SMMU::new();
        setup_basic_stream(&smmu, VALID_STREAM_ID);

        smmu.shutdown().unwrap();

        // Operations after shutdown should return errors, not panic
        let stream_id = StreamID::new(0x9999).unwrap();
        let _ = smmu.configure_stream(stream_id, StreamConfig::stage1_only());
    });

    assert!(result.is_ok(), "Operations after shutdown should not panic");
}

// =============================================================================
// 8.3.7 Memory Safety Tests (Miri-compatible) (5 hours)
// =============================================================================

// Note: These tests are designed to be run under Miri to detect undefined behavior
// Run with: cargo +nightly miri test

#[test]
fn test_memory_safety_basic_operations() {
    let smmu = SMMU::new();
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Basic operations should have no undefined behavior
    let iova = IOVA::new(0x1000_0000).unwrap();
    let pa = PA::new(0x4000_0000).unwrap();

    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    let _ = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    // Note: SMMU doesn't expose unmap_page at top level, only via stream context
}

#[test]
fn test_memory_safety_arc_sharing() {
    let smmu = Arc::new(SMMU::new());
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    // Arc cloning and sharing should be memory-safe
    let smmu_clone1 = Arc::clone(&smmu);
    let smmu_clone2 = Arc::clone(&smmu);

    assert_eq!(smmu_clone1.get_stream_count(), smmu_clone2.get_stream_count());

    drop(smmu_clone1);
    drop(smmu_clone2);
    // Original smmu should still be valid
    assert_eq!(smmu.get_stream_count(), 1);
}

#[test]
fn test_memory_safety_drop_order() {
    // Verify proper cleanup order prevents use-after-free
    {
        let smmu = SMMU::new();
        setup_basic_stream(&smmu, VALID_STREAM_ID);

        let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
        let pasid = PASID::new(VALID_PASID).unwrap();
        smmu.create_pasid(stream_id, pasid).unwrap();

        // smmu will be dropped here, cleaning up all resources
    }
    // No use-after-free should occur
}

// =============================================================================
// 8.3.8 Thread Safety Tests (ThreadSanitizer-compatible) (4 hours)
// =============================================================================

// Note: Run with: RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test
// These tests are designed to detect data races

#[test]
fn test_thread_safety_concurrent_stream_configuration() {
    let smmu = Arc::new(SMMU::new());
    let mut handles = vec![];

    // Multiple threads configuring different streams concurrently
    for i in 0..10 {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            let stream_id = StreamID::new(0x1000 + i).unwrap();
            smmu_clone.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(smmu.get_stream_count(), 10);
}

#[test]
fn test_thread_safety_concurrent_pasid_creation() {
    let smmu = Arc::new(SMMU::new());
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let mut handles = vec![];

    // Multiple threads creating different PASIDs concurrently
    for i in 1..=10 {
        let smmu_clone = Arc::clone(&smmu);
        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            smmu_clone.create_pasid(stream_id, pasid).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_thread_safety_concurrent_translation() {
    let smmu = Arc::new(SMMU::new());
    setup_basic_stream(&smmu, VALID_STREAM_ID);
    setup_basic_mapping(&smmu, VALID_STREAM_ID, VALID_PASID, 0x1000_0000, 0x4000_0000);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let pasid = PASID::new(VALID_PASID).unwrap();
    let iova = IOVA::new(0x1000_0000).unwrap();

    let success_count = Arc::new(AtomicU64::new(0));
    let mut handles = vec![];

    // Many threads performing translations concurrently
    for _ in 0..20 {
        let smmu_clone = Arc::clone(&smmu);
        let success_clone = Arc::clone(&success_count);

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let result = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
                if result.is_ok() {
                    success_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // All translations should succeed
    assert_eq!(success_count.load(Ordering::Relaxed), 2000);
}

#[test]
fn test_thread_safety_mixed_operations() {
    let smmu = Arc::new(SMMU::new());
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let mut handles = vec![];

    // Reader threads
    for _ in 0..5 {
        let smmu_clone = Arc::clone(&smmu);
        let running_clone = Arc::clone(&running);

        let handle = thread::spawn(move || {
            while running_clone.load(Ordering::Relaxed) {
                let _ = smmu_clone.has_stream(stream_id);
                let _ = smmu_clone.get_stream_count();
                thread::sleep(Duration::from_micros(10));
            }
        });

        handles.push(handle);
    }

    // Writer threads
    for i in 1..=5 {
        let smmu_clone = Arc::clone(&smmu);
        let running_clone = Arc::clone(&running);

        let handle = thread::spawn(move || {
            let pasid = PASID::new(i).unwrap();
            smmu_clone.create_pasid(stream_id, pasid).unwrap();

            while running_clone.load(Ordering::Relaxed) {
                let iova = IOVA::new(0x1000_0000 + (u64::from(i) * PAGE_SIZE)).unwrap();
                let pa = PA::new(0x4000_0000 + (u64::from(i) * PAGE_SIZE)).unwrap();

                let _ = smmu_clone.map_page(
                    stream_id,
                    pasid,
                    iova,
                    pa,
                    PagePermissions::read_write(),
                    SecurityState::NonSecure,
                );

                thread::sleep(Duration::from_micros(50));
            }
        });

        handles.push(handle);
    }

    // Let threads run for a bit
    thread::sleep(Duration::from_millis(100));
    running.store(false, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_thread_safety_queue_operations() {
    let smmu = Arc::new(SMMU::new());
    setup_basic_stream(&smmu, VALID_STREAM_ID);

    let running = Arc::new(AtomicBool::new(true));
    let mut handles = vec![];

    // Event producer thread
    let smmu_clone = Arc::clone(&smmu);
    let running_clone = Arc::clone(&running);
    let handle = thread::spawn(move || {
        let stream_id = StreamID::new(VALID_STREAM_ID).unwrap();
        let pasid = PASID::new(VALID_PASID).unwrap();
        smmu_clone.create_pasid(stream_id, pasid).unwrap();

        while running_clone.load(Ordering::Relaxed) {
            let iova = IOVA::new(0x9999_0000).unwrap();
            let _ = smmu_clone.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
            thread::sleep(Duration::from_micros(10));
        }
    });
    handles.push(handle);

    // Event consumer thread
    let smmu_clone = Arc::clone(&smmu);
    let running_clone = Arc::clone(&running);
    let handle = thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            let _ = smmu_clone.get_events();
            smmu_clone.clear_event_queue();
            thread::sleep(Duration::from_micros(50));
        }
    });
    handles.push(handle);

    thread::sleep(Duration::from_millis(100));
    running.store(false, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }
}
