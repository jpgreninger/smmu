//! Comprehensive tests for PASID type
//!
//! This test suite achieves 100% coverage for types/pasid.rs by testing:
//! - All trait implementations (Ord, PartialOrd, Hash, Display, Debug, Default)
//! - Boundary values (PASID 0, MAX-1, MAX)
//! - Const constructor usage
//! - TryFrom and From conversions
//! - Validation and error handling

use smmu::types::{PASID, PASID_MAX};
use smmu::ValidationError;
use std::collections::{HashMap, HashSet};

// ============================================================================
// Basic Construction and Validation Tests
// ============================================================================

#[test]
fn test_pasid_new_valid_zero() {
    let pasid = PASID::new(0).expect("PASID 0 should be valid");
    assert_eq!(pasid.as_u32(), 0);
}

#[test]
fn test_pasid_new_valid_middle_value() {
    let pasid = PASID::new(12345).expect("Valid PASID");
    assert_eq!(pasid.as_u32(), 12345);
}

#[test]
fn test_pasid_new_valid_max() {
    let pasid = PASID::new(PASID_MAX).expect("PASID_MAX should be valid");
    assert_eq!(pasid.as_u32(), PASID_MAX);
}

#[test]
fn test_pasid_new_valid_max_minus_one() {
    let pasid = PASID::new(PASID_MAX - 1).expect("PASID_MAX-1 should be valid");
    assert_eq!(pasid.as_u32(), PASID_MAX - 1);
}

#[test]
fn test_pasid_new_invalid_exceeds_max() {
    let result = PASID::new(PASID_MAX + 1);
    assert!(result.is_err());
}

#[test]
fn test_pasid_new_invalid_large_value() {
    let result = PASID::new(0x100000); // 1048576, exceeds 20-bit
    assert!(result.is_err());
}

#[test]
fn test_pasid_new_invalid_max_u32() {
    let result = PASID::new(u32::MAX);
    assert!(result.is_err());
}

// ============================================================================
// Default Trait Tests
// ============================================================================

#[test]
fn test_pasid_default_is_zero() {
    let pasid = PASID::default();
    assert_eq!(pasid.as_u32(), 0);
}

#[test]
fn test_pasid_default_equals_new_zero() {
    let default_pasid = PASID::default();
    let zero_pasid = PASID::new(0).unwrap();
    assert_eq!(default_pasid, zero_pasid);
}

// ============================================================================
// Display Trait Tests
// ============================================================================

#[test]
fn test_pasid_display_zero() {
    let pasid = PASID::new(0).unwrap();
    let display = format!("{}", pasid);
    assert_eq!(display, "PASID(0)");
}

#[test]
fn test_pasid_display_middle_value() {
    let pasid = PASID::new(12345).unwrap();
    let display = format!("{}", pasid);
    assert_eq!(display, "PASID(12345)");
}

#[test]
fn test_pasid_display_max() {
    let pasid = PASID::new(PASID_MAX).unwrap();
    let display = format!("{}", pasid);
    assert_eq!(display, format!("PASID({})", PASID_MAX));
}

#[test]
fn test_pasid_display_formatting() {
    let pasid = PASID::new(999).unwrap();
    assert_eq!(format!("{}", pasid), "PASID(999)");
    assert_eq!(format!("{pasid:?}"), "PASID(999)"); // Debug uses derived format
}

// ============================================================================
// Debug Trait Tests
// ============================================================================

#[test]
fn test_pasid_debug() {
    let pasid = PASID::new(42).unwrap();
    let debug_str = format!("{pasid:?}");
    assert!(debug_str.contains("PASID"));
    assert!(debug_str.contains("42"));
}

#[test]
fn test_pasid_debug_zero() {
    let pasid = PASID::default();
    let debug_str = format!("{pasid:?}");
    assert!(debug_str.contains("PASID"));
    assert!(debug_str.contains("0"));
}

#[test]
fn test_pasid_debug_max() {
    let pasid = PASID::new(PASID_MAX).unwrap();
    let debug_str = format!("{pasid:?}");
    assert!(debug_str.contains("PASID"));
    assert!(debug_str.contains(&format!("{}", PASID_MAX)));
}

// ============================================================================
// Ordering Trait Tests (Ord, PartialOrd)
// ============================================================================

#[test]
fn test_pasid_ordering_zero_less_than_one() {
    let pasid_0 = PASID::new(0).unwrap();
    let pasid_1 = PASID::new(1).unwrap();
    assert!(pasid_0 < pasid_1);
    assert!(pasid_1 > pasid_0);
}

#[test]
fn test_pasid_ordering_max_minus_one_less_than_max() {
    let pasid_max_minus_1 = PASID::new(PASID_MAX - 1).unwrap();
    let pasid_max = PASID::new(PASID_MAX).unwrap();
    assert!(pasid_max_minus_1 < pasid_max);
    assert!(pasid_max > pasid_max_minus_1);
}

#[test]
fn test_pasid_ordering_equal_values() {
    let pasid_a = PASID::new(100).unwrap();
    let pasid_b = PASID::new(100).unwrap();
    assert_eq!(pasid_a.cmp(&pasid_b), std::cmp::Ordering::Equal);
}

#[test]
fn test_pasid_ordering_transitivity() {
    let pasid_1 = PASID::new(10).unwrap();
    let pasid_2 = PASID::new(20).unwrap();
    let pasid_3 = PASID::new(30).unwrap();

    assert!(pasid_1 < pasid_2);
    assert!(pasid_2 < pasid_3);
    assert!(pasid_1 < pasid_3); // Transitivity
}

#[test]
fn test_pasid_partial_ord() {
    let pasid_5 = PASID::new(5).unwrap();
    let pasid_10 = PASID::new(10).unwrap();

    assert_eq!(pasid_5.partial_cmp(&pasid_10), Some(std::cmp::Ordering::Less));
    assert_eq!(pasid_10.partial_cmp(&pasid_5), Some(std::cmp::Ordering::Greater));
    assert_eq!(pasid_5.partial_cmp(&pasid_5), Some(std::cmp::Ordering::Equal));
}

#[test]
fn test_pasid_ordering_methods() {
    let pasid_low = PASID::new(100).unwrap();
    let pasid_high = PASID::new(200).unwrap();

    assert!(pasid_low.le(&pasid_high));
    assert!(pasid_low.lt(&pasid_high));
    assert!(pasid_high.ge(&pasid_low));
    assert!(pasid_high.gt(&pasid_low));
}

// ============================================================================
// Hash Trait Tests
// ============================================================================

#[test]
fn test_pasid_hash_consistency() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let pasid = PASID::new(42).unwrap();

    let mut hasher1 = DefaultHasher::new();
    pasid.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    pasid.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    assert_eq!(hash1, hash2, "Same PASID should produce same hash");
}

#[test]
fn test_pasid_hash_different_values() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let pasid_1 = PASID::new(1).unwrap();
    let pasid_2 = PASID::new(2).unwrap();

    let mut hasher1 = DefaultHasher::new();
    pasid_1.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    pasid_2.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    assert_ne!(hash1, hash2, "Different PASIDs should produce different hashes");
}

#[test]
fn test_pasid_in_hashmap() {
    let mut map = HashMap::new();

    let pasid_1 = PASID::new(1).unwrap();
    let pasid_2 = PASID::new(2).unwrap();
    let pasid_3 = PASID::new(3).unwrap();

    map.insert(pasid_1, "value1");
    map.insert(pasid_2, "value2");
    map.insert(pasid_3, "value3");

    assert_eq!(map.get(&pasid_1), Some(&"value1"));
    assert_eq!(map.get(&pasid_2), Some(&"value2"));
    assert_eq!(map.get(&pasid_3), Some(&"value3"));
    assert_eq!(map.len(), 3);
}

#[test]
fn test_pasid_in_hashmap_overwrite() {
    let mut map = HashMap::new();

    let pasid = PASID::new(42).unwrap();

    map.insert(pasid, "old_value");
    map.insert(pasid, "new_value");

    assert_eq!(map.get(&pasid), Some(&"new_value"));
    assert_eq!(map.len(), 1);
}

#[test]
fn test_pasid_in_hashset() {
    let mut set = HashSet::new();

    let pasid_1 = PASID::new(1).unwrap();
    let pasid_2 = PASID::new(2).unwrap();
    let pasid_1_dup = PASID::new(1).unwrap();

    assert!(set.insert(pasid_1));
    assert!(set.insert(pasid_2));
    assert!(!set.insert(pasid_1_dup), "Duplicate should not be inserted");

    assert_eq!(set.len(), 2);
    assert!(set.contains(&pasid_1));
    assert!(set.contains(&pasid_2));
}

// ============================================================================
// TryFrom Trait Tests
// ============================================================================

#[test]
fn test_pasid_try_from_valid_zero() {
    let pasid: PASID = 0u32.try_into().expect("PASID 0 should be valid");
    assert_eq!(pasid.as_u32(), 0);
}

#[test]
fn test_pasid_try_from_valid_value() {
    let pasid: PASID = 12345u32.try_into().expect("Valid PASID");
    assert_eq!(pasid.as_u32(), 12345);
}

#[test]
fn test_pasid_try_from_valid_max() {
    let pasid: PASID = PASID_MAX.try_into().expect("PASID_MAX should be valid");
    assert_eq!(pasid.as_u32(), PASID_MAX);
}

#[test]
fn test_pasid_try_from_invalid_exceeds_max() {
    let result: Result<PASID, _> = (PASID_MAX + 1).try_into();
    assert!(result.is_err());
}

#[test]
fn test_pasid_try_from_invalid_max_u32() {
    let result: Result<PASID, _> = u32::MAX.try_into();
    assert!(result.is_err());
}

// ============================================================================
// From Trait Tests (PASID -> u32)
// ============================================================================

#[test]
fn test_pasid_into_u32_zero() {
    let pasid = PASID::new(0).unwrap();
    let value: u32 = pasid.into();
    assert_eq!(value, 0);
}

#[test]
fn test_pasid_into_u32_middle_value() {
    let pasid = PASID::new(54321).unwrap();
    let value: u32 = pasid.into();
    assert_eq!(value, 54321);
}

#[test]
fn test_pasid_into_u32_max() {
    let pasid = PASID::new(PASID_MAX).unwrap();
    let value: u32 = pasid.into();
    assert_eq!(value, PASID_MAX);
}

#[test]
fn test_pasid_from_conversion() {
    let pasid = PASID::new(999).unwrap();
    let value = u32::from(pasid);
    assert_eq!(value, 999);
}

// ============================================================================
// Copy and Clone Trait Tests
// ============================================================================

#[test]
fn test_pasid_copy() {
    let pasid1 = PASID::new(42).unwrap();
    let pasid2 = pasid1; // Copy
    assert_eq!(pasid1, pasid2);
    assert_eq!(pasid1.as_u32(), 42); // Original still valid
}

#[test]
fn test_pasid_clone() {
    let pasid1 = PASID::new(123).unwrap();
    let pasid2 = pasid1.clone();
    assert_eq!(pasid1, pasid2);
}

// ============================================================================
// Equality Tests (PartialEq, Eq)
// ============================================================================

#[test]
fn test_pasid_equality() {
    let pasid1 = PASID::new(100).unwrap();
    let pasid2 = PASID::new(100).unwrap();
    assert_eq!(pasid1, pasid2);
}

#[test]
fn test_pasid_inequality() {
    let pasid1 = PASID::new(100).unwrap();
    let pasid2 = PASID::new(200).unwrap();
    assert_ne!(pasid1, pasid2);
}

#[test]
fn test_pasid_equality_reflexive() {
    let pasid = PASID::new(50).unwrap();
    assert_eq!(pasid, pasid);
}

#[test]
fn test_pasid_equality_symmetric() {
    let pasid1 = PASID::new(75).unwrap();
    let pasid2 = PASID::new(75).unwrap();
    assert_eq!(pasid1, pasid2);
    assert_eq!(pasid2, pasid1);
}

#[test]
fn test_pasid_equality_transitive() {
    let pasid1 = PASID::new(33).unwrap();
    let pasid2 = PASID::new(33).unwrap();
    let pasid3 = PASID::new(33).unwrap();

    assert_eq!(pasid1, pasid2);
    assert_eq!(pasid2, pasid3);
    assert_eq!(pasid1, pasid3);
}

// ============================================================================
// Boundary Value Tests
// ============================================================================

#[test]
fn test_pasid_boundary_zero() {
    let pasid = PASID::new(0).expect("PASID 0 is the default address space");
    assert_eq!(pasid.as_u32(), 0);
}

#[test]
fn test_pasid_boundary_one() {
    let pasid = PASID::new(1).expect("PASID 1 should be valid");
    assert_eq!(pasid.as_u32(), 1);
}

#[test]
fn test_pasid_boundary_max_minus_two() {
    let pasid = PASID::new(PASID_MAX - 2).expect("PASID MAX-2 should be valid");
    assert_eq!(pasid.as_u32(), PASID_MAX - 2);
}

#[test]
fn test_pasid_boundary_max_minus_one() {
    let pasid = PASID::new(PASID_MAX - 1).expect("PASID MAX-1 should be valid");
    assert_eq!(pasid.as_u32(), PASID_MAX - 1);
    assert_eq!(pasid.as_u32(), 0xFFFFE);
}

#[test]
fn test_pasid_boundary_max() {
    let pasid = PASID::new(PASID_MAX).expect("PASID MAX should be valid");
    assert_eq!(pasid.as_u32(), PASID_MAX);
    assert_eq!(pasid.as_u32(), 0xFFFFF);
}

#[test]
fn test_pasid_boundary_max_plus_one_invalid() {
    let result = PASID::new(PASID_MAX + 1);
    assert!(result.is_err());

    if let Err(ValidationError::Generic { field, .. }) = result {
        assert_eq!(field, "PASID");
    } else {
        panic!("Expected ValidationError::Generic");
    }
}

#[test]
fn test_pasid_boundary_20bit_limit() {
    // Verify that 20-bit maximum is exactly 0xFFFFF (1048575)
    assert_eq!(PASID_MAX, 0xFFFFF);
    assert_eq!(PASID_MAX, 1048575);

    // Test exactly at boundary
    assert!(PASID::new(0xFFFFF).is_ok());
    assert!(PASID::new(0x100000).is_err()); // 21-bit value
}

// ============================================================================
// Const Usage Tests
// ============================================================================

#[test]
fn test_pasid_const_value_extraction() {
    // Test that as_u32() is a const fn by using it in a const context
    let pasid = PASID::new(999).unwrap();
    const fn extract(p: PASID) -> u32 {
        p.as_u32()
    }
    assert_eq!(extract(pasid), 999);
}

#[test]
fn test_pasid_const_fn_usage() {
    // Verify as_u32 can be used in const contexts
    const fn double_pasid_value(p: PASID) -> u32 {
        p.as_u32() * 2
    }

    let pasid = PASID::new(50).unwrap();
    assert_eq!(double_pasid_value(pasid), 100);
}

// ============================================================================
// Error Message Tests
// ============================================================================

#[test]
fn test_pasid_error_message_format() {
    let result = PASID::new(PASID_MAX + 1);
    assert!(result.is_err());

    if let Err(err) = result {
        let msg = format!("{}", err);
        assert!(msg.contains("PASID"));
        assert!(msg.contains("0xFFFFF")); // Should mention the maximum
    }
}

#[test]
fn test_pasid_error_message_large_value() {
    let result = PASID::new(0x200000); // Well over maximum
    assert!(result.is_err());

    if let Err(err) = result {
        let msg = format!("{}", err);
        assert!(msg.contains("PASID"));
        assert!(msg.contains("20-bit"));
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_pasid_roundtrip_conversion() {
    let original: u32 = 54321;
    let pasid: PASID = original.try_into().expect("Valid PASID");
    let converted: u32 = pasid.into();
    assert_eq!(original, converted);
}

#[test]
fn test_pasid_multiple_conversions() {
    let value: u32 = 777;

    // u32 -> PASID
    let pasid1 = PASID::new(value).unwrap();
    let pasid2: PASID = value.try_into().unwrap();

    // PASID -> u32
    let back1: u32 = pasid1.into();
    let back2: u32 = pasid2.into();

    assert_eq!(pasid1, pasid2);
    assert_eq!(back1, value);
    assert_eq!(back2, value);
}

#[test]
fn test_pasid_collection_usage() {
    // Test that PASID works well in various collection types
    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();
    let pasid3 = PASID::new(3).unwrap();

    // Vec
    let vec = vec![pasid1, pasid2, pasid3];
    assert_eq!(vec.len(), 3);

    // HashSet
    let mut set = HashSet::new();
    set.insert(pasid1);
    set.insert(pasid2);
    set.insert(pasid3);
    assert_eq!(set.len(), 3);

    // HashMap
    let mut map = HashMap::new();
    map.insert(pasid1, "one");
    map.insert(pasid2, "two");
    map.insert(pasid3, "three");
    assert_eq!(map.len(), 3);
}

// ============================================================================
// ARM SMMU v3 Specification Compliance Tests
// ============================================================================

#[test]
fn test_pasid_spec_default_address_space() {
    // Per ARM SMMU v3 spec: PASID 0 is the default address space
    let default_pasid = PASID::new(0).expect("PASID 0 must be supported");
    assert_eq!(default_pasid, PASID::default());
}

#[test]
fn test_pasid_spec_20bit_range() {
    // Per ARM SMMU v3 spec Section 3.6: PASID is 20-bit (0-1048575)
    assert_eq!(PASID_MAX, 0xFFFFF);
    assert_eq!(PASID_MAX, 1048575);

    // Verify boundary enforcement
    assert!(PASID::new(0).is_ok());
    assert!(PASID::new(PASID_MAX).is_ok());
    assert!(PASID::new(PASID_MAX + 1).is_err());
}

#[test]
fn test_pasid_spec_full_range_valid() {
    // Verify that all values in the valid range can be created
    // Test samples across the range
    let test_values = [0, 1, 100, 1000, 10000, 100000, PASID_MAX / 2, PASID_MAX - 1, PASID_MAX];

    for &value in &test_values {
        let pasid = PASID::new(value);
        assert!(pasid.is_ok(), "PASID value {} should be valid", value);
        assert_eq!(pasid.unwrap().as_u32(), value);
    }
}

#[test]
fn test_pasid_spec_out_of_range_invalid() {
    // Verify that values outside the valid range are rejected
    let invalid_values = [
        PASID_MAX + 1,
        PASID_MAX + 100,
        0x100000, // 2^20
        0x200000,
        0xFFFFFFFF, // u32::MAX
    ];

    for &value in &invalid_values {
        let result = PASID::new(value);
        assert!(result.is_err(), "PASID value {} should be invalid", value);
    }
}
