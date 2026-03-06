#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::doc_lazy_continuation)]

//! TDD test for BUG-RUST-5: pasids_by_security_state() always returns empty iterator
//!
//! In `src/stream_context/mod.rs` around lines 2134–2144, the public method
//! `StreamContextQuery::pasids_by_security_state()` is a stub that always returns
//! `std::iter::empty()` regardless of the security state argument or how many PASIDs
//! have been created on the stream.
//!
//! # Data model
//!
//! Security state is tracked at the *stream* level, not per-PASID (the `security_state`
//! `AtomicU8` field on `StreamContext` holds the stream's security state, written by
//! `set_security_state()` or `update_configuration()`).  Individual PASIDs do not carry
//! independent security state attributes.
//!
//! # Correct semantics
//!
//! Given the stream-level security state model, the correct behaviour is:
//! - If the stream's security state matches the requested state, return an iterator
//!   over *all* PASIDs configured on that stream.
//! - If the stream's security state does not match, return an empty iterator.
//!
//! This is consistent with the ARM SMMU v3 specification §5.2 where the STE.SEC_SID
//! field classifies the entire stream (and therefore all its substreams/PASIDs) into
//! a single security domain.
//!
//! # Test Strategy
//!
//! 1. Configure a stream with NonSecure security state via `set_security_state()`.
//! 2. Add two PASIDs (1 and 2) with address spaces.
//! 3. Call `pasids_by_security_state(SecurityState::NonSecure)` — FAILS before fix
//!    (stub always returns empty).
//! 4. Call `pasids_by_security_state(SecurityState::Secure)` — must return empty
//!    (stream is NonSecure, not Secure).
//! 5. After fix, step 3 returns both PASIDs.

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, IOVA, PA, PASID};

// ── Helper ───────────────────────────────────────────────────────────────────

fn make_nonsecure_stream() -> StreamContext {
    let ctx = StreamContext::new();
    // Default is already NonSecure; set explicitly to be declarative.
    ctx.set_security_state(SecurityState::NonSecure);
    ctx
}

fn add_pasid_with_mapping(ctx: &StreamContext, pasid_val: u32) {
    let pasid = PASID::new(pasid_val).unwrap();
    ctx.create_pasid(pasid).unwrap();
    let iova = IOVA::new(u64::from(pasid_val) * 0x1000).unwrap();
    let pa = PA::new(u64::from(pasid_val) * 0x2000).unwrap();
    ctx.map_page(pasid, iova, pa, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();
}

// ── Test 1: THE CRITICAL FAILING TEST — NonSecure stream returns all PASIDs ──

/// A NonSecure stream with two PASIDs must return both when querying NonSecure.
///
/// BEFORE FIX: `pasids_by_security_state()` is a stub that returns `empty()`,
/// so `count()` is always 0.
///
/// AFTER FIX: The method checks whether the stream's security state matches the
/// requested state and, if so, returns an iterator over all configured PASIDs.
#[test]
fn pasids_by_security_state_nonsecure_stream_returns_all_pasids() {
    let ctx = make_nonsecure_stream();
    add_pasid_with_mapping(&ctx, 1);
    add_pasid_with_mapping(&ctx, 2);

    let query = ctx.query();
    let pasids: Vec<PASID> = query
        .pasids_by_security_state(SecurityState::NonSecure)
        .collect();

    let len = pasids.len();
    assert_eq!(
        len,
        2,
        "BUG-RUST-5: pasids_by_security_state(NonSecure) on a NonSecure stream \
         with 2 PASIDs must return 2, got {len}",
    );

    // Verify the returned PASIDs are the ones we inserted.
    let mut pasid_vals: Vec<u32> = pasids.iter().map(|p| p.as_u32()).collect();
    pasid_vals.sort_unstable();
    assert_eq!(
        pasid_vals,
        vec![1, 2],
        "BUG-RUST-5: returned PASIDs must be [1, 2], got {pasid_vals:?}",
    );
}

// ── Test 2: Querying a mismatched security state returns empty ────────────────

/// Querying Secure on a NonSecure stream must return empty.
///
/// This test should pass before AND after the fix because the stub also returns
/// empty for mismatched states.  Included to guard against over-eager fixes
/// that return all PASIDs regardless of the requested state.
#[test]
fn pasids_by_security_state_nonsecure_stream_secure_query_is_empty() {
    let ctx = make_nonsecure_stream();
    add_pasid_with_mapping(&ctx, 1);
    add_pasid_with_mapping(&ctx, 2);

    let query = ctx.query();
    let count = query
        .pasids_by_security_state(SecurityState::Secure)
        .count();

    assert_eq!(
        count, 0,
        "A NonSecure stream must return 0 PASIDs when queried for Secure, got {count}",
    );
}

// ── Test 3: Empty stream returns empty regardless of security state ────────────

/// An empty stream (no PASIDs) must return 0 for any security state query.
#[test]
fn pasids_by_security_state_empty_stream_is_always_empty() {
    let ctx = make_nonsecure_stream();
    let query = ctx.query();

    assert_eq!(query.pasids_by_security_state(SecurityState::NonSecure).count(), 0);
    assert_eq!(query.pasids_by_security_state(SecurityState::Secure).count(), 0);
    assert_eq!(query.pasids_by_security_state(SecurityState::Realm).count(), 0);
    assert_eq!(query.pasids_by_security_state(SecurityState::Root).count(), 0);
}

// ── Test 4: Single PASID on matching state ────────────────────────────────────

/// A stream with a single PASID returns exactly 1 when queried with the matching
/// security state, and 0 for all other states.
#[test]
fn pasids_by_security_state_single_pasid_matching_state() {
    let ctx = make_nonsecure_stream();
    add_pasid_with_mapping(&ctx, 5);

    let query = ctx.query();

    assert_eq!(
        query.pasids_by_security_state(SecurityState::NonSecure).count(),
        1,
        "Single-PASID NonSecure stream must return 1 for NonSecure query"
    );
    assert_eq!(
        query.pasids_by_security_state(SecurityState::Secure).count(),
        0,
        "Single-PASID NonSecure stream must return 0 for Secure query"
    );
}

// ── Test 5: Secure stream returns all PASIDs for Secure query ─────────────────

/// A Secure stream with PASIDs must return them for a Secure query and zero
/// for a NonSecure query.
#[test]
fn pasids_by_security_state_secure_stream_returns_all_pasids_for_secure_query() {
    let ctx = StreamContext::new();
    ctx.set_security_state(SecurityState::Secure);

    // Add PASIDs (no address space mappings needed — we only test the iterator)
    let pasid1 = PASID::new(10).unwrap();
    let pasid2 = PASID::new(20).unwrap();
    ctx.create_pasid(pasid1).unwrap();
    ctx.create_pasid(pasid2).unwrap();

    let query = ctx.query();

    let secure_count = query.pasids_by_security_state(SecurityState::Secure).count();
    assert_eq!(
        secure_count, 2,
        "Secure stream with 2 PASIDs must return 2 for Secure query, got {secure_count}",
    );

    let nonsecure_count = query
        .pasids_by_security_state(SecurityState::NonSecure)
        .count();
    assert_eq!(
        nonsecure_count, 0,
        "Secure stream must return 0 for NonSecure query, got {nonsecure_count}",
    );
}

// ── Test 6: Returned PASIDs are valid and translatable ───────────────────────

/// PASIDs returned by `pasids_by_security_state()` must be functional — each
/// must be present in the context and able to perform translations.
#[test]
fn pasids_by_security_state_returned_pasids_are_translatable() {
    let ctx = make_nonsecure_stream();

    let pasid_val = 7u32;
    let iova_addr = 0x7000u64;
    let pa_addr = 0xE000u64;

    let pasid = PASID::new(pasid_val).unwrap();
    ctx.create_pasid(pasid).unwrap();
    ctx.map_page(
        pasid,
        IOVA::new(iova_addr).unwrap(),
        PA::new(pa_addr).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let query = ctx.query();
    let returned_pasids: Vec<PASID> = query
        .pasids_by_security_state(SecurityState::NonSecure)
        .collect();

    assert_eq!(returned_pasids.len(), 1);
    let returned_pasid = returned_pasids[0];

    let result = ctx.translate(
        returned_pasid,
        IOVA::new(iova_addr).unwrap(),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "PASID returned by pasids_by_security_state must be translatable, got: {result:?}",
    );
    assert_eq!(result.unwrap().physical_address(), PA::new(pa_addr).unwrap());
}
