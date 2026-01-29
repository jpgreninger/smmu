# ARM SMMU v3 Rust API Reference

This document clarifies the correct API signatures and usage patterns for integration tests.

## Core Translation API

### SMMU::translate()

**Signature:**
```rust
pub fn translate(
    &self,
    stream_id: StreamID,
    pasid: PASID,
    iova: IOVA,
    access: AccessType,
) -> TranslationResult
```

**Parameters:**
- `stream_id: StreamID` - Stream identifier
- `pasid: PASID` - Process Address Space ID
- `iova: IOVA` - Input/Output Virtual Address
- `access: AccessType` - Access type (Read/Write/Execute)

**Returns:** `TranslationResult` which is `Result<TranslationData, TranslationError>`

**Note:** This method takes **4 parameters**, NOT 5. SecurityState is determined internally.

### TranslationData Field Access

**TranslationData** fields are **private**. Use accessor methods:

```rust
// WRONG: Direct field access
let pa = result.physical_address;  // ERROR: private field
let perms = result.permissions;    // ERROR: private field

// CORRECT: Use accessor methods
let pa = result.physical_address();      // Returns PA
let perms = result.permissions();        // Returns PagePermissions
let security = result.security_state();  // Returns SecurityState
```

**Accessor Methods:**
```rust
impl TranslationData {
    pub const fn physical_address(&self) -> PA;
    pub const fn permissions(&self) -> PagePermissions;
    pub const fn security_state(&self) -> SecurityState;
}
```

## Address Type Accessors

All address types use `.as_u64()` to get the raw value:

```rust
// StreamID and PASID
let stream_value: u32 = stream_id.as_u32();
let pasid_value: u32 = pasid.as_u32();

// Address types (IOVA, IPA, PA)
let iova_value: u64 = iova.as_u64();
let pa_value: u64 = pa.as_u64();
```

## Event Queue API

### SMMU::get_events()

**Signature:**
```rust
pub fn get_events(&self) -> Vec<EventEntry>
```

**Returns:** `Vec<EventEntry>` directly (NOT a Result type)

**Usage:**
```rust
// CORRECT
let events = smmu.get_events();

// WRONG
let events = smmu.get_events().unwrap();  // ERROR: Vec has no unwrap()
```

### EventEntry Fields

EventEntry stores raw numeric values for simple access:

```rust
pub struct EventEntry {
    pub event_type: EventType,
    pub stream_id: u32,        // Raw u32, not StreamID type
    pub pasid: u32,            // Raw u32, not PASID type
    pub address: u64,          // Raw u64, not IOVA type
    pub security_state: SecurityState,
    pub error_code: u32,
    pub timestamp: u64,
}
```

**Usage:**
```rust
let events = smmu.get_events();
for event in events {
    // Fields are already u32/u64, no conversion needed
    if event.stream_id == stream_id.as_u32() {
        println!("Event for stream {}", event.stream_id);
    }
}
```

## PASID Lifecycle Management

### SMMU::create_pasid()

```rust
pub fn create_pasid(&self, stream_id: StreamID, pasid: PASID) -> Result<(), SMMUError>
```

### SMMU::remove_pasid()

```rust
pub fn remove_pasid(&self, stream_id: StreamID, pasid: PASID) -> Result<(), SMMUError>
```

**Example:**
```rust
// Create PASID
smmu.create_pasid(stream_id, pasid)?;

// Use PASID
smmu.map_page(stream_id, pasid, iova, pa, perms, SecurityState::NonSecure)?;

// Remove PASID
smmu.remove_pasid(stream_id, pasid)?;
```

## Page Mapping API

### SMMU::map_page()

**Signature:**
```rust
pub fn map_page(
    &self,
    stream_id: StreamID,
    pasid: PASID,
    iova: IOVA,
    pa: PA,
    permissions: PagePermissions,
    security_state: SecurityState,
) -> Result<(), SMMUError>
```

**Note:** This method takes **6 parameters** including `security_state`.

## PagePermissions Access

PagePermissions fields are **public**:

```rust
pub struct PagePermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}
```

**Usage:**
```rust
let perms = result.permissions();
assert!(perms.read);
assert!(!perms.write);
assert!(!perms.execute);
```

## Complete Example

```rust
use smmu::SMMU;
use smmu::types::{StreamID, StreamConfig, PASID, IOVA, PA, PagePermissions, AccessType, SecurityState};

// Create and configure SMMU
let smmu = SMMU::new();
let stream_id = StreamID::new(1).unwrap();
let pasid = PASID::new(0).unwrap();

smmu.configure_stream(stream_id, StreamConfig::stage1_only()).unwrap();
smmu.create_pasid(stream_id, pasid).unwrap();

// Map a page
let iova = IOVA::new(0x1000).unwrap();
let pa = PA::new(0x2000).unwrap();
let perms = PagePermissions::read_write();

smmu.map_page(
    stream_id,
    pasid,
    iova,
    pa,
    perms,
    SecurityState::NonSecure,
).unwrap();

// Translate (4 parameters, NOT 5)
let result = smmu.translate(stream_id, pasid, iova, AccessType::Read);

// Access translation data using methods
assert!(result.is_ok());
let trans_data = result.unwrap();
let physical_addr = trans_data.physical_address();  // Use method, not field
let permissions = trans_data.permissions();         // Use method, not field

assert_eq!(physical_addr.as_u64(), 0x2000);
assert!(permissions.read);
assert!(permissions.write);

// Check events (returns Vec directly, not Result)
let events = smmu.get_events();  // NOT .unwrap()
for event in events {
    println!("Event: stream_id={}", event.stream_id);  // Already u32
}

// Remove PASID
smmu.remove_pasid(stream_id, pasid).unwrap();
```

## Common Mistakes to Avoid

1. **DO NOT** pass SecurityState to `translate()` - it only takes 4 parameters
2. **DO NOT** access TranslationData fields directly - use accessor methods
3. **DO NOT** call `.unwrap()` on `get_events()` - it returns Vec directly
4. **DO NOT** mix address types - use appropriate constructors and conversions
5. **DO USE** `.as_u32()` for StreamID and PASID, `.as_u64()` for addresses

## Migration Guide

If your test code has these patterns, fix them as follows:

```rust
// BEFORE (WRONG):
let result = smmu.translate(stream_id, pasid, iova, AccessType::Read, SecurityState::NonSecure);
let pa = result.unwrap().physical_address.as_u64();

// AFTER (CORRECT):
let result = smmu.translate(stream_id, pasid, iova, AccessType::Read);
let pa = result.unwrap().physical_address().as_u64();
```

```rust
// BEFORE (WRONG):
let events = smmu.get_events().unwrap();

// AFTER (CORRECT):
let events = smmu.get_events();
```

```rust
// BEFORE (WRONG):
if event.stream_id == stream_id.as_u64() {  // EventEntry.stream_id is u32!

// AFTER (CORRECT):
if event.stream_id == stream_id.as_u32() {
```
