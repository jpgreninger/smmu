// ARM SMMU v3 FINDING-L-04: Fault Syndrome Register Bit-Position Fix
//
// TDD spec tests for correct ARM IHI0070G.b §7.3 syndrome bit layout.
//
// The syndromeRegister field in FaultSyndrome maps event record bits
// [127:96] to a 32-bit value: bit N = event record bit (96 + N).
//
// Spec-correct bit layout per §7.3:
//   [2]   InD  — instruction fetch (event bit [98])
//   [3]   RnW  — write access      (event bit [99])
//   [7]   S2   — stage-2 fault     (event bit [103])
//   [9:8] CLASS — access class     (event bits [105:104])
//         00 = IN (input transaction)
//         01 = TT (translation table walk)
//         10 = CD (context descriptor fetch)
//
// Prior (buggy) encoding placed WnR at bit [6] and INST at bit [8],
// which does NOT match the SMMU v3 event record format.
//
// Two-stage streams (stage1Enabled=true, stage2Enabled=true) are used
// because only performBothStagesTranslation calls recordComprehensiveFault,
// which in turn calls generateFaultSyndrome -> encodeFaultSyndromeRegister.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>
#include <vector>

namespace smmu {
namespace test {

// ── §7.3 Syndrome bit masks ───────────────────────────────────────────────
// syndromeRegister bit N corresponds to event record bit (96 + N).

static constexpr uint32_t SYND_IND_BIT    = (1u << 2);   // InD: event bit [98]
static constexpr uint32_t SYND_RNW_BIT    = (1u << 3);   // RnW: event bit [99]
static constexpr uint32_t SYND_S2_BIT     = (1u << 7);   // S2:  event bit [103]
static constexpr uint32_t SYND_CLASS_MASK = (0x3u << 8); // CLASS: event bits [105:104]
static constexpr uint32_t SYND_CLASS_IN   = (0x0u << 8); // CLASS=IN (00)
static constexpr uint32_t SYND_CLASS_TT   = (0x1u << 8); // CLASS=TT (01)
static constexpr uint32_t SYND_CLASS_CD   = (0x2u << 8); // CLASS=CD (10)

// Wrong positions present in the buggy code — these must NOT be set.
static constexpr uint32_t OLD_WNR_BIT  = (1u << 6); // old wrong WnR at bit [6]
static constexpr uint32_t OLD_INST_BIT = (1u << 8); // old wrong INST at bit [8]
                                                      // (collides with CLASS[0])

// ── Test constants ────────────────────────────────────────────────────────

static constexpr StreamID TEST_STREAM = 0x50;
static constexpr PASID    TEST_PASID  = 0;
static constexpr IOVA     BASE_IOVA   = 0x0001'0000ULL;
static constexpr PA       BASE_PA     = 0x4000'0000ULL;

// ── Fixture ───────────────────────────────────────────────────────────────

class FaultSyndromeSpecTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = std::make_unique<SMMU>();

        // ARM §6.3.9: SMMU starts disabled; enable globally before tests.
        smmu->enable();

        // Create a two-stage stream so that all faults go through
        // performBothStagesTranslation -> recordComprehensiveFault ->
        // generateFaultSyndrome -> encodeFaultSyndromeRegister.
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        ASSERT_TRUE(smmu->configureStream(TEST_STREAM, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(TEST_STREAM).isOk());

        // PASID 0 must exist so stage-1 address space is available.
        ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM, TEST_PASID).isOk());
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper: retrieve the first FaultRecord whose accessType matches 'at'.
    // Returns a default (invalid) FaultSyndrome when none is found.
    FaultSyndrome getSyndromeForAccessType(AccessType at) {
        auto evResult = smmu->getEvents();
        if (!evResult.isOk()) {
            return FaultSyndrome{};
        }
        for (const FaultRecord& rec : evResult.getValue()) {
            if (rec.syndrome.validSyndrome && rec.accessType == at) {
                return rec.syndrome;
            }
        }
        return FaultSyndrome{};
    }

    // Helper: retrieve the first FaultRecord regardless of access type.
    FaultSyndrome getFirstValidSyndrome() {
        auto evResult = smmu->getEvents();
        if (!evResult.isOk()) {
            return FaultSyndrome{};
        }
        for (const FaultRecord& rec : evResult.getValue()) {
            if (rec.syndrome.validSyndrome) {
                return rec.syndrome;
            }
        }
        return FaultSyndrome{};
    }

    std::unique_ptr<SMMU> smmu;
};

// ── Test 1: Read fault — RnW bit [3] must be CLEAR ───────────────────────

/// §7.3: For a Read transaction that hits a Stage-1 translation fault
/// (no mapping), RnW (bit [3]) must be 0.
TEST_F(FaultSyndromeSpecTest, ReadFault_RnWBitClear) {
    // No page mapped — Stage-1 will fail, recording Stage1Only fault.
    IOVA unmapped = BASE_IOVA + 0x1000;
    smmu->translate(TEST_STREAM, TEST_PASID, unmapped, AccessType::Read);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Read);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded for Read";

    EXPECT_EQ(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3: RnW (bit [3]) must be 0 for a read transaction; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 2: Write fault — RnW bit [3] must be SET ────────────────────────

/// §7.3: For a Write transaction on a read-only Stage-1 page,
/// RnW (bit [3]) must be 1.
TEST_F(FaultSyndromeSpecTest, WriteFault_RnWBitSet) {
    // Map a read-only page so a Write access triggers a permission fault.
    PagePermissions readOnly(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM, TEST_PASID, BASE_IOVA, BASE_PA, readOnly).isOk());

    smmu->translate(TEST_STREAM, TEST_PASID, BASE_IOVA, AccessType::Write);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Write);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded for Write";

    EXPECT_NE(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3: RnW (bit [3]) must be 1 for a write transaction; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 3: Write fault — RnW at bit [3] NOT bit [6] ─────────────────────

/// §7.3: Write fault must set bit [3] (RnW) and NOT bit [6] (old wrong WnR).
/// This is the primary regression test for FINDING-L-04: current buggy code
/// sets bit [6] instead of bit [3].
TEST_F(FaultSyndromeSpecTest, WriteFault_RnWAtBit3_NotBit6) {
    PagePermissions readOnly(true, false, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM, TEST_PASID, BASE_IOVA, BASE_PA, readOnly).isOk());

    smmu->translate(TEST_STREAM, TEST_PASID, BASE_IOVA, AccessType::Write);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Write);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded for Write";

    // Spec-correct position: bit [3] must be 1.
    EXPECT_NE(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3: RnW must be set at bit [3] for a write fault; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;

    // Wrong position: bit [6] must be 0 (old WnR placement).
    EXPECT_EQ(syn.syndromeRegister & OLD_WNR_BIT, 0u)
        << "§7.3: bit [6] must be 0 (old wrong WnR position); "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 4: Data read — InD bit [2] must be CLEAR ────────────────────────

/// §7.3: InD (bit [2]) = 0 for a data transaction (Read or Write).
TEST_F(FaultSyndromeSpecTest, DataRead_InDBitClear) {
    IOVA unmapped = BASE_IOVA + 0x2000;
    smmu->translate(TEST_STREAM, TEST_PASID, unmapped, AccessType::Read);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Read);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded for Read";

    EXPECT_EQ(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3: InD (bit [2]) must be 0 for a data transaction; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 5: Instruction fetch — InD bit [2] must be SET ──────────────────

/// §7.3: InD (bit [2]) = 1 for an instruction fetch.
/// A no-execute page causes an AccessFault on Execute.
TEST_F(FaultSyndromeSpecTest, InstructionFetch_InDBitSet) {
    // Map read+write, no execute — Execute access hits AccessFault.
    PagePermissions rwNoExec(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM, TEST_PASID, BASE_IOVA, BASE_PA, rwNoExec).isOk());

    smmu->translate(TEST_STREAM, TEST_PASID, BASE_IOVA, AccessType::Execute);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Execute);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded for Execute";

    EXPECT_NE(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3: InD (bit [2]) must be 1 for an instruction fetch; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 6: InD at bit [2] NOT bit [8] ───────────────────────────────────

/// §7.3: Instruction fetch must set bit [2] (InD) and NOT bit [8].
/// Bit [8] is the low bit of CLASS, not the InD indicator.
/// This is the second primary regression test for FINDING-L-04: current
/// buggy code sets bit [8] instead of bit [2].
TEST_F(FaultSyndromeSpecTest, InstructionFetch_InDAtBit2_NotBit8) {
    PagePermissions rwNoExec(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM, TEST_PASID, BASE_IOVA, BASE_PA, rwNoExec).isOk());

    smmu->translate(TEST_STREAM, TEST_PASID, BASE_IOVA, AccessType::Execute);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Execute);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded for Execute";

    // Spec-correct position: bit [2] must be 1.
    EXPECT_NE(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3: InD must be set at bit [2] for an instruction fetch; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;

    // Wrong position: bit [8] (= OLD_INST_BIT) must be 0.
    // (Bit [8] is CLASS[0] and must be 0 for an IN-class fault.)
    EXPECT_EQ(syn.syndromeRegister & OLD_INST_BIT, 0u)
        << "§7.3: bit [8] must be 0 (old wrong INST position / CLASS[0]); "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 7: Stage-1-only fault — S2 bit [7] must be CLEAR ────────────────

/// §7.3: S2 (bit [7]) = 0 when the fault is a Stage-1 fault.
/// An unmapped address produces a FaultStage::Stage1Only record.
TEST_F(FaultSyndromeSpecTest, Stage1Fault_S2BitClear) {
    IOVA unmapped = BASE_IOVA + 0x3000;
    smmu->translate(TEST_STREAM, TEST_PASID, unmapped, AccessType::Read);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Read);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded";

    // Stage-1 fault: S2 bit must be clear.
    EXPECT_EQ(syn.syndromeRegister & SYND_S2_BIT, 0u)
        << "§7.3: S2 (bit [7]) must be 0 for a Stage-1-only fault; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 8: Stage-2 fault — S2 bit [7] must be SET ───────────────────────

/// §7.3: S2 (bit [7]) = 1 when the fault occurs in Stage-2 translation.
/// PASID=1 is used for Stage-1 so that the Stage-1 address space is a
/// distinct object from the Stage-2 address space (which is aliased to
/// PASID=0's AS per BUG-07 integration fix).  Stage-1 translates
/// BASE_IOVA → BASE_PA (IPA).  Stage-2 looks up IPA=BASE_PA in PASID-0's
/// AS, where it is not mapped, triggering a Stage2Only fault.
TEST_F(FaultSyndromeSpecTest, Stage2Fault_S2BitSet) {
    // Create a non-PASID-0 context so Stage-1 uses a separate address space
    // from the Stage-2 AS (which BUG-07 aliases to PASID-0's AS).
    static constexpr PASID STAGE1_PASID = 1;
    ASSERT_TRUE(smmu->createStreamPASID(TEST_STREAM, STAGE1_PASID).isOk());

    // Map stage-1 page in PASID=1 space so Stage-1 succeeds → IPA = BASE_PA.
    // Stage-2 AS (PASID=0's AS) has no IPA→PA mapping for BASE_PA, so
    // performBothStagesTranslation records FaultStage::Stage2Only.
    PagePermissions rw(true, true, false);
    ASSERT_TRUE(smmu->mapPage(TEST_STREAM, STAGE1_PASID, BASE_IOVA, BASE_PA, rw).isOk());

    smmu->translate(TEST_STREAM, STAGE1_PASID, BASE_IOVA, AccessType::Read);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Read);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded";

    // Stage-2 fault: S2 bit must be set.
    EXPECT_NE(syn.syndromeRegister & SYND_S2_BIT, 0u)
        << "§7.3: S2 (bit [7]) must be 1 for a Stage-2-only fault; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── Test 9: Translation fault CLASS — must be IN (00) ────────────────────

/// §7.3: For an ordinary input-transaction fault (unmapped page),
/// CLASS bits [9:8] must be 00 (IN = input transaction class).
TEST_F(FaultSyndromeSpecTest, TranslationFault_ClassIsIN) {
    IOVA unmapped = BASE_IOVA + 0x4000;
    smmu->translate(TEST_STREAM, TEST_PASID, unmapped, AccessType::Read);

    FaultSyndrome syn = getSyndromeForAccessType(AccessType::Read);
    ASSERT_TRUE(syn.validSyndrome) << "No valid fault syndrome recorded";

    uint32_t classField = syn.syndromeRegister & SYND_CLASS_MASK;
    EXPECT_EQ(classField, SYND_CLASS_IN)
        << "§7.3: CLASS bits [9:8] must be 00 (IN) for a plain translation fault; "
        << "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

}  // namespace test
}  // namespace smmu
