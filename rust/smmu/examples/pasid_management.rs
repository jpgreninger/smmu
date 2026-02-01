//! PASID Management Example
//!
//! This example demonstrates Process Address Space ID (PASID) management,
//! which allows multiple independent address spaces per stream (device).
//! This is essential for:
//! - Multi-process access to shared devices (e.g., GPU)
//! - Virtual machine isolation
//! - Container-based workloads
//!
//! PASIDs enable fine-grained memory isolation at the sub-device level.

use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 PASID Management Example ===\n");

    // Create SMMU instance
    println!("Creating SMMU instance...");
    let smmu = SMMU::new();
    println!("  ✓ SMMU created\n");

    // Configure stream with PASID support
    println!("Configuring stream with PASID support...");
    let stream_id = StreamID::new(1)?;
    let stream_config = StreamConfig::builder()
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(1024)  // Support up to 1024 PASIDs
        .build()?;

    smmu.configure_stream(stream_id, stream_config)?;
    println!("  ✓ Stream {} configured with PASID support (max PASID: 1024)\n", stream_id.as_u32());

    // Scenario: Three processes sharing a GPU
    println!("Scenario: Three processes sharing a GPU\n");

    // Process 1: Machine learning workload
    println!("Creating PASID for Process 1 (ML workload)...");
    let ml_pasid = PASID::new(10)?;
    smmu.create_pasid(stream_id, ml_pasid)?;

    // Map ML training data buffer
    let ml_data_iova = IOVA::new(0x1000000)?;
    let ml_data_pa = PA::new(0x10000000)?;
    smmu.map_page(
        stream_id,
        ml_pasid,
        ml_data_iova,
        ml_data_pa,
        PagePermissions::read_only(),  // Training data is read-only
        SecurityState::NonSecure,
    )?;

    // Map ML model buffer
    let ml_model_iova = IOVA::new(0x2000000)?;
    let ml_model_pa = PA::new(0x20000000)?;
    smmu.map_page(
        stream_id,
        ml_pasid,
        ml_model_iova,
        ml_model_pa,
        PagePermissions::read_write(),  // Model is updated during training
        SecurityState::NonSecure,
    )?;

    println!("  ✓ PASID {} configured for ML workload", ml_pasid.as_u32());
    println!("    Data buffer:  IOVA 0x{:x} -> PA 0x{:x} (RO)", ml_data_iova.as_u64(), ml_data_pa.as_u64());
    println!("    Model buffer: IOVA 0x{:x} -> PA 0x{:x} (RW)\n", ml_model_iova.as_u64(), ml_model_pa.as_u64());

    // Process 2: Graphics rendering workload
    println!("Creating PASID for Process 2 (Graphics rendering)...");
    let gfx_pasid = PASID::new(20)?;
    smmu.create_pasid(stream_id, gfx_pasid)?;

    // Map framebuffer
    let gfx_fb_iova = IOVA::new(0x1000000)?;  // Same IOVA as ML, different PASID!
    let gfx_fb_pa = PA::new(0x30000000)?;
    smmu.map_page(
        stream_id,
        gfx_pasid,
        gfx_fb_iova,
        gfx_fb_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Map texture buffer
    let gfx_tex_iova = IOVA::new(0x2000000)?;
    let gfx_tex_pa = PA::new(0x40000000)?;
    smmu.map_page(
        stream_id,
        gfx_pasid,
        gfx_tex_iova,
        gfx_tex_pa,
        PagePermissions::read_only(),  // Textures are read-only
        SecurityState::NonSecure,
    )?;

    println!("  ✓ PASID {} configured for graphics workload", gfx_pasid.as_u32());
    println!("    Framebuffer:  IOVA 0x{:x} -> PA 0x{:x} (RW)", gfx_fb_iova.as_u64(), gfx_fb_pa.as_u64());
    println!("    Textures:     IOVA 0x{:x} -> PA 0x{:x} (RO)\n", gfx_tex_iova.as_u64(), gfx_tex_pa.as_u64());

    // Process 3: Video encoding workload
    println!("Creating PASID for Process 3 (Video encoding)...");
    let video_pasid = PASID::new(30)?;
    smmu.create_pasid(stream_id, video_pasid)?;

    // Map video input buffer
    let video_in_iova = IOVA::new(0x3000000)?;
    let video_in_pa = PA::new(0x50000000)?;
    smmu.map_page(
        stream_id,
        video_pasid,
        video_in_iova,
        video_in_pa,
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )?;

    // Map video output buffer
    let video_out_iova = IOVA::new(0x4000000)?;
    let video_out_pa = PA::new(0x60000000)?;
    smmu.map_page(
        stream_id,
        video_pasid,
        video_out_iova,
        video_out_pa,
        PagePermissions::write_only(),
        SecurityState::NonSecure,
    )?;

    println!("  ✓ PASID {} configured for video encoding", video_pasid.as_u32());
    println!("    Input buffer:  IOVA 0x{:x} -> PA 0x{:x} (RO)", video_in_iova.as_u64(), video_in_pa.as_u64());
    println!("    Output buffer: IOVA 0x{:x} -> PA 0x{:x} (WO)\n", video_out_iova.as_u64(), video_out_pa.as_u64());

    // Demonstrate PASID isolation
    println!("Demonstrating PASID isolation...\n");

    // ML process accesses its own data - should succeed
    println!("ML process accessing its data buffer:");
    let ml_result = smmu.translate(stream_id, ml_pasid, ml_data_iova, AccessType::Read)?;
    println!("  ✓ PASID {}: IOVA 0x{:x} -> PA 0x{:x}",
             ml_pasid.as_u32(), ml_data_iova.as_u64(), ml_result.physical_address().as_u64());
    assert_eq!(ml_result.physical_address().as_u64(), ml_data_pa.as_u64());

    // Graphics process accesses same IOVA but gets different PA
    println!("\nGraphics process accessing same IOVA (different PASID):");
    let gfx_result = smmu.translate(stream_id, gfx_pasid, gfx_fb_iova, AccessType::Read)?;
    println!("  ✓ PASID {}: IOVA 0x{:x} -> PA 0x{:x}",
             gfx_pasid.as_u32(), gfx_fb_iova.as_u64(), gfx_result.physical_address().as_u64());
    assert_eq!(gfx_result.physical_address().as_u64(), gfx_fb_pa.as_u64());

    // Verify isolation: Same IOVA, different PASIDs, different PAs
    println!("\n  ℹ Isolation verified: Same IOVA 0x{:x} maps to different PAs:", gfx_fb_iova.as_u64());
    println!("    PASID {}: PA 0x{:x}", ml_pasid.as_u32(), ml_data_pa.as_u64());
    println!("    PASID {}: PA 0x{:x}", gfx_pasid.as_u32(), gfx_fb_pa.as_u64());
    assert_ne!(ml_result.physical_address().as_u64(), gfx_result.physical_address().as_u64());

    // Demonstrate permission enforcement per PASID
    println!("\nDemonstrating per-PASID permission enforcement:");

    // ML can read model data
    println!("  ML process reading model data (RW permission):");
    let ml_read = smmu.translate(stream_id, ml_pasid, ml_model_iova, AccessType::Read)?;
    println!("    ✓ Read succeeded: IOVA 0x{:x} -> PA 0x{:x}",
             ml_model_iova.as_u64(), ml_read.physical_address().as_u64());

    // ML can write model data
    println!("  ML process writing model data (RW permission):");
    let ml_write = smmu.translate(stream_id, ml_pasid, ml_model_iova, AccessType::Write)?;
    println!("    ✓ Write succeeded: IOVA 0x{:x} -> PA 0x{:x}",
             ml_model_iova.as_u64(), ml_write.physical_address().as_u64());

    // Graphics can't write textures (read-only)
    println!("  Graphics process trying to write textures (RO permission):");
    match smmu.translate(stream_id, gfx_pasid, gfx_tex_iova, AccessType::Write) {
        Ok(_) => println!("    ✗ ERROR: Write to read-only texture should have failed!"),
        Err(TranslationError::Fault(fault)) => {
            println!("    ✓ Permission fault as expected: {:?}", fault.fault_type());
        }
        Err(e) => println!("    Unexpected error: {}", e),
    }

    // Video encoder can't read output buffer (write-only)
    println!("  Video encoder trying to read output buffer (WO permission):");
    match smmu.translate(stream_id, video_pasid, video_out_iova, AccessType::Read) {
        Ok(_) => println!("    ✗ ERROR: Read from write-only buffer should have failed!"),
        Err(TranslationError::Fault(fault)) => {
            println!("    ✓ Permission fault as expected: {:?}", fault.fault_type());
        }
        Err(e) => println!("    Unexpected error: {}", e),
    }

    // PASID 0 support - legacy compatibility
    println!("\nDemonstrating PASID 0 (legacy compatibility):");
    let legacy_pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, legacy_pasid)?;

    let legacy_iova = IOVA::new(0x8000000)?;
    let legacy_pa = PA::new(0x80000000)?;
    smmu.map_page(
        stream_id,
        legacy_pasid,
        legacy_iova,
        legacy_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    let legacy_result = smmu.translate(stream_id, legacy_pasid, legacy_iova, AccessType::Read)?;
    println!("  ✓ PASID 0: IOVA 0x{:x} -> PA 0x{:x} (legacy devices)",
             legacy_iova.as_u64(), legacy_result.physical_address().as_u64());

    println!("\n=== PASID Configuration Summary ===");
    println!("Stream {}: 4 PASIDs configured", stream_id.as_u32());
    println!("  PASID {}: ML workload (2 mappings)", ml_pasid.as_u32());
    println!("  PASID {}: Graphics workload (2 mappings)", gfx_pasid.as_u32());
    println!("  PASID {}: Video encoding (2 mappings)", video_pasid.as_u32());
    println!("  PASID {}: Legacy compatibility (1 mapping)", legacy_pasid.as_u32());

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
