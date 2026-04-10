//! TDD test for BUG-AIDR-01.
//!
//! ARM SMMU v3 spec §6.3.8 `SMMU_AIDR`:
//!   Bits `[7:4]` `ArchMajorRev` — must be 1 for all `SMMUv3` implementations.
//!   Bits `[3:0]` `ArchMinorRev` — 0=v3.0, 1=v3.1, 2=v3.2, 3=v3.3.
//!
//! BUG: `get_aidr()` returns `0x02` (`ArchMajorRev=0`, `ArchMinorRev=2`).
//!      The spec mandates `ArchMajorRev==1` in bits `[7:4]`, so the correct
//!      value for SMMUv3.2 is `0x12`.
//!
//! BEFORE FIX: `get_aidr() == 0x02` — `ArchMajorRev == 0` (WRONG)
//! AFTER FIX:  `get_aidr() == 0x12` — `ArchMajorRev == 1` (CORRECT)

use smmu::SMMU;

fn make_smmu() -> SMMU {
    SMMU::new()
}

/// §6.3.8: `SMMU_AIDR` bits `[7:4]` must be 1 (`ArchMajorRev` for `SMMUv3`).
#[test]
fn test_aidr_archmajorrev_is_one() {
    let smmu = make_smmu();
    let aidr = smmu.get_aidr();
    let arch_major_rev = (aidr >> 4) & 0xF;
    assert_eq!(arch_major_rev, 1, "AIDR ArchMajorRev (bits [7:4]) must be 1 for SMMUv3, got {arch_major_rev} (AIDR=0x{aidr:02x})");
}

/// §6.3.8: `SMMU_AIDR` bits `[3:0]` must be 2 for SMMUv3.2.
#[test]
fn test_aidr_archminorrev_is_two() {
    let smmu = make_smmu();
    let aidr = smmu.get_aidr();
    let arch_minor_rev = aidr & 0xF;
    assert_eq!(arch_minor_rev, 2, "AIDR ArchMinorRev (bits [3:0]) must be 2 for SMMUv3.2, got {arch_minor_rev} (AIDR=0x{aidr:02x})");
}

/// §6.3.8: Full `AIDR` value for SMMUv3.2 must be `0x12`.
#[test]
fn test_aidr_value_is_0x12_for_smmv3_2() {
    let smmu = make_smmu();
    assert_eq!(smmu.get_aidr(), 0x12, "AIDR must be 0x12 (ArchMajorRev=1, ArchMinorRev=2) for SMMUv3.2");
}
