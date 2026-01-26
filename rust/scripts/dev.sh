#!/usr/bin/env bash
# Development helper script - common development commands
# Usage: ./scripts/dev.sh [command]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_ROOT}"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_usage() {
    echo "ARM SMMU v3 Rust - Development Helper"
    echo ""
    echo "Usage: ./scripts/dev.sh [command]"
    echo ""
    echo "Commands:"
    echo "  check         - Quick check (fmt + clippy + check)"
    echo "  fmt           - Format code"
    echo "  lint          - Run clippy lints"
    echo "  build         - Build all targets"
    echo "  test          - Run all tests"
    echo "  bench         - Run benchmarks"
    echo "  doc           - Generate and open documentation"
    echo "  clean         - Clean build artifacts"
    echo "  audit         - Run security audit"
    echo "  outdated      - Check for outdated dependencies"
    echo "  coverage      - Generate code coverage report"
    echo "  watch         - Watch for changes and run checks"
    echo "  install-tools - Install required development tools"
    echo "  help          - Show this help message"
    echo ""
    echo "Examples:"
    echo "  ./scripts/dev.sh check      # Quick validation before commit"
    echo "  ./scripts/dev.sh test       # Run full test suite"
    echo "  ./scripts/dev.sh watch      # Auto-run checks on file changes"
    echo ""
}

cmd_check() {
    echo -e "${BLUE}Running quick checks...${NC}"
    echo ""

    echo -e "${YELLOW}1. Formatting check...${NC}"
    cargo fmt --all -- --check
    echo -e "${GREEN}✓ Formatting OK${NC}"
    echo ""

    echo -e "${YELLOW}2. Clippy lints...${NC}"
    cargo clippy --all-targets --all-features -- -D warnings
    echo -e "${GREEN}✓ Clippy OK${NC}"
    echo ""

    echo -e "${YELLOW}3. Compilation check...${NC}"
    cargo check --all-targets --all-features
    echo -e "${GREEN}✓ Compilation OK${NC}"
    echo ""

    echo -e "${GREEN}All checks passed!${NC}"
}

cmd_fmt() {
    echo -e "${BLUE}Formatting code...${NC}"
    cargo fmt --all
    echo -e "${GREEN}✓ Code formatted${NC}"
}

cmd_lint() {
    echo -e "${BLUE}Running clippy lints...${NC}"
    cargo clippy --all-targets --all-features -- \
        -W clippy::pedantic \
        -W clippy::nursery \
        -W clippy::cargo \
        -D warnings
    echo -e "${GREEN}✓ Linting complete${NC}"
}

cmd_build() {
    echo -e "${BLUE}Building all targets...${NC}"
    cargo build --all-targets --all-features
    echo -e "${GREEN}✓ Build complete${NC}"
}

cmd_test() {
    echo -e "${BLUE}Running all tests...${NC}"
    cargo test --all-targets --all-features
    echo -e "${GREEN}✓ Tests complete${NC}"
}

cmd_bench() {
    echo -e "${BLUE}Running benchmarks...${NC}"
    cargo bench
    echo -e "${GREEN}✓ Benchmarks complete${NC}"
}

cmd_doc() {
    echo -e "${BLUE}Generating documentation...${NC}"
    cargo doc --no-deps --all-features --open
    echo -e "${GREEN}✓ Documentation generated${NC}"
}

cmd_clean() {
    echo -e "${BLUE}Cleaning build artifacts...${NC}"
    cargo clean
    echo -e "${GREEN}✓ Clean complete${NC}"
}

cmd_audit() {
    echo -e "${BLUE}Running security audit...${NC}"
    ./scripts/audit.sh
}

cmd_outdated() {
    echo -e "${BLUE}Checking for outdated dependencies...${NC}"
    ./scripts/check-outdated.sh
}

cmd_coverage() {
    echo -e "${BLUE}Generating coverage report...${NC}"
    ./scripts/coverage.sh
}

cmd_watch() {
    if ! command -v cargo-watch &> /dev/null; then
        echo -e "${RED}ERROR: cargo-watch not installed${NC}"
        echo "Install with: cargo install cargo-watch"
        exit 1
    fi

    echo -e "${BLUE}Watching for changes...${NC}"
    echo "Press Ctrl+C to stop"
    echo ""
    cargo watch -x 'fmt --all' -x 'clippy --all-targets --all-features' -x 'check --all-targets'
}

cmd_install_tools() {
    echo -e "${BLUE}Installing development tools...${NC}"
    echo ""

    tools=(
        "cargo-watch"
        "cargo-deny"
        "cargo-outdated"
        "cargo-audit"
        "cargo-llvm-cov"
        "cargo-expand"
    )

    for tool in "${tools[@]}"; do
        if command -v "$tool" &> /dev/null; then
            echo -e "${GREEN}✓ $tool already installed${NC}"
        else
            echo -e "${YELLOW}Installing $tool...${NC}"
            cargo install "$tool"
            echo -e "${GREEN}✓ $tool installed${NC}"
        fi
    done

    echo ""
    echo -e "${GREEN}All tools installed!${NC}"
}

# Main command dispatcher
COMMAND="${1:-help}"

case "$COMMAND" in
    check)      cmd_check ;;
    fmt)        cmd_fmt ;;
    lint)       cmd_lint ;;
    build)      cmd_build ;;
    test)       cmd_test ;;
    bench)      cmd_bench ;;
    doc)        cmd_doc ;;
    clean)      cmd_clean ;;
    audit)      cmd_audit ;;
    outdated)   cmd_outdated ;;
    coverage)   cmd_coverage ;;
    watch)      cmd_watch ;;
    install-tools) cmd_install_tools ;;
    help|--help|-h) print_usage ;;
    *)
        echo -e "${RED}Error: Unknown command '$COMMAND'${NC}"
        echo ""
        print_usage
        exit 1
        ;;
esac
