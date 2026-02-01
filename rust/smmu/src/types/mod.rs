//! Core types and protocol definitions for ARM SMMU v3
//!
//! This module provides the fundamental types used throughout the SMMU implementation,
//! including stream identifiers, address types, access permissions, and configuration structures.
//!
//! # Type Safety
//!
//! Strong typing is used extensively to prevent common errors:
//!
//! - [StreamID] - Type-safe stream identifier
//! - [PASID] - Type-safe Process Address Space ID
//! - [AccessType] - Memory access type (Read/Write/Execute)
//! - [TranslationResult] - Result of address translation operations
//!
//! # ARM SMMU v3 Compliance
//!
//! All types follow the ARM SMMU v3 specification definitions and constraints.

#![warn(missing_docs)]

mod access_type;
mod address;
mod command_entry;
pub mod config;
mod event_entry;
mod fault_record;
mod fault_type;
mod page_entry;
mod pasid;
mod pri_entry;
mod queue_statistics;
mod security_state;
mod smmu_error;
mod stream_context_error;
mod stream_id;
mod translation_result;
pub mod translation_stage;
mod validation_error;

pub use access_type::AccessType;
pub use address::{IOVA, IPA, PA, PAGE_SIZE};
pub use command_entry::{CommandEntry, CommandType};
pub use event_entry::{EventEntry, EventType};
pub use fault_record::{FaultRecord, FaultRecordBuilder, FaultSyndrome, FaultSyndromeBuilder};
pub use fault_type::{AddressType, FaultContext, FaultSeverity, FaultType, TranslationStep};
pub use page_entry::{PageEntry, PageEntryBuilder, PagePermissions};
pub use pasid::{PASID, PASID_MAX};
pub use pri_entry::PRIEntry;
pub use queue_statistics::QueueStatistics;
pub use security_state::SecurityState;
pub use smmu_error::SMMUError;
pub use stream_context_error::StreamContextError;
pub use stream_id::StreamID;
pub use translation_result::{TranslationData, TranslationDataBuilder, TranslationError, TranslationResult};
pub use translation_stage::TranslationStage;
pub use validation_error::ValidationError;

// Export configuration types - note StreamConfig from config module
pub use config::{
    AddressConfig, AddressConfigBuilder, CacheConfig, CacheConfigBuilder, ConfigConstants, ConfigurationError,
    ConfigurationErrorType, FaultMode, QueueConfig, QueueConfigBuilder, ResourceLimits, ResourceLimitsBuilder,
    SMMUConfig, SMMUConfigBuilder, StreamConfig, StreamConfigBuilder, ValidationResult,
};
