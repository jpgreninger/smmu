//! Per-stream state and PASID management
//!
//! This module manages the state associated with each stream (device), including:
//!
//! - PASID (Process Address Space ID) management
//! - Per-PASID address space mappings
//! - Stream configuration and capabilities
//! - Translation context switching
//!
//! # Stream Context
//!
//! Each stream represents a device or logical channel that can access memory.
//! Streams may support multiple PASIDs for virtualization and process isolation.
//!
//! # PASID Support
//!
//! Full PASID support including PASID 0 (default/legacy mode) per ARM SMMU v3 specification.

// Placeholder for stream_context module - implementation will follow in subsequent tasks
// This file establishes the module structure only

#![warn(missing_docs)]
