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
    VoidResult mapRange(IOVA startIova, IOVA endIova, PA startPa, const PagePermissions& permissions,
                        SecurityState securityState = SecurityState::NonSecure);
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
    
    // Address size configuration (ARM §3.4.1 — TCR.T0SZ)
    // Sets the maximum IOVA bit-width for this context.  Valid range: 32–52.
    // IOVAs >= (1ULL << bits) produce AddressSizeFault during translation.
    void setInputAddressSize(uint8_t bits);

    // Access Flag and Dirty State management (ARM §3.13)
    // Updates AF/dirty bits on page entry after successful translation.
    // ha=true: set accessFlag on first access
    // hd=true: set dirty on write access
    // Returns true if any flag was updated.
    bool updateAccessFlags(IOVA iova, bool ha, bool hd, AccessType accessType);

    // Query AF/dirty state of a page entry (for testing/inspection)
    bool getPageAccessFlag(IOVA iova) const;
    bool getPageDirty(IOVA iova) const;

    // Cache invalidation mechanisms
    void invalidateRange(IOVA startIova, IOVA endIova);
    void invalidateAll();
    void invalidateCache();
    void invalidatePage(IOVA iova);
    
private:
    // Sparse page table using hash map for efficiency
    std::unordered_map<uint64_t, PageEntry> pageTable;

    // Per-context input address size (ARM §3.4.1, TCR.T0SZ).
    // Default 52 — allows any IOVA up to MAX_VIRTUAL_ADDRESS.
    uint8_t inputAddressSizeBits;
    
    // Helper methods
    uint64_t pageNumber(IOVA iova) const;
    bool checkPermissions(const PagePermissions& perms, AccessType accessType) const;
};

} // namespace smmu

#endif // SMMU_ADDRESS_SPACE_H