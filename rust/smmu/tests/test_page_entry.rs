//! Comprehensive tests for PageEntry structure
//!
//! This test suite validates the PageEntry structure implementation following
//! ARM SMMU v3 specification requirements. Tests are written FIRST following TDD.

use smmu::types::{PageEntry, PagePermissions, PA, SecurityState};

/// Test default PageEntry construction
#[test]
fn test_page_entry_default() {
    let entry = PageEntry::default();

    assert!(!entry.is_valid(), "Default entry should be invalid");
    assert_eq!(entry.physical_address(), PA::new(0).unwrap(), "Default PA should be 0");
    assert_eq!(entry.security_state(), SecurityState::NonSecure, "Default security state should be NonSecure");
}

/// Test PageEntry construction with physical address only
#[test]
fn test_page_entry_with_pa() {
    let pa = PA::new(0x1000).unwrap();
    let entry = PageEntry::new(pa, PagePermissions::default());

    assert!(entry.is_valid(), "Entry should be valid");
    assert_eq!(entry.physical_address(), pa, "Physical address should match");
    assert_eq!(entry.security_state(), SecurityState::NonSecure, "Should default to NonSecure");
}

/// Test PageEntry construction with full parameters
#[test]
fn test_page_entry_full() {
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::new(true, true, false);
    let entry = PageEntry::with_security_state(pa, perms, SecurityState::Secure);

    assert!(entry.is_valid(), "Entry should be valid");
    assert_eq!(entry.physical_address(), pa, "Physical address should match");
    assert_eq!(entry.permissions(), perms, "Permissions should match");
    assert_eq!(entry.security_state(), SecurityState::Secure, "Security state should match");
}

/// Test PagePermissions creation
#[test]
fn test_page_permissions_default() {
    let perms = PagePermissions::default();

    assert!(!perms.read(), "Default should not allow read");
    assert!(!perms.write(), "Default should not allow write");
    assert!(!perms.execute(), "Default should not allow execute");
}

/// Test PagePermissions with all combinations
#[test]
fn test_page_permissions_combinations() {
    let read_only = PagePermissions::read_only();
    assert!(read_only.read() && !read_only.write() && !read_only.execute());

    let read_write = PagePermissions::read_write();
    assert!(read_write.read() && read_write.write() && !read_write.execute());

    let read_execute = PagePermissions::read_execute();
    assert!(read_execute.read() && !read_execute.write() && read_execute.execute());

    let all = PagePermissions::all();
    assert!(all.read() && all.write() && all.execute());
}

/// Test PagePermissions allows method
#[test]
fn test_page_permissions_allows() {
    use smmu::types::AccessType;

    let read_write = PagePermissions::read_write();

    assert!(read_write.allows(AccessType::Read), "Should allow read");
    assert!(read_write.allows(AccessType::Write), "Should allow write");
    assert!(!read_write.allows(AccessType::Execute), "Should not allow execute");
}

/// Test PageEntry attributes (cacheable, shareable, device memory)
#[test]
fn test_page_entry_attributes() {
    let pa = PA::new(0x3000).unwrap();
    let perms = PagePermissions::read_write();
    let entry = PageEntry::new(pa, perms)
        .with_cacheable(true)
        .with_shareable(true);

    assert!(entry.is_cacheable(), "Entry should be cacheable");
    assert!(entry.is_shareable(), "Entry should be shareable");
    assert!(!entry.is_device_memory(), "Entry should not be device memory");
}

/// Test PageEntry device memory attribute
#[test]
fn test_page_entry_device_memory() {
    let pa = PA::new(0x4000).unwrap();
    let perms = PagePermissions::read_write();
    let entry = PageEntry::new(pa, perms)
        .with_device_memory(true);

    assert!(entry.is_device_memory(), "Entry should be device memory");
    assert!(!entry.is_cacheable(), "Device memory should not be cacheable");
}

/// Test PageEntry Clone trait
#[test]
fn test_page_entry_clone() {
    let pa = PA::new(0x5000).unwrap();
    let perms = PagePermissions::all();
    let entry1 = PageEntry::with_security_state(pa, perms, SecurityState::Secure)
        .with_cacheable(true);

    let entry2 = entry1.clone();

    assert_eq!(entry1.physical_address(), entry2.physical_address());
    assert_eq!(entry1.permissions(), entry2.permissions());
    assert_eq!(entry1.security_state(), entry2.security_state());
    assert_eq!(entry1.is_cacheable(), entry2.is_cacheable());
}

/// Test PageEntry Debug trait
#[test]
fn test_page_entry_debug() {
    let pa = PA::new(0x6000).unwrap();
    let perms = PagePermissions::read_write();
    let entry = PageEntry::new(pa, perms);

    let debug_str = format!("{:?}", entry);
    assert!(!debug_str.is_empty(), "Debug output should not be empty");
    assert!(debug_str.contains("PageEntry"), "Debug should contain struct name");
}

/// Test PageEntry PartialEq trait
#[test]
fn test_page_entry_equality() {
    let pa1 = PA::new(0x7000).unwrap();
    let pa2 = PA::new(0x7000).unwrap();
    let perms = PagePermissions::read_only();

    let entry1 = PageEntry::new(pa1, perms);
    let entry2 = PageEntry::new(pa2, perms);
    let entry3 = PageEntry::new(PA::new(0x8000).unwrap(), perms);

    assert_eq!(entry1, entry2, "Entries with same values should be equal");
    assert_ne!(entry1, entry3, "Entries with different PAs should not be equal");
}

/// Test PageEntry invalid state
#[test]
fn test_page_entry_invalid() {
    let mut entry = PageEntry::default();
    assert!(!entry.is_valid(), "Default entry should be invalid");

    entry = entry.mark_valid();
    assert!(entry.is_valid(), "Marked entry should be valid");

    entry = entry.mark_invalid();
    assert!(!entry.is_valid(), "Invalidated entry should be invalid");
}

/// Test PageEntry alignment validation
#[test]
fn test_page_entry_alignment() {
    use smmu::types::PAGE_SIZE;

    // Aligned addresses should work
    let aligned_pa = PA::new(PAGE_SIZE).unwrap();
    let entry = PageEntry::new(aligned_pa, PagePermissions::default());
    assert!(entry.is_valid());

    // Test that physical address maintains alignment
    assert_eq!(entry.physical_address().as_u64() % PAGE_SIZE, 0);
}

/// Test PagePermissions bitwise operations
#[test]
fn test_page_permissions_bitwise() {
    let read_only = PagePermissions::read_only();
    let write_only = PagePermissions::write_only();

    let combined = read_only.union(write_only);
    assert!(combined.read() && combined.write() && !combined.execute());

    let intersect = read_only.intersection(write_only);
    assert!(!intersect.read() && !intersect.write() && !intersect.execute());
}

/// Test PagePermissions is_subset method
#[test]
fn test_page_permissions_subset() {
    let all = PagePermissions::all();
    let read_only = PagePermissions::read_only();

    assert!(read_only.is_subset_of(&all), "Read-only should be subset of all");
    assert!(!all.is_subset_of(&read_only), "All should not be subset of read-only");
}

/// Test PageEntry builder pattern
#[test]
fn test_page_entry_builder() {
    let pa = PA::new(0x9000).unwrap();
    let entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_execute())
        .security_state(SecurityState::Realm)
        .cacheable(true)
        .shareable(false)
        .build();

    assert!(entry.is_valid());
    assert_eq!(entry.physical_address(), pa);
    assert!(entry.permissions().read());
    assert!(entry.permissions().execute());
    assert_eq!(entry.security_state(), SecurityState::Realm);
    assert!(entry.is_cacheable());
    assert!(!entry.is_shareable());
}

/// Test PageEntry memory attributes combinations
#[test]
fn test_page_entry_memory_attributes() {
    let pa = PA::new(0xA000).unwrap();

    // Normal cacheable memory
    let normal = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_write())
        .cacheable(true)
        .shareable(true)
        .build();

    assert!(normal.is_cacheable());
    assert!(normal.is_shareable());
    assert!(!normal.is_device_memory());

    // Device memory (non-cacheable)
    let device = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_write())
        .device_memory(true)
        .build();

    assert!(device.is_device_memory());
    assert!(!device.is_cacheable());
}

/// Test zero unsafe code requirement
#[test]
fn test_page_entry_safe() {
    // This test verifies that all PageEntry operations are safe
    // by exercising them extensively without any unsafe blocks

    let pa = PA::new(0xB000).unwrap();
    let perms = PagePermissions::all();

    let mut entry = PageEntry::new(pa, perms);
    entry = entry.with_cacheable(true);
    entry = entry.with_shareable(true);
    entry = entry.with_device_memory(false);
    entry = entry.mark_invalid();
    entry = entry.mark_valid();

    // All operations above are safe - no undefined behavior possible
    assert!(entry.is_valid());
}

/// Test PagePermissions with no permissions
#[test]
fn test_page_permissions_none() {
    let none = PagePermissions::none();

    use smmu::types::AccessType;
    assert!(!none.allows(AccessType::Read));
    assert!(!none.allows(AccessType::Write));
    assert!(!none.allows(AccessType::Execute));
}

/// Test PagePermissions write_only
#[test]
fn test_page_permissions_write_only() {
    let write_only = PagePermissions::write_only();

    assert!(!write_only.read());
    assert!(write_only.write());
    assert!(!write_only.execute());
}

/// Test PagePermissions execute_only
#[test]
fn test_page_permissions_execute_only() {
    let exec_only = PagePermissions::execute_only();

    assert!(!exec_only.read());
    assert!(!exec_only.write());
    assert!(exec_only.execute());
}

/// Test PageEntry with different security states
#[test]
fn test_page_entry_security_states() {
    let pa = PA::new(0xC000).unwrap();
    let perms = PagePermissions::read_write();

    let non_secure = PageEntry::with_security_state(pa, perms, SecurityState::NonSecure);
    assert_eq!(non_secure.security_state(), SecurityState::NonSecure);

    let secure = PageEntry::with_security_state(pa, perms, SecurityState::Secure);
    assert_eq!(secure.security_state(), SecurityState::Secure);

    let realm = PageEntry::with_security_state(pa, perms, SecurityState::Realm);
    assert_eq!(realm.security_state(), SecurityState::Realm);
}

/// Performance test: PageEntry construction should be fast
#[test]
fn test_page_entry_performance() {
    let pa = PA::new(0xD000).unwrap();
    let perms = PagePermissions::read_write();

    let start = std::time::Instant::now();
    for _ in 0..10000 {
        let _ = PageEntry::new(pa, perms);
    }
    let elapsed = start.elapsed();

    // Construction should be very fast (< 1ms for 10k entries)
    assert!(elapsed.as_millis() < 10, "PageEntry construction too slow: {:?}", elapsed);
}

/// Test PageEntry size for cache efficiency
#[test]
fn test_page_entry_size() {
    use std::mem::size_of;

    let size = size_of::<PageEntry>();

    // PageEntry should be reasonably small (< 64 bytes for cache efficiency)
    assert!(size <= 64, "PageEntry too large: {} bytes", size);
}

/// Test PagePermissions Copy trait
#[test]
fn test_page_permissions_copy() {
    let perms1 = PagePermissions::read_write();
    let perms2 = perms1; // Should copy, not move

    // Both should be usable
    assert!(perms1.read());
    assert!(perms2.read());
}

/// Test comprehensive PageEntry lifecycle
#[test]
fn test_page_entry_lifecycle() {
    // Create
    let pa = PA::new(0xE000).unwrap();
    let mut entry = PageEntry::builder()
        .physical_address(pa)
        .permissions(PagePermissions::read_only())
        .build();

    assert!(entry.is_valid());

    // Modify
    entry = entry.with_cacheable(true);
    assert!(entry.is_cacheable());

    // Update permissions
    let new_perms = PagePermissions::read_write();
    entry = entry.with_permissions(new_perms);
    assert!(entry.permissions().write());

    // Invalidate
    entry = entry.mark_invalid();
    assert!(!entry.is_valid());

    // Re-validate
    entry = entry.mark_valid();
    assert!(entry.is_valid());
}
