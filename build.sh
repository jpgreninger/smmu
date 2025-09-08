#!/bin/bash
# ARM SMMU v3 C++11 Project Build Script
# Copyright (c) 2024 John Greninger
# 
# This script provides an easy way to build the ARM SMMU v3 project with proper
# out-of-source builds and various configuration options.

set -e  # Exit on any error

# Default configuration
BUILD_TYPE="Release"
CLEAN_BUILD=false
VERBOSE=false
JOBS=$(nproc)
BUILD_TESTING=ON
BUILD_EXAMPLES=ON
RUN_TESTS=false
COVERAGE=false
CLANG_FORMAT=false

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print usage information
show_usage() {
    cat << EOF
ARM SMMU v3 C++11 Project Build Script

Usage: $0 [options]

Options:
    -h, --help          Show this help message
    -c, --clean         Clean build (remove build directory first)
    -d, --debug         Debug build (default: Release)
    -r, --release       Release build (default)
    -v, --verbose       Verbose build output
    -j, --jobs N        Number of parallel jobs (default: $(nproc))
    --no-tests          Disable test building
    --no-examples       Disable example building
    --run-tests         Build and run tests after successful build
    --coverage          Enable coverage reporting (debug builds only)
    --format            Run clang-format on all source files before building

Examples:
    $0                  # Basic release build
    $0 --debug --verbose # Debug build with verbose output
    $0 --clean --jobs 8  # Clean build with 8 parallel jobs
    $0 --debug --run-tests --coverage # Debug build with tests and coverage

Requirements:
    - CMake 3.10 or higher
    - C++11 compatible compiler (GCC 4.8+, Clang 3.3+)
    - For tests: GoogleTest (will be downloaded if not found)
    - For coverage: gcov and lcov tools

EOF
}

# Print colored message
print_message() {
    local color=$1
    local message=$2
    echo -e "${color}[BUILD]${NC} $message"
}

# Check if required tools are available
check_requirements() {
    print_message $BLUE "Checking build requirements..."
    
    # Check CMake
    if ! command -v cmake &> /dev/null; then
        print_message $RED "ERROR: CMake is not installed or not in PATH"
        print_message $YELLOW "Please install CMake 3.10 or higher"
        exit 1
    fi
    
    local cmake_version=$(cmake --version | head -n1 | sed 's/.*version //' | sed 's/ .*//')
    print_message $GREEN "Found CMake version: $cmake_version"
    
    # Check compiler
    if command -v g++ &> /dev/null; then
        local gcc_version=$(g++ --version | head -n1)
        print_message $GREEN "Found compiler: $gcc_version"
    elif command -v clang++ &> /dev/null; then
        local clang_version=$(clang++ --version | head -n1)
        print_message $GREEN "Found compiler: $clang_version"
    else
        print_message $RED "ERROR: No C++ compiler found (g++ or clang++)"
        print_message $YELLOW "Please install a C++11 compatible compiler"
        exit 1
    fi
    
    # Check coverage tools if requested
    if [ "$COVERAGE" = true ]; then
        if [ "$BUILD_TYPE" != "Debug" ]; then
            print_message $YELLOW "WARNING: Coverage requires Debug build, switching to Debug"
            BUILD_TYPE="Debug"
        fi
        
        if ! command -v gcov &> /dev/null; then
            print_message $YELLOW "WARNING: gcov not found, coverage reports may not work"
        fi
        
        if ! command -v lcov &> /dev/null; then
            print_message $YELLOW "WARNING: lcov not found, HTML coverage reports will not be available"
        fi
    fi
}

# Run clang-format on all source files
run_clang_format() {
    if command -v clang-format &> /dev/null; then
        print_message $BLUE "Running clang-format on all source files..."
        
        find . -name "*.cpp" -o -name "*.h" | while read file; do
            if [[ $file != *"/build/"* ]] && [[ $file != *"/.serena/"* ]]; then
                echo "Formatting: $file"
                clang-format -i "$file"
            fi
        done
        
        print_message $GREEN "Code formatting complete"
    else
        print_message $YELLOW "WARNING: clang-format not found, skipping code formatting"
    fi
}

# Clean build directory
clean_build() {
    if [ -d "build" ]; then
        print_message $YELLOW "Removing existing build directory..."
        rm -rf build
    fi
}

# Configure build with CMake
configure_build() {
    print_message $BLUE "Configuring build (Type: $BUILD_TYPE, Jobs: $JOBS)..."
    
    mkdir -p build
    cd build
    
    local cmake_args=(
        "-DCMAKE_BUILD_TYPE=$BUILD_TYPE"
        "-DCMAKE_CXX_STANDARD=11"
        "-DCMAKE_CXX_STANDARD_REQUIRED=ON"
        "-DCMAKE_CXX_EXTENSIONS=OFF"
        "-DBUILD_TESTING=$BUILD_TESTING"
        "-DBUILD_EXAMPLES=$BUILD_EXAMPLES"
    )
    
    # Add coverage flags for debug builds
    if [ "$COVERAGE" = true ]; then
        cmake_args+=(
            "-DCMAKE_CXX_FLAGS_DEBUG=-g -O0 --coverage"
            "-DCMAKE_C_FLAGS_DEBUG=-g -O0 --coverage"
        )
    fi
    
    cmake .. "${cmake_args[@]}"
    cd ..
}

# Build the project
build_project() {
    print_message $BLUE "Building project..."
    
    cd build
    
    if [ "$VERBOSE" = true ]; then
        make -j$JOBS VERBOSE=1
    else
        make -j$JOBS
    fi
    
    cd ..
    
    print_message $GREEN "Build completed successfully!"
}

# Run tests if requested
run_tests() {
    if [ "$RUN_TESTS" = true ] && [ "$BUILD_TESTING" = "ON" ]; then
        print_message $BLUE "Running tests..."
        
        cd build
        
        if [ "$VERBOSE" = true ]; then
            ctest --output-on-failure --verbose
        else
            ctest --output-on-failure
        fi
        
        local test_result=$?
        cd ..
        
        if [ $test_result -eq 0 ]; then
            print_message $GREEN "All tests passed!"
        else
            print_message $RED "Some tests failed!"
            return $test_result
        fi
        
        # Generate coverage report if requested
        if [ "$COVERAGE" = true ]; then
            generate_coverage_report
        fi
    fi
}

# Generate coverage report
generate_coverage_report() {
    if [ "$COVERAGE" = true ] && command -v lcov &> /dev/null; then
        print_message $BLUE "Generating coverage report..."
        
        cd build
        
        # Capture coverage data
        lcov --directory . --capture --output-file coverage.info
        
        # Filter out system and test files
        lcov --remove coverage.info '/usr/*' '*/_deps/*' '*/tests/*' --output-file coverage_filtered.info
        
        # Generate HTML report if genhtml is available
        if command -v genhtml &> /dev/null; then
            genhtml coverage_filtered.info --output-directory coverage_html
            print_message $GREEN "Coverage report generated in build/coverage_html/"
        fi
        
        # Show coverage summary
        lcov --summary coverage_filtered.info
        
        cd ..
    fi
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_usage
            exit 0
            ;;
        -c|--clean)
            CLEAN_BUILD=true
            shift
            ;;
        -d|--debug)
            BUILD_TYPE="Debug"
            shift
            ;;
        -r|--release)
            BUILD_TYPE="Release"
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -j|--jobs)
            JOBS="$2"
            shift 2
            ;;
        --no-tests)
            BUILD_TESTING=OFF
            shift
            ;;
        --no-examples)
            BUILD_EXAMPLES=OFF
            shift
            ;;
        --run-tests)
            RUN_TESTS=true
            shift
            ;;
        --coverage)
            COVERAGE=true
            shift
            ;;
        --format)
            CLANG_FORMAT=true
            shift
            ;;
        *)
            print_message $RED "Unknown option: $1"
            print_message $YELLOW "Use --help to see available options"
            exit 1
            ;;
    esac
done

# Main build process
print_message $GREEN "ARM SMMU v3 C++11 Build Script"
print_message $BLUE "Build configuration:"
echo "  - Build Type: $BUILD_TYPE"
echo "  - Parallel Jobs: $JOBS"
echo "  - Build Tests: $BUILD_TESTING"
echo "  - Build Examples: $BUILD_EXAMPLES"
echo "  - Clean Build: $CLEAN_BUILD"
echo "  - Verbose: $VERBOSE"
echo "  - Run Tests: $RUN_TESTS"
echo "  - Coverage: $COVERAGE"
echo ""

# Execute build steps
check_requirements

if [ "$CLANG_FORMAT" = true ]; then
    run_clang_format
fi

if [ "$CLEAN_BUILD" = true ]; then
    clean_build
fi

configure_build
build_project
run_tests

print_message $GREEN "Build process completed successfully!"
print_message $BLUE "Build outputs are in the 'build/' directory"

if [ "$BUILD_TESTING" = "ON" ]; then
    print_message $BLUE "To run tests manually: cd build && ctest --output-on-failure"
fi

if [ "$BUILD_EXAMPLES" = "ON" ]; then
    print_message $BLUE "Example programs are in build/examples/"
fi