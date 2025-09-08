// ARM SMMU v3 Fault Handler Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/fault_handler.h"
#include <algorithm>
#include <chrono>

namespace smmu {

FaultHandler::FaultHandler() : maxQueueSize(1000), totalFaults(0), translationFaults(0), permissionFaults(0) {
}

FaultHandler::~FaultHandler() {
}

void FaultHandler::recordFault(const FaultRecord& fault) {
    std::lock_guard<std::mutex> lock(queueMutex);
    eventQueue.push_back(fault);
    
    // Update statistics
    totalFaults++;
    if (fault.faultType == FaultType::TranslationFault) {
        translationFaults++;
    }
    else if (fault.faultType == FaultType::PermissionFault) {
        permissionFaults++;
    }
    
    enforceQueueLimit();
}

void FaultHandler::recordTranslationFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType) {
    FaultRecord fault;
    fault.streamID = streamID;
    fault.pasid = pasid;
    fault.address = iova;
    fault.faultType = FaultType::TranslationFault;
    fault.accessType = accessType;
    fault.timestamp = getCurrentTimestamp();
    recordFault(fault);
}

void FaultHandler::recordPermissionFault(StreamID streamID, PASID pasid, IOVA iova, AccessType accessType) {
    FaultRecord fault;
    fault.streamID = streamID;
    fault.pasid = pasid;
    fault.address = iova;
    fault.faultType = FaultType::PermissionFault;
    fault.accessType = accessType;
    fault.timestamp = getCurrentTimestamp();
    recordFault(fault);
}

std::vector<FaultRecord> FaultHandler::getEvents() {
    std::lock_guard<std::mutex> lock(queueMutex);
    return std::vector<FaultRecord>(eventQueue.begin(), eventQueue.end());
}

std::vector<FaultRecord> FaultHandler::getFaults() {
    return getEvents();
}

void FaultHandler::clearEvents() {
    std::lock_guard<std::mutex> lock(queueMutex);
    eventQueue.clear();
}

void FaultHandler::clearFaults() {
    clearEvents();
}

bool FaultHandler::hasEvents() const {
    std::lock_guard<std::mutex> lock(queueMutex);
    return !eventQueue.empty();
}

size_t FaultHandler::getEventCount() const {
    std::lock_guard<std::mutex> lock(queueMutex);
    return eventQueue.size();
}

size_t FaultHandler::getFaultCount() const {
    return getEventCount();
}

std::vector<FaultRecord> FaultHandler::getFaultsByStream(StreamID streamID) {
    std::lock_guard<std::mutex> lock(queueMutex);
    std::vector<FaultRecord> result;
    for (const auto& fault : eventQueue) {
        if (fault.streamID == streamID) {
            result.push_back(fault);
        }
    }
    return result;
}

std::vector<FaultRecord> FaultHandler::getFaultsByPASID(PASID pasid) {
    std::lock_guard<std::mutex> lock(queueMutex);
    std::vector<FaultRecord> result;
    for (const auto& fault : eventQueue) {
        if (fault.pasid == pasid) {
            result.push_back(fault);
        }
    }
    return result;
}

std::vector<FaultRecord> FaultHandler::getRecentFaults(uint64_t currentTime, uint64_t timeWindow) {
    std::lock_guard<std::mutex> lock(queueMutex);
    std::vector<FaultRecord> result;
    uint64_t earliestTime = (currentTime > timeWindow) ? (currentTime - timeWindow) : 0;
    
    for (const auto& fault : eventQueue) {
        if (fault.timestamp > earliestTime && fault.timestamp <= currentTime) {
            result.push_back(fault);
        }
    }
    return result;
}

void FaultHandler::setMaxQueueSize(size_t maxSize) {
    std::lock_guard<std::mutex> lock(queueMutex);
    maxQueueSize = maxSize;
    enforceQueueLimit();
}

void FaultHandler::setMaxFaults(size_t maxFaults) {
    setMaxQueueSize(maxFaults);
}

size_t FaultHandler::getMaxQueueSize() const {
    return maxQueueSize;
}

uint64_t FaultHandler::getTotalFaultCount() const {
    return totalFaults;
}

uint64_t FaultHandler::getTranslationFaultCount() const {
    return translationFaults;
}

uint64_t FaultHandler::getPermissionFaultCount() const {
    return permissionFaults;
}

size_t FaultHandler::getFaultCountByType(FaultType faultType) const {
    std::lock_guard<std::mutex> lock(queueMutex);
    size_t count = 0;
    for (const auto& fault : eventQueue) {
        if (fault.faultType == faultType) {
            count++;
        }
    }
    return count;
}

size_t FaultHandler::getFaultCountByAccessType(AccessType accessType) const {
    std::lock_guard<std::mutex> lock(queueMutex);
    size_t count = 0;
    for (const auto& fault : eventQueue) {
        if (fault.accessType == accessType) {
            count++;
        }
    }
    return count;
}

uint64_t FaultHandler::getFaultRate(uint64_t currentTime, uint64_t timeWindow) const {
    std::vector<FaultRecord> recentFaults = const_cast<FaultHandler*>(this)->getRecentFaults(currentTime, timeWindow);
    return recentFaults.size();
}

void FaultHandler::resetStatistics() {
    std::lock_guard<std::mutex> lock(queueMutex);
    totalFaults = 0;
    translationFaults = 0;
    permissionFaults = 0;
}

void FaultHandler::reset() {
    clearEvents();
    resetStatistics();
}

uint64_t FaultHandler::getCurrentTimestamp() const {
    auto now = std::chrono::high_resolution_clock::now();
    auto duration = now.time_since_epoch();
    return std::chrono::duration_cast<std::chrono::microseconds>(duration).count();
}

void FaultHandler::enforceQueueLimit() {
    while (eventQueue.size() > maxQueueSize) {
        eventQueue.pop_front();
    }
}

}