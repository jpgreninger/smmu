//! ARM SMMU v3 §8.3 PRG Response Message codes — conformance tests
//!
//! Verifies the three valid `Resp` values (`0b00`=`ResponseFailure`,
//! `0b01`=`InvalidRequest`, `0b10`=`Success`) are all accepted by `CMD_PRI_RESP`,
//! and that the reserved encoding `0b11` raises `CERROR_ILL`.
//!
//! The 2-bit `Resp` field is carried in `CommandEntry.flags[1:0]` per §4.5.2.

use smmu::types::{AccessType, CommandEntry, CommandType, PRIEntry, SecurityState};
use smmu::SMMU;

fn make_smmu() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    smmu
}

fn submit_ppr(smmu: &SMMU, stream_id: u32, prg_index: u16) {
    let entry = PRIEntry {
        stream_id,
        pasid: 0,
        requested_address: 0x1000,
        access_type: AccessType::Read,
        is_last_request: true,
        timestamp: 0,
        prg_index,
        security_state: SecurityState::NonSecure,
        span: 0,
    };
    smmu.submit_page_request(entry).unwrap();
}

fn send_pri_resp(smmu: &SMMU, stream_id: u32, prg_index: u16, resp_bits: u32) {
    let mut cmd = CommandEntry::new(CommandType::PriResp, stream_id, 0);
    cmd.prg_index = prg_index;
    cmd.flags = resp_bits & 0x03; // Resp lives in bits[1:0]
    smmu.submit_command(cmd).unwrap();
    // process_command_queue() may return Err for CERROR_ILL or Ok; either way
    // the CMDQ_CONS.ERR register reflects the outcome (§4.1.3).
    let _ = smmu.process_command_queue();
}

// ── §8.3 / §4.5.2 valid Resp encodings ──────────────────────────────────────

/// §4.5.2 `Resp==0b00` (`ResponseFailure`) must be accepted without error.
#[test]
fn test_pri_resp_response_failure_accepted() {
    let smmu = make_smmu();
    submit_ppr(&smmu, 1, 3);
    send_pri_resp(&smmu, 1, 3, 0b00);
    assert_eq!(smmu.get_cmdq_cons_err(), 0, "Resp=0b00 must not raise CERROR_ILL");
}

/// §4.5.2 `Resp==0b01` (`InvalidRequest`) must be accepted without error.
#[test]
fn test_pri_resp_invalid_request_accepted() {
    let smmu = make_smmu();
    submit_ppr(&smmu, 1, 5);
    send_pri_resp(&smmu, 1, 5, 0b01);
    assert_eq!(smmu.get_cmdq_cons_err(), 0, "Resp=0b01 must not raise CERROR_ILL");
}

/// §4.5.2 `Resp==0b10` (Success) must be accepted without error.
#[test]
fn test_pri_resp_success_accepted() {
    let smmu = make_smmu();
    submit_ppr(&smmu, 1, 7);
    send_pri_resp(&smmu, 1, 7, 0b10);
    assert_eq!(smmu.get_cmdq_cons_err(), 0, "Resp=0b10 must not raise CERROR_ILL");
}

/// §4.5.2 `Resp==0b11` (Reserved) must raise `CERROR_ILL` and `GERROR.CMDQ_ERR`.
/// The SMMU signals the error via `CMDQ_CONS.ERR` — `process_command_queue()`
/// may still return `Ok` (command queue continues after `CERROR_ILL` is latched).
#[test]
fn test_pri_resp_reserved_raises_cerror_ill() {
    let smmu = make_smmu();
    submit_ppr(&smmu, 1, 9);
    smmu.clear_gerror(smmu.get_gerror() ^ smmu.get_gerrorn());
    send_pri_resp(&smmu, 1, 9, 0b11);
    assert_eq!(smmu.get_cmdq_cons_err(), SMMU::CERROR_ILL,
        "Resp=0b11 must write CERROR_ILL to CMDQ_CONS.ERR");
    let active_gerror = smmu.get_gerror() ^ smmu.get_gerrorn();
    assert_ne!(active_gerror & SMMU::GERROR_CMDQ_ERR, 0,
        "Resp=0b11 must activate GERROR.CMDQ_ERR");
}

/// §8.3: all three valid `Resp` values consume the matching `PRIEntry` (FIFO pop).
#[test]
fn test_pri_resp_all_valid_codes_consume_entry() {
    // Resp=0b00 (ResponseFailure) pops matching entry
    let smmu = make_smmu();
    submit_ppr(&smmu, 10, 1);
    assert_eq!(smmu.get_pri_queue_size(), 1);
    send_pri_resp(&smmu, 10, 1, 0b00);
    assert_eq!(smmu.get_pri_queue_size(), 0, "Resp=0b00 must pop matching entry");

    // Resp=0b01 (InvalidRequest) pops matching entry
    let smmu = make_smmu();
    submit_ppr(&smmu, 10, 2);
    send_pri_resp(&smmu, 10, 2, 0b01);
    assert_eq!(smmu.get_pri_queue_size(), 0, "Resp=0b01 must pop matching entry");

    // Resp=0b10 (Success) pops matching entry
    let smmu = make_smmu();
    submit_ppr(&smmu, 10, 3);
    send_pri_resp(&smmu, 10, 3, 0b10);
    assert_eq!(smmu.get_pri_queue_size(), 0, "Resp=0b10 must pop matching entry");
}
