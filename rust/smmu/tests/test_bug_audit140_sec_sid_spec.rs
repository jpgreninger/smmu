//! TDD tests for §3.10 / §3.10.1 conformance (Security States).
//!
//! ARM SMMU v3 spec §3.10, line 2549:
//! "Non-secure streams can only generate transactions targeting Non-secure (NS==1) PA space."
//!
//! This test suite provides spec-anchored regression coverage for the enforcement path at
//! `address_space/mod.rs` which rejects cross-security-state translations.
//!
//! Also covers §3.10.1 (`SEC_SID` encoding) and `SECURE_IMPL=0` default behavior.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]

use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, TranslationError, IOVA,
    PA, PASID,
};
use smmu::SMMU;

// ============================================================================
// Helpers
// ============================================================================

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(n: u64) -> IOVA {
    IOVA::new(n).unwrap()
}

fn pa(n: u64) -> PA {
    PA::new(n).unwrap()
}

// ============================================================================
// §3.10 line 2549: NS stream → NS PA only
//
// A Non-secure stream MUST NOT be able to produce a translation result that
// targets a Secure PA. The SMMU must reject the translation.
// ============================================================================

/// §3.10 line 2549: Non-secure stream cannot access Secure-mapped page.
///
/// Maps a page as Secure, then issues a Non-secure translation request.
/// Must return `TranslationError::SecurityViolation`.
#[test]
fn test_ns_stream_cannot_access_secure_pa() {
    let smmu = make_smmu();
    let stream = sid(1);
    let pid = pasid(0);
    let addr = iova(0x1000);
    let phys = pa(0x8000);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pid).unwrap();

    // Map the page as Secure
    smmu.map_page(stream, pid, addr, phys, PagePermissions::read_write(), SecurityState::Secure)
        .unwrap();

    // Non-secure translate request must be rejected (§3.10, line 2549)
    let result = smmu.translate(stream, pid, addr, AccessType::Read, SecurityState::NonSecure);
    assert_eq!(
        result,
        Err(TranslationError::SecurityViolation),
        "§3.10 line 2549: NS stream must not access Secure PA — expected SecurityViolation"
    );
}

/// §3.10 line 2549: Non-secure stream succeeds on Non-secure page.
///
/// Verifies the happy path is unaffected: NS translate → NS-mapped page → Ok.
#[test]
fn test_ns_stream_can_access_ns_pa() {
    let smmu = make_smmu();
    let stream = sid(2);
    let pid = pasid(0);
    let addr = iova(0x2000);
    let phys = pa(0x9000);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pid).unwrap();

    smmu.map_page(stream, pid, addr, phys, PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();

    let result = smmu.translate(stream, pid, addr, AccessType::Read, SecurityState::NonSecure);
    assert!(result.is_ok(), "§3.10: NS stream must succeed on NS-mapped page, got {result:?}");
}

/// §3.10 line 2549: Multiple IOVAs — only NS-mapped ones reachable from NS stream.
#[test]
fn test_ns_stream_mixed_mapping_isolation() {
    let smmu = make_smmu();
    let stream = sid(3);
    let pid = pasid(0);
    let ns_iova = iova(0x3000);
    let sec_iova = iova(0x4000);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, pid).unwrap();

    smmu.map_page(stream, pid, ns_iova, pa(0xA000), PagePermissions::read_write(), SecurityState::NonSecure)
        .unwrap();
    smmu.map_page(stream, pid, sec_iova, pa(0xB000), PagePermissions::read_write(), SecurityState::Secure)
        .unwrap();

    // NS stream can access NS page
    assert!(
        smmu.translate(stream, pid, ns_iova, AccessType::Read, SecurityState::NonSecure).is_ok()
    );
    // NS stream cannot access Secure page (§3.10, line 2549)
    assert_eq!(
        smmu.translate(stream, pid, sec_iova, AccessType::Read, SecurityState::NonSecure),
        Err(TranslationError::SecurityViolation)
    );
}

// ============================================================================
// §3.10.1 SEC_SID: SECURE_IMPL=0 default — all streams are Non-secure
//
// The implementation has no Secure interface (SECURE_IMPL==0 by construction).
// All stream configs must default to NonSecure security state.
// ============================================================================

/// §3.10.1, line 2494: `StreamConfig` defaults to NonSecure (`SECURE_IMPL==0` behavior).
#[test]
fn test_stream_config_defaults_to_non_secure() {
    let config = StreamConfig::default();
    assert_eq!(
        config.security_state,
        SecurityState::NonSecure,
        "§3.10.1: SECURE_IMPL==0 — all streams must default to NonSecure"
    );
}

/// §3.10.1, line 2494: `stage1_only()` config defaults to NonSecure.
#[test]
fn test_stage1_only_config_defaults_to_non_secure() {
    let config = StreamConfig::stage1_only();
    assert_eq!(
        config.security_state,
        SecurityState::NonSecure,
        "§3.10.1: stage1_only() stream must default to NonSecure"
    );
}

// ============================================================================
// §3.10.1 SEC_SID encoding (2-bit for RME DA per line 2511)
// ============================================================================

/// §3.10.1, line 2511: `SEC_SID=0b00` decodes to NonSecure.
#[test]
fn test_sec_sid_0b00_is_non_secure() {
    assert_eq!(
        SecurityState::from_bits(0b00).unwrap(),
        SecurityState::NonSecure,
        "§3.10.1 SEC_SID 0b00 must decode to NonSecure"
    );
}

/// §3.10.1, line 2511: `SEC_SID=0b01` decodes to Secure.
#[test]
fn test_sec_sid_0b01_is_secure() {
    assert_eq!(
        SecurityState::from_bits(0b01).unwrap(),
        SecurityState::Secure,
        "§3.10.1 SEC_SID 0b01 must decode to Secure"
    );
}

/// §3.10.1, line 2511: `SEC_SID=0b10` decodes to Realm.
#[test]
fn test_sec_sid_0b10_is_realm() {
    assert_eq!(
        SecurityState::from_bits(0b10).unwrap(),
        SecurityState::Realm,
        "§3.10.1 SEC_SID 0b10 must decode to Realm"
    );
}

/// §3.10.1, line 2511: NonSecure must encode as `0b00`.
#[test]
fn test_non_secure_encodes_as_0b00() {
    assert_eq!(
        SecurityState::NonSecure.to_bits(),
        0b00,
        "§3.10.1 NonSecure must encode as SEC_SID=0b00"
    );
}

/// §3.10.1, line 2511: Realm must encode as `0b10`.
#[test]
fn test_realm_encodes_as_0b10() {
    assert_eq!(
        SecurityState::Realm.to_bits(),
        0b10,
        "§3.10.1 Realm must encode as SEC_SID=0b10"
    );
}
