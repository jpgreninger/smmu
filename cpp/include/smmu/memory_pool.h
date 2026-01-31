/**
 * @file memory_pool.h
 * @brief Memory Pool Template for Performance Optimization
 * @details Implements a thread-safe object pool for efficient memory allocation
 *          and reuse. Reduces allocation overhead for frequently created/destroyed
 *          objects like PageEntry structures.
 *
 * Key Features:
 * - Thread-safe allocation and deallocation
 * - Configurable initial size and growth strategy
 * - Object reuse with proper initialization
 * - Zero-overhead when pool has available objects
 * - C++11 compliant with smart pointer support
 *
 * QA.5 Task 1: Performance Optimization - Memory Pooling
 * Copyright (c) 2024 John Greninger
 */

#ifndef SMMU_MEMORY_POOL_H
#define SMMU_MEMORY_POOL_H

#include "smmu/types.h"
#include <vector>
#include <mutex>
#include <memory>

namespace smmu {

/**
 * @class MemoryPool
 * @brief Template-based memory pool for efficient object allocation
 * @tparam T Type of objects to pool (e.g., PageEntry)
 *
 * @details Provides a thread-safe memory pool that reduces allocation overhead
 *          by reusing previously allocated objects. When an object is released,
 *          it returns to the pool for future reuse instead of being deallocated.
 *
 * Performance Benefits:
 * - Eliminates repeated malloc/free overhead
 * - Reduces memory fragmentation
 * - Improves cache locality for frequently used objects
 * - Provides predictable allocation performance
 *
 * Thread Safety:
 * - All public methods are thread-safe
 * - Uses std::mutex for protection
 * - Safe for concurrent acquire/release operations
 *
 * Usage Example:
 * @code
 *   MemoryPool<PageEntry> pool(100, 50);  // 100 initial, grow by 50
 *
 *   // Acquire object from pool
 *   PageEntry* entry = pool.acquire(0x1000, perms, SecurityState::NonSecure);
 *
 *   // Use the entry...
 *
 *   // Release back to pool for reuse
 *   pool.release(entry);
 * @endcode
 */
template<typename T>
class MemoryPool {
private:
    std::vector<T*> freeList;           ///< Available objects for reuse
    std::vector<T*> allocatedObjects;   ///< All allocated objects (for cleanup)
    mutable std::mutex poolMutex;       ///< Thread safety protection (mutable for const methods)
    size_t initialSize;                 ///< Initial pool size
    size_t growthSize;                  ///< Number of objects to add when pool is empty
    size_t totalCapacity;               ///< Total number of objects allocated
    size_t usedCount;                   ///< Number of objects currently in use

    /**
     * @brief Grows the pool by allocating additional objects
     * @param count Number of new objects to allocate
     * @pre poolMutex must be locked by caller
     */
    void grow(size_t count) {
        for (size_t i = 0; i < count; ++i) {
            T* obj = new T();
            allocatedObjects.push_back(obj);
            freeList.push_back(obj);
        }

        totalCapacity += count;
    }

public:
    /**
     * @brief Constructs memory pool with specified initial and growth sizes
     * @param initialSize Initial number of objects to pre-allocate
     * @param growthSize Number of objects to allocate when pool is exhausted
     */
    explicit MemoryPool(size_t initialSize = 100, size_t growthSize = 50)
        : initialSize(initialSize)
        , growthSize(growthSize)
        , totalCapacity(0)
        , usedCount(0) {

        std::lock_guard<std::mutex> lock(poolMutex);
        grow(initialSize);
    }

    /**
     * @brief Destructor - deallocates all pool objects
     * @details Cleans up all allocated objects regardless of whether
     *          they are in use or in the free list
     */
    ~MemoryPool() {
        std::lock_guard<std::mutex> lock(poolMutex);
        for (T* obj : allocatedObjects) {
            delete obj;
        }

        allocatedObjects.clear();
        freeList.clear();
    }

    /**
     * @brief Acquires object from pool (specialized for PageEntry)
     * @param physicalAddress Physical address for the PageEntry
     * @param permissions Page permissions
     * @param securityState Security state (NonSecure/Secure/Realm)
     * @return Pointer to initialized PageEntry object
     *
     * @details If pool is empty, automatically grows by growthSize.
     *          Object is initialized with provided parameters before return.
     *          Thread-safe for concurrent acquire operations.
     */
    T* acquire(PA physicalAddress, PagePermissions permissions, SecurityState securityState) {
        std::lock_guard<std::mutex> lock(poolMutex);

        // Grow pool if no free objects available
        if (freeList.empty()) {
            grow(growthSize);
        }

        // Get object from free list
        T* obj = freeList.back();
        freeList.pop_back();
        usedCount++;

        // Initialize object with parameters
        obj->physicalAddress = physicalAddress;
        obj->permissions = permissions;
        obj->securityState = securityState;
        obj->valid = true;

        return obj;
    }

    /**
     * @brief Acquires default-initialized object from pool
     * @return Pointer to default-initialized object
     *
     * @details Generic acquire method that returns object with default
     *          initialization. Use parameter version for PageEntry.
     *          Thread-safe for concurrent acquire operations.
     */
    T* acquire() {
        std::lock_guard<std::mutex> lock(poolMutex);

        // Grow pool if no free objects available
        if (freeList.empty()) {
            grow(growthSize);
        }

        // Get object from free list
        T* obj = freeList.back();
        freeList.pop_back();
        usedCount++;

        return obj;
    }

    /**
     * @brief Releases object back to pool for reuse
     * @param obj Pointer to object to release
     *
     * @details Returns object to free list for future reuse.
     *          Object is not deallocated, just marked as available.
     *          Thread-safe for concurrent release operations.
     *
     * @note Caller must not use object after release.
     * @warning Releasing an object not from this pool causes undefined behavior.
     */
    void release(T* obj) {
        if (!obj) {
            return;
        }

        std::lock_guard<std::mutex> lock(poolMutex);
        freeList.push_back(obj);
        usedCount--;
    }

    /**
     * @brief Gets total capacity of pool
     * @return Total number of objects allocated by pool
     *
     * @details Returns the sum of objects in use and available in free list.
     *          This value may increase over time as pool grows.
     *          Thread-safe.
     */
    size_t getTotalCapacity() const {
        std::lock_guard<std::mutex> lock(poolMutex);
        return totalCapacity;
    }

    /**
     * @brief Gets number of objects currently in use
     * @return Number of objects acquired but not yet released
     *
     * @details Returns count of objects currently held by clients.
     *          Value decreases as objects are released back to pool.
     *          Thread-safe.
     */
    size_t getUsedCount() const {
        std::lock_guard<std::mutex> lock(poolMutex);
        return usedCount;
    }

    /**
     * @brief Gets number of available objects in pool
     * @return Number of objects available for immediate acquisition
     *
     * @details Returns size of free list (objects ready for reuse).
     *          If zero, next acquire() will trigger pool growth.
     *          Thread-safe.
     */
    size_t getAvailableCount() const {
        std::lock_guard<std::mutex> lock(poolMutex);
        return freeList.size();
    }

    /**
     * @brief Gets pool utilization percentage
     * @return Utilization as value between 0.0 and 1.0
     *
     * @details Returns ratio of objects in use to total capacity.
     *          1.0 means all objects are in use (pool exhausted).
     *          0.0 means no objects are in use (all available).
     *          Thread-safe.
     */
    double getUtilization() const {
        std::lock_guard<std::mutex> lock(poolMutex);
        if (totalCapacity == 0) {
            return 0.0;
        }

        return static_cast<double>(usedCount) / static_cast<double>(totalCapacity);
    }

    /**
     * @brief Clears all free objects from pool (shrink pool)
     * @details Deallocates all objects in free list while preserving
     *          objects currently in use. Useful for reducing memory
     *          footprint when pool is over-allocated.
     *          Thread-safe.
     *
     * @note Objects in use are not affected.
     */
    void clear() {
        std::lock_guard<std::mutex> lock(poolMutex);

        // Delete objects in free list
        for (T* obj : freeList) {
            // Remove from allocatedObjects vector
            auto it = std::find(allocatedObjects.begin(), allocatedObjects.end(), obj);
            if (it != allocatedObjects.end()) {
                allocatedObjects.erase(it);
            }

            delete obj;
        }

        freeList.clear();
        totalCapacity = allocatedObjects.size();
    }

    // Prevent copying (pool should not be copied)
    MemoryPool(const MemoryPool&) = delete;
    MemoryPool& operator=(const MemoryPool&) = delete;

    // Allow moving (pool can be moved)
    MemoryPool(MemoryPool&& other) noexcept
        : freeList(std::move(other.freeList))
        , allocatedObjects(std::move(other.allocatedObjects))
        , initialSize(other.initialSize)
        , growthSize(other.growthSize)
        , totalCapacity(other.totalCapacity)
        , usedCount(other.usedCount) {

        other.totalCapacity = 0;
        other.usedCount = 0;
    }

    MemoryPool& operator=(MemoryPool&& other) noexcept {
        if (this != &other) {
            // Clean up existing objects
            std::lock_guard<std::mutex> lock(poolMutex);
            for (T* obj : allocatedObjects) {
                delete obj;
            }

            // Move from other
            freeList = std::move(other.freeList);
            allocatedObjects = std::move(other.allocatedObjects);
            initialSize = other.initialSize;
            growthSize = other.growthSize;
            totalCapacity = other.totalCapacity;
            usedCount = other.usedCount;

            other.totalCapacity = 0;
            other.usedCount = 0;
        }

        return *this;
    }
};

} // namespace smmu

#endif // SMMU_MEMORY_POOL_H
