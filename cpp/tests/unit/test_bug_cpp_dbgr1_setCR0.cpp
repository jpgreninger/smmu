// BUG-CPP-DBGR-1: setCR0() read-after-store and non-atomic stores (§6.3.9)
// TDD: failing test written BEFORE the fix.
// The fix ensures setCR0() derives smmuen_ from `value` directly (not by re-reading cr0_),
// and uses release stores for both cr0_ and smmuen_.
// enable() and disable() must also use atomic stores properly.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"

namespace smmu {
namespace test {

// -----------------------------------------------------------------------
// setCR0(CR0_SMMUEN) must make isEnabled() return true
// -----------------------------------------------------------------------
TEST(SetCR0Spec, SetCR0_SmmUenBit_EnablesSmmu) {
    SMMU smmu;
    EXPECT_FALSE(smmu.isEnabled()) << "SMMU must start disabled";
    smmu.setCR0(SMMU::CR0_SMMUEN);
    EXPECT_TRUE(smmu.isEnabled()) << "setCR0(CR0_SMMUEN) must set SMMUEN and make isEnabled() true";
}

// -----------------------------------------------------------------------
// setCR0(0) must make isEnabled() return false
// -----------------------------------------------------------------------
TEST(SetCR0Spec, SetCR0_Zero_DisablesSmmu) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN);
    ASSERT_TRUE(smmu.isEnabled());
    smmu.setCR0(0u);
    EXPECT_FALSE(smmu.isEnabled()) << "setCR0(0) must clear SMMUEN and make isEnabled() false";
}

// -----------------------------------------------------------------------
// getCR0() must return exactly what was stored by setCR0()
// -----------------------------------------------------------------------
TEST(SetCR0Spec, SetCR0_StoresCorrectValue) {
    SMMU smmu;
    uint32_t val = SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN;
    smmu.setCR0(val);
    EXPECT_EQ(smmu.getCR0(), val) << "getCR0() must return exactly the value stored by setCR0()";
}

// -----------------------------------------------------------------------
// enable() must set SMMUEN (isEnabled() == true)
// -----------------------------------------------------------------------
TEST(SetCR0Spec, Enable_SetsEnabled) {
    SMMU smmu;
    EXPECT_FALSE(smmu.isEnabled());
    smmu.enable();
    EXPECT_TRUE(smmu.isEnabled()) << "enable() must set SMMUEN via release store";
}

// -----------------------------------------------------------------------
// disable() must clear SMMUEN (isEnabled() == false)
// -----------------------------------------------------------------------
TEST(SetCR0Spec, Disable_ClearsEnabled) {
    SMMU smmu;
    smmu.enable();
    ASSERT_TRUE(smmu.isEnabled());
    smmu.disable();
    EXPECT_FALSE(smmu.isEnabled()) << "disable() must clear SMMUEN via release store";
}

// -----------------------------------------------------------------------
// enable() followed by disable() must leave isEnabled() == false
// -----------------------------------------------------------------------
TEST(SetCR0Spec, EnableThenDisable_ConsistentState) {
    SMMU smmu;
    smmu.enable();
    smmu.disable();
    EXPECT_FALSE(smmu.isEnabled()) << "disable() after enable() must result in disabled state";
    // CR0 SMMUEN bit must be 0
    EXPECT_EQ(smmu.getCR0() & SMMU::CR0_SMMUEN, 0u)
        << "CR0 SMMUEN bit must be 0 after disable()";
}

// -----------------------------------------------------------------------
// enable() must preserve queue enable bits in CR0 (§6.3.9)
// enable() also sets EVENTQEN/CMDQEN/PRIQEN per ARM spec
// -----------------------------------------------------------------------
TEST(SetCR0Spec, Enable_SetsQueueEnableBitsInCR0) {
    SMMU smmu;
    smmu.enable();
    uint32_t cr0 = smmu.getCR0();
    EXPECT_NE(cr0 & SMMU::CR0_SMMUEN, 0u) << "enable() must set SMMUEN in CR0";
    EXPECT_NE(cr0 & SMMU::CR0_EVENTQEN, 0u) << "enable() must set EVENTQEN in CR0";
    EXPECT_NE(cr0 & SMMU::CR0_CMDQEN, 0u) << "enable() must set CMDQEN in CR0";
}

// -----------------------------------------------------------------------
// disable() must clear SMMUEN but preserve other CR0 bits
// -----------------------------------------------------------------------
TEST(SetCR0Spec, Disable_ClearsSmmUenPreservesOtherBits) {
    SMMU smmu;
    smmu.enable();
    uint32_t cr0Before = smmu.getCR0();
    smmu.disable();
    uint32_t cr0After = smmu.getCR0();
    // SMMUEN must be cleared
    EXPECT_EQ(cr0After & SMMU::CR0_SMMUEN, 0u) << "disable() must clear CR0.SMMUEN";
    // Other bits that were set by enable() may still be set (only SMMUEN cleared)
    uint32_t otherBits = cr0Before & ~SMMU::CR0_SMMUEN;
    EXPECT_EQ(cr0After & ~SMMU::CR0_SMMUEN, otherBits)
        << "disable() must only clear SMMUEN, not other CR0 bits";
}

// -----------------------------------------------------------------------
// Sequence: setCR0 then enable then disable consistency
// -----------------------------------------------------------------------
TEST(SetCR0Spec, SetCR0_Then_EnableDisable_Sequence) {
    SMMU smmu;
    // Write arbitrary value without SMMUEN
    smmu.setCR0(SMMU::CR0_EVENTQEN);
    EXPECT_FALSE(smmu.isEnabled());
    // Now set SMMUEN
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN);
    EXPECT_TRUE(smmu.isEnabled());
    // Clear via setCR0(0)
    smmu.setCR0(0u);
    EXPECT_FALSE(smmu.isEnabled());
}

} // namespace test
} // namespace smmu
