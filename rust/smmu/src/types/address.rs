//! Address types for ARM `SMMU` v3
//!
//! This module provides type-safe wrappers for different address types used in
//! ARM `SMMU` v3 translation:
//!
//! - [`IOVA`] - Input/Output Virtual Address (input to Stage 1)
//! - [`IPA`] - Intermediate Physical Address (output from Stage 1, input to Stage 2)
//! - [`PA`] - Physical Address (final output from translation)
//!
//! # Type Safety
//!
//! Each address type is a distinct newtype wrapper to prevent mixing address types.
//! The compiler will catch errors like passing an `IPA` where an `IOVA` is expected.
//!
//! # Zero-Cost Abstractions
//!
//! All address types use `const fn` constructors and are marked with `#[repr(transparent)]`
//! to ensure zero runtime overhead compared to raw `u64` values.
//!
//! # ARM `SMMU` v3 Compliance
//!
//! Addresses support ARM `SMMU` v3 requirements including:
//! - 4KB page alignment (0x1000)
//! - Page offset extraction
//! - Address range validation
//! - Bitwise operations for descriptor manipulation

use crate::types::ValidationError;
use core::fmt;

/// Page size constant - 4KB pages
pub const PAGE_SIZE: u64 = 0x1000;

/// Page offset mask - lower 12 bits
const PAGE_OFFSET_MASK: u64 = PAGE_SIZE - 1;

/// Page number shift - 12 bits
const PAGE_SHIFT: u32 = 12;

// ============================================================================
// IOVA - Input/Output Virtual Address
// ============================================================================

/// Input/Output Virtual Address (`IOVA`)
///
/// `IOVA` is the input address to Stage 1 translation in ARM `SMMU` v3.
/// It represents the virtual address from the device's perspective.
///
/// # Example
///
/// ```
/// use smmu::`IOVA`;
///
/// // Create a page-aligned `IOVA`
/// let iova = `IOVA`::new_page_aligned(0x1000).unwrap();
/// assert!(iova.is_page_aligned());
///
/// // Extract page offset
/// let addr = `IOVA`::new(0x1234).unwrap();
/// assert_eq!(addr.page_offset(), 0x234);
/// ```
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IOVA(u64);

impl IOVA {
    /// Create a new `IOVA` from a u64 address
    ///
    /// # Errors
    ///
    /// Currently always succeeds, but returns `Result` for API consistency.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::`IOVA`;
    /// let iova = `IOVA`::new(0x1000).unwrap();
    /// assert_eq!(iova.as_u64(), 0x1000);
    /// ```
    #[inline]
    pub const fn new(addr: u64) -> Result<Self, ValidationError> {
        Ok(Self(addr))
    }

    /// Create a new `IOVA` with const fn for compile-time construction
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::`IOVA`;
    /// const ADDR: `IOVA` = `IOVA`::const_new(0x1000);
    /// ```
    #[inline]
    #[must_use]
    pub const fn const_new(addr: u64) -> Self {
        Self(addr)
    }

    /// Create a page-aligned `IOVA`
    ///
    /// # Errors
    ///
    /// Returns an error if the address is not page-aligned.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::`IOVA`;
    /// assert!(`IOVA`::new_page_aligned(0x1000).is_ok());
    /// assert!(`IOVA`::new_page_aligned(0x1001).is_err());
    /// ```
    #[inline]
    pub const fn new_page_aligned(addr: u64) -> Result<Self, ValidationError> {
        if addr & PAGE_OFFSET_MASK != 0 {
            return Err(ValidationError::InvalidAlignment {
                address: addr,
                required_alignment: PAGE_SIZE,
            });
        }
        Ok(Self(addr))
    }

    /// Get the raw u64 address value
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Alias for `as_u64()` - gets the raw u64 address value
    #[inline]
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Check if the address is page-aligned
    #[inline]
    #[must_use]
    pub const fn is_page_aligned(self) -> bool {
        self.0 & PAGE_OFFSET_MASK == 0
    }

    /// Align the address down to the nearest page boundary
    #[inline]
    #[must_use]
    pub const fn align_down_to_page(self) -> Self {
        Self(self.0 & !PAGE_OFFSET_MASK)
    }

    /// Align the address up to the next page boundary
    #[inline]
    #[must_use]
    pub const fn align_up_to_page(self) -> Self {
        Self((self.0 + PAGE_OFFSET_MASK) & !PAGE_OFFSET_MASK)
    }

    /// Get the page offset (lower 12 bits)
    #[inline]
    #[must_use]
    pub const fn page_offset(self) -> u64 {
        self.0 & PAGE_OFFSET_MASK
    }

    /// Get the page number (upper bits, excluding offset)
    #[inline]
    #[must_use]
    pub const fn page_number(self) -> u64 {
        self.0 >> PAGE_SHIFT
    }

    /// Add an offset to the address (wrapping on overflow)
    #[inline]
    #[must_use]
    pub const fn add_offset(self, offset: u64) -> Self {
        Self(self.0.wrapping_add(offset))
    }

    /// Add an offset to the address, returning `None` on overflow
    #[inline]
    #[must_use]
    pub const fn checked_add(self, offset: u64) -> Option<Self> {
        match self.0.checked_add(offset) {
            Some(addr) => Some(Self(addr)),
            None => None,
        }
    }

    /// Apply a bitmask to the address
    #[inline]
    #[must_use]
    pub const fn mask(self, mask: u64) -> Self {
        Self(self.0 & mask)
    }
}

impl fmt::Debug for IOVA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IOVA({:#018x})", self.0)
    }
}

impl fmt::Display for IOVA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

// ============================================================================
// IPA - Intermediate Physical Address
// ============================================================================

/// Intermediate Physical Address (`IPA`)
///
/// `IPA` is the output address from Stage 1 translation and the input address
/// to Stage 2 translation in ARM `SMMU` v3. In two-stage translation, it
/// represents the guest physical address.
///
/// # Example
///
/// ```
/// use smmu::`IPA`;
///
/// let ipa = `IPA`::new_page_aligned(0x2000).unwrap();
/// assert!(ipa.is_page_aligned());
/// ```
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IPA(u64);

impl IPA {
    /// Create a new `IPA` from a u64 address
    ///
    /// # Errors
    ///
    /// Currently always succeeds, but returns `Result` for API consistency.
    #[inline]
    pub const fn new(addr: u64) -> Result<Self, ValidationError> {
        Ok(Self(addr))
    }

    /// Create a new `IPA` with const fn for compile-time construction
    #[inline]
    #[must_use]
    pub const fn const_new(addr: u64) -> Self {
        Self(addr)
    }

    /// Create a page-aligned `IPA`
    ///
    /// # Errors
    ///
    /// Returns an error if the address is not page-aligned.
    #[inline]
    pub const fn new_page_aligned(addr: u64) -> Result<Self, ValidationError> {
        if addr & PAGE_OFFSET_MASK != 0 {
            return Err(ValidationError::InvalidAlignment {
                address: addr,
                required_alignment: PAGE_SIZE,
            });
        }
        Ok(Self(addr))
    }

    /// Get the raw u64 address value
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Alias for `as_u64()` - gets the raw u64 address value
    #[inline]
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Check if the address is page-aligned
    #[inline]
    #[must_use]
    pub const fn is_page_aligned(self) -> bool {
        self.0 & PAGE_OFFSET_MASK == 0
    }

    /// Align the address down to the nearest page boundary
    #[inline]
    #[must_use]
    pub const fn align_down_to_page(self) -> Self {
        Self(self.0 & !PAGE_OFFSET_MASK)
    }

    /// Align the address up to the next page boundary
    #[inline]
    #[must_use]
    pub const fn align_up_to_page(self) -> Self {
        Self((self.0 + PAGE_OFFSET_MASK) & !PAGE_OFFSET_MASK)
    }

    /// Get the page offset (lower 12 bits)
    #[inline]
    #[must_use]
    pub const fn page_offset(self) -> u64 {
        self.0 & PAGE_OFFSET_MASK
    }

    /// Get the page number (upper bits, excluding offset)
    #[inline]
    #[must_use]
    pub const fn page_number(self) -> u64 {
        self.0 >> PAGE_SHIFT
    }

    /// Add an offset to the address (wrapping on overflow)
    #[inline]
    #[must_use]
    pub const fn add_offset(self, offset: u64) -> Self {
        Self(self.0.wrapping_add(offset))
    }

    /// Add an offset to the address, returning `None` on overflow
    #[inline]
    #[must_use]
    pub const fn checked_add(self, offset: u64) -> Option<Self> {
        match self.0.checked_add(offset) {
            Some(addr) => Some(Self(addr)),
            None => None,
        }
    }

    /// Apply a bitmask to the address
    #[inline]
    #[must_use]
    pub const fn mask(self, mask: u64) -> Self {
        Self(self.0 & mask)
    }
}

impl fmt::Debug for IPA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IPA({:#018x})", self.0)
    }
}

impl fmt::Display for IPA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

// ============================================================================
// PA - Physical Address
// ============================================================================

/// Physical Address (`PA`)
///
/// `PA` is the final output address from `SMMU` translation. It represents the
/// actual physical address in system memory that will be accessed.
///
/// # Example
///
/// ```
/// use smmu::`PA`;
///
/// let pa = `PA`::new_page_aligned(0x8000).unwrap();
/// assert!(pa.is_page_aligned());
/// ```
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PA(u64);

impl PA {
    /// Create a new `PA` from a u64 address
    ///
    /// # Errors
    ///
    /// Currently always succeeds, but returns `Result` for API consistency.
    #[inline]
    pub const fn new(addr: u64) -> Result<Self, ValidationError> {
        Ok(Self(addr))
    }

    /// Create a new `PA` with const fn for compile-time construction
    #[inline]
    #[must_use]
    pub const fn const_new(addr: u64) -> Self {
        Self(addr)
    }

    /// Create a page-aligned `PA`
    ///
    /// # Errors
    ///
    /// Returns an error if the address is not page-aligned.
    #[inline]
    pub const fn new_page_aligned(addr: u64) -> Result<Self, ValidationError> {
        if addr & PAGE_OFFSET_MASK != 0 {
            return Err(ValidationError::InvalidAlignment {
                address: addr,
                required_alignment: PAGE_SIZE,
            });
        }
        Ok(Self(addr))
    }

    /// Get the raw u64 address value
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Alias for `as_u64()` - gets the raw u64 address value
    #[inline]
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Check if the address is page-aligned
    #[inline]
    #[must_use]
    pub const fn is_page_aligned(self) -> bool {
        self.0 & PAGE_OFFSET_MASK == 0
    }

    /// Align the address down to the nearest page boundary
    #[inline]
    #[must_use]
    pub const fn align_down_to_page(self) -> Self {
        Self(self.0 & !PAGE_OFFSET_MASK)
    }

    /// Align the address up to the next page boundary
    #[inline]
    #[must_use]
    pub const fn align_up_to_page(self) -> Self {
        Self((self.0 + PAGE_OFFSET_MASK) & !PAGE_OFFSET_MASK)
    }

    /// Get the page offset (lower 12 bits)
    #[inline]
    #[must_use]
    pub const fn page_offset(self) -> u64 {
        self.0 & PAGE_OFFSET_MASK
    }

    /// Get the page number (upper bits, excluding offset)
    #[inline]
    #[must_use]
    pub const fn page_number(self) -> u64 {
        self.0 >> PAGE_SHIFT
    }

    /// Add an offset to the address (wrapping on overflow)
    #[inline]
    #[must_use]
    pub const fn add_offset(self, offset: u64) -> Self {
        Self(self.0.wrapping_add(offset))
    }

    /// Add an offset to the address, returning `None` on overflow
    #[inline]
    #[must_use]
    pub const fn checked_add(self, offset: u64) -> Option<Self> {
        match self.0.checked_add(offset) {
            Some(addr) => Some(Self(addr)),
            None => None,
        }
    }

    /// Apply a bitmask to the address
    #[inline]
    #[must_use]
    pub const fn mask(self, mask: u64) -> Self {
        Self(self.0 & mask)
    }
}

impl fmt::Debug for PA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PA({:#018x})", self.0)
    }
}

impl fmt::Display for PA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_size_constant() {
        assert_eq!(PAGE_SIZE, 0x1000);
    }

    #[test]
    fn test_iova_basic() {
        let iova = IOVA::new(0x1000).unwrap();
        assert_eq!(iova.as_u64(), 0x1000);
    }

    #[test]
    fn test_ipa_basic() {
        let ipa = IPA::new(0x2000).unwrap();
        assert_eq!(ipa.as_u64(), 0x2000);
    }

    #[test]
    fn test_pa_basic() {
        let pa = PA::new(0x3000).unwrap();
        assert_eq!(pa.as_u64(), 0x3000);
    }
}
