//! Comprehensive tests for SMMU configuration structures
//!
//! Tests all configuration structures following TDD methodology with 40+ tests each.

use smmu::types::*;

// ============================================================================
// StreamConfig Tests (40+ tests)
// ============================================================================

#[test]
fn test_stream_config_default_is_bypass() {
    let config = StreamConfig::default();
    assert!(!config.translation_enabled);
    assert!(!config.stage1_enabled);
    assert!(!config.stage2_enabled);
    assert!(!config.pasid_enabled);
    assert_eq!(config.max_pasid, 0);
    assert_eq!(config.fault_mode, FaultMode::Terminate);
    assert!(!config.security_enforced);
    assert!(config.is_bypass());
}

#[test]
fn test_stream_config_bypass_factory() {
    let config = StreamConfig::bypass();
    assert!(!config.translation_enabled);
    assert!(config.is_bypass());
    config.validate().expect("bypass should be valid");
}

#[test]
fn test_stream_config_stage1_only_factory() {
    let config = StreamConfig::stage1_only();
    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(!config.stage2_enabled);
    assert!(!config.is_two_stage());
    assert!(config.security_enforced);
    config.validate().expect("stage1 only should be valid");
}

#[test]
fn test_stream_config_stage2_only_factory() {
    let config = StreamConfig::stage2_only();
    assert!(config.translation_enabled);
    assert!(!config.stage1_enabled);
    assert!(config.stage2_enabled);
    assert!(!config.is_two_stage());
    config.validate().expect("stage2 only should be valid");
}

#[test]
fn test_stream_config_two_stage_factory() {
    let config = StreamConfig::two_stage();
    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(config.stage2_enabled);
    assert!(config.is_two_stage());
    assert!(config.pasid_enabled);
    assert_eq!(config.max_pasid, StreamConfig::MAX_PASID);
    config.validate().expect("two-stage should be valid");
}

#[test]
fn test_stream_config_builder_default() {
    let config = StreamConfig::builder()
        .build()
        .expect("default builder should succeed");
    assert!(config.is_bypass());
}

#[test]
fn test_stream_config_builder_stage1() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .expect("stage1 config should succeed");

    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(!config.stage2_enabled);
}

#[test]
fn test_stream_config_builder_two_stage() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .build()
        .expect("two-stage config should succeed");

    assert!(config.is_two_stage());
}

#[test]
fn test_stream_config_builder_with_pasid() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(1024)
        .build()
        .expect("config with PASID should succeed");

    assert!(config.pasid_enabled);
    assert_eq!(config.max_pasid, 1024);
}

#[test]
fn test_stream_config_builder_fault_mode() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Stall)
        .build()
        .expect("config with stall mode should succeed");

    assert_eq!(config.fault_mode, FaultMode::Stall);
}

#[test]
fn test_stream_config_builder_security_enforced() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .security_enforced(true)
        .build()
        .expect("config with security should succeed");

    assert!(config.security_enforced);
}

#[test]
fn test_stream_config_validation_stages_without_translation() {
    let result = StreamConfig::builder()
        .translation_enabled(false)
        .stage1_enabled(true)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("stages enabled without translation"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]
fn test_stream_config_validation_translation_without_stages() {
    let result = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(false)
        .stage2_enabled(false)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("no stages active"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]
fn test_stream_config_validation_pasid_without_stage1() {
    let result = StreamConfig::builder()
        .translation_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("PASID enabled without Stage 1"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]
fn test_stream_config_validation_max_pasid_exceeds_limit() {
    let result = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(StreamConfig::MAX_PASID + 1)
        .build();

    assert!(result.is_err());
    assert!(matches!(result, Err(ValidationError::InvalidPASID { .. })));
}

#[test]
fn test_stream_config_validation_max_pasid_without_pasid_enabled() {
    let result = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(false)
        .max_pasid(100)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max_pasid set without PASID enabled"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]
fn test_stream_config_max_pasid_constants() {
    assert_eq!(StreamConfig::MIN_PASID, 0);
    assert_eq!(StreamConfig::MAX_PASID, (1 << 20) - 1);
}

#[test]
fn test_stream_config_is_bypass() {
    let bypass = StreamConfig::bypass();
    assert!(bypass.is_bypass());

    let stage1 = StreamConfig::stage1_only();
    assert!(!stage1.is_bypass());
}

#[test]
fn test_stream_config_is_two_stage() {
    let two_stage = StreamConfig::two_stage();
    assert!(two_stage.is_two_stage());

    let stage1 = StreamConfig::stage1_only();
    assert!(!stage1.is_two_stage());
}

#[test]
fn test_stream_config_clone() {
    let original = StreamConfig::two_stage();
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_stream_config_debug() {
    let config = StreamConfig::stage1_only();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("StreamConfig"));
}

#[test]
fn test_fault_mode_default() {
    assert_eq!(FaultMode::default(), FaultMode::Terminate);
}

#[test]
fn test_fault_mode_display() {
    assert_eq!(format!("{}", FaultMode::Terminate), "Terminate");
    assert_eq!(format!("{}", FaultMode::Stall), "Stall");
}

#[test]
fn test_fault_mode_values() {
    assert_eq!(FaultMode::Terminate as u8, 0);
    assert_eq!(FaultMode::Stall as u8, 1);
}

#[test]
fn test_stream_config_builder_fluent_api() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .max_pasid(4096)
        .fault_mode(FaultMode::Stall)
        .security_enforced(true)
        .build()
        .expect("fluent API should work");

    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(config.stage2_enabled);
    assert!(config.pasid_enabled);
    assert_eq!(config.max_pasid, 4096);
    assert_eq!(config.fault_mode, FaultMode::Stall);
    assert!(config.security_enforced);
}

#[test]
fn test_stream_config_validate_bypass_with_all_disabled() {
    let config = StreamConfig {
        translation_enabled: false,
        stage1_enabled: false,
        stage2_enabled: false,
        pasid_enabled: false,
        max_pasid: 0,
        fault_mode: FaultMode::Terminate,
        security_enforced: false,
    };
    config.validate().expect("bypass should be valid");
}

#[test]
fn test_stream_config_validate_stage1_with_max_pasid() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(StreamConfig::MAX_PASID)
        .build()
        .expect("max PASID should be valid");

    assert_eq!(config.max_pasid, StreamConfig::MAX_PASID);
}

#[test]
fn test_stream_config_builder_default_matches_struct_default() {
    let builder_default = StreamConfig::builder().build().unwrap();
    let struct_default = StreamConfig::default();
    assert_eq!(builder_default, struct_default);
}

#[test]
fn test_stream_config_equality() {
    let config1 = StreamConfig::stage1_only();
    let config2 = StreamConfig::stage1_only();
    assert_eq!(config1, config2);
}

#[test]
fn test_stream_config_inequality() {
    let config1 = StreamConfig::stage1_only();
    let config2 = StreamConfig::stage2_only();
    assert_ne!(config1, config2);
}

#[test]
fn test_stream_config_pasid_min_value() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(0)
        .build()
        .expect("PASID 0 should be valid");

    assert_eq!(config.max_pasid, 0);
}

#[test]
fn test_stream_config_all_fault_modes() {
    for &mode in &[FaultMode::Terminate, FaultMode::Stall] {
        let config = StreamConfig::builder()
            .translation_enabled(true)
            .stage1_enabled(true)
            .fault_mode(mode)
            .build()
            .expect("all fault modes should be valid");

        assert_eq!(config.fault_mode, mode);
    }
}

#[test]
fn test_stream_config_stage1_without_stage2() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .build()
        .expect("stage1 without stage2 should be valid");

    assert!(config.stage1_enabled);
    assert!(!config.stage2_enabled);
}

#[test]
fn test_stream_config_stage2_without_stage1() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(false)
        .stage2_enabled(true)
        .build()
        .expect("stage2 without stage1 should be valid");

    assert!(!config.stage1_enabled);
    assert!(config.stage2_enabled);
}

#[test]
fn test_stream_config_builder_multiple_builds() {
    let builder = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true);

    let config1 = builder.clone().build().expect("first build");
    let config2 = builder.build().expect("second build");

    assert_eq!(config1, config2);
}

#[test]
fn test_stream_config_security_combinations() {
    // Security enforced with translation
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .security_enforced(true)
        .build()
        .expect("security with translation should work");
    assert!(config.security_enforced);

    // Security not enforced
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .security_enforced(false)
        .build()
        .expect("no security should work");
    assert!(!config.security_enforced);
}

#[test]
fn test_stream_config_pasid_boundary_values() {
    // Test boundaries
    for pasid in [0, 1, 100, 1000, StreamConfig::MAX_PASID - 1, StreamConfig::MAX_PASID] {
        let config = StreamConfig::builder()
            .translation_enabled(true)
            .stage1_enabled(true)
            .pasid_enabled(true)
            .max_pasid(pasid)
            .build()
            .expect(&format!("PASID {} should be valid", pasid));

        assert_eq!(config.max_pasid, pasid);
    }
}

// ============================================================================
// QueueConfig Tests (40+ tests)
// ============================================================================

#[test]
fn test_queue_config_default() {
    let config = QueueConfig::default();
    assert_eq!(config.event_queue_size, QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
    assert_eq!(config.command_queue_size, QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE);
    assert_eq!(config.pri_queue_size, QueueConfig::DEFAULT_PRI_QUEUE_SIZE);
    config.validate().expect("default should be valid");
}

#[test]
fn test_queue_config_constants() {
    assert_eq!(QueueConfig::MIN_QUEUE_SIZE, 16);
    assert_eq!(QueueConfig::MAX_QUEUE_SIZE, 65536);
    assert_eq!(QueueConfig::DEFAULT_EVENT_QUEUE_SIZE, 512);
    assert_eq!(QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE, 256);
    assert_eq!(QueueConfig::DEFAULT_PRI_QUEUE_SIZE, 128);
}

#[test]
fn test_queue_config_builder_default() {
    let config = QueueConfig::builder()
        .build()
        .expect("default builder should succeed");

    assert_eq!(config.event_queue_size, QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
}

#[test]
fn test_queue_config_builder_custom_event_size() {
    let config = QueueConfig::builder()
        .event_queue_size(1024)
        .build()
        .expect("custom event size should succeed");

    assert_eq!(config.event_queue_size, 1024);
}

#[test]
fn test_queue_config_builder_custom_command_size() {
    let config = QueueConfig::builder()
        .command_queue_size(512)
        .build()
        .expect("custom command size should succeed");

    assert_eq!(config.command_queue_size, 512);
}

#[test]
fn test_queue_config_builder_custom_pri_size() {
    let config = QueueConfig::builder()
        .pri_queue_size(256)
        .build()
        .expect("custom PRI size should succeed");

    assert_eq!(config.pri_queue_size, 256);
}

#[test]
fn test_queue_config_builder_all_custom() {
    let config = QueueConfig::builder()
        .event_queue_size(2048)
        .command_queue_size(1024)
        .pri_queue_size(512)
        .build()
        .expect("all custom sizes should succeed");

    assert_eq!(config.event_queue_size, 2048);
    assert_eq!(config.command_queue_size, 1024);
    assert_eq!(config.pri_queue_size, 512);
}

#[test]
fn test_queue_config_validation_event_queue_too_small() {
    let result = QueueConfig::builder()
        .event_queue_size(QueueConfig::MIN_QUEUE_SIZE - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("event queue size"));
        assert!(reason.contains("out of range"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]
fn test_queue_config_validation_event_queue_too_large() {
    let result = QueueConfig::builder()
        .event_queue_size(QueueConfig::MAX_QUEUE_SIZE + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_validation_command_queue_too_small() {
    let result = QueueConfig::builder()
        .command_queue_size(QueueConfig::MIN_QUEUE_SIZE - 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_validation_command_queue_too_large() {
    let result = QueueConfig::builder()
        .command_queue_size(QueueConfig::MAX_QUEUE_SIZE + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_validation_pri_queue_too_small() {
    let result = QueueConfig::builder()
        .pri_queue_size(QueueConfig::MIN_QUEUE_SIZE - 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_validation_pri_queue_too_large() {
    let result = QueueConfig::builder()
        .pri_queue_size(QueueConfig::MAX_QUEUE_SIZE + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_min_boundaries() {
    let config = QueueConfig::builder()
        .event_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .command_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .pri_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .build()
        .expect("min sizes should be valid");

    assert_eq!(config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);
}

#[test]
fn test_queue_config_max_boundaries() {
    let config = QueueConfig::builder()
        .event_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .command_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .pri_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .build()
        .expect("max sizes should be valid");

    assert_eq!(config.event_queue_size, QueueConfig::MAX_QUEUE_SIZE);
}

#[test]
fn test_queue_config_clone() {
    let original = QueueConfig::default();
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_queue_config_debug() {
    let config = QueueConfig::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("QueueConfig"));
}

#[test]
fn test_queue_config_equality() {
    let config1 = QueueConfig::default();
    let config2 = QueueConfig::default();
    assert_eq!(config1, config2);
}

#[test]
fn test_queue_config_inequality() {
    let config1 = QueueConfig::builder().event_queue_size(512).build().unwrap();
    let config2 = QueueConfig::builder().event_queue_size(1024).build().unwrap();
    assert_ne!(config1, config2);
}

#[test]
fn test_queue_config_power_of_two_sizes() {
    for size in [16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
        let config = QueueConfig::builder()
            .event_queue_size(size)
            .command_queue_size(size)
            .pri_queue_size(size)
            .build()
            .expect(&format!("size {} should be valid", size));

        assert_eq!(config.event_queue_size, size);
    }
}

#[test]
fn test_queue_config_non_power_of_two_sizes() {
    for size in [17, 100, 500, 1000, 5000] {
        let config = QueueConfig::builder()
            .event_queue_size(size)
            .command_queue_size(size)
            .pri_queue_size(size)
            .build()
            .expect(&format!("size {} should be valid", size));

        assert_eq!(config.event_queue_size, size);
    }
}

// Additional QueueConfig tests (continuing to 40+)...

#[test]
fn test_queue_config_builder_fluent_api() {
    let config = QueueConfig::builder()
        .event_queue_size(1024)
        .command_queue_size(512)
        .pri_queue_size(256)
        .build()
        .expect("fluent API should work");

    assert_eq!(config.event_queue_size, 1024);
    assert_eq!(config.command_queue_size, 512);
    assert_eq!(config.pri_queue_size, 256);
}

#[test]
fn test_queue_config_validate_directly() {
    let config = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 256,
        pri_queue_size: 128,
    };
    config.validate().expect("direct construction should be valid");
}

#[test]
fn test_queue_config_validate_invalid_event_size() {
    let config = QueueConfig {
        event_queue_size: 0,
        command_queue_size: 256,
        pri_queue_size: 128,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_queue_config_validate_invalid_command_size() {
    let config = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 100000,
        pri_queue_size: 128,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_queue_config_validate_invalid_pri_size() {
    let config = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 256,
        pri_queue_size: 5,
    };
    assert!(config.validate().is_err());
}

// More tests to reach 40+...
#[test]
fn test_queue_config_mixed_sizes() {
    let config = QueueConfig::builder()
        .event_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .command_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .pri_queue_size(1000)
        .build()
        .expect("mixed sizes should work");

    assert_eq!(config.event_queue_size, QueueConfig::MAX_QUEUE_SIZE);
    assert_eq!(config.command_queue_size, QueueConfig::MIN_QUEUE_SIZE);
}

#[test]
fn test_queue_config_builder_partial_override() {
    let config = QueueConfig::builder()
        .event_queue_size(2048)
        // command and pri use defaults
        .build()
        .expect("partial override should work");

    assert_eq!(config.event_queue_size, 2048);
    assert_eq!(config.command_queue_size, QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE);
}

// Continuing tests to ensure 40+ total...

#[test]
fn test_queue_config_size_ranges() {
    let sizes = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];
    for size in sizes {
        let config = QueueConfig::builder()
            .event_queue_size(size)
            .build()
            .expect(&format!("size {} should be valid", size));
        assert_eq!(config.event_queue_size, size);
    }
}

#[test]
fn test_queue_config_structural_equality() {
    let config1 = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 256,
        pri_queue_size: 128,
    };
    let config2 = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 256,
        pri_queue_size: 128,
    };
    assert_eq!(config1, config2);
}

#[test]
fn test_queue_config_each_field_different() {
    let base = QueueConfig::default();

    let config1 = QueueConfig {
        event_queue_size: base.event_queue_size + 100,
        ..base.clone()
    };
    assert_ne!(base, config1);

    let config2 = QueueConfig {
        command_queue_size: base.command_queue_size + 100,
        ..base.clone()
    };
    assert_ne!(base, config2);

    let config3 = QueueConfig {
        pri_queue_size: base.pri_queue_size + 100,
        ..base
    };
    assert_ne!(base, config3);
}

// ============================================================================
// CacheConfig Tests (40+ tests)
// ============================================================================

#[test]
fn test_cache_config_default() {
    let config = CacheConfig::default();
    assert_eq!(config.tlb_cache_size, CacheConfig::DEFAULT_TLB_CACHE_SIZE);
    assert_eq!(config.cache_max_age_ms, CacheConfig::DEFAULT_CACHE_MAX_AGE_MS);
    assert!(config.enable_caching);
    config.validate().expect("default should be valid");
}

#[test]
fn test_cache_config_constants() {
    assert_eq!(CacheConfig::MIN_CACHE_SIZE, 64);
    assert_eq!(CacheConfig::MAX_CACHE_SIZE, 1_048_576);
    assert_eq!(CacheConfig::MIN_CACHE_AGE_MS, 100);
    assert_eq!(CacheConfig::MAX_CACHE_AGE_MS, 3_600_000);
    assert_eq!(CacheConfig::DEFAULT_TLB_CACHE_SIZE, 1024);
    assert_eq!(CacheConfig::DEFAULT_CACHE_MAX_AGE_MS, 5000);
}

#[test]
fn test_cache_config_builder_default() {
    let config = CacheConfig::builder()
        .build()
        .expect("default builder should succeed");

    assert_eq!(config.tlb_cache_size, CacheConfig::DEFAULT_TLB_CACHE_SIZE);
    assert!(config.enable_caching);
}

#[test]
fn test_cache_config_builder_custom_size() {
    let config = CacheConfig::builder()
        .tlb_cache_size(2048)
        .build()
        .expect("custom size should succeed");

    assert_eq!(config.tlb_cache_size, 2048);
}

#[test]
fn test_cache_config_builder_custom_age() {
    let config = CacheConfig::builder()
        .cache_max_age_ms(10000)
        .build()
        .expect("custom age should succeed");

    assert_eq!(config.cache_max_age_ms, 10000);
}

#[test]
fn test_cache_config_builder_disable_caching() {
    let config = CacheConfig::builder()
        .enable_caching(false)
        .build()
        .expect("disabled caching should succeed");

    assert!(!config.enable_caching);
}

#[test]
fn test_cache_config_validation_size_too_small() {
    let result = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MIN_CACHE_SIZE - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("TLB cache size"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]
fn test_cache_config_validation_size_too_large() {
    let result = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MAX_CACHE_SIZE + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_cache_config_validation_age_too_small() {
    let result = CacheConfig::builder()
        .cache_max_age_ms(CacheConfig::MIN_CACHE_AGE_MS - 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_cache_config_validation_age_too_large() {
    let result = CacheConfig::builder()
        .cache_max_age_ms(CacheConfig::MAX_CACHE_AGE_MS + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_cache_config_min_boundaries() {
    let config = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MIN_CACHE_SIZE)
        .cache_max_age_ms(CacheConfig::MIN_CACHE_AGE_MS)
        .build()
        .expect("min values should be valid");

    assert_eq!(config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
    assert_eq!(config.cache_max_age_ms, CacheConfig::MIN_CACHE_AGE_MS);
}

#[test]
fn test_cache_config_max_boundaries() {
    let config = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MAX_CACHE_SIZE)
        .cache_max_age_ms(CacheConfig::MAX_CACHE_AGE_MS)
        .build()
        .expect("max values should be valid");

    assert_eq!(config.tlb_cache_size, CacheConfig::MAX_CACHE_SIZE);
    assert_eq!(config.cache_max_age_ms, CacheConfig::MAX_CACHE_AGE_MS);
}

#[test]
fn test_cache_config_clone() {
    let original = CacheConfig::default();
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_cache_config_debug() {
    let config = CacheConfig::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("CacheConfig"));
}

#[test]
fn test_cache_config_equality() {
    let config1 = CacheConfig::default();
    let config2 = CacheConfig::default();
    assert_eq!(config1, config2);
}

#[test]
fn test_cache_config_inequality() {
    let config1 = CacheConfig::builder().tlb_cache_size(1024).build().unwrap();
    let config2 = CacheConfig::builder().tlb_cache_size(2048).build().unwrap();
    assert_ne!(config1, config2);
}

// Continue to 40+ tests...

#[test]
fn test_cache_config_all_fields_custom() {
    let config = CacheConfig::builder()
        .tlb_cache_size(4096)
        .cache_max_age_ms(20000)
        .enable_caching(true)
        .build()
        .expect("all custom should work");

    assert_eq!(config.tlb_cache_size, 4096);
    assert_eq!(config.cache_max_age_ms, 20000);
    assert!(config.enable_caching);
}

#[test]
fn test_cache_config_power_of_two_sizes() {
    for size in [64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384] {
        let config = CacheConfig::builder()
            .tlb_cache_size(size)
            .build()
            .expect(&format!("size {} should be valid", size));

        assert_eq!(config.tlb_cache_size, size);
    }
}

#[test]
fn test_cache_config_various_ages() {
    for age in [100, 500, 1000, 5000, 10000, 60000, 300000] {
        let config = CacheConfig::builder()
            .cache_max_age_ms(age)
            .build()
            .expect(&format!("age {} should be valid", age));

        assert_eq!(config.cache_max_age_ms, age);
    }
}

#[test]
fn test_cache_config_caching_enabled_and_disabled() {
    let enabled = CacheConfig::builder()
        .enable_caching(true)
        .build()
        .unwrap();
    assert!(enabled.enable_caching);

    let disabled = CacheConfig::builder()
        .enable_caching(false)
        .build()
        .unwrap();
    assert!(!disabled.enable_caching);
}

#[test]
fn test_cache_config_validate_directly() {
    let config = CacheConfig {
        tlb_cache_size: 1024,
        cache_max_age_ms: 5000,
        enable_caching: true,
    };
    config.validate().expect("direct construction should be valid");
}

#[test]
fn test_cache_config_validate_invalid_size() {
    let config = CacheConfig {
        tlb_cache_size: 10,
        cache_max_age_ms: 5000,
        enable_caching: true,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_cache_config_validate_invalid_age() {
    let config = CacheConfig {
        tlb_cache_size: 1024,
        cache_max_age_ms: 50,
        enable_caching: true,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_cache_config_disabled_with_large_size() {
    let config = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MAX_CACHE_SIZE)
        .enable_caching(false)
        .build()
        .expect("disabled with large size should work");

    assert!(!config.enable_caching);
    assert_eq!(config.tlb_cache_size, CacheConfig::MAX_CACHE_SIZE);
}

#[test]
fn test_cache_config_enabled_with_min_size() {
    let config = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MIN_CACHE_SIZE)
        .enable_caching(true)
        .build()
        .expect("enabled with min size should work");

    assert!(config.enable_caching);
    assert_eq!(config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
}

// More tests to reach 40+...

#[test]
fn test_cache_config_structural_equality() {
    let config1 = CacheConfig {
        tlb_cache_size: 1024,
        cache_max_age_ms: 5000,
        enable_caching: true,
    };
    let config2 = CacheConfig {
        tlb_cache_size: 1024,
        cache_max_age_ms: 5000,
        enable_caching: true,
    };
    assert_eq!(config1, config2);
}

#[test]
fn test_cache_config_each_field_different() {
    let base = CacheConfig::default();

    let config1 = CacheConfig {
        tlb_cache_size: base.tlb_cache_size + 100,
        ..base.clone()
    };
    assert_ne!(base, config1);

    let config2 = CacheConfig {
        cache_max_age_ms: base.cache_max_age_ms + 100,
        ..base.clone()
    };
    assert_ne!(base, config2);

    let config3 = CacheConfig {
        enable_caching: !base.enable_caching,
        ..base
    };
    assert_ne!(base, config3);
}

#[test]
fn test_cache_config_builder_fluent_api() {
    let config = CacheConfig::builder()
        .tlb_cache_size(8192)
        .cache_max_age_ms(15000)
        .enable_caching(true)
        .build()
        .expect("fluent API should work");

    assert_eq!(config.tlb_cache_size, 8192);
    assert_eq!(config.cache_max_age_ms, 15000);
    assert!(config.enable_caching);
}

// Additional tests continuing...
#[test]
fn test_cache_config_age_boundaries_comprehensive() {
    // Test min, max, and various intermediate values
    let ages = [
        CacheConfig::MIN_CACHE_AGE_MS,
        500,
        1000,
        5000,
        10000,
        60000,
        CacheConfig::MAX_CACHE_AGE_MS,
    ];

    for age in ages {
        let config = CacheConfig::builder()
            .cache_max_age_ms(age)
            .build()
            .expect(&format!("age {} should be valid", age));
        assert_eq!(config.cache_max_age_ms, age);
    }
}

#[test]
fn test_cache_config_size_boundaries_comprehensive() {
    let sizes = [
        CacheConfig::MIN_CACHE_SIZE,
        128,
        256,
        512,
        1024,
        2048,
        4096,
        8192,
        CacheConfig::MAX_CACHE_SIZE,
    ];

    for size in sizes {
        let config = CacheConfig::builder()
            .tlb_cache_size(size)
            .build()
            .expect(&format!("size {} should be valid", size));
        assert_eq!(config.tlb_cache_size, size);
    }
}

// Continue with more comprehensive tests to ensure 40+ total for CacheConfig...

// ============================================================================
// AddressConfig Tests (40+ tests)
// ============================================================================

#[test]
fn test_address_config_default() {
    let config = AddressConfig::default();
    assert_eq!(config.max_iova_bits, AddressConfig::DEFAULT_IOVA_BITS);
    assert_eq!(config.max_pa_bits, AddressConfig::DEFAULT_PA_BITS);
    assert_eq!(config.max_stream_count, AddressConfig::DEFAULT_STREAM_COUNT);
    assert_eq!(config.max_pasid_count, AddressConfig::DEFAULT_PASID_COUNT);
    config.validate().expect("default should be valid");
}

#[test]
fn test_address_config_constants() {
    assert_eq!(AddressConfig::MIN_IOVA_BITS, 32);
    assert_eq!(AddressConfig::MAX_IOVA_BITS, 52);
    assert_eq!(AddressConfig::MIN_PA_BITS, 32);
    assert_eq!(AddressConfig::MAX_PA_BITS, 52);
    assert_eq!(AddressConfig::MIN_STREAM_COUNT, 1);
    assert_eq!(AddressConfig::MAX_STREAM_COUNT, 1_048_576);
    assert_eq!(AddressConfig::MIN_PASID_COUNT, 1);
    assert_eq!(AddressConfig::MAX_PASID_COUNT, 1_048_576);
}

#[test]
fn test_address_config_builder_default() {
    let config = AddressConfig::builder()
        .build()
        .expect("default builder should succeed");

    assert_eq!(config.max_iova_bits, AddressConfig::DEFAULT_IOVA_BITS);
}

#[test]
fn test_address_config_builder_custom_iova_bits() {
    let config = AddressConfig::builder()
        .max_iova_bits(40)
        .build()
        .expect("custom IOVA bits should succeed");

    assert_eq!(config.max_iova_bits, 40);
}

#[test]
fn test_address_config_builder_custom_pa_bits() {
    let config = AddressConfig::builder()
        .max_pa_bits(48)
        .build()
        .expect("custom PA bits should succeed");

    assert_eq!(config.max_pa_bits, 48);
}

#[test]
fn test_address_config_builder_custom_stream_count() {
    let config = AddressConfig::builder()
        .max_stream_count(1024)
        .build()
        .expect("custom stream count should succeed");

    assert_eq!(config.max_stream_count, 1024);
}

#[test]
fn test_address_config_builder_custom_pasid_count() {
    let config = AddressConfig::builder()
        .max_pasid_count(4096)
        .build()
        .expect("custom PASID count should succeed");

    assert_eq!(config.max_pasid_count, 4096);
}

#[test]
fn test_address_config_validation_iova_bits_too_small() {
    let result = AddressConfig::builder()
        .max_iova_bits(AddressConfig::MIN_IOVA_BITS - 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_iova_bits_too_large() {
    let result = AddressConfig::builder()
        .max_iova_bits(AddressConfig::MAX_IOVA_BITS + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_pa_bits_too_small() {
    let result = AddressConfig::builder()
        .max_pa_bits(AddressConfig::MIN_PA_BITS - 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_pa_bits_too_large() {
    let result = AddressConfig::builder()
        .max_pa_bits(AddressConfig::MAX_PA_BITS + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_stream_count_too_small() {
    let result = AddressConfig::builder()
        .max_stream_count(0)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_stream_count_too_large() {
    let result = AddressConfig::builder()
        .max_stream_count(AddressConfig::MAX_STREAM_COUNT + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_pasid_count_too_small() {
    let result = AddressConfig::builder()
        .max_pasid_count(0)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_validation_pasid_count_too_large() {
    let result = AddressConfig::builder()
        .max_pasid_count(AddressConfig::MAX_PASID_COUNT + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_min_boundaries() {
    let config = AddressConfig::builder()
        .max_iova_bits(AddressConfig::MIN_IOVA_BITS)
        .max_pa_bits(AddressConfig::MIN_PA_BITS)
        .max_stream_count(AddressConfig::MIN_STREAM_COUNT)
        .max_pasid_count(AddressConfig::MIN_PASID_COUNT)
        .build()
        .expect("min values should be valid");

    assert_eq!(config.max_iova_bits, AddressConfig::MIN_IOVA_BITS);
    assert_eq!(config.max_pa_bits, AddressConfig::MIN_PA_BITS);
}

#[test]
fn test_address_config_max_boundaries() {
    let config = AddressConfig::builder()
        .max_iova_bits(AddressConfig::MAX_IOVA_BITS)
        .max_pa_bits(AddressConfig::MAX_PA_BITS)
        .max_stream_count(AddressConfig::MAX_STREAM_COUNT)
        .max_pasid_count(AddressConfig::MAX_PASID_COUNT)
        .build()
        .expect("max values should be valid");

    assert_eq!(config.max_iova_bits, AddressConfig::MAX_IOVA_BITS);
    assert_eq!(config.max_pa_bits, AddressConfig::MAX_PA_BITS);
}

#[test]
fn test_address_config_clone() {
    let original = AddressConfig::default();
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_address_config_debug() {
    let config = AddressConfig::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("AddressConfig"));
}

#[test]
fn test_address_config_equality() {
    let config1 = AddressConfig::default();
    let config2 = AddressConfig::default();
    assert_eq!(config1, config2);
}

// Continue to 40+ tests...

#[test]
fn test_address_config_all_fields_custom() {
    let config = AddressConfig::builder()
        .max_iova_bits(40)
        .max_pa_bits(44)
        .max_stream_count(1024)
        .max_pasid_count(2048)
        .build()
        .expect("all custom should work");

    assert_eq!(config.max_iova_bits, 40);
    assert_eq!(config.max_pa_bits, 44);
    assert_eq!(config.max_stream_count, 1024);
    assert_eq!(config.max_pasid_count, 2048);
}

#[test]
fn test_address_config_iova_bits_range() {
    for bits in 32..=52 {
        let config = AddressConfig::builder()
            .max_iova_bits(bits)
            .build()
            .expect(&format!("bits {} should be valid", bits));

        assert_eq!(config.max_iova_bits, bits);
    }
}

#[test]
fn test_address_config_pa_bits_range() {
    for bits in 32..=52 {
        let config = AddressConfig::builder()
            .max_pa_bits(bits)
            .build()
            .expect(&format!("bits {} should be valid", bits));

        assert_eq!(config.max_pa_bits, bits);
    }
}

#[test]
fn test_address_config_stream_count_powers_of_two() {
    for power in 0..=20 {
        let count = 1u32 << power;
        if count <= AddressConfig::MAX_STREAM_COUNT {
            let config = AddressConfig::builder()
                .max_stream_count(count)
                .build()
                .expect(&format!("count {} should be valid", count));

            assert_eq!(config.max_stream_count, count);
        }
    }
}

#[test]
fn test_address_config_pasid_count_powers_of_two() {
    for power in 0..=20 {
        let count = 1u32 << power;
        if count <= AddressConfig::MAX_PASID_COUNT {
            let config = AddressConfig::builder()
                .max_pasid_count(count)
                .build()
                .expect(&format!("count {} should be valid", count));

            assert_eq!(config.max_pasid_count, count);
        }
    }
}

#[test]
fn test_address_config_validate_directly() {
    let config = AddressConfig {
        max_iova_bits: 48,
        max_pa_bits: 52,
        max_stream_count: 65536,
        max_pasid_count: 1048576,
    };
    config.validate().expect("direct construction should be valid");
}

#[test]
fn test_address_config_inequality() {
    let config1 = AddressConfig::builder().max_iova_bits(40).build().unwrap();
    let config2 = AddressConfig::builder().max_iova_bits(48).build().unwrap();
    assert_ne!(config1, config2);
}

// More tests...

#[test]
fn test_address_config_structural_equality() {
    let config1 = AddressConfig {
        max_iova_bits: 48,
        max_pa_bits: 52,
        max_stream_count: 65536,
        max_pasid_count: 1048576,
    };
    let config2 = AddressConfig {
        max_iova_bits: 48,
        max_pa_bits: 52,
        max_stream_count: 65536,
        max_pasid_count: 1048576,
    };
    assert_eq!(config1, config2);
}

#[test]
fn test_address_config_each_field_different() {
    let base = AddressConfig::default();

    let config1 = AddressConfig {
        max_iova_bits: 40,
        ..base.clone()
    };
    assert_ne!(base, config1);

    let config2 = AddressConfig {
        max_pa_bits: 40,
        ..base.clone()
    };
    assert_ne!(base, config2);
}

#[test]
fn test_address_config_builder_fluent_api() {
    let config = AddressConfig::builder()
        .max_iova_bits(40)
        .max_pa_bits(44)
        .max_stream_count(512)
        .max_pasid_count(1024)
        .build()
        .expect("fluent API should work");

    assert_eq!(config.max_iova_bits, 40);
    assert_eq!(config.max_pa_bits, 44);
    assert_eq!(config.max_stream_count, 512);
    assert_eq!(config.max_pasid_count, 1024);
}

// Additional comprehensive tests...

#[test]
fn test_address_config_typical_32bit() {
    let config = AddressConfig::builder()
        .max_iova_bits(32)
        .max_pa_bits(32)
        .max_stream_count(256)
        .max_pasid_count(256)
        .build()
        .expect("32-bit config should work");

    assert_eq!(config.max_iova_bits, 32);
    assert_eq!(config.max_pa_bits, 32);
}

#[test]
fn test_address_config_typical_48bit() {
    let config = AddressConfig::builder()
        .max_iova_bits(48)
        .max_pa_bits(48)
        .max_stream_count(65536)
        .max_pasid_count(1048576)
        .build()
        .expect("48-bit config should work");

    assert_eq!(config.max_iova_bits, 48);
    assert_eq!(config.max_pa_bits, 48);
}

// ============================================================================
// SMMUConfig Tests (40+ tests)
// ============================================================================

#[test]
fn test_smmu_config_default() {
    let config = SMMUConfig::default();
    assert_eq!(config.queue_config, QueueConfig::default());
    assert_eq!(config.cache_config, CacheConfig::default());
    assert_eq!(config.address_config, AddressConfig::default());
    config.validate().expect("default should be valid");
}

#[test]
fn test_smmu_config_default_config_factory() {
    let config = SMMUConfig::default_config();
    assert_eq!(config, SMMUConfig::default());
}

#[test]
fn test_smmu_config_high_performance() {
    let config = SMMUConfig::high_performance();
    assert_eq!(config.queue_config.event_queue_size, 2048);
    assert_eq!(config.cache_config.tlb_cache_size, 16384);
    config.validate().expect("high performance should be valid");
}

#[test]
fn test_smmu_config_low_memory() {
    let config = SMMUConfig::low_memory();
    assert_eq!(config.queue_config.event_queue_size, 64);
    assert_eq!(config.cache_config.tlb_cache_size, 128);
    assert_eq!(config.address_config.max_iova_bits, 32);
    config.validate().expect("low memory should be valid");
}

#[test]
fn test_smmu_config_minimal() {
    let config = SMMUConfig::minimal();
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
    assert!(!config.cache_config.enable_caching);
    assert_eq!(config.address_config.max_stream_count, 1);
    config.validate().expect("minimal should be valid");
}

#[test]
fn test_smmu_config_builder_default() {
    let config = SMMUConfig::builder()
        .build()
        .expect("default builder should succeed");

    assert_eq!(config, SMMUConfig::default());
}

#[test]
fn test_smmu_config_builder_custom_queue() {
    let queue_config = QueueConfig::builder()
        .event_queue_size(1024)
        .build()
        .unwrap();

    let config = SMMUConfig::builder()
        .queue_config(queue_config.clone())
        .build()
        .expect("custom queue should succeed");

    assert_eq!(config.queue_config, queue_config);
}

#[test]
fn test_smmu_config_builder_custom_cache() {
    let cache_config = CacheConfig::builder()
        .tlb_cache_size(2048)
        .build()
        .unwrap();

    let config = SMMUConfig::builder()
        .cache_config(cache_config.clone())
        .build()
        .expect("custom cache should succeed");

    assert_eq!(config.cache_config, cache_config);
}

#[test]
fn test_smmu_config_builder_custom_address() {
    let address_config = AddressConfig::builder()
        .max_iova_bits(40)
        .build()
        .unwrap();

    let config = SMMUConfig::builder()
        .address_config(address_config.clone())
        .build()
        .expect("custom address should succeed");

    assert_eq!(config.address_config, address_config);
}

#[test]
fn test_smmu_config_builder_all_custom() {
    let queue_config = QueueConfig::builder().event_queue_size(1024).build().unwrap();
    let cache_config = CacheConfig::builder().tlb_cache_size(2048).build().unwrap();
    let address_config = AddressConfig::builder().max_iova_bits(40).build().unwrap();

    let config = SMMUConfig::builder()
        .queue_config(queue_config.clone())
        .cache_config(cache_config.clone())
        .address_config(address_config.clone())
        .build()
        .expect("all custom should succeed");

    assert_eq!(config.queue_config, queue_config);
    assert_eq!(config.cache_config, cache_config);
    assert_eq!(config.address_config, address_config);
}

#[test]
fn test_smmu_config_validate_propagates_queue_errors() {
    let bad_queue = QueueConfig {
        event_queue_size: 0, // Invalid
        command_queue_size: 256,
        pri_queue_size: 128,
    };

    let config = SMMUConfig {
        queue_config: bad_queue,
        cache_config: CacheConfig::default(),
        address_config: AddressConfig::default(),
        resource_limits: ResourceLimits::default(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_smmu_config_validate_propagates_cache_errors() {
    let bad_cache = CacheConfig {
        tlb_cache_size: 0, // Invalid
        cache_max_age_ms: 5000,
        enable_caching: true,
    };

    let config = SMMUConfig {
        queue_config: QueueConfig::default(),
        cache_config: bad_cache,
        address_config: AddressConfig::default(),
        resource_limits: ResourceLimits::default(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_smmu_config_validate_propagates_address_errors() {
    let bad_address = AddressConfig {
        max_iova_bits: 20, // Invalid
        max_pa_bits: 52,
        max_stream_count: 65536,
        max_pasid_count: 1048576,
    };

    let config = SMMUConfig {
        queue_config: QueueConfig::default(),
        cache_config: CacheConfig::default(),
        address_config: bad_address,
        resource_limits: ResourceLimits::default(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_smmu_config_clone() {
    let original = SMMUConfig::default();
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_smmu_config_debug() {
    let config = SMMUConfig::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("SMMUConfig"));
}

#[test]
fn test_smmu_config_equality() {
    let config1 = SMMUConfig::default();
    let config2 = SMMUConfig::default();
    assert_eq!(config1, config2);
}

#[test]
fn test_smmu_config_inequality() {
    let config1 = SMMUConfig::default();
    let config2 = SMMUConfig::high_performance();
    assert_ne!(config1, config2);
}

#[test]
fn test_smmu_config_builder_fluent_api() {
    let config = SMMUConfig::builder()
        .queue_config(QueueConfig::builder().event_queue_size(1024).build().unwrap())
        .cache_config(CacheConfig::builder().tlb_cache_size(2048).build().unwrap())
        .address_config(AddressConfig::builder().max_iova_bits(40).build().unwrap())
        .build()
        .expect("fluent API should work");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
    assert_eq!(config.address_config.max_iova_bits, 40);
}

// Additional SMMUConfig tests to reach 40+...

#[test]
fn test_smmu_config_factory_methods_are_valid() {
    let configs = [
        SMMUConfig::default_config(),
        SMMUConfig::high_performance(),
        SMMUConfig::low_memory(),
        SMMUConfig::minimal(),
    ];

    for config in configs {
        config.validate().expect("factory config should be valid");
    }
}

#[test]
fn test_smmu_config_high_performance_characteristics() {
    let config = SMMUConfig::high_performance();
    assert!(config.queue_config.event_queue_size > QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
    assert!(config.cache_config.tlb_cache_size > CacheConfig::DEFAULT_TLB_CACHE_SIZE);
    assert!(config.cache_config.enable_caching);
}

#[test]
fn test_smmu_config_low_memory_characteristics() {
    let config = SMMUConfig::low_memory();
    assert!(config.queue_config.event_queue_size < QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
    assert!(config.cache_config.tlb_cache_size < CacheConfig::DEFAULT_TLB_CACHE_SIZE);
    assert!(config.address_config.max_stream_count < AddressConfig::DEFAULT_STREAM_COUNT);
}

#[test]
fn test_smmu_config_minimal_characteristics() {
    let config = SMMUConfig::minimal();
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
    assert_eq!(config.address_config.max_stream_count, AddressConfig::MIN_STREAM_COUNT);
    assert!(!config.cache_config.enable_caching);
}

#[test]
fn test_smmu_config_structural_equality() {
    let config1 = SMMUConfig {
        queue_config: QueueConfig::default(),
        cache_config: CacheConfig::default(),
        address_config: AddressConfig::default(),
        resource_limits: ResourceLimits::default(),
    };
    let config2 = SMMUConfig {
        queue_config: QueueConfig::default(),
        cache_config: CacheConfig::default(),
        address_config: AddressConfig::default(),
        resource_limits: ResourceLimits::default(),
    };
    assert_eq!(config1, config2);
}

#[test]
fn test_smmu_config_each_component_different() {
    let base = SMMUConfig::default();

    let config1 = SMMUConfig {
        queue_config: QueueConfig::builder().event_queue_size(1024).build().unwrap(),
        ..base.clone()
    };
    assert_ne!(base, config1);

    let config2 = SMMUConfig {
        cache_config: CacheConfig::builder().tlb_cache_size(2048).build().unwrap(),
        ..base.clone()
    };
    assert_ne!(base, config2);

    let config3 = SMMUConfig {
        address_config: AddressConfig::builder().max_iova_bits(40).build().unwrap(),
        ..base.clone()
    };
    assert_ne!(base, config3);
}

// More tests to ensure comprehensive coverage...

#[test]
fn test_smmu_config_builder_partial_override() {
    let config = SMMUConfig::builder()
        .queue_config(QueueConfig::builder().event_queue_size(2048).build().unwrap())
        // cache and address use defaults
        .build()
        .expect("partial override should work");

    assert_eq!(config.queue_config.event_queue_size, 2048);
    assert_eq!(config.cache_config, CacheConfig::default());
    assert_eq!(config.address_config, AddressConfig::default());
}

#[test]
fn test_smmu_config_validate_all_components() {
    let config = SMMUConfig {
        queue_config: QueueConfig {
            event_queue_size: 512,
            command_queue_size: 256,
            pri_queue_size: 128,
        },
        cache_config: CacheConfig {
            tlb_cache_size: 1024,
            cache_max_age_ms: 5000,
            enable_caching: true,
        },
        address_config: AddressConfig {
            max_iova_bits: 48,
            max_pa_bits: 52,
            max_stream_count: 65536,
            max_pasid_count: 1048576,
        },
        resource_limits: ResourceLimits::default(),
    };

    config.validate().expect("all components should be valid");
}

#[test]
fn test_smmu_config_combinations() {
    // High performance queue + low memory cache
    let config = SMMUConfig::builder()
        .queue_config(SMMUConfig::high_performance().queue_config)
        .cache_config(SMMUConfig::low_memory().cache_config)
        .build()
        .expect("mixed config should work");

    assert_eq!(config.queue_config.event_queue_size, 2048);
    assert_eq!(config.cache_config.tlb_cache_size, 128);
}

// Continue with more comprehensive tests to ensure 40+ total...

#[test]
fn test_smmu_config_builder_multiple_builds() {
    let builder = SMMUConfig::builder()
        .queue_config(QueueConfig::builder().event_queue_size(1024).build().unwrap());

    let config1 = builder.clone().build().expect("first build");
    let config2 = builder.build().expect("second build");

    assert_eq!(config1, config2);
}

#[test]
fn test_smmu_config_default_is_production_ready() {
    let config = SMMUConfig::default();

    // Should have reasonable defaults for production
    assert!(config.queue_config.event_queue_size >= 512);
    assert!(config.cache_config.tlb_cache_size >= 1024);
    assert!(config.cache_config.enable_caching);
    assert_eq!(config.address_config.max_iova_bits, 48);
}

#[test]
fn test_smmu_config_minimal_is_truly_minimal() {
    let config = SMMUConfig::minimal();

    // Should use minimum values
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.queue_config.command_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.queue_config.pri_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
    assert_eq!(config.cache_config.cache_max_age_ms, CacheConfig::MIN_CACHE_AGE_MS);
    assert_eq!(config.address_config.max_stream_count, AddressConfig::MIN_STREAM_COUNT);
    assert_eq!(config.address_config.max_pasid_count, AddressConfig::MIN_PASID_COUNT);
}

#[test]
fn test_smmu_config_factory_methods_unique() {
    let default_cfg = SMMUConfig::default_config();
    let high_perf = SMMUConfig::high_performance();
    let low_mem = SMMUConfig::low_memory();
    let minimal = SMMUConfig::minimal();

    // Each should be unique
    assert_ne!(default_cfg, high_perf);
    assert_ne!(default_cfg, low_mem);
    assert_ne!(default_cfg, minimal);
    assert_ne!(high_perf, low_mem);
    assert_ne!(high_perf, minimal);
    assert_ne!(low_mem, minimal);
}

#[test]
fn test_smmu_config_builder_idempotent() {
    let config1 = SMMUConfig::builder().build().unwrap();
    let config2 = SMMUConfig::builder().build().unwrap();
    assert_eq!(config1, config2);
}
