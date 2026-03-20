// ARM SMMU v3 Bug-fix tests
//
// BUG-1 (MODERATE): UWXN check fires for EL2/EL3 streams.
//   ARM §5.4: "In configurations for which all accesses are considered
//   privileged (StreamWorld == EL2 or StreamWorld == EL3), UWXN has no
//   effect and is IGNORED."  EL2_E2H is explicitly excluded — it still
//   enforces UWXN.
//
//   Tests:
//     Bug1_UwxnIgnoredForEL2:
//       strw=EL2, uwxn=true, RW page, privileged execute → must SUCCEED
//       (UWXN must be ignored for EL2 streams).
//     Bug1_UwxnIgnoredForEL3:
//       strw=EL3, uwxn=true, RW page, privileged execute → must SUCCEED.
//     Bug1_UwxnEnforcedForEL2_E2H:
//       strw=EL2_E2H, uwxn=true, RW page, privileged execute → must
//       FAIL with F_PERMISSION (EL2_E2H is NOT ignored per §3.3.4).
//     Bug1_UwxnEnforcedForEL1_EL0 (regression):
//       strw=EL1_EL0, uwxn=true, RW page, privileged execute → must FAIL.
//
// BUG-3 (LOW): isCmdqEmptyByIndex() and getCmdqOccupiedEntries() return
//   wrong results after CERROR_ILL is written into CMDQ_CONS.ERR.
//   ARM §6.3.28: CMDQ_CONS.ERR at bits[30:24], RD at bits[19:0].
//   Queue empty iff PROD.WR == CONS.RD (RD bits only; ERR bits ignored).
//
//   Tests:
//     Bug3_IsCmdqEmptyTrueWhenRdEqualsWrWithCerror:
//       After CERROR_ILL is written into CONS.ERR and both PROD and CONS
//       RD pointers are equal, isCmdqEmptyByIndex() must return true.
//     Bug3_GetCmdqOccupiedZeroWithCerror:
//       Same state → getCmdqOccupiedEntries() must return 0.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>

using namespace smmu;

namespace {

void enableSMMU(SMMU& smmu) {
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Build a stage-1-only stream with the given strw, uwxn, and security state.
// Returns a configured + enabled SMMU with a RW-mapped page.
// The SMMU must already have been constructed by the caller.
void setupUwxnStream(SMMU& smmu, StreamID sid, StreamWorld strw,
                     SecurityState ss, bool uwxn) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.strw               = strw;
    cfg.securityState      = ss;
    cfg.uwxn               = uwxn;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0).isOk());

    // Map a read-write (non-privileged-only) page so UWXN has a writable
    // target to evaluate.
    PagePermissions perms;
    perms.read           = true;
    perms.write          = true;
    perms.execute        = true;
    perms.privilegedOnly = false;
    ASSERT_TRUE(smmu.mapPage(sid, 0, 0x1000ULL, 0x2000ULL, perms, ss).isOk());
}

} // anonymous namespace

// ============================================================================
// BUG-1: UWXN must be IGNORED for EL2 streams (§5.4)
// ============================================================================

// BEFORE FIX: the UWXN check fires regardless of strw → F_PERMISSION returned.
// AFTER FIX:  isAllPrivilegedStream(EL2)=true → UWXN check skipped → success.
TEST(Bug1UwxnElStream, UwxnIgnoredForEL2) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID = 0xE0u;
    setupUwxnStream(smmu, SID, StreamWorld::EL2, SecurityState::NonSecure, /*uwxn=*/true);

    // Privileged execute on an unprivileged-writable page.
    // With strw=EL2 and UWXN=true, the spec says UWXN is IGNORED → success.
    auto result = smmu.translate(SID, 0, 0x1000ULL, AccessType::ExecutePrivileged,
                                 SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "BUG-1: UWXN must be ignored for EL2 streams (ARM §5.4). "
           "Before fix, UWXN check fires incorrectly → F_PERMISSION.";
}

// BEFORE FIX: the UWXN check fires for EL3 → F_PERMISSION.
// AFTER FIX:  isAllPrivilegedStream(EL3)=true → UWXN skipped → success.
TEST(Bug1UwxnElStream, UwxnIgnoredForEL3) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID = 0xE1u;
    // EL3 streams require Secure security state per §5.2.2/CONF-GAP-16.
    setupUwxnStream(smmu, SID, StreamWorld::EL3, SecurityState::Secure, /*uwxn=*/true);

    auto result = smmu.translate(SID, 0, 0x1000ULL, AccessType::ExecutePrivileged,
                                 SecurityState::Secure);
    EXPECT_TRUE(result.isOk())
        << "BUG-1: UWXN must be ignored for EL3 streams (ARM §5.4). "
           "Before fix, UWXN check fires incorrectly → F_PERMISSION.";
}

// BEFORE FIX (regression guard): EL2_E2H is NOT in the ignored set.
// Both before and after the fix, UWXN must still be enforced for EL2_E2H.
TEST(Bug1UwxnElStream, UwxnEnforcedForEL2_E2H) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID = 0xE2u;
    setupUwxnStream(smmu, SID, StreamWorld::EL2_E2H, SecurityState::NonSecure, /*uwxn=*/true);

    auto result = smmu.translate(SID, 0, 0x1000ULL, AccessType::ExecutePrivileged,
                                 SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "BUG-1 regression: UWXN must still be enforced for EL2_E2H (§3.3.4). "
           "EL2_E2H is explicitly excluded from the UWXN-ignored set.";
}

// Regression: EL1_EL0 must also continue to enforce UWXN.
TEST(Bug1UwxnElStream, UwxnEnforcedForEL1_EL0) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID = 0xE3u;
    setupUwxnStream(smmu, SID, StreamWorld::EL1_EL0, SecurityState::NonSecure, /*uwxn=*/true);

    auto result = smmu.translate(SID, 0, 0x1000ULL, AccessType::ExecutePrivileged,
                                 SecurityState::NonSecure);
    EXPECT_FALSE(result.isOk())
        << "Regression: UWXN must be enforced for EL1_EL0 streams.";
}

// ============================================================================
// BUG-3: isCmdqEmptyByIndex() must mask ERR bits before comparing PROD and CONS
// ============================================================================

// Set up a CERROR_ILL condition by submitting CMD_SYNC with CS=3 (reserved),
// which triggers CERROR_ILL in CMDQ_CONS.ERR.  After the queue drains
// (PROD.WR == CONS.RD), isCmdqEmptyByIndex() must return true.
//
// BEFORE FIX: cmdqProd == cmdqCons fails because ERR bits are set in cmdqCons
//             but not in cmdqProd → isCmdqEmptyByIndex() returns false.
// AFTER FIX:  mask applied → (prod & 0xFFFFF) == (cons & 0xFFFFF) → true.
TEST(Bug3CmdqEmpty, EmptyAfterCerrorIll) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // Submit an illegal CMD_SYNC (CS=3) to trigger CERROR_ILL.
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu.submitCommand(illSync).isOk());
    smmu.processCommandQueue();

    // Confirm the ERR field is now CERROR_ILL.
    ASSERT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "Precondition: CERROR_ILL must be set after illegal CMD_SYNC";

    // The queue is now empty (the one command was processed; RD == WR).
    // isCmdqEmptyByIndex() must return true regardless of the ERR bits.
    EXPECT_TRUE(smmu.isCmdqEmptyByIndex())
        << "BUG-3: isCmdqEmptyByIndex() must return true when RD==WR, "
           "even when CERROR_ILL is set in CMDQ_CONS.ERR bits[30:24]. "
           "Before fix, ERR bits corrupt the comparison and return false.";
}

// After CERROR_ILL, getCmdqOccupiedEntries() must return 0 when queue is empty.
//
// BEFORE FIX: raw cmdqCons (including ERR bits) passed to queueOccupied() →
//             queueOccupied() computes a non-zero "occupied" count.
// AFTER FIX:  masked RD portion used → queueOccupied() returns 0.
TEST(Bug3CmdqEmpty, OccupiedZeroAfterCerrorIll) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu.submitCommand(illSync).isOk());
    smmu.processCommandQueue();

    ASSERT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "Precondition: CERROR_ILL must be set";

    EXPECT_EQ(smmu.getCmdqOccupiedEntries(), 0u)
        << "BUG-3: getCmdqOccupiedEntries() must return 0 when queue is empty, "
           "even when CERROR_ILL is set in CMDQ_CONS.ERR bits[30:24]. "
           "Before fix, ERR bits are passed raw to queueOccupied() → non-zero result.";
}
