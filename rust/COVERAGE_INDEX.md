# Coverage Report Index - February 15, 2026

## Quick Stats

**Overall Coverage:** 95.17% lines, 94.42% regions, 94.16% functions ⭐⭐⭐⭐⭐

**Status:** ✅ EXCEEDS 95% TARGET

---

## Report Files

### 1. Quick Reference
**File:** `COVERAGE_QUICKREF.md` (1.9 KB)
**Purpose:** One-page summary with key metrics and commands
**Best for:** Quick status check

### 2. Executive Summary
**File:** `COVERAGE-RUST.md` (13 KB)
**Purpose:** Comprehensive executive summary with module breakdown
**Best for:** Stakeholder reporting and trend analysis

### 3. Detailed Report
**File:** `COVERAGE_REPORT_2026-02-15.md` (12 KB)
**Purpose:** Complete analysis with recommendations
**Best for:** Deep dive into coverage details

### 4. Text Summary
**File:** `COVERAGE_SUMMARY.txt` (5.7 KB)
**Purpose:** Plain text summary for terminal viewing
**Best for:** Console/script integration

### 5. Machine-Readable Data
**File:** `coverage.lcov` (438 KB)
**Purpose:** LCOV format for CI/CD integration
**Best for:** Codecov, Coveralls, SonarQube integration

### 6. Interactive HTML
**Path:** `target/llvm-cov/html/index.html`
**Purpose:** Line-by-line interactive coverage browser
**Best for:** Detailed code-level analysis
**Open with:** `xdg-open target/llvm-cov/html/index.html`

---

## Coverage Highlights

### Perfect Coverage Modules (100%)
13 modules achieved 100% coverage:
- types/address.rs
- types/config.rs
- types/command_entry.rs
- types/event_entry.rs
- types/fault_type.rs
- types/pasid.rs
- types/pri_entry.rs
- types/queue_statistics.rs
- types/security_state.rs
- types/stream_context_error.rs
- types/stream_id.rs
- types/translation_stage.rs
- types/validation_error.rs

### Near-Perfect Coverage (95-99%)
- fault/processing.rs: 99.53%
- types/page_entry.rs: 99.55%
- types/translation_result.rs: 98.77%
- types/fault_record.rs: 98.66%
- fault/queue.rs: 100% lines
- address_space/mod.rs: 96.69%
- stream_context/mod.rs: 96.62%

### Focus Areas for Improvement
- fault/detection.rs: 84.98% → Target 95%
- fault/recovery.rs: 83.06% → Target 90%
- smmu/mod.rs: 89.48% → Target 95%

---

## Test Statistics

- **Total Tests:** 548+
- **Test Suites:** 14
- **Success Rate:** 100%
- **Execution Time:** ~2 seconds

### Test Categories
- Unit Tests: 228 tests
- Integration Tests: 22 tests
- Configuration Tests: 257 tests
- Concurrency Tests: 41 tests

---

## How to Use These Reports

### For Daily Development
1. Check `COVERAGE_QUICKREF.md` for quick status
2. View HTML report for line-by-line details

### For Code Reviews
1. Check module coverage in `COVERAGE-RUST.md`
2. Review detailed analysis in `COVERAGE_REPORT_2026-02-15.md`

### For CI/CD Integration
1. Use `coverage.lcov` with your coverage service
2. Set threshold to 95% (currently exceeding)

### For Stakeholders
1. Share `COVERAGE_SUMMARY.txt` or `COVERAGE-RUST.md`
2. Highlight 95.17% coverage achievement

---

## Regenerate Coverage

```bash
# Clean and regenerate all coverage
cd rust
cargo llvm-cov clean
cargo llvm-cov --all-features --workspace \
  --lcov --output-path coverage.lcov \
  -- --skip test_concurrent_translation_performance

# Generate HTML
cargo llvm-cov --all-features --workspace --html \
  -- --skip test_concurrent_translation_performance

# View results
xdg-open target/llvm-cov/html/index.html
```

---

## Version History

- **v1.2.2** (Feb 15, 2026): 95.17% - EXCELLENT ⭐⭐⭐⭐⭐
- **v1.2.1** (Previous): 71.06% - Partial run
- **v1.2.0**: ~73% estimated
- **v1.1.0**: ~72% estimated

---

## Next Coverage Review

**Target Date:** March 1, 2026 or v1.2.3 release
**Goals:**
- Fault detection to 95%+
- Fault recovery to 90%+
- Overall to 96%+

---

Generated: February 15, 2026
Tool: cargo-llvm-cov 0.8.2
Rust: 1.93.0
