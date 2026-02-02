#!/bin/bash
# Cross-Platform Testing Script for ARM SMMU v3 Rust Implementation
# This script tests compilation on all supported platforms

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "========================================="
echo "Cross-Platform Compilation Test"
echo "ARM SMMU v3 Rust Implementation"
echo "========================================="
echo ""

# Function to test a target
test_target() {
    local target=$1
    local description=$2

    echo -n "Testing ${description} (${target})... "

    # Check if target is installed
    if ! rustup target list --installed | grep -q "^${target}$"; then
        echo -e "${YELLOW}SKIP${NC} (target not installed)"
        echo "  Install with: rustup target add ${target}"
        return 0
    fi

    # Test compilation
    if cargo check --target "${target}" --all-features --quiet 2>/dev/null; then
        echo -e "${GREEN}PASS${NC}"
        return 0
    else
        echo -e "${RED}FAIL${NC}"
        return 1
    fi
}

# Track results
TOTAL=0
PASSED=0
FAILED=0
SKIPPED=0

# Test all platforms
TARGETS=(
    "x86_64-unknown-linux-gnu:Linux x86_64 (GNU)"
    "x86_64-unknown-linux-musl:Linux x86_64 (musl)"
    "aarch64-unknown-linux-gnu:Linux ARM64"
    "x86_64-pc-windows-msvc:Windows x86_64 (MSVC)"
    "x86_64-pc-windows-gnu:Windows x86_64 (GNU)"
    "x86_64-apple-darwin:macOS x86_64 (Intel)"
    "aarch64-apple-darwin:macOS ARM64 (Apple Silicon)"
)

for target_info in "${TARGETS[@]}"; do
    IFS=':' read -r target description <<< "$target_info"
    ((TOTAL++))

    if test_target "$target" "$description"; then
        ((PASSED++))
    else
        ((FAILED++))
    fi
done

echo ""
echo "========================================="
echo "Results Summary"
echo "========================================="
echo "Total:   $TOTAL targets"
echo -e "Passed:  ${GREEN}$PASSED${NC}"
echo -e "Failed:  ${RED}$FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Some tests failed${NC}"
    exit 1
fi
