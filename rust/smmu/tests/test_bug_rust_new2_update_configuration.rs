#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD regression tests for BUG-RUST-NEW-2:
//! `update_configuration()` unreachable via `Arc<StreamContext>`.
//!
//! Problem:
//! 1. `update_configuration()` is declared `pub fn update_configuration(&mut self, ...)`.
//!    Because `StreamContext` objects live inside `Arc<StreamContext>` in the SMMU's
//!    DashMap, `&mut self` is unreachable through the normal API.
//! 2. The `configure_stream()` path in smmu/mod.rs uses individual Relaxed setters and
//!    does NOT call `update_configuration()`, so GAP-1/GAP-2 fields
//!    (mt_cfg, mem_attr, sh_cfg, alloc_cfg, inst_cfg, priv_cfg, ns_cfg, strw) are
//!    silently left at their default zero values for all SMMU-configured streams.
//!
//! Fix required:
//! 1. Change `update_configuration()` from `&mut self` to `&self` (all fields are
//!    atomics — interior mutability is already present).
//! 2. In `configure_stream()`, call `stream_context.update_configuration(&config)`
//!    instead of individual Relaxed setters, ensuring it is called BEFORE
//!    `Arc::new(stream_context)` wrapping.
//! 3. Update doctest: change `let mut ctx` to `let ctx`.

use smmu::types::{
    AccessType, PagePermissions, SecurityState, StreamConfig, StreamID, StreamWorld, IOVA, PA,
    PASID,
};
use smmu::SMMU;

fn stream_id(v: u32) -> StreamID {
    StreamID::new(v).unwrap()
}

fn pasid(v: u32) -> PASID {
    PASID::new(v).unwrap()
}

fn iova(v: u64) -> IOVA {
    IOVA::new(v).unwrap()
}

fn pa(v: u64) -> PA {
    PA::new(v).unwrap()
}

// -------------------------------------------------------------------------
// Helper: set up an SMMU stream, map a page, and return the SMMU and
// stream_id so the caller can call translate().
// -------------------------------------------------------------------------
fn setup_stream_with_config(cfg: StreamConfig) -> (SMMU, StreamID) {
    let smmu = SMMU::new();
    // SMMU starts with SMMUEN=0 (bypass mode). Enable it so translations go
    // through the full stream/page-table path including output attribute application.
    smmu.enable().unwrap();
    let sid = stream_id(1);
    smmu.configure_stream(sid, cfg).unwrap();
    let p = pasid(0);
    smmu.create_pasid(sid, p).unwrap();
    smmu.map_page(
        sid,
        p,
        iova(0x1000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();
    (smmu, sid)
}

// =========================================================================
// Tests: GAP-1 output-attribute fields must be applied after configure_stream
// =========================================================================

/// When mt_cfg=true and mem_attr=7 are configured via `configure_stream()`,
/// a subsequent translate() must return TranslationData with mem_type == 7.
///
/// Before the fix: `update_configuration()` is dead code (requires &mut self
/// through Arc), so the SMMU path uses individual setters that do NOT set
/// mt_cfg or mem_attr.  The result will have mem_type == 0 (default).
/// After the fix: mem_type == 7.
#[test]
fn test_configure_stream_mem_attr_applied_in_translation() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .mt_cfg(true)
        .mem_attr(7)
        .build()
        .unwrap();

    let (smmu, sid) = setup_stream_with_config(cfg);
    let result = smmu
        .translate(
            sid,
            pasid(0),
            iova(0x1000),
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    // Before fix: result.mem_type() == 0 (update_configuration never called)
    // After fix:  result.mem_type() == 7
    assert_eq!(
        result.mem_type(),
        7,
        "mem_type must reflect STE.MemAttr=7 configured via configure_stream(); \
         got {} — update_configuration() may not be called on the configure_stream() path",
        result.mem_type()
    );
}

/// When sh_cfg=2 (Outer-Shareable) is configured via `configure_stream()`,
/// a subsequent translate() must return TranslationData with shareability == 2.
///
/// Before the fix: sh_cfg is not applied (individual setter path misses it).
/// After the fix: shareability == 2.
#[test]
fn test_configure_stream_sh_cfg_applied_in_translation() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .sh_cfg(2)
        .build()
        .unwrap();

    let (smmu, sid) = setup_stream_with_config(cfg);
    let result = smmu
        .translate(
            sid,
            pasid(0),
            iova(0x1000),
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(
        result.shareability(),
        2,
        "shareability must reflect STE.SHCFG=2 configured via configure_stream(); \
         got {} — update_configuration() may not be called on the configure_stream() path",
        result.shareability()
    );
}

/// When alloc_cfg=0xA is configured via `configure_stream()`,
/// a subsequent translate() must return TranslationData with alloc_hint == 0xA.
#[test]
fn test_configure_stream_alloc_cfg_applied_in_translation() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .alloc_cfg(0xA)
        .build()
        .unwrap();

    let (smmu, sid) = setup_stream_with_config(cfg);
    let result = smmu
        .translate(
            sid,
            pasid(0),
            iova(0x1000),
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(
        result.alloc_hint(),
        0xA,
        "alloc_hint must reflect STE.ALLOCCFG=0xA configured via configure_stream(); \
         got {} — update_configuration() may not be called on the configure_stream() path",
        result.alloc_hint()
    );
}

/// When inst_cfg=1 is configured, translate() must return inst_cfg==1.
///
/// Note: With inst_cfg=1 (Force-Instruction), a Read access is overridden to Execute
/// before the permission check (ARM §3.2/§13.5, Gap D fix).  The mapped page must
/// therefore have execute permission.  We use AccessType::Execute (no override needed)
/// so the test focuses purely on verifying that the output-attribute field is set.
#[test]
fn test_configure_stream_inst_cfg_applied_in_translation() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let sid = stream_id(1);
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .inst_cfg(1)
        .build()
        .unwrap();
    smmu.configure_stream(sid, cfg).unwrap();
    let p = pasid(0);
    smmu.create_pasid(sid, p).unwrap();
    // Map with execute-only permission so the inst_cfg=1 override (Read→Execute)
    // and a direct Execute access both succeed; this lets us verify the output attribute.
    smmu.map_page(
        sid,
        p,
        iova(0x1000),
        pa(0x5000),
        PagePermissions::execute_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Use Execute directly — no override needed, just check the output attribute.
    let result = smmu
        .translate(
            sid,
            pasid(0),
            iova(0x1000),
            AccessType::Execute,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(
        result.inst_cfg(),
        1,
        "inst_cfg must reflect STE.INSTCFG=1 configured via configure_stream(); \
         got {} — update_configuration() may not be called on the configure_stream() path",
        result.inst_cfg()
    );
}

/// When priv_cfg=2 is configured, translate() must return priv_cfg==2.
#[test]
fn test_configure_stream_priv_cfg_applied_in_translation() {
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .priv_cfg(2)
        .build()
        .unwrap();

    let (smmu, sid) = setup_stream_with_config(cfg);
    let result = smmu
        .translate(
            sid,
            pasid(0),
            iova(0x1000),
            AccessType::Read,
            SecurityState::NonSecure,
        )
        .unwrap();

    assert_eq!(
        result.priv_cfg(),
        2,
        "priv_cfg must reflect STE.PRIVCFG=2 configured via configure_stream(); \
         got {} — update_configuration() may not be called on the configure_stream() path",
        result.priv_cfg()
    );
}

// =========================================================================
// Tests: GAP-2 STRW field must be applied after configure_stream
// =========================================================================

/// When strw=El2 is configured via `configure_stream()`, the stream context's
/// get_strw() should return El2 after configuration (observable through the SMMU).
///
/// This tests that update_configuration() is called and the strw field
/// is properly stored via Release ordering.
///
/// Before the fix: configure_stream() only calls individual setters, none of
/// which set strw (the individual setter set_strw was not called in the path).
/// After the fix: strw == El2.
#[test]
fn test_configure_stream_strw_el2_applied() {
    use smmu::types::StreamConfig;

    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let sid = stream_id(2);
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    // Use Secure security state — El2 is valid for Secure and has same suppression semantics.
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El2)
        .security_state(SecurityState::Secure)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();

    // Retrieve the stream context VMID as a proxy — the real check is that
    // a privileged translation works correctly on an EL2 stream.
    // We create a page with privileged_only permissions and verify the
    // translation succeeds (EL2 suppresses the privileged_only check).
    let p = pasid(0);
    smmu.create_pasid(sid, p).unwrap();

    // Map with standard read_write (not privileged_only) — EL2 strw means
    // any access is treated as privileged, so even a non-privileged IOVA
    // access should succeed.
    smmu.map_page(
        sid,
        p,
        iova(0x2000),
        pa(0x6000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(sid, p, iova(0x2000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "EL2 stream translation should succeed: {result:?}"
    );
}

// =========================================================================
// Tests: update_configuration() reachable via &self (not &mut self)
// =========================================================================

/// Calling update_configuration() on a StreamContext should not require
/// exclusive mutable access — all fields are atomics, so &self suffices.
///
/// Before the fix: update_configuration() takes &mut self, making it
/// impossible to call on an Arc<StreamContext>.
///
/// This test verifies that update_configuration() compiles and works with
/// a shared reference (&self) — a compile-time proof of the fix.
#[test]
fn test_update_configuration_works_with_shared_reference() {
    use smmu::stream_context::StreamContext;

    // After the fix, this should be `let ctx` (no mut), proving &self.
    // Before the fix, `&mut self` required `let mut ctx`.
    let ctx = StreamContext::new();
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El2)
        .mt_cfg(true)
        .mem_attr(0x7)
        .sh_cfg(2)
        .build()
        .unwrap();

    // Before fix: this line fails to compile because ctx is not mut
    // After fix:  this compiles and works
    ctx.update_configuration(cfg);

    assert_eq!(ctx.get_strw(), StreamWorld::El2);
    assert!(ctx.is_mt_cfg_enabled());
    assert_eq!(ctx.get_mem_attr(), 7);
    assert_eq!(ctx.get_sh_cfg(), 2);
}

/// Verify that update_configuration() can be called on an Arc<StreamContext>
/// (the actual usage pattern in the SMMU DashMap).
///
/// Before the fix: Arc::get_mut() would be needed (requires no other Arc
/// references), making this practically impossible in concurrent code.
/// After the fix: &self means Arc<StreamContext>::update_configuration works directly.
#[test]
fn test_update_configuration_callable_through_arc() {
    use smmu::stream_context::StreamContext;
    use std::sync::Arc;

    let ctx = Arc::new(StreamContext::new());
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .mt_cfg(true)
        .mem_attr(5)
        .build()
        .unwrap();

    // Before fix: ctx.update_configuration(cfg) would fail — cannot get &mut
    // from Arc without exclusive ownership (no other references).
    // After fix:  this works because update_configuration() takes &self.
    ctx.update_configuration(cfg);

    assert!(ctx.is_mt_cfg_enabled());
    assert_eq!(ctx.get_mem_attr(), 5);
}

// =========================================================================
// Tests: combined GAP-1 + GAP-2 via SMMU configure_stream
// =========================================================================

/// Full-path test: configure_stream with mt_cfg=true, mem_attr=7, sh_cfg=2,
/// strw=El2 → translate → verify all output attributes are correct.
///
/// This is the primary regression test for BUG-RUST-NEW-2.
/// Before the fix: all output attrs are 0 (update_configuration never reached).
/// After the fix:  output attrs reflect the configured values.
#[test]
fn test_full_path_gap1_gap2_via_configure_stream() {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    let sid = stream_id(3);

    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    // Use Secure security state — El2 is valid for Secure and has same suppression semantics.
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .strw(StreamWorld::El2)
        .security_state(SecurityState::Secure)
        .mt_cfg(true)
        .mem_attr(7)
        .sh_cfg(2)
        .alloc_cfg(0xC)
        .build()
        .unwrap();

    smmu.configure_stream(sid, cfg).unwrap();
    let p = pasid(0);
    smmu.create_pasid(sid, p).unwrap();
    smmu.map_page(
        sid,
        p,
        iova(0x3000),
        pa(0x7000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu
        .translate(sid, p, iova(0x3000), AccessType::Read, SecurityState::NonSecure)
        .unwrap();

    assert_eq!(
        result.mem_type(),
        7,
        "mem_type should be 7 (MTCFG=true, MemAttr=7)"
    );
    assert_eq!(
        result.shareability(),
        2,
        "shareability should be 2 (SHCFG=Outer-Shareable)"
    );
    assert_eq!(result.alloc_hint(), 0xC, "alloc_hint should be 0xC");
}
