# Test configuration with GoogleTest integration
# First try to find system-installed GoogleTest
find_package(GTest QUIET)

if(NOT GTest_FOUND)
    # Fallback to downloading GoogleTest via FetchContent (CMake 3.11+)
    if(CMAKE_VERSION VERSION_GREATER_EQUAL 3.11)
        include(FetchContent)
        
        message(STATUS "GoogleTest not found. Downloading GoogleTest via FetchContent...")
        
        FetchContent_Declare(
            googletest
            GIT_REPOSITORY https://github.com/google/googletest.git
            GIT_TAG        main  # Use latest main branch
        )
        
        # Configure GoogleTest for C++11 compatibility
        set(gtest_force_shared_crt ON CACHE BOOL "" FORCE)
        set(CMAKE_CXX_STANDARD 11)
        
        FetchContent_MakeAvailable(googletest)
        
        # Create aliases to match the expected interface
        add_library(GTest::GTest ALIAS gtest)
        add_library(GTest::Main ALIAS gtest_main)
        
        set(GTest_FOUND TRUE)
        message(STATUS "GoogleTest downloaded and configured successfully.")
    else()
        message(WARNING "CMake version < 3.11. Cannot download GoogleTest automatically.")
        message(WARNING "Please install GoogleTest manually or upgrade CMake to enable testing.")
        set(GTest_FOUND FALSE)
    endif()
endif()

if(GTest_FOUND)
    message(STATUS "GoogleTest found. Building test suite...")
    
    # Unit tests
    add_subdirectory(unit)
    
    # Integration tests
    add_subdirectory(integration)
    
    # Performance tests
    add_subdirectory(performance)
    
    # Custom test targets for easy execution
    add_custom_target(run_unit_tests
        COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure -L "unit"
        DEPENDS unit_tests
        WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
        COMMENT "Running unit tests..."
    )
    
    add_custom_target(run_integration_tests
        COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure -L "integration" 
        DEPENDS integration_tests
        WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
        COMMENT "Running integration tests..."
    )
    
    add_custom_target(run_performance_tests
        COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure -L "performance"
        DEPENDS performance_tests
        WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
        COMMENT "Running performance tests..."
    )
    
    add_custom_target(run_validation_tests
        COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure -L "validation"
        DEPENDS unit_tests integration_tests
        WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
        COMMENT "Running SMMU specification compliance tests..."
    )
    
    add_custom_target(run_thread_safety_tests
        COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure -L "thread_safety"
        DEPENDS unit_tests
        WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
        COMMENT "Running thread safety validation tests..."
    )
    
    add_custom_target(run_all_tests
        COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure
        DEPENDS unit_tests integration_tests performance_tests
        WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
        COMMENT "Running complete test suite..."
    )
    
    # Coverage target (if gcov is available)
    find_program(GCOV_PATH gcov)
    if(GCOV_PATH AND CMAKE_BUILD_TYPE STREQUAL "Debug")
        add_custom_target(coverage
            COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure
            COMMAND lcov --directory . --capture --output-file coverage.info
            COMMAND lcov --remove coverage.info '/usr/*' --output-file coverage_filtered.info
            COMMAND genhtml coverage_filtered.info --output-directory coverage_html
            DEPENDS run_all_tests
            WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
            COMMENT "Generating code coverage report..."
        )
    endif()
else()
    message(WARNING "GoogleTest not available. Tests will not be built.")
    message(WARNING "To enable testing, either:")
    message(WARNING "  1. Install GoogleTest system-wide")
    message(WARNING "  2. Upgrade CMake to 3.11+ for automatic GoogleTest download")
endif()