#!/usr/bin/env bash
# Build script with various configurations
# Usage: ./scripts/build.sh [profile]

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
    echo "ARM SMMU v3 Rust - Build Script"
    echo ""
    echo "Usage: ./scripts/build.sh [profile]"
    echo ""
    echo "Profiles:"
    echo "  dev          - Development build (default, fast compile)"
    echo "  release      - Release build (optimized)"
    echo "  bench        - Benchmark build (optimized + bench features)"
    echo "  all-targets  - Build all targets (bins, tests, benches)"
    echo "  check        - Quick check without full build"
    echo ""
    echo "Examples:"
    echo "  ./scripts/build.sh           # Development build"
    echo "  ./scripts/build.sh release   # Optimized release build"
    echo "  ./scripts/build.sh check     # Fast syntax check"
    echo ""
}

build_dev() {
    echo -e "${BLUE}Building in development mode...${NC}"
    cargo build --all-features
    echo -e "${GREEN}✓ Development build complete${NC}"
    echo ""
    echo "Binary location: target/debug/smmu-cli"
}

build_release() {
    echo -e "${BLUE}Building optimized release...${NC}"
    cargo build --release --all-features
    echo -e "${GREEN}✓ Release build complete${NC}"
    echo ""
    echo "Binary location: target/release/smmu-cli"
    echo ""
    echo "Build info:"
    ls -lh target/release/smmu-cli 2>/dev/null || true
}

build_bench() {
    echo -e "${BLUE}Building benchmarks...${NC}"
    cargo bench --no-run
    echo -e "${GREEN}✓ Benchmark build complete${NC}"
}

build_all_targets() {
    echo -e "${BLUE}Building all targets...${NC}"
    cargo build --all-targets --all-features
    echo -e "${GREEN}✓ All targets built${NC}"
}

build_check() {
    echo -e "${BLUE}Running quick check...${NC}"
    cargo check --all-targets --all-features
    echo -e "${GREEN}✓ Check complete${NC}"
}

# Parse profile argument
PROFILE="${1:-dev}"

case "$PROFILE" in
    dev|debug)       build_dev ;;
    release)         build_release ;;
    bench|benchmark) build_bench ;;
    all-targets|all) build_all_targets ;;
    check)           build_check ;;
    help|--help|-h)  print_usage ;;
    *)
        echo "Error: Unknown profile '$PROFILE'"
        echo ""
        print_usage
        exit 1
        ;;
esac
