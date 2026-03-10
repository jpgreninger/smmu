#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

//! Tests for three new bugs in the Rust ARM SMMU v3 implementation.
//!
//! BUG-NEW-03: `CommandEntry` missing `security_state` field — command error events
//!   hardcode `SecurityState::NonSecure` instead of propagating the command's actual
//!   security state. (ARM §7.3 / §3.10.2.1)
//!
//! BUG-NEW-04: `StreamContext::map_page` rejects disabled streams — the enabled check
//!   belongs only in `translate()`, not in page-table setup. (ARM §3.4 / §5.2)
//!
//! BUG-NEW-09: Bypass OAS violation is cached in TLB before the OAS check fires —
//!   subsequent translations of the same out-of-range IOVA succeed from cache instead
//!   of failing. (ARM §3.4 / §3.16)

// ─── BUG-NEW-03 ───────────────────────────────────────────────────────────────

/// §7.3 / §3.10.2.1 / BUG-NEW-03:
/// When `CMD_CFGI_STE` targets an unknown `stream_id`, the resulting `C_BAD_STREAMID`
/// event must carry the security state of the originating command, not a hardcoded
/// `SecurityState::NonSecure`.
#[test]
fn test_cfgi_ste_bad_stream_event_carries_command_security_state() {
    use smmu::types::{CommandEntry, CommandType, EventType, SecurityState, StreamID};
    use smmu::SMMU;

    let smmu = SMMU::new();
    smmu.enable().unwrap();
    // §6.3.12: set RECINVSID so C_BAD_STREAMID events from CMD_CFGI_STE are recorded.
    smmu.set_cr2(SMMU::CR2_RECINVSID);

    // Build a CfgiSte command with a non-default security state (SecureEL2 == Secure)
    // targeting a stream that has never been configured.
    let unknown_sid = StreamID::new(0xDEAD).unwrap();
    let cmd = CommandEntry::new(CommandType::CfgiSte, unknown_sid.as_u32(), 0)
        .with_security_state(SecurityState::Secure);

    smmu.submit_command(cmd).unwrap();
    // Processing will fail with C_BAD_STREAMID (unknown stream)
    let _ = smmu.process_command_queue();

    let events = smmu.get_events_by_type(EventType::CBadStreamid);
    assert!(
        !events.is_empty(),
        "expected a C_BAD_STREAMID event for unknown stream"
    );
    assert_eq!(
        events[0].security_state,
        SecurityState::Secure,
        "BUG-NEW-03: C_BAD_STREAMID event must carry the command's security_state \
         (Secure), not a hardcoded NonSecure"
    );
}

// ─── BUG-NEW-04 ───────────────────────────────────────────────────────────────

/// §3.4 / §5.2 / BUG-NEW-04:
/// `StreamContext::map_page` must succeed when the stream context is not yet
/// enabled (i.e., the stream exists, has PASIDs, but `is_enabled()` returns false
/// because `enabled` is stored as false).
///
/// The `is_enabled()` guard was incorrectly present in `map_page()`.  The ARM spec
/// places no restriction on page-table setup based on stream enable state; the guard
/// belongs only in `translate()`.
///
/// Because `StreamContext::disable()` also clears the PASID map, we cannot test
/// the full scenario by calling `disable()`.  Instead we verify the correct
/// boundary:
/// - `translate()` rejects a stream in abort mode (`StreamDisabled`).
/// - `map_page()` on the same stream succeeds — the `is_enabled()` guard has been
///   removed, so the PASID-level address space operation is no longer blocked by
///   abort-mode/disabled state.
#[test]
fn test_map_page_succeeds_when_stream_disabled() {
    use smmu::stream_context::StreamContext;
    use smmu::types::{AccessType, PagePermissions, SecurityState, TranslationError, IOVA, PA, PASID};

    let ctx = StreamContext::new();
    let pasid = PASID::new(0).unwrap();
    ctx.create_pasid(pasid).unwrap();

    // Set the stream to abort/disabled mode via set_abort_mode(true), which
    // causes translate() to return StreamDisabled.  The old bug added an
    // is_enabled() guard to map_page() that was also triggered after
    // disable() — that guard has been removed.
    ctx.set_abort_mode(true);

    let iova = IOVA::new(0x1000).unwrap();
    let pa = PA::new(0x2000).unwrap();
    let perms = PagePermissions::read_write();

    // map_page must succeed even when the stream context is in abort mode.
    // Page-table setup is independent of stream enable/disable state (ARM §3.4 / §5.2).
    let result = ctx.map_page(pasid, iova, pa, perms, SecurityState::NonSecure);
    assert!(
        result.is_ok(),
        "BUG-NEW-04: map_page must succeed on a stream in abort mode; got: {result:?}"
    );

    // translate() must still reject an abort-mode stream.
    // BUG-R-05 fix: abort mode now returns AbortMode (distinct from StreamDisabled).
    let translate_result = ctx.translate(pasid, iova, AccessType::Read, SecurityState::NonSecure);
    assert!(
        matches!(translate_result, Err(TranslationError::AbortMode)),
        "translate() must return AbortMode (§5.2 silent termination) for an abort-mode stream; got: {translate_result:?}"
    );
}

// ─── BUG-NEW-09 ───────────────────────────────────────────────────────────────

/// §3.4 / §3.16 / BUG-NEW-09:
/// When a bypass-mode stream translates an IOVA that violates OAS, the error must
/// NOT be cached in the TLB. A second translation of the same IOVA must also fail,
/// not succeed from a stale cache entry.
#[test]
fn test_bypass_oas_violation_not_cached_in_tlb() {
    use smmu::types::{
        AccessType, AddressConfig, SecurityState, SMMUConfig, StreamConfig, StreamID,
        TranslationError, IOVA, PASID,
    };
    use smmu::SMMU;

    // Configure SMMU with max_pa_bits = 36 (OAS = 36 bits = 64 GiB).
    let address_config = AddressConfig {
        max_iova_bits: 48,
        max_pa_bits: 36,
        max_stream_count: 65_536,
        max_pasid_count: 1_048_576,
    };
    let config = SMMUConfig::builder()
        .address_config(address_config)
        .build()
        .unwrap();

    let smmu = SMMU::with_config(config);
    smmu.enable().unwrap();

    // Configure a stream in bypass mode (both stages disabled, identity mapping).
    let stream_id = StreamID::new(1).unwrap();
    smmu.configure_stream(stream_id, StreamConfig::bypass()).unwrap();

    let pasid = PASID::new(0).unwrap();
    // IOVA = 2^36 — one bit beyond the 36-bit OAS boundary.
    let out_of_range_iova = IOVA::new(1u64 << 36).unwrap();

    // First translation: must fail with AddressSizeError.
    let result1 = smmu.translate(
        stream_id,
        pasid,
        out_of_range_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        matches!(result1, Err(TranslationError::AddressSizeError)),
        "first bypass OAS-violation translation must fail; got: {result1:?}"
    );

    // Second translation of the SAME IOVA: must ALSO fail.
    // If the bug is present the first Ok result was cached, and this returns Ok.
    let result2 = smmu.translate(
        stream_id,
        pasid,
        out_of_range_iova,
        AccessType::Read,
        SecurityState::NonSecure,
    );
    assert!(
        matches!(result2, Err(TranslationError::AddressSizeError)),
        "BUG-NEW-09: second bypass OAS-violation translation must still fail (not a cached hit); \
         got: {result2:?}"
    );
}
