# Example programs
set(EXAMPLE_SOURCES
    configuration_example.cpp         # Configuration System Demonstration
    # basic_translation.cpp           # TODO: Implement for Task demonstrations
    # stream_configuration.cpp        # TODO: Implement for Task demonstrations
    # fault_handling_demo.cpp         # TODO: Implement for Task demonstrations  
    # performance_demo.cpp            # TODO: Implement for Task demonstrations
)

foreach(example_source ${EXAMPLE_SOURCES})
    get_filename_component(example_name ${example_source} NAME_WE)
    
    add_executable(${example_name} ${example_source})
    target_link_libraries(${example_name} smmu_lib)
endforeach()

# Create a list of example target names
set(EXAMPLE_TARGETS "")
foreach(example_source ${EXAMPLE_SOURCES})
    get_filename_component(example_name ${example_source} NAME_WE)
    list(APPEND EXAMPLE_TARGETS ${example_name})
endforeach()

# Custom target for all examples
add_custom_target(examples)
add_dependencies(examples ${EXAMPLE_TARGETS})