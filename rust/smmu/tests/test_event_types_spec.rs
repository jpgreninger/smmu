#![allow(missing_docs)]
//! Spec-compliance tests for ARM SMMU v3 §7.3 `EventType` codes.
//!
//! Each test asserts that the exact event number from the ARM IHI0070G.b
//! specification is encoded as the `#[repr(u8)]` discriminant of the
//! corresponding `EventType` variant.
//!
//! FINDING-H-01 — these tests must all be RED before the fix.

use smmu::types::EventType;

// ── §7.3.2  F_UUT ──────────────────────────────────────────────────────────
#[test]
fn test_f_uut_event_code() {
    assert_eq!(EventType::FUut as u8, 0x01, "F_UUT must be event number 0x01 per §7.3.2");
}

// ── §7.3.3  C_BAD_STREAMID ─────────────────────────────────────────────────
#[test]
fn test_c_bad_streamid_event_code() {
    assert_eq!(EventType::CBadStreamid as u8, 0x02, "C_BAD_STREAMID must be 0x02 per §7.3.3");
}

// ── §7.3.4  F_STE_FETCH ────────────────────────────────────────────────────
#[test]
fn test_f_ste_fetch_event_code() {
    assert_eq!(EventType::FSteFetch as u8, 0x03, "F_STE_FETCH must be 0x03 per §7.3.4");
}

// ── §7.3.5  C_BAD_STE ──────────────────────────────────────────────────────
#[test]
fn test_c_bad_ste_event_code() {
    assert_eq!(EventType::CBadSte as u8, 0x04, "C_BAD_STE must be 0x04 per §7.3.5");
}

// ── §7.3.6  F_BAD_ATS_TREQ ─────────────────────────────────────────────────
#[test]
fn test_f_bad_ats_treq_event_code() {
    assert_eq!(EventType::FBadAtsTreq as u8, 0x05, "F_BAD_ATS_TREQ must be 0x05 per §7.3.6");
}

// ── §7.3.7  F_STREAM_DISABLED ──────────────────────────────────────────────
#[test]
fn test_f_stream_disabled_event_code() {
    assert_eq!(EventType::FStreamDisabled as u8, 0x06, "F_STREAM_DISABLED must be 0x06 per §7.3.7");
}

// ── §7.3.8  F_TRANSL_FORBIDDEN ─────────────────────────────────────────────
#[test]
fn test_f_transl_forbidden_event_code() {
    assert_eq!(EventType::FTranslForbidden as u8, 0x07, "F_TRANSL_FORBIDDEN must be 0x07 per §7.3.8");
}

// ── §7.3.9  C_BAD_SUBSTREAMID ──────────────────────────────────────────────
#[test]
fn test_c_bad_substreamid_event_code() {
    assert_eq!(EventType::CBadSubstreamid as u8, 0x08, "C_BAD_SUBSTREAMID must be 0x08 per §7.3.9");
}

// ── §7.3.10 F_CD_FETCH ─────────────────────────────────────────────────────
#[test]
fn test_f_cd_fetch_event_code() {
    assert_eq!(EventType::FCdFetch as u8, 0x09, "F_CD_FETCH must be 0x09 per §7.3.10");
}

// ── §7.3.11 C_BAD_CD ───────────────────────────────────────────────────────
#[test]
fn test_c_bad_cd_event_code() {
    assert_eq!(EventType::CBadCd as u8, 0x0A, "C_BAD_CD must be 0x0A per §7.3.11");
}

// ── §7.3.12 F_WALK_EABT ────────────────────────────────────────────────────
#[test]
fn test_f_walk_eabt_event_code() {
    assert_eq!(EventType::FWalkEabt as u8, 0x0B, "F_WALK_EABT must be 0x0B per §7.3.12");
}

// ── §7.3.13 F_TRANSLATION ──────────────────────────────────────────────────
#[test]
fn test_f_translation_event_code() {
    assert_eq!(EventType::FTranslation as u8, 0x10, "F_TRANSLATION must be 0x10 per §7.3.13");
}

// ── §7.3.14 F_ADDR_SIZE ────────────────────────────────────────────────────
#[test]
fn test_f_addr_size_event_code() {
    assert_eq!(EventType::FAddrSize as u8, 0x11, "F_ADDR_SIZE must be 0x11 per §7.3.14");
}

// ── §7.3.15 F_ACCESS ───────────────────────────────────────────────────────
#[test]
fn test_f_access_event_code() {
    assert_eq!(EventType::FAccess as u8, 0x12, "F_ACCESS must be 0x12 per §7.3.15");
}

// ── §7.3.16 F_PERMISSION ───────────────────────────────────────────────────
#[test]
fn test_f_permission_event_code() {
    assert_eq!(EventType::FPermission as u8, 0x13, "F_PERMISSION must be 0x13 per §7.3.16");
}

// ── §7.3.17 F_TLB_CONFLICT ─────────────────────────────────────────────────
#[test]
fn test_f_tlb_conflict_event_code() {
    assert_eq!(EventType::FTlbConflict as u8, 0x20, "F_TLB_CONFLICT must be 0x20 per §7.3.17");
}

// ── §7.3.18 F_CFG_CONFLICT ─────────────────────────────────────────────────
#[test]
fn test_f_cfg_conflict_event_code() {
    assert_eq!(EventType::FCfgConflict as u8, 0x21, "F_CFG_CONFLICT must be 0x21 per §7.3.18");
}

// ── §7.3.19 E_PAGE_REQUEST ─────────────────────────────────────────────────
#[test]
fn test_e_page_request_event_code() {
    assert_eq!(EventType::EPageRequest as u8, 0x24, "E_PAGE_REQUEST must be 0x24 per §7.3.19");
}

// ── §7.3.20 F_VMS_FETCH ────────────────────────────────────────────────────
#[test]
fn test_f_vms_fetch_event_code() {
    assert_eq!(EventType::FVmsFetch as u8, 0x25, "F_VMS_FETCH must be 0x25 per §7.3.20");
}

// ── Completeness: all 19 spec variants present ─────────────────────────────
#[test]
fn test_spec_all_event_variants_present() {
    // Ensure every ARM IHI0070G.b §7.3 event type is represented.
    let spec_variants: &[(EventType, u8, &str)] = &[
        (EventType::FUut,             0x01, "F_UUT"),
        (EventType::CBadStreamid,     0x02, "C_BAD_STREAMID"),
        (EventType::FSteFetch,        0x03, "F_STE_FETCH"),
        (EventType::CBadSte,          0x04, "C_BAD_STE"),
        (EventType::FBadAtsTreq,      0x05, "F_BAD_ATS_TREQ"),
        (EventType::FStreamDisabled,  0x06, "F_STREAM_DISABLED"),
        (EventType::FTranslForbidden, 0x07, "F_TRANSL_FORBIDDEN"),
        (EventType::CBadSubstreamid,  0x08, "C_BAD_SUBSTREAMID"),
        (EventType::FCdFetch,         0x09, "F_CD_FETCH"),
        (EventType::CBadCd,           0x0A, "C_BAD_CD"),
        (EventType::FWalkEabt,        0x0B, "F_WALK_EABT"),
        (EventType::FTranslation,     0x10, "F_TRANSLATION"),
        (EventType::FAddrSize,        0x11, "F_ADDR_SIZE"),
        (EventType::FAccess,          0x12, "F_ACCESS"),
        (EventType::FPermission,      0x13, "F_PERMISSION"),
        (EventType::FTlbConflict,     0x20, "F_TLB_CONFLICT"),
        (EventType::FCfgConflict,     0x21, "F_CFG_CONFLICT"),
        (EventType::EPageRequest,     0x24, "E_PAGE_REQUEST"),
        (EventType::FVmsFetch,        0x25, "F_VMS_FETCH"),
    ];
    for (variant, expected_code, name) in spec_variants {
        assert_eq!(
            *variant as u8, *expected_code,
            "{name} has wrong event number: got 0x{:02X}, want 0x{:02X}",
            *variant as u8, expected_code
        );
    }
}
