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
| §13.1.5 | Combine — two-stage MemAttr | ✅ | **BUG-13.1.5-CPP FIXED**: Added `combinePageAttr()` lambda in `performTwoStageTranslation()` (smmu.cpp:2749). Device (0x00) wins over Normal (0xFF) per §13.1.5 strength ordering. Also added `mapPageDevice()` API for S1 Device page mapping. TDD test: `test_cpp_audit_p1_memattr.cpp`. | `cpp/src/smmu/smmu.cpp:2749` |
| §13.2 | GBPA bypass — Device/NC→OSH | ✅ | **BUG-13.2-CPP FIXED**: After setting `td.shareability = gbpa.shCfg`, enforce Device→OSH: `if (td.memType == 0x00u) td.shareability = 2u` at `smmu.cpp:309`. TDD test: `test_cpp_audit_p1_gbpa_osh.cpp`. | `cpp/src/smmu/smmu.cpp:309` |
| §13.1.4 | Replace — ATOS must skip INSTCFG/PRIVCFG | ✅ | **BUG-13.1.4-CPP-A FIXED**: Added `TransactionType::GatosTranslation` (0b11). `gatosTranslate()` now passes this type to `translate()`. INSTCFG/PRIVCFG blocks in TLB fast path, `performTwoStageTranslation`, and `StreamContext::translateUnlocked` are all guarded with `transactionType != GatosTranslation` / `!isAtos`. TDD test: `test_cpp_audit_p1_atos_bypass.cpp`. | `cpp/src/smmu/smmu.cpp:3747` `cpp/src/stream_context/stream_context.cpp:1147,1174` |

---

## Priority 2 — Sections Audited in Rust Only, C++ Not Checked

These sections received Rust-only TDD test coverage. The equivalent C++ code exists but
was never verified against the findings. May be correct, may have the same bugs.

### §7 — Faults, Errors and Event Queue

| Section | Title | C++ Status | What to Check | C++ Location |
|---------|-------|-----------|---------------|--------------|
| §7.3.1 | Event record merging — stall guard | ☐ | Rust fix (BUG-7.3.1-01): stall events (Stall==1) must never be merged even when MEV=1. Check C++ MEV dedup path has `!event.stall` guard. | `cpp/src/stream_context/stream_context.cpp` MEV/generateEvent |
| §7.3.22 | Event queue record priorities | ☐ | **Explicitly marked `☐` in TASKS_BUGS.md.** Verify C++ emits events in correct priority order: config-class faults before translation-class faults. Structural early-returns should enforce this — confirm they do. | `cpp/src/stream_context/stream_context.cpp` translate path |
| §7.4 | Event queue overflow | ☐ | Rust verified: OVFLG toggle, stall-drain on overflow, non-stall discard+OVFLG. C++ left at ⚠️. Verify `smmu.cpp` enqueue path matches all 5 §7.4 requirements. | `cpp/src/smmu/smmu.cpp` enqueue_event |
| §7.5 | Global error recording | ☐ | Rust fix (BUG-GERROR-01): GERROR_DPT_ERR bit 10 added. Verify C++ GERROR bitmask is complete and toggle protocol is correct. | `cpp/src/smmu/smmu.cpp` GERROR |
| §7.5.1 | GERROR interrupt notification | ☐ | Rust fix (BUG-GERROR-02/03): `gerror_irq_pending` flag; `signal_gerror()` gates on `GERROR_IRQEN`. Check C++ equivalent gating. | `cpp/src/smmu/smmu.cpp` signal_gerror |

### §8 — Page Request Queue

| Section | Title | C++ Status | What to Check | C++ Location |
|---------|-------|-----------|---------------|--------------|
| §8.1 | PRI queue overflow | ☐ | Marked ⚠️ in TASKS_BUGS.md but only Rust tests added. Verify C++ OVFLG toggle, inhibit-new-PPRs on overflow, `priq_emitted` CAS equivalent. | `cpp/src/stream_context/stream_context.cpp` PRI path |
| §8.1.1 | PRI recovery procedure | ☐ | **C++ marked N/A in TASKS_BUGS.md** — but C++ has a PRI queue (`priqProd`/`priqCons` at `stream_context.cpp:96–97`). Rust added `recover_priq_overflow()`. Verify whether C++ needs an equivalent or document why N/A is correct. | `cpp/src/stream_context/stream_context.cpp:91–170` |
| §8.2 | PRI miscellaneous | ☐ | Rust fixes: `PRIQ_ABT_ERR` gate + Secure stream discard (`ResponseCode=0b1111`). Check C++ PRI submission path applies same gates. | `cpp/src/stream_context/stream_context.cpp` |
| §8.3 | PRG Response Message codes | ☐ | Rust fix: `Resp=0b11` → `CERROR_ILL`. Verify C++ `CMD_PRI_RESP` handler rejects `Resp=0b11` identically. | `cpp/src/smmu/smmu.cpp` CMD_PRI_RESP |

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

| Section | Title | Notes |
|---------|-------|-------|
| §3.4 / §3.4.1 | Address sizes / Input address size | Rust ✅, C++ ⚠️. BUG-AUDIT-114/115/123 fixed Rust-only. C++ OAS/IPS checks at `stream_context.cpp` should be re-verified. |
| §3.5.1–3.5.4 | Queue semantics | Rust ✅ after 2026-04-07 re-audit. C++ left ⚠️. OVFLG/modulus/commit semantics should be spot-checked. |
| §3.13.4–3.13.5 | HTTU behavior summary / Two stages | Rust ✅ (BUG-AUDIT-129/130). C++ marked N/A. C++ has `ha`/`hd` fields (`stream_context.cpp:31–32`). Confirm HTTU checks in C++ match spec or document why N/A is correct. |
| §3.17 / §3.17.2 / §3.17.5 | TLB tagging / Broadcast TLBI | Rust ✅ (BUG-AUDIT-131, BUG-NEW-37–40). C++ left ⚠️. VMID=0 substitution, PTM polarity, EL2-E2H ASID scoping should be re-checked. |
| §3.18 / §3.18.2 | Interrupts / GERROR IRQ | Rust ✅. C++ left ⚠️. IRQ_CTRL gating, SEV signal path should be spot-checked. |
| §3.19 | Power control / DORMANT | Rust ✅. C++ left ⚠️. `STATUSR.DORMANT` set after `shutdown()` — verify C++ equivalent. |
| §3.21 / §3.21.3 | Structure access rules / CFGI | Rust ✅. C++ left ⚠️. `disable_stream()` TLB flush and `CMD_CFGI_*` handlers should be re-checked. |
| §5.2 (STE general) | STE — stream table entry | Rust ✅ after BUG-AUDIT-133. C++ ⚠️ — overlaps Priority 1 INSTCFG encoding bug. |

---

## Priority 4 — C++ Marked N/A Where Code Exists (Needs Justification or Fix)

These were marked `N/A` for C++ in TASKS_BUGS.md but C++ has relevant implementation.
Each needs either a documented rationale confirming N/A, or reclassification + audit.

| Section | Why Flagged | C++ Code Evidence |
|---------|-------------|-------------------|
| §3.4.3 | Marked "C++ not applicable" but C++ has OAS checks in `stream_context.cpp`. BUG-AUDIT-114/115/123 were Rust-only fixes — did C++ already have correct OAS checks, or were they silently skipped? | `cpp/src/stream_context/stream_context.cpp` OAS/IPS validation |
| §3.13.4–3.13.5 | Marked N/A for C++. C++ has `ha`/`hd` member fields and `setHA()`/`setHD()` methods. BUG-AUDIT-129/130 (S2HA requires S2HD guard, two-stage HTTU update) may apply. | `cpp/src/stream_context/stream_context.cpp:31–32, 355–361` |
| §7.3.1 | Marked N/A for C++. C++ has MEV dedup logic. Stall-guard missing? | `cpp/src/stream_context/stream_context.cpp` generateEvent/MEV |
| §8.1.1 | Marked N/A for C++. C++ has `priqProd`/`priqCons`. Recovery procedure may be relevant. | `cpp/src/stream_context/stream_context.cpp:96–97` |
| §13.1.2 | Marked N/A for C++. C++ has INSTCFG override at `stream_context.cpp:1126`. Write→Data clamping needed. | `cpp/src/stream_context/stream_context.cpp:1126–1135` |
| §13.1.5 | Marked N/A for C++. C++ has two-stage translation. Attribute combining gap is real. | `cpp/src/stream_context/stream_context.cpp:1354–1391` |
| §13.1.7 | Marked N/A for C++. C++ has `applyOutputAttrs` lambda. Device/NC→OSH not enforced there. | `cpp/src/stream_context/stream_context.cpp:1177–1185` |

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

**Last updated**: 2026-04-11 (Priority 1 bugs all fixed: BUG-AUDIT-133-CPP, BUG-13.1.5-CPP, BUG-13.2-CPP, BUG-13.1.4-CPP-A; BUG-13.1.2-CPP-A confirmed N/A)
