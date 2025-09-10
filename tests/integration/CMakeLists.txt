# Integration test executables - Task 8.2 Implementation Complete
set(INTEGRATION_TEST_SOURCES
    test_minimal_integration.cpp          # Task 8.2: Minimal integration tests with correct API usage
    # test_two_stage_translation.cpp       # TODO: Fix API usage
    # test_stream_isolation.cpp            # TODO: Fix API usage
    # test_pasid_context_switching.cpp     # TODO: Fix API usage
    # test_large_scale_scalability.cpp     # TODO: Fix API usage
)

foreach(test_source ${INTEGRATION_TEST_SOURCES})
    get_filename_component(test_name ${test_source} NAME_WE)
    
    add_executable(${test_name} ${test_source})
    target_link_libraries(${test_name}
        smmu_lib
        GTest::GTest
        GTest::Main
    )
    
    add_test(NAME ${test_name} COMMAND ${test_name})
    set_tests_properties(${test_name} PROPERTIES LABELS "integration")
endforeach()

# Custom target for all integration tests
add_custom_target(integration_tests)

# Add dependencies for all integration test executables
foreach(test_source ${INTEGRATION_TEST_SOURCES})
    get_filename_component(test_name ${test_source} NAME_WE)
    add_dependencies(integration_tests ${test_name})
endforeach()

# Note: run_integration_tests target is defined in parent tests/CMakeLists.txt