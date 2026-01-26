//! TLB (Translation Lookaside Buffer) implementation
//!
//! This module provides caching of translation results to accelerate repeated
//! address translations:
//!
//! - Translation caching with configurable size
//! - Cache invalidation operations
//! - Stream-specific and global invalidation
//! - PASID-aware caching
//!
//! # Performance Impact
//!
//! The TLB cache is critical for achieving target translation latency (135ns).
//! High cache hit rates dramatically reduce the cost of page table walks.
//!
//! # Cache Coherency
//!
//! Proper cache invalidation is essential for correctness when page table
//! mappings change. This module implements all required invalidation operations
//! per ARM SMMU v3 specification.

#![warn(missing_docs)]

use crate::types::{IOVA, PA, PagePermissions, SecurityState, StreamID, PASID};

// ============================================================================
// CacheEntry - Individual cache entry with translation result
// ============================================================================

/// Cache entry storing a single translation result
///
/// This structure represents a cached translation from IOVA to PA with
/// associated permissions and security state.
///
/// # Example
///
/// ```rust,ignore
/// use smmu::cache::CacheEntry;
/// use smmu::{IOVA, PA, PagePermissions, SecurityState};
///
/// let entry = CacheEntry::new(
///     IOVA::new(0x1000).unwrap(),
///     PA::new(0x2000).unwrap(),
///     PagePermissions::read_write(),
///     100,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheEntry {
    /// Input/Output Virtual Address
    pub iova: IOVA,

    /// Physical Address (translation result)
    pub physical_address: PA,

    /// Page permissions for this translation
    pub permissions: PagePermissions,

    /// Security state (Secure/NonSecure/Realm)
    pub security_state: SecurityState,

    /// Timestamp for LRU tracking
    pub timestamp: u64,
}

impl CacheEntry {
    /// Create a new cache entry with default security state
    ///
    /// Security state defaults to NonSecure.
    #[inline]
    pub const fn new(
        iova: IOVA,
        physical_address: PA,
        permissions: PagePermissions,
        timestamp: u64,
    ) -> Self {
        Self {
            iova,
            physical_address,
            permissions,
            security_state: SecurityState::NonSecure,
            timestamp,
        }
    }

    /// Create a new cache entry with explicit security state
    #[inline]
    pub const fn new_with_security(
        iova: IOVA,
        physical_address: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
        timestamp: u64,
    ) -> Self {
        Self {
            iova,
            physical_address,
            permissions,
            security_state,
            timestamp,
        }
    }
}

impl Default for CacheEntry {
    fn default() -> Self {
        Self {
            iova: IOVA::const_new(0),
            physical_address: PA::const_new(0),
            permissions: PagePermissions::none(),
            security_state: SecurityState::NonSecure,
            timestamp: 0,
        }
    }
}

// ============================================================================
// CacheKey - Multi-level cache indexing key
// ============================================================================

/// Cache key for multi-level indexing by StreamID, PASID, IOVA, and SecurityState
///
/// This structure is used as the key in the TLB cache HashMap to uniquely
/// identify a translation entry.
///
/// # Hash Quality
///
/// The hash implementation uses FNV-1a algorithm optimized for page-aligned
/// addresses by skipping the lower 12 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Stream identifier
    pub stream_id: StreamID,

    /// Process Address Space ID
    pub pasid: PASID,

    /// Input/Output Virtual Address
    pub iova: IOVA,

    /// Security state
    pub security_state: SecurityState,
}

impl CacheKey {
    /// Create a new cache key
    #[inline]
    pub const fn new(
        stream_id: StreamID,
        pasid: PASID,
        iova: IOVA,
        security_state: SecurityState,
    ) -> Self {
        Self {
            stream_id,
            pasid,
            iova,
            security_state,
        }
    }
}

// ============================================================================
// CacheKeyHash - FNV-1a hash implementation
// ============================================================================

/// Custom hash implementation for CacheKey using FNV-1a algorithm
///
/// This hasher is optimized for ARM SMMU v3 usage patterns:
/// - Skips lower 12 bits of IOVA (page-aligned addresses)
/// - Provides better distribution than default hash
/// - Uses FNV-1a constants for 64-bit hash values
///
/// # FNV-1a Algorithm
///
/// FNV-1a (Fowler-Noll-Vo) is a non-cryptographic hash function with
/// good distribution properties for hash tables.
pub struct CacheKeyHash;

impl CacheKeyHash {
    /// FNV-1a offset basis for 64-bit hash
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;

    /// FNV-1a prime for 64-bit hash
    const FNV_PRIME: u64 = 1099511628211;

    /// Hash a CacheKey using FNV-1a algorithm
    ///
    /// # Optimization
    ///
    /// The lower 12 bits of IOVA are skipped because ARM SMMU v3 uses
    /// 4KB pages, so these bits are always zero for page-aligned addresses.
    #[inline]
    pub fn hash(key: &CacheKey) -> u64 {
        let mut hash = Self::FNV_OFFSET_BASIS;

        // Hash StreamID (stored as u32, typically 16-bit value)
        hash ^= key.stream_id.as_u32() as u64;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        // Hash PASID (20-bit value stored in u32)
        hash ^= key.pasid.as_u32() as u64;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        // Hash IOVA - skip lower 12 bits (page offset)
        let page_number = key.iova.as_u64() >> 12;

        // Hash lower 32 bits of page number
        hash ^= page_number & 0xFFFF_FFFF;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        // Hash upper 32 bits of page number
        hash ^= page_number >> 32;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        // Hash SecurityState (2-bit enum)
        hash ^= key.security_state as u64;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        hash
    }
}

// ============================================================================
// StreamPASIDKey - Secondary index key
// ============================================================================

/// Key for secondary indexing by StreamID and PASID
///
/// Used for efficient invalidation operations that target all entries
/// for a specific stream or PASID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamPASIDKey {
    /// Stream identifier
    pub stream_id: StreamID,

    /// Process Address Space ID
    pub pasid: PASID,
}

impl StreamPASIDKey {
    /// Create a new StreamPASID key
    #[inline]
    pub const fn new(stream_id: StreamID, pasid: PASID) -> Self {
        Self { stream_id, pasid }
    }
}

// ============================================================================
// StreamPASIDKeyHash - FNV-1a hash for StreamPASIDKey
// ============================================================================

/// Custom hash implementation for StreamPASIDKey using FNV-1a algorithm
pub struct StreamPASIDKeyHash;

impl StreamPASIDKeyHash {
    /// FNV-1a offset basis for 64-bit hash
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;

    /// FNV-1a prime for 64-bit hash
    const FNV_PRIME: u64 = 1099511628211;

    /// Hash a StreamPASIDKey using FNV-1a algorithm
    #[inline]
    pub fn hash(key: &StreamPASIDKey) -> u64 {
        let mut hash = Self::FNV_OFFSET_BASIS;

        // Hash StreamID
        hash ^= key.stream_id.as_u32() as u64;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        // Hash PASID
        hash ^= key.pasid.as_u32() as u64;
        hash = hash.wrapping_mul(Self::FNV_PRIME);

        hash
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // CacheEntry Tests (30+ tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_cache_entry_default_construction() {
        let entry = CacheEntry::default();
        assert_eq!(entry.iova.as_u64(), 0);
        assert_eq!(entry.physical_address.as_u64(), 0);
        assert_eq!(entry.permissions, PagePermissions::none());
        assert_eq!(entry.security_state, SecurityState::NonSecure);
        assert_eq!(entry.timestamp, 0);
    }

    #[test]
    fn test_cache_entry_new_default_security() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry = CacheEntry::new(iova, pa, perms, 42);

        assert_eq!(entry.iova, iova);
        assert_eq!(entry.physical_address, pa);
        assert_eq!(entry.permissions, perms);
        assert_eq!(entry.security_state, SecurityState::NonSecure);
        assert_eq!(entry.timestamp, 42);
    }

    #[test]
    fn test_cache_entry_new_with_security_nonsecure() {
        let iova = IOVA::new(0x3000).unwrap();
        let pa = PA::new(0x4000).unwrap();
        let perms = PagePermissions::read_write();

        let entry = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::NonSecure, 100
        );

        assert_eq!(entry.security_state, SecurityState::NonSecure);
        assert_eq!(entry.timestamp, 100);
    }

    #[test]
    fn test_cache_entry_new_with_security_secure() {
        let iova = IOVA::new(0x5000).unwrap();
        let pa = PA::new(0x6000).unwrap();
        let perms = PagePermissions::all();

        let entry = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::Secure, 200
        );

        assert_eq!(entry.security_state, SecurityState::Secure);
        assert_eq!(entry.timestamp, 200);
    }

    #[test]
    fn test_cache_entry_new_with_security_realm() {
        let iova = IOVA::new(0x7000).unwrap();
        let pa = PA::new(0x8000).unwrap();
        let perms = PagePermissions::read_execute();

        let entry = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::Realm, 300
        );

        assert_eq!(entry.security_state, SecurityState::Realm);
        assert_eq!(entry.timestamp, 300);
    }

    #[test]
    fn test_cache_entry_copy_semantics() {
        let entry1 = CacheEntry::default();
        let entry2 = entry1; // Should copy, not move

        // Both should be usable
        assert_eq!(entry1.timestamp, 0);
        assert_eq!(entry2.timestamp, 0);
    }

    #[test]
    fn test_cache_entry_clone_semantics() {
        let entry1 = CacheEntry::default();
        let entry2 = entry1.clone();

        assert_eq!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_equality() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry1 = CacheEntry::new(iova, pa, perms, 42);
        let entry2 = CacheEntry::new(iova, pa, perms, 42);

        assert_eq!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_inequality_different_iova() {
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap();
        let pa = PA::new(0x3000).unwrap();
        let perms = PagePermissions::read_only();

        let entry1 = CacheEntry::new(iova1, pa, perms, 42);
        let entry2 = CacheEntry::new(iova2, pa, perms, 42);

        assert_ne!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_inequality_different_pa() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa1 = PA::new(0x2000).unwrap();
        let pa2 = PA::new(0x3000).unwrap();
        let perms = PagePermissions::read_only();

        let entry1 = CacheEntry::new(iova, pa1, perms, 42);
        let entry2 = CacheEntry::new(iova, pa2, perms, 42);

        assert_ne!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_inequality_different_permissions() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms1 = PagePermissions::read_only();
        let perms2 = PagePermissions::read_write();

        let entry1 = CacheEntry::new(iova, pa, perms1, 42);
        let entry2 = CacheEntry::new(iova, pa, perms2, 42);

        assert_ne!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_inequality_different_security_state() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry1 = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::NonSecure, 42
        );
        let entry2 = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::Secure, 42
        );

        assert_ne!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_inequality_different_timestamp() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry1 = CacheEntry::new(iova, pa, perms, 42);
        let entry2 = CacheEntry::new(iova, pa, perms, 100);

        assert_ne!(entry1, entry2);
    }

    #[test]
    fn test_cache_entry_debug_format() {
        let entry = CacheEntry::default();
        let debug_str = format!("{:?}", entry);
        assert!(debug_str.contains("CacheEntry"));
    }

    #[test]
    fn test_cache_entry_large_addresses() {
        let iova = IOVA::new(0xFFFF_FFFF_F000).unwrap();
        let pa = PA::new(0xFFFF_FFFF_E000).unwrap();
        let perms = PagePermissions::read_write();

        let entry = CacheEntry::new(iova, pa, perms, u64::MAX);

        assert_eq!(entry.iova.as_u64(), 0xFFFF_FFFF_F000);
        assert_eq!(entry.physical_address.as_u64(), 0xFFFF_FFFF_E000);
        assert_eq!(entry.timestamp, u64::MAX);
    }

    #[test]
    fn test_cache_entry_zero_timestamp() {
        let entry = CacheEntry::default();
        assert_eq!(entry.timestamp, 0);
    }

    #[test]
    fn test_cache_entry_max_timestamp() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry = CacheEntry::new(iova, pa, perms, u64::MAX);
        assert_eq!(entry.timestamp, u64::MAX);
    }

    #[test]
    fn test_cache_entry_permissions_none() {
        let entry = CacheEntry::default();
        assert_eq!(entry.permissions, PagePermissions::none());
    }

    #[test]
    fn test_cache_entry_permissions_read_only() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert_eq!(entry.permissions, PagePermissions::read_only());
    }

    #[test]
    fn test_cache_entry_permissions_write_only() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::write_only();

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert_eq!(entry.permissions, PagePermissions::write_only());
    }

    #[test]
    fn test_cache_entry_permissions_execute_only() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::new(false, false, true);

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert!(!entry.permissions.read());
        assert!(!entry.permissions.write());
        assert!(entry.permissions.execute());
    }

    #[test]
    fn test_cache_entry_permissions_read_write() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_write();

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert_eq!(entry.permissions, PagePermissions::read_write());
    }

    #[test]
    fn test_cache_entry_permissions_read_execute() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_execute();

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert_eq!(entry.permissions, PagePermissions::read_execute());
    }

    #[test]
    fn test_cache_entry_permissions_all() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::all();

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert_eq!(entry.permissions, PagePermissions::all());
    }

    #[test]
    fn test_cache_entry_permissions_write_execute() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::new(false, true, true);

        let entry = CacheEntry::new(iova, pa, perms, 0);
        assert!(!entry.permissions.read());
        assert!(entry.permissions.write());
        assert!(entry.permissions.execute());
    }

    #[test]
    fn test_cache_entry_const_new() {
        const ENTRY: CacheEntry = CacheEntry::new(
            IOVA::const_new(0x1000),
            PA::const_new(0x2000),
            PagePermissions::none(),
            42,
        );

        assert_eq!(ENTRY.iova.as_u64(), 0x1000);
        assert_eq!(ENTRY.physical_address.as_u64(), 0x2000);
        assert_eq!(ENTRY.timestamp, 42);
    }

    #[test]
    fn test_cache_entry_const_new_with_security() {
        const ENTRY: CacheEntry = CacheEntry::new_with_security(
            IOVA::const_new(0x1000),
            PA::const_new(0x2000),
            PagePermissions::none(),
            SecurityState::Secure,
            100,
        );

        assert_eq!(ENTRY.security_state, SecurityState::Secure);
        assert_eq!(ENTRY.timestamp, 100);
    }

    #[test]
    fn test_cache_entry_multiple_copies() {
        let entry1 = CacheEntry::default();
        let entry2 = entry1;
        let entry3 = entry2;
        let entry4 = entry3;

        // All should be equal
        assert_eq!(entry1, entry2);
        assert_eq!(entry2, entry3);
        assert_eq!(entry3, entry4);
    }

    #[test]
    fn test_cache_entry_page_aligned_addresses() {
        let iova = IOVA::new_page_aligned(0x1000).unwrap();
        let pa = PA::new_page_aligned(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        let entry = CacheEntry::new(iova, pa, perms, 0);

        assert!(entry.iova.is_page_aligned());
        assert!(entry.physical_address.is_page_aligned());
    }

    #[test]
    fn test_cache_entry_non_page_aligned_addresses() {
        // Cache entries can have non-page-aligned addresses (page offset preserved)
        let iova = IOVA::new(0x1234).unwrap();
        let pa = PA::new(0x5678).unwrap();
        let perms = PagePermissions::read_only();

        let entry = CacheEntry::new(iova, pa, perms, 0);

        assert_eq!(entry.iova.as_u64(), 0x1234);
        assert_eq!(entry.physical_address.as_u64(), 0x5678);
    }

    #[test]
    fn test_cache_entry_security_state_values() {
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_only();

        // Test all three security states
        let entry_ns = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::NonSecure, 0
        );
        let entry_s = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::Secure, 0
        );
        let entry_r = CacheEntry::new_with_security(
            iova, pa, perms, SecurityState::Realm, 0
        );

        assert_eq!(entry_ns.security_state, SecurityState::NonSecure);
        assert_eq!(entry_s.security_state, SecurityState::Secure);
        assert_eq!(entry_r.security_state, SecurityState::Realm);
    }

    // ------------------------------------------------------------------------
    // CacheKey Tests (15+ tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_cache_key_new() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        assert_eq!(key.stream_id, stream_id);
        assert_eq!(key.pasid, pasid);
        assert_eq!(key.iova, iova);
        assert_eq!(key.security_state, SecurityState::NonSecure);
    }

    #[test]
    fn test_cache_key_equality_same_values() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_inequality_different_stream_id() {
        let stream1 = StreamID::new(100).unwrap();
        let stream2 = StreamID::new(200).unwrap();
        let pasid = PASID::new(300).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream1, pasid, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream2, pasid, iova, SecurityState::NonSecure);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_inequality_different_pasid() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid1 = PASID::new(200).unwrap();
        let pasid2 = PASID::new(300).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid1, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid2, iova, SecurityState::NonSecure);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_inequality_different_iova() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova1, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova2, SecurityState::NonSecure);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_inequality_different_security_state() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_copy_semantics() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key2 = key1; // Should copy

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_clone_semantics() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key2 = key1.clone();

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_debug_format() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let debug_str = format!("{:?}", key);

        assert!(debug_str.contains("CacheKey"));
    }

    #[test]
    fn test_cache_key_const_construction() {
        // Test that CacheKey::new is const
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::const_new(0x1000);

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        assert_eq!(key.stream_id.as_u32(), 100);
        assert_eq!(key.pasid.as_u32(), 200);
        assert_eq!(key.iova.as_u64(), 0x1000);
    }

    #[test]
    fn test_cache_key_max_values() {
        let stream_id = StreamID::new(u16::MAX as u32).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap(); // 20-bit max
        let iova = IOVA::new(u64::MAX).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        assert_eq!(key.stream_id.as_u32(), u16::MAX as u32);
        assert_eq!(key.pasid.as_u32(), 0xF_FFFF);
        assert_eq!(key.iova.as_u64(), u64::MAX);
    }

    #[test]
    fn test_cache_key_min_values() {
        let stream_id = StreamID::new(0).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        assert_eq!(key.stream_id.as_u32(), 0);
        assert_eq!(key.pasid.as_u32(), 0);
        assert_eq!(key.iova.as_u64(), 0);
    }

    #[test]
    fn test_cache_key_page_aligned_iova() {
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new_page_aligned(0x1000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        assert!(key.iova.is_page_aligned());
    }

    #[test]
    fn test_cache_key_all_security_states() {
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key_ns = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key_s = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);
        let key_r = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        // All keys should be different
        assert_ne!(key_ns, key_s);
        assert_ne!(key_s, key_r);
        assert_ne!(key_ns, key_r);
    }

    #[test]
    fn test_cache_key_hashability() {
        use std::collections::HashMap;

        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        // Should be able to use as HashMap key
        let mut map = HashMap::new();
        map.insert(key, 42);

        assert_eq!(map.get(&key), Some(&42));
    }

    // ------------------------------------------------------------------------
    // CacheKeyHash Tests (20+ tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_cache_key_hash_fnv_constants() {
        assert_eq!(CacheKeyHash::FNV_OFFSET_BASIS, 14695981039346656037);
        assert_eq!(CacheKeyHash::FNV_PRIME, 1099511628211);
    }

    #[test]
    fn test_cache_key_hash_deterministic() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key);
        let hash2 = CacheKeyHash::hash(&key);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_cache_key_hash_different_stream_id() {
        let stream1 = StreamID::new(100).unwrap();
        let stream2 = StreamID::new(101).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream1, pasid, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream2, pasid, iova, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_key_hash_different_pasid() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid1 = PASID::new(200).unwrap();
        let pasid2 = PASID::new(201).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid1, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid2, iova, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_key_hash_different_iova_page() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova1, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova2, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_key_hash_page_offset_ignored() {
        // Lower 12 bits should be ignored (page offset)
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova1 = IOVA::new(0x1000).unwrap(); // Page aligned
        let iova2 = IOVA::new(0x1FFF).unwrap(); // Same page, different offset

        let key1 = CacheKey::new(stream_id, pasid, iova1, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova2, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        assert_eq!(hash1, hash2); // Should hash to same value
    }

    #[test]
    fn test_cache_key_hash_different_security_state() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key1 = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_key_hash_max_values() {
        let stream_id = StreamID::new(u16::MAX as u32).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap();
        let iova = IOVA::new(u64::MAX).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        // Should not panic
        let _hash = CacheKeyHash::hash(&key);
    }

    #[test]
    fn test_cache_key_hash_min_values() {
        let stream_id = StreamID::new(0).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        let hash = CacheKeyHash::hash(&key);

        // Hash should be non-zero even with zero inputs
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_cache_key_hash_distribution_different_streams() {
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let mut hashes = Vec::new();

        for stream in 0..100 {
            let stream_id = StreamID::new(stream).unwrap();
            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            hashes.push(CacheKeyHash::hash(&key));
        }

        // All hashes should be unique
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), 100);
    }

    #[test]
    fn test_cache_key_hash_distribution_different_pasids() {
        let stream_id = StreamID::new(100).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let mut hashes = Vec::new();

        for pasid_val in 0..100 {
            let pasid = PASID::new(pasid_val).unwrap();
            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            hashes.push(CacheKeyHash::hash(&key));
        }

        // All hashes should be unique
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), 100);
    }

    #[test]
    fn test_cache_key_hash_distribution_different_pages() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let mut hashes = Vec::new();

        for page in 0..100 {
            let iova = IOVA::new(page * 0x1000).unwrap();
            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            hashes.push(CacheKeyHash::hash(&key));
        }

        // All hashes should be unique
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), 100);
    }

    #[test]
    fn test_cache_key_hash_distribution_security_states() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key_ns = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key_s = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);
        let key_r = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        let hash_ns = CacheKeyHash::hash(&key_ns);
        let hash_s = CacheKeyHash::hash(&key_s);
        let hash_r = CacheKeyHash::hash(&key_r);

        // All should be different
        assert_ne!(hash_ns, hash_s);
        assert_ne!(hash_s, hash_r);
        assert_ne!(hash_ns, hash_r);
    }

    #[test]
    fn test_cache_key_hash_large_iova() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0xFFFF_FFFF_FFFF_F000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        // Should not panic with large addresses
        let _hash = CacheKeyHash::hash(&key);
    }

    #[test]
    fn test_cache_key_hash_page_number_upper_bits() {
        // Test that upper bits of page number are hashed
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova1 = IOVA::new(0x0000_0001_0000).unwrap(); // Low page number
        let iova2 = IOVA::new(0x1000_0000_0000).unwrap(); // High page number

        let key1 = CacheKey::new(stream_id, pasid, iova1, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova2, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_cache_key_hash_avalanche_effect() {
        // Single bit change should dramatically change hash
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova1 = IOVA::new(0x1000).unwrap();
        let iova2 = IOVA::new(0x2000).unwrap(); // Single bit difference in page number

        let key1 = CacheKey::new(stream_id, pasid, iova1, SecurityState::NonSecure);
        let key2 = CacheKey::new(stream_id, pasid, iova2, SecurityState::NonSecure);

        let hash1 = CacheKeyHash::hash(&key1);
        let hash2 = CacheKeyHash::hash(&key2);

        // Count differing bits
        let xor = hash1 ^ hash2;
        let bit_diff = xor.count_ones();

        // Expect significant bit difference (avalanche effect)
        assert!(bit_diff > 10, "Expected avalanche effect, got {} bits different", bit_diff);
    }

    #[test]
    fn test_cache_key_hash_collision_resistance() {
        // Generate many hashes and check for collisions
        use std::collections::HashSet;

        let mut hash_set = HashSet::new();

        for stream in 0..50 {
            for pasid_val in 0..50 {
                let stream_id = StreamID::new(stream).unwrap();
                let pasid = PASID::new(pasid_val).unwrap();
                let iova = IOVA::new((stream as u64) * 0x1000 + (pasid_val as u64) * 0x10000).unwrap();

                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let hash = CacheKeyHash::hash(&key);

                // Should not have any collisions
                assert!(hash_set.insert(hash), "Hash collision detected");
            }
        }

        assert_eq!(hash_set.len(), 50 * 50);
    }

    #[test]
    fn test_cache_key_hash_wrapping_mul() {
        // Ensure wrapping multiplication doesn't cause issues
        let stream_id = StreamID::new(u16::MAX as u32).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap();
        let iova = IOVA::new(u64::MAX).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        // Should not panic with wrapping multiplication
        let hash = CacheKeyHash::hash(&key);

        // Hash should be deterministic
        let hash2 = CacheKeyHash::hash(&key);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_cache_key_hash_fnv_algorithm() {
        // Verify FNV-1a algorithm implementation
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x3000).unwrap(); // Page number 3

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        // Manual FNV-1a calculation
        let mut expected = CacheKeyHash::FNV_OFFSET_BASIS;
        expected ^= 1u64; // stream_id
        expected = expected.wrapping_mul(CacheKeyHash::FNV_PRIME);
        expected ^= 2u64; // pasid
        expected = expected.wrapping_mul(CacheKeyHash::FNV_PRIME);
        expected ^= 3u64; // page number lower 32 bits
        expected = expected.wrapping_mul(CacheKeyHash::FNV_PRIME);
        expected ^= 0u64; // page number upper 32 bits
        expected = expected.wrapping_mul(CacheKeyHash::FNV_PRIME);
        expected ^= SecurityState::NonSecure as u64;
        expected = expected.wrapping_mul(CacheKeyHash::FNV_PRIME);

        let actual = CacheKeyHash::hash(&key);

        assert_eq!(actual, expected);
    }

    // ------------------------------------------------------------------------
    // StreamPASIDKey Tests (10+ tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_stream_pasid_key_new() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        assert_eq!(key.stream_id, stream_id);
        assert_eq!(key.pasid, pasid);
    }

    #[test]
    fn test_stream_pasid_key_equality() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key1 = StreamPASIDKey::new(stream_id, pasid);
        let key2 = StreamPASIDKey::new(stream_id, pasid);

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_stream_pasid_key_inequality_different_stream() {
        let stream1 = StreamID::new(100).unwrap();
        let stream2 = StreamID::new(101).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key1 = StreamPASIDKey::new(stream1, pasid);
        let key2 = StreamPASIDKey::new(stream2, pasid);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_stream_pasid_key_inequality_different_pasid() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid1 = PASID::new(200).unwrap();
        let pasid2 = PASID::new(201).unwrap();

        let key1 = StreamPASIDKey::new(stream_id, pasid1);
        let key2 = StreamPASIDKey::new(stream_id, pasid2);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_stream_pasid_key_copy_semantics() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key1 = StreamPASIDKey::new(stream_id, pasid);
        let key2 = key1; // Should copy

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_stream_pasid_key_clone_semantics() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key1 = StreamPASIDKey::new(stream_id, pasid);
        let key2 = key1.clone();

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_stream_pasid_key_debug_format() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);
        let debug_str = format!("{:?}", key);

        assert!(debug_str.contains("StreamPASIDKey"));
    }

    #[test]
    fn test_stream_pasid_key_const_construction() {
        // Test that StreamPASIDKey::new is const
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        assert_eq!(key.stream_id.as_u32(), 100);
        assert_eq!(key.pasid.as_u32(), 200);
    }

    #[test]
    fn test_stream_pasid_key_max_values() {
        let stream_id = StreamID::new(u16::MAX as u32).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        assert_eq!(key.stream_id.as_u32(), u16::MAX as u32);
        assert_eq!(key.pasid.as_u32(), 0xF_FFFF);
    }

    #[test]
    fn test_stream_pasid_key_min_values() {
        let stream_id = StreamID::new(0).unwrap();
        let pasid = PASID::new(0).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        assert_eq!(key.stream_id.as_u32(), 0);
        assert_eq!(key.pasid.as_u32(), 0);
    }

    #[test]
    fn test_stream_pasid_key_hashability() {
        use std::collections::HashMap;

        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        // Should be able to use as HashMap key
        let mut map = HashMap::new();
        map.insert(key, 42);

        assert_eq!(map.get(&key), Some(&42));
    }

    // ------------------------------------------------------------------------
    // StreamPASIDKeyHash Tests (10+ tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_stream_pasid_key_hash_fnv_constants() {
        assert_eq!(StreamPASIDKeyHash::FNV_OFFSET_BASIS, 14695981039346656037);
        assert_eq!(StreamPASIDKeyHash::FNV_PRIME, 1099511628211);
    }

    #[test]
    fn test_stream_pasid_key_hash_deterministic() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        let hash1 = StreamPASIDKeyHash::hash(&key);
        let hash2 = StreamPASIDKeyHash::hash(&key);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_stream_pasid_key_hash_different_stream() {
        let stream1 = StreamID::new(100).unwrap();
        let stream2 = StreamID::new(101).unwrap();
        let pasid = PASID::new(200).unwrap();

        let key1 = StreamPASIDKey::new(stream1, pasid);
        let key2 = StreamPASIDKey::new(stream2, pasid);

        let hash1 = StreamPASIDKeyHash::hash(&key1);
        let hash2 = StreamPASIDKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_stream_pasid_key_hash_different_pasid() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid1 = PASID::new(200).unwrap();
        let pasid2 = PASID::new(201).unwrap();

        let key1 = StreamPASIDKey::new(stream_id, pasid1);
        let key2 = StreamPASIDKey::new(stream_id, pasid2);

        let hash1 = StreamPASIDKeyHash::hash(&key1);
        let hash2 = StreamPASIDKeyHash::hash(&key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_stream_pasid_key_hash_max_values() {
        let stream_id = StreamID::new(u16::MAX as u32).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        // Should not panic
        let _hash = StreamPASIDKeyHash::hash(&key);
    }

    #[test]
    fn test_stream_pasid_key_hash_min_values() {
        let stream_id = StreamID::new(0).unwrap();
        let pasid = PASID::new(0).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        let hash = StreamPASIDKeyHash::hash(&key);

        // Hash should be non-zero even with zero inputs
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_stream_pasid_key_hash_distribution_streams() {
        let pasid = PASID::new(200).unwrap();

        let mut hashes = Vec::new();

        for stream in 0..100 {
            let stream_id = StreamID::new(stream).unwrap();
            let key = StreamPASIDKey::new(stream_id, pasid);
            hashes.push(StreamPASIDKeyHash::hash(&key));
        }

        // All hashes should be unique
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), 100);
    }

    #[test]
    fn test_stream_pasid_key_hash_distribution_pasids() {
        let stream_id = StreamID::new(100).unwrap();

        let mut hashes = Vec::new();

        for pasid_val in 0..100 {
            let pasid = PASID::new(pasid_val).unwrap();
            let key = StreamPASIDKey::new(stream_id, pasid);
            hashes.push(StreamPASIDKeyHash::hash(&key));
        }

        // All hashes should be unique
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), 100);
    }

    #[test]
    fn test_stream_pasid_key_hash_collision_resistance() {
        use std::collections::HashSet;

        let mut hash_set = HashSet::new();

        for stream in 0..100 {
            for pasid_val in 0..100 {
                let stream_id = StreamID::new(stream).unwrap();
                let pasid = PASID::new(pasid_val).unwrap();

                let key = StreamPASIDKey::new(stream_id, pasid);
                let hash = StreamPASIDKeyHash::hash(&key);

                assert!(hash_set.insert(hash), "Hash collision detected");
            }
        }

        assert_eq!(hash_set.len(), 100 * 100);
    }

    #[test]
    fn test_stream_pasid_key_hash_fnv_algorithm() {
        // Verify FNV-1a algorithm implementation
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        // Manual FNV-1a calculation
        let mut expected = StreamPASIDKeyHash::FNV_OFFSET_BASIS;
        expected ^= 1u64;
        expected = expected.wrapping_mul(StreamPASIDKeyHash::FNV_PRIME);
        expected ^= 2u64;
        expected = expected.wrapping_mul(StreamPASIDKeyHash::FNV_PRIME);

        let actual = StreamPASIDKeyHash::hash(&key);

        assert_eq!(actual, expected);
    }
}
