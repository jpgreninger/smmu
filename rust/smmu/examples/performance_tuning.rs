#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]

//! Performance Tuning Example
//!
//! This example demonstrates performance optimization techniques for the ARM SMMU v3:
//! - Cache configuration and sizing
//! - TLB tuning for different workloads
//! - Queue configuration for event handling
//! - Resource limits and capacity planning
//! - Performance monitoring and statistics
//!
//! Understanding these techniques is essential for production deployments.

use smmu::prelude::*;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 Performance Tuning Example ===\n");

    // Example 1: Default Configuration Baseline
    println!("Example 1: Default Configuration (Baseline)\n");
    let start = Instant::now();
    let _smmu_default = SMMU::new();
    let default_init_time = start.elapsed();

    println!("  ✓ Default SMMU initialized in {default_init_time:?}");
    println!("    - Default cache size: 8192 entries");
    println!("    - Default queue sizes: 512 entries");
    println!("    - Default max streams: unlimited\n");

    // Example 2: High-Performance Configuration
    println!("Example 2: High-Performance Configuration\n");
    println!("  Optimized for: High throughput, low latency, many devices");

    let hp_config = SMMUConfig::builder()
        .cache_config(
            CacheConfig::builder()
                .tlb_cache_size(16_384)      // Large TLB for better hit rate
                .build()?,
        )
        .queue_config(
            QueueConfig::builder()
                .event_queue_size(2048)     // Large event queue
                .command_queue_size(1024)   // Large command queue
                .pri_queue_size(512)        // Page request interface queue
                .build()?,
        )
        .build()?;

    let start = Instant::now();
    let smmu_hp = SMMU::with_config(hp_config);
    let hp_init_time = start.elapsed();

    println!("  ✓ High-performance SMMU initialized in {hp_init_time:?}");
    println!("    - TLB size: 16_384 entries (2x default)");
    println!("    - Event queue: 2048 entries (4x default)");
    println!("    - Command queue: 1024 entries");
    println!("    - PRI queue: 512 entries\n");

    // Example 3: Low-Latency Configuration
    println!("Example 3: Low-Latency Configuration\n");
    println!("  Optimized for: Minimal latency, real-time systems");

    let ll_config = SMMUConfig::builder()
        .cache_config(
            CacheConfig::builder()
                .tlb_cache_size(32_768)      // Very large TLB
                .build()?,
        )
        .build()?;

    let _smmu_ll = SMMU::with_config(ll_config);
    println!("  ✓ Low-latency SMMU configured");
    println!("    - TLB size: 32_768 entries (4x default)\n");

    // Example 4: Memory-Constrained Configuration
    println!("Example 4: Memory-Constrained Configuration\n");
    println!("  Optimized for: Minimal memory footprint, embedded systems");

    let mc_config = SMMUConfig::builder()
        .cache_config(
            CacheConfig::builder()
                .tlb_cache_size(1024)       // Small TLB
                .build()?,
        )
        .queue_config(QueueConfig::builder()
                .event_queue_size(128)      // Minimal event queue
                .command_queue_size(64)
                .build()?)
        .build()?;

    let _smmu_mc = SMMU::with_config(mc_config);
    println!("  ✓ Memory-constrained SMMU configured");
    println!("    - TLB size: 1024 entries");
    println!("    - Event queue: 128 entries");
    println!("    - Command queue: 64 entries\n");

    // Example 5: Performance Measurement
    println!("Example 5: Translation Performance Measurement\n");

    let stream_id = StreamID::new(1)?;
    let stream_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()?;

    smmu_hp.configure_stream(stream_id, stream_config)?;

    let pasid = PASID::new(0)?;
    smmu_hp.create_pasid(stream_id, pasid)?;

    // Map pages for benchmarking
    println!("  Mapping 1000 pages...");
    for i in 0..1000 {
        let iova = IOVA::new(0x1000 * i)?;
        let pa = PA::new(0x1_0000 * i)?;
        smmu_hp.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )?;
    }
    println!("  ✓ 1000 pages mapped\n");

    // Measure cold translation (TLB miss)
    println!("  Measuring cold translation (TLB miss):");

    let cold_iova = IOVA::new(0x1000 * 500)?; // Use a mapped page
    let start = Instant::now();
    let _ = smmu_hp.translate(stream_id, pasid, cold_iova, AccessType::Read, SecurityState::NonSecure)?;
    let cold_time = start.elapsed();
    println!("    Cold translation time: {cold_time:?}");

    // Measure hot translation (TLB hit)
    println!("  Measuring hot translation (TLB hit):");
    let start = Instant::now();
    for _ in 0..10_000 {
        let _ = smmu_hp.translate(stream_id, pasid, cold_iova, AccessType::Read, SecurityState::NonSecure)?;
    }
    let hot_time = start.elapsed();
    let avg_hot_time = hot_time / 10_000;
    println!(
        "    Hot translation time: {avg_hot_time:?} avg ({hot_time:?} total for 10k)"
    );

    println!("\n  Performance comparison:");
    println!("    Cold: ~{cold_time:?} (page table walk)");
    println!("    Hot:  ~{avg_hot_time:?} (TLB hit)");
    println!("    Speedup: ~{}x", cold_time.as_nanos() / avg_hot_time.as_nanos().max(1));

    // Example 6: TLB Statistics and Monitoring
    println!("\nExample 6: Performance Statistics\n");

    // Get translation statistics
    let (total, successful, failed) = smmu_hp.get_translation_stats();
    println!("  Translation Statistics:");
    println!("    Total translations: {total}");
    println!("    Successful: {successful}");
    println!("    Failed: {failed}");
    if total > 0 {
        println!("    Success rate: {:.2}%", (successful as f64 / total as f64) * 100.0);
    }

    // Get cache statistics
    let cache_stats = smmu_hp.get_cache_statistics();
    println!("\n  Cache Statistics:");
    println!("    Invalidations: {}", cache_stats.invalidation_count());

    // Get queue statistics
    let queue_stats = smmu_hp.get_queue_statistics();
    println!("\n  Queue Statistics:");
    println!(
        "    Event queue size: {}",
        queue_stats.event_queue_size()
    );
    println!(
        "    Command queue size: {}",
        queue_stats.command_queue_size()
    );

    // Example 7: Workload-Specific Tuning
    println!("\nExample 7: Workload-Specific Recommendations\n");

    println!("  Database Server (Random Access):");
    println!("    - Large TLB (32k+ entries)");
    println!("    - Disable prefetching (no locality)");
    println!("    - Enable hugepages (reduce page table levels)");
    println!("    - Large event queue (many faults)");

    println!("\n  Network Packet Processing (Sequential):");
    println!("    - Medium TLB (8k-16k entries)");
    println!("    - Enable prefetching (good locality)");
    println!("    - Fast path enabled");
    println!("    - Small event queue (few faults)");

    println!("\n  GPU Compute (Large Working Set):");
    println!("    - Very large TLB (64k+ entries)");
    println!("    - Aggressive prefetching (distance 16+)");
    println!("    - Hugepage support mandatory");
    println!("    - Many PASIDs per stream");

    println!("\n  Embedded IoT (Constrained):");
    println!("    - Tiny TLB (512-1k entries)");
    println!("    - No prefetching");
    println!("    - Sparse tables");
    println!("    - Minimal queues");

    println!("\n=== Performance Tuning Best Practices ===");
    println!("1. Profile your workload before tuning");
    println!("2. Monitor TLB hit rate - aim for >95%");
    println!("3. Size TLB to fit working set");
    println!("4. Use hugepages when possible");
    println!("5. Enable prefetching for sequential access");
    println!("6. Disable prefetching for random access");
    println!("7. Use locked TLB entries for hot translations");
    println!("8. Monitor queue fullness - resize if needed");
    println!("9. Test under realistic load conditions");
    println!("10. Validate latency requirements are met");

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
