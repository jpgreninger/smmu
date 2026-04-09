//! TDD tests for §4.1 Commands Overview — BUG-AUDIT-132.
//!
//! ## BUG-AUDIT-132: Reserved command opcode must generate `CERROR_ILL` (§4.1.1/§4.1.3)
//!
//! ARM spec §4.1.3: "A Reserved command opcode is encountered" → `CERROR_ILL`.
//! `SMMU::submit_raw_opcode()` must detect reserved opcodes and set `CERROR_ILL`.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

use smmu::SMMU;

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

/// §4.1.1/§4.1.3 — Reserved opcode 0x08 must generate `CERROR_ILL`.
///
/// BEFORE FIX: `submit_raw_opcode` doesn't exist → test FAILS (compile error).
/// AFTER FIX: `submit_raw_opcode(0x08)` returns `Err` and sets CMDQ error → PASSES.
#[test]
fn test_reserved_opcode_generates_cerror_ill() {
    let smmu = make_smmu();

    // 0x08 is Reserved per §4.1.1 (range 0x08-0x0A)
    let result = smmu.submit_raw_opcode(0x08u8);
    assert!(
        result.is_err(),
        "BUG-AUDIT-132: Reserved opcode 0x08 must return Err (`CERROR_ILL`), got Ok"
    );

    // The CERROR_ILL must also be flagged in CMDQ state
    let err_code = smmu.get_cmdq_cons_err();
    assert_eq!(
        err_code,
        1u32, // CERROR_ILL = 1
        "BUG-AUDIT-132: CMDQ_CONS.ERR must be set to `CERROR_ILL`=1 for reserved opcode"
    );
}

/// §4.1.1/§4.1.3 — Reserved opcode 0x00 must generate `CERROR_ILL`.
#[test]
fn test_opcode_zero_generates_cerror_ill() {
    let smmu = make_smmu();
    let result = smmu.submit_raw_opcode(0x00u8);
    assert!(result.is_err(), "BUG-AUDIT-132: Opcode 0x00 (Reserved) must return Err");
}

/// §4.1.1/§4.1.3 — Reserved opcode 0xFF must generate `CERROR_ILL`.
#[test]
fn test_opcode_0xff_generates_cerror_ill() {
    let smmu = make_smmu();
    let result = smmu.submit_raw_opcode(0xFFu8);
    assert!(result.is_err(), "BUG-AUDIT-132: Opcode 0xFF (Reserved) must return Err");
}

/// §4.1.1 regression — Valid opcode 0x46 (`CMD_SYNC`) must NOT generate `CERROR_ILL`.
#[test]
fn test_valid_opcode_sync_not_cerror_ill() {
    let smmu = make_smmu();
    // 0x46 = CMD_SYNC — a valid opcode
    let result = smmu.submit_raw_opcode(0x46u8);
    // Should succeed (Ok) because 0x46 is a known valid opcode
    assert!(
        result.is_ok(),
        "§4.1.1 regression: Valid opcode 0x46 (CMD_SYNC) must not be rejected, got {result:?}"
    );
}
