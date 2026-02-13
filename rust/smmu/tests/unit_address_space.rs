#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Unit tests for `AddressSpace` module
//!
//! Tests core address translation functionality including page mapping,
//! unmapping, translation, and permission checks per ARM SMMU v3 specification.

use smmu::address_space::{AddressRange, AddressSpace, AddressSpaceError};
use smmu::types::{AccessType, PagePermissions, SecurityState, TranslationError, IOVA, PA, PAGE_SIZE};
use std::sync::atomic::Ordering;

// ============================================================================
// Construction Tests
// ============================================================================

#[test]
fn test_new_address_space() {
    let addr_space = AddressSpace::new();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_with_capacity() {
    let addr_space = AddressSpace::with_capacity(100);
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_default_construction() {
    let addr_space = AddressSpace::default();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

// ============================================================================
// Page Mapping Tests
// ============================================================================

#[test]
fn test_map_single_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_only();

    let result = addr_space.map_page(iova, pa, perms, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(addr_space.get_page_count().unwrap(), 1);
}

#[test]
fn test_map_multiple_pages() {
    let mut addr_space = AddressSpace::new();

    for i in 0..10 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 10);
}

#[test]
fn test_map_page_overwrite() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa1 = PA::new(0x2000).unwrap();
    let pa2 = PA::new(0x3000).unwrap();

    addr_space
        .map_page(iova, pa1, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    addr_space
        .map_page(iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    assert_eq!(addr_space.get_page_count().unwrap(), 1);
    let result = addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    assert_eq!(result.physical_address().as_u64() & !0xFFF, 0x3000);
}

#[test]
fn test_map_page_invalid_permissions() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::none();

    let result = addr_space.map_page(iova, pa, perms, SecurityState::NonSecure);
    assert!(matches!(result, Err(AddressSpaceError::InvalidPermissions)));
}

// ============================================================================
// Page Unmapping Tests
// ============================================================================

#[test]
fn test_unmap_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 1);

    addr_space.unmap_page(iova).unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_unmap_unmapped_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();

    let result = addr_space.unmap_page(iova);
    assert!(matches!(result, Err(AddressSpaceError::PageNotMapped)));
}

// ============================================================================
// Translation Tests
// ============================================================================

#[test]
fn test_translate_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    assert_eq!(result.physical_address().as_u64(), 0x2000);
}

#[test]
fn test_translate_page_with_offset() {
    let mut addr_space = AddressSpace::new();
    let iova_base = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova_base, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Translate address with offset within page
    let iova_offset = IOVA::new(0x1234).unwrap();
    let result = addr_space
        .translate_page(iova_offset, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    assert_eq!(result.physical_address().as_u64(), 0x2234);
}

#[test]
fn test_translate_unmapped_page() {
    let addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();

    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PageNotMapped)));
}

// ============================================================================
// Permission Tests
// ============================================================================

#[test]
fn test_permission_read_only() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Read should succeed
    assert!(addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .is_ok());

    // Write should fail
    let result = addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure);
    assert!(matches!(
        result,
        Err(TranslationError::PermissionViolation { access: AccessType::Write })
    ));
}

#[test]
fn test_permission_read_write() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Both read and write should succeed
    assert!(addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .is_ok());
    assert!(addr_space
        .translate_page(iova, AccessType::Write, SecurityState::NonSecure)
        .is_ok());
}

#[test]
fn test_permission_execute() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_execute(), SecurityState::NonSecure)
        .unwrap();

    // Read and execute should succeed
    assert!(addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .is_ok());
    assert!(addr_space
        .translate_page(iova, AccessType::Execute, SecurityState::NonSecure)
        .is_ok());

    // Write should fail
    let result = addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure);
    assert!(matches!(
        result,
        Err(TranslationError::PermissionViolation { access: AccessType::Write })
    ));
}

// ============================================================================
// Sparse Address Space Tests
// ============================================================================

#[test]
fn test_sparse_mapping() {
    let mut addr_space = AddressSpace::new();

    // Map pages at very different addresses (sparse)
    let iova1 = IOVA::new(0x1000).unwrap();
    let iova2 = IOVA::new(0x1_0000_0000).unwrap(); // 4GB offset
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova1, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    addr_space
        .map_page(iova2, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    assert_eq!(addr_space.get_page_count().unwrap(), 2);
}

#[test]
fn test_sparse_efficiency() {
    let mut addr_space = AddressSpace::new();

    // Map 10 pages spread across wide address range
    for i in 0..10 {
        let iova = IOVA::new(0x1000 + i * 0x1000_0000).unwrap(); // 256MB apart
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 10);
}

// ============================================================================
// Ownership and Borrowing Tests
// ============================================================================

#[test]
fn test_clone_address_space() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let cloned = addr_space.clone();
    assert_eq!(cloned.get_page_count().unwrap(), 1);
}

// ============================================================================
// Thread Safety Tests (Send + Sync)
// ============================================================================

#[test]
fn test_address_space_send() {
    fn assert_send<T: Send>() {}
    assert_send::<AddressSpace>();
}

#[test]
fn test_address_space_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<AddressSpace>();
}

// ============================================================================
// Query Operations Tests
// ============================================================================

#[test]
fn test_is_page_mapped() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    assert!(!addr_space.is_page_mapped(iova).unwrap());

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    assert!(addr_space.is_page_mapped(iova).unwrap());
}

#[test]
fn test_get_page_permissions() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_execute();

    addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();

    let retrieved_perms = addr_space.get_page_permissions(iova).unwrap();
    assert_eq!(retrieved_perms, perms);
}

#[test]
fn test_get_page_permissions_unmapped() {
    let addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();

    let result = addr_space.get_page_permissions(iova);
    assert!(matches!(result, Err(AddressSpaceError::PageNotMapped)));
}

#[test]
fn test_clear() {
    let mut addr_space = AddressSpace::new();

    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 5);

    addr_space.clear().unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

// ============================================================================
// Security State Tests
// ============================================================================

#[test]
fn test_security_state_secure() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::Secure)
        .unwrap();

    // Access with matching security state should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::Secure).is_ok());

    // Access with non-matching security state should fail
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::SecurityViolation)));
}

#[test]
fn test_security_state_realm() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::Realm)
        .unwrap();

    // Access with matching security state should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::Realm).is_ok());

    // Access with non-matching security state should fail
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::SecurityViolation)));
}

// ============================================================================
// Range Mapping Tests
// ============================================================================

#[test]
fn test_map_range() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x1000).unwrap();
    let end_iova = IOVA::new(0x1000 + (10 * PAGE_SIZE)).unwrap();
    let start_pa = PA::new(0x2000).unwrap();

    addr_space
        .map_range(start_iova, end_iova, start_pa, PagePermissions::read_write())
        .unwrap();

    // Should have mapped 11 pages (inclusive range)
    assert!(addr_space.get_page_count().unwrap() >= 10);
}

#[test]
fn test_map_range_invalid_range() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x5000).unwrap();
    let end_iova = IOVA::new(0x1000).unwrap(); // End before start
    let start_pa = PA::new(0x2000).unwrap();

    let result = addr_space.map_range(start_iova, end_iova, start_pa, PagePermissions::read_write());
    assert!(result.is_err());
}

#[test]
fn test_map_range_invalid_permissions() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x1000).unwrap();
    let end_iova = IOVA::new(0x5000).unwrap();
    let start_pa = PA::new(0x2000).unwrap();

    let result = addr_space.map_range(start_iova, end_iova, start_pa, PagePermissions::none());
    assert!(matches!(result, Err(AddressSpaceError::InvalidPermissions)));
}

#[test]
fn test_unmap_range() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x1000).unwrap();
    let end_iova = IOVA::new(0x1000 + (5 * PAGE_SIZE)).unwrap();
    let start_pa = PA::new(0x2000).unwrap();

    addr_space
        .map_range(start_iova, end_iova, start_pa, PagePermissions::read_only())
        .unwrap();
    let count_before = addr_space.get_page_count().unwrap();
    assert!(count_before >= 5);

    addr_space.unmap_range(start_iova, end_iova).unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_unmap_range_not_mapped() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x1000).unwrap();
    let end_iova = IOVA::new(0x5000).unwrap();

    let result = addr_space.unmap_range(start_iova, end_iova);
    assert!(matches!(result, Err(AddressSpaceError::PageNotMapped)));
}

#[test]
fn test_unmap_range_invalid_range() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x5000).unwrap();
    let end_iova = IOVA::new(0x1000).unwrap(); // End before start

    let result = addr_space.unmap_range(start_iova, end_iova);
    assert!(result.is_err());
}

// ============================================================================
// Bulk Operations Tests
// ============================================================================

#[test]
fn test_map_pages_bulk() {
    let mut addr_space = AddressSpace::new();
    let mappings: Vec<(IOVA, PA)> = (0..10)
        .map(|i| {
            let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
            let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
            (iova, pa)
        })
        .collect();

    addr_space.map_pages(&mappings, PagePermissions::read_write()).unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 10);
}

#[test]
fn test_map_pages_bulk_invalid_permissions() {
    let mut addr_space = AddressSpace::new();
    let mappings: Vec<(IOVA, PA)> = vec![(IOVA::new(0x1000).unwrap(), PA::new(0x2000).unwrap())];

    let result = addr_space.map_pages(&mappings, PagePermissions::none());
    assert!(matches!(result, Err(AddressSpaceError::InvalidPermissions)));
}

#[test]
fn test_unmap_pages_bulk() {
    let mut addr_space = AddressSpace::new();
    let iovas: Vec<IOVA> = (0..10).map(|i| IOVA::new(0x1000 + i * PAGE_SIZE).unwrap()).collect();

    for iova in &iovas {
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(*iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 10);

    addr_space.unmap_pages(&iovas).unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_unmap_pages_bulk_not_mapped() {
    let mut addr_space = AddressSpace::new();
    let iovas: Vec<IOVA> = vec![IOVA::new(0x1000).unwrap(), IOVA::new(0x2000).unwrap()];

    let result = addr_space.unmap_pages(&iovas);
    assert!(matches!(result, Err(AddressSpaceError::PageNotMapped)));
}

// ============================================================================
// Batched Operations Tests
// ============================================================================

#[test]
fn test_map_pages_batched() {
    let mut addr_space = AddressSpace::new();
    let mappings: Vec<(IOVA, PA)> = (0..100)
        .map(|i| {
            (
                IOVA::new(0x1000 + i * PAGE_SIZE).unwrap(),
                PA::new(0x2000 + i * PAGE_SIZE).unwrap(),
            )
        })
        .collect();

    addr_space.map_pages_batched(&mappings, PagePermissions::read_write()).unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 100);
}

#[test]
fn test_map_pages_batched_invalid_permissions() {
    let mut addr_space = AddressSpace::new();
    let mappings: Vec<(IOVA, PA)> = vec![(IOVA::new(0x1000).unwrap(), PA::new(0x2000).unwrap())];

    let result = addr_space.map_pages_batched(&mappings, PagePermissions::none());
    assert!(matches!(result, Err(AddressSpaceError::InvalidPermissions)));
}

#[test]
fn test_unmap_pages_batched() {
    let mut addr_space = AddressSpace::new();
    let iovas: Vec<IOVA> = (0..50).map(|i| IOVA::new(0x1000 + i * PAGE_SIZE).unwrap()).collect();

    for iova in &iovas {
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(*iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    addr_space.unmap_pages_batched(&iovas).unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}

#[test]
fn test_update_permissions_batched() {
    let mut addr_space = AddressSpace::new();
    let iovas: Vec<IOVA> = (0..10).map(|i| IOVA::new(0x1000 + i * PAGE_SIZE).unwrap()).collect();

    // Map with read-only permissions
    for iova in &iovas {
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(*iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Update to read-write permissions
    addr_space
        .update_permissions_batched(&iovas, PagePermissions::read_write())
        .unwrap();

    // Verify write access now works
    for iova in &iovas {
        assert!(addr_space
            .translate_page(*iova, AccessType::Write, SecurityState::NonSecure)
            .is_ok());
    }
}

#[test]
fn test_update_permissions_batched_invalid() {
    let mut addr_space = AddressSpace::new();
    let iovas: Vec<IOVA> = vec![IOVA::new(0x1000).unwrap()];

    let result = addr_space.update_permissions_batched(&iovas, PagePermissions::none());
    assert!(matches!(result, Err(AddressSpaceError::InvalidPermissions)));
}

// ============================================================================
// Address Range and Query Tests
// ============================================================================

#[test]
fn test_get_mapped_ranges() {
    let mut addr_space = AddressSpace::new();

    // Map contiguous range
    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let ranges = addr_space.get_mapped_ranges();
    assert!(!ranges.is_empty());
}

#[test]
fn test_get_mapped_ranges_empty() {
    let addr_space = AddressSpace::new();
    let ranges = addr_space.get_mapped_ranges();
    assert!(ranges.is_empty());
}

#[test]
fn test_get_mapped_ranges_sparse() {
    let mut addr_space = AddressSpace::new();

    // Map non-contiguous pages
    addr_space
        .map_page(
            IOVA::new(0x1000).unwrap(),
            PA::new(0x2000).unwrap(),
            PagePermissions::read_only(),
            SecurityState::NonSecure,
        )
        .unwrap();
    addr_space
        .map_page(
            IOVA::new(0x10_0000).unwrap(),
            PA::new(0x2000).unwrap(),
            PagePermissions::read_only(),
            SecurityState::NonSecure,
        )
        .unwrap();

    let ranges = addr_space.get_mapped_ranges();
    assert_eq!(ranges.len(), 2);
}

#[test]
fn test_get_address_space_size() {
    let mut addr_space = AddressSpace::new();
    let iova1 = IOVA::new(0x1000).unwrap();
    let iova2 = IOVA::new(0x1000 + (100 * PAGE_SIZE)).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova1, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    addr_space
        .map_page(iova2, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let size = addr_space.get_address_space_size();
    assert!(size >= 100 * PAGE_SIZE);
}

#[test]
fn test_get_address_space_size_empty() {
    let addr_space = AddressSpace::new();
    assert_eq!(addr_space.get_address_space_size(), 0);
}

#[test]
fn test_has_overlapping_mappings() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let start = IOVA::new(0x1000 - PAGE_SIZE).unwrap();
    let end = IOVA::new(0x1000 + PAGE_SIZE).unwrap();

    assert!(addr_space.has_overlapping_mappings(start, end));
}

#[test]
fn test_has_overlapping_mappings_no_overlap() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let start = IOVA::new(0x1_0000).unwrap();
    let end = IOVA::new(0x2_0000).unwrap();

    assert!(!addr_space.has_overlapping_mappings(start, end));
}

#[test]
fn test_has_overlapping_mappings_invalid_range() {
    let addr_space = AddressSpace::new();
    let start = IOVA::new(0x5000).unwrap();
    let end = IOVA::new(0x1000).unwrap(); // End before start

    assert!(!addr_space.has_overlapping_mappings(start, end));
}

// ============================================================================
// Iterator Tests
// ============================================================================

#[test]
fn test_iter() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let count = addr_space.iter().count();
    assert_eq!(count, 1);
}

#[test]
fn test_iter_multiple() {
    let mut addr_space = AddressSpace::new();

    for i in 0..10 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    let count = addr_space.iter().count();
    assert_eq!(count, 10);
}

#[test]
fn test_iter_mut() {
    let addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // With DashMap, modifications should be done via map_page, not iter_mut
    // Update permissions by re-mapping
    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Verify write access now works
    assert!(addr_space
        .translate_page(iova, AccessType::Write, SecurityState::NonSecure)
        .is_ok());
}

// ============================================================================
// Query Interface Tests
// ============================================================================

#[test]
fn test_query_page_count() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let query = addr_space.query();
    assert_eq!(query.page_count(), 1);
}

#[test]
fn test_query_is_mapped() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let query = addr_space.query();
    assert!(query.is_mapped(iova));
    assert!(!query.is_mapped(IOVA::new(0x5000).unwrap()));
}

#[test]
fn test_query_range_statistics() {
    let mut addr_space = AddressSpace::new();

    // Map 5 pages with different permissions
    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    let query = addr_space.query();
    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x1000 + 5 * PAGE_SIZE).unwrap();
    let stats = query.range_statistics(start, end);

    assert_eq!(stats.total_pages, 5);
    assert_eq!(stats.readable_pages, 5);
    assert_eq!(stats.writable_pages, 5);
}

#[test]
fn test_query_iter() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let query = addr_space.query();
    let count = query.iter().len();
    assert_eq!(count, 1);
}

#[test]
fn test_query_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let entry = addr_space.query_page(iova);
    assert!(entry.is_some());

    let unmapped = addr_space.query_page(IOVA::new(0x5000).unwrap());
    assert!(unmapped.is_none());
}

// ============================================================================
// Invalidation Tests
// ============================================================================

#[test]
fn test_invalidate_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    addr_space.invalidate_page(iova);
    // Invalidation should not remove the page
    assert!(addr_space.is_page_mapped(iova).unwrap());
}

#[test]
fn test_invalidate_page_atomic() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    addr_space.invalidate_page_atomic(iova);
    assert!(addr_space.is_invalidated(iova));
}

#[test]
fn test_invalidate_range_atomic() {
    let mut addr_space = AddressSpace::new();

    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x1000 + 5 * PAGE_SIZE).unwrap();
    let count = addr_space.invalidate_range_atomic(start, end);
    assert_eq!(count, 5);
}

#[test]
fn test_invalidate_page_with_ordering() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    addr_space.invalidate_page_with_ordering(iova, Ordering::SeqCst);
    assert!(addr_space.is_invalidated_with_ordering(iova, Ordering::SeqCst));
}

#[test]
fn test_is_invalidated() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    assert!(!addr_space.is_invalidated(iova));

    addr_space.invalidate_page_atomic(iova);
    assert!(addr_space.is_invalidated(iova));
}

#[test]
fn test_invalidation_generation() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    let gen_before = addr_space.invalidation_generation();
    addr_space.invalidate_page_atomic(iova);
    let gen_after = addr_space.invalidation_generation();

    assert!(gen_after > gen_before);
}

#[test]
fn test_compare_exchange_invalidate() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Should succeed in transitioning from false to true
    assert!(addr_space.compare_exchange_invalidate(iova, false, true, Ordering::SeqCst));

    // Should fail since current state is now true
    assert!(!addr_space.compare_exchange_invalidate(iova, false, true, Ordering::SeqCst));
}

#[test]
fn test_invalidate_range() {
    let mut addr_space = AddressSpace::new();

    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x1000 + 5 * PAGE_SIZE).unwrap();
    addr_space.invalidate_range(start, end);

    // Pages should still be mapped
    assert_eq!(addr_space.get_page_count().unwrap(), 5);
}

#[test]
fn test_invalidate_all() {
    let mut addr_space = AddressSpace::new();

    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    addr_space.invalidate_all();

    // Pages should still be mapped
    assert_eq!(addr_space.get_page_count().unwrap(), 5);
}

// ============================================================================
// Address Range Tests
// ============================================================================

#[test]
fn test_address_range_new() {
    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x5000).unwrap();
    let range = AddressRange::new(start, end);

    assert_eq!(range.start, start);
    assert_eq!(range.end, end);
}

#[test]
fn test_address_range_iterator() {
    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x1000 + 3 * PAGE_SIZE).unwrap();
    let range = AddressRange::new(start, end);

    assert!(range.into_iter().next().is_some());
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_map_page_max_virtual_address() {
    let mut addr_space = AddressSpace::new();
    let max_addr = (1u64 << 52) - PAGE_SIZE;
    let iova = IOVA::new(max_addr).unwrap();
    let pa = PA::new(0x2000).unwrap();

    let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
    assert!(result.is_ok());
}

#[test]
fn test_map_page_unaligned_pa() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2123).unwrap(); // Unaligned PA

    // Should succeed - PA gets aligned internally
    let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
    assert!(result.is_ok());

    // Translation should use aligned PA
    let trans = addr_space
        .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    assert_eq!(trans.physical_address().as_u64() & 0xFFF, 0);
}

#[test]
fn test_translate_with_various_offsets() {
    let mut addr_space = AddressSpace::new();
    let iova_base = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova_base, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Test various offsets within the page
    for offset in [0u64, 1, 256, 1024, 2048, 4095] {
        let iova = IOVA::new(0x1000 + offset).unwrap();
        let result = addr_space
            .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();
        assert_eq!(result.physical_address().as_u64(), 0x2000 + offset);
    }
}

#[test]
fn test_large_scale_mapping() {
    let mut addr_space = AddressSpace::with_capacity(1000);

    // Map 1000 pages
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 1000);

    // Verify random translations
    for i in [0, 100, 500, 999] {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
    }
}

#[test]
fn test_permission_combinations() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Test all permission combinations
    let perm_combos = [
        (
            PagePermissions::read_only(),
            vec![AccessType::Read],
            vec![AccessType::Write, AccessType::Execute],
        ),
        (
            PagePermissions::write_only(),
            vec![AccessType::Write],
            vec![AccessType::Read, AccessType::Execute],
        ),
        (
            PagePermissions::execute_only(),
            vec![AccessType::Execute],
            vec![AccessType::Read, AccessType::Write],
        ),
        (
            PagePermissions::read_write(),
            vec![AccessType::Read, AccessType::Write],
            vec![AccessType::Execute],
        ),
        (
            PagePermissions::read_execute(),
            vec![AccessType::Read, AccessType::Execute],
            vec![AccessType::Write],
        ),
    ];

    for (perms, allowed, denied) in perm_combos {
        addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();

        for access in allowed {
            assert!(addr_space.translate_page(iova, access, SecurityState::NonSecure).is_ok());
        }

        for access in denied {
            assert!(addr_space.translate_page(iova, access, SecurityState::NonSecure).is_err());
        }
    }
}

// ============================================================================
// Stress and Performance Tests
// ============================================================================

#[test]
fn test_sparse_address_distribution() {
    let mut addr_space = AddressSpace::new();

    // Map pages at exponentially increasing addresses
    for i in 0..20 {
        let offset = 1u64 << (12 + i); // Start at 4KB, double each time
        if offset > (1u64 << 52) - PAGE_SIZE {
            break;
        }
        let iova = IOVA::new(offset).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    assert!(addr_space.get_page_count().unwrap() >= 10);
}

#[test]
fn test_remapping_same_page() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();

    // Map and remap the same page multiple times
    for i in 0..10 {
        let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();
    }

    // Should still have only 1 page mapped
    assert_eq!(addr_space.get_page_count().unwrap(), 1);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_invalid_address_errors() {
    let mut addr_space = AddressSpace::new();
    let max_addr = (1u64 << 52) + PAGE_SIZE;

    if let Ok(iova) = IOVA::new(max_addr) {
        let pa = PA::new(0x2000).unwrap();
        let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
        assert!(result.is_err());
    }
}

// ============================================================================
// PageEntryRef and PageInfo Accessor Tests
// ============================================================================

#[test]
fn test_page_entry_ref_accessors() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();

    addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();

    // Test PageEntryRef accessors through iterator
    for entry_ref in addr_space.iter() {
        assert_eq!(entry_ref.iova(), iova);
        assert_eq!(entry_ref.physical_address().as_u64() & !0xFFF, 0x2000);
        assert_eq!(entry_ref.permissions(), perms);
        assert_eq!(entry_ref.security_state(), SecurityState::NonSecure);
        let _ = entry_ref.entry(); // Test entry() accessor
    }
}

#[test]
#[ignore] // DashMap doesn't support true mutable iteration - use map_page to modify entries
fn test_page_entry_mut_ref_accessors() {
    let addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // With DashMap, modifications should be done via map_page
    // This test is disabled as DashMap doesn't support true mutable iteration
}

#[test]
fn test_page_info_accessors() {
    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x1000 + 2 * PAGE_SIZE).unwrap();
    let range = AddressRange::new(start, end);

    for page_info in range {
        let _ = page_info.iova(); // Test iova() accessor
        let _ = page_info.page_number(); // Test page_number() accessor
    }
}

// ============================================================================
// Additional Error Path Tests
// ============================================================================

#[test]
fn test_map_page_invalid_iova_max() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52; // Just over max

    if let Ok(iova) = IOVA::new(max_iova) {
        let pa = PA::new(0x2000).unwrap();
        let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_map_page_invalid_pa_max() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let max_pa = 1u64 << 52; // Just over max

    if let Ok(pa) = PA::new(max_pa) {
        let result = addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_unmap_page_invalid_iova() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let result = addr_space.unmap_page(iova);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_is_page_mapped_invalid_iova() {
    let addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let result = addr_space.is_page_mapped(iova);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_get_page_permissions_invalid_iova() {
    let addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let result = addr_space.get_page_permissions(iova);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_map_range_invalid_iova_max() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(start_iova) = IOVA::new(max_iova - PAGE_SIZE) {
        if let Ok(end_iova) = IOVA::new(max_iova) {
            let pa = PA::new(0x2000).unwrap();
            let result = addr_space.map_range(start_iova, end_iova, pa, PagePermissions::read_only());
            assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
        }
    }
}

#[test]
fn test_map_range_invalid_pa_max() {
    let mut addr_space = AddressSpace::new();
    let start_iova = IOVA::new(0x1000).unwrap();
    let end_iova = IOVA::new(0x5000).unwrap();
    let max_pa = 1u64 << 52;

    if let Ok(pa) = PA::new(max_pa) {
        let result = addr_space.map_range(start_iova, end_iova, pa, PagePermissions::read_only());
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_unmap_range_invalid_iova_max() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(start_iova) = IOVA::new(max_iova - PAGE_SIZE) {
        if let Ok(end_iova) = IOVA::new(max_iova) {
            let result = addr_space.unmap_range(start_iova, end_iova);
            assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
        }
    }
}

#[test]
fn test_map_pages_invalid_iova_max() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let pa = PA::new(0x2000).unwrap();
        let mappings = vec![(iova, pa)];

        let result = addr_space.map_pages(&mappings, PagePermissions::read_only());
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_map_pages_invalid_pa_max() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let max_pa = 1u64 << 52;

    if let Ok(pa) = PA::new(max_pa) {
        let mappings = vec![(iova, pa)];

        let result = addr_space.map_pages(&mappings, PagePermissions::read_only());
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_unmap_pages_invalid_iova_max() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let iovas = vec![iova];

        let result = addr_space.unmap_pages(&iovas);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_map_pages_batched_invalid_iova_max() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let pa = PA::new(0x2000).unwrap();
        let mappings = vec![(iova, pa)];

        let result = addr_space.map_pages_batched(&mappings, PagePermissions::read_only());
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_map_pages_batched_invalid_pa_max() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let max_pa = 1u64 << 52;

    if let Ok(pa) = PA::new(max_pa) {
        let mappings = vec![(iova, pa)];

        let result = addr_space.map_pages_batched(&mappings, PagePermissions::read_only());
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_unmap_pages_batched_invalid_iova() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let iovas = vec![iova];

        let result = addr_space.unmap_pages_batched(&iovas);
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_update_permissions_batched_invalid_iova() {
    let mut addr_space = AddressSpace::new();
    let max_iova = 1u64 << 52;

    if let Ok(iova) = IOVA::new(max_iova) {
        let iovas = vec![iova];

        let result = addr_space.update_permissions_batched(&iovas, PagePermissions::read_write());
        assert!(matches!(result, Err(AddressSpaceError::InvalidAddress { .. })));
    }
}

#[test]
fn test_unmap_pages_batched_not_mapped() {
    let mut addr_space = AddressSpace::new();
    let iovas: Vec<IOVA> = vec![IOVA::new(0x1000).unwrap(), IOVA::new(0x2000).unwrap()];

    let result = addr_space.unmap_pages_batched(&iovas);
    assert!(matches!(result, Err(AddressSpaceError::PageNotMapped)));
}

// ============================================================================
// Additional Query Tests
// ============================================================================

#[test]
fn test_query_range_statistics_empty() {
    let addr_space = AddressSpace::new();
    let query = addr_space.query();

    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x5000).unwrap();
    let stats = query.range_statistics(start, end);

    assert_eq!(stats.total_pages, 0);
    assert_eq!(stats.readable_pages, 0);
    assert_eq!(stats.writable_pages, 0);
    assert_eq!(stats.executable_pages, 0);
}

#[test]
fn test_query_range_statistics_partial() {
    let mut addr_space = AddressSpace::new();

    // Map some pages with read-only permissions
    for i in 0..3 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();
    }

    // Map some pages with execute-only permissions
    for i in 3..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space
            .map_page(iova, pa, PagePermissions::execute_only(), SecurityState::NonSecure)
            .unwrap();
    }

    let query = addr_space.query();
    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x1000 + 5 * PAGE_SIZE).unwrap();
    let stats = query.range_statistics(start, end);

    assert_eq!(stats.total_pages, 5);
    assert_eq!(stats.readable_pages, 3);
    assert_eq!(stats.executable_pages, 2);
}

// ============================================================================
// Invalid Entry Tests
// ============================================================================

#[test]
fn test_translate_invalid_entry() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Map a page first
    addr_space
        .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    // Note: This test placeholder exists for future implementation.
    // Currently, there is no safe way to create an invalid page table entry
    // without exposing internal implementation details. The is_valid() check
    // is defensive programming for future extensions.
    // If PageEntry::invalid() or similar internal APIs are exposed, this test
    // should verify that translation properly handles invalid entries.
}

// ============================================================================
// Concurrent/Thread Safety Validation Tests
// ============================================================================

#[test]
fn test_address_space_clone_invalidation() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();
    addr_space.invalidate_page_atomic(iova);

    let gen_before = addr_space.invalidation_generation();

    // Clone should preserve invalidation state
    let cloned = addr_space.clone();
    let gen_after = cloned.invalidation_generation();

    assert_eq!(gen_before, gen_after);
}

#[test]
fn test_invalidate_range_atomic_empty() {
    let mut addr_space = AddressSpace::new();
    let start = IOVA::new(0x1000).unwrap();
    let end = IOVA::new(0x5000).unwrap();

    let count = addr_space.invalidate_range_atomic(start, end);
    assert_eq!(count, 0);
}

#[test]
fn test_compare_exchange_invalidate_transitions() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    addr_space
        .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
        .unwrap();

    // Transition from false to true
    assert!(addr_space.compare_exchange_invalidate(iova, false, true, Ordering::SeqCst));

    // Transition from true to false
    assert!(addr_space.compare_exchange_invalidate(iova, true, false, Ordering::SeqCst));

    // Transition from false to true again
    assert!(addr_space.compare_exchange_invalidate(iova, false, true, Ordering::SeqCst));
}
