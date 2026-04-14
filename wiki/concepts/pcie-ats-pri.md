---
title: "PCIe ATS and PRI"
type: concept
tags: [smmu, pcie, ats, pri, pasid, address-translation, cxl, rme, prg, overflow]
created: 2026-04-07
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# PCIe ATS and PRI

## Definition

**ATS (Address Translation Services)** is a PCIe mechanism allowing an endpoint device to request translations from the SMMU/IOMMU, cache them locally in an Address Translation Cache (ATC), and use the resulting PA directly for DMA (bypassing per-transaction SMMU translation). This reduces translation overhead for high-bandwidth devices.

**PRI (Page Request Interface)** is a PCIe extension to ATS that allows an endpoint to request the OS to make a virtual memory mapping present for DMA. It enables page-fault handling for device DMA without aborting the transaction.

Both features are optional. ATS support is indicated by `SMMU_IDR0.ATS == 1`; PRI by `SMMU_IDR0.PRI == 1`.

## Transaction Types

| Transaction Type | AT Field | Description |
|-----------------|----------|-------------|
| Untranslated | 0b00 | Normal DMA; SMMU performs translation |
| Translation Request | 0b01 | Endpoint requesting a PA translation (ATS request) |
| Translated | 0b10 | DMA using a previously-obtained ATS translation; SMMU checks/passes through |

## STE.EATS Field

`STE.EATS` controls ATS behavior per stream:

| STE.EATS | Behavior |
|----------|----------|
| 0b00     | ATS disabled; Translation Requests returned with failure; Translated transactions treated as Untranslated (checked by SMMU) |
| 0b01     | Full ATS — SMMU returns a PA translation; Translated transactions bypass translation |
| 0b10     | Split-stage ATS — SMMU performs only stage 2 on Translation Requests and Translated transactions (stage 1 handled by endpoint); only valid when `SMMU_CR0.ATSCHK == 1` |
| 0b11     | Use DPT — same behavior as Full ATS (0b01) plus DPT check on Translated transactions |

**Critical transition rules** (§3.9.2):
- Transitioning between `0b00` and `0b×1`/`0b10` requires STE invalidation and endpoint ATC flush.
- Transitioning directly between `0b×1` and `0b10` is forbidden; must go through `0b00`.
- Transitioning between `0b01` and `0b11` is permitted without going through `0b00`.
- `EATS == 0b10` is only valid when `SMMU_CR0.ATSCHK == 1`.

## SMMU_CR0.ATSCHK

When `ATSCHK == 1`:
- Translated transactions are subject to additional SMMU checks (fault recording, GPC for Realm streams).
- `EATS == 0b10` (split-stage ATS) is a valid configuration.

When `ATSCHK == 0`:
- Translated transactions bypass the SMMU with no additional checking.
- `EATS == 0b10` is interpreted as `0b00` (ATS disabled).

`ATSCHK` is required to be 1 (RES1) for Realm streams (`SMMU_R_CR0.ATSCHK`).

## ATS Translation Request Handling

When an endpoint sends a Translation Request (AT=0b01) and ATS is enabled:
1. SMMU performs the standard translation lookup (STE → CD → table walk).
2. Returns an ATS Translation Completion containing the PA and memory attributes.
3. The endpoint caches this in its ATC and may use it for subsequent Translated transactions.

For split-stage ATS (`EATS == 0b10`): only stage 2 translation is returned. The endpoint performs stage 1 and combines results internally.

## ATS Translated Transaction Handling

When a Translated transaction (AT=0b10) arrives:
1. If `ATSCHK == 0`: passes through SMMU without translation or checking.
2. If `ATSCHK == 1`: SMMU checks the STE, verifies `EATS != 0b00`, applies overrides, records faults.
3. For Realm streams: always subject to GPC ([granule-protection-check.md](granule-protection-check.md)) and DPT checks if `EATS == 0b11`.

## ATC Invalidation (CMD_ATC_INV)

When a mapping is unmapped or changed, the endpoint's ATC must be invalidated:
- `CMD_ATC_INV(StreamID, SubstreamID, SSV, Global, Address, Size)` is issued to the Command queue.
- The SMMU forwards an ATS Invalidate Request to the endpoint via the Root Complex.
- `CMD_SYNC` after `CMD_ATC_INV` waits for the invalidation to complete. If the endpoint does not respond within the ATS timeout, `CERROR_ATC_INV_SYNC` is reported on the next `CMD_SYNC`.

**ATS invalidation timeout behavior** (Arm strong recommendation):
- Root Complex should isolate the non-responsive endpoint.
- `CMD_SYNC` should return `CERROR_ATC_INV_SYNC` — command processing stops.
- Do NOT continue as if the invalidation succeeded; doing so risks data corruption via stale ATC entries.

## PRI Queue

The PRI queue (`SMMU_PRIQ_*`) receives PCIe Page Request messages. It is separate from the Event queue, allowing PRI processing independently of fault events.

### PRI Message Format (Chapter 8)

Each PRI message in the queue contains:

| Field | Notes |
|---|---|
| `StreamID[31:0]` | For ATS/PRI, bits [15:0] equal PCIe RequesterID[15:0]. Bits [n:16] may distinguish multiple Root Complex sources |
| `SSV` | Substream Valid: 1 if the Page Request carried a PASID TLP prefix; 0 otherwise. When SSV==0, X and Priv bits are 0 |
| `SubstreamID[19:0]` | Valid only when SSV==1 |
| `Page address[63:12]` | Address of the requested page (4 KB aligned) |
| `PRGIndex[8:0]` | Page Request Group index — identifies which PRG this request belongs to |
| `Last, W, R, X, Priv` | Last/Write/Read/eXecute/Privileged — encode requested page permissions; Last==1 marks the final request in a PRG |

**Stop PASID Markers:** A PCIe Stop PASID marker arrives as a PPR with `LWR == 0b100` and `SSV==1`. A message with `LWR == 0b100` and `SSV==0` is not a Stop Marker; it is treated as a normal PRI Page Request.

### Page Request Groups (PRGs)

Related PPRs (Page Request messages) that share the same `PRGIndex` form a **Page Request Group**. Key properties:
- Multiple pages may be requested in a PRG, all with the same PRGIndex.
- The endpoint sets `Last==1` on the final PPR in the PRG. An interrupt can be configured to fire when Last==1 is received (`SMMU_PRIQ_IRQ_CFG2`).
- PRG members are **not guaranteed to be contiguous** in the PRI queue — multiple PRGs may be interleaved.
- PRG members are **not guaranteed to arrive in order**, except that the `Last==1` entry is not reordered with respect to prior entries.
- The SMMU is not required to verify that all PPRs in a PRG have identical PASID prefixes.

Software issues `CMD_PRI_RESP` after processing all PPRs in a PRG, including the Last==1 entry, to return a success or failure status to the endpoint.

### §8.1 PRI Queue Overflow

The PRI queue enters an **overflow condition** when:
- `SMMU_CR0.PRIQEN == 1`, and
- One or more PRI messages arrive while the queue is full.

**Overflow signaling:** The SMMU toggles `SMMU_PRIQ_PROD.OVFLG`. Software acknowledges by writing `SMMU_PRIQ_CONS.OVACKFLG` to match the value of OVFLG.

**While overflow is active:**
- New entries are **inhibited** from being written (unlike Event queue overflow, which does not inhibit).
- Incoming PPRs with `Last==1` receive an **automatic PRG Response**; ResponseCode depends on PASID presence and STE.PPAR (see below).
- Incoming PPRs with `Last==0` are **silently discarded**.
- Stop PASID Markers are discarded with no auto-response.

**Auto-response PASID and ResponseCode rules for overflow:**

| Condition | PASID on response | ResponseCode |
|---|---|---|
| PPR had no PASID | No PASID | Success (0b0000) |
| `SMMU_IDR3.PPS == 1` | Same PASID as PPR | Success (0b0000) |
| `PPS == 0`, STE valid and `STE.PPAR == 1` | Same PASID as PPR | Success (0b0000) |
| `PPS == 0`, STE valid and `STE.PPAR == 0` | No PASID | Success (0b0000) |
| `PPS == 0`, STE inaccessible or invalid (SMMUEN==0, StreamID OOB, STE fetch abort, STE ILLEGAL) | No PASID | Response Failure (0b1111) |

`STE.PPAR` reflects the PCIe endpoint's **PRG Response PASID Required** flag in its PRI Status register. Arm expects software to program `STE.PPAR` to match the endpoint's advertised flag.

When PASIDs are not supported by the SMMU, responses have no PASID; ResponseCode is Success unless the implementation checks the STE and finds it invalid (implementation-defined).

### §8.1.1 Overflow Recovery Procedure

On overflow, the PRI queue may contain truncated PRGs whose `Last==1` entries were lost and auto-responded to. The recovery procedure:

1. **Process the entire queue** from `SMMU_PRIQ_CONS.RD` to `SMMU_PRIQ_PROD.WR`, issuing `CMD_PRI_RESP` for each PRG where a `Last==1` entry is found.
2. **Ignore truncated groups** — any PRG for which no `Last==1` has been seen up to the current `WR` pointer has had its Last entry auto-responded to; discard the partial group.
3. **Update `SMMU_PRIQ_CONS.RD` together with `OVACKFLG`** in a single write to simultaneously mark the queue processed and clear the overflow condition.

After recovery, a PRGIndex may be reused by the endpoint for a semantically different PRG than any pre-overflow group with the same index.

### §8.2 Miscellaneous

**PRIQ_ABT_ERR:** An external abort during a write to the PRI queue activates `SMMU_GERROR.PRIQ_ABT_ERR`. While `PRIQ_ABT_ERR` is active:
- No entries are written to the PRI queue.
- All incoming PPRs receive an automatic Response with `ResponseCode == 0b1111` (Response Failure).
- If the abort was synchronous: an automatic PRG Response with `ResponseCode == 0b1111` is generated for the associated PPR.
- If the abort was asynchronous: one or more queue entries may be lost with no auto-response (IMPLEMENTATION DEFINED whether synchronous or asynchronous).

**Secure stream PPRs:** A PPR received from a Secure stream is discarded, not recorded into the PRI queue, and automatically responded to with `ResponseCode == 0b1111`.

**PRIQEN == 0:** If `SMMU_CR0.PRIQEN == 0` (including when `SMMUEN == 0`), all incoming PPRs receive automatic PRG Responses with `ResponseCode == 0b1111` and are discarded. No PASID prefix is used on these auto-responses.

**Stop Markers:** The SMMU does not generate responses to Stop Marker messages.

**Incoming PPRs** are not affected by `SMMU_CR0.ATSCHK` or `STE.EATS` configuration.

### §8.3 PRG Response Message Codes

| ResponseCode | Status | Returned for |
|---|---|---|
| 0b0000 | Success | `CMD_PRI_RESP` with Resp==Success; overflow auto-response when no PASID was supplied, or `PPS==1`, or no STE.PPAR error |
| 0b0001 | Invalid Request | `CMD_PRI_RESP` with Resp==InvalidRequest; software could not page in all requested pages (pages don't exist or insufficient privileges) |
| 0b1111 | Response Failure | `CMD_PRI_RESP` with Resp==ResponseFailure; overflow auto-response when `PPS==0`, PASID present, and STE inaccessible/invalid; all auto-responses when `PRIQEN==0`, `PRIQ_ABT_ERR` active, or PPR from Secure stream |

## PCIe IDE TLP Prefix and Security State

| TLP IDE prefix | T-bit | SMMU interpretation |
|----------------|-------|---------------------|
| Absent         | —     | SEC_SID = Non-secure |
| Present, T=0   | 0     | SEC_SID = Non-secure |
| Present, T=1   | 1     | SEC_SID = Realm |

When `SMMU_R_IDR3.XT == 1` (TDISP XT Extensions supported):
- **TE bit** in ATS Translation Completions: set to 1 for Realm PA, 0 for Non-secure PA (on Realm stream completions with Success status and R or W set).
- **XT bit** in Translation Requests and Translated transactions: used to distinguish TEE-targeted Realm transactions from non-TEE Realm transactions.

## CXL Interactions

- CXL Type 1/2 devices (issuing CXL.cache transactions) require `SMMU_IDR0.ATS == 1`.
- `STE.EATS == 0b10` must not be configured for StreamIDs associated with CXL devices issuing CXL.cache transactions (software error; no event recorded).
- If an ATS Translation Request has `Source-CXL` bit set for an `EATS == 0b10` stream, the Translation Completion has the `CXL.io` bit set.
- If a translation for a CXL ATS request returns non-cacheable memory type, the `CXL.io` bit is set in the completion.

## Model Implementation Notes

- ATS transaction type is a 2-bit sideband; a model must distinguish Untranslated, Translation Request, and Translated at the SMMU input.
- The ATC invalidation flow (CMD_ATC_INV → SMMU → Root Complex → endpoint → response → CMD_SYNC completion) must be modeled for correctness in systems with PCIe endpoints.
- Split-stage ATS (`EATS == 0b10`) is a complex path: stage 2 translates, but the result (IPA) is returned as the "PA" to the endpoint. Subsequent Translated transactions carry this IPA and stage 2 is applied by the SMMU.
- PASID (PCIe) maps 1:1 to SubstreamID in the SMMU model.

## Related Concepts

- [streamid-substreamid.md](streamid-substreamid.md) — StreamID from RequesterID; SubstreamID from PASID
- [stream-table-entry.md](stream-table-entry.md) — STE.EATS controls ATS behavior
- [security-states.md](security-states.md) — T/XT bits determine SEC_SID for Realm streams
- [granule-protection-check.md](granule-protection-check.md) — Translated transactions subject to GPC for Realm streams
- [device-permission-table.md](device-permission-table.md) — DPT check for EATS == 0b11
- [command-queue.md](command-queue.md) — CMD_ATC_INV, CMD_PRI_RESP commands

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.9 Support for PCI Express, PASIDs, PRI, and ATS; §3.9.1 ATS Interface; §3.9.2 Changing ATS configuration; §3.9.3 SMMU interactions with CXL; §3.9.4 PCIe fields T, TE and XT; §4.5 ATS and PRI commands
