//! ARM SMMU v3 FINDING-NEW-24 spec tests
//!
//! Tests for `StreamConfigBuilder` missing `s1dss` and `s1cd_max` setter methods.
//! ARM §5.2 (STE.S1DSS, STE.S1CDMax)

use smmu::types::{FaultMode, StreamConfigBuilder};

// ── FINDING-NEW-24: StreamConfigBuilder must have .s1dss() and .s1cd_max() setters ──

/// The builder must expose a `.s1dss()` setter that propagates to the built config.
#[test]
fn builder_s1dss_setter_propagates() {
    let cfg = StreamConfigBuilder::new()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1dss(0x01)
        .build()
        .expect("build must succeed");
    assert_eq!(cfg.s1dss, 0x01, "s1dss must be 0x01 as set via builder");
}

/// The builder must expose a `.s1cd_max()` setter that propagates to the built config.
#[test]
fn builder_s1cd_max_setter_propagates() {
    let cfg = StreamConfigBuilder::new()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1cd_max(4)
        .build()
        .expect("build must succeed");
    assert_eq!(cfg.s1cd_max, 4, "s1cd_max must be 4 as set via builder");
}

/// Both setters together: verify independent propagation.
#[test]
fn builder_s1dss_and_s1cd_max_both_propagate() {
    let cfg = StreamConfigBuilder::new()
        .translation_enabled(true)
        .stage1_enabled(true)
        .s1dss(0x00)
        .s1cd_max(8)
        .build()
        .expect("build must succeed");
    assert_eq!(cfg.s1dss, 0x00);
    assert_eq!(cfg.s1cd_max, 8);
}

/// Default values are preserved when setters are not called.
#[test]
fn builder_s1dss_s1cd_max_defaults_when_not_set() {
    let cfg = StreamConfigBuilder::new()
        .build()
        .expect("build must succeed");
    assert_eq!(cfg.s1dss, 2, "default s1dss must be 0b10");
    assert_eq!(cfg.s1cd_max, 0, "default s1cd_max must be 0");
}

/// Setter chaining works: later call overrides earlier call.
#[test]
fn builder_s1dss_setter_is_chainable() {
    let cfg = StreamConfigBuilder::new()
        .s1dss(0x00)
        .s1dss(0x02) // override
        .build()
        .expect("build must succeed");
    assert_eq!(cfg.s1dss, 0x02, "second .s1dss() call must override first");
}

/// The `fault_mode` setter still works alongside the new setters (regression guard).
#[test]
fn builder_new_setters_do_not_break_existing_api() {
    let cfg = StreamConfigBuilder::new()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .s1dss(0x01)
        .s1cd_max(2)
        .build()
        .expect("build must succeed");
    assert!(cfg.translation_enabled);
    assert!(cfg.stage1_enabled);
    assert_eq!(cfg.s1dss, 0x01);
    assert_eq!(cfg.s1cd_max, 2);
}
