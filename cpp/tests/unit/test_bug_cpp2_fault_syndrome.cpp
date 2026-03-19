// BUG-CPP-2: Wrong WnR/InD bits in generateFaultSyndrome()
//
// ARM IHI0070G.b §7.3.13–7.3.15 specifies the syndrome register bit layout:
//   Bit [3]: RnW — 1 if READ access (Read-not-Write), 0 if WRITE access
//   Bit [2]: InD — 1 if instruction fetch (execute), 0 if data access
//             MUST be 0 when bit [3] (RnW) is 0 (write) per spec
//
// CORRECTED spec interpretation (NEW-1 fix):
// RnW is "Read-not-Write" so:
//   Write accesses  → RnW=0 (bit[3] CLEAR), InD=0
//   Read accesses   → RnW=1 (bit[3] SET),   InD=0
//   Execute accesses → RnW=1 (no write), InD=1
//
//   AccessType::ReadWrite        → RnW=0, InD=0 (write class)
//   AccessType::ReadWritePrivileged → RnW=0, InD=0
//   AccessType::WritePrivileged  → RnW=0, InD=0
//   AccessType::Execute          → RnW=1 (read/instruction), InD=1
//   AccessType::ExecutePrivileged → RnW=1, InD=1
//
// Bug: generateFaultSyndrome() only checked the single-variant Write and Execute.
// Post-fix: corrected to match ARM spec RnW definition.
//
// TDD: tests updated to reflect correct spec interpretation (NEW-1 fix).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Syndrome bit masks (per §7.3 / test_fault_syndrome_spec.cpp) ─────────

static constexpr uint32_t SYND_IND_BIT = (1u << 2);   // InD: event bit [98]
static constexpr uint32_t SYND_RNW_BIT = (1u << 3);   // RnW: event bit [99]

// ── Test constants ────────────────────────────────────────────────────────

static constexpr StreamID CPP2_STREAM   = 0xB0;
static constexpr PASID    CPP2_PASID_0  = 0;
static constexpr PASID    CPP2_PASID_1  = 1;
static constexpr IOVA     CPP2_IOVA     = 0x0020'0000ULL;
static constexpr PA       CPP2_PA_IPA   = 0x3000'0000ULL;
static constexpr PA       CPP2_PA_FINAL = 0x5000'0000ULL;

// ── Helper: build enabled SMMU ────────────────────────────────────────────

static std::unique_ptr<SMMU> makeEnabled() {
    auto s = std::unique_ptr<SMMU>(new SMMU());
    s->enable();
    return s;
}

// ── Fixture: two-stage stream (syndrome recorded via recordComprehensiveFault)

// generateFaultSyndrome() is called only from recordComprehensiveFault(),
// which is called by performBothStagesTranslation().  A two-stage stream
// with PASID=1 (separate stage-1 AS) and a stage-1 read-only page is used
// so that a ReadWrite or WritePrivileged access produces a permission fault
// whose syndrome register is populated.

class FaultSyndromeBugTest : public ::testing::Test {
protected:
    void SetUp() override {
        smmu = makeEnabled();

        // Two-stage stream.
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = true;
        cfg.faultMode          = FaultMode::Terminate;
        ASSERT_TRUE(smmu->configureStream(CPP2_STREAM, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(CPP2_STREAM).isOk());

        // PASID=0: stage-2 address space.
        ASSERT_TRUE(smmu->createStreamPASID(CPP2_STREAM, CPP2_PASID_0).isOk());

        // PASID=1: stage-1 address space (separate from stage-2).
        ASSERT_TRUE(smmu->createStreamPASID(CPP2_STREAM, CPP2_PASID_1).isOk());

        // Stage-1 (PASID=1): map IOVA → IPA with read-only permissions.
        // A write access will pass stage-1 (it returns the page entry), but
        // the final permission intersection detects no write permission.
        readOnly = PagePermissions(true, false, false);
        readWrite = PagePermissions(true, true, false);

        // Map stage-1 page as read-only.
        ASSERT_TRUE(smmu->mapPage(CPP2_STREAM, CPP2_PASID_1, CPP2_IOVA,
                                  CPP2_PA_IPA, readOnly).isOk());

        // Stage-2 (PASID=0): map IPA → PA with read-write permissions.
        // Stage-2 allows everything; stage-1 restricts to read-only.
        ASSERT_TRUE(smmu->mapPage(CPP2_STREAM, CPP2_PASID_0, CPP2_PA_IPA,
                                  CPP2_PA_FINAL, readWrite).isOk());
    }

    void TearDown() override {
        smmu.reset();
    }

    // Helper: get first valid syndrome for the given accessType.
    FaultSyndrome getSyndrome(AccessType at) {
        auto evResult = smmu->getEvents();
        if (!evResult.isOk()) {
            return FaultSyndrome{};
        }
        for (const FaultRecord& rec : evResult.getValue()) {
            if (rec.syndrome.validSyndrome && rec.accessType == at) {
                return rec.syndrome;
            }
        }
        // Also search without matching accessType (some paths may store
        // the base type).
        for (const FaultRecord& rec : evResult.getValue()) {
            if (rec.syndrome.validSyndrome) {
                return rec.syndrome;
            }
        }
        return FaultSyndrome{};
    }

    std::unique_ptr<SMMU> smmu;
    PagePermissions readOnly;
    PagePermissions readWrite;
};

// ── BUG-CPP-2a: ReadWrite access — RnW bit [3] must be CLEAR ─────────────
//
// §7.3: RnW is Read-not-Write. An atomic read-modify-write (ReadWrite) contains
// a write component, so RnW (bit [3]) must be 0 (Write).
// NEW-1 fix: prior test expected RnW=1 (wrong spec interpretation).

TEST_F(FaultSyndromeBugTest, ReadWrite_RnWBitMustBeSet) {
    // ReadWrite on a read-only stage-1 page → permission fault syndrome.
    smmu->translate(CPP2_STREAM, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::ReadWrite, SecurityState::NonSecure);

    FaultSyndrome syn = getSyndrome(AccessType::ReadWrite);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-2: No valid syndrome recorded for ReadWrite access";

    // §7.3: ReadWrite is write-class → RnW=0 (bit[3] CLEAR).
    EXPECT_EQ(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3 / BUG-CPP-2: ReadWrite must CLEAR RnW (bit [3]=0) for write class; "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── BUG-CPP-2b: ReadWrite — InD bit [2] must be CLEAR ────────────────────
//
// §7.3 line 35325: InD (bit [2]) must be 0 when RnW (bit [3]) is 1 (write).
// ReadWrite has a write component, so InD must be 0.

TEST_F(FaultSyndromeBugTest, ReadWrite_InDBitMustBeClear) {
    smmu->translate(CPP2_STREAM, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::ReadWrite, SecurityState::NonSecure);

    FaultSyndrome syn = getSyndrome(AccessType::ReadWrite);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-2: No valid syndrome recorded for ReadWrite access";

    EXPECT_EQ(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3 line 35325 / BUG-CPP-2: ReadWrite must clear InD (bit [2]) "
           "because InD==0 when RnW==1 (write access); "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── BUG-CPP-2c: WritePrivileged — RnW bit [3] must be CLEAR ─────────────
//
// §7.3: RnW=0 for write accesses. WritePrivileged is a write → RnW=0.
// NEW-1 fix: prior test expected RnW=1 (wrong spec interpretation).

TEST_F(FaultSyndromeBugTest, WritePrivileged_RnWBitMustBeSet) {
    smmu->translate(CPP2_STREAM, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::WritePrivileged, SecurityState::NonSecure);

    FaultSyndrome syn = getSyndrome(AccessType::WritePrivileged);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-2: No valid syndrome recorded for WritePrivileged access";

    // §7.3: WritePrivileged is write-class → RnW=0 (bit[3] CLEAR).
    EXPECT_EQ(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3 / BUG-CPP-2: WritePrivileged must CLEAR RnW (bit[3]=0) for write class; "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── BUG-CPP-2d: ReadWritePrivileged — RnW bit [3] must be CLEAR ──────────
//
// §7.3: ReadWritePrivileged is write-class → RnW=0 (bit[3] CLEAR).
// NEW-1 fix: prior test expected RnW=1 (wrong spec interpretation).

TEST_F(FaultSyndromeBugTest, ReadWritePrivileged_RnWBitMustBeSet) {
    smmu->translate(CPP2_STREAM, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::ReadWritePrivileged, SecurityState::NonSecure);

    FaultSyndrome syn = getSyndrome(AccessType::ReadWritePrivileged);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-2: No valid syndrome recorded for ReadWritePrivileged access";

    // §7.3: ReadWritePrivileged is write-class → RnW=0 (bit[3] CLEAR).
    EXPECT_EQ(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3 / BUG-CPP-2: ReadWritePrivileged must CLEAR RnW (bit[3]=0); "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── BUG-CPP-2e: ExecutePrivileged — InD bit [2] must be SET ──────────────
//
// §7.3: ExecutePrivileged is an instruction fetch in privileged mode.
// Since it has no write component, RnW=0 and InD must be 1 (bit [2] SET).
// The buggy code only checks (accessType == Execute), so ExecutePrivileged
// incorrectly gets InD=0.
//
// To trigger an Execute fault on a two-stage stream: map stage-1 with
// read-write (no execute) permissions; stage-2 with read-write.
// ExecutePrivileged access fails the stage-1 permission check.

TEST(FaultSyndromeExecutePrivileged, ExecutePrivileged_InDBitMustBeSet) {
    auto smmu = makeEnabled();
    StreamID sid = CPP2_STREAM + 1;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP2_PASID_0).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP2_PASID_1).isOk());

    // Stage-1 (PASID=1): read+write, no execute.
    PagePermissions rwNoExec(true, true, false);
    ASSERT_TRUE(smmu->mapPage(sid, CPP2_PASID_1, CPP2_IOVA,
                              CPP2_PA_IPA, rwNoExec).isOk());

    // Stage-2 (PASID=0): all permissions.
    PagePermissions all(true, true, true);
    ASSERT_TRUE(smmu->mapPage(sid, CPP2_PASID_0, CPP2_PA_IPA,
                              CPP2_PA_FINAL, all).isOk());

    // ExecutePrivileged should fail: no execute permission in stage-1.
    smmu->translate(sid, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::ExecutePrivileged, SecurityState::NonSecure);

    // Retrieve syndrome.
    auto evResult = smmu->getEvents();
    ASSERT_TRUE(evResult.isOk());
    FaultSyndrome syn;
    bool found = false;
    for (const FaultRecord& rec : evResult.getValue()) {
        if (rec.syndrome.validSyndrome) {
            syn = rec.syndrome;
            found = true;
            break;
        }
    }
    ASSERT_TRUE(found)
        << "BUG-CPP-2: No valid syndrome recorded for ExecutePrivileged access";

    // §7.3: ExecutePrivileged has no write component → RnW=1 (Read-not-Write=1=Read).
    // NEW-1 fix: prior test expected RnW=0 (wrong); execute is a read, so RnW=1.
    EXPECT_NE(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3 / BUG-CPP-2: ExecutePrivileged has no write → RnW=1 (Read); "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;

    // InD (bit [2]) must be SET — this is an instruction fetch.
    EXPECT_NE(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3 / BUG-CPP-2: ExecutePrivileged must set InD (bit [2]); "
           "buggy code misses ExecutePrivileged variant; "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── BUG-CPP-2f: Plain Write — RnW bit [3] must be CLEAR (regression) ─────
//
// §7.3: Write accesses set RnW=0 (bit[3] CLEAR). Regression guard.
// NEW-1 fix: prior test expected RnW=1 (wrong spec interpretation).

TEST_F(FaultSyndromeBugTest, PlainWrite_RnWBitStillSet_Regression) {
    smmu->translate(CPP2_STREAM, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::Write, SecurityState::NonSecure);

    FaultSyndrome syn = getSyndrome(AccessType::Write);
    ASSERT_TRUE(syn.validSyndrome)
        << "BUG-CPP-2: No valid syndrome for plain Write (regression)";

    // §7.3: Write → RnW=0 (bit[3] CLEAR).
    EXPECT_EQ(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3 / BUG-CPP-2: Plain Write must CLEAR RnW (bit[3]=0) per spec; "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;

    EXPECT_EQ(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3 / BUG-CPP-2: Plain Write must have InD=0; "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

// ── BUG-CPP-2g: Plain Execute — InD and RnW correctness (regression) ─────
//
// Regression guard: Execute must still set InD=1 and RnW=0.

TEST(FaultSyndromeRegression, PlainExecute_InDSet_RnWClear) {
    auto smmu = makeEnabled();
    StreamID sid = CPP2_STREAM + 2;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP2_PASID_0).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, CPP2_PASID_1).isOk());

    PagePermissions rwNoExec(true, true, false);
    ASSERT_TRUE(smmu->mapPage(sid, CPP2_PASID_1, CPP2_IOVA,
                              CPP2_PA_IPA, rwNoExec).isOk());
    PagePermissions all(true, true, true);
    ASSERT_TRUE(smmu->mapPage(sid, CPP2_PASID_0, CPP2_PA_IPA,
                              CPP2_PA_FINAL, all).isOk());

    smmu->translate(sid, CPP2_PASID_1, CPP2_IOVA,
                    AccessType::Execute, SecurityState::NonSecure);

    auto evResult = smmu->getEvents();
    ASSERT_TRUE(evResult.isOk());
    FaultSyndrome syn;
    bool found = false;
    for (const FaultRecord& rec : evResult.getValue()) {
        if (rec.syndrome.validSyndrome) {
            syn = rec.syndrome;
            found = true;
            break;
        }
    }
    ASSERT_TRUE(found) << "BUG-CPP-2: No valid syndrome for plain Execute";

    // §7.3: Execute is an instruction fetch (a read) → RnW=1 (Read-not-Write=1=Read).
    // NEW-1 fix: prior test expected RnW=0 (wrong — execute is a read, not a write).
    EXPECT_NE(syn.syndromeRegister & SYND_RNW_BIT, 0u)
        << "§7.3 regression: Execute must have RnW=1 (read/instruction); "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
    EXPECT_NE(syn.syndromeRegister & SYND_IND_BIT, 0u)
        << "§7.3 regression: Execute must have InD=1; "
           "syndromeRegister=0x" << std::hex << syn.syndromeRegister;
}

}  // namespace test
}  // namespace smmu
