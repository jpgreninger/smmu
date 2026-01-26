# Task 2.1 Quick Reference - StreamID & PASID Implementation

## TL;DR

✅ **Status**: Test suite complete (125+ tests), implementation needed
🎯 **Goal**: Implement StreamID, PASID, ValidationError newtypes
⏱️ **Time**: ~7 hours estimated
🔥 **Critical**: PASID 0 support is MANDATORY (ARM SMMU v3 spec)

## File Quick Reference

```
Implementation Files (src/types/):
├── validation_error.rs  → Implement error type FIRST
├── stream_id.rs         → Then StreamID (16-bit typical)
└── pasid.rs             → Finally PASID (20-bit, PASID 0 critical!)

Test Files (tests/):
├── test_validation_error.rs  → 25+ tests, 250+ assertions
├── test_stream_id.rs         → 40+ tests, 400+ assertions
└── test_pasid.rs             → 60+ tests, 600+ assertions (PASID 0!)
```

## Implementation Order

### 1. ValidationError (~1 hour)

```rust
pub struct ValidationError {
    field: String,
    value: String,
    constraint: String,
}

impl ValidationError {
    pub fn new(field: &str, value: &str, constraint: &str) -> Self {
        Self {
            field: field.to_string(),
            value: value.to_string(),
            constraint: constraint.to_string(),
        }
    }
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Invalid {}: value '{}' {}", self.field, self.value, self.constraint)
    }
}
```

**Test**: `cargo test --test test_validation_error`

### 2. StreamID (~2 hours)

```rust
const STREAM_ID_MAX: u32 = 65535; // Typical 16-bit max

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StreamID(u32);

impl StreamID {
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value <= STREAM_ID_MAX {
            Ok(StreamID(value))
        } else {
            Err(ValidationError::new(
                "StreamID",
                &value.to_string(),
                &format!("must be <= {}", STREAM_ID_MAX)
            ))
        }
    }

    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for StreamID {
    fn default() -> Self {
        StreamID(0)
    }
}

impl Display for StreamID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "StreamID({})", self.0)
    }
}

impl TryFrom<u32> for StreamID {
    type Error = ValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        StreamID::new(value)
    }
}

impl From<StreamID> for u32 {
    fn from(s: StreamID) -> u32 {
        s.0
    }
}
```

**Test**: `cargo test --test test_stream_id`

### 3. PASID (~2 hours) ⚠️ CRITICAL: PASID 0 SUPPORT

```rust
pub const PASID_MAX: u32 = 0xFFFFF; // 20-bit maximum

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PASID(u32);

impl PASID {
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value <= PASID_MAX {
            Ok(PASID(value))
        } else {
            Err(ValidationError::new(
                "PASID",
                &value.to_string(),
                &format!("must be <= {} (20-bit maximum)", PASID_MAX)
            ))
        }
    }

    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for PASID {
    fn default() -> Self {
        PASID(0)  // ⚠️ CRITICAL: Default is PASID 0 (default address space)
    }
}

impl Display for PASID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PASID({})", self.0)
    }
}

impl TryFrom<u32> for PASID {
    type Error = ValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        PASID::new(value)
    }
}

impl From<PASID> for u32 {
    fn from(p: PASID) -> u32 {
        p.0
    }
}
```

**Test**: `cargo test --test test_pasid`

## Critical Test Cases

### PASID 0 (MANDATORY)

```rust
#[test]
fn test_pasid_zero_required() {
    let pasid = PASID::new(0);
    assert!(pasid.is_ok(), "PASID 0 is REQUIRED");
}

#[test]
fn test_pasid_zero_default() {
    let default = PASID::default();
    assert_eq!(default.as_u32(), 0, "Default must be PASID 0");
}
```

### 20-bit Boundary

```rust
#[test]
fn test_pasid_boundary_max() {
    assert!(PASID::new(0xFFFFF).is_ok());   // Valid
    assert!(PASID::new(0x100000).is_err()); // Invalid
}
```

## Running Tests

```bash
cd /home/jpgreninger/Work/smmu/rust/smmu

# Run all tests
cargo test --test test_stream_id --test test_pasid --test test_validation_error

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test --test test_pasid test_pasid_zero_required

# Coverage
cargo llvm-cov --test test_stream_id --test test_pasid --test test_validation_error
```

## Success Checklist

- [ ] ValidationError implemented and all tests pass
- [ ] StreamID implemented and all 40+ tests pass
- [ ] PASID implemented and all 60+ tests pass
- [ ] **PASID 0 tests pass** (CRITICAL - 3 specific tests)
- [ ] 20-bit validation tests pass (boundary testing)
- [ ] Zero Clippy warnings: `cargo clippy -- -D warnings`
- [ ] Formatted: `cargo fmt --check`
- [ ] Coverage >95%: `cargo llvm-cov`
- [ ] All 125+ tests pass: 100% pass rate

## ARM SMMU v3 Compliance

### StreamID
- ✓ 32-bit value with configurable max (typically 65535)
- ✓ StreamID 0 supported
- ✓ Type-safe validation

### PASID (20-bit)
- ✓ Maximum: 0xFFFFF (1,048,575)
- ✓ **PASID 0 = default address space** (MANDATORY)
- ✓ Reject values > 0xFFFFF
- ✓ Type-safe validation

## Common Issues

### Issue: Test panics with "unimplemented!"
**Solution**: Implementation not complete. Follow implementation order above.

### Issue: PASID 0 tests fail
**Solution**: Check `Default::default()` returns `PASID(0)` and `new(0)` accepts it.

### Issue: 20-bit boundary tests fail
**Solution**: Validate `value <= 0xFFFFF`, not `value < 0x100000`.

### Issue: Clippy warnings
**Solution**: Add `#[allow(dead_code)]` during development or fix warnings.

## Performance Validation

After tests pass, verify zero-cost abstraction:

```bash
# Check assembly
cargo asm smmu::types::PASID::as_u32

# Benchmark (should be <1ns)
cargo bench
```

**Expected**: Newtype wrappers compile to identical code as raw `u32`.

## Next Steps After Implementation

1. ✅ All tests pass (125+ tests)
2. ✅ Coverage >95%
3. ✅ Zero warnings
4. ⏭️ **qa-engineer**: Review against ARM SMMU v3 spec
5. ⏭️ Update TASKS-RUST.md (mark Task 2.1 complete)
6. ⏭️ Integrate into CI/CD pipeline
7. ⏭️ Move to Task 2.2 (Address types)

## Documentation

- Full test documentation: `tests/unit/README.md`
- Test suite summary: `TEST_SUITE_TASK_2_1_SUMMARY.md`
- This quick reference: `TASK_2_1_QUICK_REFERENCE.md`

---

**Reminder**: All tests WILL FAIL until implementation is complete. This is correct TDD behavior!
