#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]

//! TDD tests for three ARM SMMU v3 conformance gaps:
//!
//! - GAP-E: §3.4.1/§5.4 CD.TBI top-byte-ignore before T0SZ range check
//! - GAP-F: §5.4/§3.4 CD.IPS per-CD stage-1 output IPA size bound
//! - GAP-H: §3.13.6 PRIQ overflow auto-PRG failure response

use smmu::types::{
    AccessType, FaultMode, PagePermissions, PRIEntry, QueueConfig, SMMUConfig, SecurityState,
    StreamConfig, StreamID, IOVA, PA, PASID,
};
use smmu::SMMU;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_smmu() -> SMMU {
    let s = SMMU::new();
    s.enable().unwrap();
    s
}

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

// ─────────────────────────────────────────────────────────────────────────────
// GAP-E: §3.4.1/§5.4 — CD.TBI top-byte-ignore
// ─────────────────────────────────────────────────────────────────────────────

/// GAP-E Test 1: TBI=1, T0SZ=16 — IOVA with tag byte set but masked address
/// within 48-bit range must SUCCEED.
///
/// ARM IHI0070G.b §3.4.1: "When TBI is 1, the top byte of the input address
/// is not used in address range checks."  With T0SZ=16 the VA limit is 2^48.
/// IOVA=0xFF00_1000_0000_0000 has tag=0xFF; masked=0x0000_1000_0000_0000 < 2^48.
///
/// BEFORE FIX: Translation fails because 0xFF00_1000_0000_0000 >= 2^48.
/// AFTER FIX:  Translation succeeds (or fails with PageNotMapped — not a range
///             error) because masked address is within the valid VA range.
#[test]
fn gap_e_tbi_1_top_byte_masked_within_range() {
    let smmu = make_smmu();

    // T0SZ=16 → 48-bit VA space → limit = 2^48
    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16)
        .tbi(true) // GAP-E: top-byte-ignore enabled
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE1), cfg).unwrap();
    smmu.create_pasid(sid(0xE1), pasid(0)).unwrap();

    // Map the page at the masked address (top byte stripped → 0x0000_1000_0000_0000)
    let masked_addr: u64 = 0x0000_1000_0000_0000;
    smmu.map_page(
        sid(0xE1),
        pasid(0),
        iova(masked_addr),
        pa(0x8000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Present IOVA with a tag byte in bits[63:56]; masked address is within range
    let tagged_iova: u64 = 0xFF00_1000_0000_0000;
    let result = smmu.translate(
        sid(0xE1),
        pasid(0),
        iova(tagged_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    // The range check must pass (TBI masks the top byte).
    // The translation itself may fail with PageNotMapped because the page is
    // mapped at the masked address but the SMMU translates using the original
    // address internally — this is acceptable: what matters is that
    // VaRangeExceeded is NOT returned.
    assert!(
        !matches!(result, Err(smmu::types::TranslationError::VaRangeExceeded)),
        "GAP-E: TBI=1 with tagged IOVA={tagged_iova:#018x} must NOT fail with \
         VaRangeExceeded (top byte is a tag, not part of the range check); got {result:?}"
    );
}

/// GAP-E Test 2: TBI=0, T0SZ=16 — same IOVA with top byte set must FAIL with
/// VaRangeExceeded.
///
/// Without TBI the full IOVA participates in the range check.
/// 0xFF00_1000_0000_0000 >= 2^48 → F_TRANSLATION / VaRangeExceeded.
///
/// BEFORE FIX: (baseline — was already failing correctly without TBI gap fix)
/// AFTER FIX:  Must still fail (regression guard for TBI=0 path).
#[test]
fn gap_e_tbi_0_full_iova_checked() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16)
        .tbi(false) // TBI disabled — full IOVA checked
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE2), cfg).unwrap();
    smmu.create_pasid(sid(0xE2), pasid(0)).unwrap();

    let tagged_iova: u64 = 0xFF00_1000_0000_0000;
    let result = smmu.translate(
        sid(0xE2),
        pasid(0),
        iova(tagged_iova),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(smmu::types::TranslationError::VaRangeExceeded)),
        "GAP-E: TBI=0, full IOVA={tagged_iova:#018x} >= 2^48 must return \
         VaRangeExceeded; got {result:?}"
    );
}

/// GAP-E Test 3: TBI=1, T0SZ=16 — masked address still exceeds 2^48 → VaRangeExceeded.
///
/// IOVA=0xFF01_0000_0000_0000; masked=0x0001_0000_0000_0000 = 2^48 (exactly at limit).
/// With TBI=1 the mask is applied: masked >= 2^48 → still out of range.
///
/// BEFORE FIX: Would fail with VaRangeExceeded for the wrong reason (unmasked full value).
/// AFTER FIX:  Must still fail (the masked address is also >= the limit).
#[test]
fn gap_e_tbi_1_masked_still_exceeds() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .t0sz(16)
        .tbi(true)
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xE3), cfg).unwrap();
    smmu.create_pasid(sid(0xE3), pasid(0)).unwrap();

    // masked = 0x0001_0000_0000_0000 = 2^48, which equals the limit — out of range
    let iova_val: u64 = 0xFF01_0000_0000_0000;
    let result = smmu.translate(
        sid(0xE3),
        pasid(0),
        iova(iova_val),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(smmu::types::TranslationError::VaRangeExceeded)),
        "GAP-E: TBI=1, masked IOVA 0x0001_0000_0000_0000 == 2^48 (at limit) \
         must return VaRangeExceeded; got {result:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-F: §5.4/§3.4 — CD.IPS stage-1 output IPA size bound
// ─────────────────────────────────────────────────────────────────────────────

/// GAP-F Test 1: Two-stage with IPS=0 (32-bit), stage-1 IPA = 2^32 → AddressSizeError.
///
/// ARM IHI0070G.b §5.4/§3.4: CD.IPS bounds the stage-1 output IPA.  With IPS=0
/// (32-bit) any IPA >= 2^32 violates the bound → F_ADDR_SIZE.
///
/// BEFORE FIX: IPA bound not checked; translation continues (wrong behaviour).
/// AFTER FIX:  AddressSizeError returned.
#[test]
fn gap_f_ips_32bit_ipa_overflow_f_addr_size() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .ips(0) // GAP-F: 32-bit IPS → IPA must be < 2^32
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF1), cfg).unwrap();
    smmu.create_pasid(sid(0xF1), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xF1)).unwrap();

    // Map stage-1: IOVA → IPA at 2^32 (exactly at the 32-bit limit → out of range)
    let ipa_at_limit: u64 = 1u64 << 32; // 4 GiB — out of 32-bit range
    smmu.map_page(
        sid(0xF1),
        pasid(0),
        iova(0x1000),
        pa(ipa_at_limit),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF1),
        pasid(0),
        iova(0x1000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        matches!(result, Err(smmu::types::TranslationError::AddressSizeError)),
        "GAP-F: IPS=0 (32-bit), IPA={ipa_at_limit:#x} >= 2^32 must return \
         AddressSizeError (F_ADDR_SIZE); got {result:?}"
    );
}

/// GAP-F Test 2: Two-stage with IPS=5 (48-bit), IPA well within range → success.
///
/// With IPS=5 (48-bit) an IPA of 0x1000 is far below 2^48.  Both stage-1 and
/// stage-2 succeed; translation completes normally.
///
/// BEFORE FIX: Passes (no IPS check, so always succeeded for in-range addresses).
/// AFTER FIX:  Must still pass (regression guard).
#[test]
fn gap_f_ips_48bit_within_succeeds() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .ips(5) // 48-bit IPS
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF2), cfg).unwrap();
    smmu.create_pasid(sid(0xF2), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xF2)).unwrap();

    // Stage-1: IOVA 0x2000 → IPA 0x3000 (well within 48-bit range)
    let ipa_val: u64 = 0x3000;
    smmu.map_page(
        sid(0xF2),
        pasid(0),
        iova(0x2000),
        pa(ipa_val),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: IPA 0x3000 → PA 0x4000
    smmu.map_stage2_page(
        sid(0xF2),
        iova(ipa_val),
        pa(0x4000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF2),
        pasid(0),
        iova(0x2000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "GAP-F: IPS=5 (48-bit), IPA={ipa_val:#x} well within range must succeed; \
         got {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x4000,
        "GAP-F: expected PA=0x4000 after two-stage translation"
    );
}

/// GAP-F Test 3: Two-stage with IPS=6 (52-bit, no restriction) — large IPA succeeds.
///
/// IPS=6 means 52-bit output IPA.  The `s2ps_to_bits(6)` helper returns 52;
/// the code path skips the bound check (52 is not < 52).  A large IPA such as
/// 2^40 must succeed (assuming it is also within the S2T0SZ range).
///
/// BEFORE FIX: Passes (no IPS check).
/// AFTER FIX:  Must still pass (IPS=6 has no effective bound on IPAs < 2^52).
#[test]
fn gap_f_ips_52bit_no_restriction() {
    let smmu = make_smmu();

    let cfg = StreamConfig::builder()
        .translation_enabled(true)
        .stage1_enabled(true)
        .stage2_enabled(true)
        .pasid_enabled(true)
        .fault_mode(FaultMode::Terminate)
        .ips(6)     // 52-bit IPS — no additional IPA bound below 2^52
        .s2_t0sz(0) // no S2T0SZ restriction either
        .s2_ps(6)   // S2PS=6 (52-bit) so stage-2 PA check also passes
        .build()
        .unwrap();
    smmu.configure_stream(sid(0xF3), cfg).unwrap();
    smmu.create_pasid(sid(0xF3), pasid(0)).unwrap();
    smmu.create_stage2_address_space(sid(0xF3)).unwrap();

    // Stage-1: IOVA 0x5000 → large IPA (2^40)
    let large_ipa: u64 = 1u64 << 40;
    smmu.map_page(
        sid(0xF3),
        pasid(0),
        iova(0x5000),
        pa(large_ipa),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    // Stage-2: large IPA → PA 0x6000
    smmu.map_stage2_page(
        sid(0xF3),
        iova(large_ipa),
        pa(0x6000),
        PagePermissions::read_write(),
        SecurityState::NonSecure,
    )
    .unwrap();

    let result = smmu.translate(
        sid(0xF3),
        pasid(0),
        iova(0x5000),
        AccessType::Read,
        SecurityState::NonSecure,
    );

    assert!(
        result.is_ok(),
        "GAP-F: IPS=6 (52-bit), large IPA={large_ipa:#x} must succeed; got {result:?}"
    );
    assert_eq!(
        result.unwrap().physical_address().as_u64(),
        0x6000,
        "GAP-F: expected PA=0x6000 after two-stage translation with IPS=6"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GAP-H: §3.13.6 — PRIQ overflow auto-PRG failure response
// ─────────────────────────────────────────────────────────────────────────────

/// Build an SMMU with a tiny PRI queue (capacity=4) for overflow testing.
fn make_smmu_small_priq() -> SMMU {
    let config = SMMUConfig {
        queue_config: QueueConfig {
            event_queue_size: 16,
            command_queue_size: 16,
            pri_queue_size: 4, // small — triggers overflow quickly
        },
        cache_config: smmu::types::CacheConfig {
            tlb_cache_size: 64,
            cache_max_age_ms: 5000,
            enable_caching: true,
        },
        address_config: smmu::types::AddressConfig::default(),
        resource_limits: smmu::types::ResourceLimits::default(),
    };
    let s = SMMU::with_config(config);
    s.enable().unwrap();
    s
}

/// GAP-H Test 1: fill PRIQ to capacity, then overflow → auto-failure response present.
///
/// ARM IHI0070G.b §3.13.6: When the PRIQ is full and a new page request cannot
/// be enqueued, the SMMU must automatically generate a PRG_RESPONSE with
/// RESPONSE=FAILURE.
///
/// BEFORE FIX: Overflow is silent — device can stall indefinitely.
/// AFTER FIX:  Overflowing request appears in get_pri_auto_failure_responses().
#[test]
fn gap_h_priq_overflow_generates_auto_failure_response() {
    let smmu = make_smmu_small_priq();

    // Fill the queue (capacity=4) to the brim
    for i in 0..4u32 {
        let req = PRIEntry::new(i, 0);
        smmu.submit_page_request(req).expect("queue not yet full");
    }

    // One more request — PRIQ is now full → auto-failure response.
    // BUG-NEW-12: auto-failure PRG_RESPONSE is generated ONLY when is_last_request=true
    // (ARM §8.1).  Set is_last_request=true so the device gets a FAILURE response.
    let mut overflow_req = PRIEntry::new(99, 0).with_prg_index(7);
    overflow_req.is_last_request = true;
    let result = smmu.submit_page_request(overflow_req);
    assert!(
        matches!(result, Err(smmu::types::SMMUError::PriQueueFull)),
        "GAP-H: submit_page_request on full PRIQ must return PriQueueFull; got {result:?}"
    );

    // The auto-failure responses must contain the overflowing entry
    let auto_failures = smmu.get_pri_auto_failure_responses();
    assert_eq!(
        auto_failures.len(),
        1,
        "GAP-H: exactly one auto-failure response expected after one overflow; \
         got {} responses",
        auto_failures.len()
    );
    assert_eq!(
        auto_failures[0].entry.stream_id, 99,
        "GAP-H: auto-failure response must carry the stream_id of the overflowing request"
    );
    assert_eq!(
        auto_failures[0].entry.prg_index, 7,
        "GAP-H: auto-failure response must carry the prg_index of the overflowing request"
    );

    // Second call must return empty (responses are drained on first read)
    let second = smmu.get_pri_auto_failure_responses();
    assert!(
        second.is_empty(),
        "GAP-H: get_pri_auto_failure_responses() must drain responses on first call"
    );
}

/// GAP-H Test 2: no overflow → no auto-failure response.
///
/// When requests fit in the queue, get_pri_auto_failure_responses() returns empty.
///
/// BEFORE FIX: (baseline — always returned empty, no auto-response mechanism existed)
/// AFTER FIX:  Must still return empty when there is no overflow (regression guard).
#[test]
fn gap_h_priq_normal_no_auto_response() {
    let smmu = make_smmu_small_priq();

    // Submit one request — well within capacity of 4
    let req = PRIEntry::new(1, 0);
    smmu.submit_page_request(req).unwrap();

    let auto_failures = smmu.get_pri_auto_failure_responses();
    assert!(
        auto_failures.is_empty(),
        "GAP-H: no overflow occurred — auto-failure responses must be empty; \
         got {} entries",
        auto_failures.len()
    );
}
