#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::similar_names)]

//! Two-Stage Translation Example
//!
//! This example demonstrates two-stage translation (nested translation),
//! which is essential for:
//! - Virtual machine memory management (guest OS + hypervisor)
//! - Container isolation with nested virtualization
//! - Advanced memory protection scenarios
//!
//! Two-stage translation flow:
//! - Stage 1: IOVA → IPA (guest/VM address space)
//! - Stage 2: IPA → PA (hypervisor/host address space)
//! - Combined: IOVA → IPA → PA

use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 Two-Stage Translation Example ===\n");

    println!("Scenario: Virtual Machine with device assignment\n");
    println!("  Guest OS (VM): Manages IOVA → IPA translation (Stage 1)");
    println!("  Hypervisor: Manages IPA → PA translation (Stage 2)");
    println!("  Result: IOVA → IPA → PA (two-stage walk)\n");

    // Create SMMU instance
    let smmu = SMMU::new();
    println!("  ✓ SMMU created\n");

    // Configure stream for two-stage translation
    println!("Step 1: Configure stream for two-stage translation");
    let stream_id = StreamID::new(1)?;
    let stream_config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .max_pasid(256)
        .build()?;

    smmu.configure_stream(stream_id, stream_config)?;
    println!("  ✓ Stream {} configured: Stage-1 + Stage-2 enabled\n", stream_id.as_u32());

    // Create Stage-2 address space (hypervisor manages this)
    println!("Step 2: Create Stage-2 address space (hypervisor)");
    smmu.create_stage2_address_space(stream_id)?;
    println!("  ✓ Stage-2 address space created\n");

    // Create PASID for VM (guest manages this)
    println!("Step 3: Create PASID for guest VM");
    let vm_pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, vm_pasid)?;
    println!("  ✓ PASID {} created for VM\n", vm_pasid.as_u32());

    // Example 1: Basic two-stage translation
    println!("Example 1: Basic Two-Stage Translation\n");

    // Stage 1 mapping (Guest OS perspective)
    println!("  Stage 1 (Guest OS): Map guest virtual to guest physical");
    let guest_va = IOVA::new(0x1000)?; // Guest virtual address
    let guest_pa = IOVA::new(0x1_0000)?; // Guest physical (= IPA for Stage 2)
    let stage1_pa = PA::new(0x1_0000)?; // Must match IPA

    smmu.map_page(
        stream_id,
        vm_pasid,
        guest_va,
        stage1_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;
    println!("    IOVA 0x{:x} → IPA 0x{:x}", guest_va.as_u64(), guest_pa.as_u64());

    // Stage 2 mapping (Hypervisor perspective)
    println!("  Stage 2 (Hypervisor): Map guest physical to host physical");
    let host_pa = PA::new(0x10_0000)?; // Actual physical address

    smmu.map_stage2_page(
        stream_id,
        guest_pa,
        host_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;
    println!("    IPA 0x{:x} → PA 0x{:x}", guest_pa.as_u64(), host_pa.as_u64());

    // Perform two-stage translation
    println!("\n  Combined Translation:");
    let result = smmu.translate(stream_id, vm_pasid, guest_va, AccessType::Read, SecurityState::NonSecure)?;
    println!(
        "    IOVA 0x{:x} → IPA 0x{:x} → PA 0x{:x}",
        guest_va.as_u64(),
        guest_pa.as_u64(),
        result.physical_address().as_u64()
    );

    assert_eq!(result.physical_address().as_u64(), host_pa.as_u64());
    println!("    ✓ Two-stage translation successful!\n");

    // Example 2: Multiple VMs with isolation
    println!("Example 2: Multiple VMs with Isolation\n");

    // VM 1 (already configured above)
    println!("  VM 1 (PASID 0): Primary guest");

    // VM 2 - Different PASID for different VM
    println!("  VM 2 (PASID 1): Secondary guest");
    let vm2_pasid = PASID::new(1)?;
    smmu.create_pasid(stream_id, vm2_pasid)?;

    // Both VMs can use same guest virtual addresses
    let vm2_guest_va = IOVA::new(0x1000)?; // Same as VM1!
    let vm2_guest_pa = IOVA::new(0x2_0000)?; // Different guest physical
    let vm2_stage1_pa = PA::new(0x2_0000)?;

    smmu.map_page(
        stream_id,
        vm2_pasid,
        vm2_guest_va,
        vm2_stage1_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Hypervisor maps VM2's guest physical to different host physical
    let vm2_host_pa = PA::new(0x20_0000)?;
    smmu.map_stage2_page(
        stream_id,
        vm2_guest_pa,
        vm2_host_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Verify isolation
    println!("\n  Translation comparison (same IOVA, different VMs):");

    let vm1_result = smmu.translate(stream_id, vm_pasid, guest_va, AccessType::Read, SecurityState::NonSecure)?;
    println!(
        "    VM1: IOVA 0x{:x} → PA 0x{:x}",
        guest_va.as_u64(),
        vm1_result.physical_address().as_u64()
    );

    let vm2_result = smmu.translate(stream_id, vm2_pasid, vm2_guest_va, AccessType::Read, SecurityState::NonSecure)?;
    println!(
        "    VM2: IOVA 0x{:x} → PA 0x{:x}",
        vm2_guest_va.as_u64(),
        vm2_result.physical_address().as_u64()
    );

    assert_ne!(vm1_result.physical_address().as_u64(), vm2_result.physical_address().as_u64());
    println!("    ✓ VM isolation verified!\n");

    // Example 3: Stage 2 permission enforcement
    println!("Example 3: Stage 2 Permission Enforcement\n");

    // Guest maps page with RW permissions in Stage 1
    let perm_va = IOVA::new(0x2000)?;
    let perm_ipa = IOVA::new(0x3_0000)?;
    let perm_stage1_pa = PA::new(0x3_0000)?;

    smmu.map_page(
        stream_id,
        vm_pasid,
        perm_va,
        perm_stage1_pa,
        PagePermissions::read_write(), // Guest allows RW
        SecurityState::NonSecure,
    )?;
    println!(
        "  Stage 1: IOVA 0x{:x} → IPA 0x{:x} (RW permissions)",
        perm_va.as_u64(),
        perm_ipa.as_u64()
    );

    // Hypervisor restricts to read-only in Stage 2
    let perm_host_pa = PA::new(0x30_0000)?;
    smmu.map_stage2_page(
        stream_id,
        perm_ipa,
        perm_host_pa,
        PagePermissions::read_only(), // Hypervisor restricts to RO
        SecurityState::NonSecure,
    )?;
    println!(
        "  Stage 2: IPA 0x{:x} → PA 0x{:x} (RO permissions - restricted!)",
        perm_ipa.as_u64(),
        perm_host_pa.as_u64()
    );

    // Read should succeed
    println!("\n  Testing read access:");
    let read_result = smmu.translate(stream_id, vm_pasid, perm_va, AccessType::Read, SecurityState::NonSecure)?;
    println!("    ✓ Read succeeded: PA 0x{:x}", read_result.physical_address().as_u64());

    // Write should fail (Stage 2 restriction)
    println!("  Testing write access:");
    match smmu.translate(stream_id, vm_pasid, perm_va, AccessType::Write, SecurityState::NonSecure) {
        Ok(_) => println!("    ✗ ERROR: Write should have been blocked by Stage 2!"),
        Err(TranslationError::PermissionViolation { access }) => {
            println!("    ✓ Write blocked by Stage 2 permission");
            println!("      Access type: {access:?}");
            assert_eq!(access, AccessType::Write);
        },
        Err(e) => println!("    Unexpected error: {e}"),
    }

    // Example 4: Fault handling in two-stage translation
    println!("\nExample 4: Two-Stage Fault Handling\n");

    // Stage 1 fault - unmapped in guest
    println!("  Stage 1 fault (unmapped in guest):");
    let unmapped_va = IOVA::new(0x5000)?;
    match smmu.translate(stream_id, vm_pasid, unmapped_va, AccessType::Read, SecurityState::NonSecure) {
        Ok(_) => println!("    ✗ Should have faulted"),
        Err(TranslationError::PageNotMapped) => {
            println!("    ✓ Stage 1 translation fault");
            println!("      Error: Page not mapped");
            println!("      Address: 0x{:x}", unmapped_va.as_u64());
        },
        Err(e) => println!("    Error: {e}"),
    }

    // Stage 2 fault - mapped in Stage 1 but not Stage 2
    println!("\n  Stage 2 fault (mapped in guest, not in hypervisor):");
    let stage2_fault_va = IOVA::new(0x6000)?;
    let stage2_fault_ipa = IOVA::new(0x6_0000)?;
    let stage2_fault_stage1_pa = PA::new(0x6_0000)?;

    // Map in Stage 1
    smmu.map_page(
        stream_id,
        vm_pasid,
        stage2_fault_va,
        stage2_fault_stage1_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;
    println!(
        "    Stage 1 mapped: IOVA 0x{:x} → IPA 0x{:x}",
        stage2_fault_va.as_u64(),
        stage2_fault_ipa.as_u64()
    );

    // Don't map in Stage 2 - will cause Stage 2 fault
    match smmu.translate(stream_id, vm_pasid, stage2_fault_va, AccessType::Read, SecurityState::NonSecure) {
        Ok(_) => println!("    ✗ Should have faulted at Stage 2"),
        Err(TranslationError::PageNotMapped) => {
            println!("    ✓ Stage 2 translation fault");
            println!("      Error: Page not mapped");
            println!("      Address (IPA): 0x{:x}", stage2_fault_ipa.as_u64());
        },
        Err(e) => println!("    Error: {e}"),
    }

    println!("\n=== Translation Summary ===");
    println!("Two-stage translation enables:");
    println!("  ✓ Virtual machine isolation");
    println!("  ✓ Hypervisor-enforced memory protection");
    println!("  ✓ Independent guest and host address spaces");
    println!("  ✓ Fine-grained permission control");
    println!("\nTranslation flow: IOVA → [Stage 1] → IPA → [Stage 2] → PA");

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
