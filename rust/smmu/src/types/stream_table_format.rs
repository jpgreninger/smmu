//! Stream Table Format enumeration (ARM SMMU v3 §3.3.1.2, §6.3.25)
//!
//! Defines the two supported stream table formats: linear and two-level.

/// Stream Table Format — controls whether the stream table is linear or two-level (§3.3.1.2)
///
/// The `SMMU_STRTAB_BASE_CFG.FMT` field selects the format:
/// - `Linear` (0): The stream table is a flat array of `2^LOG2SIZE` STEs indexed
///   directly by the StreamID.
/// - `TwoLevel` (1): The stream table is split into a first-level descriptor array
///   (L1STD) and second-level pages (L2STEs).  The StreamID is split at bit
///   `SPLIT` — the upper bits index L1 and the lower bits index within L2.
#[repr(u32)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum StreamTableFormat {
    /// Linear stream table — direct array, StreamID is the index (§3.3.1.1)
    #[default]
    Linear = 0,
    /// Two-level stream table — L1 descriptor + L2 pages (§3.3.1.2)
    TwoLevel = 1,
}
