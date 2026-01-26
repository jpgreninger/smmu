# ARM SMMU v3 Rust Project Structure

Complete directory and file structure for the Rust implementation.

## Directory Tree

```
rust/
├── Cargo.toml                      # Workspace manifest
├── rust-toolchain.toml             # Rust 1.75.0 toolchain specification
├── rustfmt.toml                    # Code formatting configuration
├── .clippy.toml                    # Linting configuration
├── .gitignore                      # Git ignore patterns
├── README.md                       # Project overview and quick start
├── DEVELOPMENT.md                  # Comprehensive development guide
├── PROJECT_STRUCTURE.md            # This file
├── TASK_1_1_COMPLETION.md          # Task completion report
│
├── smmu/                           # Main library crate
│   ├── Cargo.toml                  # Library manifest
│   │
│   ├── src/                        # Source code
│   │   ├── lib.rs                  # Crate root with documentation
│   │   │
│   │   ├── types/                  # Core types module
│   │   │   └── mod.rs              # Type definitions and enums
│   │   │
│   │   ├── address_space/          # Address space module
│   │   │   └── mod.rs              # Page table management
│   │   │
│   │   ├── stream_context/         # Stream context module
│   │   │   └── mod.rs              # Per-stream state management
│   │   │
│   │   ├── smmu/                   # SMMU controller module
│   │   │   └── mod.rs              # Main translation engine
│   │   │
│   │   ├── fault/                  # Fault handling module
│   │   │   └── mod.rs              # Fault detection and reporting
│   │   │
│   │   └── cache/                  # TLB cache module
│   │       └── mod.rs              # Translation cache implementation
│   │
│   ├── tests/                      # Integration tests
│   │   ├── integration_test.rs     # Cross-component tests
│   │   └── compliance_test.rs      # ARM SMMU v3 specification tests
│   │
│   └── benches/                    # Performance benchmarks
│       ├── translation.rs          # Translation latency benchmarks
│       └── address_space.rs        # Address space operation benchmarks
│
└── smmu-cli/                       # Command-line interface
    ├── Cargo.toml                  # CLI manifest
    └── src/
        └── main.rs                 # CLI entry point
```

## File Descriptions

### Root Configuration Files

| File | Purpose | Key Features |
|------|---------|--------------|
| `Cargo.toml` | Workspace manifest | Members, profiles, shared lints |
| `rust-toolchain.toml` | Toolchain version | Rust 1.75.0, components |
| `rustfmt.toml` | Code formatting | 4 spaces, 120 chars, K&R style |
| `.clippy.toml` | Linting rules | Pedantic configuration |
| `.gitignore` | Version control | Build artifacts, IDE files |

### Documentation Files

| File | Purpose | Audience |
|------|---------|----------|
| `README.md` | Quick start guide | All users |
| `DEVELOPMENT.md` | Development guide | Contributors |
| `PROJECT_STRUCTURE.md` | Structure reference | Developers |
| `TASK_1_1_COMPLETION.md` | Task report | Project tracking |

### Library Source Files

| File | Module | Purpose |
|------|--------|---------|
| `smmu/src/lib.rs` | Root | Crate entry point, documentation |
| `smmu/src/types/mod.rs` | types | Core types and enums |
| `smmu/src/address_space/mod.rs` | address_space | Page table operations |
| `smmu/src/stream_context/mod.rs` | stream_context | Stream state management |
| `smmu/src/smmu/mod.rs` | smmu | Main SMMU controller |
| `smmu/src/fault/mod.rs` | fault | Fault handling |
| `smmu/src/cache/mod.rs` | cache | TLB caching |

### Test Files

| File | Type | Purpose |
|------|------|---------|
| `smmu/tests/integration_test.rs` | Integration | Cross-component testing |
| `smmu/tests/compliance_test.rs` | Compliance | Specification validation |

### Benchmark Files

| File | Target | Purpose |
|------|--------|---------|
| `smmu/benches/translation.rs` | Translation | Latency measurement |
| `smmu/benches/address_space.rs` | Address Space | Operation performance |

### CLI Files

| File | Purpose |
|------|---------|
| `smmu-cli/src/main.rs` | CLI entry point and command handling |

## Module Dependencies

```
lib.rs
├── types          (no dependencies)
├── fault          (depends on: types)
├── address_space  (depends on: types, fault)
├── cache          (depends on: types, address_space)
├── stream_context (depends on: types, address_space, fault)
└── smmu           (depends on: all modules)
```

## Build Outputs

### Development Build
- Location: `target/debug/`
- Library: `libsmmu.rlib`
- CLI: `smmu-cli` (executable)

### Release Build
- Location: `target/release/`
- Library: `libsmmu.rlib`, `libsmmu.a`, `libsmmu.so`
- CLI: `smmu-cli` (optimized executable)

### Documentation
- Location: `target/doc/`
- Entry: `smmu/index.html`

### Test Results
- Location: `target/debug/deps/`
- Coverage: `coverage/` (if using tarpaulin)

### Benchmark Results
- Location: `target/criterion/`
- Reports: HTML and JSON format

## Line Counts (Estimated)

| Category | Files | Lines | Purpose |
|----------|-------|-------|---------|
| Configuration | 5 | ~300 | Build and tool config |
| Documentation | 4 | ~800 | Guides and references |
| Source Code | 7 | ~400 | Module placeholders |
| Tests | 2 | ~50 | Initial test structure |
| Benchmarks | 2 | ~80 | Benchmark placeholders |
| CLI | 1 | ~30 | CLI entry point |
| **Total** | **21** | **~1,660** | **Complete scaffold** |

## Key Design Decisions

### 1. Module Structure
- **Mirror C++ layout** for familiarity and parity
- **Separate concerns** through dedicated modules
- **Minimal coupling** between modules

### 2. Configuration Strategy
- **Workspace-level** shared settings
- **Strict linting** for code quality
- **Consistent formatting** enforced by rustfmt

### 3. Testing Approach
- **Integration tests** in `tests/` directory
- **Unit tests** inline with code (future)
- **Benchmarks** separate from tests
- **Compliance tests** for specification validation

### 4. Documentation Philosophy
- **Module-level docs** for each component
- **API docs** for all public items
- **Examples** in documentation
- **Guides** for developers

### 5. Performance Focus
- **Release profile** optimized for speed
- **Benchmarks** to validate targets
- **Profiling-ready** configuration

## Extension Points

### Adding New Modules

1. Create directory: `smmu/src/new_module/`
2. Add `mod.rs` with documentation
3. Declare in `lib.rs`: `pub mod new_module;`
4. Add tests in `tests/`
5. Add benchmarks if performance-critical

### Adding Dependencies

1. Review necessity and alternatives
2. Check crate audit status
3. Add to `Cargo.toml` with justification comment
4. Document usage in DEVELOPMENT.md

### Adding Features

1. Add feature flag in `Cargo.toml`
2. Use conditional compilation: `#[cfg(feature = "name")]`
3. Document feature in README.md
4. Test with and without feature enabled

## Build Commands Quick Reference

```bash
# Development
cargo check              # Fast compilation check
cargo build             # Debug build
cargo test              # Run tests
cargo doc --open        # View documentation

# Quality Assurance
cargo fmt --all         # Format code
cargo clippy            # Run lints
cargo test --all        # All tests

# Performance
cargo build --release   # Optimized build
cargo bench             # Run benchmarks

# Coverage (requires cargo-tarpaulin)
cargo tarpaulin --out Html
```

## Maintenance

### Regular Tasks
- Keep dependencies updated: `cargo update`
- Check for unused dependencies: `cargo tree`
- Audit security: `cargo audit` (requires cargo-audit)
- Clean build: `cargo clean`

### Version Updates
1. Update version in workspace `Cargo.toml`
2. Update CHANGELOG.md
3. Tag release: `git tag -a v1.x.x`
4. Verify: `cargo publish --dry-run`

## Summary

The Rust project structure provides:

- ✅ Clean, organized workspace
- ✅ Comprehensive configuration
- ✅ Documentation-first approach
- ✅ Performance-oriented design
- ✅ Test-driven development support
- ✅ Maintainable architecture
- ✅ Production-ready scaffolding

Ready for incremental feature implementation starting with Task 1.2 (Core Types).
