//! Comprehensive AddressSpace tests for ARM SMMU v3 Rust implementation
//!
//! This test suite covers:
//! - Page table operations (map, unmap, translate)
//! - Edge cases (unmapped addresses, permission violations, overlapping mappings)
//! - Concurrency tests for thread-safe operations
//! - RAII cleanup and resource management
//! - Error handling for all error paths
//! - Integration with existing types
//!
//! All tests are written BEFORE implementation following TDD principles.
//! These tests MUST fail initially and pass only after correct implementation.

use smmu::types::{
    AccessType, PageEntry, PagePermissions, SecurityState, TranslationError, TranslationResult,
    IOVA, PA, PAGE_SIZE,
};
use std::sync::{Arc, RwLock};
use std::thread;

// NOTE: AddressSpace is not yet implemented - these tests will fail to compile
// until the AddressSpace struct is created with proper methods.

// Helper constants for testing
const TEST_IOVA_1: u64 = 0x1000_0000;
const TEST_IOVA_2: u64 = 0x2000_0000;
const TEST_IOVA_3: u64 = 0x3000_0000;
const TEST_PA_1: u64 = 0x4000_0000;
const TEST_PA_2: u64 = 0x5000_0000;
const TEST_PA_3: u64 = 0x6000_0000;

// ============================================================================
// BASIC PAGE TABLE OPERATIONS
// ============================================================================

#[test]
fn test_address_space_new() {
    // Test default construction creates empty address space
    let _addr_space = smmu::address_space::AddressSpace::new();
    // Test will fail until AddressSpace::new() is implemented
}

#[test]
fn test_address_space_initial_state_empty() {
    // Verify newly created address space has no mappings
    let addr_space = smmu::address_space::AddressSpace::new();

    // Page count should be zero
    let count = addr_space.get_page_count().expect("Failed to get page count");
    assert_eq!(count, 0, "New address space should have zero pages");

    // Attempt to query unmapped page should return None/false
    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    assert!(
        !addr_space.is_page_mapped(iova).unwrap(),
        "Unmapped page should return false"
    );
}

#[test]
fn test_map_single_page() {
    // Test mapping a single page with valid parameters
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_write();

    // Map the page
    addr_space
        .map_page(iova, pa, perms, SecurityState::NonSecure)
        .expect("Failed to map page");

    // Verify page is now mapped
    assert!(
        addr_space.is_page_mapped(iova).unwrap(),
        "Page should be mapped after map_page()"
    );

    // Verify page count is 1
    assert_eq!(
        addr_space.get_page_count().unwrap(),
        1,
        "Page count should be 1 after mapping one page"
    );
}

#[test]
fn test_map_multiple_pages() {
    // Test mapping multiple distinct pages
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova1 = IOVA::new(TEST_IOVA_1).unwrap();
    let iova2 = IOVA::new(TEST_IOVA_2).unwrap();
    let iova3 = IOVA::new(TEST_IOVA_3).unwrap();

    let pa1 = PA::new(TEST_PA_1).unwrap();
    let pa2 = PA::new(TEST_PA_2).unwrap();
    let pa3 = PA::new(TEST_PA_3).unwrap();

    let perms = PagePermissions::read_only();

    // Map three pages
    addr_space.map_page(iova1, pa1, perms, SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova2, pa2, perms, SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova3, pa3, perms, SecurityState::NonSecure).unwrap();

    // Verify all pages are mapped
    assert!(addr_space.is_page_mapped(iova1).unwrap());
    assert!(addr_space.is_page_mapped(iova2).unwrap());
    assert!(addr_space.is_page_mapped(iova3).unwrap());

    // Verify page count
    assert_eq!(addr_space.get_page_count().unwrap(), 3);
}

#[test]
fn test_map_page_overwrites_existing() {
    // Test that mapping the same IOVA twice overwrites the previous mapping
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa1 = PA::new(TEST_PA_1).unwrap();
    let pa2 = PA::new(TEST_PA_2).unwrap();

    // Map with first PA and read-only permissions
    addr_space
        .map_page(iova, pa1, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Verify first mapping
    let result1 = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    assert_eq!(result1.physical_address().raw(), TEST_PA_1);

    // Remap with second PA and read-write permissions
    addr_space
        .map_page(iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Verify second mapping overwrote first
    let result2 = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    assert_eq!(result2.physical_address().raw(), TEST_PA_2);

    // Verify write is now allowed (was not allowed in first mapping)
    assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_ok());

    // Page count should still be 1 (overwrite, not addition)
    assert_eq!(addr_space.get_page_count().unwrap(), 1);
}

#[test]
fn test_unmap_page() {
    // Test unmapping a previously mapped page
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map a page
    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Verify it's mapped
    assert!(addr_space.is_page_mapped(iova).unwrap());
    assert_eq!(addr_space.get_page_count().unwrap(), 1);

    // Unmap the page
    addr_space.unmap_page(iova).expect("Failed to unmap page");

    // Verify it's no longer mapped
    assert!(!addr_space.is_page_mapped(iova).unwrap());
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_unmap_unmapped_page_returns_error() {
    // Test that unmapping a non-existent page returns an error
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();

    // Attempt to unmap page that was never mapped
    let result = addr_space.unmap_page(iova);

    // Should return error (PageNotMapped or similar)
    assert!(result.is_err(), "Unmapping non-existent page should return error");
}

#[test]
fn test_unmap_partial_subset() {
    // Test unmapping some pages while leaving others mapped
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova1 = IOVA::new(TEST_IOVA_1).unwrap();
    let iova2 = IOVA::new(TEST_IOVA_2).unwrap();
    let iova3 = IOVA::new(TEST_IOVA_3).unwrap();

    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_only();

    // Map three pages
    addr_space.map_page(iova1, pa, perms, SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova2, pa, perms, SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova3, pa, perms, SecurityState::NonSecure).unwrap();

    assert_eq!(addr_space.get_page_count().unwrap(), 3);

    // Unmap middle page
    addr_space.unmap_page(iova2).unwrap();

    // Verify correct state
    assert!(addr_space.is_page_mapped(iova1).unwrap());
    assert!(!addr_space.is_page_mapped(iova2).unwrap());
    assert!(addr_space.is_page_mapped(iova3).unwrap());
    assert_eq!(addr_space.get_page_count().unwrap(), 2);
}

#[test]
fn test_clear_all_mappings() {
    // Test clearing all mappings at once
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Map multiple pages
    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_write();

    for i in 0..10 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 10);

    // Clear all mappings
    addr_space.clear().expect("Failed to clear address space");

    // Verify address space is empty
    assert_eq!(addr_space.get_page_count().unwrap(), 0);

    // Verify no pages are mapped
    for i in 0..10 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        assert!(!addr_space.is_page_mapped(iova).unwrap());
    }
}

// ============================================================================
// TRANSLATION OPERATIONS
// ============================================================================

#[test]
fn test_translate_mapped_page() {
    // Test successful translation of a mapped page
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_write();

    addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Translate for read access
    let result = addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .expect("Translation should succeed");

    // Verify translation result
    assert_eq!(result.physical_address().raw(), TEST_PA_1);
    assert_eq!(result.permissions(), perms);
    assert_eq!(result.security_state(), SecurityState::NonSecure);
}

#[test]
fn test_translate_unmapped_page_fails() {
    // Test that translating an unmapped page returns error
    let addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();

    // Attempt translation on unmapped page
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);

    // Should return PageNotMapped error
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TranslationError::PageNotMapped
    ));
}

#[test]
fn test_translate_page_offset_preservation() {
    // Test that page offset is preserved in translation
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Map page-aligned addresses
    let iova_base = TEST_IOVA_1 & !((PAGE_SIZE as u64) - 1); // Align to page
    let pa_base = TEST_PA_1 & !((PAGE_SIZE as u64) - 1);

    let iova = IOVA::new(iova_base).unwrap();
    let pa = PA::new(pa_base).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Translate with various offsets within the page
    for offset in [0u64, 1, 256, 1024, 2048, 4095] {
        let iova_with_offset = IOVA::new(iova_base + offset).unwrap();
        let result = addr_space
            .translate_page(iova_with_offset, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        // Verify offset is preserved
        let expected_pa = pa_base + offset;
        assert_eq!(
            result.physical_address().raw(),
            expected_pa,
            "Page offset {} should be preserved in translation",
            offset
        );
    }
}

// ============================================================================
// PERMISSION CHECKING
// ============================================================================

#[test]
fn test_permission_read_only_allows_read() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Read should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_permission_read_only_denies_write() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Write should fail
    let result = addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TranslationError::PermissionViolation { .. }
    ));
}

#[test]
fn test_permission_read_only_denies_execute() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Execute should fail
    let result = addr_space.translate_page(iova, AccessType::Execute, SecurityState::NonSecure);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TranslationError::PermissionViolation { .. }
    ));
}

#[test]
fn test_permission_write_only_denies_read() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::write_only(), SecurityState::NonSecure).unwrap();

    // Read should fail (write-only)
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_permission_write_only_allows_write() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::write_only(), SecurityState::NonSecure).unwrap();

    // Write should succeed
    assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_permission_execute_only_allows_execute() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::execute_only(), SecurityState::NonSecure).unwrap();

    // Execute should succeed
    assert!(addr_space.translate_page(iova, AccessType::Execute, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_permission_execute_only_denies_read() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::execute_only(), SecurityState::NonSecure).unwrap();

    // Read should fail
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

#[test]
fn test_permission_read_write_allows_both() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Both read and write should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_permission_all_allows_all_access_types() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::all(), SecurityState::NonSecure).unwrap();

    // All access types should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_ok());
    assert!(addr_space.translate_page(iova, AccessType::Execute, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_permission_none_denies_all_access_types() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map with no permissions - should this even be allowed?
    // If allowed, all access should fail
    let result = addr_space.map_page(iova, pa, PagePermissions::none(), SecurityState::NonSecure);

    // If mapping succeeds, verify all access is denied
    if result.is_ok() {
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_err());
        assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_err());
        assert!(addr_space.translate_page(iova, AccessType::Execute, SecurityState::NonSecure).is_err());
    }
    // Otherwise, mapping with no permissions should fail
}

// ============================================================================
// SECURITY STATE ENFORCEMENT
// ============================================================================

#[test]
fn test_security_state_nonsecure_match() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map as NonSecure
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Access with matching NonSecure state should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_security_state_secure_match() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map as Secure
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::Secure).unwrap();

    // Access with matching Secure state should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::Secure).is_ok());
}

#[test]
fn test_security_state_realm_match() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map as Realm
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::Realm).unwrap();

    // Access with matching Realm state should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::Realm).is_ok());
}

#[test]
fn test_security_state_mismatch_nonsecure_to_secure() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map as Secure
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::Secure).unwrap();

    // Access with NonSecure state should fail (security violation)
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TranslationError::SecurityViolation
    ));
}

#[test]
fn test_security_state_mismatch_secure_to_nonsecure() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map as NonSecure
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Access with Secure state should fail
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::Secure);
    assert!(result.is_err());
}

#[test]
fn test_security_state_mismatch_realm_to_nonsecure() {
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map as Realm
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::Realm).unwrap();

    // Access with NonSecure state should fail
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err());
}

// ============================================================================
// EDGE CASES AND ERROR HANDLING
// ============================================================================

#[test]
fn test_map_page_maximum_valid_address() {
    // Test mapping at maximum valid IOVA
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Maximum 52-bit address (ARM SMMU v3 spec)
    let max_iova = (1u64 << 52) - PAGE_SIZE as u64;
    let iova = IOVA::new(max_iova).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Should succeed
    let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
    assert!(result.is_ok(), "Mapping at maximum valid address should succeed");
}

#[test]
fn test_map_page_minimum_valid_address() {
    // Test mapping at minimum valid IOVA (0)
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(0).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Should succeed
    let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
    assert!(result.is_ok(), "Mapping at address 0 should succeed");
}

#[test]
fn test_sparse_address_space_efficiency() {
    // Test that sparse page table handles large gaps efficiently
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_only();

    // Map pages with huge gaps (64GB, 128GB, 256GB, 512GB)
    let sparse_addresses = [
        0x10_0000_0000u64, // 64GB
        0x20_0000_0000u64, // 128GB
        0x40_0000_0000u64, // 256GB
        0x80_0000_0000u64, // 512GB
    ];

    for &addr in &sparse_addresses {
        let iova = IOVA::new(addr).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // Verify all pages are accessible
    assert_eq!(addr_space.get_page_count().unwrap(), 4);

    for &addr in &sparse_addresses {
        let iova = IOVA::new(addr).unwrap();
        assert!(addr_space.is_page_mapped(iova).unwrap());
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    }
}

#[test]
fn test_page_alignment_handling() {
    // Test that unaligned addresses are handled correctly
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Unaligned IOVA and PA (not on 4KB boundary)
    let unaligned_iova = 0x1234_5678;
    let unaligned_pa = 0x8765_4321;

    let iova = IOVA::new(unaligned_iova).unwrap();
    let pa = PA::new(unaligned_pa).unwrap();

    // Implementation should handle alignment internally
    let result = addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure);

    // Should succeed - implementation aligns addresses
    assert!(result.is_ok(), "Should handle unaligned addresses");
}

#[test]
fn test_get_page_permissions() {
    // Test querying permissions for a mapped page
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_execute();

    addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Query permissions
    let retrieved_perms = addr_space.get_page_permissions(iova).expect("Failed to get permissions");
    assert_eq!(retrieved_perms, perms, "Retrieved permissions should match mapped permissions");
}

#[test]
fn test_get_page_permissions_unmapped() {
    // Test querying permissions for an unmapped page
    let addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();

    // Should return error for unmapped page
    let result = addr_space.get_page_permissions(iova);
    assert!(result.is_err(), "Getting permissions for unmapped page should fail");
}

#[test]
fn test_double_unmap_same_page() {
    // Test unmapping the same page twice
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    // Map and unmap
    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    addr_space.unmap_page(iova).unwrap();

    // Second unmap should fail
    let result = addr_space.unmap_page(iova);
    assert!(result.is_err(), "Second unmap should fail");
}

// ============================================================================
// CONCURRENCY AND THREAD SAFETY
// ============================================================================

#[test]
fn test_concurrent_reads() {
    // Test multiple threads reading concurrently
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Map several pages
    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_only();

    for i in 0..10 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // Wrap in Arc<RwLock<>> for shared access
    let addr_space = Arc::new(RwLock::new(addr_space));

    // Spawn multiple reader threads
    let mut handles = vec![];
    for thread_id in 0..10 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let space = addr_space_clone.read().unwrap();

            // Each thread reads different pages
            let iova = IOVA::new(TEST_IOVA_1 + (thread_id * PAGE_SIZE as u64)).unwrap();
            let result = space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);

            assert!(result.is_ok(), "Concurrent read should succeed");
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_concurrent_writes_different_pages() {
    // Test multiple threads writing to different pages concurrently
    let addr_space = Arc::new(RwLock::new(smmu::address_space::AddressSpace::new()));

    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_write();

    // Spawn multiple writer threads
    let mut handles = vec![];
    for thread_id in 0..10 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            let mut space = addr_space_clone.write().unwrap();

            // Each thread maps a different page
            let iova = IOVA::new(TEST_IOVA_1 + (thread_id * PAGE_SIZE as u64)).unwrap();
            let result = space.map_page(iova, pa, perms, SecurityState::NonSecure);

            assert!(result.is_ok(), "Concurrent map should succeed");
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all pages were mapped
    let space = addr_space.read().unwrap();
    assert_eq!(space.get_page_count().unwrap(), 10);
}

#[test]
fn test_concurrent_map_unmap() {
    // Test concurrent mapping and unmapping operations
    let addr_space = Arc::new(RwLock::new(smmu::address_space::AddressSpace::new()));

    // Pre-map some pages
    {
        let mut space = addr_space.write().unwrap();
        let pa = PA::new(TEST_PA_1).unwrap();
        let perms = PagePermissions::read_only();

        for i in 0..20 {
            let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
            space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
        }
    }

    // Spawn threads: half mapping, half unmapping
    let mut handles = vec![];
    for thread_id in 0..10 {
        let addr_space_clone = Arc::clone(&addr_space);
        let handle = thread::spawn(move || {
            if thread_id % 2 == 0 {
                // Map new pages
                let mut space = addr_space_clone.write().unwrap();
                let iova = IOVA::new(TEST_IOVA_1 + ((20 + thread_id) * PAGE_SIZE as u64)).unwrap();
                let pa = PA::new(TEST_PA_1).unwrap();
                space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
            } else {
                // Unmap existing pages
                let mut space = addr_space_clone.write().unwrap();
                let iova = IOVA::new(TEST_IOVA_1 + (thread_id * PAGE_SIZE as u64)).unwrap();
                let _ = space.unmap_page(iova); // May or may not exist
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify address space is still consistent
    let space = addr_space.read().unwrap();
    let count = space.get_page_count().unwrap();
    assert!(count > 0, "Should have some pages remaining");
}

// ============================================================================
// BULK OPERATIONS
// ============================================================================

#[test]
fn test_map_range() {
    // Test mapping a contiguous range of pages
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let start_iova = IOVA::new(TEST_IOVA_1).unwrap();
    let end_iova = IOVA::new(TEST_IOVA_1 + (10 * PAGE_SIZE as u64)).unwrap();
    let start_pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_write();

    // Map range
    addr_space.map_range(start_iova, end_iova, start_pa, perms).expect("Failed to map range");

    // Verify all pages in range are mapped
    for i in 0..=10 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        assert!(addr_space.is_page_mapped(iova).unwrap(), "Page {} should be mapped", i);
    }
}

#[test]
fn test_unmap_range() {
    // Test unmapping a contiguous range of pages
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Map range first
    let start_iova = IOVA::new(TEST_IOVA_1).unwrap();
    let end_iova = IOVA::new(TEST_IOVA_1 + (10 * PAGE_SIZE as u64)).unwrap();
    let start_pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_range(start_iova, end_iova, start_pa, PagePermissions::read_only()).unwrap();

    // Unmap range
    addr_space.unmap_range(start_iova, end_iova).expect("Failed to unmap range");

    // Verify all pages in range are unmapped
    for i in 0..=10 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        assert!(!addr_space.is_page_mapped(iova).unwrap(), "Page {} should be unmapped", i);
    }
}

#[test]
fn test_map_pages_bulk() {
    // Test bulk mapping operation
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let perms = PagePermissions::read_write();

    // Create vector of (IOVA, PA) mappings
    let mut mappings = vec![];
    for i in 0..100 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_1 + (i * PAGE_SIZE as u64)).unwrap();
        mappings.push((iova, pa));
    }

    // Bulk map
    addr_space.map_pages(&mappings, perms).expect("Failed to bulk map pages");

    // Verify all pages mapped
    assert_eq!(addr_space.get_page_count().unwrap(), 100);
}

#[test]
fn test_unmap_pages_bulk() {
    // Test bulk unmapping operation
    let mut addr_space = smmu::address_space::AddressSpace::new();

    // Map pages first
    let perms = PagePermissions::read_only();
    let mut iovas = vec![];

    for i in 0..50 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        let pa = PA::new(TEST_PA_1).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
        iovas.push(iova);
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 50);

    // Bulk unmap
    addr_space.unmap_pages(&iovas).expect("Failed to bulk unmap pages");

    // Verify all unmapped
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

// ============================================================================
// CACHE INVALIDATION
// ============================================================================

#[test]
fn test_invalidate_page() {
    // Test page-specific cache invalidation
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Invalidate specific page
    addr_space.invalidate_page(iova);

    // Translation should still work (page table is authoritative)
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_invalidate_range() {
    // Test range cache invalidation
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let start_iova = IOVA::new(TEST_IOVA_1).unwrap();
    let end_iova = IOVA::new(TEST_IOVA_1 + (10 * PAGE_SIZE as u64)).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_range(start_iova, end_iova, pa, PagePermissions::read_only()).unwrap();

    // Invalidate range
    addr_space.invalidate_range(start_iova, end_iova);

    // Translations should still work
    let iova = IOVA::new(TEST_IOVA_1 + (5 * PAGE_SIZE as u64)).unwrap();
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_invalidate_all() {
    // Test complete cache invalidation
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_only();

    // Map multiple pages
    for i in 0..20 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // Invalidate all
    addr_space.invalidate_all();

    // All translations should still work (page table is authoritative)
    for i in 0..20 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    }
}

// ============================================================================
// QUERY OPERATIONS
// ============================================================================

#[test]
fn test_get_mapped_ranges() {
    // Test getting all mapped address ranges
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_only();

    // Map contiguous range
    for i in 0..5 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    // Map separate range
    for i in 10..15 {
        let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    }

    let ranges = addr_space.get_mapped_ranges();

    // Should have 2 ranges (contiguous pages consolidated)
    assert_eq!(ranges.len(), 2, "Should have 2 distinct ranges");
}

#[test]
fn test_get_address_space_size() {
    // Test calculating address space size
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let pa = PA::new(TEST_PA_1).unwrap();
    let perms = PagePermissions::read_only();

    // Map first and last pages with gap
    let iova1 = IOVA::new(TEST_IOVA_1).unwrap();
    let iova2 = IOVA::new(TEST_IOVA_1 + (100 * PAGE_SIZE as u64)).unwrap();

    addr_space.map_page(iova1, pa, perms, SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova2, pa, perms, SecurityState::NonSecure).unwrap();

    let size = addr_space.get_address_space_size();

    // Size should span from first to last page
    assert!(size > 0, "Address space size should be non-zero");
    assert!(size >= (100 * PAGE_SIZE as u64), "Should span at least 100 pages");
}

#[test]
fn test_has_overlapping_mappings() {
    // Test checking for overlapping mappings
    let mut addr_space = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    let start = IOVA::new(TEST_IOVA_1 - PAGE_SIZE as u64).unwrap();
    let end = IOVA::new(TEST_IOVA_1 + PAGE_SIZE as u64).unwrap();

    // Should detect overlap
    assert!(addr_space.has_overlapping_mappings(start, end), "Should detect overlapping mapping");

    // Non-overlapping range
    let start2 = IOVA::new(TEST_IOVA_2).unwrap();
    let end2 = IOVA::new(TEST_IOVA_2 + PAGE_SIZE as u64).unwrap();

    assert!(!addr_space.has_overlapping_mappings(start2, end2), "Should not detect overlap");
}

// ============================================================================
// RAII AND RESOURCE MANAGEMENT
// ============================================================================

#[test]
fn test_address_space_drop_cleanup() {
    // Test that dropping AddressSpace properly cleans up resources
    {
        let mut addr_space = smmu::address_space::AddressSpace::new();

        let pa = PA::new(TEST_PA_1).unwrap();
        let perms = PagePermissions::read_only();

        // Map many pages
        for i in 0..1000 {
            let iova = IOVA::new(TEST_IOVA_1 + (i * PAGE_SIZE as u64)).unwrap();
            addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
        }

        // Address space goes out of scope here
    } // Drop should clean up all resources

    // If no memory leaks, test passes
    // (Run with valgrind or ASAN to verify)
}

#[test]
fn test_clone_independence() {
    // Test that cloned AddressSpace is independent
    let mut addr_space1 = smmu::address_space::AddressSpace::new();

    let iova = IOVA::new(TEST_IOVA_1).unwrap();
    let pa = PA::new(TEST_PA_1).unwrap();

    addr_space1.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Clone
    let mut addr_space2 = addr_space1.clone();

    // Verify both have the mapping
    assert!(addr_space1.is_page_mapped(iova).unwrap());
    assert!(addr_space2.is_page_mapped(iova).unwrap());

    // Modify clone
    let iova2 = IOVA::new(TEST_IOVA_2).unwrap();
    addr_space2.map_page(iova2, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Verify original is unchanged
    assert!(!addr_space1.is_page_mapped(iova2).unwrap());
    assert!(addr_space2.is_page_mapped(iova2).unwrap());
}
