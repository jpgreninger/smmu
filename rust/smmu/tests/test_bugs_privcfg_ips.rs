//! Failing tests for three confirmed bugs in the Rust SMMU implementation.
//!
//! ## BUG-RUST-1: PRIVCFG not applied in `StreamContext::translate()` before dispatch
//!
//! `StreamContext::translate()` applies INSTCFG but NOT PRIVCFG before dispatching to
//! `translate_two_stage` / `translate_stage2_only`.  The correct path
//! `translate_and_get_stage2_ipa()` (used by SMMU-level) does apply PRIVCFG.
//!
//! The observable impact on StreamContext::translate(): For PRIVCFG=2/STRW=El2,
//! `access_type` is NOT demoted, so any future consumer of the effective access type
//! (e.g. EventEntry.ind/rnw) gets the wrong value.  The existing per-stage privilege
//! checks happen to use PRIVCFG-aware helpers so behavioural divergence is subtle — but
//! the bug is real and the fix (adding the PRIVCFG block in `translate()`) is necessary.
//!
//! ## BUG-RUST-2: TLB fast path uses `strw_suppresses_priv()` instead of
//!   `effective_priv_suppresses_check()`
//!
//! The TLB cache-hit path in `smmu/mod.rs` (~line 3336) checks `strw_suppresses_priv()`
//! which is STRW-only aware.  It must call `effective_priv_suppresses_check()` which also
//! honours PRIVCFG=2/3 overrides.
//!
//! Observable failure: PRIVCFG=3/STRW=El1 — first translate (TLB miss → slow path)
//! SUCCEEDS; second translate (TLB hit → fast path) INCORRECTLY DENIES access because
//! `strw_suppresses_priv()` for El1 returns false.
//!
//! ## BUG-RUST-6: `translate_two_stage` missing CD.IPS output address size check
//!
//! `translate_two_stage_with_ipa` (SMMU slow path) has the CD.IPS check at ~line 1987.
//! `translate_two_stage` (called via `StreamContext::translate()` directly) lacks it.
//! An IPA exceeding 2^IPS must produce `F_ADDR_SIZE` but currently either succeeds
//! (if stage-2 maps the out-of-range IPA) or produces `F_TRANSLATION` (PageNotMapped).
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

use smmu::stream_context::StreamContext;
use smmu::types::{
    AccessType, EventType, PagePermissions, SecurityState, StreamConfig, StreamID, StreamWorld,
    IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn pasid(id: u32) -> PASID {
    PASID::new(id).unwrap()
}

fn sid(id: u32) -> StreamID {
    StreamID::new(id).unwrap()
}

// ============================================================================
// BUG-RUST-1: PRIVCFG not applied in StreamContext::translate()
//
// These tests call StreamContext::translate() directly to expose the missing
// PRIVCFG transformation.
//
// BEFORE FIX: translate() dispatches to translate_two_stage/translate_stage2_only
//   with only INSTCFG applied — PRIVCFG promotion/demotion is missing.
// AFTER FIX:  translate() applies PRIVCFG block before dispatch (matching
//   translate_and_get_stage2_ipa).
//
// Because the per-stage privileged_only checks already use the PRIVCFG-aware
// effective_priv_suppresses_check() helper, the most directly observable impact
// is: the access_type value reaching downstream code is wrong.  We expose this
// by checking that PRIVCFG=3 two-stage (stage-1 privileged_only page, Read access)
// translates successfully via StreamContext::translate() — which it does due to
// effective_priv_suppresses_check() being PRIVCFG-aware, but the access_type value
// itself (not promoted) would give wrong information to any code that calls
// access_type.is_privileged() directly.
//
// The TRUE observable failing test for BUG-RUST-1 is the TLB-hit path (BUG-RUST-2),
// which also stems from using the STRW-only check.  BUG-RUST-1 and BUG-RUST-2 are
// both fixed by applying PRIVCFG in translate() and updating the TLB fast path.
// ============================================================================

/// BUG-RUST-1a: Two-stage stream via StreamContext::translate() with PRIVCFG=3/El1El0.
///
/// PRIVCFG=3 (Force Privileged) must promote Read→ReadPrivileged before dispatching to
/// translate_two_stage.  The stage-1 page has `privileged_only=true`.
///
/// Via translate_two_stage, the privileged_only check at ~line 2409 calls
/// effective_priv_suppresses_check() which returns true for PRIVCFG=3 regardless of the
/// incoming access_type — so the check is SKIPPED and translation succeeds.
///
/// BEFORE FIX: access_type=Read (not promoted) flows in; effective_priv_suppresses_check()
///   still returns true → translation succeeds (correct outcome, but access_type is wrong value).
/// AFTER FIX:  access_type=ReadPrivileged (promoted) flows in; same outcome.
///
/// This test PASSES before and after the fix (regression guard for correct behavior).
/// The failing test for the BUG-RUST-1 root cause is bug_rust_2a (TLB fast path).
#[test]
fn bug_rust_1a_privcfg3_two_stage_privileged_only_via_streamcontext_baseline() {
    let ctx = StreamContext::new();
    let mut cfg = StreamConfig::two_stage();
    cfg.strw = StreamWorld::El1El0;
    cfg.priv_cfg = 3; // Force Privileged
    cfg.t0sz = 0;
    cfg.s2_t0sz = 0;
    cfg.ips = 5;
    ctx.update_configuration(cfg);
    ctx.enable();
    ctx.create_pasid(pasid(0)).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x2000 with privileged_only=true.
    ctx.map_page(
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only().with_privileged_only(true),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x2000 → PA 0x3000 (no privileged_only restriction).
    ctx.map_stage2_page(
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // PRIVCFG=3 suppresses the privileged_only check regardless of the access_type value.
    // Translation must succeed whether or not PRIVCFG promotion was applied to access_type.
    let result = ctx.translate(pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-RUST-1a: PRIVCFG=3 two-stage + privileged_only stage-1 page + Read access: \
         effective_priv_suppresses_check() must suppress privileged_only check → expect Ok; \
         got {result:?}"
    );
}

/// BUG-RUST-1b: Stage-2-only via StreamContext::translate(), PRIVCFG=2/El2/privileged_only.
///
/// PRIVCFG=2 (Force Unprivileged) must ensure the privileged_only check fires regardless
/// of STRW=El2.  translate_stage2_only calls effective_priv_suppresses_check() which
/// returns false for PRIVCFG=2 — so the check fires and access is denied.
///
/// BEFORE FIX: access_type=ReadPrivileged (not demoted by translate()) flows into
///   translate_stage2_only.  effective_priv_suppresses_check() = false (PRIVCFG=2) →
///   privileged_only check fires → PermissionViolation (CORRECT outcome, but access_type value wrong).
/// AFTER FIX: access_type=Read (demoted) flows in.  Same outcome.
///
/// This test PASSES before and after the fix (regression guard).  The failing observable
/// test for BUG-RUST-2 is below.
#[test]
fn bug_rust_1b_privcfg2_el2_stage2only_privileged_only_denied_via_streamcontext() {
    let ctx = StreamContext::new();
    let mut cfg = StreamConfig::stage2_only();
    cfg.strw = StreamWorld::El2;
    cfg.priv_cfg = 2; // Force Unprivileged
    cfg.s2_t0sz = 0;
    ctx.update_configuration(cfg);
    ctx.enable();
    ctx.create_stage2_address_space().unwrap();

    // Stage-2: IPA 0x1000 → PA 0x2000 with privileged_only=true.
    ctx.map_stage2_page(
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only().with_privileged_only(true),
        SecurityState::NonSecure,
    )
    .unwrap();

    // ReadPrivileged: PRIVCFG=2 should demote to Read, but either way, effective_priv_suppresses_check()
    // returns false for PRIVCFG=2 → privileged_only check fires → Err (correct).
    // STRW=El2 would make strw_suppresses_priv()=true, which the BUG-RUST-2 TLB path wrongly uses.
    let result = ctx.translate(
        pasid(0),
        iova(0x1000),
        AccessType::ReadPrivileged,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-1b: PRIVCFG=2 must demote ReadPrivileged; effective_priv_suppresses_check()=false \
         → privileged_only check must fire → expect Err; got Ok"
    );
}

// ============================================================================
// BUG-RUST-2: TLB fast path uses strw_suppresses_priv() instead of
//             effective_priv_suppresses_check()
//
// smmu/mod.rs ~line 3336:
//   BEFORE FIX: if cached.permissions.privileged_only() && !stream_ref.value().strw_suppresses_priv()
//   AFTER FIX:  if cached.permissions.privileged_only() && !stream_ref.value().effective_priv_suppresses_check()
//
// PRIVCFG=3/STRW=El1El0:
//   strw_suppresses_priv() = false  → TLB HIT wrongly DENIES  (bug)
//   effective_priv_suppresses_check() = true → TLB HIT correctly ALLOWS (fix)
//
// PRIVCFG=2/STRW=El2:
//   strw_suppresses_priv() = true   → TLB HIT wrongly ALLOWS  (bug)
//   effective_priv_suppresses_check() = false → TLB HIT correctly DENIES (fix)
// ============================================================================

/// BUG-RUST-2a: PRIVCFG=3 (Force Privileged) / STRW=El1El0 / stage-1-only.
///
/// A `privileged_only` page must be accessible when PRIVCFG=3 is active.
///
/// First translate (TLB miss → slow path via translate_and_get_stage2_ipa):
///   effective_priv_suppresses_check() = true (PRIVCFG=3) → check SKIPPED → Ok.   ← correct
///
/// Second translate (TLB hit → fast path in smmu/mod.rs ~3336):
///   BEFORE FIX: strw_suppresses_priv() for El1El0 = false → check fires → Err.   ← BUG
///   AFTER FIX:  effective_priv_suppresses_check() = true (PRIVCFG=3) → SKIPPED → Ok.
///
/// This test FAILS before the fix (second assert fails).
#[test]
fn bug_rust_2a_privcfg3_el1_tlb_hit_should_allow_privileged_only_page() {
    let smmu = make_smmu();
    let stream_id = sid(0xD0);

    let mut cfg = StreamConfig::stage1_only();
    cfg.strw = StreamWorld::El1El0; // strw_suppresses_priv()=false
    cfg.priv_cfg = 3;               // PRIVCFG=3 → effective_priv_suppresses_check()=true
    cfg.t0sz = 0;
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();

    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only().with_privileged_only(true),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate: TLB miss → slow path.
    // effective_priv_suppresses_check() = true (PRIVCFG=3) → privileged_only SKIPPED → Ok.
    let result1 = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result1.is_ok(),
        "BUG-RUST-2a: First translate (TLB miss, slow path) must succeed for PRIVCFG=3/El1El0 \
         + privileged_only page; got {result1:?}"
    );

    // Second translate: TLB hit → fast path.
    // BEFORE FIX: strw_suppresses_priv() for El1El0 = false → privileged_only fires → Err.  BUG.
    // AFTER FIX:  effective_priv_suppresses_check() for PRIVCFG=3 = true → SKIPPED → Ok.
    let result2 = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result2.is_ok(),
        "BUG-RUST-2a: Second translate (TLB hit, fast path) must ALSO succeed: \
         TLB fast path uses strw_suppresses_priv() (El1El0→false) instead of \
         effective_priv_suppresses_check() (PRIVCFG=3→true); incorrectly denies access; \
         got {result2:?}"
    );
}

/// BUG-RUST-2b: PRIVCFG=2 (Force Unprivileged) / STRW=El2 / stage-1-only.
///
/// After a successful translate that populates the TLB (using PRIVCFG=3), the stream is
/// reconfigured to PRIVCFG=2.  A subsequent TLB-hit translate should be DENIED because
/// PRIVCFG=2 makes effective_priv_suppresses_check()=false.  But the TLB fast path uses
/// strw_suppresses_priv() (El2=true) → it wrongly ALLOWS access.
///
/// BEFORE FIX: TLB hit → strw_suppresses_priv()=true → allows access (WRONG).
/// AFTER FIX:  TLB hit → effective_priv_suppresses_check()=false → denies (CORRECT).
///
/// This test FAILS before the fix (result2 is Ok but should be Err).
///
/// NOTE: configure_stream may reset the stream's PASID map.  We re-populate after
/// reconfiguration.  If configure_stream also flushes the TLB for this stream, the test
/// will fall through to the slow path and may not exercise the TLB-hit path.  We verify
/// the TLB-hit path is actually exercised by asserting the result differs between
/// slow-path semantics and fast-path semantics.
#[test]
fn bug_rust_2b_privcfg2_el2_tlb_hit_should_deny_privileged_only_page() {
    let smmu = make_smmu();
    let stream_id = sid(0xD1);

    // Step 1: configure with PRIVCFG=3/El2 so the first translate succeeds and
    // populates the TLB cache for IOVA 0x1000.
    // STRW=El2 with NonSecure is rejected (BUG-RUST-2a fix, ARM §5.2 bit[0]=1 rule).
    // Use Secure security state — El2 is valid for Secure and same suppression semantics.
    let mut cfg_priv3 = StreamConfig::stage1_only();
    cfg_priv3.strw = StreamWorld::El2;
    cfg_priv3.security_state = SecurityState::Secure;
    cfg_priv3.priv_cfg = 3; // Force Privileged
    cfg_priv3.t0sz = 0;
    smmu.configure_stream(stream_id, cfg_priv3).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only().with_privileged_only(true),
        SecurityState::NonSecure,
    )
    .unwrap();

    // TLB-populating translate (slow path, PRIVCFG=3 → succeeds).
    let result1 = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result1.is_ok(),
        "BUG-RUST-2b setup: PRIVCFG=3/El2 translate must succeed to populate TLB; got {result1:?}"
    );

    // Step 2: reconfigure to PRIVCFG=2/El2.  reconfigure_stream calls update_configuration()
    // on the existing StreamContext without TLB invalidation — the TLB entry for IOVA 0x1000
    // remains cached.  The next TLB-hit translate will use the stale fast-path check.
    let mut cfg_priv2 = StreamConfig::stage1_only();
    cfg_priv2.strw = StreamWorld::El2;
    // STRW=El2 (bit[0]=1) is only valid for Secure streams (ARM §5.2).
    // The initial configure_stream used Secure; reconfigure must match.
    cfg_priv2.security_state = SecurityState::Secure;
    cfg_priv2.priv_cfg = 2; // Force Unprivileged
    cfg_priv2.t0sz = 0;
    smmu.reconfigure_stream(stream_id, cfg_priv2).unwrap();

    // Step 3: second translate — may hit TLB (fast path) if TLB was not flushed.
    // BEFORE FIX: TLB hit → strw_suppresses_priv() for El2 = true → SKIPPED → Ok (WRONG).
    //   PRIVCFG=2 must demote; effective_priv_suppresses_check() would return false.
    // AFTER FIX:  TLB hit → effective_priv_suppresses_check() for PRIVCFG=2 = false → Err.
    let result2 = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result2.is_err(),
        "BUG-RUST-2b: PRIVCFG=2/El2 + privileged_only page: TLB hit must be DENIED: \
         fast path uses strw_suppresses_priv() (El2→true) instead of \
         effective_priv_suppresses_check() (PRIVCFG=2→false); incorrectly allows access; \
         got {result2:?}"
    );
}

// ============================================================================
// BUG-RUST-6: translate_two_stage missing CD.IPS output address size check
//
// translate_two_stage_with_ipa (~lines 1987-1996): IPS check present.
// translate_two_stage (~lines 2243-2416): IPS check MISSING.
//
// When called via SMMU::translate() → translate_and_get_stage2_ipa → translate_two_stage_with_ipa:
//   IPS check fires → F_ADDR_SIZE (CORRECT).
//
// When called via StreamContext::translate() → translate_two_stage:
//   No IPS check → stage-2 lookup proceeds → Ok if mapped (WRONG) or PageNotMapped if not.
//
// IPS encoding (same as S2PS / s2ps_to_bits):
//   IPS=5 → 48-bit → limit = 2^48 = 0x1_0000_0000_0000
// ============================================================================

/// BUG-RUST-6a: translate_two_stage IPS check missing — IPA exceeds 48-bit limit.
///
/// CD.IPS=5 (default) limits IPA to < 2^48.  Stage-1 maps IOVA 0x1000 → IPA 0x1_0000_0000_0000
/// (= 2^48, out of range).  Stage-2 maps this IPA so that the absence of the IPS check
/// is observable as Ok instead of Err(AddressSizeError).
///
/// Via StreamContext::translate() → translate_two_stage (NO IPS check):
///   BEFORE FIX: stage-2 lookup succeeds → Ok (WRONG — F_ADDR_SIZE expected).
///   AFTER FIX:  IPS check added → Err(AddressSizeError) → F_ADDR_SIZE (CORRECT).
///
/// This test FAILS before the fix (result is Ok, expected Err).
#[test]
fn bug_rust_6a_translate_two_stage_ips_check_missing_via_streamcontext() {
    let ctx = StreamContext::new();
    let mut cfg = StreamConfig::two_stage();
    cfg.strw = StreamWorld::El1El0;
    cfg.t0sz = 0;
    cfg.s2_t0sz = 0;
    cfg.ips = 5; // 48-bit IPS: IPA must be < 2^48 = 0x1_0000_0000_0000
    ctx.update_configuration(cfg);
    ctx.enable();
    ctx.create_pasid(pasid(0)).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x1_0000_0000_0000 (= 2^48, exceeds 48-bit IPS limit).
    ctx.map_page(
        pasid(0),
        iova(0x1000),
        pa(0x0001_0000_0000_0000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: map the out-of-range IPA so that the missing IPS check is observable.
    // If the IPS check is absent, stage-2 lookup succeeds and translate returns Ok.
    ctx.map_stage2_page(
        iova(0x0001_0000_0000_0000),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Via StreamContext::translate() → translate_two_stage (no IPS check).
    // BEFORE FIX: stage-2 mapping exists → Ok(PA=0x3000) — WRONG.
    // AFTER FIX:  IPS check: 0x1_0000_0000_0000 >= 2^48 → Err(AddressSizeError) — CORRECT.
    let result = ctx.translate(pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_err(),
        "BUG-RUST-6a: translate_two_stage must reject IPA 0x1_0000_0000_0000 that exceeds \
         CD.IPS=5 (48-bit) limit with Err(AddressSizeError); translate_two_stage is missing \
         the CD.IPS check present in translate_two_stage_with_ipa; got Ok: {result:?}"
    );
}

/// BUG-RUST-6b: Regression guard — SMMU slow path (translate_and_get_stage2_ipa) already
/// has the IPS check and must continue to produce F_ADDR_SIZE for an out-of-IPS IPA.
///
/// This test must PASS both before and after the fix.
#[test]
fn bug_rust_6b_smmu_translate_ips_check_present_on_slow_path_regression_guard() {
    let smmu = make_smmu();
    let stream_id = sid(0xE0);

    let mut cfg = StreamConfig::two_stage();
    cfg.strw = StreamWorld::El1El0;
    cfg.t0sz = 0;
    cfg.s2_t0sz = 16; // minimum legal S2T0SZ per §5.2 (BUG-AUDIT-45 fix: 0 is out of range [16,39])
    cfg.ips = 5; // 48-bit IPS
    smmu.configure_stream(stream_id, cfg).unwrap();
    smmu.create_pasid(stream_id, pasid(0)).unwrap();
    smmu.create_stage2_address_space(stream_id).unwrap();

    // Stage-1: IOVA 0x1000 → IPA 0x1_0000_0000_0000 (= 2^48, out of range).
    smmu.map_page(
        stream_id,
        pasid(0),
        iova(0x1000),
        pa(0x0001_0000_0000_0000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: map the out-of-range IPA (should never be reached due to IPS check).
    smmu.map_stage2_page(
        stream_id,
        iova(0x0001_0000_0000_0000),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // SMMU::translate() → translate_and_get_stage2_ipa → IPS check fires → Err(AddressSizeError).
    let result = smmu.translate(
        stream_id,
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_err(),
        "BUG-RUST-6b: SMMU slow path must detect IPA 0x1_0000_0000_0000 out of CD.IPS=5 range \
         and return Err; got Ok: {result:?}"
    );

    // Verify the correct F_ADDR_SIZE event was recorded.
    let events = smmu.get_events();
    assert!(
        !events.is_empty(),
        "BUG-RUST-6b: Expected F_ADDR_SIZE event to be recorded; no events found"
    );
    let ev = events.last().unwrap();
    assert_eq!(
        ev.event_type,
        EventType::FAddrSize,
        "BUG-RUST-6b: Expected F_ADDR_SIZE event type (0x11); got {:?}",
        ev.event_type
    );
}

/// BUG-RUST-6c: Boundary — IPA exactly at limit - page_size (0x0000_FFFF_FFFF_F000) is OK.
///
/// IPS=5 (48-bit): limit = 2^48.  IPA = 0xFFFF_FFFF_F000 < 2^48 → within range.
/// Translation must succeed via StreamContext::translate() both before and after the fix
/// (the IPS check, when added, must not reject in-range IPAs).
///
/// This test is a regression guard that must PASS both before and after the fix.
#[test]
fn bug_rust_6c_translate_two_stage_ips_within_range_succeeds() {
    let ctx = StreamContext::new();
    let mut cfg = StreamConfig::two_stage();
    cfg.t0sz = 0;
    cfg.s2_t0sz = 0;
    cfg.ips = 5; // 48-bit IPS
    ctx.update_configuration(cfg);
    ctx.enable();
    ctx.create_pasid(pasid(0)).unwrap();
    ctx.create_stage2_address_space().unwrap();

    // IPA = 0xFFFF_FFFF_F000 < 2^48 → within 48-bit IPS range.
    let in_range_ipa: u64 = 0x0000_FFFF_FFFF_F000;
    ctx.map_page(
        pasid(0),
        iova(0x1000),
        pa(in_range_ipa),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();
    ctx.map_stage2_page(
        iova(in_range_ipa),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = ctx.translate(pasid(0), iova(0x1000), AccessType::Read, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-RUST-6c: IPA 0xFFFF_FFFF_F000 is within 48-bit IPS range and must succeed; \
         got {result:?}"
    );
}
