#!/usr/bin/env bash
# Comprehensive test execution script
# Usage: ./scripts/test.sh [test-type]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_ROOT}"

# Color output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

print_usage() {
    echo "ARM SMMU v3 Rust - Test Script"
    echo ""
    echo "Usage: ./scripts/test.sh [test-type] [options]"
    echo ""
    echo "Test Types:"
    echo "  all          - Run all tests (default)"
    echo "  unit         - Run unit tests only"
    echo "  integration  - Run integration tests only"
    echo "  doc          - Run documentation tests"
    echo "  quick        - Run tests without compilation optimization"
    echo "  verbose      - Run tests with verbose output"
    echo "  specific     - Run specific test (requires test name)"
    echo ""
    echo "Options:"
    echo "  --nocapture  - Show test output (stdout/stderr)"
    echo "  --ignored    - Run ignored tests"
    echo "  --threads N  - Number of test threads"
    echo ""
    echo "Examples:"
    echo "  ./scripts/test.sh                    # Run all tests"
    echo "  ./scripts/test.sh unit               # Run unit tests"
    echo "  ./scripts/test.sh verbose            # Verbose output"
    echo "  ./scripts/test.sh specific test_name # Run specific test"
    echo ""
}

test_all() {
    echo -e "${BLUE}Running all tests...${NC}"
    echo ""
    cargo test --all-targets --all-features "$@"
    echo ""
    echo -e "${GREEN}✓ All tests passed${NC}"
}

test_unit() {
    echo -e "${BLUE}Running unit tests...${NC}"
    echo ""
    cargo test --lib --all-features "$@"
    echo ""
    echo -e "${GREEN}✓ Unit tests passed${NC}"
}

test_integration() {
    echo -e "${BLUE}Running integration tests...${NC}"
    echo ""
    cargo test --test '*' --all-features "$@"
    echo ""
    echo -e "${GREEN}✓ Integration tests passed${NC}"
}

test_doc() {
    echo -e "${BLUE}Running documentation tests...${NC}"
    echo ""
    cargo test --doc --all-features "$@"
    echo ""
    echo -e "${GREEN}✓ Documentation tests passed${NC}"
}

test_quick() {
    echo -e "${BLUE}Running quick tests (dev build)...${NC}"
    echo ""
    cargo test --all-targets --all-features --no-fail-fast "$@"
    echo ""
    echo -e "${GREEN}✓ Quick tests passed${NC}"
}

test_verbose() {
    echo -e "${BLUE}Running tests with verbose output...${NC}"
    echo ""
    cargo test --all-targets --all-features --verbose -- --nocapture "$@"
    echo ""
    echo -e "${GREEN}✓ Tests complete${NC}"
}

test_specific() {
    if [ -z "${2:-}" ]; then
        echo -e "${RED}Error: Test name required${NC}"
        echo "Usage: ./scripts/test.sh specific <test-name>"
        exit 1
    fi

    TEST_NAME="$2"
    echo -e "${BLUE}Running test: $TEST_NAME${NC}"
    echo ""
    cargo test "$TEST_NAME" --all-features -- --nocapture --exact
    echo ""
    echo -e "${GREEN}✓ Test complete${NC}"
}

# Main command dispatcher
TEST_TYPE="${1:-all}"
shift || true  # Remove first argument, keep rest

case "$TEST_TYPE" in
    all)         test_all "$@" ;;
    unit)        test_unit "$@" ;;
    integration) test_integration "$@" ;;
    doc)         test_doc "$@" ;;
    quick)       test_quick "$@" ;;
    verbose)     test_verbose "$@" ;;
    specific)    test_specific "$TEST_TYPE" "$@" ;;
    help|--help|-h) print_usage ;;
    *)
        echo -e "${RED}Error: Unknown test type '$TEST_TYPE'${NC}"
        echo ""
        print_usage
        exit 1
        ;;
esac

# Test Summary
echo ""
echo "========================================="
echo "Test Summary"
echo "========================================="
echo ""
echo "For more test commands:"
echo "  cargo test --help"
echo ""
echo "Run with coverage:"
echo "  ./scripts/coverage.sh"
echo ""
