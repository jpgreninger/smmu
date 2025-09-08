# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a comprehensive ARM SMMU (System Memory Management Unit) v3 implementation following the ARM SMMU v3 specification, providing a C++11-compliant software model for development, simulation, and testing environments.

## Build System

**IMPORTANT**: Always build in the `build/` subdirectory for out-of-source builds. Never build in the source root.

### Main Build Commands
```bash
# REQUIRED: Always use build directory for out-of-source builds
mkdir -p build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=11
make -j$(nproc)

# Debug build
cd build
cmake .. -DCMAKE_BUILD_TYPE=Debug -DCMAKE_CXX_STANDARD=11
make -j$(nproc)

# Build with testing enabled
cd build
cmake .. -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
make -j$(nproc)
```

### Test Execution
```bash
# REQUIRED: Always run tests from build directory
cd build

# Run all tests
make test
# or
ctest --output-on-failure

# Run specific test categories
make run_unit_tests           # Unit tests
make run_integration_tests    # Integration tests
make run_performance_tests    # Performance benchmarks
make run_validation_tests     # SMMU specification compliance
```

## Code Architecture

### Core Library Structure (`src/`)
- **Core Types**: `src/types/` - StreamID, PASID, address types, enums
- **Address Space**: `src/address_space/` - Page table management, translation logic
- **Stream Context**: `src/stream_context/` - Per-stream state and PASID management
- **SMMU Controller**: `src/smmu/` - Main SMMU class and translation engine
- **Fault Handling**: `src/fault/` - Fault detection, classification, and recovery
- **Caching**: `src/cache/` - TLB implementation and cache management

### Header Organization
- **Public API**: `include/smmu/` - Public interfaces and main SMMU class
- **Types**: `include/smmu/types.h` - Core protocol types and enums
- **Address Space**: `include/smmu/address_space.h` - AddressSpace class interface
- **Stream Context**: `include/smmu/stream_context.h` - StreamContext class interface
- **Fault System**: `include/smmu/fault.h` - Fault handling interfaces

### Key Design Patterns
- **Sparse Data Structures**: Using `std::unordered_map` for efficient memory usage in large address spaces
- **RAII**: Smart pointers (`std::unique_ptr`, `std::shared_ptr`) for automatic resource management
- **Template Specialization**: Templates with explicit specializations in `.cpp` files for clean API
- **State Machine**: Stream and translation state management using strongly-typed enums
- **Result Pattern**: Error handling through return values rather than exceptions

### C++11 Compliance Requirements
- **Strict C++11**: No C++14/C++17/C++20 features allowed
- **STL Only**: No external dependencies beyond C++11 standard library
- **Move Semantics**: Efficient resource transfer using move constructors/assignment
- **Smart Pointers**: Automatic memory management with `std::unique_ptr` and `std::shared_ptr`
- **Strongly-Typed Enums**: `enum class` for type safety

## Development Workflows

### ⚠️ CRITICAL DEVELOPMENT REQUIREMENTS ⚠️

**ABSOLUTELY MANDATORY**: Use the following subagents for ALL development work:

- **cpp-pro**: ALWAYS use for implementing new C++ code
- **debugger**: ALWAYS use for debugging compile errors and runtime bugs  
- **qa-engineer**: ALWAYS use after each step to review against ARM SMMU v3 specification, update TASKS.md
- **test-writer-fixer**: ALWAYS use to write tests and integrate into regression suite

**FAILURE TO USE THESE SUBAGENTS WILL RESULT IN INCOMPLETE/NON-COMPLIANT IMPLEMENTATIONS**

### Required Subagent Usage
**CRITICAL**: Always use these specialized subagents for development tasks:

- **cpp-pro**: **ALWAYS** use for implementing new C++ code. Use PROACTIVELY for C++ refactoring, performance optimization, or complex template solutions.
- **debugger**: **ALWAYS** use for debugging compile errors, runtime bugs, and build issues. Use proactively when encountering any compilation or runtime issues.
- **qa-engineer**: **ALWAYS** use after each development step to review code against ARM SMMU v3 specification and update TASKS.md with missing features. Use proactively to ensure compliance and code quality.
- **test-writer-fixer**: **ALWAYS** use to write comprehensive tests for each implementation step and integrate into overall regression test suite. Use proactively after code modifications to ensure comprehensive test coverage and suite health.

**MANDATORY WORKFLOW**: 
1. Use `cpp-pro` for all code implementation
2. Use `debugger` immediately when compilation errors or bugs occur
3. Use `qa-engineer` after each implementation step for compliance review
4. Use `test-writer-fixer` to create/update tests and integrate into regression suite

### Adding New Features
**MANDATORY STEP-BY-STEP WORKFLOW:**
1. **Planning**: Use `protocol-modeler` to review requirements against ARM SMMU v3 specification
2. **Test-Driven Development**: Add comprehensive unit tests in `tests/` using `test-writer-fixer` that fail before implementation
3. **Build Verification**: Build tests and verify they fail before implementing code
4. **Implementation**: Implement in corresponding `src/` subdirectory using `cpp-pro`
5. **Debug Issues**: Use `debugger` for any compilation errors or runtime issues
6. **Code Review**: Use `qa-engineer` to review implementation against ARM SMMU v3 spec, update TASKS.md
7. **Test Integration**: Use `test-writer-fixer` to ensure tests pass and integrate into regression suite
8. **Final Validation**: Use `qa-engineer` to verify complete compliance and >95% test coverage

**IMPORTANT**: Never proceed to next step without using the required subagent for current step.

### Core Implementation Classes

#### SMMU Class (Main Controller)
```cpp
class SMMU {
public:
    // Main translation API
    TranslationResult translate(uint32_t streamID, uint32_t pasid, 
                               uint64_t iova, AccessType access);
    
    // Stream management
    void configureStream(uint32_t streamID, const StreamConfig& config);
    
    // Event handling
    std::vector<FaultRecord> getEvents();
};
```

#### AddressSpace Class (Translation Context)
```cpp
class AddressSpace {
    std::unordered_map<uint64_t, PageEntry> pageTable; // Sparse representation
    
public:
    void mapPage(uint64_t iova, uint64_t pa, PagePermissions perms);
    TranslationResult translatePage(uint64_t iova, AccessType access);
};
```

#### StreamContext Class (Per-Stream State)
```cpp
class StreamContext {
    std::unordered_map<uint32_t, std::shared_ptr<AddressSpace>> pasidMap;
    
public:
    TranslationResult translate(uint32_t pasid, uint64_t iova, AccessType access);
};
```

## Coding Standards

**CRITICAL**: All source files must end in `.cpp` instead of `.cc`. Update any files with `.cc` suffix immediately.

### Naming Conventions
- **Classes**: PascalCase (`SMMU`, `StreamContext`, `AddressSpace`)
- **Functions/Methods**: camelCase (`translateAddress`, `mapPage`)
- **Variables**: camelCase (`streamID`, `pageEntry`)
- **Constants**: ALL_CAPS (`PAGE_SIZE`, `MAX_STREAM_ID`)
- **Enums**: PascalCase with scoped values (`AccessType::Read`)

### Code Style Requirements
- **Indentation**: Always use 4 spaces, never tabs
- **Braces**: K&R style with opening brace on same line
- **Line Length**: Maximum 120 characters
- **Control Flow**: Prefer case statements over if/else chains when >1 else-if
- **Carriage Returns**: Always add carriage return after every closing curly brace
- **Forward Declarations**: Never allow forward declarations - include full headers
- **Templates**: Prefer templates over function overloads, specialize in `.cpp` files

### Template Implementation Pattern
```cpp
// In header file (include/smmu/template_class.h)
template<typename T>
class TemplateClass {
public:
    void method(const T& value);
};

// In implementation file (src/template_class.cpp)
template class TemplateClass<uint32_t>;  // Explicit instantiation
template class TemplateClass<uint64_t>;  // Explicit instantiation

template<typename T>
void TemplateClass<T>::method(const T& value) {
    // Implementation here
}
```

## Testing Strategy

### Test Categories
- **Unit Tests**: Individual component testing (AddressSpace, StreamContext, etc.)
- **Integration Tests**: Cross-component interactions and full translation paths
- **Performance Tests**: Benchmarking and O(1)/O(log n) complexity validation
- **Compliance Tests**: ARM SMMU v3 specification conformance testing
- **Stress Tests**: Large-scale device/PASID scenarios

### Coverage Requirements
- Minimum 95% code coverage for new features
- All public APIs must have comprehensive unit tests
- Critical paths require additional integration testing
- Performance requirements must be validated with benchmarks

### Test-Driven Development (TDD)
**MANDATORY**: Always follow TDD approach:
1. Write failing tests first using `test-writer-fixer`
2. Implement minimal code to make tests pass using `cpp-pro`
3. Refactor and optimize while maintaining test coverage
4. Use `qa-engineer` to verify against specification requirements

## Performance Requirements

### Algorithmic Complexity
- **Translation Lookups**: Average O(1) or O(log n) performance required
- **Memory Usage**: Sparse representation to avoid waste in large address spaces
- **Scalability**: Handle 100s of PASIDs and large numbers of devices efficiently

### Performance Targets
- **Translation Time**: Sub-microsecond for cached translations
- **Memory Overhead**: Minimal per-stream/PASID overhead
- **Cache Efficiency**: High hit rates for typical access patterns

## Implementation Status

**Current Status**: PROJECT INITIALIZATION - Ready for core implementation

### Next Priority Tasks (from TASKS.md):
1. **Project Setup**: Create C++11 build system and directory structure
2. **Core Types**: Define fundamental types, enums, and structures  
3. **AddressSpace**: Implement sparse page table and translation logic
4. **StreamContext**: Build per-stream state and PASID management
5. **SMMU Controller**: Create main translation engine and API

## Key Files to Understand

- `ARM_SMMU_v3_PRD.md` - Complete product requirements document
- `IHI0070G_b-System_Memory_Management_Unit_Architecture_Specification.pdf` - ARM official specification
- `TASKS.md` - Detailed implementation task breakdown with time estimates
- `include/smmu/types.h` - Core protocol definitions (when created)
- `include/smmu/smmu.h` - Main SMMU controller interface (when created)

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.