#!/usr/bin/env bash
# Security audit script using cargo-deny
# Usage: ./scripts/audit.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_ROOT}"

echo "========================================="
echo "Security Audit - ARM SMMU v3 Rust"
echo "========================================="
echo ""

# Check if cargo-deny is installed
if ! command -v cargo-deny &> /dev/null; then
    echo "ERROR: cargo-deny is not installed"
    echo "Install it with: cargo install cargo-deny"
    exit 1
fi

# Update advisory database
echo "Updating RustSec advisory database..."
cargo deny fetch advisories
echo ""

# Run all cargo-deny checks
echo "Running security audits..."
echo ""

echo "1. Checking for security vulnerabilities..."
cargo deny check advisories
echo "   ✓ Security vulnerabilities check passed"
echo ""

echo "2. Checking license compliance..."
cargo deny check licenses
echo "   ✓ License compliance check passed"
echo ""

echo "3. Checking for banned crates..."
cargo deny check bans
echo "   ✓ Banned crates check passed"
echo ""

echo "4. Checking dependency sources..."
cargo deny check sources
echo "   ✓ Dependency sources check passed"
echo ""

echo "========================================="
echo "Security Audit Complete"
echo "========================================="
echo ""
echo "All security checks passed!"
echo ""
echo "For more information:"
echo "  - Advisory database: https://rustsec.org/"
echo "  - cargo-deny config: deny.toml"
echo "  - Update database: cargo deny fetch advisories"
echo ""
