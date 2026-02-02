# Scripts Directory

Utility scripts for development and CI/CD automation.

## Available Scripts

### `ci-check.sh`

**Purpose**: Run all CI checks locally before pushing

**Usage**:
```bash
./scripts/ci-check.sh
```

**Checks Performed**:
1. ✅ Code formatting (rustfmt)
2. ✅ Clippy lints (all features, no features, minimal)
3. ✅ Library build
4. ✅ All targets build
5. ✅ Library tests
6. ✅ Integration tests
7. ✅ Documentation tests
8. ✅ Minimal feature tests
9. ✅ No default feature tests
10. ✅ Examples compilation
11. ✅ Documentation build
12. ✅ Security audit (if cargo-audit installed)
13. ✅ License check (if cargo-deny installed)

**Output**:
- Green ✓ for passing checks
- Red ✗ for failing checks
- Summary with pass/fail counts
- Exit code 0 if all pass, 1 if any fail

**Benefits**:
- Catch CI failures before pushing
- Save time on failed CI runs
- Ensure consistent code quality
- Quick feedback loop

**Recommended**: Run before every `git push`

## Installation of Optional Tools

For full local CI validation:

```bash
# Security audit
cargo install cargo-audit

# License and dependency checking
cargo install cargo-deny

# Code coverage
cargo install cargo-llvm-cov
```

## Git Hooks

To automatically run CI checks before pushing, add to `.git/hooks/pre-push`:

```bash
#!/bin/bash
./scripts/ci-check.sh
```

Make it executable:
```bash
chmod +x .git/hooks/pre-push
```

## Future Scripts

Planned scripts to be added:

- `release.sh` - Automated release preparation
- `benchmark.sh` - Local benchmark runner
- `coverage.sh` - Generate coverage reports locally
- `fuzz.sh` - Local fuzz testing runner
