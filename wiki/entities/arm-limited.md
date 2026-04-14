---
title: "Arm Limited"
type: entity
entity-type: org
tags: [arm, semiconductor, ip, architecture]
created: 2026-04-07
updated: 2026-04-07
sources: [ihi0070g-b-smmuv3-architecture-spec]
---

# Arm Limited

**Type:** org
**Also known as:** Arm, ARM Limited

## Overview

Arm Limited is a semiconductor IP company headquartered in Cambridge, England (Company 02557590, 110 Fulbourn Road, Cambridge CB1 9NJ). Arm designs processor architectures, memory management units, and related IP that are licensed to chip manufacturers. Arm does not manufacture chips directly.

## What We Know

- Arm is the author and publisher of the SMMUv3 architecture specification ([[sources/ihi0070g-b-smmuv3-architecture-spec]]).
- Arm defines the A-profile architecture (Armv8-A, Armv9-A), of which the SMMU is a companion component.
- The SMMU architecture is versioned alongside Armv8.x-A feature sets; each SMMUv3.x release adds features corresponding to new A-profile PE capabilities.

## Role in Sources

- [[sources/ihi0070g-b-smmuv3-architecture-spec]] — sole author; specification governs all compliant SMMU implementations

## Related

- [[concepts/two-stage-translation]]
- [[concepts/security-states]]
