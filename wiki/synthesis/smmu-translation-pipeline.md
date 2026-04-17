---
title: "SMMU Translation Pipeline"
type: synthesis
tags: [smmu, translation, pipeline, model, functional, ats, translation-procedure]
created: 2026-04-07
updated: 2026-04-16
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# SMMU Translation Pipeline

A complete procedural description of how the SMMU processes an incoming client device transaction from arrival to output PA. This page is intended as the primary reference for functional model implementers.

## Input to the Pipeline

Each incoming transaction carries:
- **Address** — 64-bit input address (VA, IPA, or bus address).
- **Read/Write** — access type.
- **StreamID** — device identifier (0–32 bits, IMPLEMENTATION DEFINED size).
- **SubstreamID + SSV** — optional process identifier (0–20 bits) and valid flag.
- **SEC_SID** — security state of the transaction (Non-secure, Secure, Realm).
- **AT field** — transaction type: Untranslated (0b00), Translation Request (0b01), Translated (0b10).
- **NS** — input NS attribute (relevant for Secure and Realm streams).
- Memory attributes: Shareability, Cacheability, transaction type modifiers.

## Step-by-Step Pipeline

### Step 0: Global Enable Check

- If `SMMU_(*_)CR0.SMMUEN == 0` for the relevant security state:
  - If `SMMU_(*_)GBPA.ABORT == 1`: abort transaction.
  - Else: apply bypass attributes from `SMMU_(A)GBPA` / `SMMU_S_(A)GBPA` and forward transaction.
  - **Exit pipeline.**

### Step 1: StreamID Range Check

- Look up the Stream table base for the transaction's security state.
- Check StreamID against configured table size (`SMMU_(*_)STRTAB_BASE_CFG`).
- If out of range → **C_BAD_STREAMID** event, abort transaction.

### Step 2: Stream Table Lookup (STE Fetch)

- For linear table: STE address = `STRTAB_BASE + StreamID × STE_size`.
- For 2-level table:
  - L1 index = `StreamID[n:SPLIT]`; fetch L1STD.
  - Check L1STD.Valid; if invalid → **C_BAD_STREAMID**, abort.
  - L2 index = `StreamID[SPLIT-1:0]`; check span. If out of span → **C_BAD_STREAMID**, abort.
  - STE address from L1STD.L2Ptr.
- Fetch STE from PA (Non-secure/Secure) or RA (Realm).
- If fetch causes external abort → **F_STE_FETCH**, abort.
- Validate STE (check V bit, ILLEGAL conditions): if invalid → **C_BAD_STE**, abort.

### Step 3: STE Configuration Decode

Decode `STE.Config[2:0]`:
- `0b000`: stream disabled → **F_STREAM_DISABLED**, abort.
- `0b100`: stream bypass → go to **Step 9** (apply attribute overrides, forward).
- `0b101`, `0b110`, `0b111`: translation configured. Continue.

Check for ATS-related faults (if AT != Untranslated):
- If `STE.EATS == 0b00` and AT == Translation Request → **F_BAD_ATS_TREQ**, abort.
- If AT == Translated and `ATSCHK == 1`: apply Translated transaction checks (see [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md)).

### Step 4: SubstreamID Handling (if stage 1 enabled)

- If `SSV == 1` (SubstreamID provided) and stage 1 is enabled:
  - Check SubstreamID against `STE.S1CDMax`. If out of range → **C_BAD_SUBSTREAMID**, abort.
- If `SSV == 0` and substreams configured, apply `STE.S1DSS` logic.
- Compute CD address from `STE.S1ContextPtr` + SubstreamID index.
- CD address is IPA (if nested) or PA (if stage 1 only).

### Step 5: VMS Fetch (if STE.VMSPtr non-zero, SMMUv3.2+)

- Fetch VMS from PA address `STE.VMSPtr`.
- If fetch fails or VMS invalid → **C_BAD_STE**, abort.

### Step 6: CD Fetch and Validation (if stage 1 enabled)

- Fetch L1CD (if 2-level CD table) and then CD from address computed in Step 4.
- If nested: CD address is IPA → perform stage 2 translation of CD address before fetch.
  - Stage 2 fault during CD fetch → stage 2 fault event (F_WALK_EABT or stage 2 Translation fault), abort.
- If fetch causes external abort → **F_CD_FETCH**, abort.
- Validate CD (check V bit, ILLEGAL conditions): if invalid → **C_BAD_CD**, abort.

### Step 7: Stage 1 Translation Table Walk

Applies when stage 1 is configured to translate (`STE.Config` includes stage 1 and not bypassed by S1DSS).

- Select TTB0 or TTB1 based on VA[55] and TBI configuration.
- Input range check: VA must be sign-extended correctly or F_ADDR_SIZE.
- Walk translation table levels using `CD.TG0`/`CD.TG1`, `CD.T0SZ`/`CD.T1SZ`.
- For each table walk memory access:
  - If stage 2 enabled: translate walk address through stage 2 first.
  - External abort during walk → **F_WALK_EABT** (always terminate).
  - Stage 2 Translation fault during walk → stage 2 fault (configurable per STE.S2S).
- On reaching leaf descriptor:
  - AF == 0 and HTTU disabled → **F_ACCESS** (configurable per CD fault flags).
  - AF == 0 and HTTU enabled (`CD.HA == 1`) → hardware sets AF=1, continue.
  - Permission check fail → **F_PERMISSION** (configurable per CD fault flags).
  - Translation fault (invalid descriptor) → **F_TRANSLATION** (configurable per CD fault flags).
- Output IPA from stage 1. Check IPA against IPS cap → **F_ADDR_SIZE** if exceeded.
- If stage 1 bypass (S1DSS == 0b01, or STE.Config == stage 2 only): input VA passes directly as IPA.

### Step 8: Stage 2 Translation Table Walk

Applies when `STE.Config` includes stage 2.

- Input IPA; check against `STE.S2T0SZ` configured range → **F_TRANSLATION** (stage 2) if out of range.
- Walk stage 2 translation table using `STE.S2TTB`, `STE.S2TG`, `STE.S2T0SZ`.
- External abort during walk → **F_WALK_EABT** (always terminate).
- On reaching leaf descriptor:
  - AF == 0 and stage 2 HTTU disabled → **F_ACCESS** (configurable per STE.S2S/S2R).
  - Permission check fail → **F_PERMISSION** (configurable per STE.S2S/S2R).
  - Translation fault → **F_TRANSLATION** (configurable per STE.S2S/S2R).
- Check output PA against `STE.S2PS` (capped to OAS) → **F_ADDR_SIZE** (stage 2) if exceeded.
- Output PA.

### Step 9: Attribute Transformation

- Apply memory attribute overrides from STE and CD fields (MTCFG, SHCFG, MemAttr, etc.).
- Determine output NS attribute / PA space based on security state, STE.NSCFG, stage translation results.

### Step 10: Granule Protection Check (if RME)

- If `SMMU_ROOT_IDR0.ROOT_IMPL == 1` and transaction is from a Realm stream (or RME_IMPL mandates GPC):
  - Look up GPT for output PA.
  - If GPT lookup fails (access fault) or PA space mismatch → **GPC fault** (always terminate).

### Step 11: Forward Transaction

- Output PA, memory attributes, and PA space into system interconnect.

## Fault Handling at Each Step

Each fault above either:
- **Always terminates** with abort (configuration errors, F_WALK_EABT, GPC, F_TLB_CONFLICT, bypassed F_ADDR_SIZE).
- **Is configurable** (Translation-related faults during translation walks at either stage): behavior determined by CD.{S,R,A} for stage 1 and STE.{S2S,S2R} for stage 2. See [../concepts/fault-models.md](../concepts/fault-models.md).

## TLB and Configuration Cache Interaction

The SMMU may cache:
- **STE cache:** avoids re-fetching STE from memory.
- **CD / L1CD cache:** avoids re-fetching CD.
- **TLB:** avoids re-walking translation tables. Tagged by ASID (stage 1), VMID (stage 2), security state.
- **VMS cache** (SMMUv3.2+): avoids re-fetching VMS.
- **GPT / DPT cache:** avoids re-walking GPT/DPT.

All caches must be invalidated correctly after configuration changes.

## Version-Specific Variations

| Version | Notable pipeline additions |
|---------|---------------------------|
| SMMUv3.0 | Base pipeline |
| SMMUv3.1 | 52-bit addresses, PBHA, destructive read support |
| SMMUv3.2 | Secure EL2, VMS, range TLB invalidation, BBM |
| SMMUv3.3 | RME (Realm security state, GPT/GPC) |
| SMMUv3.4 / RME DA | DPT, S1PIE/S2PIE/S2POE, THE, D128, LPA2, MECID |

---

## Chapter 15: Translation Procedure (Flowcharts)

Chapter 15 of the SMMUv3 specification consists almost entirely of decision flowcharts (Figures 15.1–15.6) that graphically present the translation procedure described in prose elsewhere in the specification. The figures are image-only in the specification document and are not reproduced here. The step-by-step pipeline above provides the complete prose equivalent of those flowcharts.

The six flowcharts and their corresponding pipeline steps are:

| Figure | Title | Covered by |
|---|---|---|
| Figure 15.1 | Top-level translation procedure | Step 0–3 above |
| Figure 15.2 | Stage 1 translation | Step 4–7 above |
| Figure 15.3 | Stage 2 translation | Step 8 above |
| Figure 15.4 | ATS Translation Request handling | Step 3 + [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) §3.9 |
| Figure 15.5 | Translated transaction (ATSCHK) handling | [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) §ATSCHK |
| Figure 15.6 | SMMU-originated transaction flow | [../concepts/external-interfaces.md](../concepts/external-interfaces.md) §14.3 |

### §15.2 ATS Translation Request Response Categories

The specification states that Translation Request (AT == 0b01) responses fall into four mutually exclusive categories:

| Category | Condition | Response |
|---|---|---|
| Configuration error | ATS disabled (EATS == 0b00), or ATS configuration error (C_BAD_STREAMID, C_BAD_STE, etc.) | **Complete with Abort (CA) status** — signals misconfiguration to the Root Complex |
| ATS disabled for Security state | Secure ATS disabled or NS ATS disabled for the stream's security state | **Unsuccessful (UR) status** |
| Translation fault | Translation or Access fault (F_TRANSLATION, F_ACCESS, F_ADDR_SIZE) | **Successful response with R==0, W==0** — fault PA, zero permissions; endpoint flushes ATC entry |
| Permission fault | F_PERMISSION | **Successful response with partial permissions** — actual accessible permissions returned; ATC may cache the partial entry |

This design means the endpoint ATC can cache "no permission" entries, enabling the PRI/PASID flow to later request more permissions without reconfiguring the ATC.

**Scope exclusion:** The Chapter 15 flowcharts explicitly exclude from their scope: TLB conflict handling (§3.20.1), speculative transaction behavior (§3.14), attribute transformation details (Chapter 13), and REC_CFG_ATS reporting. These topics are covered in their respective wiki pages.

---

## Related Pages

- [../concepts/two-stage-translation.md](../concepts/two-stage-translation.md) — complete address size and translation semantics
- [../concepts/stream-table-entry.md](../concepts/stream-table-entry.md) — STE structure and Config encoding
- [../concepts/context-descriptor.md](../concepts/context-descriptor.md) — CD structure and stage 1 parameters
- [../concepts/fault-models.md](../concepts/fault-models.md) — fault behavior at each translation step
- [../concepts/tlb-invalidation.md](../concepts/tlb-invalidation.md) — cache maintenance
- [../concepts/security-states.md](../concepts/security-states.md) — SEC_SID governs which tables/queues are used
- [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md) — Step 10 detail
- [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) — AT field handling, ATS response semantics (§3.9.1.2), ATSCHK
- [../concepts/external-interfaces.md](../concepts/external-interfaces.md) — SMMU-originated transactions (§14.3); Chapter 14 ingress/egress port
- [../concepts/atos.md](../concepts/atos.md) — Address Translation Operations; ATOS_PAR encoding; GATOS/VATOS register groups
- [../concepts/translation-hardening.md](../concepts/translation-hardening.md) — AssuredOnly permission checks at stage 2; Protected attribute at stage 1 (SMMUv3.4)
- [smmu-fault-model.md](smmu-fault-model.md) — detailed fault model subsystem reference
- [smmu-security-states.md](smmu-security-states.md) — security state pipeline variations
- [smmu-pcie-ats-integration.md](smmu-pcie-ats-integration.md) — complete ATS/PRI dispatch model: EATS table, split-stage flows, PRI queue, security state routing
- [smmu-version-feature-map.md](smmu-version-feature-map.md) — version-gated feature availability for pipeline steps (SMMUv3.0–3.4, RME DA)
