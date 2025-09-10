cmake_minimum_required(VERSION 3.10)

# Project configuration
project(ARM_SMMU_v3
    VERSION 1.0.0
    DESCRIPTION "ARM SMMU v3 C++11 Implementation"
    LANGUAGES CXX
)

# C++11 requirement (strict compliance)
set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Ensure out-of-source builds
if(CMAKE_SOURCE_DIR STREQUAL CMAKE_BINARY_DIR)
    message(FATAL_ERROR "In-source builds are not allowed. Please create a 'build' directory and run cmake from there.")
endif()

# Build type default
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

# Compiler-specific flags for strict C++11 compliance
if(CMAKE_CXX_COMPILER_ID STREQUAL "GNU")
    set(CMAKE_CXX_FLAGS "-Wall -Wextra -Wpedantic -std=c++11 -pedantic-errors")
    set(CMAKE_CXX_FLAGS_DEBUG "-g -O0 -DDEBUG")
    set(CMAKE_CXX_FLAGS_RELEASE "-O3 -DNDEBUG")
elseif(CMAKE_CXX_COMPILER_ID STREQUAL "Clang")
    set(CMAKE_CXX_FLAGS "-Wall -Wextra -Wpedantic -std=c++11 -pedantic-errors")
    set(CMAKE_CXX_FLAGS_DEBUG "-g -O0 -DDEBUG")
    set(CMAKE_CXX_FLAGS_RELEASE "-O3 -DNDEBUG")
elseif(CMAKE_CXX_COMPILER_ID STREQUAL "MSVC")
    set(CMAKE_CXX_FLAGS "/W4 /std:c++11")
    set(CMAKE_CXX_FLAGS_DEBUG "/Od /Zi /DDEBUG")
    set(CMAKE_CXX_FLAGS_RELEASE "/O2 /DNDEBUG")
endif()

# Additional strict C++11 compile-time checks
add_compile_definitions(
    $<$<CONFIG:Debug>:SMMU_DEBUG>
    $<$<CONFIG:Debug>:SMMU_ENABLE_ASSERTIONS>
)

# Include directories
include_directories(include)

# Source files
set(SMMU_SOURCES
    src/types/types.cpp
    src/address_space/address_space.cpp
    src/stream_context/stream_context.cpp
    src/smmu/smmu.cpp
    src/fault/fault_handler.cpp
    src/cache/tlb_cache.cpp
    src/configuration/configuration.cpp
    src/memory/memory_pool.cpp
)

# Create static library
add_library(smmu_lib STATIC ${SMMU_SOURCES})

# Target properties
target_include_directories(smmu_lib PUBLIC include)
target_compile_features(smmu_lib PUBLIC cxx_std_11)

# Examples (optional)
option(BUILD_EXAMPLES "Build example programs" ON)
if(BUILD_EXAMPLES)
    add_subdirectory(examples)
endif()

# Testing (optional)
option(BUILD_TESTING "Build tests" ON)
if(BUILD_TESTING)
    enable_testing()
    add_subdirectory(tests)
endif()

# Installation rules
install(TARGETS smmu_lib
    ARCHIVE DESTINATION lib
    LIBRARY DESTINATION lib
)

install(DIRECTORY include/
    DESTINATION include
    FILES_MATCHING PATTERN "*.h"
)

# Package configuration
set(CPACK_PACKAGE_NAME "ARM_SMMU_v3")
set(CPACK_PACKAGE_VERSION ${PROJECT_VERSION})
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY ${PROJECT_DESCRIPTION})
include(CPack)