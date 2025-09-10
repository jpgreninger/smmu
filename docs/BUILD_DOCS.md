# ARM SMMU v3 Documentation Generation

## Overview

This document describes how to generate the comprehensive API reference documentation for the ARM SMMU v3 C++11 implementation using Doxygen.

## Prerequisites

- **Doxygen**: Version 1.8.13 or higher
- **GraphViz**: For generating collaboration diagrams and call graphs (optional but recommended)
- **LaTeX**: For PDF documentation generation (optional)

### Installing Prerequisites

**Ubuntu/Debian:**
```bash
sudo apt-get install doxygen graphviz texlive-latex-base texlive-latex-extra
```

**CentOS/RHEL:**
```bash
sudo yum install doxygen graphviz texlive-latex texlive-latex-extra
```

**macOS:**
```bash
brew install doxygen graphviz
brew install --cask mactex  # For LaTeX support
```

## Building Documentation

### Quick Start
```bash
# From project root
mkdir -p build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release -DBUILD_DOCUMENTATION=ON
make docs
```

### Manual Configuration
```bash
# Enable documentation in CMake
cmake .. -DBUILD_DOCUMENTATION=ON

# Generate documentation
make docs

# Generate and open documentation
make docs-open
```

### Documentation Targets
- `docs` - Generate HTML and LaTeX documentation
- `docs-open` - Generate documentation and open in browser

## Output Structure

Generated documentation is placed in `build/docs/`:
```
build/docs/
├── html/           # HTML documentation
│   ├── index.html  # Main documentation page
│   ├── classes.html
│   ├── files.html
│   └── ...
└── latex/          # LaTeX documentation
    ├── Makefile
    ├── refman.tex
    └── ...
```

### Accessing Documentation

**HTML Version (Recommended):**
- Open `build/docs/html/index.html` in your web browser
- Fully interactive with search, cross-references, and diagrams

**PDF Version:**
```bash
cd build/docs/latex
make
# Generates refman.pdf
```

## Documentation Features

### Generated Content
- **API Reference**: Complete class and function documentation
- **Call Graphs**: Function call relationships with GraphViz
- **Collaboration Diagrams**: Class interaction diagrams
- **Source Browser**: Hyperlinked source code
- **Cross-References**: Navigate between related elements
- **Search Functionality**: Full-text search within documentation

### Covered Components
- `SMMU` - Main controller class
- `AddressSpace` - Translation context management
- `StreamContext` - Per-stream state and PASID management
- `TLBCache` - High-performance caching layer
- `FaultHandler` - Fault detection and recovery
- `Configuration` - SMMU configuration management
- `MemoryPool` - Memory resource management
- **Core Types** - Protocol enums, structs, and type definitions

### Documentation Standards
- **ARM SMMU v3 Compliance**: All interfaces documented per ARM specification
- **Thread Safety**: Concurrent usage patterns clearly documented
- **Performance Notes**: Algorithmic complexity and optimization details
- **Usage Examples**: Practical code examples for key functionality
- **Error Handling**: Complete error condition documentation

## Customization

### Modifying Configuration
The main Doxygen configuration is in `Doxyfile`. Key settings:

- **Input Sources**: `INPUT` - Files and directories to document
- **Output Formats**: `GENERATE_HTML`, `GENERATE_LATEX`
- **Diagrams**: `HAVE_DOT`, `CALL_GRAPH`, `COLLABORATION_GRAPH`
- **Styling**: `HTML_COLORSTYLE_*` settings

### Adding Custom Content
- **Main Page**: Edit `README.md` (used as main documentation page)
- **Custom Sections**: Add markdown files to `INPUT` in Doxyfile
- **Code Examples**: Place in `examples/` directory

## Troubleshooting

### Common Issues

**"Doxygen not found"**
```bash
# Install Doxygen or ensure it's in PATH
which doxygen
```

**"GraphViz diagrams not generated"**
```bash
# Install GraphViz
sudo apt-get install graphviz  # Ubuntu/Debian
brew install graphviz          # macOS
```

**"LaTeX documentation fails"**
```bash
# Install complete LaTeX distribution
sudo apt-get install texlive-full  # Ubuntu/Debian
```

**"Documentation appears empty"**
- Ensure header files contain Doxygen comments (`/** */`)
- Check `INPUT` paths in Doxyfile are correct
- Verify `FILE_PATTERNS` includes your file types

### Performance Notes
- Generation time: ~30-60 seconds for complete documentation
- Output size: ~50MB for HTML, ~5MB for PDF
- GraphViz diagram generation adds ~15 seconds but significantly improves documentation quality

## Integration with IDEs

### Visual Studio Code
1. Install "Doxygen Documentation Generator" extension
2. Generated docs integrate with IntelliSense

### Qt Creator
1. Generated documentation appears in Help mode
2. Context-sensitive help for SMMU classes

### CLion
1. External tool integration for documentation generation
2. Built-in Doxygen comment generation

## Continuous Integration

Example GitHub Actions integration:
```yaml
- name: Generate Documentation
  run: |
    mkdir build && cd build
    cmake .. -DBUILD_DOCUMENTATION=ON
    make docs
    
- name: Deploy Documentation
  uses: peaceiris/actions-gh-pages@v3
  with:
    github_token: ${{ secrets.GITHUB_TOKEN }}
    publish_dir: ./build/docs/html
```

## Maintenance

### Keeping Documentation Current
- **Regular Updates**: Regenerate documentation after significant code changes
- **Review Coverage**: Use Doxygen warnings to identify undocumented code
- **Version Tagging**: Update `PROJECT_NUMBER` in Doxyfile for releases

### Quality Assurance
- All public APIs must have comprehensive documentation
- Examples should compile and run correctly
- Cross-references should be accurate and complete

For questions or issues with documentation generation, please refer to the main project README or create an issue in the project repository.