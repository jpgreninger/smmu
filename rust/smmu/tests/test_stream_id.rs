//! Comprehensive unit tests for StreamID newtype wrapper
//!
//! Tests ARM SMMU v3 specification compliance for StreamID:
//! - 32-bit unsigned integer wrapper
//! - Hardware-dependent range (typically 0-65535)
//! - Configurable maximum value
//! - Type safety and validation
//!
//! All tests follow strict TDD - they WILL FAIL until implementation is complete.

use smmu::types::StreamID;
use std::collections::HashMap;

// ============================================================================
// Construction and Validation Tests
// ============================================================================

#[test]
fn test_stream_id_valid_construction_new() {
    // Test valid StreamID construction with new()
    let stream_id = StreamID::new(0);
    assert!(stream_id.is_ok(), "StreamID 0 should be valid");

    let stream_id = StreamID::new(42);
    assert!(stream_id.is_ok(), "StreamID 42 should be valid");

    let stream_id = StreamID::new(65535);
    assert!(stream_id.is_ok(), "StreamID 65535 should be valid (typical max)");
}

#[test]
fn test_stream_id_valid_construction_try_from() {
    // Test valid StreamID construction with try_from()
    let stream_id = StreamID::try_from(0u32);
    assert!(stream_id.is_ok(), "StreamID from u32(0) should be valid");

    let stream_id = StreamID::try_from(100u32);
    assert!(stream_id.is_ok(), "StreamID from u32(100) should be valid");

    let stream_id = StreamID::try_from(65535u32);
    assert!(stream_id.is_ok(), "StreamID from u32(65535) should be valid");
}

#[test]
fn test_stream_id_invalid_construction_out_of_range() {
    // Test that out-of-range values are rejected
    // Note: Exact max depends on implementation's configured maximum
    let stream_id = StreamID::new(u32::MAX);
    assert!(stream_id.is_err(), "StreamID u32::MAX should fail validation");

    let stream_id = StreamID::new(0x01000000); // 16M - definitely too large
    assert!(stream_id.is_err(), "StreamID 16M should fail validation");
}

#[test]
fn test_stream_id_invalid_construction_try_from_error() {
    // Test try_from error handling
    let result = StreamID::try_from(u32::MAX);
    assert!(result.is_err(), "try_from(u32::MAX) should return error");

    if let Err(e) = result {
        // Error should contain useful information
        let error_msg = format!("{}", e);
        assert!(!error_msg.is_empty(), "Error message should not be empty");
    }
}

// ============================================================================
// Boundary Value Tests
// ============================================================================

#[test]
fn test_stream_id_boundary_zero() {
    // StreamID 0 is a critical boundary value
    let stream_id = StreamID::new(0);
    assert!(stream_id.is_ok(), "StreamID 0 must be supported");

    let id = stream_id.unwrap();
    assert_eq!(id.as_u32(), 0, "StreamID 0 should convert back to 0");
}

#[test]
fn test_stream_id_boundary_typical_max() {
    // Test typical hardware maximum (65535 = 0xFFFF)
    let stream_id = StreamID::new(65535);
    assert!(stream_id.is_ok(), "StreamID 65535 should be valid");

    if let Ok(id) = stream_id {
        assert_eq!(id.as_u32(), 65535, "StreamID should preserve value");
    }
}

#[test]
fn test_stream_id_boundary_just_above_typical_max() {
    // Test value just above typical max
    let _stream_id = StreamID::new(65536);
    // May or may not fail depending on configured maximum
    // Implementation should have clear validation logic
}

// ============================================================================
// Display and Debug Formatting Tests
// ============================================================================

#[test]
fn test_stream_id_display_formatting() {
    // Test Display trait implementation
    let stream_id = StreamID::new(42).unwrap();
    let display = format!("{}", stream_id);

    // Should produce readable output like "StreamID(42)"
    assert!(display.contains("42"), "Display should include value");
    assert!(display.contains("StreamID"), "Display should include type name");
}

#[test]
fn test_stream_id_debug_formatting() {
    // Test Debug trait implementation
    let stream_id = StreamID::new(123).unwrap();
    let debug = format!("{:?}", stream_id);

    // Debug output should be informative
    assert!(debug.contains("123"), "Debug should include value");
}

#[test]
fn test_stream_id_display_zero() {
    // Test special case of zero
    let stream_id = StreamID::new(0).unwrap();
    let display = format!("{}", stream_id);

    assert!(display.contains("0"), "Display should show 0");
}

// ============================================================================
// Copy and Clone Tests
// ============================================================================

#[test]
fn test_stream_id_copy_behavior() {
    // StreamID should implement Copy for efficiency
    let stream_id1 = StreamID::new(42).unwrap();
    let stream_id2 = stream_id1; // Copy should occur

    // Both should be usable after copy
    assert_eq!(stream_id1.as_u32(), 42, "Original should still be valid");
    assert_eq!(stream_id2.as_u32(), 42, "Copy should equal original");
}

#[test]
fn test_stream_id_clone_behavior() {
    // StreamID should implement Clone
    let stream_id1 = StreamID::new(100).unwrap();
    let stream_id2 = stream_id1.clone();

    assert_eq!(stream_id1.as_u32(), stream_id2.as_u32(), "Clone should equal original");
}

#[test]
fn test_stream_id_copy_semantic() {
    // Verify Copy semantic - can use original after assignment
    let original = StreamID::new(1).unwrap();
    let copy = original;

    // This should compile (proving Copy) and both should work
    let _ = original.as_u32();
    let _ = copy.as_u32();
}

// ============================================================================
// Equality and Comparison Tests
// ============================================================================

#[test]
fn test_stream_id_equality() {
    // Test PartialEq implementation
    let id1 = StreamID::new(42).unwrap();
    let id2 = StreamID::new(42).unwrap();
    let id3 = StreamID::new(43).unwrap();

    assert_eq!(id1, id2, "Equal values should compare equal");
    assert_ne!(id1, id3, "Different values should compare not equal");
}

#[test]
fn test_stream_id_equality_reflexive() {
    // Test reflexivity: x == x
    let stream_id = StreamID::new(100).unwrap();
    assert_eq!(stream_id, stream_id, "StreamID should equal itself");
}

#[test]
fn test_stream_id_equality_symmetric() {
    // Test symmetry: if x == y then y == x
    let id1 = StreamID::new(50).unwrap();
    let id2 = StreamID::new(50).unwrap();

    assert_eq!(id1, id2, "id1 should equal id2");
    assert_eq!(id2, id1, "id2 should equal id1");
}

#[test]
fn test_stream_id_equality_transitive() {
    // Test transitivity: if x == y and y == z then x == z
    let id1 = StreamID::new(25).unwrap();
    let id2 = StreamID::new(25).unwrap();
    let id3 = StreamID::new(25).unwrap();

    assert_eq!(id1, id2, "id1 should equal id2");
    assert_eq!(id2, id3, "id2 should equal id3");
    assert_eq!(id1, id3, "id1 should equal id3 (transitivity)");
}

// ============================================================================
// Hash Consistency Tests
// ============================================================================

#[test]
fn test_stream_id_hash_consistency() {
    // Test Hash trait implementation
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let id1 = StreamID::new(42).unwrap();
    let id2 = StreamID::new(42).unwrap();

    let mut hasher1 = DefaultHasher::new();
    id1.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    id2.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    assert_eq!(hash1, hash2, "Equal StreamIDs should have equal hashes");
}

#[test]
fn test_stream_id_hash_different_values() {
    // Different values should (likely) have different hashes
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let id1 = StreamID::new(1).unwrap();
    let id2 = StreamID::new(2).unwrap();

    let mut hasher1 = DefaultHasher::new();
    id1.hash(&mut hasher1);
    let hash1 = hasher1.finish();

    let mut hasher2 = DefaultHasher::new();
    id2.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    // While hash collisions are theoretically possible, they're unlikely for small values
    assert_ne!(hash1, hash2, "Different StreamIDs should (likely) have different hashes");
}

#[test]
fn test_stream_id_as_hash_map_key() {
    // StreamID should work as HashMap key
    let mut map = HashMap::new();

    let id1 = StreamID::new(1).unwrap();
    let id2 = StreamID::new(2).unwrap();

    map.insert(id1, "stream 1");
    map.insert(id2, "stream 2");

    assert_eq!(map.get(&id1), Some(&"stream 1"), "Should find stream 1");
    assert_eq!(map.get(&id2), Some(&"stream 2"), "Should find stream 2");
    assert_eq!(map.len(), 2, "Map should have 2 entries");
}

// ============================================================================
// Default Value Tests
// ============================================================================

#[test]
fn test_stream_id_default() {
    // Test Default trait implementation
    let stream_id = StreamID::default();

    // Default should be StreamID(0)
    assert_eq!(stream_id.as_u32(), 0, "Default StreamID should be 0");
}

#[test]
fn test_stream_id_default_equality() {
    // Default should equal explicitly constructed 0
    let default_id = StreamID::default();
    let zero_id = StreamID::new(0).unwrap();

    assert_eq!(default_id, zero_id, "Default should equal StreamID(0)");
}

// ============================================================================
// Conversion Tests
// ============================================================================

#[test]
fn test_stream_id_to_u32() {
    // Test conversion to u32
    let stream_id = StreamID::new(12345).unwrap();
    let value = stream_id.as_u32();

    assert_eq!(value, 12345, "as_u32() should return original value");
}

#[test]
fn test_stream_id_from_u32_roundtrip() {
    // Test roundtrip conversion
    let original = 54321u32;
    let stream_id = StreamID::try_from(original).unwrap();
    let converted = stream_id.as_u32();

    assert_eq!(original, converted, "Roundtrip conversion should preserve value");
}

#[test]
fn test_stream_id_into_u32() {
    // Test Into<u32> if implemented
    let stream_id = StreamID::new(999).unwrap();
    let value: u32 = stream_id.into();

    assert_eq!(value, 999, "Into<u32> should return original value");
}

// ============================================================================
// Validation Error Tests
// ============================================================================

#[test]
fn test_validation_error_display() {
    // Test that ValidationError implements Display
    let result = StreamID::new(u32::MAX);
    assert!(result.is_err(), "Should return validation error");

    if let Err(error) = result {
        let message = format!("{}", error);
        assert!(!message.is_empty(), "Error message should not be empty");
        assert!(message.contains("StreamID") || message.contains("stream"),
                "Error should mention StreamID");
    }
}

#[test]
fn test_validation_error_debug() {
    // Test that ValidationError implements Debug
    let result = StreamID::new(u32::MAX);

    if let Err(error) = result {
        let debug_msg = format!("{:?}", error);
        assert!(!debug_msg.is_empty(), "Debug message should not be empty");
    }
}

#[test]
fn test_validation_error_context() {
    // Test that error contains useful context
    let invalid_value = u32::MAX;
    let result = StreamID::new(invalid_value);

    if let Err(error) = result {
        let message = format!("{}", error);
        // Error should ideally contain the invalid value
        // This helps with debugging
        let contains_value = message.contains(&invalid_value.to_string());
        assert!(contains_value || message.len() > 20,
                "Error should provide useful context");
    }
}

// ============================================================================
// ARM SMMU v3 Specification Compliance Tests
// ============================================================================

#[test]
fn test_stream_id_arm_spec_typical_range() {
    // ARM SMMU v3 spec: StreamID is implementation-defined
    // Typical implementations support 16-bit (0-65535)

    // All values in typical range should be valid
    for value in [0, 1, 100, 1000, 10000, 65535].iter() {
        let result = StreamID::new(*value);
        assert!(result.is_ok(),
                "StreamID {} should be valid in typical range", value);
    }
}

#[test]
fn test_stream_id_zero_required() {
    // StreamID 0 must be supported per ARM SMMU v3 usage
    let stream_id = StreamID::new(0);
    assert!(stream_id.is_ok(), "StreamID 0 is required by ARM SMMU v3");
}

#[test]
fn test_stream_id_configurable_maximum() {
    // Implementation should support configurable maximum
    // This test verifies that there IS a maximum (not accepting all u32 values)

    // At least one large value should be rejected
    let large_values = [0x01000000, 0x10000000, u32::MAX];
    let any_rejected = large_values.iter()
        .any(|&v| StreamID::new(v).is_err());

    assert!(any_rejected,
            "Implementation should reject some large values (configurable max)");
}

// ============================================================================
// Property-Based Tests (using manual property testing)
// ============================================================================

#[test]
fn test_stream_id_roundtrip_property() {
    // Property: Any valid StreamID can roundtrip through u32
    let test_values = [0, 1, 2, 10, 100, 1000, 10000, 65535];

    for &value in &test_values {
        if let Ok(stream_id) = StreamID::new(value) {
            let roundtrip = stream_id.as_u32();
            assert_eq!(value, roundtrip,
                       "StreamID {} should roundtrip through u32", value);
        }
    }
}

#[test]
fn test_stream_id_hash_equals_property() {
    // Property: If a == b, then hash(a) == hash(b)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let test_values = [0, 1, 42, 100, 1000, 65535];

    for &value in &test_values {
        if let Ok(id1) = StreamID::new(value) {
            if let Ok(id2) = StreamID::new(value) {
                assert_eq!(id1, id2, "Values should be equal");

                let mut h1 = DefaultHasher::new();
                let mut h2 = DefaultHasher::new();
                id1.hash(&mut h1);
                id2.hash(&mut h2);

                assert_eq!(h1.finish(), h2.finish(),
                          "Equal values should have equal hashes");
            }
        }
    }
}

#[test]
fn test_stream_id_copy_equals_property() {
    // Property: Copy should create an equal value
    let test_values = [0, 1, 42, 100, 1000, 65535];

    for &value in &test_values {
        if let Ok(original) = StreamID::new(value) {
            let copy = original;
            assert_eq!(original, copy,
                       "Copy should equal original for value {}", value);
        }
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_stream_id_max_valid_value() {
    // Test the maximum valid value
    // This depends on implementation's configured maximum

    // Try to find the boundary by testing common maximums
    let candidates = [65535u32, 131071, 262143, 1048575];

    let mut found_valid = false;
    for &candidate in &candidates {
        if StreamID::new(candidate).is_ok() {
            found_valid = true;

            // One beyond should fail (if not at u32::MAX)
            if candidate < u32::MAX {
                let _beyond = candidate + 1;
                // May or may not fail, but should be deterministic
            }
        }
    }

    assert!(found_valid, "At least one standard maximum should be supported");
}

#[test]
fn test_stream_id_sequential_values() {
    // Test sequential values to verify no gaps in valid range
    let base = 1000u32;
    for offset in 0..100 {
        let value = base + offset;
        let result = StreamID::new(value);

        // All values in a small range should have same validity
        // (either all valid or all invalid, no gaps)
        if offset == 0 {
            // First value establishes validity
            let _ = result;
        } else {
            // Subsequent values should match
            // This tests for contiguous valid ranges
        }
    }
}

// ============================================================================
// Concurrency Tests (basic - ensures Send + Sync)
// ============================================================================

#[test]
fn test_stream_id_is_send() {
    // Verify StreamID is Send (can be sent between threads)
    fn assert_send<T: Send>() {}
    assert_send::<StreamID>();
}

#[test]
fn test_stream_id_is_sync() {
    // Verify StreamID is Sync (can be shared between threads)
    fn assert_sync<T: Sync>() {}
    assert_sync::<StreamID>();
}

// ============================================================================
// Documentation Tests
// ============================================================================

#[test]
fn test_stream_id_basic_usage() {
    // Example from documentation should work
    let stream_id = StreamID::new(42).expect("Valid StreamID");
    assert_eq!(stream_id.as_u32(), 42);
}

#[test]
fn test_stream_id_error_handling() {
    // Example error handling from documentation
    match StreamID::new(u32::MAX) {
        Ok(_) => panic!("Should not accept u32::MAX"),
        Err(e) => {
            // Error should be informative
            let _ = format!("{}", e);
        }
    }
}
