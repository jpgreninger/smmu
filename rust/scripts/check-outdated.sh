#!/usr/bin/env bash
# Check for outdated dependencies using cargo-outdated
# Usage: ./scripts/check-outdated.sh [--aggressive]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_ROOT}"

echo "========================================="
echo "Checking for Outdated Dependencies"
echo "========================================="
echo ""

# Check if cargo-outdated is installed
if ! command -v cargo-outdated &> /dev/null; then
    echo "ERROR: cargo-outdated is not installed"
    echo "Install it with: cargo install cargo-outdated"
    exit 1
fi

# Parse command line arguments
AGGRESSIVE=false
if [[ "${1:-}" == "--aggressive" ]]; then
    AGGRESSIVE=true
fi

# Run cargo-outdated with appropriate flags
echo "Checking for outdated dependencies..."
echo ""

if [ "$AGGRESSIVE" = true ]; then
    echo "Running in aggressive mode (includes all updates)..."
    cargo outdated --root-deps-only --verbose
else
    echo "Running in conservative mode (compatible updates only)..."
    cargo outdated --root-deps-only --verbose --compatible
fi

echo ""
echo "========================================="
echo "Dependency Update Strategy"
echo "========================================="
echo ""
echo "Conservative Updates (default):"
echo "  - Only show updates compatible with current version constraints"
echo "  - Safe to apply without major changes"
echo "  - Run: cargo update"
echo ""
echo "Aggressive Updates (--aggressive flag):"
echo "  - Show all available updates including breaking changes"
echo "  - May require code changes"
echo "  - Manually update Cargo.toml version constraints"
echo ""
echo "Security Updates:"
echo "  - Run: ./scripts/audit.sh to check for security advisories"
echo "  - Update vulnerable dependencies immediately"
echo ""
echo "Before updating dependencies:"
echo "  1. Review CHANGELOG for breaking changes"
echo "  2. Run full test suite: ./scripts/test.sh"
echo "  3. Check for security advisories: ./scripts/audit.sh"
echo "  4. Update one dependency at a time for large updates"
echo ""
