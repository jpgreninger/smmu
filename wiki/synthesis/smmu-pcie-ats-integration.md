---
title: "SMMU PCIe ATS Integration"
type: synthesis
tags: [smmu, pcie, ats, pri, translated, split-stage, dpt, cxl, model]
created: 2026-04-07
updated: 2026-04-07
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# SMMU PCIe ATS Integration

Complete model reference for SMMU handling of PCIe Address Translation Services (ATS) and Page Request Interface (PRI). This covers the three transaction types, STE.EATS encoding, per-security-state behavior, invalidation, and CXL interactions.

## ATS Transaction Processing Model

### Transaction Type Input

The SMMU sees three AT field values:

```
AT = 0b00  →  Untranslated (normal DMA; SMMU translates)
AT = 0b01  →  Translation Request (endpoint requesting PA; SMMU returns ATS completion)
AT = 0b10  →  Translated (endpoint using cached PA from prior ATS; SMMU checks or passes through)
```

SubstreamID/SSV is carried with Translation Requests and Translated transactions when the endpoint supports PASID (SubstreamID = PASID, 1:1 mapping, up to 20 bits).

### STE.EATS-Based Dispatch

| STE.EATS | AT=Untranslated | AT=Translation Request | AT=Translated (ATSCHK=0) | AT=Translated (ATSCHK=1) |
|----------|----------------|----------------------|--------------------------|--------------------------|
| 0b00     | Normal translation | Return failure completion | Treat as Untranslated | SMMU checks, records faults |
| 0b01     | Normal translation | Return full (stage 1+2) PA translation | Pass through (no checks) | SMMU checks STE, records |
| 0b10     | Normal translation | Return stage 2 PA only (IPA→PA) | Apply stage 2 to address | Treat as Untranslated (ATSCHK reinterprets) |
| 0b11     | Normal translation | Same as 0b01 | DPT check, then pass/fault | DPT check; records faults |

**Note:** `EATS == 0b10` is invalid when `ATSCHK == 0` (interpreted as 0b00).

## Full ATS Flow (EATS = 0b01)

1. **Translation Request arrives** (AT=0b01):
   - SMMU performs full two-stage translation (or configured stages).
   - On success: returns ATS Translation Completion with PA and memory attributes.
   - On fault: returns completion with failure status (no PA).
2. **Endpoint caches PA in ATC** and uses it for subsequent DMA.
3. **Translated transaction arrives** (AT=0b10):
   - `ATSCHK=0`: SMMU passes through, no checks.
   - `ATSCHK=1`: SMMU checks STE validity, EATS, NS rules, GPC (Realm), records faults if applicable.

## Split-Stage ATS Flow (EATS = 0b10)

Used when endpoint participates in stage 1 translation (guest OS owns stage 1, hypervisor owns stage 2):

1. **Translation Request**: SMMU applies stage 2 only (IPA→PA). Returns stage 2 PA to endpoint.
2. **Endpoint performs stage 1** (VA→IPA) internally.
3. **Translated transaction** (AT=0b10): carries the IPA (not PA). SMMU applies stage 2 to translate IPA→PA.

Requires `SMMU_(*_)CR0.ATSCHK == 1`.

**Warning:** A direct transition from `EATS == 0b10` to `EATS == 0b×1` (or vice versa) without going through `0b00` risks data corruption because the endpoint's ATC may have IPAs instead of PAs (or vice versa).

## ATS Invalidation Flow

When a mapping changes (page unmapped, permissions changed):

1. Software updates translation table.
2. Software issues `CMD_TLBI_*` to invalidate SMMU TLB.
3. Software issues `CMD_ATC_INV(StreamID, SubstreamID, SSV, Global, Address, Size)`.
   - SMMU sends ATS Invalidate Request to endpoint via Root Complex.
   - Endpoint flushes its ATC for the specified address range.
   - Endpoint sends Invalidate Completion back.
4. Software issues `CMD_SYNC` to wait for invalidation to complete.
   - If endpoint does not respond within the ATS timeout → `CERROR_ATC_INV_SYNC` reported on next `CMD_SYNC`.
   - Command processing halts; Root Complex should isolate the non-responsive endpoint.
   - **Critical:** software must not proceed as if invalidation succeeded after a timeout.

## PRI Queue Flow

1. Endpoint sends Page Request message (PCI PRI) for a VA/IPA it cannot resolve.
2. SMMU delivers the page request to the PRI queue (`SMMU_PRIQ_*`).
3. Software reads the PRI queue entry.
4. Software resolves the mapping (maps the page, pins it, etc.).
5. Software issues `CMD_PRI_RESP(StreamID, SubstreamID, SSV, PRGIndex, Resp)` to respond to the endpoint.
6. SMMU forwards the PRI Response Message to the endpoint via the Root Complex.

## Enabled/Disabled Summary

| `SMMU_CR0.SMMUEN` | `SMMU_CR0.PRIQEN` | ATS/PRI Behavior |
|------------------|--------------------|-----------------|
| 0 | X | Translation Requests return failure; PRI denied as if PRIQEN=0 |
| 1 | 0 | PRI Page Requests denied |
| 1 | 1 | Normal PRI operation |

## ATS Configuration Change Procedures

### Enable ATS (EATS 0b00 → 0b×1 or 0b10)
1. Update STE with new EATS value.
2. Issue `CMD_CFGI_STE` + `CMD_SYNC`.
3. Enable ATS at the endpoint.

### Disable ATS (EATS 0b×1 or 0b10 → 0b00)
1. Disable ATS at endpoint.
2. Flush endpoint ATC (`CMD_ATC_INV` + `CMD_SYNC`).
3. Set `STE.EATS = 0b00`.
4. Issue `CMD_CFGI_STE` + `CMD_SYNC`.

### Transition EATS 0b10 → 0b×1 (or reverse)
Must go through 0b00:
1. Disable ATS per above procedure (through 0b00).
2. Enable ATS with new EATS value per above.

### Enable/Disable ATSCHK
To set ATSCHK=1:
1. Set `SMMU_CR0.ATSCHK = 1`, wait for update.
2. STEs fetched after this point interpret EATS with new ATSCHK.

To clear ATSCHK=0:
1. First disable all `EATS == 0b10` streams (transition to 0b00).
2. Set `SMMU_CR0.ATSCHK = 0`, wait for update.

## Security State ATS Interactions

### Realm Streams (SEC_SID = Realm)

- `SMMU_R_CR0.ATSCHK` is RES1 (always 1).
- Translated transactions (AT=0b10) are always checked.
- PA space for Translated transactions determined by:
  - `EATS=0b01`: from STE.NSCFG applied to input NS.
  - `EATS=0b10`: from stage 2 translation result. If NS PA and Instruction access → F_PERMISSION abort.
  - `EATS=0b11`: from DPT lookup.
- GPC is applied to the PA after all checks.

### PCIe IDE TLP Prefix → Security State

- T=0 or no prefix: Realm streams not applicable; SEC_SID=Non-secure.
- T=1: SEC_SID=Realm.
- ATS Translation Completions for Realm streams carry TE=1 (when `SMMU_R_IDR3.XT==1`) if translation resolved to Realm PA and completion is Success with R or W set.

### CMD_ATC_INV Security State Routing

- Issued on NS Command queue → forwarded to PCIe with T=0.
- Issued on Realm Command queue → forwarded to PCIe with T=1. If StreamID not in Root Port's IDE Selective Stream RID range → not propagated; reported as `CERROR_ATC_INV_SYNC` on next CMD_SYNC.

## CXL Interactions

- CXL Type 1/2 devices require `SMMU_IDR0.ATS == 1`.
- `STE.EATS == 0b10` must NOT be used for CXL.cache-issuing device StreamIDs (software error; no event).
- If ATS Translation Request has `Source-CXL` bit set:
  - For `EATS == 0b10` stream: Translation Completion has CXL.io bit set.
  - If translation returns non-cacheable memory type (not Inner WB, Outer WB, Shareable): CXL.io bit set.
  - If memory attributes undetermined (both stages disabled + Use incoming): default to WB Cacheable Shareable.

## Model Implementation Notes

- The three AT values are first-class inputs to the model's dispatch logic.
- `STE.EATS` and `SMMU_CR0.ATSCHK` together determine the exact behavior for AT=0b10; model must check both.
- ATC invalidation timeout (CERROR_ATC_INV_SYNC) requires modeling the PCIe response path or a timeout counter.
- For functional models, ATC state (what translations the endpoint has cached) may need to be tracked to verify correctness of invalidation sequencing.
- Substream/PASID handling in ATS: SubstreamID present in ATS Translation Requests and CMD_ATC_INV when SSV=1.

## Related Pages

- [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) — concept-level reference
- [../concepts/security-states.md](../concepts/security-states.md) — SEC_SID from T/XT bits
- [../concepts/stream-table-entry.md](../concepts/stream-table-entry.md) — STE.EATS encoding
- [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md) — applied to Translated transactions
- [../concepts/device-permission-table.md](../concepts/device-permission-table.md) — EATS=0b11 DPT check
- [../concepts/command-queue.md](../concepts/command-queue.md) — CMD_ATC_INV, CMD_PRI_RESP
- [../synthesis/smmu-translation-pipeline.md](../synthesis/smmu-translation-pipeline.md) — ATS in translation pipeline
