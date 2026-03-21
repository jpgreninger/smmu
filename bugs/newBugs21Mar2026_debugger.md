  Debugger Audit — ARM IHI0070G.b
  (Findings from post-21Mar2026_12am fixes pass)

  Summary Table

  ┌──────────────────────────────────────────────────────────┬──────────────────────┬───────────┐
  │ Bug                                                       │ Verdict              │ Priority  │
  ├──────────────────────────────────────────────────────────┼──────────────────────┼───────────┤
  │ BUG-CPP-1 — Constructor split-brain (queue LOG2SIZEs)    │ NON-CONFORMANT       │ Critical  │
  ├──────────────────────────────────────────────────────────┼──────────────────────┼───────────┤
  │ BUG-CPP-5 — C_BAD_STE/STREAMID/CD have rnw=1 (RES0)     │ NON-CONFORMANT       │ Should Fix│
  ├──────────────────────────────────────────────────────────┼──────────────────────┼───────────┤
  │ BUG-RUST-A — nsipa=false hardcoded (S1DSS=1 + S2 path)  │ NON-CONFORMANT       │ Critical  │
  ├──────────────────────────────────────────────────────────┼──────────────────────┼───────────┤
  │ BUG-RUST-B — stage-2 translate_page uses AccessType::Read│ SPEC SILENT / FIXED  │ N/A       │
  ├──────────────────────────────────────────────────────────┼──────────────────────┼───────────┤
  │ BUG-RUST-I — map_range/map_pages miss .with_access_flag  │ NON-CONFORMANT       │ Should Fix│
  └──────────────────────────────────────────────────────────┴──────────────────────┴───────────┘

  ---
  BUG-CPP-1 — Constructor split-brain (§3.5.1)

  In SMMU(const SMMUConfiguration& config), cmdqLog2Size / eventqLog2Size / priqLog2Size and
  maxEventQueueSize / maxCommandQueueSize / maxPRIQueueSize are initialized from `config` in the
  initializer list.  If config.isValid() returns false, the body replaces `configuration` with the
  default, but the already-initialized queue members keep the (possibly invalid) config values.
  Result: queue LOG2SIZE and capacity are inconsistent with `configuration`.

  Fix: After the fallback assignment, recompute all six queue members from `configuration`.

  ---
  BUG-CPP-5 — C_BAD_* rnw/ind/pnu are RES0 (§7.3.1)

  ARM IHI0070G.b §7.3.1 — For configuration-class events (C_BAD_STREAMID, C_BAD_STE,
  C_BAD_SUBSTREAMID, C_BAD_CD, F_CFG_CONFLICT), the RnW, InD, and PnU fields are RES0 (must be 0).
  Currently generateEvent() runs the accessType switch for ALL event types, setting rnw=1
  (from the default AccessType::Read) for config events.

  Fix: After the accessType switch (both stall-pending path and normal path), add:
    if event is a config class, force rnw=false, ind=false, pnu=false.

  ---
  BUG-RUST-A — nsipa=false hardcoded on S1DSS=0b01 + stage-2 fault path (§7.3)

  smmu/mod.rs S1DSS=0b01 + stream_stage2_enabled fault path (line ~3847):
    self.record_translation_fault(
        stream_id, pasid, iova, access, security_state,
        e, false, 0u16, true, iova.as_u64(), pnu, false,   // ← last arg is nsipa
    );

  nsipa should be `security_state == SecurityState::NonSecure` when fault_s2=true (§7.3 NSIPA rule).
  Hardcoding false means all S1DSS=1 stage-2 faults on NonSecure streams report nsipa=0 instead of 1.

  Fix: Replace the literal `false` with a computed `nsipa` variable.

  ---
  BUG-RUST-B — Already fixed (noted for completeness)

  translate_two_stage() calls stage2.translate_page(ipa, AccessType::Read, ...) for the PTW step.
  This is correct per §7.3.16 ("CLASS == TT, access is implicitly Data and a read").
  The actual permission check uses access_type in the S1∩S2 intersection below.
  translate_and_get_stage2_ipa() uses translate_two_stage_with_ipa() for two-stage (not translate_two_stage()),
  so the s2/ipa fields are correctly populated. No bug here.

  ---
  BUG-RUST-I — map_range / map_pages / map_pages_batched miss access flag (§3.13.2)

  map_page() correctly calls .with_access_flag(true).
  map_range() at line ~803, map_pages() at line ~923, and map_pages_batched() at line ~1215
  create PageEntry::with_security_state(...) WITHOUT .with_access_flag(true).
  Any page mapped via these bulk APIs has AF=0, causing spurious F_ACCESS faults
  when ha=false and affd=false (the default StreamConfig).

  Fix: Add .with_access_flag(true) to all three sites, matching map_page().
