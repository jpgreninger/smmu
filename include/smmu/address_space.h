// ARM SMMU v3 Address Space
// Copyright (c) 2024 John Greninger

#ifndef SMMU_ADDRESS_SPACE_H
#define SMMU_ADDRESS_SPACE_H

#include "smmu/types.h"
#include <unordered_map>
#include <memory>
#include <vector>
#include <utility>
#include <cstddef>

namespace smmu {

class AddressSpace {
public:
    AddressSpace();
    ~AddressSpace();
    
    // Page mapping operations
    VoidResult mapPage(IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState = SecurityState::NonSecure);
    VoidResult unmapPage(IOVA iova);
    
    // Translation operations
    TranslationResult translatePage(IOVA iova, AccessType accessType, SecurityState securityState = SecurityState::NonSecure) const;
    
    // Address range mapping operations
    VoidResult mapRange(IOVA startIova, IOVA endIova, PA startPa, const PagePermissions& permissions);
    VoidResult unmapRange(IOVA startIova, IOVA endIova);
    
    // Bulk page operations
    VoidResult mapPages(const std::vector<std::pair<IOVA, PA>>& mappings, const PagePermissions& permissions);
    VoidResult unmapPages(const std::vector<IOVA>& iovas);
    
    // Query operations
    Result<bool> isPageMapped(IOVA iova) const;           // Returns Result<bool> - error on invalid address or system failure
    Result<PagePermissions> getPagePermissions(IOVA iova) const; // Returns Result<PagePermissions> - error on unmapped page or invalid address
    Result<size_t> getPageCount() const;                  // Returns Result<size_t> - error on page table corruption or count overflow
    
    // Address space state querying methods
    std::vector<AddressRange> getMappedRanges() const;
    uint64_t getAddressSpaceSize() const;
    bool hasOverlappingMappings(IOVA startIova, IOVA endIova) const;
    
    // Management operations
    VoidResult clear();
    
    // Copy constructor and assignment operator for C++11
    AddressSpace(const AddressSpace& other);
    AddressSpace& operator=(const AddressSpace& other);
    
    // Cache invalidation mechanisms
    void invalidateRange(IOVA startIova, IOVA endIova);
    void invalidateAll();
    void invalidateCache();
    void invalidatePage(IOVA iova);
    
private:
    // Sparse page table using hash map for efficiency
    std::unordered_map<uint64_t, PageEntry> pageTable;
    
    // Helper methods
    uint64_t pageNumber(IOVA iova) const;
    bool checkPermissions(const PagePermissions& perms, AccessType accessType) const;
};

} // namespace smmu

#endif // SMMU_ADDRESS_SPACE_H