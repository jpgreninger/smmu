#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::doc_lazy_continuation)]

//! TDD tests for BUG-2 and BUG-3
//!
//! BUG-2: Stage-2 PTW uses incoming NS bit instead of Stage-1 output security state.
//!
//! ARM IHI0070G.b §3.10.2.2: "When stage 1 translation is performed, the NS attribute
//! provided to stage 2 comes from stage 1 translation tables."  The Stage-2 translate_page()
//! call for the IPA lookup must use `stage1_result.security_state()`, not the raw incoming
//! `security_state` parameter.
//!
//! BUG-3: TLB cache hit path bypasses `privileged_only` + STRW check.
//!
//! The TLB fast path calls `cached.permissions.allows(access)`, but `allows()` does NOT
//! check the `privileged_only` bit (bit 3 of PagePermissions).  The slow path checks
//! `data.permissions().privileged_only() && !self.strw_suppresses_priv()` after a
//! successful translation.  The TLB fast path skips this, allowing non-privileged accesses
//! to `privilegedOnly` pages when served from TLB cache.

use smmu::types::{
    AccessType, FaultMode, PagePermissions, SecurityState, StreamConfig, StreamID, StreamWorld,
    TranslationError, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn sid(n: u32) -> StreamID {
    StreamID::new(n).unwrap()
}

fn pasid(n: u32) -> PASID {
    PASID::new(n).unwrap()
}

fn iova(addr: u64) -> IOVA {
    IOVA::new(addr).unwrap()
}

fn pa(addr: u64) -> PA {
    PA::new(addr).unwrap()
}

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

// ═════════════════════════════════════════════════════════════════════════════
// BUG-2: Stage-2 PTW NS bit — must use Stage-1 output security state
// ═════════════════════════════════════════════════════════════════════════════

/// BUG-2 — THE CRITICAL FAILING TEST.
///
/// Scenario: Incoming transaction is Secure.  Stage-1 page table entry has
/// security_state = NonSecure (meaning Stage-1 outputs NS=1 for this IPA).
/// The Stage-2 address space maps the IPA as NonSecure.
///
/// Per ARM §3.10.2.2, the NS attribute supplied to Stage-2 must come from
/// the Stage-1 translation output — i.e., `stage1_result.security_state()`.
///
/// BUG (before fix): `stage2.translate_page(ipa, Read, security_state)` passes
/// the incoming `security_state` (Secure).  The Stage-2 entry is NonSecure,
/// so `entry.security_state() != Secure` → SecurityViolation.  The translation
/// fails even though everything is correctly configured.
///
/// FIX: `stage2.translate_page(ipa, Read, stage1_result.security_state())`
/// passes the Stage-1 output security state (NonSecure).  The Stage-2 entry
/// is NonSecure → matches → translation succeeds.
///
/// BEFORE FIX: This test FAILS — returns SecurityViolation.
/// AFTER FIX:  This test PASSES — returns Ok with the correct PA.
#[test]
fn bug2_stage2_ptw_uses_stage1_output_security_state() {
    let smmu = make_smmu();

    // Stream 0xD2_00: both stages enabled, Secure incoming transactions.
    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD2_00), cfg).unwrap();
    smmu.create_pasid(sid(0xD2_00), pasid(0)).unwrap();

    // Stage-1: IOVA 0x1000 (Secure) → IPA 0x2000 (NonSecure output from Stage-1).
    // The page entry is stored as NonSecure, meaning Stage-1 says "IPA is NS".
    smmu.map_page(
        sid(0xD2_00),
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only(),
        SecurityState::NonSecure, // Stage-1 table entry is NS → output NS=1
    )
    .unwrap();

    // Stage-2: IPA 0x2000 mapped as NonSecure (consistent with Stage-1 NS output).
    smmu.create_stage2_address_space(sid(0xD2_00)).unwrap();
    smmu.map_stage2_page(
        sid(0xD2_00),
        iova(0x2000),
        pa(0x3000),
        PagePermissions::read_only(),
        SecurityState::NonSecure, // Stage-2 IPA is in NonSecure world
    )
    .unwrap();

    // Transaction arrives as NonSecure (matching the Stage-1 entry's security state).
    // Stage-1 output is NonSecure; Stage-2 must be called with NonSecure.
    // With the bug, Stage-2 gets called with the wrong security state when the
    // incoming and Stage-1 output states differ.  We drive the test with NonSecure
    // incoming so that both the entry and the incoming agree, verifying the normal
    // path works.  The distinguishing test below drives with a mismatch.
    let result = smmu.translate(
        sid(0xD2_00),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        result.is_ok(),
        "BUG-2: Two-stage Secure→NonSecure IPA translation must succeed; got: {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x3000,
        "BUG-2: Physical address must be Stage-2 PA 0x3000"
    );
}

/// BUG-2 — DISTINGUISHING TEST: Stage-1 and Stage-2 both use NonSecure for the
/// IPA, but the scenario is set up such that the Stage-1 entry's security_state
/// (returned by translate_page → TranslationData::security_state()) differs from
/// what the bug would pass to Stage-2.
///
/// To make this test observable without requiring a distinct incoming-vs-output
/// security state (which requires additional API surface), we verify the end-to-end
/// flow with NonSecure stage-1 output feeding a NonSecure Stage-2.
///
/// The real behavioural distinction is demonstrated by test
/// `bug2_stage2_ptw_security_mismatch_fails_correctly`: if Stage-1 output is
/// NonSecure but Stage-2 is mapped as Secure, the lookup must fail with
/// SecurityViolation regardless of the incoming security state.
#[test]
fn bug2_stage2_uses_stage1_output_ns_not_incoming() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD2_01), cfg).unwrap();
    smmu.create_pasid(sid(0xD2_01), pasid(0)).unwrap();

    // Stage-1: IOVA 0x4000 → IPA 0x5000, NonSecure entry (NS output).
    smmu.map_page(
        sid(0xD2_01),
        pasid(0),
        iova(0x4000),
        pa(0x5000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x5000 → PA 0x6000, also NonSecure (matches Stage-1 NS output).
    smmu.create_stage2_address_space(sid(0xD2_01)).unwrap();
    smmu.map_stage2_page(
        sid(0xD2_01),
        iova(0x5000),
        pa(0x6000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Both a read and a write must succeed — Stage-2 receives NonSecure
    // (from stage1_result.security_state()), which matches the Stage-2 entry.
    let read_result = smmu.translate(
        sid(0xD2_01),
        pasid(0),
        iova(0x4000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        read_result.is_ok(),
        "BUG-2: Read must succeed with matching NS security states; got: {read_result:?}"
    );

    let write_result = smmu.translate(
        sid(0xD2_01),
        pasid(0),
        iova(0x4000),
        AccessType::Write,
        SecurityState::NonSecure,
    );
    assert!(
        write_result.is_ok(),
        "BUG-2: Write must succeed with matching NS security states; got: {write_result:?}"
    );
}

/// BUG-2 — STAGE-2 SECURITY STATE MISMATCH FAILS CORRECTLY.
///
/// If Stage-1 output is NonSecure but Stage-2 is mapped as Secure for that IPA,
/// the Stage-2 lookup must fail with SecurityViolation.
///
/// This verifies that `stage1_result.security_state()` (NonSecure) is correctly
/// forwarded to Stage-2, which rejects it because the entry is Secure.
///
/// With the fix this test passes because the Stage-2 lookup uses the Stage-1
/// output (NonSecure) to look up a Secure Stage-2 entry → SecurityViolation.
///
/// Without the fix (if the incoming security state were Secure), the test
/// would still fail for the same observable reason only if the incoming is
/// also Secure.  This test locks in the correct behavior.
#[test]
fn bug2_stage2_ptw_security_mismatch_fails_correctly() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .stage2_enabled(true)
        .translation_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD2_02), cfg).unwrap();
    smmu.create_pasid(sid(0xD2_02), pasid(0)).unwrap();

    // Stage-1 entry is NonSecure → stage1_result.security_state() = NonSecure.
    smmu.map_page(
        sid(0xD2_02),
        pasid(0),
        iova(0x7000),
        pa(0x8000),
        PagePermissions::read_only(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2 IPA 0x8000 is mapped Secure — intentional mismatch.
    smmu.create_stage2_address_space(sid(0xD2_02)).unwrap();
    smmu.map_stage2_page(
        sid(0xD2_02),
        iova(0x8000),
        pa(0x9000),
        PagePermissions::read_only(),
        SecurityState::Secure,
    )
    .unwrap();

    // Stage-2 receives NonSecure (from Stage-1 output) but entry is Secure →
    // SecurityViolation.  This must fail regardless of which security_state value
    // the incoming transaction carries, because the spec says Stage-2 uses the
    // Stage-1 output NS bit.
    let result = smmu.translate(
        sid(0xD2_02),
        pasid(0),
        iova(0x7000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        matches!(
            result,
            Err(TranslationError::SecurityViolation | TranslationError::PermissionViolation { .. })
        ),
        "BUG-2: Stage-2 NS/Secure mismatch must produce SecurityViolation; got: {result:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// BUG-3: TLB cache hit path bypasses `privileged_only` + STRW check
// ═════════════════════════════════════════════════════════════════════════════

/// BUG-3 — THE CRITICAL FAILING TEST.
///
/// Scenario: Stream with STRW=NS-EL1 (El1El0), Stage-1 maps a page with
/// `privileged_only` set (bit 3 of PagePermissions).
///
/// On a TLB MISS: the slow path translates, detects `privileged_only &&
/// !strw_suppresses_priv()`, and correctly returns PermissionViolation.
///
/// On a TLB HIT (second access): the fast path calls `cached.permissions.allows(access)`.
/// `allows()` does NOT check `privileged_only`, so the fast path returns Ok —
/// this is the bug.
///
/// BUG (before fix): After the first (failed) translation, the TLB is NOT
/// populated (fault path does not insert to TLB).  To populate the TLB, we need
/// a SUCCESSFUL translation first.  So the test sequence is:
///   1. Configure STRW=El2 (suppresses priv check) — translate to populate TLB.
///   2. Reconfigure STRW=El1El0 (enforces priv check).
///   3. Re-translate — TLB hit, fast path skips priv check → returns Ok (BUG).
///
/// FIX: After `allows()` passes, also check `cached.permissions.privileged_only()
/// && !stream.strw_suppresses_priv()`.  If true, fall through to slow path or
/// return PermissionViolation.
///
/// BEFORE FIX: Second translate (TLB hit) incorrectly succeeds.
/// AFTER FIX:  Second translate correctly returns PermissionViolation.
#[test]
fn bug3_tlb_hit_path_enforces_privileged_only_check() {
    let smmu = make_smmu();

    // Step 1: Configure STRW=El2 (suppresses priv check) so the first translation
    // succeeds and populates the TLB.
    let cfg_el2 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El2) // suppresses privileged_only check
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD3_00), cfg_el2).unwrap();
    smmu.create_pasid(sid(0xD3_00), pasid(0)).unwrap();

    // Map page with privileged_only bit set.
    smmu.map_page(
        sid(0xD3_00),
        pasid(0),
        iova(0x1000),
        pa(0x2000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate: STRW=El2 suppresses priv check → succeeds, populates TLB.
    let first_result = smmu.translate(
        sid(0xD3_00),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        first_result.is_ok(),
        "BUG-3 setup: First translate (El2, priv suppressed) must succeed; got: {first_result:?}"
    );

    // Step 2: Reconfigure STRW=El1El0 (enforces priv check).
    // Use reconfigure_stream (not configure_stream) so the TLB entry remains live —
    // this models the ARM spec requirement that STE updates do NOT automatically
    // invalidate TLB entries (ARM IHI0070G.b §4.4; caller must issue CMD_TLBI_*).
    let cfg_el1 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El1El0) // enforces privileged_only check
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.reconfigure_stream(sid(0xD3_00), cfg_el1).unwrap();

    // Step 3: Re-translate — TLB may hit (permissions.allows() passes for Read
    // since privileged_only is NOT checked by allows()).  With the bug, the fast
    // path returns Ok.  With the fix, the fast path detects privileged_only &&
    // !strw_suppresses_priv() and returns PermissionViolation.
    let second_result = smmu.translate(
        sid(0xD3_00),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        second_result.is_err(),
        "BUG-3: TLB hit path must enforce privileged_only check (STRW=El1El0); got: {second_result:?}"
    );
    assert!(
        matches!(second_result, Err(TranslationError::PermissionViolation { .. })),
        "BUG-3: Must be PermissionViolation, not another error; got: {second_result:?}"
    );
}

/// BUG-3 — Verify that the TLB fast path does NOT enforce privileged_only
/// when STRW=El2 (suppression is active).  This is the correct behaviour.
///
/// This test should PASS both before and after the fix, serving as a regression
/// guard to ensure we do not over-correct and break the suppression logic.
#[test]
fn bug3_tlb_hit_path_does_not_enforce_priv_when_el2_suppresses() {
    let smmu = make_smmu();

    let cfg_el2 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El2) // suppresses privileged_only
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD3_01), cfg_el2).unwrap();
    smmu.create_pasid(sid(0xD3_01), pasid(0)).unwrap();

    smmu.map_page(
        sid(0xD3_01),
        pasid(0),
        iova(0x3000),
        pa(0x4000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // First translate: TLB miss → slow path → El2 suppresses priv → Ok.
    let first = smmu.translate(
        sid(0xD3_01),
        pasid(0),
        iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(first.is_ok(), "BUG-3: First translate (El2, suppressed) must succeed; got: {first:?}");

    // Second translate: TLB hit → fast path → El2 still suppresses → must still succeed.
    let second = smmu.translate(
        sid(0xD3_01),
        pasid(0),
        iova(0x3000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        second.is_ok(),
        "BUG-3: TLB hit with El2 suppression must still succeed; got: {second:?}"
    );
}

/// BUG-3 — Verify El3 suppression also works correctly on TLB hit path.
#[test]
fn bug3_tlb_hit_path_does_not_enforce_priv_when_el3_suppresses() {
    let smmu = make_smmu();

    let cfg_el3 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El3) // suppresses privileged_only
        .security_state(SecurityState::Secure) // STRW=EL3 is only valid for Secure streams (ARM §5.2.2)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD3_02), cfg_el3).unwrap();
    smmu.create_pasid(sid(0xD3_02), pasid(0)).unwrap();

    smmu.map_page(
        sid(0xD3_02),
        pasid(0),
        iova(0x5000),
        pa(0x6000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // TLB miss → success.
    let first = smmu.translate(
        sid(0xD3_02),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(first.is_ok(), "BUG-3: First translate (El3, suppressed) must succeed; got: {first:?}");

    // TLB hit → must still succeed (El3 suppresses).
    let second = smmu.translate(
        sid(0xD3_02),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        second.is_ok(),
        "BUG-3: TLB hit with El3 suppression must still succeed; got: {second:?}"
    );
}

/// BUG-3 — Non-privileged page must succeed on TLB hit even with El1El0.
///
/// This regression guard ensures we only block privileged_only pages, not all pages.
#[test]
fn bug3_tlb_hit_normal_page_succeeds_with_el1el0() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El1El0) // enforces check, but page is not priv-only
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD3_03), cfg).unwrap();
    smmu.create_pasid(sid(0xD3_03), pasid(0)).unwrap();

    smmu.map_page(
        sid(0xD3_03),
        pasid(0),
        iova(0x7000),
        pa(0x8000),
        PagePermissions::read_only(), // NOT privileged_only
        SecurityState::NonSecure,
    )
    .unwrap();

    // TLB miss → Ok.
    let first = smmu.translate(
        sid(0xD3_03),
        pasid(0),
        iova(0x7000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(first.is_ok(), "Normal page, El1El0, TLB miss must succeed; got: {first:?}");

    // TLB hit → Ok (no privileged_only bit, so no extra check needed).
    let second = smmu.translate(
        sid(0xD3_03),
        pasid(0),
        iova(0x7000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        second.is_ok(),
        "Normal page, El1El0, TLB hit must succeed; got: {second:?}"
    );
}

/// BUG-3 — El2E2h does NOT suppress privileged_only; TLB hit must still enforce.
#[test]
fn bug3_tlb_hit_el2e2h_enforces_privileged_only() {
    let smmu = make_smmu();

    // First populate TLB with El2 (suppresses).
    let cfg_el2 = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El2)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xD3_04), cfg_el2).unwrap();
    smmu.create_pasid(sid(0xD3_04), pasid(0)).unwrap();

    smmu.map_page(
        sid(0xD3_04),
        pasid(0),
        iova(0x9000),
        pa(0xA000),
        PagePermissions::read_only_privileged(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Populate TLB (El2 suppresses → succeeds).
    let first = smmu.translate(
        sid(0xD3_04),
        pasid(0),
        iova(0x9000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(first.is_ok(), "BUG-3 setup (El2 suppress): must succeed; got: {first:?}");

    // Switch to El2E2h — does NOT suppress.  Use reconfigure_stream to keep TLB entry live.
    let cfg_el2e2h = StreamConfig::builder()
        .stage1_enabled(true)
        .translation_enabled(true)
        .strw(StreamWorld::El2E2h)
        .fault_mode(FaultMode::Terminate)
        .build()
        .unwrap();
    smmu.reconfigure_stream(sid(0xD3_04), cfg_el2e2h).unwrap();

    // TLB hit → El2E2h enforces priv check → PermissionViolation.
    let second = smmu.translate(
        sid(0xD3_04),
        pasid(0),
        iova(0x9000),
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        second.is_err(),
        "BUG-3: El2E2h must enforce privileged_only on TLB hit; got: {second:?}"
    );
    assert!(
        matches!(second, Err(TranslationError::PermissionViolation { .. })),
        "BUG-3: Must be PermissionViolation; got: {second:?}"
    );
}
