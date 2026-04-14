---
title: "Security States"
type: concept
tags: [smmu, security, non-secure, secure, realm, root, rme, trustzone]
created: 2026-04-07
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# Security States

## Definition

The SMMU supports up to four security states, mirroring the Arm A-profile PE security model. Each state has its own programming interface (register set), Stream table, Command queue, Event queue, and optional PRI queue. The security state of an incoming transaction is determined by the `SEC_SID` sideband attribute, which disambiguates the StreamID namespace.

| Security State | `SEC_SID` Value | Register Prefix | Stream Table | Notes |
|---------------|----------------|-----------------|--------------|-------|
| Non-secure     | Non-secure      | `SMMU_*`        | Non-secure   | Always present |
| Secure         | Secure          | `SMMU_S_*`      | Secure       | Present when `SMMU_S_IDR1.SECURE_IMPL == 1` |
| Realm          | Realm           | `SMMU_R_*`      | Realm        | Present when `SMMU_ROOT_IDR0.REALM_IMPL == 1` |
| Root           | —               | `SMMU_ROOT_*`   | —            | Control-only; manages GPT, no stream traffic |

## Non-secure State

- Present in all SMMUs.
- Controlled by `SMMU_CR0.SMMUEN`.
- Transactions always target Non-secure PA space.
- No input NS attribute override (always Non-secure).
- Event queue: one global Non-secure Event queue for all Non-secure streams.

## Secure State

- Present when `SMMU_S_IDR1.SECURE_IMPL == 1`.
- Controlled by `SMMU_S_CR0.SMMUEN`.
- Secure streams may target Secure PA, Non-secure PA, or both (determined by stage 1 table descriptors or STE.NSCFG).
- Input NS attribute: a client device provides an NS bit; a Secure STE may override it.
- `SMMU_S_CR0.SIF == 1` terminates instruction fetches from Secure streams targeting Non-secure PAs or Non-secure IPAs (in certain configurations).
- Secure stage 2: supported when `SMMU_S_IDR1.SEL2 == 1` (SMMUv3.2+). Enables Secure EL2 translation regime with two IPA spaces (Secure IPA and Secure-stream Non-secure IPA), each with separate translation tables.
- Two programming interfaces operate independently (separate queues, separate enable, separate error state). Some Secure commands can affect Non-secure state.
- StreamWorld: differentiates Secure EL1 from EL3 translation regimes for TLB tagging. Arm recommends only one of Secure EL1 or EL3 uses the Secure Command queue at a time.

## Realm State (RME)

- Present when `SMMU_ROOT_IDR0.REALM_IMPL == 1`.
- Controlled by `SMMU_R_CR0.SMMUEN`.
- Supports the Arm CCA (Confidential Compute Architecture) / RME model.
- Realm streams use VMSAv8-64 or VMSAv9-128 translation tables only.
- Input NS attribute distinguishes Realm from Non-secure for Realm streams (using `NSE` signal in AMBA, or T-bit in PCIe IDE TLP prefix).
- Output PA space is Realm by default; determined by stage 2 for nested configurations.
- Realm STEs, CDs, L1CDs, L1STDs: same format as Non-secure but all pointers are Realm physical addresses.
- Commands on Realm Command queue: `SSec == 1` is always CERROR_ILL; all commands apply to Realm StreamIDs only.
- In PCIe: T=1 in IDE TLP prefix → SEC_SID=Realm; T=0 or no prefix → SEC_SID=Non-secure.
- RME DA (Delegated Assignment, added in G.b): extends RME with additional required features including XT bit support (TDISP), potential DPT support, and MECID (Memory Encryption Context ID) for Realm streams.

## Root State

- A control-only interface (no stream traffic associated with Root StreamIDs as a translation namespace).
- Manages the Granule Protection Table (GPT) base via `SMMU_ROOT_GPT_BASE`, `SMMU_ROOT_GPT_BASE_CFG`.
- `SMMU_ROOT_IDR0.ROOT_IMPL == 1` indicates Root register page is present.
- Root register access is restricted to the most privileged software (monitor/Root world).
- Controls Granule Protection Checks (GPC) that apply to all security states.

## SMMU Enable State Table

| `SMMU_CR0.SMMUEN` | `SMMU_S_CR0.SMMUEN` | Traffic |
|------------------|--------------------|---------|
| 0 | Unimplemented | All traffic bypasses/aborts per `SMMU_GBPA`. Always Non-secure PA. |
| 1 | Unimplemented | All traffic follows SMMU translation. Always Non-secure PA. |
| 0 | 0 | Both states controlled by respective GBPA registers. |
| 0 | 1 | Secure traffic follows SMMU flow; Non-secure bypasses/aborts. |
| 1 | 0 | Non-secure traffic follows SMMU flow; Secure bypasses/aborts. |
| 1 | 1 | Both states follow SMMU translation flow. |

## Permitted PA Spaces per Security State

| Incoming Security State | Permitted Target PA Spaces |
|------------------------|---------------------------|
| Non-secure             | Non-secure only |
| Secure                 | Secure, Non-secure |
| Realm                  | Realm, Non-secure (determined by translation/bypass config) |

## Two Programming Interfaces — Independence

When Secure state is implemented, the two programming interfaces (Non-secure and Secure) operate as logically separate SMMUs:
- Independent Command queues, Event queues, error state, enable flags.
- Queue full/overflow in one does not affect the other.
- Each has its own ATOS interface.
- Interrupts configured independently.
- Some Secure commands may affect Non-secure state (explicitly indicated in the spec).

## §3.6 Structure and Queue Ownership

The SMMU specification defines which software entity is expected to own each data structure:

| Structure | Expected Owner |
|---|---|
| Non-secure Stream table, Command queue, Event queue, PRI queue | Most privileged Non-secure system software |
| Secure Stream table, Secure Command queue, Secure Event queue | Secure software (e.g., EL3 or S-EL2) |
| Stage 2 translation tables (all STEs) | Hypervisor |
| Stage 1 CDs and translation tables — Secure STE | Secure software (EL3, S-EL2, or S-EL1) |
| Stage 1 CDs and translation tables — Non-secure STE | Non-secure software (NS-EL2 or NS-EL1) |
| Stage 1 CDs and translation tables — Realm STE | Realm software (Realm-EL2 or Realm-EL1) |

In guest VM scenarios: CDs and translation tables are controlled by the guest and addressed by IPA (not PA). The hypervisor is expected to manage the physical Event queue and forward events to guest VMs with StreamID remapping as needed.

**Hypervisor responsibilities in virtualization (§3.6):**
- Convert guest STEs into physical SMMU STEs, controlling permissions.
- Read and interpret commands from the guest Command queue; issue corresponding SMMU commands or invalidate shadowed structures.
- Consume PRI and Event queue entries and deliver them to guest queues with host→guest StreamID mapping.

## §3.8 Virtualization

The SMMU **does not provide** programming interfaces for use directly by virtual machines. Guest VMs interact with the SMMU through one of two mechanisms:

- **Stage 2-only:** Devices appear directly connected to the guest (DMA to IPA/PA). No guest interaction with the SMMU at all; hypervisor programs all STEs.
- **Hypervisor-emulated virtual SMMU:** When stage 1 facilities are required by a guest, the hypervisor emulates a virtual SMMU interface. The guest issues SMMU commands and receives events through the virtual interface; the hypervisor translates these to physical SMMU operations.

Implementations may provide an **IMPLEMENTATION DEFINED** number of extra hardware interfaces that are architecturally compatible with the SMMUv3 programming interface. These may be mapped directly into guest VMs (e.g., as a stage 1-only interface while the hypervisor's interface operates as stage 2-only). The management of such interfaces is outside the scope of the specification.

## Model Implementation Notes

- A complete model must implement the full SEC_SID disambiguation: physical StreamID value N refers to up to four distinct streams across security states when all states are supported.
- Secure stage 2 (SEL2) adds significant complexity: two IPA spaces with separate translation tables (`STE.S_S2TTB` and `STE.S2TTB`), and PA space determined by `S2SW`, `S2SA`, `S2NSW`, `S2NSA` fields.
- Realm state requires Granule Protection Checks (GPT lookups) on all transaction PA outputs — see [granule-protection-check.md](granule-protection-check.md).
- RME DA introduces MECID (Memory Encryption Context ID) on Realm streams; the `STE.MECID` field must be tracked.
- The Root register page does not process stream traffic; it configures GPT and other cross-state controls.

## Related Concepts

- [granule-protection-check.md](granule-protection-check.md) — GPC checks applied to Realm-state transactions
- [two-stage-translation.md](two-stage-translation.md) — each security state has independent translation pipeline
- [stream-table-entry.md](stream-table-entry.md) — Secure and Realm STEs are in separate stream tables
- [command-queue.md](command-queue.md) — separate Command queues per security state
- [event-queue.md](event-queue.md) — events routed to the queue matching the stream's security state
- [smmu-initialization.md](smmu-initialization.md) — separate enable sequences per security state
- [pcie-ats-pri.md](pcie-ats-pri.md) — T/TE/XT bits in PCIe map to Realm security state

## Sources That Use This Concept

- [../sources/ihi0070g-b-smmuv3-architecture-spec.md](../sources/ihi0070g-b-smmuv3-architecture-spec.md) — §3.10 Security states support; §3.10.1 StreamID Security state (SEC_SID); §3.10.2 Support for Secure state; §3.10.2.2 Secure EL2; §3.10.3 Support for Realm state; §2.6 SMMU for RME features; §2.7 SMMU for RME DA features
