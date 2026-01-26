//! Integration tests for TLB cache entry structures
//!
//! This test suite validates the cache entry structures against the C++
//! reference implementation in `/home/jpgreninger/Work/smmu/include/smmu/tlb_cache.h`
//!
//! Tests focus on:
//! - CacheEntry construction and behavior
//! - CacheKey equality and hashing
//! - CacheKeyHash FNV-1a algorithm
//! - StreamPASIDKey operations
//! - HashMap integration
//! - Page alignment optimization

use smmu::cache::{CacheEntry, CacheKey, CacheKeyHash, StreamPASIDKey, StreamPASIDKeyHash};
use smmu::{IOVA, PA, PagePermissions, SecurityState, StreamID, PASID};
use std::collections::HashMap;

// ============================================================================
// CacheEntry Integration Tests
// ============================================================================

#[test]
fn test_cache_entry_full_lifecycle() {
    // Create entry with default security
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();

    let entry = CacheEntry::new(iova, pa, perms, 100);

    // Verify all fields
    assert_eq!(entry.iova.as_u64(), 0x1000);
    assert_eq!(entry.physical_address.as_u64(), 0x2000);
    assert_eq!(entry.permissions, PagePermissions::read_write());
    assert_eq!(entry.security_state, SecurityState::NonSecure);
    assert_eq!(entry.timestamp, 100);
}

#[test]
fn test_cache_entry_security_state_transitions() {
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_only();

    // Create entries with different security states
    let entry_ns = CacheEntry::new_with_security(
        iova, pa, perms, SecurityState::NonSecure, 100
    );
    let entry_s = CacheEntry::new_with_security(
        iova, pa, perms, SecurityState::Secure, 200
    );
    let entry_r = CacheEntry::new_with_security(
        iova, pa, perms, SecurityState::Realm, 300
    );

    // Verify isolation - entries with different security states are different
    assert_ne!(entry_ns, entry_s);
    assert_ne!(entry_s, entry_r);
    assert_ne!(entry_ns, entry_r);
}

#[test]
fn test_cache_entry_permission_combinations() {
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Test all permission combinations
    let perms_combinations = vec![
        PagePermissions::none(),
        PagePermissions::read_only(),
        PagePermissions::write_only(),
        PagePermissions::new(false, false, true), // execute only
        PagePermissions::read_write(),
        PagePermissions::read_execute(),
        PagePermissions::new(false, true, true), // write + execute
        PagePermissions::all(), // all permissions
    ];

    for (idx, perms) in perms_combinations.iter().enumerate() {
        let entry = CacheEntry::new(iova, pa, *perms, idx as u64);
        assert_eq!(entry.permissions, *perms);
        assert_eq!(entry.timestamp, idx as u64);
    }
}

#[test]
fn test_cache_entry_timestamp_ordering() {
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_only();

    // Create entries with sequential timestamps
    let mut entries = Vec::new();
    for ts in 0..100 {
        entries.push(CacheEntry::new(iova, pa, perms, ts));
    }

    // Verify timestamps are preserved
    for (idx, entry) in entries.iter().enumerate() {
        assert_eq!(entry.timestamp, idx as u64);
    }
}

#[test]
fn test_cache_entry_large_address_space() {
    // Test with 48-bit address space
    let iova_48bit = IOVA::new(0xFFFF_FFFF_FFFF).unwrap();
    let pa_48bit = PA::new(0xFFFF_FFFF_FFFF).unwrap();
    let perms = PagePermissions::read_write();

    let entry = CacheEntry::new(iova_48bit, pa_48bit, perms, u64::MAX);

    assert_eq!(entry.iova.as_u64(), 0xFFFF_FFFF_FFFF);
    assert_eq!(entry.physical_address.as_u64(), 0xFFFF_FFFF_FFFF);
    assert_eq!(entry.timestamp, u64::MAX);
}

// ============================================================================
// CacheKey Integration Tests
// ============================================================================

#[test]
fn test_cache_key_unique_indexing() {
    // Create keys with different combinations
    let stream1 = StreamID::new(1).unwrap();
    let stream2 = StreamID::new(2).unwrap();
    let pasid1 = PASID::new(10).unwrap();
    let pasid2 = PASID::new(20).unwrap();
    let iova1 = IOVA::new(0x1000).unwrap();
    let iova2 = IOVA::new(0x2000).unwrap();

    // All keys should be unique
    let key1 = CacheKey::new(stream1, pasid1, iova1, SecurityState::NonSecure);
    let key2 = CacheKey::new(stream2, pasid1, iova1, SecurityState::NonSecure);
    let key3 = CacheKey::new(stream1, pasid2, iova1, SecurityState::NonSecure);
    let key4 = CacheKey::new(stream1, pasid1, iova2, SecurityState::NonSecure);
    let key5 = CacheKey::new(stream1, pasid1, iova1, SecurityState::Secure);

    // All should be different
    assert_ne!(key1, key2);
    assert_ne!(key1, key3);
    assert_ne!(key1, key4);
    assert_ne!(key1, key5);
}

#[test]
fn test_cache_key_hashmap_usage() {
    let mut cache = HashMap::new();

    // Insert multiple entries
    for stream in 0..10 {
        for pasid_val in 0..10 {
            let stream_id = StreamID::new(stream).unwrap();
            let pasid = PASID::new(pasid_val).unwrap();
            let iova = IOVA::new((stream as u64) * 0x1000).unwrap();

            let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
            let pa = PA::new((stream as u64) * 0x2000).unwrap();
            let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

            cache.insert(key, entry);
        }
    }

    // Should have 100 unique entries
    assert_eq!(cache.len(), 100);

    // Verify lookup works
    let lookup_key = CacheKey::new(
        StreamID::new(5).unwrap(),
        PASID::new(7).unwrap(),
        IOVA::new(0x5000).unwrap(),
        SecurityState::NonSecure,
    );

    assert!(cache.contains_key(&lookup_key));
}

#[test]
fn test_cache_key_security_state_isolation() {
    let mut cache = HashMap::new();

    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(2).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    // Insert same translation with different security states
    let key_ns = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
    let key_s = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);
    let key_r = CacheKey::new(stream_id, pasid, iova, SecurityState::Realm);

    let pa_ns = PA::new(0x2000).unwrap();
    let pa_s = PA::new(0x3000).unwrap();
    let pa_r = PA::new(0x4000).unwrap();

    cache.insert(key_ns, CacheEntry::new(iova, pa_ns, PagePermissions::read_only(), 1));
    cache.insert(key_s, CacheEntry::new(iova, pa_s, PagePermissions::read_only(), 2));
    cache.insert(key_r, CacheEntry::new(iova, pa_r, PagePermissions::read_only(), 3));

    // Should have 3 separate entries
    assert_eq!(cache.len(), 3);

    // Each should map to different PA
    assert_eq!(cache.get(&key_ns).unwrap().physical_address.as_u64(), 0x2000);
    assert_eq!(cache.get(&key_s).unwrap().physical_address.as_u64(), 0x3000);
    assert_eq!(cache.get(&key_r).unwrap().physical_address.as_u64(), 0x4000);
}

// ============================================================================
// CacheKeyHash Integration Tests
// ============================================================================

#[test]
fn test_cache_key_hash_page_alignment_optimization() {
    // Keys with same page but different offsets should hash to same value
    let stream_id = StreamID::new(100).unwrap();
    let pasid = PASID::new(200).unwrap();

    // Same page (0x1000), different offsets
    let offsets = vec![0x000, 0x001, 0x100, 0x200, 0x500, 0xFFF];

    let mut hashes = Vec::new();
    for offset in offsets {
        let iova = IOVA::new(0x1000 + offset).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        hashes.push(CacheKeyHash::hash(&key));
    }

    // All should hash to the same value (page-aligned optimization)
    for i in 1..hashes.len() {
        assert_eq!(
            hashes[0], hashes[i],
            "Hash mismatch for offsets within same page"
        );
    }
}

#[test]
fn test_cache_key_hash_different_pages() {
    let stream_id = StreamID::new(100).unwrap();
    let pasid = PASID::new(200).unwrap();

    // Different pages should have different hashes
    let page_addresses = vec![
        0x0000_1000,
        0x0000_2000,
        0x0001_0000,
        0x0010_0000,
        0x1000_0000,
    ];

    let mut hashes = Vec::new();
    for addr in page_addresses {
        let iova = IOVA::new(addr).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        hashes.push(CacheKeyHash::hash(&key));
    }

    // All should be unique
    for i in 0..hashes.len() {
        for j in i + 1..hashes.len() {
            assert_ne!(hashes[i], hashes[j], "Hash collision for different pages");
        }
    }
}

#[test]
fn test_cache_key_hash_distribution_quality() {
    // Test hash distribution across different dimensions
    let mut hashes = std::collections::HashSet::new();

    // Generate diverse keys
    for stream in 0..20 {
        for pasid_val in 0..20 {
            for page in 0..20 {
                let stream_id = StreamID::new(stream).unwrap();
                let pasid = PASID::new(pasid_val).unwrap();
                let iova = IOVA::new(page * 0x1000).unwrap();

                for sec_state in &[SecurityState::NonSecure, SecurityState::Secure, SecurityState::Realm] {
                    let key = CacheKey::new(stream_id, pasid, iova, *sec_state);
                    let hash = CacheKeyHash::hash(&key);

                    assert!(hashes.insert(hash), "Hash collision detected");
                }
            }
        }
    }

    // Should have 20 * 20 * 20 * 3 = 24,000 unique hashes
    assert_eq!(hashes.len(), 20 * 20 * 20 * 3);
}

#[test]
fn test_cache_key_hash_fnv1a_correctness() {
    // Test against manual FNV-1a calculation
    let stream_id = StreamID::new(42).unwrap();
    let pasid = PASID::new(100).unwrap();
    let iova = IOVA::new(0x5000).unwrap(); // Page number 5

    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::Secure);

    // Manual FNV-1a calculation
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let mut hash = FNV_OFFSET_BASIS;

    // Hash stream_id
    hash ^= 42u64;
    hash = hash.wrapping_mul(FNV_PRIME);

    // Hash pasid
    hash ^= 100u64;
    hash = hash.wrapping_mul(FNV_PRIME);

    // Hash page number lower 32 bits
    hash ^= 5u64;
    hash = hash.wrapping_mul(FNV_PRIME);

    // Hash page number upper 32 bits
    hash ^= 0u64;
    hash = hash.wrapping_mul(FNV_PRIME);

    // Hash security state
    hash ^= SecurityState::Secure as u64;
    hash = hash.wrapping_mul(FNV_PRIME);

    assert_eq!(CacheKeyHash::hash(&key), hash);
}

#[test]
fn test_cache_key_hash_with_hashmap_custom_hasher() {
    // Use custom hasher with HashMap
    use std::collections::HashMap;
    use std::hash::{BuildHasher, Hasher};

    // Create a custom BuildHasher for CacheKeyHash
    struct CacheKeyBuildHasher;

    impl BuildHasher for CacheKeyBuildHasher {
        type Hasher = CacheKeyHasherWrapper;

        fn build_hasher(&self) -> Self::Hasher {
            CacheKeyHasherWrapper { hash: 0 }
        }
    }

    struct CacheKeyHasherWrapper {
        hash: u64,
    }

    impl Hasher for CacheKeyHasherWrapper {
        fn finish(&self) -> u64 {
            self.hash
        }

        fn write(&mut self, _bytes: &[u8]) {
            // Not used for our specific hash
        }
    }

    // While we can't directly use CacheKeyHash as a Hasher,
    // we can verify it works in a standard HashMap
    let mut cache: HashMap<CacheKey, u64> = HashMap::new();

    let key = CacheKey::new(
        StreamID::new(1).unwrap(),
        PASID::new(2).unwrap(),
        IOVA::new(0x1000).unwrap(),
        SecurityState::NonSecure,
    );

    cache.insert(key, 42);
    assert_eq!(cache.get(&key), Some(&42));
}

// ============================================================================
// StreamPASIDKey Integration Tests
// ============================================================================

#[test]
fn test_stream_pasid_key_secondary_indexing() {
    // Simulate secondary index for invalidation
    let mut secondary_index: HashMap<StreamPASIDKey, Vec<IOVA>> = HashMap::new();

    // Add multiple IOVAs for same stream+PASID
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(2).unwrap();
    let key = StreamPASIDKey::new(stream_id, pasid);

    let iovas = vec![
        IOVA::new(0x1000).unwrap(),
        IOVA::new(0x2000).unwrap(),
        IOVA::new(0x3000).unwrap(),
    ];

    secondary_index.insert(key, iovas.clone());

    // Lookup by stream+PASID
    let found = secondary_index.get(&key).unwrap();
    assert_eq!(found.len(), 3);
    assert_eq!(found, &iovas);
}

#[test]
fn test_stream_pasid_key_invalidation_scenario() {
    // Simulate cache with secondary index for fast invalidation
    let mut main_cache: HashMap<CacheKey, CacheEntry> = HashMap::new();
    let mut stream_pasid_index: HashMap<StreamPASIDKey, Vec<CacheKey>> = HashMap::new();

    // Insert entries
    for stream in 0..5 {
        for pasid_val in 0..5 {
            let stream_id = StreamID::new(stream).unwrap();
            let pasid = PASID::new(pasid_val).unwrap();
            let sp_key = StreamPASIDKey::new(stream_id, pasid);

            let mut cache_keys = Vec::new();

            for page in 0..10 {
                let iova = IOVA::new(page * 0x1000).unwrap();
                let pa = PA::new(page * 0x2000).unwrap();

                let cache_key = CacheKey::new(
                    stream_id, pasid, iova, SecurityState::NonSecure
                );

                main_cache.insert(
                    cache_key,
                    CacheEntry::new(iova, pa, PagePermissions::read_only(), 0),
                );

                cache_keys.push(cache_key);
            }

            stream_pasid_index.insert(sp_key, cache_keys);
        }
    }

    // Should have 5 * 5 * 10 = 250 entries
    assert_eq!(main_cache.len(), 250);

    // Invalidate all entries for stream 2, PASID 3
    let invalidate_key = StreamPASIDKey::new(
        StreamID::new(2).unwrap(),
        PASID::new(3).unwrap(),
    );

    if let Some(keys_to_remove) = stream_pasid_index.remove(&invalidate_key) {
        for key in keys_to_remove {
            main_cache.remove(&key);
        }
    }

    // Should have removed 10 entries
    assert_eq!(main_cache.len(), 240);
}

// ============================================================================
// StreamPASIDKeyHash Integration Tests
// ============================================================================

#[test]
fn test_stream_pasid_key_hash_distribution() {
    let mut hashes = std::collections::HashSet::new();

    for stream in 0..100 {
        for pasid_val in 0..100 {
            let stream_id = StreamID::new(stream).unwrap();
            let pasid = PASID::new(pasid_val).unwrap();
            let key = StreamPASIDKey::new(stream_id, pasid);

            let hash = StreamPASIDKeyHash::hash(&key);
            assert!(hashes.insert(hash), "Hash collision detected");
        }
    }

    // Should have 10,000 unique hashes
    assert_eq!(hashes.len(), 100 * 100);
}

#[test]
fn test_stream_pasid_key_hash_fnv1a_correctness() {
    let stream_id = StreamID::new(10).unwrap();
    let pasid = PASID::new(20).unwrap();
    let key = StreamPASIDKey::new(stream_id, pasid);

    // Manual FNV-1a calculation
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let mut hash = FNV_OFFSET_BASIS;
    hash ^= 10u64;
    hash = hash.wrapping_mul(FNV_PRIME);
    hash ^= 20u64;
    hash = hash.wrapping_mul(FNV_PRIME);

    assert_eq!(StreamPASIDKeyHash::hash(&key), hash);
}

// ============================================================================
// Cross-Structure Integration Tests
// ============================================================================

#[test]
fn test_cache_full_workflow() {
    // Simulate complete cache workflow
    let mut cache: HashMap<CacheKey, CacheEntry> = HashMap::new();

    // 1. Insert translation
    let stream_id = StreamID::new(100).unwrap();
    let pasid = PASID::new(200).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();

    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
    let entry = CacheEntry::new(iova, pa, perms, 100);

    cache.insert(key, entry);

    // 2. Lookup translation
    let lookup_key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
    let cached_entry = cache.get(&lookup_key).unwrap();

    assert_eq!(cached_entry.physical_address.as_u64(), 0x2000);
    assert_eq!(cached_entry.permissions, PagePermissions::read_write());

    // 3. Update timestamp (LRU)
    let updated_entry = CacheEntry::new(iova, pa, perms, 200);
    cache.insert(key, updated_entry);

    let refreshed_entry = cache.get(&lookup_key).unwrap();
    assert_eq!(refreshed_entry.timestamp, 200);

    // 4. Invalidate entry
    cache.remove(&lookup_key);
    assert!(cache.get(&lookup_key).is_none());
}

#[test]
fn test_cache_multi_level_indexing() {
    // Test cache with multiple streams, PASIDs, and security states
    let mut cache: HashMap<CacheKey, CacheEntry> = HashMap::new();

    let streams = vec![
        StreamID::new(1).unwrap(),
        StreamID::new(2).unwrap(),
        StreamID::new(3).unwrap(),
    ];

    let pasids = vec![
        PASID::new(10).unwrap(),
        PASID::new(20).unwrap(),
    ];

    let security_states = vec![
        SecurityState::NonSecure,
        SecurityState::Secure,
    ];

    // Insert entries for all combinations
    for stream_id in &streams {
        for pasid in &pasids {
            for sec_state in &security_states {
                for page in 0..5 {
                    let iova = IOVA::new(page * 0x1000).unwrap();
                    let pa = PA::new(page * 0x2000).unwrap();

                    let key = CacheKey::new(*stream_id, *pasid, iova, *sec_state);
                    let entry = CacheEntry::new_with_security(
                        iova, pa, PagePermissions::read_only(), *sec_state, page
                    );

                    cache.insert(key, entry);
                }
            }
        }
    }

    // Should have 3 * 2 * 2 * 5 = 60 entries
    assert_eq!(cache.len(), 60);

    // Verify specific lookup
    let lookup_key = CacheKey::new(
        StreamID::new(2).unwrap(),
        PASID::new(20).unwrap(),
        IOVA::new(0x2000).unwrap(),
        SecurityState::Secure,
    );

    let entry = cache.get(&lookup_key).unwrap();
    assert_eq!(entry.iova.as_u64(), 0x2000);
    assert_eq!(entry.security_state, SecurityState::Secure);
}

#[test]
fn test_cache_page_offset_preservation() {
    // Ensure page offsets are preserved in cache entries
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(2).unwrap();

    // Non-page-aligned addresses
    let iova = IOVA::new(0x1234).unwrap();
    let pa = PA::new(0x5678).unwrap();

    let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
    let entry = CacheEntry::new(iova, pa, PagePermissions::read_only(), 0);

    let mut cache: HashMap<CacheKey, CacheEntry> = HashMap::new();
    cache.insert(key, entry);

    // Lookup and verify offset preservation
    let found_entry = cache.get(&key).unwrap();
    assert_eq!(found_entry.iova.as_u64(), 0x1234);
    assert_eq!(found_entry.physical_address.as_u64(), 0x5678);
    assert_eq!(found_entry.iova.page_offset(), 0x234);
    assert_eq!(found_entry.physical_address.page_offset(), 0x678);
}

#[test]
fn test_cache_stress_test() {
    // Stress test with large number of entries
    let mut cache: HashMap<CacheKey, CacheEntry> = HashMap::new();

    const NUM_STREAMS: u32 = 100;
    const NUM_PASIDS: u32 = 100;
    const NUM_PAGES: u64 = 10;

    for stream in 0..NUM_STREAMS {
        for pasid_val in 0..NUM_PASIDS {
            for page in 0..NUM_PAGES {
                let stream_id = StreamID::new(stream).unwrap();
                let pasid = PASID::new(pasid_val).unwrap();
                let iova = IOVA::new(page * 0x1000).unwrap();
                let pa = PA::new((stream as u64) * 0x10000 + (pasid_val as u64) * 0x1000 + page * 0x100).unwrap();

                let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
                let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page);

                cache.insert(key, entry);
            }
        }
    }

    // Should have 100 * 100 * 10 = 100,000 entries
    assert_eq!(cache.len(), (NUM_STREAMS as usize) * (NUM_PASIDS as usize) * (NUM_PAGES as usize));

    // Verify random lookups
    let test_key = CacheKey::new(
        StreamID::new(50).unwrap(),
        PASID::new(75).unwrap(),
        IOVA::new(0x5000).unwrap(),
        SecurityState::NonSecure,
    );

    assert!(cache.contains_key(&test_key));
}
