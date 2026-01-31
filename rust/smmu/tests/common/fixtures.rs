//! Test fixtures and sample data
//!
//! Provides pre-defined test scenarios, sample configurations, and
//! commonly used test data.

/// Standard test page size (4KB)
pub const TEST_PAGE_SIZE_4K: u64 = 4096;

/// Large page size (2MB)
pub const TEST_PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;

/// Huge page size (1GB)
pub const TEST_PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;

/// Maximum physical address bits (48-bit)
pub const TEST_MAX_PA_BITS: u8 = 48;

/// Maximum virtual address bits (48-bit)
pub const TEST_MAX_VA_BITS: u8 = 48;

/// Default test stream ID
pub const TEST_STREAM_ID: u32 = 42;

/// Default test PASID
pub const TEST_PASID: u32 = 0;

/// Test PASID for multi-PASID scenarios
pub const TEST_PASID_SECONDARY: u32 = 1;

/// Sample virtual address range for testing
pub const TEST_VA_RANGE_START: u64 = 0x0000_0000_1000_0000;
pub const TEST_VA_RANGE_END: u64 = 0x0000_0000_2000_0000;

/// Sample physical address range for testing
pub const TEST_PA_RANGE_START: u64 = 0x0000_8000_0000_0000;
pub const TEST_PA_RANGE_END: u64 = 0x0000_8000_1000_0000;

/// Standard test configuration with commonly used values
pub struct StandardTestFixture {
    pub page_size: u64,
    pub num_pages: usize,
    pub stream_id: u32,
    pub pasid: u32,
    pub base_va: u64,
    pub base_pa: u64,
}

impl StandardTestFixture {
    /// Create a standard test fixture with default values
    #[must_use]
    pub fn new() -> Self {
        Self {
            page_size: TEST_PAGE_SIZE_4K,
            num_pages: 256,
            stream_id: TEST_STREAM_ID,
            pasid: TEST_PASID,
            base_va: TEST_VA_RANGE_START,
            base_pa: TEST_PA_RANGE_START,
        }
    }

    /// Create a fixture with large pages
    #[must_use]
    pub fn with_large_pages() -> Self {
        Self {
            page_size: TEST_PAGE_SIZE_2M,
            num_pages: 64,
            ..Self::new()
        }
    }

    /// Create a fixture with huge pages
    #[must_use]
    pub fn with_huge_pages() -> Self {
        Self {
            page_size: TEST_PAGE_SIZE_1G,
            num_pages: 16,
            ..Self::new()
        }
    }

    /// Generate an array of test virtual addresses
    #[must_use]
    pub fn generate_test_vas(&self, count: usize) -> Vec<u64> {
        (0..count)
            .map(|i| self.base_va + (u64::from(i) * self.page_size))
            .collect()
    }

    /// Generate an array of test physical addresses
    #[must_use]
    pub fn generate_test_pas(&self, count: usize) -> Vec<u64> {
        (0..count)
            .map(|i| self.base_pa + (u64::from(i) * self.page_size))
            .collect()
    }

    /// Generate identity-mapped VA/PA pairs
    #[must_use]
    pub fn generate_identity_mappings(&self, count: usize) -> Vec<(u64, u64)> {
        (0..count)
            .map(|i| {
                let addr = self.base_va + (u64::from(i) * self.page_size);
                (addr, addr)
            })
            .collect()
    }
}

impl Default for StandardTestFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-stream test scenario
pub struct MultiStreamFixture {
    pub num_streams: u32,
    pub pages_per_stream: usize,
}

impl MultiStreamFixture {
    /// Create a multi-stream test fixture
    #[must_use]
    pub fn new(num_streams: u32, pages_per_stream: usize) -> Self {
        Self {
            num_streams,
            pages_per_stream,
        }
    }

    /// Get total number of pages across all streams
    #[must_use]
    pub fn total_pages(&self) -> usize {
        self.num_streams as usize * self.pages_per_stream
    }
}

/// Multi-PASID test scenario
pub struct MultiPasidFixture {
    pub num_pasids: u32,
    pub pages_per_pasid: usize,
}

impl MultiPasidFixture {
    /// Create a multi-PASID test fixture
    #[must_use]
    pub fn new(num_pasids: u32, pages_per_pasid: usize) -> Self {
        Self {
            num_pasids,
            pages_per_pasid,
        }
    }

    /// Get total number of pages across all PASIDs
    #[must_use]
    pub fn total_pages(&self) -> usize {
        self.num_pasids as usize * self.pages_per_pasid
    }

    /// Generate test PASIDs
    #[must_use]
    pub fn generate_pasids(&self) -> Vec<u32> {
        (0..self.num_pasids).collect()
    }
}

/// Performance test fixture with large-scale data
pub struct PerformanceFixture {
    pub num_streams: u32,
    pub num_pasids: u32,
    pub pages_per_context: usize,
    pub iterations: usize,
}

impl PerformanceFixture {
    /// Create a performance test fixture
    #[must_use]
    pub fn new() -> Self {
        Self {
            num_streams: 256,
            num_pasids: 16,
            pages_per_context: 1024,
            iterations: 10000,
        }
    }

    /// Create a small performance fixture for quick tests
    #[must_use]
    pub fn small() -> Self {
        Self {
            num_streams: 16,
            num_pasids: 4,
            pages_per_context: 64,
            iterations: 1000,
        }
    }

    /// Create a large performance fixture for stress tests
    #[must_use]
    pub fn large() -> Self {
        Self {
            num_streams: 1024,
            num_pasids: 64,
            pages_per_context: 4096,
            iterations: 100000,
        }
    }

    /// Get total number of translation contexts
    #[must_use]
    pub fn total_contexts(&self) -> usize {
        self.num_streams as usize * self.num_pasids as usize
    }

    /// Get total number of pages
    #[must_use]
    pub fn total_pages(&self) -> usize {
        self.total_contexts() * self.pages_per_context
    }
}

impl Default for PerformanceFixture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_fixture_defaults() {
        let fixture = StandardTestFixture::new();
        assert_eq!(fixture.page_size, TEST_PAGE_SIZE_4K);
        assert_eq!(fixture.num_pages, 256);
        assert_eq!(fixture.stream_id, TEST_STREAM_ID);
        assert_eq!(fixture.pasid, TEST_PASID);
    }

    #[test]
    fn test_standard_fixture_large_pages() {
        let fixture = StandardTestFixture::with_large_pages();
        assert_eq!(fixture.page_size, TEST_PAGE_SIZE_2M);
    }

    #[test]
    fn test_generate_test_vas() {
        let fixture = StandardTestFixture::new();
        let vas = fixture.generate_test_vas(4);
        assert_eq!(vas.len(), 4);
        assert_eq!(vas[0], fixture.base_va);
        assert_eq!(vas[1], fixture.base_va + TEST_PAGE_SIZE_4K);
    }

    #[test]
    fn test_generate_identity_mappings() {
        let fixture = StandardTestFixture::new();
        let mappings = fixture.generate_identity_mappings(4);
        assert_eq!(mappings.len(), 4);
        assert_eq!(mappings[0].0, mappings[0].1);
    }

    #[test]
    fn test_multi_stream_fixture() {
        let fixture = MultiStreamFixture::new(16, 256);
        assert_eq!(fixture.total_pages(), 16 * 256);
    }

    #[test]
    fn test_multi_pasid_fixture() {
        let fixture = MultiPasidFixture::new(8, 128);
        assert_eq!(fixture.total_pages(), 8 * 128);
        assert_eq!(fixture.generate_pasids().len(), 8);
    }

    #[test]
    fn test_performance_fixture() {
        let fixture = PerformanceFixture::new();
        assert_eq!(fixture.total_contexts(), 256 * 16);
        assert_eq!(fixture.total_pages(), 256 * 16 * 1024);
    }

    #[test]
    fn test_performance_fixture_small() {
        let fixture = PerformanceFixture::small();
        assert_eq!(fixture.num_streams, 16);
        assert_eq!(fixture.iterations, 1000);
    }
}
