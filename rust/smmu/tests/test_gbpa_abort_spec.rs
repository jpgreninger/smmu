#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-NEW-01: GBPA.ABORT abort-on-disable path not modeled.
//!
//! Spec: ARM IHI0070G.b §3.11, §13.2 (SMMU_GBPA register)
//!
//! Requirements:
//! - When SMMUEN=0 and GBPA.ABORT=0 (default): transactions bypass with identity PA (PA==IOVA).
//! - When SMMUEN=0 and GBPA.ABORT=1: all transactions are aborted with
//!   `TranslationError::GbpaAbort` (no identity mapping, no fault event enqueued).
//! - When SMMUEN=1: GBPA.ABORT has no effect; translations proceed through stream lookup.
//! - `SMMU::is_gbpa_abort()` returns `false` by default.
//! - `SMMU::set_gbpa_abort(true/false)` toggles the GBPA.ABORT bit.

use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, TranslationError, IOVA, PA, PASID};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    SMMU::new()
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

// ── Default / accessor tests ──────────────────────────────────────────────────

/// §13.2 SMMU_GBPA reset value: ABORT=0 (bypass, not abort).
#[test]
fn test_gbpa_abort_default_is_false() {
    let smmu = make_smmu();
    assert!(
        !smmu.is_gbpa_abort(),
        "GBPA.ABORT must default to 0 (bypass, not abort) after reset"
    );
}

/// set_gbpa_abort(true) must set the GBPA.ABORT bit.
#[test]
fn test_gbpa_abort_set_true() {
    let smmu = make_smmu();
    smmu.set_gbpa_abort(true);
    assert!(smmu.is_gbpa_abort(), "GBPA.ABORT must read 1 after set_gbpa_abort(true)");
}

/// set_gbpa_abort(false) must clear the GBPA.ABORT bit.
#[test]
fn test_gbpa_abort_clear() {
    let smmu = make_smmu();
    smmu.set_gbpa_abort(true);
    smmu.set_gbpa_abort(false);
    assert!(!smmu.is_gbpa_abort(), "GBPA.ABORT must read 0 after set_gbpa_abort(false)");
}

// ── Abort when SMMUEN=0 and GBPA.ABORT=1 ────────────────────────────────────

/// §3.11: SMMUEN=0 + GBPA.ABORT=1 → all transactions abort with GbpaAbort error.
#[test]
fn test_gbpa_abort_disabled_smmu_abort_true_returns_err() {
    let smmu = make_smmu();
    // SMMUEN=0 (default) + GBPA.ABORT=1
    smmu.set_gbpa_abort(true);

    let result = smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "SMMUEN=0 + GBPA.ABORT=1 must abort the transaction"
    );
    assert_eq!(
        result.unwrap_err(),
        TranslationError::GbpaAbort,
        "Error must be TranslationError::GbpaAbort"
    );
}

/// SMMUEN=0 + GBPA.ABORT=1 → aborts even for configured streams with valid mappings.
#[test]
fn test_gbpa_abort_aborts_configured_stream() {
    let smmu = make_smmu();
    let stream = sid(10);
    let p = pasid(0);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(stream, p, iova(0x2000), pa(0x9000), PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    // Enable and disable to verify we can re-disable
    smmu.enable().unwrap();
    smmu.disable().unwrap();

    smmu.set_gbpa_abort(true);

    let result = smmu.translate(stream, p, iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert_eq!(
        result.unwrap_err(),
        TranslationError::GbpaAbort,
        "GBPA.ABORT=1 must abort even for configured+mapped streams"
    );
}

/// SMMUEN=0 + GBPA.ABORT=1 → write accesses also abort.
#[test]
fn test_gbpa_abort_aborts_write_access() {
    let smmu = make_smmu();
    smmu.set_gbpa_abort(true);

    let result = smmu.translate(sid(2), pasid(0), iova(0x5000), AccessType::Write, SecurityState::NonSecure);
    assert_eq!(
        result.unwrap_err(),
        TranslationError::GbpaAbort,
        "Write access must also abort when GBPA.ABORT=1"
    );
}

// ── Bypass when SMMUEN=0 and GBPA.ABORT=0 ───────────────────────────────────

/// §3.11: SMMUEN=0 + GBPA.ABORT=0 (default) → identity bypass (PA == IOVA).
#[test]
fn test_gbpa_abort_disabled_smmu_abort_false_returns_identity() {
    let smmu = make_smmu();
    // GBPA.ABORT=0 (default), SMMUEN=0 (default)

    let result = smmu.translate(sid(1), pasid(0), iova(0x3000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "GBPA.ABORT=0 must bypass (identity), got: {:?}", result);
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x3000,
        "Bypass must return PA == IOVA"
    );
}

// ── GBPA.ABORT has no effect when SMMUEN=1 ──────────────────────────────────

/// §3.11: When SMMUEN=1, GBPA.ABORT has no effect; unmapped page → PageNotMapped.
#[test]
fn test_gbpa_abort_enabled_smmu_abort_true_no_effect_on_unmapped() {
    let smmu = make_smmu();
    smmu.set_gbpa_abort(true);
    smmu.enable().unwrap();

    let stream = sid(5);
    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();

    // Unmapped IOVA → PageNotMapped (not GbpaAbort) because SMMUEN=1
    let result = smmu.translate(stream, pasid(0), iova(0x4000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Unmapped page must fault");
    assert_ne!(
        result.unwrap_err(),
        TranslationError::GbpaAbort,
        "SMMUEN=1 must not produce GbpaAbort regardless of GBPA.ABORT"
    );
}

/// §3.11: SMMUEN=1 + GBPA.ABORT=1 → mapped page translates normally.
#[test]
fn test_gbpa_abort_enabled_smmu_abort_true_mapped_translates_normally() {
    let smmu = make_smmu();
    smmu.set_gbpa_abort(true);
    smmu.enable().unwrap();

    let stream = sid(7);
    let p = pasid(0);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(stream, p, iova(0x6000), pa(0xB000), PagePermissions::read_write(), SecurityState::NonSecure).unwrap();

    let result = smmu.translate(stream, p, iova(0x6000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "SMMUEN=1 + GBPA.ABORT=1 must translate normally for mapped page");
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0xB000,
        "PA must match mapped value"
    );
}

// ── Toggle behavior ──────────────────────────────────────────────────────────

/// Toggling GBPA.ABORT while SMMUEN=0 must change translation outcome.
#[test]
fn test_gbpa_abort_toggle_changes_behavior() {
    let smmu = make_smmu();
    // SMMUEN=0

    // GBPA.ABORT=0 → bypass (Ok)
    let r1 = smmu.translate(sid(1), pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure);
    assert!(r1.is_ok(), "GBPA.ABORT=0: expect bypass Ok");

    smmu.set_gbpa_abort(true);

    // GBPA.ABORT=1 → abort (Err)
    let r2 = smmu.translate(sid(1), pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure);
    assert_eq!(r2.unwrap_err(), TranslationError::GbpaAbort, "GBPA.ABORT=1: expect GbpaAbort");

    smmu.set_gbpa_abort(false);

    // GBPA.ABORT=0 again → bypass (Ok)
    let r3 = smmu.translate(sid(1), pasid(0), iova(0x7000), AccessType::Read, SecurityState::NonSecure);
    assert!(r3.is_ok(), "GBPA.ABORT=0 again: expect bypass Ok");
}

// ── Fault queue interaction ──────────────────────────────────────────────────

/// GBPA.ABORT abort must not enqueue a fault event (it is a global disable, not a translation fault).
#[test]
fn test_gbpa_abort_no_fault_event_enqueued() {
    let smmu = make_smmu();
    smmu.set_gbpa_abort(true);

    let _ = smmu.translate(sid(1), pasid(0), iova(0x8000), AccessType::Read, SecurityState::NonSecure);

    assert_eq!(
        smmu.get_events().len(),
        0,
        "GBPA.ABORT abort must not enqueue a fault event"
    );
}
