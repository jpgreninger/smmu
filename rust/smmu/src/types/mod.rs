//! Core types and protocol definitions for ARM SMMU v3
//!
//! This module provides the fundamental types used throughout the SMMU implementation,
//! including stream identifiers, address types, access permissions, and configuration structures.
//!
//! # Type Safety
//!
//! Strong typing is used extensively to prevent common errors:
//!
//! - [`StreamID`] - Type-safe stream identifier
//! - [`PASID`] - Type-safe Process Address Space ID
//! - [`AccessType`] - Memory access type (Read/Write/Execute)
//! - [`TranslationResult`] - Result of address translation operations
//!
//! # ARM SMMU v3 Compliance
//!
//! All types follow the ARM SMMU v3 specification definitions and constraints.

#![warn(missing_docs)]

mod validation_error;
mod stream_id;
mod pasid;
mod address;
mod access_type;
mod security_state;
mod fault_type;
pub mod translation_stage;
mod page_entry;
mod fault_record;
mod translation_result;
mod stream_context_error;
mod smmu_error;
pub mod config;

pub use validation_error::ValidationError;
pub use stream_id::StreamID;
pub use pasid::{PASID, PASID_MAX};
pub use address::{IOVA, IPA, PA, PAGE_SIZE};
pub use access_type::AccessType;
pub use security_state::SecurityState;
pub use fault_type::{FaultType, FaultSeverity, FaultContext, TranslationStep, AddressType};
pub use translation_stage::TranslationStage;
pub use page_entry::{PageEntry, PagePermissions, PageEntryBuilder};
pub use fault_record::{FaultRecord, FaultSyndrome, FaultRecordBuilder, FaultSyndromeBuilder};
pub use translation_result::{
    TranslationData, TranslationDataBuilder, TranslationError, TranslationResult,
};
pub use stream_context_error::StreamContextError;
pub use smmu_error::SMMUError;

// Export configuration types - note StreamConfig from config module
pub use config::{
    FaultMode, StreamConfig, StreamConfigBuilder,
    QueueConfig, QueueConfigBuilder,
    CacheConfig, CacheConfigBuilder,
    AddressConfig, AddressConfigBuilder,
    ResourceLimits, ResourceLimitsBuilder,
    ConfigurationError, ConfigurationErrorType,
    ValidationResult, ConfigConstants,
    SMMUConfig, SMMUConfigBuilder,
};
