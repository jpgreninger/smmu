//! Comprehensive tests for AccessType enum
//!
//! Tests cover:
//! - AccessType enum variants (Read, Write, Execute, ReadWrite, etc.)
//! - Permission operations and bitwise operations
//! - Permission intersection and validation
//! - ARM SMMU v3 access type encoding
//! - Copy, Clone, Debug, PartialEq, Eq traits
//! - Const methods for compile-time evaluation

use smmu::{AccessType, ValidationError};

// ============================================================================
// Basic AccessType Creation and Validation
// ============================================================================

#[test]
fn test_access_type_read() {
    let access = AccessType::Read;
    assert!(access.can_read());
    assert!(!access.can_write());
    assert!(!access.can_execute());
}

#[test]
fn test_access_type_write() {
    let access = AccessType::Write;
    assert!(!access.can_read());
    assert!(access.can_write());
    assert!(!access.can_execute());
}

#[test]
fn test_access_type_execute() {
    let access = AccessType::Execute;
    assert!(!access.can_read());
    assert!(!access.can_write());
    assert!(access.can_execute());
}

#[test]
fn test_access_type_read_write() {
    let access = AccessType::ReadWrite;
    assert!(access.can_read());
    assert!(access.can_write());
    assert!(!access.can_execute());
}

#[test]
fn test_access_type_read_execute() {
    let access = AccessType::ReadExecute;
    assert!(access.can_read());
    assert!(!access.can_write());
    assert!(access.can_execute());
}

#[test]
fn test_access_type_read_write_execute() {
    let access = AccessType::ReadWriteExecute;
    assert!(access.can_read());
    assert!(access.can_write());
    assert!(access.can_execute());
}

// ============================================================================
// Permission Intersection Tests
// ============================================================================

#[test]
fn test_permission_intersection_read_read() {
    let perm1 = AccessType::Read;
    let perm2 = AccessType::Read;
    let result = perm1.intersect(perm2);

    assert_eq!(result, AccessType::Read);
}

#[test]
fn test_permission_intersection_read_write() {
    let perm1 = AccessType::Read;
    let perm2 = AccessType::Write;
    let result = perm1.intersect(perm2);

    // No common permissions
    assert_eq!(result, AccessType::None);
}

#[test]
fn test_permission_intersection_rw_read() {
    let perm1 = AccessType::ReadWrite;
    let perm2 = AccessType::Read;
    let result = perm1.intersect(perm2);

    assert_eq!(result, AccessType::Read);
}

#[test]
fn test_permission_intersection_rw_write() {
    let perm1 = AccessType::ReadWrite;
    let perm2 = AccessType::Write;
    let result = perm1.intersect(perm2);

    assert_eq!(result, AccessType::Write);
}

#[test]
fn test_permission_intersection_rw_rw() {
    let perm1 = AccessType::ReadWrite;
    let perm2 = AccessType::ReadWrite;
    let result = perm1.intersect(perm2);

    assert_eq!(result, AccessType::ReadWrite);
}

#[test]
fn test_permission_intersection_rwx_rx() {
    let perm1 = AccessType::ReadWriteExecute;
    let perm2 = AccessType::ReadExecute;
    let result = perm1.intersect(perm2);

    assert_eq!(result, AccessType::ReadExecute);
}

// ============================================================================
// Permission Union Tests
// ============================================================================

#[test]
fn test_permission_union_read_write() {
    let perm1 = AccessType::Read;
    let perm2 = AccessType::Write;
    let result = perm1.union(perm2);

    assert_eq!(result, AccessType::ReadWrite);
}

#[test]
fn test_permission_union_read_execute() {
    let perm1 = AccessType::Read;
    let perm2 = AccessType::Execute;
    let result = perm1.union(perm2);

    assert_eq!(result, AccessType::ReadExecute);
}

#[test]
fn test_permission_union_rw_execute() {
    let perm1 = AccessType::ReadWrite;
    let perm2 = AccessType::Execute;
    let result = perm1.union(perm2);

    assert_eq!(result, AccessType::ReadWriteExecute);
}

// ============================================================================
// Permission Checking Tests
// ============================================================================

#[test]
fn test_has_permission_read() {
    let page_perms = AccessType::ReadWrite;
    let requested = AccessType::Read;

    assert!(page_perms.has_permission(requested));
}

#[test]
fn test_has_permission_write_denied() {
    let page_perms = AccessType::Read;
    let requested = AccessType::Write;

    assert!(!page_perms.has_permission(requested));
}

#[test]
fn test_has_permission_execute_allowed() {
    let page_perms = AccessType::ReadExecute;
    let requested = AccessType::Execute;

    assert!(page_perms.has_permission(requested));
}

#[test]
fn test_has_permission_multiple() {
    let page_perms = AccessType::ReadWriteExecute;

    assert!(page_perms.has_permission(AccessType::Read));
    assert!(page_perms.has_permission(AccessType::Write));
    assert!(page_perms.has_permission(AccessType::Execute));
    assert!(page_perms.has_permission(AccessType::ReadWrite));
}

// ============================================================================
// ARM SMMU v3 Encoding Tests
// ============================================================================

#[test]
fn test_access_type_to_bits() {
    // ARM SMMU v3 defines specific bit encodings for access types
    assert_eq!(AccessType::Read.to_bits(), 0b001);
    assert_eq!(AccessType::Write.to_bits(), 0b010);
    assert_eq!(AccessType::Execute.to_bits(), 0b100);
    assert_eq!(AccessType::ReadWrite.to_bits(), 0b011);
    assert_eq!(AccessType::ReadExecute.to_bits(), 0b101);
    assert_eq!(AccessType::ReadWriteExecute.to_bits(), 0b111);
}

#[test]
fn test_access_type_from_bits() {
    assert_eq!(AccessType::from_bits(0b001), Ok(AccessType::Read));
    assert_eq!(AccessType::from_bits(0b010), Ok(AccessType::Write));
    assert_eq!(AccessType::from_bits(0b100), Ok(AccessType::Execute));
    assert_eq!(AccessType::from_bits(0b011), Ok(AccessType::ReadWrite));
    assert_eq!(AccessType::from_bits(0b101), Ok(AccessType::ReadExecute));
    assert_eq!(AccessType::from_bits(0b111), Ok(AccessType::ReadWriteExecute));
}

#[test]
fn test_access_type_from_bits_invalid() {
    // Invalid bit patterns should return error
    let result = AccessType::from_bits(0b1000);
    assert!(result.is_err());
    assert!(matches!(result, Err(ValidationError::InvalidAccessType { .. })));
}

#[test]
fn test_access_type_from_bits_zero() {
    // Zero bits = no permissions
    let result = AccessType::from_bits(0);
    assert_eq!(result, Ok(AccessType::None));
}

// ============================================================================
// Const Methods Tests
// ============================================================================

#[test]
fn test_const_can_read() {
    const READ_ACCESS: AccessType = AccessType::Read;
    const CAN_READ: bool = READ_ACCESS.const_can_read();
    assert!(CAN_READ);
}

#[test]
fn test_const_can_write() {
    const WRITE_ACCESS: AccessType = AccessType::Write;
    const CAN_WRITE: bool = WRITE_ACCESS.const_can_write();
    assert!(CAN_WRITE);
}

#[test]
fn test_const_can_execute() {
    const EXEC_ACCESS: AccessType = AccessType::Execute;
    const CAN_EXECUTE: bool = EXEC_ACCESS.const_can_execute();
    assert!(CAN_EXECUTE);
}

#[test]
fn test_const_evaluation() {
    // Test compile-time evaluation
    const _ACCESS1: AccessType = AccessType::Read;
    const _ACCESS2: AccessType = AccessType::ReadWrite;
    const _CAN_READ: bool = AccessType::ReadWrite.const_can_read();
    const _CAN_WRITE: bool = AccessType::ReadWrite.const_can_write();

    // If this compiles, const evaluation works
}

// ============================================================================
// Trait Implementation Tests
// ============================================================================

#[test]
fn test_access_type_copy_clone() {
    let access1 = AccessType::Read;
    let access2 = access1; // Copy
    let access3 = access1.clone(); // Clone

    assert_eq!(access1, access2);
    assert_eq!(access1, access3);
}

#[test]
fn test_access_type_equality() {
    let read1 = AccessType::Read;
    let read2 = AccessType::Read;
    let write = AccessType::Write;

    assert_eq!(read1, read2);
    assert_ne!(read1, write);
}

#[test]
fn test_access_type_debug() {
    let access = AccessType::ReadWrite;
    let debug = format!("{:?}", access);
    assert!(debug.contains("Read") || debug.contains("Write") || debug.contains("RW"));
}

#[test]
fn test_access_type_display() {
    let access = AccessType::ReadWriteExecute;
    let display = format!("{}", access);
    assert!(!display.is_empty());
}

// ============================================================================
// Validation Tests
// ============================================================================

#[test]
fn test_validate_access_allowed() {
    let page_perms = AccessType::ReadWrite;
    let requested = AccessType::Read;

    let result = requested.validate_against(page_perms);
    assert!(result.is_ok());
}

#[test]
fn test_validate_access_denied() {
    let page_perms = AccessType::Read;
    let requested = AccessType::Write;

    let result = requested.validate_against(page_perms);
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(ValidationError::PermissionDenied { .. })
    ));
}

#[test]
fn test_validate_access_partial_allowed() {
    let page_perms = AccessType::ReadExecute;
    let requested = AccessType::Read;

    let result = requested.validate_against(page_perms);
    assert!(result.is_ok());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_none_access_type() {
    let none = AccessType::None;
    assert!(!none.can_read());
    assert!(!none.can_write());
    assert!(!none.can_execute());
}

#[test]
fn test_none_permissions_deny_all() {
    let page_perms = AccessType::None;

    assert!(!page_perms.has_permission(AccessType::Read));
    assert!(!page_perms.has_permission(AccessType::Write));
    assert!(!page_perms.has_permission(AccessType::Execute));
}

#[test]
fn test_permission_combinations() {
    // Test all 8 possible combinations (2^3 for R, W, X)
    let combinations = [
        (AccessType::None, 0b000),
        (AccessType::Read, 0b001),
        (AccessType::Write, 0b010),
        (AccessType::ReadWrite, 0b011),
        (AccessType::Execute, 0b100),
        (AccessType::ReadExecute, 0b101),
        (AccessType::WriteExecute, 0b110),
        (AccessType::ReadWriteExecute, 0b111),
    ];

    for (access_type, expected_bits) in combinations.iter() {
        assert_eq!(access_type.to_bits(), *expected_bits);
        assert_eq!(AccessType::from_bits(*expected_bits).unwrap(), *access_type);
    }
}

#[test]
fn test_write_execute_combination() {
    let access = AccessType::WriteExecute;
    assert!(!access.can_read());
    assert!(access.can_write());
    assert!(access.can_execute());
}

// ============================================================================
// Performance Tests
// ============================================================================

#[test]
fn test_permission_check_performance() {
    const ITERATIONS: usize = 100_000;
    let page_perms = AccessType::ReadWriteExecute;

    let start = std::time::Instant::now();
    for _ in 0..ITERATIONS {
        let _r = page_perms.can_read();
        let _w = page_perms.can_write();
        let _x = page_perms.can_execute();
    }
    let duration = start.elapsed();

    // Permission checks should be extremely fast
    assert!(
        duration.as_millis() < 10,
        "Permission checks too slow: {:?}",
        duration
    );
}
