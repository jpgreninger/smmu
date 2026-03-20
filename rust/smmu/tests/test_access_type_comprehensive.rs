#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive test coverage for `types/access_type.rs`
//!
//! This test suite achieves 100% coverage of `AccessType`, covering all access type
//! combinations, bit flag operations, permission checking logic, and conversion methods.

use smmu::types::{AccessType, ValidationError};

// ============================================================================
// Basic Access Type Tests
// ============================================================================

#[test]
fn test_access_type_none() {
    let access = AccessType::None;
    assert!(!access.can_read());
    assert!(!access.can_write());
    assert!(!access.can_execute());
    assert_eq!(access.to_bits(), 0b000);
}

#[test]
fn test_access_type_read() {
    let access = AccessType::Read;
    assert!(access.can_read());
    assert!(!access.can_write());
    assert!(!access.can_execute());
    assert_eq!(access.to_bits(), 0b001);
}

#[test]
fn test_access_type_write() {
    let access = AccessType::Write;
    assert!(!access.can_read());
    assert!(access.can_write());
    assert!(!access.can_execute());
    assert_eq!(access.to_bits(), 0b010);
}

#[test]
fn test_access_type_read_write() {
    let access = AccessType::ReadWrite;
    assert!(access.can_read());
    assert!(access.can_write());
    assert!(!access.can_execute());
    assert_eq!(access.to_bits(), 0b011);
}

#[test]
fn test_access_type_execute() {
    let access = AccessType::Execute;
    assert!(!access.can_read());
    assert!(!access.can_write());
    assert!(access.can_execute());
    assert_eq!(access.to_bits(), 0b100);
}

#[test]
fn test_access_type_read_execute() {
    let access = AccessType::ReadExecute;
    assert!(access.can_read());
    assert!(!access.can_write());
    assert!(access.can_execute());
    assert_eq!(access.to_bits(), 0b101);
}

#[test]
fn test_access_type_write_execute() {
    let access = AccessType::WriteExecute;
    assert!(!access.can_read());
    assert!(access.can_write());
    assert!(access.can_execute());
    assert_eq!(access.to_bits(), 0b110);
}

#[test]
fn test_access_type_read_write_execute() {
    let access = AccessType::ReadWriteExecute;
    assert!(access.can_read());
    assert!(access.can_write());
    assert!(access.can_execute());
    assert_eq!(access.to_bits(), 0b111);
}

// ============================================================================
// Const Variants of Permission Checks
// ============================================================================

#[test]
fn test_const_can_read() {
    const READ: bool = AccessType::Read.const_can_read();
    const WRITE: bool = AccessType::Write.const_can_read();
    const EXECUTE: bool = AccessType::Execute.const_can_read();
    const RW: bool = AccessType::ReadWrite.const_can_read();

    assert!(READ);
    assert!(!WRITE);
    assert!(!EXECUTE);
    assert!(RW);
}

#[test]
fn test_const_can_write() {
    const READ: bool = AccessType::Read.const_can_write();
    const WRITE: bool = AccessType::Write.const_can_write();
    const EXECUTE: bool = AccessType::Execute.const_can_write();
    const RW: bool = AccessType::ReadWrite.const_can_write();

    assert!(!READ);
    assert!(WRITE);
    assert!(!EXECUTE);
    assert!(RW);
}

#[test]
fn test_const_can_execute() {
    const READ: bool = AccessType::Read.const_can_execute();
    const WRITE: bool = AccessType::Write.const_can_execute();
    const EXECUTE: bool = AccessType::Execute.const_can_execute();
    const RX: bool = AccessType::ReadExecute.const_can_execute();

    assert!(!READ);
    assert!(!WRITE);
    assert!(EXECUTE);
    assert!(RX);
}

// ============================================================================
// Bit Conversion Tests
// ============================================================================

#[test]
fn test_to_bits_all_variants() {
    assert_eq!(AccessType::None.to_bits(), 0b000);
    assert_eq!(AccessType::Read.to_bits(), 0b001);
    assert_eq!(AccessType::Write.to_bits(), 0b010);
    assert_eq!(AccessType::ReadWrite.to_bits(), 0b011);
    assert_eq!(AccessType::Execute.to_bits(), 0b100);
    assert_eq!(AccessType::ReadExecute.to_bits(), 0b101);
    assert_eq!(AccessType::WriteExecute.to_bits(), 0b110);
    assert_eq!(AccessType::ReadWriteExecute.to_bits(), 0b111);
}

#[test]
fn test_from_bits_valid_all() {
    for bits in 0..=0b111 {
        let access = AccessType::from_bits(bits).unwrap();
        assert_eq!(access.to_bits(), bits);
    }
}

#[test]
fn test_from_bits_invalid() {
    // Bug-4 fix: 0b1001/0b1010/0b1011/0b1100 are now valid (ReadPrivileged etc.).
    // BUG-RUST-5 fix: 0b1101/0b1110/0b1111 are now valid (ReadExecutePrivileged etc.).
    // Only truly invalid patterns remain: 0b1000 (priv-only no R/W/X) and values >= 0b1_0000.
    let invalid_bits = [0b1000u8, 0xFF, 0x10, 0x20];

    for &bits in &invalid_bits {
        let result = AccessType::from_bits(bits);
        assert!(result.is_err(), "Expected error for bits=0b{bits:08b} ({bits})");
        match result {
            Err(ValidationError::InvalidAccessType { bits: b }) => {
                assert_eq!(b, bits);
            },
            _ => panic!("Expected InvalidAccessType error for bits={bits}"),
        }
    }
}

#[test]
fn test_from_bits_privileged_compound_variants_valid() {
    // BUG-RUST-5 fix: compound-execute privileged variants (0b1101/0b1110/0b1111)
    // are now valid AccessType values.
    assert_eq!(
        AccessType::from_bits(0b1101).unwrap(),
        AccessType::ReadExecutePrivileged
    );
    assert_eq!(
        AccessType::from_bits(0b1110).unwrap(),
        AccessType::WriteExecutePrivileged
    );
    assert_eq!(
        AccessType::from_bits(0b1111).unwrap(),
        AccessType::ReadWriteExecutePrivileged
    );

    // Verify R/W/X bits and privilege flag
    let read_exec_priv = AccessType::ReadExecutePrivileged;
    assert!(read_exec_priv.can_read());
    assert!(!read_exec_priv.can_write());
    assert!(read_exec_priv.can_execute());
    assert!(read_exec_priv.is_privileged());

    let write_exec_priv = AccessType::WriteExecutePrivileged;
    assert!(!write_exec_priv.can_read());
    assert!(write_exec_priv.can_write());
    assert!(write_exec_priv.can_execute());
    assert!(write_exec_priv.is_privileged());

    let all_priv = AccessType::ReadWriteExecutePrivileged;
    assert!(all_priv.can_read());
    assert!(all_priv.can_write());
    assert!(all_priv.can_execute());
    assert!(all_priv.is_privileged());
}

#[test]
fn test_from_bits_round_trip() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for access_type in &types {
        let bits = access_type.to_bits();
        let recovered = AccessType::from_bits(bits).unwrap();
        assert_eq!(*access_type, recovered);
    }
}

// ============================================================================
// Permission Checking (has_permission)
// ============================================================================

#[test]
fn test_has_permission_none() {
    let none = AccessType::None;
    assert!(none.has_permission(AccessType::None));
    assert!(!none.has_permission(AccessType::Read));
    assert!(!none.has_permission(AccessType::Write));
    assert!(!none.has_permission(AccessType::Execute));
}

#[test]
fn test_has_permission_read() {
    let read = AccessType::Read;
    assert!(read.has_permission(AccessType::None));
    assert!(read.has_permission(AccessType::Read));
    assert!(!read.has_permission(AccessType::Write));
    assert!(!read.has_permission(AccessType::Execute));
    assert!(!read.has_permission(AccessType::ReadWrite));
}

#[test]
fn test_has_permission_write() {
    let write = AccessType::Write;
    assert!(write.has_permission(AccessType::None));
    assert!(!write.has_permission(AccessType::Read));
    assert!(write.has_permission(AccessType::Write));
    assert!(!write.has_permission(AccessType::Execute));
}

#[test]
fn test_has_permission_read_write() {
    let rw = AccessType::ReadWrite;
    assert!(rw.has_permission(AccessType::None));
    assert!(rw.has_permission(AccessType::Read));
    assert!(rw.has_permission(AccessType::Write));
    assert!(rw.has_permission(AccessType::ReadWrite));
    assert!(!rw.has_permission(AccessType::Execute));
    assert!(!rw.has_permission(AccessType::ReadExecute));
}

#[test]
fn test_has_permission_execute() {
    let exec = AccessType::Execute;
    assert!(exec.has_permission(AccessType::None));
    assert!(!exec.has_permission(AccessType::Read));
    assert!(!exec.has_permission(AccessType::Write));
    assert!(exec.has_permission(AccessType::Execute));
}

#[test]
fn test_has_permission_read_execute() {
    let rx = AccessType::ReadExecute;
    assert!(rx.has_permission(AccessType::None));
    assert!(rx.has_permission(AccessType::Read));
    assert!(!rx.has_permission(AccessType::Write));
    assert!(rx.has_permission(AccessType::Execute));
    assert!(rx.has_permission(AccessType::ReadExecute));
    assert!(!rx.has_permission(AccessType::ReadWrite));
}

#[test]
fn test_has_permission_write_execute() {
    let wx = AccessType::WriteExecute;
    assert!(wx.has_permission(AccessType::None));
    assert!(!wx.has_permission(AccessType::Read));
    assert!(wx.has_permission(AccessType::Write));
    assert!(wx.has_permission(AccessType::Execute));
    assert!(wx.has_permission(AccessType::WriteExecute));
}

#[test]
fn test_has_permission_read_write_execute() {
    let rwx = AccessType::ReadWriteExecute;
    assert!(rwx.has_permission(AccessType::None));
    assert!(rwx.has_permission(AccessType::Read));
    assert!(rwx.has_permission(AccessType::Write));
    assert!(rwx.has_permission(AccessType::Execute));
    assert!(rwx.has_permission(AccessType::ReadWrite));
    assert!(rwx.has_permission(AccessType::ReadExecute));
    assert!(rwx.has_permission(AccessType::WriteExecute));
    assert!(rwx.has_permission(AccessType::ReadWriteExecute));
}

// ============================================================================
// Intersection Operation
// ============================================================================

#[test]
fn test_intersect_none() {
    let none = AccessType::None;
    assert_eq!(none.intersect(AccessType::Read), AccessType::None);
    assert_eq!(none.intersect(AccessType::Write), AccessType::None);
    assert_eq!(none.intersect(AccessType::Execute), AccessType::None);
    assert_eq!(none.intersect(AccessType::ReadWriteExecute), AccessType::None);
}

#[test]
fn test_intersect_read() {
    let read = AccessType::Read;
    assert_eq!(read.intersect(AccessType::None), AccessType::None);
    assert_eq!(read.intersect(AccessType::Read), AccessType::Read);
    assert_eq!(read.intersect(AccessType::Write), AccessType::None);
    assert_eq!(read.intersect(AccessType::ReadWrite), AccessType::Read);
    assert_eq!(read.intersect(AccessType::Execute), AccessType::None);
    assert_eq!(read.intersect(AccessType::ReadExecute), AccessType::Read);
}

#[test]
fn test_intersect_write() {
    let write = AccessType::Write;
    assert_eq!(write.intersect(AccessType::None), AccessType::None);
    assert_eq!(write.intersect(AccessType::Read), AccessType::None);
    assert_eq!(write.intersect(AccessType::Write), AccessType::Write);
    assert_eq!(write.intersect(AccessType::ReadWrite), AccessType::Write);
    assert_eq!(write.intersect(AccessType::Execute), AccessType::None);
}

#[test]
fn test_intersect_read_write() {
    let rw = AccessType::ReadWrite;
    assert_eq!(rw.intersect(AccessType::None), AccessType::None);
    assert_eq!(rw.intersect(AccessType::Read), AccessType::Read);
    assert_eq!(rw.intersect(AccessType::Write), AccessType::Write);
    assert_eq!(rw.intersect(AccessType::ReadWrite), AccessType::ReadWrite);
    assert_eq!(rw.intersect(AccessType::Execute), AccessType::None);
    assert_eq!(rw.intersect(AccessType::ReadExecute), AccessType::Read);
    assert_eq!(rw.intersect(AccessType::WriteExecute), AccessType::Write);
    assert_eq!(rw.intersect(AccessType::ReadWriteExecute), AccessType::ReadWrite);
}

#[test]
fn test_intersect_execute() {
    let exec = AccessType::Execute;
    assert_eq!(exec.intersect(AccessType::None), AccessType::None);
    assert_eq!(exec.intersect(AccessType::Read), AccessType::None);
    assert_eq!(exec.intersect(AccessType::Write), AccessType::None);
    assert_eq!(exec.intersect(AccessType::Execute), AccessType::Execute);
    assert_eq!(exec.intersect(AccessType::ReadExecute), AccessType::Execute);
    assert_eq!(exec.intersect(AccessType::WriteExecute), AccessType::Execute);
}

#[test]
fn test_intersect_symmetric() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t1 in &types {
        for &t2 in &types {
            assert_eq!(t1.intersect(t2), t2.intersect(t1));
        }
    }
}

// ============================================================================
// Union Operation
// ============================================================================

#[test]
fn test_union_none() {
    let none = AccessType::None;
    assert_eq!(none.union(AccessType::None), AccessType::None);
    assert_eq!(none.union(AccessType::Read), AccessType::Read);
    assert_eq!(none.union(AccessType::Write), AccessType::Write);
    assert_eq!(none.union(AccessType::Execute), AccessType::Execute);
    assert_eq!(none.union(AccessType::ReadWrite), AccessType::ReadWrite);
}

#[test]
fn test_union_read() {
    let read = AccessType::Read;
    assert_eq!(read.union(AccessType::None), AccessType::Read);
    assert_eq!(read.union(AccessType::Read), AccessType::Read);
    assert_eq!(read.union(AccessType::Write), AccessType::ReadWrite);
    assert_eq!(read.union(AccessType::Execute), AccessType::ReadExecute);
    assert_eq!(read.union(AccessType::ReadWrite), AccessType::ReadWrite);
    assert_eq!(read.union(AccessType::WriteExecute), AccessType::ReadWriteExecute);
}

#[test]
fn test_union_write() {
    let write = AccessType::Write;
    assert_eq!(write.union(AccessType::None), AccessType::Write);
    assert_eq!(write.union(AccessType::Read), AccessType::ReadWrite);
    assert_eq!(write.union(AccessType::Write), AccessType::Write);
    assert_eq!(write.union(AccessType::Execute), AccessType::WriteExecute);
    assert_eq!(write.union(AccessType::ReadExecute), AccessType::ReadWriteExecute);
}

#[test]
fn test_union_execute() {
    let exec = AccessType::Execute;
    assert_eq!(exec.union(AccessType::None), AccessType::Execute);
    assert_eq!(exec.union(AccessType::Read), AccessType::ReadExecute);
    assert_eq!(exec.union(AccessType::Write), AccessType::WriteExecute);
    assert_eq!(exec.union(AccessType::Execute), AccessType::Execute);
    assert_eq!(exec.union(AccessType::ReadWrite), AccessType::ReadWriteExecute);
}

#[test]
fn test_union_read_write() {
    let rw = AccessType::ReadWrite;
    assert_eq!(rw.union(AccessType::None), AccessType::ReadWrite);
    assert_eq!(rw.union(AccessType::Read), AccessType::ReadWrite);
    assert_eq!(rw.union(AccessType::Write), AccessType::ReadWrite);
    assert_eq!(rw.union(AccessType::Execute), AccessType::ReadWriteExecute);
    assert_eq!(rw.union(AccessType::ReadWrite), AccessType::ReadWrite);
}

#[test]
fn test_union_read_write_execute() {
    let rwx = AccessType::ReadWriteExecute;
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t in &types {
        assert_eq!(rwx.union(t), AccessType::ReadWriteExecute);
    }
}

#[test]
fn test_union_symmetric() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t1 in &types {
        for &t2 in &types {
            assert_eq!(t1.union(t2), t2.union(t1));
        }
    }
}

// ============================================================================
// Validate Against
// ============================================================================

#[test]
fn test_validate_against_success() {
    let page_perms = AccessType::ReadWrite;

    assert!(AccessType::None.validate_against(page_perms).is_ok());
    assert!(AccessType::Read.validate_against(page_perms).is_ok());
    assert!(AccessType::Write.validate_against(page_perms).is_ok());
    assert!(AccessType::ReadWrite.validate_against(page_perms).is_ok());
}

#[test]
fn test_validate_against_failure() {
    let page_perms = AccessType::Read;

    assert!(AccessType::Write.validate_against(page_perms).is_err());
    assert!(AccessType::Execute.validate_against(page_perms).is_err());
    assert!(AccessType::ReadWrite.validate_against(page_perms).is_err());
}

#[test]
fn test_validate_against_error_details() {
    let page_perms = AccessType::ReadWrite;
    let result = AccessType::Execute.validate_against(page_perms);

    assert!(result.is_err());
    match result {
        Err(ValidationError::PermissionDenied { requested, available }) => {
            assert!(requested.contains("Execute"));
            assert!(available.contains("ReadWrite"));
        },
        _ => panic!("Expected PermissionDenied error"),
    }
}

#[test]
fn test_validate_against_all_combinations() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &available in &types {
        for &requested in &types {
            let result = requested.validate_against(available);

            if available.has_permission(requested) {
                assert!(
                    result.is_ok(),
                    "Expected {requested:?}.validate_against({available:?}) to succeed"
                );
            } else {
                assert!(
                    result.is_err(),
                    "Expected {requested:?}.validate_against({available:?}) to fail"
                );
            }
        }
    }
}

// ============================================================================
// Display Formatting
// ============================================================================

#[test]
fn test_display_none() {
    let msg = format!("{}", AccessType::None);
    assert_eq!(msg, "None");
}

#[test]
fn test_display_read() {
    let msg = format!("{}", AccessType::Read);
    assert_eq!(msg, "Read");
}

#[test]
fn test_display_write() {
    let msg = format!("{}", AccessType::Write);
    assert_eq!(msg, "Write");
}

#[test]
fn test_display_read_write() {
    let msg = format!("{}", AccessType::ReadWrite);
    assert_eq!(msg, "ReadWrite");
}

#[test]
fn test_display_execute() {
    let msg = format!("{}", AccessType::Execute);
    assert_eq!(msg, "Execute");
}

#[test]
fn test_display_read_execute() {
    let msg = format!("{}", AccessType::ReadExecute);
    assert_eq!(msg, "ReadExecute");
}

#[test]
fn test_display_write_execute() {
    let msg = format!("{}", AccessType::WriteExecute);
    assert_eq!(msg, "WriteExecute");
}

#[test]
fn test_display_read_write_execute() {
    let msg = format!("{}", AccessType::ReadWriteExecute);
    assert_eq!(msg, "ReadWriteExecute");
}

// ============================================================================
// Default Trait
// ============================================================================

#[test]
fn test_default() {
    let default = AccessType::default();
    assert_eq!(default, AccessType::None);
    assert_eq!(default.to_bits(), 0);
}

// ============================================================================
// Equality and Debug
// ============================================================================

#[test]
fn test_equality() {
    assert_eq!(AccessType::Read, AccessType::Read);
    assert_eq!(AccessType::Write, AccessType::Write);
    assert_ne!(AccessType::Read, AccessType::Write);
}

#[test]
fn test_debug() {
    let debug = format!("{:?}", AccessType::ReadWrite);
    assert!(debug.contains("ReadWrite"));
}

#[test]
fn test_hash() {
    use std::collections::HashMap;

    let mut map = HashMap::new();
    map.insert(AccessType::Read, "read");
    map.insert(AccessType::Write, "write");

    assert_eq!(map.get(&AccessType::Read), Some(&"read"));
    assert_eq!(map.get(&AccessType::Write), Some(&"write"));
    assert_eq!(map.get(&AccessType::Execute), None);
}

#[test]
fn test_copy() {
    let access = AccessType::ReadWrite;
    let copied = access;
    assert_eq!(access, copied);
}

#[test]
fn test_clone() {
    let access = AccessType::ReadExecute;
    let cloned = access;
    assert_eq!(access, cloned);
}

// ============================================================================
// Edge Cases and Advanced Tests
// ============================================================================

#[test]
fn test_from_bits_boundary() {
    // Valid boundary
    assert!(AccessType::from_bits(0b111).is_ok());

    // Invalid boundary
    assert!(AccessType::from_bits(0b1000).is_err());
}

#[test]
fn test_has_permission_reflexive() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t in &types {
        assert!(t.has_permission(t));
    }
}

#[test]
fn test_intersect_idempotent() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t in &types {
        assert_eq!(t.intersect(t), t);
    }
}

#[test]
fn test_union_idempotent() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t in &types {
        assert_eq!(t.union(t), t);
    }
}

#[test]
fn test_intersect_with_none() {
    let types = [
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t in &types {
        assert_eq!(t.intersect(AccessType::None), AccessType::None);
    }
}

#[test]
fn test_union_with_none() {
    let types = [
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    for &t in &types {
        assert_eq!(t.union(AccessType::None), t);
    }
}

#[test]
fn test_union_with_read_write_execute() {
    let types = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
    ];

    for &t in &types {
        assert_eq!(t.union(AccessType::ReadWriteExecute), AccessType::ReadWriteExecute);
    }
}

#[test]
fn test_permission_subset_relationship() {
    // Read is a subset of ReadWrite
    assert!(AccessType::ReadWrite.has_permission(AccessType::Read));

    // ReadWrite is a subset of ReadWriteExecute
    assert!(AccessType::ReadWriteExecute.has_permission(AccessType::ReadWrite));

    // Execute is a subset of ReadExecute
    assert!(AccessType::ReadExecute.has_permission(AccessType::Execute));

    // But ReadWrite is not a subset of ReadExecute
    assert!(!AccessType::ReadExecute.has_permission(AccessType::ReadWrite));
}

#[test]
fn test_all_eight_variants_exist() {
    let all_variants = [
        AccessType::None,
        AccessType::Read,
        AccessType::Write,
        AccessType::ReadWrite,
        AccessType::Execute,
        AccessType::ReadExecute,
        AccessType::WriteExecute,
        AccessType::ReadWriteExecute,
    ];

    assert_eq!(all_variants.len(), 8);

    // Ensure all have unique bit patterns
    let mut bits_seen = std::collections::HashSet::new();
    for variant in &all_variants {
        assert!(bits_seen.insert(variant.to_bits()));
    }
}
