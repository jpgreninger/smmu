//! Tests for BUG-RUST-DBGR-4 — STE.Config=0b000 abort_mode coexistence with stage1_enabled
#![allow(clippy::doc_markdown)]
//!
//! ARM §5.2/§7.3.6: STE.Config=0b000 (disabled/abort_mode) must force both stages
//! to disabled. abort_mode=true and stage1_enabled=true is a contradictory state.

use smmu::stream_context::StreamContext;
use smmu::types::{AccessType, SecurityState, StreamConfig, TranslationError, IOVA, PASID};

/// Helper: return a StreamConfig where disabled=true and stage1_enabled=true are
/// both set in the struct fields, bypassing the builder validation, so we can test
/// that update_configuration() itself enforces consistency.
fn disabled_with_stage1_raw() -> StreamConfig {
    // Start from bypass (disabled=false, both stages off) then mutate the fields.
    // The bypass() constructor is the easiest source of a fully-initialized struct.
    let mut cfg = StreamConfig::bypass();
    // Force the contradictory combination: abort mode set, but stage1 also set.
    cfg.disabled = true;
    cfg.stage1_enabled = true;
    cfg
}

/// When abort_mode is set (disabled=true), update_configuration() must
/// force stage1_enabled and stage2_enabled to false.
#[test]
fn dbgr4_abort_mode_forces_stages_disabled() {
    let ctx = StreamContext::new();
    ctx.update_configuration(disabled_with_stage1_raw());

    // After update_configuration, both stages must be disabled because abort_mode wins.
    assert!(
        !ctx.is_stage1_enabled(),
        "abort_mode must force stage1_enabled to false"
    );
    assert!(
        !ctx.is_stage2_enabled(),
        "abort_mode must force stage2_enabled to false"
    );
}

/// Translation on an abort_mode stream must return AbortMode (ARM §5.2: silent, no event).
/// BUG-R-05 fix: abort mode is distinct from administrative stream-disabled (§7.3.7).
#[test]
fn dbgr4_abort_mode_translation_returns_stream_disabled() {
    let ctx = StreamContext::new();
    let mut cfg = StreamConfig::bypass();
    cfg.disabled = true;
    ctx.update_configuration(cfg);

    let iova = IOVA::new(0x1000).unwrap();
    let pasid = PASID::new(0).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::AbortMode)),
        "abort_mode stream must return AbortMode (§5.2 silent termination); got: {result:?}"
    );
}

/// abort_mode set via builder (translation_enabled=false) produces disabled=true,
/// and translation returns AbortMode (ARM §5.2 silent termination, no event).
/// BUG-R-05 fix: AbortMode is distinct from StreamDisabled (§7.3.7).
#[test]
fn dbgr4_abort_mode_via_builder_translation_enabled_false() {
    let ctx = StreamContext::new();
    // translation_enabled(false) sets disabled=true (builder semantics)
    let cfg = StreamConfig::builder()
        .translation_enabled(false)
        .build()
        .expect("disabled config must build");
    ctx.update_configuration(cfg);

    let iova = IOVA::new(0x1000).unwrap();
    let pasid = PASID::new(0).unwrap();
    let result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(result, Err(TranslationError::AbortMode)),
        "builder-disabled stream must return AbortMode (§5.2 silent termination); got: {result:?}"
    );
}

/// Contradictory config: setting abort_mode and then re-enabling stage1 should
/// not override abort_mode — the abort_mode atomically stores false for stages.
#[test]
fn dbgr4_abort_mode_stage1_not_accessible() {
    let ctx = StreamContext::new();
    // Apply abort_mode config
    ctx.update_configuration(disabled_with_stage1_raw());
    // Verify stage1 is forced off
    assert!(
        !ctx.is_stage1_enabled(),
        "abort_mode config must disable stage1"
    );
}
