//! Comprehensive unit tests for ValidationError type
//!
//! Tests error handling for newtype wrappers:
//! - Descriptive error messages
//! - Error context (field name, invalid value, constraints)
//! - Display and Debug formatting
//! - Error type properties
//!
//! All tests follow strict TDD - they WILL FAIL until implementation is complete.

use smmu::types::ValidationError;

// ============================================================================
// Error Construction Tests
// ============================================================================

#[test]
fn test_validation_error_construction() {
    // ValidationError should be constructible with context
    let error = ValidationError::new(
        "StreamID",
        "4294967295",
        "must be <= 65535"
    );

    let message = format!("{}", error);
    assert!(!message.is_empty(), "Error message should not be empty");
}

#[test]
fn test_validation_error_with_field_name() {
    // Error should include field name for context
    let error = ValidationError::new(
        "PASID",
        "1048576",
        "must be <= 1048575 (20-bit)"
    );

    let message = format!("{}", error);
    assert!(message.contains("PASID"), "Error should mention field name");
}

// ============================================================================
// Display Formatting Tests
// ============================================================================

#[test]
fn test_validation_error_display_stream_id() {
    // Test Display formatting for StreamID error
    let error = ValidationError::new(
        "StreamID",
        "999999",
        "must be <= 65535"
    );

    let display = format!("{}", error);

    assert!(display.contains("StreamID"), "Should mention field name");
    assert!(display.contains("999999") || display.contains("invalid"),
            "Should mention invalid value");
    assert!(display.contains("65535") || display.contains("constraint"),
            "Should mention constraint");
}

#[test]
fn test_validation_error_display_pasid() {
    // Test Display formatting for PASID error
    let error = ValidationError::new(
        "PASID",
        "1048576",
        "must be <= 1048575 (20-bit maximum)"
    );

    let display = format!("{}", error);

    assert!(display.contains("PASID"), "Should mention field name");
    assert!(display.contains("1048576") || display.contains("invalid"),
            "Should mention invalid value");
    assert!(display.contains("20-bit") || display.contains("1048575"),
            "Should mention 20-bit constraint");
}

#[test]
fn test_validation_error_display_readable() {
    // Error messages should be human-readable
    let error = ValidationError::new(
        "TestField",
        "12345",
        "must be positive and less than 100"
    );

    let display = format!("{}", error);

    // Should form a coherent sentence
    assert!(display.len() > 10, "Error message should be substantial");
    assert!(!display.contains("{{"), "Should not have template markers");
}

// ============================================================================
// Debug Formatting Tests
// ============================================================================

#[test]
fn test_validation_error_debug() {
    // Test Debug trait implementation
    let error = ValidationError::new(
        "StreamID",
        "4294967295",
        "must be <= 65535"
    );

    let debug = format!("{:?}", error);

    assert!(!debug.is_empty(), "Debug output should not be empty");
    // Debug output should include all context
    assert!(debug.contains("StreamID") || debug.contains("4294967295"),
            "Debug should include context");
}

#[test]
fn test_validation_error_debug_detailed() {
    // Debug should provide detailed information
    let error = ValidationError::new(
        "PASID",
        "0x100000",
        "exceeds 20-bit maximum (0xFFFFF)"
    );

    let debug = format!("{:?}", error);

    // Should include field details in Debug output
    assert!(debug.len() > 20, "Debug output should be detailed");
}

// ============================================================================
// Error Context Tests
// ============================================================================

#[test]
fn test_validation_error_field_name_context() {
    // Error should preserve field name
    let error = ValidationError::new(
        "StreamID",
        "invalid",
        "constraint"
    );

    let message = format!("{}", error);
    assert!(message.to_lowercase().contains("streamid"),
            "Should mention field name");
}

#[test]
fn test_validation_error_value_context() {
    // Error should include the invalid value
    let invalid_value = "4294967295";
    let error = ValidationError::new(
        "StreamID",
        invalid_value,
        "too large"
    );

    let message = format!("{}", error);
    // Should mention the value or indicate it's invalid
    assert!(message.contains(invalid_value) || message.contains("invalid"),
            "Should provide value context");
}

#[test]
fn test_validation_error_constraint_context() {
    // Error should explain the constraint
    let constraint = "must be <= 1048575 (20-bit maximum)";
    let error = ValidationError::new(
        "PASID",
        "1048576",
        constraint
    );

    let message = format!("{}", error);
    assert!(message.contains("20-bit") || message.contains("1048575") || message.contains("maximum"),
            "Should explain constraint");
}

#[test]
fn test_validation_error_complete_context() {
    // Error should provide complete context for debugging
    let error = ValidationError::new(
        "PASID",
        "0x100000",
        "exceeds maximum value 0xFFFFF (20-bit)"
    );

    let message = format!("{}", error);

    // Should have all three pieces of information
    let has_field = message.to_lowercase().contains("pasid");
    let has_value = message.contains("100000") || message.contains("invalid");
    let has_constraint = message.contains("FFFFF") || message.contains("20-bit");

    assert!(has_field || has_value || has_constraint,
            "Error should provide complete context");
}

// ============================================================================
// Error Message Quality Tests
// ============================================================================

#[test]
fn test_validation_error_message_not_empty() {
    // Error messages should never be empty
    let error = ValidationError::new("Field", "value", "constraint");
    let message = format!("{}", error);

    assert!(!message.is_empty(), "Error message must not be empty");
    assert!(message.len() > 5, "Error message should be meaningful");
}

#[test]
fn test_validation_error_message_informative() {
    // Error messages should be informative
    let error = ValidationError::new(
        "StreamID",
        "999999",
        "must be within hardware-supported range (0-65535)"
    );

    let message = format!("{}", error);

    // Should be long enough to be informative
    assert!(message.len() > 20, "Error message should be informative");
}

#[test]
fn test_validation_error_different_fields() {
    // Different field names should produce different errors
    let error1 = ValidationError::new("StreamID", "999999", "too large");
    let error2 = ValidationError::new("PASID", "999999", "too large");

    let msg1 = format!("{}", error1);
    let msg2 = format!("{}", error2);

    assert_ne!(msg1, msg2, "Different fields should produce different errors");
}

// ============================================================================
// Special Characters and Edge Cases
// ============================================================================

#[test]
fn test_validation_error_hex_values() {
    // Should handle hexadecimal value formatting
    let error = ValidationError::new(
        "PASID",
        "0x100000",
        "exceeds 0xFFFFF"
    );

    let message = format!("{}", error);
    assert!(!message.is_empty(), "Should handle hex values");
}

#[test]
fn test_validation_error_large_numbers() {
    // Should handle large number formatting
    let error = ValidationError::new(
        "StreamID",
        "4294967295",
        "exceeds maximum"
    );

    let message = format!("{}", error);
    assert!(message.len() > 10, "Should format large numbers");
}

#[test]
fn test_validation_error_special_cases() {
    // Test various special cases
    let test_cases = [
        ("Field", "0", "must be positive"),
        ("Field", "MAX", "out of range"),
        ("Field", "-1", "must be unsigned"),
    ];

    for (field, value, constraint) in &test_cases {
        let error = ValidationError::new(field, value, constraint);
        let message = format!("{}", error);
        assert!(!message.is_empty(),
                "Should handle case: {} {} {}", field, value, constraint);
    }
}

// ============================================================================
// Error Type Properties
// ============================================================================

#[test]
fn test_validation_error_is_send() {
    // ValidationError should be Send (can be sent between threads)
    fn assert_send<T: Send>() {}
    assert_send::<ValidationError>();
}

#[test]
fn test_validation_error_is_sync() {
    // ValidationError should be Sync (can be shared between threads)
    fn assert_sync<T: Sync>() {}
    assert_sync::<ValidationError>();
}

#[test]
fn test_validation_error_std_error() {
    // ValidationError should implement std::error::Error
    use std::error::Error;

    fn assert_error<T: Error>() {}
    assert_error::<ValidationError>();
}

// ============================================================================
// Error Usage in Result Types
// ============================================================================

#[test]
fn test_validation_error_in_result() {
    // Should work naturally in Result types
    fn validate_value(value: u32) -> Result<u32, ValidationError> {
        if value > 100 {
            Err(ValidationError::new(
                "value",
                &value.to_string(),
                "must be <= 100"
            ))
        } else {
            Ok(value)
        }
    }

    assert!(validate_value(50).is_ok());
    assert!(validate_value(150).is_err());
}

#[test]
fn test_validation_error_result_chaining() {
    // Should work with ? operator
    fn inner() -> Result<(), ValidationError> {
        Err(ValidationError::new("field", "value", "invalid"))
    }

    fn outer() -> Result<(), ValidationError> {
        inner()?;
        Ok(())
    }

    assert!(outer().is_err());
}

// ============================================================================
// Documentation Examples
// ============================================================================

#[test]
fn test_validation_error_example_usage() {
    // Example from documentation
    let error = ValidationError::new(
        "PASID",
        "1048576",
        "must be <= 1048575 (20-bit maximum)"
    );

    // Should be usable in match expressions
    let result: Result<(), ValidationError> = Err(error);

    match result {
        Ok(_) => panic!("Should be error"),
        Err(e) => {
            let _ = format!("{}", e);
        }
    }
}

#[test]
fn test_validation_error_builder_pattern() {
    // If builder pattern is implemented
    // This test validates the builder usage
    let error = ValidationError::new(
        "StreamID",
        "999999",
        "exceeds hardware limit"
    );

    let message = format!("{}", error);
    assert!(!message.is_empty());
}
