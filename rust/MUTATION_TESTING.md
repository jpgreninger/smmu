# Mutation Testing Report

## Overview

This document describes the mutation testing setup and results for the ARM SMMU v3 Rust implementation using `cargo-mutants`.

## What is Mutation Testing?

Mutation testing is a technique to evaluate test suite quality by introducing small changes (mutations) to the source code and checking if the tests catch these changes. A "killed" mutant means tests detected the change (good), while a "surviving" mutant suggests missing test coverage (needs investigation).

## Setup

### Installation

```bash
cargo install cargo-mutants
```

### Running Mutation Tests

```bash
# Run mutation testing on entire codebase
cargo mutants

# Run on specific file
cargo mutants --file src/address_space/mod.rs

# Run with timeout (recommended for large codebases)
cargo mutants --timeout 60

# Generate HTML report
cargo mutants --output html

# Run incrementally (only test mutants that haven't been tested)
cargo mutants --in-place
```

### Configuration

Mutation testing configuration is specified in `.cargo/mutants.toml`:

```toml
# Timeout for each test run (seconds)
timeout = 300

# Minimum test time (to detect tests that always pass immediately)
minimum_test_time = 0.5

# Exclude certain files or patterns
exclude_files = [
    "tests/*",           # Don't mutate test files
    "benches/*",         # Don't mutate benchmarks
    "examples/*",        # Don't mutate examples
]

# Exclude certain mutation types
exclude_mutations = [
    # None currently - we want comprehensive coverage
]
```

## Mutation Testing Goals

### Target: >90% Mutation Score

Mutation Score = (Killed Mutants / Total Mutants) × 100%

### Metrics Tracked

1. **Total Mutants**: Number of mutations generated
2. **Killed Mutants**: Mutations detected by tests (caught = good)
3. **Survived Mutants**: Mutations not detected (missed = investigate)
4. **Timeout Mutants**: Tests that ran too long (may indicate infinite loop)
5. **Unviable Mutants**: Mutations that don't compile (expected, not counted)

## Mutation Test Results

### Latest Run: [DATE]

**Overall Metrics:**
- Total Mutants: TBD
- Killed: TBD (--%)
- Survived: TBD (--%)
- Timeout: TBD
- Unviable: TBD
- **Mutation Score: --%**

### Results by Module

#### `src/types/` - Core Type Definitions

| File | Total | Killed | Survived | Score |
|------|-------|--------|----------|-------|
| `address.rs` | TBD | TBD | TBD | --% |
| `pasid.rs` | TBD | TBD | TBD | --% |
| `stream_id.rs` | TBD | TBD | TBD | --% |
| `permissions.rs` | TBD | TBD | TBD | --% |

#### `src/address_space/` - Address Space Management

| File | Total | Killed | Survived | Score |
|------|-------|--------|----------|-------|
| `mod.rs` | TBD | TBD | TBD | --% |

#### `src/stream_context/` - Stream Context Management

| File | Total | Killed | Survived | Score |
|------|-------|--------|----------|-------|
| `mod.rs` | TBD | TBD | TBD | --% |

#### `src/cache/` - TLB Cache Implementation

| File | Total | Killed | Survived | Score |
|------|-------|--------|----------|-------|
| `mod.rs` | TBD | TBD | TBD | --% |

#### `src/fault/` - Fault Handling

| File | Total | Killed | Survived | Score |
|------|-------|--------|----------|-------|
| `detection.rs` | TBD | TBD | TBD | --% |
| `processing.rs` | TBD | TBD | TBD | --% |
| `queue.rs` | TBD | TBD | TBD | --% |
| `recovery.rs` | TBD | TBD | TBD | --% |

#### `src/smmu/` - Main SMMU Controller

| File | Total | Killed | Survived | Score |
|------|-------|--------|----------|-------|
| `mod.rs` | TBD | TBD | TBD | --% |

## Surviving Mutants Analysis

### Critical Surviving Mutants (Must Fix)

**None identified yet** - Run mutation testing first

### Acceptable Surviving Mutants (Documented)

Document any mutants that survive for valid reasons:

1. **Equivalent Mutants**: Changes that don't affect behavior
   - Example: Changing `i += 1` to `i = i + 1`

2. **Performance Optimizations**: Changes that only affect performance, not correctness
   - Example: Removing early return optimization

3. **Defensive Code**: Extra safety checks that are hard to test
   - Example: Bounds checks that are always satisfied by type system

### Investigated Surviving Mutants

| Mutant | Location | Type | Reason Survived | Action Taken |
|--------|----------|------|-----------------|--------------|
| TBD | TBD | TBD | TBD | TBD |

## Common Mutation Types

cargo-mutants generates these mutation types:

1. **Binary Operators**: Changes `+` to `-`, `<` to `<=`, etc.
2. **Unary Operators**: Removes `!`, changes `-x` to `x`
3. **Literals**: Changes numeric constants, booleans
4. **Return Values**: Changes return values to defaults
5. **Function Calls**: Replaces function calls with default values
6. **Control Flow**: Removes branches, changes conditions

## Integration with CI/CD

Mutation testing can be integrated into CI/CD pipeline:

```yaml
# .github/workflows/mutation-testing.yml
name: Mutation Testing

on:
  schedule:
    - cron: '0 2 * * 0'  # Weekly on Sunday at 2 AM
  workflow_dispatch:      # Manual trigger

jobs:
  mutation-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable

      - name: Install cargo-mutants
        run: cargo install cargo-mutants

      - name: Run mutation tests
        run: cargo mutants --timeout 300 --output json

      - name: Upload results
        uses: actions/upload-artifact@v3
        with:
          name: mutation-results
          path: mutants.out/

      - name: Check mutation score
        run: |
          SCORE=$(jq '.score' mutants.out/mutants.json)
          if (( $(echo "$SCORE < 90" | bc -l) )); then
            echo "Mutation score $SCORE is below 90% threshold"
            exit 1
          fi
```

## Best Practices

### Writing Mutation-Resistant Tests

1. **Assert Specific Values**: Don't just check `is_ok()`, verify actual values
   ```rust
   // Bad
   assert!(result.is_ok());

   // Good
   assert_eq!(result.unwrap(), expected_value);
   ```

2. **Test Boundary Conditions**: Explicitly test edge cases
   ```rust
   #[test]
   fn test_boundary() {
       assert_eq!(func(0), expected_zero);
       assert_eq!(func(MAX), expected_max);
       assert_eq!(func(MAX + 1), error);
   }
   ```

3. **Test Both Branches**: Ensure both true and false paths are tested
   ```rust
   #[test]
   fn test_condition_true() {
       assert_eq!(func(true_case), true_result);
   }

   #[test]
   fn test_condition_false() {
       assert_eq!(func(false_case), false_result);
   }
   ```

4. **Check Error Cases**: Verify error messages and types
   ```rust
   #[test]
   fn test_error() {
       let result = func(invalid_input);
       assert!(matches!(result, Err(SpecificError::ExpectedType)));
   }
   ```

### Improving Mutation Score

1. **Add Missing Tests**: When mutants survive, add tests for that code path
2. **Improve Assertions**: Make assertions more specific
3. **Test Invariants**: Add property-based tests for invariants
4. **Remove Dead Code**: If mutation doesn't affect tests, code may be unused

## Mutation Testing Workflow

1. **Initial Run**: Establish baseline mutation score
   ```bash
   cargo mutants --output json > initial-results.json
   ```

2. **Analyze Results**: Review surviving mutants
   ```bash
   cargo mutants --list --caught false
   ```

3. **Add Tests**: Write tests to kill surviving mutants

4. **Verify Improvement**: Re-run mutation testing
   ```bash
   cargo mutants --in-place
   ```

5. **Document Decisions**: Update this file with findings

6. **Iterate**: Repeat until target mutation score achieved

## Performance Considerations

Mutation testing is computationally expensive:

- **Time**: Expect 10-100x longer than regular test runs
- **Parallelization**: cargo-mutants runs tests in parallel by default
- **Incremental**: Use `--in-place` to avoid re-testing killed mutants
- **Selective**: Focus on critical modules first with `--file` flag

## References

- [cargo-mutants Documentation](https://mutants.rs/)
- [Mutation Testing Concepts](https://en.wikipedia.org/wiki/Mutation_testing)
- [Effective Mutation Testing](https://pedrorijo.com/blog/mutation-testing/)

## Appendix A: Running Full Mutation Suite

```bash
#!/bin/bash
# run-mutation-tests.sh

set -e

echo "Running comprehensive mutation testing..."

# Clean previous results
rm -rf mutants.out/

# Run mutation testing with detailed output
cargo mutants \
    --timeout 300 \
    --output json \
    --output html \
    --jobs $(nproc) \
    2>&1 | tee mutation-test.log

# Generate summary
echo ""
echo "=== Mutation Testing Summary ==="
jq -r '.summary' mutants.out/mutants.json

# Check if we meet threshold
SCORE=$(jq -r '.score * 100' mutants.out/mutants.json)
THRESHOLD=90

echo ""
if (( $(echo "$SCORE >= $THRESHOLD" | bc -l) )); then
    echo "✓ Mutation score ${SCORE}% meets ${THRESHOLD}% threshold"
    exit 0
else
    echo "✗ Mutation score ${SCORE}% below ${THRESHOLD}% threshold"
    echo ""
    echo "Surviving mutants:"
    cargo mutants --list --caught false
    exit 1
fi
```

## Appendix B: Mutation Testing Checklist

- [ ] Install cargo-mutants
- [ ] Configure `.cargo/mutants.toml`
- [ ] Run initial baseline mutation testing
- [ ] Document baseline mutation score
- [ ] Analyze surviving mutants by category
- [ ] Add tests for high-priority surviving mutants
- [ ] Re-run mutation testing
- [ ] Achieve >90% mutation score
- [ ] Document acceptable surviving mutants
- [ ] Integrate into CI/CD (optional)
- [ ] Schedule regular mutation testing runs

---

**Last Updated**: [DATE]
**Mutation Score Target**: 90%
**Current Mutation Score**: TBD
**Status**: Initial Setup Complete
