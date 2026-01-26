//! Common test utilities and helpers
//!
//! This module provides shared test infrastructure for both unit and integration tests,
//! including test data builders, assertion helpers, and common test scenarios.

#![allow(dead_code)] // Test utilities may not all be used immediately

/// Test data builders for creating consistent test scenarios
pub mod builders;

/// Common assertion helpers for SMMU-specific validations
pub mod assertions;

/// Test fixtures and sample data
pub mod fixtures;

/// Performance test utilities
pub mod perf;

/// Mock objects and test doubles
pub mod mocks;

// Re-export commonly used utilities
pub use builders::*;
pub use assertions::*;
pub use fixtures::*;
