//! Property-based tests using proptest
//!
//! Tests invariants and properties across random input spaces to find
//! edge cases and ensure correctness under diverse conditions.

use proptest::prelude::*;
use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA, PASID, PAGE_SIZE};

// ============================================================================
// AddressSpace Property Tests
// ============================================================================

proptest! {
    #[test]
    fn prop_map_then_unmap_restores_state(iova in 0x1000u64..0x100000u64) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        // Map page
        addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
        assert_eq!(addr_space.get_page_count().unwrap(), 1);

        // Unmap page
        addr_space.unmap_page(iova).unwrap();
        assert_eq!(addr_space.get_page_count().unwrap(), 0);
    }

    #[test]
    fn prop_translation_preserves_page_offset(iova in 0x1000u64..0x100000u64) {
        let mut addr_space = AddressSpace::new();
        let page_base = iova & !0xFFF;
        let offset = iova & 0xFFF;
        let iova_base = IOVA::new(page_base).unwrap();
        let iova_offset = IOVA::new(iova).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space.map_page(iova_base, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        let result = addr_space.translate_page(iova_offset, AccessType::Read, SecurityState::NonSecure).unwrap();
        assert_eq!(result.physical_address().as_u64() & 0xFFF, offset);
    }

    #[test]
    fn prop_overwrite_changes_mapping(iova in 0x1000u64..0x100000u64, pa1 in 0x1000u64..0x100000u64, pa2 in 0x1000u64..0x100000u64) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa1 = PA::new(pa1 & !0xFFF).unwrap();
        let pa2 = PA::new(pa2 & !0xFFF).unwrap();

        // Map first PA
        addr_space.map_page(iova, pa1, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

        // Overwrite with second PA
        addr_space.map_page(iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Should still have only one page
        assert_eq!(addr_space.get_page_count().unwrap(), 1);

        // Translation should return second PA
        let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
        assert_eq!(result.physical_address().as_u64() & !0xFFF, pa2.as_u64());
    }

    #[test]
    fn prop_clear_removes_all_mappings(count in 1usize..100) {
        let mut addr_space = AddressSpace::new();

        // Map multiple pages
        for i in 0..count {
            let iova = IOVA::new((0x1000 + u64::from(i) * PAGE_SIZE) & !0xFFF).unwrap();
            let pa = PA::new(0x2000).unwrap();
            addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
        }

        addr_space.clear().unwrap();
        assert_eq!(addr_space.get_page_count().unwrap(), 0);
    }

    #[test]
    fn prop_multiple_pages_independent(count in 1usize..50) {
        let mut addr_space = AddressSpace::new();

        // Map multiple pages
        for i in 0..count {
            let iova = IOVA::new(0x1000 + u64::from(i) * PAGE_SIZE).unwrap();
            let pa = PA::new(0x2000 + u64::from(i) * PAGE_SIZE).unwrap();
            addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        }

        assert_eq!(addr_space.get_page_count().unwrap(), count);

        // Verify each translation
        for i in 0..count {
            let iova = IOVA::new(0x1000 + u64::from(i) * PAGE_SIZE).unwrap();
            let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
            let expected_pa = 0x2000 + u64::from(i) * PAGE_SIZE;
            assert_eq!(result.physical_address().as_u64(), expected_pa);
        }
    }
}

// ============================================================================
// StreamContext Property Tests
// ============================================================================

proptest! {
    #[test]
    fn prop_create_then_remove_pasid(pasid_val in 0u32..100) {
        let stream_context = StreamContext::new();
        let pasid = PASID::new(pasid_val).unwrap();

        stream_context.create_pasid(pasid).unwrap();
        assert!(stream_context.has_pasid(pasid));

        stream_context.remove_pasid(pasid).unwrap();
        assert!(!stream_context.has_pasid(pasid));
    }

    #[test]
    fn prop_multiple_pasids_independent(count in 1usize..50) {
        let stream_context = StreamContext::new();

        // Create multiple PASIDs
        for i in 0..count {
            let pasid = PASID::new(i as u32).unwrap();
            stream_context.create_pasid(pasid).unwrap();
        }

        assert_eq!(stream_context.pasid_count(), count);

        // Verify each PASID exists
        for i in 0..count {
            let pasid = PASID::new(i as u32).unwrap();
            assert!(stream_context.has_pasid(pasid));
        }
    }

    #[test]
    fn prop_translation_per_pasid(pasid_val in 0u32..10, iova in 0x1000u64..0x10000u64) {
        let stream_context = StreamContext::new();
        let pasid = PASID::new(pasid_val).unwrap();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        stream_context.create_pasid(pasid).unwrap();
        stream_context.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
    }

    #[test]
    fn prop_clear_removes_all_pasids(count in 1usize..50) {
        let stream_context = StreamContext::new();

        // Create multiple PASIDs
        for i in 0..count {
            let pasid = PASID::new(i as u32).unwrap();
            stream_context.create_pasid(pasid).unwrap();
        }

        stream_context.clear_all_pasids().unwrap();
        assert_eq!(stream_context.pasid_count(), 0);
    }
}

// ============================================================================
// Type Validation Property Tests
// ============================================================================

proptest! {
    #[test]
    fn prop_iova_validation(addr in 0u64..(1u64 << 52)) {
        // Valid addresses should succeed
        let result = IOVA::new(addr);
        assert!(result.is_ok());
    }

    #[test]
    fn prop_pa_validation(addr in 0u64..(1u64 << 52)) {
        // Valid addresses should succeed
        let result = PA::new(addr);
        assert!(result.is_ok());
    }

    #[test]
    fn prop_pasid_validation(val in 0u32..=1048575u32) {
        // Valid PASIDs (0 to 2^20-1) should succeed
        let result = PASID::new(val);
        assert!(result.is_ok());
    }
}

// ============================================================================
// Permission Property Tests
// ============================================================================

proptest! {
    #[test]
    fn prop_read_only_denies_write(iova in 0x1000u64..0x10000u64) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

        // Read should succeed
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());

        // Write should fail
        assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_err());
    }

    #[test]
    fn prop_read_write_allows_both(iova in 0x1000u64..0x10000u64) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Both read and write should succeed
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
        assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_ok());
    }

    #[test]
    fn prop_execute_only_denies_read_write(iova in 0x1000u64..0x10000u64) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space.map_page(iova, pa, PagePermissions::execute_only(), SecurityState::NonSecure).unwrap();

        // Execute should succeed
        assert!(addr_space.translate_page(iova, AccessType::Execute, SecurityState::NonSecure).is_ok());

        // Read and write should fail
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_err());
        assert!(addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure).is_err());
    }
}

// ============================================================================
// Invariant Tests
// ============================================================================

proptest! {
    #[test]
    fn prop_page_count_equals_mappings(count in 1usize..100) {
        let mut addr_space = AddressSpace::new();

        for i in 0..count {
            let iova = IOVA::new(0x1000 + u64::from(i) * PAGE_SIZE).unwrap();
            let pa = PA::new(0x2000).unwrap();
            addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
        }

        assert_eq!(addr_space.get_page_count().unwrap(), count);
    }

    #[test]
    fn prop_is_mapped_consistent_with_translation(iova in 0x1000u64..0x10000u64) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        // Before mapping
        assert!(!addr_space.is_page_mapped(iova).unwrap());
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_err());

        // After mapping
        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        assert!(addr_space.is_page_mapped(iova).unwrap());
        assert!(addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).is_ok());
    }

    #[test]
    fn prop_pasid_count_consistent(count in 1usize..50) {
        let stream_context = StreamContext::new();

        for i in 0..count {
            let pasid = PASID::new(i as u32).unwrap();
            stream_context.create_pasid(pasid).unwrap();
        }

        // Count should match
        assert_eq!(stream_context.pasid_count(), count);

        // Remove half
        for i in 0..count/2 {
            let pasid = PASID::new(i as u32).unwrap();
            stream_context.remove_pasid(pasid).unwrap();
        }

        // Count should be updated
        assert_eq!(stream_context.pasid_count(), count - count/2);
    }
}
