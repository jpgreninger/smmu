// TDD failing test for BUG-13.2-CPP: GBPA bypass missing Device/NC→OSH enforcement.
//
// ARM §13.1.7 Rule 1: Device and Non-Cacheable memory types MUST use Outer Shareable
// (OSH=2) shareability regardless of the configured GBPA.SHCfg value.
//
// BUG: smmu.cpp GBPA bypass path (lines 305-312):
//     if (gbpa.mtCfg) { td.memType = gbpa.memAttr; }
//     td.shareability = gbpa.shCfg;   // BUG: no Device/NC→OSH enforcement
//
// When GBPA sets Device memory (memAttr=0x00) with ISH (shCfg=1), the result
// must have OSH (shareability=2), not ISH (shareability=1).
//
// The GBPA path fires when SMMUEN=0 (SMMU disabled), which causes all transactions
// to go through SMMU_GBPA for bypass/abort (ARM §6.3.9 / §3.11 / §13.2).
//
// These tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static void setGbpaAttrs(SMMU& smmu, bool mtCfg, uint8_t memAttr, uint8_t shCfg)
{
    GbpaConfig gbpa;
    gbpa.abort    = false;
    gbpa.mtCfg    = mtCfg;
    gbpa.memAttr  = memAttr;
    gbpa.shCfg    = shCfg;
    gbpa.allocCfg = 0;
    gbpa.instCfg  = 0;
    gbpa.privCfg  = 0;
    smmu.setGbpaConfig(gbpa);
}

// ── BUG-13.2-CPP Test 1 ─────────────────────────────────────────────────────
// GBPA: mtCfg=true, memAttr=0x00 (Device), shCfg=1 (ISH)
// Expected: shareability == 2 (OSH) per §13.1.7 Rule 1
// Before fix: shareability == 1 (ISH, unconditional copy from gbpa.shCfg)
// After  fix: shareability == 2 (Device memory forces OSH)
TEST(BugAudit132CppTest, GbpaDeviceMemory_ForcesOSH_NotISH)
{
    // SMMU with SMMUEN=0 (disabled) → GBPA bypass path is active
    SMMU smmu;
    // Do NOT call smmu.enable() — SMMUEN=0 triggers GBPA path

    // Set GBPA: Device memory (0x00), ISH shareability (1)
    setGbpaAttrs(smmu, /*mtCfg=*/true, /*memAttr=*/0x00u, /*shCfg=*/1u);

    TranslationResult result = smmu.translate(0x10u, 0u, 0x5000u,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "GBPA bypass translation must succeed (not abort)";

    // Device memory must force OSH (2), not use ISH (1) from gbpa.shCfg
    EXPECT_EQ(result.getValue().shareability, 2u)
        << "BUG-13.2-CPP: GBPA Device memory (0x00) must force OSH (shareability=2). "
           "§13.1.7 Rule 1: Device/NC memory must use Outer Shareable. "
           "Before fix: shareability=" << static_cast<int>(result.getValue().shareability)
        << " (ISH=1, copied unconditionally from gbpa.shCfg=1).";
    EXPECT_EQ(result.getValue().memType, 0x00u) << "memType must be Device (0x00)";
}

// ── BUG-13.2-CPP Test 2 ─────────────────────────────────────────────────────
// GBPA: Normal WB memory (0xFF), ISH (1) → shareability stays ISH (Rule 1 does not apply)
// This must pass both before and after the fix.
TEST(BugAudit132CppTest, GbpaNormalMemory_KeepsConfiguredSH)
{
    SMMU smmu;
    // Do NOT call smmu.enable() — SMMUEN=0 triggers GBPA path

    // Set GBPA: Normal WB memory (0xFF), ISH shareability (1)
    setGbpaAttrs(smmu, /*mtCfg=*/true, /*memAttr=*/0xFFu, /*shCfg=*/1u);

    TranslationResult result = smmu.translate(0x11u, 0u, 0x5000u,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "GBPA bypass translation must succeed";

    // Normal memory: shareability stays ISH (1) as configured
    EXPECT_EQ(result.getValue().shareability, 1u)
        << "Normal memory must keep configured shareability (ISH=1). "
           "§13.1.7 Rule 1 does not apply to Normal memory.";
    EXPECT_EQ(result.getValue().memType, 0xFFu) << "memType must be Normal (0xFF)";
}

// ── BUG-13.2-CPP Test 3 ─────────────────────────────────────────────────────
// GBPA: Device memory (0x00), OSH (2) → shareability stays OSH (already correct)
// This must pass before and after the fix (when the configured SH is already OSH).
TEST(BugAudit132CppTest, GbpaDeviceMemory_AlreadyOSH_Stays)
{
    SMMU smmu;
    // Do NOT call smmu.enable() — SMMUEN=0 triggers GBPA path

    // Set GBPA: Device memory (0x00), OSH shareability (2) — already correct
    setGbpaAttrs(smmu, /*mtCfg=*/true, /*memAttr=*/0x00u, /*shCfg=*/2u);

    TranslationResult result = smmu.translate(0x12u, 0u, 0x5000u,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    ASSERT_TRUE(result.isOk()) << "GBPA bypass translation must succeed";

    EXPECT_EQ(result.getValue().shareability, 2u)
        << "Device memory with OSH configured must remain OSH.";
    EXPECT_EQ(result.getValue().memType, 0x00u) << "memType must be Device (0x00)";
}

}  // namespace test
}  // namespace smmu
