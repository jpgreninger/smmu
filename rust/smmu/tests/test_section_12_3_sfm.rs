//! ARM SMMU v3 §12.3 Service Failure Mode (SFM) gating — conformance tests
//!
//! Spec requirements:
//! - "The SMMU **must** terminate client transactions after \[SFM\] mode is entered,
//!   and stop accessing its queues." (§12.3)
//! - "If an SMMU is in Service Failure Mode (SFM) it responds to PCIe requests as
//!   CA wherever possible." (§12.3)
//! - SFM is signaled by `GERROR.SFM_ERR` (bit 8) being active:
//!   `GERROR[8] != GERRORN[8]`.
//!
//! TDD: these tests were written BEFORE the SFM gate was implemented and must
//! initially FAIL.  After implementing `is_sfm_active()` + guards in `translate()`
//! and `submit_page_request()`, all tests must pass.

#![allow(clippy::doc_markdown)]

use smmu::types::SMMUError;
use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamConfig, TranslationError, IOVA, PASID,
    PRIEntry, StreamID,
};
use smmu::{SMMUConfig, SMMU};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_enabled_smmu() -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::builder().build().unwrap());
    smmu.enable().unwrap();
    smmu
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

/// Build a minimal enabled SMMU with a stage-1 stream + one page mapped at `0x1000`→`0x2000`.
fn make_smmu_with_stream() -> SMMU {
    let smmu = make_enabled_smmu();
    let stream = sid(1);
    let p = pasid(0);

    smmu.configure_stream(stream, StreamConfig::stage1_only()).unwrap();
    smmu.create_pasid(stream, p).unwrap();
    smmu.map_page(
        stream,
        p,
        IOVA::new(0x1000).unwrap(),
        smmu::types::PA::new(0x2000).unwrap(),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    smmu
}

const fn make_ppr(pasid_val: u32) -> PRIEntry {
    PRIEntry {
        stream_id: 1,
        pasid: pasid_val,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
        span: 0,
    }
}

// ── §12.3 Test A: SFM terminates translations ─────────────────────────────────

/// §12.3: Once `SFM_ERR` is active, `translate()` must return an error terminating
/// the transaction (`ServiceFailureMode`).
#[test]
fn test_sfm_terminates_translations() {
    let smmu = make_smmu_with_stream();

    // Sanity: translation works before SFM
    let ok =
        smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(ok.is_ok(), "translation must succeed before SFM: {ok:?}");

    // Activate SFM_ERR (XOR-toggle: GERROR[8] now != GERRORN[8])
    smmu.signal_gerror_for_test(SMMU::GERROR_SFM_ERR);

    // After SFM, translate() must fail with ServiceFailureMode
    let result =
        smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "translate() must be terminated when SFM is active, got: {result:?}"
    );
    assert_eq!(
        result.unwrap_err(),
        TranslationError::ServiceFailureMode,
        "error must be TranslationError::ServiceFailureMode"
    );
}

// ── §12.3 Test B: SFM terminates PRI queue writes ────────────────────────────

/// §12.3: "stop accessing its queues" — `submit_page_request()` must be rejected
/// when SFM is active.
#[test]
fn test_sfm_terminates_pri_queue() {
    let smmu = make_enabled_smmu();

    // Enable PRI queue (SMMUEN | PRIQEN)
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_PRIQEN);

    // Activate SFM_ERR
    smmu.signal_gerror_for_test(SMMU::GERROR_SFM_ERR);

    let result = smmu.submit_page_request(make_ppr(0));
    assert!(
        result.is_err(),
        "submit_page_request() must be rejected when SFM is active, got: {result:?}"
    );
    assert_eq!(
        result.unwrap_err(),
        SMMUError::ServiceFailureMode,
        "error must be SMMUError::ServiceFailureMode"
    );

    // No entries must reach the queue
    assert_eq!(smmu.get_pri_queue_size(), 0, "PRI queue must remain empty under SFM");
}

// ── §12.3 Test C: Normal operation without SFM ───────────────────────────────

/// §12.3 (negative): Without `SFM_ERR` active, translations proceed normally.
#[test]
fn test_sfm_not_active_without_signal() {
    let smmu = make_smmu_with_stream();

    // No GERROR bits set — SFM must be inactive
    let result =
        smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "translate() must succeed when SFM_ERR is not active, got: {result:?}"
    );

    // Result must NOT be ServiceFailureMode (belt-and-suspenders)
    assert_ne!(
        result.err(),
        Some(TranslationError::ServiceFailureMode),
        "ServiceFailureMode must not fire without SFM_ERR signal"
    );
}

// ── §12.3 Test D: SFM cleared restores translations ──────────────────────────

/// §12.3: After SFM is cleared (second XOR-toggle returns bits to equal),
/// translations must resume normally.
#[test]
fn test_sfm_cleared_restores_translations() {
    let smmu = make_smmu_with_stream();

    // Activate SFM
    smmu.signal_gerror_for_test(SMMU::GERROR_SFM_ERR);

    // Confirm SFM is gating
    let err_result =
        smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert_eq!(err_result, Err(TranslationError::ServiceFailureMode));

    // Clear SFM by toggling GERRORN back (acknowledge the error)
    smmu.clear_gerror(SMMU::GERROR_SFM_ERR);

    // Translation must succeed again
    let ok =
        smmu.translate(sid(1), pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(ok.is_ok(), "translation must resume after SFM is cleared, got: {ok:?}");
}
