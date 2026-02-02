#!/usr/bin/env python3
"""
Cross-Platform Testing Script for ARM SMMU v3 Rust Implementation
Tests compilation on all supported platforms
"""

import subprocess
import sys

# ANSI color codes
RED = '\033[0;31m'
GREEN = '\033[0;32m'
YELLOW = '\033[1;33m'
NC = '\033[0m'  # No Color

def test_target(target, description):
    """Test compilation for a specific target."""
    print(f"Testing {description} ({target})... ", end='', flush=True)

    # Check if target is installed
    result = subprocess.run(
        ['rustup', 'target', 'list', '--installed'],
        capture_output=True,
        text=True
    )

    if target not in result.stdout:
        print(f"{YELLOW}SKIP{NC} (target not installed)")
        print(f"  Install with: rustup target add {target}")
        return 'skip'

    # Test compilation
    result = subprocess.run(
        ['cargo', 'check', '--target', target, '--all-features'],
        capture_output=True,
        text=True
    )

    if result.returncode == 0:
        print(f"{GREEN}PASS{NC}")
        return 'pass'
    else:
        print(f"{RED}FAIL{NC}")
        print(f"  Error: {result.stderr[:200]}")
        return 'fail'

def main():
    """Main test function."""
    print("=" * 41)
    print("Cross-Platform Compilation Test")
    print("ARM SMMU v3 Rust Implementation")
    print("=" * 41)
    print()

    # Test targets
    targets = [
        ("x86_64-unknown-linux-gnu", "Linux x86_64 (GNU)"),
        ("x86_64-unknown-linux-musl", "Linux x86_64 (musl)"),
        ("aarch64-unknown-linux-gnu", "Linux ARM64"),
        ("x86_64-pc-windows-msvc", "Windows x86_64 (MSVC)"),
        ("x86_64-pc-windows-gnu", "Windows x86_64 (GNU)"),
        ("x86_64-apple-darwin", "macOS x86_64 (Intel)"),
        ("aarch64-apple-darwin", "macOS ARM64 (Apple Silicon)"),
    ]

    results = {'pass': 0, 'fail': 0, 'skip': 0}

    for target, description in targets:
        result = test_target(target, description)
        results[result] += 1

    # Print summary
    print()
    print("=" * 41)
    print("Results Summary")
    print("=" * 41)
    total = sum(results.values())
    print(f"Total:   {total} targets")
    print(f"Passed:  {GREEN}{results['pass']}{NC}")
    print(f"Skipped: {YELLOW}{results['skip']}{NC}")
    print(f"Failed:  {RED}{results['fail']}{NC}")
    print()

    if results['fail'] == 0:
        print(f"{GREEN}✓ All tests passed!{NC}")
        return 0
    else:
        print(f"{RED}✗ Some tests failed{NC}")
        return 1

if __name__ == '__main__':
    sys.exit(main())
