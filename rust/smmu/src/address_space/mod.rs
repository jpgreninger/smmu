//! Address space management and page table operations
//!
//! This module implements the core address translation functionality, including:
//!
//! - Page table management with sparse representation
//! - Virtual to physical address translation
//! - Permission checking and enforcement
//! - Multi-level page table walks
//!
//! # Design
//!
//! The address space uses sparse data structures (HashMap-based) to efficiently
//! handle large address spaces without wasting memory on unmapped regions.
//!
//! # Performance
//!
//! Translation operations target O(1) average case performance through efficient
//! caching and hash-based lookups.
//!
//! # Thread Safety
//!
//! The AddressSpace struct is `Send + Sync` safe and can be shared across threads
//! when wrapped in appropriate synchronization primitives like `Arc<RwLock<>>`.

#![warn(missing_docs)]

use crate::types::{
    AccessType, PageEntry, PagePermissions, SecurityState, TranslationData, TranslationError, TranslationResult, IOVA,
    PA, PAGE_SIZE,
};
use dashmap::DashMap;
use smallvec::SmallVec;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

/// Maximum valid virtual address (52-bit address space per ARM SMMU v3 spec)
const MAX_VIRTUAL_ADDRESS: u64 = (1u64 << 52) - 1;

/// Maximum valid physical address (52-bit PA space per ARM SMMU v3 spec)
const MAX_PHYSICAL_ADDRESS: u64 = (1u64 << 52) - 1;

/// Page alignment mask (lower 12 bits for 4KB pages)
const PAGE_MASK: u64 = PAGE_SIZE - 1;

/// AddressSpace operation error types
///
/// Comprehensive error enumeration for all address space operation failure cases.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum AddressSpaceError {
    /// Invalid address provided
    #[error("Invalid address: 0x{address:x}")]
    InvalidAddress {
        /// The invalid address value
        address: u64,
    },

    /// Invalid permissions
    #[error("Invalid permissions - at least one permission must be set")]
    InvalidPermissions,

    /// Invalid security state
    #[error("Invalid security state")]
    InvalidSecurityState,

    /// Page not mapped
    #[error("Page not mapped")]
    PageNotMapped,

    /// Internal error
    #[error("Internal error")]
    InternalError,
}

/// Address range structure for query operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressRange {
    /// Start address of the range
    pub start: IOVA,
    /// End address of the range (inclusive)
    pub end: IOVA,
}

impl AddressRange {
    /// Creates a new AddressRange
    #[must_use]
    pub const fn new(start: IOVA, end: IOVA) -> Self {
        Self { start, end }
    }
}

/// Iterator over pages in an AddressRange
#[derive(Debug)]
pub struct AddressRangeIterator {
    current: u64,
    end: u64,
}

impl Iterator for AddressRangeIterator {
    type Item = PageInfo;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current <= self.end {
            let iova = IOVA::new(self.current).ok()?;
            let page_number = self.current >> 12;
            self.current += PAGE_SIZE;
            Some(PageInfo { iova, page_number })
        } else {
            None
        }
    }
}

impl IntoIterator for AddressRange {
    type Item = PageInfo;
    type IntoIter = AddressRangeIterator;

    fn into_iter(self) -> Self::IntoIter {
        AddressRangeIterator {
            current: self.start.as_u64() & !(PAGE_SIZE - 1),
            end: self.end.as_u64(),
        }
    }
}

/// Information about a page in an address range
#[derive(Debug, Clone, Copy)]
pub struct PageInfo {
    iova: IOVA,
    page_number: u64,
}

impl PageInfo {
    /// Returns the IOVA for this page
    #[must_use]
    pub const fn iova(&self) -> IOVA {
        self.iova
    }

    /// Returns the page number
    #[must_use]
    pub const fn page_number(&self) -> u64 {
        self.page_number
    }
}

/// Reference to a page entry with its IOVA
#[derive(Debug, Clone)]
pub struct PageEntryRef {
    iova: IOVA,
    entry: PageEntry,
}

impl PageEntryRef {
    /// Returns the IOVA
    #[must_use]
    pub const fn iova(&self) -> IOVA {
        self.iova
    }

    /// Returns the page entry
    #[must_use]
    pub const fn entry(&self) -> &PageEntry {
        &self.entry
    }

    /// Returns the physical address
    #[must_use]
    pub const fn physical_address(&self) -> PA {
        self.entry.physical_address()
    }

    /// Returns the permissions
    #[must_use]
    pub const fn permissions(&self) -> PagePermissions {
        self.entry.permissions()
    }

    /// Returns the security state
    #[must_use]
    pub const fn security_state(&self) -> SecurityState {
        self.entry.security_state()
    }
}

/// Mutable reference to a page entry with its IOVA
#[derive(Debug)]
pub struct PageEntryMutRef<'a> {
    iova: IOVA,
    entry: &'a mut PageEntry,
}

impl<'a> PageEntryMutRef<'a> {
    /// Returns the IOVA
    #[must_use]
    pub const fn iova(&self) -> IOVA {
        self.iova
    }

    /// Sets new permissions on the entry
    pub fn set_permissions(&mut self, perms: PagePermissions) {
        *self.entry = self.entry.clone().with_permissions(perms);
    }

    /// Returns the current permissions
    #[must_use]
    pub fn permissions(&self) -> PagePermissions {
        self.entry.permissions()
    }
}

/// Statistics for an address range
#[derive(Debug, Clone, Copy, Default)]
pub struct RangeStats {
    /// Total number of pages in the range
    pub total_pages: usize,
    /// Number of readable pages
    pub readable_pages: usize,
    /// Number of writable pages
    pub writable_pages: usize,
    /// Number of executable pages
    pub executable_pages: usize,
}

/// Immutable query interface for AddressSpace
#[derive(Debug)]
pub struct AddressSpaceQuery<'a> {
    addr_space: &'a AddressSpace,
}

impl<'a> AddressSpaceQuery<'a> {
    /// Returns the number of mapped pages
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.addr_space.page_table.len()
    }

    /// Checks if a page is mapped
    #[must_use]
    pub fn is_mapped(&self, iova: IOVA) -> bool {
        let page_num = self.addr_space.page_number(iova);
        self.addr_space.page_table.contains_key(&page_num)
    }

    /// Returns all mapped pages as a vector
    pub fn get_all_entries(&self) -> Vec<PageEntry> {
        // DashMap doesn't have values() - collect from iter()
        self.addr_space.page_table.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Returns statistics for a given address range
    #[must_use]
    pub fn range_statistics(&self, start: IOVA, end: IOVA) -> RangeStats {
        let start_page = self.addr_space.page_number(start);
        let end_page = self.addr_space.page_number(end);

        let mut stats = RangeStats::default();

        for page_num in start_page..=end_page {
            if let Some(entry_ref) = self.addr_space.page_table.get(&page_num) {
                let entry = entry_ref.value();
                stats.total_pages += 1;
                if entry.permissions().read() {
                    stats.readable_pages += 1;
                }
                if entry.permissions().write() {
                    stats.writable_pages += 1;
                }
                if entry.permissions().execute() {
                    stats.executable_pages += 1;
                }
            }
        }

        stats
    }
}

/// AddressSpace - Sparse page table for ARM SMMU v3 address translation
///
/// Implements efficient address translation with O(1) average case lookups using
/// HashMap-based sparse page table representation. This design minimizes memory
/// usage for large address spaces with sparse mappings.
///
/// # Thread Safety
///
/// AddressSpace is `Send + Sync` and can be safely shared across threads when
/// wrapped in `Arc<RwLock<>>` or similar synchronization primitives.
///
/// # Examples
///
/// ```
/// use smmu::address_space::AddressSpace;
/// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, AccessType};
///
/// let mut addr_space = AddressSpace::new();
/// let iova = IOVA::new(0x1000).unwrap();
/// let pa = PA::new(0x2000).unwrap();
/// let perms = PagePermissions::read_write();
///
/// // Map a page
/// addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
///
/// // Translate the address
/// let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
/// assert_eq!(result.physical_address().as_u64(), 0x2000);
/// ```
#[derive(Debug)]
pub struct AddressSpace {
    /// Sparse page table using page number as key (lock-free with DashMap)
    page_table: DashMap<u64, PageEntry>,
    /// Invalidation generation counter for cache coherency
    invalidation_generation: AtomicU64,
    /// Per-page invalidation tracking (lock-free with DashMap)
    invalidation_map: DashMap<u64, AtomicU64>,
}

impl Clone for AddressSpace {
    fn clone(&self) -> Self {
        // Clone DashMap entries
        let new_invalidation_map = DashMap::new();
        for entry in &self.invalidation_map {
            new_invalidation_map.insert(*entry.key(), AtomicU64::new(entry.value().load(Ordering::Acquire)));
        }

        Self {
            page_table: self.page_table.clone(),
            invalidation_generation: AtomicU64::new(self.invalidation_generation.load(Ordering::Acquire)),
            invalidation_map: new_invalidation_map,
        }
    }
}

impl AddressSpace {
    /// Creates a new empty AddressSpace
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    ///
    /// let addr_space = AddressSpace::new();
    /// assert_eq!(addr_space.get_page_count().unwrap(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            page_table: DashMap::new(),
            invalidation_generation: AtomicU64::new(0),
            invalidation_map: DashMap::new(),
        }
    }

    /// Creates a new AddressSpace with pre-allocated capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Initial capacity for the page table
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    ///
    /// let addr_space = AddressSpace::with_capacity(1000);
    /// ```
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            page_table: DashMap::with_capacity(capacity),
            invalidation_generation: AtomicU64::new(0),
            invalidation_map: DashMap::with_capacity(capacity),
        }
    }

    /// Maps a page with specified permissions and security state
    ///
    /// If the page is already mapped, the existing mapping is overwritten.
    ///
    /// # Arguments
    ///
    /// * `iova` - Input/Output Virtual Address to map
    /// * `pa` - Physical Address to map to
    /// * `permissions` - Page permissions (read/write/execute)
    /// * `security_state` - Security state (Secure/NonSecure/Realm)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - IOVA exceeds maximum supported address
    /// - PA exceeds maximum supported address
    /// - Permissions are completely empty (none set)
    /// - Security state is invalid
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// ```
    pub fn map_page(
        &self,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Validate IOVA is within supported address space
        if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
        }

        // Validate PA is within supported address space
        if pa.as_u64() > MAX_PHYSICAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: pa.as_u64() });
        }

        // Validate permissions - at least one must be set
        if !permissions.read() && !permissions.write() && !permissions.execute() {
            return Err(AddressSpaceError::InvalidPermissions);
        }

        // Convert IOVA to page number
        let page_num = self.page_number(iova);

        // Align PA to page boundary
        let aligned_pa = PA::new(pa.as_u64() & !PAGE_MASK).unwrap();

        // Create page entry — accessFlag=true means "ready-to-use mapping" (§3.13: OS has
        // already set AF=1; this is the normal state for pages in active use).
        // Use map_page_unaccessed() to create entries with AF=false for AF-management tests.
        let entry = PageEntry::with_security_state(aligned_pa, permissions, security_state)
            .with_access_flag(true);

        // Insert or update entry in page table
        self.page_table.insert(page_num, entry);

        Ok(())
    }

    /// Unmaps a page from the address space
    ///
    /// # Arguments
    ///
    /// * `iova` - Input/Output Virtual Address to unmap
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - IOVA exceeds maximum supported address
    /// - Page is not currently mapped
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// addr_space.unmap_page(iova).unwrap();
    /// ```
    pub fn unmap_page(&self, iova: IOVA) -> Result<(), AddressSpaceError> {
        // Validate IOVA is within supported address space
        if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
        }

        let page_num = self.page_number(iova);

        // Check if page is actually mapped
        if !self.page_table.contains_key(&page_num) {
            return Err(AddressSpaceError::PageNotMapped);
        }

        // Remove entry from page table
        self.page_table.remove(&page_num);

        Ok(())
    }

    /// Maps a page in "unaccessed" state (accessFlag=false), for AF-management tests (§3.13).
    ///
    /// Use this when simulating an OS creating a fresh page table entry that has not yet
    /// been accessed (AF=0). When `ha=false` and `affd=false`, a translate on this page
    /// will generate F_ACCESS per §3.13.2. Normal test code should use `map_page()`.
    pub fn map_page_unaccessed(
        &self,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        self.map_page(iova, pa, permissions, security_state)?;
        let page_num = self.page_number(iova);
        if let Some(mut entry) = self.page_table.get_mut(&page_num) {
            *entry = entry.clone().with_access_flag(false);
        }
        Ok(())
    }

    /// Maps a page as Device memory type (for S2PTW testing, §5.2 STE.S2PTW).
    ///
    /// Creates a ready-to-use mapping (accessFlag=true) marked as Device memory.
    /// When a two-stage stream has S2PTW=1, translating through this stage-2 page
    /// will generate F_PERMISSION per §5.2.
    pub fn map_page_device(
        &self,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        self.map_page(iova, pa, permissions, security_state)?;
        let page_num = self.page_number(iova);
        if let Some(mut entry) = self.page_table.get_mut(&page_num) {
            *entry = entry.clone().with_device_memory(true);
        }
        Ok(())
    }

    /// Returns true if the mapped page is Device memory type; false if not mapped or Normal memory.
    pub fn get_page_device_memory(&self, iova: IOVA) -> bool {
        let page_num = self.page_number(iova);
        self.page_table.get(&page_num).map_or(false, |e| e.is_device_memory())
    }

    /// Translates a virtual address to physical address with permission checking
    ///
    /// # Arguments
    ///
    /// * `iova` - Input/Output Virtual Address to translate
    /// * `access_type` - Type of access (Read/Write/Execute)
    /// * `security_state` - Security state for the access
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Page is not mapped (`TranslationError::PageNotMapped`)
    /// - Permissions do not allow the access type (`TranslationError::PermissionViolation`)
    /// - Security state does not match (`TranslationError::SecurityViolation`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, AccessType};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    ///
    /// let result = addr_space.translate_page(iova, AccessType::Read, SecurityState::NonSecure).unwrap();
    /// assert_eq!(result.physical_address().as_u64(), 0x2000);
    /// ```
    pub fn translate_page(
        &self,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        let page_num = self.page_number(iova);

        // Look up page entry
        let entry = self.page_table.get(&page_num).ok_or(TranslationError::PageNotMapped)?;

        // Verify entry is valid
        if !entry.is_valid() {
            return Err(TranslationError::PageNotMapped);
        }

        // Check security state
        if entry.security_state() != security_state {
            return Err(TranslationError::SecurityViolation);
        }

        // Check permissions
        if !self.check_permissions(entry.permissions(), access_type) {
            return Err(TranslationError::PermissionViolation { access: access_type });
        }

        // Calculate physical address with offset preserved
        let page_offset = iova.as_u64() & PAGE_MASK;
        let translated_pa = PA::new(entry.physical_address().as_u64() + page_offset).unwrap();

        // BUG-AUDIT-49 fix: §9.1.4 / §6.3.40 — carry per-page ATTR from translation tables.
        // Device-nGnRnE pages return ATTR=0x00; Normal WB/WA pages return ATTR=0xFF.
        let mut data = TranslationData::new(translated_pa, entry.permissions(), entry.security_state());
        if entry.is_device_memory() {
            data.page_attr = 0x00; // Device-nGnRnE
        }
        // Return successful translation
        Ok(data)
    }

    /// Checks if a page is mapped
    ///
    /// # Arguments
    ///
    /// * `iova` - Input/Output Virtual Address to check
    ///
    /// # Errors
    ///
    /// Returns error if IOVA exceeds maximum supported address.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    ///
    /// assert!(!addr_space.is_page_mapped(iova).unwrap());
    ///
    /// let pa = PA::new(0x2000).unwrap();
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    ///
    /// assert!(addr_space.is_page_mapped(iova).unwrap());
    /// ```
    pub fn is_page_mapped(&self, iova: IOVA) -> Result<bool, AddressSpaceError> {
        // Validate IOVA is within supported address space
        if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
        }

        let page_num = self.page_number(iova);
        Ok(self.page_table.contains_key(&page_num))
    }

    /// Gets the permissions for a mapped page
    ///
    /// # Arguments
    ///
    /// * `iova` - Input/Output Virtual Address to query
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - IOVA exceeds maximum supported address
    /// - Page is not mapped
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_execute();
    ///
    /// addr_space.map_page(iova, pa, perms, SecurityState::NonSecure).unwrap();
    ///
    /// let retrieved_perms = addr_space.get_page_permissions(iova).unwrap();
    /// assert_eq!(retrieved_perms, perms);
    /// ```
    pub fn get_page_permissions(&self, iova: IOVA) -> Result<PagePermissions, AddressSpaceError> {
        // Validate IOVA is within supported address space
        if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
        }

        let page_num = self.page_number(iova);

        self.page_table
            .get(&page_num)
            .map(|entry| entry.permissions())
            .ok_or(AddressSpaceError::PageNotMapped)
    }

    /// Gets the count of mapped pages
    ///
    /// # Errors
    ///
    /// Returns error on internal failures (should not occur in normal operation).
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// assert_eq!(addr_space.get_page_count().unwrap(), 0);
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    ///
    /// assert_eq!(addr_space.get_page_count().unwrap(), 1);
    /// ```
    pub fn get_page_count(&self) -> Result<usize, AddressSpaceError> {
        Ok(self.page_table.len())
    }

    /// Clears all page mappings
    ///
    /// # Errors
    ///
    /// This operation should not fail in normal circumstances.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// assert_eq!(addr_space.get_page_count().unwrap(), 1);
    ///
    /// addr_space.clear().unwrap();
    /// assert_eq!(addr_space.get_page_count().unwrap(), 0);
    /// ```
    pub fn clear(&mut self) -> Result<(), AddressSpaceError> {
        self.page_table.clear();
        Ok(())
    }

    /// Maps a contiguous address range
    ///
    /// # Arguments
    ///
    /// * `start_iova` - Start of the virtual address range
    /// * `end_iova` - End of the virtual address range (inclusive)
    /// * `start_pa` - Start of the physical address range
    /// * `permissions` - Page permissions for all pages in the range
    ///
    /// # Errors
    ///
    /// Returns error if any addresses are invalid or permissions are empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let start_iova = IOVA::new(0x1000).unwrap();
    /// let end_iova = IOVA::new(0x1000 + (10 * PAGE_SIZE)).unwrap();
    /// let start_pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_range(start_iova, end_iova, start_pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    /// ```
    pub fn map_range(
        &mut self,
        start_iova: IOVA,
        end_iova: IOVA,
        start_pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Validate range
        if end_iova.as_u64() < start_iova.as_u64() {
            return Err(AddressSpaceError::InvalidAddress { address: end_iova.as_u64() });
        }

        // Validate addresses
        if start_iova.as_u64() > MAX_VIRTUAL_ADDRESS || end_iova.as_u64() > MAX_VIRTUAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: start_iova.as_u64() });
        }

        if start_pa.as_u64() > MAX_PHYSICAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: start_pa.as_u64() });
        }

        // Validate permissions
        if !permissions.read() && !permissions.write() && !permissions.execute() {
            return Err(AddressSpaceError::InvalidPermissions);
        }

        // Align addresses to page boundaries
        let aligned_start_iova = start_iova.as_u64() & !PAGE_MASK;
        let aligned_start_pa = start_pa.as_u64() & !PAGE_MASK;

        // Calculate page numbers
        let start_page_num = self.page_number(IOVA::new(aligned_start_iova).unwrap());
        let end_page_num = self.page_number(end_iova);
        let num_pages = end_page_num.saturating_sub(start_page_num).saturating_add(1);

        // Validate that the PA range won't overflow u64 or exceed MAX_PHYSICAL_ADDRESS
        let last_pa = aligned_start_pa
            .checked_add(num_pages.saturating_sub(1).saturating_mul(PAGE_SIZE))
            .ok_or(AddressSpaceError::InvalidAddress { address: aligned_start_pa })?;
        if last_pa > MAX_PHYSICAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: last_pa });
        }

        // Map each page in the range with checked PA arithmetic to prevent overflow.
        // Set AF=true so that pages are immediately accessible without triggering
        // F_ACCESS faults (§3.13.2: AF=0 + ha=false + affd=false → F_ACCESS).
        let mut current_pa = aligned_start_pa;
        for page_num in start_page_num..=end_page_num {
            let entry = PageEntry::with_security_state(PA::new(current_pa).unwrap(), permissions, security_state)
                .with_access_flag(true);
            self.page_table.insert(page_num, entry);
            current_pa = current_pa
                .checked_add(PAGE_SIZE)
                .ok_or(AddressSpaceError::InvalidAddress { address: current_pa })?;
        }

        Ok(())
    }

    /// Unmaps a contiguous address range
    ///
    /// # Arguments
    ///
    /// * `start_iova` - Start of the virtual address range
    /// * `end_iova` - End of the virtual address range (inclusive)
    ///
    /// # Errors
    ///
    /// Returns error if addresses are invalid or no pages in the range are mapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let start_iova = IOVA::new(0x1000).unwrap();
    /// let end_iova = IOVA::new(0x1000 + (10 * PAGE_SIZE)).unwrap();
    /// let start_pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_range(start_iova, end_iova, start_pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// addr_space.unmap_range(start_iova, end_iova).unwrap();
    /// ```
    pub fn unmap_range(&mut self, start_iova: IOVA, end_iova: IOVA) -> Result<(), AddressSpaceError> {
        // Validate range
        if end_iova.as_u64() < start_iova.as_u64() {
            return Err(AddressSpaceError::InvalidAddress { address: end_iova.as_u64() });
        }

        // Validate addresses
        if start_iova.as_u64() > MAX_VIRTUAL_ADDRESS || end_iova.as_u64() > MAX_VIRTUAL_ADDRESS {
            return Err(AddressSpaceError::InvalidAddress { address: start_iova.as_u64() });
        }

        // Calculate page numbers
        let start_page_num = self.page_number(start_iova);
        let end_page_num = self.page_number(end_iova);

        // Check if any pages in the range are mapped
        let any_mapped = (start_page_num..=end_page_num).any(|page_num| self.page_table.contains_key(&page_num));

        if !any_mapped {
            return Err(AddressSpaceError::PageNotMapped);
        }

        // Unmap each page in the range
        for page_num in start_page_num..=end_page_num {
            self.page_table.remove(&page_num);
        }

        Ok(())
    }

    /// Maps multiple pages in bulk
    ///
    /// # Arguments
    ///
    /// * `mappings` - Slice of (IOVA, PA) pairs to map
    /// * `permissions` - Page permissions for all mappings
    ///
    /// # Errors
    ///
    /// Returns error if any addresses are invalid or permissions are empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let mappings: Vec<(IOVA, PA)> = (0..10)
    ///     .map(|i| {
    ///         let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
    ///         let pa = PA::new(0x2000 + i * PAGE_SIZE).unwrap();
    ///         (iova, pa)
    ///     })
    ///     .collect();
    ///
    /// addr_space.map_pages(&mappings, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    /// ```
    pub fn map_pages(
        &self,
        mappings: &[(IOVA, PA)],
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Validate permissions
        if !permissions.read() && !permissions.write() && !permissions.execute() {
            return Err(AddressSpaceError::InvalidPermissions);
        }

        // DashMap doesn't need reserve - it auto-resizes efficiently

        // Validate all mappings first
        for &(iova, pa) in mappings {
            if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
            }
            if pa.as_u64() > MAX_PHYSICAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: pa.as_u64() });
            }
        }

        // Map all pages.  Set AF=true so bulk-mapped pages do not trigger spurious
        // F_ACCESS faults (§3.13.2: AF=0 + ha=false + affd=false → F_ACCESS).
        for &(iova, pa) in mappings {
            let page_num = self.page_number(iova);
            let aligned_pa = PA::new(pa.as_u64() & !PAGE_MASK).unwrap();
            let entry = PageEntry::with_security_state(aligned_pa, permissions, security_state)
                .with_access_flag(true);
            self.page_table.insert(page_num, entry);
        }

        Ok(())
    }

    /// Unmaps multiple pages in bulk
    ///
    /// # Arguments
    ///
    /// * `iovas` - Slice of IOVAs to unmap
    ///
    /// # Errors
    ///
    /// Returns error if any addresses are invalid or no pages are mapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iovas: Vec<IOVA> = (0..10)
    ///     .map(|i| IOVA::new(0x1000 + i * PAGE_SIZE).unwrap())
    ///     .collect();
    ///
    /// for iova in &iovas {
    ///     let pa = PA::new(0x2000).unwrap();
    ///     addr_space.map_page(*iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// }
    ///
    /// addr_space.unmap_pages(&iovas).unwrap();
    /// ```
    pub fn unmap_pages(&self, iovas: &[IOVA]) -> Result<(), AddressSpaceError> {
        // Validate all IOVAs first
        for &iova in iovas {
            if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
            }
        }

        // Check if at least some pages are mapped
        let any_mapped = iovas.iter().any(|&iova| self.page_table.contains_key(&self.page_number(iova)));

        if !any_mapped {
            return Err(AddressSpaceError::PageNotMapped);
        }

        // Unmap all pages
        for &iova in iovas {
            let page_num = self.page_number(iova);
            self.page_table.remove(&page_num);
        }

        Ok(())
    }

    /// Gets all mapped address ranges (consolidated into contiguous ranges)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    ///
    /// // Map contiguous range
    /// for i in 0..5 {
    ///     let iova = IOVA::new(0x1000 + i * PAGE_SIZE).unwrap();
    ///     let pa = PA::new(0x2000).unwrap();
    ///     addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// }
    ///
    /// let ranges = addr_space.get_mapped_ranges();
    /// assert!(!ranges.is_empty());
    /// ```
    #[must_use]
    pub fn get_mapped_ranges(&self) -> Vec<AddressRange> {
        let mut ranges = Vec::new();

        if self.page_table.is_empty() {
            return ranges;
        }

        // Collect and sort page numbers
        let mut page_nums: Vec<u64> = self.page_table.iter().map(|entry| *entry.key()).collect();
        page_nums.sort_unstable();

        // Consolidate into ranges
        let mut range_start = page_nums[0] << 12;
        let mut range_end = range_start + PAGE_SIZE - 1;

        for &page_num in &page_nums[1..] {
            let current_page_addr = page_num << 12;

            if current_page_addr == range_end + 1 {
                // Extend current range
                range_end = current_page_addr + PAGE_SIZE - 1;
            } else {
                // Start new range
                ranges.push(AddressRange::new(
                    IOVA::new(range_start).unwrap(),
                    IOVA::new(range_end).unwrap(),
                ));
                range_start = current_page_addr;
                range_end = range_start + PAGE_SIZE - 1;
            }
        }

        // Add final range
        ranges.push(AddressRange::new(
            IOVA::new(range_start).unwrap(),
            IOVA::new(range_end).unwrap(),
        ));

        ranges
    }

    /// Gets the total address space size (from minimum to maximum mapped address)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova1 = IOVA::new(0x1000).unwrap();
    /// let iova2 = IOVA::new(0x1000 + (100 * PAGE_SIZE)).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_page(iova1, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    /// addr_space.map_page(iova2, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    ///
    /// let size = addr_space.get_address_space_size();
    /// assert!(size >= 100 * PAGE_SIZE);
    /// ```
    #[must_use]
    pub fn get_address_space_size(&self) -> u64 {
        if self.page_table.is_empty() {
            return 0;
        }

        let min_page_num = self.page_table.iter().map(|entry| *entry.key()).min().unwrap();
        let max_page_num = self.page_table.iter().map(|entry| *entry.key()).max().unwrap();

        let min_address = min_page_num << 12;
        let max_address = (max_page_num << 12) + PAGE_SIZE - 1;

        max_address - min_address + 1
    }

    /// Checks if a range has any overlapping mappings
    ///
    /// # Arguments
    ///
    /// * `start_iova` - Start of the range to check
    /// * `end_iova` - End of the range to check
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    ///
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    ///
    /// let start = IOVA::new(0x1000 - PAGE_SIZE).unwrap();
    /// let end = IOVA::new(0x1000 + PAGE_SIZE).unwrap();
    ///
    /// assert!(addr_space.has_overlapping_mappings(start, end));
    /// ```
    #[must_use]
    pub fn has_overlapping_mappings(&self, start_iova: IOVA, end_iova: IOVA) -> bool {
        if end_iova.as_u64() < start_iova.as_u64() {
            return false;
        }

        let start_page_num = self.page_number(start_iova);
        let end_page_num = self.page_number(end_iova);

        (start_page_num..=end_page_num).any(|page_num| self.page_table.contains_key(&page_num))
    }

    /// Returns an immutable iterator over all mapped pages
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// addr_space.map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure).unwrap();
    ///
    /// let count = addr_space.iter().count();
    /// assert_eq!(count, 1);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = PageEntryRef> + '_ {
        self.page_table.iter().map(|entry_ref| {
            let page_num = *entry_ref.key();
            let iova = IOVA::new(page_num << 12).unwrap();
            PageEntryRef { iova, entry: entry_ref.value().clone() }
        })
    }

    /// Returns a mutable iterator over all mapped pages
    ///
    /// Note: With DashMap, mutations should be done via map_page/unmap_page methods
    /// rather than direct iteration for thread-safety
    pub fn iter_mut(&self) -> impl Iterator<Item = PageEntryRef> + '_ {
        // DashMap doesn't support true iter_mut - use iter() instead
        self.iter()
    }

    /// Returns an immutable query interface
    ///
    /// The query interface borrows the AddressSpace immutably, preventing
    /// mutations while queries are active (enforced by borrow checker).
    #[must_use]
    pub const fn query(&self) -> AddressSpaceQuery<'_> {
        AddressSpaceQuery { addr_space: self }
    }

    /// Queries a single page, returning a copy
    #[must_use]
    pub fn query_page(&self, iova: IOVA) -> Option<PageEntry> {
        let page_num = self.page_number(iova);
        self.page_table.get(&page_num).map(|entry_ref| entry_ref.value().clone())
    }

    /// Maps multiple pages in a batched operation
    ///
    /// Uses internal batching with capacity pre-allocation to minimize
    /// lock contention and improve performance.
    ///
    /// # Arguments
    ///
    /// * `mappings` - Slice of (IOVA, PA) pairs to map
    /// * `permissions` - Page permissions for all mappings
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, PAGE_SIZE};
    ///
    /// let mut addr_space = AddressSpace::new();
    /// let mappings: Vec<(IOVA, PA)> = (0..100)
    ///     .map(|i| {
    ///         (IOVA::new(0x1000 + i * PAGE_SIZE).unwrap(),
    ///          PA::new(0x2000 + i * PAGE_SIZE).unwrap())
    ///     })
    ///     .collect();
    ///
    /// addr_space.map_pages_batched(&mappings, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    /// assert_eq!(addr_space.get_page_count().unwrap(), 100);
    /// ```
    pub fn map_pages_batched(
        &self,
        mappings: &[(IOVA, PA)],
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Validate permissions
        if !permissions.read() && !permissions.write() && !permissions.execute() {
            return Err(AddressSpaceError::InvalidPermissions);
        }

        // Use SmallVec for small batches (stack optimization)
        let mut batch: SmallVec<[(u64, PageEntry); 16]> = SmallVec::with_capacity(mappings.len());

        // Validate all mappings first (transactional semantics)
        for &(iova, pa) in mappings {
            if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
            }
            if pa.as_u64() > MAX_PHYSICAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: pa.as_u64() });
            }

            let page_num = self.page_number(iova);
            let aligned_pa = PA::new(pa.as_u64() & !PAGE_MASK).unwrap();
            // Set AF=true so bulk-mapped pages do not trigger spurious F_ACCESS faults
            // (§3.13.2: AF=0 + ha=false + affd=false → F_ACCESS).
            let entry = PageEntry::with_security_state(aligned_pa, permissions, security_state)
                .with_access_flag(true);
            batch.push((page_num, entry));
        }

        // DashMap doesn't need reserve - it auto-resizes efficiently

        // Insert all entries
        for (page_num, entry) in batch {
            self.page_table.insert(page_num, entry);
        }

        Ok(())
    }

    /// Unmaps multiple pages in a batched operation
    pub fn unmap_pages_batched(&self, iovas: &[IOVA]) -> Result<(), AddressSpaceError> {
        // Validate all IOVAs first
        for &iova in iovas {
            if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
            }
        }

        // Check if at least some pages are mapped
        let any_mapped = iovas.iter().any(|&iova| self.page_table.contains_key(&self.page_number(iova)));

        if !any_mapped {
            return Err(AddressSpaceError::PageNotMapped);
        }

        // Unmap all pages
        for &iova in iovas {
            let page_num = self.page_number(iova);
            self.page_table.remove(&page_num);
        }

        Ok(())
    }

    /// Updates permissions for multiple pages in a batched operation
    pub fn update_permissions_batched(
        &mut self,
        iovas: &[IOVA],
        permissions: PagePermissions,
    ) -> Result<(), AddressSpaceError> {
        // Validate permissions
        if !permissions.read() && !permissions.write() && !permissions.execute() {
            return Err(AddressSpaceError::InvalidPermissions);
        }

        // Validate all IOVAs first
        for &iova in iovas {
            if iova.as_u64() > MAX_VIRTUAL_ADDRESS {
                return Err(AddressSpaceError::InvalidAddress { address: iova.as_u64() });
            }
        }

        // Update all permissions
        for &iova in iovas {
            let page_num = self.page_number(iova);
            if let Some(mut entry) = self.page_table.get_mut(&page_num) {
                *entry = entry.clone().with_permissions(permissions);
            }
        }

        Ok(())
    }

    /// Invalidates a specific page with atomic operations
    ///
    /// Uses atomic operations for thread-safe invalidation tracking.
    pub fn invalidate_page_atomic(&mut self, iova: IOVA) {
        let page_num = self.page_number(iova);

        // Increment global generation counter
        self.invalidation_generation.fetch_add(1, Ordering::Release);

        // Track per-page invalidation
        self.invalidation_map
            .entry(page_num)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Release);
    }

    /// Invalidates a range of pages atomically
    ///
    /// Returns the number of pages invalidated.
    pub fn invalidate_range_atomic(&mut self, start: IOVA, end: IOVA) -> usize {
        let start_page = self.page_number(start);
        let end_page = self.page_number(end);

        let mut count = 0;
        for page_num in start_page..=end_page {
            if self.page_table.contains_key(&page_num) {
                self.invalidation_map
                    .entry(page_num)
                    .or_insert_with(|| AtomicU64::new(0))
                    .fetch_add(1, Ordering::Release);
                count += 1;
            }
        }

        if count > 0 {
            self.invalidation_generation.fetch_add(count as u64, Ordering::Release);
        }

        count
    }

    /// Invalidates a page with explicit memory ordering
    pub fn invalidate_page_with_ordering(&mut self, iova: IOVA, ordering: Ordering) {
        let page_num = self.page_number(iova);

        self.invalidation_generation.fetch_add(1, ordering);

        self.invalidation_map
            .entry(page_num)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, ordering);
    }

    /// Checks if a page has been invalidated
    #[must_use]
    pub fn is_invalidated(&self, iova: IOVA) -> bool {
        let page_num = self.page_number(iova);
        self.invalidation_map
            .get(&page_num)
            .map_or(false, |gen| gen.load(Ordering::Acquire) > 0)
    }

    /// Checks if a page has been invalidated with explicit memory ordering
    #[must_use]
    pub fn is_invalidated_with_ordering(&self, iova: IOVA, ordering: Ordering) -> bool {
        let page_num = self.page_number(iova);
        self.invalidation_map.get(&page_num).map_or(false, |gen| gen.load(ordering) > 0)
    }

    /// Returns the current invalidation generation counter
    #[must_use]
    pub fn invalidation_generation(&self) -> u64 {
        self.invalidation_generation.load(Ordering::Acquire)
    }

    /// Attempts to invalidate a page using compare-and-exchange
    ///
    /// Returns true if the invalidation succeeded (state transitioned from
    /// current to new), false otherwise.
    pub fn compare_exchange_invalidate(&mut self, iova: IOVA, current: bool, new: bool, ordering: Ordering) -> bool {
        let page_num = self.page_number(iova);

        let current_val = if current { 1u64 } else { 0u64 };
        let new_val = if new { 1u64 } else { 0u64 };

        let entry = self.invalidation_map.entry(page_num).or_insert_with(|| AtomicU64::new(0));

        entry
            .compare_exchange(current_val, new_val, ordering, Ordering::Relaxed)
            .is_ok()
    }

    /// Invalidates a specific page in the cache
    ///
    /// This is a no-op in the current implementation as AddressSpace maintains
    /// authoritative state. Cache invalidation is coordinated at higher levels.
    pub fn invalidate_page(&mut self, iova: IOVA) {
        // Delegate to atomic version
        self.invalidate_page_atomic(iova);
    }

    /// Invalidates a range in the cache
    ///
    /// This is a no-op in the current implementation as AddressSpace maintains
    /// authoritative state. Cache invalidation is coordinated at higher levels.
    pub fn invalidate_range(&mut self, _start_iova: IOVA, _end_iova: IOVA) {
        // No-op - AddressSpace maintains authoritative state
    }

    /// Invalidates all cache entries
    ///
    /// This is a no-op in the current implementation as AddressSpace maintains
    /// authoritative state. Cache invalidation is coordinated at higher levels.
    pub fn invalidate_all(&mut self) {
        // No-op - AddressSpace maintains authoritative state
    }

    /// Updates the Access Flag (AF) and Dirty State bits on a page entry after
    /// successful translation, simulating hardware page table walk behaviour per
    /// ARM SMMU v3 §3.13.
    ///
    /// - Sets `access_flag = true` when `ha = true` and AF is not already set.
    /// - Sets `dirty = true` when `hd = true` and the access is a write variant.
    ///
    /// Returns `true` if any flag was updated (page table entry modified).
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState, AccessType};
    ///
    /// let addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// addr_space.map_page_unaccessed(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    ///
    /// let changed = addr_space.update_access_flags(iova, true, false, AccessType::Read);
    /// assert!(changed);
    /// assert_eq!(addr_space.get_page_access_flag(iova), Some(true));
    /// ```
    pub fn update_access_flags(&self, iova: IOVA, ha: bool, hd: bool, access_type: AccessType) -> bool {
        if !ha && !hd {
            return false;
        }
        let page_num = self.page_number(iova);
        if let Some(mut entry) = self.page_table.get_mut(&page_num) {
            let af_changed = if ha && !entry.is_access_flag_set() {
                *entry = entry.clone().with_access_flag(true);
                true
            } else {
                false
            };
            // Bug-4 fix: include WritePrivileged and ReadWritePrivileged for dirty-flag tracking.
            let is_write = access_type.can_write();
            let dirty_changed = if hd && is_write && !entry.is_dirty() {
                *entry = entry.clone().with_dirty(true);
                true
            } else {
                false
            };
            af_changed || dirty_changed
        } else {
            false
        }
    }

    /// Returns the `access_flag` state of a mapped page entry (for testing and inspection).
    ///
    /// Returns `None` if the page is not mapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// addr_space.map_page_unaccessed(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    /// assert_eq!(addr_space.get_page_access_flag(iova), Some(false));
    /// ```
    #[must_use]
    pub fn get_page_access_flag(&self, iova: IOVA) -> Option<bool> {
        let page_num = self.page_number(iova);
        self.page_table.get(&page_num).map(|e| e.is_access_flag_set())
    }

    /// Returns the `dirty` state of a mapped page entry (for testing and inspection).
    ///
    /// Returns `None` if the page is not mapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::address_space::AddressSpace;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let addr_space = AddressSpace::new();
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
    /// assert_eq!(addr_space.get_page_dirty(iova), Some(false));
    /// ```
    #[must_use]
    pub fn get_page_dirty(&self, iova: IOVA) -> Option<bool> {
        let page_num = self.page_number(iova);
        self.page_table.get(&page_num).map(|e| e.is_dirty())
    }

    // Private helper methods

    /// Converts IOVA to page number for sparse indexing
    #[inline]
    fn page_number(&self, iova: IOVA) -> u64 {
        iova.as_u64() >> 12 // PAGE_SIZE = 4096 = 2^12
    }

    /// Checks if permissions allow the requested access type
    #[inline]
    fn check_permissions(&self, perms: PagePermissions, access_type: AccessType) -> bool {
        perms.allows(access_type)
    }
}

impl Default for AddressSpace {
    fn default() -> Self {
        Self::new()
    }
}

// AddressSpace is automatically Send + Sync because HashMap<u64, PageEntry> is Send + Sync

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_address_space() {
        let addr_space = AddressSpace::new();
        assert_eq!(addr_space.get_page_count().unwrap(), 0);
    }

    #[test]
    fn test_map_single_page() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();

        assert!(addr_space.is_page_mapped(iova).unwrap());
        assert_eq!(addr_space.get_page_count().unwrap(), 1);
    }

    #[test]
    fn test_translate_page() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space
            .map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
            .unwrap();

        let result = addr_space
            .translate_page(iova, AccessType::Read, SecurityState::NonSecure)
            .unwrap();

        assert_eq!(result.physical_address().as_u64(), 0x2000);
    }

    #[test]
    fn test_permission_violation() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();

        addr_space
            .map_page(iova, pa, PagePermissions::read_only(), SecurityState::NonSecure)
            .unwrap();

        let result = addr_space.translate_page(iova, AccessType::Write, SecurityState::NonSecure);
        assert!(result.is_err());
    }

    // ========================================================================
    // TDD tests for FINDING-M-04: Access Flag and Dirty State simulation
    // These tests will FAIL before implementation.
    // ========================================================================

    #[test]
    fn test_update_access_flags_ha_sets_af_on_read() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        // Use map_page_unaccessed() to simulate AF=false (fresh OS mapping, §3.13).
        addr_space.map_page_unaccessed(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // AF should be false initially (unaccessed mapping)
        assert_eq!(addr_space.get_page_access_flag(iova), Some(false));

        // After update with ha=true, AF should be set
        let changed = addr_space.update_access_flags(iova, true, false, AccessType::Read);
        assert!(changed);
        assert_eq!(addr_space.get_page_access_flag(iova), Some(true));
    }

    #[test]
    fn test_update_access_flags_hd_sets_dirty_on_write() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space.map_page_unaccessed(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        assert_eq!(addr_space.get_page_dirty(iova), Some(false));

        let changed = addr_space.update_access_flags(iova, false, true, AccessType::Write);
        assert!(changed);
        assert_eq!(addr_space.get_page_dirty(iova), Some(true));
    }

    #[test]
    fn test_update_access_flags_hd_does_not_set_dirty_on_read() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space.map_page_unaccessed(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        addr_space.update_access_flags(iova, false, true, AccessType::Read);
        assert_eq!(addr_space.get_page_dirty(iova), Some(false));
    }

    #[test]
    fn test_update_access_flags_no_change_when_already_set() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        // Map with AF=true (default), then verify second update_access_flags is no-op
        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();
        // AF already true → update should return false (no change)
        let changed = addr_space.update_access_flags(iova, true, false, AccessType::Read);
        assert!(!changed);
    }

    #[test]
    fn test_update_access_flags_no_flags_enabled_returns_false() {
        let addr_space = AddressSpace::new();
        let iova = IOVA::new(0x1000).unwrap();
        let pa = PA::new(0x2000).unwrap();
        addr_space.map_page(iova, pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        let changed = addr_space.update_access_flags(iova, false, false, AccessType::Write);
        assert!(!changed);
    }
}
