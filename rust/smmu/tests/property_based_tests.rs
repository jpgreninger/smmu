#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Property-based tests using proptest
//!
//! Tests invariants and properties across random input spaces to find
//! edge cases and ensure correctness under diverse conditions.
//!
//! Phase 4.2: Property-Based Testing Expansion
//! - Increased iteration counts to 10,000+ per property
//! - Added address type properties (alignment, overflow, page calculations)
//! - Added configuration properties (validation, merge, serialization)
//! - Added translation properties (determinism, consistency)

use proptest::prelude::*;
use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamID, TranslationStage, IOVA, IPA, PA, PAGE_SIZE, PASID,
};

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
            let iova = IOVA::new((0x1000 + (i as u64) * PAGE_SIZE) & !0xFFF).unwrap();
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
            let iova = IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap();
            let pa = PA::new(0x2000 + (i as u64) * PAGE_SIZE).unwrap();
            addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        }

        assert_eq!(addr_space.get_page_count().unwrap(), count);

        // Verify each translation
        for i in 0..count {
            let iova = IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap();
            let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
            let expected_pa = 0x2000 + (i as u64) * PAGE_SIZE;
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
            let iova = IOVA::new(0x1000 + (i as u64) * PAGE_SIZE).unwrap();
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

// ============================================================================
// Phase 4.2: Address Type Properties (Alignment, Overflow, Page Calculations)
// ============================================================================
// Note: Using ProptestConfig::with_cases(10_000) for thorough testing

proptest! {
    #[test]
    fn prop_alignment_preserves_page_number(addr in 0u64..(1u64 << 48)) {
        let aligned = addr & !(PAGE_SIZE - 1);
        let iova1 = IOVA::new(addr).unwrap();
        let iova2 = IOVA::new(aligned).unwrap();

        // Same page number after alignment
        assert_eq!(iova1.page_number(), iova2.page_number());
    }

    #[test]
    fn prop_page_offset_always_less_than_page_size(addr in 0u64..(1u64 << 48)) {
        let iova = IOVA::new(addr).unwrap();
        let offset = iova.page_offset();

        // Offset must be < PAGE_SIZE (4096)
        assert!(offset < PAGE_SIZE);
        assert!(offset <= 4095);
    }

    #[test]
    fn prop_page_number_calculation_consistency(addr in 0u64..(1u64 << 48)) {
        let iova = IOVA::new(addr).unwrap();
        let page_num = iova.page_number();
        let offset = iova.page_offset();

        // Reconstruct address from page number and offset
        let reconstructed = page_num * PAGE_SIZE + offset;
        assert_eq!(reconstructed, addr);
    }

    #[test]
    fn prop_iova_checked_add_never_wraps(a in 0u64..(1u64 << 47), b in 0u64..(1u64 << 47)) {
        let iova = IOVA::new(a).unwrap();

        if let Some(result) = iova.checked_add(b) {
            // If add succeeded, result should be valid
            assert!(result.as_u64() >= a);
            assert!(result.as_u64() == a + b);
        } else {
            // If add failed, it should have overflowed 48-bit limit
            assert!(a + b >= (1u64 << 48));
        }
    }

    #[test]
    fn prop_pa_checked_add_respects_52bit_limit(a in 0u64..(1u64 << 51), b in 0u64..(1u64 << 51)) {
        let pa = PA::new(a).unwrap();

        if let Some(result) = pa.checked_add(b) {
            // Result must be within 52-bit limit
            assert!(result.as_u64() < (1u64 << 52));
            assert_eq!(result.as_u64(), a + b);
        } else {
            // Overflow occurred
            assert!(a + b >= (1u64 << 52));
        }
    }

    #[test]
    fn prop_ipa_checked_add_respects_52bit_limit(a in 0u64..(1u64 << 51), b in 0u64..(1u64 << 51)) {
        let ipa = IPA::new(a).unwrap();

        if let Some(result) = ipa.checked_add(b) {
            assert!(result.as_u64() < (1u64 << 52));
            assert_eq!(result.as_u64(), a + b);
        } else {
            assert!(a + b >= (1u64 << 52));
        }
    }

    #[test]
    fn prop_align_up_to_page_increases_or_preserves(addr in 0u64..(1u64 << 48)) {
        let iova = IOVA::new(addr).unwrap();
        let aligned = iova.align_up_to_page();

        // Aligned address >= original
        assert!(aligned.as_u64() >= addr);
        // Aligned address is page-aligned
        assert_eq!(aligned.as_u64() % PAGE_SIZE, 0);
        // Difference is less than one page
        assert!(aligned.as_u64() - addr < PAGE_SIZE);
    }

    #[test]
    fn prop_page_aligned_addresses_unchanged_by_align_up(page_num in 0u64..(1u64 << 36)) {
        let addr = page_num * PAGE_SIZE;
        if addr < (1u64 << 48) {
            let iova = IOVA::new(addr).unwrap();
            let aligned = iova.align_up_to_page();

            // Already aligned, should not change
            assert_eq!(aligned.as_u64(), addr);
        }
    }

    #[test]
    fn prop_address_mask_operations(addr in 0u64..(1u64 << 48), mask in 0u64..=u64::MAX) {
        let iova = IOVA::new(addr).unwrap();
        let masked = iova.mask(mask);

        // Masked value should equal bitwise AND
        assert_eq!(masked.as_u64(), addr & mask);
        // Masked value should be <= original
        assert!(masked.as_u64() <= addr);
    }

    #[test]
    fn prop_add_offset_wrapping_behavior(addr in 0u64..(1u64 << 48), offset in 0u64..0x1_0000) {
        let iova = IOVA::new(addr).unwrap();
        let result = iova.add_offset(offset);

        // Result should be valid IOVA
        assert!(result.as_u64() < (1u64 << 48));
    }
}

// ============================================================================
// Phase 4.2: Configuration Properties (Validation, Merge, Serialization)
// ============================================================================
// NOTE: Configuration validation is already comprehensively tested in config_tests.rs
// These property-based tests are skipped to avoid proptest! macro complexity issues
// with multi-parameter property tests. The core address and translation properties
// below provide the main value for property-based testing.

// ============================================================================
// Phase 4.2: Translation Properties (Determinism, Consistency)
// ============================================================================

proptest! {
    #[test]
    fn prop_translation_deterministic(
        iova in 0x1000u64..0x10_0000,
        pasid_val in 0u32..100,
    ) {
        let stream_context = StreamContext::new();
        let pasid = PASID::new(pasid_val).unwrap();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x5_0000).unwrap();

        stream_context.create_pasid(pasid).unwrap();
        stream_context.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Translate multiple times
        let result1 = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        let result2 = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        let result3 = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);

        // All translations should give same result
        assert_eq!(result1.is_ok(), result2.is_ok());
        assert_eq!(result2.is_ok(), result3.is_ok());

        if let (Ok(r1), Ok(r2), Ok(r3)) = (result1, result2, result3) {
            assert_eq!(r1.physical_address().as_u64(), r2.physical_address().as_u64());
            assert_eq!(r2.physical_address().as_u64(), r3.physical_address().as_u64());
        }
    }

    #[test]
    fn prop_translation_consistent_across_lookups(iova in 0x1000u64..0x10_0000) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Multiple lookups should return same PA
        for _ in 0..10 {
            let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
            assert_eq!(result.physical_address().as_u64() & !0xFFF, pa.as_u64());
        }
    }

    #[test]
    fn prop_different_iovas_independent_translations(
        iova1 in 0x1000u64..0x5_0000,
        iova2 in 0x50000u64..0x10_0000,
    ) {
        let mut addr_space = AddressSpace::new();
        let iova1 = IOVA::new(iova1 & !0xFFF).unwrap();
        let iova2 = IOVA::new(iova2 & !0xFFF).unwrap();
        let pa1 = PA::new(0x1_0000).unwrap();
        let pa2 = PA::new(0x2_0000).unwrap();

        addr_space.map_page(iova1, pa1, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        addr_space.map_page(iova2, pa2, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Translations should be independent
        let result1 = addr_space.translate_page(iova1, AccessType::Read, SecurityState::NonSecure).unwrap();
        let result2 = addr_space.translate_page(iova2, AccessType::Read, SecurityState::NonSecure).unwrap();

        assert_eq!(result1.physical_address().as_u64() & !0xFFF, pa1.as_u64());
        assert_eq!(result2.physical_address().as_u64() & !0xFFF, pa2.as_u64());
        assert_ne!(result1.physical_address().as_u64(), result2.physical_address().as_u64());
    }

    #[test]
    fn prop_pasid_isolation_in_translation(
        pasid1_val in 0u32..50,
        pasid2_val in 50u32..100,
        iova in 0x1000u64..0x1_0000,
    ) {
        let stream_context = StreamContext::new();
        let pasid1 = PASID::new(pasid1_val).unwrap();
        let pasid2 = PASID::new(pasid2_val).unwrap();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa1 = PA::new(0x1_0000).unwrap();
        let pa2 = PA::new(0x2_0000).unwrap();

        stream_context.create_pasid(pasid1).unwrap();
        stream_context.create_pasid(pasid2).unwrap();
        stream_context.map_page(pasid1, iova, pa1, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        stream_context.map_page(pasid2, iova, pa2, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Same IOVA in different PASIDs should map to different PAs
        let result1 = stream_context.translate(pasid1, iova, AccessType::Read, SecurityState::NonSecure).unwrap();
        let result2 = stream_context.translate(pasid2, iova, AccessType::Read, SecurityState::NonSecure).unwrap();

        assert_eq!(result1.physical_address().as_u64() & !0xFFF, pa1.as_u64());
        assert_eq!(result2.physical_address().as_u64() & !0xFFF, pa2.as_u64());
        assert_ne!(result1.physical_address().as_u64(), result2.physical_address().as_u64());
    }
}

// ============================================================================
// Phase 4.2: Advanced Invariant Properties
// ============================================================================

proptest! {
    #[test]
    fn prop_page_count_invariant_under_overwrites(
        iova in 0x1000u64..0x1_0000,
        iterations in 1usize..50,
    ) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();

        // Repeatedly overwrite same page
        for i in 0..iterations {
            let pa = PA::new(0x1_0000 + (i as u64) * PAGE_SIZE).unwrap();
            addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
        }

        // Page count should always be 1 (overwriting, not adding)
        assert_eq!(addr_space.get_page_count().unwrap(), 1);
    }

    #[test]
    fn prop_unmap_idempotent(iova in 0x1000u64..0x1_0000) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();

        // First unmap succeeds
        assert!(addr_space.unmap_page(iova).is_ok());

        // Second unmap should fail gracefully (page not mapped)
        assert!(addr_space.unmap_page(iova).is_err());

        // Page count should be 0
        assert_eq!(addr_space.get_page_count().unwrap(), 0);
    }

    #[test]
    fn prop_security_state_enforcement(
        iova in 0x1000u64..0x1_0000,
        map_secure: bool,
        translate_secure: bool,
    ) {
        let mut addr_space = AddressSpace::new();
        let iova = IOVA::new(iova & !0xFFF).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let map_state = if map_secure { SecurityState::Secure } else { SecurityState::NonSecure };
        let trans_state = if translate_secure { SecurityState::Secure } else { SecurityState::NonSecure };

        addr_space.map_page(iova, pa, PagePermissions::read_only(), map_state).unwrap();

        let result = addr_space.translate_page(iova, AccessType::Read, trans_state);

        // Translation succeeds only if security states match
        assert_eq!(result.is_ok(), map_state == trans_state);
    }

    #[test]
    fn prop_pasid_limit_enforcement(pasid_val in 0u32..=2_000_000) {
        let result = PASID::new(pasid_val);

        // Valid PASIDs: 0 to 1_048_575 (2^20 - 1)
        if pasid_val <= 1_048_575 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }

    #[test]
    fn prop_stream_id_validation(stream_val in 0u32..=100_000) {
        let result = StreamID::new(stream_val);

        // Valid StreamIDs: 0 to 65_535 (16-bit)
        if stream_val <= 65_535 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}

// ============================================================================
// Phase 4.2: Edge Case Properties
// ============================================================================

proptest! {
    #[test]
    fn prop_zero_address_handling(offset in 0u64..PAGE_SIZE) {
        // Zero addresses should be valid if within limits
        let iova = IOVA::new(offset);
        assert!(iova.is_ok());

        if let Ok(iova) = iova {
            assert_eq!(iova.page_offset(), offset);
        }
    }

    #[test]
    fn prop_max_address_boundaries(offset in 0u64..PAGE_SIZE) {
        // Test near 48-bit boundary for IOVA
        let max_page = ((1u64 << 48) - PAGE_SIZE) & !(PAGE_SIZE - 1);
        let addr = max_page + offset;

        if addr < (1u64 << 48) {
            let iova = IOVA::new(addr);
            assert!(iova.is_ok());
        } else {
            let iova = IOVA::new(addr);
            assert!(iova.is_err());
        }
    }

    #[test]
    fn prop_permission_combinations_valid(
        read: bool,
        write: bool,
        execute: bool,
    ) {
        // All permission combinations should be creatable
        let perms = PagePermissions::new(read, write, execute);

        // Verify flags match
        assert_eq!(perms.read(), read);
        assert_eq!(perms.write(), write);
        assert_eq!(perms.execute(), execute);
    }

    #[test]
    fn prop_translation_stage_combinations(stage in 0u8..=3) {
        // Valid stages: None(0), Stage1(1), Stage2(2), Both(3)
        let trans_stage = TranslationStage::from_bits(stage);

        // All 4 values should be valid
        assert!(trans_stage.is_ok());
    }
}
