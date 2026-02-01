//! Iterator APIs Example
//!
//! This example demonstrates the idiomatic Rust iterator-based APIs
//! provided by the ARM SMMU v3 implementation:
//! - Iterating over configured streams
//! - Iterating over active PASIDs for a stream
//! - Iterating over fault records
//! - Iterating over event queue entries
//! - Iterating over page requests
//!
//! Using iterators provides zero-cost abstractions and composability
//! with standard Rust iterator adapters.

use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 Iterator APIs Example ===\n");

    // Setup: Create SMMU and configure multiple streams
    let smmu = SMMU::new();
    println!("Setting up test configuration...\n");

    // Configure 5 streams
    for i in 1..=5 {
        let stream_id = StreamID::new(i)?;
        let config = StreamConfig::builder()
            .stage1_enabled(true)
            .pasid_enabled(i <= 3) // First 3 streams have PASID support
            .build()?;
        smmu.configure_stream(stream_id, config)?;
    }

    // Create PASIDs for streams 1-3
    for stream_num in 1..=3 {
        let stream_id = StreamID::new(stream_num)?;
        for pasid_num in 0..3 {
            smmu.create_pasid(stream_id, PASID::new(pasid_num)?)?;
        }
    }

    println!("✓ Configured 5 streams (3 with PASID support)\n");

    // Example 1: Iterating over configured streams
    println!("Example 1: Stream Iteration\n");

    println!("  All configured streams:");
    for stream_id in smmu.streams() {
        println!("    - Stream ID: {}", stream_id.as_u32());
    }

    // Count streams using iterator
    let stream_count = smmu.streams().count();
    println!("\n  Total streams: {}", stream_count);
    assert_eq!(stream_count, 5);

    // Filter streams
    let high_streams = smmu.streams()
        .filter(|id| id.as_u32() > 3)
        .count();
    println!("  Streams with ID > 3: {}", high_streams);

    // Collect into a vector
    let stream_vec: Vec<StreamID> = smmu.streams().collect();
    println!("  Collected {} streams into vector\n", stream_vec.len());

    // Example 2: Iterating over PASIDs
    println!("Example 2: PASID Iteration\n");

    for stream_num in 1..=3 {
        let stream_id = StreamID::new(stream_num)?;
        println!("  Stream {}:", stream_num);

        if let Some(pasids) = smmu.pasids(stream_id) {
            for pasid in pasids {
                println!("    - PASID: {}", pasid.as_u32());
            }

            // Count PASIDs
            if let Some(pasids) = smmu.pasids(stream_id) {
                let count = pasids.count();
                println!("    Total: {} PASIDs", count);
            }
        }
        println!();
    }

    // PASID-less stream
    let stream_4 = StreamID::new(4)?;
    match smmu.pasids(stream_4) {
        Some(pasids) => {
            let count = pasids.count();
            println!("  Stream 4: {} PASIDs (PASID support disabled)\n", count);
        }
        None => println!("  Stream 4: No PASID context\n"),
    }

    // Example 3: Iterating over faults
    println!("Example 3: Fault Record Iteration\n");

    // Trigger some faults for demonstration
    let test_stream = StreamID::new(1)?;
    let test_pasid = PASID::new(0)?;

    println!("  Triggering test faults...");
    for i in 0..3 {
        let unmapped_iova = IOVA::new(0x10000 * (i + 1))?;
        let _ = smmu.translate(test_stream, test_pasid, unmapped_iova, AccessType::Read);
    }
    println!("  ✓ Triggered 3 translation faults\n");

    // Iterate over all faults
    println!("  All fault records:");
    for (idx, fault) in smmu.faults().enumerate() {
        println!("    {}. Fault type: {:?}, Address: 0x{:x}",
                 idx + 1,
                 fault.fault_type(),
                 fault.address().as_u64());
    }

    // Count faults
    let fault_count = smmu.faults().count();
    println!("\n  Total faults: {}", fault_count);

    // Filter faults by type
    let translation_faults = smmu.faults()
        .filter(|f| f.fault_type() == FaultType::Translation)
        .count();
    println!("  Translation faults: {}", translation_faults);

    // Filter faults by address range
    let high_addr_faults = smmu.faults()
        .filter(|f| f.address().as_u64() > 0x20000)
        .count();
    println!("  Faults at address > 0x20000: {}\n", high_addr_faults);

    // Example 4: Draining iterator (consuming)
    println!("Example 4: Draining Fault Iterator\n");

    println!("  Processing and removing all faults:");
    for fault in smmu.drain_faults() {
        println!("    Handling: {:?} at 0x{:x}",
                 fault.fault_type(),
                 fault.address().as_u64());
    }

    // Verify faults were removed
    let remaining_faults = smmu.faults().count();
    println!("\n  Remaining faults after drain: {}", remaining_faults);
    assert_eq!(remaining_faults, 0);
    println!("  ✓ All faults cleared\n");

    // Example 5: Event iteration
    println!("Example 5: Event Queue Iteration\n");

    // Events would be generated by SMMU operations
    println!("  All events:");
    for (idx, event) in smmu.events().enumerate() {
        println!("    {}. Event type: {:?}, Stream: {}",
                 idx + 1,
                 event.event_type(),
                 event.stream_id());
    }

    let event_count = smmu.events().count();
    println!("  Total events: {}\n", event_count);

    // Filter events by stream
    let stream_1 = StreamID::new(1)?;
    println!("  Events for stream {}:", stream_1.as_u32());
    for event in smmu.events_for_stream(stream_1) {
        println!("    - {:?}", event.event_type());
    }
    println!();

    // Example 6: Page request iteration
    println!("Example 6: Page Request Iteration\n");

    println!("  All page requests:");
    for (idx, request) in smmu.page_requests().enumerate() {
        println!("    {}. Address: 0x{:x}, Stream: {}",
                 idx + 1,
                 request.address(),
                 request.stream_id());
    }

    let request_count = smmu.page_requests().count();
    println!("  Total page requests: {}\n", request_count);

    // Example 7: Combining iterators
    println!("Example 7: Iterator Composition\n");

    // Find all streams with more than 1 PASID
    println!("  Streams with multiple PASIDs:");
    for stream_id in smmu.streams() {
        if let Some(pasids) = smmu.pasids(stream_id) {
            let count = pasids.count();
            if count > 1 {
                println!("    - Stream {}: {} PASIDs", stream_id.as_u32(), count);
            }
        }
    }
    println!();

    // Example 8: Iterator chaining and functional composition
    println!("Example 8: Advanced Iterator Patterns\n");

    // Map-filter-collect pattern
    println!("  Stream IDs as vector (filtered):");
    let filtered_streams: Vec<u32> = smmu.streams()
        .map(|id| id.as_u32())
        .filter(|&id| id % 2 == 1)  // Odd stream IDs only
        .collect();
    println!("    {:?}", filtered_streams);

    // Fold pattern - sum all stream IDs
    let sum: u32 = smmu.streams()
        .map(|id| id.as_u32())
        .fold(0, |acc, id| acc + id);
    println!("\n  Sum of all stream IDs: {}", sum);

    // Any/all patterns
    let has_high_stream = smmu.streams().any(|id| id.as_u32() > 4);
    println!("  Has stream with ID > 4: {}", has_high_stream);

    let all_valid = smmu.streams().all(|id| id.as_u32() <= 5);
    println!("  All streams have ID <= 5: {}", all_valid);

    // Take/skip patterns
    println!("\n  First 3 streams:");
    for stream_id in smmu.streams().take(3) {
        println!("    - {}", stream_id.as_u32());
    }

    println!("\n  Last 2 streams (skip first 3):");
    for stream_id in smmu.streams().skip(3) {
        println!("    - {}", stream_id.as_u32());
    }

    // Example 9: Zero-cost abstraction demonstration
    println!("\nExample 9: Zero-Cost Abstractions\n");

    println!("  Iterator-based code (idiomatic Rust):");
    let count1 = smmu.streams().count();
    println!("    Stream count: {} (via iterator)", count1);

    println!("\n  Direct method (legacy style):");
    let count2 = smmu.get_stream_count();
    println!("    Stream count: {} (via direct call)", count2);

    println!("\n  Both approaches have identical performance!");
    println!("  Iterators compile to the same machine code (zero cost).");

    // Example 10: Best practices
    println!("\n=== Iterator API Best Practices ===");
    println!("1. Use iterators for enumeration (avoid collecting to Vec unless needed)");
    println!("2. Chain iterators for complex queries (map, filter, fold, etc.)");
    println!("3. Use drain_faults() when you want to consume fault queue");
    println!("4. Use faults() for non-destructive iteration");
    println!("5. Iterators are lazy - no work done until consumed");
    println!("6. Prefer iterator chains over manual loops for clarity");
    println!("7. Use take(n) to limit iteration for performance");
    println!("8. Iterators are composable with all std::iter adapters");

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
