// ARM SMMU v3 Address Space Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/address_space.h"
#include <algorithm>  // Required for std::sort in getMappedRanges()

namespace smmu {

// Constructor - initializes empty sparse page table
AddressSpace::AddressSpace() {
    // Empty sparse page table - no initialization required for std::unordered_map
    // This provides efficient O(1) average case lookups with minimal memory overhead
}

// Destructor - automatic cleanup via RAII
AddressSpace::~AddressSpace() {
    // std::unordered_map automatically cleans up all PageEntry objects
    // No manual cleanup required due to RAII design
}

// Copy constructor - deep copy of page table for C++11 compliance
AddressSpace::AddressSpace(const AddressSpace& other) 
    : pageTable(other.pageTable) {
    // std::unordered_map copy constructor performs deep copy of all entries
    // Each PageEntry is copied, maintaining independent page tables
}

// Assignment operator - safe copy with self-assignment protection
AddressSpace& AddressSpace::operator=(const AddressSpace& other) {
    if (this != &other) {
        pageTable = other.pageTable;  // Deep copy via map assignment
    }
    return *this;
}

// Map a page with specified permissions
// Implements sparse page table storage for ARM SMMU v3 address translation
VoidResult AddressSpace::mapPage(IOVA iova, PA pa, const PagePermissions& permissions, SecurityState securityState) {
    // Validate IOVA is within supported address space (ARM SMMU v3 supports up to 52-bit)
    if (iova > 0x000FFFFFFFFFFFFFULL) {  // 52-bit address space limit
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Validate physical address is within reasonable bounds (52-bit PA space)
    if (pa > 0x000FFFFFFFFFFFFFULL) {
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Validate permissions are at least partially set (not completely empty)
    if (!permissions.read && !permissions.write && !permissions.execute) {
        return makeVoidError(SMMUError::InvalidPermissions);
    }
    
    // Validate security state is within valid range
    if (securityState != SecurityState::NonSecure && 
        securityState != SecurityState::Secure && 
        securityState != SecurityState::Realm) {
        return makeVoidError(SMMUError::InvalidSecurityState);
    }
    
    // Convert IOVA to page-aligned page number for sparse indexing
    uint64_t pageNum = pageNumber(iova);
    
    // Create new page entry with physical address and permissions
    // ARM SMMU v3 spec: Each page entry contains PA, permissions, and validity
    PageEntry entry(pa & ~PAGE_MASK, permissions, securityState);  // Align PA to page boundary
    entry.valid = true;
    
    // Insert or update entry in sparse page table
    // Using [] operator allows both insertion and update operations
    pageTable[pageNum] = entry;
    
    // Note: TLB cache integration is handled at higher levels (SMMU/StreamContext)
    // AddressSpace maintains the authoritative page table mapping
    return makeVoidSuccess();
}

// Unmap a page and clean up resources
VoidResult AddressSpace::unmapPage(IOVA iova) {
    // Validate IOVA is within supported address space (ARM SMMU v3 supports up to 52-bit)
    if (iova > 0x000FFFFFFFFFFFFFULL) {  // 52-bit address space limit
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    uint64_t pageNum = pageNumber(iova);
    
    // Check if page is actually mapped before attempting to unmap
    auto it = pageTable.find(pageNum);
    if (it == pageTable.end() || !it->second.valid) {
        // ARM SMMU v3 spec: Unmapping non-existent page can be considered an error
        return makeVoidError(SMMUError::PageNotMapped);
    }
    
    // Remove entry from sparse page table
    // ARM SMMU v3 spec: Unmapping should clean up translation state
    pageTable.erase(pageNum);
    
    // Note: TLB invalidation is coordinated at higher levels
    // AddressSpace focuses on maintaining authoritative mapping state
    return makeVoidSuccess();
}

// Translate virtual address to physical address with ARM SMMU v3 semantics
TranslationResult AddressSpace::translatePage(IOVA iova, AccessType accessType, SecurityState securityState) const {
    uint64_t pageNum = pageNumber(iova);
    
    // Look up page entry in sparse page table
    auto it = pageTable.find(pageNum);
    if (it == pageTable.end()) {
        // ARM SMMU v3 fault: Translation fault when no mapping exists
        return makeTranslationError(FaultType::TranslationFault);
    }
    
    const PageEntry& entry = it->second;
    
    // Verify page entry is valid
    if (!entry.valid) {
        return makeTranslationError(FaultType::TranslationFault);
    }
    
    // Security state validation - ensure page security matches request
    if (entry.securityState != securityState) {
        // Security fault: requested security state doesn't match page security state
        return makeTranslationError(FaultType::SecurityFault);
    }
    
    // Check access permissions according to ARM SMMU v3 specification
    if (!checkPermissions(entry.permissions, accessType)) {
        // ARM SMMU v3 fault: Permission fault when access not allowed
        return makeTranslationError(FaultType::PermissionFault);
    }
    
    // Successful translation - combine page PA with offset
    uint64_t pageOffset = iova & PAGE_MASK;
    PA translatedPA = entry.physicalAddress + pageOffset;
    
    // Create successful translation result
    return makeTranslationSuccess(translatedPA, entry.permissions, entry.securityState);
}

// Query if a specific page is mapped
Result<bool> AddressSpace::isPageMapped(IOVA iova) const {
    // Validate IOVA is within supported address space (ARM SMMU v3 supports up to 52-bit)
    if (iova > 0x000FFFFFFFFFFFFFULL) {  // 52-bit address space limit
        return makeError<bool>(SMMUError::InvalidAddress);
    }
    
    try {
        uint64_t pageNum = pageNumber(iova);
        auto it = pageTable.find(pageNum);
        bool mapped = (it != pageTable.end() && it->second.valid);
        return Result<bool>(mapped);
    } catch (...) {
        return makeError<bool>(SMMUError::InternalError);
    }
}

// Get permissions for a mapped page
Result<PagePermissions> AddressSpace::getPagePermissions(IOVA iova) const {
    // Validate IOVA is within supported address space (ARM SMMU v3 supports up to 52-bit)
    if (iova > 0x000FFFFFFFFFFFFFULL) {  // 52-bit address space limit
        return makeError<PagePermissions>(SMMUError::InvalidAddress);
    }
    
    try {
        uint64_t pageNum = pageNumber(iova);
        auto it = pageTable.find(pageNum);
        
        if (it != pageTable.end() && it->second.valid) {
            return Result<PagePermissions>(it->second.permissions);
        }
        
        // Page not mapped
        return makeError<PagePermissions>(SMMUError::PageNotMapped);
    } catch (...) {
        return makeError<PagePermissions>(SMMUError::InternalError);
    }
}

// Get count of mapped pages for statistics and management
Result<size_t> AddressSpace::getPageCount() const {
    try {
        // Count only valid entries in sparse page table
        size_t count = 0;
        for (const auto& pair : pageTable) {
            if (pair.second.valid) {
                count++;
                // Check for potential overflow
                if (count == SIZE_MAX) {
                    return makeError<size_t>(SMMUError::InternalError);
                }
            }
        }
        return Result<size_t>(count);
    } catch (...) {
        return makeError<size_t>(SMMUError::InternalError);
    }
}

// Clear all page mappings
VoidResult AddressSpace::clear() {
    // Clear entire sparse page table
    // ARM SMMU v3 spec: Complete invalidation of translation context
    pageTable.clear();
    
    // Clear operation should always succeed for in-memory data structures
    return makeVoidSuccess();
}

// Invalidate entire address space cache (for higher-level TLB coordination)
void AddressSpace::invalidateCache() {
    // AddressSpace maintains authoritative state - actual TLB cache
    // invalidation is coordinated at StreamContext/SMMU levels
    // This method provides interface consistency for future TLB integration
}

// Invalidate specific page cache entry
void AddressSpace::invalidatePage(IOVA iova) {
    // Similar to invalidateCache - provides interface for TLB coordination
    // Actual cache invalidation happens at higher levels with proper
    // StreamID/PASID context for multi-level cache management
    (void)iova;  // Suppress unused parameter warning in C++11
}

// Map an address range with contiguous physical addresses
// ARM SMMU v3 spec: Efficient mapping of large contiguous regions
VoidResult AddressSpace::mapRange(IOVA startIova, IOVA endIova, PA startPa, const PagePermissions& permissions) {
    // Validate input parameters - range must be valid
    if (endIova < startIova) {
        return makeVoidError(SMMUError::InvalidAddress);  // Invalid range - end before start
    }
    
    // Validate addresses are within supported address space (ARM SMMU v3 supports up to 52-bit)
    if (startIova > 0x000FFFFFFFFFFFFFULL || endIova > 0x000FFFFFFFFFFFFFULL) {
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Validate physical address is within reasonable bounds
    if (startPa > 0x000FFFFFFFFFFFFFULL) {
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Validate permissions are at least partially set
    if (!permissions.read && !permissions.write && !permissions.execute) {
        return makeVoidError(SMMUError::InvalidPermissions);
    }
    
    // Check for potential overflow in range calculation
    uint64_t rangeSize = endIova - startIova + 1;
    if (startPa + rangeSize < startPa) {  // Overflow check
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Align start addresses to page boundaries
    IOVA alignedStartIova = startIova & ~PAGE_MASK;
    PA alignedStartPa = startPa & ~PAGE_MASK;
    
    // Calculate range in pages, ensuring we cover the entire requested range
    uint64_t startPageNum = pageNumber(alignedStartIova);
    uint64_t endPageNum = pageNumber(endIova);
    
    // Map each page in the range with contiguous physical addresses
    PA currentPa = alignedStartPa;
    for (uint64_t pageNum = startPageNum; pageNum <= endPageNum; ++pageNum) {
        // Create page entry with current physical address and permissions
        PageEntry entry(currentPa, permissions);
        entry.valid = true;
        
        // Insert into sparse page table
        pageTable[pageNum] = entry;
        
        // Advance to next page physical address
        currentPa += PAGE_SIZE;
    }
    
    return makeVoidSuccess();
}

// Unmap an address range
// ARM SMMU v3 spec: Efficient unmapping of contiguous regions
VoidResult AddressSpace::unmapRange(IOVA startIova, IOVA endIova) {
    // Validate input parameters
    if (endIova < startIova) {
        return makeVoidError(SMMUError::InvalidAddress);  // Invalid range - end before start
    }
    
    // Validate addresses are within supported address space
    if (startIova > 0x000FFFFFFFFFFFFFULL || endIova > 0x000FFFFFFFFFFFFFULL) {
        return makeVoidError(SMMUError::InvalidAddress);
    }
    
    // Calculate page numbers for the range
    uint64_t startPageNum = pageNumber(startIova);
    uint64_t endPageNum = pageNumber(endIova);
    
    // Check if any pages in the range are actually mapped
    bool anyMapped = false;
    for (uint64_t pageNum = startPageNum; pageNum <= endPageNum; ++pageNum) {
        auto it = pageTable.find(pageNum);
        if (it != pageTable.end() && it->second.valid) {
            anyMapped = true;
            break;
        }
    }
    
    if (!anyMapped) {
        // ARM SMMU v3 spec: Unmapping non-existent pages can be considered an error
        return makeVoidError(SMMUError::PageNotMapped);
    }
    
    // Unmap each page in the range
    for (uint64_t pageNum = startPageNum; pageNum <= endPageNum; ++pageNum) {
        pageTable.erase(pageNum);
    }
    
    return makeVoidSuccess();
}

// Map multiple pages efficiently with same permissions
// ARM SMMU v3 spec: Bulk operations for performance optimization
VoidResult AddressSpace::mapPages(const std::vector<std::pair<IOVA, PA>>& mappings, const PagePermissions& permissions) {
    // Validate permissions are at least partially set
    if (!permissions.read && !permissions.write && !permissions.execute) {
        return makeVoidError(SMMUError::InvalidPermissions);
    }
    
    // Validate all mappings before processing any
    for (const auto& mapping : mappings) {
        IOVA iova = mapping.first;
        PA pa = mapping.second;
        
        // Validate IOVA is within supported address space
        if (iova > 0x000FFFFFFFFFFFFFULL) {
            return makeVoidError(SMMUError::InvalidAddress);
        }
        
        // Validate physical address is within reasonable bounds
        if (pa > 0x000FFFFFFFFFFFFFULL) {
            return makeVoidError(SMMUError::InvalidAddress);
        }
    }
    
    // All validation passed - now process all mappings
    for (const auto& mapping : mappings) {
        IOVA iova = mapping.first;
        PA pa = mapping.second;
        
        // Convert to page number and align physical address
        uint64_t pageNum = pageNumber(iova);
        PA alignedPa = pa & ~PAGE_MASK;
        
        // Create and insert page entry
        PageEntry entry(alignedPa, permissions);
        entry.valid = true;
        pageTable[pageNum] = entry;
    }
    
    return makeVoidSuccess();
}

// Unmap multiple pages efficiently
// ARM SMMU v3 spec: Bulk unmapping for performance optimization
VoidResult AddressSpace::unmapPages(const std::vector<IOVA>& iovas) {
    // Validate all IOVAs before processing any
    for (IOVA iova : iovas) {
        // Validate IOVA is within supported address space
        if (iova > 0x000FFFFFFFFFFFFFULL) {
            return makeVoidError(SMMUError::InvalidAddress);
        }
    }
    
    // Check if at least some of the pages are actually mapped
    bool anyMapped = false;
    for (IOVA iova : iovas) {
        uint64_t pageNum = pageNumber(iova);
        auto it = pageTable.find(pageNum);
        if (it != pageTable.end() && it->second.valid) {
            anyMapped = true;
            break;
        }
    }
    
    if (!anyMapped) {
        // ARM SMMU v3 spec: Unmapping non-existent pages can be considered an error
        return makeVoidError(SMMUError::PageNotMapped);
    }
    
    // All validation passed - now unmap all pages
    for (IOVA iova : iovas) {
        uint64_t pageNum = pageNumber(iova);
        pageTable.erase(pageNum);
    }
    
    return makeVoidSuccess();
}

// Get all mapped address ranges in sorted order
// ARM SMMU v3 spec: Address space introspection for management
std::vector<AddressRange> AddressSpace::getMappedRanges() const {
    std::vector<AddressRange> ranges;
    
    if (pageTable.empty()) {
        return ranges;  // No mappings exist
    }
    
    // Collect all valid page numbers and sort them
    std::vector<uint64_t> sortedPageNums;
    sortedPageNums.reserve(pageTable.size());
    
    for (const auto& pair : pageTable) {
        if (pair.second.valid) {
            sortedPageNums.push_back(pair.first);
        }
    }
    
    // Sort page numbers for range consolidation (C++11 compatible)
    std::sort(sortedPageNums.begin(), sortedPageNums.end());
    
    if (sortedPageNums.empty()) {
        return ranges;  // No valid mappings
    }
    
    // Consolidate consecutive pages into ranges
    IOVA rangeStart = sortedPageNums[0] << 12;  // Convert page number to address
    IOVA rangeEnd = rangeStart + PAGE_SIZE - 1;
    
    for (size_t i = 1; i < sortedPageNums.size(); ++i) {
        IOVA currentPageAddr = sortedPageNums[i] << 12;
        
        // Check if this page is consecutive with current range
        if (currentPageAddr == rangeEnd + 1) {
            // Extend current range
            rangeEnd = currentPageAddr + PAGE_SIZE - 1;
        } else {
            // Non-consecutive page - complete current range and start new one
            ranges.push_back(AddressRange(rangeStart, rangeEnd));
            rangeStart = currentPageAddr;
            rangeEnd = rangeStart + PAGE_SIZE - 1;
        }
    }
    
    // Add final range
    ranges.push_back(AddressRange(rangeStart, rangeEnd));
    
    return ranges;
}

// Get total address space size covered by mappings
// ARM SMMU v3 spec: Address space utilization metrics
uint64_t AddressSpace::getAddressSpaceSize() const {
    if (pageTable.empty()) {
        return 0;
    }
    
    // Find minimum and maximum page numbers with valid entries
    uint64_t minPageNum = UINT64_MAX;
    uint64_t maxPageNum = 0;
    bool hasValidEntries = false;
    
    for (const auto& pair : pageTable) {
        if (pair.second.valid) {
            hasValidEntries = true;
            uint64_t pageNum = pair.first;
            if (pageNum < minPageNum) {
                minPageNum = pageNum;
            }
            if (pageNum > maxPageNum) {
                maxPageNum = pageNum;
            }
        }
    }
    
    if (!hasValidEntries) {
        return 0;
    }
    
    // Calculate address space size from min to max page
    uint64_t minAddress = minPageNum << 12;
    uint64_t maxAddress = (maxPageNum << 12) + PAGE_SIZE - 1;
    
    return maxAddress - minAddress + 1;
}

// Check for overlapping mappings in specified range
// ARM SMMU v3 spec: Conflict detection for mapping operations
bool AddressSpace::hasOverlappingMappings(IOVA startIova, IOVA endIova) const {
    // Validate input parameters
    if (endIova < startIova) {
        return false;  // Invalid range
    }
    
    // Calculate page numbers for the range
    uint64_t startPageNum = pageNumber(startIova);
    uint64_t endPageNum = pageNumber(endIova);
    
    // Check each page in the range for existing mappings
    for (uint64_t pageNum = startPageNum; pageNum <= endPageNum; ++pageNum) {
        auto it = pageTable.find(pageNum);
        if (it != pageTable.end() && it->second.valid) {
            return true;  // Found overlapping mapping
        }
    }
    
    return false;  // No overlapping mappings found
}

// Invalidate cache entries for specified address range
// ARM SMMU v3 spec: Selective cache invalidation for performance
void AddressSpace::invalidateRange(IOVA startIova, IOVA endIova) {
    // AddressSpace maintains authoritative state - actual cache invalidation
    // is coordinated at higher levels (StreamContext/SMMU) with proper
    // StreamID/PASID context for multi-level TLB cache management
    //
    // This method provides interface consistency for future integration
    // with hardware TLB invalidation mechanisms
    
    // Suppress unused parameter warnings in C++11
    (void)startIova;
    (void)endIova;
}

// Invalidate all cache entries for this address space
// ARM SMMU v3 spec: Complete cache invalidation
void AddressSpace::invalidateAll() {
    // Similar to invalidateRange - provides interface for coordinated
    // TLB cache invalidation at higher levels where StreamID/PASID
    // context is available for proper multi-level cache management
    //
    // Future implementations may trigger hardware-specific invalidation
    // commands through the SMMU controller interface
}

// Convert IOVA to page number for sparse indexing
// ARM SMMU v3 spec: 4KB page granularity with 64-bit address space
uint64_t AddressSpace::pageNumber(IOVA iova) const {
    // Right shift by page size bits (12 for 4KB pages)
    // This provides efficient sparse indexing for large address spaces
    return iova >> 12;  // PAGE_SIZE = 4096 = 2^12
}

// Check if requested access type is permitted by page permissions
// ARM SMMU v3 specification permission checking semantics
bool AddressSpace::checkPermissions(const PagePermissions& perms, AccessType accessType) const {
    switch (accessType) {
        case AccessType::Read:
            return perms.read;
            
        case AccessType::Write:
            return perms.write;
            
        case AccessType::Execute:
            return perms.execute;
            
        default:
            // Unknown access type - deny by default for security
            return false;
    }
}

} // namespace smmu