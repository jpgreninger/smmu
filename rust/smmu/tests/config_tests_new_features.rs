//! Additional comprehensive tests for missing SMMU configuration structures
//!
//! Tests for ResourceLimits, advanced SMMUConfiguration methods, and configuration
//! profiles based on C++ implementation. All tests should FAIL initially.

use smmu::types::*;

// ============================================================================
// ResourceLimits Tests (40+ tests) - THESE WILL FAIL - NOT IMPLEMENTED YET
// ============================================================================

#[test]

fn test_resource_limits_default() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_memory_usage, ResourceLimits::DEFAULT_MAX_MEMORY_USAGE);
    assert_eq!(limits.max_thread_count, ResourceLimits::DEFAULT_MAX_THREAD_COUNT);
    assert_eq!(limits.timeout_ms, ResourceLimits::DEFAULT_TIMEOUT_MS);
    assert!(limits.enable_resource_tracking);
    limits.validate().expect("default should be valid");
}

#[test]

fn test_resource_limits_constants() {
    assert_eq!(ResourceLimits::MIN_MEMORY_USAGE, 1024 * 1024); // 1MB
    assert_eq!(ResourceLimits::MAX_MEMORY_USAGE, 64 * 1024 * 1024 * 1024); // 64GB
    assert_eq!(ResourceLimits::MIN_THREAD_COUNT, 1);
    assert_eq!(ResourceLimits::MAX_THREAD_COUNT, 256);
    assert_eq!(ResourceLimits::MIN_TIMEOUT_MS, 10);
    assert_eq!(ResourceLimits::MAX_TIMEOUT_MS, 300_000); // 5 minutes
    assert_eq!(ResourceLimits::DEFAULT_MAX_MEMORY_USAGE, 1024 * 1024 * 1024); // 1GB
    assert_eq!(ResourceLimits::DEFAULT_MAX_THREAD_COUNT, 8);
    assert_eq!(ResourceLimits::DEFAULT_TIMEOUT_MS, 1000); // 1 second
}

#[test]

fn test_resource_limits_builder_default() {
    let limits = ResourceLimits::builder()
        .build()
        .expect("default builder should succeed");

    assert_eq!(limits.max_memory_usage, ResourceLimits::DEFAULT_MAX_MEMORY_USAGE);
    assert_eq!(limits.max_thread_count, ResourceLimits::DEFAULT_MAX_THREAD_COUNT);
}

#[test]

fn test_resource_limits_builder_custom_memory() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(2 * 1024 * 1024 * 1024) // 2GB
        .build()
        .expect("custom memory should succeed");

    assert_eq!(limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
}

#[test]

fn test_resource_limits_builder_custom_threads() {
    let limits = ResourceLimits::builder()
        .max_thread_count(16)
        .build()
        .expect("custom threads should succeed");

    assert_eq!(limits.max_thread_count, 16);
}

#[test]

fn test_resource_limits_builder_custom_timeout() {
    let limits = ResourceLimits::builder()
        .timeout_ms(5000)
        .build()
        .expect("custom timeout should succeed");

    assert_eq!(limits.timeout_ms, 5000);
}

#[test]

fn test_resource_limits_builder_disable_tracking() {
    let limits = ResourceLimits::builder()
        .enable_resource_tracking(false)
        .build()
        .expect("disabled tracking should succeed");

    assert!(!limits.enable_resource_tracking);
}

#[test]

fn test_resource_limits_validation_memory_too_small() {
    let result = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MIN_MEMORY_USAGE - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("memory"));
        assert!(reason.contains("out of range"));
    } else {
        panic!("Expected InvalidConfiguration error");
    }
}

#[test]

fn test_resource_limits_validation_memory_too_large() {
    let result = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MAX_MEMORY_USAGE + 1)
        .build();

    assert!(result.is_err());
}

#[test]

fn test_resource_limits_validation_thread_count_too_small() {
    let result = ResourceLimits::builder()
        .max_thread_count(0)
        .build();

    assert!(result.is_err());
}

#[test]

fn test_resource_limits_validation_thread_count_too_large() {
    let result = ResourceLimits::builder()
        .max_thread_count(ResourceLimits::MAX_THREAD_COUNT + 1)
        .build();

    assert!(result.is_err());
}

#[test]

fn test_resource_limits_validation_timeout_too_small() {
    let result = ResourceLimits::builder()
        .timeout_ms(ResourceLimits::MIN_TIMEOUT_MS - 1)
        .build();

    assert!(result.is_err());
}

#[test]

fn test_resource_limits_validation_timeout_too_large() {
    let result = ResourceLimits::builder()
        .timeout_ms(ResourceLimits::MAX_TIMEOUT_MS + 1)
        .build();

    assert!(result.is_err());
}

#[test]

fn test_resource_limits_min_boundaries() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MIN_MEMORY_USAGE)
        .max_thread_count(ResourceLimits::MIN_THREAD_COUNT)
        .timeout_ms(ResourceLimits::MIN_TIMEOUT_MS)
        .build()
        .expect("min values should be valid");

    assert_eq!(limits.max_memory_usage, ResourceLimits::MIN_MEMORY_USAGE);
    assert_eq!(limits.max_thread_count, ResourceLimits::MIN_THREAD_COUNT);
    assert_eq!(limits.timeout_ms, ResourceLimits::MIN_TIMEOUT_MS);
}

#[test]

fn test_resource_limits_max_boundaries() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MAX_MEMORY_USAGE)
        .max_thread_count(ResourceLimits::MAX_THREAD_COUNT)
        .timeout_ms(ResourceLimits::MAX_TIMEOUT_MS)
        .build()
        .expect("max values should be valid");

    assert_eq!(limits.max_memory_usage, ResourceLimits::MAX_MEMORY_USAGE);
    assert_eq!(limits.max_thread_count, ResourceLimits::MAX_THREAD_COUNT);
    assert_eq!(limits.timeout_ms, ResourceLimits::MAX_TIMEOUT_MS);
}

#[test]

fn test_resource_limits_clone() {
    let original = ResourceLimits::default();
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]

fn test_resource_limits_debug() {
    let limits = ResourceLimits::default();
    let debug_str = format!("{:?}", limits);
    assert!(debug_str.contains("ResourceLimits"));
}

#[test]

fn test_resource_limits_equality() {
    let limits1 = ResourceLimits::default();
    let limits2 = ResourceLimits::default();
    assert_eq!(limits1, limits2);
}

#[test]

fn test_resource_limits_inequality() {
    let limits1 = ResourceLimits::builder().max_memory_usage(1_000_000_000).build().unwrap();
    let limits2 = ResourceLimits::builder().max_memory_usage(2_000_000_000).build().unwrap();
    assert_ne!(limits1, limits2);
}

#[test]

fn test_resource_limits_memory_sizes() {
    let memory_sizes = [
        1024 * 1024,           // 1MB
        10 * 1024 * 1024,      // 10MB
        100 * 1024 * 1024,     // 100MB
        1024 * 1024 * 1024,    // 1GB
        4 * 1024 * 1024 * 1024, // 4GB
    ];

    for size in memory_sizes {
        let limits = ResourceLimits::builder()
            .max_memory_usage(size)
            .build()
            .expect(&format!("memory size {} should be valid", size));

        assert_eq!(limits.max_memory_usage, size);
    }
}

#[test]

fn test_resource_limits_thread_counts() {
    for count in [1, 2, 4, 8, 16, 32, 64, 128, 256] {
        let limits = ResourceLimits::builder()
            .max_thread_count(count)
            .build()
            .expect(&format!("thread count {} should be valid", count));

        assert_eq!(limits.max_thread_count, count);
    }
}

#[test]

fn test_resource_limits_timeout_values() {
    for timeout in [10, 100, 500, 1000, 5000, 10000, 60000, 300000] {
        let limits = ResourceLimits::builder()
            .timeout_ms(timeout)
            .build()
            .expect(&format!("timeout {} should be valid", timeout));

        assert_eq!(limits.timeout_ms, timeout);
    }
}

#[test]

fn test_resource_limits_all_fields_custom() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(2 * 1024 * 1024 * 1024)
        .max_thread_count(32)
        .timeout_ms(5000)
        .enable_resource_tracking(true)
        .build()
        .expect("all custom should work");

    assert_eq!(limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(limits.max_thread_count, 32);
    assert_eq!(limits.timeout_ms, 5000);
    assert!(limits.enable_resource_tracking);
}

#[test]

fn test_resource_limits_tracking_enabled_disabled() {
    let enabled = ResourceLimits::builder()
        .enable_resource_tracking(true)
        .build()
        .unwrap();
    assert!(enabled.enable_resource_tracking);

    let disabled = ResourceLimits::builder()
        .enable_resource_tracking(false)
        .build()
        .unwrap();
    assert!(!disabled.enable_resource_tracking);
}

#[test]

fn test_resource_limits_validate_directly() {
    let limits = ResourceLimits {
        max_memory_usage: 1024 * 1024 * 1024,
        max_thread_count: 8,
        timeout_ms: 1000,
        enable_resource_tracking: true,
    };
    limits.validate().expect("direct construction should be valid");
}

#[test]

fn test_resource_limits_validate_invalid_memory() {
    let limits = ResourceLimits {
        max_memory_usage: 100,
        max_thread_count: 8,
        timeout_ms: 1000,
        enable_resource_tracking: true,
    };
    assert!(limits.validate().is_err());
}

#[test]

fn test_resource_limits_validate_invalid_threads() {
    let limits = ResourceLimits {
        max_memory_usage: 1024 * 1024 * 1024,
        max_thread_count: 1000,
        timeout_ms: 1000,
        enable_resource_tracking: true,
    };
    assert!(limits.validate().is_err());
}

#[test]

fn test_resource_limits_validate_invalid_timeout() {
    let limits = ResourceLimits {
        max_memory_usage: 1024 * 1024 * 1024,
        max_thread_count: 8,
        timeout_ms: 1,
        enable_resource_tracking: true,
    };
    assert!(limits.validate().is_err());
}

#[test]

fn test_resource_limits_builder_fluent_api() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(4 * 1024 * 1024 * 1024)
        .max_thread_count(16)
        .timeout_ms(2000)
        .enable_resource_tracking(true)
        .build()
        .expect("fluent API should work");

    assert_eq!(limits.max_memory_usage, 4 * 1024 * 1024 * 1024);
    assert_eq!(limits.max_thread_count, 16);
    assert_eq!(limits.timeout_ms, 2000);
    assert!(limits.enable_resource_tracking);
}

#[test]

fn test_resource_limits_structural_equality() {
    let limits1 = ResourceLimits {
        max_memory_usage: 1024 * 1024 * 1024,
        max_thread_count: 8,
        timeout_ms: 1000,
        enable_resource_tracking: true,
    };
    let limits2 = ResourceLimits {
        max_memory_usage: 1024 * 1024 * 1024,
        max_thread_count: 8,
        timeout_ms: 1000,
        enable_resource_tracking: true,
    };
    assert_eq!(limits1, limits2);
}

#[test]

fn test_resource_limits_each_field_different() {
    let base = ResourceLimits::default();

    let limits1 = ResourceLimits {
        max_memory_usage: base.max_memory_usage + 1000,
        ..base.clone()
    };
    assert_ne!(base, limits1);

    let limits2 = ResourceLimits {
        max_thread_count: base.max_thread_count + 1,
        ..base.clone()
    };
    assert_ne!(base, limits2);

    let limits3 = ResourceLimits {
        timeout_ms: base.timeout_ms + 100,
        ..base.clone()
    };
    assert_ne!(base, limits3);

    let limits4 = ResourceLimits {
        enable_resource_tracking: !base.enable_resource_tracking,
        ..base
    };
    assert_ne!(base, limits4);
}

#[test]

fn test_resource_limits_timeout_as_duration() {
    let limits = ResourceLimits::builder()
        .timeout_ms(5000)
        .build()
        .unwrap();

    #[cfg(feature = "std")]
    {
        use std::time::Duration;
        let duration = limits.timeout();
        assert_eq!(duration, Duration::from_millis(5000));
    }
}

#[test]

fn test_resource_limits_memory_in_bytes() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(1024 * 1024 * 1024)
        .build()
        .unwrap();

    assert_eq!(limits.max_memory_bytes(), 1024 * 1024 * 1024);
    assert_eq!(limits.max_memory_kb(), 1024 * 1024);
    assert_eq!(limits.max_memory_mb(), 1024);
    assert_eq!(limits.max_memory_gb(), 1);
}

#[test]

fn test_resource_limits_hardware_concurrency() {
    let limits = ResourceLimits::default();
    // Should default to reasonable thread count (8 or hardware concurrency)
    assert!(limits.max_thread_count >= 1);
    assert!(limits.max_thread_count <= 256);
}

#[test]

fn test_resource_limits_partial_override() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(2 * 1024 * 1024 * 1024)
        // Other fields use defaults
        .build()
        .expect("partial override should work");

    assert_eq!(limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(limits.max_thread_count, ResourceLimits::DEFAULT_MAX_THREAD_COUNT);
    assert_eq!(limits.timeout_ms, ResourceLimits::DEFAULT_TIMEOUT_MS);
}

#[test]

fn test_resource_limits_builder_multiple_builds() {
    let builder = ResourceLimits::builder()
        .max_memory_usage(2 * 1024 * 1024 * 1024);

    let limits1 = builder.clone().build().expect("first build");
    let limits2 = builder.build().expect("second build");

    assert_eq!(limits1, limits2);
}

// ============================================================================
// Extended SMMUConfig Tests - Additional Factory Methods (40+ tests)
// ============================================================================

#[test]

fn test_smmu_config_with_resource_limits() {
    let config = SMMUConfig::builder()
        .resource_limits(ResourceLimits::default())
        .build()
        .expect("config with resource limits should succeed");

    assert_eq!(config.resource_limits, ResourceLimits::default());
}

#[test]

fn test_smmu_config_server_profile() {
    let config = SMMUConfig::server_profile();

    // Server profile should have high performance characteristics
    assert!(config.queue_config.event_queue_size >= 1024);
    assert!(config.cache_config.tlb_cache_size >= 8192);
    assert!(config.resource_limits.max_memory_usage >= 2 * 1024 * 1024 * 1024);
    assert!(config.resource_limits.max_thread_count >= 16);
    config.validate().expect("server profile should be valid");
}

#[test]

fn test_smmu_config_embedded_profile() {
    let config = SMMUConfig::embedded_profile();

    // Embedded profile should have minimal resource usage
    assert!(config.queue_config.event_queue_size <= 128);
    assert!(config.cache_config.tlb_cache_size <= 512);
    assert!(config.resource_limits.max_memory_usage <= 256 * 1024 * 1024);
    assert_eq!(config.address_config.max_iova_bits, 32);
    config.validate().expect("embedded profile should be valid");
}

#[test]

fn test_smmu_config_development_profile() {
    let config = SMMUConfig::development_profile();

    // Development profile should have debug-friendly settings
    assert!(config.resource_limits.enable_resource_tracking);
    assert!(config.resource_limits.timeout_ms >= 5000); // Longer timeout for debugging
    config.validate().expect("development profile should be valid");
}

#[test]

fn test_smmu_config_update_queue_sizes() {
    let mut config = SMMUConfig::default();

    config.update_queue_sizes(1024, 512, 256)
        .expect("update should succeed");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
    assert_eq!(config.queue_config.pri_queue_size, 256);
}

#[test]

fn test_smmu_config_update_queue_sizes_invalid() {
    let mut config = SMMUConfig::default();

    let result = config.update_queue_sizes(0, 256, 128);
    assert!(result.is_err());
}

#[test]

fn test_smmu_config_update_cache_settings() {
    let mut config = SMMUConfig::default();

    config.update_cache_settings(2048, 10000, true)
        .expect("update should succeed");

    assert_eq!(config.cache_config.tlb_cache_size, 2048);
    assert_eq!(config.cache_config.cache_max_age_ms, 10000);
    assert!(config.cache_config.enable_caching);
}

#[test]

fn test_smmu_config_update_cache_settings_invalid() {
    let mut config = SMMUConfig::default();

    let result = config.update_cache_settings(10, 5000, true);
    assert!(result.is_err());
}

#[test]

fn test_smmu_config_update_address_limits() {
    let mut config = SMMUConfig::default();

    config.update_address_limits(40, 44, 1024, 2048)
        .expect("update should succeed");

    assert_eq!(config.address_config.max_iova_bits, 40);
    assert_eq!(config.address_config.max_pa_bits, 44);
    assert_eq!(config.address_config.max_stream_count, 1024);
    assert_eq!(config.address_config.max_pasid_count, 2048);
}

#[test]

fn test_smmu_config_update_address_limits_invalid() {
    let mut config = SMMUConfig::default();

    let result = config.update_address_limits(20, 52, 1024, 2048);
    assert!(result.is_err());
}

#[test]

fn test_smmu_config_update_resource_limits() {
    let mut config = SMMUConfig::default();

    config.update_resource_limits(2 * 1024 * 1024 * 1024, 16, 2000)
        .expect("update should succeed");

    assert_eq!(config.resource_limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(config.resource_limits.max_thread_count, 16);
    assert_eq!(config.resource_limits.timeout_ms, 2000);
}

#[test]

fn test_smmu_config_update_resource_limits_invalid() {
    let mut config = SMMUConfig::default();

    let result = config.update_resource_limits(100, 8, 1000);
    assert!(result.is_err());
}

#[test]

fn test_smmu_config_merge() {
    let mut base = SMMUConfig::default();
    let overlay = SMMUConfig::high_performance();

    base.merge(&overlay).expect("merge should succeed");

    assert_eq!(base.queue_config, overlay.queue_config);
    assert_eq!(base.cache_config, overlay.cache_config);
}

#[test]

fn test_smmu_config_merge_partial() {
    let mut base = SMMUConfig::default();
    let mut overlay = SMMUConfig::default();
    overlay.queue_config.event_queue_size = 2048;

    base.merge(&overlay).expect("merge should succeed");

    assert_eq!(base.queue_config.event_queue_size, 2048);
}

#[test]

fn test_smmu_config_merge_invalid() {
    let mut base = SMMUConfig::default();
    let mut overlay = SMMUConfig::default();
    overlay.queue_config.event_queue_size = 0; // Invalid

    let result = base.merge(&overlay);
    assert!(result.is_err());
}

#[test]

fn test_smmu_config_reset() {
    let mut config = SMMUConfig::high_performance();
    config.reset();

    assert_eq!(config, SMMUConfig::default());
}

#[test]

fn test_smmu_config_validate_detailed() {
    let config = SMMUConfig::default();
    let result = config.validate_detailed();

    assert!(result.is_valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]

fn test_smmu_config_validate_detailed_with_errors() {
    let mut config = SMMUConfig::default();
    config.queue_config.event_queue_size = 0; // Invalid

    let result = config.validate_detailed();

    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("event queue"));
}

#[test]

fn test_smmu_config_validate_detailed_with_warnings() {
    let config = SMMUConfig::minimal();
    let result = config.validate_detailed();

    assert!(result.is_valid);
    // Minimal config might have warnings about low resources
    // but should still be valid
}

#[test]

fn test_smmu_config_to_string() {
    let config = SMMUConfig::default();
    let config_str = config.to_string();

    assert!(!config_str.is_empty());
    assert!(config_str.contains("event_queue_size"));
    assert!(config_str.contains("512")); // Default event queue size
}

#[test]

fn test_smmu_config_from_string() {
    let config_str = r#"
        event_queue_size=1024
        command_queue_size=512
        pri_queue_size=256
        tlb_cache_size=2048
        cache_max_age_ms=10000
        enable_caching=true
        max_iova_bits=48
        max_pa_bits=52
    "#;

    let config = SMMUConfig::from_string(config_str)
        .expect("parsing should succeed");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
}

#[test]

fn test_smmu_config_from_string_invalid() {
    let config_str = "event_queue_size=invalid";

    let result = SMMUConfig::from_string(config_str);
    assert!(result.is_err());
}

#[test]

fn test_smmu_config_from_string_partial() {
    let config_str = "event_queue_size=1024";

    let config = SMMUConfig::from_string(config_str)
        .expect("partial parsing should succeed");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    // Other fields should use defaults
    assert_eq!(config.cache_config, CacheConfig::default());
}

#[test]

fn test_smmu_config_roundtrip_string() {
    let original = SMMUConfig::high_performance();
    let config_str = original.to_string();
    let parsed = SMMUConfig::from_string(&config_str)
        .expect("roundtrip should succeed");

    assert_eq!(original, parsed);
}

#[test]

fn test_smmu_config_from_string_with_comments() {
    let config_str = r#"
        # Queue configuration
        event_queue_size=1024
        # Cache configuration
        tlb_cache_size=2048
    "#;

    let config = SMMUConfig::from_string(config_str)
        .expect("parsing with comments should succeed");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
}

#[test]

fn test_smmu_config_all_profiles_valid() {
    let profiles = [
        SMMUConfig::default_config(),
        SMMUConfig::high_performance(),
        SMMUConfig::low_memory(),
        SMMUConfig::minimal(),
        SMMUConfig::server_profile(),
        SMMUConfig::embedded_profile(),
        SMMUConfig::development_profile(),
    ];

    for (i, config) in profiles.iter().enumerate() {
        config.validate()
            .expect(&format!("profile {} should be valid", i));
    }
}

#[test]

fn test_smmu_config_all_profiles_detailed_validation() {
    let profiles = [
        ("default", SMMUConfig::default_config()),
        ("high_performance", SMMUConfig::high_performance()),
        ("low_memory", SMMUConfig::low_memory()),
        ("minimal", SMMUConfig::minimal()),
        ("server", SMMUConfig::server_profile()),
        ("embedded", SMMUConfig::embedded_profile()),
        ("development", SMMUConfig::development_profile()),
    ];

    for (name, config) in profiles {
        let result = config.validate_detailed();
        assert!(result.is_valid, "Profile {} should be valid", name);
    }
}

#[test]

fn test_smmu_config_profile_characteristics() {
    let server = SMMUConfig::server_profile();
    let embedded = SMMUConfig::embedded_profile();

    // Server should have more resources than embedded
    assert!(server.queue_config.event_queue_size > embedded.queue_config.event_queue_size);
    assert!(server.cache_config.tlb_cache_size > embedded.cache_config.tlb_cache_size);
    assert!(server.resource_limits.max_memory_usage > embedded.resource_limits.max_memory_usage);
}

#[test]

fn test_smmu_config_update_methods_thread_safe() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let config = Arc::new(Mutex::new(SMMUConfig::default()));
    let mut handles = vec![];

    for i in 0..10 {
        let config_clone = Arc::clone(&config);
        let handle = thread::spawn(move || {
            let mut cfg = config_clone.lock().unwrap();
            cfg.update_queue_sizes(512 + i * 10, 256, 128).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let final_config = config.lock().unwrap();
    final_config.validate().expect("config should still be valid");
}

#[test]

fn test_smmu_config_builder_with_resource_limits() {
    let config = SMMUConfig::builder()
        .resource_limits(ResourceLimits::builder()
            .max_memory_usage(2 * 1024 * 1024 * 1024)
            .build()
            .unwrap())
        .build()
        .expect("builder with resource limits should succeed");

    assert_eq!(config.resource_limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
}

#[test]

fn test_smmu_config_partial_update_preserves_other_fields() {
    let mut config = SMMUConfig::default();
    let original_cache = config.cache_config.clone();

    config.update_queue_sizes(1024, 512, 256).unwrap();

    // Cache config should be unchanged
    assert_eq!(config.cache_config, original_cache);
}

#[test]

fn test_smmu_config_chained_updates() {
    let mut config = SMMUConfig::default();

    config.update_queue_sizes(1024, 512, 256).unwrap();
    config.update_cache_settings(2048, 10000, true).unwrap();
    config.update_address_limits(40, 44, 1024, 2048).unwrap();

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
    assert_eq!(config.address_config.max_iova_bits, 40);
}

#[test]

fn test_smmu_config_update_rollback_on_error() {
    let mut config = SMMUConfig::default();
    let original = config.clone();

    // Try invalid update
    let result = config.update_queue_sizes(0, 256, 128);
    assert!(result.is_err());

    // Config should be unchanged
    assert_eq!(config, original);
}

#[test]

fn test_smmu_config_merge_preserves_valid_fields() {
    let mut base = SMMUConfig::default();
    let mut overlay = SMMUConfig::default();

    // Only change one field
    overlay.queue_config.event_queue_size = 2048;

    base.merge(&overlay).unwrap();

    // Changed field should be updated
    assert_eq!(base.queue_config.event_queue_size, 2048);
    // Other fields should be unchanged
    assert_eq!(base.cache_config, CacheConfig::default());
}

// ============================================================================
// ConfigurationError Tests (20+ tests)
// ============================================================================

#[test]

fn test_configuration_error_types() {
    let error_types = [
        ConfigurationErrorType::InvalidQueueSize,
        ConfigurationErrorType::InvalidCacheSize,
        ConfigurationErrorType::InvalidAddressSize,
        ConfigurationErrorType::InvalidResourceLimit,
        ConfigurationErrorType::InvalidFormat,
        ConfigurationErrorType::MissingRequired,
        ConfigurationErrorType::OutOfRange,
    ];

    for error_type in error_types {
        let error = ConfigurationError::new(
            error_type,
            "test_field".to_string(),
            "test message".to_string(),
        );
        assert_eq!(error.error_type, error_type);
    }
}

#[test]

fn test_configuration_error_display() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "event_queue_size".to_string(),
        "value out of range".to_string(),
    );

    let display = format!("{}", error);
    assert!(display.contains("event_queue_size"));
    assert!(display.contains("out of range"));
}

#[test]

fn test_configuration_error_debug() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "event_queue_size".to_string(),
        "value out of range".to_string(),
    );

    let debug = format!("{:?}", error);
    assert!(debug.contains("ConfigurationError"));
}

#[test]

fn test_configuration_error_from_validation_error() {
    let validation_error = ValidationError::InvalidConfiguration {
        reason: "test error".to_string(),
    };

    let config_error = ConfigurationError::from(validation_error);
    assert!(matches!(config_error.error_type, ConfigurationErrorType::InvalidFormat));
}

// ============================================================================
// ValidationResult Tests (20+ tests)
// ============================================================================

#[test]

fn test_validation_result_default() {
    let result = ValidationResult::default();
    assert!(!result.is_valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]

fn test_validation_result_success() {
    let result = ValidationResult::success();
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]

fn test_validation_result_with_error() {
    let result = ValidationResult::with_error("test error".to_string());
    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0], "test error");
}

#[test]

fn test_validation_result_with_warning() {
    let mut result = ValidationResult::success();
    result.add_warning("test warning".to_string());

    assert!(result.is_valid);
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0], "test warning");
}

#[test]

fn test_validation_result_multiple_errors() {
    let mut result = ValidationResult::default();
    result.add_error("error 1".to_string());
    result.add_error("error 2".to_string());

    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 2);
}

#[test]

fn test_validation_result_multiple_warnings() {
    let mut result = ValidationResult::success();
    result.add_warning("warning 1".to_string());
    result.add_warning("warning 2".to_string());

    assert!(result.is_valid);
    assert_eq!(result.warnings.len(), 2);
}

#[test]

fn test_validation_result_merge() {
    let mut result1 = ValidationResult::with_error("error 1".to_string());
    let mut result2 = ValidationResult::success();
    result2.add_warning("warning 1".to_string());

    result1.merge(result2);

    assert!(!result1.is_valid);
    assert_eq!(result1.errors.len(), 1);
    assert_eq!(result1.warnings.len(), 1);
}

// ============================================================================
// Configuration Constants Tests (10+ tests)
// ============================================================================

#[test]

fn test_config_constants_default_file() {
    assert_eq!(ConfigConstants::DEFAULT_CONFIG_FILE, "smmu_config.conf");
}

#[test]

fn test_config_constants_backup_file() {
    assert_eq!(ConfigConstants::BACKUP_CONFIG_FILE, "smmu_config.conf.bak");
}

#[test]

fn test_config_constants_version() {
    assert!(!ConfigConstants::CONFIG_VERSION.is_empty());
    assert!(ConfigConstants::CONFIG_VERSION.starts_with("v"));
}

#[test]

fn test_config_constants_env_vars() {
    assert_eq!(ConfigConstants::ENV_CONFIG_FILE, "SMMU_CONFIG_FILE");
    assert_eq!(ConfigConstants::ENV_QUEUE_SIZE, "SMMU_QUEUE_SIZE");
    assert_eq!(ConfigConstants::ENV_CACHE_SIZE, "SMMU_CACHE_SIZE");
    assert_eq!(ConfigConstants::ENV_MEMORY_LIMIT, "SMMU_MEMORY_LIMIT");
}
