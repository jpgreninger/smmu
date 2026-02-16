# Build Profiles Guide

This document describes the available build profiles for the ARM SMMU v3 Rust implementation and when to use each one.

---

## Available Profiles

The project provides **four build profiles** optimized for different use cases:

| Profile | Purpose | Opt Level | Debug Info | Binary Size | Build Time |
|---------|---------|-----------|------------|-------------|------------|
| **dev** | Development (default) | 0 | Full | 7.2M | Fast |
| **dev-opt** | Development + Performance | 2 | Full | 4.4M | Medium |
| **release** | Production | 3 | None | 308K | Slow |
| **release-small** | Embedded/Size-critical | z | None | 308K | Slowest |

---

## Profile Details

### 1. `dev` - Development (Default)

**Use when**: Daily development, debugging, testing

**Command**:
```bash
cargo build
cargo test
cargo run
```

**Configuration**:
```toml
[profile.dev]
opt-level = 0            # No optimization for fast compile
debug = true             # Full debug information
split-debuginfo = "unpacked"  # Better debugger support
```

**Characteristics**:
- ✅ Fastest compilation
- ✅ Best debugging experience
- ✅ Complete stack traces
- ❌ Slowest runtime performance
- ❌ Largest binary size (7.2M)

**Best for**:
- Writing new code
- Running tests
- Debugging with gdb/lldb
- Iterative development

---

### 2. `dev-opt` - Debug-Optimized

**Use when**: Debugging performance-sensitive code

**Command**:
```bash
cargo build --profile dev-opt
cargo test --profile dev-opt
cargo run --profile dev-opt
```

**Configuration**:
```toml
[profile.dev-opt]
inherits = "dev"
opt-level = 2            # Moderate optimization
debug = true             # Keep full debug information
incremental = true       # Enable incremental compilation
overflow-checks = true   # Keep safety checks
```

**Characteristics**:
- ✅ Good debugging experience
- ✅ Reasonable runtime performance
- ✅ Full stack traces available
- ⚖️ Medium compilation time
- ⚖️ Moderate binary size (4.4M)

**Best for**:
- Profiling with debug symbols
- Debugging performance issues
- Testing performance-critical paths
- Integration testing with realistic performance
- Development when performance matters

---

### 3. `release` - Production

**Use when**: Production deployments, benchmarking

**Command**:
```bash
cargo build --release
cargo test --release
cargo bench
```

**Configuration**:
```toml
[profile.release]
opt-level = 3            # Maximum optimization
lto = true               # Link-time optimization
codegen-units = 1        # Single compilation unit for better optimization
strip = true             # Remove debug symbols
panic = "abort"          # Smaller panic handler
```

**Characteristics**:
- ✅ Maximum runtime performance
- ✅ Small binary size (308K)
- ✅ Production-ready
- ❌ Slow compilation
- ❌ Limited debugging capability

**Best for**:
- Production deployments
- Performance benchmarking
- CI/CD release builds
- Final testing before release
- Performance validation

---

### 4. `release-small` - Size-Optimized

**Use when**: Embedded systems, resource-constrained environments

**Command**:
```bash
cargo build --profile release-small
```

**Configuration**:
```toml
[profile.release-small]
inherits = "release"
opt-level = "z"          # Optimize for size
lto = true               # Link-time optimization
codegen-units = 1        # Better optimization
strip = true             # Remove debug symbols
panic = "abort"          # Smaller panic handler
overflow-checks = false  # Disable for smaller size
```

**Characteristics**:
- ✅ Smallest possible binary
- ✅ Good for embedded systems
- ✅ Minimal memory footprint
- ⚖️ Slightly slower than release profile
- ❌ Slowest compilation
- ❌ No overflow checks (safety trade-off)

**Best for**:
- Embedded deployments
- Resource-constrained environments
- Containers with size limits
- Edge computing devices
- When binary size is critical

---

## Benchmark Profile

### `bench` - Benchmarking

**Command**:
```bash
cargo bench
```

**Configuration**:
```toml
[profile.bench]
inherits = "release"
```

**Characteristics**:
- Identical to release profile
- Used automatically by `cargo bench`
- Ensures consistent benchmark conditions

---

## Usage Examples

### Development Workflow

```bash
# Start development (fast iteration)
cargo build

# Run tests quickly
cargo test

# Profile code with debugging
cargo build --profile dev-opt
perf record target/dev-opt/smmu-cli

# Test performance before release
cargo build --release
cargo test --release
```

### CI/CD Pipeline

```bash
# Fast testing on every commit
cargo test

# Performance regression tests
cargo test --release
cargo bench

# Size validation for embedded targets
cargo build --profile release-small
ls -lh target/release-small/libsmmu.so
```

### Production Build

```bash
# Standard production build
cargo build --release --all-features

# Embedded/size-critical production build
cargo build --profile release-small --all-features

# Verify binary size
ls -lh target/release*/libsmmu.so
```

---

## Binary Size Comparison

Real measurements from the SMMU library:

| Profile | libsmmu.so | Reduction | Notes |
|---------|------------|-----------|-------|
| dev | 7.2 MB | baseline | Unoptimized + full debug |
| dev-opt | 4.4 MB | -39% | Optimized + full debug |
| release | 308 KB | -96% | Optimized + stripped |
| release-small | 308 KB | -96% | Size-optimized + stripped |

**Notes**:
- `release` and `release-small` similar size for this library
- Difference more pronounced for larger binaries
- Debug symbols add ~7 MB overhead
- Optimization reduces code size by ~96%

---

## Performance Comparison

Approximate relative performance (higher is better):

| Profile | Translation Speed | Compile Time | Use Case |
|---------|------------------|--------------|----------|
| dev | 1.0x | 1.0x | Development |
| dev-opt | ~3-5x | ~2x | Debug + Performance |
| release | ~15-20x | ~5-8x | Production |
| release-small | ~12-18x | ~6-10x | Embedded |

**Notes**:
- Actual speedup varies by workload
- Translation operations see largest benefit
- Size optimization may slightly reduce performance
- LTO increases compile time significantly

---

## Platform-Specific Notes

### Linux
- All profiles work on Linux (tested on Fedora 43)
- `strip = true` uses system strip utility
- `split-debuginfo = "unpacked"` optimized for gdb

### macOS
- All profiles should work (untested)
- May need `split-debuginfo = "unpacked"` for lldb

### Windows
- All profiles should work (untested)
- `strip = true` uses llvm-strip on Windows
- Debug info format may differ (PDB files)

---

## Troubleshooting

### Profile Not Found

**Error**: `error: profile 'dev-opt' not found`

**Solution**: Ensure you're in the workspace root or using a recent Cargo version (1.57+)

### Binary Too Large

**Problem**: Release binary still too large

**Solutions**:
1. Use `release-small` profile
2. Check for debug info: `file target/release/libsmmu.so`
3. Enable additional stripping: `strip target/release/libsmmu.so`
4. Disable default features: `cargo build --release --no-default-features --features minimal`

### Debugging Release Builds

**Problem**: Need to debug optimized code

**Solutions**:
1. Use `dev-opt` profile instead of `release`
2. Temporarily modify `[profile.release]` to include `debug = true`
3. Use `debug = 1` for minimal debug info in release

---

## Custom Profile Configuration

You can create additional custom profiles by adding to `Cargo.toml`:

```toml
[profile.my-custom-profile]
inherits = "release"
opt-level = 2
debug = true
lto = "thin"
```

Then use with:
```bash
cargo build --profile my-custom-profile
```

---

## Environment Variables

Override profile settings with environment variables:

```bash
# Force optimization level
CARGO_PROFILE_DEV_OPT_LEVEL=2 cargo build

# Enable debug info in release
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release

# Disable LTO for faster compile
CARGO_PROFILE_RELEASE_LTO=false cargo build --release
```

---

## Best Practices

### Development
1. Use `dev` for most development work
2. Switch to `dev-opt` when debugging performance
3. Regularly test with `release` to catch optimization issues

### Testing
1. Run unit tests with `dev` for speed
2. Run integration tests with `release` for realism
3. Run benchmarks with `bench` for consistency

### Production
1. Always use `release` or `release-small` for production
2. Test production builds before deployment
3. Verify binary size and performance metrics
4. Document which profile was used for deployment

### CI/CD
1. Test with multiple profiles (dev, release)
2. Benchmark with `bench` profile
3. Validate size with `release-small` if relevant
4. Cache build artifacts by profile

---

## References

- [Cargo Profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [Cargo Configuration](https://doc.rust-lang.org/cargo/reference/config.html)
- [Rust Optimization](https://doc.rust-lang.org/cargo/reference/profiles.html#opt-level)

---

**Last Updated**: February 1, 2026
**Version**: 1.0.0
