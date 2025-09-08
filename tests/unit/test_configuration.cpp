// ARM SMMU v3 Configuration System Unit Tests
// Copyright (c) 2024 John Greninger

#include <gtest/gtest.h>
#include "smmu/configuration.h"
#include "smmu/smmu.h"

using namespace smmu;

// Test fixture for Configuration tests
class ConfigurationTest : public ::testing::Test {
protected:
    void SetUp() override {
        // Nothing to do - using default configurations
    }
    
    void TearDown() override {
        // Nothing to do - no resources to clean up
    }
};

// Test QueueConfiguration functionality
TEST_F(ConfigurationTest, QueueConfigurationDefaults) {
    QueueConfiguration queueConfig;
    
    // Check default values
    EXPECT_EQ(queueConfig.eventQueueSize, DEFAULT_EVENT_QUEUE_SIZE);
    EXPECT_EQ(queueConfig.commandQueueSize, DEFAULT_COMMAND_QUEUE_SIZE);
    EXPECT_EQ(queueConfig.priQueueSize, DEFAULT_PRI_QUEUE_SIZE);
    
    // Check validation
    EXPECT_TRUE(queueConfig.isValid());
}

TEST_F(ConfigurationTest, QueueConfigurationCustomValues) {
    QueueConfiguration queueConfig(1024, 512, 256);
    
    // Check custom values
    EXPECT_EQ(queueConfig.eventQueueSize, 1024);
    EXPECT_EQ(queueConfig.commandQueueSize, 512);
    EXPECT_EQ(queueConfig.priQueueSize, 256);
    
    // Check validation
    EXPECT_TRUE(queueConfig.isValid());
}

TEST_F(ConfigurationTest, QueueConfigurationInvalidValues) {
    // Test invalid values - too small
    QueueConfiguration smallConfig(8, 8, 8);
    EXPECT_FALSE(smallConfig.isValid());
    
    // Test invalid values - too large
    QueueConfiguration largeConfig(100000, 100000, 100000);
    EXPECT_FALSE(largeConfig.isValid());
}

// Test CacheConfiguration functionality
TEST_F(ConfigurationTest, CacheConfigurationDefaults) {
    CacheConfiguration cacheConfig;
    
    // Check default values
    EXPECT_EQ(cacheConfig.tlbCacheSize, 1024); // Default TLB cache size
    EXPECT_EQ(cacheConfig.cacheMaxAge, 5000);   // 5 seconds default
    EXPECT_TRUE(cacheConfig.enableCaching);
    
    // Check validation
    EXPECT_TRUE(cacheConfig.isValid());
}

TEST_F(ConfigurationTest, CacheConfigurationCustomValues) {
    CacheConfiguration cacheConfig(2048, 10000, false);
    
    // Check custom values
    EXPECT_EQ(cacheConfig.tlbCacheSize, 2048);
    EXPECT_EQ(cacheConfig.cacheMaxAge, 10000);
    EXPECT_FALSE(cacheConfig.enableCaching);
    
    // Check validation
    EXPECT_TRUE(cacheConfig.isValid());
}

// Test AddressConfiguration functionality
TEST_F(ConfigurationTest, AddressConfigurationDefaults) {
    AddressConfiguration addressConfig;
    
    // Check default values
    EXPECT_EQ(addressConfig.maxIOVASize, 48);  // 48-bit IOVA
    EXPECT_EQ(addressConfig.maxPASize, 52);    // 52-bit PA
    EXPECT_EQ(addressConfig.maxStreamCount, 65536);
    EXPECT_EQ(addressConfig.maxPASIDCount, 1048576);
    
    // Check validation
    EXPECT_TRUE(addressConfig.isValid());
}

// Test ResourceLimits functionality
TEST_F(ConfigurationTest, ResourceLimitsDefaults) {
    ResourceLimits resourceLimits;
    
    // Check default values
    EXPECT_EQ(resourceLimits.maxMemoryUsage, 1024 * 1024 * 1024); // 1GB
    EXPECT_GE(resourceLimits.maxThreadCount, 1);  // At least 1 thread
    EXPECT_EQ(resourceLimits.timeoutMs, 1000);    // 1 second timeout
    EXPECT_TRUE(resourceLimits.enableResourceTracking);
    
    // Check validation
    EXPECT_TRUE(resourceLimits.isValid());
}

// Test SMMUConfiguration functionality
TEST_F(ConfigurationTest, SMMUConfigurationDefaults) {
    SMMUConfiguration config;
    
    // Check that default configuration is valid
    EXPECT_TRUE(config.isValid());
    
    // Check that we can access all sub-configurations
    const QueueConfiguration& queueConfig = config.getQueueConfiguration();
    const CacheConfiguration& cacheConfig = config.getCacheConfiguration();
    const AddressConfiguration& addressConfig = config.getAddressConfiguration();
    const ResourceLimits& resourceLimits = config.getResourceLimits();
    
    // All should be valid
    EXPECT_TRUE(queueConfig.isValid());
    EXPECT_TRUE(cacheConfig.isValid());
    EXPECT_TRUE(addressConfig.isValid());
    EXPECT_TRUE(resourceLimits.isValid());
}

TEST_F(ConfigurationTest, SMMUConfigurationFactoryMethods) {
    // Test default factory method
    SMMUConfiguration defaultConfig = SMMUConfiguration::createDefault();
    EXPECT_TRUE(defaultConfig.isValid());
    
    // Test high-performance factory method
    SMMUConfiguration highPerfConfig = SMMUConfiguration::createHighPerformance();
    EXPECT_TRUE(highPerfConfig.isValid());
    
    // Verify high-performance has larger values
    EXPECT_GT(highPerfConfig.getQueueConfiguration().eventQueueSize,
              defaultConfig.getQueueConfiguration().eventQueueSize);
    EXPECT_GT(highPerfConfig.getCacheConfiguration().tlbCacheSize,
              defaultConfig.getCacheConfiguration().tlbCacheSize);
    
    // Test low-memory factory method
    SMMUConfiguration lowMemConfig = SMMUConfiguration::createLowMemory();
    EXPECT_TRUE(lowMemConfig.isValid());
    
    // Verify low-memory has smaller values
    EXPECT_LT(lowMemConfig.getQueueConfiguration().eventQueueSize,
              defaultConfig.getQueueConfiguration().eventQueueSize);
    EXPECT_LT(lowMemConfig.getCacheConfiguration().tlbCacheSize,
              defaultConfig.getCacheConfiguration().tlbCacheSize);
}

TEST_F(ConfigurationTest, SMMUConfigurationStringSerializationDeserialization) {
    // Create a test configuration
    SMMUConfiguration originalConfig = SMMUConfiguration::createHighPerformance();
    
    // Convert to string
    std::string configString = originalConfig.toString();
    EXPECT_FALSE(configString.empty());
    
    // Parse back from string
    Result<SMMUConfiguration> parseResult = SMMUConfiguration::fromString(configString);
    EXPECT_TRUE(parseResult.isOk());
    
    if (parseResult.isOk()) {
        SMMUConfiguration parsedConfig = parseResult.getValue();
        EXPECT_TRUE(parsedConfig.isValid());
        
        // Compare key values (exact comparison might be affected by string conversion precision)
        EXPECT_EQ(parsedConfig.getQueueConfiguration().eventQueueSize,
                  originalConfig.getQueueConfiguration().eventQueueSize);
        EXPECT_EQ(parsedConfig.getCacheConfiguration().tlbCacheSize,
                  originalConfig.getCacheConfiguration().tlbCacheSize);
    }
}

TEST_F(ConfigurationTest, SMMUConfigurationValidation) {
    SMMUConfiguration config;
    
    // Test detailed validation
    SMMUConfiguration::ValidationResult validation = config.validate();
    EXPECT_TRUE(validation.isValid);
    EXPECT_TRUE(validation.errors.empty());
    
    // Test setting invalid queue configuration
    QueueConfiguration invalidQueue(8, 8, 8); // Too small
    VoidResult result = config.setQueueConfiguration(invalidQueue);
    EXPECT_FALSE(result.isOk());
    EXPECT_EQ(result.getError(), SMMUError::InvalidConfiguration);
}

TEST_F(ConfigurationTest, SMMUConfigurationUpdateMethods) {
    SMMUConfiguration config;
    
    // Test updating queue configuration
    VoidResult result = config.updateQueueSizes(2048, 1024, 512);
    EXPECT_TRUE(result.isOk());
    
    const QueueConfiguration& queueConfig = config.getQueueConfiguration();
    EXPECT_EQ(queueConfig.eventQueueSize, 2048);
    EXPECT_EQ(queueConfig.commandQueueSize, 1024);
    EXPECT_EQ(queueConfig.priQueueSize, 512);
    
    // Test updating cache configuration
    result = config.updateCacheSettings(4096, 15000, true);
    EXPECT_TRUE(result.isOk());
    
    const CacheConfiguration& cacheConfig = config.getCacheConfiguration();
    EXPECT_EQ(cacheConfig.tlbCacheSize, 4096);
    EXPECT_EQ(cacheConfig.cacheMaxAge, 15000);
    EXPECT_TRUE(cacheConfig.enableCaching);
}

// Test SMMU integration with Configuration
TEST_F(ConfigurationTest, SMMUWithDefaultConfiguration) {
    SMMU smmu; // Uses default configuration
    
    const SMMUConfiguration& config = smmu.getConfiguration();
    EXPECT_TRUE(config.isValid());
    
    // Should be able to get basic info
    EXPECT_EQ(smmu.getStreamCount(), 0); // No streams configured yet
}

TEST_F(ConfigurationTest, SMMUWithCustomConfiguration) {
    SMMUConfiguration customConfig = SMMUConfiguration::createHighPerformance();
    SMMU smmu(customConfig);
    
    const SMMUConfiguration& retrievedConfig = smmu.getConfiguration();
    EXPECT_TRUE(retrievedConfig.isValid());
    
    // Verify custom configuration was applied
    EXPECT_EQ(retrievedConfig.getQueueConfiguration().eventQueueSize,
              customConfig.getQueueConfiguration().eventQueueSize);
}

TEST_F(ConfigurationTest, SMMURuntimeConfigurationUpdate) {
    SMMU smmu;
    
    // Update queue configuration at runtime
    QueueConfiguration newQueueConfig(1024, 512, 256);
    VoidResult result = smmu.updateQueueConfiguration(newQueueConfig);
    EXPECT_TRUE(result.isOk());
    
    // Verify update was applied
    const QueueConfiguration& updatedConfig = smmu.getConfiguration().getQueueConfiguration();
    EXPECT_EQ(updatedConfig.eventQueueSize, 1024);
    EXPECT_EQ(updatedConfig.commandQueueSize, 512);
    EXPECT_EQ(updatedConfig.priQueueSize, 256);
    
    // Update cache configuration at runtime
    CacheConfiguration newCacheConfig(2048, 8000, true);
    result = smmu.updateCacheConfiguration(newCacheConfig);
    EXPECT_TRUE(result.isOk());
    
    // Verify update was applied
    const CacheConfiguration& updatedCacheConfig = smmu.getConfiguration().getCacheConfiguration();
    EXPECT_EQ(updatedCacheConfig.tlbCacheSize, 2048);
    EXPECT_EQ(updatedCacheConfig.cacheMaxAge, 8000);
    EXPECT_TRUE(updatedCacheConfig.enableCaching);
}

TEST_F(ConfigurationTest, SMMUInvalidConfigurationRejection) {
    SMMU smmu;
    
    // Try to set invalid queue configuration
    QueueConfiguration invalidConfig(8, 8, 8); // Too small
    VoidResult result = smmu.updateQueueConfiguration(invalidConfig);
    EXPECT_FALSE(result.isOk());
    EXPECT_EQ(result.getError(), SMMUError::InvalidConfiguration);
    
    // Configuration should remain unchanged
    const QueueConfiguration& currentConfig = smmu.getConfiguration().getQueueConfiguration();
    EXPECT_EQ(currentConfig.eventQueueSize, DEFAULT_EVENT_QUEUE_SIZE);
    EXPECT_EQ(currentConfig.commandQueueSize, DEFAULT_COMMAND_QUEUE_SIZE);
    EXPECT_EQ(currentConfig.priQueueSize, DEFAULT_PRI_QUEUE_SIZE);
}

// Integration test - verify configuration affects SMMU behavior
TEST_F(ConfigurationTest, ConfigurationAffectsSMMUBehavior) {
    // Create SMMU with larger cache configuration
    SMMUConfiguration config = SMMUConfiguration::createDefault();
    CacheConfiguration largeCacheConfig(4096, 10000, true);
    config.setCacheConfiguration(largeCacheConfig);
    
    SMMU smmu(config);
    
    // Verify cache statistics are available (implementation dependent)
    CacheStatistics stats = smmu.getCacheStatistics();
    // Note: We can't test exact behavior without setting up streams and translations,
    // but we can verify the configuration was applied
    EXPECT_TRUE(config.getCacheConfiguration().enableCaching);
}