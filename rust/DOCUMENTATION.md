# Documentation Guide

This document explains how to build, view, and publish the ARM SMMU v3 Rust implementation documentation.

## Building Documentation

### Quick Build

```bash
cargo doc --open
```

This builds the documentation and opens it in your default web browser.

### Full Build with All Features

```bash
./build-docs.sh
```

This script:
1. Cleans previous documentation
2. Builds documentation with all features enabled
3. Checks for documentation warnings
4. Builds example documentation
5. Reports any issues

### Manual Build Options

#### Build with specific features

```bash
cargo doc --features serde
```

#### Build without dependencies

```bash
cargo doc --no-deps
```

#### Build with private items (internal documentation)

```bash
cargo doc --document-private-items
```

#### Build for docs.rs

```bash
RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --all-features
```

## Documentation Structure

```
target/doc/
├── smmu/                  # Main crate documentation
│   ├── index.html        # Crate root
│   ├── struct.SMMU.html  # SMMU controller
│   ├── types/            # Types module
│   ├── prelude/          # Prelude module
│   └── ...
├── src/                  # Source code viewer
└── search-index.js       # Search index
```

## Viewing Documentation

### Local Viewing

```bash
# Open in browser
cargo doc --open

# Or manually open
firefox target/doc/smmu/index.html
```

### Serving Documentation (HTTP)

```bash
# Using Python's HTTP server
cd target/doc
python3 -m http.server 8000
# Open http://localhost:8000/smmu/
```

## Documentation Quality Checks

### Check for Missing Documentation

```bash
RUSTDOCFLAGS="-D missing-docs" cargo doc
```

This treats missing documentation as errors.

### Check for Broken Links

```bash
RUSTDOCFLAGS="-D rustdoc::broken-intra-doc-links" cargo doc
```

### Check All Warnings

```bash
RUSTDOCFLAGS="-D warnings" cargo doc
```

### Run All Checks

```bash
./build-docs.sh
```

## Writing Good Documentation

### Module-Level Documentation

```rust
//! Module documentation goes here
//!
//! # Examples
//!
//! \`\`\`rust
//! use smmu::types::*;
//! let stream_id = StreamID::new(1)?;
//! \`\`\`
```

### Item Documentation

```rust
/// Brief description of the function
///
/// # Arguments
///
/// * `stream_id` - The stream identifier
/// * `config` - Stream configuration
///
/// # Returns
///
/// Returns `Ok(())` on success, or `SMMUError` on failure.
///
/// # Errors
///
/// This function will return an error if:
/// - Stream ID is invalid
/// - Configuration is invalid
///
/// # Examples
///
/// \`\`\`rust
/// use smmu::prelude::*;
///
/// let smmu = SMMU::new();
/// let stream_id = StreamID::new(1)?;
/// let config = StreamConfig::stage1_only();
/// smmu.configure_stream(stream_id, config)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// \`\`\`
///
/// # Panics
///
/// This function panics if...
///
/// # Safety
///
/// (For unsafe functions only)
/// This function is unsafe because...
pub fn configure_stream(&self, stream_id: StreamID, config: StreamConfig) -> Result<(), SMMUError> {
    // ...
}
```

### Required Documentation Sections

1. **Brief description** - What does it do?
2. **Arguments** - What are the parameters?
3. **Returns** - What does it return?
4. **Errors** - When does it fail?
5. **Examples** - How do I use it?
6. **Panics** - When does it panic? (if applicable)
7. **Safety** - Why is it safe? (for unsafe items)

### Cross-References

```rust
/// See also [`SMMU::translate`] for translation details.
/// Uses [`StreamConfig`] for configuration.
/// Related to [`types::TranslationResult`].
```

### Code Examples

All code examples should:
- Compile without errors
- Include error handling with `?`
- Add `# Ok::<(), Box<dyn std::error::Error>>(())` at the end
- Use realistic scenarios

```rust
/// # Examples
///
/// \`\`\`rust
/// use smmu::prelude::*;
///
/// let smmu = SMMU::new();
/// // Example code here...
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// \`\`\`
```

### Testing Documentation Examples

```bash
# Test all doc examples
cargo test --doc

# Test specific module's examples
cargo test --doc --package smmu
```

## Custom Styling

Documentation uses custom CSS in `rustdoc-custom.css`:

- Improved code blocks with blue left border
- Highlighted warning/note/important blocks
- Better table styling
- ARM SMMU v3 branding
- Enhanced sidebar

## Publishing to docs.rs

Documentation is automatically published to docs.rs when you publish to crates.io.

### Configuration for docs.rs

In `Cargo.toml`:

```toml
[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
targets = ["x86_64-unknown-linux-gnu"]
```

### Testing docs.rs Build Locally

```bash
RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --all-features
```

## Continuous Integration

Add to your CI pipeline:

```yaml
# GitHub Actions example
- name: Build documentation
  run: |
    ./build-docs.sh

- name: Check documentation
  run: |
    RUSTDOCFLAGS="-D warnings" cargo doc --all-features
```

## Documentation Checklist

Before publishing, ensure:

- [ ] All public items have documentation
- [ ] All examples compile and run
- [ ] No broken intra-doc links
- [ ] No documentation warnings
- [ ] Module-level docs exist for all modules
- [ ] README.md is up to date
- [ ] CHANGELOG.md is up to date
- [ ] Custom CSS renders correctly
- [ ] Examples in docs/ directory are included
- [ ] API reference is complete

## Common Issues

### Issue: Missing `#[doc]` attributes

**Solution**: Add documentation to all public items.

### Issue: Broken intra-doc links

**Solution**: Use correct paths:
- Same module: `[`Item`]`
- Different module: `[`module::Item`]`
- Crate root: `[`crate::Item`]`

### Issue: Code examples fail to compile

**Solution**: Test examples with:
```bash
cargo test --doc
```

### Issue: Documentation not showing custom CSS

**Solution**: Ensure files are in correct locations:
- `rustdoc-custom.css` in crate root
- `rustdoc-header.html` in crate root
- Referenced correctly in `config.toml`

## Tools and Resources

### Useful Commands

```bash
# Build and open
cargo doc --open

# Build with all features
cargo doc --all-features

# Build workspace
cargo doc --workspace

# Clean and rebuild
cargo clean --doc && cargo doc

# Check for issues
cargo rustdoc -- -D warnings
```

### External Tools

- [cargo-deadlinks](https://github.com/deadlinks/cargo-deadlinks) - Find broken links
- [cargo-docs](https://github.com/Manishearth/cargo-docs) - Documentation helper
- [rustdoc guide](https://doc.rust-lang.org/rustdoc/) - Official rustdoc documentation

## Maintenance

### Regular Tasks

1. **Weekly**: Build docs and check for warnings
2. **Before releases**: Full documentation review
3. **After API changes**: Update all affected documentation
4. **Monthly**: Review and update examples

### Documentation Coverage

Target: 100% of public API documented

Check coverage:
```bash
# Count undocumented items
cargo rustdoc -- -D missing-docs 2>&1 | grep "warning" | wc -l
```

## Questions?

- Check [DESIGN.md](DESIGN.md) for architecture details
- See [GUIDE.md](GUIDE.md) for usage patterns
- Read [examples/](smmu/examples/) for practical code
- Open an issue on GitHub for documentation bugs

---

**Remember**: Good documentation is as important as good code!
