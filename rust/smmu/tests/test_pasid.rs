//! Comprehensive unit tests for PASID newtype wrapper
//!
//! Tests ARM SMMU v3 specification compliance for PASID:
//! - 20-bit value (0-1048575, per ARM SMMU v3 spec Section 3.6)
//! - PASID 0 must be supported (default address space)
//! - Maximum value: 0xFFFFF (1048575)
//! - Type safety and validation
//!
//! All tests follow strict TDD - they WILL FAIL until implementation is complete.

use smmu::types::PASID;
use std::collections::HashMap;

// ARM SMMU v3 spec constants
const PASID_MAX: u32 = 0xFFFFF; // 20-bit maximum (1048575)
const PASID_INVALID: u32 = 0x100000; // Just beyond 20-bit range

// ============================================================================
// Construction and Validation Tests
// ============================================================================

#[test]
fn test_pasid_valid_construction_new() {
    // Test valid PASID construction with new()
    let pasid = PASID::new(0);
    assert!(pasid.is_ok(), "PASID 0 should be valid (default address space)");

    let pasid = PASID::new(42);
    assert!(pasid.is_ok(), "PASID 42 should be valid");

    let pasid = PASID::new(PASID_MAX);
    assert!(pasid.is_ok(), "PASID 0xFFFFF should be valid (20-bit max)");
}

#[test]
fn test_pasid_valid_construction_try_from() {
    // Test valid PASID construction with try_from()
    let pasid = PASID::try_from(0u32);
    assert!(pasid.is_ok(), "PASID from u32(0) should be valid");

    let pasid = PASID::try_from(100u32);
    assert!(pasid.is_ok(), "PASID from u32(100) should be valid");

    let pasid = PASID::try_from(PASID_MAX);
    assert!(pasid.is_ok(), "PASID from u32(0xFFFFF) should be valid");
}

#[test]
fn test_pasid_invalid_construction_exceeds_20bit() {
    // Test that values exceeding 20-bit max are rejected
    let pasid = PASID::new(PASID_INVALID);
    assert!(pasid.is_err(), "PASID 0x100000 should fail (exceeds 20-bit)");

    let pasid = PASID::new(u32::MAX);
    assert!(pasid.is_err(), "PASID u32::MAX should fail validation");

    let pasid = PASID::new(0x00200000);
    assert!(pasid.is_err(), "PASID 0x200000 should fail validation");
}

#[test]
fn test_pasid_invalid_construction_try_from_error() {
    // Test try_from error handling
    let result = PASID::try_from(PASID_INVALID);
    assert!(result.is_err(), "try_from(0x100000) should return error");

    if let Err(e) = result {
        // Error should contain useful information
        let error_msg = format!("{}", e);
        assert!(!error_msg.is_empty(), "Error message should not be empty");
    }
}

// ============================================================================
// PASID 0 Support (Critical for ARM SMMU v3)
// ============================================================================

#[test]
fn test_pasid_zero_required() {
    // ARM SMMU v3 spec: PASID 0 represents the default address space
    // This is a CRITICAL requirement - PASID 0 must always be supported
    let pasid = PASID::new(0);
    assert!(pasid.is_ok(), "PASID 0 is REQUIRED by ARM SMMU v3 (default address space)");
}

#[test]
fn test_pasid_zero_roundtrip() {
    // PASID 0 must roundtrip correctly
    let pasid = PASID::new(0).expect("PASID 0 must be valid");
    assert_eq!(pasid.as_u32(), 0, "PASID 0 should convert back to 0");
}

#[test]
fn test_pasid_zero_default() {
    // Default PASID should be 0 (default address space)
    let default_pasid = PASID::default();
    assert_eq!(default_pasid.as_u32(), 0, "Default PASID must be 0");

    let zero_pasid = PASID::new(0).unwrap();
    assert_eq!(default_pasid, zero_pasid, "Default should equal PASID(0)");
}

// ============================================================================
// Boundary Value Tests (20-bit specific)
// ============================================================================

#[test]
fn test_pasid_boundary_zero() {
    // Test lower boundary
    let pasid = PASID::new(0);
    assert!(pasid.is_ok(), "PASID 0 must be supported");

    let p = pasid.unwrap();
    assert_eq!(p.as_u32(), 0, "PASID 0 should convert back to 0");
}

#[test]
fn test_pasid_boundary_max_valid() {
    // Test upper boundary (0xFFFFF = 1048575)
    let pasid = PASID::new(PASID_MAX);
    assert!(pasid.is_ok(), "PASID 0xFFFFF should be valid (20-bit max)");

    let p = pasid.unwrap();
    assert_eq!(p.as_u32(), PASID_MAX, "PASID should preserve 20-bit max value");
}

#[test]
fn test_pasid_boundary_one_beyond_max() {
    // Test first invalid value (0x100000 = 1048576)
    let pasid = PASID::new(PASID_INVALID);
    assert!(pasid.is_err(), "PASID 0x100000 should fail (beyond 20-bit)");
}

#[test]
fn test_pasid_boundary_just_below_max() {
    // Test value just below maximum
    let pasid = PASID::new(PASID_MAX - 1);
    assert!(pasid.is_ok(), "PASID 0xFFFFE should be valid");

    if let Ok(p) = pasid {
        assert_eq!(p.as_u32(), PASID_MAX - 1, "PASID should preserve value");
    }
}

#[test]
fn test_pasid_boundary_powers_of_two() {
    // Test power-of-two boundaries within 20-bit range
    let valid_powers = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024,
                        2048, 4096, 8192, 16384, 32768, 65536, 131072, 524288];

    for &power in &valid_powers {
        let pasid = PASID::new(power);
        assert!(pasid.is_ok(), "PASID {} should be valid", power);
    }

    // 0x100000 is first power of 2 beyond 20-bit
    let invalid_power = PASID::new(0x100000);
    assert!(invalid_power.is_err(), "PASID 0x100000 should be invalid");
}

// ============================================================================
// Display and Debug Formatting Tests
// ============================================================================

#[test]
fn test_pasid_display_formatting() {
    // Test Display trait implementation
    let pasid = PASID::new(42).unwrap();
    let display = format!("{}", pasid);

    // Should produce readable output like "PASID(42)"
    assert!(display.contains("42"), "Display should include value");
    assert!(display.contains("PASID"), "Display should include type name");
}

#[test]
fn test_pasid_debug_formatting() {
    // Test Debug trait implementation
    let pasid = PASID::new(123).unwrap();
    let debug = format!("{:?}", pasid);

    // Debug output should be informative
    assert!(debug.contains("123"), "Debug should include value");
}

#[test]
fn test_pasid_display_zero() {
    // Test special case of zero (default address space)
    let pasid = PASID::new(0).unwrap();
    let display = format!("{}", pasid);

    assert!(display.contains("0"), "Display should show 0");
}

#[test]
fn test_pasid_display_max() {
    // Test maximum value display
    let pasid = PASID::new(PASID_MAX).unwrap();
    let display = format!("{}", pasid);

    assert!(display.contains(&PASID_MAX.to_string()) || display.contains("FFFFF"),
            "Display should show maximum value");
}

// ============================================================================
// Copy and Clone Tests
// ============================================================================

#[test]
fn test_pasid_copy_behavior() {
    // PASID should implement Copy for efficiency
    let pasid1 = PASID::new(42).unwrap();
    let pasid2 = pasid1; // Copy should occur

    // Both should be usable after copy
    assert_eq!(pasid1.as_u32(), 42, "Original should still be valid");
    assert_eq!(pasid2.as_u32(), 42, "Copy should equal original");
}

#[test]
fn test_pasid_clone_behavior() {
    // PASID should implement Clone
    let pasid1 = PASID::new(100).unwrap();
    let pasid2 = pasid1.clone();

    assert_eq!(pasid1.as_u32(), pasid2.as_u32(), "Clone should equal original");
}

#[test]
fn test_pasid_copy_semantic() {
    // Verify Copy semantic - can use original after assignment
    let original = PASID::new(1).unwrap();
    let copy = original;

    // This should compile (proving Copy) and both should work
    let _ = original.as_u32();
    let _ = copy.as_u32();
}

// ============================================================================
// Equality and Comparison Tests
// ============================================================================

#[test]
fn test_pasid_equality() {
    // Test PartialEq implementation
    let p1 = PASID::new(42).unwrap();
    let p2 = PASID::new(42).unwrap();
    let p3 = PASID::new(43).unwrap();

    assert_eq!(p1, p2, "Equal values should compare equal");
    assert_ne!(p1, p3, "Different values should compare not equal");
}

#[test]
fn test_pasid_equality_reflexive() {
    // Test reflexivity: x == x
    let pasid = PASID::new(100).unwrap();
    assert_eq!(pasid, pasid, "PASID should equal itself");
}

#[test]
fn test_pasid_equality_symmetric() {
    // Test symmetry: if x == y then y == x
    let p1 = PASID::new(50).unwrap();
    let p2 = PASID::new(50).unwrap();

    assert_eq!(p1, p2, "p1 should equal p2");
    assert_eq!(p2, p1, "p2 should equal p1");
}

#[test]
fn test_pasid_equality_transitive() {
    // Test transitivity: if x == y and y == z then x == z
    let p1 = PASID::new(25).unwrap();
    let p2 = PASID::new(25).unwrap();
    let p3 = PASID::new(25).unwrap();

    assert_eq!(p1, p2, "p1 should equal p2");
    assert_eq!(p2, p3, "p2 should equal p3");
    assert_eq!(p1, p3, "p1 should equal p3 (transitivity)");
}

#[test]
fn test_pasid_equality_boundary_values() {
    // Test equality at boundaries
    let p0_1 = PASID::new(0).unwrap();
    let p0_2 = PASID::new(0).unwrap();
    assert_eq!(p0_1, p0_2, "PASID 0 should equal PASID 0");

    let pmax_1 = PASID::new(PASID_MAX).unwrap();
    let pmax_2 = PASID::new(PASID_MAX).unwrap();
    assert_eq!(pmax_1, pmax_2, "PASID max should equal PASID max");
}

// ============================================================================
// Hash Consistency Tests
// ============================================================================

#[test]
fn test_pasid_hash_consistency() {
    // Test Hash trait implementation
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let p1 = PASID::new(42).unwrap();
    let p2 = PASID::new(42).unwrap();

    let mut hasher1 = DefaultHasher::new();
    p1.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    p2.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    assert_eq!(hash1, hash2, "Equal PASIDs should have equal hashes");
}

#[test]
fn test_pasid_hash_different_values() {
    // Different values should (likely) have different hashes
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let p1 = PASID::new(1).unwrap();
    let p2 = PASID::new(2).unwrap();

    let mut hasher1 = DefaultHasher::new();
    p1.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    p2.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    // While hash collisions are theoretically possible, they're unlikely for small values
    assert_ne!(hash1, hash2, "Different PASIDs should (likely) have different hashes");
}

#[test]
fn test_pasid_as_hash_map_key() {
    // PASID should work as HashMap key
    let mut map = HashMap::new();

    let p0 = PASID::new(0).unwrap();
    let p1 = PASID::new(1).unwrap();
    let pmax = PASID::new(PASID_MAX).unwrap();

    map.insert(p0, "default address space");
    map.insert(p1, "process 1");
    map.insert(pmax, "max PASID");

    assert_eq!(map.get(&p0), Some(&"default address space"), "Should find PASID 0");
    assert_eq!(map.get(&p1), Some(&"process 1"), "Should find PASID 1");
    assert_eq!(map.get(&pmax), Some(&"max PASID"), "Should find max PASID");
    assert_eq!(map.len(), 3, "Map should have 3 entries");
}

// ============================================================================
// Default Value Tests
// ============================================================================

#[test]
fn test_pasid_default() {
    // Test Default trait implementation
    let pasid = PASID::default();

    // Default should be PASID(0) - the default address space
    assert_eq!(pasid.as_u32(), 0, "Default PASID should be 0");
}

#[test]
fn test_pasid_default_equality() {
    // Default should equal explicitly constructed 0
    let default_p = PASID::default();
    let zero_p = PASID::new(0).unwrap();

    assert_eq!(default_p, zero_p, "Default should equal PASID(0)");
}

// ============================================================================
// Conversion Tests
// ============================================================================

#[test]
fn test_pasid_to_u32() {
    // Test conversion to u32
    let pasid = PASID::new(12345).unwrap();
    let value = pasid.as_u32();

    assert_eq!(value, 12345, "as_u32() should return original value");
}

#[test]
fn test_pasid_from_u32_roundtrip() {
    // Test roundtrip conversion for various values
    let test_values = [0, 1, 42, 1000, 65535, PASID_MAX];

    for &original in &test_values {
        let pasid = PASID::try_from(original).unwrap();
        let converted = pasid.as_u32();

        assert_eq!(original, converted,
                   "Roundtrip conversion should preserve value {}", original);
    }
}

#[test]
fn test_pasid_into_u32() {
    // Test Into<u32> if implemented
    let pasid = PASID::new(999).unwrap();
    let value: u32 = pasid.into();

    assert_eq!(value, 999, "Into<u32> should return original value");
}

// ============================================================================
// Validation Error Tests
// ============================================================================

#[test]
fn test_validation_error_display_pasid() {
    // Test that ValidationError implements Display
    let result = PASID::new(PASID_INVALID);
    assert!(result.is_err(), "Should return validation error");

    if let Err(error) = result {
        let message = format!("{}", error);
        assert!(!message.is_empty(), "Error message should not be empty");
        assert!(message.contains("PASID") || message.contains("pasid") || message.contains("20-bit"),
                "Error should mention PASID or 20-bit constraint");
    }
}

#[test]
fn test_validation_error_debug_pasid() {
    // Test that ValidationError implements Debug
    let result = PASID::new(u32::MAX);

    if let Err(error) = result {
        let debug_msg = format!("{:?}", error);
        assert!(!debug_msg.is_empty(), "Debug message should not be empty");
    }
}

#[test]
fn test_validation_error_context_pasid() {
    // Test that error contains useful context
    let invalid_value = PASID_INVALID;
    let result = PASID::new(invalid_value);

    if let Err(error) = result {
        let message = format!("{}", error);
        // Error should ideally contain:
        // - The invalid value
        // - The maximum allowed value (0xFFFFF)
        // - Mention of 20-bit constraint
        let has_context = message.contains(&invalid_value.to_string())
            || message.contains("FFFFF")
            || message.contains("20")
            || message.len() > 20;

        assert!(has_context, "Error should provide useful context");
    }
}

// ============================================================================
// ARM SMMU v3 Specification Compliance Tests
// ============================================================================

#[test]
fn test_pasid_arm_spec_20bit_max() {
    // ARM SMMU v3 spec Section 3.6: PASID is a 20-bit value
    // Maximum value is 0xFFFFF (1048575)

    let pasid = PASID::new(PASID_MAX);
    assert!(pasid.is_ok(), "PASID 0xFFFFF should be valid per ARM SMMU v3");

    let pasid = PASID::new(PASID_INVALID);
    assert!(pasid.is_err(), "PASID 0x100000 should be invalid (exceeds 20-bit)");
}

#[test]
fn test_pasid_arm_spec_default_address_space() {
    // ARM SMMU v3 spec: PASID 0 represents the default address space
    // This is used when PASID is not enabled or for default translations

    let pasid = PASID::new(0);
    assert!(pasid.is_ok(), "PASID 0 must be valid (default address space)");

    let default_pasid = PASID::default();
    assert_eq!(default_pasid.as_u32(), 0,
               "Default PASID must be 0 (default address space)");
}

#[test]
fn test_pasid_arm_spec_typical_range() {
    // Test common PASID values used in typical systems
    let typical_values = [0, 1, 2, 10, 100, 1000, 10000, 65535];

    for &value in &typical_values {
        let pasid = PASID::new(value);
        assert!(pasid.is_ok(),
                "PASID {} should be valid (typical range)", value);
    }
}

#[test]
fn test_pasid_arm_spec_full_20bit_range() {
    // Verify that all 20-bit values are accepted
    let test_values = [
        0x00000,  // Minimum
        0x00001,  // Minimum + 1
        0x0FFFF,  // 16-bit max
        0x10000,  // Beyond 16-bit
        0xFFFFE,  // Maximum - 1
        0xFFFFF,  // Maximum
    ];

    for &value in &test_values {
        let pasid = PASID::new(value);
        assert!(pasid.is_ok(),
                "PASID 0x{:X} should be valid (within 20-bit range)", value);
    }

    // Beyond 20-bit should fail
    let invalid_values = [
        0x100000,  // First invalid
        0x200000,  // Well beyond
        u32::MAX,  // Maximum u32
    ];

    for &value in &invalid_values {
        let pasid = PASID::new(value);
        assert!(pasid.is_err(),
                "PASID 0x{:X} should be invalid (exceeds 20-bit)", value);
    }
}

// ============================================================================
// Property-Based Tests (using manual property testing)
// ============================================================================

#[test]
fn test_pasid_roundtrip_property() {
    // Property: Any valid PASID can roundtrip through u32
    let test_values = [0, 1, 2, 10, 100, 1000, 10000, 65535, 524288, PASID_MAX];

    for &value in &test_values {
        let pasid = PASID::new(value).expect("Test value should be valid");
        let roundtrip = pasid.as_u32();
        assert_eq!(value, roundtrip,
                   "PASID {} should roundtrip through u32", value);
    }
}

#[test]
fn test_pasid_hash_equals_property() {
    // Property: If a == b, then hash(a) == hash(b)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let test_values = [0, 1, 42, 100, 1000, 65535, PASID_MAX];

    for &value in &test_values {
        let p1 = PASID::new(value).unwrap();
        let p2 = PASID::new(value).unwrap();

        assert_eq!(p1, p2, "Values should be equal");

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        p1.hash(&mut h1);
        p2.hash(&mut h2);

        assert_eq!(h1.finish(), h2.finish(),
                  "Equal values should have equal hashes");
    }
}

#[test]
fn test_pasid_copy_equals_property() {
    // Property: Copy should create an equal value
    let test_values = [0, 1, 42, 100, 1000, 65535, PASID_MAX];

    for &value in &test_values {
        let original = PASID::new(value).unwrap();
        let copy = original;
        assert_eq!(original, copy,
                   "Copy should equal original for value {}", value);
    }
}

#[test]
fn test_pasid_validation_consistency_property() {
    // Property: Validation should be consistent (same input, same result)
    let test_values = [0, PASID_MAX, PASID_INVALID, u32::MAX];

    for &value in &test_values {
        let result1 = PASID::new(value);
        let result2 = PASID::new(value);

        match (result1, result2) {
            (Ok(p1), Ok(p2)) => assert_eq!(p1, p2, "Valid results should match"),
            (Err(_), Err(_)) => {}, // Both errors, consistent
            _ => panic!("Inconsistent validation for value {}", value),
        }
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_pasid_20bit_boundary_exhaustive() {
    // Exhaustively test around the 20-bit boundary
    let boundary = PASID_MAX;

    // Just below boundary
    for offset in 1..=10 {
        let value = boundary - offset;
        let pasid = PASID::new(value);
        assert!(pasid.is_ok(),
                "PASID {} should be valid (below 20-bit max)", value);
    }

    // At boundary
    assert!(PASID::new(boundary).is_ok(),
            "PASID at 20-bit max should be valid");

    // Just above boundary
    for offset in 1..=10 {
        let value = boundary + offset;
        let pasid = PASID::new(value);
        assert!(pasid.is_err(),
                "PASID {} should be invalid (above 20-bit max)", value);
    }
}

#[test]
fn test_pasid_bit_patterns() {
    // Test various bit patterns within 20-bit range
    let patterns = [
        0x00000, // All zeros
        0xFFFFF, // All ones (20-bit)
        0x55555, // Alternating 01
        0xAAAAA, // Alternating 10
        0x0F0F0, // Nibble pattern
        0xF0F0F, // Inverse nibble pattern
    ];

    for &pattern in &patterns {
        let pasid = PASID::new(pattern);
        assert!(pasid.is_ok(),
                "PASID 0x{:X} should be valid (20-bit pattern)", pattern);
    }

    // 21-bit patterns should fail
    let invalid_patterns = [
        0x100000, // Bit 20 set
        0x1FFFFF, // All 21 bits set
    ];

    for &pattern in &invalid_patterns {
        let pasid = PASID::new(pattern);
        assert!(pasid.is_err(),
                "PASID 0x{:X} should be invalid (21-bit pattern)", pattern);
    }
}

// ============================================================================
// Concurrency Tests (basic - ensures Send + Sync)
// ============================================================================

#[test]
fn test_pasid_is_send() {
    // Verify PASID is Send (can be sent between threads)
    fn assert_send<T: Send>() {}
    assert_send::<PASID>();
}

#[test]
fn test_pasid_is_sync() {
    // Verify PASID is Sync (can be shared between threads)
    fn assert_sync<T: Sync>() {}
    assert_sync::<PASID>();
}

// ============================================================================
// Documentation Tests
// ============================================================================

#[test]
fn test_pasid_basic_usage() {
    // Example from documentation should work
    let pasid = PASID::new(42).expect("Valid PASID");
    assert_eq!(pasid.as_u32(), 42);
}

#[test]
fn test_pasid_default_address_space_usage() {
    // Example: Using PASID 0 for default address space
    let default_pasid = PASID::new(0).expect("PASID 0 must be valid");
    assert_eq!(default_pasid.as_u32(), 0);

    // Or use Default
    let default_pasid = PASID::default();
    assert_eq!(default_pasid.as_u32(), 0);
}

#[test]
fn test_pasid_error_handling() {
    // Example error handling from documentation
    match PASID::new(PASID_INVALID) {
        Ok(_) => panic!("Should not accept value beyond 20-bit"),
        Err(e) => {
            // Error should be informative
            let _ = format!("{}", e);
        }
    }
}

#[test]
fn test_pasid_max_value_usage() {
    // Example: Using maximum PASID value
    let max_pasid = PASID::new(PASID_MAX).expect("Maximum PASID should be valid");
    assert_eq!(max_pasid.as_u32(), 0xFFFFF);
}
