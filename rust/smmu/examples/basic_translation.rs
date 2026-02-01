#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]

//! Basic Translation Example
//!
//! This example demonstrates the simplest use case for the ARM SMMU v3:
//! - Creating an SMMU instance
//! - Configuring a single stream for Stage-1 translation
//! - Mapping pages with permissions
//! - Performing address translation
//!
//! This is the recommended starting point for understanding the SMMU API.

use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 Basic Translation Example ===\n");

    // Step 1: Create SMMU instance with default configuration
    println!("Step 1: Creating SMMU instance...");
    let smmu = SMMU::new();
    println!("  ✓ SMMU created (version: {SMMU_IMPL_VERSION})\n");

    // Step 2: Configure a stream (represents a device)
    println!("Step 2: Configuring stream...");
    let stream_id = StreamID::new(42)?;

    // Create stream configuration for Stage-1 translation only
    let stream_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(256)
        .build()?;

    smmu.configure_stream(stream_id, stream_config)?;
    println!("  ✓ Stream {} configured for Stage-1 translation\n", stream_id.as_u32());

    // Step 3: Create PASID (Process Address Space ID)
    println!("Step 3: Creating PASID...");
    let pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, pasid)?;
    println!("  ✓ PASID {} created for stream {}\n", pasid.as_u32(), stream_id.as_u32());

    // Step 4: Map virtual pages to physical pages
    println!("Step 4: Mapping pages...");

    // Map page at IOVA 0x1000 to PA 0x1_0000 with read/write permissions
    let iova1 = IOVA::new(0x1000)?;
    let pa1 = PA::new(0x1_0000)?;
    smmu.map_page(
        stream_id,
        pasid,
        iova1,
        pa1,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;
    println!("  ✓ Mapped IOVA 0x{:04x} -> PA 0x{:05x} (RW)", iova1.as_u64(), pa1.as_u64());

    // Map page at IOVA 0x2000 to PA 0x2_0000 with read-only permissions
    let iova2 = IOVA::new(0x2000)?;
    let pa2 = PA::new(0x2_0000)?;
    smmu.map_page(
        stream_id,
        pasid,
        iova2,
        pa2,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )?;
    println!("  ✓ Mapped IOVA 0x{:04x} -> PA 0x{:05x} (RO)\n", iova2.as_u64(), pa2.as_u64());

    // Step 5: Perform address translation
    println!("Step 5: Performing translations...");

    // Translate read access to first page
    let result1 = smmu.translate(stream_id, pasid, iova1, AccessType::Read)?;
    println!(
        "  ✓ Translate IOVA 0x{:04x} -> PA 0x{:05x} (Read)",
        iova1.as_u64(),
        result1.physical_address().as_u64()
    );
    assert_eq!(result1.physical_address().as_u64(), pa1.as_u64());

    // Translate write access to first page
    let result2 = smmu.translate(stream_id, pasid, iova1, AccessType::Write)?;
    println!(
        "  ✓ Translate IOVA 0x{:04x} -> PA 0x{:05x} (Write)",
        iova1.as_u64(),
        result2.physical_address().as_u64()
    );
    assert_eq!(result2.physical_address().as_u64(), pa1.as_u64());

    // Translate read access to second page (read-only)
    let result3 = smmu.translate(stream_id, pasid, iova2, AccessType::Read)?;
    println!(
        "  ✓ Translate IOVA 0x{:04x} -> PA 0x{:05x} (Read)",
        iova2.as_u64(),
        result3.physical_address().as_u64()
    );
    assert_eq!(result3.physical_address().as_u64(), pa2.as_u64());

    // Demonstrate permission fault - try to write to read-only page
    println!("\nStep 6: Demonstrating permission fault...");
    match smmu.translate(stream_id, pasid, iova2, AccessType::Write) {
        Ok(_) => println!("  ✗ ERROR: Write to read-only page should have failed!"),
        Err(TranslationError::PermissionViolation { access }) => {
            println!("  ✓ Permission fault detected as expected:");
            println!("    Access type: {access:?}");
            println!("    Address: 0x{:x}", iova2.as_u64());
        },
        Err(e) => println!("  ✗ Unexpected error: {e}"),
    }

    // Demonstrate translation fault - unmapped address
    println!("\nStep 7: Demonstrating translation fault...");
    let unmapped_iova = IOVA::new(0x5000)?;
    match smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read) {
        Ok(_) => println!("  ✗ ERROR: Unmapped address should have failed!"),
        Err(TranslationError::PageNotMapped) => {
            println!("  ✓ Translation fault detected as expected:");
            println!("    Error: Page not mapped");
            println!("    Address: 0x{:x}", unmapped_iova.as_u64());
        },
        Err(e) => println!("  ✗ Unexpected error: {e}"),
    }

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
