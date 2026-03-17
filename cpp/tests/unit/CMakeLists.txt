# Unit test executables
set(UNIT_TEST_SOURCES
    test_types.cpp
    test_address_space.cpp
    test_address_space_coverage.cpp
    test_address_space_edge_cases.cpp
    test_stream_context.cpp
    test_stream_context_coverage.cpp
    test_stream_context_extended.cpp
    test_stream_context_coverage_70pct.cpp
    test_stream_context_phase3_coverage.cpp
    test_stream_context_phase5_errors.cpp
    test_stream_context_two_stage_comprehensive.cpp
    test_stream_context_two_stage_advanced.cpp
    test_smmu.cpp
    test_smmu_coverage.cpp
    test_smmu_advanced_coverage.cpp
    test_smmu_priority2_coverage.cpp
    test_smmu_priority2_phase2.cpp
    test_smmu_phase4_coverage.cpp
    test_smmu_phase4b_coverage.cpp
    test_smmu_phase5_errors.cpp
    test_smmu_two_stage_comprehensive.cpp
    test_smmu_controller_advanced.cpp
    test_smmu_phase1_two_stage_errors.cpp
    test_smmu_phase1_address_size_validation.cpp
    test_fault_handler.cpp
    test_fault_handler_coverage.cpp
    test_tlb_cache.cpp
    test_tlb_cache_coverage.cpp
    test_task53_event_command_processing.cpp
    test_addr_size_fault_spec.cpp
    test_gerror_spec.cpp
    test_fault_syndrome_spec.cpp
    test_stream_reconfigure_spec.cpp
    test_c_bad_streamid_spec.cpp
    test_event_queue_stall_overflow_spec.cpp
    test_stall_resume_spec.cpp
    test_cfgi_cd_spec.cpp
    test_c_bad_substreamid_spec.cpp
    test_stall_event_type_spec.cpp
    test_smmuen_spec.cpp
    test_asid_vmid_tlb_spec.cpp
    test_s1dss_spec.cpp
    test_new21_22_23_spec.cpp
    test_new25_28_spec.cpp
    test_new31_33_spec.cpp
    test_ct30_ct33_spec.cpp
    test_ct04_09_13_14_19_20_23_spec.cpp
    test_new34_43_spec.cpp
    test_new47_52_spec.cpp
    test_configuration.cpp
    test_configuration_coverage.cpp
    test_configuration_validation.cpp
    test_edge_cases.cpp
    optimization_regression_test.cpp
    test_medium_bugs.cpp
    test_tenth_pass_bugs_spec.cpp
    test_new_bugs_spec.cpp
    test_output_attributes.cpp
    test_strw_privilege.cpp
    test_two_stage_access_type.cpp
    test_queue_modulus.cpp
    test_cr0_smmuen_sync.cpp
    test_bug_cpp1_event_codes.cpp
    test_bug_cpp2_fault_syndrome.cpp
    test_bug_cpp3_stall_lock_order.cpp
    test_bug_cpp4_config_fault_stall.cpp
    test_bug_cpp5_priv_level_syndrome.cpp
    test_bug_cpp6_strw_tlb_recheck.cpp
    test_bug_cpp_new1_atomic_cr0.cpp
    test_bug_cpp_new3_s1dss_tlb.cpp
    test_bug_cpp_new5_stage2_ns.cpp
    test_bug_cpp_new6_dirty_bit.cpp
    test_bug_cpp_dbgr7_stage2_sec_check.cpp
    test_bug_cpp_dbgr3_tlb_output_attrs.cpp
    test_bug_cpp_dbgr1_setCR0.cpp
    test_bug_cpp_dbgr2_gerror_toggle.cpp
    test_bug_cpp_dbgr4_raw_ptr.cpp
    test_bug_cpp_dbgr5_strw_twostage.cpp
    test_bug_cpp_dbgr6_fault_classify.cpp
    test_bug_cpp_dbgr8_bulk_map.cpp
    test_bug_cpp_dbgr9_as_exhausted.cpp
    test_bug_cpp_dbgr10_stage2_sec.cpp
    test_bug_cpp_dbgr11_constructor.cpp
    test_bug_cpp_dbgr12_bad_streamid_error.cpp
    test_bug1_strw_slowpath_stage2.cpp
    test_bug5_cache_key_collision.cpp
    test_bug_cpp_f1_f2.cpp
    test_new_gaps_cpp.cpp
    test_new_bugs_mar2026.cpp
    test_bug_r2_bugs_cpp.cpp
    test_new_cpp_bugs_mar2026b.cpp
    test_gerror_concurrency.cpp
    test_stall_queue.cpp
    test_bug2_bug4_classify_tlb.cpp
    test_bug_cpp_s1_double_translation.cpp
    test_bugs_11mar2026.cpp
    test_conf_gaps_11_fixes.cpp
    test_conf_gaps_moderate.cpp
    test_new_qa_findings.cpp
    test_conf_gaps_CD_fixes.cpp
    test_conf_gaps_3456789_fixes.cpp
    test_conf_gaps_EFH_fixes.cpp
    test_bug3_stall_pending_fields.cpp
    test_bug2_reset_pri_failures.cpp
    test_bug1_broadcast_tlbi_scope.cpp
    test_bug4_dead_code_lookup.cpp
    test_event_access_type_fix.cpp
    test_bug_fixes_mar2026.cpp
    test_new10_11_12_spec.cpp
    test_new13_15_19_spec.cpp
    test_conf_gaps_new_abdef.cpp
    test_gatos_faultcode_idr0_httu.cpp
    test_conf_gaps_17mar2026.cpp
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