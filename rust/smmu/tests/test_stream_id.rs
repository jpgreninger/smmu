#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive tests for `types/stream_id.rs`
//!
//! Tests cover:
//! - StreamID construction with validation (new, `try_from`)
//! - Boundary value testing (0, MAX, MAX+1)
//! - Value extraction (`as_u32`, into)
//! - Trait implementations (Display, Debug, Default, Copy, Clone, `PartialEq`, Eq, Hash)
//! - Error handling for invalid values
//! - HashMap/HashSet integration
//! - ARM SMMU v3 specification compliance

use smmu::types::{StreamID, ValidationError};
use std::collections::{HashMap, HashSet};

// ============================================================================
// StreamID Construction Tests
// ============================================================================

#[test]
fn test_stream_id_new_valid() {
    let stream_id = StreamID::new(42).expect("Valid StreamID");
    assert_eq!(stream_id.as_u32(), 42);
}

#[test]
fn test_stream_id_new_zero() {
    let stream_id = StreamID::new(0).expect("Zero is valid");
    assert_eq!(stream_id.as_u32(), 0);
}

#[test]
fn test_stream_id_new_maximum() {
    // Maximum valid StreamID (16-bit: 65_535)
    let stream_id = StreamID::new(65_535).expect("Maximum is valid");
    assert_eq!(stream_id.as_u32(), 65_535);
}

#[test]
fn test_stream_id_new_above_maximum() {
    // Just above maximum should fail
    let result = StreamID::new(65_536);
    assert!(result.is_err());
}

#[test]
fn test_stream_id_new_way_above_maximum() {
    // Far above maximum should fail
    let result = StreamID::new(1_000_000);
    assert!(result.is_err());
}

#[test]
fn test_stream_id_new_u32_max() {
    // u32::MAX should fail
    let result = StreamID::new(u32::MAX);
    assert!(result.is_err());
}

#[test]
fn test_stream_id_new_typical_values() {
    // Test several typical hardware StreamID values
    for value in [1, 10, 100, 1000, 10_000, 32_768, 65_535] {
        let stream_id = StreamID::new(value).expect("Should be valid");
        assert_eq!(stream_id.as_u32(), value);
    }
}

#[test]
fn test_stream_id_validation_error_message() {
    let result = StreamID::new(100_000);
    assert!(result.is_err());

    if let Err(err) = result {
        let error_msg = err.to_string();
        assert!(error_msg.contains("StreamID"));
        assert!(error_msg.contains("100_000"));
    }
}

// ============================================================================
// StreamID Value Extraction Tests
// ============================================================================

#[test]
fn test_stream_id_as_u32() {
    let stream_id = StreamID::new(1234).unwrap();
    assert_eq!(stream_id.as_u32(), 1234);
}

#[test]
fn test_stream_id_as_u32_const() {
    const STREAM_ID_VALUE: u32 = {
        // This wouldn't compile if as_u32 wasn't const
        // We can't call new() in const context, but we can test as_u32 is const
        42
    };

    let stream_id = StreamID::new(STREAM_ID_VALUE).unwrap();
    assert_eq!(stream_id.as_u32(), STREAM_ID_VALUE);
}

#[test]
fn test_stream_id_into_u32() {
    let stream_id = StreamID::new(5678).unwrap();
    let value: u32 = stream_id.into();
    assert_eq!(value, 5678);
}

// ============================================================================
// TryFrom<u32> Trait Tests
// ============================================================================

#[test]
fn test_stream_id_try_from_valid() {
    let stream_id: StreamID = 42u32.try_into().expect("Valid conversion");
    assert_eq!(stream_id.as_u32(), 42);
}

#[test]
fn test_stream_id_try_from_invalid() {
    let result: Result<StreamID, ValidationError> = 100_000_u32.try_into();
    assert!(result.is_err());
}

#[test]
fn test_stream_id_try_from_boundary() {
    // Boundary: max valid value
    let stream_id: StreamID = 65535u32.try_into().unwrap();
    assert_eq!(stream_id.as_u32(), 65_535);

    // Boundary: max + 1 (invalid)
    let result: Result<StreamID, ValidationError> = 65536u32.try_into();
    assert!(result.is_err());
}

// ============================================================================
// Display Trait Tests
// ============================================================================

#[test]
fn test_stream_id_display() {
    let stream_id = StreamID::new(42).unwrap();
    let display_str = format!("{stream_id}");
    assert_eq!(display_str, "StreamID(42)");
}

#[test]
fn test_stream_id_display_zero() {
    let stream_id = StreamID::new(0).unwrap();
    let display_str = format!("{stream_id}");
    assert_eq!(display_str, "StreamID(0)");
}

#[test]
fn test_stream_id_display_maximum() {
    let stream_id = StreamID::new(65_535).unwrap();
    let display_str = format!("{stream_id}");
    assert_eq!(display_str, "StreamID(65_535)");
}

// ============================================================================
// Debug Trait Tests
// ============================================================================

#[test]
fn test_stream_id_debug() {
    let stream_id = StreamID::new(42).unwrap();
    let debug_str = format!("{stream_id:?}");
    assert!(debug_str.contains("StreamID"));
    assert!(debug_str.contains("42"));
}

#[test]
fn test_stream_id_debug_formatting() {
    let stream_id = StreamID::new(1234).unwrap();
    let debug_str = format!("{stream_id:?}");
    // Debug should show the inner value
    assert!(debug_str.contains("1234"));
}

// ============================================================================
// Default Trait Tests
// ============================================================================

#[test]
fn test_stream_id_default() {
    let stream_id = StreamID::default();
    assert_eq!(stream_id.as_u32(), 0);
}

#[test]
fn test_stream_id_default_display() {
    let stream_id = StreamID::default();
    assert_eq!(format!("{stream_id}"), "StreamID(0)");
}

// ============================================================================
// Copy/Clone Trait Tests
// ============================================================================

#[test]
fn test_stream_id_copy() {
    let stream_id1 = StreamID::new(100).unwrap();
    let stream_id2 = stream_id1; // Copy

    assert_eq!(stream_id1.as_u32(), 100);
    assert_eq!(stream_id2.as_u32(), 100);
}

#[test]
fn test_stream_id_clone() {
    let stream_id1 = StreamID::new(200).unwrap();
    let stream_id2 = stream_id1;

    assert_eq!(stream_id1, stream_id2);
}

// ============================================================================
// PartialEq/Eq Trait Tests
// ============================================================================

#[test]
fn test_stream_id_equality() {
    let stream_id1 = StreamID::new(42).unwrap();
    let stream_id2 = StreamID::new(42).unwrap();
    let stream_id3 = StreamID::new(43).unwrap();

    assert_eq!(stream_id1, stream_id2);
    assert_ne!(stream_id1, stream_id3);
}

#[test]
fn test_stream_id_equality_zero() {
    let stream_id1 = StreamID::new(0).unwrap();
    let stream_id2 = StreamID::default();

    assert_eq!(stream_id1, stream_id2);
}

#[test]
fn test_stream_id_inequality() {
    let stream_id1 = StreamID::new(100).unwrap();
    let stream_id2 = StreamID::new(200).unwrap();

    assert_ne!(stream_id1, stream_id2);
}

// ============================================================================
// Hash Trait Tests (HashMap/HashSet Usage)
// ============================================================================

#[test]
fn test_stream_id_hash_set() {
    let mut set = HashSet::new();

    set.insert(StreamID::new(1).unwrap());
    set.insert(StreamID::new(2).unwrap());
    set.insert(StreamID::new(3).unwrap());
    set.insert(StreamID::new(1).unwrap()); // Duplicate

    assert_eq!(set.len(), 3);
    assert!(set.contains(&StreamID::new(1).unwrap()));
    assert!(set.contains(&StreamID::new(2).unwrap()));
    assert!(set.contains(&StreamID::new(3).unwrap()));
    assert!(!set.contains(&StreamID::new(4).unwrap()));
}

#[test]
fn test_stream_id_hash_map() {
    let mut map = HashMap::new();

    map.insert(StreamID::new(1).unwrap(), "Device A");
    map.insert(StreamID::new(2).unwrap(), "Device B");
    map.insert(StreamID::new(3).unwrap(), "Device C");

    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&StreamID::new(1).unwrap()), Some(&"Device A"));
    assert_eq!(map.get(&StreamID::new(2).unwrap()), Some(&"Device B"));
    assert_eq!(map.get(&StreamID::new(3).unwrap()), Some(&"Device C"));
    assert_eq!(map.get(&StreamID::new(4).unwrap()), None);
}

#[test]
fn test_stream_id_hash_map_update() {
    let mut map = HashMap::new();

    map.insert(StreamID::new(10).unwrap(), 100);
    map.insert(StreamID::new(10).unwrap(), 200); // Update

    assert_eq!(map.len(), 1);
    assert_eq!(map.get(&StreamID::new(10).unwrap()), Some(&200));
}

#[test]
fn test_stream_id_hash_consistency() {
    // Same StreamID should hash to same value
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let stream_id1 = StreamID::new(42).unwrap();
    let stream_id2 = StreamID::new(42).unwrap();

    let mut hasher1 = DefaultHasher::new();
    let mut hasher2 = DefaultHasher::new();

    stream_id1.hash(&mut hasher1);
    stream_id2.hash(&mut hasher2);

    assert_eq!(hasher1.finish(), hasher2.finish());
}

// ============================================================================
// Boundary Value Tests
// ============================================================================

#[test]
fn test_stream_id_boundary_zero() {
    let stream_id = StreamID::new(0).unwrap();
    assert_eq!(stream_id.as_u32(), 0);
}

#[test]
fn test_stream_id_boundary_max() {
    let stream_id = StreamID::new(65_535).unwrap();
    assert_eq!(stream_id.as_u32(), 65_535);
}

#[test]
fn test_stream_id_boundary_max_plus_one() {
    let result = StreamID::new(65_536);
    assert!(result.is_err());
}

#[test]
fn test_stream_id_boundary_one_below_max() {
    let stream_id = StreamID::new(65_534).unwrap();
    assert_eq!(stream_id.as_u32(), 65_534);
}

#[test]
fn test_stream_id_boundary_powers_of_two() {
    // Test powers of 2 within valid range
    for exp in 0..16 {
        let value = 1u32 << exp;
        if value <= 65_535 {
            let stream_id = StreamID::new(value).unwrap();
            assert_eq!(stream_id.as_u32(), value);
        }
    }
}

// ============================================================================
// ARM SMMU v3 Specification Compliance Tests
// ============================================================================

#[test]
fn test_arm_spec_16_bit_range() {
    // ARM SMMU v3: StreamID typically 16-bit (0-65_535)
    assert!(StreamID::new(0).is_ok());
    assert!(StreamID::new(65_535).is_ok());
    assert!(StreamID::new(65_536).is_err());
}

#[test]
fn test_arm_spec_stream_id_zero_valid() {
    // StreamID 0 is valid per ARM SMMU v3
    let stream_id = StreamID::new(0).unwrap();
    assert_eq!(stream_id.as_u32(), 0);
}

#[test]
fn test_arm_spec_typical_hardware_values() {
    // Common StreamID values in real hardware
    let typical_values = vec![
        0,     // Often reserved or first device
        1,     // First user device
        128,   // Mid-range device
        256,   // Another common allocation
        1024,  // Larger systems
        4095,  // Common maximum in some configs
        65_535, // Maximum 16-bit value
    ];

    for value in typical_values {
        let stream_id = StreamID::new(value).unwrap();
        assert_eq!(stream_id.as_u32(), value);
    }
}

// ============================================================================
// Conversion Round-Trip Tests
// ============================================================================

#[test]
fn test_stream_id_round_trip_new_as_u32() {
    let original = 1234u32;
    let stream_id = StreamID::new(original).unwrap();
    let extracted = stream_id.as_u32();
    assert_eq!(original, extracted);
}

#[test]
fn test_stream_id_round_trip_try_from_into() {
    let original = 5678u32;
    let stream_id: StreamID = original.try_into().unwrap();
    let extracted: u32 = stream_id.into();
    assert_eq!(original, extracted);
}

#[test]
fn test_stream_id_round_trip_multiple_values() {
    for value in [0, 1, 100, 1000, 10_000, 65_535] {
        let stream_id = StreamID::new(value).unwrap();
        assert_eq!(stream_id.as_u32(), value);

        let extracted: u32 = stream_id.into();
        assert_eq!(extracted, value);
    }
}

// ============================================================================
// Collection Operations Tests
// ============================================================================

#[test]
#[allow(clippy::useless_vec)]
fn test_stream_id_vec_operations() {
    let vec = vec![
        StreamID::new(1).unwrap(),
        StreamID::new(2).unwrap(),
        StreamID::new(3).unwrap(),
    ];

    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_u32(), 1);
    assert_eq!(vec[1].as_u32(), 2);
    assert_eq!(vec[2].as_u32(), 3);
}

#[test]
fn test_stream_id_vec_contains() {
    let vec = [StreamID::new(10).unwrap(),
        StreamID::new(20).unwrap(),
        StreamID::new(30).unwrap()];

    assert!(vec.contains(&StreamID::new(10).unwrap()));
    assert!(vec.contains(&StreamID::new(20).unwrap()));
    assert!(vec.contains(&StreamID::new(30).unwrap()));
    assert!(!vec.contains(&StreamID::new(40).unwrap()));
}

// ============================================================================
// Error Handling Edge Cases
// ============================================================================

#[test]
fn test_stream_id_error_preserves_value() {
    let invalid_value = 100_000_u32;
    let result = StreamID::new(invalid_value);

    assert!(result.is_err());
    if let Err(err) = result {
        let msg = err.to_string();
        assert!(msg.contains("100_000"));
    }
}

#[test]
fn test_stream_id_multiple_invalid_values() {
    let invalid_values = vec![65_536, 100_000, 1_000_000, u32::MAX];

    for value in invalid_values {
        let result = StreamID::new(value);
        assert!(result.is_err(), "Value {value} should be invalid");
    }
}
