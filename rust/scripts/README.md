# ARM SMMU v3 Automation Scripts

This directory contains automation scripts for the ARM SMMU v3 Rust implementation.

## Quick Reference

```bash
./dev.sh check           # Quick pre-commit validation
./test.sh all            # Run all tests
./build.sh release       # Optimized build
./coverage.sh           # Generate coverage report
./bench.sh all          # Run benchmarks
./audit.sh              # Security audit
./setup-hooks.sh        # Install git hooks
```

## Scripts

### `dev.sh` ⭐ Main Development Helper
Primary script for common development tasks.

**Usage**:
```bash
./dev.sh [command]
```

**Commands**:
- `check` - Quick validation (fmt + clippy + check)
- `fmt` - Format code with rustfmt
- `lint` - Run clippy with pedantic lints
- `build` - Build all targets
- `test` - Run all tests
- `bench` - Run benchmarks
- `doc` - Generate and open documentation
- `clean` - Clean build artifacts
- `audit` - Run security audit
- `outdated` - Check for outdated dependencies
- `coverage` - Generate coverage report
- `watch` - Auto-run checks on file changes
- `install-tools` - Install development tools

**Features**:
- Colored output for readability
- Comprehensive error handling
- Integration with all other scripts
- Quick feedback for development

### `build.sh`
Build script with various configurations.

**Usage**:
```bash
./build.sh [profile]
```

**Profiles**:
- `dev` - Development build (default, fast compile)
- `release` - Release build (optimized)
- `bench` - Benchmark build
- `all-targets` - Build all targets
- `check` - Quick check without full build

**Examples**:
```bash
./build.sh                # Development build
./build.sh release        # Optimized release
./build.sh check          # Fast syntax check
```

### `test.sh`
Comprehensive test execution with multiple modes.

**Usage**:
```bash
./test.sh [test-type] [options]
```

**Test Types**:
- `all` - Run all tests (default)
- `unit` - Unit tests only
- `integration` - Integration tests only
- `doc` - Documentation tests
- `quick` - Quick tests without optimization
- `verbose` - Tests with verbose output
- `specific` - Run specific test by name

**Options**:
- `--nocapture` - Show test output
- `--ignored` - Run ignored tests
- `--threads N` - Number of test threads

**Examples**:
```bash
./test.sh                      # All tests
./test.sh unit                 # Unit tests only
./test.sh specific test_name   # Specific test
```

### `bench.sh`
Benchmark execution and performance tracking.

**Usage**:
```bash
./bench.sh [command] [options]
```

**Commands**:
- `all` - Run all benchmarks (default)
- `baseline` - Save baseline for comparison
- `compare` - Compare against baseline
- `specific` - Run specific benchmark
- `list` - List available benchmarks

**Examples**:
```bash
./bench.sh                     # Run all benchmarks
./bench.sh baseline main       # Save baseline
./bench.sh compare main        # Compare to baseline
```

**Output**: HTML reports in `target/criterion/*/report/index.html`

### `coverage.sh`
Automated code coverage generation and reporting.

**Usage**:
```bash
./coverage.sh              # Generate JSON coverage report
./coverage.sh --html       # Generate HTML report (opens in browser)
./coverage.sh --lcov       # Generate LCOV report for CI
./coverage.sh --check      # Check coverage against 95% threshold
./coverage.sh --summary    # Show coverage summary only
./coverage.sh --all        # Generate all report types
```

**Features**:
- Automatic cargo-llvm-cov installation
- Multiple report formats (JSON, HTML, LCOV)
- Coverage threshold validation (95%)
- Browser integration (auto-open HTML)
- CI-friendly output

**Requirements**:
- Rust toolchain with llvm-tools-preview
- cargo-llvm-cov (installed automatically)

**Output**:
- JSON: `../target/coverage/codecov.json`
- HTML: `../target/coverage/html/index.html`
- LCOV: `../target/coverage/lcov.info`

### `audit.sh`
Security auditing using cargo-deny.

**Usage**:
```bash
./audit.sh
```

**Checks**:
- Security vulnerabilities (RustSec database)
- License compliance
- Banned crates
- Dependency sources

**Features**:
- Automatic advisory database updates
- Comprehensive security scanning
- License validation
- CI-friendly output

**Requirements**:
- cargo-deny (installed automatically if missing)

### `check-outdated.sh`
Check for outdated dependencies.

**Usage**:
```bash
./check-outdated.sh [--aggressive]
```

**Modes**:
- Default: Compatible updates only
- `--aggressive`: All updates including breaking changes

**Features**:
- Dependency version checking
- Update strategy guidance
- Security advisory integration
- Safe update recommendations

**Requirements**:
- cargo-outdated (install with `cargo install cargo-outdated`)

### `setup-hooks.sh`
Install git pre-commit hooks for code quality.

**Usage**:
```bash
./setup-hooks.sh
```

**Installed Hooks**:
- Pre-commit: Format, lint, and compilation checks

**Features**:
- Automatic git configuration
- Pre-commit validation
- Code quality enforcement
- Easy setup and removal

**Hook Checks**:
1. Code formatting (rustfmt)
2. Linting (clippy pedantic)
3. Compilation (all targets)
4. Security audit (if available)

### `validate_infrastructure.sh`
Validation script for testing infrastructure setup.

**Usage**:
```bash
./validate_infrastructure.sh
```

**Checks**:
- ✓ Rust toolchain installed (cargo, rustc)
- ✓ Project structure (directories)
- ✓ Test utilities (6 modules)
- ✓ Test suites (integration, compliance)
- ✓ Test fixtures (JSON files)
- ✓ Benchmarks (3 suites)
- ✓ Coverage infrastructure
- ✓ CI/CD pipeline
- ✓ Cargo configuration
- ✓ JSON fixture validity
- ✓ Build validation
- ✓ Test compilation
- ✓ Benchmark compilation

**Output**:
- Colored console output
- Pass/fail summary
- Exit code 0 on success, 1 on failure

**Use Cases**:
- Post-installation verification
- Pre-commit checks
- Troubleshooting setup issues
- CI environment validation

## Common Workflows

### Initial Setup
```bash
# 1. Install development tools
./dev.sh install-tools

# 2. Setup git hooks
./setup-hooks.sh

# 3. Validate infrastructure
./validate_infrastructure.sh

# 4. Run initial tests
./test.sh all
```

### Daily Development Workflow
```bash
# 1. Auto-watch for changes (optional)
./dev.sh watch

# 2. Make code changes

# 3. Quick check before commit
./dev.sh check

# 4. Run tests
./test.sh all

# 5. Check coverage
./coverage.sh --check
```

### Pre-Commit Workflow
```bash
# Quick validation (recommended)
./dev.sh check

# Or manually:
cargo fmt --all              # Format
cargo clippy --all-targets   # Lint
cargo check --all-targets    # Compile
cargo test                   # Test
```

**Note**: Pre-commit hooks run automatically if installed with `./setup-hooks.sh`

### Feature Development
```bash
# 1. Create feature branch
git checkout -b feature/new-feature

# 2. Make changes

# 3. Add tests
./test.sh specific test_new_feature

# 4. Check coverage
./coverage.sh

# 5. Run benchmarks (if performance-critical)
./bench.sh specific new_feature_bench

# 6. Final validation
./dev.sh check
./test.sh all
./audit.sh

# 7. Commit changes (hooks run automatically)
git add .
git commit -m "Add new feature"
```

### Performance Optimization
```bash
# 1. Run baseline benchmarks
./bench.sh baseline before-optimization

# 2. Make optimizations

# 3. Compare performance
./bench.sh compare before-optimization

# 4. Verify correctness
./test.sh all

# 5. Check coverage maintained
./coverage.sh --check
```

### Release Preparation
```bash
# 1. Full validation
./validate_infrastructure.sh

# 2. Security audit
./audit.sh

# 3. Check for outdated dependencies
./check-outdated.sh

# 4. Run all tests
./test.sh all

# 5. Generate coverage
./coverage.sh --all

# 6. Run benchmarks
./bench.sh all

# 7. Build release
./build.sh release

# 8. Generate documentation
./dev.sh doc
```

### Debugging Failed Tests
```bash
# Run with verbose output
./test.sh verbose

# Run specific failing test
./test.sh specific failing_test_name

# Or use cargo directly
cargo test failing_test_name -- --nocapture --exact

# With backtrace
RUST_BACKTRACE=1 cargo test failing_test_name
```

### Updating Dependencies
```bash
# 1. Check what's outdated
./check-outdated.sh

# 2. Run security audit
./audit.sh

# 3. Update compatible versions
cargo update

# 4. Run tests
./test.sh all

# 5. Verify coverage
./coverage.sh --check

# 6. Re-audit
./audit.sh
```

## Script Maintenance

### Adding New Scripts

When adding new automation scripts:

1. **Create script** in this directory
2. **Make executable**: `chmod +x script_name.sh`
3. **Add shebang**: `#!/usr/bin/env bash`
4. **Set errexit**: `set -euo pipefail`
5. **Add usage function**: Help text for users
6. **Add to README**: Document in this file
7. **Test thoroughly**: Verify on clean environment

### Script Template

```bash
#!/usr/bin/env bash
#
# Script description
#
# Usage: ./script_name.sh [OPTIONS]

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Usage function
usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Description of what this script does.

OPTIONS:
    --option1       Description
    --option2       Description
    --help, -h      Show this help message

EXAMPLES:
    $0 --option1
    $0 --option2

EOF
}

# Main function
main() {
    # Script logic here
    echo "Script execution"
}

main "$@"
```

## Troubleshooting

### cargo-llvm-cov not found
```bash
# Install manually
cargo install cargo-llvm-cov

# Or let script install
./coverage.sh
```

### Permission denied
```bash
# Make scripts executable
chmod +x coverage.sh
chmod +x validate_infrastructure.sh
```

### Coverage fails
```bash
# Ensure llvm-tools-preview is installed
rustup component add llvm-tools-preview

# Clean and retry
cargo clean
./coverage.sh
```

### Validation fails
```bash
# Check specific error message
# Ensure all files are committed
git status

# Rebuild if needed
cargo build --workspace
```

## CI Integration

These scripts are used in CI/CD pipeline:

### `.github/workflows/ci.yml`
```yaml
- name: Generate coverage
  working-directory: ./rust/smmu
  run: |
    cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info

- name: Check coverage threshold
  working-directory: ./rust/smmu
  run: |
    cargo llvm-cov --workspace --all-features --summary-only
```

## Best Practices

1. **Always use absolute paths**: Script may be called from anywhere
2. **Set errexit**: `set -euo pipefail` for safety
3. **Provide usage**: Help users understand options
4. **Use colors**: Make output readable
5. **Handle errors**: Graceful failure with clear messages
6. **Test on clean system**: Verify script works from scratch
7. **Document thoroughly**: Update README when changing scripts

## Development Tools Installation

### Required Tools

Install all development tools at once:

```bash
./dev.sh install-tools
```

This installs:
- `cargo-watch` - Auto-run commands on file changes
- `cargo-deny` - Security and license auditing
- `cargo-outdated` - Check for outdated dependencies
- `cargo-audit` - Security vulnerability scanning
- `cargo-llvm-cov` - Code coverage generation
- `cargo-expand` - Macro expansion for debugging

### Manual Installation

If you prefer manual installation:

```bash
cargo install cargo-watch
cargo install cargo-deny
cargo install cargo-outdated
cargo install cargo-audit
cargo install cargo-llvm-cov
cargo install cargo-expand
```

## IDE Integration

### VS Code Setup

Configuration files in `.vscode/`:
- `settings.json` - rust-analyzer with pedantic lints
- `extensions.json` - Recommended extensions
- `launch.json` - Debug configurations
- `tasks.json` - Build tasks

**Recommended Extensions** (auto-suggested):
- rust-lang.rust-analyzer
- vadimcn.vscode-lldb
- serayuzgur.crates
- usernamehw.errorlens
- ryanluker.vscode-coverage-gutters

### EditorConfig

`.editorconfig` provides consistent formatting for all editors.

## Git Hooks

### Installing Hooks

```bash
./setup-hooks.sh
```

### Pre-commit Hook

Automatically validates:
1. Code formatting (rustfmt)
2. Linting (clippy pedantic)
3. Compilation (all targets)
4. Security (if cargo-deny installed)

### Bypassing Hooks

Only in emergencies:

```bash
git commit --no-verify
```

### Removing Hooks

```bash
git config --unset core.hooksPath
```

## Security and Compliance

### Security Auditing

Configuration in `deny.toml`:
- RustSec advisory database checks
- License compliance validation
- Banned crate detection
- Source validation

**Run audit**:
```bash
./audit.sh
```

### License Compliance

**Allowed licenses**:
- MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause
- ISC, Unicode-DFS-2016, Unlicense, Zlib

**Denied licenses**:
- GPL-3.0, AGPL-3.0, LGPL-3.0

## Performance Benchmarking

### Running Benchmarks

```bash
# All benchmarks
./bench.sh all

# Save baseline for later comparison
./bench.sh baseline my-baseline

# Compare current performance to baseline
./bench.sh compare my-baseline

# Run specific benchmark
./bench.sh specific translation_bench
```

### Benchmark Results

- HTML reports: `target/criterion/*/report/index.html`
- Raw data: `target/criterion/`
- Statistics: Mean, median, std deviation
- Regression detection: Automatic performance regression alerts

## Code Coverage

### Generating Coverage

```bash
# HTML report (opens in browser)
./coverage.sh --html

# LCOV for CI integration
./coverage.sh --lcov

# Check against 95% threshold
./coverage.sh --check

# Summary only
./coverage.sh --summary
```

### Coverage Reports

- HTML: `target/coverage/html/index.html`
- LCOV: `target/coverage/lcov.info`
- JSON: `target/coverage/codecov.json`

### Coverage Requirements

- Target: >90% overall
- Critical paths: 100%
- Public APIs: Comprehensive tests
- Error paths: Complete coverage

## Documentation

See `DEVELOPMENT_TOOLS.md` for comprehensive guide including:
- Detailed script documentation
- Troubleshooting tips
- Performance optimization
- Advanced workflows
- Tool configuration

## Future Enhancements

Planned improvements:
- `release.sh` - Automated release workflow
- `docs.sh` - Documentation publishing
- Integrated fuzzing support
- Property-based testing helpers
- Performance regression tracking

---

**Maintained by**: ARM SMMU v3 Implementation Team
**Last Updated**: 2026-01-24
