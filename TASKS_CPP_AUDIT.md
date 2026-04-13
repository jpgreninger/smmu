# TASKS_CPP_AUDIT.md — C++ Implementation Audit Checklist

## Purpose

Targeted audit list for the C++ SMMU implementation. During the conformance audit
(TASKS_BUGS.md), many sections were audited exclusively against the Rust implementation,
leaving the C++ either marked `⚠️` (audited in an earlier pass but not re-verified after
new Rust-side findings) or `N/A` (meaning "not checked" rather than "not applicable").

This file tracks every section requiring a fresh C++ audit, grouped by priority.

**Source of truth**: `TASKS_BUGS.md` — cross-reference that file for full spec notes and
prior bug history. This file tracks only C++-specific work remaining.

**C++ test baseline**: 185/185 passing. No new C++ tests were added during the §7/§8/§13
audit sprints — a clear indicator those sections were not exercised against C++.

---

## Status Legend

| Symbol | Meaning |
|--------|---------|
| ☐ | Not yet audited |
| 🔴 | Bug confirmed in C++ — needs fix |
| ⚠️ | Audit in progress |
| ✅ | C++ verified conformant — no bugs |
| N/A | Genuinely not applicable to C++ (documented reason) |

---

## Priority 1 — Known C++ Bugs (Rust Fix Not Ported)

These sections had bugs **confirmed and fixed in Rust** but the equivalent C++ code was
never updated. The bug is already understood; C++ just needs the same fix + TDD test.

| Section | Title | C++ Status | Known Bug | C++ Location |
|---------|-------|-----------|-----------|--------------|
| §5.2 (INSTCFG) | STE.INSTCFG encoding | ✅ | **BUG-AUDIT-133-CPP FIXED**: Changed all `instCfg == 1u` Force-Instruction branches to `instCfg == 3u` (0b11=Force Instruction per spec). `instCfg == 2u` (0b10=Force Data) was already correct. Fixed at 5 sites: `stream_context.cpp:1147`, `smmu.cpp:490,783,963,1943`. Updated 5 pre-existing tests that encoded Force Instruction as `1u`. TDD test: `test_cpp_audit_p1_instcfg.cpp`. | `cpp/src/stream_context/stream_context.cpp:1147` `cpp/src/smmu/smmu.cpp:490,783,963,1943` |
| §13.1.2 | Attribute support — INSTCFG Write→Data | ✅ | **BUG-13.1.2-CPP-A CONFIRMED NO FIX NEEDED**: C++ INSTCFG block at stream_context.cpp:1153 comment explicitly states "writes are always considered Data" and does NOT convert Write→Execute. The Rust bug (BUG-13.1.2-A) was that Rust DID convert Write→Execute; C++ never did. No code change required. | N/A |
| §13.1.5 | Combine — two-stage MemAttr | ✅ | **BUG-13.1.5-CPP FIXED (both paths)**: (1) `smmu.cpp::performBothStagesTranslation` (~line 2749): added `combinePageAttr()` lambda — Device (0x00) wins over Normal (0xFF) per §13.1.5. (2) `stream_context.cpp::translateUnlocked` two-stage success block (~line 1411): extracted `TranslationData twoStageTd` and combined `s1pa`/`s2pa` before `applyOutputAttrs()`. Also added `mapPageDevice()` API. TDD tests: `test_cpp_audit_p1_memattr.cpp` (smmu.cpp path), `test_cpp_audit_p1_memattr_sc.cpp` (stream_context.cpp path). | `cpp/src/smmu/smmu.cpp:2749` `cpp/src/stream_context/stream_context.cpp:1411` |
| §13.2 | GBPA bypass — Device/NC→OSH | ✅ | **BUG-13.2-CPP FIXED**: After setting `td.shareability = gbpa.shCfg`, enforce Device→OSH: `if (td.memType == 0x00u) td.shareability = 2u` at `smmu.cpp:309`. TDD test: `test_cpp_audit_p1_gbpa_osh.cpp`. | `cpp/src/smmu/smmu.cpp:309` |
| §13.1.4 | Replace — ATOS must skip INSTCFG/PRIVCFG | ✅ | **BUG-13.1.4-CPP-A FIXED**: Added `TransactionType::GatosTranslation` (0b11). `gatosTranslate()` now passes this type to `translate()`. INSTCFG/PRIVCFG blocks in TLB fast path, `performTwoStageTranslation`, and `StreamContext::translateUnlocked` are all guarded with `transactionType != GatosTranslation` / `!isAtos`. TDD test: `test_cpp_audit_p1_atos_bypass.cpp`. | `cpp/src/smmu/smmu.cpp:3747` `cpp/src/stream_context/stream_context.cpp:1147,1174` |

---

## Priority 2 — Sections Audited in Rust Only, C++ Not Checked

These sections received Rust-only TDD test coverage. The equivalent C++ code exists but
was never verified against the findings. May be correct, may have the same bugs.

### §7 — Faults, Errors and Event Queue

| Section | Title | C++ Status | What to Check | C++ Location |
|---------|-------|-----------|---------------|--------------|
| §7.3.1 | Event record merging — stall guard | ✅ | **BUG-7.3.1-CPP FIXED**: Added `!isStall` outer gate and `!existing.stall` inner guard to MEV dedup block in `generateEvent()`. Stall events now always bypass MEV and cannot suppress each other. TDD test: `test_cpp_audit_p2_sec7_mev_stall.cpp` (2 tests). | `cpp/src/smmu/smmu.cpp:5754` |
| §7.3.22 | Event queue record priorities | ✅ | **VERIFIED CONFORMANT**: C++ translate path uses structural early-returns: config-class faults (`C_BAD_STREAMID`, `C_BAD_STE`, `C_BAD_SUBSTREAMID`, `C_BAD_CD`) are checked before calling `StreamContext::translateUnlocked()`, which issues translation-class faults. Priority order is enforced by code structure. No bug found. | `cpp/src/smmu/smmu.cpp` translate path |
| §7.4 | Event queue overflow | ✅ | **VERIFIED CONFORMANT**: C++ `generateEvent()` at `smmu.cpp:5770–5793` correctly handles OVFLG toggle (single transition), stall-drain from `stallPending_`, non-stall discard+OVFLG, and stall redirect to `stallPending_` when full. All 5 §7.4 requirements are met. Already tested by `test_event_queue_stall_overflow_spec.cpp`. | `cpp/src/smmu/smmu.cpp:5770–5800` |
| §7.5 | Global error recording | ✅ | **BUG-GERROR-DPT-CPP FIXED**: Added `GERROR_DPT_ERR = (1u << 10)` to `types.h` after `GERROR_CMDQP_ERR`. Toggle protocol in `signalGerror()` is correct (inactive-only toggle per ARM §6.3.19). TDD test: `test_cpp_audit_p2_sec7_gerror.cpp` (4 tests including static_assert). | `cpp/include/smmu/types.h:1470` |
| §7.5.1 | GERROR interrupt notification | N/A | **N/A DOCUMENTED**: C++ software model has no IRQ notification layer (no `GERROR_IRQEN` constant, no `notifyIrq()`, no IRQ callback infrastructure). `signalGerror()` uses pure register-toggle semantics appropriate for simulation. Adding IRQ-enable gating requires a callback layer that is out of scope. TDD test: `test_cpp_audit_p2_sec7_gerror.cpp` (IrqGatingIsNaForSoftwareModel). | N/A |

### §8 — Page Request Queue

| Section | Title | C++ Status | What to Check | C++ Location |
|---------|-------|-----------|---------------|--------------|
| §8.1 | PRI queue overflow | ✅ | **VERIFIED CONFORMANT**: C++ `submitPageRequest()` at `smmu.cpp:3996–4028` checks overflow-active state (OVFLG≠OVACKFLG) before queue capacity, inhibits new PPRs while overflow is active, toggles OVFLG only on first overflow transition, and emits auto-failure PRG_RESPONSE for `isLastRequest=true`. All §8.1 requirements met. | `cpp/src/smmu/smmu.cpp:3996–4093` |
| §8.1.1 | PRI recovery procedure | N/A | **N/A DOCUMENTED**: C++ model uses a simple deque (`priQueue`) without the Rust `recover_priq_overflow()` recovery procedure. The C++ OVFLG/OVACKFLG mechanism handles overflow state correctly (software acknowledges via `setPriqConsOvackflg()`). The Rust recovery abstraction is a Rust-specific implementation detail — no equivalent function is needed in C++. | N/A |
| §8.2 | PRI miscellaneous | ✅ | **BUG-SEC8-SECURE-PRI-CPP FIXED**: Added Secure stream discard check in `submitPageRequest()` at `smmu.cpp:3993`. Secure PPRs now generate `ResponseCode=0b1111` auto-failure and are not enqueued. PRIQ_ABT_ERR gate verified conformant — `GERROR_PRIQ_ABT_ERR` (bit 3) is defined in `types.h` and initial state is inactive. Corrected pre-existing test `New32Spec.SecurePRIRequest_*` to match §8.2. TDD test: `test_cpp_audit_p2_sec8.cpp` (6 tests). | `cpp/src/smmu/smmu.cpp:3993` |
| §8.3 | PRG Response Message codes | ✅ | **VERIFIED CONFORMANT (pre-existing fix BUG-NEW-A)**: `CMD_PRI_RESP` handler at `smmu.cpp:~4995` rejects `Resp=0b11` with `CERROR_ILL + GERROR_CMDQ_ERR`. TDD regression test: `test_cpp_audit_p2_sec8.cpp::PriRespResp0b11RaisesCerrorIll`. | `cpp/src/smmu/smmu.cpp` CMD_PRI_RESP |

### §13 — Attribute Transformation

| Section | Title | C++ Status | What to Check | C++ Location |
|---------|-------|-----------|---------------|--------------|
| §13.1.7 | Ensuring consistent output attrs | ☐ | **C++ marked N/A** — but `applyOutputAttrs` lambda exists at `stream_context.cpp:1178`. Verify Rule 1 (Device/NC→OSH) is applied in the STE override path, not just GBPA. | `cpp/src/stream_context/stream_context.cpp:1177–1185` |
| §13.3 | STE bypass — output attrs | ☐ | Re-audited for Rust 2026-04-11 and cleared. C++ bypass path (`stream_context.cpp:1192–1197`) calls `applyOutputAttrs()` — verify Device/NC→OSH is enforced there too (it is not currently; see §13.2 bug above). | `cpp/src/stream_context/stream_context.cpp:1192–1197` |
| §13.4 | Normal translation flow | ☐ | Rust cleared ✅ 2026-04-11; C++ left ⚠️. Verify MTCFG applies correctly, NSCFG output is correct, and `apply_output_attrs` covers all subsections. | `cpp/src/stream_context/stream_context.cpp` |
| §13.4.1 | Stage 1 page permissions | ☐ | Rust cleared ✅. Verify C++ WXN/UWXN suppression in `check_permissions()` equivalent. | `cpp/src/stream_context/stream_context.cpp` |
| §13.4.2 | Stage 1 memory attributes | ☐ | Rust cleared ✅ (NSCFG/MAIR lookups). Verify C++ NSCFG applied correctly on stage-1 output. | `cpp/src/stream_context/stream_context.cpp` |
| §13.4.3 | Stage 2 | ☐ | Rust cleared ✅ (S2FWB gated, `combine_mem_type()` used). C++ has no `combine_mem_type()` — this overlaps §13.1.5 bug above. | `cpp/src/stream_context/stream_context.cpp:1354–1391` |
| §13.4.4 | Output | ☐ | Rust cleared ✅ (Device/NC→OSH via `apply_output_attrs`). Verify C++ `applyOutputAttrs` enforces the same. | `cpp/src/stream_context/stream_context.cpp:1177–1185` |
| §13.6 | PCIe and ATS attribute handling | ☐ | Rust cleared ✅ 2026-04-11. C++ left ⚠️. Verify §13.6.3 split-stage ATS path and §13.6.4/13.6.5 S1DSS logic in C++. | `cpp/src/smmu/smmu.cpp` ATS path |
| §13.6.3 | Split-stage ATS behavior | ☐ | Rust fix (BUG-13.6.3-A): ATSCHK=1+EATS=0b10 AtsTranslated must run stage-2-only on IPA. Verify C++ ATSCHK block applies same restriction. | `cpp/src/smmu/smmu.cpp` ATSCHK block |
| §13.7 | PCIe permission attribute interpretation | ☐ | Rust fix (BUG-13.7-A): EATS=0b10 AtsTranslated+ATSCHK=1 must apply `effective_access_type()` before `translate_stage2_only()`. Verify C++ ATS translated path does the same. | `cpp/src/smmu/smmu.cpp` ATS translated path |

---

## Priority 3 — Sections Left at ⚠️ in C++ After Rust Was Cleared to ✅

These sections were audited earlier (C++ bugs fixed at that time) but Rust received
additional re-audit passes that C++ did not. Low risk but should be spot-checked.

| Section | Title | C++ Status | Notes |
|---------|-------|------------|-------|
| §3.4 / §3.4.1 | Address sizes / Input address size | ✅ | BUG-AUDIT-114/115/123 were Rust-only fixes. C++ `AddressSpace::translatePage()` enforces address-size faults via the `inputAddressSizeBits < 52` guard. No C++ bug. Regression test: `P3AddrSize.AddressSizeFaultOnOversizedAddress`. |
| §3.5.1–3.5.4 | Queue semantics | ✅ | OVFLG/modulus/commit semantics verified conformant in Priority 2 (§7.4 ✅ and §8.1 ✅). No additional work needed. |
| §3.13.4–3.13.5 | HTTU behavior summary / Two stages | ✅ | **BUG-AUDIT-130-CPP FIXED**: Stage-2 AF update was missing. `performBothStagesTranslation()` (`smmu.cpp:2745`) and `translateUnlocked()` (`stream_context.cpp:1424`) now call `stage2AddressSpace->updateAccessFlags()` after successful stage-2 translation when `s2ha=true`. Stage-1 HA path was already correct. BUG-AUDIT-129 (S2HD requires S2HA): C++ rejects `S2HD=1` entirely (`smmu.cpp:1144`) — stronger than the Rust guard, conformant. TDD tests: `P3Httu.Stage2HttuUpdateSetsAccessFlag`, `P3Httu.Stage2HttuS2HaTranslationSucceeds`. |
| §3.17 / §3.17.2 / §3.17.5 | TLB tagging / Broadcast TLBI | ✅ | VMID=0 substitution for Secure S1-only streams at `smmu.cpp:752–754` ✅. EL2-E2H ASID=0 when CR2.E2H=0 at `smmu.cpp:766–768` ✅. PTM polarity (`receiveBroadcastTLBI()` at `smmu.cpp:5678`) ✅. All BUG-NEW-37–40 fixes present. TDD regression tests: `P3TlbTagging.SecureStage1StreamUsesVmidZero`, `P3TlbTagging.PtmEnabledBlocksBroadcastTlbi`. |
| §3.18 / §3.18.2 | Interrupts / GERROR IRQ | ✅ | `setIrqCtrl()`/`getIrqCtrlAck()` synchronous pair conformant (`smmu.cpp:3568–3577`). No IRQ delivery infrastructure — N/A for SW model (same as §7.5.1). TDD regression test: `P3IrqCtrl.IrqCtrlAckMirrorsWrite`. |
| §3.19 | Power control / DORMANT | ✅ | **BUG-DORMANT-CPP FIXED**: `IDR0.DORMHINT=1` (bit 8) was set but `getStatusr()` always returned 0. `disable()` (`smmu.cpp:5589`) now sets `statusr_=1` (DORMANT=1); `enable()` (`smmu.cpp:5576`) clears `statusr_=0`. TDD tests: `P3Dormant.StatusrDormantSetAfterDisable`, `P3Dormant.StatusrDormantClearedAfterReEnable`, `P3Dormant.Idr0DormhintIsSet`. |
| §3.21 / §3.21.3 | Structure access rules / CFGI | ✅ | `disableStream()` TLB flush is SW responsibility per §3.21 (software issues CMD_CFGI_STE + CMD_TLBI_*). `CMD_CFGI_STE`, `CMD_CFGI_ALL`, `CMD_CFGI_STE_RANGE`, `CMD_CFGI_CD`, `CMD_CFGI_CD_ALL` all present and correct. TDD regression tests: `P3Cfgi.CfgiSteInvalidatesTlbEntry`, `P3Cfgi.CfgiAllDoesNotRaiseCmdqErr`. |
| §5.2 (STE general) | STE — stream table entry | ✅ | Fixed in Priority 1 (INSTCFG encoding BUG-AUDIT-133-CPP). TDD regression test: `P3Ste.InstCfgForceDataIsEncoding2`. |

---

## Priority 4 — C++ Marked N/A Where Code Exists (Needs Justification or Fix)

These were marked `N/A` for C++ in TASKS_BUGS.md but C++ has relevant implementation.
Each needs either a documented rationale confirming N/A, or reclassification + audit.

| Section | Why Flagged | C++ Status | Final Disposition |
|---------|-------------|-----------|-------------------|
| §3.4.3 | Marked "C++ not applicable" but C++ has OAS checks in `stream_context.cpp`. BUG-AUDIT-114/115/123 were Rust-only fixes — did C++ already have correct OAS checks, or were they silently skipped? | ✅ | **RESOLVED BY PRIORITY 3**: P3 spot-check §3.4/§3.4.1 audit confirmed C++ `AddressSpace::translatePage()` enforces address-size faults correctly. Regression tests added in `test_cpp_audit_p3_spotcheck.cpp` (`P3OasIps.*` tests). No code change required. Cross-ref: P3 sprint. |
| §3.13.4–3.13.5 | Marked N/A for C++. C++ has `ha`/`hd` member fields and `setHA()`/`setHD()` methods. BUG-AUDIT-129/130 (S2HA requires S2HD guard, two-stage HTTU update) may apply. | ✅ | **RESOLVED BY PRIORITY 3**: BUG-AUDIT-130-CPP fixed in P3 sprint — stage-2 `updateAccessFlags()` call added in both `performBothStagesTranslation()` (smmu.cpp) and `translateUnlocked()` (stream_context.cpp) for `s2ha`/`s2affd` paths. TDD tests: `test_cpp_audit_p3_spotcheck.cpp` (`P3Httu.*`). |
| §7.3.1 | Marked N/A for C++. C++ has MEV dedup logic. Stall-guard missing? | ✅ | **RESOLVED BY PRIORITY 2**: BUG-7.3.1-CPP fixed in P2 sprint — added `!isStall` outer gate and `!existing.stall` inner guard to MEV dedup block in `generateEvent()`. TDD test: `test_cpp_audit_p2_sec7_mev_stall.cpp`. |
| §8.1.1 | Marked N/A for C++. C++ has `priqProd`/`priqCons`. Recovery procedure may be relevant. | ✅ | **RESOLVED BY PRIORITY 2**: P2 §8 sprint audited PRI queue `priqProd`/`priqCons` modulus, PRIQ_ABT_ERR gate, and Secure stream discard. BUG-SEC8-SECURE-PRI-CPP fixed (Secure stream PRI discard). TDD tests: `test_cpp_audit_p2_sec8.cpp`. |
| §13.1.2 | Marked N/A for C++. C++ has INSTCFG override at `stream_context.cpp:1126`. Write→Data clamping needed. | ✅ | **RESOLVED BY PRIORITY 1**: BUG-13.1.2-CPP-A confirmed NO FIX NEEDED — C++ already does not convert Write→Execute in INSTCFG path (stream_context.cpp:1153 explicitly handles writes as Data). The Rust bug was Rust-only. See P1 row for full details. |
| §13.1.5 | Marked N/A for C++. C++ has two-stage translation. Attribute combining gap is real. | ✅ | **RESOLVED BY PRIORITY 1**: BUG-13.1.5-CPP fixed — `combinePageAttr()` lambda added to `performBothStagesTranslation()` (smmu.cpp:2749) and two-stage result block in `translateUnlocked()` (stream_context.cpp:~1411). TDD tests: `test_cpp_audit_p1_memattr.cpp`, `test_cpp_audit_p1_memattr_sc.cpp`. |
| §13.1.7 | Marked N/A for C++. C++ has `applyOutputAttrs` lambda. Device/NC→OSH not enforced there. | ✅ | **BUG-13.1.7-CPP FIXED (this sprint)**: Three-part fix: (1) Added `TLBEntry::pageAttr` field (types.h) + propagation in `cacheTranslation()` (smmu.cpp:~2341). (2) `applyOutputAttrs` lambda (stream_context.cpp:1200): added Device→OSH guard using effective type (`mtCfg ? memAttr==0x00 : data.pageAttr==0x00`). (3) TLB fast path (smmu.cpp:~552): same guard using `entry.pageAttr`. Also fixed `pageAttr` propagation from stage-1 result into `s1data` (stream_context.cpp:~1455). TDD tests: `test_cpp_audit_p4_na_justification.cpp` (5 tests — slow-path mtCfg=false/true, TLB-hit path, regression guards). |

---

## Audit Order Recommendation

1. **Fix Priority 1 bugs first** — these are already understood, just need TDD + fix ported from Rust.
   Start with INSTCFG encoding (`§5.2`) since it affects 9+ call sites and underlies §13.1.2/§13.1.4.

2. **§13.1.5 two-stage MemAttr combining** — implement `combineMemAttr()` equivalent in C++
   mirroring Rust's `combine_mem_type()`.

3. **§13.2 GBPA Device/NC→OSH** — single-location fix in `smmu.cpp:305–312`.

4. **Priority 2 §7/§8 sections** — verify event priority, PRI overflow, GERROR gating.
   Most are likely already correct in C++ (earlier audit passes) but need explicit test coverage.

5. **Priority 3 spot-checks** — re-read the relevant C++ paths against spec and confirm or escalate.

6. **Priority 4 N/A justifications** — for each, either document why N/A is correct or
   reclassify to ☐ and audit.

---

## Test Coverage Gap

At audit completion, **C++ had 185 tests** and **Rust had 223 tests**. The 38-test gap
corresponds almost entirely to the §7/§8/§13 sprint that targeted Rust only. Every fix
in this audit must follow the TDD workflow: failing test first, then implementation.

**Last updated**: 2026-04-12 (Priority 4 complete: BUG-13.1.7-CPP FIXED [Device/NC→OSH enforcement in applyOutputAttrs and TLB fast path; TLBEntry::pageAttr field added; pageAttr propagation fixed in s1data path]. 6 of 7 P4 items resolved by earlier sprints (P1/P2/P3), documented with cross-references. All 5 new P4 TDD tests pass. Test count: 187/187 test executables passing (5 new P4 test cases in test_cpp_audit_p4_na_justification). **AUDIT COMPLETE — all priorities resolved.**)
