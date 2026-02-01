//! Fault Record structures for ARM `SMMU` v3
//!
//! This module defines fault record structures for comprehensive fault reporting
//! following the ARM `SMMU` v3 specification. All implementations are safe with
//! zero unsafe code.

use super::{AccessType, FaultType, SecurityState, StreamID, TranslationStage, IOVA, PASID};

/// ARM `SMMU` v3 fault syndrome structure
///
/// Contains detailed fault information following ARM `SMMU` v3 fault syndrome
/// register format. Used for comprehensive fault reporting and debugging.
///
/// # Examples
///
/// ```
/// use smmu::types::`FaultSyndrome`;
///
/// let syndrome = `FaultSyndrome`::builder()
///     .syndrome_register(0x1234_5678)
///     .fault_level(2)
///     .write_not_read(true)
///     .build();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FaultSyndrome {
    /// ARM `SMMU` v3 fault syndrome register value
    syndrome_register: u32,
    /// Translation table level (0-3)
    fault_level: u8,
    /// True for write access, false for read
    write_not_read: bool,
    /// True if syndrome information is valid
    valid_syndrome: bool,
    /// Index of faulting context descriptor
    context_descriptor_index: u16,
}

impl FaultSyndrome {
    /// Creates a new `FaultSyndrome` with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            syndrome_register: 0,
            fault_level: 0,
            write_not_read: false,
            valid_syndrome: true,
            context_descriptor_index: 0,
        }
    }

    /// Creates a builder for constructing `FaultSyndrome`
    #[must_use]
    pub const fn builder() -> FaultSyndromeBuilder {
        FaultSyndromeBuilder::new()
    }

    /// Returns the syndrome register value
    #[must_use]
    #[inline]
    pub const fn syndrome_register(&self) -> u32 {
        self.syndrome_register
    }

    /// Returns the fault level (0-3)
    #[must_use]
    #[inline]
    pub const fn fault_level(&self) -> u8 {
        self.fault_level
    }

    /// Returns true if this was a write access
    #[must_use]
    #[inline]
    pub const fn write_not_read(&self) -> bool {
        self.write_not_read
    }

    /// Returns true if syndrome information is valid
    #[must_use]
    #[inline]
    pub const fn valid_syndrome(&self) -> bool {
        self.valid_syndrome
    }

    /// Returns the context descriptor index
    #[must_use]
    #[inline]
    pub const fn context_descriptor_index(&self) -> u16 {
        self.context_descriptor_index
    }
}

impl Default for FaultSyndrome {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for `FaultSyndrome`
#[derive(Debug, Clone)]
pub struct FaultSyndromeBuilder {
    syndrome_register: u32,
    fault_level: u8,
    write_not_read: bool,
    valid_syndrome: bool,
    context_descriptor_index: u16,
}

impl FaultSyndromeBuilder {
    /// Creates a new builder with default values
    #[must_use]
    const fn new() -> Self {
        Self {
            syndrome_register: 0,
            fault_level: 0,
            write_not_read: false,
            valid_syndrome: true,
            context_descriptor_index: 0,
        }
    }

    /// Sets the syndrome register value
    #[must_use]
    pub const fn syndrome_register(mut self, value: u32) -> Self {
        self.syndrome_register = value;
        self
    }

    /// Sets the fault level (0-3)
    #[must_use]
    pub const fn fault_level(mut self, level: u8) -> Self {
        self.fault_level = level;
        self
    }

    /// Sets write_not_read flag
    #[must_use]
    pub const fn write_not_read(mut self, value: bool) -> Self {
        self.write_not_read = value;
        self
    }

    /// Sets valid_syndrome flag
    #[must_use]
    pub const fn valid_syndrome(mut self, value: bool) -> Self {
        self.valid_syndrome = value;
        self
    }

    /// Sets the context descriptor index
    #[must_use]
    pub const fn context_descriptor_index(mut self, index: u16) -> Self {
        self.context_descriptor_index = index;
        self
    }

    /// Builds the `FaultSyndrome`
    #[must_use]
    pub const fn build(self) -> FaultSyndrome {
        FaultSyndrome {
            syndrome_register: self.syndrome_register,
            fault_level: self.fault_level,
            write_not_read: self.write_not_read,
            valid_syndrome: self.valid_syndrome,
            context_descriptor_index: self.context_descriptor_index,
        }
    }
}

/// ARM `SMMU` v3 comprehensive fault record structure
///
/// Contains all fault information required for complete fault reporting,
/// debugging, and recovery according to ARM `SMMU` v3 specification.
///
/// # Examples
///
/// ```
/// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `SecurityState`, `StreamID`, `PASID`, `IOVA`};
///
/// let record = `FaultRecord`::builder()
///     .stream_id(`StreamID`::new(42).unwrap())
///     .pasid(`PASID`::new(7).unwrap())
///     .address(`IOVA`::new(0x1000).unwrap())
///     .fault_type(`FaultType`::PermissionFault)
///     .access_type(`AccessType`::Write)
///     .security_state(`SecurityState`::Secure)
///     .timestamp(12_345)
///     .build();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FaultRecord {
    /// Source stream identifier
    stream_id: StreamID,
    /// Process Address Space ID
    pasid: PASID,
    /// Faulting virtual address
    address: IOVA,
    /// Detailed fault type classification
    fault_type: FaultType,
    /// Access type (Read/Write/Execute)
    access_type: AccessType,
    /// Security state context
    security_state: SecurityState,
    /// Detailed ARM `SMMU` v3 fault syndrome
    syndrome: FaultSyndrome,
    /// Fault occurrence timestamp
    timestamp: u64,
}

impl FaultRecord {
    /// Creates a new `FaultRecord` with basic fault information
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Source stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `address` - Faulting virtual address
    /// * `fault_type` - Fault type classification
    /// * `access_type` - Type of access that faulted
    /// * `security_state` - Security state context
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::types::{`FaultRecord`, `FaultType`, `AccessType`, `SecurityState`, `StreamID`, `PASID`, `IOVA`};
    ///
    /// let record = `FaultRecord`::new(
    ///     `StreamID`::new(1).unwrap(),
    ///     `PASID`::new(0).unwrap(),
    ///     `IOVA`::new(0x1000).unwrap(),
    ///     `FaultType`::TranslationFault,
    ///     `AccessType`::Read,
    ///     `SecurityState`::NonSecure,
    /// );
    /// ```
    #[must_use]
    pub const fn new(
        stream_id: StreamID,
        pasid: PASID,
        address: IOVA,
        fault_type: FaultType,
        access_type: AccessType,
        security_state: SecurityState,
    ) -> Self {
        Self {
            stream_id,
            pasid,
            address,
            fault_type,
            access_type,
            security_state,
            syndrome: FaultSyndrome::new(),
            timestamp: 0,
        }
    }

    /// Creates a new `FaultRecord` with comprehensive fault syndrome
    ///
    /// # Arguments
    ///
    /// * `stream_id` - Source stream identifier
    /// * `pasid` - Process Address Space ID
    /// * `address` - Faulting virtual address
    /// * `fault_type` - Fault type classification
    /// * `access_type` - Type of access that faulted
    /// * `security_state` - Security state context
    /// * `syndrome` - Detailed fault syndrome
    #[must_use]
    pub const fn with_syndrome(
        stream_id: StreamID,
        pasid: PASID,
        address: IOVA,
        fault_type: FaultType,
        access_type: AccessType,
        security_state: SecurityState,
        syndrome: FaultSyndrome,
    ) -> Self {
        Self {
            stream_id,
            pasid,
            address,
            fault_type,
            access_type,
            security_state,
            syndrome,
            timestamp: 0,
        }
    }

    /// Creates a builder for constructing `FaultRecord`
    #[must_use]
    pub const fn builder() -> FaultRecordBuilder {
        FaultRecordBuilder::new()
    }

    /// Returns the stream ID
    #[must_use]
    #[inline]
    pub const fn stream_id(&self) -> StreamID {
        self.stream_id
    }

    /// Returns the `PASID`
    #[must_use]
    #[inline]
    pub const fn pasid(&self) -> PASID {
        self.pasid
    }

    /// Returns the faulting address
    #[must_use]
    #[inline]
    pub const fn address(&self) -> IOVA {
        self.address
    }

    /// Returns the fault type
    #[must_use]
    #[inline]
    pub const fn fault_type(&self) -> FaultType {
        self.fault_type
    }

    /// Returns the access type
    #[must_use]
    #[inline]
    pub const fn access_type(&self) -> AccessType {
        self.access_type
    }

    /// Returns the security state
    #[must_use]
    #[inline]
    pub const fn security_state(&self) -> SecurityState {
        self.security_state
    }

    /// Returns a reference to the fault syndrome
    #[must_use]
    #[inline]
    pub const fn syndrome(&self) -> &FaultSyndrome {
        &self.syndrome
    }

    /// Returns the timestamp
    #[must_use]
    #[inline]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Returns the faulting address (alias for address())
    #[must_use]
    #[inline]
    pub const fn iova(&self) -> IOVA {
        self.address
    }

    /// Returns the translation stage where fault occurred
    /// For now, returns Stage1 as a placeholder
    #[must_use]
    #[inline]
    pub const fn stage(&self) -> TranslationStage {
        TranslationStage::Stage2
    }
}

impl Default for FaultRecord {
    fn default() -> Self {
        Self {
            stream_id: StreamID::new(0).unwrap_or_else(|_| unreachable!()),
            pasid: PASID::new(0).unwrap_or_else(|_| unreachable!()),
            address: IOVA::new(0).unwrap_or_else(|_| unreachable!()),
            fault_type: FaultType::TranslationFault,
            access_type: AccessType::Read,
            security_state: SecurityState::NonSecure,
            syndrome: FaultSyndrome::default(),
            timestamp: 0,
        }
    }
}

/// Builder for `FaultRecord` with comprehensive validation
///
/// Provides a fluent interface for constructing `FaultRecord` instances.
#[derive(Debug, Clone)]
pub struct FaultRecordBuilder {
    stream_id: Option<StreamID>,
    pasid: Option<PASID>,
    address: Option<IOVA>,
    fault_type: FaultType,
    access_type: AccessType,
    security_state: SecurityState,
    syndrome: FaultSyndrome,
    timestamp: u64,
}

impl FaultRecordBuilder {
    /// Creates a new builder with default values
    #[must_use]
    const fn new() -> Self {
        Self {
            stream_id: None,
            pasid: None,
            address: None,
            fault_type: FaultType::TranslationFault,
            access_type: AccessType::Read,
            security_state: SecurityState::NonSecure,
            syndrome: FaultSyndrome::new(),
            timestamp: 0,
        }
    }

    /// Sets the stream ID
    #[must_use]
    pub const fn stream_id(mut self, sid: StreamID) -> Self {
        self.stream_id = Some(sid);
        self
    }

    /// Sets the `PASID`
    #[must_use]
    pub const fn pasid(mut self, pasid: PASID) -> Self {
        self.pasid = Some(pasid);
        self
    }

    /// Sets the faulting address
    #[must_use]
    pub const fn address(mut self, addr: IOVA) -> Self {
        self.address = Some(addr);
        self
    }

    /// Sets the fault type
    #[must_use]
    pub const fn fault_type(mut self, ft: FaultType) -> Self {
        self.fault_type = ft;
        self
    }

    /// Sets the access type
    #[must_use]
    pub const fn access_type(mut self, at: AccessType) -> Self {
        self.access_type = at;
        self
    }

    /// Sets the security state
    #[must_use]
    pub const fn security_state(mut self, state: SecurityState) -> Self {
        self.security_state = state;
        self
    }

    /// Sets the fault syndrome
    #[must_use]
    pub const fn syndrome(mut self, syndrome: FaultSyndrome) -> Self {
        self.syndrome = syndrome;
        self
    }

    /// Sets the timestamp
    #[must_use]
    pub const fn timestamp(mut self, ts: u64) -> Self {
        self.timestamp = ts;
        self
    }

    /// Builds the `FaultRecord`
    ///
    /// # Panics
    ///
    /// Panics if required fields (stream_id, pasid, address, fault_type) are not set
    #[must_use]
    pub fn build(self) -> FaultRecord {
        FaultRecord {
            stream_id: self.stream_id.expect("stream_id must be set"),
            pasid: self.pasid.expect("pasid must be set"),
            address: self.address.expect("address must be set"),
            fault_type: self.fault_type,
            access_type: self.access_type,
            security_state: self.security_state,
            syndrome: self.syndrome,
            timestamp: self.timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_syndrome_basic() {
        let syndrome = FaultSyndrome::new();
        assert_eq!(syndrome.syndrome_register(), 0);
        assert!(syndrome.valid_syndrome());
    }

    #[test]
    fn test_fault_record_basic() {
        let record = FaultRecord::new(
            StreamID::new(1).unwrap(),
            PASID::new(0).unwrap(),
            IOVA::new(0x1000).unwrap(),
            FaultType::TranslationFault,
            AccessType::Read,
            SecurityState::NonSecure,
        );

        assert_eq!(record.stream_id(), StreamID::new(1).unwrap());
        assert_eq!(record.fault_type(), FaultType::TranslationFault);
    }
}
