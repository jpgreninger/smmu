# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a comprehensive ARM SMMU (System Memory Management Unit) v3 implementation following the ARM SMMU v3 specification, providing a C++11-compliant software model for development, simulation, and testing environments.

This is also a Rust SMMU project. Language is Rust with Cargo. Always run `cargo test` and `cargo clippy` before committing. Documentation is in Markdown.

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

## Rust Development
Always run `cargo clippy -- -D warnings` with `--all-targets` flag to catch ALL warnings in a single pass, not just a subset.

## Git Workflow
After completing any implementation or fix, always commit changes immediately before starting the next task. Use descriptive commit messages with section/task numbers.

## Task Implementation
When the user says 'implement X' where X is a section or task number, always check TASKS.md or TASKS-RUST.md first to find the matching task specification before starting work.

### ⚠️ CRITICAL DEVELOPMENT REQUIREMENTS ⚠️

## Before spawning any subagent
Only spawn subagents for complex, multi-step tasks.
For simple tasks, handle directly without calling route_task.
When you do need a subagent:
Call route_task(task, files, directory) first. Always.
- REUSE → call get_context(agent_id), check stale_files, re-read any that changed
- CREATE_NEW → check existing_agents in response before spawning

## For code search
Prefer cocoindex.search() over Grep for semantic/exploratory queries.
Use Grep only for exact string matches.

## Memory
claude-mem auto-captures observations. Use search() → get_observations()
for progressive retrieval (don't load everything)

### Required Subagent Usage
**CRITICAL**: Always use these specialized subagents for development tasks:

- **cpp-pro**: **ALWAYS** use for implementing new C++ code. Use PROACTIVELY for C++ refactoring, performance optimization, or complex template solutions.
- **rust-engineer**: **ALWAYS** use for implementing new Rust code. Use PROACTIVELY for Rust refactoring, performance optimization, or complex template solutions.
- **debugger**: **ALWAYS** use for debugging compile errors, runtime bugs, and build issues. Use proactively when encountering any compilation or runtime issues.
- **qa-engineer**: **ALWAYS** use after each development step to review code against ARM SMMU v3 specification and update TASKS.md with missing features. Use proactively to ensure compliance and code quality.
- **test-writer-fixer**: **ALWAYS** use to write comprehensive tests for each implementation step and integrate into overall regression test suite. Use proactively after code modifications to ensure comprehensive test coverage and suite health.

### ⚠️ MANDATORY: Test-Driven Workflow for ALL Fixes and Features ⚠️

**ABSOLUTE REQUIREMENT**: Test-Driven Development (TDD) or Test-Driven Debug (TDD) MUST ALWAYS be used when implementing any fix or feature. No code changes may be made without first writing a failing test that demonstrates the problem or missing behavior.

Always write a high-quality, general-purpose solution using the standard tools available. Do not create helper scripts or workarounds to accomplish the task more efficiently. Implement a solution that works correctly for all valid inputs, not just the test cases. Do not hard-code values or create solutions that only work for specific test inputs. Instead, implement the actual logic that solves the problem generally.

Focus on understanding the problem requirements and implementing the correct algorithm. Tests are there to verify correctness, not to define the solution. Provide a principled implementation that follows best practices and software design principles.

If the task is unreasonable or infeasible, or if any of the tests are incorrect, inform me rather than working around them. The solution should be robust, maintainable, and extendable.

#### Test-Driven Development (TDD) — for new features
1. Write a failing test that asserts the desired behavior (test MUST fail before any implementation)
2. Verify the test fails (build and run to confirm red state)
3. Implement the minimal code to make the test pass
4. Verify the test passes (green state)
5. Refactor as needed while keeping tests green

#### Test-Driven Debug (TDD) — for bug fixes
1. Write a failing test that reproduces the bug (test MUST fail before any fix)
2. Verify the test fails (confirms the bug is captured)
3. Implement the fix
4. Verify the test passes and no regression tests broke
5. Commit the test alongside the fix

**NO EXCEPTIONS:** A fix without a prior failing test is not acceptable.

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

**Current Status**: ✅ **PRODUCTION RELEASE v1.0.0** - 100% COMPLETE

### ✅ **PRODUCTION ACHIEVEMENTS**:
1. **Complete ARM SMMU v3 Implementation**: All core functionality implemented and tested
2. **100% Test Success Rate**: 200+ tests across 14 test suites with perfect results
3. **Production Quality**: 5/5 star rating with zero build warnings
4. **Performance Excellence**: 135ns translation latency (500x better than target)
5. **Full Specification Compliance**: Complete ARM SMMU v3 adherence including PASID 0 support

## Key Files to Understand

- `ARM_SMMU_v3_PRD.md` - Complete product requirements document
- `IHI0070G_b-System_Memory_Management_Unit_Architecture_Specification.md` - ARM official specification in markdown format
- `TASKS.md` - Detailed implementation task breakdown with time estimates
- `include/smmu/types.h` - Core protocol definitions (when created)
- `include/smmu/smmu.h` - Main SMMU controller interface (when created)

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
