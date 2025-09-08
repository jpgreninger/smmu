// ARM SMMU v3 Configuration System Example
// Copyright (c) 2024 John Greninger

#include <iostream>
#include <string>
#include "smmu/smmu.h"
#include "smmu/configuration.h"

using namespace smmu;

void printConfiguration(const SMMUConfiguration& config) {
    const QueueConfiguration& queueConfig = config.getQueueConfiguration();
    const CacheConfiguration& cacheConfig = config.getCacheConfiguration();
    const AddressConfiguration& addressConfig = config.getAddressConfiguration();
    const ResourceLimits& resourceLimits = config.getResourceLimits();
    
    std::cout << "SMMU Configuration:\n";
    std::cout << "  Queue Configuration:\n";
    std::cout << "    Event Queue Size: " << queueConfig.eventQueueSize << "\n";
    std::cout << "    Command Queue Size: " << queueConfig.commandQueueSize << "\n";
    std::cout << "    PRI Queue Size: " << queueConfig.priQueueSize << "\n";
    
    std::cout << "  Cache Configuration:\n";
    std::cout << "    TLB Cache Size: " << cacheConfig.tlbCacheSize << "\n";
    std::cout << "    Cache Max Age: " << cacheConfig.cacheMaxAge << "ms\n";
    std::cout << "    Caching Enabled: " << (cacheConfig.enableCaching ? "Yes" : "No") << "\n";
    
    std::cout << "  Address Configuration:\n";
    std::cout << "    Max IOVA Size: " << addressConfig.maxIOVASize << " bits\n";
    std::cout << "    Max PA Size: " << addressConfig.maxPASize << " bits\n";
    std::cout << "    Max Stream Count: " << addressConfig.maxStreamCount << "\n";
    std::cout << "    Max PASID Count: " << addressConfig.maxPASIDCount << "\n";
    
    std::cout << "  Resource Limits:\n";
    std::cout << "    Max Memory Usage: " << (resourceLimits.maxMemoryUsage / (1024*1024)) << " MB\n";
    std::cout << "    Max Thread Count: " << resourceLimits.maxThreadCount << "\n";
    std::cout << "    Timeout: " << resourceLimits.timeoutMs << "ms\n";
    std::cout << "    Resource Tracking: " << (resourceLimits.enableResourceTracking ? "Enabled" : "Disabled") << "\n";
    std::cout << std::endl;
}

int main() {
    std::cout << "ARM SMMU v3 Configuration System Example\n";
    std::cout << "========================================\n\n";
    
    // 1. Default Configuration
    std::cout << "1. Default Configuration\n";
    std::cout << "------------------------\n";
    SMMUConfiguration defaultConfig = SMMUConfiguration::createDefault();
    printConfiguration(defaultConfig);
    
    // 2. High Performance Configuration
    std::cout << "2. High Performance Configuration\n";
    std::cout << "----------------------------------\n";
    SMMUConfiguration highPerfConfig = SMMUConfiguration::createHighPerformance();
    printConfiguration(highPerfConfig);
    
    // 3. Low Memory Configuration
    std::cout << "3. Low Memory Configuration\n";
    std::cout << "---------------------------\n";
    SMMUConfiguration lowMemConfig = SMMUConfiguration::createLowMemory();
    printConfiguration(lowMemConfig);
    
    // 4. Custom Configuration
    std::cout << "4. Custom Configuration\n";
    std::cout << "-----------------------\n";
    QueueConfiguration customQueue(1024, 512, 256);
    CacheConfiguration customCache(2048, 8000, true);
    AddressConfiguration customAddress(48, 48, 32768, 65536);
    ResourceLimits customLimits(512 * 1024 * 1024, 4, 2000, true); // 512MB, 4 threads, 2 second timeout
    
    SMMUConfiguration customConfig(customQueue, customCache, customAddress, customLimits);
    printConfiguration(customConfig);
    
    // 5. Configuration String Serialization
    std::cout << "5. Configuration Serialization\n";
    std::cout << "-------------------------------\n";
    std::string configString = customConfig.toString();
    std::cout << "Configuration as string:\n" << configString << "\n";
    
    // Parse the configuration back from string
    Result<SMMUConfiguration> parseResult = SMMUConfiguration::fromString(configString);
    if (parseResult.isOk()) {
        std::cout << "Successfully parsed configuration from string!\n\n";
        SMMUConfiguration parsedConfig = parseResult.getValue();
        printConfiguration(parsedConfig);
    } else {
        std::cout << "Failed to parse configuration from string\n\n";
    }
    
    // 6. SMMU with Configuration
    std::cout << "6. SMMU with Custom Configuration\n";
    std::cout << "----------------------------------\n";
    try {
        SMMU smmu(highPerfConfig);
        
        std::cout << "SMMU initialized with high performance configuration\n";
        std::cout << "Current stream count: " << smmu.getStreamCount() << "\n";
        std::cout << "Total translations: " << smmu.getTotalTranslations() << "\n";
        
        // Update configuration at runtime
        QueueConfiguration newQueue(4096, 2048, 1024);
        VoidResult updateResult = smmu.updateQueueConfiguration(newQueue);
        
        if (updateResult.isOk()) {
            std::cout << "Successfully updated queue configuration at runtime\n";
            const QueueConfiguration& updatedQueue = smmu.getConfiguration().getQueueConfiguration();
            std::cout << "New event queue size: " << updatedQueue.eventQueueSize << "\n";
        } else {
            std::cout << "Failed to update configuration: Error code " << static_cast<int>(updateResult.getError()) << "\n";
        }
        
    } catch (const std::exception& e) {
        std::cout << "Exception creating SMMU: " << e.what() << "\n";
    }
    
    // 7. Configuration Validation
    std::cout << "\n7. Configuration Validation\n";
    std::cout << "---------------------------\n";
    
    SMMUConfiguration testConfig = SMMUConfiguration::createDefault();
    SMMUConfiguration::ValidationResult validation = testConfig.validate();
    
    std::cout << "Configuration validation result:\n";
    std::cout << "  Valid: " << (validation.isValid ? "Yes" : "No") << "\n";
    std::cout << "  Errors: " << validation.errors.size() << "\n";
    std::cout << "  Warnings: " << validation.warnings.size() << "\n";
    
    for (const std::string& error : validation.errors) {
        std::cout << "    Error: " << error << "\n";
    }
    for (const std::string& warning : validation.warnings) {
        std::cout << "    Warning: " << warning << "\n";
    }
    
    std::cout << "\nConfiguration system example completed successfully!\n";
    
    return 0;
}