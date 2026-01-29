//! Unit tests for AddressSpace module
//!
//! Tests core address translation functionality including page mapping,
//! unmapping, translation, and permission checks per ARM SMMU v3 specification.

use smmu::address_space::{AddressSpace, AddressSpaceError};
use smmu::types::{AccessType, PagePermissions, SecurityState, TranslationError, IOVA, PA, PAGE_SIZE};

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
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE as u64).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE as u64).unwrap();
        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 10);
}

#[test]
fn test_map_page_overwrite() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa1 = PA::new(0x2000).unwrap();
    let pa2 = PA::new(0x3000).unwrap();

    addr_space.map_page(iova, pa1, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    assert_eq!(addr_space.get_page_count().unwrap(), 1);
    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
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

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
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

    addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    assert_eq!(result.physical_address().as_u64(), 0x2000);
}

#[test]
fn test_translate_page_with_offset() {
    let mut addr_space = AddressSpace::new();
    let iova_base = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space.map_page(iova_base, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Translate address with offset within page
    let iova_offset = IOVA::new(0x1234).unwrap();
    let result = addr_space.translate_page(iova_offset, AccessType::Read, SecurityState::NonSecure).unwrap();
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

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    // Read should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());

    // Write should fail
    let result = addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PermissionViolation { access: AccessType::Write })));
}

#[test]
fn test_permission_read_write() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Both read and write should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_ok());
}

#[test]
fn test_permission_execute() {
    let mut addr_space = AddressSpace::new();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    addr_space.map_page(iova, pa, PagePermissions::read_execute(), SecurityState::NonSecure).unwrap();

    // Read and execute should succeed
    assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    assert!(addr_space.translate_page(iova, AccessType::Execute, SecurityState::NonSecure).is_ok());

    // Write should fail
    let result = addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure);
    assert!(matches!(result, Err(TranslationError::PermissionViolation { access: AccessType::Write })));
}

// ============================================================================
// Sparse Address Space Tests
// ============================================================================

#[test]
fn test_sparse_mapping() {
    let mut addr_space = AddressSpace::new();

    // Map pages at very different addresses (sparse)
    let iova1 = IOVA::new(0x1000).unwrap();
    let iova2 = IOVA::new(0x100000000).unwrap(); // 4GB offset
    let pa = PA::new(0x2000).unwrap();

    addr_space.map_page(iova1, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    addr_space.map_page(iova2, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

    assert_eq!(addr_space.get_page_count().unwrap(), 2);
}

#[test]
fn test_sparse_efficiency() {
    let mut addr_space = AddressSpace::new();

    // Map 10 pages spread across wide address range
    for i in 0..10 {
        let iova = IOVA::new(0x1000 + i * 0x10000000).unwrap(); // 256MB apart
        let pa = PA::new(0x2000 + i * PAGE_SIZE as u64).unwrap();
        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
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

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

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

    addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
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
fn test_clear() {
    let mut addr_space = AddressSpace::new();

    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * PAGE_SIZE as u64).unwrap();
        let pa = PA::new(0x2000 + i * PAGE_SIZE as u64).unwrap();
        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    }

    assert_eq!(addr_space.get_page_count().unwrap(), 5);

    addr_space.clear().unwrap();
    assert_eq!(addr_space.get_page_count().unwrap(), 0);
}
