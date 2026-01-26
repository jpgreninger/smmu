//! Comprehensive tests for address types (IOVA, IPA, PA)
//!
//! Tests cover:
//! - Address creation with validation
//! - Alignment checks and enforcement
//! - Bitwise operations (masking, alignment, offset calculation)
//! - Const fn constructors for zero-cost abstractions
//! - Page-aligned address operations
//! - Address range validation
//! - Edge cases (zero, max, alignment boundaries)

use smmu::{IOVA, IPA, PA, ValidationError};

// ============================================================================
// IOVA (Input/Output Virtual Address) Tests
// ============================================================================

#[test]
fn test_iova_new_valid() {
    // Valid IOVA addresses
    assert!(IOVA::new(0x0000_0000_0000_0000).is_ok());
    assert!(IOVA::new(0x1000).is_ok());
    assert!(IOVA::new(0x0000_FFFF_FFFF_F000).is_ok());
}

#[test]
fn test_iova_new_from_u64() {
    let addr = 0x1234_5678_9ABC_0000u64;
    let iova = IOVA::new(addr).unwrap();
    assert_eq!(iova.as_u64(), addr);
}

#[test]
fn test_iova_const_new() {
    // Test const fn constructor for compile-time creation
    const IOVA_CONST: IOVA = IOVA::const_new(0x1000);
    assert_eq!(IOVA_CONST.as_u64(), 0x1000);
}

#[test]
fn test_iova_page_aligned() {
    // Test page-aligned address creation (4KB pages)
    let iova = IOVA::new_page_aligned(0x1234_5000).unwrap();
    assert_eq!(iova.as_u64(), 0x1234_5000);
    assert!(iova.is_page_aligned());
}

#[test]
fn test_iova_not_page_aligned() {
    // Non-page-aligned address should fail
    let result = IOVA::new_page_aligned(0x1234_5001);
    assert!(result.is_err());
    assert!(matches!(result, Err(ValidationError::InvalidAlignment { .. })));
}

#[test]
fn test_iova_alignment_check() {
    let aligned = IOVA::new(0x1000).unwrap();
    assert!(aligned.is_page_aligned());

    let unaligned = IOVA::new(0x1001).unwrap();
    assert!(!unaligned.is_page_aligned());
}

#[test]
fn test_iova_align_down() {
    let addr = IOVA::new(0x1234_5678).unwrap();
    let aligned = addr.align_down_to_page();
    assert_eq!(aligned.as_u64(), 0x1234_5000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_iova_align_up() {
    let addr = IOVA::new(0x1234_5678).unwrap();
    let aligned = addr.align_up_to_page();
    assert_eq!(aligned.as_u64(), 0x1234_6000);
    assert!(aligned.is_page_aligned());
}

#[test]
fn test_iova_page_offset() {
    let addr = IOVA::new(0x1234_5678).unwrap();
    assert_eq!(addr.page_offset(), 0x678);
}

#[test]
fn test_iova_page_number() {
    let addr = IOVA::new(0x1234_5000).unwrap();
    assert_eq!(addr.page_number(), 0x1234_5);
}

#[test]
fn test_iova_add_offset() {
    let addr = IOVA::new(0x1000).unwrap();
    let new_addr = addr.add_offset(0x500);
    assert_eq!(new_addr.as_u64(), 0x1500);
}

#[test]
fn test_iova_add_offset_overflow() {
    let addr = IOVA::new(0xFFFF_FFFF_FFFF_F000).unwrap();
    let new_addr = addr.add_offset(0x2000);
    // Should wrap or saturate
    assert!(new_addr.as_u64() <= 0x1000 || new_addr.as_u64() == u64::MAX);
}

#[test]
fn test_iova_checked_add() {
    let addr = IOVA::new(0x1000).unwrap();
    let result = addr.checked_add(0x500);
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_u64(), 0x1500);
}

#[test]
fn test_iova_checked_add_overflow() {
    let addr = IOVA::new(0xFFFF_FFFF_FFFF_F000).unwrap();
    let result = addr.checked_add(0x2000);
    assert!(result.is_none()); // Overflow detected
}

#[test]
fn test_iova_mask_operations() {
    let addr = IOVA::new(0x1234_5678).unwrap();

    // Mask off lower bits
    let masked = addr.mask(0xFFFF_FFFF_FFFF_F000);
    assert_eq!(masked.as_u64(), 0x1234_5000);

    // Extract page offset
    let offset = addr.mask(0xFFF);
    assert_eq!(offset.as_u64(), 0x678);
}

#[test]
fn test_iova_display() {
    let addr = IOVA::new(0x1234_5678_9ABC_DEF0).unwrap();
    let display = format!("{}", addr);
    assert!(display.contains("1234567") || display.contains("0x"));
}

#[test]
fn test_iova_debug() {
    let addr = IOVA::new(0x1000).unwrap();
    let debug = format!("{:?}", addr);
    assert!(debug.contains("IOVA") || debug.contains("0x1000"));
}

#[test]
fn test_iova_copy_clone() {
    let addr1 = IOVA::new(0x1000).unwrap();
    let addr2 = addr1; // Copy
    let addr3 = addr1.clone(); // Clone

    assert_eq!(addr1.as_u64(), addr2.as_u64());
    assert_eq!(addr1.as_u64(), addr3.as_u64());
}

#[test]
fn test_iova_equality() {
    let addr1 = IOVA::new(0x1000).unwrap();
    let addr2 = IOVA::new(0x1000).unwrap();
    let addr3 = IOVA::new(0x2000).unwrap();

    assert_eq!(addr1, addr2);
    assert_ne!(addr1, addr3);
}

#[test]
fn test_iova_ordering() {
    let addr1 = IOVA::new(0x1000).unwrap();
    let addr2 = IOVA::new(0x2000).unwrap();

    assert!(addr1 < addr2);
    assert!(addr2 > addr1);
}

#[test]
fn test_iova_zero() {
    let zero = IOVA::new(0).unwrap();
    assert_eq!(zero.as_u64(), 0);
    assert!(zero.is_page_aligned());
}

#[test]
fn test_iova_max_value() {
    let max = IOVA::new(u64::MAX).unwrap();
    assert_eq!(max.as_u64(), u64::MAX);
}

// ============================================================================
// IPA (Intermediate Physical Address) Tests
// ============================================================================

#[test]
fn test_ipa_new_valid() {
    assert!(IPA::new(0x0000_0000_0000_0000).is_ok());
    assert!(IPA::new(0x1000).is_ok());
    assert!(IPA::new(0x0000_FFFF_FFFF_F000).is_ok());
}

#[test]
fn test_ipa_page_aligned() {
    let ipa = IPA::new_page_aligned(0x2000).unwrap();
    assert_eq!(ipa.as_u64(), 0x2000);
    assert!(ipa.is_page_aligned());
}

#[test]
fn test_ipa_not_page_aligned() {
    let result = IPA::new_page_aligned(0x2001);
    assert!(result.is_err());
}

#[test]
fn test_ipa_alignment_operations() {
    let addr = IPA::new(0xABCD_1234).unwrap();

    let down = addr.align_down_to_page();
    assert_eq!(down.as_u64(), 0xABCD_1000);

    let up = addr.align_up_to_page();
    assert_eq!(up.as_u64(), 0xABCD_2000);
}

#[test]
fn test_ipa_const_new() {
    const IPA_CONST: IPA = IPA::const_new(0x5000);
    assert_eq!(IPA_CONST.as_u64(), 0x5000);
}

#[test]
fn test_ipa_copy_clone() {
    let ipa1 = IPA::new(0x3000).unwrap();
    let ipa2 = ipa1;
    let ipa3 = ipa1.clone();

    assert_eq!(ipa1.as_u64(), ipa2.as_u64());
    assert_eq!(ipa1.as_u64(), ipa3.as_u64());
}

// ============================================================================
// PA (Physical Address) Tests
// ============================================================================

#[test]
fn test_pa_new_valid() {
    assert!(PA::new(0x0000_0000_0000_0000).is_ok());
    assert!(PA::new(0x1000).is_ok());
    assert!(PA::new(0x0000_FFFF_FFFF_F000).is_ok());
}

#[test]
fn test_pa_page_aligned() {
    let pa = PA::new_page_aligned(0x8000).unwrap();
    assert_eq!(pa.as_u64(), 0x8000);
    assert!(pa.is_page_aligned());
}

#[test]
fn test_pa_not_page_aligned() {
    let result = PA::new_page_aligned(0x8001);
    assert!(result.is_err());
}

#[test]
fn test_pa_alignment_operations() {
    let addr = PA::new(0x9876_5432).unwrap();

    let down = addr.align_down_to_page();
    assert_eq!(down.as_u64(), 0x9876_5000);

    let up = addr.align_up_to_page();
    assert_eq!(up.as_u64(), 0x9876_6000);
}

#[test]
fn test_pa_const_new() {
    const PA_CONST: PA = PA::const_new(0xA000);
    assert_eq!(PA_CONST.as_u64(), 0xA000);
}

#[test]
fn test_pa_copy_clone() {
    let pa1 = PA::new(0x7000).unwrap();
    let pa2 = pa1;
    let pa3 = pa1.clone();

    assert_eq!(pa1.as_u64(), pa2.as_u64());
    assert_eq!(pa1.as_u64(), pa3.as_u64());
}

// ============================================================================
// Address Type Distinction Tests
// ============================================================================

#[test]
fn test_address_types_are_distinct() {
    // Ensure IOVA, IPA, and PA are distinct types (won't compile if not)
    let iova = IOVA::new(0x1000).unwrap();
    let ipa = IPA::new(0x1000).unwrap();
    let pa = PA::new(0x1000).unwrap();

    // These should all have the same value but be different types
    assert_eq!(iova.as_u64(), 0x1000);
    assert_eq!(ipa.as_u64(), 0x1000);
    assert_eq!(pa.as_u64(), 0x1000);

    // Type safety: can't compare across types (won't compile)
    // assert_eq!(iova, ipa); // Compilation error - good!
}

// ============================================================================
// Performance Tests - Zero-Cost Abstractions
// ============================================================================

#[test]
fn test_address_conversion_overhead() {
    // Verify conversions are zero-cost
    const ITERATIONS: usize = 10_000;

    let start = std::time::Instant::now();
    for i in 0..ITERATIONS {
        let addr = IOVA::new((i as u64) * 0x1000).unwrap();
        let _value = addr.as_u64();
    }
    let duration = start.elapsed();

    // Should be extremely fast (< 1ms for 10k iterations)
    assert!(duration.as_millis() < 10, "Address operations too slow: {:?}", duration);
}

#[test]
fn test_const_evaluation() {
    // Test that const fn works at compile time
    const _ADDR1: IOVA = IOVA::const_new(0x1000);
    const _ADDR2: IPA = IPA::const_new(0x2000);
    const _ADDR3: PA = PA::const_new(0x3000);

    // If this compiles, const evaluation works
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_address_alignment_boundary() {
    // Test addresses at alignment boundaries
    let addrs = [
        0x0FFF, // Just before page boundary
        0x1000, // Exact page boundary
        0x1001, // Just after page boundary
    ];

    for &addr in &addrs {
        let iova = IOVA::new(addr).unwrap();
        if addr % 0x1000 == 0 {
            assert!(iova.is_page_aligned());
        } else {
            assert!(!iova.is_page_aligned());
        }
    }
}

#[test]
fn test_large_page_alignment() {
    // Test 2MB (0x20_0000) alignment
    let addr = IOVA::new(0x40_0000).unwrap();
    assert_eq!(addr.as_u64() & 0x1F_FFFF, 0); // 2MB aligned
}

#[test]
fn test_address_page_size_constant() {
    // Verify page size constant is available
    use smmu::PAGE_SIZE;
    assert_eq!(PAGE_SIZE, 0x1000); // 4KB
}
