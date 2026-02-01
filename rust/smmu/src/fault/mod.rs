//! Fault detection, classification, handling, and recovery
//!
//! This module implements comprehensive fault handling for the `SMMU` per ARM `SMMU` v3 Sections 6.1 and 6.2:
//!
//! - Translation faults (unmapped addresses, permission violations)
//! - Configuration faults (invalid stream or `PASID` configuration)
//! - Hardware faults (internal errors, parity errors)
//! - Event recording and reporting
//! - Fault processing in Terminate and Stall modes
//! - Recovery mechanisms for transient faults
//!
//! # Fault Types
//!
//! The `SMMU` specification defines 15 fault types that must be properly
//! detected, classified, and reported. This module implements all required
//! fault handling per ARM `SMMU` v3 specification.
//!
//! # Fault Detection (Section 6.1)
//!
//! The [`detection`] module provides comprehensive fault detection with:
//! - Translation fault detection with full context capture
//! - Permission fault checking with bitwise operations
//! - Address range validation (32/48/52-bit support)
//! - Fault syndrome generation per ARM `SMMU` v3 specification
//!
//! # Validation
//!
//! The [`validator`] module provides specialized validators for:
//! - Permission validation with detailed violation reporting
//! - Address range boundary checking
//! - Alignment validation
//!
//! # Fault Processing (Section 6.2)
//!
//! The [`processing`] module implements fault processing modes:
//! - Terminate mode with immediate reporting
//! - Stall mode with fault queuing
//! - Event generation and filtering
//! - Statistics tracking
//!
//! # Fault Queue
//!
//! The [`queue`] module provides thread-safe fault queuing:
//! - FIFO ordering with VecDeque
//! - Configurable capacity limits
//! - Concurrent access support
//!
//! # Fault Recovery
//!
//! The [`recovery`] module implements recovery strategies:
//! - Retry logic for transient faults
//! - Recovery strategy selection per fault type
//! - State save/restore for recovery attempts

#![warn(missing_docs)]

pub mod detection;
pub mod processing;
pub mod queue;
pub mod recovery;
pub mod validator;

// Re-export main types for convenience
pub use detection::{
    AddressSize, AddressValidator, FaultDetectionResult, FaultDetector, PermissionFaultDetector,
    TranslationFaultDetector,
};
pub use processing::{FaultMode, FaultProcessingError, FaultProcessor};
pub use queue::{FaultQueue, FaultQueueError};
pub use recovery::{FaultRecovery, RecoveryResult, RecoveryState, RecoveryStrategy};
pub use validator::{AddressRangeValidator, PermissionValidator, ValidationContext};
