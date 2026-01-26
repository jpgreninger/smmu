#!/usr/bin/env bash
#
# Validation script for testing infrastructure
#
# This script validates that all testing infrastructure is properly set up
# and ready for development.
#
# Usage: ./scripts/validate_infrastructure.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

passed=0
failed=0

# Helper functions
print_header() {
    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}"
}

check_pass() {
    echo -e "${GREEN}✓ $1${NC}"
    ((passed++))
}

check_fail() {
    echo -e "${RED}✗ $1${NC}"
    ((failed++))
}

check_file() {
    if [[ -f "$1" ]]; then
        check_pass "File exists: $1"
        return 0
    else
        check_fail "Missing file: $1"
        return 1
    fi
}

check_dir() {
    if [[ -d "$1" ]]; then
        check_pass "Directory exists: $1"
        return 0
    else
        check_fail "Missing directory: $1"
        return 1
    fi
}

# Start validation
echo -e "${BLUE}ARM SMMU v3 Rust - Testing Infrastructure Validation${NC}"
echo "Project root: ${PROJECT_ROOT}"

# Check Rust installation
print_header "Rust Toolchain"
if command -v cargo &> /dev/null; then
    check_pass "cargo found: $(cargo --version)"
    if command -v rustc &> /dev/null; then
        check_pass "rustc found: $(rustc --version)"
    else
        check_fail "rustc not found"
    fi
else
    check_fail "cargo not found - please install Rust from https://rustup.rs"
fi

# Check project structure
print_header "Project Structure"
check_dir "${PROJECT_ROOT}"
check_dir "${PROJECT_ROOT}/smmu"
check_dir "${PROJECT_ROOT}/smmu-cli"
check_dir "${PROJECT_ROOT}/scripts"
check_dir "${PROJECT_ROOT}/../.github/workflows"

# Check test utilities
print_header "Test Utilities"
check_dir "${PROJECT_ROOT}/smmu/tests/common"
check_file "${PROJECT_ROOT}/smmu/tests/common/mod.rs"
check_file "${PROJECT_ROOT}/smmu/tests/common/builders.rs"
check_file "${PROJECT_ROOT}/smmu/tests/common/assertions.rs"
check_file "${PROJECT_ROOT}/smmu/tests/common/fixtures.rs"
check_file "${PROJECT_ROOT}/smmu/tests/common/perf.rs"
check_file "${PROJECT_ROOT}/smmu/tests/common/mocks.rs"

# Check test suites
print_header "Test Suites"
check_file "${PROJECT_ROOT}/smmu/tests/integration_test.rs"
check_file "${PROJECT_ROOT}/smmu/tests/compliance_test.rs"
check_file "${PROJECT_ROOT}/smmu/tests/README.md"

# Check test fixtures
print_header "Test Fixtures"
check_dir "${PROJECT_ROOT}/smmu/tests/fixtures"
check_file "${PROJECT_ROOT}/smmu/tests/fixtures/translation_scenarios.json"
check_file "${PROJECT_ROOT}/smmu/tests/fixtures/fault_scenarios.json"

# Check benchmarks
print_header "Benchmarks"
check_dir "${PROJECT_ROOT}/smmu/benches"
check_file "${PROJECT_ROOT}/smmu/benches/translation.rs"
check_file "${PROJECT_ROOT}/smmu/benches/address_space.rs"
check_file "${PROJECT_ROOT}/smmu/benches/cache.rs"

# Check coverage infrastructure
print_header "Coverage Infrastructure"
check_file "${PROJECT_ROOT}/scripts/coverage.sh"
check_file "${PROJECT_ROOT}/.llvm-cov.toml"
if [[ -x "${PROJECT_ROOT}/scripts/coverage.sh" ]]; then
    check_pass "coverage.sh is executable"
else
    check_fail "coverage.sh is not executable"
fi

# Check CI/CD
print_header "CI/CD Pipeline"
check_file "${PROJECT_ROOT}/../.github/workflows/ci.yml"

# Check Cargo configuration
print_header "Cargo Configuration"
check_file "${PROJECT_ROOT}/Cargo.toml"
check_file "${PROJECT_ROOT}/smmu/Cargo.toml"
check_file "${PROJECT_ROOT}/smmu-cli/Cargo.toml"

# Check other configuration files
print_header "Configuration Files"
check_file "${PROJECT_ROOT}/rustfmt.toml"
check_file "${PROJECT_ROOT}/.clippy.toml"
check_file "${PROJECT_ROOT}/rust-toolchain.toml"
check_file "${PROJECT_ROOT}/.gitignore"

# Try to build if cargo is available
if command -v cargo &> /dev/null; then
    print_header "Build Validation"

    echo "Checking workspace build..."
    if cargo check --manifest-path="${PROJECT_ROOT}/Cargo.toml" --workspace --all-features &> /dev/null; then
        check_pass "Workspace builds successfully"
    else
        check_fail "Workspace build failed"
    fi

    echo "Checking test compilation..."
    if cargo test --manifest-path="${PROJECT_ROOT}/Cargo.toml" --workspace --all-features --no-run &> /dev/null; then
        check_pass "Tests compile successfully"
    else
        check_fail "Test compilation failed"
    fi

    echo "Checking benchmark compilation..."
    if cargo bench --manifest-path="${PROJECT_ROOT}/Cargo.toml" --workspace --all-features --no-run &> /dev/null; then
        check_pass "Benchmarks compile successfully"
    else
        check_fail "Benchmark compilation failed"
    fi
fi

# Validate JSON fixtures
print_header "JSON Fixture Validation"
if command -v python3 &> /dev/null; then
    for json_file in "${PROJECT_ROOT}/smmu/tests/fixtures"/*.json; do
        if python3 -m json.tool "$json_file" &> /dev/null; then
            check_pass "Valid JSON: $(basename "$json_file")"
        else
            check_fail "Invalid JSON: $(basename "$json_file")"
        fi
    done
else
    echo -e "${YELLOW}⚠ python3 not found, skipping JSON validation${NC}"
fi

# Summary
print_header "Validation Summary"
total=$((passed + failed))
echo "Total checks: ${total}"
echo -e "${GREEN}Passed: ${passed}${NC}"
echo -e "${RED}Failed: ${failed}${NC}"

if [[ ${failed} -eq 0 ]]; then
    echo ""
    echo -e "${GREEN}✓ All validation checks passed!${NC}"
    echo -e "${GREEN}Testing infrastructure is ready for development.${NC}"
    exit 0
else
    echo ""
    echo -e "${RED}✗ Some validation checks failed.${NC}"
    echo -e "${YELLOW}Please review the errors above and fix any issues.${NC}"
    exit 1
fi
