// ARM SMMU v3 Configuration System
// Copyright (c) 2024 John Greninger

#ifndef SMMU_CONFIGURATION_H
#define SMMU_CONFIGURATION_H

#include "smmu/types.h"
#include <string>
#include <unordered_map>
#include <vector>
#include <cstddef>

namespace smmu {

// Queue configuration structure
struct QueueConfiguration {
    size_t eventQueueSize;      // Maximum event queue size (default: 512)
    size_t commandQueueSize;    // Maximum command queue size (default: 256)
    size_t priQueueSize;        // Maximum PRI queue size (default: 128)
    
    // Constructor with default values
    QueueConfiguration() 
        : eventQueueSize(DEFAULT_EVENT_QUEUE_SIZE),
          commandQueueSize(DEFAULT_COMMAND_QUEUE_SIZE),
          priQueueSize(DEFAULT_PRI_QUEUE_SIZE) {
    }
    
    // Constructor with custom values
    QueueConfiguration(size_t eventSize, size_t commandSize, size_t priSize)
        : eventQueueSize(eventSize),
          commandQueueSize(commandSize),
          priQueueSize(priSize) {
    }
    
    // Validation method
    bool isValid() const {
        return eventQueueSize >= MIN_QUEUE_SIZE && eventQueueSize <= MAX_QUEUE_SIZE &&
               commandQueueSize >= MIN_QUEUE_SIZE && commandQueueSize <= MAX_QUEUE_SIZE &&
               priQueueSize >= MIN_QUEUE_SIZE && priQueueSize <= MAX_QUEUE_SIZE;
    }
    
private:
    static const size_t MIN_QUEUE_SIZE = 16;
    static const size_t MAX_QUEUE_SIZE = 65536;
};

// Cache configuration structure
struct CacheConfiguration {
    size_t tlbCacheSize;        // TLB cache size in entries (default: 1024)
    uint32_t cacheMaxAge;       // Maximum cache entry age in milliseconds (default: 5000)
    bool enableCaching;         // Enable/disable caching globally (default: true)
    
    // Constructor with default values
    CacheConfiguration()
        : tlbCacheSize(DEFAULT_TLB_CACHE_SIZE),
          cacheMaxAge(DEFAULT_CACHE_MAX_AGE),
          enableCaching(true) {
    }
    
    // Constructor with custom values
    CacheConfiguration(size_t cacheSize, uint32_t maxAge, bool enable)
        : tlbCacheSize(cacheSize),
          cacheMaxAge(maxAge),
          enableCaching(enable) {
    }
    
    // Validation method
    bool isValid() const {
        return tlbCacheSize >= MIN_CACHE_SIZE && tlbCacheSize <= MAX_CACHE_SIZE &&
               cacheMaxAge >= MIN_CACHE_AGE && cacheMaxAge <= MAX_CACHE_AGE;
    }
    
private:
    static const size_t MIN_CACHE_SIZE = 64;
    static const size_t MAX_CACHE_SIZE = 1048576;  // 1M entries
    static const uint32_t MIN_CACHE_AGE = 100;     // 100ms
    static const uint32_t MAX_CACHE_AGE = 3600000; // 1 hour
    static const size_t DEFAULT_TLB_CACHE_SIZE = 1024;
    static const uint32_t DEFAULT_CACHE_MAX_AGE = 5000; // 5 seconds
};

// Address space configuration structure
struct AddressConfiguration {
    uint64_t maxIOVASize;       // Maximum IOVA address space size (default: 48-bit)
    uint64_t maxPASize;         // Maximum PA address space size (default: 52-bit) 
    uint32_t maxStreamCount;    // Maximum number of streams (default: 65536)
    uint32_t maxPASIDCount;     // Maximum PASIDs per stream (default: 1048576)
    
    // Constructor with default values
    AddressConfiguration()
        : maxIOVASize(DEFAULT_MAX_IOVA_SIZE),
          maxPASize(DEFAULT_MAX_PA_SIZE),
          maxStreamCount(DEFAULT_MAX_STREAM_COUNT),
          maxPASIDCount(DEFAULT_MAX_PASID_COUNT) {
    }
    
    // Constructor with custom values
    AddressConfiguration(uint64_t iovaSize, uint64_t paSize, uint32_t streamCount, uint32_t pasidCount)
        : maxIOVASize(iovaSize),
          maxPASize(paSize),
          maxStreamCount(streamCount),
          maxPASIDCount(pasidCount) {
    }
    
    // Validation method
    bool isValid() const {
        return maxIOVASize >= MIN_IOVA_BITS && maxIOVASize <= MAX_IOVA_BITS &&
               maxPASize >= MIN_PA_BITS && maxPASize <= MAX_PA_BITS &&
               maxStreamCount >= MIN_STREAM_COUNT && maxStreamCount <= MAX_STREAM_COUNT &&
               maxPASIDCount >= MIN_PASID_COUNT && maxPASIDCount <= MAX_PASID_COUNT;
    }
    
private:
    static const uint64_t MIN_IOVA_BITS = 32;
    static const uint64_t MAX_IOVA_BITS = 52;
    static const uint64_t MIN_PA_BITS = 32;
    static const uint64_t MAX_PA_BITS = 52;
    static const uint32_t MIN_STREAM_COUNT = 1;
    static const uint32_t MAX_STREAM_COUNT = 1048576;
    static const uint32_t MIN_PASID_COUNT = 1;
    static const uint32_t MAX_PASID_COUNT = 1048576;
    static const uint64_t DEFAULT_MAX_IOVA_SIZE = 48; // 48-bit addressing (256TB)
    static const uint64_t DEFAULT_MAX_PA_SIZE = 52;   // 52-bit addressing (4PB)
    static const uint32_t DEFAULT_MAX_STREAM_COUNT = 65536;   // 16-bit StreamID
    static const uint32_t DEFAULT_MAX_PASID_COUNT = 1048576;  // 20-bit PASID
};

// Resource limits configuration structure
struct ResourceLimits {
    uint64_t maxMemoryUsage;    // Maximum memory usage in bytes (default: 1GB)
    uint32_t maxThreadCount;    // Maximum thread count (default: hardware_concurrency)
    uint32_t timeoutMs;         // Operation timeout in milliseconds (default: 1000ms)
    bool enableResourceTracking; // Enable resource usage tracking (default: true)
    
    // Constructor with default values
    ResourceLimits()
        : maxMemoryUsage(DEFAULT_MAX_MEMORY_USAGE),
          maxThreadCount(DEFAULT_MAX_THREAD_COUNT),
          timeoutMs(DEFAULT_TIMEOUT_MS),
          enableResourceTracking(true) {
    }
    
    // Constructor with custom values
    ResourceLimits(uint64_t memoryUsage, uint32_t threadCount, uint32_t timeout, bool enableTracking)
        : maxMemoryUsage(memoryUsage),
          maxThreadCount(threadCount),
          timeoutMs(timeout),
          enableResourceTracking(enableTracking) {
    }
    
    // Validation method
    bool isValid() const {
        return maxMemoryUsage >= MIN_MEMORY_USAGE && maxMemoryUsage <= MAX_MEMORY_USAGE &&
               maxThreadCount >= MIN_THREAD_COUNT && maxThreadCount <= MAX_THREAD_COUNT &&
               timeoutMs >= MIN_TIMEOUT_MS && timeoutMs <= MAX_TIMEOUT_MS;
    }
    
private:
    static const uint64_t MIN_MEMORY_USAGE = 1024 * 1024;        // 1MB minimum
    static const uint64_t MAX_MEMORY_USAGE = 64ULL * 1024 * 1024 * 1024; // 64GB maximum
    static const uint32_t MIN_THREAD_COUNT = 1;
    static const uint32_t MAX_THREAD_COUNT = 256;
    static const uint32_t MIN_TIMEOUT_MS = 10;
    static const uint32_t MAX_TIMEOUT_MS = 300000;  // 5 minutes
    static const uint64_t DEFAULT_MAX_MEMORY_USAGE = 1024 * 1024 * 1024; // 1GB
    static const uint32_t DEFAULT_MAX_THREAD_COUNT = 8; // Will be set to hardware_concurrency in implementation
    static const uint32_t DEFAULT_TIMEOUT_MS = 1000; // 1 second
};

// Main SMMU Configuration class
class SMMUConfiguration {
public:
    // Default constructor - creates default configuration
    SMMUConfiguration();
    
    // Constructor with all configuration structures
    SMMUConfiguration(const QueueConfiguration& queueConfig,
                     const CacheConfiguration& cacheConfig,
                     const AddressConfiguration& addressConfig,
                     const ResourceLimits& resourceLimits);
    
    // Copy constructor and assignment operator
    SMMUConfiguration(const SMMUConfiguration& other);
    SMMUConfiguration& operator=(const SMMUConfiguration& other);
    
    // Destructor
    ~SMMUConfiguration();
    
    // Configuration access methods
    const QueueConfiguration& getQueueConfiguration() const;
    const CacheConfiguration& getCacheConfiguration() const;
    const AddressConfiguration& getAddressConfiguration() const;
    const ResourceLimits& getResourceLimits() const;
    
    // Configuration modification methods
    VoidResult setQueueConfiguration(const QueueConfiguration& queueConfig);
    VoidResult setCacheConfiguration(const CacheConfiguration& cacheConfig);
    VoidResult setAddressConfiguration(const AddressConfiguration& addressConfig);
    VoidResult setResourceLimits(const ResourceLimits& resourceLimits);
    
    // Validation methods
    bool isValid() const;
    std::vector<std::string> validateConfiguration() const;
    
    // JSON-like string parsing and serialization (simple key-value format)
    static Result<SMMUConfiguration> fromString(const std::string& configString);
    std::string toString() const;
    
    // Factory methods for common configurations
    static SMMUConfiguration createDefault();
    static SMMUConfiguration createHighPerformance();
    static SMMUConfiguration createLowMemory();
    static SMMUConfiguration createMinimal();
    
    // Configuration profiles
    static SMMUConfiguration createServerProfile();     // High throughput, more memory
    static SMMUConfiguration createEmbeddedProfile();   // Low memory, basic features
    static SMMUConfiguration createDevelopmentProfile(); // Debug-friendly settings
    
    // Configuration update methods (thread-safe)
    VoidResult updateQueueSizes(size_t eventSize, size_t commandSize, size_t priSize);
    VoidResult updateCacheSettings(size_t cacheSize, uint32_t maxAge, bool enableCaching);
    VoidResult updateAddressLimits(uint64_t iovaSize, uint64_t paSize, uint32_t streamCount, uint32_t pasidCount);
    VoidResult updateResourceLimits(uint64_t memoryUsage, uint32_t threadCount, uint32_t timeoutMs);
    
    // Configuration comparison
    bool operator==(const SMMUConfiguration& other) const;
    bool operator!=(const SMMUConfiguration& other) const;
    
    // Configuration merge (overlay other configuration on this one)
    VoidResult merge(const SMMUConfiguration& other);
    
    // Configuration reset to defaults
    void reset();
    
    // Configuration validation with detailed error reporting
    struct ValidationResult {
        bool isValid;
        std::vector<std::string> errors;
        std::vector<std::string> warnings;
        
        ValidationResult() : isValid(false) {}
    };
    ValidationResult validate() const;

private:
    QueueConfiguration queueConfig;
    CacheConfiguration cacheConfig;
    AddressConfiguration addressConfig;
    ResourceLimits resourceLimits;
    
    // Helper methods for string parsing
    static std::unordered_map<std::string, std::string> parseKeyValuePairs(const std::string& configString);
    static bool parseBoolean(const std::string& value);
    static uint64_t parseUInt64(const std::string& value);
    static uint32_t parseUInt32(const std::string& value);
    static size_t parseSize(const std::string& value);
    
    // Helper methods for validation
    bool validateQueueConfiguration() const;
    bool validateCacheConfiguration() const;
    bool validateAddressConfiguration() const;
    bool validateResourceLimits() const;
    
    // Helper methods for string serialization
    std::string booleanToString(bool value) const;
    std::string uint64ToString(uint64_t value) const;
    std::string uint32ToString(uint32_t value) const;
    std::string sizeToString(size_t value) const;
};

// Configuration validation error types
enum class ConfigurationErrorType {
    InvalidQueueSize,
    InvalidCacheSize,
    InvalidAddressSize,
    InvalidResourceLimit,
    InvalidFormat,
    MissingRequired,
    OutOfRange
};

// Configuration error structure
struct ConfigurationError {
    ConfigurationErrorType type;
    std::string field;
    std::string message;
    
    ConfigurationError(ConfigurationErrorType t, const std::string& f, const std::string& m)
        : type(t), field(f), message(m) {}
};

// Global configuration constants
namespace ConfigConstants {
    // Default configuration file names
    extern const std::string DEFAULT_CONFIG_FILE;
    extern const std::string BACKUP_CONFIG_FILE;
    
    // Configuration format version
    extern const std::string CONFIG_VERSION;
    
    // Environment variable names for configuration override
    extern const std::string ENV_CONFIG_FILE;
    extern const std::string ENV_QUEUE_SIZE;
    extern const std::string ENV_CACHE_SIZE;
    extern const std::string ENV_MEMORY_LIMIT;
}

} // namespace smmu

#endif // SMMU_CONFIGURATION_H