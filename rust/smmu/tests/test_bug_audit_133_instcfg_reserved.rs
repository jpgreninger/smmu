#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

//! TDD tests for BUG-AUDIT-133: INSTCFG encoding corrected per ARM SMMU v3 §5.2
//!
//! ARM §5.2 STE.INSTCFG field encoding:
//!   0b00 (0) = Use incoming INST attribute (passthrough)
//!   0b01 (1) = Reserved — behaves as 0b00 (passthrough), NOT Force Instruction
//!   0b10 (2) = Force Data (Execute → Read)
//!   0b11 (3) = Force Instruction (Read/Write → Execute)
//!
//! These tests MUST FAIL before the fix is applied.

use smmu::address_space::AddressSpace;
use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, PagePermissions, SecurityState, StreamConfig, IOVA, PA, PASID};

fn make_pasid(v: u32) -> PASID {
    PASID::new(v).unwrap()
}

fn make_iova(v: u64) -> IOVA {
    IOVA::new(v).unwrap()
}

fn make_pa(v: u64) -> PA {
    PA::new(v).unwrap()
}

/// Build a stage-1-only `StreamContext` with a read+execute page mapped at IOVA 0x1000.
/// The page grants both Read and Execute so neither access type causes a permission fault.
fn make_ctx_with_rx_page(inst_cfg: u8) -> StreamContext {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .inst_cfg(inst_cfg)
        .build()
        .unwrap();

    let ctx = StreamContext::new();
    ctx.update_configuration(cfg);
    ctx.enable();

    let as1 = AddressSpace::new();
    // Map a page with read+execute so INSTCFG=3 (Force Instruction) turning Read → Execute
    // does not produce a permission fault.
    let perms = PagePermissions::read_execute();
    as1.map_page(
        make_iova(0x1000),
        make_pa(0x2000),
        perms,
        SecurityState::NonSecure,
    )
    .unwrap();
    ctx.add_pasid(make_pasid(0), std::sync::Arc::new(as1)).unwrap();
    ctx.set_stage1_enabled(true);
    ctx
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-133, test 1: INSTCFG=0b01 (Reserved) must behave as passthrough
// ---------------------------------------------------------------------------

/// INSTCFG=1 (0b01) is Reserved per ARM §5.2.
/// Reserved fields must NOT force the instruction attribute; they must pass through
/// the incoming access type unchanged (same as INSTCFG=0b00).
///
/// Bug (before fix): the match arm `1 =>` maps Read → Execute (Force Instruction).
/// After fix: value 1 falls through to the wildcard `_` arm → passthrough.
#[test]
fn instcfg_0b01_reserved_behaves_as_passthrough() {
    let ctx = make_ctx_with_rx_page(1); // INSTCFG = 0b01 (Reserved)
    let result = ctx.translate(
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    // Must succeed (page has read permission).
    assert!(
        result.is_ok(),
        "INSTCFG=1 (Reserved) — Read should translate successfully: {result:?}"
    );
    // The STE's inst_cfg output attribute must be recorded as-is (1).
    let data = result.unwrap();
    assert_eq!(
        data.inst_cfg(),
        1u8,
        "TranslationData.inst_cfg() must reflect the raw STE field value (1)"
    );
    // The effective access type must remain Read, not Execute.
    // We verify this indirectly: a Write access to the same IOVA should also work,
    // because the page has read+execute but NOT write. If INSTCFG=1 were still
    // Force-Instruction (Read → Execute), then a Read arriving as Read would
    // successfully check against the execute permission (since Execute is granted).
    // But a Write has no execute component — with Force-Instruction the write
    // would become Execute and succeed; without it, write fails (page is RX only).
    //
    // The key observable: with INSTCFG=1 as passthrough, Write must FAIL on an
    // RX-only page (because Write is not permitted), whereas with the buggy
    // Force-Instruction it would incorrectly turn Write → Execute and succeed.
    let write_result = ctx.translate(
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        write_result.is_err(),
        "INSTCFG=1 (Reserved/passthrough) — Write on RX-only page must fail, \
         but it succeeded (indicates INSTCFG=1 is still incorrectly acting as \
         Force-Instruction, turning Write→Execute): {write_result:?}"
    );
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-133, test 2: INSTCFG=0b11 (Force Instruction) must force Execute
// ---------------------------------------------------------------------------

/// INSTCFG=3 (0b11) is Force Instruction per ARM §5.2.
/// A Read access must be treated as Execute after the override is applied.
///
/// Bug (before fix): value 3 falls into the wildcard `_` arm → passthrough (Read stays Read).
/// After fix: the `3 =>` arm maps Read → Execute.
#[test]
fn instcfg_0b11_forces_instruction() {
    // Map a page with Execute-only permission to prove the access type became Execute.
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .inst_cfg(3) // INSTCFG = 0b11 (Force Instruction)
        .build()
        .unwrap();

    let ctx = StreamContext::new();
    ctx.update_configuration(cfg);
    ctx.enable();

    let as1 = AddressSpace::new();
    // Execute-only page: if the access stays as Read, the permission check fails.
    // If INSTCFG=3 correctly promotes Read → Execute, the check passes.
    let perms = PagePermissions::execute_only();
    as1.map_page(
        make_iova(0x1000),
        make_pa(0x2000),
        perms,
        SecurityState::NonSecure,
    )
    .unwrap();
    ctx.add_pasid(make_pasid(0), std::sync::Arc::new(as1)).unwrap();
    ctx.set_stage1_enabled(true);

    let result = ctx.translate(
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "INSTCFG=3 (Force Instruction) — Read should be promoted to Execute and \
         succeed on an execute-only page. Got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-133, test 3: INSTCFG=0b10 (Force Data) still forces data (regression)
// ---------------------------------------------------------------------------

/// INSTCFG=2 (0b10) must still force Execute → Read.
/// This is a regression guard: value 2 must not be disturbed by the fix.
#[test]
fn instcfg_0b10_forces_data() {
    // Map a read-only page.
    // Without the override an Execute access would fail (no X permission).
    // With INSTCFG=2 (Force Data) Execute → Read, so it should succeed.
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .inst_cfg(2) // INSTCFG = 0b10 (Force Data)
        .build()
        .unwrap();

    let ctx = StreamContext::new();
    ctx.update_configuration(cfg);
    ctx.enable();

    let as1 = AddressSpace::new();
    let perms = PagePermissions::read_only();
    as1.map_page(
        make_iova(0x1000),
        make_pa(0x2000),
        perms,
        SecurityState::NonSecure,
    )
    .unwrap();
    ctx.add_pasid(make_pasid(0), std::sync::Arc::new(as1)).unwrap();
    ctx.set_stage1_enabled(true);

    let result = ctx.translate(
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Execute,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "INSTCFG=2 (Force Data) — Execute should be demoted to Read and succeed \
         on a read-only page. Got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// BUG-AUDIT-133, test 4: INSTCFG=0b00 (Use Incoming) is unchanged (regression)
// ---------------------------------------------------------------------------

/// INSTCFG=0 (0b00) is passthrough — the incoming access type is used as-is.
#[test]
fn instcfg_0b00_passthrough() {
    let ctx = make_ctx_with_rx_page(0); // INSTCFG = 0b00 (Use Incoming)

    // Read on RX page → succeeds (Read is permitted).
    let result = ctx.translate(
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "INSTCFG=0 passthrough — Read on RX page must succeed: {result:?}"
    );

    // Write on RX page → fails (Write is not permitted; no override applied).
    let write_result = ctx.translate(
        make_pasid(0),
        make_iova(0x1000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        write_result.is_err(),
        "INSTCFG=0 passthrough — Write on RX-only page must fail: {write_result:?}"
    );
}
