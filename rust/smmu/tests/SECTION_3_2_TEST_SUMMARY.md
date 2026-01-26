# Section 3.2 Test Suite Summary: Address Space Operations

## Overview

This document summarizes the comprehensive failing test suite for TASKS-RUST.md Section 3.2: Address Space Operations. All tests are written BEFORE implementation following Test-Driven Development (TDD) principles.

**Test File**: `/home/jpgreninger/Work/smmu/rust/smmu/tests/test_address_space_section_3_2.rs`

**Total Tests**: 34 comprehensive tests covering 4 major feature areas

**Current Status**: ✅ All tests compile but fail (as expected - implementations not yet written)

---

## Section 3.2 Requirements (from TASKS-RUST.md)

### 3.2.1 Address Range Mapping with Iterator API (5 hours)
- Iterators for zero-cost abstractions
- IntoIterator implementation for ergonomic usage
- Parallel iteration with rayon if beneficial

### 3.2.2 Bulk Page Operations with Batch Processing (4 hours)
- Minimize lock contention with batching
- Vec for efficient bulk operations
- SmallVec for stack optimization (small batches)

### 3.2.3 State Querying with Immutable Borrows (3 hours)
- API preventing mutation during queries
- & references for efficient read access
- Iterator for lazy evaluation

### 3.2.4 Cache Invalidation with Proper Synchronization (4 hours)
- Atomic operations where possible
- Proper memory ordering semantics
- Fence operations for cross-core visibility

---

## Test Breakdown

### 3.2.1 Iterator API Tests (7 tests)

#### 1. `test_address_range_into_iterator`
**Purpose**: Test IntoIterator implementation for AddressRange
**Missing Implementation**: `AddressRange::into_iter()` method
**Test Coverage**:
- Maps contiguous range of 11 pages
- Iterates using `for page in range.into_iter()`
- Verifies all pages visited (IOVA range validation)
- Validates correct page count (11 pages for 0-10 inclusive)

#### 2. `test_address_space_iter`
**Purpose**: Test immutable iterator over all mapped pages
**Missing Implementation**: `AddressSpace::iter()` method
**Test Coverage**:
- Maps 5 non-contiguous pages
- Uses `addr_space.iter()` to enumerate all mappings
- Verifies immutable references to (IOVA, &PageEntry)
- Validates correct count (5 pages)

#### 3. `test_address_space_iter_mut`
**Purpose**: Test mutable iterator for in-place updates
**Missing Implementation**: `AddressSpace::iter_mut()` method
**Test Coverage**:
- Maps 5 pages with read-only permissions
- Uses `iter_mut()` to update permissions to read-write
- Verifies mutations are persisted
- Validates permission changes successful

#### 4. `test_iterator_lazy_evaluation`
**Purpose**: Verify iterators use lazy evaluation
**Missing Implementation**: Lazy iterator implementation
**Test Coverage**:
- Maps 1000 pages
- Creates iterator without consuming
- Takes only first 10 items
- Validates O(1) iterator creation (behavioral test)

#### 5. `test_iterator_filter_map_chain`
**Purpose**: Test iterator combinator chains
**Missing Implementation**: Iterator trait implementations
**Test Coverage**:
- Maps 20 pages with alternating permissions
- Uses `.filter().map()` combinator chain
- Validates filtered results (10 writable pages)
- Checks odd indices are writable

#### 6. `test_iterator_parallel_with_rayon` [IGNORED]
**Purpose**: Test parallel iteration with rayon
**Missing Implementation**: `par_iter()` method (requires rayon)
**Test Coverage**:
- Maps 10000 pages for stress test
- Uses rayon's parallel iterators
- Validates count with concurrent filtering
**Status**: Ignored until rayon dependency added

#### 7. `test_iterator_security_state_filtering`
**Purpose**: Test filtering by security state
**Missing Implementation**: Security state access in iterator
**Test Coverage**:
- Maps 30 pages with 3 security states (rotating)
- Filters for SecurityState::Secure pages
- Validates correct count (10 secure pages)

---

### 3.2.2 Bulk Operations Tests (7 tests)

#### 8. `test_bulk_map_with_batching`
**Purpose**: Test bulk mapping with internal batching
**Missing Implementation**: `map_pages_batched()` method
**Test Coverage**:
- Creates 1000 page mappings
- Uses batched API to minimize lock contention
- Validates all pages mapped (1000 total)

#### 9. `test_bulk_unmap_with_batching`
**Purpose**: Test bulk unmapping with batching
**Missing Implementation**: `unmap_pages_batched()` method
**Test Coverage**:
- Maps 500 pages
- Bulk unmaps all in batched operation
- Verifies page count drops to 0

#### 10. `test_small_batch_stack_optimization`
**Purpose**: Test SmallVec stack optimization for small batches
**Missing Implementation**: SmallVec usage in batching
**Test Coverage**:
- Maps 8 pages (small batch)
- Expects stack allocation (SmallVec<[_; 16]>)
- Validates mapping success

#### 11. `test_batch_permission_update`
**Purpose**: Test bulk permission updates
**Missing Implementation**: `update_permissions_batched()` method
**Test Coverage**:
- Maps 100 pages with read-only
- Bulk updates to read-write
- Verifies all permissions updated

#### 12. `test_batch_with_partial_failure`
**Purpose**: Test transactional semantics for batch operations
**Missing Implementation**: Transactional or partial failure handling
**Test Coverage**:
- Creates batch with invalid entry (u64::MAX address)
- Validates either all-or-nothing OR detailed error reporting
- Checks well-defined behavior

#### 13. `test_concurrent_batch_operations`
**Purpose**: Test concurrent batching with reduced lock contention
**Missing Implementation**: Concurrent batching support
**Test Coverage**:
- Spawns 4 threads mapping 250 pages each
- Validates all 1000 pages mapped
- Tests lock contention reduction

---

### 3.2.3 State Querying Tests (6 tests)

#### 14. `test_query_prevents_mutation`
**Purpose**: Test immutable query API prevents mutations
**Missing Implementation**: `AddressSpace::query()` method
**Test Coverage**:
- Creates query interface
- Verifies borrow checker prevents mutations during query
- Tests query operations (page_count, is_mapped)
- Validates mutations allowed after query dropped

#### 15. `test_immutable_reference_query`
**Purpose**: Test efficient read access with references
**Missing Implementation**: `query_page()` returning &PageEntry
**Test Coverage**:
- Queries single page
- Returns reference (no copy/clone)
- Validates physical address via reference

#### 16. `test_lazy_query_iterator`
**Purpose**: Test lazy evaluation in query iterators
**Missing Implementation**: Lazy query iterator
**Test Coverage**:
- Maps 1000 pages
- Creates filtered iterator
- Validates lazy evaluation (only evaluates until condition met)

#### 17. `test_concurrent_immutable_queries`
**Purpose**: Test multiple concurrent readers
**Missing Implementation**: Concurrent query support
**Test Coverage**:
- Spawns 10 concurrent reader threads
- Each performs independent queries
- Validates no lock contention for readers

#### 18. `test_query_range_statistics`
**Purpose**: Test statistics over address range
**Missing Implementation**: `query().range_statistics()` method
**Test Coverage**:
- Maps 50 pages with varying permissions
- Queries statistics (total, readable, writable, executable)
- Validates counts without copying entries

---

### 3.2.4 Cache Invalidation Tests (11 tests)

#### 19. `test_atomic_cache_invalidation`
**Purpose**: Test atomic invalidation operations
**Missing Implementation**: `invalidate_page_atomic()` method
**Test Coverage**:
- Maps 10 pages
- Invalidates using atomic operations
- Verifies invalidation occurred

#### 20. `test_memory_ordering_acquire_release`
**Purpose**: Test Acquire/Release memory ordering
**Missing Implementation**: `invalidate_page_with_ordering()` method
**Test Coverage**:
- Thread 1: Maps and invalidates with Release ordering
- Thread 2: Checks invalidation with Acquire ordering
- Validates cross-thread visibility

#### 21. `test_fence_cross_core_visibility`
**Purpose**: Test fence operations for cross-core visibility
**Missing Implementation**: Fence integration in invalidation
**Test Coverage**:
- Uses `std::sync::atomic::fence(Ordering::Release/Acquire)`
- Thread 1: Invalidates and sets flag
- Thread 2: Waits for flag and verifies invalidation visible
- Tests cross-core memory synchronization

#### 22. `test_invalidation_seqcst_ordering`
**Purpose**: Test SeqCst ordering for strongest guarantees
**Missing Implementation**: SeqCst invalidation support
**Test Coverage**:
- 4 threads invalidating 5 pages each (20 total)
- All use SeqCst ordering
- Atomic counter validates all invalidations visible

#### 23. `test_bulk_invalidation_atomic`
**Purpose**: Test bulk invalidation with atomics
**Missing Implementation**: `invalidate_range_atomic()` method
**Test Coverage**:
- Maps 100 pages
- Bulk invalidates range atomically
- Returns count of invalidated pages (100)

#### 24. `test_invalidation_generation_counter`
**Purpose**: Test generation counter for invalidation tracking
**Missing Implementation**: `invalidation_generation()` method
**Test Coverage**:
- Tracks invalidation generation counter
- Verifies counter increments on each invalidation
- Tests multiple invalidations increment multiple times

#### 25. `test_cross_thread_invalidation_visibility`
**Purpose**: Test invalidation visibility across threads
**Missing Implementation**: Thread-safe invalidation state
**Test Coverage**:
- Thread 1: Invalidates specific page
- Thread 2: Checks if invalidation visible
- Uses atomic flag for synchronization
- Validates proper memory ordering

#### 26. `test_compare_exchange_invalidation`
**Purpose**: Test compare-and-exchange for invalidation
**Missing Implementation**: `compare_exchange_invalidate()` method
**Test Coverage**:
- First CAS succeeds (valid → invalid)
- Second CAS fails (already invalid)
- Uses AcqRel ordering for synchronization

---

### Integration Tests (3 tests)

#### 27. `test_iterator_with_concurrent_invalidation`
**Purpose**: Test iterator stability during invalidations
**Missing Implementation**: Concurrent-safe iterator
**Test Coverage**:
- Maps 50 pages
- Thread 1: Iterates over all pages
- Thread 2: Invalidates 10 pages concurrently
- Validates consistent snapshot (40-50 pages)

#### 28. `test_batch_operations_with_query`
**Purpose**: Combine batch operations with queries
**Missing Implementation**: Integrated batch + query API
**Test Coverage**:
- Bulk maps 100 pages
- Queries state (count, writable)
- Validates combined functionality

---

## Missing Implementations Summary

### New Methods Required

#### AddressSpace Core Methods (10 methods)
1. `iter(&self) -> impl Iterator<Item = PageEntryRef>` - Immutable iterator
2. `iter_mut(&mut self) -> impl Iterator<Item = PageEntryMut>` - Mutable iterator
3. `query(&self) -> AddressSpaceQuery` - Immutable query interface
4. `query_page(&self, iova: IOVA) -> Option<&PageEntry>` - Single page query
5. `map_pages_batched(&mut self, mappings: &[(IOVA, PA)], perms: PagePermissions) -> Result<()>` - Batched mapping
6. `unmap_pages_batched(&mut self, iovas: &[IOVA]) -> Result<()>` - Batched unmapping
7. `update_permissions_batched(&mut self, iovas: &[IOVA], perms: PagePermissions) -> Result<()>` - Batched permission updates
8. `invalidate_page_atomic(&mut self, iova: IOVA)` - Atomic invalidation
9. `invalidate_range_atomic(&mut self, start: IOVA, end: IOVA) -> usize` - Atomic range invalidation
10. `invalidate_page_with_ordering(&mut self, iova: IOVA, ordering: Ordering)` - Invalidation with explicit ordering

#### AddressSpace Query Methods (4 methods)
11. `is_invalidated(&self, iova: IOVA) -> bool` - Check invalidation state
12. `is_invalidated_with_ordering(&self, iova: IOVA, ordering: Ordering) -> bool` - Check with ordering
13. `invalidation_generation(&self) -> u64` - Get generation counter
14. `compare_exchange_invalidate(&mut self, iova: IOVA, current: bool, new: bool, ordering: Ordering) -> bool` - CAS invalidation

#### AddressRange Methods (1 method)
15. `into_iter(self) -> impl Iterator<Item = PageInfo>` - Convert to iterator

#### AddressSpaceQuery Methods (3 methods)
16. `page_count(&self) -> usize` - Count mapped pages
17. `is_mapped(&self, iova: IOVA) -> bool` - Check if page mapped
18. `iter(&self) -> impl Iterator<Item = &PageEntry>` - Lazy iterator
19. `range_statistics(&self, start: IOVA, end: IOVA) -> RangeStats` - Range statistics

### New Types Required

#### 1. `AddressSpaceQuery<'a>`
Immutable query interface that borrows AddressSpace
```rust
pub struct AddressSpaceQuery<'a> {
    addr_space: &'a AddressSpace,
}
```

#### 2. `RangeStats`
Statistics for address range queries
```rust
pub struct RangeStats {
    pub total_pages: usize,
    pub readable_pages: usize,
    pub writable_pages: usize,
    pub executable_pages: usize,
}
```

#### 3. `PageEntryRef` / `PageEntryMut`
Iterator item types (or use tuple)
```rust
pub struct PageEntryRef<'a> {
    iova: IOVA,
    entry: &'a PageEntry,
}
```

#### 4. `PageInfo`
Information from AddressRange iteration
```rust
pub struct PageInfo {
    iova: IOVA,
    // ... other fields
}
```

### Dependencies to Add

#### Optional: rayon (for parallel iteration)
```toml
[dev-dependencies]
rayon = "1.8"
```

#### Optional: smallvec (for stack optimization)
```toml
[dependencies]
smallvec = "1.11"
```

---

## Implementation Priorities

### Phase 1: Core Iterator Support (Highest Priority)
**Time Estimate**: 5 hours
**Tests**: 7 tests (3.2.1)
- Implement `iter()` and `iter_mut()` methods
- Add IntoIterator for AddressRange
- Enable lazy evaluation
- **Blockers**: None
- **Dependencies**: Section 3.1 complete ✅

### Phase 2: Bulk Operations with Batching (High Priority)
**Time Estimate**: 4 hours
**Tests**: 7 tests (3.2.2)
- Implement batched map/unmap/update operations
- Add SmallVec for stack optimization
- Support transactional semantics
- **Blockers**: None
- **Dependencies**: Section 3.1 complete ✅

### Phase 3: State Querying (Medium Priority)
**Time Estimate**: 3 hours
**Tests**: 6 tests (3.2.3)
- Create AddressSpaceQuery interface
- Implement immutable query methods
- Add range statistics
- **Blockers**: Iterator support (Phase 1)
- **Dependencies**: Phase 1

### Phase 4: Cache Invalidation Synchronization (Medium Priority)
**Time Estimate**: 4 hours
**Tests**: 11 tests (3.2.4)
- Add atomic invalidation operations
- Implement memory ordering support
- Add generation counter tracking
- **Blockers**: None
- **Dependencies**: Section 3.1 complete ✅

### Phase 5: Integration Testing (Low Priority)
**Time Estimate**: 2 hours (covered by existing tests)
**Tests**: 3 integration tests
- Validate combined functionality
- Test concurrent scenarios
- **Blockers**: Phases 1-4 complete
- **Dependencies**: All previous phases

---

## Test Execution Instructions

### Current Status
```bash
cd /home/jpgreninger/Work/smmu/rust/smmu
cargo test --test test_address_space_section_3_2 --no-run
```

**Expected Result**: ❌ 40 compilation errors (missing implementations)

### After Implementation
```bash
# Run all section 3.2 tests
cargo test --test test_address_space_section_3_2

# Run specific feature tests
cargo test --test test_address_space_section_3_2 test_iterator
cargo test --test test_address_space_section_3_2 test_bulk
cargo test --test test_address_space_section_3_2 test_query
cargo test --test test_address_space_section_3_2 test_invalidation

# Run with rayon tests (when implemented)
cargo test --test test_address_space_section_3_2 -- --include-ignored
```

---

## Code Quality Targets

### Test Coverage
- **Target**: >95% line coverage for all new implementations
- **Measurement**: `cargo llvm-cov --test test_address_space_section_3_2`

### Performance
- **Iterator Creation**: O(1) time complexity
- **Lazy Evaluation**: Verified through behavioral tests
- **Batch Operations**: Minimize lock acquisitions (measured via profiling)
- **Memory Ordering**: Correct synchronization (verified via ThreadSanitizer)

### Memory Safety
- **Zero Unsafe Code**: All implementations must be safe Rust
- **Miri Clean**: `cargo +nightly miri test --test test_address_space_section_3_2`
- **ThreadSanitizer**: No data races detected

### Documentation
- **All Public APIs**: Comprehensive rustdoc with examples
- **Safety Requirements**: Document any preconditions
- **Memory Ordering**: Document ordering guarantees

---

## ARM SMMU v3 Specification Compliance

All implementations must maintain 100% ARM SMMU v3 specification compliance:

1. **Page Table Operations**: Must preserve ARM spec semantics
2. **Permission Checking**: Must enforce ARM permission model
3. **Security State Isolation**: Must maintain isolation boundaries
4. **Fault Detection**: Must detect all fault conditions per spec
5. **Cache Invalidation**: Must follow ARM cache coherency rules

---

## TDD Workflow Checklist

For each implementation phase:

- [ ] **Phase 1**: Review failing tests for feature
- [ ] **Phase 2**: Implement minimal code to pass tests
- [ ] **Phase 3**: Run tests - verify all pass
- [ ] **Phase 4**: Refactor for performance and clarity
- [ ] **Phase 5**: Run tests again - verify still passing
- [ ] **Phase 6**: Use `qa-engineer` to review against ARM spec
- [ ] **Phase 7**: Update TASKS-RUST.md progress
- [ ] **Phase 8**: Commit with clear message

**Mandatory**: Never skip test execution. Tests must fail before implementation and pass after.

---

## Success Criteria

✅ **Section 3.2 Complete** when:

1. All 34 tests passing (100% success rate)
2. Zero unsafe code in implementations
3. >95% code coverage achieved
4. Zero Clippy warnings (pedantic mode)
5. Miri clean (no undefined behavior)
6. ThreadSanitizer clean (no data races)
7. Performance targets met (O(1) iterators, efficient batching)
8. Full ARM SMMU v3 compliance maintained
9. Comprehensive rustdoc documentation
10. All methods integrated into existing test suite

**Estimated Total Time**: 16 hours (5 + 4 + 3 + 4 implementation)

**Current Status**: ✅ Tests written and ready for implementation

**Next Step**: Begin Phase 1 (Core Iterator Support) using rust-engineer subagent

---

## Related Documentation

- **TASKS-RUST.md**: Section 3.2 requirements (lines 525-546)
- **AddressSpace Implementation**: `/home/jpgreninger/Work/smmu/rust/smmu/src/address_space/mod.rs`
- **Existing Tests**: `/home/jpgreninger/Work/smmu/rust/smmu/tests/test_address_space.rs`
- **ARM SMMU v3 Spec**: Section on address translation and caching

---

## Notes

1. **Rayon Integration**: Test `test_iterator_parallel_with_rayon` is currently ignored. Enable after adding rayon dependency.

2. **SmallVec Optimization**: Consider adding smallvec dependency for stack optimization of small batches.

3. **Atomic Operations**: All cache invalidation tests require proper memory ordering - critical for thread safety.

4. **Iterator Stability**: Iterators should provide consistent snapshots even under concurrent modifications.

5. **Borrow Checker**: Query API design leverages borrow checker to prevent mutations during queries.

---

**Document Version**: 1.0
**Created**: January 25, 2026
**Status**: Ready for Implementation
