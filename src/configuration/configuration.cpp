// ARM SMMU v3 Configuration System Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/configuration.h"
#include <sstream>
#include <algorithm>
#include <cctype>
#include <thread>

namespace smmu {

// Global configuration constants
namespace ConfigConstants {
    const std::string DEFAULT_CONFIG_FILE = "smmu_config.txt";
    const std::string BACKUP_CONFIG_FILE = "smmu_config.backup.txt";
    const std::string CONFIG_VERSION = "1.0";
    
    const std::string ENV_CONFIG_FILE = "SMMU_CONFIG_FILE";
    const std::string ENV_QUEUE_SIZE = "SMMU_QUEUE_SIZE";
    const std::string ENV_CACHE_SIZE = "SMMU_CACHE_SIZE";
    const std::string ENV_MEMORY_LIMIT = "SMMU_MEMORY_LIMIT";
}

// SMMUConfiguration implementation

// Default constructor
SMMUConfiguration::SMMUConfiguration()
    : queueConfig(),
      cacheConfig(),
      addressConfig(),
      resourceLimits() {
    // Set default thread count to hardware concurrency
    resourceLimits.maxThreadCount = std::thread::hardware_concurrency();
    if (resourceLimits.maxThreadCount == 0) {
        resourceLimits.maxThreadCount = 8; // Fallback default
    }
}

// Constructor with all configuration structures
SMMUConfiguration::SMMUConfiguration(const QueueConfiguration& qConfig,
                                   const CacheConfiguration& cConfig,
                                   const AddressConfiguration& aConfig,
                                   const ResourceLimits& rLimits)
    : queueConfig(qConfig),
      cacheConfig(cConfig),
      addressConfig(aConfig),
      resourceLimits(rLimits) {
}

// Copy constructor
SMMUConfiguration::SMMUConfiguration(const SMMUConfiguration& other)
    : queueConfig(other.queueConfig),
      cacheConfig(other.cacheConfig),
      addressConfig(other.addressConfig),
      resourceLimits(other.resourceLimits) {
}

// Assignment operator
SMMUConfiguration& SMMUConfiguration::operator=(const SMMUConfiguration& other) {
    if (this != &other) {
        queueConfig = other.queueConfig;
        cacheConfig = other.cacheConfig;
        addressConfig = other.addressConfig;
        resourceLimits = other.resourceLimits;
    }
    return *this;
}

// Destructor
SMMUConfiguration::~SMMUConfiguration() {
    // Nothing to do - all members are value types
}

// Configuration access methods
const QueueConfiguration& SMMUConfiguration::getQueueConfiguration() const {
    return queueConfig;
}

const CacheConfiguration& SMMUConfiguration::getCacheConfiguration() const {
    return cacheConfig;
}

const AddressConfiguration& SMMUConfiguration::getAddressConfiguration() const {
    return addressConfig;
}

const ResourceLimits& SMMUConfiguration::getResourceLimits() const {
    return resourceLimits;
}

// Configuration modification methods
VoidResult SMMUConfiguration::setQueueConfiguration(const QueueConfiguration& qConfig) {
    if (!qConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    queueConfig = qConfig;
    return makeVoidSuccess();
}

VoidResult SMMUConfiguration::setCacheConfiguration(const CacheConfiguration& cConfig) {
    if (!cConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    cacheConfig = cConfig;
    return makeVoidSuccess();
}

VoidResult SMMUConfiguration::setAddressConfiguration(const AddressConfiguration& aConfig) {
    if (!aConfig.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    addressConfig = aConfig;
    return makeVoidSuccess();
}

VoidResult SMMUConfiguration::setResourceLimits(const ResourceLimits& rLimits) {
    if (!rLimits.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    resourceLimits = rLimits;
    return makeVoidSuccess();
}

// Validation methods
bool SMMUConfiguration::isValid() const {
    return queueConfig.isValid() && cacheConfig.isValid() && 
           addressConfig.isValid() && resourceLimits.isValid();
}

std::vector<std::string> SMMUConfiguration::validateConfiguration() const {
    std::vector<std::string> errors;
    
    if (!queueConfig.isValid()) {
        errors.push_back("Invalid queue configuration");
    }
    if (!cacheConfig.isValid()) {
        errors.push_back("Invalid cache configuration");
    }
    if (!addressConfig.isValid()) {
        errors.push_back("Invalid address configuration");
    }
    if (!resourceLimits.isValid()) {
        errors.push_back("Invalid resource limits");
    }
    
    return errors;
}

// String parsing and serialization
Result<SMMUConfiguration> SMMUConfiguration::fromString(const std::string& configString) {
    try {
        std::unordered_map<std::string, std::string> keyValuePairs = parseKeyValuePairs(configString);
        
        SMMUConfiguration config;
        
        // Parse queue configuration
        if (keyValuePairs.find("event_queue_size") != keyValuePairs.end()) {
            config.queueConfig.eventQueueSize = parseSize(keyValuePairs["event_queue_size"]);
        }
        if (keyValuePairs.find("command_queue_size") != keyValuePairs.end()) {
            config.queueConfig.commandQueueSize = parseSize(keyValuePairs["command_queue_size"]);
        }
        if (keyValuePairs.find("pri_queue_size") != keyValuePairs.end()) {
            config.queueConfig.priQueueSize = parseSize(keyValuePairs["pri_queue_size"]);
        }
        
        // Parse cache configuration
        if (keyValuePairs.find("tlb_cache_size") != keyValuePairs.end()) {
            config.cacheConfig.tlbCacheSize = parseSize(keyValuePairs["tlb_cache_size"]);
        }
        if (keyValuePairs.find("cache_max_age") != keyValuePairs.end()) {
            config.cacheConfig.cacheMaxAge = parseUInt32(keyValuePairs["cache_max_age"]);
        }
        if (keyValuePairs.find("enable_caching") != keyValuePairs.end()) {
            config.cacheConfig.enableCaching = parseBoolean(keyValuePairs["enable_caching"]);
        }
        
        // Parse address configuration
        if (keyValuePairs.find("max_iova_size") != keyValuePairs.end()) {
            config.addressConfig.maxIOVASize = parseUInt64(keyValuePairs["max_iova_size"]);
        }
        if (keyValuePairs.find("max_pa_size") != keyValuePairs.end()) {
            config.addressConfig.maxPASize = parseUInt64(keyValuePairs["max_pa_size"]);
        }
        if (keyValuePairs.find("max_stream_count") != keyValuePairs.end()) {
            config.addressConfig.maxStreamCount = parseUInt32(keyValuePairs["max_stream_count"]);
        }
        if (keyValuePairs.find("max_pasid_count") != keyValuePairs.end()) {
            config.addressConfig.maxPASIDCount = parseUInt32(keyValuePairs["max_pasid_count"]);
        }
        
        // Parse resource limits
        if (keyValuePairs.find("max_memory_usage") != keyValuePairs.end()) {
            config.resourceLimits.maxMemoryUsage = parseUInt64(keyValuePairs["max_memory_usage"]);
        }
        if (keyValuePairs.find("max_thread_count") != keyValuePairs.end()) {
            config.resourceLimits.maxThreadCount = parseUInt32(keyValuePairs["max_thread_count"]);
        }
        if (keyValuePairs.find("timeout_ms") != keyValuePairs.end()) {
            config.resourceLimits.timeoutMs = parseUInt32(keyValuePairs["timeout_ms"]);
        }
        if (keyValuePairs.find("enable_resource_tracking") != keyValuePairs.end()) {
            config.resourceLimits.enableResourceTracking = parseBoolean(keyValuePairs["enable_resource_tracking"]);
        }
        
        // Validate the final configuration
        if (!config.isValid()) {
            return makeError<SMMUConfiguration>(SMMUError::InvalidConfiguration);
        }
        
        return makeSuccess<SMMUConfiguration>(config);
        
    } catch (const std::exception&) {
        return makeError<SMMUConfiguration>(SMMUError::ParseError);
    }
}

std::string SMMUConfiguration::toString() const {
    std::ostringstream oss;
    
    // Queue configuration
    oss << "event_queue_size=" << sizeToString(queueConfig.eventQueueSize) << "\n";
    oss << "command_queue_size=" << sizeToString(queueConfig.commandQueueSize) << "\n";
    oss << "pri_queue_size=" << sizeToString(queueConfig.priQueueSize) << "\n";
    
    // Cache configuration
    oss << "tlb_cache_size=" << sizeToString(cacheConfig.tlbCacheSize) << "\n";
    oss << "cache_max_age=" << uint32ToString(cacheConfig.cacheMaxAge) << "\n";
    oss << "enable_caching=" << booleanToString(cacheConfig.enableCaching) << "\n";
    
    // Address configuration
    oss << "max_iova_size=" << uint64ToString(addressConfig.maxIOVASize) << "\n";
    oss << "max_pa_size=" << uint64ToString(addressConfig.maxPASize) << "\n";
    oss << "max_stream_count=" << uint32ToString(addressConfig.maxStreamCount) << "\n";
    oss << "max_pasid_count=" << uint32ToString(addressConfig.maxPASIDCount) << "\n";
    
    // Resource limits
    oss << "max_memory_usage=" << uint64ToString(resourceLimits.maxMemoryUsage) << "\n";
    oss << "max_thread_count=" << uint32ToString(resourceLimits.maxThreadCount) << "\n";
    oss << "timeout_ms=" << uint32ToString(resourceLimits.timeoutMs) << "\n";
    oss << "enable_resource_tracking=" << booleanToString(resourceLimits.enableResourceTracking) << "\n";
    
    return oss.str();
}

// Factory methods
SMMUConfiguration SMMUConfiguration::createDefault() {
    return SMMUConfiguration();
}

SMMUConfiguration SMMUConfiguration::createHighPerformance() {
    QueueConfiguration queueConfig(2048, 1024, 512);    // Larger queues
    CacheConfiguration cacheConfig(8192, 10000, true);  // Larger cache, longer retention
    AddressConfiguration addressConfig(52, 52, 1048576, 1048576); // Maximum addressing
    ResourceLimits resourceLimits(4ULL * 1024 * 1024 * 1024, 16, 5000, true); // 4GB, 16 threads
    
    return SMMUConfiguration(queueConfig, cacheConfig, addressConfig, resourceLimits);
}

SMMUConfiguration SMMUConfiguration::createLowMemory() {
    QueueConfiguration queueConfig(128, 64, 32);        // Smaller queues
    CacheConfiguration cacheConfig(256, 2000, true);    // Smaller cache, shorter retention
    AddressConfiguration addressConfig(40, 40, 4096, 256); // Limited addressing
    ResourceLimits resourceLimits(128 * 1024 * 1024, 2, 500, false); // 128MB, 2 threads, no tracking
    
    return SMMUConfiguration(queueConfig, cacheConfig, addressConfig, resourceLimits);
}

SMMUConfiguration SMMUConfiguration::createMinimal() {
    QueueConfiguration queueConfig(64, 32, 16);         // Minimal queues
    CacheConfiguration cacheConfig(128, 1000, true);    // Minimal cache
    AddressConfiguration addressConfig(32, 32, 256, 64); // Minimal addressing
    ResourceLimits resourceLimits(32 * 1024 * 1024, 1, 100, false); // 32MB, 1 thread
    
    return SMMUConfiguration(queueConfig, cacheConfig, addressConfig, resourceLimits);
}

SMMUConfiguration SMMUConfiguration::createServerProfile() {
    QueueConfiguration queueConfig(4096, 2048, 1024);   // Large queues for high throughput
    CacheConfiguration cacheConfig(16384, 30000, true); // Very large cache, long retention
    AddressConfiguration addressConfig(52, 52, 1048576, 1048576); // Full addressing
    ResourceLimits resourceLimits(8ULL * 1024 * 1024 * 1024, 32, 10000, true); // 8GB, 32 threads
    
    return SMMUConfiguration(queueConfig, cacheConfig, addressConfig, resourceLimits);
}

SMMUConfiguration SMMUConfiguration::createEmbeddedProfile() {
    QueueConfiguration queueConfig(256, 128, 64);       // Medium queues
    CacheConfiguration cacheConfig(512, 3000, true);    // Small cache
    AddressConfiguration addressConfig(40, 40, 1024, 256); // Limited addressing
    ResourceLimits resourceLimits(256 * 1024 * 1024, 4, 1000, false); // 256MB, 4 threads
    
    return SMMUConfiguration(queueConfig, cacheConfig, addressConfig, resourceLimits);
}

SMMUConfiguration SMMUConfiguration::createDevelopmentProfile() {
    QueueConfiguration queueConfig(1024, 512, 256);     // Medium queues for debugging
    CacheConfiguration cacheConfig(2048, 15000, true);  // Medium cache with longer retention for debugging
    AddressConfiguration addressConfig(48, 48, 65536, 65536); // Standard addressing
    ResourceLimits resourceLimits(2ULL * 1024 * 1024 * 1024, 8, 30000, true); // 2GB, debug-friendly timeout
    
    return SMMUConfiguration(queueConfig, cacheConfig, addressConfig, resourceLimits);
}

// Configuration update methods
VoidResult SMMUConfiguration::updateQueueSizes(size_t eventSize, size_t commandSize, size_t priSize) {
    QueueConfiguration newConfig(eventSize, commandSize, priSize);
    return setQueueConfiguration(newConfig);
}

VoidResult SMMUConfiguration::updateCacheSettings(size_t cacheSize, uint32_t maxAge, bool enableCaching) {
    CacheConfiguration newConfig(cacheSize, maxAge, enableCaching);
    return setCacheConfiguration(newConfig);
}

VoidResult SMMUConfiguration::updateAddressLimits(uint64_t iovaSize, uint64_t paSize, uint32_t streamCount, uint32_t pasidCount) {
    AddressConfiguration newConfig(iovaSize, paSize, streamCount, pasidCount);
    return setAddressConfiguration(newConfig);
}

VoidResult SMMUConfiguration::updateResourceLimits(uint64_t memoryUsage, uint32_t threadCount, uint32_t timeoutMs) {
    ResourceLimits newLimits(memoryUsage, threadCount, timeoutMs, resourceLimits.enableResourceTracking);
    return setResourceLimits(newLimits);
}

// Configuration comparison
bool SMMUConfiguration::operator==(const SMMUConfiguration& other) const {
    return queueConfig.eventQueueSize == other.queueConfig.eventQueueSize &&
           queueConfig.commandQueueSize == other.queueConfig.commandQueueSize &&
           queueConfig.priQueueSize == other.queueConfig.priQueueSize &&
           cacheConfig.tlbCacheSize == other.cacheConfig.tlbCacheSize &&
           cacheConfig.cacheMaxAge == other.cacheConfig.cacheMaxAge &&
           cacheConfig.enableCaching == other.cacheConfig.enableCaching &&
           addressConfig.maxIOVASize == other.addressConfig.maxIOVASize &&
           addressConfig.maxPASize == other.addressConfig.maxPASize &&
           addressConfig.maxStreamCount == other.addressConfig.maxStreamCount &&
           addressConfig.maxPASIDCount == other.addressConfig.maxPASIDCount &&
           resourceLimits.maxMemoryUsage == other.resourceLimits.maxMemoryUsage &&
           resourceLimits.maxThreadCount == other.resourceLimits.maxThreadCount &&
           resourceLimits.timeoutMs == other.resourceLimits.timeoutMs &&
           resourceLimits.enableResourceTracking == other.resourceLimits.enableResourceTracking;
}

bool SMMUConfiguration::operator!=(const SMMUConfiguration& other) const {
    return !(*this == other);
}

// Configuration merge
VoidResult SMMUConfiguration::merge(const SMMUConfiguration& other) {
    if (!other.isValid()) {
        return makeVoidError(SMMUError::InvalidConfiguration);
    }
    
    queueConfig = other.queueConfig;
    cacheConfig = other.cacheConfig;
    addressConfig = other.addressConfig;
    resourceLimits = other.resourceLimits;
    
    return makeVoidSuccess();
}

// Configuration reset
void SMMUConfiguration::reset() {
    *this = SMMUConfiguration();
}

// Detailed validation
SMMUConfiguration::ValidationResult SMMUConfiguration::validate() const {
    ValidationResult result;
    result.isValid = true;
    
    // Validate queue configuration
    if (!queueConfig.isValid()) {
        result.isValid = false;
        result.errors.push_back("Queue configuration validation failed");
        
        if (queueConfig.eventQueueSize < 16 || queueConfig.eventQueueSize > 65536) {
            result.errors.push_back("Event queue size out of range [16, 65536]");
        }
        if (queueConfig.commandQueueSize < 16 || queueConfig.commandQueueSize > 65536) {
            result.errors.push_back("Command queue size out of range [16, 65536]");
        }
        if (queueConfig.priQueueSize < 16 || queueConfig.priQueueSize > 65536) {
            result.errors.push_back("PRI queue size out of range [16, 65536]");
        }
    }
    
    // Validate cache configuration
    if (!cacheConfig.isValid()) {
        result.isValid = false;
        result.errors.push_back("Cache configuration validation failed");
        
        if (cacheConfig.tlbCacheSize < 64 || cacheConfig.tlbCacheSize > 1048576) {
            result.errors.push_back("TLB cache size out of range [64, 1048576]");
        }
        if (cacheConfig.cacheMaxAge < 100 || cacheConfig.cacheMaxAge > 3600000) {
            result.errors.push_back("Cache max age out of range [100ms, 1 hour]");
        }
    }
    
    // Validate address configuration
    if (!addressConfig.isValid()) {
        result.isValid = false;
        result.errors.push_back("Address configuration validation failed");
        
        if (addressConfig.maxIOVASize < 32 || addressConfig.maxIOVASize > 52) {
            result.errors.push_back("Max IOVA size out of range [32, 52] bits");
        }
        if (addressConfig.maxPASize < 32 || addressConfig.maxPASize > 52) {
            result.errors.push_back("Max PA size out of range [32, 52] bits");
        }
        if (addressConfig.maxStreamCount < 1 || addressConfig.maxStreamCount > 1048576) {
            result.errors.push_back("Max stream count out of range [1, 1048576]");
        }
        if (addressConfig.maxPASIDCount < 1 || addressConfig.maxPASIDCount > 1048576) {
            result.errors.push_back("Max PASID count out of range [1, 1048576]");
        }
    }
    
    // Validate resource limits
    if (!resourceLimits.isValid()) {
        result.isValid = false;
        result.errors.push_back("Resource limits validation failed");
        
        if (resourceLimits.maxMemoryUsage < 1024 * 1024 || resourceLimits.maxMemoryUsage > 64ULL * 1024 * 1024 * 1024) {
            result.errors.push_back("Max memory usage out of range [1MB, 64GB]");
        }
        if (resourceLimits.maxThreadCount < 1 || resourceLimits.maxThreadCount > 256) {
            result.errors.push_back("Max thread count out of range [1, 256]");
        }
        if (resourceLimits.timeoutMs < 10 || resourceLimits.timeoutMs > 300000) {
            result.errors.push_back("Timeout out of range [10ms, 5 minutes]");
        }
    }
    
    // Add warnings for potentially suboptimal settings
    if (cacheConfig.tlbCacheSize > 4096) {
        result.warnings.push_back("Large TLB cache size may consume significant memory");
    }
    if (resourceLimits.timeoutMs > 10000) {
        result.warnings.push_back("Long timeout may affect system responsiveness");
    }
    if (queueConfig.eventQueueSize > 2048) {
        result.warnings.push_back("Large event queue may consume significant memory");
    }
    
    return result;
}

// Helper methods for string parsing
std::unordered_map<std::string, std::string> SMMUConfiguration::parseKeyValuePairs(const std::string& configString) {
    std::unordered_map<std::string, std::string> result;
    std::istringstream stream(configString);
    std::string line;
    
    while (std::getline(stream, line)) {
        // Skip empty lines and comments
        if (line.empty() || line[0] == '#') {
            continue;
        }
        
        size_t equalPos = line.find('=');
        if (equalPos != std::string::npos) {
            std::string key = line.substr(0, equalPos);
            std::string value = line.substr(equalPos + 1);
            
            // Trim whitespace
            key.erase(0, key.find_first_not_of(" \t"));
            key.erase(key.find_last_not_of(" \t") + 1);
            value.erase(0, value.find_first_not_of(" \t"));
            value.erase(value.find_last_not_of(" \t") + 1);
            
            result[key] = value;
        }
    }
    
    return result;
}

bool SMMUConfiguration::parseBoolean(const std::string& value) {
    std::string lowercaseValue = value;
    std::transform(lowercaseValue.begin(), lowercaseValue.end(), lowercaseValue.begin(), ::tolower);
    return lowercaseValue == "true" || lowercaseValue == "1" || lowercaseValue == "yes" || lowercaseValue == "on";
}

uint64_t SMMUConfiguration::parseUInt64(const std::string& value) {
    return static_cast<uint64_t>(std::stoull(value));
}

uint32_t SMMUConfiguration::parseUInt32(const std::string& value) {
    return static_cast<uint32_t>(std::stoul(value));
}

size_t SMMUConfiguration::parseSize(const std::string& value) {
    return static_cast<size_t>(std::stoull(value));
}

// Helper methods for string serialization
std::string SMMUConfiguration::booleanToString(bool value) const {
    return value ? "true" : "false";
}

std::string SMMUConfiguration::uint64ToString(uint64_t value) const {
    return std::to_string(value);
}

std::string SMMUConfiguration::uint32ToString(uint32_t value) const {
    return std::to_string(value);
}

std::string SMMUConfiguration::sizeToString(size_t value) const {
    return std::to_string(value);
}

} // namespace smmu