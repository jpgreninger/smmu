#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]

//! Multi-Stream Management Example
//!
//! This example demonstrates how to manage multiple device streams simultaneously,
//! which is common in systems with multiple I/O devices:
//! - Configuring multiple streams with different translation modes
//! - Isolating address spaces between streams
//! - Managing per-stream configurations
//! - Demonstrating stream independence
//!
//! Use case: Multiple devices (network card, GPU, storage controller) with
//! independent memory mappings and translation requirements.

use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 Multi-Stream Management Example ===\n");

    // Create SMMU instance
    println!("Creating SMMU instance with capacity for multiple streams...");
    let smmu = SMMU::new();
    println!("  ✓ SMMU created\n");

    // Device 1: Network card - Stage-1 translation enabled
    println!("Configuring Device 1 (Network Card)...");
    let net_stream = StreamID::new(10)?;
    let net_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(false)  // Simple single address space
        .build()?;
    smmu.configure_stream(net_stream, net_config)?;

    let net_pasid = PASID::new(0)?;
    smmu.create_pasid(net_stream, net_pasid)?;

    // Map network buffers: IOVA 0x1_0000-0x1_1000 -> PA 0x10_0000-0x10_1000
    let net_iova = IOVA::new(0x1_0000)?;
    let net_pa = PA::new(0x10_0000)?;
    smmu.map_page(
        net_stream,
        net_pasid,
        net_iova,
        net_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;
    println!("  ✓ Network card configured (Stream ID: {})", net_stream.as_u32());
    println!("    Mapped IOVA 0x{:x} -> PA 0x{:x}\n", net_iova.as_u64(), net_pa.as_u64());

    // Device 2: GPU - Stage-1 translation with PASID support
    println!("Configuring Device 2 (GPU)...");
    let gpu_stream = StreamID::new(20)?;
    let gpu_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)  // Multiple contexts per process
        .max_pasid(256)
        .build()?;
    smmu.configure_stream(gpu_stream, gpu_config)?;

    // Create two PASID contexts for GPU (e.g., two processes sharing GPU)
    let gpu_pasid1 = PASID::new(1)?;
    let gpu_pasid2 = PASID::new(2)?;
    smmu.create_pasid(gpu_stream, gpu_pasid1)?;
    smmu.create_pasid(gpu_stream, gpu_pasid2)?;

    // Map different memory regions for each GPU context
    let gpu_iova = IOVA::new(0x2_0000)?;
    let gpu_pa1 = PA::new(0x20_0000)?;
    let gpu_pa2 = PA::new(0x30_0000)?;

    smmu.map_page(
        gpu_stream,
        gpu_pasid1,
        gpu_iova,
        gpu_pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    smmu.map_page(
        gpu_stream,
        gpu_pasid2,
        gpu_iova,
        gpu_pa2,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    println!("  ✓ GPU configured (Stream ID: {})", gpu_stream.as_u32());
    println!("    PASID 1: IOVA 0x{:x} -> PA 0x{:x}", gpu_iova.as_u64(), gpu_pa1.as_u64());
    println!("    PASID 2: IOVA 0x{:x} -> PA 0x{:x}\n", gpu_iova.as_u64(), gpu_pa2.as_u64());

    // Device 3: Storage controller - Bypass mode (no translation)
    println!("Configuring Device 3 (Storage Controller)...");
    let storage_stream = StreamID::new(30)?;
    let storage_config = StreamConfig::bypass();
    smmu.configure_stream(storage_stream, storage_config)?;
    println!(
        "  ✓ Storage controller configured (Stream ID: {}, Bypass mode)\n",
        storage_stream.as_u32()
    );

    // Demonstrate stream independence
    println!("Demonstrating stream independence...\n");

    // Network card translation
    println!("Network Card Translation:");
    let net_result = smmu.translate(net_stream, net_pasid, net_iova, AccessType::Read)?;
    println!(
        "  Stream {}: IOVA 0x{:x} -> PA 0x{:x}",
        net_stream.as_u32(),
        net_iova.as_u64(),
        net_result.physical_address().as_u64()
    );
    assert_eq!(net_result.physical_address().as_u64(), net_pa.as_u64());

    // GPU translations for different PASIDs
    println!("\nGPU Translations:");
    let gpu_result1 = smmu.translate(gpu_stream, gpu_pasid1, gpu_iova, AccessType::Read)?;
    println!(
        "  Stream {}, PASID {}: IOVA 0x{:x} -> PA 0x{:x}",
        gpu_stream.as_u32(),
        gpu_pasid1.as_u32(),
        gpu_iova.as_u64(),
        gpu_result1.physical_address().as_u64()
    );
    assert_eq!(gpu_result1.physical_address().as_u64(), gpu_pa1.as_u64());

    let gpu_result2 = smmu.translate(gpu_stream, gpu_pasid2, gpu_iova, AccessType::Read)?;
    println!(
        "  Stream {}, PASID {}: IOVA 0x{:x} -> PA 0x{:x}",
        gpu_stream.as_u32(),
        gpu_pasid2.as_u32(),
        gpu_iova.as_u64(),
        gpu_result2.physical_address().as_u64()
    );
    assert_eq!(gpu_result2.physical_address().as_u64(), gpu_pa2.as_u64());

    // Storage controller bypass (IOVA == PA)
    println!("\nStorage Controller Translation (Bypass):");
    let storage_pasid = PASID::new(0)?;
    let storage_iova = IOVA::new(0x50_0000)?;
    let storage_result = smmu.translate(storage_stream, storage_pasid, storage_iova, AccessType::Read)?;
    println!(
        "  Stream {}: IOVA 0x{:x} -> PA 0x{:x} (identity mapping)",
        storage_stream.as_u32(),
        storage_iova.as_u64(),
        storage_result.physical_address().as_u64()
    );
    assert_eq!(storage_result.physical_address().as_u64(), storage_iova.as_u64());

    // Demonstrate isolation: Network can't access GPU memory via same IOVA
    println!("\nDemonstrating stream isolation:");
    let gpu_iova_via_net = smmu.translate(net_stream, net_pasid, gpu_iova, AccessType::Read);
    match gpu_iova_via_net {
        Ok(_) => println!("  ✗ ERROR: Should not be able to access GPU memory from network stream!"),
        Err(TranslationError::PageNotMapped) => {
            println!("  ✓ Stream isolation verified: Network stream cannot access unmapped GPU address");
        },
        Err(e) => println!("  Unexpected error: {e}"),
    }

    // Show statistics
    println!("\n=== Stream Configuration Summary ===");
    println!("Stream {}: Network card - Stage-1 enabled, no PASID", net_stream.as_u32());
    println!(
        "Stream {}: GPU - Stage-1 enabled, PASID support (2 contexts)",
        gpu_stream.as_u32()
    );
    println!("Stream {}: Storage - Bypass mode (identity mapping)", storage_stream.as_u32());

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
