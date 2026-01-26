#!/usr/bin/env bash
# Benchmark execution script
# Usage: ./scripts/bench.sh [options]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_ROOT}"

# Color output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_usage() {
    echo "ARM SMMU v3 Rust - Benchmark Script"
    echo ""
    echo "Usage: ./scripts/bench.sh [command] [options]"
    echo ""
    echo "Commands:"
    echo "  all          - Run all benchmarks (default)"
    echo "  baseline     - Save baseline for comparison"
    echo "  compare      - Compare against baseline"
    echo "  specific     - Run specific benchmark"
    echo "  list         - List available benchmarks"
    echo ""
    echo "Options:"
    echo "  --save-baseline NAME  - Save results as baseline"
    echo "  --baseline NAME       - Compare against named baseline"
    echo ""
    echo "Examples:"
    echo "  ./scripts/bench.sh                           # Run all benchmarks"
    echo "  ./scripts/bench.sh baseline                  # Save baseline"
    echo "  ./scripts/bench.sh compare                   # Compare to baseline"
    echo "  ./scripts/bench.sh specific translation_bench # Run one benchmark"
    echo ""
}

bench_all() {
    echo -e "${BLUE}Running all benchmarks...${NC}"
    echo ""
    cargo bench --all-features "$@"
    echo ""
    echo -e "${GREEN}✓ Benchmarks complete${NC}"
    echo ""
    echo "Results saved in: target/criterion/"
    echo "View HTML reports in: target/criterion/*/report/index.html"
}

bench_baseline() {
    BASELINE_NAME="${1:-baseline}"
    echo -e "${BLUE}Running benchmarks and saving baseline: $BASELINE_NAME${NC}"
    echo ""
    cargo bench --all-features -- --save-baseline "$BASELINE_NAME"
    echo ""
    echo -e "${GREEN}✓ Baseline '$BASELINE_NAME' saved${NC}"
}

bench_compare() {
    BASELINE_NAME="${1:-baseline}"
    echo -e "${BLUE}Running benchmarks and comparing to baseline: $BASELINE_NAME${NC}"
    echo ""
    cargo bench --all-features -- --baseline "$BASELINE_NAME"
    echo ""
    echo -e "${GREEN}✓ Comparison complete${NC}"
}

bench_specific() {
    if [ -z "${1:-}" ]; then
        echo -e "${YELLOW}Error: Benchmark name required${NC}"
        echo "Usage: ./scripts/bench.sh specific <benchmark-name>"
        echo ""
        echo "Available benchmarks:"
        bench_list
        exit 1
    fi

    BENCH_NAME="$1"
    echo -e "${BLUE}Running benchmark: $BENCH_NAME${NC}"
    echo ""
    cargo bench --all-features --bench "$BENCH_NAME"
    echo ""
    echo -e "${GREEN}✓ Benchmark complete${NC}"
}

bench_list() {
    echo -e "${BLUE}Available benchmarks:${NC}"
    echo ""
    cargo bench --all-features --no-run 2>&1 | grep -E "bench/" | sed 's/.*bench\//  - /' || true
    echo ""
    echo "Run specific benchmark with:"
    echo "  ./scripts/bench.sh specific <benchmark-name>"
}

# Main command dispatcher
COMMAND="${1:-all}"
shift || true

case "$COMMAND" in
    all)         bench_all "$@" ;;
    baseline)    bench_baseline "$@" ;;
    compare)     bench_compare "$@" ;;
    specific)    bench_specific "$@" ;;
    list)        bench_list ;;
    help|--help|-h) print_usage ;;
    *)
        echo "Error: Unknown command '$COMMAND'"
        echo ""
        print_usage
        exit 1
        ;;
esac

# Post-benchmark info
echo ""
echo "========================================="
echo "Benchmark Resources"
echo "========================================="
echo ""
echo "View results:"
echo "  - HTML reports: target/criterion/*/report/index.html"
echo "  - Raw data: target/criterion/"
echo ""
echo "Benchmark commands:"
echo "  cargo bench --help  # More options"
echo ""
