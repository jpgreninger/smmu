//! Page Entry structures for ARM SMMU v3
//!
//! This module defines the PageEntry and PagePermissions structures used for
//! page table entries in address translation. All implementations are safe
//! with zero unsafe code.

use super::{AccessType, PA, SecurityState};

/// Page access permissions structure
///
/// Defines read/write/execute permissions for memory pages following ARM
/// architecture memory permissions model. This structure is Copy and uses
/// compact representation for cache efficiency.
///
/// # Examples
///
/// ```
/// use smmu::types::PagePermissions;
///
/// // Create read-only permissions
/// let read_only = PagePermissions::read_only();
/// assert!(read_only.read());
/// assert!(!read_only.write());
///
/// // Create read-write permissions
/// let read_write = PagePermissions::read_write();
/// assert!(read_write.read() && read_write.write());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PagePermissions {
    /// Read permission allowed
    read: bool,
    /// Write permission allowed
    write: bool,
    /// Execute permission allowed
    execute: bool,
}

impl PagePermissions {
    /// Creates new PagePermissions with explicit flags
    ///
    /// # Arguments
    ///
    /// * `read` - Read permission
    /// * `write` - Write permission
    /// * `execute` - Execute permission
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::PagePermissions;
    ///
    /// let perms = PagePermissions::new(true, false, false);
    /// assert!(perms.read());
    /// ```
    #[must_use]
    #[inline]
    pub const fn new(read: bool, write: bool, execute: bool) -> Self {
        Self {
            read,
            write,
            execute,
        }
    }

    /// Creates permissions with no access allowed
    #[must_use]
    #[inline]
    pub const fn none() -> Self {
        Self::new(false, false, false)
    }

    /// Creates read-only permissions
    #[must_use]
    #[inline]
    pub const fn read_only() -> Self {
        Self::new(true, false, false)
    }

    /// Creates write-only permissions
    #[must_use]
    #[inline]
    pub const fn write_only() -> Self {
        Self::new(false, true, false)
    }

    /// Creates execute-only permissions
    #[must_use]
    #[inline]
    pub const fn execute_only() -> Self {
        Self::new(false, false, true)
    }

    /// Creates read-write permissions
    #[must_use]
    #[inline]
    pub const fn read_write() -> Self {
        Self::new(true, true, false)
    }

    /// Creates read-execute permissions
    #[must_use]
    #[inline]
    pub const fn read_execute() -> Self {
        Self::new(true, false, true)
    }

    /// Creates all permissions (read, write, execute)
    #[must_use]
    #[inline]
    pub const fn all() -> Self {
        Self::new(true, true, true)
    }

    /// Returns true if read permission is allowed
    #[must_use]
    #[inline]
    pub const fn read(self) -> bool {
        self.read
    }

    /// Returns true if write permission is allowed
    #[must_use]
    #[inline]
    pub const fn write(self) -> bool {
        self.write
    }

    /// Returns true if execute permission is allowed
    #[must_use]
    #[inline]
    pub const fn execute(self) -> bool {
        self.execute
    }

    /// Checks if the given access type is allowed
    ///
    /// # Arguments
    ///
    /// * `access` - The access type to check
    ///
    /// # Returns
    ///
    /// `true` if the access is allowed, `false` otherwise
    #[must_use]
    pub const fn allows(self, access: AccessType) -> bool {
        match access {
            AccessType::None => true, // No access required, always allowed
            AccessType::Read => self.read,
            AccessType::Write => self.write,
            AccessType::Execute => self.execute,
            AccessType::ReadWrite => self.read && self.write,
            AccessType::ReadExecute => self.read && self.execute,
            AccessType::WriteExecute => self.write && self.execute,
            AccessType::ReadWriteExecute => self.read && self.write && self.execute,
        }
    }

    /// Returns the union of two permission sets
    ///
    /// # Arguments
    ///
    /// * `other` - The other permission set
    ///
    /// # Returns
    ///
    /// A new `PagePermissions` with permissions from both sets
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self::new(
            self.read || other.read,
            self.write || other.write,
            self.execute || other.execute,
        )
    }

    /// Returns the intersection of two permission sets
    ///
    /// # Arguments
    ///
    /// * `other` - The other permission set
    ///
    /// # Returns
    ///
    /// A new `PagePermissions` with only permissions present in both sets
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self::new(
            self.read && other.read,
            self.write && other.write,
            self.execute && other.execute,
        )
    }

    /// Checks if this permission set is a subset of another
    ///
    /// # Arguments
    ///
    /// * `other` - The other permission set
    ///
    /// # Returns
    ///
    /// `true` if all permissions in `self` are also in `other`
    #[must_use]
    pub const fn is_subset_of(self, other: &Self) -> bool {
        (!self.read || other.read)
            && (!self.write || other.write)
            && (!self.execute || other.execute)
    }
}

impl Default for PagePermissions {
    /// Creates PagePermissions with no access allowed (secure default)
    fn default() -> Self {
        Self::none()
    }
}

/// Page table entry structure
///
/// Represents a single page table entry containing physical address mapping,
/// permissions, security state, and memory attributes. This structure follows
/// ARM SMMU v3 specification requirements.
///
/// # Memory Safety
///
/// This structure uses zero unsafe code and leverages Rust's ownership system
/// for safe memory management.
///
/// # Examples
///
/// ```
/// use smmu::types::{PageEntry, PagePermissions, PA, SecurityState};
///
/// let pa = PA::new(0x1000).unwrap();
/// let perms = PagePermissions::read_write();
/// let entry = PageEntry::new(pa, perms);
///
/// assert!(entry.is_valid());
/// assert_eq!(entry.physical_address(), pa);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageEntry {
    /// Physical address mapping
    physical_address: PA,
    /// Page permissions
    permissions: PagePermissions,
    /// Entry validity flag
    valid: bool,
    /// Security state
    security_state: SecurityState,
    /// Cacheable attribute
    cacheable: bool,
    /// Shareable attribute
    shareable: bool,
    /// Device memory attribute
    device_memory: bool,
}

impl PageEntry {
    /// Creates a new valid PageEntry with physical address and permissions
    ///
    /// Security state defaults to NonSecure, and memory attributes default to
    /// normal memory (cacheable, shareable, not device memory).
    ///
    /// # Arguments
    ///
    /// * `physical_address` - Physical address for this entry
    /// * `permissions` - Access permissions
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{PageEntry, PagePermissions, PA};
    ///
    /// let pa = PA::new(0x1000).unwrap();
    /// let entry = PageEntry::new(pa, PagePermissions::read_only());
    /// assert!(entry.is_valid());
    /// ```
    #[must_use]
    pub const fn new(physical_address: PA, permissions: PagePermissions) -> Self {
        Self {
            physical_address,
            permissions,
            valid: true,
            security_state: SecurityState::NonSecure,
            cacheable: true,
            shareable: true,
            device_memory: false,
        }
    }

    /// Creates a new PageEntry with explicit security state
    ///
    /// # Arguments
    ///
    /// * `physical_address` - Physical address for this entry
    /// * `permissions` - Access permissions
    /// * `security_state` - Security state (Secure/NonSecure/Realm)
    #[must_use]
    pub const fn with_security_state(
        physical_address: PA,
        permissions: PagePermissions,
        security_state: SecurityState,
    ) -> Self {
        Self {
            physical_address,
            permissions,
            valid: true,
            security_state,
            cacheable: true,
            shareable: true,
            device_memory: false,
        }
    }

    /// Creates a builder for constructing PageEntry
    #[must_use]
    pub const fn builder() -> PageEntryBuilder {
        PageEntryBuilder::new()
    }

    /// Returns the physical address
    #[must_use]
    #[inline]
    pub const fn physical_address(&self) -> PA {
        self.physical_address
    }

    /// Returns the permissions
    #[must_use]
    #[inline]
    pub const fn permissions(&self) -> PagePermissions {
        self.permissions
    }

    /// Returns the security state
    #[must_use]
    #[inline]
    pub const fn security_state(&self) -> SecurityState {
        self.security_state
    }

    /// Returns true if entry is valid
    #[must_use]
    #[inline]
    pub const fn is_valid(&self) -> bool {
        self.valid
    }

    /// Returns true if entry is cacheable
    #[must_use]
    #[inline]
    pub const fn is_cacheable(&self) -> bool {
        self.cacheable
    }

    /// Returns true if entry is shareable
    #[must_use]
    #[inline]
    pub const fn is_shareable(&self) -> bool {
        self.shareable
    }

    /// Returns true if entry represents device memory
    #[must_use]
    #[inline]
    pub const fn is_device_memory(&self) -> bool {
        self.device_memory
    }

    /// Marks the entry as valid
    #[must_use]
    pub const fn mark_valid(mut self) -> Self {
        self.valid = true;
        self
    }

    /// Marks the entry as invalid
    #[must_use]
    pub const fn mark_invalid(mut self) -> Self {
        self.valid = false;
        self
    }

    /// Sets the cacheable attribute
    #[must_use]
    pub const fn with_cacheable(mut self, cacheable: bool) -> Self {
        self.cacheable = cacheable;
        // Device memory cannot be cacheable
        if self.device_memory && cacheable {
            self.cacheable = false;
        }
        self
    }

    /// Sets the shareable attribute
    #[must_use]
    pub const fn with_shareable(mut self, shareable: bool) -> Self {
        self.shareable = shareable;
        self
    }

    /// Sets the device memory attribute
    #[must_use]
    pub const fn with_device_memory(mut self, device_memory: bool) -> Self {
        self.device_memory = device_memory;
        // Device memory cannot be cacheable
        if device_memory {
            self.cacheable = false;
        }
        self
    }

    /// Updates the permissions
    #[must_use]
    pub const fn with_permissions(mut self, permissions: PagePermissions) -> Self {
        self.permissions = permissions;
        self
    }
}

impl Default for PageEntry {
    /// Creates an invalid PageEntry with zero physical address
    fn default() -> Self {
        Self {
            physical_address: PA::new(0).unwrap_or_else(|_| unreachable!()),
            permissions: PagePermissions::default(),
            valid: false,
            security_state: SecurityState::NonSecure,
            cacheable: false,
            shareable: false,
            device_memory: false,
        }
    }
}

/// Builder for PageEntry with compile-time validation
///
/// Provides a fluent interface for constructing `PageEntry` instances.
///
/// # Examples
///
/// ```
/// use smmu::types::{PageEntry, PagePermissions, PA, SecurityState};
///
/// let pa = PA::new(0x2000).unwrap();
/// let entry = PageEntry::builder()
///     .physical_address(pa)
///     .permissions(PagePermissions::read_execute())
///     .security_state(SecurityState::Secure)
///     .cacheable(true)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct PageEntryBuilder {
    physical_address: Option<PA>,
    permissions: PagePermissions,
    security_state: SecurityState,
    cacheable: bool,
    shareable: bool,
    device_memory: bool,
}

impl PageEntryBuilder {
    /// Creates a new builder with default values
    #[must_use]
    const fn new() -> Self {
        Self {
            physical_address: None,
            permissions: PagePermissions::none(),
            security_state: SecurityState::NonSecure,
            cacheable: true,
            shareable: true,
            device_memory: false,
        }
    }

    /// Sets the physical address
    #[must_use]
    pub const fn physical_address(mut self, pa: PA) -> Self {
        self.physical_address = Some(pa);
        self
    }

    /// Sets the permissions
    #[must_use]
    pub const fn permissions(mut self, perms: PagePermissions) -> Self {
        self.permissions = perms;
        self
    }

    /// Sets the security state
    #[must_use]
    pub const fn security_state(mut self, state: SecurityState) -> Self {
        self.security_state = state;
        self
    }

    /// Sets the cacheable attribute
    #[must_use]
    pub const fn cacheable(mut self, cacheable: bool) -> Self {
        self.cacheable = cacheable;
        self
    }

    /// Sets the shareable attribute
    #[must_use]
    pub const fn shareable(mut self, shareable: bool) -> Self {
        self.shareable = shareable;
        self
    }

    /// Sets the device memory attribute
    #[must_use]
    pub const fn device_memory(mut self, device_memory: bool) -> Self {
        self.device_memory = device_memory;
        self
    }

    /// Builds the PageEntry
    ///
    /// # Panics
    ///
    /// Panics if physical address was not set
    #[must_use]
    pub fn build(self) -> PageEntry {
        let physical_address = self
            .physical_address
            .expect("Physical address must be set");

        let mut entry = PageEntry {
            physical_address,
            permissions: self.permissions,
            valid: true,
            security_state: self.security_state,
            cacheable: self.cacheable,
            shareable: self.shareable,
            device_memory: self.device_memory,
        };

        // Enforce device memory constraints
        if entry.device_memory {
            entry.cacheable = false;
        }

        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_permissions_basic() {
        let perms = PagePermissions::new(true, false, true);
        assert!(perms.read());
        assert!(!perms.write());
        assert!(perms.execute());
    }

    #[test]
    fn test_page_entry_basic() {
        let pa = PA::new(0x1000).unwrap();
        let perms = PagePermissions::read_write();
        let entry = PageEntry::new(pa, perms);

        assert!(entry.is_valid());
        assert_eq!(entry.physical_address(), pa);
        assert_eq!(entry.permissions(), perms);
    }

    #[test]
    fn test_device_memory_not_cacheable() {
        let pa = PA::new(0x2000).unwrap();
        let entry = PageEntry::new(pa, PagePermissions::default())
            .with_device_memory(true)
            .with_cacheable(true);

        assert!(entry.is_device_memory());
        assert!(!entry.is_cacheable(), "Device memory should not be cacheable");
    }
}
