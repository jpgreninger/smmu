# Unit test executables
set(UNIT_TEST_SOURCES
    test_types.cpp
    test_address_space.cpp
    test_stream_context.cpp
    test_smmu.cpp
    test_fault_handler.cpp
    test_tlb_cache.cpp
    test_task53_event_command_processing.cpp
    test_configuration.cpp
    test_edge_cases.cpp
    optimization_regression_test.cpp
    ../test_thread_safety.cpp
)

foreach(test_source ${UNIT_TEST_SOURCES})
    get_filename_component(test_name ${test_source} NAME_WE)
    
    add_executable(${test_name} ${test_source})
    target_link_libraries(${test_name} 
        smmu_lib 
        GTest::GTest 
        GTest::Main
    )
    
    add_test(NAME ${test_name} COMMAND ${test_name})
    
    # Add threading support for thread safety tests
    if(${test_name} STREQUAL "test_thread_safety")
        find_package(Threads REQUIRED)
        target_link_libraries(${test_name} Threads::Threads)
        set_tests_properties(${test_name} PROPERTIES LABELS "unit;thread_safety")
    else()
        set_tests_properties(${test_name} PROPERTIES LABELS "unit")
    endif()
endforeach()

# Create a list of test target names
set(UNIT_TEST_TARGETS "")
foreach(test_source ${UNIT_TEST_SOURCES})
    get_filename_component(test_name ${test_source} NAME_WE)
    list(APPEND UNIT_TEST_TARGETS ${test_name})
endforeach()

# Custom target for all unit tests
add_custom_target(unit_tests)
add_dependencies(unit_tests ${UNIT_TEST_TARGETS})