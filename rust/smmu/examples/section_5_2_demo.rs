#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]

//! Section 5.2: Translation Engine Demo
//!
//! Demonstrates the main `translate()` API and two-stage translation support.

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, IOVA, PA, PASID};
use smmu::SMMU;

fn main() {
    println!("=== ARM SMMU v3 Section 5.2: Translation Engine ===\n");

    // Create SMMU instance
    let smmu = SMMU::new();
    println!("✓ Created SMMU instance");

    // Configure stream with Stage-1 only translation
    let stream_id = StreamID::new(1).unwrap();
    let config = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, config).unwrap();
    println!("✓ Configured stream {} with Stage-1 only translation", stream_id.as_u32());

    // Create PASID 0 (default/legacy mode per ARM SMMU v3 spec)
    let pasid = PASID::new(0).unwrap();
    smmu.create_pasid(stream_id, pasid).unwrap();
    println!("✓ Created PASID {} (default/legacy mode)", pasid.as_u32());

    // Map pages in virtual address space
    let mappings = vec![
        (0x1000, 0x2000), // Code page
        (0x2000, 0x3000), // Data page
        (0x3000, 0x4000), // Stack page
    ];

    for (iova_val, pa_val) in &mappings {
        let iova = IOVA::new(*iova_val).unwrap();
        let pa = PA::new(*pa_val).unwrap();
        let perms = PagePermissions::read_write();
        smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure)
            .unwrap();
        println!("✓ Mapped IOVA 0x{iova_val:x} → PA 0x{pa_val:x}");
    }

    println!("\n--- Translation Tests ---\n");

    // Test 1: Successful read translation
    let iova = IOVA::new(0x1000).unwrap();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    match result {
        Ok(data) => {
            println!(
                "✓ Read translation: IOVA 0x{:x} → PA 0x{:x}",
                iova.as_u64(),
                data.physical_address().as_u64()
            );
            assert_eq!(data.physical_address().as_u64(), 0x2000);
        },
        Err(e) => panic!("Translation failed: {e}"),
    }

    // Test 2: Successful write translation
    let iova = IOVA::new(0x2000).unwrap();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Write);
    match result {
        Ok(data) => {
            println!(
                "✓ Write translation: IOVA 0x{:x} → PA 0x{:x}",
                iova.as_u64(),
                data.physical_address().as_u64()
            );
            assert_eq!(data.physical_address().as_u64(), 0x3000);
        },
        Err(e) => panic!("Translation failed: {e}"),
    }

    // Test 3: Translation fault (unmapped address)
    let unmapped_iova = IOVA::new(0x5000).unwrap();
    let result = smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read);
    match result {
        Ok(_) => panic!("Translation should have failed for unmapped address"),
        Err(e) => {
            println!("✓ Translation fault detected: {} (IOVA 0x{:x})", e, unmapped_iova.as_u64());
        },
    }

    // Test 4: Stream not found error
    let bad_stream = StreamID::new(99).unwrap();
    let iova = IOVA::new(0x1000).unwrap();
    let result = smmu.translate(bad_stream, pasid, iova, AccessType::Read);
    match result {
        Ok(_) => panic!("Translation should have failed for non-existent stream"),
        Err(e) => {
            println!("✓ Stream not found error: {} (StreamID {})", e, bad_stream.as_u32());
        },
    }

    println!("\n--- Fault Recording ---\n");

    // Check recorded faults
    let faults = smmu.get_faults();
    println!("✓ {} faults recorded:", faults.len());
    for (i, fault) in faults.iter().enumerate() {
        println!(
            "  Fault {}: Type={:?}, IOVA=0x{:x}, StreamID={}",
            i + 1,
            fault.fault_type(),
            fault.address().as_u64(),
            fault.stream_id().as_u32()
        );
    }

    println!("\n--- Translation Statistics ---\n");

    // Get statistics
    let (total, successful, failed) = smmu.get_translation_stats();
    println!("Total translations:      {total}");
    println!("Successful translations: {successful}");
    println!("Failed translations:     {failed}");
    println!("Success rate:            {:.1}%", (successful as f64 / total as f64) * 100.0);

    assert_eq!(total, 4, "Expected 4 total translations");
    assert_eq!(successful, 2, "Expected 2 successful translations");
    assert_eq!(failed, 2, "Expected 2 failed translations");

    println!("\n--- Bypass Mode Test ---\n");

    // Configure second stream with bypass mode
    let stream_id2 = StreamID::new(2).unwrap();
    let bypass_config = StreamConfig::bypass();
    smmu.configure_stream(stream_id2, bypass_config).unwrap();
    println!("✓ Configured stream {} with bypass mode", stream_id2.as_u32());

    // In bypass mode, IOVA = PA (identity mapping)
    let iova = IOVA::new(0x1000).unwrap();
    let result = smmu.translate(stream_id2, pasid, iova, AccessType::Read);
    match result {
        Ok(data) => {
            println!(
                "✓ Bypass translation: IOVA 0x{:x} = PA 0x{:x}",
                iova.as_u64(),
                data.physical_address().as_u64()
            );
            assert_eq!(data.physical_address().as_u64(), iova.as_u64());
        },
        Err(e) => panic!("Bypass translation failed: {e}"),
    }

    println!("\n--- Shutdown ---\n");

    // Graceful shutdown
    smmu.shutdown().unwrap();
    println!("✓ SMMU shutdown complete");

    // Verify operations fail after shutdown
    let iova = IOVA::new(0x1000).unwrap();
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read);
    assert!(result.is_err(), "Translation should fail after shutdown");
    println!("✓ Translation correctly rejected after shutdown");

    println!("\n=== Section 5.2 Translation Engine: All Tests Passed ✅ ===");
}
