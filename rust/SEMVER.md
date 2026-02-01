# Semantic Versioning Policy

This document provides detailed information about the semantic versioning policy for the ARM SMMU v3 Rust implementation.

## Table of Contents

- [Overview](#overview)
- [Version Number Format](#version-number-format)
- [Breaking Changes](#breaking-changes)
- [Non-Breaking Changes](#non-breaking-changes)
- [Deprecation Process](#deprecation-process)
- [MSRV Policy](#msrv-policy)
- [Stability Levels](#stability-levels)
- [Examples](#examples)
- [FAQ](#faq)

## Overview

We follow [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html) strictly for all releases from 1.0.0 onwards.

**Version format**: MAJOR.MINOR.PATCH

- **MAJOR** (x.0.0): Incompatible API changes
- **MINOR** (1.x.0): New features, backward compatible
- **PATCH** (1.0.x): Bug fixes, backward compatible

## Version Number Format

### MAJOR Version (x.0.0)

Increment when you make **incompatible** API changes.

#### Examples of MAJOR changes:

✅ **Removing a public function**
```rust
// v1.0.0
pub fn old_translate(...) -> Result<...> { }

// v2.0.0 - BREAKING
// Function removed entirely
```

✅ **Changing function signature**
```rust
// v1.0.0
pub fn translate(stream_id: StreamID) -> Result<PA> { }

// v2.0.0 - BREAKING
pub fn translate(stream_id: StreamID, pasid: PASID) -> Result<PA> { }
```

✅ **Changing error types**
```rust
// v1.0.0
pub enum Error { Fault, Invalid }

// v2.0.0 - BREAKING
pub enum Error { TranslationFault, PermissionFault, ConfigError }
```

✅ **Changing default behavior**
```rust
// v1.0.0
impl Default for SMMUConfig {
    fn default() -> Self {
        Self { cache_size: 1024 } // Small cache
    }
}

// v2.0.0 - BREAKING
impl Default for SMMUConfig {
    fn default() -> Self {
        Self { cache_size: 16384 } // Larger cache - may affect memory usage
    }
}
```

✅ **Renaming public items**
```rust
// v1.0.0
pub struct StreamID { }

// v2.0.0 - BREAKING
pub struct StreamIdentifier { } // Renamed
```

### MINOR Version (1.x.0)

Increment when you add functionality in a **backward-compatible** manner.

#### Examples of MINOR changes:

✅ **Adding new public functions**
```rust
// v1.0.0
impl SMMU {
    pub fn translate(...) -> Result<...> { }
}

// v1.1.0 - New feature, backward compatible
impl SMMU {
    pub fn translate(...) -> Result<...> { }
    pub fn batch_translate(...) -> Result<Vec<...>> { } // New!
}
```

✅ **Adding trait implementations**
```rust
// v1.0.0
pub struct StreamID { }

// v1.1.0 - New trait, backward compatible
impl serde::Serialize for StreamID { } // New!
impl serde::Deserialize for StreamID { } // New!
```

✅ **Adding new types**
```rust
// v1.1.0 - New type, backward compatible
pub struct CacheStatistics { } // New!
```

✅ **Deprecating APIs** (with migration path)
```rust
// v1.1.0
#[deprecated(since = "1.1.0", note = "use `translate()` instead")]
pub fn old_translate(...) -> Result<...> { }
```

✅ **Relaxing trait bounds**
```rust
// v1.0.0
pub fn process<T: Send + Sync + Clone>(data: T) { }

// v1.1.0 - Relaxed bounds, backward compatible
pub fn process<T: Send + Sync>(data: T) { } // Clone removed
```

### PATCH Version (1.0.x)

Increment when you make **backward-compatible bug fixes**.

#### Examples of PATCH changes:

✅ **Fixing incorrect behavior**
```rust
// v1.0.0 - Bug: wrong permission check
pub fn check_permission(perm: u8) -> bool {
    perm & 0x01 == 0 // Wrong!
}

// v1.0.1 - Fixed
pub fn check_permission(perm: u8) -> bool {
    perm & 0x01 != 0 // Correct
}
```

✅ **Performance improvements**
```rust
// v1.0.0
pub fn translate(...) -> Result<PA> {
    // Slow implementation
}

// v1.0.1 - Faster, but same API
pub fn translate(...) -> Result<PA> {
    // Optimized implementation
}
```

✅ **Documentation fixes**
```rust
// v1.0.1 - Fixed typo in docs
/// Translates an IOVA to a PA (was: "translates an IOVA to a PAS")
pub fn translate(...) -> Result<PA> { }
```

✅ **Internal refactoring**
```rust
// v1.0.1 - Internal changes, public API unchanged
// Refactored internal cache implementation for better performance
```

## Breaking Changes

### What Constitutes a Breaking Change?

1. **API Removals**
   - Removing public functions, methods, structs, enums, traits
   - Removing public fields from structs
   - Removing variants from public enums

2. **API Modifications**
   - Changing function signatures (parameters, return types)
   - Changing struct fields (names, types, visibility)
   - Changing enum variant contents

3. **Behavioral Changes**
   - Changing default values
   - Changing error conditions
   - Changing performance characteristics that users depend on

4. **Trait Bound Changes**
   - Adding new required trait bounds
   - Removing auto-trait implementations (Send, Sync, etc.)

5. **Feature Flag Changes**
   - Removing feature flags
   - Changing feature flag defaults
   - Making previously default features optional

6. **MSRV Increases**
   - Increasing Minimum Supported Rust Version
   - (Treated as minor version bump, but documented prominently)

### How to Avoid Breaking Changes

1. **Use deprecation** instead of immediate removal
2. **Add new APIs** instead of changing existing ones
3. **Use feature flags** for experimental APIs
4. **Version enums carefully** - use `#[non_exhaustive]` for enums that may grow
5. **Hide implementation details** - keep internals private
6. **Use builder patterns** for complex types with many fields

### `#[non_exhaustive]` Annotation

For enums and structs that may gain new variants/fields:

```rust
#[non_exhaustive]
pub enum FaultType {
    Translation,
    Permission,
    // May add more variants in future minor versions
}
```

This allows adding variants in minor versions without breaking existing matches.

## Non-Breaking Changes

### Safe Additions

✅ **New public items** (functions, types, modules)
✅ **New trait implementations**
✅ **Deprecation** with clear migration path
✅ **Documentation improvements**
✅ **Performance optimizations** (same API)
✅ **Bug fixes** that don't change intended behavior
✅ **Internal refactoring** (private APIs)

### Subtle Non-Breaking Changes

✅ **Relaxing trait bounds**
```rust
// Before: fn foo<T: A + B>()
// After:  fn foo<T: A>()      // OK - more permissive
```

✅ **Weakening constraints**
```rust
// Before: fn foo() where X: Copy
// After:  fn foo()               // OK - less restrictive
```

✅ **Adding default trait methods**
```rust
pub trait Translate {
    fn translate(&self, addr: u64) -> Result<u64>;

    // Adding default method is OK in minor version
    fn batch_translate(&self, addrs: &[u64]) -> Result<Vec<u64>> {
        addrs.iter().map(|&a| self.translate(a)).collect()
    }
}
```

## Deprecation Process

### Step 1: Mark as Deprecated (Minor Release)

```rust
#[deprecated(
    since = "1.2.0",
    note = "use `new_translate()` instead. See migration guide in docs."
)]
pub fn old_translate(addr: u64) -> Result<u64> {
    // Still works, but emits warning
    self.new_translate(addr)
}
```

### Step 2: Keep for 2+ Minor Versions

```
v1.2.0 - Deprecate old_translate()
v1.3.0 - Still present (1st minor version)
v1.4.0 - Still present (2nd minor version)
v2.0.0 - Remove old_translate() (next major version)
```

### Step 3: Document in CHANGELOG

```markdown
## [1.2.0] - Deprecation Notice

### Deprecated
- `SMMU::old_translate()` - Use `SMMU::new_translate()` instead
  - Migration: Replace `smmu.old_translate(addr)` with `smmu.new_translate(addr, flags)`
  - Will be removed in v2.0.0
```

### Step 4: Provide Migration Guide

Update documentation with clear examples:

```rust
/// Old way (deprecated):
/// ```rust,ignore
/// let result = smmu.old_translate(addr)?;
/// ```
///
/// New way:
/// ```rust
/// let result = smmu.new_translate(addr, TranslateFlags::default())?;
/// ```
```

## MSRV Policy

### Current MSRV

**Rust 1.75.0** (as of January 2026)

### MSRV Changes

- MSRV increases are **minor version changes** (not major)
- Will be prominently documented in:
  - CHANGELOG.md
  - README.md
  - Cargo.toml
- MSRV will only increase when:
  - Security vulnerabilities require newer Rust
  - Essential features need newer Rust
  - Dependencies require newer Rust
  - At most once every 6 months

### Testing MSRV

We test against MSRV in CI:

```yaml
rust-versions:
  - 1.75.0  # MSRV
  - stable
  - nightly
```

## Stability Levels

### Stable (1.0+)

**Guarantees**: Full semver compliance, no breaking changes in minor versions.

Modules:
- ✅ `smmu::SMMU`
- ✅ `smmu::types::*`
- ✅ `smmu::prelude::*`
- ✅ All builder types
- ✅ All error types

### Internal (Exposed but Unstable)

**Guarantees**: May change in minor versions. Use at your own risk.

Modules:
- ⚠️ `smmu::address_space::*`
- ⚠️ `smmu::stream_context::*`
- ⚠️ `smmu::fault::*`
- ⚠️ `smmu::cache::*`

These are public for advanced use but not covered by semver guarantees.

### Experimental (Feature-Gated)

**Guarantees**: No stability guarantees. May change or be removed.

Features:
- 🧪 `experimental` - Cutting-edge features
- 🧪 `unstable` - Experimental APIs

Usage:
```toml
[dependencies]
smmu = { version = "1.0", features = ["experimental"] }
```

## Examples

### Example 1: Adding a New Method (Minor)

```rust
// v1.0.0
impl SMMU {
    pub fn translate(&self, addr: u64) -> Result<u64> { }
}

// v1.1.0 - Minor version bump
impl SMMU {
    pub fn translate(&self, addr: u64) -> Result<u64> { }

    // New method, backward compatible
    pub fn translate_batch(&self, addrs: &[u64]) -> Result<Vec<u64>> { }
}
```

### Example 2: Deprecating and Replacing (Minor → Major)

```rust
// v1.0.0
pub fn get_config(&self) -> Config { }

// v1.1.0 - Deprecate (minor bump)
#[deprecated(since = "1.1.0", note = "use `config()` instead")]
pub fn get_config(&self) -> Config {
    self.config()
}

pub fn config(&self) -> &Config { } // New preferred method

// v2.0.0 - Remove deprecated (major bump)
pub fn config(&self) -> &Config { } // Only new method remains
```

### Example 3: Fixing a Bug (Patch)

```rust
// v1.0.0 - Bug: returns wrong value
pub fn calculate_size(&self) -> usize {
    self.items.len() * 4 // Wrong multiplier
}

// v1.0.1 - Fix (patch bump)
pub fn calculate_size(&self) -> usize {
    self.items.len() * 8 // Correct multiplier
}
```

### Example 4: Adding Optional Feature (Minor)

```rust
// v1.1.0 - Minor bump
// Cargo.toml
[features]
serde = ["dep:serde"]

// lib.rs
#[cfg(feature = "serde")]
impl serde::Serialize for StreamID { }
```

## FAQ

### Q: Can I add fields to a public struct in a minor version?

**A**: Only if the struct is marked with `#[non_exhaustive]` or has a private field that prevents direct construction. Otherwise, it's a breaking change.

```rust
// Safe to add fields (user can't construct directly):
#[non_exhaustive]
pub struct Config {
    pub field1: u32,
    pub field2: u32,
    // Can add field3 in minor version
}

// Unsafe to add fields (user can construct with struct literal):
pub struct UnsafeConfig {
    pub field1: u32,
    pub field2: u32,
    // Adding field3 would break: UnsafeConfig { field1: 1, field2: 2 }
}

// Safe alternative - use builder:
pub struct SafeConfig {
    field1: u32,
    field2: u32,
}

impl SafeConfig {
    pub fn builder() -> ConfigBuilder { }
}
```

### Q: Can I add variants to an enum in a minor version?

**A**: Only if the enum is marked with `#[non_exhaustive]`.

```rust
#[non_exhaustive]
pub enum FaultType {
    Translation,
    Permission,
    // Can add new variants in minor version
}
```

### Q: Can I change internal implementation?

**A**: Yes! Internal changes are always allowed as long as public API and behavior remain stable.

### Q: Can I add trait bounds to existing functions?

**A**: No, that's a breaking change. Users' code may break if they use types that don't implement the new trait.

```rust
// v1.0.0
pub fn process<T>(data: T) { }

// v2.0.0 - BREAKING
pub fn process<T: Clone>(data: T) { } // Now requires Clone
```

### Q: Can I remove auto-traits like Send or Sync?

**A**: No, that's a breaking change (major version). Users may depend on these traits.

### Q: Can I increase MSRV without a major version bump?

**A**: Yes, but it's a minor version bump and must be clearly documented. This is allowed by our policy but treated seriously.

## References

- [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Cargo Book - SemVer Compatibility](https://doc.rust-lang.org/cargo/reference/semver.html)

## Questions or Concerns?

If you're unsure about a change:

1. Check this document
2. Review CHANGELOG.md for examples
3. Open a GitHub issue for discussion
4. When in doubt, be conservative - treat it as breaking

Better to be cautious and bump major version than break users' code unexpectedly.
