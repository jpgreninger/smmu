---
title: "External Interfaces"
type: concept
tags: [smmu, external-interface, ingress, egress, sec-sid, streamid, coherency, amba, pcie, httu]
created: 2026-04-13
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# External Interfaces

## Definition

Chapter 14 of the SMMUv3 specification defines the external interfaces through which the SMMU connects to the rest of the system: the data-path ingress/egress port carrying per-transaction sideband signals, the ATS/PRI protocol interface, and rules governing SMMU-originated transactions into the memory system.

---

## §14.1 Data Path Ingress/Egress Port

Every transaction arriving at the SMMU carries the following sideband signals in addition to address and read/write data:

| Signal | Description |
|---|---|
| `StreamID` | Device identifier; indexes into the Stream table. Passed through to the memory system as DeviceID for GICv3 ITS interrupt differentiation. |
| `SubstreamID` + `SSV` | Optional process identifier (PASID equivalent) and its valid flag. Present only when the endpoint supports PASID. |
| `SEC_SID` | Security state qualifier for the StreamID: Non-secure (0b00), Secure (0b01), Realm (0b10) in RME DA; 1-bit (0=Non-secure, 1=Secure) in non-RME implementations. Determines which Stream table (NS/S/Realm) is used for lookup. If the SMMU supports only Non-secure state, SEC_SID may be absent and is treated as Non-secure. |
| `AT` field | Transaction type: Untranslated (0b00), Translation Request (0b01), Translated (0b10). Controls ATS bypass behavior. |
| `NS` | Input Non-secure attribute; relevant for Secure and Realm streams determining output PA space. |
| Memory attributes | Memory type, Shareability, Cacheability, and allocation hints from the upstream device. All are optional and can be overridden per STE configuration. See [attribute-transformation.md](attribute-transformation.md). |

### Internal StreamID for SMMU MSIs

The SMMU generates MSI transactions using an internally-assigned StreamID. This StreamID **must differ** from all StreamIDs associated with client devices so that the GICv3 ITS can differentiate SMMU-generated MSIs from device-generated MSIs.

### Port Coherency Requirements

The interconnect port type between the SMMU and the downstream memory system determines which HTTU mechanisms are available:

| Port Type | Condition | Notes |
|---|---|---|
| Fully-coherent | Required if HTTU uses local monitor atomic updates | Enables `LDREX`/`STREX`-style atomic read-modify-write for Access flag and dirty state updates |
| IO-coherent | Sufficient if HTTU is not implemented, or if the memory system provides far atomics (Armv8.1 LSE) | Most common deployment; structure and table walks are IO-coherent |
| Either | For queue and MSI accesses | No stronger requirement than IO-coherent for queue mechanics |

The SMMU does **not** translate outgoing coherency or broadcast DVM invalidation traffic from client devices. Therefore, no DVM-capable interconnect is required between the SMMU and client devices. Client devices may connect via an IO-coherent port.

See [coherency-and-embedded-implementations.md](coherency-and-embedded-implementations.md) for the full coherency model including COHACC, HTTU atomicity rules, and client-side coherency behavior.

---

## §14.2 ATS Interface

An SMMU implementation may provide a separate interface to support ATS and PRI protocol with a compatible PCIe Root Complex. **This interface is outside the scope of the SMMUv3 specification.** The specification governs the SMMU's internal handling of Translation Requests, Translation Completions, and PRI messages; the physical ATS/PRI bus protocol between Root Complex and endpoint is defined by the PCIe specification.

See [pcie-ats-pri.md](pcie-ats-pri.md) for the SMMU-side ATS and PRI semantics.

---

## §14.3 SMMU-Originated Transactions

When the SMMU performs reads for translation table walks, configuration structure fetches (STE, CD, VMS), or queue accesses, these SMMU-originated transactions may target any address in the physical address space including PCIe-mapped addresses. The specification states:

> An SMMU read for any translation, configuration, or queue structure that is performed into any PCIe address space is **permitted to return any value** or be **terminated with an external abort**.

**Deadlock risk:** A potential deadlock can arise in the following scenario:
1. An SMMU table walk read targets a PCIe address.
2. The PCIe completion depends on an incoming PCIe write that is stalled.
3. The incoming PCIe write requires SMMU translation to proceed.
4. The SMMU translation is blocked waiting for the table walk read to complete — circular dependency.

The recommended mitigation is that the system terminates SMMU-originated accesses targeted to the PCIe domain. Arm expects SMMU structures and translation tables in non-embedded implementations to be located in **system memory**, not PCIe address space, eliminating this risk.

External aborts on SMMU-originated structure reads are reported as:
- `F_STE_FETCH` — STE fetch abort
- `F_CD_FETCH` — CD fetch abort
- `F_VMS_FETCH` — VMS fetch abort
- `F_WALK_EABT` — translation table walk abort

See [../synthesis/smmu-system-implementation.md](../synthesis/smmu-system-implementation.md) §16.7.4 for the full downstream abort event table.

---

## Related Concepts

- [streamid-substreamid.md](streamid-substreamid.md) — StreamID and SubstreamID semantics; SEC_SID interaction with Stream table selection
- [security-states.md](security-states.md) — SEC_SID encoding (NS/Secure/Realm/Root); which programming interface controls a stream
- [pcie-ats-pri.md](pcie-ats-pri.md) — AT field handling; Translation Request/Translated/Untranslated transaction types; ATS interface semantics
- [coherency-and-embedded-implementations.md](coherency-and-embedded-implementations.md) — COHACC flag; HTTU atomicity and coherency port requirements; client coherency model
- [attribute-transformation.md](attribute-transformation.md) — input memory attribute override rules (Chapter 13)
- [httu.md](httu.md) — HTTU atomic update mechanism and its port coherency dependency
- [../synthesis/smmu-system-implementation.md](../synthesis/smmu-system-implementation.md) — system integration requirements including downstream abort handling (§16.4, §16.7)

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §14.1 Data path ingress/egress ports; §14.2 ATS Interface; §14.3 SMMU-originated transactions; §3.10.1 SEC_SID; §3.13 HTTU coherency requirements
