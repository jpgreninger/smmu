//! FINDING-NEW-36: `StreamConfig::is_bypass()` conflates bypass and abort mode.
//!
//! ARM IHI0070G.b §5.2, Table 5-5:
//! - STE.Config==0b000 → Abort: transactions terminated silently, no event recorded.
//! - STE.Config==0b100 → Bypass: transactions passed through with full RWX (PA == IOVA).
//!
//! Before the fix, `is_bypass()` returned `!translation_enabled`, which is `true` for
//! BOTH abort mode (`disabled=true`) AND bypass mode (`disabled=false`).  API consumers
//! could not distinguish the two.  After the fix:
//! - `is_bypass()`     → true only when `disabled=false && translation_enabled=false`
//! - `is_abort_mode()` → true only when `disabled=true  && translation_enabled=false`

use smmu::types::config::StreamConfig;

// ─────────────────────────────────────────────────────────────────────────────
// is_bypass() must be FALSE for abort / disabled mode (STE.Config==0b000)
// ─────────────────────────────────────────────────────────────────────────────

/// Builder default with explicit `translation_enabled(false)` produces abort mode
/// (STE.Config==0b000, `disabled=true`).  `is_bypass()` must return false.
#[test]
fn test_is_bypass_false_for_abort_mode_via_builder() {
    // Calling translation_enabled(false) on the builder sets disabled=true.
    let config = StreamConfig::builder()
        .translation_enabled(false)
        .build()
        .unwrap();

    assert!(
        config.disabled,
        "builder with translation_enabled(false) should set disabled=true"
    );
    assert!(
        !config.translation_enabled,
        "translation_enabled should be false"
    );
    assert!(
        !config.is_bypass(),
        "abort mode (STE.Config==0b000, disabled=true) must NOT report is_bypass() == true"
    );
}

/// Direct struct construction representing abort mode (`disabled=true`,
/// `translation_enabled=false`).  `is_bypass()` must return false.
#[test]
fn test_is_bypass_false_for_abort_mode_direct() {
    let config = StreamConfig {
        translation_enabled: false,
        disabled: true,
        stage1_enabled: false,
        stage2_enabled: false,
        ..StreamConfig::bypass()
    };

    assert!(
        !config.is_bypass(),
        "abort mode (disabled=true, translation_enabled=false) must NOT be bypass"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// is_bypass() must be TRUE for bypass mode (STE.Config==0b100)
// ─────────────────────────────────────────────────────────────────────────────

/// `StreamConfig::bypass()` factory produces STE.Config==0b100 (`disabled=false`,
/// `translation_enabled=false`).  `is_bypass()` must return true.
#[test]
fn test_is_bypass_true_for_bypass_factory() {
    let config = StreamConfig::bypass();

    assert!(
        !config.disabled,
        "bypass() factory must have disabled=false"
    );
    assert!(
        !config.translation_enabled,
        "bypass() factory must have translation_enabled=false"
    );
    assert!(
        config.is_bypass(),
        "bypass mode (STE.Config==0b100, disabled=false) must report is_bypass() == true"
    );
}

/// Builder default (no calls to `translation_enabled`) is bypass mode.
/// `is_bypass()` must return true.
#[test]
fn test_is_bypass_true_for_builder_default() {
    let config = StreamConfig::builder().build().unwrap();

    assert!(
        !config.disabled,
        "builder default should have disabled=false (bypass, not abort)"
    );
    assert!(
        config.is_bypass(),
        "builder default (no explicit translation_enabled call) must be bypass mode"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// is_abort_mode() — new method required by the fix
// ─────────────────────────────────────────────────────────────────────────────

/// Abort mode (`disabled=true`, `translation_enabled=false`) must report
/// `is_abort_mode()` == true.
#[test]
fn test_is_abort_mode_true_for_disabled_via_builder() {
    let config = StreamConfig::builder()
        .translation_enabled(false)
        .build()
        .unwrap();

    assert!(
        config.is_abort_mode(),
        "abort mode via builder translation_enabled(false) must report is_abort_mode() == true"
    );
}

/// Abort mode constructed directly must report `is_abort_mode()` == true.
#[test]
fn test_is_abort_mode_true_for_direct_construction() {
    let config = StreamConfig {
        translation_enabled: false,
        disabled: true,
        stage1_enabled: false,
        stage2_enabled: false,
        ..StreamConfig::bypass()
    };

    assert!(
        config.is_abort_mode(),
        "directly constructed abort mode must report is_abort_mode() == true"
    );
}

/// Bypass mode must NOT report `is_abort_mode()` == true.
#[test]
fn test_is_abort_mode_false_for_bypass_factory() {
    let config = StreamConfig::bypass();

    assert!(
        !config.is_abort_mode(),
        "bypass mode (disabled=false) must NOT report is_abort_mode() == true"
    );
}

/// Builder default (bypass, not abort) must NOT report `is_abort_mode()` == true.
#[test]
fn test_is_abort_mode_false_for_builder_default() {
    let config = StreamConfig::builder().build().unwrap();

    assert!(
        !config.is_abort_mode(),
        "builder default (bypass) must NOT report is_abort_mode() == true"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Translation-enabled mode: neither is_bypass() nor is_abort_mode() should be true
// ─────────────────────────────────────────────────────────────────────────────

/// Normal Stage-1 translation: `is_bypass()` and `is_abort_mode()` both false.
#[test]
fn test_is_bypass_false_for_translation_enabled_stage1() {
    let config = StreamConfig::stage1_only();

    assert!(
        config.translation_enabled,
        "stage1_only() must have translation_enabled=true"
    );
    assert!(
        !config.is_bypass(),
        "Stage-1 translation mode must NOT report is_bypass() == true"
    );
    assert!(
        !config.is_abort_mode(),
        "Stage-1 translation mode must NOT report is_abort_mode() == true"
    );
}

/// Normal Stage-2 translation: `is_bypass()` and `is_abort_mode()` both false.
#[test]
fn test_is_bypass_false_for_translation_enabled_stage2() {
    let config = StreamConfig::stage2_only();

    assert!(
        !config.is_bypass(),
        "Stage-2 translation mode must NOT report is_bypass() == true"
    );
    assert!(
        !config.is_abort_mode(),
        "Stage-2 translation mode must NOT report is_abort_mode() == true"
    );
}

/// Two-stage translation: `is_bypass()` and `is_abort_mode()` both false.
#[test]
fn test_is_bypass_false_for_two_stage_translation() {
    let config = StreamConfig::two_stage();

    assert!(
        !config.is_bypass(),
        "Two-stage translation mode must NOT report is_bypass() == true"
    );
    assert!(
        !config.is_abort_mode(),
        "Two-stage translation mode must NOT report is_abort_mode() == true"
    );
}

/// Builder with `translation_enabled(true)` + `stage1_enabled(true)`:
/// `is_bypass()` and `is_abort_mode()` both false.
#[test]
fn test_is_bypass_false_for_builder_with_translation_enabled_true() {
    let config = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .build()
        .unwrap();

    assert!(
        !config.is_bypass(),
        "builder translation_enabled(true) must NOT report is_bypass() == true"
    );
    assert!(
        !config.is_abort_mode(),
        "builder translation_enabled(true) must NOT report is_abort_mode() == true"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Mutual exclusivity: is_bypass() and is_abort_mode() cannot both be true
// ─────────────────────────────────────────────────────────────────────────────

/// For any valid `StreamConfig`, `is_bypass()` and `is_abort_mode()` must not both be true.
#[test]
fn test_bypass_and_abort_mode_are_mutually_exclusive() {
    let configs = [
        StreamConfig::bypass(),
        StreamConfig::stage1_only(),
        StreamConfig::stage2_only(),
        StreamConfig::two_stage(),
        StreamConfig::builder().translation_enabled(false).build().unwrap(),
        StreamConfig::builder().build().unwrap(),
    ];

    for config in &configs {
        let both_true = config.is_bypass() && config.is_abort_mode();
        assert!(
            !both_true,
            "is_bypass() and is_abort_mode() must never both be true; \
             config: translation_enabled={}, disabled={}",
            config.translation_enabled,
            config.disabled
        );
    }
}
