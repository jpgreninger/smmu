# Integration test executables (currently empty for initial framework verification)
set(INTEGRATION_TEST_SOURCES
    # test_two_stage_translation.cpp      # TODO: Implement for Task 2.2
    # test_stream_isolation.cpp           # TODO: Implement for Task 2.3  
    # test_pasid_switching.cpp            # TODO: Implement for Task 2.3
    # test_fault_handling.cpp             # TODO: Implement for Task 2.4
    # test_cache_integration.cpp          # TODO: Implement for Task 2.5
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

# Custom target for all integration tests (currently empty)  
add_custom_target(integration_tests)
# Note: No dependencies added since no integration test sources are defined yet