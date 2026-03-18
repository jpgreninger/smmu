//! Address and Permission Validation for ARM SMMU v3
//!
//! This module provides specialized validators for address ranges and permissions
//! following ARM SMMU v3 specification requirements.

use crate::types::{
    AccessType, FaultRecord, FaultSyndrome, FaultType, PagePermissions, SecurityState, StreamID, IOVA, PASID,
};

use super::detection::{AddressSize, FaultDetectionResult};

/// Permission validator with detailed violation reporting
#[derive(Debug, Clone)]
pub struct PermissionValidator;

impl PermissionValidator {
    /// Validate read permission
    ///
    /// # Arguments
    ///
    /// * `permissions` - Page permissions to check
    ///
    /// # Returns
    ///
    /// `true` if read is allowed
    #[must_use]
    #[inline]
    pub const fn can_read(permissions: PagePermissions) -> bool {
        permissions.read()
    }

    /// Validate write permission
    ///
    /// # Arguments
    ///
    /// * `permissions` - Page permissions to check
    ///
    /// # Returns
    ///
    /// `true` if write is allowed
    #[must_use]
    #[inline]
    pub const fn can_write(permissions: PagePermissions) -> bool {
        permissions.write()
    }

    /// Validate execute permission
    ///
    /// # Arguments
    ///
    /// * `permissions` - Page permissions to check
    ///
    /// # Returns
    ///
    /// `true` if execute is allowed
    #[must_use]
    #[inline]
    pub const fn can_execute(permissions: PagePermissions) -> bool {
        permissions.execute()
    }

    /// Check if permissions allow the requested access type
    ///
    /// # Arguments
    ///
    /// * `permissions` - Page permissions to check
    /// * `access_type` - Requested access type
    ///
    /// # Returns
    ///
    /// `true` if access is permitted
    #[must_use]
    pub const fn allows_access(permissions: PagePermissions, access_type: AccessType) -> bool {
        match access_type {
            AccessType::None => true, // No access requested always succeeds
            AccessType::Read => Self::can_read(permissions),
            AccessType::Write => Self::can_write(permissions),
            AccessType::Execute => Self::can_execute(permissions),
            AccessType::ReadWrite => Self::can_read(permissions) && Self::can_write(permissions),
            AccessType::ReadExecute => Self::can_read(permissions) && Self::can_execute(permissions),
            AccessType::WriteExecute => Self::can_write(permissions) && Self::can_execute(permissions),
            AccessType::ReadWriteExecute => {
                Self::can_read(permissions) && Self::can_write(permissions) && Self::can_execute(permissions)
            },
            // Bug-4 fix: privileged variants — privilege bit does not affect R/W/X permission check.
            AccessType::ReadPrivileged => Self::can_read(permissions),
            AccessType::WritePrivileged => Self::can_write(permissions),
            AccessType::ReadWritePrivileged => Self::can_read(permissions) && Self::can_write(permissions),
            AccessType::ExecutePrivileged => Self::can_execute(permissions),
        }
    }

    /// Get a detailed description of permission violation
    ///
    /// # Arguments
    ///
    /// * `permissions` - Page permissions
    /// * `access_type` - Requested access type that was denied
    ///
    /// # Returns
    ///
    /// Human-readable description of the violation
    #[must_use]
    pub fn violation_description(permissions: PagePermissions, access_type: AccessType) -> String {
        let perm_str = format!(
            "{}{}{}",
            if permissions.read() { "R" } else { "-" },
            if permissions.write() { "W" } else { "-" },
            if permissions.execute() { "X" } else { "-" }
        );

        let access_str = match access_type {
            AccessType::None => "no",
            AccessType::Read => "read",
            AccessType::Write => "write",
            AccessType::Execute => "execute",
            AccessType::ReadWrite => "read+write",
            AccessType::ReadExecute => "read+execute",
            AccessType::WriteExecute => "write+execute",
            AccessType::ReadWriteExecute => "read+write+execute",
            // Bug-4 fix: privileged variants.
            AccessType::ReadPrivileged => "read(privileged)",
            AccessType::WritePrivileged => "write(privileged)",
            AccessType::ReadWritePrivileged => "read+write(privileged)",
            AccessType::ExecutePrivileged => "execute(privileged)",
        };

        format!(
            "Permission violation: attempted {} access on page with {} permissions",
            access_str, perm_str
        )
    }
}

/// Address range validator with boundary checking
#[derive(Debug, Clone)]
pub struct AddressRangeValidator {
    address_size: AddressSize,
    timestamp_generator: u64,
}

impl AddressRangeValidator {
    /// Create a new address range validator
    ///
    /// # Arguments
    ///
    /// * `address_size` - Address size configuration
    #[must_use]
    pub const fn new(address_size: AddressSize) -> Self {
        Self { address_size, timestamp_generator: 0 }
    }

    /// Check if an address is within valid range
    ///
    /// # Arguments
    ///
    /// * `addr` - Address to validate
    ///
    /// # Returns
    ///
    /// `true` if address is within valid range
    #[must_use]
    pub const fn is_valid_address(&self, addr: u64) -> bool {
        addr <= self.address_size.max_address()
    }

    /// Validate address is within supported range
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Source stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Address to validate
    /// * `access_type` - Access type
    /// * `security_state` - Security state context
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, `Err(FaultRecord)` with address size fault otherwise
    pub fn validate_range(
        &mut self,
        stream_id: StreamID,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> FaultDetectionResult {
        if !self.is_valid_address(iova.as_u64()) {
            self.timestamp_generator += 1;

            let syndrome = FaultSyndrome::builder()
                .syndrome_register(0x0200_0000)
                .fault_level(0)
                .write_not_read(access_type == AccessType::Write)
                .valid_syndrome(true)
                .build();

            Err(FaultRecord::builder()
                .stream_id(stream_id)
                .pasid(pasid)
                .address(iova)
                .fault_type(FaultType::AddressSizeFault)
                .access_type(access_type)
                .security_state(security_state)
                .syndrome(syndrome)
                .timestamp(self.timestamp_generator)
                .build())
        } else {
            Ok(())
        }
    }

    /// Check if address is page-aligned
    ///
    /// # Arguments
    ///
    /// * `iova` - Address to check
    ///
    /// # Returns
    ///
    /// `true` if page-aligned (lower 12 bits are zero)
    #[must_use]
    pub fn is_page_aligned(iova: IOVA) -> bool {
        iova.is_page_aligned()
    }

    /// Validate page alignment
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Source stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `iova` - Address to validate
    /// * `access_type` - Access type
    /// * `security_state` - Security state context
    ///
    /// # Returns
    ///
    /// `Ok(())` if aligned, `Err(FaultRecord)` with alignment fault otherwise
    pub fn validate_page_alignment(
        &mut self,
        stream_id: StreamID,
        pasid: PASID,
        iova: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> FaultDetectionResult {
        if !Self::is_page_aligned(iova) {
            self.timestamp_generator += 1;

            let syndrome = FaultSyndrome::builder()
                .syndrome_register(0x0800_0000)
                .fault_level(0)
                .write_not_read(access_type == AccessType::Write)
                .valid_syndrome(true)
                .build();

            Err(FaultRecord::builder()
                .stream_id(stream_id)
                .pasid(pasid)
                .address(iova)
                .fault_type(FaultType::AlignmentFault)
                .access_type(access_type)
                .security_state(security_state)
                .syndrome(syndrome)
                .timestamp(self.timestamp_generator)
                .build())
        } else {
            Ok(())
        }
    }

    /// Get the configured address size
    #[must_use]
    pub const fn address_size(&self) -> AddressSize {
        self.address_size
    }
}

impl Default for AddressRangeValidator {
    fn default() -> Self {
        Self::new(AddressSize::Bits48)
    }
}

/// Combined validation context
///
/// Captures all validation failures with comprehensive context.
#[derive(Debug, Clone)]
pub struct ValidationContext {
    /// Stream identifier for the translation request
    pub stream_id: StreamID,
    /// Process Address Space ID
    pub pasid: PASID,
    /// Virtual address being accessed
    pub address: IOVA,
    /// Type of memory access (Read/Write/Execute)
    pub access_type: AccessType,
    /// Security state of the access
    pub security_state: SecurityState,
}

impl ValidationContext {
    /// Create a new validation context
    #[must_use]
    pub const fn new(
        stream_id: StreamID,
        pasid: PASID,
        address: IOVA,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> Self {
        Self {
            stream_id,
            pasid,
            address,
            access_type,
            security_state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_stream_id(value: u32) -> StreamID {
        StreamID::new(value).unwrap()
    }

    fn test_pasid(value: u32) -> PASID {
        PASID::new(value).unwrap()
    }

    fn test_iova(value: u64) -> IOVA {
        IOVA::new(value).unwrap()
    }

    #[test]
    fn test_permission_validator_read() {
        let perms = PagePermissions::new(true, false, false);
        assert!(PermissionValidator::can_read(perms));
        assert!(!PermissionValidator::can_write(perms));
        assert!(!PermissionValidator::can_execute(perms));
    }

    #[test]
    fn test_permission_validator_write() {
        let perms = PagePermissions::new(false, true, false);
        assert!(!PermissionValidator::can_read(perms));
        assert!(PermissionValidator::can_write(perms));
        assert!(!PermissionValidator::can_execute(perms));
    }

    #[test]
    fn test_permission_validator_execute() {
        let perms = PagePermissions::new(false, false, true);
        assert!(!PermissionValidator::can_read(perms));
        assert!(!PermissionValidator::can_write(perms));
        assert!(PermissionValidator::can_execute(perms));
    }

    #[test]
    fn test_permission_allows_access() {
        let perms = PagePermissions::new(true, true, false);
        assert!(PermissionValidator::allows_access(perms, AccessType::Read));
        assert!(PermissionValidator::allows_access(perms, AccessType::Write));
        assert!(!PermissionValidator::allows_access(perms, AccessType::Execute));
    }

    #[test]
    fn test_permission_violation_description() {
        let perms = PagePermissions::new(true, false, false);
        let desc = PermissionValidator::violation_description(perms, AccessType::Write);
        assert!(desc.contains("write"));
        assert!(desc.contains("R-"));
    }

    #[test]
    fn test_address_range_validator_32bit() {
        let validator = AddressRangeValidator::new(AddressSize::Bits32);
        assert!(validator.is_valid_address(0xFFFF_FFFF));
        assert!(!validator.is_valid_address(0x1_0000_0000));
    }

    #[test]
    fn test_address_range_validator_48bit() {
        let validator = AddressRangeValidator::new(AddressSize::Bits48);
        assert!(validator.is_valid_address(0x0000_FFFF_FFFF_FFFF));
        assert!(!validator.is_valid_address(0x0001_0000_0000_0000));
    }

    #[test]
    fn test_address_range_validation() {
        let mut validator = AddressRangeValidator::new(AddressSize::Bits32);

        // Valid address
        assert!(validator
            .validate_range(
                test_stream_id(1),
                test_pasid(0),
                test_iova(0x1000),
                AccessType::Read,
                SecurityState::NonSecure,
            )
            .is_ok());

        // Invalid address
        let result = validator.validate_range(
            test_stream_id(1),
            test_pasid(0),
            test_iova(0x1_0000_0000),
            AccessType::Read,
            SecurityState::NonSecure,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().fault_type(), FaultType::AddressSizeFault);
    }

    #[test]
    fn test_page_alignment_check() {
        assert!(AddressRangeValidator::is_page_aligned(test_iova(0x1000)));
        assert!(AddressRangeValidator::is_page_aligned(test_iova(0x2000)));
        assert!(!AddressRangeValidator::is_page_aligned(test_iova(0x1001)));
        assert!(!AddressRangeValidator::is_page_aligned(test_iova(0x1FFF)));
    }

    #[test]
    fn test_page_alignment_validation() {
        let mut validator = AddressRangeValidator::default();

        // Aligned
        assert!(validator
            .validate_page_alignment(
                test_stream_id(1),
                test_pasid(0),
                test_iova(0x1000),
                AccessType::Read,
                SecurityState::NonSecure,
            )
            .is_ok());

        // Not aligned
        let result = validator.validate_page_alignment(
            test_stream_id(1),
            test_pasid(0),
            test_iova(0x1001),
            AccessType::Read,
            SecurityState::NonSecure,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().fault_type(), FaultType::AlignmentFault);
    }

    #[test]
    fn test_validation_context() {
        let ctx = ValidationContext::new(
            test_stream_id(42),
            test_pasid(7),
            test_iova(0x1000),
            AccessType::Write,
            SecurityState::Secure,
        );

        assert_eq!(ctx.stream_id, test_stream_id(42));
        assert_eq!(ctx.pasid, test_pasid(7));
        assert_eq!(ctx.address, test_iova(0x1000));
        assert_eq!(ctx.access_type, AccessType::Write);
        assert_eq!(ctx.security_state, SecurityState::Secure);
    }
}
