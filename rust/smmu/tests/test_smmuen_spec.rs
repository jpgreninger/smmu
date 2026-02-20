#![allow(clippy::doc_markdown)]
#![allow(clippy::uninlined_format_args)]

//! FINDING-H-08: SMMU_CR0.SMMUEN global enable/disable
//!
//! Spec: §6.3.9 (SMMU_CR0), bit 0 SMMUEN
//!
//! Requirements:
//! - SMMU must start disabled after reset (SMMUEN=0)
//! - When SMMUEN=0, all transactions bypass the SMMU (identity: PA = IOVA, no fault)
//! - `enable()` / `disable()` methods must toggle the SMMUEN state atomically
//! - After `enable()`, translations proceed through stream lookup / page table walk
//! - After `disable()`, translations bypass regardless of stream configuration

use smmu::SMMU;
use smmu::types::{AccessType, IOVA, PA, PASID, PagePermissions, SecurityState, StreamConfig, StreamID};

// ── Helper ────────────────────────────────────────────────────────────────────

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

// ── Boot state ────────────────────────────────────────────────────────────────

/// §6.3.9: SMMU must start in disabled state (SMMUEN=0) after reset.
#[test]
fn test_smmuen_starts_disabled() {
    let smmu = make_smmu();
    assert!(!smmu.is_enabled(), "SMMU must start with SMMUEN=0 (disabled)");
}

// ── Bypass when disabled ──────────────────────────────────────────────────────

/// §6.3.9 SMMUEN=0: all transactions bypass the SMMU without fault.
/// When disabled, translate() must succeed and return IOVA as PA (identity mapping).
#[test]
fn test_smmuen_bypass_no_stream_configured() {
    let smmu = make_smmu();
    // No stream configured, SMMU disabled → bypass
    let result = smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "Disabled SMMU must bypass (no fault), got: {:?}",
        result
    );
    let data = result.unwrap();
    assert_eq!(
        data.physical_address().as_u64(),
        0x1000,
        "Bypass must return identity mapping PA == IOVA"
    );
}

/// SMMUEN=0: bypass even when a stage-1 stream IS configured with mappings.
/// The configured translation must be skipped when disabled.
#[test]
fn test_smmuen_bypass_overrides_configured_stream() {
    let smmu = make_smmu();
    let stream = sid(10);
    let p = pasid(0);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, p).unwrap();

    // Map IOVA 0x2000 → PA 0x9000
    smmu.map_page(
        stream,
        p,
        iova(0x2000),
        pa(0x9000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // SMMU still disabled → bypass (return IOVA=0x2000 as PA, no fault)
    let result = smmu.translate(stream, p, iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Disabled SMMU must bypass even with configured stream");
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x2000,
        "Bypass PA must equal IOVA"
    );
}

/// SMMUEN=0: no fault records generated during bypass.
#[test]
fn test_smmuen_bypass_no_fault_recorded() {
    let smmu = make_smmu();
    // Translate unmapped address while disabled — must not generate a fault
    let _ = smmu.translate(sid(99), pasid(0), iova(0xDEAD_0000), AccessType::Write, SecurityState::Secure);
    assert_eq!(
        smmu.get_faults().len(),
        0,
        "Disabled SMMU must not record faults during bypass"
    );
}

// ── Enable/disable toggle ─────────────────────────────────────────────────────

/// enable() must set SMMUEN=1.
#[test]
fn test_smmuen_enable() {
    let smmu = make_smmu();
    assert!(!smmu.is_enabled());
    smmu.enable().unwrap();
    assert!(smmu.is_enabled(), "is_enabled() must return true after enable()");
}

/// disable() must clear SMMUEN=0.
#[test]
fn test_smmuen_disable() {
    let smmu = make_smmu();
    smmu.enable().unwrap();
    assert!(smmu.is_enabled());
    smmu.disable().unwrap();
    assert!(!smmu.is_enabled(), "is_enabled() must return false after disable()");
}

/// Re-enabling after disable must work (toggle).
#[test]
fn test_smmuen_toggle_multiple_times() {
    let smmu = make_smmu();
    for _ in 0..3 {
        smmu.enable().unwrap();
        assert!(smmu.is_enabled());
        smmu.disable().unwrap();
        assert!(!smmu.is_enabled());
    }
}

// ── Translation after enable ──────────────────────────────────────────────────

/// After enable(), translations must go through the page table.
/// An unmapped address must produce a fault (not bypass).
#[test]
fn test_smmuen_enabled_translation_faults_on_unmapped() {
    let smmu = make_smmu();
    smmu.enable().unwrap();

    let stream = sid(5);
    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pasid(0)).unwrap();

    // IOVA not mapped → should fault
    let result =
        smmu.translate(stream, pasid(0), iova(0x5000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_err(), "Enabled SMMU must fault on unmapped page");
}

/// After enable(), a properly mapped page must translate correctly.
#[test]
fn test_smmuen_enabled_translation_succeeds_on_mapped() {
    let smmu = make_smmu();
    smmu.enable().unwrap();

    let stream = sid(7);
    let p = pasid(0);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream,
        p,
        iova(0x3000),
        pa(0xA000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(stream, p, iova(0x3000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Enabled SMMU must translate mapped page");
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0xA000,
        "Translated PA must match mapped PA"
    );
}

/// disable() after enable() must revert to bypass.
#[test]
fn test_smmuen_disable_after_enable_reverts_to_bypass() {
    let smmu = make_smmu();
    smmu.enable().unwrap();
    smmu.disable().unwrap();

    // Translate unconfigured stream → bypass (no fault)
    let result =
        smmu.translate(sid(42), pasid(0), iova(0xCAFE_0000), AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "Re-disabled SMMU must bypass again");
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0xCAFE_0000,
        "Bypass identity mapping must hold"
    );
}

// ── Shutdown interaction ──────────────────────────────────────────────────────

/// enable() after shutdown must fail.
#[test]
fn test_smmuen_enable_after_shutdown_fails() {
    let smmu = make_smmu();
    smmu.shutdown().unwrap();
    assert!(
        smmu.enable().is_err(),
        "enable() after shutdown must return an error"
    );
}

/// disable() after shutdown must fail.
#[test]
fn test_smmuen_disable_after_shutdown_fails() {
    let smmu = make_smmu();
    smmu.enable().unwrap();
    smmu.shutdown().unwrap();
    assert!(
        smmu.disable().is_err(),
        "disable() after shutdown must return an error"
    );
}
