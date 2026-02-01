//! Prelude for convenient imports
//!
//! This module re-exports the most commonly used types and traits for ergonomic usage.
//! Import everything from this module to get started quickly:
//!
//! ```rust
//! use smmu::prelude::*;
//!
//! // Now you have access to all common types:
//! let smmu = SMMU::new();
//! let stream_id = StreamID::new(1)?;
//! let pasid = PASID::new(0)?;
//! let iova = IOVA::new(0x1000)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # What's included?
//!
//! - **Main SMMU controller**: [`SMMU`]
//! - **Identifiers**: [`StreamID`], [`PASID`]
//! - **Addresses**: [`IOVA`], [`IPA`], [`PA`], [`PAGE_SIZE`]
//! - **Access types**: [`AccessType`], [`SecurityState`]
//! - **Configuration**: [`SMMUConfig`], [`StreamConfig`], [`PagePermissions`]
//! - **Builders**: [`SMMUConfigBuilder`], [`StreamConfigBuilder`], etc.
//! - **Translation types**: [`TranslationResult`], [`TranslationData`], [`TranslationError`]
//! - **Fault types**: [`FaultRecord`], [`FaultType`]
//! - **Error types**: [`ValidationError`], [`StreamContextError`]

// Re-export main SMMU controller
pub use crate::SMMU;

// Re-export core identifiers
pub use crate::types::{StreamID, PASID, PASID_MAX};

// Re-export address types
pub use crate::types::{IOVA, IPA, PA, PAGE_SIZE};

// Re-export access and security types
pub use crate::types::{AccessType, SecurityState};

// Re-export page management types
pub use crate::types::{PageEntry, PagePermissions};

// Re-export configuration types
pub use crate::types::{AddressConfig, CacheConfig, FaultMode, QueueConfig, ResourceLimits, SMMUConfig, StreamConfig};

// Re-export builder types
pub use crate::types::{
    AddressConfigBuilder, CacheConfigBuilder, FaultRecordBuilder, PageEntryBuilder, QueueConfigBuilder,
    ResourceLimitsBuilder, SMMUConfigBuilder, StreamConfigBuilder, TranslationDataBuilder,
};

// Re-export translation types
pub use crate::types::{TranslationData, TranslationError, TranslationResult, TranslationStage};

// Re-export fault types
pub use crate::types::{
    AddressType, FaultContext, FaultRecord, FaultSeverity, FaultSyndrome, FaultType, TranslationStep,
};

// Re-export queue types
pub use crate::types::{CommandEntry, CommandType, EventEntry, EventType, PRIEntry, QueueStatistics};

// Re-export error types
pub use crate::types::{StreamContextError, ValidationError};

// Re-export constants
pub use crate::{SMMU_IMPL_VERSION, SMMU_SPEC_DOCUMENT, SMMU_SPEC_VERSION};
