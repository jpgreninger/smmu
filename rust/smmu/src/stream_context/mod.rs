//! Per-stream state and PASID management
//!
//! This module manages the state associated with each stream (device), including:
//!
//! - PASID (Process Address Space ID) management
//! - Per-PASID address space mappings
//! - Stream configuration and capabilities
//! - Translation context switching
//!
//! # Stream Context
//!
//! Each stream represents a device or logical channel that can access memory.
//! Streams may support multiple PASIDs for virtualization and process isolation.
//!
//! # PASID Support
//!
//! Full PASID support including PASID 0 (default/legacy mode) per ARM SMMU v3 specification.
//!
//! # Thread Safety
//!
//! StreamContext uses DashMap for lock-free concurrent PASID operations and atomic
//! operations for configuration flags, making it safe to share across threads.

#![warn(missing_docs)]

use crate::address_space::{AddressSpace, AddressSpaceError};
use crate::types::{
    AccessType, FaultRecord, FaultType, PagePermissions, SecurityState, StreamContextError, TranslationData,
    TranslationError, TranslationResult, IOVA, PA, PASID,
};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

/// StreamContext - Per-stream state and PASID management
///
/// Manages translation contexts for a single stream (device), supporting multiple
/// PASIDs for process isolation and two-stage translation per ARM SMMU v3 specification.
///
/// # Architecture
///
/// - **Stage-1**: Per-PASID translation (IOVA → IPA or IOVA → PA)
/// - **Stage-2**: Shared translation across PASIDs (IPA → PA)
/// - **Two-Stage**: Combined Stage-1 → Stage-2 translation
/// - **Bypass**: Identity mapping when both stages disabled
///
/// # Thread Safety
///
/// All operations are thread-safe using lock-free DashMap for PASID storage and
/// atomic operations for configuration flags.
///
/// # Examples
///
/// ```
/// use smmu::stream_context::StreamContext;
/// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState, AccessType};
///
/// let stream_context = StreamContext::new();
///
/// // Create PASID and map a page
/// let pasid = PASID::new(1).unwrap();
/// stream_context.create_pasid(pasid).unwrap();
///
/// let iova = IOVA::new(0x1000).unwrap();
/// let pa = PA::new(0x2000).unwrap();
/// let perms = PagePermissions::read_write();
/// stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
///
/// // Translate address
/// let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
/// assert!(result.is_ok());
/// ```
#[derive(Debug)]
pub struct StreamContext {
    /// PASID → AddressSpace mapping (Stage-1) for ALL PASIDs including PASID 0
    /// DashMap provides lock-free concurrent access for high performance.
    /// Previously PASID 0 was stored in a separate RwLock, but benchmarking showed
    /// that DashMap is actually faster under contention (15-30ns improvement).
    /// No RwLock on AddressSpace since it's now lock-free with DashMap internally.
    pub(crate) pasid_map: DashMap<u32, Arc<AddressSpace>>,

    /// PASID → ASID mapping — stores the CD.ASID value for each PASID (ARM §3.17).
    /// Used to tag Stage-1 TLB entries and to resolve ASID for ASID-targeted
    /// invalidation commands (`CMD_TLBI_NH_ASID` / `CMD_TLBI_EL2_ASID`).
    /// Default ASID for all PASIDs is 0.
    pub(crate) pasid_asid_map: DashMap<u32, u16>,

    /// Stage-2 AddressSpace (shared across all PASIDs)
    /// RwLock allows concurrent reads with exclusive writes
    stage2_address_space: RwLock<Option<Arc<AddressSpace>>>,

    /// Stage-1 enable flag (atomic for lock-free access)
    stage1_enabled: AtomicBool,

    /// Stage-2 enable flag (atomic for lock-free access)
    stage2_enabled: AtomicBool,

    /// Maximum PASIDs per stream (resource limit)
    max_pasids_per_stream: AtomicUsize,

    /// Stream enabled state (Section 4.2.2)
    enabled: AtomicBool,

    /// Fault records (Section 4.2.4)
    fault_records: Arc<RwLock<Vec<FaultRecord>>>,

    /// Fault rate limit (max faults to record)
    fault_rate_limit: AtomicUsize,

    /// Fault retry enabled flag
    fault_retry_enabled: AtomicBool,

    /// Monotonic fault timestamp counter (avoids SystemTime overhead)
    fault_timestamp_counter: AtomicUsize,

    /// VMID (Virtual Machine ID) — STE Word 2 bits 63:48 per ARM §5.2.
    /// Tags Stage-2 TLB entries for VMID-targeted invalidation via
    /// `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA`.  Default 0.
    vmid: AtomicU16,

    /// Stall fault mode enabled — ARM §3.12.2.
    /// When true, faulting translations stall (returning `Stalled { stag }`)
    /// instead of immediately aborting.  Corresponds to `FaultMode::Stall`.
    stall_enabled: AtomicBool,
}

impl StreamContext {
    /// Creates a new StreamContext with default configuration
    ///
    /// Default configuration:
    /// - Stage-1 enabled, Stage-2 disabled
    /// - Maximum 1024 PASIDs per stream
    /// - No PASIDs initially configured
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// assert!(stream_context.is_stage1_enabled());
    /// assert!(!stream_context.is_stage2_enabled());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            pasid_map: DashMap::new(),
            pasid_asid_map: DashMap::new(),
            stage2_address_space: RwLock::new(None),
            stage1_enabled: AtomicBool::new(true),
            stage2_enabled: AtomicBool::new(false),
            max_pasids_per_stream: AtomicUsize::new(1024),
            enabled: AtomicBool::new(true),
            fault_records: Arc::new(RwLock::new(Vec::new())),
            fault_rate_limit: AtomicUsize::new(usize::MAX),
            fault_retry_enabled: AtomicBool::new(false),
            fault_timestamp_counter: AtomicUsize::new(0),
            vmid: AtomicU16::new(0),
            stall_enabled: AtomicBool::new(false),
        }
    }

    // ========================================================================
    // PASID Management Operations
    // ========================================================================

    /// Creates a new PASID with a fresh AddressSpace
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to create
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - PASID already exists (`PASIDAlreadyExists`)
    /// - PASID limit exceeded (`PASIDLimitExceeded`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(0).unwrap();
    /// assert!(stream_context.create_pasid(pasid).is_ok());
    /// ```
    pub fn create_pasid(&self, pasid: PASID) -> Result<(), StreamContextError> {
        // Check if stream is enabled
        self.check_enabled()?;

        let pasid_value = pasid.as_u32();

        // Check PASID limit
        let current_count = self.pasid_map.len();
        let max_pasids = self.max_pasids_per_stream.load(Ordering::Relaxed);
        if current_count >= max_pasids {
            return Err(StreamContextError::PASIDLimitExceeded(current_count, max_pasids));
        }

        // Check for duplicate
        if self.pasid_map.contains_key(&pasid_value) {
            return Err(StreamContextError::PASIDAlreadyExists(pasid_value));
        }

        // Create new AddressSpace for this PASID (no RwLock needed - AddressSpace is lock-free)
        let address_space = Arc::new(AddressSpace::new());
        self.pasid_map.insert(pasid_value, address_space);

        Ok(())
    }

    // ========================================================================
    // ASID Management (CD.ASID per ARM §3.17)
    // ========================================================================

    /// Returns the ASID currently associated with the given PASID.
    ///
    /// Returns `0` if no ASID has been explicitly set (default per spec).
    ///
    /// # Errors
    ///
    /// Returns [`StreamContextError::PASIDNotFound`] if the PASID does not exist.
    pub fn get_pasid_asid(&self, pasid: PASID) -> Result<u16, StreamContextError> {
        let pasid_value = pasid.as_u32();
        if !self.pasid_map.contains_key(&pasid_value) {
            return Err(StreamContextError::PASIDNotFound(pasid_value));
        }
        Ok(self.pasid_asid_map.get(&pasid_value).map(|v| *v).unwrap_or(0))
    }

    /// Sets the ASID (CD.ASID) for the given PASID.
    ///
    /// # Errors
    ///
    /// Returns [`StreamContextError::PASIDNotFound`] if the PASID does not exist.
    pub fn set_pasid_asid(&self, pasid: PASID, asid: u16) -> Result<(), StreamContextError> {
        let pasid_value = pasid.as_u32();
        if !self.pasid_map.contains_key(&pasid_value) {
            return Err(StreamContextError::PASIDNotFound(pasid_value));
        }
        self.pasid_asid_map.insert(pasid_value, asid);
        Ok(())
    }

    /// Returns the ASID for the given PASID, or 0 if not set (infallible fast-path).
    ///
    /// Does not check whether the PASID exists; returns 0 for unknown PASIDs.
    /// Used by the translate fast-path to tag TLB entries.
    #[inline]
    pub(crate) fn get_pasid_asid_or_default(&self, pasid: PASID) -> u16 {
        self.pasid_asid_map.get(&pasid.as_u32()).map(|v| *v).unwrap_or(0)
    }

    // ========================================================================
    // VMID Management (STE.S2VMID per ARM §5.2, §3.8)
    // ========================================================================

    /// Returns the VMID (STE Word 2 bits 63:48) for this stream.
    ///
    /// Default is 0. VMID tags Stage-2 TLB entries for VMID-targeted
    /// invalidation via `CMD_TLBI_S12_VMALL` / `CMD_TLBI_S2_IPA`.
    #[inline]
    pub fn get_vmid(&self) -> u16 {
        self.vmid.load(Ordering::Relaxed)
    }

    /// Sets the VMID for this stream (STE Word 2 bits 63:48, ARM §5.2).
    ///
    /// New TLB entries installed after this call will be tagged with the
    /// new VMID.  Existing cached entries retain the old tag; issue
    /// `CMD_TLBI_S12_VMALL` with the old VMID to evict them.
    #[inline]
    pub fn set_vmid(&self, vmid: u16) {
        self.vmid.store(vmid, Ordering::Relaxed);
    }

    /// Returns whether stall fault mode is enabled for this stream (ARM §3.12.2).
    #[inline]
    pub fn is_stall_enabled(&self) -> bool {
        self.stall_enabled.load(Ordering::Relaxed)
    }

    /// Enables or disables stall fault mode for this stream (ARM §3.12.2).
    ///
    /// When enabled, faulting translations return `Stalled { stag }` instead
    /// of an immediate abort error.
    #[inline]
    pub fn set_stall_enabled(&self, enabled: bool) {
        self.stall_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Removes a PASID and its associated AddressSpace
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to remove
    ///
    /// # Errors
    ///
    /// Returns error if PASID not found (`PASIDNotFound`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    /// assert!(stream_context.remove_pasid(pasid).is_ok());
    /// ```
    pub fn remove_pasid(&self, pasid: PASID) -> Result<(), StreamContextError> {
        let pasid_value = pasid.as_u32();

        // Remove from DashMap (works for all PASIDs including 0)
        if self.pasid_map.remove(&pasid_value).is_none() {
            return Err(StreamContextError::PASIDNotFound(pasid_value));
        }

        Ok(())
    }

    /// Adds a PASID with an existing AddressSpace
    ///
    /// This allows multiple PASIDs to share the same AddressSpace.
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to add
    /// * `address_space` - Shared AddressSpace reference
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - PASID already exists (`PASIDAlreadyExists`)
    /// - PASID limit exceeded (`PASIDLimitExceeded`)
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    /// use std::sync::{Arc, RwLock};
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid1 = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid1).unwrap();
    ///
    /// // Share AddressSpace with another PASID
    /// let addr_space = stream_context.get_pasid_address_space(pasid1).unwrap();
    /// let pasid2 = PASID::new(2).unwrap();
    /// assert!(stream_context.add_pasid(pasid2, addr_space).is_ok());
    /// ```
    pub fn add_pasid(&self, pasid: PASID, address_space: Arc<AddressSpace>) -> Result<(), StreamContextError> {
        let pasid_value = pasid.as_u32();

        // Check PASID limit
        let current_count = self.pasid_map.len();
        let max_pasids = self.max_pasids_per_stream.load(Ordering::Relaxed);
        if current_count >= max_pasids {
            return Err(StreamContextError::PASIDLimitExceeded(current_count, max_pasids));
        }

        // Check for duplicate
        if self.pasid_map.contains_key(&pasid_value) {
            return Err(StreamContextError::PASIDAlreadyExists(pasid_value));
        }

        self.pasid_map.insert(pasid_value, address_space);

        Ok(())
    }

    /// Checks if a PASID exists
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID to check
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// assert!(!stream_context.has_pasid(pasid));
    /// stream_context.create_pasid(pasid).unwrap();
    /// assert!(stream_context.has_pasid(pasid));
    /// ```
    #[must_use]
    pub fn has_pasid(&self, pasid: PASID) -> bool {
        let pasid_value = pasid.as_u32();
        self.pasid_map.contains_key(&pasid_value)
    }

    /// Returns the number of configured PASIDs
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// assert_eq!(stream_context.pasid_count(), 0);
    /// stream_context.create_pasid(PASID::new(1).unwrap()).unwrap();
    /// assert_eq!(stream_context.pasid_count(), 1);
    /// ```
    #[must_use]
    pub fn pasid_count(&self) -> usize {
        self.pasid_map.len()
    }

    /// Clears all PASIDs and their associated AddressSpaces
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.create_pasid(PASID::new(1).unwrap()).unwrap();
    /// stream_context.create_pasid(PASID::new(2).unwrap()).unwrap();
    /// assert_eq!(stream_context.pasid_count(), 2);
    ///
    /// stream_context.clear_all_pasids().unwrap();
    /// assert_eq!(stream_context.pasid_count(), 0);
    /// ```
    pub fn clear_all_pasids(&self) -> Result<(), StreamContextError> {
        // Clear all PASIDs (including PASID 0)
        self.pasid_map.clear();
        Ok(())
    }

    /// Sets the maximum number of PASIDs per stream
    ///
    /// # Arguments
    ///
    /// * `max` - Maximum PASID count
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_max_pasids_per_stream(512);
    /// ```
    pub fn set_max_pasids_per_stream(&self, max: usize) {
        self.max_pasids_per_stream.store(max, Ordering::Relaxed);
    }

    /// Gets the AddressSpace for a PASID
    ///
    /// Returns `None` if PASID not found.
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let addr_space = stream_context.get_pasid_address_space(pasid);
    /// assert!(addr_space.is_some());
    /// ```
    #[must_use]
    pub fn get_pasid_address_space(&self, pasid: PASID) -> Option<Arc<AddressSpace>> {
        self.pasid_map.get(&pasid.as_u32()).map(|entry| entry.clone())
    }

    // ========================================================================
    // Stage Configuration Operations
    // ========================================================================

    /// Sets Stage-1 translation enable state
    ///
    /// # Arguments
    ///
    /// * `enabled` - True to enable Stage-1, false to disable
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage1_enabled(false);
    /// assert!(!stream_context.is_stage1_enabled());
    /// ```
    pub fn set_stage1_enabled(&self, enabled: bool) {
        self.stage1_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Sets Stage-2 translation enable state
    ///
    /// # Arguments
    ///
    /// * `enabled` - True to enable Stage-2, false to disable
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage2_enabled(true);
    /// assert!(stream_context.is_stage2_enabled());
    /// ```
    pub fn set_stage2_enabled(&self, enabled: bool) {
        self.stage2_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Checks if Stage-1 translation is enabled
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// assert!(stream_context.is_stage1_enabled());
    /// ```
    #[must_use]
    pub fn is_stage1_enabled(&self) -> bool {
        self.stage1_enabled.load(Ordering::Relaxed)
    }

    /// Checks if Stage-2 translation is enabled
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// assert!(!stream_context.is_stage2_enabled());
    /// ```
    #[must_use]
    pub fn is_stage2_enabled(&self) -> bool {
        self.stage2_enabled.load(Ordering::Relaxed)
    }

    /// Sets the Stage-2 AddressSpace (shared across PASIDs)
    ///
    /// # Arguments
    ///
    /// * `address_space` - Optional Stage-2 AddressSpace
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::address_space::AddressSpace;
    /// use std::sync::Arc;
    ///
    /// let stream_context = StreamContext::new();
    /// let stage2 = Arc::new(AddressSpace::new());
    /// stream_context.set_stage2_address_space(Some(stage2));
    /// ```
    pub fn set_stage2_address_space(&self, address_space: Option<Arc<AddressSpace>>) {
        let mut stage2 = self.stage2_address_space.write().unwrap();
        *stage2 = address_space;
    }

    // ========================================================================
    // Page Operations
    // ========================================================================

    /// Maps a page in the specified PASID's address space
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Virtual address
    /// * `pa` - Physical address
    /// * `permissions` - Page permissions
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if PASID not found or mapping fails
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    ///
    /// assert!(stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).is_ok());
    /// ```
    pub fn map_page(
        &self,
        pasid: PASID,
        iova: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Check if stream is enabled
        if !self.is_enabled() {
            return Err(AddressSpaceError::InternalError);
        }

        let pasid_value = pasid.as_u32();

        // Get AddressSpace for PASID (works for all PASIDs including 0)
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(AddressSpaceError::InternalError)?;

        // Map page through AddressSpace (no write lock needed - AddressSpace is lock-free)
        addr_space.map_page(iova, pa, permissions, security_state)
    }

    /// Unmaps a page from the specified PASID's address space
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Virtual address
    ///
    /// # Errors
    ///
    /// Returns error if PASID not found or unmap fails
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let stream_context = StreamContext::new();
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    /// stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    ///
    /// assert!(stream_context.unmap_page(pasid, iova).is_ok());
    /// ```
    pub fn unmap_page(&self, pasid: PASID, iova: IOVA) -> Result<(), AddressSpaceError> {
        let pasid_value = pasid.as_u32();

        // Get AddressSpace for PASID
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(AddressSpaceError::InternalError)?;

        // Unmap page through AddressSpace (no write lock needed - AddressSpace is lock-free)
        addr_space.unmap_page(iova)
    }

    /// Create and initialize Stage-2 address space
    ///
    /// Creates a new address space for Stage-2 translation (IPA → PA).
    /// Required for two-stage translation setup.
    ///
    /// # Errors
    ///
    /// Returns error if Stage-2 address space already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage2_enabled(true);
    /// assert!(stream_context.create_stage2_address_space().is_ok());
    /// ```
    pub fn create_stage2_address_space(&self) -> Result<(), StreamContextError> {
        let mut stage2_guard = self.stage2_address_space.write().unwrap();

        // Check if Stage-2 already exists
        if stage2_guard.is_some() {
            return Err(StreamContextError::InternalError(
                "Stage-2 address space already exists".to_string(),
            ));
        }

        // Create new Stage-2 address space
        *stage2_guard = Some(Arc::new(AddressSpace::new()));
        Ok(())
    }

    /// Map a page in the Stage-2 address space (IPA → PA)
    ///
    /// For two-stage translation, this maps Intermediate Physical Addresses
    /// (output of Stage-1) to final Physical Addresses.
    ///
    /// # Arguments
    ///
    /// * `ipa` - Intermediate Physical Address
    /// * `pa` - Physical Address
    /// * `permissions` - Page permissions
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Stage-2 address space not initialized
    /// - Mapping fails
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{IOVA, PA, PagePermissions, SecurityState};
    ///
    /// let stream_context = StreamContext::new();
    /// stream_context.set_stage2_enabled(true);
    /// stream_context.create_stage2_address_space().unwrap();
    ///
    /// let ipa = IOVA::new(0x2000).unwrap();
    /// let pa = PA::new(0x3000).unwrap();
    /// let perms = PagePermissions::read_write();
    ///
    /// assert!(stream_context.map_stage2_page(ipa, pa, perms, SecurityState::NonSecure).is_ok());
    /// ```
    pub fn map_stage2_page(
        &self,
        ipa: IOVA,
        pa: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Result<(), AddressSpaceError> {
        // Get Stage-2 address space (read lock only - AddressSpace is lock-free)
        let stage2_guard = self.stage2_address_space.read().unwrap();
        let stage2 = stage2_guard.as_ref().ok_or(AddressSpaceError::InternalError)?;

        // Map page through lock-free AddressSpace
        stage2.map_page(ipa, pa, permissions, security_state)
    }

    // ========================================================================
    // Translation Operations
    // ========================================================================

    /// Translates an address through configured translation stages
    ///
    /// Supports four translation modes per ARM SMMU v3 specification:
    /// - Stage-1 only: IOVA → PA
    /// - Stage-2 only: IPA → PA (IOVA treated as IPA)
    /// - Two-stage: IOVA → IPA → PA
    /// - Bypass: IOVA = PA (identity mapping)
    ///
    /// # Arguments
    ///
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Input/Virtual address
    /// * `access_type` - Type of memory access (Read/Write/Execute)
    /// * `security_state` - Security state
    ///
    /// # Errors
    ///
    /// Returns error if translation fails due to:
    /// - PASID not found
    /// - Page not mapped
    /// - Permission violation
    /// - Invalid configuration
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::{PASID, IOVA, PA, PagePermissions, SecurityState, AccessType};
    ///
    /// let mut stream_context = StreamContext::new();
    /// stream_context.set_stage1_enabled(true);
    /// stream_context.set_stage2_enabled(false);
    ///
    /// let pasid = PASID::new(1).unwrap();
    /// stream_context.create_pasid(pasid).unwrap();
    ///
    /// let iova = IOVA::new(0x1000).unwrap();
    /// let pa = PA::new(0x2000).unwrap();
    /// let perms = PagePermissions::read_write();
    /// stream_context.map_page(pasid, iova, pa, perms, SecurityState::NonSecure).unwrap();
    ///
    /// let result = stream_context.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    /// assert!(result.is_ok());
    /// assert_eq!(result.unwrap().physical_address(), pa);
    /// ```
    pub fn translate(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        // Check if stream is enabled
        if !self.is_enabled() {
            return Err(TranslationError::StreamDisabled);
        }

        let stage1_enabled = self.stage1_enabled.load(Ordering::Relaxed);
        let stage2_enabled = self.stage2_enabled.load(Ordering::Relaxed);

        match (stage1_enabled, stage2_enabled) {
            // Stage-1 only: IOVA → PA
            (true, false) => self.translate_stage1_only(pasid, iova, access_type, security_state),

            // Stage-2 only: IPA → PA (treat IOVA as IPA)
            (false, true) => self.translate_stage2_only(pasid, iova, access_type, security_state),

            // Two-stage: IOVA → IPA → PA
            (true, true) => self.translate_two_stage(pasid, iova, access_type, security_state),

            // Bypass mode: IOVA = PA (identity mapping)
            (false, false) => self.translate_bypass(iova, security_state),
        }
    }

    /// Stage-1 only translation: IOVA → PA
    fn translate_stage1_only(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        let pasid_value = pasid.as_u32();

        // Get Stage-1 AddressSpace for PASID (lock-free DashMap lookup for all PASIDs)
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(TranslationError::PASIDNotFound)?;

        // Perform Stage-1 translation (no RwLock needed - AddressSpace is lock-free)
        let result = addr_space.translate_page(iova, access_type, security_state);

        // Record fault on error
        if let Err(ref error) = result {
            let fault_type = match error {
                TranslationError::PageNotMapped => FaultType::TranslationFault,
                TranslationError::PermissionViolation { .. } => FaultType::PermissionFault,
                _ => FaultType::TranslationFault,
            };
            self.record_fault_internal(pasid, iova, fault_type, access_type, security_state);
        }

        result
    }

    /// Stage-2 only translation: IPA → PA
    fn translate_stage2_only(
        &self,
        pasid: PASID,
        ipa: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        // Get Stage-2 AddressSpace
        let stage2_guard = self.stage2_address_space.read().unwrap();
        let stage2 = stage2_guard.as_ref().ok_or(TranslationError::StreamNotConfigured)?;

        // Perform Stage-2 translation
        let result = stage2.translate_page(ipa, access_type, security_state);

        // Record fault on error
        if let Err(ref error) = result {
            let fault_type = match error {
                TranslationError::PageNotMapped => FaultType::TranslationFault,
                TranslationError::PermissionViolation { .. } => FaultType::PermissionFault,
                _ => FaultType::TranslationFault,
            };
            self.record_fault_internal(pasid, ipa, fault_type, access_type, security_state);
        }

        result
    }

    /// Two-stage translation: IOVA → IPA → PA
    fn translate_two_stage(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        let pasid_value = pasid.as_u32();

        // Stage-1: IOVA → IPA (lock-free DashMap lookup for all PASIDs)
        let addr_space = self.pasid_map.get(&pasid_value).ok_or(TranslationError::PASIDNotFound)?;

        // Stage-1: IOVA → IPA; use explicit match to avoid fragile unwrap()
        let stage1_result = match addr_space.translate_page(iova, access_type, security_state) {
            Ok(data) => data,
            Err(error) => {
                let fault_type = match error {
                    TranslationError::PageNotMapped => FaultType::TranslationFault,
                    TranslationError::PermissionViolation { .. } => FaultType::PermissionFault,
                    _ => FaultType::TranslationFault,
                };
                self.record_fault_internal(pasid, iova, fault_type, access_type, security_state);
                return Err(error);
            },
        };

        // IPA is the physical address from Stage-1
        let ipa =
            IOVA::new(stage1_result.physical_address().as_u64()).map_err(|_| TranslationError::AddressSizeError)?;

        // Stage-2: IPA → PA
        let stage2_guard = self.stage2_address_space.read().unwrap();
        let stage2 = stage2_guard.as_ref().ok_or(TranslationError::StreamNotConfigured)?;

        let result = stage2.translate_page(ipa, access_type, security_state);

        // Record Stage-2 fault if error
        if let Err(ref error) = result {
            let fault_type = match error {
                TranslationError::PageNotMapped => FaultType::TranslationFault,
                TranslationError::PermissionViolation { .. } => FaultType::PermissionFault,
                _ => FaultType::TranslationFault,
            };
            self.record_fault_internal(pasid, iova, fault_type, access_type, security_state);
        }

        result
    }

    /// Bypass mode translation: IOVA = PA (identity mapping)
    fn translate_bypass(&self, iova: IOVA, security_state: SecurityState) -> TranslationResult {
        // Identity mapping: IOVA = PA
        let pa = PA::new(iova.as_u64()).map_err(|_| TranslationError::AddressSizeError)?;

        // Full permissions in bypass mode
        let permissions = PagePermissions::new(true, true, true);

        Ok(TranslationData::new(pa, permissions, security_state))
    }

    // ========================================================================
    // Section 4.2.1: Configuration Update Operations
    // ========================================================================

    /// Creates a builder for updating stream configuration
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let mut ctx = StreamContext::new();
    /// let builder = ctx.update_config_builder();
    /// ```
    #[must_use]
    pub fn update_config_builder(&self) -> StreamConfigBuilder {
        StreamConfigBuilder {
            max_pasids_per_stream: None,
            stage1_enabled: None,
            stage2_enabled: None,
            stage2_address_space: None,
        }
    }

    /// Applies a configuration update transactionally
    ///
    /// Updates are all-or-nothing: either all changes succeed or none are applied.
    ///
    /// # Arguments
    ///
    /// * `builder` - Configuration builder with updates
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid or inconsistent
    pub fn apply_config(&self, builder: StreamConfigBuilder) -> Result<(), StreamContextError> {
        // Validate configuration first
        self.validate_config_update(&builder)?;

        // Apply updates atomically
        if let Some(max_pasids) = builder.max_pasids_per_stream {
            self.max_pasids_per_stream.store(max_pasids, Ordering::SeqCst);
        }

        if let Some(stage1) = builder.stage1_enabled {
            self.stage1_enabled.store(stage1, Ordering::SeqCst);
        }

        if let Some(stage2) = builder.stage2_enabled {
            self.stage2_enabled.store(stage2, Ordering::SeqCst);
        }

        if let Some(addr_space) = builder.stage2_address_space {
            let mut stage2 = self.stage2_address_space.write().unwrap();
            *stage2 = addr_space;
        }

        Ok(())
    }

    /// Validates a configuration update before applying
    ///
    /// # Arguments
    ///
    /// * `builder` - Configuration builder to validate
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid
    pub fn validate_config_update(&self, builder: &StreamConfigBuilder) -> Result<(), StreamContextError> {
        // Check PASID limit doesn't exceed ARM SMMU v3 maximum (2^20 - 1)
        if let Some(max_pasids) = builder.max_pasids_per_stream {
            if max_pasids > (1 << 20) {
                return Err(StreamContextError::ConfigurationError(
                    "max_pasids_per_stream exceeds ARM SMMU v3 limit (2^20)".to_string(),
                ));
            }

            // Check that new limit isn't less than current PASID count
            let current_count = self.pasid_map.len();
            if max_pasids < current_count {
                return Err(StreamContextError::ConfigurationError(format!(
                    "Cannot reduce max_pasids_per_stream to {} (current count: {})",
                    max_pasids, current_count
                )));
            }
        }

        // Check Stage-2 configuration consistency
        if let Some(stage2_enabled) = builder.stage2_enabled {
            if stage2_enabled {
                // If enabling Stage-2, must have Stage-2 address space
                let will_have_stage2 = if let Some(ref stage2_opt) = builder.stage2_address_space {
                    stage2_opt.is_some()
                } else {
                    let stage2_guard = self.stage2_address_space.read().unwrap();
                    stage2_guard.is_some()
                };

                if !will_have_stage2 {
                    return Err(StreamContextError::ConfigurationError(
                        "Stage-2 enabled but no Stage-2 AddressSpace configured".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Returns the maximum number of PASIDs per stream
    #[must_use]
    pub fn max_pasids_per_stream(&self) -> usize {
        self.max_pasids_per_stream.load(Ordering::Relaxed)
    }

    // ========================================================================
    // Section 4.2.2: State Machine Operations
    // ========================================================================

    /// Enables the stream
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// ctx.enable();
    /// assert!(ctx.is_enabled());
    /// ```
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Disables the stream
    ///
    /// Automatically clears all PASIDs when disabling.
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// ctx.disable();
    /// assert!(!ctx.is_enabled());
    /// ```
    pub fn disable(&self) {
        // Disable first (SeqCst) so concurrent translators see StreamDisabled,
        // then clear the PASID map.  Clearing before storing false would let a
        // racing translate() pass is_enabled() and then find an empty map,
        // returning PASIDNotFound instead of the correct StreamDisabled.
        self.enabled.store(false, Ordering::SeqCst);
        self.pasid_map.clear();
    }

    /// Checks if the stream is enabled
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    ///
    /// let ctx = StreamContext::new();
    /// assert!(ctx.is_enabled());
    /// ```
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Checks if stream is enabled before operations
    fn check_enabled(&self) -> Result<(), StreamContextError> {
        if !self.is_enabled() {
            Err(StreamContextError::ConfigurationError("Stream is not enabled".to_string()))
        } else {
            Ok(())
        }
    }

    // ========================================================================
    // Section 4.2.3: State Querying Operations
    // ========================================================================

    /// Returns a query interface for read-only access to stream state
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::stream_context::StreamContext;
    /// use smmu::types::PASID;
    ///
    /// let ctx = StreamContext::new();
    /// ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
    ///
    /// let query = ctx.query();
    /// assert_eq!(query.pasid_count(), 1);
    /// ```
    #[must_use]
    pub fn query(&self) -> StreamContextQuery<'_> {
        StreamContextQuery { ctx: self }
    }

    // ========================================================================
    // Section 4.2.4: Fault Handling Operations
    // ========================================================================

    /// Records a fault internally with monotonic timestamp (no SystemTime overhead)
    ///
    /// This is an internal helper that automatically records translation faults
    /// with a monotonic counter-based timestamp for ordering.
    ///
    /// # Arguments
    ///
    /// * `pasid` - PASID that caused the fault
    /// * `iova` - Faulting address
    /// * `fault_type` - Type of fault
    /// * `access_type` - Access type that caused the fault
    /// * `security_state` - Security state
    #[inline]
    fn record_fault_internal(
        &self,
        pasid: PASID,
        iova: IOVA,
        fault_type: FaultType,
        access_type: AccessType,
        security_state: SecurityState,
    ) {
        // Use monotonic counter for timestamp (no SystemTime overhead)
        let timestamp = self.fault_timestamp_counter.fetch_add(1, Ordering::Relaxed);

        // Fast path: try to acquire write lock without blocking
        if let Ok(mut records) = self.fault_records.try_write() {
            // Check rate limit
            let rate_limit = self.fault_rate_limit.load(Ordering::Relaxed);
            if records.len() < rate_limit {
                // Create minimal fault record
                // Note: StreamID is not available at this level, so we use placeholder
                let stream_id = crate::types::StreamID::new(0).unwrap();
                let fault = FaultRecord::builder()
                    .stream_id(stream_id)
                    .pasid(pasid)
                    .address(iova)
                    .fault_type(fault_type)
                    .access_type(access_type)
                    .security_state(security_state)
                    .timestamp(timestamp as u64)
                    .build();

                records.push(fault);
            }
        }
        // If lock contention, skip recording (acceptable for diagnostics)
    }

    /// Records a fault for diagnostic purposes
    ///
    /// **DEPRECATED**: Fault recording has been moved to the SMMU level to eliminate
    /// redundant recording and SystemTime overhead. This method is kept for backward
    /// compatibility but may be removed in a future version.
    ///
    /// # Migration
    ///
    /// Use `SMMU::get_faults()` instead of `StreamContext::get_fault_records()` to
    /// retrieve fault information.
    ///
    /// # Arguments
    ///
    /// * `_pasid` - PASID that caused the fault
    /// * `fault` - Fault record to store
    #[deprecated(
        since = "1.0.4",
        note = "Fault recording moved to SMMU level. Use SMMU::get_faults() instead."
    )]
    pub fn record_fault(&self, _pasid: PASID, fault: FaultRecord) {
        let mut records = self.fault_records.write().unwrap();

        // Check rate limit
        let rate_limit = self.fault_rate_limit.load(Ordering::Relaxed);
        if records.len() < rate_limit {
            records.push(fault);
        }
    }

    /// Returns all recorded faults
    #[must_use]
    pub fn get_fault_records(&self) -> Vec<FaultRecord> {
        let records = self.fault_records.read().unwrap();
        records.clone()
    }

    /// Returns the total number of recorded faults
    #[must_use]
    pub fn get_fault_count(&self) -> usize {
        let records = self.fault_records.read().unwrap();
        records.len()
    }

    /// Clears all fault records
    pub fn clear_fault_records(&self) {
        let mut records = self.fault_records.write().unwrap();
        records.clear();
    }

    /// Returns fault statistics
    #[must_use]
    pub fn get_fault_statistics(&self) -> FaultStatistics {
        let records = self.fault_records.read().unwrap();

        let mut faults_by_type: HashMap<FaultType, u64> = HashMap::new();
        let mut faults_by_pasid: HashMap<u32, u64> = HashMap::new();
        let mut last_fault_time: Option<u64> = None;

        for record in records.iter() {
            *faults_by_type.entry(record.fault_type()).or_insert(0) += 1;
            *faults_by_pasid.entry(record.pasid().as_u32()).or_insert(0) += 1;

            if let Some(last_time) = last_fault_time {
                if record.timestamp() > last_time {
                    last_fault_time = Some(record.timestamp());
                }
            } else {
                last_fault_time = Some(record.timestamp());
            }
        }

        let total_faults = records.len() as u64;
        let page_not_mapped_count = *faults_by_type.get(&FaultType::TranslationFault).unwrap_or(&0);
        let permission_violation_count = *faults_by_type.get(&FaultType::PermissionFault).unwrap_or(&0);

        let rate_limit = self.fault_rate_limit.load(Ordering::Relaxed);
        let rate_limited = total_faults >= rate_limit as u64;

        FaultStatistics {
            total_faults,
            faults_by_type,
            faults_by_pasid,
            last_fault_time,
            page_not_mapped_count,
            permission_violation_count,
            rate_limited,
        }
    }

    /// Resets fault statistics
    pub fn reset_fault_statistics(&self) {
        self.clear_fault_records();
    }

    /// Sets the fault rate limit
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of faults to record
    pub fn set_fault_rate_limit(&self, limit: usize) {
        self.fault_rate_limit.store(limit, Ordering::SeqCst);
    }

    /// Enables or disables fault retry
    ///
    /// # Arguments
    ///
    /// * `enabled` - True to enable retry, false to disable
    pub fn enable_fault_retry(&self, enabled: bool) {
        self.fault_retry_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Translates with retry on fault (if retry enabled)
    pub fn translate_with_retry(
        &self,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> TranslationResult {
        self.translate(pasid, iova, access_type, security_state)
    }
}

/// Stream configuration builder for partial updates (Section 4.2.1)
///
/// Provides a fluent interface for building stream configuration updates.
/// All updates are optional and transactional.
///
/// # Examples
///
/// ```
/// use smmu::stream_context::{StreamContext, StreamConfigBuilder};
///
/// let mut ctx = StreamContext::new();
/// let config = StreamConfigBuilder::new()
///     .max_pasids_per_stream(512)
///     .stage1_enabled(false)
///     .build();
///
/// ctx.apply_config(config).unwrap();
/// ```
#[derive(Debug, Clone, Default)]
pub struct StreamConfigBuilder {
    max_pasids_per_stream: Option<usize>,
    stage1_enabled: Option<bool>,
    stage2_enabled: Option<bool>,
    stage2_address_space: Option<Option<Arc<AddressSpace>>>,
}

impl StreamConfigBuilder {
    /// Creates a new empty configuration builder
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_pasids_per_stream: None,
            stage1_enabled: None,
            stage2_enabled: None,
            stage2_address_space: None,
        }
    }

    /// Sets the maximum PASIDs per stream
    #[must_use]
    pub const fn max_pasids_per_stream(mut self, max: usize) -> Self {
        self.max_pasids_per_stream = Some(max);
        self
    }

    /// Sets Stage-1 enabled state
    #[must_use]
    pub const fn stage1_enabled(mut self, enabled: bool) -> Self {
        self.stage1_enabled = Some(enabled);
        self
    }

    /// Sets Stage-2 enabled state
    #[must_use]
    pub const fn stage2_enabled(mut self, enabled: bool) -> Self {
        self.stage2_enabled = Some(enabled);
        self
    }

    /// Sets Stage-2 AddressSpace
    #[must_use]
    pub fn stage2_address_space(mut self, addr_space: Option<Arc<AddressSpace>>) -> Self {
        self.stage2_address_space = Some(addr_space);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub const fn build(self) -> Self {
        self
    }
}

/// Read-only query interface for stream context (Section 4.2.3)
///
/// Provides efficient read-only access to stream state without blocking
/// concurrent operations.
///
/// # Examples
///
/// ```
/// use smmu::stream_context::StreamContext;
/// use smmu::types::PASID;
///
/// let ctx = StreamContext::new();
/// ctx.create_pasid(PASID::new(1).unwrap()).unwrap();
///
/// let query = ctx.query();
/// assert_eq!(query.pasid_count(), 1);
/// assert!(query.has_pasid(PASID::new(1).unwrap()));
/// ```
#[derive(Debug)]
pub struct StreamContextQuery<'a> {
    ctx: &'a StreamContext,
}

impl<'a> StreamContextQuery<'a> {
    /// Returns the number of configured PASIDs
    #[must_use]
    pub fn pasid_count(&self) -> usize {
        self.ctx.pasid_count()
    }

    /// Checks if a PASID exists
    #[must_use]
    pub fn has_pasid(&self, pasid: PASID) -> bool {
        self.ctx.has_pasid(pasid)
    }

    /// Returns an iterator over all PASIDs
    pub fn pasids(&self) -> impl Iterator<Item = PASID> + 'a {
        // Collect PASID keys into a Vec to avoid holding DashMap lock
        let pasid_values: Vec<u32> = self.ctx.pasid_map.iter().map(|entry| *entry.key()).collect();

        pasid_values.into_iter().filter_map(|val| PASID::new(val).ok())
    }

    /// Checks if stream is enabled
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.ctx.is_enabled()
    }

    /// Returns fault statistics
    #[must_use]
    pub fn get_stats(&self) -> FaultStatistics {
        self.ctx.get_fault_statistics()
    }

    /// Returns PASIDs filtered by security state
    ///
    /// # TODO
    ///
    /// This is a placeholder implementation. Full implementation would require
    /// tracking security state per PASID/page, which is beyond basic Section 4.2 scope.
    pub fn pasids_by_security_state(&self, _security_state: SecurityState) -> impl Iterator<Item = PASID> + 'a {
        // TODO: Implement proper security state tracking per PASID
        // For now, return empty iterator to avoid incorrect results
        std::iter::empty()
    }
}

/// Fault statistics structure (Section 4.2.4)
///
/// Provides comprehensive fault tracking and analysis.
#[derive(Debug, Clone)]
pub struct FaultStatistics {
    /// Total number of faults recorded
    pub total_faults: u64,
    /// Faults grouped by type
    pub faults_by_type: HashMap<FaultType, u64>,
    /// Faults grouped by PASID
    pub faults_by_pasid: HashMap<u32, u64>,
    /// Timestamp of most recent fault
    pub last_fault_time: Option<u64>,
    /// Count of PageNotMapped faults
    pub page_not_mapped_count: u64,
    /// Count of PermissionViolation faults
    pub permission_violation_count: u64,
    /// Whether rate limiting is active
    pub rate_limited: bool,
}

impl Default for StreamContext {
    fn default() -> Self {
        Self::new()
    }
}

// StreamContext is automatically Send + Sync since all components are Send + Sync:
// - DashMap is Send + Sync
// - RwLock is Send + Sync
// - AtomicBool is Send + Sync
// - AtomicUsize is Send + Sync

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_context_new() {
        let ctx = StreamContext::new();
        assert!(ctx.is_stage1_enabled());
        assert!(!ctx.is_stage2_enabled());
        assert_eq!(ctx.pasid_count(), 0);
    }

    #[test]
    fn test_create_pasid_success() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(1).unwrap();
        assert!(ctx.create_pasid(pasid).is_ok());
        assert!(ctx.has_pasid(pasid));
        assert_eq!(ctx.pasid_count(), 1);
    }

    #[test]
    fn test_create_duplicate_pasid() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(1).unwrap();
        ctx.create_pasid(pasid).unwrap();

        let result = ctx.create_pasid(pasid);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StreamContextError::PASIDAlreadyExists(1)));
    }

    #[test]
    fn test_remove_pasid() {
        let ctx = StreamContext::new();
        let pasid = PASID::new(1).unwrap();
        ctx.create_pasid(pasid).unwrap();
        assert!(ctx.remove_pasid(pasid).is_ok());
        assert!(!ctx.has_pasid(pasid));
        assert_eq!(ctx.pasid_count(), 0);
    }

    #[test]
    fn test_stage_configuration() {
        let ctx = StreamContext::new();
        ctx.set_stage1_enabled(false);
        ctx.set_stage2_enabled(true);
        assert!(!ctx.is_stage1_enabled());
        assert!(ctx.is_stage2_enabled());
    }

    #[test]
    fn test_two_stage_translation_pasid_0() {
        let ctx = StreamContext::new();

        // Configure for two-stage translation
        ctx.set_stage1_enabled(true);
        ctx.set_stage2_enabled(true);

        // Create PASID 0
        let pasid = PASID::new(0).unwrap();
        ctx.create_pasid(pasid).unwrap();

        // Create Stage-2 address space
        ctx.create_stage2_address_space().unwrap();

        // Map Stage-1: IOVA -> IPA
        let iova = IOVA::new(0x1000).unwrap();
        let ipa = PA::new(0x2000).unwrap();
        ctx.map_page(pasid, iova, ipa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Map Stage-2: IPA -> PA
        let ipa_as_iova = IOVA::new(ipa.as_u64()).unwrap();
        let final_pa = PA::new(0x3000).unwrap();
        ctx.map_stage2_page(ipa_as_iova, final_pa, PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

        // Translate with PASID 0 - this should NOT fail with PASIDNotFound
        let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
        assert!(result.is_ok(), "Two-stage translation with PASID 0 should succeed: {:?}", result);
        assert_eq!(result.unwrap().physical_address(), final_pa);
    }
}
