//! Access type definitions for ARM SMMU v3
//!
//! This module defines memory access types and permission checking operations
//! following the ARM SMMU v3 specification.
//!
//! # Access Types
//!
//! ARM SMMU v3 defines three basic access permission bits:
//! - Read (R) - bit 0
//! - Write (W) - bit 1
//! - Execute (X) - bit 2
//!
//! These can be combined to form compound permissions like ReadWrite, ReadExecute, etc.

use crate::types::ValidationError;
use core::fmt;

/// Memory access type
///
/// Represents the type of memory access being performed or permitted.
/// Corresponds to ARM SMMU v3 access permission bits (R/W/X) plus a privilege bit.
///
/// # Encoding
///
/// Bits 0-2 encode R/W/X; bit 3 encodes the privilege flag (AxPROT[1]):
/// - Bit 0: Read
/// - Bit 1: Write
/// - Bit 2: Execute
/// - Bit 3: Privileged (set for *Privileged variants — see `is_privileged()`)
///
/// The `can_read()`, `can_write()`, `can_execute()` methods mask out bit 3,
/// so `ReadPrivileged.can_read() == true` and `ReadPrivileged.can_write() == false`.
///
/// # Example
///
/// ```
/// use smmu::AccessType;
///
/// let rw = AccessType::ReadWrite;
/// assert!(rw.can_read());
/// assert!(rw.can_write());
/// assert!(!rw.can_execute());
/// assert!(!rw.is_privileged());
///
/// let rp = AccessType::ReadPrivileged;
/// assert!(rp.can_read());
/// assert!(!rp.can_write());
/// assert!(rp.is_privileged());
///
/// // Check permissions
/// let page_perms = AccessType::ReadWriteExecute;
/// assert!(page_perms.has_permission(AccessType::Read));
/// assert!(page_perms.has_permission(AccessType::Write));
/// ```
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AccessType {
    /// No permissions
    None = 0b0000,

    /// Read access only
    Read = 0b0001,

    /// Write access only (unusual but valid)
    Write = 0b0010,

    /// Read and Write access
    ReadWrite = 0b0011,

    /// Execute access only
    Execute = 0b0100,

    /// Read and Execute access
    ReadExecute = 0b0101,

    /// Write and Execute access (unusual but valid)
    WriteExecute = 0b0110,

    /// Full permissions (Read + Write + Execute)
    ReadWriteExecute = 0b0111,

    // ---- Bug-4 fix: privileged variants (AxPROT[1]=1) ----
    // Bit 3 set signals that the transaction is a privileged access.
    // can_read/write/execute() mask off bit 3 so the R/W/X semantics
    // are preserved for all callers.

    /// Privileged read access — AxPROT[1]=1, AxPROT[2]=0 (data read, privileged).
    ReadPrivileged = 0b1001,

    /// Privileged write access — AxPROT[1]=1 (data write, privileged).
    WritePrivileged = 0b1010,

    /// Privileged read+write access.
    ReadWritePrivileged = 0b1011,

    /// Privileged execute access — AxPROT[1]=1, AxPROT[2]=1 (instruction, privileged).
    ExecutePrivileged = 0b1100,
}

impl AccessType {
    /// Check if read access is permitted
    ///
    /// Returns true for both `Read` and `ReadPrivileged` (and any combined variant
    /// that includes the read bit).  Bit 3 (privilege flag) is masked out.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// assert!(AccessType::Read.can_read());
    /// assert!(AccessType::ReadWrite.can_read());
    /// assert!(!AccessType::Write.can_read());
    /// assert!(AccessType::ReadPrivileged.can_read());
    /// assert!(!AccessType::WritePrivileged.can_read());
    /// ```
    #[inline]
    #[must_use]
    pub const fn can_read(self) -> bool {
        (self as u8 & 0b001) != 0
    }

    /// Check if write access is permitted
    ///
    /// Returns true for both `Write` and `WritePrivileged` (and combined variants).
    /// Bit 3 (privilege flag) is masked out.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// assert!(AccessType::Write.can_write());
    /// assert!(AccessType::ReadWrite.can_write());
    /// assert!(!AccessType::Read.can_write());
    /// assert!(AccessType::WritePrivileged.can_write());
    /// assert!(!AccessType::ReadPrivileged.can_write());
    /// ```
    #[inline]
    #[must_use]
    pub const fn can_write(self) -> bool {
        (self as u8 & 0b010) != 0
    }

    /// Check if execute access is permitted
    ///
    /// Returns true for both `Execute` and `ExecutePrivileged`.
    /// Bit 3 (privilege flag) is masked out.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// assert!(AccessType::Execute.can_execute());
    /// assert!(AccessType::ReadExecute.can_execute());
    /// assert!(!AccessType::Read.can_execute());
    /// assert!(AccessType::ExecutePrivileged.can_execute());
    /// assert!(!AccessType::ReadPrivileged.can_execute());
    /// ```
    #[inline]
    #[must_use]
    pub const fn can_execute(self) -> bool {
        (self as u8 & 0b100) != 0
    }

    /// Check if this is a privileged access (AxPROT\[1\]=1).
    ///
    /// Returns `true` for the four `*Privileged` variants:
    /// `ReadPrivileged`, `WritePrivileged`, `ReadWritePrivileged`, `ExecutePrivileged`.
    /// Returns `false` for all other (non-privileged) variants.
    ///
    /// Bug-4 fix: ARM IHI0070G.b §7.3 (EventEntry.PnU) — for PRIVCFG=Use Incoming,
    /// pnu must reflect the AxPROT\[1\] bit of the incoming transaction.  Use
    /// `is_privileged()` to determine whether the incoming AccessType is privileged.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// assert!(!AccessType::Read.is_privileged());
    /// assert!(!AccessType::ReadWrite.is_privileged());
    /// assert!(AccessType::ReadPrivileged.is_privileged());
    /// assert!(AccessType::WritePrivileged.is_privileged());
    /// assert!(AccessType::ReadWritePrivileged.is_privileged());
    /// assert!(AccessType::ExecutePrivileged.is_privileged());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_privileged(self) -> bool {
        (self as u8 & 0b1000) != 0
    }

    /// Const version of can_read for compile-time evaluation
    #[inline]
    #[must_use]
    pub const fn const_can_read(self) -> bool {
        self.can_read()
    }

    /// Const version of can_write for compile-time evaluation
    #[inline]
    #[must_use]
    pub const fn const_can_write(self) -> bool {
        self.can_write()
    }

    /// Const version of can_execute for compile-time evaluation
    #[inline]
    #[must_use]
    pub const fn const_can_execute(self) -> bool {
        self.can_execute()
    }

    /// Check if this access type contains the requested permissions
    ///
    /// Returns true if all permissions in `requested` are present in `self`.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// let page_perms = AccessType::ReadWrite;
    /// assert!(page_perms.has_permission(AccessType::Read));
    /// assert!(page_perms.has_permission(AccessType::Write));
    /// assert!(!page_perms.has_permission(AccessType::Execute));
    /// ```
    #[inline]
    #[must_use]
    pub const fn has_permission(self, requested: Self) -> bool {
        (self as u8 & requested as u8) == requested as u8
    }

    /// Compute the intersection of two permission sets
    ///
    /// Returns the common permissions between two AccessType values.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// let perm1 = AccessType::ReadWrite;
    /// let perm2 = AccessType::Read;
    /// assert_eq!(perm1.intersect(perm2), AccessType::Read);
    /// ```
    #[inline]
    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        let bits = self as u8 & other as u8;
        // Safe because we're ANDing two valid bit patterns
        Self::from_bits_unchecked(bits)
    }

    /// Compute the union of two permission sets
    ///
    /// Returns the combined permissions of two AccessType values.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// let perm1 = AccessType::Read;
    /// let perm2 = AccessType::Write;
    /// assert_eq!(perm1.union(perm2), AccessType::ReadWrite);
    /// ```
    #[inline]
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        let bits = self as u8 | other as u8;
        // Safe because we're ORing two valid bit patterns
        Self::from_bits_unchecked(bits)
    }

    /// Convert to ARM SMMU v3 bit representation
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// assert_eq!(AccessType::Read.to_bits(), 0b001);
    /// assert_eq!(AccessType::ReadWrite.to_bits(), 0b011);
    /// ```
    #[inline]
    #[must_use]
    pub const fn to_bits(self) -> u8 {
        self as u8
    }

    /// Create from ARM SMMU v3 bit representation
    ///
    /// Accepts non-privileged values 0b0000-0b0111 and privileged values
    /// 0b1001-0b1100 (bit 3 set).  Bit pattern 0b1000 (privilege-only, no R/W/X)
    /// and reserved patterns (0b1101-0b1111) return an error.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the bit pattern is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// assert_eq!(AccessType::from_bits(0b001).unwrap(), AccessType::Read);
    /// assert_eq!(AccessType::from_bits(0b011).unwrap(), AccessType::ReadWrite);
    /// assert_eq!(AccessType::from_bits(0b1001).unwrap(), AccessType::ReadPrivileged);
    /// assert!(AccessType::from_bits(0b1000).is_err()); // privilege-only, no R/W/X
    /// assert!(AccessType::from_bits(0b1000_0000).is_err());
    /// ```
    #[inline]
    pub const fn from_bits(bits: u8) -> Result<Self, ValidationError> {
        match bits {
            0b0000..=0b0111 | 0b1001 | 0b1010 | 0b1011 | 0b1100 => {
                Ok(Self::from_bits_unchecked(bits))
            }
            _ => Err(ValidationError::InvalidAccessType { bits }),
        }
    }

    /// Create from bits without validation (internal use)
    #[inline]
    const fn from_bits_unchecked(bits: u8) -> Self {
        match bits {
            0b0000 => Self::None,
            0b0001 => Self::Read,
            0b0010 => Self::Write,
            0b0011 => Self::ReadWrite,
            0b0100 => Self::Execute,
            0b0101 => Self::ReadExecute,
            0b0110 => Self::WriteExecute,
            0b0111 => Self::ReadWriteExecute,
            0b1001 => Self::ReadPrivileged,
            0b1010 => Self::WritePrivileged,
            0b1011 => Self::ReadWritePrivileged,
            0b1100 => Self::ExecutePrivileged,
            _ => Self::None, // Fallback for invalid bits
        }
    }

    /// Validate that the requested access is permitted by the available permissions
    ///
    /// # Errors
    ///
    /// Returns `Err` if the requested access is not fully contained in the available permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use smmu::AccessType;
    /// let page_perms = AccessType::ReadWrite;
    /// assert!(AccessType::Read.validate_against(page_perms).is_ok());
    /// assert!(AccessType::Write.validate_against(page_perms).is_ok());
    /// assert!(AccessType::Execute.validate_against(page_perms).is_err());
    /// ```
    #[inline]
    pub fn validate_against(self, available: Self) -> Result<(), ValidationError> {
        if available.has_permission(self) {
            Ok(())
        } else {
            Err(ValidationError::PermissionDenied {
                requested: format!("{self}"),
                available: format!("{available}"),
            })
        }
    }
}

impl fmt::Display for AccessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::None => "None",
            Self::Read => "Read",
            Self::Write => "Write",
            Self::ReadWrite => "ReadWrite",
            Self::Execute => "Execute",
            Self::ReadExecute => "ReadExecute",
            Self::WriteExecute => "WriteExecute",
            Self::ReadWriteExecute => "ReadWriteExecute",
            Self::ReadPrivileged => "ReadPrivileged",
            Self::WritePrivileged => "WritePrivileged",
            Self::ReadWritePrivileged => "ReadWritePrivileged",
            Self::ExecutePrivileged => "ExecutePrivileged",
        };
        write!(f, "{s}")
    }
}

impl Default for AccessType {
    fn default() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_type_bits() {
        assert_eq!(AccessType::None.to_bits(), 0b000);
        assert_eq!(AccessType::Read.to_bits(), 0b001);
        assert_eq!(AccessType::Write.to_bits(), 0b010);
        assert_eq!(AccessType::ReadWrite.to_bits(), 0b011);
        assert_eq!(AccessType::Execute.to_bits(), 0b100);
        assert_eq!(AccessType::ReadExecute.to_bits(), 0b101);
        assert_eq!(AccessType::WriteExecute.to_bits(), 0b110);
        assert_eq!(AccessType::ReadWriteExecute.to_bits(), 0b111);
    }

    #[test]
    fn test_access_type_from_bits() {
        for bits in 0..=0b111 {
            let access = AccessType::from_bits(bits).unwrap();
            assert_eq!(access.to_bits(), bits);
        }
    }

    #[test]
    fn test_permissions() {
        assert!(AccessType::Read.can_read());
        assert!(!AccessType::Read.can_write());
        assert!(!AccessType::Read.can_execute());

        assert!(AccessType::ReadWrite.can_read());
        assert!(AccessType::ReadWrite.can_write());
        assert!(!AccessType::ReadWrite.can_execute());

        assert!(AccessType::ReadWriteExecute.can_read());
        assert!(AccessType::ReadWriteExecute.can_write());
        assert!(AccessType::ReadWriteExecute.can_execute());
    }
}
