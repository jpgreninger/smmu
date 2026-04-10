//! ARM SMMU v3 §8.2 Miscellaneous — conformance tests
//!
//! Covers:
//! - ``GERROR.PRIQ_ABT_ERR`` gate: blocks PRI queue writes, auto-response `0b1111`
//! - Secure stream PPR discard: blocked with auto-response `0b1111`

use smmu::types::{AccessType, PRIEntry, SecurityState};
use smmu::{SMMUConfig, SMMU};

fn make_smmu() -> SMMU {
    let smmu = SMMU::with_config(SMMUConfig::builder().build().unwrap());
    smmu.enable().unwrap();
    smmu
}

const fn last1_ppr(pasid: u32) -> PRIEntry {
    PRIEntry {
        stream_id: 1,
        pasid,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index: 0,
        security_state: SecurityState::NonSecure,
        span: 0,
    }
}

const fn last0_ppr() -> PRIEntry {
    PRIEntry { is_last_request: false, ..last1_ppr(0) }
}

// ── §8.2 Clause A: PRIQ_ABT_ERR gate ────────────────────────────────────────

/// When `GERROR.PRIQ_ABT_ERR` is active, all incoming PPRs must be rejected and
/// Last=1 PPRs must get an auto-failure response with ResponseCode=0b1111.
#[test]
fn test_priq_abt_err_blocks_queue_writes() {
    let smmu = make_smmu();

    // Activate PRIQ_ABT_ERR
    smmu.signal_gerror_for_test(SMMU::GERROR_PRIQ_ABT_ERR);

    // Last=1 PPR with no PASID — must be rejected and get auto-failure 0xF
    let result = smmu.submit_page_request(last1_ppr(0));
    assert!(result.is_err(), "PRIQ_ABT_ERR must block PRI queue write");

    let auto_resps = smmu.get_pri_auto_failure_responses();
    assert_eq!(auto_resps.len(), 1, "expected one auto-failure response");
    assert_eq!(auto_resps[0].response_code, 0xF,
        "PRIQ_ABT_ERR auto-response must be ResponseCode=0b1111");

    // PRI queue must remain empty
    assert_eq!(smmu.get_pri_queue_size(), 0, "no entries should reach the queue");
}

/// When `GERROR.PRIQ_ABT_ERR` is active, Last=0 PPRs are silently discarded
/// (no auto-failure response for partial PRG members).
#[test]
fn test_priq_abt_err_last0_no_auto_response() {
    let smmu = make_smmu();
    smmu.signal_gerror_for_test(SMMU::GERROR_PRIQ_ABT_ERR);

    let result = smmu.submit_page_request(last0_ppr());
    assert!(result.is_err(), "PRIQ_ABT_ERR must reject Last=0 PPR");

    let auto_resps = smmu.get_pri_auto_failure_responses();
    assert_eq!(auto_resps.len(), 0, "no auto-response for Last=0");
    assert_eq!(smmu.get_pri_queue_size(), 0);
}

/// §8.2: "No PASID prefix is used on responses auto-generated because of
/// `PRIQ_ABT_ERR`" — the `response_code` is unconditionally `0b1111` regardless
/// of whether the PPR had a PASID.
#[test]
fn test_priq_abt_err_with_pasid_still_failure() {
    let smmu = make_smmu();
    smmu.signal_gerror_for_test(SMMU::GERROR_PRIQ_ABT_ERR);

    // Last=1 PPR with PASID=42
    let result = smmu.submit_page_request(last1_ppr(42));
    assert!(result.is_err());

    let auto_resps = smmu.get_pri_auto_failure_responses();
    assert_eq!(auto_resps.len(), 1);
    assert_eq!(auto_resps[0].response_code, 0xF,
        "PRIQ_ABT_ERR response must be 0xF regardless of PASID");
}

// ── §8.2 Clause B: Secure stream PPR discard ────────────────────────────────

/// §8.2: "A PPR received from a Secure stream is discarded, is not recorded
/// into the PRI queue and results in an automatic Response Message having
/// `ResponseCode` == `0b1111`."
#[test]
fn test_secure_stream_ppr_discarded_with_failure() {
    let smmu = make_smmu();

    let ppr = PRIEntry {
        security_state: SecurityState::Secure,
        is_last_request: true,
        ..last1_ppr(0)
    };
    let result = smmu.submit_page_request(ppr);
    assert!(result.is_err(), "Secure PPR must be rejected");

    let auto_resps = smmu.get_pri_auto_failure_responses();
    assert_eq!(auto_resps.len(), 1, "expected one auto-failure for Secure Last=1 PPR");
    assert_eq!(auto_resps[0].response_code, 0xF,
        "Secure stream auto-response must be ResponseCode=0b1111");

    assert_eq!(smmu.get_pri_queue_size(), 0, "Secure PPR must not enter queue");
}

/// §8.2: Secure Last=0 PPRs are silently discarded — no auto-failure.
#[test]
fn test_secure_stream_ppr_last0_no_auto_response() {
    let smmu = make_smmu();

    let ppr = PRIEntry {
        security_state: SecurityState::Secure,
        is_last_request: false,
        ..last1_ppr(0)
    };
    let result = smmu.submit_page_request(ppr);
    assert!(result.is_err(), "Secure Last=0 PPR must be rejected");

    let auto_resps = smmu.get_pri_auto_failure_responses();
    assert_eq!(auto_resps.len(), 0, "no auto-response for Secure Last=0");
    assert_eq!(smmu.get_pri_queue_size(), 0);
}

/// §8.2: Secure PPR with PASID — still ResponseCode=0b1111, not Success.
#[test]
fn test_secure_stream_ppr_with_pasid_still_failure() {
    let smmu = make_smmu();

    let ppr = PRIEntry {
        security_state: SecurityState::Secure,
        is_last_request: true,
        pasid: 99,
        ..last1_ppr(99)
    };
    let result = smmu.submit_page_request(ppr);
    assert!(result.is_err());

    let auto_resps = smmu.get_pri_auto_failure_responses();
    assert_eq!(auto_resps.len(), 1);
    assert_eq!(auto_resps[0].response_code, 0xF);
}
