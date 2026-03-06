#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD test for BUG-RUST-6: FxHasher::write() uses different multiplier than write_u64()
//!
//! In `src/cache/mod.rs`, `FxHasher::write()` processes bytes one-at-a-time using
//! the FNV-1a-64 prime `0x0100_0000_01B3`, while `FxHasher::write_u64()` uses the
//! MurmurHash3 constant `0xff51_afd7_ed55_8ccd` with an extra XOR-shift finaliser.
//!
//! The Rust `Hasher` trait contract requires that `write_u64(x)` produces the same
//! hash state as `write(&x.to_ne_bytes())`.  When the two methods use different
//! multipliers they diverge, breaking this contract.
//!
//! # Manifestation
//!
//! A `StreamID` or other small-integer key hashed via `write_u32()` → `write_u64()`
//! will get a completely different bucket from the same value pushed through
//! `write()` byte-by-byte.  Because `DashMap` chooses the internal path based
//! on the concrete `Hash` impl of the key type, the mismatch is latent but
//! surfaces when two different code paths hash the same logical key and expect
//! equal results (e.g. a custom derive vs a manual impl).
//!
//! # Fix
//!
//! Rewrite `write()` to accumulate bytes into little-endian u64 words and call
//! `write_u64()` for each complete word, then handle trailing bytes with a final
//! `write_u64()` padded with zeroes.  This ensures `write(&x.to_ne_bytes())` and
//! `write_u64(x)` are bit-identical regardless of the call path.
//!
//! # Test Strategy
//!
//! 1. Create an `FxHasher`, call `write(&42u64.to_ne_bytes())`, capture hash.
//! 2. Create a fresh `FxHasher`, call `write_u64(42u64)`, capture hash.
//! 3. Assert the two hashes are equal — this FAILS before the fix.
//! 4. After the fix the assertion passes.

use smmu::cache::FxHasher;
use std::hash::Hasher;

// ── Test 1: THE CRITICAL FAILING TEST — write() vs write_u64() consistency ──

/// `write(&x.to_ne_bytes())` must produce the same hash as `write_u64(x)`.
///
/// BEFORE FIX: `write()` uses FNV-1a prime `0x0100_0000_01B3` byte-by-byte;
/// `write_u64()` uses MurmurHash3 constant `0xff51_afd7_ed55_8ccd` + XOR-shift.
/// The two hashes are entirely different.
///
/// AFTER FIX: `write()` accumulates bytes into u64 words and delegates to
/// `write_u64()`, so both paths produce identical results.
#[test]
fn fxhasher_write_bytes_equals_write_u64_for_aligned_value() {
    let value: u64 = 42;

    let mut h1 = FxHasher::default();
    h1.write(&value.to_ne_bytes());
    let hash_via_bytes = h1.finish();

    let mut h2 = FxHasher::default();
    h2.write_u64(value);
    let hash_via_u64 = h2.finish();

    assert_eq!(
        hash_via_bytes,
        hash_via_u64,
        "BUG-RUST-6: FxHasher::write(&x.to_ne_bytes()) produced {hash_via_bytes:#018x} \
         but write_u64(x) produced {hash_via_u64:#018x}. \
         The two methods must produce identical hash state for the same data."
    );
}

// ── Test 2: write() vs write_u64() for zero ──────────────────────────────────

/// Consistency check for the zero value (edge case).
#[test]
fn fxhasher_write_bytes_equals_write_u64_for_zero() {
    let value: u64 = 0;

    let mut h1 = FxHasher::default();
    h1.write(&value.to_ne_bytes());
    let hash_via_bytes = h1.finish();

    let mut h2 = FxHasher::default();
    h2.write_u64(value);
    let hash_via_u64 = h2.finish();

    assert_eq!(
        hash_via_bytes,
        hash_via_u64,
        "BUG-RUST-6 (zero): write bytes={hash_via_bytes:#018x} != write_u64={hash_via_u64:#018x}"
    );
}

// ── Test 3: write() vs write_u64() for u64::MAX ──────────────────────────────

/// Consistency check for u64::MAX (all-ones, stress-tests the multiplier).
#[test]
fn fxhasher_write_bytes_equals_write_u64_for_max() {
    let value: u64 = u64::MAX;

    let mut h1 = FxHasher::default();
    h1.write(&value.to_ne_bytes());
    let hash_via_bytes = h1.finish();

    let mut h2 = FxHasher::default();
    h2.write_u64(value);
    let hash_via_u64 = h2.finish();

    assert_eq!(
        hash_via_bytes,
        hash_via_u64,
        "BUG-RUST-6 (MAX): write bytes={hash_via_bytes:#018x} != write_u64={hash_via_u64:#018x}"
    );
}

// ── Test 4: write() vs write_u64() for a typical StreamID-derived value ──────

/// Real-world scenario: a 32-bit StreamID value written as 4 bytes vs write_u32().
///
/// `write_u32()` delegates to `write_u64()` internally, so `write(&val.to_ne_bytes())`
/// must agree with `write_u32(val)` for 4-byte inputs.
#[test]
fn fxhasher_write_bytes_equals_write_u32_for_stream_id() {
    let value: u32 = 0x0ABC_1234;

    let mut h1 = FxHasher::default();
    h1.write(&value.to_ne_bytes());
    let hash_via_bytes = h1.finish();

    let mut h2 = FxHasher::default();
    h2.write_u32(value);
    let hash_via_u32 = h2.finish();

    assert_eq!(
        hash_via_bytes,
        hash_via_u32,
        "BUG-RUST-6 (u32): write(&val.to_ne_bytes())={hash_via_bytes:#018x} \
         != write_u32(val)={hash_via_u32:#018x}"
    );
}

// ── Test 5: Chained write() calls match equivalent write_u64() sequence ───────

/// Two sequential `write_u64()` calls must produce the same result as two
/// sequential `write(&x.to_ne_bytes())` calls for the same values.
#[test]
fn fxhasher_chained_write_matches_chained_write_u64() {
    let v1: u64 = 0xDEAD_BEEF_CAFE_BABE;
    let v2: u64 = 0x1234_5678_9ABC_DEF0;

    let mut h1 = FxHasher::default();
    h1.write(&v1.to_ne_bytes());
    h1.write(&v2.to_ne_bytes());
    let hash_via_bytes = h1.finish();

    let mut h2 = FxHasher::default();
    h2.write_u64(v1);
    h2.write_u64(v2);
    let hash_via_u64 = h2.finish();

    assert_eq!(
        hash_via_bytes,
        hash_via_u64,
        "BUG-RUST-6 (chained): write bytes={hash_via_bytes:#018x} != write_u64 sequence={hash_via_u64:#018x}"
    );
}
