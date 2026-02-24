// ARM SMMU v3 Fault Handler
// Copyright (c) 2024 John Greninger

#ifndef SMMU_FAULT_HANDLER_H
#define SMMU_FAULT_HANDLER_H

#include "smmu/types.h"
#include <vector>
#include <deque>
#include <mutex>
#include <atomic>
#include <cstddef>
#include <unordered_map>

namespace smmu {

class FaultHandler {
public:
    FaultHandler();
    ~FaultHandler();
    
    // Fault recording
    void recordFault(const FaultRecord& fault);
    void recordTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType);
    void recordPermissionFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType);
    
    // Event queue management
    std::vector<FaultRecord> getEvents();
    std::vector<FaultRecord> getFaults();  // Alias for getEvents
    void clearEvents();
    void clearFaults();  // Alias for clearEvents
    bool hasEvents() const;
    size_t getEventCount() const;
    size_t getFaultCount() const;  // Alias for getEventCount
    
    // Filtering operations
    std::vector<FaultRecord> getFaultsByStream(StreamID streamID);
    std::vector<FaultRecord> getFaultsByPASID(PASID pasid);
    std::vector<FaultRecord> getRecentFaults(uint64_t currentTime, uint64_t timeWindow);
    
    // Configuration
    void setMaxQueueSize(size_t maxSize);
    void setMaxFaults(size_t maxFaults);  // Alias for setMaxQueueSize
    size_t getMaxQueueSize() const;
    
    // Statistics
    uint64_t getTotalFaultCount() const;
    uint64_t getTranslationFaultCount() const;
    uint64_t getPermissionFaultCount() const;
    size_t getFaultCountByType(FaultType faultType) const;
    size_t getFaultCountByAccessType(AccessType accessType) const;
    uint64_t getFaultRate(uint64_t currentTime, uint64_t timeWindow) const;
    void resetStatistics();
    void reset();  // Complete reset
    
private:
    // Event queue (thread-safe)
    std::deque<FaultRecord> eventQueue;
    mutable std::mutex queueMutex;
    
    // Configuration
    size_t maxQueueSize;
    
    // BUG-CPP-H01 fix: statistics counters are written under queueMutex but read
    // without any lock from getTotalFaultCount() / getTranslationFaultCount() /
    // getPermissionFaultCount().  Using std::atomic<uint64_t> eliminates the data
    // race and lets getters use load(relaxed) without needing the mutex.
    std::atomic<uint64_t> totalFaults;
    std::atomic<uint64_t> translationFaults;
    std::atomic<uint64_t> permissionFaults;

    // Running counters for O(1) count queries (reflect current queue contents)
    std::unordered_map<int, size_t> faultTypeCounters;
    std::unordered_map<int, size_t> accessTypeCounters;
    
    // Helper methods
    uint64_t getCurrentTimestamp() const;
    void enforceQueueLimit();
};

} // namespace smmu

#endif // SMMU_FAULT_HANDLER_H