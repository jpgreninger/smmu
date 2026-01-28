//! Fault recovery mechanisms for ARM SMMU v3
//!
//! This module implements recovery strategies for different fault types, including
//! retry logic for transient faults, remapping strategies, and state restoration.
//!
//! # Recovery Strategies
//!
//! - **Retry**: Attempt translation again (for transient faults)
//! - **Remap**: Request page mapping update
//! - **Terminate**: Abort transaction (for permanent faults)
//!
//! # Examples
//!
//! ```
//! use smmu::fault::recovery::{FaultRecovery, RecoveryStrategy, RecoveryResult};
//! use smmu::types::{FaultRecordBuilder, FaultType, AccessType, StreamID, PASID, IOVA};
//!
//! let recovery = FaultRecovery::new();
//!
//! let fault = FaultRecordBuilder::new()
//!     .stream_id(StreamID::new(0x100).unwrap())
//!     .pasid(PASID::new(1).unwrap())
//!     .address(IOVA::new(0x1000_0000).unwrap())
//!     .fault_type(FaultType::TranslationFault)
//!     .access_type(AccessType::Read)
//!     .build();
//!
//! let strategy = recovery.get_recommended_strategy(&fault);
//! let result = recovery.attempt_recovery(&fault, strategy);
//! ```

use crate::types::{FaultRecord, FaultType};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Recovery strategy for fault handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Retry the translation (for transient faults)
    Retry {
        /// Maximum number of retry attempts
        max_attempts: u32,
    },
    /// Request page remapping
    Remap,
    /// Terminate the transaction
    Terminate,
}

/// Result of a recovery attempt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryResult {
    /// Fault was successfully recovered
    Recovered,
    /// Recovery requires retry
    Retry,
    /// Fault is unrecoverable
    Unrecoverable,
}

/// Saved state for fault recovery
#[derive(Debug, Clone)]
pub struct RecoveryState {
    /// Number of retry attempts made
    retry_count: u32,
    /// Timestamp of first fault
    first_fault_time: u64,
    /// Last recovery strategy attempted
    last_strategy: Option<RecoveryStrategy>,
}

impl RecoveryState {
    /// Creates a new recovery state
    #[must_use]
    pub const fn new() -> Self {
        Self {
            retry_count: 0,
            first_fault_time: 0,
            last_strategy: None,
        }
    }

    /// Returns the retry count
    #[must_use]
    pub const fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// Returns the first fault timestamp
    #[must_use]
    pub const fn first_fault_time(&self) -> u64 {
        self.first_fault_time
    }

    /// Returns the last strategy attempted
    #[must_use]
    pub const fn last_strategy(&self) -> Option<RecoveryStrategy> {
        self.last_strategy
    }
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self::new()
    }
}

/// Fault recovery manager
///
/// Manages recovery attempts for faults, tracking retry counts and
/// recommending recovery strategies based on fault type.
#[derive(Debug)]
pub struct FaultRecovery {
    /// Recovery state per fault (keyed by stream_id + pasid + address)
    state_map: Arc<Mutex<HashMap<String, RecoveryState>>>,
}

impl FaultRecovery {
    /// Creates a new fault recovery manager
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::FaultRecovery;
    ///
    /// let recovery = FaultRecovery::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            state_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Gets recommended recovery strategy for a fault
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to analyze
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::{FaultRecovery, RecoveryStrategy};
    /// use smmu::types::{FaultRecord, FaultType, AccessType, StreamID, PASID, IOVA};
    ///
    /// let recovery = FaultRecovery::new();
    /// let fault = FaultRecord::builder()
    ///     .stream_id(StreamID::new(0x100).unwrap())
    ///     .pasid(PASID::new(1).unwrap())
    ///     .address(IOVA::new(0x1000_0000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .build();
    ///
    /// let strategy = recovery.get_recommended_strategy(&fault);
    /// ```
    #[must_use]
    pub fn get_recommended_strategy(&self, fault: &FaultRecord) -> RecoveryStrategy {
        match fault.fault_type() {
            // Translation faults may be recoverable via retry or remap
            FaultType::TranslationFault => RecoveryStrategy::Retry { max_attempts: 3 },

            // Access flag faults may require remapping
            FaultType::AccessFlagFault => RecoveryStrategy::Remap,

            // Permission faults are typically permanent
            FaultType::PermissionFault => RecoveryStrategy::Terminate,

            // Address size faults are configuration errors
            FaultType::AddressSizeFault => RecoveryStrategy::Terminate,

            // Most other faults are unrecoverable
            _ => RecoveryStrategy::Terminate,
        }
    }

    /// Attempts recovery for a fault using specified strategy
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to recover
    /// * `strategy` - Recovery strategy to use
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::{FaultRecovery, RecoveryStrategy};
    /// use smmu::types::{FaultRecord, FaultType, AccessType, StreamID, PASID, IOVA};
    ///
    /// let recovery = FaultRecovery::new();
    /// let fault = FaultRecord::builder()
    ///     .stream_id(StreamID::new(0x100).unwrap())
    ///     .pasid(PASID::new(1).unwrap())
    ///     .address(IOVA::new(0x1000_0000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .build();
    ///
    /// let result = recovery.attempt_recovery(&fault, RecoveryStrategy::Retry { max_attempts: 3 });
    /// ```
    pub fn attempt_recovery(
        &self,
        fault: &FaultRecord,
        strategy: RecoveryStrategy,
    ) -> RecoveryResult {
        let key = Self::fault_key(fault);
        let mut state_map = self.state_map.lock().unwrap();

        let state = state_map.entry(key).or_insert_with(|| {
            let mut s = RecoveryState::new();
            s.first_fault_time = Self::current_timestamp();
            s
        });

        state.last_strategy = Some(strategy);

        match strategy {
            RecoveryStrategy::Retry { max_attempts } => {
                state.retry_count += 1;
                if state.retry_count >= max_attempts {
                    RecoveryResult::Unrecoverable
                } else {
                    RecoveryResult::Retry
                }
            }
            RecoveryStrategy::Remap => {
                // Remapping would be handled by external software
                RecoveryResult::Retry
            }
            RecoveryStrategy::Terminate => RecoveryResult::Unrecoverable,
        }
    }

    /// Saves current recovery state for a fault
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to save state for
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::FaultRecovery;
    /// use smmu::types::{FaultRecordBuilder, FaultType, AccessType, StreamID, PASID, IOVA};
    ///
    /// let recovery = FaultRecovery::new();
    /// let fault = FaultRecordBuilder::new()
    ///     .stream_id(StreamID::new(0x100).unwrap())
    ///     .pasid(PASID::new(1).unwrap())
    ///     .address(IOVA::new(0x1000_0000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .build();
    ///
    /// let state = recovery.save_state(&fault);
    /// ```
    #[must_use]
    pub fn save_state(&self, fault: &FaultRecord) -> RecoveryState {
        let key = Self::fault_key(fault);
        let state_map = self.state_map.lock().unwrap();
        state_map
            .get(&key)
            .cloned()
            .unwrap_or_else(RecoveryState::new)
    }

    /// Restores recovery state for a fault
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to restore state for
    /// * `state` - State to restore
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::FaultRecovery;
    /// use smmu::types::{FaultRecordBuilder, FaultType, AccessType, StreamID, PASID, IOVA};
    ///
    /// let recovery = FaultRecovery::new();
    /// let fault = FaultRecordBuilder::new()
    ///     .stream_id(StreamID::new(0x100).unwrap())
    ///     .pasid(PASID::new(1).unwrap())
    ///     .address(IOVA::new(0x1000_0000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .build();
    ///
    /// let state = recovery.save_state(&fault);
    /// let result = recovery.restore_state(&fault, state);
    /// assert!(result.is_ok());
    /// ```
    pub fn restore_state(&self, fault: &FaultRecord, state: RecoveryState) -> Result<(), String> {
        let key = Self::fault_key(fault);
        let mut state_map = self.state_map.lock().unwrap();
        state_map.insert(key, state);
        Ok(())
    }

    /// Clears recovery state for a fault
    ///
    /// # Arguments
    ///
    /// * `fault` - Fault record to clear state for
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::FaultRecovery;
    /// use smmu::types::{FaultRecordBuilder, FaultType, AccessType, StreamID, PASID, IOVA};
    ///
    /// let recovery = FaultRecovery::new();
    /// let fault = FaultRecordBuilder::new()
    ///     .stream_id(StreamID::new(0x100).unwrap())
    ///     .pasid(PASID::new(1).unwrap())
    ///     .address(IOVA::new(0x1000_0000).unwrap())
    ///     .fault_type(FaultType::TranslationFault)
    ///     .access_type(AccessType::Read)
    ///     .build();
    ///
    /// recovery.clear_state(&fault);
    /// ```
    pub fn clear_state(&self, fault: &FaultRecord) {
        let key = Self::fault_key(fault);
        let mut state_map = self.state_map.lock().unwrap();
        state_map.remove(&key);
    }

    /// Clears all recovery state
    ///
    /// # Examples
    ///
    /// ```
    /// use smmu::fault::recovery::FaultRecovery;
    ///
    /// let recovery = FaultRecovery::new();
    /// recovery.clear_all();
    /// ```
    pub fn clear_all(&self) {
        let mut state_map = self.state_map.lock().unwrap();
        state_map.clear();
    }

    /// Generates a unique key for a fault
    fn fault_key(fault: &FaultRecord) -> String {
        format!(
            "{:x}:{:x}:{:x}",
            fault.stream_id().as_u32(),
            fault.pasid().as_u32(),
            fault.address().as_u64()
        )
    }

    /// Gets current timestamp in microseconds
    fn current_timestamp() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }
}

impl Default for FaultRecovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccessType, FaultType, StreamID, PASID, IOVA};

    fn create_test_fault(fault_type: FaultType) -> FaultRecord {
        FaultRecord::builder()
            .stream_id(StreamID::new(0x100).unwrap())
            .pasid(PASID::new(1).unwrap())
            .address(IOVA::new(0x1000_0000).unwrap())
            .fault_type(fault_type)
            .access_type(AccessType::Read)
            .build()
    }

    #[test]
    fn test_recommended_strategy_translation_fault() {
        let recovery = FaultRecovery::new();
        let fault = create_test_fault(FaultType::TranslationFault);
        let strategy = recovery.get_recommended_strategy(&fault);
        assert!(matches!(strategy, RecoveryStrategy::Retry { .. }));
    }

    #[test]
    fn test_recommended_strategy_permission_fault() {
        let recovery = FaultRecovery::new();
        let fault = create_test_fault(FaultType::PermissionFault);
        let strategy = recovery.get_recommended_strategy(&fault);
        assert_eq!(strategy, RecoveryStrategy::Terminate);
    }

    #[test]
    fn test_retry_limit() {
        let recovery = FaultRecovery::new();
        let fault = create_test_fault(FaultType::TranslationFault);

        let strategy = RecoveryStrategy::Retry { max_attempts: 3 };

        // First two attempts should retry
        assert_eq!(
            recovery.attempt_recovery(&fault, strategy),
            RecoveryResult::Retry
        );
        assert_eq!(
            recovery.attempt_recovery(&fault, strategy),
            RecoveryResult::Retry
        );

        // Third attempt should be unrecoverable
        assert_eq!(
            recovery.attempt_recovery(&fault, strategy),
            RecoveryResult::Unrecoverable
        );
    }

    #[test]
    fn test_state_save_restore() {
        let recovery = FaultRecovery::new();
        let fault = create_test_fault(FaultType::TranslationFault);

        // Perform some recovery attempts
        let strategy = RecoveryStrategy::Retry { max_attempts: 3 };
        recovery.attempt_recovery(&fault, strategy);

        // Save state
        let state = recovery.save_state(&fault);
        assert_eq!(state.retry_count(), 1);

        // Clear and restore
        recovery.clear_state(&fault);
        assert!(recovery.restore_state(&fault, state).is_ok());

        // State should be restored
        let restored = recovery.save_state(&fault);
        assert_eq!(restored.retry_count(), 1);
    }
}
