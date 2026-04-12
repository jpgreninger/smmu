// §7.5 GERROR_DPT_ERR and §7.5.1 GERROR IRQ gating — conformance tests
//
// §7.5 BUG-GERROR-DPT-CPP:
//   ARM §6.3.17 defines bit 10 of SMMU_GERROR as DPT_ERR (DPT lookup fault,
//   §3.24.4). The C++ implementation was missing this constant.
//   Fix: added GERROR_DPT_ERR = (1u << 10) to types.h after GERROR_CMDQP_ERR.
//
// §7.5.1 GERROR IRQ gating — N/A for this software model:
//   The Rust implementation gates signalGerror() on GERROR_IRQEN. In the C++
//   model there is no IRQ notification layer (no IRQ callbacks, no GERROR_IRQEN
//   constant, no notifyIrq() method). The software model uses a pure register-
//   toggle approach (gerrorStatus XOR gerrorNStatus = active bits). Adding an
//   IRQ-enable gate would require introducing an IRQ callback infrastructure
//   that does not exist and is out of scope for this software simulation model.
//   This is documented here as N/A with justification.
//
// TDD: The GERROR_DPT_ERR test MUST fail before the constant is added to
// types.h (compile error: missing symbol). After the constant is added, the
// static_assert and runtime check both pass.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <cstdint>
#include <memory>

namespace smmu {
namespace test {

// ── §7.5 GERROR_DPT_ERR constant value ─────────────────────────────────────

// Compile-time check: GERROR_DPT_ERR must be exactly bit 10.
// This static_assert is the primary TDD assertion — it fails to compile when
// the constant is absent, and passes silently once the constant is added.
static_assert(GERROR_DPT_ERR == (1u << 10),
    "§7.5: GERROR_DPT_ERR must equal (1u << 10) per ARM §6.3.17");

TEST(GerrorDptErrTest, ConstantValueIsBit10) {
    // Runtime verification — belt-and-suspenders alongside the static_assert.
    EXPECT_EQ(GERROR_DPT_ERR, static_cast<uint32_t>(1u << 10))
        << "§7.5: GERROR_DPT_ERR must be bit 10 per ARM §6.3.17";
}

// ── §7.5 GERROR constant set completeness ──────────────────────────────────

// Verify all GERROR bit constants defined in §6.3.17 are present and correct,
// including the newly-added GERROR_DPT_ERR at bit 10.
TEST(GerrorDptErrTest, AllGerrorBitsMatchSpec) {
    EXPECT_EQ(GERROR_CMDQ_ERR,           static_cast<uint32_t>(1u << 0))
        << "§6.3.17: GERROR_CMDQ_ERR must be bit 0";
    EXPECT_EQ(GERROR_EVENTQ_ABT_ERR,     static_cast<uint32_t>(1u << 2))
        << "§6.3.17: GERROR_EVENTQ_ABT_ERR must be bit 2";
    EXPECT_EQ(GERROR_PRIQ_ABT_ERR,       static_cast<uint32_t>(1u << 3))
        << "§6.3.17: GERROR_PRIQ_ABT_ERR must be bit 3";
    EXPECT_EQ(GERROR_SFM_ERR,            static_cast<uint32_t>(1u << 8))
        << "§6.3.17: GERROR_SFM_ERR must be bit 8";
    EXPECT_EQ(GERROR_CMDQP_ERR,          static_cast<uint32_t>(1u << 9))
        << "§6.3.17: GERROR_CMDQP_ERR must be bit 9";
    // The newly-added constant:
    EXPECT_EQ(GERROR_DPT_ERR,            static_cast<uint32_t>(1u << 10))
        << "§6.3.17 §7.5 BUG-GERROR-DPT-CPP: GERROR_DPT_ERR must be bit 10";
}

// ── §7.5 GERROR_DPT_ERR does not overlap with existing constants ───────────

// Verify no bit collision between GERROR_DPT_ERR and neighbouring constants.
TEST(GerrorDptErrTest, DptErrBitHasNoCollision) {
    EXPECT_EQ(GERROR_CMDQP_ERR & GERROR_DPT_ERR, 0u)
        << "§6.3.17: GERROR_CMDQP_ERR (bit 9) must not overlap GERROR_DPT_ERR (bit 10)";
    EXPECT_EQ(GERROR_SFM_ERR & GERROR_DPT_ERR, 0u)
        << "§6.3.17: GERROR_SFM_ERR (bit 8) must not overlap GERROR_DPT_ERR (bit 10)";
}

// ── §7.5 GERROR signaling works for an existing GERROR bit ─────────────────

// Indirectly verify the GERROR signaling mechanism functions correctly by
// triggering GERROR_CMDQ_ERR via a bad command. This confirms that the
// getGerror() API used by the DPT_ERR constant test is reliable.
TEST(GerrorDptErrTest, GerrorSignalingMechanismWorks) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN);

    EXPECT_EQ(smmu.getGerror() & GERROR_CMDQ_ERR, 0u)
        << "Pre-condition: GERROR must be clear before test";

    // Trigger CMDQ_ERR indirectly via an illegal command.
    CommandEntry badCmd;
    badCmd.type        = static_cast<CommandType>(0xFF);
    badCmd.streamID    = 0u;
    badCmd.pasid       = 0u;
    badCmd.startAddress = 0u;
    badCmd.endAddress  = 0u;
    badCmd.flags       = 0u;
    badCmd.timestamp   = 0u;
    smmu.submitCommand(badCmd);
    smmu.processCommandQueue();

    EXPECT_NE(smmu.getGerror() & GERROR_CMDQ_ERR, 0u)
        << "§7.5: GERROR signaling mechanism must work for GERROR_CMDQ_ERR";
}

// ── §7.5.1 GERROR IRQ gating — N/A documentation test ─────────────────────

// This test documents why §7.5.1 IRQ gating is N/A for the C++ software model.
// It is intentionally a passing no-op assertion with explanatory message.
//
// Justification for N/A:
//   ARM §7.5.1 specifies that the GERROR IRQ is gated by GERRORN_IRQ_ENABLE.
//   The Rust implementation added a gerror_irq_pending flag + GERROR_IRQEN guard.
//   The C++ software model has NO IRQ notification infrastructure:
//     - No GERROR_IRQEN constant in types.h or smmu.h
//     - No IRQ callback, irqCallback, notifyIrq() method in SMMU
//     - No IRQEN register field in the SMMU register set
//   signalGerror() is a pure register-toggle function for simulation purposes.
//   Adding IRQ-enable gating requires a callback/notification layer that is
//   out of scope for this software simulation model.
TEST(GerrorIrqGatingTest, IrqGatingIsNaForSoftwareModel) {
    // Verify that the C++ model correctly uses register-toggle semantics
    // (GERROR XOR GERRORN = active bits) without any IRQ enable gating.
    // This is the correct behavior for a software simulation model.
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN);

    // getGerror() returns active bits = GERROR XOR GERRORN = 0 initially.
    EXPECT_EQ(smmu.getGerror(), 0u)
        << "§7.5.1 N/A: Software model starts with no active GERROR bits";

    // After triggering a CMDQ error, the bit is active without IRQ gating.
    CommandEntry badCmd;
    badCmd.type        = static_cast<CommandType>(0xFF);
    badCmd.streamID    = 0u;
    badCmd.pasid       = 0u;
    badCmd.startAddress = 0u;
    badCmd.endAddress  = 0u;
    badCmd.flags       = 0u;
    badCmd.timestamp   = 0u;
    smmu.submitCommand(badCmd);
    smmu.processCommandQueue();

    EXPECT_NE(smmu.getGerror() & GERROR_CMDQ_ERR, 0u)
        << "§7.5.1 N/A: Software model signals GERROR immediately without "
           "IRQ-enable gating (no IRQ callback infrastructure exists)";
}

} // namespace test
} // namespace smmu
