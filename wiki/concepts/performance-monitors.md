---
title: "Performance Monitors Extension (PMCG)"
type: concept
tags: [smmu, pmu, performance, monitoring, pmcg, events, counters]
created: 2026-04-13
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Performance Monitors Extension (PMCG)

## Definition

The SMMU Performance Monitors Extension (Chapter 10) provides optional hardware event counters for measuring SMMU activity. When implemented, the SMMU has one or more **Performance Monitor Counter Groups (PMCGs)**, each independently implemented and potentially located in separate hardware components associated with the SMMU. Presence and location of each PMCG is **IMPLEMENTATION DEFINED**; there is no centralized enumeration scheme because PMCGs may be independently designed.

## Structure: Counter Groups

Each PMCG:
- Contains **1–64 counters**, each configurable to count any supported event type.
- Is associated with a **subset of StreamIDs** it can observe (fixed at design time, not reprogrammable). The association between counter groups and StreamID ranges is IMPLEMENTATION DEFINED.
- Has a **discoverable set of supported events**; groups are not required to support the same events.
- Occupies one 4 KB register page (Page 0) and an optional second 4 KB page (Page 1). If `SMMU_PMCG_CFGR.RELOC_CTRS == 1`, counter value registers are relocated to Page 1 (enabling hypervisor-controlled guest access to counter values while trapping configuration).

Arm strongly recommends that the union of all counter groups' StreamID associations covers all possible SMMU StreamIDs.

## Architected Events

Seven mandatory event types are defined (IDs 0x0000–0x007F are architected; 0x0080–0xFFFF are IMPLEMENTATION DEFINED):

| Event ID | Description | Mandatory | StreamID filterable |
|---|---|---|---|
| 0 | Clock cycle | Yes | No |
| 1 | Translation or request | Yes | Yes |
| 2 | TLB miss | Yes | Yes |
| 3 | Configuration cache miss | Yes | Yes |
| 4 | Translation table walk access | Yes | Yes |
| 5 | Configuration structure access | Yes | Yes |
| 6 | PCIe ATS Translation Request received | If ATS supported | Yes |
| 7 | PCIe ATS Translated transaction passed through | If ATS supported | Yes |

When GPC (Granule Protection Checks) or DPT are enabled, events 2 and 4 also count GPT/DPT-related accesses.

## StreamID Filtering

Each counter can filter events by StreamID using three modes:

| Mode | Condition |
|---|---|
| ExactSID | `FILTER_SID_SPAN == 0`; matches exact StreamID |
| PartialSID | `FILTER_SID_SPAN == 1`; variable-width mask matching upper bits |
| AllSIDOneSECSID | All-ones except MSB; matches all StreamIDs of one Security state |
| AllSIDManySECSID | All-ones in all implemented bits; matches all StreamIDs regardless of state |

When `SMMU_PMCG_CFGR.SID_FILTER_TYPE == 0`, each counter has its own filter (`SMMU_PMCG_EVTYPERn` + `SMMU_PMCG_SMRn`). When `SID_FILTER_TYPE == 1`, a single filter applies to all counters in the group.

The `FILTER_SEC_SID` bit selects between Non-secure and Secure StreamID namespaces. When RME is implemented, `FILTER_REALM_SID` additionally controls Realm StreamID counting. The `SMMU_PMCG_ROOTCR` register gates Realm and Root observation.

For NoStreamID accesses, events are only counted in AllSIDOneSECSID or AllSIDManySECSID modes, with the target PA space treated as the effective Security state.

## PARTID/PMG Filtering

If `SMMU_PMCG_CFGR.FILTER_PARTID_PMG == 1`, counters can filter by MPAM PARTID and PMG (mutually exclusive with StreamID filtering for that counter). The MPAM security space is selected via `FILTER_MPAM_SP` (or `FILTER_MPAM_NS` in non-RME systems).

## Overflow, Interrupts, and Capture

- A counter overflows when it wraps past its maximum value; the corresponding bit in the Overflow Status array (`SMMU_PMCG_OVS{SET0,CLR0}`) is set.
- Overflow does not prevent the counter from continuing to count.
- When enabled, overflow asserts a per-group interrupt (wired edge-triggered or MSI).
- **Capture:** If `SMMU_PMCG_CFGR.CAPTURE == 1`, a write to `SMMU_PMCG_CAPR.CAPTURE` or an overflow from a counter with `OVFCAP == 1` simultaneously snapshots all counter values into shadow registers (`SMMU_PMCG_SVRn`). This enables race-free sampling.
- Arm recommends all PMCGs support MSIs if the core SMMU supports MSIs.

## Page 0 Key Registers

| Offset | Register | Purpose |
|---|---|---|
| 0x000+n×stride | `SMMU_PMCG_EVCNTRn` | Counter value (32- or 64-bit) |
| 0x400+4×n | `SMMU_PMCG_EVTYPERn` | Per-counter event type and filter configuration |
| 0x600+n×stride | `SMMU_PMCG_SVRn` | Shadow value register (capture output) |
| 0xA00+4×n | `SMMU_PMCG_SMRn` | StreamID and PARTID/PMG filter values |
| 0xC00 | `SMMU_PMCG_CNTENSET0` | Counter enable set |
| 0xC20 | `SMMU_PMCG_CNTENCLR0` | Counter enable clear |
| 0xC40 | `SMMU_PMCG_INTENSET0` | Interrupt enable set |
| 0xC60 | `SMMU_PMCG_INTENCLR0` | Interrupt enable clear |
| 0xC80 | `SMMU_PMCG_OVSCLR0` | Overflow status clear |
| 0xCC0 | `SMMU_PMCG_OVSSET0` | Overflow status set |
| 0xD88 | `SMMU_PMCG_CAPR` | Capture trigger (WO) |
| 0xDF8 | `SMMU_PMCG_SCR` | Secure control (Secure-only access) |
| 0xE00 | `SMMU_PMCG_CFGR` | Configuration/capability discovery (RO) |
| 0xE04 | `SMMU_PMCG_CR` | Counter group enable/reset |
| 0xE08 | `SMMU_PMCG_IIDR` | Implementation identity |
| 0xE20 | `SMMU_PMCG_CEID0/1` | Supported event ID bitmaps |
| 0xE48 | `SMMU_PMCG_ROOTCR` | Root control (Root-only access) |
| 0xE50–0xE64 | `SMMU_PMCG_IRQ_*` | MSI interrupt configuration |
| 0xE6C | `SMMU_PMCG_GMPAM` | MPAM attributes for PMCG MSI writes |
| 0xE70 | `SMMU_PMCG_AIDR` | Architecture version |
| 0xE74/0xE78 | `SMMU_PMCG_{S_}MPAMIDR` | MPAM capability discovery |

## Secure State Support (§10.6)

- `SMMU_PMCG_SCR.SO` (Secure Observation) gates whether Secure StreamID events are visible to Non-secure counters.
- A Non-secure debug agent must not observe Secure transaction events unless SO is enabled.
- When Realm is supported, `SMMU_PMCG_ROOTCR.RLO` gates Realm observation; `SMMU_PMCG_ROOTCR.RTO` gates Root observation.

## MPAM for PMCG MSIs (§17.5)

PMCGs that generate MSI writes can independently carry MPAM PARTID and PMG. Configuration is via `SMMU_PMCG_GMPAM`. The PARTID space is governed by `SMMU_PMCG_SCR.{NSMSI, MSI_MPAM_NS}`.

## Related Concepts

- [streamid-substreamid.md](streamid-substreamid.md) — StreamID filtering targets specific device streams
- [security-states.md](security-states.md) — Secure, Realm, Root observation controls
- [smmu-initialization.md](smmu-initialization.md) — PMCG pages are at IMPLEMENTATION DEFINED base addresses; no central discovery
- [pcie-ats-pri.md](pcie-ats-pri.md) — Events 6 and 7 count ATS-specific activity
- [granule-protection-check.md](granule-protection-check.md) — Events 2 and 4 count GPT accesses
- [mpam.md](mpam.md) — PARTID/PMG filtering (§10.5.1) and PMCG MSI MPAM attributes (§17.5) tightly couple PMCG and MPAM configuration
- [device-permission-table.md](device-permission-table.md) — Events 2 and 4 count DPT lookups
- [debug-trace.md](debug-trace.md) — complementary Chapter 11 IMPLEMENTATION DEFINED debug/trace facility; PMCG_CAPR provides limited diagnostic visibility via standardized interface
- [../synthesis/smmu-register-map.md](../synthesis/smmu-register-map.md) — PMCG pages at IMPLEMENTATION DEFINED base addresses outside the main SMMU register map

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — Chapter 10 Performance Monitors Extension; §10.1–10.6; register descriptions §10.5.2
