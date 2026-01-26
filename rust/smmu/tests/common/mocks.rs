//! Mock objects and test doubles
//!
//! Provides mock implementations for testing components in isolation

/// Mock translation result for testing
///
/// This will be replaced with actual types as they're implemented
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockTranslationResult {
    Success { pa: u64, size: u64 },
    Fault { fault_type: MockFaultType },
}

/// Mock fault types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockFaultType {
    Translation,
    Permission,
    Access,
    External,
}

/// Mock page table entry
#[derive(Debug, Clone)]
pub struct MockPageTableEntry {
    pub valid: bool,
    pub pa: u64,
    pub size: u64,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl MockPageTableEntry {
    /// Create a valid page table entry
    #[must_use]
    pub fn valid(pa: u64, size: u64) -> Self {
        Self {
            valid: true,
            pa,
            size,
            readable: true,
            writable: true,
            executable: false,
        }
    }

    /// Create an invalid (unmapped) entry
    #[must_use]
    pub fn invalid() -> Self {
        Self {
            valid: false,
            pa: 0,
            size: 0,
            readable: false,
            writable: false,
            executable: false,
        }
    }

    /// Set read permission
    #[must_use]
    pub fn with_read(mut self, readable: bool) -> Self {
        self.readable = readable;
        self
    }

    /// Set write permission
    #[must_use]
    pub fn with_write(mut self, writable: bool) -> Self {
        self.writable = writable;
        self
    }

    /// Set execute permission
    #[must_use]
    pub fn with_execute(mut self, executable: bool) -> Self {
        self.executable = executable;
        self
    }
}

/// Mock address space for testing
#[derive(Debug)]
pub struct MockAddressSpace {
    entries: std::collections::HashMap<u64, MockPageTableEntry>,
    page_size: u64,
}

impl MockAddressSpace {
    /// Create a new mock address space
    #[must_use]
    pub fn new(page_size: u64) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            page_size,
        }
    }

    /// Map a page
    pub fn map(&mut self, va: u64, entry: MockPageTableEntry) {
        let page_addr = va & !(self.page_size - 1);
        self.entries.insert(page_addr, entry);
    }

    /// Unmap a page
    pub fn unmap(&mut self, va: u64) {
        let page_addr = va & !(self.page_size - 1);
        self.entries.remove(&page_addr);
    }

    /// Translate an address
    #[must_use]
    pub fn translate(&self, va: u64) -> MockTranslationResult {
        let page_addr = va & !(self.page_size - 1);
        let offset = va & (self.page_size - 1);

        match self.entries.get(&page_addr) {
            Some(entry) if entry.valid => MockTranslationResult::Success {
                pa: entry.pa + offset,
                size: entry.size,
            },
            _ => MockTranslationResult::Fault {
                fault_type: MockFaultType::Translation,
            },
        }
    }

    /// Get number of mapped pages
    #[must_use]
    pub fn mapped_pages(&self) -> usize {
        self.entries.len()
    }

    /// Clear all mappings
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_page_table_entry() {
        let entry = MockPageTableEntry::valid(0x1000, 4096);
        assert!(entry.valid);
        assert_eq!(entry.pa, 0x1000);
        assert!(entry.readable);
        assert!(entry.writable);
    }

    #[test]
    fn test_mock_page_table_entry_permissions() {
        let entry = MockPageTableEntry::valid(0x1000, 4096)
            .with_read(true)
            .with_write(false)
            .with_execute(true);

        assert!(entry.readable);
        assert!(!entry.writable);
        assert!(entry.executable);
    }

    #[test]
    fn test_mock_address_space_map_translate() {
        let mut space = MockAddressSpace::new(4096);
        let entry = MockPageTableEntry::valid(0x8000_0000, 4096);

        space.map(0x1000, entry);

        let result = space.translate(0x1000);
        match result {
            MockTranslationResult::Success { pa, .. } => {
                assert_eq!(pa, 0x8000_0000);
            }
            _ => panic!("Expected successful translation"),
        }
    }

    #[test]
    fn test_mock_address_space_translate_offset() {
        let mut space = MockAddressSpace::new(4096);
        let entry = MockPageTableEntry::valid(0x8000_0000, 4096);

        space.map(0x1000, entry);

        let result = space.translate(0x1100);
        match result {
            MockTranslationResult::Success { pa, .. } => {
                assert_eq!(pa, 0x8000_0100); // Base + offset
            }
            _ => panic!("Expected successful translation"),
        }
    }

    #[test]
    fn test_mock_address_space_unmapped() {
        let space = MockAddressSpace::new(4096);
        let result = space.translate(0x1000);

        match result {
            MockTranslationResult::Fault { fault_type } => {
                assert_eq!(fault_type, MockFaultType::Translation);
            }
            _ => panic!("Expected translation fault"),
        }
    }

    #[test]
    fn test_mock_address_space_unmap() {
        let mut space = MockAddressSpace::new(4096);
        let entry = MockPageTableEntry::valid(0x8000_0000, 4096);

        space.map(0x1000, entry);
        assert_eq!(space.mapped_pages(), 1);

        space.unmap(0x1000);
        assert_eq!(space.mapped_pages(), 0);

        let result = space.translate(0x1000);
        match result {
            MockTranslationResult::Fault { .. } => {}
            _ => panic!("Expected fault after unmap"),
        }
    }

    #[test]
    fn test_mock_address_space_clear() {
        let mut space = MockAddressSpace::new(4096);

        for i in 0..10 {
            let entry = MockPageTableEntry::valid(0x8000_0000 + i * 4096, 4096);
            space.map(0x1000 + i * 4096, entry);
        }

        assert_eq!(space.mapped_pages(), 10);

        space.clear();
        assert_eq!(space.mapped_pages(), 0);
    }
}
