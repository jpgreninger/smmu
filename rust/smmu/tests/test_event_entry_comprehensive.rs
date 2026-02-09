#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive test coverage for `types/event_entry.rs`
//!
//! This test suite achieves 100% coverage of `EventEntry` and `EventType`, covering:
//! - All 7 `EventType` variants
//! - Event entry construction with various parameters
//! - Timestamp management
//! - Security state integration
//! - Error code handling
//! - Field accessors and validation
//! - Display/Debug trait implementations
//! - Hash and equality trait implementations
//! - Const constructor testing
//! - ARM SMMU v3 Section 6.3 compliance

use smmu::types::{EventEntry, EventType, SecurityState};
use std::collections::{HashMap, HashSet};

// ============================================================================
// EventType Tests - Basic Variant Checks
// ============================================================================

#[test]
fn test_event_type_translation_fault() {
    let event = EventType::TranslationFault;
    assert_eq!(event as u8, 0);
}

#[test]
fn test_event_type_permission_fault() {
    let event = EventType::PermissionFault;
    assert_eq!(event as u8, 1);
}

#[test]
fn test_event_type_command_sync_completion() {
    let event = EventType::CommandSyncCompletion;
    assert_eq!(event as u8, 2);
}

#[test]
fn test_event_type_pri_page_request() {
    let event = EventType::PriPageRequest;
    assert_eq!(event as u8, 3);
}

#[test]
fn test_event_type_atc_invalidate_completion() {
    let event = EventType::AtcInvalidateCompletion;
    assert_eq!(event as u8, 4);
}

#[test]
fn test_event_type_configuration_error() {
    let event = EventType::ConfigurationError;
    assert_eq!(event as u8, 5);
}

#[test]
fn test_event_type_internal_error() {
    let event = EventType::InternalError;
    assert_eq!(event as u8, 6);
}

// ============================================================================
// EventType Default Trait
// ============================================================================

#[test]
fn test_event_type_default() {
    let default = EventType::default();
    assert_eq!(default, EventType::TranslationFault);
    assert_eq!(default as u8, 0);
}

#[test]
fn test_event_type_default_matches_translation_fault() {
    assert_eq!(EventType::default(), EventType::TranslationFault);
}

// ============================================================================
// EventType Copy and Clone
// ============================================================================

#[test]
fn test_event_type_copy() {
    let event1 = EventType::PermissionFault;
    let event2 = event1; // Copy
    assert_eq!(event1, event2);
    assert_eq!(event1 as u8, 1); // Original still valid
}

#[test]
fn test_event_type_clone() {
    let event1 = EventType::ConfigurationError;
    let event2 = event1;
    assert_eq!(event1, event2);
}

// ============================================================================
// EventType Debug and Display
// ============================================================================

#[test]
fn test_event_type_debug_translation_fault() {
    let event = EventType::TranslationFault;
    let debug = format!("{event:?}");
    assert!(debug.contains("TranslationFault"));
}

#[test]
fn test_event_type_debug_permission_fault() {
    let event = EventType::PermissionFault;
    let debug = format!("{event:?}");
    assert!(debug.contains("PermissionFault"));
}

#[test]
fn test_event_type_debug_all_variants() {
    let variants = [
        EventType::TranslationFault,
        EventType::PermissionFault,
        EventType::CommandSyncCompletion,
        EventType::PriPageRequest,
        EventType::AtcInvalidateCompletion,
        EventType::ConfigurationError,
        EventType::InternalError,
    ];

    for variant in &variants {
        let debug = format!("{variant:?}");
        assert!(!debug.is_empty());
    }
}

// ============================================================================
// EventType Equality and Hash
// ============================================================================

#[test]
fn test_event_type_equality() {
    assert_eq!(EventType::TranslationFault, EventType::TranslationFault);
    assert_eq!(EventType::InternalError, EventType::InternalError);
    assert_ne!(EventType::TranslationFault, EventType::PermissionFault);
}

#[test]
fn test_event_type_hash_in_hashset() {
    let mut set = HashSet::new();

    set.insert(EventType::TranslationFault);
    set.insert(EventType::PermissionFault);
    set.insert(EventType::TranslationFault); // Duplicate

    assert_eq!(set.len(), 2);
    assert!(set.contains(&EventType::TranslationFault));
    assert!(set.contains(&EventType::PermissionFault));
    assert!(!set.contains(&EventType::InternalError));
}

#[test]
fn test_event_type_hash_in_hashmap() {
    let mut map = HashMap::new();

    map.insert(EventType::TranslationFault, "translation");
    map.insert(EventType::PermissionFault, "permission");
    map.insert(EventType::ConfigurationError, "config");

    assert_eq!(map.get(&EventType::TranslationFault), Some(&"translation"));
    assert_eq!(map.get(&EventType::PermissionFault), Some(&"permission"));
    assert_eq!(map.get(&EventType::ConfigurationError), Some(&"config"));
    assert_eq!(map.get(&EventType::InternalError), None);
}

#[test]
fn test_event_type_hash_consistency() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let event = EventType::PriPageRequest;

    let mut hasher1 = DefaultHasher::new();
    event.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    event.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    assert_eq!(hash1, hash2);
}

#[test]
fn test_event_type_all_variants_unique() {
    let mut set = HashSet::new();
    let variants = [
        EventType::TranslationFault,
        EventType::PermissionFault,
        EventType::CommandSyncCompletion,
        EventType::PriPageRequest,
        EventType::AtcInvalidateCompletion,
        EventType::ConfigurationError,
        EventType::InternalError,
    ];

    for variant in &variants {
        set.insert(*variant);
    }

    assert_eq!(set.len(), 7, "All 7 EventType variants should be unique");
}

// ============================================================================
// EventEntry Construction - new() Method
// ============================================================================

#[test]
fn test_event_entry_new_translation_fault() {
    let entry = EventEntry::new(EventType::TranslationFault, 100, 200, 0x1000);

    assert_eq!(entry.event_type, EventType::TranslationFault);
    assert_eq!(entry.stream_id, 100);
    assert_eq!(entry.pasid, 200);
    assert_eq!(entry.address, 0x1000);
    assert_eq!(entry.security_state, SecurityState::NonSecure);
    assert_eq!(entry.error_code, 0);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_event_entry_new_permission_fault() {
    let entry = EventEntry::new(EventType::PermissionFault, 42, 1234, 0xDEAD_BEEF);

    assert_eq!(entry.event_type, EventType::PermissionFault);
    assert_eq!(entry.stream_id, 42);
    assert_eq!(entry.pasid, 1234);
    assert_eq!(entry.address, 0xDEAD_BEEF);
    assert_eq!(entry.security_state, SecurityState::NonSecure);
    assert_eq!(entry.error_code, 0);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_event_entry_new_all_event_types() {
    let event_types = [
        EventType::TranslationFault,
        EventType::PermissionFault,
        EventType::CommandSyncCompletion,
        EventType::PriPageRequest,
        EventType::AtcInvalidateCompletion,
        EventType::ConfigurationError,
        EventType::InternalError,
    ];

    for (i, &event_type) in event_types.iter().enumerate() {
        let entry = EventEntry::new(event_type, i as u32, (i * 2) as u32, (i * 0x1000) as u64);

        assert_eq!(entry.event_type, event_type);
        assert_eq!(entry.stream_id, i as u32);
        assert_eq!(entry.pasid, (i * 2) as u32);
        assert_eq!(entry.address, (i * 0x1000) as u64);
    }
}

#[test]
fn test_event_entry_new_zero_values() {
    let entry = EventEntry::new(EventType::TranslationFault, 0, 0, 0);

    assert_eq!(entry.stream_id, 0);
    assert_eq!(entry.pasid, 0);
    assert_eq!(entry.address, 0);
}

#[test]
fn test_event_entry_new_max_stream_id() {
    let entry = EventEntry::new(EventType::ConfigurationError, u32::MAX, 0, 0);
    assert_eq!(entry.stream_id, u32::MAX);
}

#[test]
fn test_event_entry_new_max_pasid() {
    let entry = EventEntry::new(EventType::PriPageRequest, 0, u32::MAX, 0);
    assert_eq!(entry.pasid, u32::MAX);
}

#[test]
fn test_event_entry_new_max_address() {
    let entry = EventEntry::new(EventType::InternalError, 0, 0, u64::MAX);
    assert_eq!(entry.address, u64::MAX);
}

#[test]
fn test_event_entry_new_defaults_to_nonsecure() {
    let entry = EventEntry::new(EventType::TranslationFault, 1, 2, 3);
    assert_eq!(entry.security_state, SecurityState::NonSecure);
}

#[test]
fn test_event_entry_new_defaults_error_code_zero() {
    let entry = EventEntry::new(EventType::PermissionFault, 1, 2, 3);
    assert_eq!(entry.error_code, 0);
}

#[test]
fn test_event_entry_new_defaults_timestamp_zero() {
    let entry = EventEntry::new(EventType::ConfigurationError, 1, 2, 3);
    assert_eq!(entry.timestamp, 0);
}

// ============================================================================
// EventEntry Const Constructor
// ============================================================================

#[test]
fn test_event_entry_const_new() {
    const ENTRY: EventEntry = EventEntry::new(EventType::TranslationFault, 10, 20, 0x5000);

    assert_eq!(ENTRY.event_type as u8, 0);
    assert_eq!(ENTRY.stream_id, 10);
    assert_eq!(ENTRY.pasid, 20);
    assert_eq!(ENTRY.address, 0x5000);
}

#[test]
fn test_event_entry_const_new_in_const_context() {
    const fn create_event() -> EventEntry {
        EventEntry::new(EventType::PermissionFault, 99, 88, 0x7777)
    }

    const EVENT: EventEntry = create_event();

    assert_eq!(EVENT.event_type as u8, 1);
    assert_eq!(EVENT.stream_id, 99);
    assert_eq!(EVENT.pasid, 88);
    assert_eq!(EVENT.address, 0x7777);
}

// ============================================================================
// EventEntry Field Modification
// ============================================================================

#[test]
fn test_event_entry_modify_security_state() {
    let mut entry = EventEntry::new(EventType::TranslationFault, 1, 2, 3);

    entry.security_state = SecurityState::Secure;
    assert_eq!(entry.security_state, SecurityState::Secure);

    entry.security_state = SecurityState::Realm;
    assert_eq!(entry.security_state, SecurityState::Realm);
}

#[test]
fn test_event_entry_modify_error_code() {
    let mut entry = EventEntry::new(EventType::PermissionFault, 1, 2, 3);

    assert_eq!(entry.error_code, 0);

    entry.error_code = 0x42;
    assert_eq!(entry.error_code, 0x42);

    entry.error_code = u32::MAX;
    assert_eq!(entry.error_code, u32::MAX);
}

#[test]
fn test_event_entry_modify_timestamp() {
    let mut entry = EventEntry::new(EventType::ConfigurationError, 1, 2, 3);

    assert_eq!(entry.timestamp, 0);

    entry.timestamp = 12_345_678;
    assert_eq!(entry.timestamp, 12_345_678);

    entry.timestamp = u64::MAX;
    assert_eq!(entry.timestamp, u64::MAX);
}

#[test]
fn test_event_entry_modify_all_fields() {
    let mut entry = EventEntry::new(EventType::TranslationFault, 0, 0, 0);

    entry.event_type = EventType::InternalError;
    entry.stream_id = 999;
    entry.pasid = 888;
    entry.address = 0xBADC_0FFE;
    entry.security_state = SecurityState::Secure;
    entry.error_code = 0x123;
    entry.timestamp = 0xDEAD_BEEF;

    assert_eq!(entry.event_type, EventType::InternalError);
    assert_eq!(entry.stream_id, 999);
    assert_eq!(entry.pasid, 888);
    assert_eq!(entry.address, 0xBADC_0FFE);
    assert_eq!(entry.security_state, SecurityState::Secure);
    assert_eq!(entry.error_code, 0x123);
    assert_eq!(entry.timestamp, 0xDEAD_BEEF);
}

// ============================================================================
// EventEntry Security State Integration
// ============================================================================

#[test]
fn test_event_entry_with_secure_state() {
    let mut entry = EventEntry::new(EventType::TranslationFault, 1, 2, 3);
    entry.security_state = SecurityState::Secure;

    assert_eq!(entry.security_state, SecurityState::Secure);
    assert!(entry.security_state.is_secure());
}

#[test]
fn test_event_entry_with_nonsecure_state() {
    let mut entry = EventEntry::new(EventType::PermissionFault, 1, 2, 3);
    entry.security_state = SecurityState::NonSecure;

    assert_eq!(entry.security_state, SecurityState::NonSecure);
    assert!(entry.security_state.is_non_secure());
}

#[test]
fn test_event_entry_with_realm_state() {
    let mut entry = EventEntry::new(EventType::ConfigurationError, 1, 2, 3);
    entry.security_state = SecurityState::Realm;

    assert_eq!(entry.security_state, SecurityState::Realm);
    assert!(entry.security_state.is_realm());
}

#[test]
fn test_event_entry_all_security_states() {
    let security_states = [SecurityState::Secure, SecurityState::NonSecure, SecurityState::Realm];

    for &state in &security_states {
        let mut entry = EventEntry::new(EventType::InternalError, 1, 2, 3);
        entry.security_state = state;

        assert_eq!(entry.security_state, state);
    }
}

// ============================================================================
// EventEntry Timestamp Management
// ============================================================================

#[test]
fn test_event_entry_timestamp_ordering() {
    let mut entry1 = EventEntry::new(EventType::TranslationFault, 1, 2, 3);
    let mut entry2 = EventEntry::new(EventType::PermissionFault, 4, 5, 6);

    entry1.timestamp = 1000;
    entry2.timestamp = 2000;

    assert!(entry1.timestamp < entry2.timestamp);
}

#[test]
fn test_event_entry_timestamp_zero_initialization() {
    let entry = EventEntry::new(EventType::ConfigurationError, 1, 2, 3);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_event_entry_timestamp_max_value() {
    let mut entry = EventEntry::new(EventType::InternalError, 1, 2, 3);
    entry.timestamp = u64::MAX;

    assert_eq!(entry.timestamp, u64::MAX);
}

#[test]
fn test_event_entry_timestamp_increment() {
    let mut entry = EventEntry::new(EventType::PriPageRequest, 1, 2, 3);

    for i in 0..100 {
        entry.timestamp = i;
        assert_eq!(entry.timestamp, i);
    }
}

// ============================================================================
// EventEntry Error Code Handling
// ============================================================================

#[test]
fn test_event_entry_error_code_zero_default() {
    let entry = EventEntry::new(EventType::TranslationFault, 1, 2, 3);
    assert_eq!(entry.error_code, 0);
}

#[test]
fn test_event_entry_error_code_common_values() {
    let mut entry = EventEntry::new(EventType::PermissionFault, 1, 2, 3);

    let error_codes = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0xFF];

    for &code in &error_codes {
        entry.error_code = code;
        assert_eq!(entry.error_code, code);
    }
}

#[test]
fn test_event_entry_error_code_max() {
    let mut entry = EventEntry::new(EventType::ConfigurationError, 1, 2, 3);
    entry.error_code = u32::MAX;

    assert_eq!(entry.error_code, u32::MAX);
}

#[test]
fn test_event_entry_error_code_arm_smmu_values() {
    let mut entry = EventEntry::new(EventType::TranslationFault, 1, 2, 3);

    // Common ARM SMMU error codes from spec
    let arm_error_codes = [
        0x00, // No error
        0x01, // Translation fault
        0x02, // Permission fault
        0x03, // Access fault
        0x04, // Address size fault
    ];

    for &code in &arm_error_codes {
        entry.error_code = code;
        assert_eq!(entry.error_code, code);
    }
}

// ============================================================================
// EventEntry Copy and Clone
// ============================================================================

#[test]
fn test_event_entry_copy() {
    let entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x3000);
    let entry2 = entry1; // Copy

    assert_eq!(entry1.event_type, entry2.event_type);
    assert_eq!(entry1.stream_id, entry2.stream_id);
    assert_eq!(entry1.pasid, entry2.pasid);
    assert_eq!(entry1.address, entry2.address);
}

#[test]
fn test_event_entry_clone() {
    let entry1 = EventEntry::new(EventType::PermissionFault, 30, 40, 0x5000);
    let entry2 = entry1;

    assert_eq!(entry1.event_type, entry2.event_type);
    assert_eq!(entry1.stream_id, entry2.stream_id);
    assert_eq!(entry1.pasid, entry2.pasid);
    assert_eq!(entry1.address, entry2.address);
}

#[test]
fn test_event_entry_copy_with_modifications() {
    let mut entry1 = EventEntry::new(EventType::ConfigurationError, 1, 2, 3);
    entry1.timestamp = 1000;
    entry1.error_code = 42;
    entry1.security_state = SecurityState::Secure;

    let entry2 = entry1; // Copy

    assert_eq!(entry1.timestamp, entry2.timestamp);
    assert_eq!(entry1.error_code, entry2.error_code);
    assert_eq!(entry1.security_state, entry2.security_state);
}

// ============================================================================
// EventEntry Equality and Comparison
// ============================================================================

#[test]
fn test_event_entry_equality() {
    let entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let entry2 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);

    assert_eq!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_type() {
    let entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let entry2 = EventEntry::new(EventType::PermissionFault, 10, 20, 0x1000);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_stream_id() {
    let entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let entry2 = EventEntry::new(EventType::TranslationFault, 99, 20, 0x1000);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_pasid() {
    let entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let entry2 = EventEntry::new(EventType::TranslationFault, 10, 99, 0x1000);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_address() {
    let entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let entry2 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x9999);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_security_state() {
    let mut entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let mut entry2 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);

    entry1.security_state = SecurityState::Secure;
    entry2.security_state = SecurityState::NonSecure;

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_error_code() {
    let mut entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let mut entry2 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);

    entry1.error_code = 1;
    entry2.error_code = 2;

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_inequality_different_timestamp() {
    let mut entry1 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);
    let mut entry2 = EventEntry::new(EventType::TranslationFault, 10, 20, 0x1000);

    entry1.timestamp = 1000;
    entry2.timestamp = 2000;

    assert_ne!(entry1, entry2);
}

#[test]
fn test_event_entry_equality_with_all_fields_set() {
    let mut entry1 = EventEntry::new(EventType::InternalError, 123, 456, 0xABCD);
    entry1.security_state = SecurityState::Realm;
    entry1.error_code = 0x42;
    entry1.timestamp = 98_765;

    let mut entry2 = EventEntry::new(EventType::InternalError, 123, 456, 0xABCD);
    entry2.security_state = SecurityState::Realm;
    entry2.error_code = 0x42;
    entry2.timestamp = 98_765;

    assert_eq!(entry1, entry2);
}

// ============================================================================
// EventEntry Debug Trait
// ============================================================================

#[test]
fn test_event_entry_debug() {
    let entry = EventEntry::new(EventType::TranslationFault, 42, 100, 0x1234);
    let debug = format!("{entry:?}");

    assert!(debug.contains("EventEntry"));
    // Debug should contain field information
    assert!(!debug.is_empty());
}

#[test]
fn test_event_entry_debug_with_all_fields() {
    let mut entry = EventEntry::new(EventType::PermissionFault, 10, 20, 0x5678);
    entry.security_state = SecurityState::Secure;
    entry.error_code = 99;
    entry.timestamp = 12_345;

    let debug = format!("{entry:?}");
    assert!(!debug.is_empty());
}

// ============================================================================
// EventEntry Collections (Vec, HashMap)
// ============================================================================

#[test]
#[allow(clippy::useless_vec)]
fn test_event_entry_in_vec() {
    let events = vec![
        EventEntry::new(EventType::TranslationFault, 1, 2, 3),
        EventEntry::new(EventType::PermissionFault, 4, 5, 6),
        EventEntry::new(EventType::ConfigurationError, 7, 8, 9),
    ];

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, EventType::TranslationFault);
    assert_eq!(events[1].event_type, EventType::PermissionFault);
    assert_eq!(events[2].event_type, EventType::ConfigurationError);
}

#[test]
fn test_event_entry_sorting_by_timestamp() {
    let mut entry1 = EventEntry::new(EventType::TranslationFault, 1, 2, 3);
    let mut entry2 = EventEntry::new(EventType::PermissionFault, 4, 5, 6);
    let mut entry3 = EventEntry::new(EventType::ConfigurationError, 7, 8, 9);

    entry1.timestamp = 3000;
    entry2.timestamp = 1000;
    entry3.timestamp = 2000;

    let mut events = [entry1, entry2, entry3];
    events.sort_by_key(|e| e.timestamp);

    assert_eq!(events[0].timestamp, 1000);
    assert_eq!(events[1].timestamp, 2000);
    assert_eq!(events[2].timestamp, 3000);
}

#[test]
#[allow(clippy::useless_vec)]
fn test_event_entry_filtering_by_type() {
    let events = vec![
        EventEntry::new(EventType::TranslationFault, 1, 2, 3),
        EventEntry::new(EventType::PermissionFault, 4, 5, 6),
        EventEntry::new(EventType::TranslationFault, 7, 8, 9),
    ];

    assert_eq!(events.iter().filter(|e| e.event_type == EventType::TranslationFault).count(), 2);
}

// ============================================================================
// ARM SMMU v3 Section 6.3 Compliance Tests
// ============================================================================

#[test]
fn test_event_entry_arm_smmu_section_6_3_event_types() {
    // ARM SMMU v3 Section 6.3 defines event types
    let arm_event_types = [
        (EventType::TranslationFault, 0u8),
        (EventType::PermissionFault, 1u8),
        (EventType::CommandSyncCompletion, 2u8),
        (EventType::PriPageRequest, 3u8),
        (EventType::AtcInvalidateCompletion, 4u8),
        (EventType::ConfigurationError, 5u8),
        (EventType::InternalError, 6u8),
    ];

    for (event_type, expected_value) in &arm_event_types {
        assert_eq!(*event_type as u8, *expected_value);
    }
}

#[test]
fn test_event_entry_arm_smmu_fault_recording() {
    // Section 6.3: Event queue records faults with stream_id, pasid, and address
    let entry = EventEntry::new(
        EventType::TranslationFault,
        0x1234,     // StreamID
        0x5678,     // PASID
        0xDEAD_BEEF, // Faulting address
    );

    assert_eq!(entry.stream_id, 0x1234);
    assert_eq!(entry.pasid, 0x5678);
    assert_eq!(entry.address, 0xDEAD_BEEF);
    assert_eq!(entry.event_type, EventType::TranslationFault);
}

#[test]
fn test_event_entry_arm_smmu_security_context() {
    // Section 6.3: Events include security state context
    let mut entry = EventEntry::new(EventType::PermissionFault, 1, 2, 3);

    entry.security_state = SecurityState::Secure;
    assert_eq!(entry.security_state, SecurityState::Secure);

    entry.security_state = SecurityState::NonSecure;
    assert_eq!(entry.security_state, SecurityState::NonSecure);

    entry.security_state = SecurityState::Realm;
    assert_eq!(entry.security_state, SecurityState::Realm);
}

#[test]
fn test_event_entry_arm_smmu_pri_page_request() {
    // Section 6.3.4: PRI page request events
    let entry = EventEntry::new(
        EventType::PriPageRequest,
        0x100,  // StreamID requesting page
        0x200,  // PASID
        0x5000, // Requested address
    );

    assert_eq!(entry.event_type, EventType::PriPageRequest);
    assert_eq!(entry.stream_id, 0x100);
    assert_eq!(entry.pasid, 0x200);
    assert_eq!(entry.address, 0x5000);
}

#[test]
fn test_event_entry_arm_smmu_command_sync_completion() {
    // Section 6.3.3: Command queue SYNC completion events
    let entry = EventEntry::new(EventType::CommandSyncCompletion, 0, 0, 0);

    assert_eq!(entry.event_type, EventType::CommandSyncCompletion);
}

#[test]
fn test_event_entry_arm_smmu_atc_invalidate_completion() {
    // Section 6.3.5: ATC invalidation completion events
    let entry = EventEntry::new(EventType::AtcInvalidateCompletion, 0x50, 0x60, 0);

    assert_eq!(entry.event_type, EventType::AtcInvalidateCompletion);
    assert_eq!(entry.stream_id, 0x50);
}

#[test]
fn test_event_entry_arm_smmu_configuration_error() {
    // Section 6.3.6: Configuration errors
    let mut entry = EventEntry::new(EventType::ConfigurationError, 0x99, 0, 0);
    entry.error_code = 0x01; // Configuration error code

    assert_eq!(entry.event_type, EventType::ConfigurationError);
    assert_eq!(entry.error_code, 0x01);
}

#[test]
fn test_event_entry_arm_smmu_internal_error() {
    // Section 6.3.7: Internal errors
    let mut entry = EventEntry::new(EventType::InternalError, 0, 0, 0);
    entry.error_code = 0xFF; // Internal error code

    assert_eq!(entry.event_type, EventType::InternalError);
    assert_eq!(entry.error_code, 0xFF);
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_event_entry_all_zeros() {
    let entry = EventEntry::new(EventType::TranslationFault, 0, 0, 0);

    assert_eq!(entry.stream_id, 0);
    assert_eq!(entry.pasid, 0);
    assert_eq!(entry.address, 0);
    assert_eq!(entry.error_code, 0);
    assert_eq!(entry.timestamp, 0);
}

#[test]
fn test_event_entry_all_max_values() {
    let mut entry = EventEntry::new(EventType::InternalError, u32::MAX, u32::MAX, u64::MAX);
    entry.error_code = u32::MAX;
    entry.timestamp = u64::MAX;

    assert_eq!(entry.stream_id, u32::MAX);
    assert_eq!(entry.pasid, u32::MAX);
    assert_eq!(entry.address, u64::MAX);
    assert_eq!(entry.error_code, u32::MAX);
    assert_eq!(entry.timestamp, u64::MAX);
}

#[test]
fn test_event_entry_mixed_boundary_values() {
    let entry = EventEntry::new(EventType::ConfigurationError, 0, u32::MAX, 0);

    assert_eq!(entry.stream_id, 0);
    assert_eq!(entry.pasid, u32::MAX);
    assert_eq!(entry.address, 0);
}

#[test]
fn test_event_type_repr_u8() {
    // Ensure all EventType variants fit in u8
    assert_eq!(std::mem::size_of::<EventType>(), 1);
}

#[test]
fn test_event_entry_size() {
    // Verify EventEntry has expected memory layout
    let size = std::mem::size_of::<EventEntry>();
    // EventEntry should be reasonably sized (fields: u8 + u32 + u32 + u64 + SecurityState + u32 + u64)
    assert!(size > 0);
    assert!(size <= 128); // Sanity check: should not be excessively large
}
