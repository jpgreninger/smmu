# ARM SMMU v3 Rust Implementation - User Guide

A comprehensive guide to using the ARM SMMU v3 Rust implementation effectively.

## Table of Contents

- [Getting Started](#getting-started)
- [Common Usage Patterns](#common-usage-patterns)
- [Configuration Guide](#configuration-guide)
- [Error Handling](#error-handling)
- [Performance Tuning](#performance-tuning)
- [Advanced Topics](#advanced-topics)
- [Troubleshooting](#troubleshooting)
- [Best Practices](#best-practices)

## Getting Started

### Installation

Add the SMMU crate to your `Cargo.toml`:

```toml
[dependencies]
smmu = "1.0"
```

For specific features:

```toml
[dependencies]
smmu = { version = "1.0", features = ["serde"] }
```

### Quick Start

The fastest way to get started is using the prelude:

```rust
use smmu::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create SMMU with default configuration
    let smmu = SMMU::new();

    // Configure a device stream
    let stream_id = StreamID::new(1)?;
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .build()?;
    smmu.configure_stream(stream_id, config)?;

    // Create PASID (address space)
    let pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, pasid)?;

    // Map a page
    let iova = IOVA::new(0x1000)?;
    let pa = PA::new(0x10000)?;
    smmu.map_page(
        stream_id,
        pasid,
        iova,
        pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Translate an address
    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read)?;
    println!("Translated: 0x{:x} -> 0x{:x}",
             iova.as_u64(),
             result.physical_address().as_u64());

    Ok(())
}
```

### Core Concepts

#### 1. Streams (Devices)

A **stream** represents a device or logical channel that can access memory.

```rust
// Create a stream ID
let stream_id = StreamID::new(42)?;

// Configure the stream
let config = StreamConfig::builder()
    .stage1_enabled(true)
    .pasid_enabled(true)
    .build()?;

smmu.configure_stream(stream_id, config)?;
```

#### 2. PASIDs (Process Address Spaces)

A **PASID** (Process Address Space ID) provides isolation between different processes using the same device.

```rust
// Create multiple PASIDs for different processes
let process1 = PASID::new(1)?;
let process2 = PASID::new(2)?;

smmu.create_pasid(stream_id, process1)?;
smmu.create_pasid(stream_id, process2)?;

// Each PASID has independent address mappings
smmu.map_page(stream_id, process1, iova, pa1, perms, state)?;
smmu.map_page(stream_id, process2, iova, pa2, perms, state)?;
```

#### 3. Address Types

The SMMU uses strongly-typed addresses to prevent bugs:

- **IOVA** (Input/Output Virtual Address): Virtual address from device
- **IPA** (Intermediate Physical Address): Guest physical (for VMs)
- **PA** (Physical Address): Real physical address

```rust
let iova = IOVA::new(0x1000)?;  // Device virtual
let ipa = IPA::new(0x2000)?;    // Guest physical
let pa = PA::new(0x10000)?;     // Host physical

// Compiler prevents mixing these up!
// smmu.map_page(..., pa, iova, ...)?;  // Won't compile!
```

#### 4. Translation Stages

- **Stage 1**: IOVA → IPA (or IOVA → PA if no Stage 2)
- **Stage 2**: IPA → PA
- **Two-Stage**: IOVA → IPA → PA (for VMs)

```rust
// Stage 1 only (simple)
let config = StreamConfig::builder()
    .stage1_enabled(true)
    .build()?;

// Two-stage translation (VM scenario)
let config = StreamConfig::builder()
    .stage1_enabled(true)
    .stage2_enabled(true)
    .build()?;
```

## Common Usage Patterns

### Pattern 1: Simple Device Translation

**Use Case**: Single device with one address space

```rust
use smmu::prelude::*;

fn setup_simple_device(smmu: &SMMU, device_id: u32) -> Result<(), Box<dyn std::error::Error>> {
    // Configure stream
    let stream_id = StreamID::new(device_id)?;
    let config = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, config)?;

    // Create default PASID
    let pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, pasid)?;

    // Map device memory region
    let device_base = IOVA::new(0x0)?;
    let physical_base = PA::new(0x1_0000_0000)?;
    let region_size = 1024 * 1024; // 1MB

    for offset in (0..region_size).step_by(4096) {
        let iova = IOVA::new(device_base.as_u64() + offset)?;
        let pa = PA::new(physical_base.as_u64() + offset)?;

        smmu.map_page(
            stream_id,
            pasid,
            iova,
            pa,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )?;
    }

    Ok(())
}
```

### Pattern 2: Multi-Process GPU

**Use Case**: GPU shared by multiple processes

```rust
use smmu::prelude::*;

fn setup_shared_gpu(smmu: &SMMU) -> Result<(), Box<dyn std::error::Error>> {
    // Configure GPU stream with PASID support
    let gpu_stream = StreamID::new(10)?;
    let config = StreamConfig::builder()
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(1024)
        .build()?;

    smmu.configure_stream(gpu_stream, config)?;

    // Create PASID for each process
    for process_id in 0..10 {
        let pasid = PASID::new(process_id)?;
        smmu.create_pasid(gpu_stream, pasid)?;

        // Map process-specific memory
        let process_base = IOVA::new(0x10000 * process_id as u64)?;
        let physical_base = PA::new(0x2000_0000 + (0x10_0000 * process_id as u64))?;

        smmu.map_page(
            gpu_stream,
            pasid,
            process_base,
            physical_base,
            PagePermissions::read_write(),
            SecurityState::NonSecure,
        )?;
    }

    Ok(())
}
```

### Pattern 3: Virtual Machine Device Assignment

**Use Case**: Device assigned to VM with nested translation

```rust
use smmu::prelude::*;

fn setup_vm_device(
    smmu: &SMMU,
    device_id: u32,
    vm_id: u32
) -> Result<(), Box<dyn std::error::Error>> {
    // Configure two-stage translation
    let stream_id = StreamID::new(device_id)?;
    let config = StreamConfig::builder()
        .stage1_enabled(true)   // Guest manages Stage 1
        .stage2_enabled(true)   // Hypervisor manages Stage 2
        .pasid_enabled(true)
        .build()?;

    smmu.configure_stream(stream_id, config)?;

    // Create Stage 2 address space (hypervisor)
    smmu.create_stage2_address_space(stream_id)?;

    // Guest VM creates PASID
    let guest_pasid = PASID::new(0)?;
    smmu.create_pasid(stream_id, guest_pasid)?;

    // Guest maps: IOVA → IPA (Stage 1)
    let guest_va = IOVA::new(0x1000)?;
    let guest_pa = PA::new(0x10000)?;  // Guest physical = IPA

    smmu.map_page(
        stream_id,
        guest_pasid,
        guest_va,
        guest_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Hypervisor maps: IPA → PA (Stage 2)
    let guest_ipa = IOVA::new(0x10000)?;  // Same as guest_pa
    let host_pa = PA::new(0x8000_0000 + (0x10_0000 * vm_id as u64))?;

    smmu.map_stage2_page(
        stream_id,
        guest_ipa,
        host_pa,
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )?;

    // Translation: IOVA (0x1000) → IPA (0x10000) → PA (host_pa)
    let result = smmu.translate(stream_id, guest_pasid, guest_va, AccessType::Read)?;
    assert_eq!(result.physical_address().as_u64(), host_pa.as_u64());

    Ok(())
}
```

### Pattern 4: Bypass Mode

**Use Case**: Device that doesn't need translation (identity mapping)

```rust
use smmu::prelude::*;

fn setup_bypass_device(smmu: &SMMU, device_id: u32) -> Result<(), Box<dyn std::error::Error>> {
    let stream_id = StreamID::new(device_id)?;

    // Bypass configuration: IOVA == PA
    let config = StreamConfig::bypass();
    smmu.configure_stream(stream_id, config)?;

    // No mappings needed - addresses pass through unchanged
    let pasid = PASID::new(0)?;
    let addr = IOVA::new(0x5000)?;

    let result = smmu.translate(stream_id, pasid, addr, AccessType::Read)?;
    assert_eq!(result.physical_address().as_u64(), addr.as_u64());

    Ok(())
}
```

### Pattern 5: Fault Handling

**Use Case**: Handling and recovering from translation faults

```rust
use smmu::prelude::*;

fn handle_translation_with_faults(
    smmu: &SMMU,
    stream_id: StreamID,
    pasid: PASID,
    iova: IOVA
) -> Result<PA, Box<dyn std::error::Error>> {
    match smmu.translate(stream_id, pasid, iova, AccessType::Read) {
        Ok(result) => {
            // Success
            Ok(result.physical_address())
        }
        Err(TranslationError::Fault(fault)) => {
            // Handle fault based on type
            match fault.fault_type() {
                FaultType::Translation => {
                    // Page not mapped - could implement demand paging here
                    eprintln!("Translation fault at 0x{:x}", fault.address().as_u64());

                    // Example: Map the page and retry
                    let pa = allocate_physical_page()?;
                    smmu.map_page(
                        stream_id,
                        pasid,
                        iova,
                        pa,
                        PagePermissions::read_write(),
                        SecurityState::NonSecure,
                    )?;

                    // Retry translation
                    let result = smmu.translate(stream_id, pasid, iova, AccessType::Read)?;
                    Ok(result.physical_address())
                }
                FaultType::Permission => {
                    // Permission denied - log and fail
                    eprintln!("Permission fault: {:?} access denied", fault.access_type());
                    Err("Permission denied".into())
                }
                _ => {
                    eprintln!("Other fault: {:?}", fault.fault_type());
                    Err("Translation failed".into())
                }
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn allocate_physical_page() -> Result<PA, Box<dyn std::error::Error>> {
    // Placeholder: allocate from memory pool
    PA::new(0x20000)
}
```

## Configuration Guide

### SMMU Configuration

#### Default Configuration

```rust
let smmu = SMMU::new();  // Uses SMMUConfig::default()
```

#### Custom Configuration

```rust
let config = SMMUConfig::builder()
    .max_streams(2048)
    .cache_config(
        CacheConfig::builder()
            .tlb_size(16384)
            .enable_prefetch(true)
            .build()?
    )
    .queue_config(
        QueueConfig::builder()
            .event_queue_size(2048)
            .command_queue_size(1024)
            .build()?
    )
    .resource_limits(
        ResourceLimits::builder()
            .max_pasids_per_stream(1024)
            .build()?
    )
    .build()?;

let smmu = SMMU::with_config(config);
```

### Stream Configuration

#### Stage 1 Only

```rust
let config = StreamConfig::builder()
    .stage1_enabled(true)
    .build()?;

// Or use convenience method
let config = StreamConfig::stage1_only();
```

#### Two-Stage Translation

```rust
let config = StreamConfig::builder()
    .stage1_enabled(true)
    .stage2_enabled(true)
    .build()?;

// Or use convenience method
let config = StreamConfig::two_stage();
```

#### With PASID Support

```rust
let config = StreamConfig::builder()
    .stage1_enabled(true)
    .pasid_enabled(true)
    .max_pasid(1024)
    .build()?;
```

#### Fault Mode

```rust
// Terminate on fault (default)
let config = StreamConfig::builder()
    .fault_mode(FaultMode::Terminate)
    .build()?;

// Stall on fault (for demand paging)
let config = StreamConfig::builder()
    .fault_mode(FaultMode::Stall)
    .build()?;
```

### Page Permissions

```rust
// Read-only
let perms = PagePermissions::read_only();

// Write-only
let perms = PagePermissions::write_only();

// Read-write
let perms = PagePermissions::read_write();

// Executable (read + execute)
let perms = PagePermissions::executable();

// Custom permissions
let perms = PagePermissions::builder()
    .read(true)
    .write(true)
    .execute(false)
    .build();
```

## Error Handling

### Error Types

The SMMU uses `Result` types for all fallible operations:

```rust
pub enum TranslationError {
    StreamNotConfigured(StreamID),
    PASIDNotFound(PASID),
    Fault(FaultRecord),
    PermissionDenied(AccessType),
    // ...
}

pub enum ValidationError {
    InvalidStreamID(u32),
    InvalidPASID(u32),
    InvalidAddress(u64),
    // ...
}
```

### Error Handling Patterns

#### Pattern 1: Early Return with `?`

```rust
fn configure_device(smmu: &SMMU, id: u32) -> Result<(), Box<dyn std::error::Error>> {
    let stream_id = StreamID::new(id)?;  // Returns error if invalid
    let config = StreamConfig::stage1_only();
    smmu.configure_stream(stream_id, config)?;  // Propagates error
    Ok(())
}
```

#### Pattern 2: Match on Error Type

```rust
match smmu.translate(stream_id, pasid, iova, access) {
    Ok(result) => println!("Success: 0x{:x}", result.physical_address().as_u64()),
    Err(TranslationError::Fault(fault)) => {
        eprintln!("Fault: {:?}", fault.fault_type());
    }
    Err(TranslationError::StreamNotConfigured(_)) => {
        eprintln!("Stream not configured");
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

#### Pattern 3: Logging Errors

```rust
if let Err(e) = smmu.translate(stream_id, pasid, iova, access) {
    log::error!("Translation failed: {}", e);
    // Handle error...
}
```

### Common Error Scenarios

#### Invalid Stream ID

```rust
// Error: StreamID must be > 0
let invalid = StreamID::new(0);  // Returns Err(ValidationError::InvalidStreamID(0))

// Correct
let valid = StreamID::new(1)?;
```

#### Unmapped Page

```rust
// Error: Page not mapped
let result = smmu.translate(stream_id, pasid, unmapped_iova, AccessType::Read);
// Returns Err(TranslationError::Fault(...))

// Fix: Map the page first
smmu.map_page(stream_id, pasid, iova, pa, perms, state)?;
```

#### PASID Not Created

```rust
// Error: PASID doesn't exist
let result = smmu.translate(stream_id, nonexistent_pasid, iova, access);
// Returns Err(TranslationError::PASIDNotFound(...))

// Fix: Create PASID first
smmu.create_pasid(stream_id, pasid)?;
```

## Performance Tuning

### Cache Configuration

#### High-Performance Systems

```rust
let config = SMMUConfig::builder()
    .cache_config(
        CacheConfig::builder()
            .tlb_size(32768)         // Large TLB
            .enable_prefetch(true)    // Prefetch adjacent pages
            .prefetch_distance(16)    // Aggressive prefetching
            .enable_hugepages(true)   // 2MB/1GB page support
            .build()?
    )
    .build()?;
```

#### Low-Latency Systems

```rust
let config = SMMUConfig::builder()
    .cache_config(
        CacheConfig::builder()
            .tlb_size(16384)
            .enable_locked_entries(true)  // Pin critical translations
            .build()?
    )
    .address_config(
        AddressConfig::builder()
            .enable_fast_path(true)       // Fast path for common cases
            .enable_speculation(true)     // Speculative translation
            .build()?
    )
    .build()?;
```

#### Memory-Constrained Systems

```rust
let config = SMMUConfig::builder()
    .cache_config(
        CacheConfig::builder()
            .tlb_size(1024)               // Small TLB
            .enable_prefetch(false)       // No prefetching
            .enable_compression(true)     // Compress cache entries
            .build()?
    )
    .resource_limits(
        ResourceLimits::builder()
            .enable_sparse_tables(true)   // Sparse allocation
            .build()?
    )
    .build()?;
```

### Monitoring Performance

```rust
// Get translation statistics
let stats = smmu.get_translation_stats();
println!("Total translations: {}", stats.total_translations());
println!("TLB hits: {}", stats.tlb_hits());
println!("TLB misses: {}", stats.tlb_misses());
println!("Hit rate: {:.2}%", stats.tlb_hit_rate() * 100.0);

// Get cache statistics
let cache_stats = smmu.get_cache_stats();
println!("Cache occupancy: {:.1}%", cache_stats.occupancy_percent());
println!("Evictions: {}", cache_stats.evictions());

// Get queue statistics
let queue_stats = smmu.get_queue_stats();
println!("Event queue: {}/{}",
         queue_stats.event_queue_used(),
         queue_stats.event_queue_size());
```

### TLB Invalidation

```rust
// Invalidate entire TLB
smmu.invalidate_tlb()?;

// Invalidate specific stream
smmu.invalidate_stream_tlb(stream_id)?;

// Invalidate specific PASID
smmu.invalidate_pasid_tlb(stream_id, pasid)?;

// Invalidate specific page
smmu.invalidate_page_tlb(stream_id, pasid, iova)?;
```

## Advanced Topics

### Custom Allocators

*(Future feature - placeholder)*

```rust
// Custom allocator for page tables
struct CustomAllocator { /* ... */ }

let config = SMMUConfig::builder()
    .allocator(Box::new(CustomAllocator::new()))
    .build()?;
```

### Iterator-Based APIs

```rust
// Iterate over all configured streams
for stream_id in smmu.streams() {
    println!("Stream: {}", stream_id.as_u32());
}

// Iterate over PASIDs for a stream
if let Some(pasids) = smmu.pasids(stream_id) {
    for pasid in pasids {
        println!("  PASID: {}", pasid.as_u32());
    }
}

// Iterate and process faults
for fault in smmu.drain_faults() {
    handle_fault(fault);
}

// Filter and collect
let high_priority_streams: Vec<_> = smmu.streams()
    .filter(|id| id.as_u32() > 100)
    .collect();
```

### Concurrent Access

```rust
use std::sync::Arc;
use std::thread;

// Share SMMU across threads
let smmu = Arc::new(SMMU::new());

let mut handles = vec![];
for i in 0..4 {
    let smmu_clone = Arc::clone(&smmu);
    let handle = thread::spawn(move || {
        // Each thread can safely access SMMU
        let stream_id = StreamID::new(i + 1).unwrap();
        // ... perform translations ...
    });
    handles.push(handle);
}

for handle in handles {
    handle.join().unwrap();
}
```

### Serialization (with `serde` feature)

```toml
[dependencies]
smmu = { version = "1.0", features = ["serde"] }
```

```rust
use serde_json;

// Serialize configuration
let config = SMMUConfig::default();
let json = serde_json::to_string(&config)?;

// Deserialize
let config: SMMUConfig = serde_json::from_str(&json)?;
```

## Troubleshooting

### Problem: Translation Always Fails

**Symptoms**: All translations return errors

**Checklist**:
1. ✓ Stream configured? `smmu.configure_stream()`
2. ✓ PASID created? `smmu.create_pasid()`
3. ✓ Page mapped? `smmu.map_page()`
4. ✓ Using correct stream ID and PASID?

```rust
// Verify configuration
assert!(smmu.has_stream(stream_id));
// Verify PASID exists
// Verify mappings
```

### Problem: Permission Faults

**Symptoms**: `TranslationError::Fault(FaultType::Permission)`

**Solutions**:
- Check page permissions match access type
- Read access needs `PagePermissions::read_only()` or higher
- Write access needs `PagePermissions::write_*()` or `read_write()`
- Execute access needs `PagePermissions::executable()`

```rust
// For read-write access
let perms = PagePermissions::read_write();
smmu.map_page(stream_id, pasid, iova, pa, perms, state)?;
```

### Problem: Poor Performance

**Symptoms**: Translation latency > 1μs

**Diagnostics**:
```rust
let stats = smmu.get_translation_stats();
if stats.tlb_hit_rate() < 0.90 {
    println!("Low TLB hit rate: {:.2}%", stats.tlb_hit_rate() * 100.0);
    // Increase TLB size or enable prefetching
}
```

**Solutions**:
1. Increase TLB size
2. Enable prefetching for sequential access
3. Use hugepages for large regions
4. Pin frequently accessed translations

### Problem: High Memory Usage

**Symptoms**: Excessive memory consumption

**Solutions**:
1. Use sparse tables
2. Reduce TLB size
3. Enable compression
4. Unmap unused pages

```rust
// Clean up unused mappings
smmu.unmap_page(stream_id, pasid, iova)?;

// Remove unused PASIDs
smmu.remove_pasid(stream_id, pasid)?;
```

## Best Practices

### 1. Always Use the Prelude

```rust
use smmu::prelude::*;  // Gets all common types
```

### 2. Use Builder Patterns

```rust
// Good: Builder pattern with validation
let config = SMMUConfig::builder()
    .max_streams(1024)
    .build()?;

// Avoid: Manual construction (if struct fields were public)
// let config = SMMUConfig { max_streams: 1024, ... };  // No validation
```

### 3. Handle Errors Explicitly

```rust
// Good: Explicit error handling
match smmu.translate(...) {
    Ok(result) => { /* ... */ }
    Err(e) => { /* ... */ }
}

// Avoid: Panicking on errors
let result = smmu.translate(...).unwrap();  // Don't do this in production
```

### 4. Configure Before Mapping

```rust
// Correct order:
smmu.configure_stream(stream_id, config)?;  // 1. Configure
smmu.create_pasid(stream_id, pasid)?;       // 2. Create PASID
smmu.map_page(...)?;                        // 3. Map pages
smmu.translate(...)?;                       // 4. Translate
```

### 5. Clean Up Resources

```rust
// SMMU cleans up automatically when dropped
{
    let smmu = SMMU::new();
    // ... use smmu ...
}  // Automatic cleanup here (RAII)

// Or shutdown explicitly
smmu.shutdown()?;
```

### 6. Monitor Statistics in Production

```rust
// Periodically check performance
let stats = smmu.get_translation_stats();
if stats.tlb_hit_rate() < 0.95 {
    log::warn!("Low TLB hit rate: {:.2}%", stats.tlb_hit_rate() * 100.0);
}
```

### 7. Use Type Safety

```rust
// Types prevent bugs
let iova = IOVA::new(0x1000)?;
let pa = PA::new(0x2000)?;

// Compiler catches this error:
// smmu.map_page(..., pa, iova, ...)?;  // Wrong order - won't compile!
```

### 8. Leverage Iterators

```rust
// Good: Iterator-based (zero-cost)
let count = smmu.streams().count();

// Avoid: Collecting when not needed
let streams: Vec<_> = smmu.streams().collect();  // Unnecessary allocation
let count = streams.len();
```

---

## Summary

This guide covered:
- ✅ Getting started with basic usage
- ✅ Common patterns for different scenarios
- ✅ Configuration options for SMMU and streams
- ✅ Error handling strategies
- ✅ Performance tuning techniques
- ✅ Advanced topics and iterators
- ✅ Troubleshooting common issues
- ✅ Best practices for production use

For more information:
- [API Documentation](https://docs.rs/smmu)
- [Design Documentation](DESIGN.md)
- [Migration Guide](MIGRATION.md)
- [Examples](smmu/examples/)

Happy SMMU programming! 🚀
