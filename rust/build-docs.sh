#!/bin/bash
# Build documentation for ARM SMMU v3 Rust implementation

set -e

echo "Building ARM SMMU v3 documentation..."

# Clean previous build
echo "Cleaning previous documentation..."
cargo clean --doc

# Build documentation with all features
echo "Building documentation with all features..."
cargo doc \
    --no-deps \
    --all-features \
    --document-private-items \
    --workspace

# Check for documentation warnings
echo "Checking for documentation warnings..."
RUSTDOCFLAGS="-D warnings" cargo doc \
    --no-deps \
    --all-features \
    --workspace \
    2>&1 | tee doc-warnings.log

if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "WARNING: Documentation build has warnings. See doc-warnings.log"
else
    echo "✓ Documentation built successfully with no warnings"
    rm -f doc-warnings.log
fi

# Build examples documentation
echo "Building examples documentation..."
cargo doc --examples --no-deps

echo ""
echo "Documentation built successfully!"
echo "Open documentation with:"
echo "  cargo doc --open"
echo ""
echo "Or manually at:"
echo "  file://$(pwd)/target/doc/smmu/index.html"
