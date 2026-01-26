#!/usr/bin/env bash
# Setup git hooks for the Rust SMMU project
# Usage: ./scripts/setup-hooks.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "========================================="
echo "Git Hooks Setup"
echo "========================================="
echo ""

# Check if we're in a git repository
if [ ! -d "${PROJECT_ROOT}/.git" ]; then
    echo "ERROR: Not in a git repository"
    echo "Expected .git directory at: ${PROJECT_ROOT}/.git"
    exit 1
fi

# Configure git to use our custom hooks directory
echo "Configuring git to use custom hooks directory..."
cd "${PROJECT_ROOT}"
git config core.hooksPath rust/.githooks

echo "✓ Git hooks configured successfully"
echo ""
echo "Installed hooks:"
echo "  - pre-commit: Validates formatting, linting, and compilation"
echo ""
echo "Available hooks in rust/.githooks/:"
ls -1 "${PROJECT_ROOT}/rust/.githooks/"
echo ""
echo "========================================="
echo "Setup Complete"
echo "========================================="
echo ""
echo "The pre-commit hook will now run automatically on every commit."
echo "It checks:"
echo "  1. Code formatting (rustfmt)"
echo "  2. Linting (clippy with pedantic lints)"
echo "  3. Compilation (cargo check)"
echo "  4. Security audit (if cargo-deny installed)"
echo ""
echo "To bypass the hook (not recommended):"
echo "  git commit --no-verify"
echo ""
echo "To disable hooks:"
echo "  git config --unset core.hooksPath"
echo ""
