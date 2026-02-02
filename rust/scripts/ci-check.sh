#!/bin/bash
# Local CI Validation Script
# Runs the same checks as CI before pushing

set -e  # Exit on error

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}    ARM SMMU v3 - Local CI Validation${NC}"
echo -e "${BLUE}================================================${NC}"
echo ""

# Track results
TOTAL=0
PASSED=0
FAILED=0
FAILED_CHECKS=()

# Function to run a check
run_check() {
    local name="$1"
    local description="$2"
    shift 2
    local cmd=("$@")

    ((TOTAL++))
    echo -e "${YELLOW}[$TOTAL]${NC} Running: $description"

    if "${cmd[@]}" > /dev/null 2>&1; then
        echo -e "     ${GREEN}✓ PASS${NC}"
        ((PASSED++))
        return 0
    else
        echo -e "     ${RED}✗ FAIL${NC}"
        ((FAILED++))
        FAILED_CHECKS+=("$name: $description")
        return 1
    fi
}

# 1. Format Check
run_check "fmt" "Format check (rustfmt)" \
    cargo fmt --all -- --check

# 2. Clippy - All Features
run_check "clippy-all" "Clippy with all features" \
    cargo clippy --all-targets --all-features -- -D warnings

# 3. Clippy - No Default Features
run_check "clippy-no-default" "Clippy with no default features" \
    cargo clippy --all-targets --no-default-features -- -D warnings

# 4. Clippy - Minimal Features
run_check "clippy-minimal" "Clippy with minimal features" \
    cargo clippy --all-targets --no-default-features --features minimal -- -D warnings

# 5. Build - Library
run_check "build-lib" "Build library" \
    cargo build --lib --all-features

# 6. Build - All Targets
run_check "build-all" "Build all targets" \
    cargo build --all-targets --all-features

# 7. Test - Library
run_check "test-lib" "Library tests" \
    cargo test --lib --all-features

# 8. Test - Integration
run_check "test-integration" "Integration tests" \
    cargo test --test '*' --all-features

# 9. Test - Doc Tests
run_check "test-doc" "Documentation tests" \
    cargo test --doc --all-features

# 10. Test - Minimal Features
run_check "test-minimal" "Tests with minimal features" \
    cargo test --no-default-features --features minimal

# 11. Test - No Default Features
run_check "test-no-default" "Tests with no default features" \
    cargo test --no-default-features

# 12. Examples
run_check "examples" "Build examples" \
    cargo build --examples --all-features

# 13. Documentation
run_check "docs" "Build documentation" \
    env RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps

# 14. Security Audit (if cargo-audit is installed)
if command -v cargo-audit &> /dev/null; then
    run_check "audit" "Security audit" \
        cargo audit
else
    echo -e "${YELLOW}[SKIP]${NC} Security audit (cargo-audit not installed)"
fi

# 15. License Check (if cargo-deny is installed)
if command -v cargo-deny &> /dev/null; then
    run_check "deny" "License and dependency check" \
        cargo deny check
else
    echo -e "${YELLOW}[SKIP]${NC} License check (cargo-deny not installed)"
fi

# Summary
echo ""
echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}                  Summary${NC}"
echo -e "${BLUE}================================================${NC}"
echo -e "Total checks:  $TOTAL"
echo -e "Passed:        ${GREEN}$PASSED${NC}"
echo -e "Failed:        ${RED}$FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed! Ready to push.${NC}"
    echo ""
    exit 0
else
    echo -e "${RED}✗ Some checks failed:${NC}"
    for check in "${FAILED_CHECKS[@]}"; do
        echo -e "  ${RED}-${NC} $check"
    done
    echo ""
    echo -e "${YELLOW}Fix the issues above before pushing.${NC}"
    echo ""
    exit 1
fi
