//! Test data builders for creating SMMU test scenarios
//!
//! Provides fluent builder APIs for constructing complex test cases with
//! sensible defaults and easy customization.

/// Builder for creating test SMMU configurations
///
/// # Examples
///
/// ```rust,ignore
/// let config = SMMUConfigBuilder::new()
///     .with_stream_count(16)
///     .with_pasid_bits(16)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct SMMUConfigBuilder {
    stream_count: u32,
    pasid_bits: u8,
    enable_caching: bool,
    enable_two_stage: bool,
}

impl SMMUConfigBuilder {
    /// Create a new builder with default values
    #[must_use]
    pub fn new() -> Self {
        Self {
            stream_count: 256,
            pasid_bits: 20,
            enable_caching: true,
            enable_two_stage: false,
        }
    }

    /// Set the number of streams
    #[must_use]
    pub fn with_stream_count(mut self, count: u32) -> Self {
        self.stream_count = count;
        self
    }

    /// Set the number of PASID bits
    #[must_use]
    pub fn with_pasid_bits(mut self, bits: u8) -> Self {
        self.pasid_bits = bits;
        self
    }

    /// Enable or disable caching
    #[must_use]
    pub fn with_caching(mut self, enable: bool) -> Self {
        self.enable_caching = enable;
        self
    }

    /// Enable or disable two-stage translation
    #[must_use]
    pub fn with_two_stage(mut self, enable: bool) -> Self {
        self.enable_two_stage = enable;
        self
    }

    /// Build the configuration
    ///
    /// # Panics
    ///
    /// Panics if configuration values are invalid
    #[must_use]
    pub fn build(self) -> SMMUTestConfig {
        assert!(self.stream_count > 0, "Stream count must be > 0");
        assert!(self.pasid_bits <= 20, "PASID bits must be <= 20");

        SMMUTestConfig {
            stream_count: self.stream_count,
            pasid_bits: self.pasid_bits,
            enable_caching: self.enable_caching,
            enable_two_stage: self.enable_two_stage,
        }
    }
}

impl Default for SMMUConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Test configuration for SMMU
#[derive(Debug, Clone)]
pub struct SMMUTestConfig {
    pub stream_count: u32,
    pub pasid_bits: u8,
    pub enable_caching: bool,
    pub enable_two_stage: bool,
}

/// Builder for creating address space test scenarios
#[derive(Debug, Clone)]
pub struct AddressSpaceBuilder {
    page_size: u64,
    total_pages: usize,
    identity_mapped: bool,
}

impl AddressSpaceBuilder {
    /// Create a new address space builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            page_size: 4096,
            total_pages: 256,
            identity_mapped: false,
        }
    }

    /// Set page size (must be power of 2)
    #[must_use]
    pub fn with_page_size(mut self, size: u64) -> Self {
        self.page_size = size;
        self
    }

    /// Set total number of pages to map
    #[must_use]
    pub fn with_page_count(mut self, count: usize) -> Self {
        self.total_pages = count;
        self
    }

    /// Enable identity mapping (VA == PA)
    #[must_use]
    pub fn identity_mapped(mut self) -> Self {
        self.identity_mapped = true;
        self
    }

    /// Build the address space configuration
    #[must_use]
    pub fn build(self) -> AddressSpaceTestConfig {
        assert!(
            self.page_size.is_power_of_two(),
            "Page size must be power of 2"
        );
        assert!(self.total_pages > 0, "Must have at least one page");

        AddressSpaceTestConfig {
            page_size: self.page_size,
            total_pages: self.total_pages,
            identity_mapped: self.identity_mapped,
        }
    }
}

impl Default for AddressSpaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Address space test configuration
#[derive(Debug, Clone)]
pub struct AddressSpaceTestConfig {
    pub page_size: u64,
    pub total_pages: usize,
    pub identity_mapped: bool,
}

/// Builder for creating translation test scenarios
#[derive(Debug, Clone)]
pub struct TranslationTestBuilder {
    stream_id: u32,
    pasid: u32,
    iova: u64,
    expected_pa: Option<u64>,
    should_fault: bool,
}

impl TranslationTestBuilder {
    /// Create a new translation test builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            stream_id: 0,
            pasid: 0,
            iova: 0x1000,
            expected_pa: None,
            should_fault: false,
        }
    }

    /// Set stream ID
    #[must_use]
    pub fn with_stream_id(mut self, id: u32) -> Self {
        self.stream_id = id;
        self
    }

    /// Set PASID
    #[must_use]
    pub fn with_pasid(mut self, pasid: u32) -> Self {
        self.pasid = pasid;
        self
    }

    /// Set input virtual address
    #[must_use]
    pub fn with_iova(mut self, iova: u64) -> Self {
        self.iova = iova;
        self
    }

    /// Set expected physical address
    #[must_use]
    pub fn expect_pa(mut self, pa: u64) -> Self {
        self.expected_pa = Some(pa);
        self.should_fault = false;
        self
    }

    /// Expect translation to fault
    #[must_use]
    pub fn expect_fault(mut self) -> Self {
        self.should_fault = true;
        self.expected_pa = None;
        self
    }

    /// Build the translation test case
    #[must_use]
    pub fn build(self) -> TranslationTestCase {
        TranslationTestCase {
            stream_id: self.stream_id,
            pasid: self.pasid,
            iova: self.iova,
            expected_pa: self.expected_pa,
            should_fault: self.should_fault,
        }
    }
}

impl Default for TranslationTestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Translation test case
#[derive(Debug, Clone)]
pub struct TranslationTestCase {
    pub stream_id: u32,
    pub pasid: u32,
    pub iova: u64,
    pub expected_pa: Option<u64>,
    pub should_fault: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smmu_config_builder_defaults() {
        let config = SMMUConfigBuilder::new().build();
        assert_eq!(config.stream_count, 256);
        assert_eq!(config.pasid_bits, 20);
        assert!(config.enable_caching);
        assert!(!config.enable_two_stage);
    }

    #[test]
    fn test_smmu_config_builder_custom() {
        let config = SMMUConfigBuilder::new()
            .with_stream_count(128)
            .with_pasid_bits(16)
            .with_caching(false)
            .with_two_stage(true)
            .build();

        assert_eq!(config.stream_count, 128);
        assert_eq!(config.pasid_bits, 16);
        assert!(!config.enable_caching);
        assert!(config.enable_two_stage);
    }

    #[test]
    #[should_panic(expected = "Stream count must be > 0")]
    fn test_smmu_config_builder_invalid_stream_count() {
        SMMUConfigBuilder::new().with_stream_count(0).build();
    }

    #[test]
    #[should_panic(expected = "PASID bits must be <= 20")]
    fn test_smmu_config_builder_invalid_pasid_bits() {
        SMMUConfigBuilder::new().with_pasid_bits(21).build();
    }

    #[test]
    fn test_address_space_builder_defaults() {
        let config = AddressSpaceBuilder::new().build();
        assert_eq!(config.page_size, 4096);
        assert_eq!(config.total_pages, 256);
        assert!(!config.identity_mapped);
    }

    #[test]
    fn test_translation_test_builder() {
        let test = TranslationTestBuilder::new()
            .with_stream_id(5)
            .with_pasid(10)
            .with_iova(0x2000)
            .expect_pa(0x3000)
            .build();

        assert_eq!(test.stream_id, 5);
        assert_eq!(test.pasid, 10);
        assert_eq!(test.iova, 0x2000);
        assert_eq!(test.expected_pa, Some(0x3000));
        assert!(!test.should_fault);
    }
}
