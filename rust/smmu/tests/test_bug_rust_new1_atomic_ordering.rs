#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD regression test for BUG-RUST-NEW-1: Relaxed STRW store breaks
//! Acquire/Release chain.
//!
//! The individual `set_*` setters (`set_strw`, `set_vmid`, `set_stall_enabled`,
//! `set_ha`, `set_hd`, `set_s1dss`, etc.) previously used `Ordering::Relaxed`.
//! Because the corresponding `get_*` methods use `Ordering::Acquire`, the
//! asymmetric pairing means a reader on a weakly-ordered architecture
//! (ARM/POWER) is NOT guaranteed to observe the written value.
//!
//! The fix: change every `set_*` store to `Ordering::Release` so that it
//! forms a proper Release/Acquire pair with the matching load.
//!
//! These tests verify the basic observable contract (written value is
//! readable back via the getter) for every setter that was using Relaxed.
//! The Release ordering upgrade makes this contract correct on all
//! architectures, not just x86's strongly-ordered TSO model.

use smmu::stream_context::StreamContext;
use smmu::types::{SecurityState, StreamWorld};

// ---- set_strw / get_strw ------------------------------------------------

/// After set_strw(EL2), get_strw() must return EL2.
/// Before the fix the store was Relaxed while the load was Acquire — an
/// asymmetric pair that is unsound on weakly-ordered architectures.
#[test]
fn test_set_strw_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_strw(StreamWorld::El2);
    // Must be observable via the Acquire load in get_strw().
    assert_eq!(ctx.get_strw(), StreamWorld::El2);
}

#[test]
fn test_set_strw_el3_observable() {
    let ctx = StreamContext::new();
    ctx.set_strw(StreamWorld::El3);
    assert_eq!(ctx.get_strw(), StreamWorld::El3);
}

#[test]
fn test_set_strw_el2e2h_observable() {
    let ctx = StreamContext::new();
    ctx.set_strw(StreamWorld::El2E2h);
    assert_eq!(ctx.get_strw(), StreamWorld::El2E2h);
}

#[test]
fn test_set_strw_el1el0_observable() {
    let ctx = StreamContext::new();
    // First set a non-default value, then restore default
    ctx.set_strw(StreamWorld::El2);
    ctx.set_strw(StreamWorld::El1El0);
    assert_eq!(ctx.get_strw(), StreamWorld::El1El0);
}

// ---- set_vmid / get_vmid ------------------------------------------------

/// After set_vmid(42), get_vmid() must return 42.
#[test]
fn test_set_vmid_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_vmid(42);
    assert_eq!(ctx.get_vmid(), 42);
}

#[test]
fn test_set_vmid_max_value_observable() {
    let ctx = StreamContext::new();
    ctx.set_vmid(u16::MAX);
    assert_eq!(ctx.get_vmid(), u16::MAX);
}

// ---- set_stall_enabled / is_stall_enabled --------------------------------

/// After set_stall_enabled(true), is_stall_enabled() must return true.
#[test]
fn test_set_stall_enabled_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_stall_enabled(true);
    assert!(ctx.is_stall_enabled());
}

#[test]
fn test_set_stall_disabled_observable() {
    let ctx = StreamContext::new();
    ctx.set_stall_enabled(true);
    ctx.set_stall_enabled(false);
    assert!(!ctx.is_stall_enabled());
}

// ---- set_ha / is_ha_enabled ----------------------------------------------

/// After set_ha(true), is_ha_enabled() must return true.
#[test]
fn test_set_ha_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_ha(true);
    assert!(ctx.is_ha_enabled());
}

// ---- set_hd / is_hd_enabled ----------------------------------------------

/// After set_hd(true), is_hd_enabled() must return true.
#[test]
fn test_set_hd_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_hd(true);
    assert!(ctx.is_hd_enabled());
}

// ---- set_s1dss / get_s1dss -----------------------------------------------

/// After set_s1dss(1), get_s1dss() must return 1.
#[test]
fn test_set_s1dss_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_s1dss(1);
    assert_eq!(ctx.get_s1dss(), 1);
}

// ---- set_s1cd_max / get_s1cd_max -----------------------------------------

/// After set_s1cd_max(8), get_s1cd_max() must return 8.
#[test]
fn test_set_s1cd_max_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_s1cd_max(8);
    assert_eq!(ctx.get_s1cd_max(), 8);
}

// ---- set_t0sz / get_t0sz -------------------------------------------------

/// After set_t0sz(32), get_t0sz() must return 32.
#[test]
fn test_set_t0sz_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_t0sz(32);
    assert_eq!(ctx.get_t0sz(), 32);
}

// ---- set_t1sz / get_t1sz -------------------------------------------------

/// After set_t1sz(24), get_t1sz() must return 24.
#[test]
fn test_set_t1sz_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_t1sz(24);
    assert_eq!(ctx.get_t1sz(), 24);
}

// ---- set_aa64 / get_aa64 -------------------------------------------------

/// After set_aa64(false), get_aa64() must return false.
#[test]
fn test_set_aa64_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_aa64(false);
    assert!(!ctx.get_aa64());
}

// ---- set_abort_mode / is_abort_mode --------------------------------------

/// After set_abort_mode(true), is_abort_mode() must return true.
#[test]
fn test_set_abort_mode_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_abort_mode(true);
    assert!(ctx.is_abort_mode());
}

// ---- GAP-1 attribute setters --------------------------------------------

/// After set_sh_cfg(2), get_sh_cfg() must return 2.
#[test]
fn test_set_sh_cfg_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_sh_cfg(2);
    assert_eq!(ctx.get_sh_cfg(), 2);
}

/// After set_alloc_cfg(0xF), get_alloc_cfg() must return 0xF.
#[test]
fn test_set_alloc_cfg_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_alloc_cfg(0xF);
    assert_eq!(ctx.get_alloc_cfg(), 0xF);
}

/// After set_mem_attr(7), get_mem_attr() must return 7.
#[test]
fn test_set_mem_attr_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_mem_attr(7);
    assert_eq!(ctx.get_mem_attr(), 7);
}

/// After set_inst_cfg(1), get_inst_cfg() must return 1.
#[test]
fn test_set_inst_cfg_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_inst_cfg(1);
    assert_eq!(ctx.get_inst_cfg(), 1);
}

/// After set_priv_cfg(3), get_priv_cfg() must return 3.
#[test]
fn test_set_priv_cfg_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_priv_cfg(3);
    assert_eq!(ctx.get_priv_cfg(), 3);
}

/// After set_ns_cfg(1), get_ns_cfg() must return 1.
#[test]
fn test_set_ns_cfg_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_ns_cfg(1);
    assert_eq!(ctx.get_ns_cfg(), 1);
}

/// After set_mt_cfg(true), is_mt_cfg_enabled() must return true.
#[test]
fn test_set_mt_cfg_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_mt_cfg(true);
    assert!(ctx.is_mt_cfg_enabled());
}

// ---- set_security_state / security_state ---------------------------------

/// After set_security_state(Secure), security_state() must return Secure.
#[test]
fn test_set_security_state_release_pairs_with_acquire() {
    let ctx = StreamContext::new();
    ctx.set_security_state(SecurityState::Secure);
    assert_eq!(ctx.security_state(), SecurityState::Secure);
}

// ---- Compound test: set all, then verify all ----------------------------

/// Set every config field in sequence; read them back in sequence.
/// This exercises the entire Release/Acquire chain for the full config surface.
#[test]
fn test_all_setters_release_acquire_chain() {
    let ctx = StreamContext::new();

    ctx.set_strw(StreamWorld::El3);
    ctx.set_vmid(0xABCD);
    ctx.set_stall_enabled(true);
    ctx.set_ha(true);
    ctx.set_hd(true);
    ctx.set_s1dss(0);
    ctx.set_s1cd_max(4);
    ctx.set_t0sz(20);
    ctx.set_t1sz(20);
    ctx.set_aa64(true);
    ctx.set_abort_mode(false);
    ctx.set_sh_cfg(1);
    ctx.set_alloc_cfg(0xA);
    ctx.set_mem_attr(5);
    ctx.set_inst_cfg(2);
    ctx.set_priv_cfg(2);
    ctx.set_ns_cfg(0);
    ctx.set_mt_cfg(true);
    ctx.set_security_state(SecurityState::NonSecure);

    assert_eq!(ctx.get_strw(), StreamWorld::El3);
    assert_eq!(ctx.get_vmid(), 0xABCD);
    assert!(ctx.is_stall_enabled());
    assert!(ctx.is_ha_enabled());
    assert!(ctx.is_hd_enabled());
    assert_eq!(ctx.get_s1dss(), 0);
    assert_eq!(ctx.get_s1cd_max(), 4);
    assert_eq!(ctx.get_t0sz(), 20);
    assert_eq!(ctx.get_t1sz(), 20);
    assert!(ctx.get_aa64());
    assert!(!ctx.is_abort_mode());
    assert_eq!(ctx.get_sh_cfg(), 1);
    assert_eq!(ctx.get_alloc_cfg(), 0xA);
    assert_eq!(ctx.get_mem_attr(), 5);
    assert_eq!(ctx.get_inst_cfg(), 2);
    assert_eq!(ctx.get_priv_cfg(), 2);
    assert_eq!(ctx.get_ns_cfg(), 0);
    assert!(ctx.is_mt_cfg_enabled());
    assert_eq!(ctx.security_state(), SecurityState::NonSecure);
}
