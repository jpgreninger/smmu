# Performance test executables
set(PERFORMANCE_TEST_SOURCES
    address_space_performance_test.cpp  # AddressSpace O(1) performance validation
    # benchmark_cache.cpp               # TODO: Implement for Task 3.2
    # benchmark_scalability.cpp         # TODO: Implement for Task 3.3
    # benchmark_memory_usage.cpp        # TODO: Implement for Task 3.4
)

foreach(test_source ${PERFORMANCE_TEST_SOURCES})
    get_filename_component(test_name ${test_source} NAME_WE)
    
    add_executable(${test_name} ${test_source})
    target_link_libraries(${test_name}
        smmu_lib
        GTest::GTest
        GTest::Main
    )
    
    add_test(NAME ${test_name} COMMAND ${test_name})
    set_tests_properties(${test_name} PROPERTIES LABELS "performance")
endforeach()

# Custom target for all performance tests
add_custom_target(performance_tests
    DEPENDS address_space_performance_test
)