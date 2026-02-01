#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive test suite for ARM SMMU v3 address types
//!
//! Tests for 100% coverage of:
//! - IOVA (Input/Output Virtual Address)
//! - IPA (Intermediate Physical Address)
//! - PA (Physical Address)
//!
//! Coverage areas:
//! 1. Overflow handling (`checked_add`)
//! 2. Wrapping operations (`add_offset`)
//! 3. Mask operations
//! 4. Alignment operations
//! 5. Accessor methods
//! 6. Formatting (Display, Debug)
//! 7. Const constructors

use smmu::{IOVA, IPA, PA, PAGE_SIZE};

// ============================================================================
// Section 1: IOVA Tests - Overflow Handling
// ============================================================================

#[test]
fn test_iova_checked_add_success() {
    let iova = IOVA::new(0x1000).unwrap();
    let result = iova.checked_add(0x1000);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), 0x2000);
}

#[test]
fn test_iova_checked_add_overflow_returns_none() {
    let iova = IOVA::new(u64::MAX - 100).unwrap();
    let result = iova.checked_add(200);
    assert!(result.is_none());
}

#[test]
fn test_iova_checked_add_at_max_boundary() {
    let iova = IOVA::new(u64::MAX - 1).unwrap();

    // Adding 1 should succeed
    let result = iova.checked_add(1);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), u64::MAX);

    // Adding 2 should overflow
    let result = iova.checked_add(2);
    assert!(result.is_none());
}

#[test]
fn test_iova_checked_add_zero_no_change() {
    let iova = IOVA::new(0x5000).unwrap();
    let result = iova.checked_add(0);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), 0x5000);
}

#[test]
fn test_iova_checked_add_max_value_overflow() {
    let iova = IOVA::new(u64::MAX).unwrap();
    let result = iova.checked_add(1);
    assert!(result.is_none());
}

// ============================================================================
// Section 2: IOVA Tests - Wrapping Operations
// ============================================================================

#[test]
fn test_iova_add_offset_wraps_correctly() {
    let iova = IOVA::new(u64::MAX - 10).unwrap();
    let result = iova.add_offset(20);

    // Should wrap around
    assert_eq!(result.as_u64(), 9);
}

#[test]
fn test_iova_add_offset_no_overflow() {
    let iova = IOVA::new(0x1000).unwrap();
    let result = iova.add_offset(0x500);
    assert_eq!(result.as_u64(), 0x1500);
}

#[test]
fn test_iova_add_offset_preserves_page_offset() {
    let iova = IOVA::new(0x1234).unwrap();
    let offset = iova.page_offset();

    // Add a page-aligned value
    let result = iova.add_offset(PAGE_SIZE);

    // Page offset should be preserved
    assert_eq!(result.page_offset(), offset);
}

#[test]
fn test_iova_add_offset_wraps_at_boundary() {
    let iova = IOVA::new(u64::MAX).unwrap();
    let result = iova.add_offset(1);
    assert_eq!(result.as_u64(), 0);
}

// ============================================================================
// Section 3: IOVA Tests - Mask Operations
// ============================================================================

#[test]
fn test_iova_mask_page_alignment() {
    let iova = IOVA::new(0x1234).unwrap();
    let page_mask = 0xFFFF_FFFF_FFFF_F000;
    let masked = iova.mask(page_mask);

    // Should clear the lower 12 bits
    assert_eq!(masked.as_u64(), 0x1000);
    assert!(masked.is_page_aligned());
}

#[test]
fn test_iova_mask_various_patterns() {
    let iova = IOVA::new(0xFFFF_FFFF_FFFF_FFFF).unwrap();

    // Mask to get specific bits
    let masked = iova.mask(0xF0F0_F0F0_F0F0_F0F0);
    assert_eq!(masked.as_u64(), 0xF0F0_F0F0_F0F0_F0F0);
}

#[test]
fn test_iova_mask_zero_returns_zero() {
    let iova = IOVA::new(0x1234_5678).unwrap();
    let masked = iova.mask(0);
    assert_eq!(masked.as_u64(), 0);
}

#[test]
fn test_iova_mask_all_ones_preserves_value() {
    let iova = IOVA::new(0xABCD_EF12_3456_7890).unwrap();
    let masked = iova.mask(u64::MAX);
    assert_eq!(masked.as_u64(), 0xABCD_EF12_3456_7890);
}

// ============================================================================
// Section 4: IOVA Tests - Alignment Operations
// ============================================================================

#[test]
fn test_iova_align_up_to_page_already_aligned() {
    let iova = IOVA::new(0x1000).unwrap();
    let aligned = iova.align_up_to_page();

    // Should be unchanged
    assert_eq!(aligned.as_u64(), 0x1000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_iova_align_up_to_page_unaligned_rounds_up() {
    let iova = IOVA::new(0x1001).unwrap();
    let aligned = iova.align_up_to_page();

    // Should round up to next page
    assert_eq!(aligned.as_u64(), 0x2000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_iova_align_up_boundary_cases() {
    // Test 0x0
    let iova = IOVA::new(0x0).unwrap();
    assert_eq!(iova.align_up_to_page().as_u64(), 0x0);

    // Test 0xFFF (one before page boundary)
    let iova = IOVA::new(0xFFF).unwrap();
    assert_eq!(iova.align_up_to_page().as_u64(), 0x1000);

    // Test 0x1001 (one after page boundary)
    let iova = IOVA::new(0x1001).unwrap();
    assert_eq!(iova.align_up_to_page().as_u64(), 0x2000);
}

#[test]
fn test_iova_align_down_to_page() {
    let iova = IOVA::new(0x1234).unwrap();
    let aligned = iova.align_down_to_page();

    assert_eq!(aligned.as_u64(), 0x1000);
    assert!(aligned.is_page_aligned());
}

// ============================================================================
// Section 5: IOVA Tests - Accessor Methods
// ============================================================================

#[test]
fn test_iova_raw_returns_value() {
    let iova = IOVA::new(0x1234_5678).unwrap();
    assert_eq!(iova.raw(), 0x1234_5678);
}

#[test]
fn test_iova_as_u64_alias() {
    let iova = IOVA::new(0x8765_4321).unwrap();
    assert_eq!(iova.as_u64(), iova.raw());
}

#[test]
fn test_iova_page_number_calculation() {
    // Address 0x5000 should be page 5 (0x5000 >> 12 = 5)
    let iova = IOVA::new(0x5000).unwrap();
    assert_eq!(iova.page_number(), 5);

    // Address 0x1234 should be page 1 (0x1234 >> 12 = 1)
    let iova = IOVA::new(0x1234).unwrap();
    assert_eq!(iova.page_number(), 1);

    // Address 0x0 should be page 0
    let iova = IOVA::new(0x0).unwrap();
    assert_eq!(iova.page_number(), 0);
}

#[test]
fn test_iova_page_offset_calculation() {
    // Address 0x1234 should have offset 0x234
    let iova = IOVA::new(0x1234).unwrap();
    assert_eq!(iova.page_offset(), 0x234);

    // Page-aligned address should have offset 0
    let iova = IOVA::new(0x1000).unwrap();
    assert_eq!(iova.page_offset(), 0);
}

#[test]
fn test_iova_page_offset_all_values_0_to_4095() {
    // Test all possible page offsets (0 to 0xFFF = 4095)
    for offset in 0..=0xFFF {
        let iova = IOVA::new(0x1000 + offset).unwrap();
        assert_eq!(iova.page_offset(), offset);
    }
}

// ============================================================================
// Section 6: IOVA Tests - Formatting
// ============================================================================

#[test]
fn test_iova_display_format() {
    let iova = IOVA::new(0x1234).unwrap();
    let formatted = format!("{iova}");

    // Display format should be hex with 0x prefix
    assert!(formatted.starts_with("0x"));
    assert!(formatted.contains("1234"));
}

#[test]
fn test_iova_debug_format() {
    let iova = IOVA::new(0x1234).unwrap();
    let formatted = format!("{iova:?}");

    // Debug format should include type name
    assert!(formatted.starts_with("IOVA("));
    assert!(formatted.contains("0x"));
    assert!(formatted.contains("1234"));
}

#[test]
fn test_iova_format_edge_cases() {
    // Test zero
    let iova = IOVA::new(0).unwrap();
    assert_eq!(format!("{iova}"), "0x0000_0000_0000_0000");

    // Test max value
    let iova = IOVA::new(u64::MAX).unwrap();
    assert_eq!(format!("{iova}"), "0xffff_ffff_ffff_ffff");
}

// ============================================================================
// Section 7: IOVA Tests - Const Constructors
// ============================================================================

#[test]
fn test_iova_const_new_in_const_context() {
    const ADDR: IOVA = IOVA::const_new(0x1000);
    assert_eq!(ADDR.as_u64(), 0x1000);
}

#[test]
fn test_iova_const_page_calculations() {
    const ADDR: IOVA = IOVA::const_new(0x5234);

    // Const operations should work at compile time
    const PAGE_NUM: u64 = ADDR.page_number();
    const PAGE_OFF: u64 = ADDR.page_offset();
    const IS_ALIGNED: bool = ADDR.is_page_aligned();

    assert_eq!(PAGE_NUM, 5);
    assert_eq!(PAGE_OFF, 0x234);
    assert!(!IS_ALIGNED);
}

// ============================================================================
// Section 8: IPA Tests - Overflow Handling
// ============================================================================

#[test]
fn test_ipa_checked_add_success() {
    let ipa = IPA::new(0x1000).unwrap();
    let result = ipa.checked_add(0x1000);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), 0x2000);
}

#[test]
fn test_ipa_checked_add_overflow_returns_none() {
    let ipa = IPA::new(u64::MAX - 100).unwrap();
    let result = ipa.checked_add(200);
    assert!(result.is_none());
}

#[test]
fn test_ipa_checked_add_at_52bit_boundary() {
    // 52-bit address boundary: 2^52 - 1 = 0xF_FFFF_FFFF_FFFF
    let max_52bit = (1u64 << 52) - 1;
    let ipa = IPA::new(max_52bit).unwrap();

    // Adding within 52-bit space should work
    let result = ipa.checked_add(0);
    assert!(result.is_some());

    // Adding beyond should overflow (in u64 terms)
    let result = ipa.checked_add(1);
    assert!(result.is_some()); // Still valid u64
}

#[test]
fn test_ipa_checked_add_max_value_overflow() {
    let ipa = IPA::new(u64::MAX).unwrap();
    let result = ipa.checked_add(1);
    assert!(result.is_none());
}

// ============================================================================
// Section 9: IPA Tests - Wrapping Operations
// ============================================================================

#[test]
fn test_ipa_add_offset_wraps_correctly() {
    let ipa = IPA::new(u64::MAX - 10).unwrap();
    let result = ipa.add_offset(20);
    assert_eq!(result.as_u64(), 9);
}

#[test]
fn test_ipa_add_offset_wraps_at_boundary() {
    let ipa = IPA::new(u64::MAX).unwrap();
    let result = ipa.add_offset(1);
    assert_eq!(result.as_u64(), 0);
}

// ============================================================================
// Section 10: IPA Tests - Mask Operations
// ============================================================================

#[test]
fn test_ipa_mask_page_alignment() {
    let ipa = IPA::new(0x2345).unwrap();
    let page_mask = 0xFFFF_FFFF_FFFF_F000;
    let masked = ipa.mask(page_mask);

    assert_eq!(masked.as_u64(), 0x2000);
    assert!(masked.is_page_aligned());
}

#[test]
fn test_ipa_mask_various_patterns() {
    let ipa = IPA::new(0xFFFF_FFFF_FFFF_FFFF).unwrap();
    let masked = ipa.mask(0x0F0F_0F0F_0F0F_0F0F);
    assert_eq!(masked.as_u64(), 0x0F0F_0F0F_0F0F_0F0F);
}

#[test]
fn test_ipa_mask_zero_returns_zero() {
    let ipa = IPA::new(0xAB_CDEF).unwrap();
    let masked = ipa.mask(0);
    assert_eq!(masked.as_u64(), 0);
}

// ============================================================================
// Section 11: IPA Tests - Alignment Operations
// ============================================================================

#[test]
fn test_ipa_align_up_to_page_already_aligned() {
    let ipa = IPA::new(0x3000).unwrap();
    let aligned = ipa.align_up_to_page();
    assert_eq!(aligned.as_u64(), 0x3000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_ipa_align_up_to_page_unaligned_rounds_up() {
    let ipa = IPA::new(0x3456).unwrap();
    let aligned = ipa.align_up_to_page();
    assert_eq!(aligned.as_u64(), 0x4000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_ipa_align_down_to_page() {
    let ipa = IPA::new(0x3ABC).unwrap();
    let aligned = ipa.align_down_to_page();
    assert_eq!(aligned.as_u64(), 0x3000);
    assert!(aligned.is_page_aligned());
}

// ============================================================================
// Section 12: IPA Tests - Accessor Methods
// ============================================================================

#[test]
fn test_ipa_raw_returns_value() {
    let ipa = IPA::new(0xFEDC_BA98).unwrap();
    assert_eq!(ipa.raw(), 0xFEDC_BA98);
}

#[test]
fn test_ipa_as_u64_alias() {
    let ipa = IPA::new(0x1122_3344).unwrap();
    assert_eq!(ipa.as_u64(), ipa.raw());
}

#[test]
fn test_ipa_page_number_calculation() {
    let ipa = IPA::new(0x7000).unwrap();
    assert_eq!(ipa.page_number(), 7);

    let ipa = IPA::new(0xABC).unwrap();
    assert_eq!(ipa.page_number(), 0);
}

#[test]
fn test_ipa_page_offset_calculation() {
    let ipa = IPA::new(0x5678).unwrap();
    assert_eq!(ipa.page_offset(), 0x678);

    let ipa = IPA::new(0x4000).unwrap();
    assert_eq!(ipa.page_offset(), 0);
}

// ============================================================================
// Section 13: IPA Tests - Formatting
// ============================================================================

#[test]
fn test_ipa_display_format() {
    let ipa = IPA::new(0x5678).unwrap();
    let formatted = format!("{ipa}");
    assert!(formatted.starts_with("0x"));
    assert!(formatted.contains("5678"));
}

#[test]
fn test_ipa_debug_format() {
    let ipa = IPA::new(0x5678).unwrap();
    let formatted = format!("{ipa:?}");
    assert!(formatted.starts_with("IPA("));
    assert!(formatted.contains("0x"));
}

#[test]
fn test_ipa_format_edge_cases() {
    let ipa = IPA::new(0).unwrap();
    assert_eq!(format!("{ipa}"), "0x0000_0000_0000_0000");

    let ipa = IPA::new(u64::MAX).unwrap();
    assert_eq!(format!("{ipa}"), "0xffff_ffff_ffff_ffff");
}

// ============================================================================
// Section 14: IPA Tests - Const Constructors
// ============================================================================

#[test]
fn test_ipa_const_new_in_const_context() {
    const ADDR: IPA = IPA::const_new(0x2000);
    assert_eq!(ADDR.as_u64(), 0x2000);

    // Also test runtime usage to ensure the function body is covered
    let runtime_addr = IPA::const_new(0x2000);
    assert_eq!(runtime_addr.as_u64(), 0x2000);
}

#[test]
fn test_ipa_const_page_calculations() {
    const ADDR: IPA = IPA::const_new(0x3ABC);
    const PAGE_NUM: u64 = ADDR.page_number();
    const PAGE_OFF: u64 = ADDR.page_offset();

    assert_eq!(PAGE_NUM, 3);
    assert_eq!(PAGE_OFF, 0xABC);

    // Also verify runtime behavior
    let runtime_addr = IPA::const_new(0x3ABC);
    assert_eq!(runtime_addr.page_number(), PAGE_NUM);
    assert_eq!(runtime_addr.page_offset(), PAGE_OFF);
}

// ============================================================================
// Section 15: PA Tests - Overflow Handling
// ============================================================================

#[test]
fn test_pa_checked_add_success() {
    let pa = PA::new(0x1000).unwrap();
    let result = pa.checked_add(0x1000);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), 0x2000);
}

#[test]
fn test_pa_checked_add_overflow_returns_none() {
    let pa = PA::new(u64::MAX - 50).unwrap();
    let result = pa.checked_add(100);
    assert!(result.is_none());
}

#[test]
fn test_pa_checked_add_at_52bit_boundary() {
    let max_52bit = (1u64 << 52) - 1;
    let pa = PA::new(max_52bit).unwrap();
    let result = pa.checked_add(0);
    assert!(result.is_some());
}

#[test]
fn test_pa_checked_add_max_value_overflow() {
    let pa = PA::new(u64::MAX).unwrap();
    let result = pa.checked_add(1);
    assert!(result.is_none());
}

#[test]
fn test_pa_checked_add_at_max_minus_one() {
    let pa = PA::new(u64::MAX - 1).unwrap();

    // Adding 1 should succeed
    let result = pa.checked_add(1);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), u64::MAX);

    // Adding 2 should overflow
    let result = pa.checked_add(2);
    assert!(result.is_none());
}

// ============================================================================
// Section 16: PA Tests - Wrapping Operations
// ============================================================================

#[test]
fn test_pa_add_offset_wraps_correctly() {
    let pa = PA::new(u64::MAX - 5).unwrap();
    let result = pa.add_offset(10);
    assert_eq!(result.as_u64(), 4);
}

#[test]
fn test_pa_add_offset_no_overflow() {
    let pa = PA::new(0x8000).unwrap();
    let result = pa.add_offset(0x1000);
    assert_eq!(result.as_u64(), 0x9000);
}

#[test]
fn test_pa_add_offset_wraps_at_boundary() {
    let pa = PA::new(u64::MAX).unwrap();
    let result = pa.add_offset(1);
    assert_eq!(result.as_u64(), 0);
}

#[test]
fn test_pa_add_offset_preserves_page_offset() {
    let pa = PA::new(0x8ABC).unwrap();
    let offset = pa.page_offset();
    let result = pa.add_offset(PAGE_SIZE);
    assert_eq!(result.page_offset(), offset);
}

// ============================================================================
// Section 17: PA Tests - Mask Operations
// ============================================================================

#[test]
fn test_pa_mask_page_alignment() {
    let pa = PA::new(0x9876).unwrap();
    let page_mask = 0xFFFF_FFFF_FFFF_F000;
    let masked = pa.mask(page_mask);

    assert_eq!(masked.as_u64(), 0x9000);
    assert!(masked.is_page_aligned());
}

#[test]
fn test_pa_mask_various_patterns() {
    let pa = PA::new(0xAAAA_AAAA_AAAA_AAAA).unwrap();
    let masked = pa.mask(0x5555_5555_5555_5555);
    assert_eq!(masked.as_u64(), 0);
}

#[test]
fn test_pa_mask_zero_returns_zero() {
    let pa = PA::new(0x12_3456).unwrap();
    let masked = pa.mask(0);
    assert_eq!(masked.as_u64(), 0);
}

#[test]
fn test_pa_mask_all_ones_preserves_value() {
    let pa = PA::new(0x9876_5432_10AB_CDEF).unwrap();
    let masked = pa.mask(u64::MAX);
    assert_eq!(masked.as_u64(), 0x9876_5432_10AB_CDEF);
}

// ============================================================================
// Section 18: PA Tests - Alignment Operations
// ============================================================================

#[test]
fn test_pa_align_up_to_page_already_aligned() {
    let pa = PA::new(0x8000).unwrap();
    let aligned = pa.align_up_to_page();
    assert_eq!(aligned.as_u64(), 0x8000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_pa_align_up_to_page_unaligned_rounds_up() {
    let pa = PA::new(0x8001).unwrap();
    let aligned = pa.align_up_to_page();
    assert_eq!(aligned.as_u64(), 0x9000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_pa_align_up_boundary_cases() {
    // Test 0x0
    let pa = PA::new(0x0).unwrap();
    assert_eq!(pa.align_up_to_page().as_u64(), 0x0);

    // Test 0xFFF
    let pa = PA::new(0xFFF).unwrap();
    assert_eq!(pa.align_up_to_page().as_u64(), 0x1000);

    // Test 0x1001
    let pa = PA::new(0x1001).unwrap();
    assert_eq!(pa.align_up_to_page().as_u64(), 0x2000);
}

#[test]
fn test_pa_align_down_to_page() {
    let pa = PA::new(0x8DEF).unwrap();
    let aligned = pa.align_down_to_page();
    assert_eq!(aligned.as_u64(), 0x8000);
    assert!(aligned.is_page_aligned());
}

// ============================================================================
// Section 19: PA Tests - Accessor Methods
// ============================================================================

#[test]
fn test_pa_raw_returns_value() {
    let pa = PA::new(0xDEAD_BEEF).unwrap();
    assert_eq!(pa.raw(), 0xDEAD_BEEF);
}

#[test]
fn test_pa_as_u64_alias() {
    let pa = PA::new(0xCAFE_BABE).unwrap();
    assert_eq!(pa.as_u64(), pa.raw());
}

#[test]
fn test_pa_page_number_calculation() {
    let pa = PA::new(0xA000).unwrap();
    assert_eq!(pa.page_number(), 10);

    let pa = PA::new(0xDEF).unwrap();
    assert_eq!(pa.page_number(), 0);
}

#[test]
fn test_pa_page_offset_calculation() {
    let pa = PA::new(0x9ABC).unwrap();
    assert_eq!(pa.page_offset(), 0xABC);

    let pa = PA::new(0x5000).unwrap();
    assert_eq!(pa.page_offset(), 0);
}

#[test]
fn test_pa_page_offset_all_values_0_to_4095() {
    for offset in 0..=0xFFF {
        let pa = PA::new(0x8000 + offset).unwrap();
        assert_eq!(pa.page_offset(), offset);
    }
}

// ============================================================================
// Section 20: PA Tests - Formatting
// ============================================================================

#[test]
fn test_pa_display_format() {
    let pa = PA::new(0x9ABC).unwrap();
    let formatted = format!("{pa}");
    assert!(formatted.starts_with("0x"));
    assert!(formatted.contains("9abc"));
}

#[test]
fn test_pa_debug_format() {
    let pa = PA::new(0x9ABC).unwrap();
    let formatted = format!("{pa:?}");
    assert!(formatted.starts_with("PA("));
    assert!(formatted.contains("0x"));
    assert!(formatted.contains("9abc"));
}

#[test]
fn test_pa_format_edge_cases() {
    let pa = PA::new(0).unwrap();
    assert_eq!(format!("{pa}"), "0x0000_0000_0000_0000");

    let pa = PA::new(u64::MAX).unwrap();
    assert_eq!(format!("{pa}"), "0xffff_ffff_ffff_ffff");
}

// ============================================================================
// Section 21: PA Tests - Const Constructors
// ============================================================================

#[test]
fn test_pa_const_new_in_const_context() {
    const ADDR: PA = PA::const_new(0x8000);
    assert_eq!(ADDR.as_u64(), 0x8000);
}

#[test]
fn test_pa_const_page_calculations() {
    const ADDR: PA = PA::const_new(0x9DEF);
    const PAGE_NUM: u64 = ADDR.page_number();
    const PAGE_OFF: u64 = ADDR.page_offset();
    const IS_ALIGNED: bool = ADDR.is_page_aligned();

    assert_eq!(PAGE_NUM, 9);
    assert_eq!(PAGE_OFF, 0xDEF);
    assert!(!IS_ALIGNED);
}

// ============================================================================
// Section 22: Cross-type Consistency Tests
// ============================================================================

#[test]
fn test_all_types_have_identical_behavior() {
    let addr_val = 0x1234u64;

    let iova = IOVA::new(addr_val).unwrap();
    let ipa = IPA::new(addr_val).unwrap();
    let pa = PA::new(addr_val).unwrap();

    // All should have same raw value
    assert_eq!(iova.as_u64(), ipa.as_u64());
    assert_eq!(ipa.as_u64(), pa.as_u64());

    // All should have same page calculations
    assert_eq!(iova.page_number(), ipa.page_number());
    assert_eq!(ipa.page_number(), pa.page_number());

    assert_eq!(iova.page_offset(), ipa.page_offset());
    assert_eq!(ipa.page_offset(), pa.page_offset());
}

#[test]
fn test_new_page_aligned_success() {
    // All types should accept page-aligned addresses
    assert!(IOVA::new_page_aligned(0x1000).is_ok());
    assert!(IPA::new_page_aligned(0x2000).is_ok());
    assert!(PA::new_page_aligned(0x3000).is_ok());
}

#[test]
fn test_new_page_aligned_failure() {
    // All types should reject non-page-aligned addresses
    assert!(IOVA::new_page_aligned(0x1001).is_err());
    assert!(IPA::new_page_aligned(0x2345).is_err());
    assert!(PA::new_page_aligned(0x3FFF).is_err());
}

#[test]
fn test_is_page_aligned_consistency() {
    for addr in [0x0, 0x1000, 0x2000, 0xFFF, 0x1001, 0x1FFF] {
        let iova = IOVA::new(addr).unwrap();
        let ipa = IPA::new(addr).unwrap();
        let pa = PA::new(addr).unwrap();

        let expected = (addr & 0xFFF) == 0;
        assert_eq!(iova.is_page_aligned(), expected);
        assert_eq!(ipa.is_page_aligned(), expected);
        assert_eq!(pa.is_page_aligned(), expected);
    }
}

// ============================================================================
// Section 23: Edge Cases and Boundary Conditions
// ============================================================================

#[test]
fn test_zero_address_all_types() {
    let iova = IOVA::new(0).unwrap();
    let ipa = IPA::new(0).unwrap();
    let pa = PA::new(0).unwrap();

    assert_eq!(iova.as_u64(), 0);
    assert_eq!(ipa.as_u64(), 0);
    assert_eq!(pa.as_u64(), 0);

    assert!(iova.is_page_aligned());
    assert!(ipa.is_page_aligned());
    assert!(pa.is_page_aligned());

    assert_eq!(iova.page_number(), 0);
    assert_eq!(ipa.page_number(), 0);
    assert_eq!(pa.page_number(), 0);
}

#[test]
fn test_max_address_all_types() {
    let iova = IOVA::new(u64::MAX).unwrap();
    let ipa = IPA::new(u64::MAX).unwrap();
    let pa = PA::new(u64::MAX).unwrap();

    assert_eq!(iova.as_u64(), u64::MAX);
    assert_eq!(ipa.as_u64(), u64::MAX);
    assert_eq!(pa.as_u64(), u64::MAX);

    assert!(!iova.is_page_aligned());
    assert!(!ipa.is_page_aligned());
    assert!(!pa.is_page_aligned());
}

#[test]
fn test_page_size_constant() {
    assert_eq!(PAGE_SIZE, 0x1000);
    assert_eq!(PAGE_SIZE, 4096);
}

// ============================================================================
// Section 24: Additional Coverage Tests
// ============================================================================

#[test]
fn test_checked_add_with_zero_all_types() {
    let iova = IOVA::new(0x1234).unwrap();
    assert_eq!(iova.checked_add(0).unwrap().as_u64(), 0x1234);

    let ipa = IPA::new(0x5678).unwrap();
    assert_eq!(ipa.checked_add(0).unwrap().as_u64(), 0x5678);

    let pa = PA::new(0x9ABC).unwrap();
    assert_eq!(pa.checked_add(0).unwrap().as_u64(), 0x9ABC);
}

#[test]
fn test_add_offset_with_zero_all_types() {
    let iova = IOVA::new(0x1234).unwrap();
    assert_eq!(iova.add_offset(0).as_u64(), 0x1234);

    let ipa = IPA::new(0x5678).unwrap();
    assert_eq!(ipa.add_offset(0).as_u64(), 0x5678);

    let pa = PA::new(0x9ABC).unwrap();
    assert_eq!(pa.add_offset(0).as_u64(), 0x9ABC);
}

#[test]
fn test_alignment_operations_comprehensive() {
    // Test a range of addresses for all alignment operations
    let test_values = [0x0, 0x1, 0xFFF, 0x1000, 0x1001, 0x1FFF, 0x2000, 0x1_0000, 0xF_FFFF, 0x10_0000];

    for &addr in &test_values {
        let iova = IOVA::new(addr).unwrap();
        let ipa = IPA::new(addr).unwrap();
        let pa = PA::new(addr).unwrap();

        // Verify align_down consistency
        let expected_down = addr & !0xFFF;
        assert_eq!(iova.align_down_to_page().as_u64(), expected_down);
        assert_eq!(ipa.align_down_to_page().as_u64(), expected_down);
        assert_eq!(pa.align_down_to_page().as_u64(), expected_down);

        // Verify align_up consistency
        let expected_up = (addr + 0xFFF) & !0xFFF;
        assert_eq!(iova.align_up_to_page().as_u64(), expected_up);
        assert_eq!(ipa.align_up_to_page().as_u64(), expected_up);
        assert_eq!(pa.align_up_to_page().as_u64(), expected_up);
    }
}
