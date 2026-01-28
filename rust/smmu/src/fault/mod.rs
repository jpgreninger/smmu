//! Fault detection, classification, and handling
//!
//! This module implements comprehensive fault handling for the SMMU per ARM SMMU v3 Section 6.1:
//!
//! - Translation faults (unmapped addresses, permission violations)
//! - Configuration faults (invalid stream or PASID configuration)
//! - Hardware faults (internal errors, parity errors)
//! - Event recording and reporting
//!
//! # Fault Types
//!
//! The SMMU specification defines 15 fault types that must be properly
//! detected, classified, and reported. This module implements all required
//! fault handling per ARM SMMU v3 specification.
//!
//! # Fault Detection
//!
//! The [`detection`] module provides comprehensive fault detection with:
//! - Translation fault detection with full context capture
//! - Permission fault checking with bitwise operations
//! - Address range validation (32/48/52-bit support)
//! - Fault syndrome generation per ARM SMMU v3 specification
//!
//! # Validation
//!
//! The [`validator`] module provides specialized validators for:
//! - Permission validation with detailed violation reporting
//! - Address range boundary checking
//! - Alignment validation
//!
//! # Event Queue
//!
//! Faults are recorded in an event queue that can be polled by software
//! to handle errors and implement recovery strategies.

#![warn(missing_docs)]

pub mod detection;
pub mod validator;

// Re-export main types for convenience
pub use detection::{
    AddressSize, AddressValidator, FaultDetectionResult, FaultDetector,
    PermissionFaultDetector, TranslationFaultDetector,
};
pub use validator::{AddressRangeValidator, PermissionValidator, ValidationContext};
