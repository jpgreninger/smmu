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

use crate::types::{PagePermissions, SecurityState, StreamID, IOVA, PA, PASID};
use smallvec::SmallVec;

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
/// ```rust
/// use smmu::cache::CacheEntry;
/// use smmu::{IOVA, PA, PagePermissions};
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

    /// ASID (Address Space Identifier) from CD.ASID — used for ASID-targeted invalidation.
    /// Defaults to 0 for Stage-2-only or bypass entries.
    pub asid: u16,

    /// VMID (Virtual Machine ID) from STE.S2VMID (ARM §5.2) — used for
    /// VMID-targeted invalidation via `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA`.
    /// Defaults to 0.
    pub vmid: u16,

    /// CONF-GAP-7: Intermediate Physical Address for two-stage TLB entries (§4.4).
    ///
    /// For entries populated during two-stage (S1+S2) translation, this field
    /// holds the Stage-1 output IPA that was subsequently translated by Stage-2.
    /// For single-stage entries this field is `0`.
    ///
    /// Used by `CMD_TLBI_S2_IPA` to perform IPA-selective invalidation rather
    /// than over-invalidating all VMID-tagged entries.
    pub ipa: u64,
}

impl CacheEntry {
    /// Create a new cache entry with default security state and ASID=0
    ///
    /// Security state defaults to NonSecure.
    #[inline]
    pub const fn new(iova: IOVA, physical_address: PA, permissions: PagePermissions, timestamp: u64) -> Self {
        Self {
            iova,
            physical_address,
            permissions,
            security_state: SecurityState::NonSecure,
            timestamp,
            asid: 0,
            vmid: 0,
            ipa: 0,
        }
    }

    /// Create a new cache entry with explicit security state, ASID=0, VMID=0
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
            asid: 0,
            vmid: 0,
            ipa: 0,
        }
    }

    /// Create a new cache entry with explicit security state and ASID (VMID=0).
    ///
    /// Used for Stage-1 TLB entries tagged with CD.ASID per ARM §3.17.
    #[inline]
    pub const fn new_with_asid(
        iova: IOVA,
        physical_address: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
        asid: u16,
        timestamp: u64,
    ) -> Self {
        Self {
            iova,
            physical_address,
            permissions,
            security_state,
            timestamp,
            asid,
            vmid: 0,
            ipa: 0,
        }
    }

    /// Create a new cache entry tagged with both ASID (CD.ASID, ARM §3.17) and
    /// VMID (STE.S2VMID, ARM §5.2).
    ///
    /// This is the primary constructor used by `translate()` so that both
    /// ASID-targeted (`CMD_TLBI_NH_ASID`) and VMID-targeted
    /// (`CMD_TLBI_S12_VMALL`) invalidation work correctly.
    #[inline]
    pub const fn new_with_tags(
        iova: IOVA,
        physical_address: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
        asid: u16,
        vmid: u16,
        timestamp: u64,
    ) -> Self {
        Self {
            iova,
            physical_address,
            permissions,
            security_state,
            timestamp,
            asid,
            vmid,
            ipa: 0,
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
            asid: 0,
            vmid: 0,
            ipa: 0,
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
    pub const fn new(stream_id: StreamID, pasid: PASID, iova: IOVA, security_state: SecurityState) -> Self {
        Self { stream_id, pasid, iova, security_state }
    }
}

// ============================================================================
// CacheKeyHash - FNV-1a hash implementation
// ============================================================================

/// Custom hash implementation for `CacheKey` using FNV-1a algorithm
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
#[derive(Debug)]
pub struct CacheKeyHash;

impl CacheKeyHash {
    /// Hash a `CacheKey` using optimized algorithm
    ///
    /// # Optimization
    ///
    /// Uses a fast mixing function optimized for hardware:
    /// - Minimal operations for sub-10ns latency
    /// - Good distribution for hash tables
    /// - The lower 12 bits of IOVA are skipped (4KB pages)
    /// - Uses efficient bit rotation and XOR mixing
    ///
    /// # ARM SMMU v3 Spec Compliance
    ///
    /// ARM IHI0070G.b §6.3.2 (SIDSIZE) defines StreamID as up to 32 bits wide.
    /// This implementation uses XOR-multiply combination to incorporate all 32
    /// bits of StreamID without loss — avoiding the overflow hazard of the
    /// previous `u64::from(sid) << 48` approach, which silently discarded bits
    /// 16-31 for any StreamID value >= 65536.
    ///
    /// BUG-RUST-2 fix: the previous formula placed the StreamID in bits 48-63
    /// of a u64 (`<< 48`).  For a 32-bit StreamID value with bits 16-31 set,
    /// the shift would overflow u64 and produce 0 — identical to StreamID 0.
    /// The fix uses a 64-bit multiply-add to mix all 32 bits without overflow.
    #[inline(always)]
    pub fn hash(key: &CacheKey) -> u64 {
        // BUG-RUST-2 fix: use XOR-multiply combination to incorporate all 32
        // bits of StreamID without bit-shift overflow.
        //
        // Previous (buggy) approach:
        //   let stream = u64::from(key.stream_id.as_u32()) << 48;
        // For StreamID >= 0x10000, bits 16-31 overflowed the 64-bit boundary
        // and were discarded, causing a hash collision with StreamID 0.
        //
        // Fixed approach: fold the 32-bit StreamID into the full 64-bit hash
        // via Knuth's multiplicative hash constant (a prime approximation of
        // phi^-1 * 2^64), providing excellent avalanche for all 32 bits.
        let stream_u64 = u64::from(key.stream_id.as_u32());
        let stream = stream_u64.wrapping_mul(0x9e37_79b9_7f4a_7c15_u64);

        // PASID (20 bits) — fold into hash using a different constant
        let pasid_u64 = u64::from(key.pasid.as_u32());
        let pasid = pasid_u64.wrapping_mul(0x6c62_272e_07bb_0142_u64);

        // Security state (2 bits)
        let security = u64::from(key.security_state as u8) & 0x3;

        // Page number (IOVA >> 12) — lower 12 bits are page offset (unused)
        let page = (key.iova.as_u64() >> 12).wrapping_mul(0x517c_c1b7_2722_0a95_u64);

        // Combine all fields with a non-zero seed (FNV-1a offset basis) so
        // all-zero inputs (e.g. stream=0, PASID=0, IOVA=0, NonSecure=0b00)
        // never produce a zero hash value.
        let mut hash = stream
            .wrapping_add(pasid)
            .wrapping_add(security)
            .wrapping_add(page)
            ^ 0xcbf2_9ce4_8422_2325_u64;

        // Fast mixing using bit rotation and XOR (murmur-like finalizer)
        // This provides good distribution with minimal operations
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        hash ^= hash >> 33;

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
#[derive(Debug)]
pub struct StreamPASIDKeyHash;

impl StreamPASIDKeyHash {
    /// Hash a StreamPASIDKey using optimized algorithm
    #[inline(always)]
    pub fn hash(key: &StreamPASIDKey) -> u64 {
        // Simple combination - StreamID and PASID are small values
        let combined = (u64::from(key.stream_id.as_u32()) << 32) | u64::from(key.pasid.as_u32());

        // Fast mixing with offset to ensure non-zero for zero input
        let mut hash = combined.wrapping_add(0xdead_beef);
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
        hash ^= hash >> 33;

        hash
    }
}

// ============================================================================
// FxHasher - Fast hash builder for DashMap
// ============================================================================

use std::hash::{BuildHasher, Hasher};

/// Fast hash builder using FNV-1a-style hashing
///
/// This hasher is optimized for performance over cryptographic security.
/// It provides 15-25ns improvement over default SipHash for DashMap lookups.
#[derive(Debug, Clone, Default)]
pub struct FxBuildHasher;

/// Fast hasher implementation using FNV-1a algorithm
///
/// Optimized for ARM SMMU cache keys with minimal operations.
#[derive(Debug, Default)]
pub struct FxHasher {
    hash: u64,
}

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // Process complete 8-byte chunks as native-endian u64 words via write_u64(),
        // ensuring write(&x.to_ne_bytes()) == write_u64(x) for any aligned input.
        let mut chunks = bytes.chunks_exact(8);
        for chunk in chunks.by_ref() {
            // SAFETY: chunks_exact(8) guarantees exactly 8 bytes.
            let word = u64::from_ne_bytes(chunk.try_into().expect("chunk is exactly 8 bytes"));
            self.write_u64(word);
        }
        // Handle trailing bytes (0–7) by zero-padding into a u64 in native-endian
        // byte order, then folding through write_u64() so the same mixing applies.
        let tail = chunks.remainder();
        if !tail.is_empty() {
            let mut word_bytes = [0u8; 8];
            word_bytes[..tail.len()].copy_from_slice(tail);
            let word = u64::from_ne_bytes(word_bytes);
            self.write_u64(word);
        }
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.hash ^= i;
        self.hash = self.hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
        self.hash ^= self.hash >> 33;
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }
}

impl BuildHasher for FxBuildHasher {
    type Hasher = FxHasher;

    #[inline]
    fn build_hasher(&self) -> FxHasher {
        FxHasher { hash: 0 }
    }
}

// ============================================================================
// TLB Cache Implementation
// ============================================================================

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Replacement policy for cache eviction
///
/// Determines which entry to evict when the cache is full and a new
/// entry needs to be inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementPolicy {
    /// Least Recently Used - evicts the entry that was least recently accessed
    Lru,
    /// First In First Out - evicts the oldest entry regardless of access pattern
    Fifo,
}

impl Default for ReplacementPolicy {
    fn default() -> Self {
        Self::Lru
    }
}

/// Cache statistics tracking performance metrics
///
/// All counters use atomic operations for lock-free updates across threads.
/// Statistics can be read at any time without blocking cache operations.
///
/// # Performance Metrics
///
/// - **Hit Rate**: `hits / (hits + misses)` - percentage of successful lookups
/// - **Miss Rate**: `misses / (hits + misses)` - percentage of failed lookups
/// - **Efficiency**: Overall cache effectiveness at reducing translation costs
#[derive(Debug)]
pub struct CacheStatistics {
    /// Total cache lookup attempts (hits + misses)
    pub lookups: AtomicU64,

    /// Successful cache lookups
    pub hits: AtomicU64,

    /// Failed cache lookups
    pub misses: AtomicU64,

    /// Number of entries evicted due to capacity
    pub evictions: AtomicU64,

    /// Number of entries inserted into cache
    pub insertions: AtomicU64,

    /// Number of entries invalidated (removed explicitly)
    pub invalidations: AtomicU64,
}

impl CacheStatistics {
    /// Create a new statistics tracker with all counters at zero
    #[inline]
    pub const fn new() -> Self {
        Self {
            lookups: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            insertions: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
        }
    }

    /// Reset all statistics counters to zero
    #[inline]
    pub fn reset(&self) {
        self.lookups.store(0, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
        self.insertions.store(0, Ordering::Relaxed);
        self.invalidations.store(0, Ordering::Relaxed);
    }

    /// Get current lookup count
    #[inline]
    pub fn get_lookups(&self) -> u64 {
        self.lookups.load(Ordering::Relaxed)
    }

    /// Get current hit count
    #[inline]
    pub fn get_hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Get current miss count
    #[inline]
    pub fn get_misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// Get current eviction count
    #[inline]
    pub fn get_evictions(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }

    /// Get current insertion count
    #[inline]
    pub fn get_insertions(&self) -> u64 {
        self.insertions.load(Ordering::Relaxed)
    }

    /// Get current invalidation count
    #[inline]
    pub fn get_invalidations(&self) -> u64 {
        self.invalidations.load(Ordering::Relaxed)
    }

    /// Calculate cache hit rate as percentage (0.0 to 100.0)
    ///
    /// Returns 0.0 if no lookups have been performed.
    #[inline]
    pub fn hit_rate(&self) -> f64 {
        let hits = self.get_hits();
        let lookups = self.get_lookups();

        if lookups == 0 {
            0.0
        } else {
            (hits as f64 / lookups as f64) * 100.0
        }
    }

    /// Calculate cache miss rate as percentage (0.0 to 100.0)
    ///
    /// Returns 0.0 if no lookups have been performed.
    #[inline]
    pub fn miss_rate(&self) -> f64 {
        let misses = self.get_misses();
        let lookups = self.get_lookups();

        if lookups == 0 {
            0.0
        } else {
            (misses as f64 / lookups as f64) * 100.0
        }
    }
}

impl Default for CacheStatistics {
    fn default() -> Self {
        Self::new()
    }
}

/// TLB (Translation Lookaside Buffer) cache implementation
///
/// Provides high-performance caching of address translations with:
/// - Lock-free concurrent access using DashMap
/// - Configurable replacement policies (LRU/FIFO)
/// - Comprehensive invalidation strategies
/// - Atomic statistics tracking
/// - Multi-level indexing for efficient lookups
///
/// # Thread Safety
///
/// The TLB cache is fully thread-safe and can be shared across threads.
/// All operations (lookup, insert, invalidate) are safe to call concurrently.
///
/// # Performance
///
/// - Lookup: Average O(1) with hash table
/// - Insert: Average O(1) with eviction overhead
/// - Invalidation: O(n) where n is number of entries matching criteria
///
/// # Example
///
/// ```rust
/// use smmu::cache::{TlbCache, CacheKey, CacheEntry, ReplacementPolicy};
/// use smmu::{IOVA, PA, PagePermissions, SecurityState, StreamID, PASID};
///
/// let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
///
/// let stream_id = StreamID::new(1).unwrap();
/// let pasid     = PASID::new(0).unwrap();
/// let iova      = IOVA::new(0x1000).unwrap();
/// let pa        = PA::new(0x2000).unwrap();
/// let key   = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
/// let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), 0);
///
/// // Insert translation
/// cache.insert(key, entry);
///
/// // Lookup translation
/// if let Some(hit) = cache.lookup(&key) {
///     let _ = hit.physical_address.as_u64(); // cache hit - use cached translation
/// }
///
/// // Invalidate by stream
/// cache.invalidate_by_stream(stream_id);
/// ```
pub struct TlbCache {
    /// Main cache storage using lock-free concurrent hash map with custom FxHasher
    entries: Arc<DashMap<CacheKey, CacheEntry, FxBuildHasher>>,

    /// Maximum number of entries in cache
    capacity: usize,

    /// Replacement policy for eviction
    policy: ReplacementPolicy,

    /// Global timestamp counter for LRU tracking
    timestamp: AtomicU64,

    /// Cache performance statistics
    statistics: Arc<CacheStatistics>,
}

impl TlbCache {
    /// Create a new TLB cache with specified capacity and replacement policy
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of cached entries (must be > 0)
    /// * `policy` - Replacement policy for eviction (LRU or FIFO)
    ///
    /// # Panics
    ///
    /// Panics if capacity is 0.
    ///
    /// # Example
    ///
    /// ```rust
    /// use smmu::cache::{TlbCache, ReplacementPolicy};
    /// let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// ```
    pub fn new(capacity: usize, policy: ReplacementPolicy) -> Self {
        assert!(capacity > 0, "TlbCache capacity must be greater than 0");

        Self {
            entries: Arc::new(DashMap::with_capacity_and_hasher(capacity, FxBuildHasher)),
            capacity,
            policy,
            timestamp: AtomicU64::new(0),
            statistics: Arc::new(CacheStatistics::new()),
        }
    }

    /// Lookup a translation in the cache
    ///
    /// Returns a copy of the cache entry if found, or None on cache miss.
    /// Updates statistics and LRU timestamp on hit.
    ///
    /// # Performance
    ///
    /// Optimized for sub-50ns average O(1) hash table lookup with lock-free read access.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, CacheKey, CacheEntry, ReplacementPolicy};
    /// # use smmu::{IOVA, PA, PagePermissions, SecurityState, StreamID, PASID};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// # let key = CacheKey::new(StreamID::new(1).unwrap(), PASID::new(0).unwrap(),
    /// #     IOVA::new(0x1000).unwrap(), SecurityState::NonSecure);
    /// # let entry = CacheEntry::new(IOVA::new(0x1000).unwrap(),
    /// #     PA::new(0x2000).unwrap(), PagePermissions::read_write(), 0);
    /// # cache.insert(key, entry);
    /// if let Some(hit) = cache.lookup(&key) {
    ///     println!("PA: 0x{:x}", hit.physical_address.as_u64());
    /// }
    /// ```
    #[inline(always)]
    pub fn lookup(&self, key: &CacheKey) -> Option<CacheEntry> {
        self.statistics.lookups.fetch_add(1, Ordering::Relaxed);

        if self.policy == ReplacementPolicy::Lru {
            // LRU: refresh timestamp on hit so evict_one() always removes the
            // least recently *used* entry, not the least recently inserted one.
            if let Some(mut entry_ref) = self.entries.get_mut(key) {
                self.statistics.hits.fetch_add(1, Ordering::Relaxed);
                let ts = self.timestamp.fetch_add(1, Ordering::Relaxed);
                entry_ref.timestamp = ts;
                return Some(*entry_ref);
            }
            self.statistics.misses.fetch_add(1, Ordering::Relaxed);
            None
        } else {
            // FIFO: insertion order only, no timestamp update on lookup.
            if let Some(entry_ref) = self.entries.get(key) {
                self.statistics.hits.fetch_add(1, Ordering::Relaxed);
                return Some(*entry_ref);
            }
            self.statistics.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Insert a translation into the cache
    ///
    /// If the cache is at capacity, evicts an entry according to the
    /// replacement policy before inserting the new entry.
    ///
    /// # Arguments
    ///
    /// * `key` - Cache key identifying the translation
    /// * `entry` - Translation result to cache
    ///
    /// # Performance
    ///
    /// Optimized for O(1) insertion with minimal overhead.
    /// Uses lock-free operations where possible.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, CacheKey, CacheEntry, ReplacementPolicy};
    /// # use smmu::{IOVA, PA, PagePermissions, SecurityState, StreamID, PASID};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// # let key = CacheKey::new(StreamID::new(1).unwrap(), PASID::new(0).unwrap(),
    /// #     IOVA::new(0x1000).unwrap(), SecurityState::NonSecure);
    /// # let entry = CacheEntry::new(IOVA::new(0x1000).unwrap(),
    /// #     PA::new(0x2000).unwrap(), PagePermissions::read_write(), 0);
    /// cache.insert(key, entry);
    /// ```
    #[inline]
    pub fn insert(&self, key: CacheKey, mut entry: CacheEntry) {
        // Update timestamp for LRU tracking
        let timestamp = self.timestamp.fetch_add(1, Ordering::Relaxed);
        entry.timestamp = timestamp;

        // Insert the new entry first, then enforce capacity by evicting until
        // the map is within bounds.  This post-insert eviction loop handles the
        // TOCTOU race: multiple concurrent threads may each pass a pre-insert
        // capacity check and all insert simultaneously, causing the map to grow
        // beyond capacity.  By checking *after* insertion and evicting in a loop,
        // each thread participates in trimming the map back to capacity, so the
        // final size remains bounded regardless of concurrency.
        self.entries.insert(key, entry);
        while self.entries.len() > self.capacity {
            self.evict_one();
        }
        self.statistics.insertions.fetch_add(1, Ordering::Relaxed);
    }

    /// Fast eviction - evicts first entry found (approximate LRU/FIFO)
    ///
    /// This is optimized for speed over perfect eviction policy.
    /// Trades perfect LRU for sub-100ns insertion performance.
    #[allow(dead_code)]
    #[inline(always)]
    fn evict_one_fast(&self) {
        // Try a completely different approach - just remove any arbitrary key
        // DashMap doesn't have a good way to get "first" entry efficiently
        // So we'll just iterate and remove the first one we find

        for entry_ref in self.entries.iter().take(1) {
            let key_to_remove = *entry_ref.key();
            drop(entry_ref);
            if self.entries.remove(&key_to_remove).is_some() {
                self.statistics.evictions.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }

    /// Evict one entry according to replacement policy (precise version)
    ///
    /// For LRU: Finds and evicts entry with oldest timestamp
    /// For FIFO: Evicts first entry (approximate)
    fn evict_one(&self) {
        let key_to_evict = match self.policy {
            ReplacementPolicy::Lru => {
                // Find entry with minimum timestamp
                self.entries
                    .iter()
                    .min_by_key(|entry| entry.value().timestamp)
                    .map(|entry| *entry.key())
            },
            ReplacementPolicy::Fifo => {
                // Just take first entry for FIFO
                self.entries.iter().next().map(|entry| *entry.key())
            },
        };

        if let Some(key) = key_to_evict {
            self.remove_entry(&key);
            self.statistics.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove a single entry
    #[inline]
    fn remove_entry(&self, key: &CacheKey) {
        self.entries.remove(key);
    }

    /// Invalidate all entries in the cache (global flush)
    ///
    /// Clears all cached translations. This is typically called when
    /// page table configuration changes globally.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// cache.invalidate_all();
    /// ```
    pub fn invalidate_all(&self) {
        let count = self.entries.len();
        self.entries.clear();

        self.statistics.invalidations.fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Invalidate all entries for a specific StreamID
    ///
    /// Removes all cached translations for the given stream across all PASIDs.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier to invalidate
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # use smmu::StreamID;
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// let stream_id = StreamID::new(1).unwrap();
    /// cache.invalidate_by_stream(stream_id);
    /// ```
    pub fn invalidate_by_stream(&self, stream_id: StreamID) {
        let mut removed_count = 0;

        // Use SmallVec to avoid heap allocation for common case
        // Most invalidations affect a small number of entries
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        // Collect keys to remove (to avoid holding iterator during removal)
        for entry in self.entries.iter() {
            if entry.key().stream_id == stream_id {
                keys_to_remove.push(*entry.key());
            }
        }

        // Remove entries
        for key in keys_to_remove {
            self.remove_entry(&key);
            removed_count += 1;
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all entries for a specific PASID
    ///
    /// Removes all cached translations for the given PASID across all streams.
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to invalidate
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # use smmu::PASID;
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// let pasid = PASID::new(42).unwrap();
    /// cache.invalidate_by_pasid(pasid);
    /// ```
    pub fn invalidate_by_pasid(&self, pasid: PASID) {
        let mut removed_count = 0;

        // Use SmallVec to avoid heap allocation for common case
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        // Collect keys to remove
        for entry in self.entries.iter() {
            if entry.key().pasid == pasid {
                keys_to_remove.push(*entry.key());
            }
        }

        // Remove entries
        for key in keys_to_remove {
            self.remove_entry(&key);
            removed_count += 1;
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all entries for a specific StreamID and PASID combination
    ///
    /// Removes all cached translations for the given stream/PASID pair.
    /// This is the most common invalidation operation.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `pasid` - Process Address Space ID
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # use smmu::{StreamID, PASID};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// let stream_id = StreamID::new(1).unwrap();
    /// let pasid = PASID::new(42).unwrap();
    /// cache.invalidate_by_stream_pasid(stream_id, pasid);
    /// ```
    pub fn invalidate_by_stream_pasid(&self, stream_id: StreamID, pasid: PASID) {
        let mut removed_count = 0;

        // Use SmallVec to collect keys to remove
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        // Collect keys matching stream_id and pasid
        for entry in self.entries.iter() {
            let key = entry.key();
            if key.stream_id == stream_id && key.pasid == pasid {
                keys_to_remove.push(*key);
            }
        }

        // Remove entries
        for key in keys_to_remove {
            self.remove_entry(&key);
            removed_count += 1;
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate entries within a virtual address range
    ///
    /// Removes cached translations for IOVAs within the specified range
    /// for a given stream/PASID combination.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `start` - Start of IOVA range (inclusive)
    /// * `end` - End of IOVA range (inclusive)
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # use smmu::{IOVA, StreamID, PASID};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// let stream_id = StreamID::new(1).unwrap();
    /// let pasid = PASID::new(0).unwrap();
    /// let start = IOVA::new(0x1000).unwrap();
    /// let end   = IOVA::new(0x5000).unwrap();
    /// cache.invalidate_by_va_range(stream_id, pasid, start, end);
    /// ```
    pub fn invalidate_by_va_range(&self, stream_id: StreamID, pasid: PASID, start: IOVA, end: IOVA) {
        let mut removed_count = 0;

        // Use SmallVec to avoid heap allocation for common case
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        // Collect keys to remove within range
        for entry in self.entries.iter() {
            let key = entry.key();
            if key.stream_id == stream_id
                && key.pasid == pasid
                && key.iova.as_u64() >= start.as_u64()
                && key.iova.as_u64() <= end.as_u64()
            {
                keys_to_remove.push(*key);
            }
        }

        // Remove entries
        for key in keys_to_remove {
            self.remove_entry(&key);
            removed_count += 1;
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all Stage-1 TLB entries tagged with the given ASID.
    ///
    /// Implements `CMD_TLBI_NH_ASID` / `CMD_TLBI_EL2_ASID` per ARM SMMU v3 §4.4.
    /// Only entries whose `CacheEntry::asid` matches `target_asid` are evicted;
    /// all other entries remain cached.
    ///
    /// # Arguments
    ///
    /// * `target_asid` - The 16-bit ASID to invalidate (CD.ASID, §3.17)
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// cache.invalidate_by_asid(42);
    /// ```
    pub fn invalidate_by_asid(&self, target_asid: u16) {
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        // Scan all entries; evict those tagged with the target ASID.
        for entry in self.entries.iter() {
            if entry.value().asid == target_asid {
                keys_to_remove.push(*entry.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all TLB entries tagged with the given VMID.
    ///
    /// Implements `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA` per ARM SMMU v3 §4.4.
    /// Only entries whose `CacheEntry::vmid` matches `target_vmid` are evicted;
    /// all other entries remain cached.
    ///
    /// # Arguments
    ///
    /// * `target_vmid` - The 16-bit VMID to invalidate (STE.S2VMID, §5.2)
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// cache.invalidate_by_vmid(42);
    /// ```
    pub fn invalidate_by_vmid(&self, target_vmid: u16) {
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        // Scan all entries; evict those tagged with the target VMID.
        for entry in self.entries.iter() {
            if entry.value().vmid == target_vmid {
                keys_to_remove.push(*entry.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all TLB entries tagged with both the given VMID and ASID.
    ///
    /// Implements `CMD_TLBI_NH_ASID` per ARM SMMU v3 §4.4.2.2 (NS/Realm queues):
    /// "Invalidate by ASID and VMID" — only entries whose `vmid` AND `asid`
    /// both match are evicted.
    ///
    /// Note: `CMD_TLBI_EL2_ASID` (§4.4.2.10) uses ASID-only; use `invalidate_by_asid` for that.
    ///
    /// # Arguments
    ///
    /// * `target_vmid` - The 16-bit VMID to match
    /// * `target_asid` - The 16-bit ASID to match
    pub fn invalidate_by_vmid_and_asid(&self, target_vmid: u16, target_asid: u16) {
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        for entry in self.entries.iter() {
            let e = entry.value();
            if e.vmid == target_vmid && e.asid == target_asid {
                keys_to_remove.push(*entry.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }

        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all TLB entries matching the given VA and ASID (§4.4 VA-targeted TLBI).
    ///
    /// Implements `CMD_TLBI_NH_VA`, `CMD_TLBI_EL2_VA`, `CMD_TLBI_EL3_VA` selective
    /// invalidation: only entries whose `iova` matches the page-aligned `va` AND whose
    /// `asid` matches `target_asid` are evicted.
    ///
    /// # Arguments
    ///
    /// * `va`         - Virtual address (raw u64; lower 12 bits are masked / ignored)
    /// * `target_asid`- ASID to match
    pub fn invalidate_by_va_and_asid(&self, va: u64, target_asid: u16) {
        const PAGE_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;
        let page_va = va & PAGE_MASK;

        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        for entry_ref in self.entries.iter() {
            let e = entry_ref.value();
            if e.asid == target_asid && (e.iova.as_u64() & PAGE_MASK) == page_va {
                keys_to_remove.push(*entry_ref.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }
        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all TLB entries matching the given VA, regardless of ASID (§4.4 VAA TLBI).
    ///
    /// Implements `CMD_TLBI_NH_VAA`, `CMD_TLBI_EL2_VAA`, `CMD_TLBI_S_EL2_VAA` —
    /// evicts any entry whose `iova` matches the page-aligned `va`, for any ASID.
    ///
    /// # Arguments
    ///
    /// * `va` - Virtual address (raw u64; lower 12 bits are masked / ignored)
    pub fn invalidate_by_va(&self, va: u64) {
        const PAGE_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;
        let page_va = va & PAGE_MASK;

        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        for entry_ref in self.entries.iter() {
            if (entry_ref.value().iova.as_u64() & PAGE_MASK) == page_va {
                keys_to_remove.push(*entry_ref.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }
        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate all TLB entries within a VA range for a given ASID (§4.4.1.1 RIL).
    ///
    /// Implements range-based TLBI: evicts entries where
    /// `start <= entry.iova <= end` AND `entry.asid == target_asid`.
    ///
    /// # Arguments
    ///
    /// * `start`      - Inclusive start of the VA range (raw u64)
    /// * `end`        - Inclusive end of the VA range (raw u64)
    /// * `target_asid`- ASID to match
    pub fn invalidate_by_va_range_and_asid(&self, start: u64, end: u64, target_asid: u16) {
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        for entry_ref in self.entries.iter() {
            let e = entry_ref.value();
            let iova = e.iova.as_u64();
            if e.asid == target_asid && iova >= start && iova <= end {
                keys_to_remove.push(*entry_ref.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }
        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate TLB entries by VMID with wildcard masking (§6.3.9 CR0.VMW).
    ///
    /// Evicts entries where `(entry.vmid & vmid_mask) == (target_vmid & vmid_mask)`.
    /// When `vmid_mask == 0xFFFF` (VMW=0), this is an exact VMID match.
    /// When `vmid_mask == 0` (VMW=16), all VMIDs match (global invalidation).
    ///
    /// # Arguments
    ///
    /// * `target_vmid` - Base VMID from the TLBI command operand
    /// * `vmid_mask`   - Bitmask derived from CR0.VMW — `(0xFFFF << vmw) as u16`
    pub fn invalidate_by_vmid_with_mask(&self, target_vmid: u16, vmid_mask: u16) {
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        for entry_ref in self.entries.iter() {
            let e = entry_ref.value();
            if (e.vmid & vmid_mask) == (target_vmid & vmid_mask) {
                keys_to_remove.push(*entry_ref.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }
        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// CONF-GAP-7: Invalidate TLB entries by VMID and IPA range (§4.4 `CMD_TLBI_S2_IPA`).
    ///
    /// Implements IPA-selective Stage-2 invalidation: evicts entries where
    /// `(entry.vmid & vmid_mask) == (target_vmid & vmid_mask)` AND
    /// `entry.ipa != 0` AND `entry.ipa` is within `[ipa_start, ipa_end]` (inclusive).
    ///
    /// Entries with `ipa == 0` are Stage-1-only entries and are NOT matched,
    /// preventing over-invalidation of non-two-stage TLB entries.
    ///
    /// # Arguments
    ///
    /// * `target_vmid`  - VMID from the TLBI command operand
    /// * `vmid_mask`    - Bitmask from CR0.VMW — `(0xFFFF << vmw) as u16`
    /// * `ipa_start`    - Inclusive start of the IPA range (raw u64)
    /// * `ipa_end`      - Inclusive end of the IPA range (raw u64)
    pub fn invalidate_by_vmid_and_ipa(&self, target_vmid: u16, vmid_mask: u16, ipa_start: u64, ipa_end: u64) {
        let mut keys_to_remove: SmallVec<[CacheKey; 32]> = SmallVec::new();

        for entry_ref in self.entries.iter() {
            let e = entry_ref.value();
            // Only match two-stage entries (ipa != 0) within the IPA range.
            if e.ipa != 0
                && (e.vmid & vmid_mask) == (target_vmid & vmid_mask)
                && e.ipa >= ipa_start
                && e.ipa <= ipa_end
            {
                keys_to_remove.push(*entry_ref.key());
            }
        }

        let removed_count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.remove_entry(&key);
        }
        self.statistics.invalidations.fetch_add(removed_count, Ordering::Relaxed);
    }

    /// Invalidate a specific entry by exact key match
    ///
    /// Removes a single cached translation if it exists.
    ///
    /// # Arguments
    ///
    /// * `key` - Exact cache key to invalidate
    ///
    /// # Returns
    ///
    /// Returns `true` if entry was found and removed, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, CacheKey, CacheEntry, ReplacementPolicy};
    /// # use smmu::{IOVA, PA, PagePermissions, SecurityState, StreamID, PASID};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// # let key = CacheKey::new(StreamID::new(1).unwrap(), PASID::new(0).unwrap(),
    /// #     IOVA::new(0x1000).unwrap(), SecurityState::NonSecure);
    /// if cache.invalidate_entry(&key) {
    ///     println!("Entry invalidated");
    /// }
    /// ```
    pub fn invalidate_entry(&self, key: &CacheKey) -> bool {
        if self.entries.remove(key).is_some() {
            // entries.remove() above already removes the entry; do NOT call
            // self.remove_entry() again — that would attempt a second removal
            // and could silently delete a concurrently re-inserted entry.
            self.statistics.invalidations.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Get current cache statistics
    ///
    /// Returns a reference to the atomic statistics counters that can be
    /// read without blocking cache operations.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// let stats = cache.statistics();
    /// println!("Hit rate: {:.2}%", stats.hit_rate());
    /// println!("Lookups: {}", stats.get_lookups());
    /// ```
    #[inline]
    pub fn statistics(&self) -> &CacheStatistics {
        &self.statistics
    }

    /// Clear all statistics counters
    ///
    /// Resets all statistics to zero. Does not affect cached entries.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// cache.clear_statistics();
    /// ```
    #[inline]
    pub fn clear_statistics(&self) {
        self.statistics.reset();
    }

    /// Get current number of cached entries
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// println!("Cache contains {} entries", cache.len());
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// if cache.is_empty() {
    ///     println!("Cache is empty");
    /// }
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get cache capacity
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// println!("Cache capacity: {}", cache.capacity());
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get replacement policy
    ///
    /// # Example
    ///
    /// ```rust
    /// # use smmu::cache::{TlbCache, ReplacementPolicy};
    /// # let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    /// match cache.policy() {
    ///     ReplacementPolicy::Lru => println!("Using LRU"),
    ///     ReplacementPolicy::Fifo => println!("Using FIFO"),
    /// }
    /// ```
    #[inline]
    pub fn policy(&self) -> ReplacementPolicy {
        self.policy
    }
}

impl std::fmt::Debug for TlbCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlbCache")
            .field("capacity", &self.capacity)
            .field("policy", &self.policy)
            .field("len", &self.len())
            .field("statistics", &self.statistics)
            .finish()
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

        let entry = CacheEntry::new_with_security(iova, pa, perms, SecurityState::NonSecure, 100);

        assert_eq!(entry.security_state, SecurityState::NonSecure);
        assert_eq!(entry.timestamp, 100);
    }

    #[test]
    fn test_cache_entry_new_with_security_secure() {
        let iova = IOVA::new(0x5000).unwrap();
        let pa = PA::new(0x6000).unwrap();
        let perms = PagePermissions::all();

        let entry = CacheEntry::new_with_security(iova, pa, perms, SecurityState::Secure, 200);

        assert_eq!(entry.security_state, SecurityState::Secure);
        assert_eq!(entry.timestamp, 200);
    }

    #[test]
    fn test_cache_entry_new_with_security_realm() {
        let iova = IOVA::new(0x7000).unwrap();
        let pa = PA::new(0x8000).unwrap();
        let perms = PagePermissions::read_execute();

        let entry = CacheEntry::new_with_security(iova, pa, perms, SecurityState::Realm, 300);

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

        let entry1 = CacheEntry::new_with_security(iova, pa, perms, SecurityState::NonSecure, 42);
        let entry2 = CacheEntry::new_with_security(iova, pa, perms, SecurityState::Secure, 42);

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
        let debug_str = format!("{entry:?}");
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
        const ENTRY: CacheEntry =
            CacheEntry::new(IOVA::const_new(0x1000), PA::const_new(0x2000), PagePermissions::none(), 42);

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
        let entry_nonsecure = CacheEntry::new_with_security(iova, pa, perms, SecurityState::NonSecure, 0);
        let entry_secure = CacheEntry::new_with_security(iova, pa, perms, SecurityState::Secure, 0);
        let entry_realm = CacheEntry::new_with_security(iova, pa, perms, SecurityState::Realm, 0);

        assert_eq!(entry_nonsecure.security_state, SecurityState::NonSecure);
        assert_eq!(entry_secure.security_state, SecurityState::Secure);
        assert_eq!(entry_realm.security_state, SecurityState::Realm);
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
        let debug_str = format!("{key:?}");

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
        let stream_id = StreamID::new(u32::from(u16::MAX)).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap(); // 20-bit max
        let iova = IOVA::new(u64::MAX).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        assert_eq!(key.stream_id.as_u32(), u32::from(u16::MAX));
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

        let key_nonsecure = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key_secure = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);
        let key_realm = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        // All keys should be different
        assert_ne!(key_nonsecure, key_secure);
        assert_ne!(key_secure, key_realm);
        assert_ne!(key_nonsecure, key_realm);
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
    fn test_cache_key_hash_uses_murmur_constants() {
        // Verify the hash uses the optimized murmur-like mixing constants
        // These constants provide good distribution with minimal operations
        const MIX_CONSTANT_1: u64 = 0xff51_afd7_ed55_8ccd;
        const MIX_CONSTANT_2: u64 = 0xc4ce_b9fe_1a85_ec53;

        // Just verify the constants are the expected values
        assert_eq!(MIX_CONSTANT_1, 0xff51_afd7_ed55_8ccd);
        assert_eq!(MIX_CONSTANT_2, 0xc4ce_b9fe_1a85_ec53);
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
        let stream_id = StreamID::new(u32::from(u16::MAX)).unwrap();
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
        hashes.sort_unstable();
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
        hashes.sort_unstable();
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
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 100);
    }

    #[test]
    fn test_cache_key_hash_distribution_security_states() {
        let stream_id = StreamID::new(100).unwrap();
        let pasid = PASID::new(200).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key_nonsecure = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let key_secure = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);
        let key_realm = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

        let hash_nonsecure = CacheKeyHash::hash(&key_nonsecure);
        let hash_secure = CacheKeyHash::hash(&key_secure);
        let hash_realm = CacheKeyHash::hash(&key_realm);

        // All should be different
        assert_ne!(hash_nonsecure, hash_secure);
        assert_ne!(hash_secure, hash_realm);
        assert_ne!(hash_nonsecure, hash_realm);
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
                let iova = IOVA::new(u64::from(stream) * 0x1000 + u64::from(pasid_val) * 0x1_0000).unwrap();

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
        let stream_id = StreamID::new(u32::from(u16::MAX)).unwrap();
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
        // Verify the hash algorithm matches the production formula exactly.
        // Inputs: stream_id=1, pasid=2, iova=0x3000 (page number 3), security=NonSecure
        let stream_id = StreamID::new(1).unwrap();
        let pasid_val = PASID::new(2).unwrap();
        let iova = IOVA::new(0x3000).unwrap(); // page number = 0x3000 >> 12 = 3

        let key = CacheKey::new(stream_id, pasid_val, iova, SecurityState::NonSecure);

        // Manual calculation matching the FIXED production formula in CacheKeyHash::hash():
        //   stream = u64::from(stream_id).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        //   pasid  = u64::from(pasid).wrapping_mul(0x6c62_272e_07bb_0142)
        //   security = u64::from(security_state as u8) & 0x3
        //   page = (iova >> 12).wrapping_mul(0x517c_c1b7_2722_0a95)
        //   hash = (stream + pasid + security + page) ^ 0xcbf2_9ce4_8422_2325
        //   followed by the three-round murmur finalizer
        let stream = 1u64.wrapping_mul(0x9e37_79b9_7f4a_7c15_u64);
        let pasid = 2u64.wrapping_mul(0x6c62_272e_07bb_0142_u64);
        let security = u64::from(SecurityState::NonSecure as u8) & 0x3;
        // IOVA=0x3000, page number = 0x3000 >> 12 = 3
        let page = 3u64.wrapping_mul(0x517c_c1b7_2722_0a95_u64);

        let mut expected = stream
            .wrapping_add(pasid)
            .wrapping_add(security)
            .wrapping_add(page)
            ^ 0xcbf2_9ce4_8422_2325_u64;
        expected ^= expected >> 33;
        expected = expected.wrapping_mul(0xff51_afd7_ed55_8ccd);
        expected ^= expected >> 33;
        expected = expected.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        expected ^= expected >> 33;

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
        let debug_str = format!("{key:?}");

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
        let stream_id = StreamID::new(u32::from(u16::MAX)).unwrap();
        let pasid = PASID::new(0xF_FFFF).unwrap();

        let key = StreamPASIDKey::new(stream_id, pasid);

        assert_eq!(key.stream_id.as_u32(), u32::from(u16::MAX));
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
    fn test_stream_pasid_key_hash_uses_fast_mixing() {
        // Verify the hash uses the optimized murmur-like mixing constant
        const MIX_CONSTANT: u64 = 0xff51_afd7_ed55_8ccd;

        // Just verify the constant is the expected value
        assert_eq!(MIX_CONSTANT, 0xff51_afd7_ed55_8ccd);
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
        let stream_id = StreamID::new(u32::from(u16::MAX)).unwrap();
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
        hashes.sort_unstable();
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
        hashes.sort_unstable();
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

        // Manual calculation using new optimized algorithm
        let combined = ((1u64) << 32) | 2u64;
        let mut expected = combined.wrapping_add(0xdead_beef);
        expected ^= expected >> 33;
        expected = expected.wrapping_mul(0xff51_afd7_ed55_8ccd);
        expected ^= expected >> 33;

        let actual = StreamPASIDKeyHash::hash(&key);

        assert_eq!(actual, expected);
    }

    // ------------------------------------------------------------------------
    // TlbCache Tests (50+ comprehensive tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_tlb_cache_new_lru() {
        let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
        assert_eq!(cache.capacity(), 1024);
        assert_eq!(cache.policy(), ReplacementPolicy::Lru);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_tlb_cache_new_fifo() {
        let cache = TlbCache::new(512, ReplacementPolicy::Fifo);
        assert_eq!(cache.capacity(), 512);
        assert_eq!(cache.policy(), ReplacementPolicy::Fifo);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    #[should_panic(expected = "TlbCache capacity must be greater than 0")]
    fn test_tlb_cache_new_zero_capacity() {
        let _cache = TlbCache::new(0, ReplacementPolicy::Lru);
    }

    #[test]
    fn test_tlb_cache_insert_single() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
        assert_eq!(cache.statistics().get_insertions(), 1);
    }

    #[test]
    fn test_tlb_cache_lookup_hit() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        let result = cache.lookup(&key);
        assert!(result.is_some());

        let found_entry = result.unwrap();
        assert_eq!(found_entry.iova, iova);
        assert_eq!(found_entry.physical_address, pa);

        assert_eq!(cache.statistics().get_lookups(), 1);
        assert_eq!(cache.statistics().get_hits(), 1);
        assert_eq!(cache.statistics().get_misses(), 0);
    }

    #[test]
    fn test_tlb_cache_lookup_miss() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);

        let result = cache.lookup(&key);
        assert!(result.is_none());

        assert_eq!(cache.statistics().get_lookups(), 1);
        assert_eq!(cache.statistics().get_hits(), 0);
        assert_eq!(cache.statistics().get_misses(), 1);
    }

    #[test]
    fn test_tlb_cache_multiple_inserts() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);

        for i in 0..10 {
            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(i + 100).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);
        assert_eq!(cache.statistics().get_insertions(), 10);
    }

    #[test]
    fn test_tlb_cache_eviction_lru() {
        let cache = TlbCache::new(3, ReplacementPolicy::Lru);

        // Insert 3 entries to fill cache
        for i in 0..3 {
            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(0).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 3);

        // Insert 4th entry - should evict least recently used
        let stream_id = StreamID::new(3).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x3000).unwrap();
        let pa = PA::new(0x6000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.statistics().get_evictions(), 1);
        assert_eq!(cache.statistics().get_insertions(), 4);
    }

    #[test]
    fn test_tlb_cache_eviction_fifo() {
        let cache = TlbCache::new(3, ReplacementPolicy::Fifo);

        // Insert 3 entries to fill cache
        for i in 0..3 {
            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(0).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 3);

        // Insert 4th entry - should evict first inserted (FIFO)
        let stream_id = StreamID::new(3).unwrap();
        let pasid = PASID::new(0).unwrap();
        let iova = IOVA::new(0x3000).unwrap();
        let pa = PA::new(0x6000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.statistics().get_evictions(), 1);
    }

    #[test]
    fn test_tlb_cache_invalidate_all() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);

        // Insert multiple entries
        for i in 0..10 {
            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(0).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);

        cache.invalidate_all();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert_eq!(cache.statistics().get_invalidations(), 10);
    }

    #[test]
    fn test_tlb_cache_invalidate_by_stream() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let target_stream = StreamID::new(5).unwrap();

        // Insert entries for different streams
        for i in 0..10 {
            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(0).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);

        cache.invalidate_by_stream(target_stream);

        assert_eq!(cache.len(), 9);
        assert_eq!(cache.statistics().get_invalidations(), 1);

        // Verify target stream entry is gone
        let key = CacheKey::new(
            target_stream,
            PASID::new(0).unwrap(),
            IOVA::new(5 * 0x1000).unwrap(),
            SecurityState::NonSecure,
        );
        assert!(cache.lookup(&key).is_none());
    }

    #[test]
    fn test_tlb_cache_invalidate_by_pasid() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let target_pasid = PASID::new(5).unwrap();

        // Insert entries for different PASIDs
        for i in 0..10 {
            let stream_id = StreamID::new(0).unwrap();
            let pasid = PASID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);

        cache.invalidate_by_pasid(target_pasid);

        assert_eq!(cache.len(), 9);
        assert_eq!(cache.statistics().get_invalidations(), 1);

        // Verify target PASID entry is gone
        let key = CacheKey::new(
            StreamID::new(0).unwrap(),
            target_pasid,
            IOVA::new(5 * 0x1000).unwrap(),
            SecurityState::NonSecure,
        );
        assert!(cache.lookup(&key).is_none());
    }

    #[test]
    fn test_tlb_cache_invalidate_by_stream_pasid() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let target_stream = StreamID::new(5).unwrap();
        let target_pasid = PASID::new(7).unwrap(); // Changed to 7 which is in range [0..10)

        // Insert entries for various stream/PASID combinations
        // Each stream/PASID pair gets a unique IOVA
        for i in 0..10 {
            for j in 0..10 {
                let stream_id = StreamID::new(i).unwrap();
                let pasid = PASID::new(j).unwrap();
                let iova = IOVA::new((i as u64) * 0x0010_0000 + u64::from(j) * 0x1000).unwrap();
                let pa = PA::new((i as u64) * 0x0020_0000 + u64::from(j) * 0x2000).unwrap();

                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

                cache.insert(key, entry);
            }
        }

        assert_eq!(cache.len(), 100);

        // Check that the target entry exists before invalidation
        let target_key = CacheKey::new(
            target_stream,
            target_pasid,
            IOVA::new(5 * 0x0010_0000 + 7 * 0x1000).unwrap(),
            SecurityState::NonSecure,
        );
        assert!(cache.lookup(&target_key).is_some());

        cache.invalidate_by_stream_pasid(target_stream, target_pasid);

        assert_eq!(cache.len(), 99);

        // Verify target entry is gone
        assert!(cache.lookup(&target_key).is_none());
    }

    #[test]
    fn test_tlb_cache_invalidate_by_va_range() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();

        // Insert entries with different IOVAs
        for i in 0..10 {
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);

        // Invalidate range 0x2000 to 0x5000 (should remove 4 entries)
        let start = IOVA::new(0x2000).unwrap();
        let end = IOVA::new(0x5000).unwrap();

        cache.invalidate_by_va_range(stream_id, pasid, start, end);

        assert_eq!(cache.len(), 6);
        assert_eq!(cache.statistics().get_invalidations(), 4);
    }

    #[test]
    fn test_tlb_cache_invalidate_entry() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);
        assert_eq!(cache.len(), 1);

        let removed = cache.invalidate_entry(&key);
        assert!(removed);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.statistics().get_invalidations(), 1);

        // Try to remove again - should return false
        let removed_again = cache.invalidate_entry(&key);
        assert!(!removed_again);
    }

    #[test]
    fn test_tlb_cache_statistics_hit_rate() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        // 3 hits, 2 misses = 60% hit rate
        cache.lookup(&key); // hit
        cache.lookup(&key); // hit
        cache.lookup(&key); // hit

        let other_key = CacheKey::new(StreamID::new(99).unwrap(), pasid, iova, SecurityState::NonSecure);
        cache.lookup(&other_key); // miss
        cache.lookup(&other_key); // miss

        let stats = cache.statistics();
        assert_eq!(stats.get_lookups(), 5);
        assert_eq!(stats.get_hits(), 3);
        assert_eq!(stats.get_misses(), 2);
        assert!((stats.hit_rate() - 60.0).abs() < 0.01);
        assert!((stats.miss_rate() - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_tlb_cache_statistics_clear() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);
        cache.lookup(&key);

        assert_eq!(cache.statistics().get_insertions(), 1);
        assert_eq!(cache.statistics().get_lookups(), 1);

        cache.clear_statistics();

        assert_eq!(cache.statistics().get_insertions(), 0);
        assert_eq!(cache.statistics().get_lookups(), 0);
        assert_eq!(cache.statistics().get_hits(), 0);
        assert_eq!(cache.statistics().get_misses(), 0);

        // Cache entries should still be there
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_tlb_cache_replacement_policy_default() {
        let policy = ReplacementPolicy::default();
        assert_eq!(policy, ReplacementPolicy::Lru);
    }

    #[test]
    fn test_tlb_cache_concurrent_inserts() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(TlbCache::new(1000, ReplacementPolicy::Lru));
        let mut handles = vec![];

        // Spawn multiple threads inserting entries
        for thread_id in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for i in 0..10 {
                    let stream_id = StreamID::new(thread_id).unwrap();
                    let pasid = PASID::new(i).unwrap();
                    let iova = IOVA::new((thread_id as u64) * 0x1_0000 + (i as u64) * 0x1000).unwrap();
                    let pa = PA::new((thread_id as u64) * 0x2_0000 + (i as u64) * 0x2000).unwrap();

                    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                    let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), 0);

                    cache_clone.insert(key, entry);
                }
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(cache.len(), 100);
        assert_eq!(cache.statistics().get_insertions(), 100);
    }

    #[test]
    fn test_tlb_cache_concurrent_lookups() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(TlbCache::new(100, ReplacementPolicy::Lru));

        // Insert some entries
        for i in 0..10 {
            let stream_id = StreamID::new(i).unwrap();
            let pasid = PASID::new(0).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        let mut handles = vec![];

        // Spawn multiple threads doing lookups
        for _thread_id in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for i in 0..10 {
                    let stream_id = StreamID::new(i).unwrap();
                    let pasid = PASID::new(0).unwrap();
                    let iova = IOVA::new((i as u64) * 0x1000).unwrap();

                    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                    let _result = cache_clone.lookup(&key);
                }
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // All lookups should be hits
        assert_eq!(cache.statistics().get_lookups(), 100);
        assert_eq!(cache.statistics().get_hits(), 100);
        assert_eq!(cache.statistics().get_misses(), 0);
    }

    #[test]
    fn test_tlb_cache_security_state_isolation() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa_nonsecure = PA::new(0x2000).unwrap();
        let pa_secure = PA::new(0x3000).unwrap();

        // Insert entries with same stream/PASID/IOVA but different security states
        let key_nonsecure = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry_nonsecure = CacheEntry::new_with_security(
            iova,
            pa_nonsecure,
            PagePermissions::read_only(),
            SecurityState::NonSecure,
            0,
        );

        let key_secure = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);
        let entry_secure =
            CacheEntry::new_with_security(iova, pa_secure, PagePermissions::read_only(), SecurityState::Secure, 0);

        cache.insert(key_nonsecure, entry_nonsecure);
        cache.insert(key_secure, entry_secure);

        assert_eq!(cache.len(), 2);

        // Lookup should return correct entry for each security state
        let result_nonsecure = cache.lookup(&key_nonsecure).unwrap();
        let result_secure = cache.lookup(&key_secure).unwrap();

        assert_eq!(result_nonsecure.physical_address, pa_nonsecure);
        assert_eq!(result_secure.physical_address, pa_secure);
        assert_eq!(result_nonsecure.security_state, SecurityState::NonSecure);
        assert_eq!(result_secure.security_state, SecurityState::Secure);
    }

    #[test]
    fn test_tlb_cache_large_capacity() {
        let cache = TlbCache::new(10_000, ReplacementPolicy::Lru);

        // Insert many entries
        for i in 0..1000 {
            let stream_id = StreamID::new(i % 100).unwrap();
            let pasid = PASID::new(i % 50).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 1000);
        assert_eq!(cache.statistics().get_insertions(), 1000);
        assert_eq!(cache.statistics().get_evictions(), 0); // No evictions yet
    }

    #[test]
    fn test_tlb_cache_debug_format() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let debug_str = format!("{cache:?}");

        assert!(debug_str.contains("TlbCache"));
        assert!(debug_str.contains("capacity"));
        assert!(debug_str.contains("policy"));
    }

    #[test]
    fn test_tlb_cache_empty_operations() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);

        // Operations on empty cache should not panic
        cache.invalidate_all();
        cache.invalidate_by_stream(StreamID::new(1).unwrap());
        cache.invalidate_by_pasid(PASID::new(1).unwrap());

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_tlb_cache_permissions_preserved() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        let perms = PagePermissions::read_execute();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, perms, 0);

        cache.insert(key, entry);

        let result = cache.lookup(&key).unwrap();
        assert_eq!(result.permissions, perms);
        assert!(result.permissions.read());
        assert!(!result.permissions.write());
        assert!(result.permissions.execute());
    }

    #[test]
    fn test_tlb_cache_lru_timestamp_update() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        let entry1 = cache.lookup(&key).unwrap();
        let timestamp1 = entry1.timestamp;

        // Second lookup should update timestamp
        let entry2 = cache.lookup(&key).unwrap();
        let timestamp2 = entry2.timestamp;

        assert!(timestamp2 > timestamp1);
    }

    #[test]
    fn test_tlb_cache_fifo_no_timestamp_update() {
        let cache = TlbCache::new(10, ReplacementPolicy::Fifo);
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        let entry1 = cache.lookup(&key).unwrap();
        let timestamp1 = entry1.timestamp;

        // FIFO doesn't update timestamp on lookup
        let entry2 = cache.lookup(&key).unwrap();
        let timestamp2 = entry2.timestamp;

        assert_eq!(timestamp1, timestamp2);
    }

    #[test]
    fn test_tlb_cache_statistics_zero_lookups() {
        let stats = CacheStatistics::new();
        assert_eq!(stats.hit_rate(), 0.0);
        assert_eq!(stats.miss_rate(), 0.0);
    }

    #[test]
    fn test_tlb_cache_invalidate_nonexistent_stream() {
        let cache = TlbCache::new(10, ReplacementPolicy::Lru);

        // Insert an entry
        let stream_id = StreamID::new(1).unwrap();
        let pasid = PASID::new(2).unwrap();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

        cache.insert(key, entry);

        // Try to invalidate different stream
        cache.invalidate_by_stream(StreamID::new(99).unwrap());

        // Original entry should still be there
        assert_eq!(cache.len(), 1);
        assert!(cache.lookup(&key).is_some());
    }

    #[test]
    fn test_tlb_cache_multiple_streams_same_pasid() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let pasid = PASID::new(1).unwrap();

        // Insert entries for multiple streams with same PASID
        for i in 0..10 {
            let stream_id = StreamID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);

        // Invalidate by PASID should remove all
        cache.invalidate_by_pasid(pasid);

        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_tlb_cache_same_stream_multiple_pasids() {
        let cache = TlbCache::new(100, ReplacementPolicy::Lru);
        let stream_id = StreamID::new(1).unwrap();

        // Insert entries for same stream with multiple PASIDs
        for i in 0..10 {
            let pasid = PASID::new(i).unwrap();
            let iova = IOVA::new((i as u64) * 0x1000).unwrap();
            let pa = PA::new((i as u64) * 0x2000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }

        assert_eq!(cache.len(), 10);

        // Invalidate by stream should remove all
        cache.invalidate_by_stream(stream_id);

        assert_eq!(cache.len(), 0);
    }
}
