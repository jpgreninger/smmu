//! Custom assertion helpers for SMMU testing
//!
//! Provides domain-specific assertions that make test failures more informative
//! and test code more readable.

/// Assert that two addresses are aligned to the same boundary
///
/// # Panics
///
/// Panics if addresses are not aligned to the specified boundary
pub fn assert_aligned(addr: u64, alignment: u64) {
    assert!(
        alignment.is_power_of_two(),
        "Alignment must be power of 2, got {alignment}"
    );
    assert_eq!(
        addr & (alignment - 1),
        0,
        "Address 0x{addr:X} is not aligned to 0x{alignment:X}"
    );
}

/// Assert that an address is page-aligned
///
/// # Panics
///
/// Panics if address is not page-aligned (4KB)
pub fn assert_page_aligned(addr: u64) {
    assert_aligned(addr, 4096);
}

/// Assert that a value is within an expected range
///
/// # Panics
///
/// Panics if value is outside the range [min, max]
pub fn assert_in_range<T: PartialOrd + std::fmt::Debug>(value: T, min: T, max: T) {
    assert!(
        value >= min && value <= max,
        "Value {value:?} is not in range [{min:?}, {max:?}]"
    );
}

/// Assert that a physical address is valid (within addressable range)
///
/// # Panics
///
/// Panics if address exceeds maximum physical address
pub fn assert_valid_pa(pa: u64, max_pa_bits: u8) {
    let max_pa = (1u64 << max_pa_bits) - 1;
    assert!(
        pa <= max_pa,
        "Physical address 0x{pa:X} exceeds maximum for {max_pa_bits} bits (0x{max_pa:X})"
    );
}

/// Assert that a virtual address is canonical (for 48-bit VA)
///
/// # Panics
///
/// Panics if address is not canonical
pub fn assert_canonical_va(va: u64) {
    // For 48-bit addresses, bits [63:48] must be all 0s or all 1s
    let top_bits = va >> 48;
    assert!(
        top_bits == 0 || top_bits == 0xFFFF,
        "Virtual address 0x{va:X} is not canonical (top bits: 0x{top_bits:X})"
    );
}

/// Assert that a stream ID is valid
///
/// # Panics
///
/// Panics if stream ID exceeds maximum
pub fn assert_valid_stream_id(stream_id: u32, max_streams: u32) {
    assert!(
        stream_id < max_streams,
        "Stream ID {stream_id} exceeds maximum {max_streams}"
    );
}

/// Assert that a PASID is valid (20 bits max per ARM SMMU v3 spec)
///
/// # Panics
///
/// Panics if PASID exceeds maximum
pub fn assert_valid_pasid(pasid: u32) {
    const MAX_PASID: u32 = (1 << 20) - 1;
    assert!(
        pasid <= MAX_PASID,
        "PASID {pasid} exceeds maximum {MAX_PASID}"
    );
}

/// Assert that a translation result represents a successful translation
///
/// This is a placeholder that will be implemented once TranslationResult is defined
pub fn assert_translation_success(_result: &str) {
    // TODO: Implement once TranslationResult type is available
}

/// Assert that a translation result represents a fault
///
/// This is a placeholder that will be implemented once TranslationResult is defined
pub fn assert_translation_fault(_result: &str) {
    // TODO: Implement once TranslationResult type is available
}

/// Assert that performance meets target (for benchmark validation)
///
/// # Panics
///
/// Panics if actual duration exceeds target by more than tolerance
pub fn assert_performance(
    actual_ns: u64,
    target_ns: u64,
    tolerance_percent: f64,
) {
    let tolerance = (target_ns as f64 * tolerance_percent / 100.0) as u64;
    let max_allowed = target_ns + tolerance;

    assert!(
        actual_ns <= max_allowed,
        "Performance target missed: {actual_ns}ns > {target_ns}ns (tolerance: {tolerance_percent}%)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_aligned_success() {
        assert_aligned(0x1000, 4096);
        assert_aligned(0x2000, 4096);
        assert_aligned(0x0, 4096);
    }

    #[test]
    #[should_panic(expected = "is not aligned")]
    fn test_assert_aligned_failure() {
        assert_aligned(0x1001, 4096);
    }

    #[test]
    fn test_assert_page_aligned_success() {
        assert_page_aligned(0x1000);
        assert_page_aligned(0x1_0000);
    }

    #[test]
    #[should_panic]
    fn test_assert_page_aligned_failure() {
        assert_page_aligned(0x1001);
    }

    #[test]
    fn test_assert_in_range_success() {
        assert_in_range(5, 0, 10);
        assert_in_range(0, 0, 10);
        assert_in_range(10, 0, 10);
    }

    #[test]
    #[should_panic(expected = "is not in range")]
    fn test_assert_in_range_failure() {
        assert_in_range(11, 0, 10);
    }

    #[test]
    fn test_assert_valid_pa_success() {
        assert_valid_pa(0x1000, 48);
        assert_valid_pa(0xFFFF_FFFF_FFFF, 48);
    }

    #[test]
    #[should_panic(expected = "exceeds maximum")]
    fn test_assert_valid_pa_failure() {
        assert_valid_pa(0x1_0000_0000_0000, 48);
    }

    #[test]
    fn test_assert_canonical_va_success() {
        assert_canonical_va(0x0000_0000_0000_1000);
        assert_canonical_va(0xFFFF_FFFF_FFFF_F000);
    }

    #[test]
    #[should_panic(expected = "is not canonical")]
    fn test_assert_canonical_va_failure() {
        assert_canonical_va(0x0001_0000_0000_0000);
    }

    #[test]
    fn test_assert_valid_stream_id_success() {
        assert_valid_stream_id(0, 256);
        assert_valid_stream_id(255, 256);
    }

    #[test]
    #[should_panic(expected = "exceeds maximum")]
    fn test_assert_valid_stream_id_failure() {
        assert_valid_stream_id(256, 256);
    }

    #[test]
    fn test_assert_valid_pasid_success() {
        assert_valid_pasid(0);
        assert_valid_pasid((1 << 20) - 1);
    }

    #[test]
    #[should_panic(expected = "exceeds maximum")]
    fn test_assert_valid_pasid_failure() {
        assert_valid_pasid(1 << 20);
    }

    #[test]
    fn test_assert_performance_success() {
        assert_performance(100, 135, 10.0); // Within target
        assert_performance(135, 135, 10.0); // Exactly at target
        assert_performance(140, 135, 10.0); // Slightly over but within tolerance
    }

    #[test]
    #[should_panic(expected = "Performance target missed")]
    fn test_assert_performance_failure() {
        assert_performance(200, 135, 10.0); // Way over target
    }
}
