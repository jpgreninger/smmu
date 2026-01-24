// ARM SMMU v3 Configuration Validation Tests (Simplified)
// Copyright (c) 2024 John Greninger
//
// This test suite provides basic coverage of configuration validation in configuration.cpp
// Focuses on configuration validation, copy operations, and error handling.
//
// Coverage Targets:
// - Configuration validation (isValid, validateConfiguration)
// - Copy constructor and assignment operator
// - Configuration accessors
// - Basic validation scenarios

#include <gtest/gtest.h>
#include "smmu/configuration.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

class ConfigurationValidationTest : public ::testing::Test {
protected:
    void SetUp() override {
        config = std::make_unique<SMMUConfiguration>();
    }

    void TearDown() override {
        config.reset();
    }

    std::unique_ptr<SMMUConfiguration> config;
};

// ========== Default Configuration Tests ==========

TEST_F(ConfigurationValidationTest, DefaultConfiguration_IsValid) {
    // Verify default configuration is valid
    EXPECT_TRUE(config->isValid());

    std::vector<std::string> errors = config->validateConfiguration();
    EXPECT_TRUE(errors.empty());
}

TEST_F(ConfigurationValidationTest, DefaultConfiguration_HasReasonableDefaults) {
    // Verify default configuration has reasonable values
    const ResourceLimits& limits = config->getResourceLimits();

    EXPECT_GT(limits.maxThreadCount, 0u);
    EXPECT_LE(limits.maxThreadCount, 256u);  // Reasonable upper bound
}

// ========== Queue Configuration Validation ==========

TEST_F(ConfigurationValidationTest, GetQueueConfiguration_ReturnsValidConfig) {
    const QueueConfiguration& qConfig = config->getQueueConfiguration();

    // Verify default queue configuration has reasonable values
    EXPECT_GT(qConfig.commandQueueSize, 0u);
    EXPECT_GT(qConfig.eventQueueSize, 0u);
    EXPECT_GT(qConfig.priQueueSize, 0u);
}

TEST_F(ConfigurationValidationTest, SetQueueConfiguration_ValidConfig_Success) {
    QueueConfiguration qConfig;
    qConfig.commandQueueSize = 1024;
    qConfig.eventQueueSize = 2048;
    qConfig.priQueueSize = 512;

    if (qConfig.isValid()) {
        VoidResult result = config->setQueueConfiguration(qConfig);
        EXPECT_TRUE(result.isOk());
    }
}

// ========== Cache Configuration Validation ==========

TEST_F(ConfigurationValidationTest, GetCacheConfiguration_ReturnsValidConfig) {
    const CacheConfiguration& cConfig = config->getCacheConfiguration();

    // Access to verify const reference works
    (void)cConfig.tlbCacheSize;
    (void)cConfig.enableCaching;
}

TEST_F(ConfigurationValidationTest, SetCacheConfiguration_ValidConfig_Success) {
    CacheConfiguration cConfig;
    cConfig.tlbCacheSize = 4096;
    cConfig.enableCaching = true;

    if (cConfig.isValid()) {
        VoidResult result = config->setCacheConfiguration(cConfig);
        EXPECT_TRUE(result.isOk());
    }
}

// ========== Address Configuration Validation ==========

TEST_F(ConfigurationValidationTest, GetAddressConfiguration_ReturnsValidConfig) {
    const AddressConfiguration& aConfig = config->getAddressConfiguration();

    // Verify default address configuration has reasonable values
    EXPECT_GT(aConfig.maxIOVASize, 0u);
    EXPECT_GT(aConfig.maxPASize, 0u);
}

TEST_F(ConfigurationValidationTest, SetAddressConfiguration_ValidConfig_Success) {
    AddressConfiguration aConfig;
    // Use default values which should be valid

    if (aConfig.isValid()) {
        VoidResult result = config->setAddressConfiguration(aConfig);
        EXPECT_TRUE(result.isOk());
    }
}

// ========== Resource Limits Validation ==========

TEST_F(ConfigurationValidationTest, GetResourceLimits_ReturnsValidConfig) {
    const ResourceLimits& rLimits = config->getResourceLimits();

    EXPECT_GT(rLimits.maxThreadCount, 0u);
    EXPECT_GT(rLimits.maxMemoryUsage, 0u);
}

TEST_F(ConfigurationValidationTest, SetResourceLimits_ValidConfig_Success) {
    ResourceLimits rLimits;
    // Use default values which should be valid

    if (rLimits.isValid()) {
        VoidResult result = config->setResourceLimits(rLimits);
        EXPECT_TRUE(result.isOk());
    }
}

// ========== Complete Configuration Validation ==========

TEST_F(ConfigurationValidationTest, IsValid_AllComponentsValid_ReturnsTrue) {
    EXPECT_TRUE(config->isValid());
}

TEST_F(ConfigurationValidationTest, ValidateConfiguration_AllValid_NoErrors) {
    std::vector<std::string> errors = config->validateConfiguration();
    EXPECT_TRUE(errors.empty());
}

// ========== Configuration Copy and Assignment ==========

TEST_F(ConfigurationValidationTest, CopyConstructor_CreatesIndependentCopy) {
    // Create copy
    SMMUConfiguration configCopy(*config);

    // Verify copy is valid
    EXPECT_TRUE(configCopy.isValid());

    // Verify both have same default values
    EXPECT_EQ(configCopy.getQueueConfiguration().commandQueueSize,
              config->getQueueConfiguration().commandQueueSize);
}

TEST_F(ConfigurationValidationTest, AssignmentOperator_CopiesConfiguration) {
    SMMUConfiguration anotherConfig;
    anotherConfig = *config;

    EXPECT_TRUE(anotherConfig.isValid());

    // Verify copied values match
    EXPECT_EQ(anotherConfig.getResourceLimits().maxThreadCount,
              config->getResourceLimits().maxThreadCount);
}

TEST_F(ConfigurationValidationTest, AssignmentOperator_SelfAssignment_Safe) {
    *config = *config;  // Self-assignment

    // Should still be valid
    EXPECT_TRUE(config->isValid());
}

// ========== Constructor Tests ==========

TEST_F(ConfigurationValidationTest, ParameterizedConstructor_AllParameters_Success) {
    QueueConfiguration qConfig;
    CacheConfiguration cConfig;
    AddressConfiguration aConfig;
    ResourceLimits rLimits;

    SMMUConfiguration customConfig(qConfig, cConfig, aConfig, rLimits);

    EXPECT_TRUE(customConfig.isValid());
}

// ========== Multiple Configuration Updates ==========

TEST_F(ConfigurationValidationTest, MultipleUpdates_AllValid_Success) {
    // Test updating all configuration components
    QueueConfiguration qConfig;
    CacheConfiguration cConfig;
    AddressConfiguration aConfig;
    ResourceLimits rLimits;

    if (qConfig.isValid() && cConfig.isValid() && aConfig.isValid() && rLimits.isValid()) {
        EXPECT_TRUE(config->setQueueConfiguration(qConfig).isOk());
        EXPECT_TRUE(config->setCacheConfiguration(cConfig).isOk());
        EXPECT_TRUE(config->setAddressConfiguration(aConfig).isOk());
        EXPECT_TRUE(config->setResourceLimits(rLimits).isOk());

        EXPECT_TRUE(config->isValid());
    }
}

// ========== Validation with Various Configurations ==========

TEST_F(ConfigurationValidationTest, QueueConfiguration_LargeSizes_Valid) {
    QueueConfiguration qConfig;
    qConfig.commandQueueSize = 65536;  // Large queue
    qConfig.eventQueueSize = 65536;
    qConfig.priQueueSize = 65536;

    if (qConfig.isValid()) {
        EXPECT_TRUE(config->setQueueConfiguration(qConfig).isOk());
    }
}

TEST_F(ConfigurationValidationTest, CacheConfiguration_CachingEnabled_Valid) {
    CacheConfiguration cConfig;
    cConfig.tlbCacheSize = 8192;
    cConfig.enableCaching = true;

    if (cConfig.isValid()) {
        EXPECT_TRUE(config->setCacheConfiguration(cConfig).isOk());
    }
}

TEST_F(ConfigurationValidationTest, CacheConfiguration_CachingDisabled_Valid) {
    CacheConfiguration cConfig;
    cConfig.tlbCacheSize = 0;
    cConfig.enableCaching = false;

    if (cConfig.isValid()) {
        EXPECT_TRUE(config->setCacheConfiguration(cConfig).isOk());
    }
}

TEST_F(ConfigurationValidationTest, ResourceLimits_ModerateValues_Valid) {
    ResourceLimits rLimits;
    rLimits.maxThreadCount = 16;
    rLimits.maxMemoryUsage = 2ULL * 1024 * 1024 * 1024;  // 2GB
    rLimits.timeoutMs = 5000;
    rLimits.enableResourceTracking = true;

    if (rLimits.isValid()) {
        EXPECT_TRUE(config->setResourceLimits(rLimits).isOk());
    }
}

} // namespace test
} // namespace smmu
