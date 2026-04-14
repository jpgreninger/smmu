---
title: "SMMU Register Map and Memory Layout"
type: synthesis
tags: [smmu, registers, memory-map, page0, page1, secure, realm, root, vatos, ecmdq]
created: 2026-04-13
updated: 2026-04-13
sources: [../sources/ihi0070g-b-smmuv3-architecture-spec.md]
---

# SMMU Register Map and Memory Layout

## Physical Address Layout

The SMMU register space consists of mandatory and optional 64 KB pages starting from a 64 KB-aligned base address. All pages are 64 KB in size and 64 KB-aligned.

| Page | Condition | Description |
|---|---|---|
| Page 0 (mandatory) | Always | Non-secure and Secure registers (`SMMU_*` and `SMMU_S_*`) |
| Page 1 (mandatory) | Always | Event queue and PRI queue PROD/CONS (NS and S) — prevents high-frequency polling from disturbing config page |
| VATOS page | `SMMU_IDR0.VATOS == 1` | Virtual ATOS registers; base at `SMMU_IDR2.BA_VATOS` |
| S_VATOS page | Secure impl + `SEL2 == 1` + VATOS | Secure VATOS registers; base at `SMMU_S_IDR2.BA_S_VATOS` |
| ECMDQ control page(s) | `SMMU_IDR1.ECMDQ == 1` | Enhanced Command Queue interfaces; count in `SMMU_IDR6` |
| Root Control Page | `SMMU_ROOT_IDR0.ROOT_IMPL == 1` | Root-only configuration (GPT, RME); IMPL DEFINED base |
| Realm Page 0 | `SMMU_ROOT_IDR0.REALM_IMPL == 1` | Realm programming interface registers |
| Realm Page 1 | Adjacent to Realm Page 0 | Realm queue PROD/CONS |

VATOS page base address formula: `SMMU_BASE + 0x20000 + (SMMU_IDR2.BA_VATOS × 0x10000)`.
S_VATOS page base: `SMMU_BASE + 0x20000 + (SMMU_S_IDR2.BA_S_VATOS × 0x10000)`.

## Access Rules

- All registers are 32-bit, little-endian by default; 64-bit registers are composed of two 32-bit halves.
- Aligned 32-bit word access is always supported; 64-bit atomic access is IMPLEMENTATION DEFINED.
- `SMMU_S_*` registers are RAZ/WI to Non-secure access when `SMMU_S_IDR1.SECURE_IMPL == 1`; RES0 when `== 0`.
- In an SMMU with RME: Secure-only registers are additionally accessible in Root PA space; Non-secure accessible registers are additionally accessible in Root and Realm PA spaces.
- Undefined/reserved register locations in Page 0 are RES0.
- Page 1 undefined locations: CONSTRAINED UNPREDICTABLE — either RES0 or alias of Page 0.
- **Guarded fields:** Many configuration fields are Guarded by enable bits (e.g., `SMMU_CMDQ_BASE` guarded by `SMMU_CR0.CMDQEN`). Software must not change a Guarded field unless its guard is clear, and must use barriers to ensure visibility.

## Page 0 Key Registers

### Identification (RO, constant after reset)
| Offset | Register | Purpose |
|---|---|---|
| 0x0000 | `SMMU_IDR0` | Feature capabilities: translation support, security states, ATS, HTTU, ATOS, VATOS, COHACC, MSI, SIDs |
| 0x0004 | `SMMU_IDR1` | Implementation limits: queue preset, SIDSIZE, ASID/VMID size, ECMDQ, ATTR overrides |
| 0x0008 | `SMMU_IDR2` | Table/queue maximum sizes; VATOS base address |
| 0x000C | `SMMU_IDR3` | Feature flags: MPAM, FWB, S1PIE, S2PIE, S2POE, THE, GPC, DPT |
| 0x0010 | `SMMU_IDR4` | Larger feature capability bits |
| 0x0014 | `SMMU_IDR5` | OAS (output address size), granule sizes, table walk memory attributes |
| 0x0018 | `SMMU_IIDR` | Implementation identity |
| 0x001C | `SMMU_AIDR` | Architecture version (`0x00`=SMMUv3.0 … `0x04`=SMMUv3.4) |
| 0x0190 | `SMMU_IDR6` | ECMDQ count and control page information |

### Control and Status
| Offset | Register | Purpose |
|---|---|---|
| 0x0020 | `SMMU_CR0` | Primary enable: SMMUEN, CMDQEN, EVENTQEN, PRIQEN, ATSCHK, VMW |
| 0x0024 | `SMMU_CR0ACK` | Handshake acknowledgment of CR0 writes (shadow register, poll until match) |
| 0x0028 | `SMMU_CR1` | Queue and table walk memory attributes (inner/outer type, shareability) |
| 0x002C | `SMMU_CR2` | ATS/REC controls: E2H, PTM, RECINVSID, REC_CFG_ATS |
| 0x0030 | `SMMU_S2PII` | Stage 2 permission indirection index register (SMMUv3.4) |
| 0x0040 | `SMMU_STATUSR` | Status: DORMANT |
| 0x0044 | `SMMU_GBPA` | Global bypass attribute overrides when SMMUEN=0 |
| 0x0048 | `SMMU_AGBPA` | Additional global bypass (PCIe No_snoop, etc.) |

### Interrupt and Error Reporting
| Offset | Register | Purpose |
|---|---|---|
| 0x0050 | `SMMU_IRQ_CTRL` | Interrupt enable: GERROR, EVENTQ, PRIQ |
| 0x0054 | `SMMU_IRQ_CTRLACK` | IRQ_CTRL acknowledgment |
| 0x0060 | `SMMU_GERROR` | Global error status bits (RO) |
| 0x0064 | `SMMU_GERRORN` | Software-written to acknowledge errors (toggle bits) |
| 0x0068–0x0074 | `SMMU_GERROR_IRQ_CFG{0,1,2}` | GERROR interrupt MSI address/data |

### Stream Table
| Offset | Register | Purpose |
|---|---|---|
| 0x0080 | `SMMU_STRTAB_BASE` | Stream table base PA; tagging bits for RA/KOHINT |
| 0x0088 | `SMMU_STRTAB_BASE_CFG` | Stream table format (linear/2-level), log2(size), split |

### Command Queue
| Offset | Register | Purpose |
|---|---|---|
| 0x0090 | `SMMU_CMDQ_BASE` | Command queue base PA + log2(size) |
| 0x0098 | `SMMU_CMDQ_PROD` | Producer index (software write) |
| 0x009C | `SMMU_CMDQ_CONS` | Consumer index + error status (SMMU write, software read) |

### Event Queue
| Offset | Register | Purpose |
|---|---|---|
| 0x00A0 | `SMMU_EVENTQ_BASE` | Event queue base PA + log2(size) |
| Page 1: 0x00A8 | `SMMU_EVENTQ_PROD` | Producer index (SMMU write) — on Page 1 |
| Page 1: 0x00AC | `SMMU_EVENTQ_CONS` | Consumer index (software write) — on Page 1 |
| 0x00B0–0x00BC | `SMMU_EVENTQ_IRQ_CFG{0,1,2}` | Event queue interrupt MSI config |

### PRI Queue
| Offset | Register | Purpose |
|---|---|---|
| 0x00C0 | `SMMU_PRIQ_BASE` | PRI queue base PA + log2(size) |
| Page 1: 0x00C8 | `SMMU_PRIQ_PROD` | PRI queue producer (SMMU write) — on Page 1 |
| Page 1: 0x00CC | `SMMU_PRIQ_CONS` | PRI queue consumer (software write) — on Page 1 |
| 0x00D0–0x00DC | `SMMU_PRIQ_IRQ_CFG{0,1,2}` | PRI interrupt MSI config |

### ATOS
| Offset | Register | Purpose |
|---|---|---|
| 0x0100 | `SMMU_GATOS_CTRL` | ATOS control (NS) |
| 0x0108 | `SMMU_GATOS_SID` | StreamID and SubstreamID for ATOS lookup |
| 0x0110 | `SMMU_GATOS_ADDR` | Input address and access attributes |
| 0x0118 | `SMMU_GATOS_PAR` | Output PA or fault code (RO) |

### MPAM
| Offset | Register | Purpose |
|---|---|---|
| 0x0130 | `SMMU_MPAMIDR` | NS PARTID/PMG capability |
| 0x0138 | `SMMU_GMPAM` | MPAM attributes for SMMU-originated transactions |
| 0x013C | `SMMU_GBPMPAM` | MPAM attributes for global bypass transactions |

### DPT (RME DA)
| Offset | Register | Purpose |
|---|---|---|
| 0x0200 | `SMMU_DPT_BASE` | DPT base PA |
| 0x0208 | `SMMU_DPT_BASE_CFG` | DPT configuration |
| 0x0210 | `SMMU_DPT_CFG_FAR` | DPT fault address record |

### ECMDQ Discovery (Page 0 high offsets)
| Offset | Register | Purpose |
|---|---|---|
| 0x4000 + 32×n | `SMMU_CMDQ_CONTROL_PAGE_BASEn` | Base address of NS ECMDQ control page n |
| 0x4008 + 32×n | `SMMU_CMDQ_CONTROL_PAGE_CFGn` | Configuration of ECMDQ page n |
| 0x400C + 32×n | `SMMU_CMDQ_CONTROL_PAGE_STATUSn` | Status of ECMDQ page n |

## Secure Registers (Page 0 offset 0x8000)

The Secure register block mirrors the Non-secure block starting at offset 0x8000. Key registers follow the `SMMU_S_*` naming pattern with identical structure:

`SMMU_S_IDR{0-4}`, `SMMU_S_CR{0,0ACK,1,2}`, `SMMU_S_S2PII`, `SMMU_S_INIT`, `SMMU_S_GBPA`, `SMMU_S_AGBPA`, `SMMU_S_IRQ_CTRL{,ACK}`, `SMMU_S_GERROR{,N,_IRQ_CFG*}`, `SMMU_S_STRTAB_BASE{,_CFG}`, `SMMU_S_CMDQ_{BASE,PROD,CONS}`, `SMMU_S_EVENTQ_{BASE,PROD,CONS,IRQ_CFG*}`, `SMMU_S_GATOS_{CTRL,SID,ADDR,PAR}`, `SMMU_S_{MPAMIDR,GMPAM,GBPMPAM}`.

`SMMU_S_INIT` (0x803C): Secure-only initialization and invalidation control. Can be accessed by Non-secure software only when specifically permitted (see §3.11).

## Root Control Page

Accessible only from Root PA space. Contains:
- `SMMU_ROOT_IDR0`, `SMMU_ROOT_IIDR`: Root capability discovery.
- `SMMU_ROOT_CR0{,ACK}`: Root enable/acknowledge.
- `SMMU_ROOT_GPT_BASE{,_CFG}`, `SMMU_ROOT_GPT_BASE2`: Granule Protection Table base address and configuration (see [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md)).
- `SMMU_ROOT_GPF_FAR`, `SMMU_ROOT_GPT_CFG_FAR`: GPC fault address records.
- `SMMU_ROOT_TLBI{,_CTRL}`: Optional Root-level TLB invalidation.
- `SMMU_ROOT_GPT_BASE_UPDATE`: Atomic GPT base pointer update control.

## Realm Pages (0 and 1)

Accessible from Realm and Root PA spaces. Contain Realm-state equivalents of all major control registers:
- `SMMU_R_IDR{0-6}`, `SMMU_R_CR{0,0ACK,1,2}`, `SMMU_R_INIT`
- `SMMU_R_STRTAB_BASE{,_CFG}`, queue registers, GATOS, MPAM, DPT registers
- `SMMU_R_MECIDR`, `SMMU_R_GMECID` (MEC, RME DA only)
- Queue PROD/CONS registers on Realm Page 1

## Key IDR0 Feature Bits

| Bit/Field | Feature |
|---|---|
| `S1P` | Stage 1 translation supported |
| `S2P` | Stage 2 translation supported |
| `COHACC` | Coherent table walk supported |
| `ATS` | PCIe ATS/PRI supported |
| `HTTU[1:0]` | HTTU: AF only, AF+HD, or none |
| `ATOS` | Software ATOS interface supported |
| `VATOS` | VATOS interface supported |
| `MSI` | MSI interrupts supported |
| `SEV` | Stall and Stall enable event supported |
| `S1PI` | Stage 1 permission indirections (SMMUv3.4) |
| `S2PI` | Stage 2 permission indirections (SMMUv3.4) |

## Key IDR3 Feature Bits

| Bit/Field | Feature |
|---|---|
| `MPAM` | MPAM supported (SMMUv3.2+) |
| `FWB` | Force-Write-Back memory type override (Armv8.4) |
| `S1PI` | S1PIE (SMMUv3.4) |
| `S2PI` | S2PIE (SMMUv3.4) |
| `S2PO` | S2POE (SMMUv3.4) |
| `THE` | Translation Hardening Extension (SMMUv3.4) |
| `GPC` | Granule Protection Check (RME) |
| `PASIDTT` | PASID determination from Translated transactions |

## Related Pages

- [../concepts/smmu-initialization.md](../concepts/smmu-initialization.md) — CR0/CR0ACK handshake and initialization sequence
- [../concepts/command-queue.md](../concepts/command-queue.md) — CMDQ_BASE, PROD, CONS register usage
- [../concepts/event-queue.md](../concepts/event-queue.md) — EVENTQ register usage; Page 1 PROD/CONS
- [../concepts/pcie-ats-pri.md](../concepts/pcie-ats-pri.md) — PRIQ register usage; GATOS for ATS debugging
- [../concepts/security-states.md](../concepts/security-states.md) — Secure register block (0x8000), Root Control Page, Realm Pages
- [../concepts/granule-protection-check.md](../concepts/granule-protection-check.md) — Root Control Page GPT registers
- [../concepts/atos.md](../concepts/atos.md) — GATOS/VATOS register groups
- [../concepts/mpam.md](../concepts/mpam.md) — MPAMIDR, GMPAM, GBPMPAM registers
- [../concepts/mec.md](../concepts/mec.md) — SMMU_R_MECIDR, SMMU_R_GMECID
- [../synthesis/smmu-version-feature-map.md](../synthesis/smmu-version-feature-map.md) — IDR0/IDR3 feature bits by version
