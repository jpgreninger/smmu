---
title: "PCIe ATS and PRI"
type: concept
tags: [smmu, pcie, ats, pri, pasid, address-translation, cxl, rme, prg, overflow]
created: 2026-04-07
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
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

### §3.9.1.1 Non-Zero Upper PA Bits on Translated Transactions

ATS Translation Completions and ATS Translated transactions carry a 64-bit address field regardless of the SMMU's implemented PA size. A buggy or malicious device may issue a Translated transaction with non-zero bits above the implemented OAS. When this occurs, the SMMU applies one of the following behaviors (IMPLEMENTATION DEFINED which occurs):

- Terminate the transaction with an abort; no event or fault is recorded.
- Truncate the address to `SMMU_IDR5.OAS` bits and use the truncated address for DPT checks, Granule Protection Checks, and propagation into the system if the transaction is otherwise permitted.

### §3.9.1.2 Responses to ATS Translation Requests

Translation Requests to streams where ATS is explicitly or implicitly disabled result in an ATS Translation Completion with Unsupported Request (UR) status:

| Configuration or scenario | For an ATS Translation Request, leads to |
|---|---|
| `SMMUEN == 0` | Terminated with UR status and `F_BAD_ATS_TREQ` generated |
| Using a Secure StreamID | Terminated with UR status and `F_BAD_ATS_TREQ` generated |
| `STE.Config == 0b000` | Terminated with UR status (no event) |
| `STE.Config == 0b100` | Terminated with UR status and `F_BAD_ATS_TREQ` generated |
| Effective `STE.EATS == 0b00` (includes `EATS == 0b1x` when `ATSCHK == 0`) | Terminated with UR status and `F_BAD_ATS_TREQ` generated |

Translation Requests that encounter Address Size, Access, or Translation faults return a **Success completion with R==W==0** (access denied); no fault is recorded in the SMMU. Permission faults may return partial permissions. Configuration errors (C_BAD_STREAMID, C_BAD_STE, F_STE_FETCH, F_VMS_FETCH, F_CFG_CONFLICT, F_TLB_CONFLICT, C_BAD_SUBSTREAMID, F_STREAM_DISABLED, F_WALK_EABT, F_CD_FETCH, C_BAD_CD) result in a **Completer Abort (CA)** status:

| If an ordinary transaction would trigger... | ATS Translation Request leads to |
|---|---|
| `C_BAD_STREAMID` | CA. Event recorded only if `REC_CFG_ATS == 1` AND `RECINVSID == 1` |
| `F_STE_FETCH`, `C_BAD_STE`, `F_VMS_FETCH`, `F_CFG_CONFLICT`, `F_TLB_CONFLICT`, `C_BAD_SUBSTREAMID`, `F_STREAM_DISABLED`, `F_WALK_EABT`, `F_CD_FETCH`, `C_BAD_CD` | CA. Event recorded only if `REC_CFG_ATS == 1` |
| `F_ADDR_SIZE`, `F_ACCESS`, `F_TRANSLATION` | Success: R==W==0. No event recorded |
| `F_PERMISSION` | Success. R/W/Exe granted where available from translation table permissions (extreme: R==W==0) |
| GPF on output address | CA. See §3.25.2 for GPC/ATS interaction details |

For events recorded under `REC_CFG_ATS == 1`, the `RnW` field in the event record is UNKNOWN. The effects of STE overrides on ATS Translation Requests are described in Table 13.4 and 13.5 of the spec (§13.6 PCIe/ATS attribute handling).

### §3.9.1.3 Handling of ATS Translated Transactions

Translated transactions (AT=0b10) generally pass through the SMMU unless SMMUEN is disabled, a Secure stream is used, or ATSCHK is 1:

| Configuration or scenario | For a Translated transaction, leads to |
|---|---|
| `SMMUEN == 0` | `F_TRANSL_FORBIDDEN` and aborted |
| Using a Secure StreamID | `F_TRANSL_FORBIDDEN` and aborted |
| `STE.Config == 0b000` | If `ATSCHK == 1`, aborted (no event) |
| `STE.Config == 0b100` | If `ATSCHK == 1`, `F_TRANSL_FORBIDDEN` and aborted |
| Effective `STE.EATS == 0b00` | If `ATSCHK == 1`, `F_TRANSL_FORBIDDEN` and aborted |
| GPC fault on output address | Aborted; GPC fault reported per §3.25.4 |
| `STE.EATS == 0b10` if inappropriate for the bus protocol | IMPLEMENTATION DEFINED whether `F_TRANSL_FORBIDDEN` and aborted |

For configuration/fault events when `ATSCHK == 1`:

| If an ordinary transaction would trigger... | Translated transaction leads to |
|---|---|
| `F_UUT` | Aborted; no event recorded in Event queue |
| `C_BAD_STREAMID`, `C_BAD_SUBSTREAMID`, `F_STE_FETCH`, `F_VMS_FETCH`, `C_BAD_STE`, `F_CFG_CONFLICT`, `F_STREAM_DISABLED` | Aborted if `ATSCHK == 1`; event recorded if `ATSCHK == 1` AND `REC_CFG_ATS == 1`. Reporting of `C_BAD_STREAMID` is not affected by `RECINVSID`. |

If a Translated transaction with `STE.EATS == 0b10` undergoes a second stage 2 translation and a fault occurs, the transaction is terminated with abort and an event is recorded exactly as for an ordinary transaction.

**PASIDTT and PnU/InD behavior:** If `SMMU_IDR3.PASIDTT == 0` or the transaction has no PASID TLP prefix, the Translated transaction is treated as `PnU == 0, InD == 0, SSV == 0`. If `PASIDTT == 1` and a PASID TLP prefix is present, `PnU` and `InD` are taken from the transaction. These attributes are overridden by `STE.PRIVCFG` and `STE.INSTCFG`, then interpreted per `STE.EATS`:

| STE.EATS | Behavior of PnU/InD on Translated transactions |
|---|---|
| `0b01` (Full ATS) | For Non-secure streams: no effect. For Realm streams: if target PA space is Non-secure and access is Instruction (not Data), `F_TRANSL_FORBIDDEN` is generated |
| `0b10` (Split-stage) | Attributes are input to stage 2 translation |
| `0b11` (Use DPT) | Same as `0b01` (Full ATS) |

**Event priority list** when `SMMUEN == 1` and `ATSCHK == 1` (highest to lowest):
1. `C_BAD_STREAMID`
2. `F_STE_FETCH`
3. `C_BAD_STE`
4. `F_VMS_FETCH` (if `PASIDTT == 1` and `SSV == 1`; priority relative to items 5–12 is IMPLEMENTATION DEFINED)
5. `F_TRANSL_FORBIDDEN` from: `EATS == 0b00`, `EATS == 0b10` used inappropriately, or `STE.NSCFG == 0b01` with Realm SEC_SID and `XT == 0`
6. `C_BAD_SUBSTREAMID` (if `PASIDTT == 1` and `SSV == 1`)
7. `F_STREAM_DISABLED`
8. Events from L1CD or CD fetch (if `PASIDTT == 1` and `SSV == 1`): stage 2 fault on CD fetch or `F_CD_FETCH`
9. `C_BAD_CD` (if `PASIDTT == 1` and `SSV == 1`)
10. `F_ADDR_SIZE` from IAS check if `EATS == 0b10` (reported as stage 1 fault)
11. Translation-related events from stage 2 translation if `EATS == 0b10`
12. `F_TRANSL_FORBIDDEN` from DPT check if `EATS == 0b11`

**Realm stream checks** for Translated transactions with `SEC_SID = Realm`:
1. `SMMU_R_CR0.ATSCHK` is RES1. For `EATS == 0b00/0b01/0b10`, behavior as specified in SMMUv3. For `EATS == 0b11` with `DPT_WALK_EN == 0`: DPT lookup fault is reported as DPT_DISABLED at level 0 and `F_TRANSL_FORBIDDEN` recorded. For `EATS == 0b11` with `DPT_WALK_EN == 1`: DPT check performed; Device Access fault → abort + `F_TRANSL_FORBIDDEN`; DPT lookup fault → abort + `F_TRANSL_FORBIDDEN` + DPT fault register.
2. PA space determined: from stage 2 if `EATS == 0b10`; from DPT if `EATS == 0b11`; otherwise from NS attribute and `STE.NSCFG`. If PA space resolves to Non-secure and access is Instruction (not Data), abort + `F_TRANSL_FORBIDDEN` or `F_PERMISSION`.
3. GPC performed against the PA space determined in step 2.

**Non-secure stream checks** for Translated transactions with `SEC_SID = Non-secure`: If `ATSCHK == 0`, no checks. If `ATSCHK == 1` and `DPT_WALK_EN == 1`, `EATS == 0b11`: DPT check performed; failure → abort + `F_TRANSL_FORBIDDEN`. GPC performed for Non-secure PA space.

## ATC Invalidation (CMD_ATC_INV)

When a mapping is unmapped or changed, the endpoint's ATC must be invalidated:
- `CMD_ATC_INV(StreamID, SubstreamID, SSV, Global, Address, Size)` is issued to the Command queue.
- The SMMU forwards an ATS Invalidate Request to the endpoint via the Root Complex.
- `CMD_SYNC` after `CMD_ATC_INV` waits for the invalidation to complete.

### §3.9.1.4 ATS Invalidation Timeout

If the endpoint does not respond to an ATS Invalidate Request within the ATS-specified timeout period, Arm **strongly recommends**:
- The Root Complex isolates the non-responsive endpoint in a PCI-specific manner, if possible.
- A `CMD_SYNC` waiting for completion of one or more prior `CMD_ATC_INV` operations returns `CERROR_ATC_INV_SYNC` if any `CMD_ATC_INV` has not successfully completed. Command processing stops; this differentiates the failure from a normal `CMD_SYNC` completion, preventing page re-use corruption.
- If returning `CERROR_ATC_INV_SYNC` is not possible, the `CMD_SYNC` either must not complete, or an error must be raised via an IMPLEMENTATION DEFINED asynchronous mechanism that records the failure. The invalidation must not silently appear to succeed.

### §3.9.1.5 ATS Invalidation Errors (UR Response)

A `CMD_ATC_INV` that generates an ATS Invalidate Request resulting in a **UR (Unsupported Request)** response from the endpoint completes without error in the SMMU. Note that an invalidation may not have been performed in response — a UR can occur for reasons such as an out-of-range PASID value.

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

## PCIe IDE TLP Prefix and Security State (§3.9.4)

The T, TE, and XT bits are defined in the PCIe specification and the TDISP eXtended TEE (XT) Extensions specification. They interact with the SMMU's SEC_SID to route transactions to the correct Security state.

### §3.9.4.1 T bit and the PCIe IDE TLP prefix

The T bit appears in the PCIe IDE TLP prefix and indicates whether a transaction originates from a TEE-associated (Trusted) device context.

| TLP IDE prefix | T bit | SEC_SID presented to SMMU |
|---|---|---|
| Absent | — | Non-secure |
| Present, T=0 | 0 | Non-secure |
| Present, T=1 | 1 | Realm |

- All transactions (Untranslated, ATS Translation Requests, PRI messages, Translated) with `T=1` are presented to the SMMU with `SEC_SID = Realm`.
- PRI requests with `T=1` are delivered to the **Realm PRI queue**.
- ATS Translation Completions carry back a T-bit matching the T-bit in the corresponding Translation Request.
- When `SMMU_R_IDR3.XT == 1`, §3.9.4.1 does not apply — see §3.9.4.3: SEC_SID is determined from T|XT (bitwise OR). Within Realm state, T further sets the input NS attribute (T=0 → Non-secure input NS; T=1 → Realm input NS). The T bit is not ignored.

### §3.9.4.2 TE bit on ATS Translation Completions

The **TE (TE Memory Attribute) bit** is introduced by the TDISP eXtended TEE (XT) Extensions specification. It occupies the same position previously used by the Global bit in ATS Translation Completion TLPs.

When `SMMU_R_IDR3.XT == 1`:
- `TE = 1` on a Translation Completion indicates the translated PA is in the **Realm physical address space**.
- `TE = 0` indicates the PA is in the **Non-secure physical address space**.
- This applies for Realm stream Translation Completions with Success status and R or W granted.
- The TE determination applies regardless of whether `STE.EATS == 0b01`, `0b10`, or `0b11`.

### §3.9.4.3 XT bit on Untranslated, Translation Requests, and Translated transactions

The **XT bit** is introduced by the TDISP XT Extensions specification and is evaluated together with T. The combination determines SEC_SID and the precise transaction type:

| T | XT | SEC_SID | Interpretation |
|---|---|---|---|
| 0 | 0 | Non-secure | Normal Non-secure transaction |
| 0 | 1 | Realm | TEE request that must target non-TEE memory |
| 1 | 0 | Realm | TEE-originated Realm transaction |
| 1 | 1 | Realm | TEE-originated Realm with XT extension |

Alternatively, the SMMU client may present the T and XT bits transformed into pre-classified attributes. When `SEC_SID = Realm`:
- T=0 (from above): may indicate a Realm transaction not from a TDISP TLP prefix context.
- T=1 (from above): TDISP-prefixed Realm transaction.

When `SMMU_R_IDR3.XT == 1`, the SEC_SID is the bitwise OR of T and XT:
```
SEC_SID = (T == 1 || XT == 1) ? Realm : Non-secure
```

**XT bit handling rules:**
- The SMMU ignores XT on PRI requests and ATS Invalidation completions (XT is always 0 on these).
- ATS Translation Completions carry back the XT bit value from the corresponding Translation Request.
- The PCIe Root Port sets XT=0 on PRI responses and ATS Invalidation requests.

**Translated transactions with XT from Realm streams:** When `STE.NSCFG == 0b01` and an ATS Translated transaction has `SEC_SID = Realm` and `XT = 0`, the transaction may be permitted to access the Non-secure PA space (implementation-defined behavior per §3.9.4.3).

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
- [command-queue.md](command-queue.md) — Queue mechanics; CMD_ATC_INV and CMD_PRI_RESP issued from Command queue
- [command-formats.md](command-formats.md) — CMD_ATC_INV and CMD_PRI_RESP encoding details (§4.5)
- [translation-hardening.md](translation-hardening.md) — AssuredOnly checks apply to ATS Translation Requests; split-stage Translated transactions exempt (SMMUv3.4 FEAT_THE)
- [../synthesis/smmu-pcie-ats-integration.md](../synthesis/smmu-pcie-ats-integration.md) — implementation-level reference: EATS dispatch table, full ATS/split-stage flows, PRI queue mechanics (PPR format, PRG, overflow), ATS configuration procedures, security state routing, CXL interactions
- [../synthesis/smmu-register-map.md](../synthesis/smmu-register-map.md) — PRIQ_BASE, PRIQ_PROD, PRIQ_CONS register addresses; GATOS register layout for ATS debugging

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.9 Support for PCI Express, PASIDs, PRI, and ATS; §3.9.1–3.9.1.5 ATS Interface and Translated transaction handling; §3.9.2 Changing ATS configuration; §3.9.3 SMMU interactions with CXL; §3.9.4 PCIe fields T, TE and XT; §4.5 ATS and PRI commands; Chapter 8 PRI queue
