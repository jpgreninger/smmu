# CI/CD Pipeline Documentation

## Overview

The ARM SMMU v3 Rust implementation uses GitHub Actions for comprehensive continuous integration and deployment. The CI/CD pipeline ensures code quality, cross-platform compatibility, and automated releases.

## Workflows

### 1. Main CI Workflow (`.github/workflows/ci.yml`)

**Triggers**: Push to main/develop, Pull Requests, Daily schedule

#### Jobs

##### Format Check (`fmt`)
- **Purpose**: Ensure consistent code formatting
- **Tool**: `rustfmt`
- **Command**: `cargo fmt --all -- --check`
- **Fails on**: Any formatting violations

##### Clippy Lints (`clippy`)
- **Purpose**: Static analysis and linting
- **Tool**: `clippy`
- **Configurations**:
  - All features enabled
  - No default features
  - Minimal feature set
- **Command**: `cargo clippy --all-targets --all-features -- -D warnings`
- **Fails on**: Any warnings (zero-warning policy)

##### Security Audit (`audit`)
- **Purpose**: Check for known security vulnerabilities
- **Tool**: `cargo-audit`
- **Database**: RustSec Advisory Database
- **Command**: `cargo audit`
- **Fails on**: Any security advisories

##### License Check (`deny`)
- **Purpose**: Verify dependency licenses
- **Tool**: `cargo-deny`
- **Checks**:
  - License compliance (MIT/Apache-2.0)
  - Banned dependencies
  - Security advisories
- **Command**: `cargo deny check`

##### Test Matrix (`test`)
- **Purpose**: Cross-platform testing
- **Matrix Dimensions**:
  - **Platforms**:
    - `ubuntu-latest` (Linux x86_64)
    - `windows-latest` (Windows x86_64)
    - `macos-latest` (macOS Intel)
    - `macos-14` (macOS Apple Silicon M1)
  - **Rust Versions**:
    - `1.75.0` (MSRV - Minimum Supported Rust Version)
    - `stable` (Latest stable)
    - `nightly` (Nightly builds)
- **Total Configurations**: 10 (3 platforms × 3 versions + 1 M1)
- **Tests Run**:
  - Library tests (`cargo test --lib`)
  - Integration tests (`cargo test --test '*'`)
  - Doc tests (`cargo test --doc`)
  - Minimal features (`--no-default-features --features minimal`)
  - No features (`--no-default-features`)

##### Feature Combinations (`features`)
- **Purpose**: Test all feature flag combinations
- **Features Tested**:
  1. No default features
  2. `std` only
  3. `std + pasid`
  4. `std + two-stage`
  5. `std + cache`
  6. `std + serde`
  7. `minimal` preset
  8. `full` preset
  9. All features
- **Total Configurations**: 9

##### Examples (`examples`)
- **Purpose**: Verify all examples compile and run
- **Examples**: 8 total
- **Timeout**: 30 seconds per example
- **Command**: `cargo run --example <name> --all-features`

##### Documentation (`docs`)
- **Purpose**: Verify documentation builds without errors
- **Rust Version**: Nightly (for latest doc features)
- **Command**: `cargo doc --all-features --no-deps`
- **Environment**: `RUSTDOCFLAGS="-D warnings"`
- **Includes**: Doc tests

##### Cross-Compilation (`cross-compile`)
- **Purpose**: Verify compilation for additional targets
- **Targets**:
  - `x86_64-unknown-linux-musl` (Linux musl/static)
  - `aarch64-unknown-linux-gnu` (Linux ARM64)
  - `x86_64-pc-windows-gnu` (Windows MinGW)
  - `x86_64-apple-darwin` (macOS Intel)
  - `aarch64-apple-darwin` (macOS Apple Silicon)
- **Cross-tools**: Automatically installed (musl-tools, gcc-aarch64, mingw-w64)

##### Benchmarks (`benchmark`)
- **Purpose**: Verify benchmarks compile and run
- **Mode**: Quick run (not full benchmark suite)
- **Command**: `cargo bench --all-features -- --quick`
- **Continues on error**: Yes (benchmarks may fail on slow runners)

##### Code Coverage (`coverage`)
- **Purpose**: Track test coverage
- **Tool**: `cargo-llvm-cov`
- **Upload**: Codecov
- **Output**: LCOV format
- **Command**: `cargo llvm-cov --all-features --workspace --lcov`
- **Minimum**: No enforced minimum (informational only)

##### Minimal Versions (`minimal-versions`)
- **Purpose**: Ensure compatibility with oldest dependency versions
- **Tools**: Rust nightly + stable
- **Command**: `cargo +nightly update -Z minimal-versions`
- **Verifies**: Dependency version ranges are correct

##### CI Success (`ci-success`)
- **Purpose**: Single job that depends on all others
- **Use**: Branch protection rules can require just this job
- **Status**: Only succeeds if all other jobs succeed

### 2. Release Workflow (`.github/workflows/release.yml`)

**Triggers**: Git tags matching `v[0-9]+.[0-9]+.[0-9]+` (e.g., `v1.0.0`)

#### Jobs

##### Pre-Release Checks (`pre-release`)
- **Version Verification**: Ensures tag matches `Cargo.toml` version
- **Package Check**: Verifies package builds correctly
- **Contents Verification**: Lists packaged files

##### Build Matrix (`build`)
- **Purpose**: Build release binaries for all platforms
- **Platforms**:
  1. Linux x86_64 (GNU)
  2. Linux x86_64 (musl/static)
  3. Linux ARM64
  4. Windows x86_64 (MSVC)
  5. macOS x86_64 (Intel)
  6. macOS ARM64 (Apple Silicon)
- **Packaging**:
  - Unix: `.tar.gz`
  - Windows: `.zip`
- **Artifacts**: Uploaded for release

##### Create Release (`release`)
- **Purpose**: Create GitHub release with binaries
- **Artifacts**: All platform binaries
- **Release Notes**: Auto-generated from commits
- **Type**: Public release (not draft, not pre-release)

##### Publish (`publish`)
- **Purpose**: Publish to crates.io
- **Token**: `CARGO_TOKEN` secret required
- **Command**: `cargo publish --manifest-path smmu/Cargo.toml`
- **Continue on error**: Yes (if already published)

### 3. Nightly Workflow (`.github/workflows/nightly.yml`)

**Triggers**: Daily at 02:00 UTC, Manual dispatch

#### Jobs

##### Performance Benchmarks (`benchmark`)
- **Purpose**: Track performance over time
- **Suite**: Full benchmark suite (not quick mode)
- **Storage**: Results stored in gh-pages branch
- **Visualization**: Performance graphs

##### Fuzz Testing (`fuzz`)
- **Purpose**: Find edge cases and crashes
- **Tool**: `cargo-fuzz`
- **Runtime**: 5 minutes per fuzzer
- **Continue on error**: Yes (informational)

##### Memory Leak Check (`valgrind`)
- **Purpose**: Detect memory leaks
- **Tool**: Valgrind
- **Scope**: All tests
- **Continue on error**: Yes (informational)

##### Latest Dependencies (`deps-latest`)
- **Purpose**: Test with bleeding-edge dependencies
- **Update**: `cargo update` (latest compatible versions)
- **Checks**: Deprecation warnings
- **Continue on error**: Yes (early warning system)

## CI/CD Configuration

### Caching Strategy

**Tool**: `Swatinem/rust-cache@v2`

**Cached Items**:
- Cargo registry (`~/.cargo/registry`)
- Cargo git checkouts (`~/.cargo/git`)
- Target directory (`target/`)
- Rustup toolchains

**Cache Keys**: Unique per:
- Operating system
- Rust version
- Feature combination
- Cargo.lock hash

**Benefits**:
- ~80% reduction in CI time
- Reduced network bandwidth
- Faster feedback on PRs

### Environment Variables

```yaml
RUST_BACKTRACE: 1           # Full backtraces on panic
CARGO_TERM_COLOR: always    # Colored output in CI logs
CARGO_INCREMENTAL: 0        # Disable incremental (better for CI)
MSRV: 1.75.0               # Minimum Supported Rust Version
```

### Required Secrets

For full CI/CD functionality:

1. **CODECOV_TOKEN** (Optional)
   - Purpose: Upload coverage to Codecov
   - Obtain: https://codecov.io
   - Required: No (coverage still runs locally)

2. **CARGO_TOKEN** (Required for releases)
   - Purpose: Publish to crates.io
   - Obtain: https://crates.io/settings/tokens
   - Required: Yes (for `cargo publish`)

3. **GITHUB_TOKEN** (Auto-provided)
   - Purpose: GitHub API access
   - Obtain: Automatic
   - Required: Yes (automatic)

## Badges

Add to `README.md`:

```markdown
[![CI](https://github.com/jpgreninger/smmu/workflows/CI/badge.svg)](https://github.com/jpgreninger/smmu/actions)
[![Coverage](https://codecov.io/gh/jpgreninger/smmu/branch/main/graph/badge.svg)](https://codecov.io/gh/jpgreninger/smmu)
[![Crates.io](https://img.shields.io/crates/v/smmu.svg)](https://crates.io/crates/smmu)
[![Docs](https://docs.rs/smmu/badge.svg)](https://docs.rs/smmu)
```

## Performance Benchmarks

### Tracked Metrics

1. **Translation Latency**: Time per address translation
2. **Cache Performance**: Hit/miss rates
3. **Memory Usage**: Peak memory consumption
4. **Throughput**: Translations per second

### Benchmark Outputs

- **Local**: `target/criterion/report/index.html`
- **CI**: Stored in gh-pages branch
- **Tracking**: Historical graphs available

## Testing Strategy

### Test Levels

1. **Unit Tests** (Fast, ~2s)
   - Individual module testing
   - Mock dependencies
   - Property-based tests

2. **Integration Tests** (Medium, ~5s)
   - Multi-module interactions
   - Realistic scenarios
   - End-to-end flows

3. **Doc Tests** (Fast, ~1s)
   - Documentation examples
   - API usage examples
   - Compile-time verification

4. **Benchmarks** (Slow, ~60s)
   - Performance validation
   - Regression detection
   - Optimization targets

### Coverage Goals

- **Target**: >95% line coverage
- **Critical Paths**: 100% coverage
- **Tracking**: Codecov integration
- **Reports**: Per-PR coverage diff

## Troubleshooting

### Common CI Failures

#### Formatting Failures
**Symptom**: `fmt` job fails
**Fix**: Run `cargo fmt --all` locally
**Prevention**: Use pre-commit hooks

#### Clippy Warnings
**Symptom**: `clippy` job fails
**Fix**: Run `cargo clippy --all-features -- -D warnings`
**Allow**: Add `#[allow(clippy::specific_lint)]` if justified

#### Test Failures
**Symptom**: `test` job fails on specific platform
**Debug**: Check platform-specific behavior
**Fix**: Use conditional compilation if needed

#### Coverage Upload Failures
**Symptom**: `coverage` job fails to upload
**Check**: CODECOV_TOKEN is set correctly
**Fix**: Coverage job has `fail_ci_if_error: false`

#### MSRV Failures
**Symptom**: Tests fail on Rust 1.75.0
**Check**: Using features newer than MSRV
**Fix**: Bump MSRV or avoid new features

### Local CI Simulation

```bash
# Run all checks locally before pushing
./scripts/ci-check.sh

# Individual checks
cargo fmt --all -- --check
cargo clippy --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
cargo doc --all-features --no-deps
```

## Maintenance

### Updating Workflows

1. Edit workflow files in `.github/workflows/`
2. Test changes in feature branch
3. Verify all jobs pass
4. Merge to main

### Adding New Platforms

1. Add to test matrix in `ci.yml`
2. Add to build matrix in `release.yml`
3. Update cross-compilation targets
4. Test with `workflow_dispatch`

### Upgrading Actions

- **Dependabot**: Automatically creates PRs for action updates
- **Manual**: Check `uses:` versions periodically
- **Testing**: Always test in feature branch first

## Best Practices

### Pull Requests

1. **Run locally first**: `cargo test --all-features`
2. **Fix all warnings**: `cargo clippy --all-features`
3. **Format code**: `cargo fmt --all`
4. **Wait for CI**: All checks must pass
5. **Review coverage**: Check Codecov report

### Releases

1. **Update version**: In `Cargo.toml`
2. **Update CHANGELOG**: Document changes
3. **Test locally**: Full test suite
4. **Create tag**: `git tag v1.0.0`
5. **Push tag**: `git push origin v1.0.0`
6. **Monitor CI**: Watch release workflow
7. **Verify publish**: Check crates.io

### Performance Monitoring

1. **Track trends**: Review nightly benchmark results
2. **Investigate regressions**: >5% slowdown requires review
3. **Document improvements**: Note optimization wins
4. **Profile locally**: Use `cargo bench` for development

## Resources

- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [rust-cache Action](https://github.com/Swatinem/rust-cache)
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)
- [cargo-audit](https://github.com/rustsec/rustsec/tree/main/cargo-audit)
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)

---

**Status**: ✅ CI/CD Pipeline Active
**Coverage**: 100% of development workflow
**Platforms**: 6 tested (Linux, Windows, macOS)
**Rust Versions**: 3 tested (MSRV, Stable, Nightly)

**Last Updated**: February 1, 2026
**Next Review**: Quarterly or when adding new features
