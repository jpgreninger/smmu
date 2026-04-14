# Wiki Index

_Last updated: 2026-04-13 | Pages: 39_

## Sources
| Page | Summary | Date | Tags |
|------|---------|------|------|
| [[sources/ihi0070g-b-smmuv3-architecture-spec]] | Arm SMMUv3 Architecture Specification v G.b — definitive reference for SMMUv3.0–3.4, RME, RME DA | 2026-04-07 | smmu, arm, iommu, virtualization, security, rme, pcie |

## Entities
| Page | Type | Summary |
|------|------|---------|
| [[entities/arm-limited]] | org | Semiconductor IP company; author of the SMMUv3 specification |

## Concepts
| Page                                   | Summary                                                                                                    |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| [[concepts/two-stage-translation]]     | VA→IPA (stage 1) and IPA→PA (stage 2) translation pipeline; precise lookup sequence and address size rules |
| [[concepts/stream-table-entry]]        | Per-stream configuration structure (STE); stream table formats, Config encoding, stage 2 fields            |
| [[concepts/context-descriptor]]        | Stage 1 translation configuration (CD); TTB0/TTB1, ASID, fault flags, validity conditions                  |
| [[concepts/streamid-substreamid]]      | Device identification (StreamID) and process identification (SubstreamID/PASID) for SMMU lookup            |
| [[concepts/command-queue]]             | Software-to-SMMU circular buffer; circular buffer mechanics, commands, CMD_SYNC, ECMDQ                     |
| [[concepts/event-queue]]               | SMMU-to-software circular buffer; event types, commit semantics, stall event buffering, overflow           |
| [[concepts/fault-models]]              | Terminate and Stall fault models; configurable per CD (stage 1) and STE (stage 2)                          |
| [[concepts/security-states]]           | Non-secure, Secure, Realm, Root states; independent queues/tables; SEC_SID disambiguation                  |
| [[concepts/granule-protection-check]]  | RME GPC/GPT — physical address space ownership check applied to Realm stream PA outputs                    |
| [[concepts/pcie-ats-pri]]              | PCIe ATS/PRI — Translation Requests, Translated transactions, STE.EATS, ATC invalidation, CXL              |
| [[concepts/tlb-invalidation]]          | TLB entry tagging (ASID/VMID), command-based invalidation, broadcast TLB maintenance                       |
| [[concepts/smmu-initialization]]       | Reset state, initialization sequence, queue setup, SMMUEN/CR0ACK handshake                                 |
| [[concepts/httu]]                      | Hardware Translation Table Update — access flag and dirty state automatic update                           |
| [[concepts/device-permission-table]]   | DPT — RME DA per-device PA-space gate for ATS Translated transactions (EATS=0b11)                          |
| [[concepts/virtual-machine-structure]] | VMS — per-VM configuration structure (SMMUv3.2+); cached, invalidated via CMD_CFGI_VMS_PIDM                |
| [[concepts/permission-indirections]]   | S1PIE/S2PIE/S2POE — permission index indirection and overlay (SMMUv3.4 / Armv8.9-A)                        |
| [[concepts/translation-hardening]]     | THE/AssuredOnly — Protected attribute and AssuredOnly permission checks (SMMUv3.4 / FEAT_THE)               |
| [[concepts/atos]]                      | Address Translation Operations — software-accessible ATOS/VATOS facility for debug and lookup               |
| [[concepts/performance-monitors]]      | PMCG — Performance Monitor Counter Groups; architected events, StreamID/PARTID filtering, overflow/capture  |
| [[concepts/debug-trace]]               | Debug and trace — IMPLEMENTATION DEFINED debug; mandatory Security state isolation constraints               |
| [[concepts/ras]]                       | RAS — Reliability/Availability/Serviceability; error classification, SFM, RAS registers, event mapping      |
| [[concepts/attribute-transformation]]  | Attribute transformation — how MT/SH/RA/WA/TR/INST/PRIV/NS are combined and overridden per transaction path |
| [[concepts/mpam]]                      | MPAM — Memory System Resource Partitioning and Monitoring; PARTID/PMG assignment rules (SMMUv3.2+)         |
| [[concepts/mec]]                       | MEC — Memory Encryption Contexts; MECID assignment for Realm PA space (RME DA only)                        |
| [[concepts/speculative-accesses]]      | Speculative transactions — write always abort+no-event; read abort-without-event on fault; speculative TRs  |
| [[concepts/coherency-and-embedded-implementations]] | COHACC; HTTU atomicity; client coherency; embedded preset tables/queues; STE/queue field storage rules |
| [[concepts/interrupts-and-power]]      | MSI/wired interrupts; 13 interrupt sources; MSI synchronization fences; power-off conditions; Dormant state |
| [[concepts/memory-tagging-extension]]  | MTE MAIR 0xF0 Reserved; SMMU accesses are Tag Unchecked; FEAT_MTE_PERM stage 2 MemAttr reinterpretation    |
| [[concepts/external-interfaces]]       | Ch. 14 ingress/egress sideband signals (StreamID, SEC_SID, AT, NS); port coherency; SMMU-originated PCIe transactions and deadlock risk |
| [[concepts/destructive-reads]]         | §3.22 RCI/DR/W-DCP/NW-DCP transaction classes; STE.DRE/DCP downgrade controls; permissions model; AMBA AXI5 Shareability constraints (SMMUv3.1+) |

## Analyses
| Page | Summary | Date |
|------|---------|------|

_No analyses yet._

## Synthesis
| Page                                     | Summary                                                                                                | Updated    |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------ | ---------- |
| [[synthesis/smmu-translation-pipeline]]  | Step-by-step procedural description of the full SMMU transaction translation pipeline; Ch. 15 flowchart prose equivalent and ATS response categories | 2026-04-13 |
| [[synthesis/smmu-queue-mechanics]]       | Complete implementation reference for SMMU circular buffer queues (Command, Event, PRI, ECMDQ)         | 2026-04-07 |
| [[synthesis/smmu-fault-model]]           | Complete fault detection, event recording, stall/terminate behavior, GERROR toggle handshake, and ordering guarantees | 2026-04-13 |
| [[synthesis/smmu-security-states]]       | Multi-security-state model reference: SEC_SID, Secure EL2, Realm, Root, independent operation          | 2026-04-07 |
| [[synthesis/smmu-pcie-ats-integration]]  | ATS/PRI integration: transaction types, EATS dispatch, invalidation flows, CXL, security state routing | 2026-04-07 |
| [[synthesis/smmu-version-feature-map]]   | Feature-by-version table for SMMUv3.0–3.4 and RME DA; SMMU_AIDR encoding; discovery bit references     | 2026-04-07 |
| [[synthesis/smmu-register-map]]          | Complete SMMU register memory map: Page 0/1, Secure, Root, Realm pages; key IDR bits                   | 2026-04-13 |
| [[synthesis/smmu-system-implementation]] | System integration, caching architectures, CMOs, AMBA specifics, software requirements                 | 2026-04-13 |
