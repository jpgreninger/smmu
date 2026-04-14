---
title: "Debug and Trace"
type: concept
tags: [smmu, debug, trace, implementation-defined, security]
created: 2026-04-13
updated: 2026-04-13
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Debug and Trace

## Definition

Chapter 11 of the SMMUv3 specification covers debug and trace support. The SMMU architecture does **not mandate** any specific debug features — all debug facilities are IMPLEMENTATION DEFINED. The chapter establishes constraints that any implementation-defined debug mechanism must satisfy.

## Security Constraints (Mandatory)

The following constraints are normative regardless of what debug features are implemented:

1. **Non-secure / Realm isolation:** A Non-secure or Realm debug agent must not be able to access any facilities related to Secure transaction handling.
2. **Realm isolation:** A Non-secure or Secure debug agent must not be able to access any facilities related to Realm transaction handling.
3. **Root isolation:** Any agent not associated with Root state must not be able to access any facilities related to Root state.

These constraints mirror the general Security state isolation model throughout the SMMU architecture (see [[concepts/security-states]]).

## Recommended Debug Mechanism

Arm **recommends** (not requires) that an implementation provides an IMPLEMENTATION DEFINED mechanism to read the contents of translation and configuration cache structures (TLBs, STE caches, CD caches, etc.) for diagnostic purposes. If such a mechanism is provided, it must not violate the Security state constraints above.

## IMPLEMENTATION DEFINED Scope

Because all debug features are IMPLEMENTATION DEFINED, the following are outside the normative architecture:

- Specific debug register layout or base address.
- Mechanisms for injecting faults or forcing cache misses.
- Trace output formats or interfaces.
- Export of translation table walk traces or event traces.

Implementations targeting functional safety or high-assurance environments typically extend debug facilities significantly beyond the architectural minimum, but these extensions are not interoperable across implementations.

## Relationship to Performance Monitors

The [[concepts/performance-monitors]] (Chapter 10) provide a standardized interface for counting architectural events. Debug/trace facilities (Chapter 11) are complementary but architecturally separate. A PMCG-based capture mechanism (`SMMU_PMCG_CAPR`) can provide limited diagnostic visibility through the standardized interface.

## Related Concepts

- [[concepts/security-states]] — Security state isolation constraints apply equally to debug access
- [[concepts/performance-monitors]] — Standardized event counting; complement to implementation-defined debug
- [[concepts/ras]] — RAS error records provide additional diagnostic visibility for faults

## Sources That Use This Concept

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — Chapter 11 Debug/Trace
