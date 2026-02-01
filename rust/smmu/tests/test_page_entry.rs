#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive tests for `PageEntry` and `PagePermissions` types
//!
//! This test suite achieves 100% coverage for `types/page_entry.rs` by testing:
//! - All `PagePermissions` factory methods and operations
//! - Memory attribute combinations (device memory, cacheable, shareable)
//! - Security state variations
//! - Access flag manipulation
//! - `PageEntryBuilder` pattern
//! - All permission combinations (8 variants)
//! - Set operations (union, intersection, subset)

use smmu::types::{AccessType, PageEntry, PagePermissions, SecurityState, PA};
use std::collections::HashSet;

// ============================================================================
// PagePermissions Factory Methods Tests
// ============================================================================

#[test]
fn test_page_permissions_none() {
    let perms = PagePermissions::none();
    assert!(!perms.read());
    assert!(!perms.write());
    assert!(!perms.execute());
}

#[test]
fn test_page_permissions_read_only() {
    let perms = PagePermissions::read_only();
    assert!(perms.read());
    assert!(!perms.write());
    assert!(!perms.execute());
}

#[test]
fn test_page_permissions_write_only() {
    let perms = PagePermissions::write_only();
    assert!(!perms.read());
    assert!(perms.write());
    assert!(!perms.execute());
}

#[test]
fn test_page_permissions_execute_only() {
    let perms = PagePermissions::execute_only();
    assert!(!perms.read());
    assert!(!perms.write());
    assert!(perms.execute());
}

#[test]
fn test_page_permissions_read_write() {
    let perms = PagePermissions::read_write();
    assert!(perms.read());
    assert!(perms.write());
    assert!(!perms.execute());
}

#[test]
fn test_page_permissions_read_execute() {
    let perms = PagePermissions::read_execute();
    assert!(perms.read());
    assert!(!perms.write());
    assert!(perms.execute());
}

#[test]
fn test_page_permissions_write_execute() {
    let perms = PagePermissions::new(false, true, true);
    assert!(!perms.read());
    assert!(perms.write());
    assert!(perms.execute());
}

#[test]
fn test_page_permissions_all() {
    let perms = PagePermissions::all();
    assert!(perms.read());
    assert!(perms.write());
    assert!(perms.execute());
}

// ============================================================================
// PagePermissions Default Trait Test
// ============================================================================

#[test]
fn test_page_permissions_default() {
    let perms = PagePermissions::default();
    assert_eq!(perms, PagePermissions::none());
    assert!(!perms.read());
    assert!(!perms.write());
    assert!(!perms.execute());
}

// ============================================================================
// PagePermissions Hash Trait Tests
// ============================================================================

#[test]
fn test_page_permissions_hash_in_hashset() {
    let mut set = HashSet::new();

    let perm1 = PagePermissions::read_only();
    let perm2 = PagePermissions::read_write();
    let perm3 = PagePermissions::read_only(); // Duplicate

    set.insert(perm1);
    set.insert(perm2);
    set.insert(perm3);

    assert_eq!(set.len(), 2); // Only 2 unique permissions
}

#[test]
fn test_page_permissions_hash_all_combinations() {
    let mut set = HashSet::new();

    // All 8 possible permission combinations
    set.insert(PagePermissions::none());
    set.insert(PagePermissions::read_only());
    set.insert(PagePermissions::write_only());
    set.insert(PagePermissions::execute_only());
    set.insert(PagePermissions::read_write());
    set.insert(PagePermissions::read_execute());
    set.insert(PagePermissions::new(false, true, true)); // write-execute
    set.insert(PagePermissions::all());

    assert_eq!(set.len(), 8);
}

// ============================================================================
// PagePermissions allows() Method Tests - All AccessType Variants
// ============================================================================

#[test]
fn test_page_permissions_allows_none() {
    let perms = PagePermissions::none();
    assert!(perms.allows(AccessType::None));
}

#[test]
fn test_page_permissions_allows_read() {
    let read_perms = PagePermissions::read_only();
    assert!(read_perms.allows(AccessType::Read));

    let no_read = PagePermissions::write_only();
    assert!(!no_read.allows(AccessType::Read));
}

#[test]
fn test_page_permissions_allows_write() {
    let write_perms = PagePermissions::write_only();
    assert!(write_perms.allows(AccessType::Write));

    let no_write = PagePermissions::read_only();
    assert!(!no_write.allows(AccessType::Write));
}

#[test]
fn test_page_permissions_allows_execute() {
    let exec_perms = PagePermissions::execute_only();
    assert!(exec_perms.allows(AccessType::Execute));

    let no_exec = PagePermissions::read_only();
    assert!(!no_exec.allows(AccessType::Execute));
}

#[test]
fn test_page_permissions_allows_read_write() {
    let rw_perms = PagePermissions::read_write();
    assert!(rw_perms.allows(AccessType::ReadWrite));

    let read_only = PagePermissions::read_only();
    assert!(!read_only.allows(AccessType::ReadWrite));

    let write_only = PagePermissions::write_only();
    assert!(!write_only.allows(AccessType::ReadWrite));
}

#[test]
fn test_page_permissions_allows_read_execute() {
    let rx_perms = PagePermissions::read_execute();
    assert!(rx_perms.allows(AccessType::ReadExecute));

    let read_only = PagePermissions::read_only();
    assert!(!read_only.allows(AccessType::ReadExecute));

    let exec_only = PagePermissions::execute_only();
    assert!(!exec_only.allows(AccessType::ReadExecute));
}

#[test]
fn test_page_permissions_allows_write_execute() {
    let wx_perms = PagePermissions::new(false, true, true);
    assert!(wx_perms.allows(AccessType::WriteExecute));

    let write_only = PagePermissions::write_only();
    assert!(!write_only.allows(AccessType::WriteExecute));

    let exec_only = PagePermissions::execute_only();
    assert!(!exec_only.allows(AccessType::WriteExecute));
}

#[test]
fn test_page_permissions_allows_read_write_execute() {
    let all_perms = PagePermissions::all();
    assert!(all_perms.allows(AccessType::ReadWriteExecute));

    let rw_only = PagePermissions::read_write();
    assert!(!rw_only.allows(AccessType::ReadWriteExecute));

    let rx_only = PagePermissions::read_execute();
    assert!(!rx_only.allows(AccessType::ReadWriteExecute));
}

// ============================================================================
// PagePermissions Set Operations Tests
// ============================================================================

#[test]
fn test_page_permissions_union() {
    let read = PagePermissions::read_only();
    let write = PagePermissions::write_only();
    let union = read.union(write);

    assert!(union.read());
    assert!(union.write());
    assert!(!union.execute());
}

#[test]
fn test_page_permissions_union_overlapping() {
    let rw = PagePermissions::read_write();
    let rx = PagePermissions::read_execute();
    let union = rw.union(rx);

    assert!(union.read());
    assert!(union.write());
    assert!(union.execute());
    assert_eq!(union, PagePermissions::all());
}

#[test]
fn test_page_permissions_union_with_none() {
    let read = PagePermissions::read_only();
    let none = PagePermissions::none();
    let union = read.union(none);

    assert_eq!(union, read);
}

#[test]
fn test_page_permissions_union_idempotent() {
    let perms = PagePermissions::read_write();
    let union = perms.union(perms);

    assert_eq!(union, perms);
}

#[test]
fn test_page_permissions_intersection() {
    let rw = PagePermissions::read_write();
    let rx = PagePermissions::read_execute();
    let intersection = rw.intersection(rx);

    assert!(intersection.read());
    assert!(!intersection.write());
    assert!(!intersection.execute());
    assert_eq!(intersection, PagePermissions::read_only());
}

#[test]
fn test_page_permissions_intersection_disjoint() {
    let read = PagePermissions::read_only();
    let write = PagePermissions::write_only();
    let intersection = read.intersection(write);

    assert_eq!(intersection, PagePermissions::none());
}

#[test]
fn test_page_permissions_intersection_with_all() {
    let read = PagePermissions::read_only();
    let all = PagePermissions::all();
    let intersection = read.intersection(all);

    assert_eq!(intersection, read);
}

#[test]
fn test_page_permissions_intersection_idempotent() {
    let perms = PagePermissions::read_execute();
    let intersection = perms.intersection(perms);

    assert_eq!(intersection, perms);
}

#[test]
fn test_page_permissions_is_subset_of() {
    let read = PagePermissions::read_only();
    let rw = PagePermissions::read_write();
    let all = PagePermissions::all();

    assert!(read.is_subset_of(&rw));
    assert!(read.is_subset_of(&all));
    assert!(rw.is_subset_of(&all));

    assert!(!rw.is_subset_of(&read));
    assert!(!all.is_subset_of(&rw));
}

#[test]
fn test_page_permissions_is_subset_of_reflexive() {
    let perms = PagePermissions::read_execute();
    assert!(perms.is_subset_of(&perms));
}

#[test]
fn test_page_permissions_is_subset_of_none() {
    let none = PagePermissions::none();
    let read = PagePermissions::read_only();

    assert!(none.is_subset_of(&read));
    assert!(none.is_subset_of(&none));
}

#[test]
fn test_page_permissions_is_subset_of_transitive() {
    let read = PagePermissions::read_only();
    let rw = PagePermissions::read_write();
    let all = PagePermissions::all();

    assert!(read.is_subset_of(&rw));
    assert!(rw.is_subset_of(&all));
    assert!(read.is_subset_of(&all)); // Transitivity
}

// ============================================================================
// PageEntry Basic Tests
// ============================================================================

#[test]
fn test_page_entry_new() {
    let pa = PA::new(0x1000).unwrap();
    let perms = PagePermissions::read_write();
    let entry = PageEntry::new(pa, perms);

    assert_eq!(entry.physical_address(), pa);
    assert_eq!(entry.permissions(), perms);
    assert!(entry.is_valid());
    assert_eq!(entry.security_state(), SecurityState::NonSecure);
    assert!(entry.is_cacheable());
    assert!(entry.is_shareable());
    assert!(!entry.is_device_memory());
}

#[test]
fn test_page_entry_with_security_state_secure() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_only();
    let entry = PageEntry::with_security_state(pa, perms, SecurityState::Secure);

    assert_eq!(entry.security_state(), SecurityState::Secure);
    assert!(entry.is_valid());
}

#[test]
fn test_page_entry_with_security_state_realm() {
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::execute_only();
    let entry = PageEntry::with_security_state(pa, perms, SecurityState::Realm);

    assert_eq!(entry.security_state(), SecurityState::Realm);
}

#[test]
fn test_page_entry_with_security_state_nonsecure() {
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::all();
    let entry = PageEntry::with_security_state(pa, perms, SecurityState::NonSecure);

    assert_eq!(entry.security_state(), SecurityState::NonSecure);
}

// ============================================================================
// PageEntry Default Trait Test
// ============================================================================

#[test]
fn test_page_entry_default() {
    let entry = PageEntry::default();

    assert!(!entry.is_valid());
    assert_eq!(entry.physical_address(), PA::new(0).unwrap());
    assert_eq!(entry.permissions(), PagePermissions::default());
    assert_eq!(entry.security_state(), SecurityState::NonSecure);
    assert!(!entry.is_cacheable());
    assert!(!entry.is_shareable());
    assert!(!entry.is_device_memory());
}

// ============================================================================
// PageEntry Validity Tests
// ============================================================================

#[test]
fn test_page_entry_mark_valid() {
    let entry = PageEntry::default()
        .mark_invalid()
        .with_permissions(PagePermissions::read_write());

    assert!(!entry.is_valid());

    let valid_entry = entry.mark_valid();
    assert!(valid_entry.is_valid());
}

#[test]
fn test_page_entry_mark_invalid() {
    let pa = PA::new(0x6000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_only());

    assert!(entry.is_valid());

    let invalid_entry = entry.mark_invalid();
    assert!(!invalid_entry.is_valid());
}

#[test]
fn test_page_entry_mark_valid_idempotent() {
    let pa = PA::new(0x7000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::all());

    let still_valid = entry.mark_valid();
    assert!(still_valid.is_valid());
}

#[test]
fn test_page_entry_mark_invalid_idempotent() {
    let entry = PageEntry::default();

    assert!(!entry.is_valid());

    let still_invalid = entry.mark_invalid();
    assert!(!still_invalid.is_valid());
}

// ============================================================================
// PageEntry Memory Attribute Tests
// ============================================================================

#[test]
fn test_page_entry_cacheable() {
    let pa = PA::new(0x8000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_write()).with_cacheable(true);

    assert!(entry.is_cacheable());
}

#[test]
fn test_page_entry_non_cacheable() {
    let pa = PA::new(0x9000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_only()).with_cacheable(false);

    assert!(!entry.is_cacheable());
}

#[test]
fn test_page_entry_shareable() {
    let pa = PA::new(0xA000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_write()).with_shareable(true);

    assert!(entry.is_shareable());
}

#[test]
fn test_page_entry_non_shareable() {
    let pa = PA::new(0xB000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_only()).with_shareable(false);

    assert!(!entry.is_shareable());
}

#[test]
fn test_page_entry_device_memory() {
    let pa = PA::new(0xC000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_write()).with_device_memory(true);

    assert!(entry.is_device_memory());
    assert!(!entry.is_cacheable()); // Device memory cannot be cacheable
}

#[test]
fn test_page_entry_device_memory_forces_non_cacheable() {
    let pa = PA::new(0xD000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_write())
        .with_device_memory(true)
        .with_cacheable(true); // Try to set cacheable, but should be forced to false

    assert!(entry.is_device_memory());
    assert!(!entry.is_cacheable()); // Should still be non-cacheable
}

#[test]
fn test_page_entry_normal_memory() {
    let pa = PA::new(0xE000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::all())
        .with_device_memory(false)
        .with_cacheable(true)
        .with_shareable(true);

    assert!(!entry.is_device_memory());
    assert!(entry.is_cacheable());
    assert!(entry.is_shareable());
}

// ============================================================================
// PageEntry Memory Attribute Combinations
// ============================================================================

#[test]
fn test_page_entry_normal_cacheable_shareable() {
    let pa = PA::new(0x1_0000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_write());

    // Default is normal memory, cacheable, shareable
    assert!(!entry.is_device_memory());
    assert!(entry.is_cacheable());
    assert!(entry.is_shareable());
}

#[test]
fn test_page_entry_normal_cacheable_non_shareable() {
    let pa = PA::new(0x1_1000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_only()).with_shareable(false);

    assert!(!entry.is_device_memory());
    assert!(entry.is_cacheable());
    assert!(!entry.is_shareable());
}

#[test]
fn test_page_entry_normal_non_cacheable_shareable() {
    let pa = PA::new(0x1_2000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::execute_only()).with_cacheable(false);

    assert!(!entry.is_device_memory());
    assert!(!entry.is_cacheable());
    assert!(entry.is_shareable());
}

#[test]
fn test_page_entry_normal_non_cacheable_non_shareable() {
    let pa = PA::new(0x1_3000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::all())
        .with_cacheable(false)
        .with_shareable(false);

    assert!(!entry.is_device_memory());
    assert!(!entry.is_cacheable());
    assert!(!entry.is_shareable());
}

#[test]
fn test_page_entry_device_non_cacheable_shareable() {
    let pa = PA::new(0x1_4000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_write())
        .with_device_memory(true)
        .with_shareable(true);

    assert!(entry.is_device_memory());
    assert!(!entry.is_cacheable()); // Forced non-cacheable
    assert!(entry.is_shareable());
}

#[test]
fn test_page_entry_device_non_cacheable_non_shareable() {
    let pa = PA::new(0x1_5000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_only())
        .with_device_memory(true)
        .with_shareable(false);

    assert!(entry.is_device_memory());
    assert!(!entry.is_cacheable()); // Forced non-cacheable
    assert!(!entry.is_shareable());
}

// ============================================================================
// PageEntry Permission Updates
// ============================================================================

#[test]
fn test_page_entry_with_permissions() {
    let pa = PA::new(0x1_6000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::read_only());

    assert_eq!(entry.permissions(), PagePermissions::read_only());

    let updated = entry.with_permissions(PagePermissions::read_write());
    assert_eq!(updated.permissions(), PagePermissions::read_write());
}

#[test]
fn test_page_entry_with_permissions_all_variants() {
    let pa = PA::new(0x1_7000).unwrap();
    let base_entry = PageEntry::new(pa, PagePermissions::none());

    let variants = [
        PagePermissions::none(),
        PagePermissions::read_only(),
        PagePermissions::write_only(),
        PagePermissions::execute_only(),
        PagePermissions::read_write(),
        PagePermissions::read_execute(),
        PagePermissions::new(false, true, true), // write-execute
        PagePermissions::all(),
    ];

    for perms in &variants {
        let entry = base_entry.clone().with_permissions(*perms);
        assert_eq!(entry.permissions(), *perms);
    }
}

// ============================================================================
// PageEntryBuilder Tests
// ============================================================================

#[test]
fn test_page_entry_builder_basic() {
    let pa = PA::new(0x2_0000).unwrap();
    let perms = PagePermissions::read_execute();

    let entry = PageEntry::builder().physical_address(pa).permissions(perms).build();

    assert_eq!(entry.physical_address(), pa);
    assert_eq!(entry.permissions(), perms);
    assert!(entry.is_valid());
}

#[test]
fn test_page_entry_builder_with_security_state() {
    let pa = PA::new(0x2_1000).unwrap();

    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::all())
        .security_state(SecurityState::Secure)
        .build();

    assert_eq!(entry.security_state(), SecurityState::Secure);
}

#[test]
fn test_page_entry_builder_with_cacheable() {
    let pa = PA::new(0x2_2000).unwrap();

    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_write())
        .cacheable(false)
        .build();

    assert!(!entry.is_cacheable());
}

#[test]
fn test_page_entry_builder_with_shareable() {
    let pa = PA::new(0x2_3000).unwrap();

    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_only())
        .shareable(false)
        .build();

    assert!(!entry.is_shareable());
}

#[test]
fn test_page_entry_builder_with_device_memory() {
    let pa = PA::new(0x2_4000).unwrap();

    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_write())
        .device_memory(true)
        .build();

    assert!(entry.is_device_memory());
    assert!(!entry.is_cacheable()); // Should be forced non-cacheable
}

#[test]
fn test_page_entry_builder_device_memory_overrides_cacheable() {
    let pa = PA::new(0x2_5000).unwrap();

    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::all())
        .cacheable(true)
        .device_memory(true) // This should force cacheable to false
        .build();

    assert!(entry.is_device_memory());
    assert!(!entry.is_cacheable());
}

#[test]
fn test_page_entry_builder_full_configuration() {
    let pa = PA::new(0x2_6000).unwrap();

    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_execute())
        .security_state(SecurityState::Realm)
        .cacheable(true)
        .shareable(true)
        .device_memory(false)
        .build();

    assert_eq!(entry.physical_address(), pa);
    assert_eq!(entry.permissions(), PagePermissions::read_execute());
    assert_eq!(entry.security_state(), SecurityState::Realm);
    assert!(entry.is_cacheable());
    assert!(entry.is_shareable());
    assert!(!entry.is_device_memory());
    assert!(entry.is_valid());
}

#[test]
fn test_page_entry_builder_default_values() {
    let pa = PA::new(0x2_7000).unwrap();

    let entry = PageEntry::builder().physical_address(pa).build();

    // Default permission is none
    assert_eq!(entry.permissions(), PagePermissions::none());
    // Default security state is NonSecure
    assert_eq!(entry.security_state(), SecurityState::NonSecure);
    // Default memory attributes: cacheable, shareable, not device
    assert!(entry.is_cacheable());
    assert!(entry.is_shareable());
    assert!(!entry.is_device_memory());
}

#[test]
#[should_panic(expected = "Physical address must be set")]
fn test_page_entry_builder_panics_without_physical_address() {
    PageEntry::builder().permissions(PagePermissions::all()).build();
}

// ============================================================================
// PageEntry Clone and Equality Tests
// ============================================================================

#[test]
fn test_page_entry_clone() {
    let pa = PA::new(0x2_8000).unwrap();
    let entry1 = PageEntry::new(pa, PagePermissions::read_write());
    let entry2 = entry1.clone();

    assert_eq!(entry1, entry2);
}

#[test]
fn test_page_entry_equality() {
    let pa = PA::new(0x2_9000).unwrap();
    let perms = PagePermissions::read_only();

    let entry1 = PageEntry::new(pa, perms);
    let entry2 = PageEntry::new(pa, perms);

    assert_eq!(entry1, entry2);
}

#[test]
fn test_page_entry_inequality_different_address() {
    let pa1 = PA::new(0x2_A000).unwrap();
    let pa2 = PA::new(0x2_B000).unwrap();
    let perms = PagePermissions::read_write();

    let entry1 = PageEntry::new(pa1, perms);
    let entry2 = PageEntry::new(pa2, perms);

    assert_ne!(entry1, entry2);
}

#[test]
fn test_page_entry_inequality_different_permissions() {
    let pa = PA::new(0x2_C000).unwrap();

    let entry1 = PageEntry::new(pa, PagePermissions::read_only());
    let entry2 = PageEntry::new(pa, PagePermissions::read_write());

    assert_ne!(entry1, entry2);
}

// ============================================================================
// PagePermissions Copy Trait Test
// ============================================================================

#[test]
fn test_page_permissions_copy() {
    let perms1 = PagePermissions::read_write();
    let perms2 = perms1; // Copy

    assert_eq!(perms1, perms2);
    assert!(perms1.read()); // Original still valid
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_page_entry_method_chaining() {
    let pa = PA::new(0x3_0000).unwrap();

    let entry = PageEntry::new(pa, PagePermissions::none())
        .with_permissions(PagePermissions::read_write())
        .with_cacheable(false)
        .with_shareable(false)
        .with_device_memory(false)
        .mark_valid();

    assert_eq!(entry.permissions(), PagePermissions::read_write());
    assert!(!entry.is_cacheable());
    assert!(!entry.is_shareable());
    assert!(!entry.is_device_memory());
    assert!(entry.is_valid());
}

#[test]
fn test_page_entry_all_permission_combinations() {
    let pa = PA::new(0x3_1000).unwrap();

    let permission_variants = [
        ("none", PagePermissions::none()),
        ("read-only", PagePermissions::read_only()),
        ("write-only", PagePermissions::write_only()),
        ("execute-only", PagePermissions::execute_only()),
        ("read-write", PagePermissions::read_write()),
        ("read-execute", PagePermissions::read_execute()),
        ("write-execute", PagePermissions::new(false, true, true)),
        ("all", PagePermissions::all()),
    ];

    for (_name, perms) in &permission_variants {
        let entry = PageEntry::new(pa, *perms);
        assert_eq!(entry.permissions(), *perms);
        assert!(entry.is_valid());
    }
}

#[test]
fn test_page_entry_all_security_states() {
    let pa = PA::new(0x3_2000).unwrap();
    let perms = PagePermissions::read_write();

    let states = [SecurityState::NonSecure, SecurityState::Secure, SecurityState::Realm];

    for state in &states {
        let entry = PageEntry::with_security_state(pa, perms, *state);
        assert_eq!(entry.security_state(), *state);
    }
}
