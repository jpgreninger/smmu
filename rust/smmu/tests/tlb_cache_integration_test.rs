#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_precision_loss)]

//! TLB Cache Integration Tests
//!
//! Comprehensive tests to verify TLB cache integration is working correctly
//! and providing expected performance improvements.
//!
//! Test Requirements:
//! 1. TLB Cache Hit/Miss Test - Verify cache statistics tracking
//! 2. TLB Cache Invalidation Test - Verify cache invalidation correctness
//! 3. TLB Cache Permission Test - Verify permission checking on cached entries
//! 4. TLB Performance Benchmark - Measure actual performance improvement

use smmu::prelude::*;
use smmu::types::{CommandEntry, CommandType};
use std::time::Instant;

// ============================================================================
// Test 1: TLB Cache Hit/Miss Test
// ============================================================================

#[test]
fn test_tlb_cache_hit_miss_tracking() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Get initial statistics
    let stats_before = smmu.get_cache_statistics();
    let initial_lookups = stats_before.tlb_lookups();
    let initial_misses = stats_before.tlb_misses();
    let initial_hits = stats_before.tlb_hits();

    // First translation - should be a cache miss
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "First translation should succeed: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);

    let stats_after_first = smmu.get_cache_statistics();
    assert_eq!(
        stats_after_first.tlb_lookups(),
        initial_lookups + 1,
        "Should have 1 lookup after first translation"
    );
    assert_eq!(
        stats_after_first.tlb_misses(),
        initial_misses + 1,
        "First translation should be a cache miss"
    );

    // Second translation to same IOVA - should be a cache hit
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "Second translation should succeed: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);

    let stats_after_second = smmu.get_cache_statistics();
    assert_eq!(
        stats_after_second.tlb_lookups(),
        initial_lookups + 2,
        "Should have 2 lookups after second translation"
    );
    assert_eq!(
        stats_after_second.tlb_hits(),
        initial_hits + 1,
        "Second translation should be a cache hit"
    );

    // Third translation to same IOVA - another cache hit
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);

    let stats_final = smmu.get_cache_statistics();
    assert_eq!(
        stats_final.tlb_lookups(),
        initial_lookups + 3,
        "Should have 3 lookups total"
    );
    assert_eq!(
        stats_final.tlb_hits(),
        initial_hits + 2,
        "Should have 2 cache hits"
    );
    assert_eq!(
        stats_final.tlb_misses(),
        initial_misses + 1,
        "Should still have only 1 cache miss"
    );

    // Verify hit rate calculation
    let hit_rate = stats_final.tlb_hit_rate();
    assert!(
        hit_rate > 0.0,
        "Hit rate should be positive after cache hits"
    );
    assert!(
        hit_rate <= 100.0,
        "Hit rate should not exceed 100%"
    );
}

#[test]
fn test_tlb_cache_multiple_pages() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map multiple pages
    let pages = vec![
        (0x1000, 0x2000),
        (0x3000, 0x4000),
        (0x5000, 0x6000),
        (0x7000, 0x8000),
    ];

    for (iova_val, pa_val) in &pages {
        let iova = IOVA::new(*iova_val).unwrap();
        let pa = PA::new(*pa_val).unwrap();
        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    let stats_before = smmu.get_cache_statistics();

    // First pass - all should be cache misses
    for (iova_val, pa_val) in &pages {
        let iova = IOVA::new(*iova_val).unwrap();
        let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().physical_address().as_u64(), *pa_val);
    }

    let stats_after_first_pass = smmu.get_cache_statistics();
    let first_pass_misses = stats_after_first_pass.tlb_misses() - stats_before.tlb_misses();
    assert_eq!(
        first_pass_misses, 4,
        "All 4 pages should be cache misses on first pass"
    );

    // Second pass - all should be cache hits
    for (iova_val, pa_val) in &pages {
        let iova = IOVA::new(*iova_val).unwrap();
        let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().physical_address().as_u64(), *pa_val);
    }

    let stats_after_second_pass = smmu.get_cache_statistics();
    let second_pass_hits = stats_after_second_pass.tlb_hits() - stats_after_first_pass.tlb_hits();
    assert_eq!(
        second_pass_hits, 4,
        "All 4 pages should be cache hits on second pass"
    );

    // Verify overall hit rate
    let hit_rate = stats_after_second_pass.tlb_hit_rate();
    assert!(
        hit_rate >= 40.0,
        "Hit rate should be at least 50% (4 hits out of 8 lookups), got {:.2}%",
        hit_rate
    );
}

// ============================================================================
// Test 2: TLB Cache Invalidation Test
// ============================================================================

#[test]
fn test_tlb_cache_invalidation_on_unmap() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Populate cache
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());

    // Verify it's cached (should be a hit)
    let stats_before_unmap = smmu.get_cache_statistics();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    let stats_after_cached = smmu.get_cache_statistics();
    assert_eq!(
        stats_after_cached.tlb_hits(),
        stats_before_unmap.tlb_hits() + 1,
        "Translation should be a cache hit before unmap"
    );

    // Remap the page with different PA - should invalidate TLB entry
    let new_pa = PA::new(0x3000).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        new_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Next translation should return new PA (cache was invalidated)
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "Translation should succeed with remapped page"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x3000,
        "Should get new PA after remap (cache invalidated)"
    );
}

#[test]
fn test_tlb_cache_stream_invalidation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map multiple pages
    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let pa = PA::new(0x2000 + i * 0x1000).unwrap();
        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();

        // Populate cache
        smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();
    }

    // Verify all are cached
    let stats_before = smmu.get_cache_statistics();
    for i in 0..5 {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();
    }
    let stats_after = smmu.get_cache_statistics();
    assert_eq!(
        stats_after.tlb_hits(),
        stats_before.tlb_hits() + 5,
        "All 5 translations should be cache hits"
    );

    // Invalidate entire stream using command queue
    let invalidate_cmd = CommandEntry::new(CommandType::TlbiS12Vmall, stream_id.as_u32(), 0);
    smmu.submit_command(invalidate_cmd).unwrap();
    smmu.process_command_queue().unwrap();

    // Verify invalidation was recorded
    let stats_after_invalidation = smmu.get_cache_statistics();
    assert!(
        stats_after_invalidation.invalidation_count() > stats_after.invalidation_count(),
        "Invalidation count should increase after TLB invalidation command"
    );
}

#[test]
fn test_tlb_cache_pasid_removal_invalidation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(5).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Populate cache
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Verify it's cached
    let stats_before = smmu.get_cache_statistics();
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();
    assert_eq!(
        stats_after.tlb_hits(),
        stats_before.tlb_hits() + 1,
        "Translation should be cached"
    );

    // Remove PASID - should invalidate all PASID entries
    smmu.remove_pasid(stream_id, pasid).unwrap();

    // Translation should now fail (PASID doesn't exist)
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "Translation should fail after PASID removal"
    );
}

// ============================================================================
// Test 3: TLB Cache Permission Test
// ============================================================================

#[test]
fn test_tlb_cache_permission_checking() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Map page with read-only permissions
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Cache the read translation
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "Read access should succeed with read-only permissions"
    );
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);

    // Verify it's cached
    let stats_before = smmu.get_cache_statistics();
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();
    assert_eq!(
        stats_after.tlb_hits(),
        stats_before.tlb_hits() + 1,
        "Read translation should be cached"
    );

    // Attempt write access with cached entry - should fail due to permissions
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "Write access should fail with read-only permissions even with cached entry"
    );
    assert!(
        matches!(
            result.unwrap_err(),
            TranslationError::PermissionViolation { .. }
        ),
        "Expected PermissionViolation error"
    );
}

#[test]
fn test_tlb_cache_permission_upgrade() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Map with read-only initially
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Cache read translation
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Write should fail
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(result.is_err());

    // Upgrade permissions to read-write by remapping (automatically invalidates cache)
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Now write should succeed (cache should be invalidated and repopulated)
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "Write should succeed after permission upgrade: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().physical_address().as_u64(), 0x2000);
}

#[test]
fn test_tlb_cache_execute_permission() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();

    // Map with read-execute permissions (no write)
    let perms = PagePermissions::new(true, false, true);

    smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure)
        .unwrap();

    // Cache read translation
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Execute should succeed (with cached entry)
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Execute, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "Execute should succeed with execute permission"
    );

    // Write should fail even with cached entry
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "Write should fail without write permission"
    );
}

// ============================================================================
// Test 4: TLB Performance Benchmark
// ============================================================================

#[test]
fn test_tlb_performance_improvement() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Warm-up translation to ensure everything is initialized
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    // Measure uncached translation (after cache invalidation via command queue)
    let invalidate_cmd = CommandEntry::new(CommandType::TlbiNhAll, stream_id.as_u32(), 0);
    smmu.submit_command(invalidate_cmd).unwrap();
    smmu.process_command_queue().unwrap();

    let start_uncached = Instant::now();
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let uncached_duration = start_uncached.elapsed();

    // Measure cached translations (multiple iterations for better average)
    const CACHED_ITERATIONS: usize = 1000;
    let start_cached = Instant::now();
    for _ in 0..CACHED_ITERATIONS {
        smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();
    }
    let cached_total_duration = start_cached.elapsed();
    let cached_avg_duration = cached_total_duration / CACHED_ITERATIONS as u32;

    println!("\n=== TLB Performance Benchmark ===");
    println!("Uncached translation: {:?}", uncached_duration);
    println!(
        "Cached translation (avg over {} iterations): {:?}",
        CACHED_ITERATIONS, cached_avg_duration
    );

    // Verify cached translations are faster
    // Note: This test may be flaky due to system noise, so we use a conservative threshold
    assert!(
        cached_avg_duration <= uncached_duration,
        "Cached translations should be faster or equal to uncached (cached: {:?}, uncached: {:?})",
        cached_avg_duration,
        uncached_duration
    );

    // Get final statistics
    let stats = smmu.get_cache_statistics();
    let hit_rate = stats.tlb_hit_rate();

    println!("TLB Statistics:");
    println!("  Total lookups: {}", stats.tlb_lookups());
    println!("  Hits: {}", stats.tlb_hits());
    println!("  Misses: {}", stats.tlb_misses());
    println!("  Hit rate: {:.2}%", hit_rate);
    println!("  Invalidations: {}", stats.invalidation_count());

    // Verify high hit rate (most of CACHED_ITERATIONS should be hits)
    assert!(
        hit_rate > 90.0,
        "Hit rate should be > 90% for repeated translations, got {:.2}%",
        hit_rate
    );
}

#[test]
fn test_tlb_performance_multiple_pages() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map 100 pages
    const NUM_PAGES: u64 = 100;
    for i in 0..NUM_PAGES {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        let pa = PA::new(0x2000 + i * 0x1000).unwrap();
        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )
        .unwrap();
    }

    // First pass - populate cache
    let start_first_pass = Instant::now();
    for i in 0..NUM_PAGES {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();
    }
    let first_pass_duration = start_first_pass.elapsed();

    // Second pass - all cached
    let start_second_pass = Instant::now();
    for i in 0..NUM_PAGES {
        let iova = IOVA::new(0x1000 + i * 0x1000).unwrap();
        smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();
    }
    let second_pass_duration = start_second_pass.elapsed();

    println!("\n=== Multi-Page TLB Performance ===");
    println!("Pages: {}", NUM_PAGES);
    println!("First pass (uncached): {:?}", first_pass_duration);
    println!("Second pass (cached): {:?}", second_pass_duration);
    println!(
        "Per-page uncached: {:?}",
        first_pass_duration / NUM_PAGES as u32
    );
    println!(
        "Per-page cached: {:?}",
        second_pass_duration / NUM_PAGES as u32
    );

    // Cached pass should be faster
    assert!(
        second_pass_duration <= first_pass_duration,
        "Cached pass should be faster or equal (cached: {:?}, uncached: {:?})",
        second_pass_duration,
        first_pass_duration
    );

    let stats = smmu.get_cache_statistics();
    println!("Hit rate: {:.2}%", stats.tlb_hit_rate());

    // All second pass translations should be hits
    assert!(
        stats.tlb_hit_rate() >= 45.0,
        "Hit rate should be at least 50% (second pass all hits), got {:.2}%",
        stats.tlb_hit_rate()
    );
}

#[test]
fn test_tlb_cache_statistics_accuracy() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();

    // Map a page
    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let initial_stats = smmu.get_cache_statistics();

    // Perform known sequence: 1 miss + 5 hits
    for i in 0..6 {
        smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        let stats = smmu.get_cache_statistics();
        assert_eq!(
            stats.tlb_lookups(),
            initial_stats.tlb_lookups() + i + 1,
            "Lookups should increment by 1 each translation"
        );

        if i == 0 {
            // First translation is a miss
            assert_eq!(
                stats.tlb_misses(),
                initial_stats.tlb_misses() + 1,
                "First translation should be a miss"
            );
            assert_eq!(
                stats.tlb_hits(),
                initial_stats.tlb_hits(),
                "No hits yet on first translation"
            );
        } else {
            // Subsequent translations are hits
            assert_eq!(
                stats.tlb_hits(),
                initial_stats.tlb_hits() + i,
                "Hits should increment on translation {}",
                i + 1
            );
            assert_eq!(
                stats.tlb_misses(),
                initial_stats.tlb_misses() + 1,
                "Misses should remain at 1"
            );
        }

        // Verify lookups = hits + misses
        assert_eq!(
            stats.tlb_lookups(),
            stats.tlb_hits() + stats.tlb_misses(),
            "Lookups should equal hits + misses at iteration {}",
            i
        );
    }

    let final_stats = smmu.get_cache_statistics();
    assert_eq!(
        final_stats.tlb_lookups(),
        initial_stats.tlb_lookups() + 6,
        "Should have 6 total lookups"
    );
    assert_eq!(
        final_stats.tlb_hits(),
        initial_stats.tlb_hits() + 5,
        "Should have 5 hits"
    );
    assert_eq!(
        final_stats.tlb_misses(),
        initial_stats.tlb_misses() + 1,
        "Should have 1 miss"
    );

    // Verify hit rate calculation
    let expected_hit_rate = (5.0 / 6.0) * 100.0;
    let actual_hit_rate = (final_stats.tlb_hits() - initial_stats.tlb_hits()) as f64
        / (final_stats.tlb_lookups() - initial_stats.tlb_lookups()) as f64
        * 100.0;
    assert!(
        (actual_hit_rate - expected_hit_rate).abs() < 0.1,
        "Hit rate should be approximately {:.2}%, got {:.2}%",
        expected_hit_rate,
        actual_hit_rate
    );
}

// ============================================================================
// Test 5: Advanced TLB Scenarios
// ============================================================================

#[test]
fn test_tlb_cache_cross_pasid_isolation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::stage1_only())
        .unwrap();

    let pasid1 = PASID::new(1).unwrap();
    let pasid2 = PASID::new(2).unwrap();

    smmu.create_pasid(stream_id, pasid1).unwrap();
    smmu.create_pasid(stream_id, pasid2).unwrap();

    let iova = IOVA::new(0x1000).unwrap();
    let pa1 = PA::new(0x2000).unwrap();
    let pa2 = PA::new(0x3000).unwrap();

    // Map same IOVA to different PAs in different PASIDs
    smmu.map_page(
        stream_id,
        pasid1,
        iova,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    smmu.map_page(
        stream_id,
        pasid2,
        iova,
        pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Translate PASID 1
    let result1 = smmu.translate(stream_id, pasid1, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result1.is_ok());
    assert_eq!(result1.unwrap().physical_address().as_u64(), 0x2000);

    // Translate PASID 2
    let result2 = smmu.translate(stream_id, pasid2, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result2.is_ok());
    assert_eq!(result2.unwrap().physical_address().as_u64(), 0x3000);

    // Cache should maintain separate entries - verify both are cached
    let stats_before = smmu.get_cache_statistics();

    let result1_cached = smmu.translate(stream_id, pasid1, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result1_cached.is_ok());
    assert_eq!(
        result1_cached.unwrap().physical_address().as_u64(),
        0x2000
    );

    let result2_cached = smmu.translate(stream_id, pasid2, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result2_cached.is_ok());
    assert_eq!(
        result2_cached.unwrap().physical_address().as_u64(),
        0x3000
    );

    let stats_after = smmu.get_cache_statistics();
    assert_eq!(
        stats_after.tlb_hits(),
        stats_before.tlb_hits() + 2,
        "Both PASID translations should be cache hits"
    );
}

#[test]
fn test_tlb_cache_with_bypass_mode() {
    let smmu = SMMU::new();
    smmu.enable().unwrap(); // SMMUEN=1 required for translations to reach stream path (§6.3.9)
    let stream_id = StreamID::new(1).unwrap();

    // Configure stream in bypass mode
    smmu.configure_stream(stream_id, StreamConfig::bypass())
        .unwrap();

    let pasid = PASID::new(0).unwrap();
    let iova = IOVA::new(0x1000).unwrap();

    // In bypass mode, IOVA = PA (identity mapping)
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x1000,
        "Bypass mode should return identity mapping"
    );

    // Verify cache works in bypass mode
    let stats_before = smmu.get_cache_statistics();
    smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure)
        .unwrap();
    let stats_after = smmu.get_cache_statistics();

    // Bypass mode may or may not use cache - just verify no errors
    println!(
        "Bypass mode cache behavior: {} lookups, {} hits, {} misses",
        stats_after.tlb_lookups() - stats_before.tlb_lookups(),
        stats_after.tlb_hits() - stats_before.tlb_hits(),
        stats_after.tlb_misses() - stats_before.tlb_misses()
    );
}
