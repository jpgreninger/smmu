#!/usr/bin/env bash
#
# Code coverage script for ARM SMMU v3 Rust implementation
#
# This script generates code coverage reports using cargo-llvm-cov
# Coverage target: >95% for all modules
#
# Usage:
#   ./scripts/coverage.sh              # Generate coverage report
#   ./scripts/coverage.sh --html       # Generate HTML report
#   ./scripts/coverage.sh --lcov       # Generate LCOV report for CI
#   ./scripts/coverage.sh --check      # Check coverage thresholds

set -euo pipefail

# Configuration
COVERAGE_TARGET=95.0
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
COVERAGE_DIR="${PROJECT_ROOT}/target/coverage"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if cargo-llvm-cov is installed
check_dependencies() {
    if ! command -v cargo-llvm-cov &> /dev/null; then
        echo -e "${YELLOW}cargo-llvm-cov not found. Installing...${NC}"
        cargo install cargo-llvm-cov
    fi
}

# Clean previous coverage data
clean_coverage() {
    echo "Cleaning previous coverage data..."
    cargo llvm-cov clean --workspace
    rm -rf "${COVERAGE_DIR}"
    mkdir -p "${COVERAGE_DIR}"
}

# Run tests with coverage instrumentation
run_coverage() {
    echo "Running tests with coverage instrumentation..."

    cd "${PROJECT_ROOT}/smmu"

    # Run unit tests and integration tests
    cargo llvm-cov \
        --workspace \
        --all-features \
        --codecov \
        --output-path "${COVERAGE_DIR}/codecov.json"

    echo -e "${GREEN}Coverage data generated: ${COVERAGE_DIR}/codecov.json${NC}"
}

# Generate HTML report
generate_html() {
    echo "Generating HTML coverage report..."

    cd "${PROJECT_ROOT}/smmu"

    cargo llvm-cov \
        --workspace \
        --all-features \
        --html \
        --output-dir "${COVERAGE_DIR}/html"

    echo -e "${GREEN}HTML report generated: ${COVERAGE_DIR}/html/index.html${NC}"

    # Try to open in browser (best effort)
    if command -v xdg-open &> /dev/null; then
        xdg-open "${COVERAGE_DIR}/html/index.html" 2>/dev/null || true
    elif command -v open &> /dev/null; then
        open "${COVERAGE_DIR}/html/index.html" 2>/dev/null || true
    fi
}

# Generate LCOV report for CI systems
generate_lcov() {
    echo "Generating LCOV coverage report..."

    cd "${PROJECT_ROOT}/smmu"

    cargo llvm-cov \
        --workspace \
        --all-features \
        --lcov \
        --output-path "${COVERAGE_DIR}/lcov.info"

    echo -e "${GREEN}LCOV report generated: ${COVERAGE_DIR}/lcov.info${NC}"
}

# Generate summary report
generate_summary() {
    echo "Generating coverage summary..."

    cd "${PROJECT_ROOT}/smmu"

    cargo llvm-cov \
        --workspace \
        --all-features \
        --summary-only
}

# Check coverage thresholds
check_thresholds() {
    echo "Checking coverage thresholds (target: ${COVERAGE_TARGET}%)..."

    cd "${PROJECT_ROOT}/smmu"

    # Get coverage percentage
    local coverage_output
    coverage_output=$(cargo llvm-cov --workspace --all-features --summary-only 2>&1)

    # Extract coverage percentage (this is a simplified extraction)
    # The actual format may vary, adjust regex as needed
    local coverage_pct
    coverage_pct=$(echo "$coverage_output" | grep -oP 'TOTAL.*\K\d+\.\d+(?=%)' | head -1 || echo "0")

    echo "Current coverage: ${coverage_pct}%"

    # Compare with threshold
    if (( $(echo "$coverage_pct >= $COVERAGE_TARGET" | bc -l) )); then
        echo -e "${GREEN}✓ Coverage threshold met: ${coverage_pct}% >= ${COVERAGE_TARGET}%${NC}"
        return 0
    else
        echo -e "${RED}✗ Coverage threshold not met: ${coverage_pct}% < ${COVERAGE_TARGET}%${NC}"
        return 1
    fi
}

# Show usage
usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Generate code coverage reports for ARM SMMU v3 Rust implementation.

OPTIONS:
    --html          Generate HTML report (interactive)
    --lcov          Generate LCOV report (for CI integration)
    --check         Check coverage against thresholds
    --summary       Show coverage summary only
    --all           Generate all report types
    --help, -h      Show this help message

EXAMPLES:
    $0                      # Generate JSON coverage report
    $0 --html               # Generate HTML report and open in browser
    $0 --check              # Check if coverage meets 95% threshold
    $0 --all                # Generate all report types

COVERAGE TARGET:
    Line coverage: >=${COVERAGE_TARGET}%
    Branch coverage: Best effort

REQUIREMENTS:
    - cargo-llvm-cov (will be installed if missing)
    - Rust toolchain with llvm-tools-preview component

EOF
}

# Main script
main() {
    local mode="json"

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --html)
                mode="html"
                shift
                ;;
            --lcov)
                mode="lcov"
                shift
                ;;
            --check)
                mode="check"
                shift
                ;;
            --summary)
                mode="summary"
                shift
                ;;
            --all)
                mode="all"
                shift
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            *)
                echo -e "${RED}Unknown option: $1${NC}"
                usage
                exit 1
                ;;
        esac
    done

    echo "ARM SMMU v3 Rust - Code Coverage Report"
    echo "========================================"
    echo ""

    check_dependencies
    clean_coverage

    case $mode in
        html)
            generate_html
            ;;
        lcov)
            generate_lcov
            ;;
        check)
            run_coverage
            check_thresholds
            ;;
        summary)
            generate_summary
            ;;
        all)
            run_coverage
            generate_html
            generate_lcov
            generate_summary
            check_thresholds || true
            ;;
        json)
            run_coverage
            generate_summary
            ;;
    esac

    echo ""
    echo -e "${GREEN}Coverage analysis complete!${NC}"
}

main "$@"
