//! TDD failing tests for three confirmed spec-compliance bugs.
//!
//! Tests are written to FAIL with the current buggy code and PASS only after
//! the corresponding fix is applied.
//!
//! # Bugs Covered
//!
//! ## BUG-RUST-1 (this file) — address/pasid fields RES0 for config events
//!
//! Four event types have no InputAddr field in the ARM §7.3 wire format.
//! The `address` and `pasid` struct fields must be 0 for:
//!   - C_BAD_STREAMID (0x02): §7.3.3 — no InputAddr
//!   - C_BAD_STE (0x04): §7.3.5 — no InputAddr
//!   - C_BAD_CD (0x0A): §7.3.11 — no InputAddr
//!   - F_STREAM_DISABLED (0x06): §7.3.7 — no InputAddr
//!
//! Note: C_BAD_SUBSTREAMID (0x08) DOES define InputAddr per §7.3.9 — regression
//! guard included to ensure it is NOT zeroed.
//!
//! ## BUG-RUST-2 — `process_command_queue()` lacks serialization mutex
//!
//! `cmdq_cons` load-then-store runs outside the command_queue lock.  Two
//! concurrent callers can read the same `cons`, each compute `N+1`, and both
//! store `N+1` — permanently undercounting consumed commands.
//!
//! ## BUG-RUST-3 — Bypass OAS F_ADDR_SIZE uses raw access type for rnw/ind
//!
//! ARM §7.3.14: InD/PnU are post-STE override values.  ARM §5.2.1: INSTCFG
//! applies to bypass (STE.Config=0b100) streams.  The bypass OAS fault path
//! passes `access` (raw) to `record_translation_fault` instead of the effective
//! access type computed by `ctx.effective_access_type(access)`.
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

use smmu::types::{
    AccessType, AddressConfig, CommandEntry, CommandType, EventType,
    QueueConfig, SMMUConfig, SecurityState, StreamConfig, StreamID, StreamWorld, IOVA, PASID,
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

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

// ============================================================================
// BUG-RUST-1a — C_BAD_STREAMID: address and pasid must be RES0 (0)
// ============================================================================
//
// Trigger: set strtab_log2size so that a high StreamID is out-of-range and
// triggers C_BAD_STREAMID.  Enable RECINVSID so the event is written to queue.
// Use a non-zero IOVA to make the bug visible.
//
// BEFORE FIX: address = iova, pasid = actual_pasid → assertions fail.
// AFTER FIX:  address = 0, pasid = 0 → assertions pass.

/// BUG-RUST-1a: C_BAD_STREAMID event must have address=0 and pasid=0 (RES0).
///
/// ARM §7.3.3: C_BAD_STREAMID has no InputAddr field; address and pasid are RES0.
///
/// BEFORE FIX: address=iova, pasid=actual_pasid → FAILS.
/// AFTER FIX:  address=0, pasid=0 → PASSES.
#[test]
fn rust1a_c_bad_streamid_address_and_pasid_are_res0() {
    // BUG-AUDIT-60 fix: CR2 is RO when SMMUEN=1 (ARM §6.3.12), so CR2 must be
    // configured before enable(). Construct SMMU manually instead of make_smmu().
    // BUG-AUDIT-63 fix: STRTAB_BASE (log2size) is RO when SMMUEN=1 (ARM §6.3.24/§6.3.25),
    // so log2size must also be configured before enable().
    let smmu = SMMU::new();
    // Enable RECINVSID so C_BAD_STREAMID is written to the event queue.
    // Must be set BEFORE enable() since CR2 is RO once SMMUEN=1.
    smmu.set_cr2(SMMU::CR2_RECINVSID);
    // Restrict table to 4 entries (log2size=2 → valid StreamIDs 0..3).
    // Must be set BEFORE enable() since STRTAB_BASE is RO once SMMUEN=1.
    smmu.set_strtab_log2size(2);
    smmu.enable().unwrap();

    // StreamID=8 is out-of-range (>= 2^2 = 4) → C_BAD_STREAMID.
    let out_of_range = sid(8);
    let nonzero_pasid = pasid(7);
    let nonzero_iova = iova(0xDEAD_0000);

    let result = smmu.translate(
        out_of_range,
        nonzero_pasid,
        nonzero_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "BUG-RUST-1a: out-of-range StreamID must fail");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == out_of_range.as_u32()
                && matches!(e.event_type, EventType::CBadStreamid)
        })
        .expect("BUG-RUST-1a: Expected a C_BAD_STREAMID event");

    assert_eq!(
        ev.address, 0,
        "BUG-RUST-1a: C_BAD_STREAMID must have address=0 (RES0 per §7.3.3). \
         Got address={:#x} — bug: address set from iova.",
        ev.address
    );
    assert_eq!(
        ev.pasid, 0,
        "BUG-RUST-1a: C_BAD_STREAMID must have pasid=0 (RES0 per §7.3.3). \
         Got pasid={} — bug: pasid set from transaction PASID.",
        ev.pasid
    );
}

// ============================================================================
// BUG-RUST-1b — C_BAD_STE: address and pasid must be RES0 (0)
// ============================================================================
//
// Trigger: configure a stream range but do NOT configure the specific StreamID,
// so the SMMU finds a valid-range StreamID with STE.V=0 → C_BAD_STE.
// Use non-zero IOVA and PASID to expose the bug.
//
// BEFORE FIX: address = iova, pasid = actual_pasid → assertions fail.
// AFTER FIX:  address = 0, pasid = 0 → assertions pass.

/// BUG-RUST-1b: C_BAD_STE event must have address=0 and pasid=0 (RES0).
///
/// ARM §7.3.5: C_BAD_STE has no InputAddr field; address and pasid are RES0.
///
/// BEFORE FIX: address=iova, pasid=actual_pasid → FAILS.
/// AFTER FIX:  address=0, pasid=0 → PASSES.
#[test]
fn rust1b_c_bad_ste_address_and_pasid_are_res0() {
    let smmu = make_smmu();

    // Use StreamID=0xBEEF — valid range (no log2size limit set), but not configured
    // in the stream table → STE.V=0 → C_BAD_STE.
    let unconfigured = sid(0xBEEF);
    let nonzero_pasid = pasid(5);
    let nonzero_iova = iova(0xCAFE_0000);

    let result = smmu.translate(
        unconfigured,
        nonzero_pasid,
        nonzero_iova,
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "BUG-RUST-1b: unconfigured StreamID must fail");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == unconfigured.as_u32()
                && matches!(e.event_type, EventType::CBadSte)
        })
        .expect("BUG-RUST-1b: Expected a C_BAD_STE event");

    assert_eq!(
        ev.address, 0,
        "BUG-RUST-1b: C_BAD_STE must have address=0 (RES0 per §7.3.5). \
         Got address={:#x} — bug: address set from iova.",
        ev.address
    );
    assert_eq!(
        ev.pasid, 0,
        "BUG-RUST-1b: C_BAD_STE must have pasid=0 (RES0 per §7.3.5). \
         Got pasid={} — bug: pasid set from transaction PASID.",
        ev.pasid
    );
}

// ============================================================================
// BUG-RUST-1c — C_BAD_CD: address and pasid must be RES0 (0)
// ============================================================================
//
// Trigger: configure aa64=false → C_BAD_CD (AArch32 LPAE unsupported §5.4).
// Use a non-zero IOVA to make the bug visible.
//
// BEFORE FIX: address = iova, pasid = actual_pasid → assertions fail.
// AFTER FIX:  address = 0, pasid = 0 → assertions pass.

/// BUG-RUST-1c: C_BAD_CD event must have address=0 and pasid=0 (RES0).
///
/// ARM §7.3.11: C_BAD_CD has no InputAddr field; address and pasid are RES0.
/// (SubstreamID/SSV ARE defined at bits [31:11] — distinct from InputAddr.)
///
/// BEFORE FIX: address=iova, pasid=actual_pasid → FAILS.
/// AFTER FIX:  address=0, pasid=0 → PASSES.
#[test]
fn rust1c_c_bad_cd_address_and_pasid_are_res0() {
    let smmu = make_smmu();
    let stream_id = sid(0xC1_0C);

    // aa64=false → C_BAD_CD (AArch32 LPAE is unsupported per ARM §5.4).
    let cfg = StreamConfig {
        aa64: false,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let nonzero_iova = iova(0xF00D_0000);

    let result = smmu.translate(
        stream_id,
        pasid(0),
        nonzero_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "BUG-RUST-1c: aa64=false must fail with C_BAD_CD");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::CBadCd)
        })
        .expect("BUG-RUST-1c: Expected a C_BAD_CD event");

    assert_eq!(
        ev.address, 0,
        "BUG-RUST-1c: C_BAD_CD must have address=0 (RES0 per §7.3.11). \
         Got address={:#x} — bug: address set from iova.",
        ev.address
    );
    assert_eq!(
        ev.pasid, 0,
        "BUG-RUST-1c: C_BAD_CD must have pasid=0 (RES0 per §7.3.11). \
         Got pasid={} — bug: pasid set from transaction PASID.",
        ev.pasid
    );
}

// ============================================================================
// BUG-RUST-1d — C_BAD_SUBSTREAMID: address must NOT be zeroed (regression guard)
// ============================================================================
//
// ARM §7.3.9 defines InputAddr for C_BAD_SUBSTREAMID — it must be the
// transaction IOVA, NOT zeroed.  This is a regression guard to ensure that
// the C_BAD_CD fix does NOT accidentally zero C_BAD_SUBSTREAMID address.
//
// This test must PASS both before and after the fix (it guards against
// over-zeroing).

/// Regression guard: C_BAD_SUBSTREAMID address must NOT be zeroed.
///
/// ARM §7.3.9 defines InputAddr for C_BAD_SUBSTREAMID.  Only C_BAD_CD and
/// C_BAD_STE have no InputAddr.
///
/// Must pass before and after fix: verifies we do not over-zero.
#[test]
fn rust1d_c_bad_substreamid_address_is_not_zeroed_regression_guard() {
    let smmu = make_smmu();
    let stream_id = sid(0xD1_0D);

    // s1cd_max=1 → valid PASID range is [0, 1] (2^1 = 2 entries). PASID=2 → C_BAD_SUBSTREAMID.
    let cfg = StreamConfig {
        s1cd_max: 1,
        strw: StreamWorld::El1El0,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_pasid(stream_id, pasid(1)).unwrap();

    let nonzero_iova = iova(0xABCD_0000);

    // PASID=2 is out of range for s1cd_max=1 (valid: [0,1]). Triggers C_BAD_SUBSTREAMID.
    let result = smmu.translate(
        stream_id,
        pasid(2),
        nonzero_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(result.is_err(), "regression-guard: PASID=2 with s1cd_max=1 must fail");

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::CBadSubstreamid)
        })
        .expect("regression-guard: Expected a C_BAD_SUBSTREAMID event for PASID=2 out-of-range");

    // §7.3.9 defines InputAddr — address must equal the IOVA, NOT 0.
    assert_eq!(
        ev.address,
        nonzero_iova.as_u64(),
        "regression-guard: C_BAD_SUBSTREAMID address must be the IOVA per §7.3.9. \
         Got address={:#x}, expected {:#x}.",
        ev.address,
        nonzero_iova.as_u64(),
    );
}

// ============================================================================
// BUG-RUST-1e — F_STREAM_DISABLED: address and pasid must be RES0 (0)
// ============================================================================
//
// Trigger: configure a stream with s1cd_max > 0 (substream-capable) and
// translate with PASID=0.  S1DSS=0x00 (default) → F_STREAM_DISABLED.
// Use non-zero IOVA to make the bug visible.
//
// BEFORE FIX: address = iova, pasid = actual_pasid → assertions fail.
// AFTER FIX:  address = 0, pasid = 0 → assertions pass.

/// BUG-RUST-1e: F_STREAM_DISABLED event must have address=0 and pasid=0 (RES0).
///
/// ARM §7.3.7: F_STREAM_DISABLED has no InputAddr field; address and pasid are RES0.
///
/// BEFORE FIX: address=iova, pasid=actual_pasid → FAILS.
/// AFTER FIX:  address=0, pasid=0 → PASSES.
#[test]
fn rust1e_f_stream_disabled_address_and_pasid_are_res0() {
    let smmu = make_smmu();
    let stream_id = sid(0xE1_0E);

    // s1cd_max=2 → substream-capable; s1_dss=0x00 → S1DSS=0b00 → F_STREAM_DISABLED
    // for non-substream (PASID=0) transactions.
    let cfg = StreamConfig {
        s1cd_max: 2,
        s1dss: 0x00,
        ..StreamConfig::stage1_only()
    };
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    let nonzero_iova = iova(0x1234_5000);

    // PASID=0 on a substream-capable stream with S1DSS=0x00 → F_STREAM_DISABLED.
    let result = smmu.translate(
        stream_id,
        pasid(0),
        nonzero_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-1e: S1DSS=0x00 non-substream must fail with StreamDisabled"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FStreamDisabled)
        })
        .expect("BUG-RUST-1e: Expected an F_STREAM_DISABLED event");

    assert_eq!(
        ev.address, 0,
        "BUG-RUST-1e: F_STREAM_DISABLED must have address=0 (RES0 per §7.3.7). \
         Got address={:#x} — bug: address set from iova.",
        ev.address
    );
    assert_eq!(
        ev.pasid, 0,
        "BUG-RUST-1e: F_STREAM_DISABLED must have pasid=0 (RES0 per §7.3.7). \
         Got pasid={} — bug: pasid set from transaction PASID.",
        ev.pasid
    );
}

// ============================================================================
// BUG-RUST-2 — process_command_queue() lacks serialization mutex
// ============================================================================
//
// Two concurrent callers read the same cons, each compute N+1, and both store
// N+1 — permanently undercounting consumed commands.
//
// The primary test is a single-threaded CONS correctness check (always
// deterministic).  The concurrency stress test runs many iterations to increase
// the probability of observing the race; after the fix, CONS is always correct.
//
// BEFORE FIX: concurrent CONS may undercount; CONS after N commands may be < N.
// AFTER FIX:  CONS advances exactly by N regardless of concurrency.

/// BUG-RUST-2 (primary): single-thread CONS advances by exactly N commands.
///
/// ARM §3.5.1: CONS.RD must accurately track consumed commands.
/// Verifies the basic per-command advance is correct (no concurrency needed).
#[test]
fn rust2a_process_command_queue_cons_advances_by_n_single_thread() {
    // Use a small command queue so advances are easy to observe.
    let smmu_cfg = SMMUConfig::builder()
        .queue_config(
            QueueConfig::builder()
                .command_queue_size(64)
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    let smmu = SMMU::with_config(smmu_cfg);
    smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    let initial_cons = smmu.cmdq_cons_index();
    assert_eq!(initial_cons, 0, "precondition: CONS must start at 0");

    // Submit 8 CMD_TLBI_NH_ALL commands (safe no-ops, always succeed).
    let num_commands: u32 = 8;
    for _ in 0..num_commands {
        smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();
    }

    let processed = smmu.process_command_queue().unwrap();
    assert_eq!(
        processed, num_commands as usize,
        "BUG-RUST-2: process_command_queue must report {num_commands} processed. \
         Got {processed}."
    );

    // For log2size=6 (64-entry queue), advance_index uses modulus=128.
    // 8 advances from 0 → 8.
    let final_cons = smmu.cmdq_cons_index();
    assert_eq!(
        final_cons, num_commands,
        "BUG-RUST-2: CONS must equal {num_commands} after {num_commands} commands. \
         Got CONS={final_cons}."
    );
}

/// BUG-RUST-2 (concurrency): total commands processed across two threads must
/// equal the number submitted, and CONS must reflect that.
///
/// ARM §3.5.1: CONS.RD must accurately track all consumed commands.  Without a
/// serialization mutex, two concurrent callers can double-advance to the same
/// index.  This test repeats many times to increase the probability of
/// observing the race before the fix.
///
/// BEFORE FIX: total_processed may be wrong (commands double-counted or missed).
/// AFTER FIX:  total_processed == num_commands for every iteration.
#[test]
fn rust2b_process_command_queue_cons_advances_correctly_under_concurrency() {
    use std::sync::Arc;

    // Run multiple iterations to increase race probability.
    for iteration in 0..20u32 {
        let smmu_cfg = SMMUConfig::builder()
            .queue_config(
                QueueConfig::builder()
                    .command_queue_size(64)
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let smmu = Arc::new(SMMU::with_config(smmu_cfg));
        smmu.set_cr0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

        // Submit 10 commands per iteration.
        let num_commands: u32 = 10;
        for _ in 0..num_commands {
            smmu.submit_command(CommandEntry::new(CommandType::TlbiNhAll, 0, 0)).unwrap();
        }

        // Launch two threads simultaneously; each tries to process the queue.
        let smmu1 = Arc::clone(&smmu);
        let smmu2 = Arc::clone(&smmu);

        let t1 = std::thread::spawn(move || smmu1.process_command_queue().unwrap());
        let t2 = std::thread::spawn(move || smmu2.process_command_queue().unwrap());

        let p1 = t1.join().unwrap();
        let p2 = t2.join().unwrap();
        let total = p1 + p2;

        assert_eq!(
            total, num_commands as usize,
            "BUG-RUST-2 iteration {iteration}: total processed must be {num_commands}. \
             Got {total} (p1={p1}, p2={p2}) — CONS advancement race detected."
        );
    }
}

// ============================================================================
// BUG-RUST-3 — Bypass OAS F_ADDR_SIZE uses raw access instead of effective
// ============================================================================
//
// ARM §7.3.14: InD/PnU are post-STE override values.
// ARM §5.2.1: INSTCFG applies to bypass (STE.Config=0b100) streams.
//
// Setup: bypass stream with INSTCFG=3 (0b11 = Force Instruction per ARM §5.2).
//   Raw access = Write → effective = Execute (ind=true, rnw=true).
//   OAS overflow → F_ADDR_SIZE event.
//
// BUG-AUDIT-133 correction: INSTCFG=1 (0b01) is Reserved (passthrough per ARM §5.2).
//   Force Instruction is INSTCFG=3 (0b11).
//
// BEFORE FIX: event.rnw=false, event.ind=false (raw Write) → FAILS.
// AFTER FIX:  event.rnw=true, event.ind=true (effective Execute) → PASSES.

/// BUG-RUST-3: F_ADDR_SIZE on bypass OAS must use effective access type.
///
/// ARM §7.3.14: InD/PnU are post-STE override values (INSTCFG applies).
/// ARM §5.2.1: INSTCFG applies to bypass (STE.Config=0b100) streams.
///
/// With INSTCFG=3 (0b11, Force Instruction), Write → Execute.
/// Execute: rnw=true (not-write), ind=true (instruction).
///
/// BEFORE FIX: rnw=false, ind=false (raw Write used) → FAILS.
/// AFTER FIX:  rnw=true, ind=true (effective Execute) → PASSES.
#[test]
fn rust3_bypass_oas_faddrsize_uses_effective_access_type() {
    // Build SMMU with 36-bit OAS so overflow is easy to trigger.
    let addr_cfg = AddressConfig::builder()
        .max_pa_bits(36_u8)
        .build()
        .unwrap();
    let smmu_cfg = SMMUConfig::builder()
        .address_config(addr_cfg)
        .build()
        .unwrap();
    let smmu = SMMU::with_config(smmu_cfg);
    smmu.enable().unwrap();

    // Configure a bypass stream (STE.Config=0b100: both stages disabled).
    // Set INSTCFG=3 (0b11, Force Instruction) so Write access is overridden to Execute.
    let bypass_cfg = StreamConfig {
        inst_cfg: 3, // INSTCFG=3 (0b11): Force Instruction per ARM §5.2
        ..StreamConfig::bypass()
    };
    let stream_id = sid(0x3B);
    smmu.configure_stream(stream_id, bypass_cfg).unwrap();

    // OAS = 36 bits → limit = 0x10_0000_0000.
    // IOVA above OAS triggers F_ADDR_SIZE on the bypass OAS check.
    let oas_limit: u64 = 1u64 << 36;
    let high_iova = iova(oas_limit + 0x1000); // above 36-bit limit

    // Submit a Write access (raw: rnw=false, ind=false).
    // With INSTCFG=3 (Force Instruction): effective = Execute (rnw=true, ind=true).
    let result = smmu.translate(
        stream_id,
        pasid(0),
        high_iova,
        AccessType::Write, // raw access
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-3: IOVA above OAS must fail with AddressSizeError"
    );

    let events = smmu.get_events();
    let ev = events
        .iter()
        .find(|e| {
            e.stream_id == stream_id.as_u32()
                && matches!(e.event_type, EventType::FAddrSize)
        })
        .expect("BUG-RUST-3: Expected an F_ADDR_SIZE event");

    // INSTCFG=3 (Force Instruction): Write → Execute. Execute: rnw=true (not-write), ind=true.
    assert!(
        ev.rnw,
        "BUG-RUST-3: F_ADDR_SIZE on bypass+INSTCFG=3 must have rnw=true \
         (effective Execute, not raw Write). Got rnw=false — bug: raw access used."
    );
    assert!(
        ev.ind,
        "BUG-RUST-3: F_ADDR_SIZE on bypass+INSTCFG=3 must have ind=true \
         (effective Execute). Got ind=false — bug: raw access used."
    );
}
