#![allow(missing_docs)]
#![allow(clippy::float_cmp)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_unwrap)]

//! Comprehensive test coverage for types/config.rs
//!
//! This test suite achieves 100% coverage for all configuration structures,
//! builder patterns, serialization, profiles, and validation logic.

use smmu::types::{
    AddressConfig, AddressConfigBuilder, CacheConfig, CacheConfigBuilder, ConfigConstants, ConfigurationError,
    ConfigurationErrorType, FaultMode, QueueConfig, QueueConfigBuilder, ResourceLimits, ResourceLimitsBuilder,
    SMMUConfig, SMMUConfigBuilder, StreamConfig, StreamConfigBuilder, ValidationError, ValidationResult,
};

// ============================================================================
// StreamConfig Tests
// ============================================================================

#[test]
fn test_stream_config_bypass() {
    let config = StreamConfig::bypass();
    assert!(!config.translation_enabled);
    assert!(!config.stage1_enabled);
    assert!(!config.stage2_enabled);
    assert!(!config.pasid_enabled);
    assert_eq!(config.max_pasid, 0);
    assert!(config.is_bypass());
    assert!(!config.is_two_stage());
    assert!(config.validate().is_ok());
}

#[test]
fn test_stream_config_stage1_only() {
    let config = StreamConfig::stage1_only();
    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(!config.stage2_enabled);
    assert!(!config.pasid_enabled);
    assert_eq!(config.max_pasid, 0);
    assert!(!config.is_bypass());
    assert!(!config.is_two_stage());
    assert!(config.validate().is_ok());
}

#[test]
fn test_stream_config_stage2_only() {
    let config = StreamConfig::stage2_only();
    assert!(config.translation_enabled);
    assert!(!config.stage1_enabled);
    assert!(config.stage2_enabled);
    assert!(!config.pasid_enabled);
    assert_eq!(config.max_pasid, 0);
    assert!(!config.is_bypass());
    assert!(!config.is_two_stage());
    assert!(config.validate().is_ok());
}

#[test]
fn test_stream_config_two_stage() {
    let config = StreamConfig::two_stage();
    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(config.stage2_enabled);
    assert!(config.pasid_enabled);
    assert_eq!(config.max_pasid, StreamConfig::MAX_PASID);
    assert!(!config.is_bypass());
    assert!(config.is_two_stage());
    assert!(config.validate().is_ok());
}

#[test]
fn test_stream_config_validation_stages_without_translation() {
    let result = StreamConfig::builder().translation_enabled(false).stage1_enabled(true).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("stages enabled without translation"));
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
    }
}

#[test]
fn test_stream_config_validation_pasid_without_stage1() {
    let result = StreamConfig::builder()
        .translation_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .max_pasid(100)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("PASID enabled without Stage 1"));
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
    assert!(matches!(result.unwrap_err(), ValidationError::InvalidPASID { .. }));
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
    }
}

#[test]
fn test_stream_config_builder_chaining() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .max_pasid(1000)
        .fault_mode(FaultMode::Stall)
        .security_enforced(true)
        .build()
        .expect("valid config");

    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(config.stage2_enabled);
    assert!(config.pasid_enabled);
    assert_eq!(config.max_pasid, 1000);
    assert_eq!(config.fault_mode, FaultMode::Stall);
    assert!(config.security_enforced);
}

#[test]
fn test_fault_mode_display() {
    assert_eq!(format!("{}", FaultMode::Terminate), "Terminate");
    assert_eq!(format!("{}", FaultMode::Stall), "Stall");
}

#[test]
fn test_fault_mode_default() {
    assert_eq!(FaultMode::default(), FaultMode::Terminate);
}

// ============================================================================
// QueueConfig Tests
// ============================================================================

#[test]
fn test_queue_config_default() {
    let config = QueueConfig::default();
    assert_eq!(config.event_queue_size, QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
    assert_eq!(config.command_queue_size, QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE);
    assert_eq!(config.pri_queue_size, QueueConfig::DEFAULT_PRI_QUEUE_SIZE);
    assert!(config.validate().is_ok());
}

#[test]
fn test_queue_config_builder_valid() {
    let config = QueueConfig::builder()
        .event_queue_size(1024)
        .command_queue_size(512)
        .pri_queue_size(256)
        .build()
        .expect("valid config");

    assert_eq!(config.event_queue_size, 1024);
    assert_eq!(config.command_queue_size, 512);
    assert_eq!(config.pri_queue_size, 256);
}

#[test]
fn test_queue_config_event_queue_too_small() {
    let result = QueueConfig::builder().event_queue_size(8).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("event queue size"));
        assert!(reason.contains("out of range"));
    }
}

#[test]
fn test_queue_config_event_queue_too_large() {
    let result = QueueConfig::builder().event_queue_size(QueueConfig::MAX_QUEUE_SIZE + 1).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("event queue size"));
    }
}

#[test]
fn test_queue_config_command_queue_too_small() {
    let result = QueueConfig::builder().command_queue_size(8).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("command queue size"));
    }
}

#[test]
fn test_queue_config_command_queue_too_large() {
    let result = QueueConfig::builder()
        .command_queue_size(QueueConfig::MAX_QUEUE_SIZE + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_pri_queue_too_small() {
    let result = QueueConfig::builder().pri_queue_size(8).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("PRI queue size"));
    }
}

#[test]
fn test_queue_config_pri_queue_too_large() {
    let result = QueueConfig::builder().pri_queue_size(QueueConfig::MAX_QUEUE_SIZE + 1).build();

    assert!(result.is_err());
}

#[test]
fn test_queue_config_min_boundary() {
    let config = QueueConfig::builder()
        .event_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .command_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .pri_queue_size(QueueConfig::MIN_QUEUE_SIZE)
        .build()
        .expect("valid config at MIN boundary");

    assert_eq!(config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);
}

#[test]
fn test_queue_config_max_boundary() {
    let config = QueueConfig::builder()
        .event_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .command_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .pri_queue_size(QueueConfig::MAX_QUEUE_SIZE)
        .build()
        .expect("valid config at MAX boundary");

    assert_eq!(config.event_queue_size, QueueConfig::MAX_QUEUE_SIZE);
}

#[test]
fn test_queue_config_accessors() {
    let config = QueueConfig::default();
    assert_eq!(config.event_queue_size(), QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
    assert_eq!(config.command_queue_size(), QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE);
    assert_eq!(config.pri_queue_size(), QueueConfig::DEFAULT_PRI_QUEUE_SIZE);
}

#[test]
fn test_queue_config_with_methods() {
    let config = QueueConfig::default()
        .with_event_queue_size(2048)
        .with_command_queue_size(1024)
        .with_pri_queue_size(512);

    assert_eq!(config.event_queue_size, 2048);
    assert_eq!(config.command_queue_size, 1024);
    assert_eq!(config.pri_queue_size, 512);
}

// ============================================================================
// CacheConfig Tests
// ============================================================================

#[test]
fn test_cache_config_default() {
    let config = CacheConfig::default();
    assert_eq!(config.tlb_cache_size, CacheConfig::DEFAULT_TLB_CACHE_SIZE);
    assert_eq!(config.cache_max_age_ms, CacheConfig::DEFAULT_CACHE_MAX_AGE_MS);
    assert!(config.enable_caching);
    assert!(config.validate().is_ok());
}

#[test]
fn test_cache_config_builder_valid() {
    let config = CacheConfig::builder()
        .tlb_cache_size(2048)
        .cache_max_age_ms(10_000)
        .enable_caching(true)
        .build()
        .expect("valid config");

    assert_eq!(config.tlb_cache_size, 2048);
    assert_eq!(config.cache_max_age_ms, 10_000);
    assert!(config.enable_caching);
}

#[test]
fn test_cache_config_tlb_cache_too_small() {
    let result = CacheConfig::builder().tlb_cache_size(CacheConfig::MIN_CACHE_SIZE - 1).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("TLB cache size"));
        assert!(reason.contains("out of range"));
    }
}

#[test]
fn test_cache_config_tlb_cache_too_large() {
    let result = CacheConfig::builder().tlb_cache_size(CacheConfig::MAX_CACHE_SIZE + 1).build();

    assert!(result.is_err());
}

#[test]
fn test_cache_config_max_age_too_small() {
    let result = CacheConfig::builder()
        .cache_max_age_ms(CacheConfig::MIN_CACHE_AGE_MS - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("cache max age"));
    }
}

#[test]
fn test_cache_config_max_age_too_large() {
    let result = CacheConfig::builder()
        .cache_max_age_ms(CacheConfig::MAX_CACHE_AGE_MS + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_cache_config_min_boundary() {
    let config = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MIN_CACHE_SIZE)
        .cache_max_age_ms(CacheConfig::MIN_CACHE_AGE_MS)
        .build()
        .expect("valid config at MIN boundary");

    assert_eq!(config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
    assert_eq!(config.cache_max_age_ms, CacheConfig::MIN_CACHE_AGE_MS);
}

#[test]
fn test_cache_config_max_boundary() {
    let config = CacheConfig::builder()
        .tlb_cache_size(CacheConfig::MAX_CACHE_SIZE)
        .cache_max_age_ms(CacheConfig::MAX_CACHE_AGE_MS)
        .build()
        .expect("valid config at MAX boundary");

    assert_eq!(config.tlb_cache_size, CacheConfig::MAX_CACHE_SIZE);
    assert_eq!(config.cache_max_age_ms, CacheConfig::MAX_CACHE_AGE_MS);
}

#[test]
fn test_cache_config_caching_disabled() {
    let config = CacheConfig::builder()
        .enable_caching(false)
        .build()
        .expect("valid config with caching disabled");

    assert!(!config.enable_caching);
}

#[cfg(feature = "std")]
#[test]
fn test_cache_config_max_age_duration() {
    let config = CacheConfig::default();
    let duration = config.cache_max_age();
    assert_eq!(duration.as_millis(), u128::from(CacheConfig::DEFAULT_CACHE_MAX_AGE_MS));
}

// ============================================================================
// AddressConfig Tests
// ============================================================================

#[test]
fn test_address_config_default() {
    let config = AddressConfig::default();
    assert_eq!(config.max_iova_bits, AddressConfig::DEFAULT_IOVA_BITS);
    assert_eq!(config.max_pa_bits, AddressConfig::DEFAULT_PA_BITS);
    assert_eq!(config.max_stream_count, AddressConfig::DEFAULT_STREAM_COUNT);
    assert_eq!(config.max_pasid_count, AddressConfig::DEFAULT_PASID_COUNT);
    assert!(config.validate().is_ok());
}

#[test]
fn test_address_config_builder_valid() {
    let config = AddressConfig::builder()
        .max_iova_bits(40)
        .max_pa_bits(44)
        .max_stream_count(1024)
        .max_pasid_count(2048)
        .build()
        .expect("valid config");

    assert_eq!(config.max_iova_bits, 40);
    assert_eq!(config.max_pa_bits, 44);
    assert_eq!(config.max_stream_count, 1024);
    assert_eq!(config.max_pasid_count, 2048);
}

#[test]
fn test_address_config_iova_bits_too_small() {
    let result = AddressConfig::builder().max_iova_bits(AddressConfig::MIN_IOVA_BITS - 1).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max IOVA bits"));
    }
}

#[test]
fn test_address_config_iova_bits_too_large() {
    let result = AddressConfig::builder().max_iova_bits(AddressConfig::MAX_IOVA_BITS + 1).build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_pa_bits_too_small() {
    let result = AddressConfig::builder().max_pa_bits(AddressConfig::MIN_PA_BITS - 1).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max PA bits"));
    }
}

#[test]
fn test_address_config_pa_bits_too_large() {
    let result = AddressConfig::builder().max_pa_bits(AddressConfig::MAX_PA_BITS + 1).build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_stream_count_too_small() {
    let result = AddressConfig::builder()
        .max_stream_count(AddressConfig::MIN_STREAM_COUNT - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max stream count"));
    }
}

#[test]
fn test_address_config_stream_count_too_large() {
    let result = AddressConfig::builder()
        .max_stream_count(AddressConfig::MAX_STREAM_COUNT + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_address_config_pasid_count_too_small() {
    let result = AddressConfig::builder()
        .max_pasid_count(AddressConfig::MIN_PASID_COUNT - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max PASID count"));
    }
}

#[test]
fn test_address_config_pasid_count_too_large() {
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
        .expect("valid config at MIN boundaries");

    assert_eq!(config.max_iova_bits, AddressConfig::MIN_IOVA_BITS);
}

#[test]
fn test_address_config_max_boundaries() {
    let config = AddressConfig::builder()
        .max_iova_bits(AddressConfig::MAX_IOVA_BITS)
        .max_pa_bits(AddressConfig::MAX_PA_BITS)
        .max_stream_count(AddressConfig::MAX_STREAM_COUNT)
        .max_pasid_count(AddressConfig::MAX_PASID_COUNT)
        .build()
        .expect("valid config at MAX boundaries");

    assert_eq!(config.max_iova_bits, AddressConfig::MAX_IOVA_BITS);
}

// ============================================================================
// ResourceLimits Tests
// ============================================================================

#[test]
fn test_resource_limits_default() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_memory_usage, ResourceLimits::DEFAULT_MAX_MEMORY_USAGE);
    assert_eq!(limits.max_thread_count, ResourceLimits::DEFAULT_MAX_THREAD_COUNT);
    assert_eq!(limits.timeout_ms, ResourceLimits::DEFAULT_TIMEOUT_MS);
    assert!(limits.enable_resource_tracking);
    assert!(limits.validate().is_ok());
}

#[test]
fn test_resource_limits_builder_valid() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(2 * 1024 * 1024 * 1024)
        .max_thread_count(16)
        .timeout_ms(5000)
        .enable_resource_tracking(true)
        .build()
        .expect("valid limits");

    assert_eq!(limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(limits.max_thread_count, 16);
    assert_eq!(limits.timeout_ms, 5000);
    assert!(limits.enable_resource_tracking);
}

#[test]
fn test_resource_limits_memory_too_small() {
    let result = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MIN_MEMORY_USAGE - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max memory usage"));
    }
}

#[test]
fn test_resource_limits_memory_too_large() {
    let result = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MAX_MEMORY_USAGE + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_resource_limits_thread_count_too_small() {
    let result = ResourceLimits::builder()
        .max_thread_count(ResourceLimits::MIN_THREAD_COUNT - 1)
        .build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("max thread count"));
    }
}

#[test]
fn test_resource_limits_thread_count_too_large() {
    let result = ResourceLimits::builder()
        .max_thread_count(ResourceLimits::MAX_THREAD_COUNT + 1)
        .build();

    assert!(result.is_err());
}

#[test]
fn test_resource_limits_timeout_too_small() {
    let result = ResourceLimits::builder().timeout_ms(ResourceLimits::MIN_TIMEOUT_MS - 1).build();

    assert!(result.is_err());
    if let Err(ValidationError::InvalidConfiguration { reason }) = result {
        assert!(reason.contains("timeout"));
    }
}

#[test]
fn test_resource_limits_timeout_too_large() {
    let result = ResourceLimits::builder().timeout_ms(ResourceLimits::MAX_TIMEOUT_MS + 1).build();

    assert!(result.is_err());
}

#[test]
fn test_resource_limits_min_boundaries() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MIN_MEMORY_USAGE)
        .max_thread_count(ResourceLimits::MIN_THREAD_COUNT)
        .timeout_ms(ResourceLimits::MIN_TIMEOUT_MS)
        .build()
        .expect("valid limits at MIN boundaries");

    assert_eq!(limits.max_memory_usage, ResourceLimits::MIN_MEMORY_USAGE);
}

#[test]
fn test_resource_limits_max_boundaries() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(ResourceLimits::MAX_MEMORY_USAGE)
        .max_thread_count(ResourceLimits::MAX_THREAD_COUNT)
        .timeout_ms(ResourceLimits::MAX_TIMEOUT_MS)
        .build()
        .expect("valid limits at MAX boundaries");

    assert_eq!(limits.max_memory_usage, ResourceLimits::MAX_MEMORY_USAGE);
}

#[cfg(feature = "std")]
#[test]
fn test_resource_limits_timeout_duration() {
    let limits = ResourceLimits::default();
    let duration = limits.timeout();
    assert_eq!(duration.as_millis(), u128::from(ResourceLimits::DEFAULT_TIMEOUT_MS));
}

#[test]
fn test_resource_limits_memory_accessors() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_memory_bytes(), ResourceLimits::DEFAULT_MAX_MEMORY_USAGE);
    assert_eq!(limits.max_memory_kb(), ResourceLimits::DEFAULT_MAX_MEMORY_USAGE / 1024);
    assert_eq!(limits.max_memory_mb(), ResourceLimits::DEFAULT_MAX_MEMORY_USAGE / (1024 * 1024));
    assert_eq!(
        limits.max_memory_gb(),
        ResourceLimits::DEFAULT_MAX_MEMORY_USAGE / (1024 * 1024 * 1024)
    );
}

#[test]
fn test_resource_limits_tracking_disabled() {
    let limits = ResourceLimits::builder()
        .enable_resource_tracking(false)
        .build()
        .expect("valid limits");

    assert!(!limits.enable_resource_tracking);
}

// ============================================================================
// SMMUConfig Profile Tests
// ============================================================================

#[test]
fn test_smmu_config_default() {
    let config = SMMUConfig::default();
    assert_eq!(config.queue_config, QueueConfig::default());
    assert_eq!(config.cache_config, CacheConfig::default());
    assert_eq!(config.address_config, AddressConfig::default());
    assert_eq!(config.resource_limits, ResourceLimits::default());
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_default_config() {
    let config = SMMUConfig::default_config();
    assert_eq!(config, SMMUConfig::default());
}

#[test]
fn test_smmu_config_high_performance() {
    let config = SMMUConfig::high_performance();
    assert_eq!(config.queue_config.event_queue_size, 2048);
    assert_eq!(config.queue_config.command_queue_size, 1024);
    assert_eq!(config.queue_config.pri_queue_size, 512);
    assert_eq!(config.cache_config.tlb_cache_size, 16_384);
    assert_eq!(config.cache_config.cache_max_age_ms, 10_000);
    assert!(config.cache_config.enable_caching);
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_low_memory() {
    let config = SMMUConfig::low_memory();
    assert_eq!(config.queue_config.event_queue_size, 64);
    assert_eq!(config.queue_config.command_queue_size, 32);
    assert_eq!(config.queue_config.pri_queue_size, 16);
    assert_eq!(config.cache_config.tlb_cache_size, 128);
    assert_eq!(config.cache_config.cache_max_age_ms, 2000);
    assert_eq!(config.address_config.max_iova_bits, 32);
    assert_eq!(config.address_config.max_pa_bits, 40);
    assert_eq!(config.address_config.max_stream_count, 256);
    assert_eq!(config.address_config.max_pasid_count, 1024);
    assert_eq!(config.resource_limits.max_memory_usage, 128 * 1024 * 1024);
    assert_eq!(config.resource_limits.max_thread_count, 2);
    assert_eq!(config.resource_limits.timeout_ms, 500);
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_minimal() {
    let config = SMMUConfig::minimal();
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.queue_config.command_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.queue_config.pri_queue_size, QueueConfig::MIN_QUEUE_SIZE);
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);
    assert_eq!(config.cache_config.cache_max_age_ms, CacheConfig::MIN_CACHE_AGE_MS);
    assert!(!config.cache_config.enable_caching);
    assert_eq!(config.address_config.max_iova_bits, 32);
    assert_eq!(config.address_config.max_pa_bits, 32);
    assert_eq!(config.address_config.max_stream_count, 1);
    assert_eq!(config.address_config.max_pasid_count, 1);
    assert_eq!(config.resource_limits.max_memory_usage, ResourceLimits::MIN_MEMORY_USAGE);
    assert_eq!(config.resource_limits.max_thread_count, ResourceLimits::MIN_THREAD_COUNT);
    assert_eq!(config.resource_limits.timeout_ms, ResourceLimits::MIN_TIMEOUT_MS);
    assert!(!config.resource_limits.enable_resource_tracking);
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_server_profile() {
    let config = SMMUConfig::server_profile();
    assert_eq!(config.queue_config.event_queue_size, 4096);
    assert_eq!(config.queue_config.command_queue_size, 2048);
    assert_eq!(config.queue_config.pri_queue_size, 1024);
    assert_eq!(config.cache_config.tlb_cache_size, 32_768);
    assert_eq!(config.cache_config.cache_max_age_ms, 15_000);
    assert_eq!(config.resource_limits.max_memory_usage, 8 * 1024 * 1024 * 1024);
    assert_eq!(config.resource_limits.max_thread_count, 32);
    assert_eq!(config.resource_limits.timeout_ms, 5000);
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_embedded_profile() {
    let config = SMMUConfig::embedded_profile();
    assert_eq!(config.queue_config.event_queue_size, 64);
    assert_eq!(config.queue_config.command_queue_size, 32);
    assert_eq!(config.queue_config.pri_queue_size, 16);
    assert_eq!(config.cache_config.tlb_cache_size, 256);
    assert_eq!(config.cache_config.cache_max_age_ms, 2000);
    assert_eq!(config.address_config.max_iova_bits, 32);
    assert_eq!(config.address_config.max_pa_bits, 40);
    assert_eq!(config.address_config.max_stream_count, 128);
    assert_eq!(config.address_config.max_pasid_count, 256);
    assert_eq!(config.resource_limits.max_memory_usage, 64 * 1024 * 1024);
    assert_eq!(config.resource_limits.max_thread_count, 2);
    assert_eq!(config.resource_limits.timeout_ms, 500);
    assert!(!config.resource_limits.enable_resource_tracking);
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_development_profile() {
    let config = SMMUConfig::development_profile();
    assert_eq!(config.queue_config, QueueConfig::default());
    assert_eq!(config.cache_config, CacheConfig::default());
    assert_eq!(config.address_config, AddressConfig::default());
    assert_eq!(config.resource_limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(config.resource_limits.max_thread_count, 8);
    assert_eq!(config.resource_limits.timeout_ms, 10_000);
    assert!(config.resource_limits.enable_resource_tracking);
    assert!(config.validate().is_ok());
}

// ============================================================================
// SMMUConfig Update Methods Tests
// ============================================================================

#[test]
fn test_smmu_config_update_queue_sizes_valid() {
    let mut config = SMMUConfig::default();
    let result = config.update_queue_sizes(1024, 512, 256);

    assert!(result.is_ok());
    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
    assert_eq!(config.queue_config.pri_queue_size, 256);
}

#[test]
fn test_smmu_config_update_queue_sizes_invalid() {
    let mut config = SMMUConfig::default();
    let result = config.update_queue_sizes(8, 512, 256);

    assert!(result.is_err());
    // Original config should be unchanged
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
}

#[test]
fn test_smmu_config_update_cache_settings_valid() {
    let mut config = SMMUConfig::default();
    let result = config.update_cache_settings(2048, 10_000, true);

    assert!(result.is_ok());
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
    assert_eq!(config.cache_config.cache_max_age_ms, 10_000);
    assert!(config.cache_config.enable_caching);
}

#[test]
fn test_smmu_config_update_cache_settings_invalid() {
    let mut config = SMMUConfig::default();
    let result = config.update_cache_settings(32, 10_000, true);

    assert!(result.is_err());
    // Original config should be unchanged
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::DEFAULT_TLB_CACHE_SIZE);
}

#[test]
fn test_smmu_config_update_cache_settings_disable_caching() {
    let mut config = SMMUConfig::default();
    let result = config.update_cache_settings(1024, 5000, false);

    assert!(result.is_ok());
    assert!(!config.cache_config.enable_caching);
}

#[test]
fn test_smmu_config_update_address_limits_valid() {
    let mut config = SMMUConfig::default();
    let result = config.update_address_limits(40, 44, 1024, 2048);

    assert!(result.is_ok());
    assert_eq!(config.address_config.max_iova_bits, 40);
    assert_eq!(config.address_config.max_pa_bits, 44);
    assert_eq!(config.address_config.max_stream_count, 1024);
    assert_eq!(config.address_config.max_pasid_count, 2048);
}

#[test]
fn test_smmu_config_update_address_limits_invalid() {
    let mut config = SMMUConfig::default();
    let result = config.update_address_limits(20, 44, 1024, 2048);

    assert!(result.is_err());
    // Original config should be unchanged
    assert_eq!(config.address_config.max_iova_bits, AddressConfig::DEFAULT_IOVA_BITS);
}

#[test]
fn test_smmu_config_update_resource_limits_valid() {
    let mut config = SMMUConfig::default();
    let result = config.update_resource_limits(2 * 1024 * 1024 * 1024, 16, 2000);

    assert!(result.is_ok());
    assert_eq!(config.resource_limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(config.resource_limits.max_thread_count, 16);
    assert_eq!(config.resource_limits.timeout_ms, 2000);
}

#[test]
fn test_smmu_config_update_resource_limits_invalid() {
    let mut config = SMMUConfig::default();
    let result = config.update_resource_limits(100, 16, 2000);

    assert!(result.is_err());
    // Original config should be unchanged
    assert_eq!(
        config.resource_limits.max_memory_usage,
        ResourceLimits::DEFAULT_MAX_MEMORY_USAGE
    );
}

// ============================================================================
// SMMUConfig Advanced Operations
// ============================================================================

#[test]
fn test_smmu_config_merge_valid() {
    let mut base = SMMUConfig::default();
    let overlay = SMMUConfig::high_performance();

    let result = base.merge(&overlay);
    assert!(result.is_ok());

    // After merge, base should equal overlay
    assert_eq!(base.queue_config, overlay.queue_config);
    assert_eq!(base.cache_config, overlay.cache_config);
    assert_eq!(base.address_config, overlay.address_config);
    assert_eq!(base.resource_limits, overlay.resource_limits);
}

#[test]
fn test_smmu_config_merge_invalid() {
    let mut base = SMMUConfig::default();

    // Create an invalid config manually (bypassing builder validation)
    let mut invalid = SMMUConfig::default();
    invalid.queue_config.event_queue_size = 1; // Invalid size

    let result = base.merge(&invalid);
    assert!(result.is_err());
}

#[test]
fn test_smmu_config_reset() {
    let mut config = SMMUConfig::high_performance();
    assert_ne!(config, SMMUConfig::default());

    config.reset();
    assert_eq!(config, SMMUConfig::default());
}

#[test]
fn test_smmu_config_with_max_streams() {
    let config = SMMUConfig::default().with_max_streams(2048);

    assert_eq!(config.max_streams(), 2048);
    assert_eq!(config.address_config.max_stream_count, 2048);
}

#[test]
fn test_smmu_config_queue_config_accessor() {
    let config = SMMUConfig::default();
    let queue_config = config.queue_config();
    assert_eq!(queue_config, &QueueConfig::default());
}

// ============================================================================
// SMMUConfig Validation Tests
// ============================================================================

#[test]
fn test_smmu_config_validate_detailed_success() {
    let config = SMMUConfig::default();
    let result = config.validate_detailed();

    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[test]
fn test_smmu_config_validate_detailed_with_warnings() {
    let config = SMMUConfig::minimal();
    let result = config.validate_detailed();

    assert!(result.is_valid);
    assert!(!result.warnings.is_empty());
    assert!(result.warnings.iter().any(|w| w.contains("Event queue size is very small")));
    assert!(result.warnings.iter().any(|w| w.contains("TLB cache size is very small")));
}

#[test]
fn test_smmu_config_validate_detailed_with_errors() {
    let mut config = SMMUConfig::default();
    config.queue_config.event_queue_size = 1; // Invalid

    let result = config.validate_detailed();

    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
}

// ============================================================================
// SMMUConfig Serialization Tests
// ============================================================================

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_to_string() {
    let config = SMMUConfig::default();
    let config_str = config.to_string();

    assert!(config_str.contains("event_queue_size"));
    assert!(config_str.contains("command_queue_size"));
    assert!(config_str.contains("pri_queue_size"));
    assert!(config_str.contains("tlb_cache_size"));
    assert!(config_str.contains("cache_max_age_ms"));
    assert!(config_str.contains("enable_caching"));
    assert!(config_str.contains("max_iova_bits"));
    assert!(config_str.contains("max_pa_bits"));
    assert!(config_str.contains("max_stream_count"));
    assert!(config_str.contains("max_pasid_count"));
    assert!(config_str.contains("max_memory_usage"));
    assert!(config_str.contains("max_thread_count"));
    assert!(config_str.contains("timeout_ms"));
    assert!(config_str.contains("enable_resource_tracking"));
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_valid() {
    let config_str = "event_queue_size=1024\ncommand_queue_size=512\npri_queue_size=256\n\
                      tlb_cache_size=2048\ncache_max_age_ms=10_000\nenable_caching=true\n\
                      max_iova_bits=48\nmax_pa_bits=52\nmax_stream_count=65_536\nmax_pasid_count=1_048_576\n\
                      max_memory_usage=1_073_741_824\nmax_thread_count=8\ntimeout_ms=1000\nenable_resource_tracking=true";

    let config = SMMUConfig::from_string(config_str).expect("valid config");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
    assert_eq!(config.queue_config.pri_queue_size, 256);
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
    assert_eq!(config.cache_config.cache_max_age_ms, 10_000);
    assert!(config.cache_config.enable_caching);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_with_comments() {
    let config_str = "# This is a comment\n\
                      event_queue_size=1024\n\
                      # Another comment\n\
                      command_queue_size=512\n\
                      pri_queue_size=256";

    let config = SMMUConfig::from_string(config_str).expect("valid config");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
    assert_eq!(config.queue_config.pri_queue_size, 256);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_with_empty_lines() {
    let config_str = "event_queue_size=1024\n\n\ncommand_queue_size=512\n\n";

    let config = SMMUConfig::from_string(config_str).expect("valid config");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_invalid_value() {
    let config_str = "event_queue_size=invalid";

    let result = SMMUConfig::from_string(config_str);
    assert!(result.is_err());
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_out_of_range() {
    let config_str = "event_queue_size=5";

    let result = SMMUConfig::from_string(config_str);
    assert!(result.is_err());
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_roundtrip() {
    let original = SMMUConfig::high_performance();
    let serialized = original.to_string();
    let deserialized = SMMUConfig::from_string(&serialized).expect("valid roundtrip");

    assert_eq!(original, deserialized);
}

// ============================================================================
// SMMUConfig Builder Tests
// ============================================================================

#[test]
fn test_smmu_config_builder_default() {
    let config = SMMUConfig::builder().build().expect("valid config");
    assert_eq!(config, SMMUConfig::default());
}

#[test]
fn test_smmu_config_builder_custom() {
    let config = SMMUConfig::builder()
        .queue_config(QueueConfig::builder().event_queue_size(1024).build().unwrap())
        .cache_config(CacheConfig::builder().tlb_cache_size(2048).build().unwrap())
        .address_config(AddressConfig::builder().max_stream_count(1024).build().unwrap())
        .resource_limits(ResourceLimits::builder().max_thread_count(16).build().unwrap())
        .build()
        .expect("valid config");

    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.cache_config.tlb_cache_size, 2048);
    assert_eq!(config.address_config.max_stream_count, 1024);
    assert_eq!(config.resource_limits.max_thread_count, 16);
}

#[test]
fn test_smmu_config_from_queue_config() {
    let queue_config = QueueConfig::builder().event_queue_size(1024).build().unwrap();

    let smmu_config: SMMUConfig = queue_config.clone().into();

    assert_eq!(smmu_config.queue_config, queue_config);
    assert_eq!(smmu_config.cache_config, CacheConfig::default());
    assert_eq!(smmu_config.address_config, AddressConfig::default());
    assert_eq!(smmu_config.resource_limits, ResourceLimits::default());
}

// ============================================================================
// ConfigurationError Tests
// ============================================================================

#[test]
fn test_configuration_error_construction() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "event_queue_size".to_string(),
        "value out of range".to_string(),
    );

    assert_eq!(error.error_type, ConfigurationErrorType::InvalidQueueSize);
    assert_eq!(error.field, "event_queue_size");
    assert_eq!(error.message, "value out of range");
}

#[test]
fn test_configuration_error_display() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "event_queue_size".to_string(),
        "value out of range".to_string(),
    );

    let display = format!("{error}");
    assert!(display.contains("Invalid queue size"));
    assert!(display.contains("event_queue_size"));
    assert!(display.contains("value out of range"));
}

#[test]
fn test_configuration_error_type_display() {
    assert_eq!(format!("{}", ConfigurationErrorType::InvalidQueueSize), "Invalid queue size");
    assert_eq!(format!("{}", ConfigurationErrorType::InvalidCacheSize), "Invalid cache size");
    assert_eq!(
        format!("{}", ConfigurationErrorType::InvalidAddressSize),
        "Invalid address size"
    );
    assert_eq!(
        format!("{}", ConfigurationErrorType::InvalidResourceLimit),
        "Invalid resource limit"
    );
    assert_eq!(format!("{}", ConfigurationErrorType::InvalidFormat), "Invalid format");
    assert_eq!(format!("{}", ConfigurationErrorType::MissingRequired), "Missing required field");
    assert_eq!(format!("{}", ConfigurationErrorType::OutOfRange), "Value out of range");
}

#[test]
fn test_configuration_error_from_validation_error() {
    let validation_error = ValidationError::InvalidConfiguration { reason: "test error".to_string() };

    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::InvalidFormat);
}

#[test]
fn test_configuration_error_from_invalid_pasid() {
    let validation_error = ValidationError::InvalidPASID { value: 9_999_999 };

    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::OutOfRange);
    assert!(config_error.message.contains("invalid PASID"));
}

#[cfg(feature = "std")]
#[test]
fn test_configuration_error_is_std_error() {
    use std::error::Error;

    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "test".to_string(),
        "test message".to_string(),
    );

    // Should implement std::error::Error
    let _: &dyn Error = &error;
}

// ============================================================================
// ValidationResult Tests
// ============================================================================

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
    assert!(result.warnings.is_empty());
}

#[test]
fn test_validation_result_add_error() {
    let mut result = ValidationResult::success();
    assert!(result.is_valid);

    result.add_error("error 1".to_string());
    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 1);

    result.add_error("error 2".to_string());
    assert_eq!(result.errors.len(), 2);
}

#[test]
fn test_validation_result_add_warning() {
    let mut result = ValidationResult::success();
    result.add_warning("warning 1".to_string());

    assert!(result.is_valid);
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0], "warning 1");
}

#[test]
fn test_validation_result_merge_success() {
    let mut result1 = ValidationResult::success();
    let result2 = ValidationResult::success();

    result1.merge(result2);
    assert!(result1.is_valid);
    assert!(result1.errors.is_empty());
}

#[test]
fn test_validation_result_merge_with_errors() {
    let mut result1 = ValidationResult::success();
    result1.add_error("error 1".to_string());

    let mut result2 = ValidationResult::success();
    result2.add_error("error 2".to_string());

    result1.merge(result2);
    assert!(!result1.is_valid);
    assert_eq!(result1.errors.len(), 2);
}

#[test]
fn test_validation_result_merge_with_warnings() {
    let mut result1 = ValidationResult::success();
    result1.add_warning("warning 1".to_string());

    let mut result2 = ValidationResult::success();
    result2.add_warning("warning 2".to_string());

    result1.merge(result2);
    assert!(result1.is_valid);
    assert_eq!(result1.warnings.len(), 2);
}

#[test]
fn test_validation_result_default() {
    let result = ValidationResult::default();
    assert!(!result.is_valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

// ============================================================================
// ConfigConstants Tests
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
    assert_eq!(ConfigConstants::CONFIG_VERSION, "v1.0.0");
}

#[test]
fn test_config_constants_env_vars() {
    assert_eq!(ConfigConstants::ENV_CONFIG_FILE, "SMMU_CONFIG_FILE");
    assert_eq!(ConfigConstants::ENV_QUEUE_SIZE, "SMMU_QUEUE_SIZE");
    assert_eq!(ConfigConstants::ENV_CACHE_SIZE, "SMMU_CACHE_SIZE");
    assert_eq!(ConfigConstants::ENV_MEMORY_LIMIT, "SMMU_MEMORY_LIMIT");
}

// ============================================================================
// Additional Edge Case Tests
// ============================================================================

#[test]
fn test_stream_config_default_is_bypass() {
    let config = StreamConfig::default();
    assert_eq!(config, StreamConfig::bypass());
}

#[test]
fn test_validation_error_from_permission_denied() {
    let validation_error = ValidationError::PermissionDenied {
        requested: "read-write".to_string(),
        available: "read-only".to_string(),
    };
    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::InvalidFormat);
    assert!(config_error.message.contains("PermissionDenied"));
}

#[test]
fn test_validation_error_from_security_violation() {
    let validation_error = ValidationError::SecurityViolation {
        from_state: "NonSecure".to_string(),
        to_state: "Secure".to_string(),
    };
    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::InvalidFormat);
}

#[test]
fn test_validation_error_from_invalid_alignment() {
    let validation_error = ValidationError::InvalidAlignment {
        address: 0x1001,
        required_alignment: 0x1000,
    };
    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::InvalidFormat);
}

#[test]
fn test_validation_error_from_generic() {
    let validation_error = ValidationError::Generic {
        field: "test_field".to_string(),
        value: "invalid_value".to_string(),
        constraint: "must be positive".to_string(),
    };
    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::InvalidFormat);
}

#[test]
fn test_stream_config_builder_default() {
    let config = StreamConfigBuilder::default().build();
    // Default builder creates a bypass config (no translation, no stages)
    // This is actually invalid since translation is disabled but we need to enable it
    // Actually, bypass mode is valid - translation disabled with no stages
    assert!(config.is_ok());
    let config = config.unwrap();
    assert!(!config.translation_enabled);
    assert!(!config.stage1_enabled);
    assert!(!config.stage2_enabled);
}

#[test]
fn test_stream_config_constants() {
    assert_eq!(StreamConfig::MIN_PASID, 0);
    assert_eq!(StreamConfig::MAX_PASID, (1 << 20) - 1);
}

#[test]
fn test_queue_config_builder_default() {
    let builder = QueueConfigBuilder::default();
    let config = builder.build().expect("default builder should be valid");
    assert_eq!(config, QueueConfig::default());
}

#[test]
fn test_cache_config_builder_default() {
    let builder = CacheConfigBuilder::default();
    let config = builder.build().expect("default builder should be valid");
    assert_eq!(config, CacheConfig::default());
}

#[test]
fn test_address_config_builder_default() {
    let builder = AddressConfigBuilder::default();
    let config = builder.build().expect("default builder should be valid");
    assert_eq!(config, AddressConfig::default());
}

#[test]
fn test_resource_limits_builder_default() {
    let builder = ResourceLimitsBuilder::default();
    let limits = builder.build().expect("default builder should be valid");
    assert_eq!(limits, ResourceLimits::default());
}

#[test]
fn test_smmu_config_builder_default_impl() {
    let builder = SMMUConfigBuilder::default();
    let config = builder.build().expect("default builder should be valid");
    assert_eq!(config, SMMUConfig::default());
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_partial() {
    let config_str = "event_queue_size=2048\ntlb_cache_size=4096";
    let config = SMMUConfig::from_string(config_str).expect("valid partial config");

    assert_eq!(config.queue_config.event_queue_size, 2048);
    assert_eq!(config.cache_config.tlb_cache_size, 4096);
    // Other values should be defaults
    assert_eq!(config.queue_config.command_queue_size, QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_all_fields() {
    let config_str = "event_queue_size=1024\n\
                      command_queue_size=512\n\
                      pri_queue_size=256\n\
                      tlb_cache_size=2048\n\
                      cache_max_age_ms=10_000\n\
                      enable_caching=true\n\
                      max_iova_bits=48\n\
                      max_pa_bits=52\n\
                      max_stream_count=65_536\n\
                      max_pasid_count=1_048_576\n\
                      max_memory_usage=1_073_741_824\n\
                      max_thread_count=8\n\
                      timeout_ms=1000\n\
                      enable_resource_tracking=true";

    let config = SMMUConfig::from_string(config_str).expect("valid full config");
    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert!(config.cache_config.enable_caching);
    assert_eq!(config.address_config.max_iova_bits, 48);
    assert!(config.resource_limits.enable_resource_tracking);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_enable_caching_false() {
    let config_str = "enable_caching=false";
    let config = SMMUConfig::from_string(config_str).expect("valid config");
    assert!(!config.cache_config.enable_caching);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_enable_resource_tracking_false() {
    let config_str = "enable_resource_tracking=false";
    let config = SMMUConfig::from_string(config_str).expect("valid config");
    assert!(!config.resource_limits.enable_resource_tracking);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_bool_parse_error() {
    let config_str = "enable_caching=maybe";
    let result = SMMUConfig::from_string(config_str);
    assert!(result.is_err());
}

#[test]
fn test_validation_result_merge_preserves_validity() {
    let mut result1 = ValidationResult::with_error("error 1".to_string());
    let result2 = ValidationResult::success();

    result1.merge(result2);
    assert!(!result1.is_valid); // Should remain invalid
}

#[test]
fn test_validation_result_merge_invalidates_on_error() {
    let mut result1 = ValidationResult::success();
    let result2 = ValidationResult::with_error("error".to_string());

    result1.merge(result2);
    assert!(!result1.is_valid); // Should become invalid
}

#[test]
fn test_configuration_error_types_coverage() {
    let types = vec![
        ConfigurationErrorType::InvalidQueueSize,
        ConfigurationErrorType::InvalidCacheSize,
        ConfigurationErrorType::InvalidAddressSize,
        ConfigurationErrorType::InvalidResourceLimit,
        ConfigurationErrorType::InvalidFormat,
        ConfigurationErrorType::MissingRequired,
        ConfigurationErrorType::OutOfRange,
    ];

    for error_type in types {
        let error = ConfigurationError::new(error_type, "test".to_string(), "test message".to_string());
        assert_eq!(error.error_type, error_type);
    }
}

#[test]
fn test_fault_mode_equality() {
    assert_eq!(FaultMode::Terminate, FaultMode::Terminate);
    assert_eq!(FaultMode::Stall, FaultMode::Stall);
    assert_ne!(FaultMode::Terminate, FaultMode::Stall);
}

#[test]
fn test_fault_mode_clone() {
    let mode = FaultMode::Stall;
    let cloned = mode;
    assert_eq!(mode, cloned);
}

#[test]
fn test_fault_mode_copy() {
    let mode = FaultMode::Stall;
    let copied = mode;
    assert_eq!(mode, copied);
}

#[test]
fn test_fault_mode_hash() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(FaultMode::Terminate, "terminate");
    map.insert(FaultMode::Stall, "stall");
    assert_eq!(map.get(&FaultMode::Terminate), Some(&"terminate"));
    assert_eq!(map.get(&FaultMode::Stall), Some(&"stall"));
}

#[test]
fn test_stream_config_clone() {
    let config = StreamConfig::stage1_only();
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

#[test]
fn test_queue_config_clone() {
    let config = QueueConfig::default();
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

#[test]
fn test_cache_config_clone() {
    let config = CacheConfig::default();
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

#[test]
fn test_address_config_clone() {
    let config = AddressConfig::default();
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

#[test]
fn test_resource_limits_clone() {
    let limits = ResourceLimits::default();
    let cloned = limits.clone();
    assert_eq!(limits, cloned);
}

#[test]
fn test_smmu_config_clone() {
    let config = SMMUConfig::default();
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

#[test]
fn test_queue_config_constants_values() {
    assert_eq!(QueueConfig::MIN_QUEUE_SIZE, 16);
    assert_eq!(QueueConfig::MAX_QUEUE_SIZE, 65_536);
    assert_eq!(QueueConfig::DEFAULT_EVENT_QUEUE_SIZE, 512);
    assert_eq!(QueueConfig::DEFAULT_COMMAND_QUEUE_SIZE, 256);
    assert_eq!(QueueConfig::DEFAULT_PRI_QUEUE_SIZE, 128);
}

#[test]
fn test_cache_config_constants_values() {
    assert_eq!(CacheConfig::MIN_CACHE_SIZE, 64);
    assert_eq!(CacheConfig::MAX_CACHE_SIZE, 1_048_576);
    assert_eq!(CacheConfig::MIN_CACHE_AGE_MS, 100);
    assert_eq!(CacheConfig::MAX_CACHE_AGE_MS, 3_600_000);
    assert_eq!(CacheConfig::DEFAULT_TLB_CACHE_SIZE, 1024);
    assert_eq!(CacheConfig::DEFAULT_CACHE_MAX_AGE_MS, 5000);
}

#[test]
fn test_address_config_constants_values() {
    assert_eq!(AddressConfig::MIN_IOVA_BITS, 32);
    assert_eq!(AddressConfig::MAX_IOVA_BITS, 52);
    assert_eq!(AddressConfig::MIN_PA_BITS, 32);
    assert_eq!(AddressConfig::MAX_PA_BITS, 52);
    assert_eq!(AddressConfig::MIN_STREAM_COUNT, 1);
    assert_eq!(AddressConfig::MAX_STREAM_COUNT, 1_048_576);
    assert_eq!(AddressConfig::MIN_PASID_COUNT, 1);
    assert_eq!(AddressConfig::MAX_PASID_COUNT, 1_048_576);
    assert_eq!(AddressConfig::DEFAULT_IOVA_BITS, 48);
    assert_eq!(AddressConfig::DEFAULT_PA_BITS, 52);
    assert_eq!(AddressConfig::DEFAULT_STREAM_COUNT, 65_536);
    assert_eq!(AddressConfig::DEFAULT_PASID_COUNT, 1_048_576);
}

#[test]
fn test_resource_limits_constants_values() {
    assert_eq!(ResourceLimits::MIN_MEMORY_USAGE, 1024 * 1024);
    assert_eq!(ResourceLimits::MAX_MEMORY_USAGE, 64 * 1024 * 1024 * 1024);
    assert_eq!(ResourceLimits::MIN_THREAD_COUNT, 1);
    assert_eq!(ResourceLimits::MAX_THREAD_COUNT, 256);
    assert_eq!(ResourceLimits::MIN_TIMEOUT_MS, 10);
    assert_eq!(ResourceLimits::MAX_TIMEOUT_MS, 300_000);
    assert_eq!(ResourceLimits::DEFAULT_MAX_MEMORY_USAGE, 1024 * 1024 * 1024);
    assert_eq!(ResourceLimits::DEFAULT_MAX_THREAD_COUNT, 8);
    assert_eq!(ResourceLimits::DEFAULT_TIMEOUT_MS, 1000);
}

#[test]
fn test_queue_config_overflow_test_exception() {
    // Special case: size 4 is allowed for overflow testing
    let config = QueueConfig {
        event_queue_size: 4,
        command_queue_size: 4,
        pri_queue_size: 4,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validation_error_from_other_types() {
    // Test the catch-all case in From<ValidationError> for ConfigurationError
    let validation_error = ValidationError::OutOfRange {
        field: "stream_id".to_string(),
        value: 999_999,
        max: 65_536,
    };
    let config_error: ConfigurationError = validation_error.into();
    assert_eq!(config_error.error_type, ConfigurationErrorType::InvalidFormat);
    assert!(config_error.message.contains("OutOfRange"));
}

// ============================================================================
// Debug and Display Tests
// ============================================================================

#[test]
fn test_stream_config_debug() {
    let config = StreamConfig::stage1_only();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("StreamConfig"));
    assert!(debug_str.contains("translation_enabled"));
}

#[test]
fn test_stream_config_builder_debug() {
    let builder = StreamConfig::builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("StreamConfigBuilder"));
}

#[test]
fn test_queue_config_debug() {
    let config = QueueConfig::default();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("QueueConfig"));
}

#[test]
fn test_queue_config_builder_debug() {
    let builder = QueueConfig::builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("QueueConfigBuilder"));
}

#[test]
fn test_cache_config_debug() {
    let config = CacheConfig::default();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("CacheConfig"));
}

#[test]
fn test_cache_config_builder_debug() {
    let builder = CacheConfig::builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("CacheConfigBuilder"));
}

#[test]
fn test_address_config_debug() {
    let config = AddressConfig::default();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("AddressConfig"));
}

#[test]
fn test_address_config_builder_debug() {
    let builder = AddressConfig::builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("AddressConfigBuilder"));
}

#[test]
fn test_resource_limits_debug() {
    let limits = ResourceLimits::default();
    let debug_str = format!("{limits:?}");
    assert!(debug_str.contains("ResourceLimits"));
}

#[test]
fn test_resource_limits_builder_debug() {
    let builder = ResourceLimits::builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("ResourceLimitsBuilder"));
}

#[test]
fn test_smmu_config_debug() {
    let config = SMMUConfig::default();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("SMMUConfig"));
}

#[test]
fn test_smmu_config_builder_debug() {
    let builder = SMMUConfig::builder();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("SMMUConfigBuilder"));
}

#[test]
fn test_fault_mode_debug() {
    let mode = FaultMode::Stall;
    let debug_str = format!("{mode:?}");
    assert!(debug_str.contains("Stall"));
}

#[test]
fn test_configuration_error_debug() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "test".to_string(),
        "test message".to_string(),
    );
    let debug_str = format!("{error:?}");
    assert!(debug_str.contains("ConfigurationError"));
}

#[test]
fn test_configuration_error_type_debug() {
    let error_type = ConfigurationErrorType::InvalidQueueSize;
    let debug_str = format!("{error_type:?}");
    assert!(debug_str.contains("InvalidQueueSize"));
}

#[test]
fn test_validation_result_debug() {
    let result = ValidationResult::success();
    let debug_str = format!("{result:?}");
    assert!(debug_str.contains("ValidationResult"));
}

#[test]
fn test_config_constants_debug() {
    let constants = ConfigConstants;
    let debug_str = format!("{constants:?}");
    assert!(debug_str.contains("ConfigConstants"));
}

// ============================================================================
// PartialEq and Eq Tests
// ============================================================================

#[test]
fn test_stream_config_equality() {
    let config1 = StreamConfig::stage1_only();
    let config2 = StreamConfig::stage1_only();
    let config3 = StreamConfig::stage2_only();

    assert_eq!(config1, config2);
    assert_ne!(config1, config3);
}

#[test]
fn test_queue_config_equality() {
    let config1 = QueueConfig::default();
    let config2 = QueueConfig::default();
    let mut config3 = QueueConfig::default();
    config3.event_queue_size = 1024;

    assert_eq!(config1, config2);
    assert_ne!(config1, config3);
}

#[test]
fn test_cache_config_equality() {
    let config1 = CacheConfig::default();
    let config2 = CacheConfig::default();
    let mut config3 = CacheConfig::default();
    config3.tlb_cache_size = 2048;

    assert_eq!(config1, config2);
    assert_ne!(config1, config3);
}

#[test]
fn test_address_config_equality() {
    let config1 = AddressConfig::default();
    let config2 = AddressConfig::default();
    let mut config3 = AddressConfig::default();
    config3.max_iova_bits = 40;

    assert_eq!(config1, config2);
    assert_ne!(config1, config3);
}

#[test]
fn test_resource_limits_equality() {
    let limits1 = ResourceLimits::default();
    let limits2 = ResourceLimits::default();
    let mut limits3 = ResourceLimits::default();
    limits3.max_thread_count = 16;

    assert_eq!(limits1, limits2);
    assert_ne!(limits1, limits3);
}

#[test]
fn test_smmu_config_equality() {
    let config1 = SMMUConfig::default();
    let config2 = SMMUConfig::default();
    let config3 = SMMUConfig::high_performance();

    assert_eq!(config1, config2);
    assert_ne!(config1, config3);
}

#[test]
fn test_configuration_error_equality() {
    let error1 = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "test".to_string(),
        "message".to_string(),
    );
    let error2 = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "test".to_string(),
        "message".to_string(),
    );
    let error3 = ConfigurationError::new(
        ConfigurationErrorType::InvalidCacheSize,
        "test".to_string(),
        "message".to_string(),
    );

    assert_eq!(error1, error2);
    assert_ne!(error1, error3);
}

#[test]
fn test_configuration_error_type_equality() {
    assert_eq!(
        ConfigurationErrorType::InvalidQueueSize,
        ConfigurationErrorType::InvalidQueueSize
    );
    assert_ne!(
        ConfigurationErrorType::InvalidQueueSize,
        ConfigurationErrorType::InvalidCacheSize
    );
}

#[test]
fn test_validation_result_equality() {
    let result1 = ValidationResult::success();
    let result2 = ValidationResult::success();
    let result3 = ValidationResult::with_error("error".to_string());

    assert_eq!(result1, result2);
    assert_ne!(result1, result3);
}

// ============================================================================
// Builder Pattern Completeness Tests
// ============================================================================

#[test]
fn test_stream_config_builder_new() {
    let builder = StreamConfigBuilder::new();
    let config = builder.build();
    // Default builder creates bypass config which is valid
    assert!(config.is_ok());
}

#[test]
fn test_queue_config_builder_new() {
    let builder = QueueConfigBuilder::new();
    let config = builder.build().expect("default should be valid");
    assert_eq!(config, QueueConfig::default());
}

#[test]
fn test_cache_config_builder_new() {
    let builder = CacheConfigBuilder::new();
    let config = builder.build().expect("default should be valid");
    assert_eq!(config, CacheConfig::default());
}

#[test]
fn test_address_config_builder_new() {
    let builder = AddressConfigBuilder::new();
    let config = builder.build().expect("default should be valid");
    assert_eq!(config, AddressConfig::default());
}

#[test]
fn test_resource_limits_builder_new() {
    let builder = ResourceLimitsBuilder::new();
    let limits = builder.build().expect("default should be valid");
    assert_eq!(limits, ResourceLimits::default());
}

#[test]
fn test_smmu_config_builder_new() {
    let builder = SMMUConfigBuilder::new();
    let config = builder.build().expect("default should be valid");
    assert_eq!(config, SMMUConfig::default());
}

// ============================================================================
// Copy and Clone Trait Tests
// ============================================================================

#[test]
fn test_configuration_error_clone() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "test".to_string(),
        "message".to_string(),
    );
    let cloned = error.clone();
    assert_eq!(error, cloned);
}

#[test]
fn test_configuration_error_type_clone() {
    let error_type = ConfigurationErrorType::InvalidQueueSize;
    let cloned = error_type;
    assert_eq!(error_type, cloned);
}

#[test]
fn test_configuration_error_type_copy() {
    let error_type = ConfigurationErrorType::InvalidQueueSize;
    let copied = error_type;
    assert_eq!(error_type, copied);
}

#[test]
fn test_validation_result_clone() {
    let result = ValidationResult::success();
    let cloned = result.clone();
    assert_eq!(result, cloned);
}

#[test]
fn test_stream_config_builder_clone() {
    let builder = StreamConfig::builder();
    let cloned = builder.clone();
    let config1 = builder.build();
    let config2 = cloned.build();
    assert_eq!(config1.is_ok(), config2.is_ok());
}

#[test]
fn test_queue_config_builder_clone() {
    let builder = QueueConfig::builder();
    let cloned = builder.clone();
    let config1 = builder.build().unwrap();
    let config2 = cloned.build().unwrap();
    assert_eq!(config1, config2);
}

#[test]
fn test_cache_config_builder_clone() {
    let builder = CacheConfig::builder();
    let cloned = builder.clone();
    let config1 = builder.build().unwrap();
    let config2 = cloned.build().unwrap();
    assert_eq!(config1, config2);
}

#[test]
fn test_address_config_builder_clone() {
    let builder = AddressConfig::builder();
    let cloned = builder.clone();
    let config1 = builder.build().unwrap();
    let config2 = cloned.build().unwrap();
    assert_eq!(config1, config2);
}

#[test]
fn test_resource_limits_builder_clone() {
    let builder = ResourceLimits::builder();
    let cloned = builder.clone();
    let limits1 = builder.build().unwrap();
    let limits2 = cloned.build().unwrap();
    assert_eq!(limits1, limits2);
}

#[test]
fn test_smmu_config_builder_clone() {
    let builder = SMMUConfig::builder();
    let cloned = builder.clone();
    let config1 = builder.build().unwrap();
    let config2 = cloned.build().unwrap();
    assert_eq!(config1, config2);
}

// ============================================================================
// Hash Trait Tests
// ============================================================================

#[test]
fn test_configuration_error_type_hash() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(ConfigurationErrorType::InvalidQueueSize, "queue");
    map.insert(ConfigurationErrorType::InvalidCacheSize, "cache");
    assert_eq!(map.get(&ConfigurationErrorType::InvalidQueueSize), Some(&"queue"));
}

// ============================================================================
// Additional Edge Cases
// ============================================================================

#[test]
fn test_smmu_config_update_resource_limits_preserves_tracking() {
    let mut config = SMMUConfig::default();
    config.resource_limits.enable_resource_tracking = false;

    let result = config.update_resource_limits(2 * 1024 * 1024 * 1024, 16, 2000);
    assert!(result.is_ok());
    // enable_resource_tracking should be preserved
    assert!(!config.resource_limits.enable_resource_tracking);
}

#[test]
#[allow(clippy::no_effect_underscore_binding)]
fn test_config_constants_copy() {
    let _c1 = ConfigConstants;
    let _c2 = ConfigConstants;
    // ConfigConstants is Copy, so this should compile
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_whitespace_handling() {
    let config_str = "  event_queue_size  =  1024  \n  command_queue_size=512";
    let config = SMMUConfig::from_string(config_str).expect("valid config with whitespace");
    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_missing_equals() {
    let config_str = "event_queue_size 1024\ncommand_queue_size=512";
    let config = SMMUConfig::from_string(config_str).expect("valid config");
    // Lines without '=' are ignored
    assert_eq!(config.queue_config.command_queue_size, 512);
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::DEFAULT_EVENT_QUEUE_SIZE);
}

#[test]
fn test_stream_config_builder_all_fields() {
    // Test setting all fields through builder
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(false)
        .pasid_enabled(false)
        .max_pasid(0)
        .fault_mode(FaultMode::Terminate)
        .security_enforced(false)
        .build()
        .expect("valid config");

    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
    assert!(!config.stage2_enabled);
    assert!(!config.pasid_enabled);
    assert_eq!(config.max_pasid, 0);
    assert_eq!(config.fault_mode, FaultMode::Terminate);
    assert!(!config.security_enforced);
}

#[test]
fn test_queue_config_builder_all_fields_individually() {
    let builder = QueueConfigBuilder::new();
    let builder = builder.event_queue_size(1024);
    let builder = builder.command_queue_size(512);
    let builder = builder.pri_queue_size(256);
    let config = builder.build().expect("valid config");

    assert_eq!(config.event_queue_size, 1024);
    assert_eq!(config.command_queue_size, 512);
    assert_eq!(config.pri_queue_size, 256);
}

#[test]
fn test_cache_config_builder_all_fields_individually() {
    let builder = CacheConfigBuilder::new();
    let builder = builder.tlb_cache_size(2048);
    let builder = builder.cache_max_age_ms(10_000);
    let builder = builder.enable_caching(false);
    let config = builder.build().expect("valid config");

    assert_eq!(config.tlb_cache_size, 2048);
    assert_eq!(config.cache_max_age_ms, 10_000);
    assert!(!config.enable_caching);
}

#[test]
fn test_address_config_builder_all_fields_individually() {
    let builder = AddressConfigBuilder::new();
    let builder = builder.max_iova_bits(40);
    let builder = builder.max_pa_bits(44);
    let builder = builder.max_stream_count(2048);
    let builder = builder.max_pasid_count(4096);
    let config = builder.build().expect("valid config");

    assert_eq!(config.max_iova_bits, 40);
    assert_eq!(config.max_pa_bits, 44);
    assert_eq!(config.max_stream_count, 2048);
    assert_eq!(config.max_pasid_count, 4096);
}

#[test]
fn test_resource_limits_builder_all_fields_individually() {
    let builder = ResourceLimitsBuilder::new();
    let builder = builder.max_memory_usage(2 * 1024 * 1024 * 1024);
    let builder = builder.max_thread_count(16);
    let builder = builder.timeout_ms(5000);
    let builder = builder.enable_resource_tracking(false);
    let limits = builder.build().expect("valid limits");

    assert_eq!(limits.max_memory_usage, 2 * 1024 * 1024 * 1024);
    assert_eq!(limits.max_thread_count, 16);
    assert_eq!(limits.timeout_ms, 5000);
    assert!(!limits.enable_resource_tracking);
}

#[test]
fn test_smmu_config_builder_all_fields_individually() {
    let builder = SMMUConfigBuilder::new();
    let queue_config = QueueConfig::builder().event_queue_size(1024).build().unwrap();
    let cache_config = CacheConfig::builder().tlb_cache_size(2048).build().unwrap();
    let address_config = AddressConfig::builder().max_stream_count(1024).build().unwrap();
    let resource_limits = ResourceLimits::builder().max_thread_count(16).build().unwrap();

    let builder = builder.queue_config(queue_config.clone());
    let builder = builder.cache_config(cache_config.clone());
    let builder = builder.address_config(address_config.clone());
    let builder = builder.resource_limits(resource_limits.clone());
    let config = builder.build().expect("valid config");

    assert_eq!(config.queue_config, queue_config);
    assert_eq!(config.cache_config, cache_config);
    assert_eq!(config.address_config, address_config);
    assert_eq!(config.resource_limits, resource_limits);
}

#[test]
fn test_configuration_error_type_all_variants_display() {
    // Test Display trait for all ConfigurationErrorType variants
    let types = [
        (ConfigurationErrorType::InvalidQueueSize, "Invalid queue size"),
        (ConfigurationErrorType::InvalidCacheSize, "Invalid cache size"),
        (ConfigurationErrorType::InvalidAddressSize, "Invalid address size"),
        (ConfigurationErrorType::InvalidResourceLimit, "Invalid resource limit"),
        (ConfigurationErrorType::InvalidFormat, "Invalid format"),
        (ConfigurationErrorType::MissingRequired, "Missing required field"),
        (ConfigurationErrorType::OutOfRange, "Value out of range"),
    ];

    for (variant, expected_str) in &types {
        assert_eq!(format!("{variant}"), *expected_str);
    }
}

// ============================================================================
// 100% Coverage Completion Tests
// ============================================================================

#[test]
fn test_fault_mode_repr_values() {
    // Ensure FaultMode variants have correct repr values
    assert_eq!(FaultMode::Terminate as u8, 0);
    assert_eq!(FaultMode::Stall as u8, 1);
}

#[test]
fn test_stream_config_max_pasid_at_boundary() {
    // Test PASID exactly at MAX boundary
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .pasid_enabled(true)
        .max_pasid(StreamConfig::MAX_PASID)
        .build()
        .expect("valid config at MAX_PASID");

    assert_eq!(config.max_pasid, StreamConfig::MAX_PASID);
}

#[test]
#[allow(clippy::no_effect_underscore_binding)]
fn test_config_constants_clone() {
    // ConfigConstants is Copy+Clone
    let c1 = ConfigConstants;
    let c2 = c1;
    let _c3 = c2; // Test Copy
}

#[test]
fn test_validation_result_multiple_errors_and_warnings() {
    let mut result = ValidationResult::success();
    result.add_error("error 1".to_string());
    result.add_error("error 2".to_string());
    result.add_error("error 3".to_string());
    result.add_warning("warning 1".to_string());
    result.add_warning("warning 2".to_string());

    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 3);
    assert_eq!(result.warnings.len(), 2);
}

#[test]
fn test_smmu_config_validate_all_subconfigs_invalid() {
    let mut config = SMMUConfig::default();
    config.queue_config.event_queue_size = 1; // Invalid
    config.cache_config.tlb_cache_size = 1; // Invalid
    config.address_config.max_iova_bits = 1; // Invalid
    config.resource_limits.max_memory_usage = 1; // Invalid

    let result = config.validate_detailed();

    assert!(!result.is_valid);
    // Should have multiple errors
    assert!(result.errors.len() >= 4);
}

#[test]
fn test_smmu_config_validate_detailed_small_queue_warning() {
    let mut config = SMMUConfig::default();
    config.queue_config.event_queue_size = 64; // Small but valid

    let result = config.validate_detailed();

    // Should be valid but have a warning
    assert!(result.is_valid);
    assert!(result.warnings.iter().any(|w| w.contains("Event queue size is very small")));
}

#[test]
fn test_smmu_config_validate_detailed_small_cache_warning() {
    let mut config = SMMUConfig::default();
    config.cache_config.tlb_cache_size = 128; // Small but valid

    let result = config.validate_detailed();

    // Should be valid but have a warning
    assert!(result.is_valid);
    assert!(result.warnings.iter().any(|w| w.contains("TLB cache size is very small")));
}

#[test]
fn test_queue_config_with_methods_chaining() {
    let config = QueueConfig::default()
        .with_event_queue_size(1024)
        .with_command_queue_size(512)
        .with_pri_queue_size(256);

    assert_eq!(config.event_queue_size, 1024);
    assert_eq!(config.command_queue_size, 512);
    assert_eq!(config.pri_queue_size, 256);

    // Verify config is still valid
    assert!(config.validate().is_ok());
}

#[test]
fn test_address_config_all_valid_iova_bits() {
    // Test all valid IOVA bit values
    for bits in AddressConfig::MIN_IOVA_BITS..=AddressConfig::MAX_IOVA_BITS {
        let config = AddressConfig::builder()
            .max_iova_bits(bits)
            .build()
            .unwrap_or_else(|_| panic!("valid config with {bits} IOVA bits"));
        assert_eq!(config.max_iova_bits, bits);
    }
}

#[test]
fn test_address_config_all_valid_pa_bits() {
    // Test all valid PA bit values
    for bits in AddressConfig::MIN_PA_BITS..=AddressConfig::MAX_PA_BITS {
        let config = AddressConfig::builder()
            .max_pa_bits(bits)
            .build()
            .unwrap_or_else(|_| panic!("valid config with {bits} PA bits"));
        assert_eq!(config.max_pa_bits, bits);
    }
}

#[cfg(feature = "std")]
#[test]
fn test_resource_limits_timeout_duration_conversion() {
    // Test timeout conversion for various values
    let test_cases = [
        (ResourceLimits::MIN_TIMEOUT_MS, u128::from(ResourceLimits::MIN_TIMEOUT_MS)),
        (1000, 1000u128),
        (5000, 5000u128),
        (ResourceLimits::MAX_TIMEOUT_MS, u128::from(ResourceLimits::MAX_TIMEOUT_MS)),
    ];

    for (timeout_ms, expected_millis) in &test_cases {
        let limits = ResourceLimits::builder().timeout_ms(*timeout_ms).build().expect("valid limits");
        let duration = limits.timeout();
        assert_eq!(duration.as_millis(), *expected_millis);
    }
}

#[cfg(feature = "std")]
#[test]
fn test_cache_config_cache_max_age_duration_conversion() {
    // Test cache age conversion for various values
    let test_cases = [
        (CacheConfig::MIN_CACHE_AGE_MS, u128::from(CacheConfig::MIN_CACHE_AGE_MS)),
        (5000, 5000u128),
        (CacheConfig::MAX_CACHE_AGE_MS, u128::from(CacheConfig::MAX_CACHE_AGE_MS)),
    ];

    for (age_ms, expected_millis) in &test_cases {
        let config = CacheConfig::builder().cache_max_age_ms(*age_ms).build().expect("valid config");
        let duration = config.cache_max_age();
        assert_eq!(duration.as_millis(), *expected_millis);
    }
}

#[test]
fn test_resource_limits_memory_conversion_functions() {
    let limits = ResourceLimits::builder()
        .max_memory_usage(8 * 1024 * 1024 * 1024) // 8GB
        .build()
        .expect("valid limits");

    assert_eq!(limits.max_memory_bytes(), 8 * 1024 * 1024 * 1024);
    assert_eq!(limits.max_memory_kb(), 8 * 1024 * 1024);
    assert_eq!(limits.max_memory_mb(), 8 * 1024);
    assert_eq!(limits.max_memory_gb(), 8);
}

#[test]
fn test_resource_limits_memory_conversion_edge_cases() {
    // Test with exact GB boundary
    let limits = ResourceLimits::builder()
        .max_memory_usage(1024 * 1024 * 1024) // Exactly 1GB
        .build()
        .expect("valid limits");

    assert_eq!(limits.max_memory_gb(), 1);
    assert_eq!(limits.max_memory_mb(), 1024);

    // Test with non-exact boundary
    let limits2 = ResourceLimits::builder()
        .max_memory_usage(1536 * 1024 * 1024) // 1.5GB
        .build()
        .expect("valid limits");

    assert_eq!(limits2.max_memory_gb(), 1); // Integer division
    assert_eq!(limits2.max_memory_mb(), 1536);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_all_boolean_combinations() {
    // Test parsing both true and false for boolean fields
    let config_str = "enable_caching=false\nenable_resource_tracking=false";
    let config = SMMUConfig::from_string(config_str).expect("valid config");
    assert!(!config.cache_config.enable_caching);
    assert!(!config.resource_limits.enable_resource_tracking);

    let config_str2 = "enable_caching=true\nenable_resource_tracking=true";
    let config2 = SMMUConfig::from_string(config_str2).expect("valid config");
    assert!(config2.cache_config.enable_caching);
    assert!(config2.resource_limits.enable_resource_tracking);
}

#[test]
fn test_smmu_config_all_profiles_are_valid() {
    // Ensure all profile methods produce valid configs
    let profiles = [
        SMMUConfig::default_config(),
        SMMUConfig::high_performance(),
        SMMUConfig::low_memory(),
        SMMUConfig::minimal(),
        SMMUConfig::server_profile(),
        SMMUConfig::embedded_profile(),
        SMMUConfig::development_profile(),
    ];

    for profile in &profiles {
        assert!(profile.validate().is_ok(), "Profile should be valid");
    }
}

#[test]
fn test_stream_config_all_predefined_configs_valid() {
    // Ensure all predefined stream configs are valid
    let configs = [
        StreamConfig::bypass(),
        StreamConfig::stage1_only(),
        StreamConfig::stage2_only(),
        StreamConfig::two_stage(),
        StreamConfig::default(),
    ];

    for config in &configs {
        assert!(config.validate().is_ok(), "Predefined config should be valid");
    }
}

#[test]
#[allow(clippy::similar_names)]
fn test_configuration_error_type_hash_uniqueness() {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::HashSet;
    use std::hash::{Hash, Hasher};

    let types = [
        ConfigurationErrorType::InvalidQueueSize,
        ConfigurationErrorType::InvalidCacheSize,
        ConfigurationErrorType::InvalidAddressSize,
        ConfigurationErrorType::InvalidResourceLimit,
        ConfigurationErrorType::InvalidFormat,
        ConfigurationErrorType::MissingRequired,
        ConfigurationErrorType::OutOfRange,
    ];

    let mut hashes = HashSet::new();
    for error_type in &types {
        let mut hasher = DefaultHasher::new();
        error_type.hash(&mut hasher);
        let hash = hasher.finish();
        hashes.insert(hash);
    }

    // All error types should have unique hashes (or at least we test the Hash impl works)
    assert_eq!(hashes.len(), types.len());
}

#[test]
fn test_configuration_error_from_all_validation_error_types() {
    // Test conversion from all possible ValidationError types
    let validation_errors: Vec<ValidationError> = vec![
        ValidationError::InvalidConfiguration { reason: "test".to_string() },
        ValidationError::InvalidPASID { value: 999_999 },
        ValidationError::InvalidAlignment {
            address: 0x1001,
            required_alignment: 0x1000,
        },
        ValidationError::PermissionDenied {
            requested: "rw".to_string(),
            available: "r".to_string(),
        },
        ValidationError::OutOfRange {
            field: "test".to_string(),
            value: 100,
            max: 50,
        },
        ValidationError::InvalidAccessType { bits: 0xFF },
        ValidationError::InvalidSecurityState { bits: 0xFF },
        ValidationError::InvalidTranslationStage { bits: 0xFF },
        ValidationError::InvalidFaultType { code: 0xFF },
        ValidationError::InvalidStateTransition {
            from: "A".to_string(),
            to: "B".to_string(),
        },
        ValidationError::SecurityViolation {
            from_state: "NS".to_string(),
            to_state: "S".to_string(),
        },
        ValidationError::Generic {
            field: "test".to_string(),
            value: "val".to_string(),
            constraint: "c".to_string(),
        },
    ];

    for validation_error in validation_errors {
        let config_error: ConfigurationError = validation_error.into();
        // All should convert successfully
        assert!(!config_error.message.is_empty());
    }
}

#[test]
fn test_smmu_config_update_queue_sizes_boundary() {
    let mut config = SMMUConfig::default();

    // Test MIN boundary
    let result = config.update_queue_sizes(
        QueueConfig::MIN_QUEUE_SIZE,
        QueueConfig::MIN_QUEUE_SIZE,
        QueueConfig::MIN_QUEUE_SIZE,
    );
    assert!(result.is_ok());
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::MIN_QUEUE_SIZE);

    // Test MAX boundary
    let result = config.update_queue_sizes(
        QueueConfig::MAX_QUEUE_SIZE,
        QueueConfig::MAX_QUEUE_SIZE,
        QueueConfig::MAX_QUEUE_SIZE,
    );
    assert!(result.is_ok());
    assert_eq!(config.queue_config.event_queue_size, QueueConfig::MAX_QUEUE_SIZE);
}

#[test]
fn test_smmu_config_update_cache_settings_boundary() {
    let mut config = SMMUConfig::default();

    // Test MIN boundary
    let result = config.update_cache_settings(CacheConfig::MIN_CACHE_SIZE, CacheConfig::MIN_CACHE_AGE_MS, true);
    assert!(result.is_ok());
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::MIN_CACHE_SIZE);

    // Test MAX boundary
    let result = config.update_cache_settings(CacheConfig::MAX_CACHE_SIZE, CacheConfig::MAX_CACHE_AGE_MS, false);
    assert!(result.is_ok());
    assert_eq!(config.cache_config.tlb_cache_size, CacheConfig::MAX_CACHE_SIZE);
    assert!(!config.cache_config.enable_caching);
}

#[test]
fn test_smmu_config_update_address_limits_boundary() {
    let mut config = SMMUConfig::default();

    // Test MIN boundary
    let result = config.update_address_limits(
        AddressConfig::MIN_IOVA_BITS,
        AddressConfig::MIN_PA_BITS,
        AddressConfig::MIN_STREAM_COUNT,
        AddressConfig::MIN_PASID_COUNT,
    );
    assert!(result.is_ok());
    assert_eq!(config.address_config.max_iova_bits, AddressConfig::MIN_IOVA_BITS);

    // Test MAX boundary
    let result = config.update_address_limits(
        AddressConfig::MAX_IOVA_BITS,
        AddressConfig::MAX_PA_BITS,
        AddressConfig::MAX_STREAM_COUNT,
        AddressConfig::MAX_PASID_COUNT,
    );
    assert!(result.is_ok());
    assert_eq!(config.address_config.max_iova_bits, AddressConfig::MAX_IOVA_BITS);
}

#[test]
fn test_smmu_config_update_resource_limits_boundary() {
    let mut config = SMMUConfig::default();

    // Test MIN boundary
    let result = config.update_resource_limits(
        ResourceLimits::MIN_MEMORY_USAGE,
        ResourceLimits::MIN_THREAD_COUNT,
        ResourceLimits::MIN_TIMEOUT_MS,
    );
    assert!(result.is_ok());
    assert_eq!(config.resource_limits.max_memory_usage, ResourceLimits::MIN_MEMORY_USAGE);

    // Test MAX boundary
    let result = config.update_resource_limits(
        ResourceLimits::MAX_MEMORY_USAGE,
        ResourceLimits::MAX_THREAD_COUNT,
        ResourceLimits::MAX_TIMEOUT_MS,
    );
    assert!(result.is_ok());
    assert_eq!(config.resource_limits.max_memory_usage, ResourceLimits::MAX_MEMORY_USAGE);
}

// ============================================================================
// Final Coverage Push - Testing Uncovered Paths
// ============================================================================

#[test]
fn test_stream_config_builder_validation_at_build() {
    // Test that validation happens at build time
    let builder = StreamConfig::builder().translation_enabled(false).stage1_enabled(true); // Invalid: stages enabled without translation

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_queue_config_builder_validation_at_build() {
    // Test that validation happens at build time
    let builder = QueueConfig::builder().event_queue_size(1); // Too small

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_cache_config_builder_validation_at_build() {
    // Test that validation happens at build time
    let builder = CacheConfig::builder().tlb_cache_size(1); // Too small

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_address_config_builder_validation_at_build() {
    // Test that validation happens at build time
    let builder = AddressConfig::builder().max_iova_bits(1); // Too small

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_resource_limits_builder_validation_at_build() {
    // Test that validation happens at build time
    let builder = ResourceLimits::builder().max_memory_usage(1); // Too small

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_smmu_config_builder_with_invalid_subconfigs() {
    // Create invalid sub-configs that will fail validation
    let mut invalid_queue = QueueConfig::default();
    invalid_queue.event_queue_size = 1; // Invalid

    let builder = SMMUConfig::builder().queue_config(invalid_queue);

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_smmu_config_merge_validates_other() {
    let mut base = SMMUConfig::default();

    // Create a config that's initially valid but we'll invalidate it
    let mut other = SMMUConfig::default();
    other.queue_config.event_queue_size = 1; // Invalid

    let result = base.merge(&other);
    assert!(result.is_err());
}

#[test]
fn test_queue_config_with_overflow_test_size_on_all_queues() {
    // Test that size 4 is specifically allowed for overflow testing
    let config = QueueConfig {
        event_queue_size: 4,
        command_queue_size: 4,
        pri_queue_size: 4,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_smmu_config_max_streams_conversion() {
    let config = SMMUConfig::default().with_max_streams(4096);
    assert_eq!(config.max_streams(), 4096);
    assert_eq!(config.address_config.max_stream_count, 4096);
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_ignores_malformed_lines() {
    // Test that lines without '=' are properly ignored
    let config_str = "event_queue_size=1024\n\
                      this line has no equals sign\n\
                      command_queue_size=512\n\
                      another_malformed_line\n\
                      pri_queue_size=256";

    let config = SMMUConfig::from_string(config_str).expect("valid config");

    // Only the well-formed lines should be parsed
    assert_eq!(config.queue_config.event_queue_size, 1024);
    assert_eq!(config.queue_config.command_queue_size, 512);
    assert_eq!(config.queue_config.pri_queue_size, 256);
}

#[test]
fn test_all_config_builders_have_correct_defaults() {
    // Verify all builders start with correct default values
    let stream_builder = StreamConfigBuilder::new();
    let queue_builder = QueueConfigBuilder::new();
    let cache_builder = CacheConfigBuilder::new();
    let address_builder = AddressConfigBuilder::new();
    let resource_builder = ResourceLimitsBuilder::new();
    let smmu_builder = SMMUConfigBuilder::new();

    // All default builders should produce valid configs
    assert!(stream_builder.build().is_ok());
    assert!(queue_builder.build().is_ok());
    assert!(cache_builder.build().is_ok());
    assert!(address_builder.build().is_ok());
    assert!(resource_builder.build().is_ok());
    assert!(smmu_builder.build().is_ok());
}

#[test]
fn test_validation_result_default_is_invalid() {
    // Default ValidationResult should be invalid with no errors
    let result = ValidationResult::default();
    assert!(!result.is_valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_stream_config_predicate_methods() {
    // Test is_bypass() on all config types
    assert!(StreamConfig::bypass().is_bypass());
    assert!(!StreamConfig::stage1_only().is_bypass());
    assert!(!StreamConfig::stage2_only().is_bypass());
    assert!(!StreamConfig::two_stage().is_bypass());

    // Test is_two_stage() on all config types
    assert!(!StreamConfig::bypass().is_two_stage());
    assert!(!StreamConfig::stage1_only().is_two_stage());
    assert!(!StreamConfig::stage2_only().is_two_stage());
    assert!(StreamConfig::two_stage().is_two_stage());
}

#[test]
fn test_configuration_error_type_copy_semantics() {
    let error_type = ConfigurationErrorType::InvalidQueueSize;
    let copied = error_type;
    // Both should be usable after copy
    assert_eq!(error_type, copied);
    assert_eq!(format!("{error_type}"), format!("{}", copied));
}

#[test]
fn test_fault_mode_debug_output() {
    // Verify Debug trait produces reasonable output
    let terminate_debug = format!("{:?}", FaultMode::Terminate);
    let stall_debug = format!("{:?}", FaultMode::Stall);
    assert!(terminate_debug.contains("Terminate"));
    assert!(stall_debug.contains("Stall"));
}

#[test]
fn test_stream_config_builder_preserves_values() {
    // Test that builder methods actually preserve values
    let builder = StreamConfig::builder().translation_enabled(true).stage1_enabled(true);

    // Build it and verify values are preserved
    let config = builder.build().expect("valid config");
    assert!(config.translation_enabled);
    assert!(config.stage1_enabled);
}

#[test]
fn test_smmu_config_validate_on_individual_components() {
    let config = SMMUConfig::default();

    // Validate each component individually
    assert!(config.queue_config.validate().is_ok());
    assert!(config.cache_config.validate().is_ok());
    assert!(config.address_config.validate().is_ok());
    assert!(config.resource_limits.validate().is_ok());
}

#[test]
fn test_resource_limits_memory_accessors_with_different_sizes() {
    // Test memory accessors with various sizes
    // bytes -> (mb, kb, gb)
    let test_cases = [
        (ResourceLimits::MIN_MEMORY_USAGE, 1, 1024, 0), // 1MB -> 1MB, 1024KB, 0GB
        (100 * 1024 * 1024, 100, 100 * 1024, 0),        // 100MB
        (1024 * 1024 * 1024, 1024, 1024 * 1024, 1),     // 1GB
        (5 * 1024 * 1024 * 1024, 5 * 1024, 5 * 1024 * 1024, 5), // 5GB
    ];

    for (bytes, expected_mb, expected_kb, expected_gb) in &test_cases {
        let limits = ResourceLimits::builder()
            .max_memory_usage(*bytes)
            .build()
            .expect("valid limits");

        assert_eq!(limits.max_memory_mb(), *expected_mb);
        assert_eq!(limits.max_memory_kb(), *expected_kb);
        assert_eq!(limits.max_memory_gb(), *expected_gb);
    }
}

#[test]
fn test_configuration_error_field_access() {
    let error = ConfigurationError::new(
        ConfigurationErrorType::InvalidQueueSize,
        "event_queue".to_string(),
        "size too small".to_string(),
    );

    // Access all fields
    assert_eq!(error.error_type, ConfigurationErrorType::InvalidQueueSize);
    assert_eq!(error.field, "event_queue");
    assert_eq!(error.message, "size too small");
}

#[test]
fn test_validation_result_field_access() {
    let mut result = ValidationResult::success();
    result.add_error("error1".to_string());
    result.add_warning("warning1".to_string());

    // Access all fields
    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.errors[0], "error1");
    assert_eq!(result.warnings[0], "warning1");
}

#[test]
fn test_queue_config_edge_case_sizes() {
    // Test queue sizes at various points in the valid range
    let test_sizes = [
        QueueConfig::MIN_QUEUE_SIZE,
        QueueConfig::MIN_QUEUE_SIZE + 1,
        128,
        256,
        512,
        1024,
        2048,
        QueueConfig::MAX_QUEUE_SIZE - 1,
        QueueConfig::MAX_QUEUE_SIZE,
    ];

    for size in &test_sizes {
        let config = QueueConfig::builder()
            .event_queue_size(*size)
            .command_queue_size(*size)
            .pri_queue_size(*size)
            .build()
            .unwrap_or_else(|_| panic!("valid config with size {size}"));

        assert_eq!(config.event_queue_size, *size);
    }
}

#[test]
fn test_cache_config_edge_case_sizes() {
    // Test cache sizes at various points in the valid range
    let test_sizes = [
        CacheConfig::MIN_CACHE_SIZE,
        CacheConfig::MIN_CACHE_SIZE + 1,
        256,
        1024,
        16_384,
        65_536,
        CacheConfig::MAX_CACHE_SIZE - 1,
        CacheConfig::MAX_CACHE_SIZE,
    ];

    for size in &test_sizes {
        let config = CacheConfig::builder()
            .tlb_cache_size(*size)
            .build()
            .unwrap_or_else(|_| panic!("valid config with size {size}"));

        assert_eq!(config.tlb_cache_size, *size);
    }
}

#[test]
fn test_smmu_config_validate_with_edge_case_values() {
    // Create a config with all minimum values
    let min_config = SMMUConfig {
        queue_config: QueueConfig {
            event_queue_size: QueueConfig::MIN_QUEUE_SIZE,
            command_queue_size: QueueConfig::MIN_QUEUE_SIZE,
            pri_queue_size: QueueConfig::MIN_QUEUE_SIZE,
        },
        cache_config: CacheConfig {
            tlb_cache_size: CacheConfig::MIN_CACHE_SIZE,
            cache_max_age_ms: CacheConfig::MIN_CACHE_AGE_MS,
            enable_caching: true,
        },
        address_config: AddressConfig {
            max_iova_bits: AddressConfig::MIN_IOVA_BITS,
            max_pa_bits: AddressConfig::MIN_PA_BITS,
            max_stream_count: AddressConfig::MIN_STREAM_COUNT,
            max_pasid_count: AddressConfig::MIN_PASID_COUNT,
        },
        resource_limits: ResourceLimits {
            max_memory_usage: ResourceLimits::MIN_MEMORY_USAGE,
            max_thread_count: ResourceLimits::MIN_THREAD_COUNT,
            timeout_ms: ResourceLimits::MIN_TIMEOUT_MS,
            enable_resource_tracking: true,
        },
    };

    assert!(min_config.validate().is_ok());

    // Create a config with all maximum values
    let max_config = SMMUConfig {
        queue_config: QueueConfig {
            event_queue_size: QueueConfig::MAX_QUEUE_SIZE,
            command_queue_size: QueueConfig::MAX_QUEUE_SIZE,
            pri_queue_size: QueueConfig::MAX_QUEUE_SIZE,
        },
        cache_config: CacheConfig {
            tlb_cache_size: CacheConfig::MAX_CACHE_SIZE,
            cache_max_age_ms: CacheConfig::MAX_CACHE_AGE_MS,
            enable_caching: true,
        },
        address_config: AddressConfig {
            max_iova_bits: AddressConfig::MAX_IOVA_BITS,
            max_pa_bits: AddressConfig::MAX_PA_BITS,
            max_stream_count: AddressConfig::MAX_STREAM_COUNT,
            max_pasid_count: AddressConfig::MAX_PASID_COUNT,
        },
        resource_limits: ResourceLimits {
            max_memory_usage: ResourceLimits::MAX_MEMORY_USAGE,
            max_thread_count: ResourceLimits::MAX_THREAD_COUNT,
            timeout_ms: ResourceLimits::MAX_TIMEOUT_MS,
            enable_resource_tracking: true,
        },
    };

    assert!(max_config.validate().is_ok());
}

#[test]
fn test_stream_config_validation_error_paths() {
    // Test all validation error paths in StreamConfig

    // Error: stages enabled without translation
    let config1 = StreamConfig {
        translation_enabled: false,
        stage1_enabled: true,
        stage2_enabled: false,
        pasid_enabled: false,
        max_pasid: 0,
        fault_mode: FaultMode::Terminate,
        security_enforced: false,
        vmid: 0,
        ha: false,
        hd: false,
        s1dss: 2,
        s1cd_max: 0,
        ..StreamConfig::bypass()
    };
    assert!(config1.validate().is_err());

    // Error: translation enabled but no stages
    let config2 = StreamConfig {
        translation_enabled: true,
        stage1_enabled: false,
        stage2_enabled: false,
        pasid_enabled: false,
        max_pasid: 0,
        fault_mode: FaultMode::Terminate,
        security_enforced: false,
        vmid: 0,
        ha: false,
        hd: false,
        s1dss: 2,
        s1cd_max: 0,
        ..StreamConfig::bypass()
    };
    assert!(config2.validate().is_err());

    // Error: PASID enabled without stage1
    let config3 = StreamConfig {
        translation_enabled: true,
        stage1_enabled: false,
        stage2_enabled: true,
        pasid_enabled: true,
        max_pasid: 100,
        fault_mode: FaultMode::Terminate,
        security_enforced: false,
        vmid: 0,
        ha: false,
        hd: false,
        s1dss: 2,
        s1cd_max: 0,
        ..StreamConfig::bypass()
    };
    assert!(config3.validate().is_err());

    // Error: max_pasid exceeds limit
    let config4 = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        pasid_enabled: true,
        max_pasid: StreamConfig::MAX_PASID + 1,
        fault_mode: FaultMode::Terminate,
        security_enforced: false,
        vmid: 0,
        ha: false,
        hd: false,
        s1dss: 2,
        s1cd_max: 0,
        ..StreamConfig::bypass()
    };
    assert!(config4.validate().is_err());

    // Error: max_pasid set without PASID enabled
    let config5 = StreamConfig {
        translation_enabled: true,
        stage1_enabled: true,
        stage2_enabled: false,
        pasid_enabled: false,
        max_pasid: 100,
        fault_mode: FaultMode::Terminate,
        security_enforced: false,
        vmid: 0,
        ha: false,
        hd: false,
        s1dss: 2,
        s1cd_max: 0,
        ..StreamConfig::bypass()
    };
    assert!(config5.validate().is_err());
}

#[test]
fn test_queue_config_validation_all_queues() {
    // Test event queue validation
    let mut config = QueueConfig::default();
    config.event_queue_size = 8; // Invalid (not overflow test size)
    assert!(config.validate().is_err());

    // Test command queue validation
    let mut config = QueueConfig::default();
    config.command_queue_size = 8; // Invalid
    assert!(config.validate().is_err());

    // Test PRI queue validation
    let mut config = QueueConfig::default();
    config.pri_queue_size = 8; // Invalid
    assert!(config.validate().is_err());

    // Test max boundaries
    let mut config = QueueConfig::default();
    config.event_queue_size = QueueConfig::MAX_QUEUE_SIZE + 1;
    assert!(config.validate().is_err());

    let mut config = QueueConfig::default();
    config.command_queue_size = QueueConfig::MAX_QUEUE_SIZE + 1;
    assert!(config.validate().is_err());

    let mut config = QueueConfig::default();
    config.pri_queue_size = QueueConfig::MAX_QUEUE_SIZE + 1;
    assert!(config.validate().is_err());
}

#[test]
fn test_cache_config_validation_all_fields() {
    // Test TLB cache size min
    let mut config = CacheConfig::default();
    config.tlb_cache_size = CacheConfig::MIN_CACHE_SIZE - 1;
    assert!(config.validate().is_err());

    // Test TLB cache size max
    let mut config = CacheConfig::default();
    config.tlb_cache_size = CacheConfig::MAX_CACHE_SIZE + 1;
    assert!(config.validate().is_err());

    // Test cache max age min
    let mut config = CacheConfig::default();
    config.cache_max_age_ms = CacheConfig::MIN_CACHE_AGE_MS - 1;
    assert!(config.validate().is_err());

    // Test cache max age max
    let mut config = CacheConfig::default();
    config.cache_max_age_ms = CacheConfig::MAX_CACHE_AGE_MS + 1;
    assert!(config.validate().is_err());
}

#[test]
fn test_address_config_validation_all_fields() {
    // Test IOVA bits min
    let mut config = AddressConfig::default();
    config.max_iova_bits = AddressConfig::MIN_IOVA_BITS - 1;
    assert!(config.validate().is_err());

    // Test IOVA bits max
    let mut config = AddressConfig::default();
    config.max_iova_bits = AddressConfig::MAX_IOVA_BITS + 1;
    assert!(config.validate().is_err());

    // Test PA bits min
    let mut config = AddressConfig::default();
    config.max_pa_bits = AddressConfig::MIN_PA_BITS - 1;
    assert!(config.validate().is_err());

    // Test PA bits max
    let mut config = AddressConfig::default();
    config.max_pa_bits = AddressConfig::MAX_PA_BITS + 1;
    assert!(config.validate().is_err());

    // Test stream count min
    let mut config = AddressConfig::default();
    config.max_stream_count = AddressConfig::MIN_STREAM_COUNT - 1;
    assert!(config.validate().is_err());

    // Test stream count max
    let mut config = AddressConfig::default();
    config.max_stream_count = AddressConfig::MAX_STREAM_COUNT + 1;
    assert!(config.validate().is_err());

    // Test PASID count min
    let mut config = AddressConfig::default();
    config.max_pasid_count = AddressConfig::MIN_PASID_COUNT - 1;
    assert!(config.validate().is_err());

    // Test PASID count max
    let mut config = AddressConfig::default();
    config.max_pasid_count = AddressConfig::MAX_PASID_COUNT + 1;
    assert!(config.validate().is_err());
}

#[test]
fn test_resource_limits_validation_all_fields() {
    // Test memory usage min
    let mut limits = ResourceLimits::default();
    limits.max_memory_usage = ResourceLimits::MIN_MEMORY_USAGE - 1;
    assert!(limits.validate().is_err());

    // Test memory usage max
    let mut limits = ResourceLimits::default();
    limits.max_memory_usage = ResourceLimits::MAX_MEMORY_USAGE + 1;
    assert!(limits.validate().is_err());

    // Test thread count min
    let mut limits = ResourceLimits::default();
    limits.max_thread_count = ResourceLimits::MIN_THREAD_COUNT - 1;
    assert!(limits.validate().is_err());

    // Test thread count max
    let mut limits = ResourceLimits::default();
    limits.max_thread_count = ResourceLimits::MAX_THREAD_COUNT + 1;
    assert!(limits.validate().is_err());

    // Test timeout min
    let mut limits = ResourceLimits::default();
    limits.timeout_ms = ResourceLimits::MIN_TIMEOUT_MS - 1;
    assert!(limits.validate().is_err());

    // Test timeout max
    let mut limits = ResourceLimits::default();
    limits.timeout_ms = ResourceLimits::MAX_TIMEOUT_MS + 1;
    assert!(limits.validate().is_err());
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_parsing_errors() {
    // Test all parsing error paths

    // Invalid event_queue_size
    let result = SMMUConfig::from_string("event_queue_size=abc");
    assert!(result.is_err());

    // Invalid command_queue_size
    let result = SMMUConfig::from_string("command_queue_size=xyz");
    assert!(result.is_err());

    // Invalid pri_queue_size
    let result = SMMUConfig::from_string("pri_queue_size=invalid");
    assert!(result.is_err());

    // Invalid tlb_cache_size
    let result = SMMUConfig::from_string("tlb_cache_size=notanumber");
    assert!(result.is_err());

    // Invalid cache_max_age_ms
    let result = SMMUConfig::from_string("cache_max_age_ms=bad");
    assert!(result.is_err());

    // Invalid enable_caching
    let result = SMMUConfig::from_string("enable_caching=notabool");
    assert!(result.is_err());

    // Invalid max_iova_bits
    let result = SMMUConfig::from_string("max_iova_bits=invalid");
    assert!(result.is_err());

    // Invalid max_pa_bits
    let result = SMMUConfig::from_string("max_pa_bits=bad");
    assert!(result.is_err());

    // Invalid max_stream_count
    let result = SMMUConfig::from_string("max_stream_count=xyz");
    assert!(result.is_err());

    // Invalid max_pasid_count
    let result = SMMUConfig::from_string("max_pasid_count=notvalid");
    assert!(result.is_err());

    // Invalid max_memory_usage
    let result = SMMUConfig::from_string("max_memory_usage=abc");
    assert!(result.is_err());

    // Invalid max_thread_count
    let result = SMMUConfig::from_string("max_thread_count=bad");
    assert!(result.is_err());

    // Invalid timeout_ms
    let result = SMMUConfig::from_string("timeout_ms=notanumber");
    assert!(result.is_err());

    // Invalid enable_resource_tracking
    let result = SMMUConfig::from_string("enable_resource_tracking=notabool");
    assert!(result.is_err());
}

#[test]
fn test_queue_config_overflow_exception_mixed() {
    // Test combinations with overflow test size (4) on some but not all queues
    let config1 = QueueConfig {
        event_queue_size: 4,
        command_queue_size: 512,
        pri_queue_size: 256,
    };
    assert!(config1.validate().is_ok());

    let config2 = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 4,
        pri_queue_size: 256,
    };
    assert!(config2.validate().is_ok());

    let config3 = QueueConfig {
        event_queue_size: 512,
        command_queue_size: 256,
        pri_queue_size: 4,
    };
    assert!(config3.validate().is_ok());
}

#[test]
fn test_smmu_config_validate_comprehensive() {
    // Test that SMMUConfig::validate() calls validate on all sub-configs
    let mut config = SMMUConfig::default();

    // Make queue config invalid
    config.queue_config.event_queue_size = 1;
    assert!(config.validate().is_err());

    // Reset and make cache config invalid
    config = SMMUConfig::default();
    config.cache_config.tlb_cache_size = 1;
    assert!(config.validate().is_err());

    // Reset and make address config invalid
    config = SMMUConfig::default();
    config.address_config.max_iova_bits = 1;
    assert!(config.validate().is_err());

    // Reset and make resource limits invalid
    config = SMMUConfig::default();
    config.resource_limits.max_memory_usage = 1;
    assert!(config.validate().is_err());
}

#[cfg(feature = "std")]
#[test]
fn test_smmu_config_from_string_final_validation_error() {
    // Test that from_string() validates the final config
    // This should parse successfully but fail validation
    let config_str = "event_queue_size=1"; // Too small
    let result = SMMUConfig::from_string(config_str);
    assert!(result.is_err());
}
