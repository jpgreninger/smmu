//! Fault Handling Example
//!
//! This example demonstrates comprehensive fault detection and handling in the
//! ARM SMMU v3, including:
//! - Translation faults (unmapped pages)
//! - Permission faults (access violations)
//! - Fault record inspection
//! - Different fault modes (Terminate vs Stall)
//! - Fault recovery strategies
//!
//! Understanding fault handling is critical for robust system operation.

use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ARM SMMU v3 Fault Handling Example ===\n");

    // Create SMMU instance
    println!("Creating SMMU instance...");
    let smmu = SMMU::new();
    println!("  ✓ SMMU created\n");

    // Configure stream with fault mode = Terminate
    println!("Part 1: Fault Mode = Terminate (abort on fault)\n");
    let stream_id = StreamID::new(1)?;
    let stream_config = StreamConfig::builder()
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()?;

    smmu.configure_stream(stream_id, stream_config)?;
    println!("  ✓ Stream configured with FaultMode::Terminate\n");

    let pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, pasid)?;

    // Set up valid mapping for testing
    let valid_iova = IOVA::new(0x1000)?;
    let valid_pa = PA::new(0x10000)?;
    smmu.map_page(
        stream_id,
        pasid,
        valid_iova,
        valid_pa,
        PagePermissions::read_only(),  // Intentionally read-only for permission fault demo
        SecurityState::NonSecure,
    )?;

    // Fault Type 1: Translation Fault (unmapped page)
    println!("1. Translation Fault - Accessing unmapped page:");
    let unmapped_iova = IOVA::new(0x5000)?;
    match smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read) {
        Ok(_) => println!("  ✗ ERROR: Should have faulted on unmapped page!"),
        Err(TranslationError::Fault(fault)) => {
            println!("  ✓ Translation fault detected");
            println!("    Fault type: {:?}", fault.fault_type());
            println!("    Address: 0x{:x}", fault.address().as_u64());
            println!("    Access type: {:?}", fault.access_type());
            println!("    Stream ID: {}", fault.stream_id().as_u32());
            println!("    PASID: {}", fault.pasid().as_u32());
            println!("    Severity: {:?}", fault.severity());

            // Verify fault details
            assert_eq!(fault.fault_type(), FaultType::Translation);
            assert_eq!(fault.address().as_u64(), unmapped_iova.as_u64());
            assert_eq!(fault.stream_id(), stream_id);
            assert_eq!(fault.pasid(), pasid);
        }
        Err(e) => println!("  ✗ Unexpected error: {}", e),
    }

    // Fault Type 2: Permission Fault (read-only page)
    println!("\n2. Permission Fault - Writing to read-only page:");
    match smmu.translate(stream_id, pasid, valid_iova, AccessType::Write) {
        Ok(_) => println!("  ✗ ERROR: Should have faulted on permission violation!"),
        Err(TranslationError::Fault(fault)) => {
            println!("  ✓ Permission fault detected");
            println!("    Fault type: {:?}", fault.fault_type());
            println!("    Address: 0x{:x}", fault.address().as_u64());
            println!("    Access type: {:?}", fault.access_type());
            println!("    Permission: Read-only, attempted Write");

            assert_eq!(fault.fault_type(), FaultType::Permission);
            assert_eq!(fault.address().as_u64(), valid_iova.as_u64());
            assert_eq!(fault.access_type(), AccessType::Write);
        }
        Err(e) => println!("  ✗ Unexpected error: {}", e),
    }

    // Fault Type 3: Instruction Fetch Fault (execute on non-executable page)
    println!("\n3. Permission Fault - Instruction fetch from non-executable page:");
    match smmu.translate(stream_id, pasid, valid_iova, AccessType::Execute) {
        Ok(_) => println!("  ✗ ERROR: Should have faulted on execute violation!"),
        Err(TranslationError::Fault(fault)) => {
            println!("  ✓ Permission fault detected");
            println!("    Fault type: {:?}", fault.fault_type());
            println!("    Access type: {:?}", fault.access_type());
            println!("    Permission: Non-executable, attempted Execute");

            assert_eq!(fault.fault_type(), FaultType::Permission);
            assert_eq!(fault.access_type(), AccessType::Execute);
        }
        Err(e) => println!("  ✗ Unexpected error: {}", e),
    }

    // Demonstrate fault context information
    println!("\n4. Detailed Fault Context:");
    match smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read) {
        Err(TranslationError::Fault(fault)) => {
            println!("  ✓ Fault context:");
            println!("    Stage: {:?}", fault.translation_stage());
            println!("    Step: {:?}", fault.translation_step());
            println!("    Address type: {:?}", fault.address_type());
            println!("    Security state: {:?}", fault.security_state());

            // Access fault syndrome for low-level debugging
            let syndrome = fault.syndrome();
            println!("\n  Fault syndrome (low-level details):");
            println!("    Syndrome value: 0x{:x}", syndrome.as_u64());
        }
        _ => println!("  ✗ Unexpected result"),
    }

    // Part 2: Fault Mode = Stall
    println!("\n\nPart 2: Fault Mode = Stall (queue fault, continue)\n");

    let stall_stream = StreamID::new(2)?;
    let stall_config = StreamConfig::builder()
        .stage1_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()?;

    smmu.configure_stream(stall_stream, stall_config)?;
    smmu.create_pasid(stall_stream, pasid)?;
    println!("  ✓ Stream configured with FaultMode::Stall\n");

    println!("5. Stall mode - Fault is queued but translation continues:");
    match smmu.translate(stall_stream, pasid, unmapped_iova, AccessType::Read) {
        Ok(_) => println!("  ℹ In stall mode, translation may return partial result"),
        Err(TranslationError::Fault(fault)) => {
            println!("  ✓ Fault recorded in event queue");
            println!("    Mode: Stall");
            println!("    Fault type: {:?}", fault.fault_type());
            println!("    Software can intervene and resolve fault");
        }
        Err(e) => println!("  Error: {}", e),
    }

    // Demonstrate fault recovery
    println!("\n6. Fault Recovery - Map missing page and retry:");

    // Initially, translation will fault
    println!("  a) Initial translation (should fault):");
    let recovery_iova = IOVA::new(0x6000)?;
    match smmu.translate(stream_id, pasid, recovery_iova, AccessType::Read) {
        Ok(_) => println!("     ✗ Should have faulted"),
        Err(TranslationError::Fault(_)) => println!("     ✓ Translation fault as expected"),
        Err(e) => println!("     Error: {}", e),
    }

    // Recover by mapping the page
    println!("  b) Map the page to recover:");
    let recovery_pa = PA::new(0x60000)?;
    smmu.map_page(
        stream_id,
        pasid,
        recovery_iova,
        recovery_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;
    println!("     ✓ Mapped IOVA 0x{:x} -> PA 0x{:x}", recovery_iova.as_u64(), recovery_pa.as_u64());

    // Retry translation - should succeed now
    println!("  c) Retry translation (should succeed):");
    match smmu.translate(stream_id, pasid, recovery_iova, AccessType::Read) {
        Ok(result) => {
            println!("     ✓ Translation succeeded after recovery");
            println!("       IOVA 0x{:x} -> PA 0x{:x}",
                     recovery_iova.as_u64(), result.physical_address().as_u64());
            assert_eq!(result.physical_address().as_u64(), recovery_pa.as_u64());
        }
        Err(e) => println!("     ✗ Translation failed: {}", e),
    }

    // Security state faults
    println!("\n7. Security State Faults:");
    println!("  (Demonstrating secure/non-secure isolation)");

    // Map page in non-secure state
    let secure_iova = IOVA::new(0x7000)?;
    let secure_pa = PA::new(0x70000)?;
    smmu.map_page(
        stream_id,
        pasid,
        secure_iova,
        secure_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Access with correct security state
    let result = smmu.translate(stream_id, pasid, secure_iova, AccessType::Read)?;
    println!("  ✓ Non-secure access to non-secure page succeeded");
    println!("    IOVA 0x{:x} -> PA 0x{:x}", secure_iova.as_u64(), result.physical_address().as_u64());

    // Best practices for fault handling
    println!("\n=== Fault Handling Best Practices ===");
    println!("1. Use FaultMode::Terminate for simple devices (fail-fast)");
    println!("2. Use FaultMode::Stall for advanced scenarios (demand paging, etc.)");
    println!("3. Always check fault records in event queue");
    println!("4. Implement proper fault recovery for stall mode");
    println!("5. Log fault details for debugging and security auditing");
    println!("6. Validate all mappings before enabling devices");
    println!("7. Use security states to isolate trusted vs untrusted memory");

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
