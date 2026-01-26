# ARM SMMU v3 Rust Implementation - Development Guide

## Project Structure Overview

This document describes the Cargo workspace structure and development workflow for the ARM SMMU v3 Rust implementation.

## Workspace Layout

```
rust/
├── Cargo.toml                    # Workspace manifest
├── rust-toolchain.toml           # Rust version pinning (1.75.0)
├── rustfmt.toml                  # Code formatting configuration
├── .clippy.toml                  # Linting configuration
├── .gitignore                    # Git ignore patterns
├── README.md                     # Project overview
├── DEVELOPMENT.md                # This file
│
├── smmu/                         # Main library crate
│   ├── Cargo.toml                # Library manifest
│   ├── src/
│   │   ├── lib.rs                # Crate root with documentation
│   │   ├── types/
│   │   │   └── mod.rs            # Core types and enums
│   │   ├── address_space/
│   │   │   └── mod.rs            # Page table management
│   │   ├── stream_context/
│   │   │   └── mod.rs            # Per-stream state
│   │   ├── smmu/
│   │   │   └── mod.rs            # Main SMMU controller
│   │   ├── fault/
│   │   │   └── mod.rs            # Fault handling
│   │   └── cache/
│   │       └── mod.rs            # TLB implementation
│   ├── tests/
│   │   ├── integration_test.rs   # Integration tests
│   │   └── compliance_test.rs    # Specification compliance tests
│   └── benches/
│       ├── translation.rs        # Translation benchmarks
│       └── address_space.rs      # Address space benchmarks
│
└── smmu-cli/                     # Command-line interface
    ├── Cargo.toml                # Binary manifest
    └── src/
        └── main.rs               # CLI entry point
```

## Development Workflow

### Initial Setup

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Navigate to Rust workspace
cd rust

# Verify toolchain installation
rustc --version  # Should show 1.75.0 or compatible

# Install development tools
rustup component add rustfmt clippy rust-src
```

### Building

```bash
# Quick check (fastest - no codegen)
cargo check --all-targets

# Build debug version
cargo build

# Build release version (optimized)
cargo build --release

# Build with all optimizations for benchmarking
cargo build --profile bench

# Build documentation
cargo doc --no-deps

# Build and open documentation
cargo doc --no-deps --open
```

### Testing

```bash
# Run all tests
cargo test --all

# Run with output
cargo test --all -- --nocapture

# Run specific test
cargo test test_smmu_creation

# Run integration tests only
cargo test --test integration_test

# Run compliance tests only
cargo test --test compliance_test

# Run with coverage (requires cargo-tarpaulin)
cargo tarpaulin --all --out Html --output-dir coverage
```

### Benchmarking

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark suite
cargo bench --bench translation

# Run specific benchmark
cargo bench bench_translation_simple

# Generate benchmark report
cargo bench -- --save-baseline main
```

### Code Quality

```bash
# Format all code
cargo fmt --all

# Check formatting without modifying
cargo fmt --all -- --check

# Run clippy lints
cargo clippy --all-targets

# Run clippy with warnings as errors
cargo clippy --all-targets -- -D warnings

# Run all quality checks
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test --all
```

## Development Standards

### Code Style

- **Indentation**: 4 spaces (enforced by rustfmt)
- **Line Length**: 120 characters maximum
- **Brace Style**: K&R (opening brace on same line)
- **Imports**: Organized by `std`, external crates, local modules
- **Documentation**: Required for all public items

### Naming Conventions

- **Types**: `PascalCase` (e.g., `StreamContext`, `AddressSpace`)
- **Functions**: `snake_case` (e.g., `translate_address`, `map_page`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `PAGE_SIZE`, `MAX_PASID`)
- **Modules**: `snake_case` (e.g., `stream_context`, `address_space`)

### Documentation Requirements

All public items must have documentation comments:

```rust
/// Short description (one line)
///
/// Detailed description explaining:
/// - Purpose and use cases
/// - Parameters and return values
/// - Errors and edge cases
/// - Examples where appropriate
///
/// # Examples
///
/// ```rust
/// use smmu::SMMU;
///
/// let smmu = SMMU::new();
/// ```
///
/// # Errors
///
/// Returns error if...
///
/// # Panics
///
/// Panics if... (avoid panics in library code)
pub fn documented_function() {}
```

### Error Handling

- **Library Code**: Use `Result<T, E>` - never panic in library code
- **Test Code**: `unwrap()` and `expect()` acceptable
- **Fallible Operations**: Always propagate errors properly
- **Custom Errors**: Use `thiserror` crate for custom error types (if needed)

### Safety Requirements

- **Unsafe Code**: Minimize and document thoroughly
- **MIRI Verification**: All unsafe code must pass MIRI
- **Soundness**: No undefined behavior allowed
- **Documentation**: Document all safety invariants

### Testing Standards

- **Coverage**: Minimum 95% code coverage
- **Unit Tests**: Test individual functions in isolation
- **Integration Tests**: Test component interactions
- **Property Tests**: Use proptest for property-based testing (planned)
- **Benchmarks**: Validate performance targets

### Performance Targets

- **Translation Latency**: 135ns average (match C++ baseline)
- **Memory Efficiency**: Sparse representation, minimal overhead
- **Cache Hit Rate**: >95% for typical workloads
- **Scalability**: Support hundreds of PASIDs and devices

## Module Organization

### Module Hierarchy

Each module follows this pattern:

```rust
// module/mod.rs

//! Module documentation
//!
//! Detailed description of module purpose and contents

// Submodules (if any)
mod submodule;

// Re-exports
pub use submodule::PublicType;

// Module contents
pub struct ModuleType {}
```

### Adding New Modules

1. Create module directory: `src/module_name/`
2. Add `mod.rs` with module documentation
3. Declare in parent module: `pub mod module_name;`
4. Add tests in `tests/` or inline with `#[cfg(test)]`
5. Update documentation

## Continuous Integration

The following checks should pass before committing:

```bash
# Format check
cargo fmt --all -- --check

# Clippy check
cargo clippy --all-targets -- -D warnings

# Test suite
cargo test --all

# Documentation build
cargo doc --no-deps

# Benchmark compilation (don't run on CI)
cargo bench --no-run
```

## Dependencies Policy

### Core Library (`smmu`)

- **Prefer**: Standard library only (no external dependencies)
- **If needed**: Only well-audited, widely-used crates
- **Review**: All dependencies must be reviewed and justified

### Development Dependencies

- **Testing**: Standard test framework + criterion for benchmarks
- **Coverage**: cargo-tarpaulin or similar
- **Property Testing**: proptest (planned)

### CLI Tool (`smmu-cli`)

- **Allow**: Common CLI crates (clap, etc.) as needed
- **Prefer**: Minimal dependencies

## Release Process

1. Update version in workspace `Cargo.toml`
2. Update CHANGELOG.md
3. Run full test suite: `cargo test --all`
4. Run benchmarks: `cargo bench`
5. Verify documentation: `cargo doc --no-deps`
6. Build release: `cargo build --release`
7. Create git tag: `git tag -a v1.0.0 -m "Release v1.0.0"`
8. Push: `git push origin v1.0.0`

## Troubleshooting

### Build Errors

```bash
# Clean build artifacts
cargo clean

# Update Cargo.lock
cargo update

# Verify toolchain
rustc --version
```

### Format Issues

```bash
# Auto-format all code
cargo fmt --all
```

### Clippy Warnings

```bash
# See all warnings with explanations
cargo clippy --all-targets -- -W clippy::pedantic

# Apply automated fixes (use with caution)
cargo clippy --fix --allow-dirty
```

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/master/)
- [ARM SMMU v3 Specification](../IHI0070G_b-System_Memory_Management_Unit_Architecture_Specification.pdf)
- [C++11 Reference Implementation](../)

## Next Steps

See `../TASKS-RUST.md` for the detailed implementation roadmap and task breakdown.
