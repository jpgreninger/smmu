// Quick test to profile insertion performance

use smmu::cache::{TlbCache, CacheKey, CacheEntry, ReplacementPolicy};
use smmu::{StreamID, PASID, IOVA, PA, PagePermissions, SecurityState};
use std::time::Instant;

fn main() {
    let cache = TlbCache::new(1024, ReplacementPolicy::Lru);
    let stream_id = StreamID::new(1).unwrap();
    let pasid = PASID::new(0).unwrap();

    println!("Testing insertion performance...");

    // First 1024 insertions (no eviction)
    let start = Instant::now();
    for page in 0..1024 {
        let iova = IOVA::new((page as u64) * 0x1000).unwrap();
        let pa = PA::new((page as u64) * 0x1000 + 0x1_0000).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page as u64);
        cache.insert(key, entry);
    }
    let elapsed = start.elapsed();
    println!("First 1024 insertions (no eviction): {:?} ({} ns/op)",
             elapsed, elapsed.as_nanos() / 1024);

    // Next 1024 insertions (with eviction)
    let start = Instant::now();
    for page in 1024..2048 {
        let iova = IOVA::new((page as u64) * 0x1000).unwrap();
        let pa = PA::new((page as u64) * 0x1000 + 0x1_0000).unwrap();
        let key = CacheKey::new(stream_id, pasid, iova, SecurityState::NonSecure);
        let entry = CacheEntry::new(iova, pa, PagePermissions::read_write(), page as u64);
        cache.insert(key, entry);
    }
    let elapsed = start.elapsed();
    println!("Second 1024 insertions (with eviction): {:?} ({} ns/op)",
             elapsed, elapsed.as_nanos() / 1024);

    println!("\nCache stats: {:?}", cache.statistics());
}
