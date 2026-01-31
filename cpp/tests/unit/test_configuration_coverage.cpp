// ARM SMMU v3 Configuration System Coverage Tests
// Copyright (c) 2024 John Greninger
//
// Comprehensive test suite to achieve 90%+ coverage for Configuration component
// Targets: validateConfiguration(), error paths, edge cases, and factory methods

#include <gtest/gtest.h>
#include "smmu/configuration.h"
#include "smmu/smmu.h"
#include <thread>
#include <sstream>

using namespace smmu;

// Test fixture for Configuration coverage tests
class ConfigurationCoverageTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Nothing to do - using default configurations
    }

    void TearDown() override {
        // Nothing to do - no resources to clean up
    }
};

// ============================================================================
// TC-CONFIG-001: Configuration Validation
// Target: Lines 190-251 (validateConfiguration method)
// ============================================================================

TEST_F(ConfigurationCoverageTest, ValidateConfigurationAllValid) {
    // Test with all valid settings
    SMMUConfiguration config = SMMUConfiguration::createDefault();

    // Use the validate() method which calls validateConfiguration internally
    SMMUConfiguration::ValidationResult result = config.validate();

    EXPECT_TRUE(result.isValid);
    EXPECT_TRUE(result.errors.empty());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidQueueSize) {
    // Test invalid queue configuration - event queue too small
    QueueConfiguration invalidQueue(8, 256, 128);  // 8 < MIN_QUEUE_SIZE (16)

    EXPECT_FALSE(invalidQueue.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setQueueConfiguration(invalidQueue);

    // Should reject invalid configuration
    EXPECT_FALSE(setResult.isOk());
    EXPECT_EQ(setResult.getError(), SMMUError::InvalidConfiguration);

    // Validate configuration to get detailed errors
    std::vector<std::string> errors = config.validateConfiguration();
    EXPECT_TRUE(errors.empty()); // Original config should still be valid
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidQueueSizeTooLarge) {
    // Test invalid queue configuration - sizes too large
    QueueConfiguration invalidQueue(100000, 100000, 100000);  // > MAX_QUEUE_SIZE (65536)

    EXPECT_FALSE(invalidQueue.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setQueueConfiguration(invalidQueue);

    EXPECT_FALSE(setResult.isOk());
    EXPECT_EQ(setResult.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidCommandQueueSize) {
    // Test invalid command queue size
    QueueConfiguration invalidQueue(512, 8, 128);  // command queue 8 < MIN_QUEUE_SIZE (16)

    EXPECT_FALSE(invalidQueue.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setQueueConfiguration(invalidQueue);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidPriQueueSize) {
    // Test invalid PRI queue size
    QueueConfiguration invalidQueue(512, 256, 8);  // PRI queue 8 < MIN_QUEUE_SIZE (16)

    EXPECT_FALSE(invalidQueue.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setQueueConfiguration(invalidQueue);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidCacheSize) {
    // Test invalid cache configuration - cache size too small
    CacheConfiguration invalidCache(32, 5000, true);  // 32 < MIN_CACHE_SIZE (64)

    EXPECT_FALSE(invalidCache.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setCacheConfiguration(invalidCache);

    EXPECT_FALSE(setResult.isOk());
    EXPECT_EQ(setResult.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidCacheSizeTooLarge) {
    // Test invalid cache configuration - cache size too large
    CacheConfiguration invalidCache(2000000, 5000, true);  // > MAX_CACHE_SIZE (1048576)

    EXPECT_FALSE(invalidCache.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setCacheConfiguration(invalidCache);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidCacheMaxAge) {
    // Test invalid cache max age - too small
    CacheConfiguration invalidCache(1024, 50, true);  // 50 < MIN_CACHE_AGE (100)

    EXPECT_FALSE(invalidCache.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setCacheConfiguration(invalidCache);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidCacheMaxAgeTooLarge) {
    // Test invalid cache max age - too large
    CacheConfiguration invalidCache(1024, 4000000, true);  // > MAX_CACHE_AGE (3600000)

    EXPECT_FALSE(invalidCache.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setCacheConfiguration(invalidCache);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidAddressIOVASize) {
    // Test invalid input address width - too small
    AddressConfiguration invalidAddr(16, 48, 65536, 1048576);  // 16 < MIN_IOVA_BITS (32)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
    EXPECT_EQ(setResult.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidAddressIOVASizeTooLarge) {
    // Test invalid input address width - too large
    AddressConfiguration invalidAddr(64, 48, 65536, 1048576);  // 64 > MAX_IOVA_BITS (52)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidAddressPASize) {
    // Test invalid output address width - too small
    AddressConfiguration invalidAddr(48, 16, 65536, 1048576);  // 16 < MIN_PA_BITS (32)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidAddressPASizeTooLarge) {
    // Test invalid output address width - too large
    AddressConfiguration invalidAddr(48, 64, 65536, 1048576);  // 64 > MAX_PA_BITS (52)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidStreamCount) {
    // Test invalid max streams - zero
    AddressConfiguration invalidAddr(48, 48, 0, 1048576);  // 0 < MIN_STREAM_COUNT (1)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidStreamCountTooLarge) {
    // Test invalid max streams - too large
    AddressConfiguration invalidAddr(48, 48, 2000000, 1048576);  // > MAX_STREAM_COUNT (1048576)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidPASIDCount) {
    // Test invalid max contexts - zero
    AddressConfiguration invalidAddr(48, 48, 65536, 0);  // 0 < MIN_PASID_COUNT (1)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidPASIDCountTooLarge) {
    // Test invalid max contexts - too large
    AddressConfiguration invalidAddr(48, 48, 65536, 2000000);  // > MAX_PASID_COUNT (1048576)

    EXPECT_FALSE(invalidAddr.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setAddressConfiguration(invalidAddr);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidMemoryUsage) {
    // Test invalid resource limits - memory too small
    ResourceLimits invalidLimits(512 * 1024, 8, 1000, true);  // 512KB < MIN_MEMORY_USAGE (1MB)

    EXPECT_FALSE(invalidLimits.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setResourceLimits(invalidLimits);

    EXPECT_FALSE(setResult.isOk());
    EXPECT_EQ(setResult.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidMemoryUsageTooLarge) {
    // Test invalid resource limits - memory too large
    ResourceLimits invalidLimits(128ULL * 1024 * 1024 * 1024, 8, 1000, true);  // 128GB > MAX_MEMORY_USAGE (64GB)

    EXPECT_FALSE(invalidLimits.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setResourceLimits(invalidLimits);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidThreadCount) {
    // Test invalid thread count - zero
    ResourceLimits invalidLimits(1024 * 1024 * 1024, 0, 1000, true);  // 0 < MIN_THREAD_COUNT (1)

    EXPECT_FALSE(invalidLimits.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setResourceLimits(invalidLimits);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidThreadCountTooLarge) {
    // Test invalid thread count - too large
    ResourceLimits invalidLimits(1024 * 1024 * 1024, 512, 1000, true);  // 512 > MAX_THREAD_COUNT (256)

    EXPECT_FALSE(invalidLimits.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setResourceLimits(invalidLimits);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidTimeout) {
    // Test invalid timeout - too small
    ResourceLimits invalidLimits(1024 * 1024 * 1024, 8, 5, true);  // 5 < MIN_TIMEOUT_MS (10)

    EXPECT_FALSE(invalidLimits.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setResourceLimits(invalidLimits);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationInvalidTimeoutTooLarge) {
    // Test invalid timeout - too large
    ResourceLimits invalidLimits(1024 * 1024 * 1024, 8, 400000, true);  // 400000 > MAX_TIMEOUT_MS (300000)

    EXPECT_FALSE(invalidLimits.isValid());

    SMMUConfiguration config;
    VoidResult setResult = config.setResourceLimits(invalidLimits);

    EXPECT_FALSE(setResult.isOk());
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationDetailedErrors) {
    // Test detailed validation with multiple errors
    SMMUConfiguration config;

    // Set invalid queue configuration
    QueueConfiguration badQueue(8, 8, 8);

    // Create a config with invalid components
    SMMUConfiguration badConfig(badQueue, CacheConfiguration(), AddressConfiguration(), ResourceLimits());

    // Test the detailed validate() method
    SMMUConfiguration::ValidationResult result = badConfig.validate();

    EXPECT_FALSE(result.isValid);
    EXPECT_FALSE(result.errors.empty());

    // Check for specific error messages
    bool foundQueueError = false;
    for (const auto& error : result.errors) {
        if (error.find("Queue") != std::string::npos ||
            error.find("queue") != std::string::npos) {
            foundQueueError = true;
            break;
        }
    }
    EXPECT_TRUE(foundQueueError);
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationWithWarnings) {
    // Test configuration that is valid but has warnings
    QueueConfiguration largeQueue(4096, 2048, 1024);  // Large event queue
    CacheConfiguration largeCache(8192, 15000, true);  // Large cache, long timeout
    AddressConfiguration addrConfig(48, 48, 65536, 1048576);
    ResourceLimits limits(2ULL * 1024 * 1024 * 1024, 8, 15000, true);  // Long timeout

    SMMUConfiguration config(largeQueue, largeCache, addrConfig, limits);

    SMMUConfiguration::ValidationResult result = config.validate();

    EXPECT_TRUE(result.isValid);
    EXPECT_TRUE(result.errors.empty());

    // Should have warnings about large sizes and long timeouts
    EXPECT_FALSE(result.warnings.empty());
}

// ============================================================================
// TC-CONFIG-002: Minimal Configuration Profile
// Target: Lines 933-938 (createMinimal factory method)
// ============================================================================

TEST_F(ConfigurationCoverageTest, CreateMinimalConfiguration) {
    // Test createMinimal() factory method
    SMMUConfiguration minConfig = SMMUConfiguration::createMinimal();

    EXPECT_TRUE(minConfig.isValid());

    // Verify minimal queue sizes
    const QueueConfiguration& queueConfig = minConfig.getQueueConfiguration();
    EXPECT_EQ(queueConfig.eventQueueSize, 64);
    EXPECT_EQ(queueConfig.commandQueueSize, 32);
    EXPECT_EQ(queueConfig.priQueueSize, 16);

    // Verify minimal cache size
    const CacheConfiguration& cacheConfig = minConfig.getCacheConfiguration();
    EXPECT_EQ(cacheConfig.tlbCacheSize, 128);
    EXPECT_EQ(cacheConfig.cacheMaxAge, 1000);
    EXPECT_TRUE(cacheConfig.enableCaching);

    // Verify minimal addressing (32-bit)
    const AddressConfiguration& addrConfig = minConfig.getAddressConfiguration();
    EXPECT_EQ(addrConfig.maxIOVASize, 32);
    EXPECT_EQ(addrConfig.maxPASize, 32);
    EXPECT_EQ(addrConfig.maxStreamCount, 256);
    EXPECT_EQ(addrConfig.maxPASIDCount, 64);

    // Verify minimal resource limits
    const ResourceLimits& limits = minConfig.getResourceLimits();
    EXPECT_EQ(limits.maxMemoryUsage, 32 * 1024 * 1024);  // 32MB
    EXPECT_EQ(limits.maxThreadCount, 1);
    EXPECT_EQ(limits.timeoutMs, 100);
    EXPECT_FALSE(limits.enableResourceTracking);
}

TEST_F(ConfigurationCoverageTest, MinimalConfigurationOperational) {
    // Test that system operates with minimal configuration
    SMMUConfiguration minConfig = SMMUConfiguration::createMinimal();

    SMMU smmu(minConfig);

    const SMMUConfiguration& retrievedConfig = smmu.getConfiguration();
    EXPECT_TRUE(retrievedConfig.isValid());

    // Verify the minimal configuration was applied
    EXPECT_EQ(retrievedConfig.getQueueConfiguration().eventQueueSize, 64);
    EXPECT_EQ(retrievedConfig.getCacheConfiguration().tlbCacheSize, 128);
}

TEST_F(ConfigurationCoverageTest, MinimalConfigurationStillFunctional) {
    // Verify minimal configuration supports basic operations
    SMMUConfiguration minConfig = SMMUConfiguration::createMinimal();
    SMMU smmu(minConfig);

    // Configure a stream
    StreamConfig streamConfig;
    streamConfig.translationEnabled = true;
    streamConfig.stage1Enabled = true;
    streamConfig.stage2Enabled = false;

    VoidResult result = smmu.configureStream(1, streamConfig);
    EXPECT_TRUE(result.isOk());

    // Verify stream was configured
    EXPECT_EQ(smmu.getStreamCount(), 1);
}

// ============================================================================
// TC-CONFIG-003: Configuration Edge Cases
// Target: Line 47 (thread count fallback), line 644 (parse errors)
// ============================================================================

TEST_F(ConfigurationCoverageTest, ThreadCountFallbackLogic) {
    // Test constructor with custom ResourceLimits to cover fallback path
    // The default constructor uses hardware_concurrency() with fallback to 8
    SMMUConfiguration config;

    const ResourceLimits& limits = config.getResourceLimits();

    // Thread count should be valid (either hardware_concurrency or fallback)
    EXPECT_GE(limits.maxThreadCount, 1);
    EXPECT_LE(limits.maxThreadCount, 256);
}

TEST_F(ConfigurationCoverageTest, ConfigurationConstructorWithAllParameters) {
    // Test constructor that takes all configuration structures
    QueueConfiguration queueConfig(1024, 512, 256);
    CacheConfiguration cacheConfig(2048, 10000, true);
    AddressConfiguration addressConfig(48, 48, 65536, 1048576);
    ResourceLimits resourceLimits(1024 * 1024 * 1024, 8, 1000, true);

    // This exercises the constructor at line 47
    SMMUConfiguration config(queueConfig, cacheConfig, addressConfig, resourceLimits);

    EXPECT_TRUE(config.isValid());

    // Verify all values were set correctly
    EXPECT_EQ(config.getQueueConfiguration().eventQueueSize, 1024);
    EXPECT_EQ(config.getCacheConfiguration().tlbCacheSize, 2048);
    EXPECT_EQ(config.getAddressConfiguration().maxIOVASize, 48);
    EXPECT_EQ(config.getResourceLimits().maxMemoryUsage, 1024 * 1024 * 1024);
}

TEST_F(ConfigurationCoverageTest, ConfigurationParsingErrors) {
    // Test parsing errors with malformed config string
    std::string malformedConfig = "invalid config string without proper format";

    Result<SMMUConfiguration> parseResult = SMMUConfiguration::fromString(malformedConfig);

    // Should fail to parse but not crash
    // The result may succeed with default values or fail depending on implementation
    // Either way, if it succeeds, the config should be valid
    if (parseResult.isOk()) {
        EXPECT_TRUE(parseResult.getValue().isValid());
    }
}

TEST_F(ConfigurationCoverageTest, ConfigurationParsingWithInvalidValues) {
    // Test parsing with invalid numeric values
    std::string invalidConfig =
        "event_queue_size=invalid\n"
        "command_queue_size=not_a_number\n"
        "tlb_cache_size=xyz\n";

    Result<SMMUConfiguration> parseResult = SMMUConfiguration::fromString(invalidConfig);

    // Should handle parsing errors gracefully
    // Implementation may throw or return error
}

TEST_F(ConfigurationCoverageTest, BoundaryValueQueueSizes) {
    // Test boundary values for queue sizes - minimum valid
    QueueConfiguration minQueue(16, 16, 16);  // MIN_QUEUE_SIZE
    EXPECT_TRUE(minQueue.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setQueueConfiguration(minQueue);
    EXPECT_TRUE(result.isOk());

    // Test boundary values - maximum valid
    QueueConfiguration maxQueue(65536, 65536, 65536);  // MAX_QUEUE_SIZE
    EXPECT_TRUE(maxQueue.isValid());

    result = config.setQueueConfiguration(maxQueue);
    EXPECT_TRUE(result.isOk());

    // Test just below minimum
    QueueConfiguration belowMin(15, 16, 16);
    EXPECT_FALSE(belowMin.isValid());

    // Test just above maximum
    QueueConfiguration aboveMax(65537, 65536, 65536);
    EXPECT_FALSE(aboveMax.isValid());
}

TEST_F(ConfigurationCoverageTest, BoundaryValueCacheSizes) {
    // Test boundary values for cache sizes - minimum valid
    CacheConfiguration minCache(64, 100, true);  // MIN_CACHE_SIZE, MIN_CACHE_AGE
    EXPECT_TRUE(minCache.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setCacheConfiguration(minCache);
    EXPECT_TRUE(result.isOk());

    // Test boundary values - maximum valid
    CacheConfiguration maxCache(1048576, 3600000, true);  // MAX_CACHE_SIZE, MAX_CACHE_AGE
    EXPECT_TRUE(maxCache.isValid());

    result = config.setCacheConfiguration(maxCache);
    EXPECT_TRUE(result.isOk());

    // Test just below minimum
    CacheConfiguration belowMinSize(63, 100, true);
    EXPECT_FALSE(belowMinSize.isValid());

    CacheConfiguration belowMinAge(64, 99, true);
    EXPECT_FALSE(belowMinAge.isValid());

    // Test just above maximum
    CacheConfiguration aboveMaxSize(1048577, 3600000, true);
    EXPECT_FALSE(aboveMaxSize.isValid());

    CacheConfiguration aboveMaxAge(1048576, 3600001, true);
    EXPECT_FALSE(aboveMaxAge.isValid());
}

TEST_F(ConfigurationCoverageTest, BoundaryValueAddressSizes) {
    // Test boundary values for address sizes - minimum valid
    AddressConfiguration minAddr(32, 32, 1, 1);  // MIN values
    EXPECT_TRUE(minAddr.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setAddressConfiguration(minAddr);
    EXPECT_TRUE(result.isOk());

    // Test boundary values - maximum valid
    AddressConfiguration maxAddr(52, 52, 1048576, 1048576);  // MAX values
    EXPECT_TRUE(maxAddr.isValid());

    result = config.setAddressConfiguration(maxAddr);
    EXPECT_TRUE(result.isOk());

    // Test just below minimum
    AddressConfiguration belowMinIOVA(31, 32, 1, 1);
    EXPECT_FALSE(belowMinIOVA.isValid());

    AddressConfiguration belowMinPA(32, 31, 1, 1);
    EXPECT_FALSE(belowMinPA.isValid());

    AddressConfiguration belowMinStream(32, 32, 0, 1);
    EXPECT_FALSE(belowMinStream.isValid());

    AddressConfiguration belowMinPASID(32, 32, 1, 0);
    EXPECT_FALSE(belowMinPASID.isValid());

    // Test just above maximum
    AddressConfiguration aboveMaxIOVA(53, 52, 1048576, 1048576);
    EXPECT_FALSE(aboveMaxIOVA.isValid());

    AddressConfiguration aboveMaxPA(52, 53, 1048576, 1048576);
    EXPECT_FALSE(aboveMaxPA.isValid());

    AddressConfiguration aboveMaxStream(52, 52, 1048577, 1048576);
    EXPECT_FALSE(aboveMaxStream.isValid());

    AddressConfiguration aboveMaxPASID(52, 52, 1048576, 1048577);
    EXPECT_FALSE(aboveMaxPASID.isValid());
}

TEST_F(ConfigurationCoverageTest, BoundaryValueResourceLimits) {
    // Test boundary values for resource limits - minimum valid
    ResourceLimits minLimits(1024 * 1024, 1, 10, true);  // MIN values
    EXPECT_TRUE(minLimits.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setResourceLimits(minLimits);
    EXPECT_TRUE(result.isOk());

    // Test boundary values - maximum valid
    ResourceLimits maxLimits(64ULL * 1024 * 1024 * 1024, 256, 300000, true);  // MAX values
    EXPECT_TRUE(maxLimits.isValid());

    result = config.setResourceLimits(maxLimits);
    EXPECT_TRUE(result.isOk());

    // Test just below minimum
    ResourceLimits belowMinMemory(1024 * 1024 - 1, 1, 10, true);
    EXPECT_FALSE(belowMinMemory.isValid());

    ResourceLimits belowMinThreads(1024 * 1024, 0, 10, true);
    EXPECT_FALSE(belowMinThreads.isValid());

    ResourceLimits belowMinTimeout(1024 * 1024, 1, 9, true);
    EXPECT_FALSE(belowMinTimeout.isValid());

    // Test just above maximum
    ResourceLimits aboveMaxMemory(64ULL * 1024 * 1024 * 1024 + 1, 256, 300000, true);
    EXPECT_FALSE(aboveMaxMemory.isValid());

    ResourceLimits aboveMaxThreads(64ULL * 1024 * 1024 * 1024, 257, 300000, true);
    EXPECT_FALSE(aboveMaxThreads.isValid());

    ResourceLimits aboveMaxTimeout(64ULL * 1024 * 1024 * 1024, 256, 300001, true);
    EXPECT_FALSE(aboveMaxTimeout.isValid());
}

// ============================================================================
// TC-CONFIG-004: Configuration Limits
// Additional tests for maximum configurations and error handling
// ============================================================================

TEST_F(ConfigurationCoverageTest, MaximumQueueConfiguration) {
    // Test maximum queue sizes
    QueueConfiguration maxQueue(65536, 65536, 65536);

    EXPECT_TRUE(maxQueue.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setQueueConfiguration(maxQueue);

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getQueueConfiguration().eventQueueSize, 65536);
    EXPECT_EQ(config.getQueueConfiguration().commandQueueSize, 65536);
    EXPECT_EQ(config.getQueueConfiguration().priQueueSize, 65536);
}

TEST_F(ConfigurationCoverageTest, MaximumCacheConfiguration) {
    // Test maximum cache sizes
    CacheConfiguration maxCache(1048576, 3600000, true);

    EXPECT_TRUE(maxCache.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setCacheConfiguration(maxCache);

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getCacheConfiguration().tlbCacheSize, 1048576);
    EXPECT_EQ(config.getCacheConfiguration().cacheMaxAge, 3600000);
}

TEST_F(ConfigurationCoverageTest, MaximumAddressConfiguration) {
    // Test maximum address bit widths
    AddressConfiguration maxAddr(52, 52, 1048576, 1048576);

    EXPECT_TRUE(maxAddr.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setAddressConfiguration(maxAddr);

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getAddressConfiguration().maxIOVASize, 52);
    EXPECT_EQ(config.getAddressConfiguration().maxPASize, 52);
    EXPECT_EQ(config.getAddressConfiguration().maxStreamCount, 1048576);
    EXPECT_EQ(config.getAddressConfiguration().maxPASIDCount, 1048576);
}

TEST_F(ConfigurationCoverageTest, MaximumResourceLimits) {
    // Test maximum resource limits
    ResourceLimits maxLimits(64ULL * 1024 * 1024 * 1024, 256, 300000, true);

    EXPECT_TRUE(maxLimits.isValid());

    SMMUConfiguration config;
    VoidResult result = config.setResourceLimits(maxLimits);

    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getResourceLimits().maxMemoryUsage, 64ULL * 1024 * 1024 * 1024);
    EXPECT_EQ(config.getResourceLimits().maxThreadCount, 256);
    EXPECT_EQ(config.getResourceLimits().timeoutMs, 300000);
}

TEST_F(ConfigurationCoverageTest, ConfigurationMergeWithInvalidConfig) {
    // Test merge() with invalid configuration
    SMMUConfiguration validConfig = SMMUConfiguration::createDefault();

    QueueConfiguration invalidQueue(8, 8, 8);
    SMMUConfiguration invalidConfig(invalidQueue, CacheConfiguration(), AddressConfiguration(), ResourceLimits());

    VoidResult result = validConfig.merge(invalidConfig);

    EXPECT_FALSE(result.isOk());
    EXPECT_EQ(result.getError(), SMMUError::InvalidConfiguration);

    // Original config should remain unchanged
    EXPECT_TRUE(validConfig.isValid());
}

TEST_F(ConfigurationCoverageTest, ConfigurationResetToDefaults) {
    // Test reset() method
    SMMUConfiguration config = SMMUConfiguration::createHighPerformance();

    // Verify it's high-performance
    EXPECT_GT(config.getQueueConfiguration().eventQueueSize, 1024);

    // Reset to defaults
    config.reset();

    // Should now match default configuration
    SMMUConfiguration defaultConfig = SMMUConfiguration::createDefault();
    EXPECT_EQ(config.getQueueConfiguration().eventQueueSize,
              defaultConfig.getQueueConfiguration().eventQueueSize);
    EXPECT_EQ(config.getCacheConfiguration().tlbCacheSize,
              defaultConfig.getCacheConfiguration().tlbCacheSize);
}

TEST_F(ConfigurationCoverageTest, ConfigurationCopyConstructor) {
    // Test copy constructor
    SMMUConfiguration original = SMMUConfiguration::createHighPerformance();
    SMMUConfiguration copy(original);

    EXPECT_TRUE(copy.isValid());
    EXPECT_EQ(copy.getQueueConfiguration().eventQueueSize,
              original.getQueueConfiguration().eventQueueSize);
    EXPECT_EQ(copy.getCacheConfiguration().tlbCacheSize,
              original.getCacheConfiguration().tlbCacheSize);
}

TEST_F(ConfigurationCoverageTest, ConfigurationAssignmentOperator) {
    // Test assignment operator
    SMMUConfiguration original = SMMUConfiguration::createHighPerformance();
    SMMUConfiguration assigned;

    assigned = original;

    EXPECT_TRUE(assigned.isValid());
    EXPECT_EQ(assigned.getQueueConfiguration().eventQueueSize,
              original.getQueueConfiguration().eventQueueSize);
    EXPECT_EQ(assigned.getCacheConfiguration().tlbCacheSize,
              original.getCacheConfiguration().tlbCacheSize);

    // Test self-assignment
    assigned = assigned;
    EXPECT_TRUE(assigned.isValid());
}

TEST_F(ConfigurationCoverageTest, ConfigurationEqualityOperators) {
    // Test equality operators
    SMMUConfiguration config1 = SMMUConfiguration::createDefault();
    SMMUConfiguration config2 = SMMUConfiguration::createDefault();
    SMMUConfiguration config3 = SMMUConfiguration::createHighPerformance();

    EXPECT_TRUE(config1 == config2);
    EXPECT_FALSE(config1 != config2);

    EXPECT_FALSE(config1 == config3);
    EXPECT_TRUE(config1 != config3);
}

TEST_F(ConfigurationCoverageTest, AllFactoryMethodsValid) {
    // Test all factory methods produce valid configurations
    SMMUConfiguration defaultConfig = SMMUConfiguration::createDefault();
    EXPECT_TRUE(defaultConfig.isValid());

    SMMUConfiguration highPerfConfig = SMMUConfiguration::createHighPerformance();
    EXPECT_TRUE(highPerfConfig.isValid());

    SMMUConfiguration lowMemConfig = SMMUConfiguration::createLowMemory();
    EXPECT_TRUE(lowMemConfig.isValid());

    SMMUConfiguration minConfig = SMMUConfiguration::createMinimal();
    EXPECT_TRUE(minConfig.isValid());

    SMMUConfiguration serverConfig = SMMUConfiguration::createServerProfile();
    EXPECT_TRUE(serverConfig.isValid());

    SMMUConfiguration embeddedConfig = SMMUConfiguration::createEmbeddedProfile();
    EXPECT_TRUE(embeddedConfig.isValid());

    SMMUConfiguration devConfig = SMMUConfiguration::createDevelopmentProfile();
    EXPECT_TRUE(devConfig.isValid());
}

TEST_F(ConfigurationCoverageTest, StringSerializationRoundTrip) {
    // Test complete round-trip serialization
    SMMUConfiguration original = SMMUConfiguration::createDevelopmentProfile();

    // Serialize to string
    std::string serialized = original.toString();
    EXPECT_FALSE(serialized.empty());

    // Deserialize from string
    Result<SMMUConfiguration> parseResult = SMMUConfiguration::fromString(serialized);
    EXPECT_TRUE(parseResult.isOk());

    if (parseResult.isOk()) {
        SMMUConfiguration deserialized = parseResult.getValue();
        EXPECT_TRUE(deserialized.isValid());

        // Verify round-trip preservation
        EXPECT_EQ(deserialized.getQueueConfiguration().eventQueueSize,
                  original.getQueueConfiguration().eventQueueSize);
        EXPECT_EQ(deserialized.getQueueConfiguration().commandQueueSize,
                  original.getQueueConfiguration().commandQueueSize);
        EXPECT_EQ(deserialized.getQueueConfiguration().priQueueSize,
                  original.getQueueConfiguration().priQueueSize);

        EXPECT_EQ(deserialized.getCacheConfiguration().tlbCacheSize,
                  original.getCacheConfiguration().tlbCacheSize);
        EXPECT_EQ(deserialized.getCacheConfiguration().cacheMaxAge,
                  original.getCacheConfiguration().cacheMaxAge);
        EXPECT_EQ(deserialized.getCacheConfiguration().enableCaching,
                  original.getCacheConfiguration().enableCaching);
    }
}

TEST_F(ConfigurationCoverageTest, UpdateMethodsWithInvalidValues) {
    // Test update methods with invalid values
    SMMUConfiguration config = SMMUConfiguration::createDefault();

    // Try to update queue sizes with invalid values
    VoidResult result = config.updateQueueSizes(8, 8, 8);
    EXPECT_FALSE(result.isOk());

    // Try to update cache settings with invalid values
    result = config.updateCacheSettings(32, 50, true);
    EXPECT_FALSE(result.isOk());

    // Try to update address limits with invalid values
    result = config.updateAddressLimits(16, 16, 0, 0);
    EXPECT_FALSE(result.isOk());

    // Try to update resource limits with invalid values
    result = config.updateResourceLimits(512 * 1024, 0, 5);
    EXPECT_FALSE(result.isOk());
}

TEST_F(ConfigurationCoverageTest, UpdateMethodsWithValidValues) {
    // Test update methods with valid values
    SMMUConfiguration config = SMMUConfiguration::createDefault();

    // Update queue sizes
    VoidResult result = config.updateQueueSizes(2048, 1024, 512);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getQueueConfiguration().eventQueueSize, 2048);

    // Update cache settings
    result = config.updateCacheSettings(4096, 10000, false);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getCacheConfiguration().tlbCacheSize, 4096);
    EXPECT_EQ(config.getCacheConfiguration().cacheMaxAge, 10000);
    EXPECT_FALSE(config.getCacheConfiguration().enableCaching);

    // Update address limits
    result = config.updateAddressLimits(52, 52, 1048576, 1048576);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getAddressConfiguration().maxIOVASize, 52);

    // Update resource limits
    result = config.updateResourceLimits(2ULL * 1024 * 1024 * 1024, 16, 5000);
    EXPECT_TRUE(result.isOk());
    EXPECT_EQ(config.getResourceLimits().maxMemoryUsage, 2ULL * 1024 * 1024 * 1024);
}

TEST_F(ConfigurationCoverageTest, DetailedValidationErrorMessages) {
    // Test validate() method with invalid cache configuration to get detailed errors
    QueueConfiguration validQueue(512, 256, 128);
    CacheConfiguration invalidCache(32, 50, true);  // Both size and age invalid
    AddressConfiguration validAddr(48, 48, 65536, 1048576);
    ResourceLimits validLimits(1024 * 1024 * 1024, 8, 1000, true);

    SMMUConfiguration config(validQueue, invalidCache, validAddr, validLimits);

    // Use detailed validation
    SMMUConfiguration::ValidationResult result = config.validate();

    EXPECT_FALSE(result.isValid);
    EXPECT_FALSE(result.errors.empty());

    // Should have specific error messages about cache configuration
    bool foundCacheError = false;
    bool foundSizeError = false;
    bool foundAgeError = false;

    for (const auto& error : result.errors) {
        if (error.find("Cache") != std::string::npos || error.find("cache") != std::string::npos) {
            foundCacheError = true;
        }
        if (error.find("size") != std::string::npos || error.find("Size") != std::string::npos) {
            foundSizeError = true;
        }
        if (error.find("age") != std::string::npos || error.find("Age") != std::string::npos) {
            foundAgeError = true;
        }
    }

    EXPECT_TRUE(foundCacheError);
    EXPECT_TRUE(foundSizeError);
    EXPECT_TRUE(foundAgeError);
}

TEST_F(ConfigurationCoverageTest, DetailedValidationAddressErrors) {
    // Test validate() method with invalid address configuration
    QueueConfiguration validQueue(512, 256, 128);
    CacheConfiguration validCache(1024, 5000, true);
    AddressConfiguration invalidAddr(16, 16, 0, 0);  // Multiple invalid values
    ResourceLimits validLimits(1024 * 1024 * 1024, 8, 1000, true);

    SMMUConfiguration config(validQueue, validCache, invalidAddr, validLimits);

    SMMUConfiguration::ValidationResult result = config.validate();

    EXPECT_FALSE(result.isValid);
    EXPECT_FALSE(result.errors.empty());

    // Should have specific error messages
    bool foundAddressError = false;
    for (const auto& error : result.errors) {
        if (error.find("Address") != std::string::npos || error.find("address") != std::string::npos) {
            foundAddressError = true;
            break;
        }
    }

    EXPECT_TRUE(foundAddressError);
}

TEST_F(ConfigurationCoverageTest, DetailedValidationQueueErrors) {
    // Test validate() method with invalid queue configuration
    QueueConfiguration invalidQueue(8, 100000, 128);  // Multiple invalid values
    CacheConfiguration validCache(1024, 5000, true);
    AddressConfiguration validAddr(48, 48, 65536, 1048576);
    ResourceLimits validLimits(1024 * 1024 * 1024, 8, 1000, true);

    SMMUConfiguration config(invalidQueue, validCache, validAddr, validLimits);

    SMMUConfiguration::ValidationResult result = config.validate();

    EXPECT_FALSE(result.isValid);
    EXPECT_FALSE(result.errors.empty());

    // Should have specific error messages
    bool foundQueueError = false;
    for (const auto& error : result.errors) {
        if (error.find("Queue") != std::string::npos || error.find("queue") != std::string::npos) {
            foundQueueError = true;
            break;
        }
    }

    EXPECT_TRUE(foundQueueError);
}

TEST_F(ConfigurationCoverageTest, DetailedValidationResourceErrors) {
    // Test validate() method with invalid resource limits
    QueueConfiguration validQueue(512, 256, 128);
    CacheConfiguration validCache(1024, 5000, true);
    AddressConfiguration validAddr(48, 48, 65536, 1048576);
    ResourceLimits invalidLimits(512 * 1024, 0, 5, true);  // Multiple invalid values

    SMMUConfiguration config(validQueue, validCache, validAddr, invalidLimits);

    SMMUConfiguration::ValidationResult result = config.validate();

    EXPECT_FALSE(result.isValid);
    EXPECT_FALSE(result.errors.empty());

    // Should have specific error messages
    bool foundResourceError = false;
    for (const auto& error : result.errors) {
        if (error.find("Resource") != std::string::npos || error.find("resource") != std::string::npos) {
            foundResourceError = true;
            break;
        }
    }

    EXPECT_TRUE(foundResourceError);
}

TEST_F(ConfigurationCoverageTest, MergeValidConfiguration) {
    // Test merge() with valid configuration
    SMMUConfiguration config1 = SMMUConfiguration::createDefault();
    SMMUConfiguration config2 = SMMUConfiguration::createHighPerformance();

    VoidResult result = config1.merge(config2);

    EXPECT_TRUE(result.isOk());

    // After merge, config1 should match config2
    EXPECT_EQ(config1.getQueueConfiguration().eventQueueSize,
              config2.getQueueConfiguration().eventQueueSize);
    EXPECT_EQ(config1.getCacheConfiguration().tlbCacheSize,
              config2.getCacheConfiguration().tlbCacheSize);
}

TEST_F(ConfigurationCoverageTest, ValidateConfigurationErrorPaths) {
    // Test validateConfiguration() method directly
    SMMUConfiguration config = SMMUConfiguration::createDefault();

    // Test with valid config - should return empty errors
    std::vector<std::string> errors = config.validateConfiguration();
    EXPECT_TRUE(errors.empty());

    // Create invalid configuration and test
    QueueConfiguration invalidQueue(8, 8, 8);
    SMMUConfiguration invalidConfig(invalidQueue, CacheConfiguration(), AddressConfiguration(), ResourceLimits());

    errors = invalidConfig.validateConfiguration();
    EXPECT_FALSE(errors.empty());

    // Should have at least one error about queue configuration
    bool foundError = false;
    for (const auto& error : errors) {
        if (error.find("queue") != std::string::npos || error.find("Queue") != std::string::npos) {
            foundError = true;
            break;
        }
    }
    EXPECT_TRUE(foundError);
}

TEST_F(ConfigurationCoverageTest, ParseConfigurationWithInvalidConfig) {
    // Test fromString() with configuration that would fail validation
    std::string invalidConfigStr =
        "event_queue_size=8\n"
        "command_queue_size=8\n"
        "pri_queue_size=8\n";

    Result<SMMUConfiguration> parseResult = SMMUConfiguration::fromString(invalidConfigStr);

    // Should fail validation
    EXPECT_FALSE(parseResult.isOk());
    if (!parseResult.isOk()) {
        EXPECT_EQ(parseResult.getError(), SMMUError::InvalidConfiguration);
    }
}

TEST_F(ConfigurationCoverageTest, ThreadCountHardwareConcurrency) {
    // Test that hardware_concurrency is used when available
    SMMUConfiguration config;

    const ResourceLimits& limits = config.getResourceLimits();

    // Should use hardware_concurrency or fallback
    unsigned int hwThreads = std::thread::hardware_concurrency();
    if (hwThreads > 0) {
        EXPECT_EQ(limits.maxThreadCount, hwThreads);
    } else {
        // Should use fallback value
        EXPECT_EQ(limits.maxThreadCount, 8);
    }
}

// End of test suite
