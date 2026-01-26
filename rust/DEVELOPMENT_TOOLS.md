# Development Tools Guide

Comprehensive guide to development tools for the ARM SMMU v3 Rust implementation.

## Quick Start

### Initial Setup

1. **Install Development Tools**:
```bash
./scripts/dev.sh install-tools
```

2. **Setup Git Hooks**:
```bash
./scripts/setup-hooks.sh
```

3. **Verify Installation**:
```bash
./scripts/validate_infrastructure.sh
```

## IDE Integration

### VS Code (Recommended)

All VS Code configuration is in `.vscode/`:

- **settings.json**: rust-analyzer configuration with pedantic lints
- **extensions.json**: Recommended extensions list
- **launch.json**: Debug configurations for tests and CLI
- **tasks.json**: Common cargo tasks

#### Recommended Extensions

Install all recommended extensions when prompted, or manually:

```bash
# Essential Rust tools
code --install-extension rust-lang.rust-analyzer
code --install-extension vadimcn.vscode-lldb
code --install-extension serayuzgur.crates

# Code quality
code --install-extension usernamehw.errorlens
code --install-extension ryanluker.vscode-coverage-gutters

# TOML support
code --install-extension tamasfe.even-better-toml
```

#### Key Features Enabled

1. **Inlay Hints**: See type information inline
2. **Auto-format on Save**: Automatic rustfmt on save
3. **Clippy Integration**: Real-time linting with pedantic checks
4. **Code Actions**: Quick fixes and refactoring
5. **Inline Diagnostics**: See errors/warnings inline

### Other Editors

The project includes `.editorconfig` for consistent formatting across all editors that support EditorConfig (Vim, Emacs, IntelliJ, etc.).

## Development Scripts

All scripts are in the `scripts/` directory and provide help with `--help`.

### Quick Development Workflow

```bash
# Check code quality (recommended before commits)
./scripts/dev.sh check

# Auto-format on file changes
./scripts/dev.sh watch

# Run full test suite
./scripts/test.sh all

# Generate coverage report
./scripts/coverage.sh
```

### Script Reference

#### `dev.sh` - Main Development Helper

Common development tasks:

```bash
./scripts/dev.sh check      # Quick validation (fmt + clippy + check)
./scripts/dev.sh fmt        # Format all code
./scripts/dev.sh lint       # Run clippy with pedantic lints
./scripts/dev.sh build      # Build all targets
./scripts/dev.sh test       # Run all tests
./scripts/dev.sh doc        # Generate and open documentation
./scripts/dev.sh watch      # Auto-run checks on file changes
./scripts/dev.sh audit      # Run security audit
./scripts/dev.sh outdated   # Check for outdated dependencies
```

#### `build.sh` - Build Configurations

```bash
./scripts/build.sh dev          # Development build (fast)
./scripts/build.sh release      # Optimized release build
./scripts/build.sh bench        # Benchmark build
./scripts/build.sh all-targets  # Build all targets
./scripts/build.sh check        # Quick syntax check
```

#### `test.sh` - Comprehensive Testing

```bash
./scripts/test.sh all           # Run all tests
./scripts/test.sh unit          # Unit tests only
./scripts/test.sh integration   # Integration tests only
./scripts/test.sh doc           # Documentation tests
./scripts/test.sh verbose       # Verbose output
./scripts/test.sh specific NAME # Run specific test
```

#### `bench.sh` - Benchmarking

```bash
./scripts/bench.sh all              # Run all benchmarks
./scripts/bench.sh baseline         # Save baseline
./scripts/bench.sh compare          # Compare to baseline
./scripts/bench.sh specific NAME    # Run specific benchmark
./scripts/bench.sh list             # List benchmarks
```

#### `coverage.sh` - Code Coverage

```bash
./scripts/coverage.sh               # Generate HTML coverage report
./scripts/coverage.sh --open        # Generate and open in browser
./scripts/coverage.sh --lcov        # Generate LCOV format
```

#### `audit.sh` - Security Auditing

```bash
./scripts/audit.sh                  # Run all security checks
```

Checks performed:
- Security vulnerabilities (RustSec database)
- License compliance
- Banned crates
- Dependency sources

#### `check-outdated.sh` - Dependency Updates

```bash
./scripts/check-outdated.sh             # Conservative updates
./scripts/check-outdated.sh --aggressive # All updates including breaking
```

## Security Auditing

### cargo-deny Configuration

Configuration in `deny.toml` enforces:

1. **Security Advisories**: Deny known vulnerabilities
2. **License Compliance**: Only approved licenses allowed
3. **Source Validation**: Only crates.io sources
4. **Banned Crates**: No known problematic crates

### Running Security Audits

```bash
# Full audit
./scripts/audit.sh

# Update advisory database
cargo deny fetch advisories

# Check specific category
cargo deny check advisories  # Security only
cargo deny check licenses    # Licenses only
cargo deny check bans        # Banned crates only
cargo deny check sources     # Source validation
```

### Approved Licenses

- MIT
- Apache-2.0
- BSD-2-Clause / BSD-3-Clause
- ISC
- Unicode-DFS-2016
- Unlicense
- Zlib

**Denied**: GPL-3.0, AGPL-3.0, LGPL-3.0

## Git Hooks

Pre-commit hook (`.githooks/pre-commit`) validates:

1. **Code Formatting**: rustfmt compliance
2. **Linting**: Clippy pedantic checks
3. **Compilation**: All targets compile
4. **Security**: Advisory check (if cargo-deny installed)

### Installing Hooks

```bash
./scripts/setup-hooks.sh
```

### Bypassing Hooks

Only use in emergencies:

```bash
git commit --no-verify
```

### Disabling Hooks

```bash
git config --unset core.hooksPath
```

## Code Quality Standards

### Formatting

Configuration in `rustfmt.toml`:

```bash
# Check formatting
cargo fmt --all -- --check

# Auto-format
cargo fmt --all
```

### Linting

Configuration in `.clippy.toml` and `Cargo.toml`:

```bash
# Run clippy with pedantic lints
cargo clippy --all-targets --all-features -- -W clippy::pedantic

# Auto-fix where possible
cargo clippy --fix --all-targets --all-features
```

### Enabled Lint Groups

- `clippy::pedantic` - Detailed style lints
- `clippy::nursery` - Experimental lints
- `clippy::cargo` - Cargo best practices
- `clippy::correctness` - Critical bugs (deny)
- `clippy::suspicious` - Suspicious patterns (deny)

## Testing Strategy

### Test Levels

1. **Unit Tests**: Test individual components
2. **Integration Tests**: Test component interactions
3. **Documentation Tests**: Validate doc examples
4. **Benchmarks**: Performance validation

### Coverage Requirements

- Target: >90% code coverage
- Critical paths: 100% coverage
- All public APIs: Comprehensive tests

### Running Tests

```bash
# All tests
cargo test --all-targets --all-features

# With coverage
./scripts/coverage.sh

# Specific test
cargo test test_name -- --exact

# Ignored tests
cargo test -- --ignored
```

## Continuous Integration

### Pre-commit Checklist

Before every commit, run:

```bash
./scripts/dev.sh check
```

This validates:
- ✓ Formatting (rustfmt)
- ✓ Linting (clippy pedantic)
- ✓ Compilation (all targets)
- ✓ Security (advisories)

### Pre-push Checklist

Before pushing:

```bash
./scripts/test.sh all      # All tests pass
./scripts/audit.sh         # Security audit clean
./scripts/coverage.sh      # Coverage >90%
```

## Performance Optimization

### Profiling

```bash
# Build with debug symbols
cargo build --release --profile bench

# Run with profiling tools
perf record target/release/smmu-cli
perf report
```

### Benchmarking

```bash
# Run benchmarks
./scripts/bench.sh all

# Save baseline
./scripts/bench.sh baseline my-baseline

# Compare performance
./scripts/bench.sh compare my-baseline
```

### Assembly Inspection

```bash
# View generated assembly
cargo rustc --release -- --emit asm

# Use cargo-asm
cargo install cargo-asm
cargo asm smmu::translate --rust
```

## Documentation

### Generating Docs

```bash
# Generate and open docs
cargo doc --no-deps --all-features --open

# Include private items
cargo doc --no-deps --all-features --document-private-items
```

### Documentation Standards

All public items must have:
- Summary description
- Detailed explanation
- Examples (as doctests)
- Safety notes (for unsafe)
- Panic conditions

## Dependency Management

### Checking for Updates

```bash
# Conservative (compatible versions)
./scripts/check-outdated.sh

# Aggressive (all versions)
./scripts/check-outdated.sh --aggressive
```

### Updating Dependencies

```bash
# Update to latest compatible versions
cargo update

# Update specific crate
cargo update -p crate-name

# Update to latest version (edit Cargo.toml first)
cargo update
```

### Audit Process

Before updating any dependency:

1. Run security audit: `./scripts/audit.sh`
2. Review CHANGELOG for breaking changes
3. Update one dependency at a time
4. Run full test suite: `./scripts/test.sh all`
5. Verify coverage: `./scripts/coverage.sh`

## Troubleshooting

### Common Issues

#### rust-analyzer Not Working

```bash
# Reload VS Code window
Ctrl+Shift+P -> "Developer: Reload Window"

# Clear rust-analyzer cache
rm -rf ~/.cache/rust-analyzer

# Rebuild project
cargo clean && cargo build
```

#### Slow Compilation

```bash
# Use mold linker (much faster)
cargo install -f mold

# Or use lld
sudo apt install lld  # Linux
brew install llvm     # macOS
```

Add to `~/.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

#### Coverage Not Generating

```bash
# Ensure cargo-llvm-cov is installed
cargo install cargo-llvm-cov

# Clean and regenerate
cargo clean
./scripts/coverage.sh
```

## Tool Installation

### Required Tools

```bash
# Install all development tools
./scripts/dev.sh install-tools
```

This installs:
- cargo-watch - Auto-run commands on file changes
- cargo-deny - Security and license auditing
- cargo-outdated - Check for outdated dependencies
- cargo-audit - Security vulnerability scanning
- cargo-llvm-cov - Code coverage generation
- cargo-expand - Expand macros

### Manual Installation

```bash
cargo install cargo-watch
cargo install cargo-deny
cargo install cargo-outdated
cargo install cargo-audit
cargo install cargo-llvm-cov
cargo install cargo-expand
```

## Resources

### Official Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust Reference](https://doc.rust-lang.org/reference/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/master/)

### Project Documentation

- `README.md` - Project overview
- `DEVELOPMENT.md` - Development guide
- `PROJECT_STRUCTURE.md` - Architecture overview
- `TESTING_INFRASTRUCTURE_SUMMARY.md` - Testing guide

### ARM Specification

- ARM SMMU v3 Architecture Specification
- Location: `../IHI0070G_b-System_Memory_Management_Unit_Architecture_Specification.pdf`

## Getting Help

### Quick Commands

```bash
# Script help
./scripts/dev.sh help
./scripts/test.sh help
./scripts/build.sh help
./scripts/bench.sh help

# Cargo help
cargo --help
cargo test --help
cargo bench --help

# rust-analyzer help
Ctrl+Shift+P -> "rust-analyzer: "
```

### Debug Mode

Run any command with verbose output:

```bash
RUST_LOG=debug cargo test
RUST_BACKTRACE=1 cargo run
RUST_BACKTRACE=full cargo test
```
