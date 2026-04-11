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

## Workflow
When implementing tasks from TASKS.md or similar task files, complete one task fully (including tests and commit) before starting the next. If rate limits are a concern, commit progress before moving on.

## Coding Standards

**CRITICAL**: All C++ source files must end in `.cpp` instead of `.cc`. Update any files with `.cc` suffix immediately.

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

