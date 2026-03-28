// TDD failing tests for BUG-NEW-A through BUG-NEW-E.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green).
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-A (§4.5.2): CMD_PRI_RESP Missing `resp` Field + No Resp==0b11 CERROR_ILL
//
//   ARM IHI0070G.b §4.5.2: CMD_PRI_RESP has a 2-bit Resp field.
//   Resp==0b11 is Reserved/ILLEGAL → must raise CERROR_ILL + GERROR_CMDQ_ERR.
//
//   Current code: CommandEntry has no `resp` field; PRI_RESP handler does not
//   validate the Resp field at all.  Resp==0b11 is silently accepted (no error).
//
//   BEFORE FIX: getCmdqConsErr() != CERROR_ILL, GERROR.CMDQ_ERR not set → FAILS.
//   AFTER FIX:  CERROR_ILL set, GERROR.CMDQ_ERR active → PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-B (§4.4): C++ RIL Reserved Check Uses `tg==1u` Instead of `tg!=0u`
//
//   ARM IHI0070G.b §4.4: The Reserved RIL combination (TG!=0 + NUM=0 + SCALE=0
//   + TTL=0) applies to ALL non-zero TG values, including TG=2 (16KB granule).
//
//   Current code (TLBI_NH_VA case in processCommand):
//     if (command.ril && command.tg == 1u && command.num == 0u
//             && command.scale == 0u && command.ttl == 0u)
//
//   The check only catches TG=1 (4KB).  TG=2 (16KB) is silently accepted.
//
//   BEFORE FIX: getCmdqConsErr() == CERROR_NONE for TG=2 → FAILS.
//   AFTER FIX:  getCmdqConsErr() == CERROR_ILL → PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-D (§4.4.2.10): TLBI_EL2_ASID Over-Invalidates EL1_EL0 Entries
//
//   ARM IHI0070G.b §4.4.2.10: Only NS-EL2-E2H entries with matching ASID
//   should be invalidated.
//
//   Current code: calls tlbCache->invalidateByASID(asid) which evicts ALL
//   entries with that ASID regardless of StreamWorld tag.
//
//   BEFORE FIX: EL1_EL0 entries with the same ASID are also invalidated
//     → TLB MISS for Stream B → FAILS.
//   AFTER FIX:  Only EL2/EL2_E2H entries are evicted → Stream B TLB HIT → PASSES.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-E (§4.4.2.8/9): TLBI_EL2_VA/VAA Over-Invalidate EL1_EL0 Entries
//
//   ARM IHI0070G.b §4.4.2.8: TLBI_EL2_VA should only invalidate NS-EL2/EL2_E2H
//   entries by VA+ASID, not EL1_EL0 entries.
//   ARM IHI0070G.b §4.4.2.9: TLBI_EL2_VAA should only invalidate NS-EL2/EL2_E2H
//   entries by VA (any ASID), not EL1_EL0 entries.
//
//   Current code: calls invalidateByVAAndASID / invalidateByVA which evict
//   matching entries regardless of StreamWorld.
//
//   BEFORE FIX: EL1_EL0 entries with the same VA are also invalidated → FAILS.
//   AFTER FIX:  Only EL2/EL2_E2H entries are evicted → EL1_EL0 TLB HIT → PASSES.
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Enable SMMU with Hyp support (IDR0.Hyp=1) for EL2 TLBI commands.
static void enableSMMUWithHyp(SMMU& s) {
    s.setHypSupported(true);
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Returns true when GERROR.CMDQ_ERR is active.
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return (s.getGerror() & GERROR_CMDQ_ERR) != 0u;
}

// Set up a stage-1 stream with a given ASID and STRW, map one page, warm the TLB.
// Returns true if the warm-up translate succeeds.
static bool setupStreamAndWarm(SMMU& smmu, uint32_t sid,
                               uint16_t asid, StreamWorld strw,
                               IOVA iova, PA pa) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0;
    cfg.asid               = asid;
    cfg.strw               = strw;
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0u);

    smmu.mapPage(sid, 0u, iova, pa, PagePermissions(true, true, false));

    // Warm TLB.
    auto r = smmu.translate(sid, 0u, iova, AccessType::Read, SecurityState::NonSecure);
    return r.isOk();
}

// Issue CMD_SYNC and process the command queue.
static void issueSync(SMMU& smmu) {
    CommandEntry sync;
    sync.type = CommandType::SYNC;
    smmu.submitCommand(sync);
    smmu.processCommandQueue();
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-A: CMD_PRI_RESP with Resp==0b11 must raise CERROR_ILL (ARM §4.5.2)
// ============================================================================
//
// ARM §4.5.2: The 2-bit Resp field in CMD_PRI_RESP encodes the response type.
// Resp==0b11 is Reserved/ILLEGAL and must raise CERROR_ILL.
//
// Current code: CommandEntry has no `resp` field; the PRI_RESP handler does not
// validate Resp at all.  Resp==0b11 is silently accepted.
//
// To expose the bug the test adds the `resp` field to CommandEntry via the
// flags field (bits [1:0] of flags carry the Resp value following the wire
// encoding in the test).  The implementation must read bits [1:0] of flags
// (or a dedicated resp field) and raise CERROR_ILL when resp==0b11.
//
// BEFORE FIX: getCmdqConsErr() == CERROR_NONE (0) → test FAILS.
// AFTER FIX:  getCmdqConsErr() == CERROR_ILL  (1), GERROR.CMDQ_ERR active → PASSES.
TEST(BugNewA_PriRespReservedResp, Resp0b11_RaisesCerrorIll) {
    // §4.5.2: CMD_PRI_RESP Resp==0b11 is Reserved → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    // Clear any pre-existing error state.
    smmu.clearGerror(smmu.getGerror());

    // Build a CMD_PRI_RESP command.  The CommandEntry struct currently lacks a
    // `resp` field.  Per the bug description the fix must:
    //   (a) add a `resp` field to CommandEntry, OR
    //   (b) encode Resp in the existing `flags` bits[1:0].
    // Either way, the test sets resp=0b11 using the `flags` field as a
    // temporary carrier until the real field is added.
    //
    // The test deliberately uses `resp` via a dedicated field (which will be
    // added as part of the fix).  Until then it won't compile — that is the
    // expected red state at the type level.  We use `flags` to encode resp
    // for now and expect the implementation to check it.
    CommandEntry cmd;
    cmd.type  = CommandType::PRI_RESP;
    // Encode Resp==0b11 in the low 2 bits of flags.
    // The fix must extract bits[1:0] of flags as the Resp field and reject 0b11.
    cmd.flags = 0x03u;  // Resp==0b11 (Reserved per §4.5.2)

    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // CERROR_ILL must be set in CMDQ_CONS.ERR.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-A: CMD_PRI_RESP with Resp==0b11 must raise CERROR_ILL "
           "(ARM §4.5.2: Reserved Resp encoding). "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();

    // GERROR.CMDQ_ERR must also be active.
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-A: CMD_PRI_RESP Resp==0b11 must set GERROR.CMDQ_ERR (ARM §4.5.2).";
}

// ============================================================================
// BUG-NEW-B: C++ TLBI_NH_VA Reserved RIL check misses TG=2 (16KB)
// ============================================================================
//
// ARM §4.4: The Reserved combination TG!=0 AND NUM==0 AND SCALE==0 AND TTL==0
// applies to ALL non-zero TG values (4KB, 16KB, 64KB).
//
// Current code: `if (command.ril && command.tg == 1u && ...)` — only checks
// TG=1 (4KB); TG=2 (16KB) passes through without raising CERROR_ILL.
//
// BEFORE FIX: getCmdqConsErr() == CERROR_NONE for TG=2 → test FAILS.
// AFTER FIX:  getCmdqConsErr() == CERROR_ILL for TG=2 → test PASSES.
TEST(BugNewB_ReservedRilCheckC, TlbiNhVa_TG2_NUM0_SCALE0_TTL0_MustRaiseCerrorIll) {
    // §4.4: TG=2 (16KB), NUM=0, SCALE=0, TTL=0 with ril=true is Reserved → CERROR_ILL.
    SMMU smmu;
    enableSMMU(smmu);

    // Clear any pre-existing error state.
    smmu.clearGerror(smmu.getGerror());

    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_NH_VA;
    cmd.streamID     = 0u;
    cmd.pasid        = 0u;
    cmd.asid         = 0u;
    cmd.vmid         = 0u;
    cmd.startAddress = 0x1000u;
    cmd.ril          = true;
    cmd.tg           = 2u;   // TG=2 (16KB granule) — RESERVED combination when NUM=0, SCALE=0, TTL=0
    cmd.num          = 0u;   // NUM == 0
    cmd.scale        = 0u;   // SCALE == 0
    cmd.ttl          = 0u;   // TTL == 0b00 → Reserved per §4.4

    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // CERROR_ILL must be set in CMDQ_CONS.ERR.
    EXPECT_EQ(smmu.getCmdqConsErr(), CERROR_ILL)
        << "BUG-NEW-B: TLBI_NH_VA with TG=2 (16KB) AND NUM==0 AND SCALE==0 AND TTL==0 "
           "must raise CERROR_ILL (ARM §4.4: Reserved RIL parameter combination). "
           "Current code only checks tg==1u (4KB), not tg!=0u. "
           "Got CMDQ_CONS.ERR=" << smmu.getCmdqConsErr();

    // GERROR.CMDQ_ERR must be active.
    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-B: TLBI_NH_VA TG=2 reserved RIL combination must set GERROR.CMDQ_ERR.";
}

// ============================================================================
// BUG-NEW-D (1 of 2): TLBI_EL2_ASID must NOT invalidate EL1_EL0 entries
// ============================================================================
//
// ARM §4.4.2.10: TLBI_EL2_ASID invalidates NS-EL2-E2H entries matching ASID.
// EL1_EL0 entries with the same ASID value must be preserved.
//
// Current code: calls tlbCache->invalidateByASID(asid) which evicts ALL
// entries with that ASID regardless of StreamWorld — EL1_EL0 entries are
// wrongly evicted.
//
// Setup:
//   Stream A: STRW=EL2_E2H, ASID=5 — page at 0x5000 → PA 0x15000. TLB warmed.
//   Stream B: STRW=EL1_EL0, ASID=5 — page at 0x6000 → PA 0x16000. TLB warmed.
//
// Issue TLBI_EL2_ASID(ASID=5).
//   Stream A's entry should be EVICTED (TLB MISS).
//   Stream B's entry should be PRESERVED (TLB HIT).
//
// BEFORE FIX: Stream B entry also evicted → TLB MISS → hits increase don't → FAILS.
// AFTER FIX:  Stream B entry preserved → TLB HIT → hits increase → PASSES.
TEST(BugNewD_TlbiEl2AsidScope, El1El0_EntryPreserved_AfterTlbiEl2Asid) {
    // §4.4.2.10: TLBI_EL2_ASID must preserve EL1_EL0 entries (different StreamWorld).
    SMMU smmu;
    smmu.enableCaching(true);
    enableSMMUWithHyp(smmu);

    const uint32_t sidA = 50u;  // STRW=EL2_E2H
    const uint32_t sidB = 51u;  // STRW=EL1_EL0
    const uint16_t kAsid = 5u;

    const IOVA iovaA = 0x5000u;
    const IOVA iovaB = 0x6000u;
    const PA   paA   = 0x15000u;
    const PA   paB   = 0x16000u;

    // Set up both streams and warm the TLB.
    ASSERT_TRUE(setupStreamAndWarm(smmu, sidA, kAsid, StreamWorld::EL2_E2H, iovaA, paA))
        << "precondition: Stream A (EL2_E2H) page must translate";
    ASSERT_TRUE(setupStreamAndWarm(smmu, sidB, kAsid, StreamWorld::EL1_EL0, iovaB, paB))
        << "precondition: Stream B (EL1_EL0) page must translate";

    // Issue TLBI_EL2_ASID with ASID=5.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_EL2_ASID;
    cmd.asid = kAsid;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    issueSync(smmu);

    // Stream B's EL1_EL0 entry must still be in the TLB (TLB HIT).
    uint64_t hitsBefore = smmu.getCacheHitCount();
    auto rB = smmu.translate(sidB, 0u, iovaB, AccessType::Read, SecurityState::NonSecure);
    uint64_t hitsAfter = smmu.getCacheHitCount();

    ASSERT_TRUE(rB.isOk())
        << "BUG-NEW-D: Stream B (EL1_EL0) page must still translate after TLBI_EL2_ASID";

    EXPECT_GT(hitsAfter, hitsBefore)
        << "BUG-NEW-D: TLBI_EL2_ASID must NOT invalidate EL1_EL0 entries with same ASID "
           "(ARM §4.4.2.10: only NS-EL2-E2H entries should be evicted). "
           "Current code uses invalidateByASID() which over-invalidates across StreamWorld. "
           "TLB hit count: before=" << hitsBefore << " after=" << hitsAfter;
}

// Stream A (EL1_EL0 same-ASID) entry MUST also be invalidated by unscoped TLBI_EL2_ASID
// — this documents the current (incorrect) behavior where all ASID entries are evicted.
// NOTE: This test verifies that a SECOND EL1_EL0 stream (different from Stream B)
// with a different ASID is NOT evicted — i.e., ASID scoping within EL1_EL0 still works.
// This is a regression guard for unrelated streams.
TEST(BugNewD_TlbiEl2AsidScope, DifferentAsid_El1El0_NotEvicted_AfterTlbiEl2Asid) {
    // Regression: EL1_EL0 stream with a DIFFERENT ASID must NOT be evicted.
    SMMU smmu;
    smmu.enableCaching(true);
    enableSMMUWithHyp(smmu);

    const uint32_t sidC = 53u;  // STRW=EL1_EL0, ASID=99 (different ASID)
    const uint16_t kAsidOther = 99u;
    const IOVA iovaC = 0x8000u;
    const PA   paC   = 0x18000u;

    ASSERT_TRUE(setupStreamAndWarm(smmu, sidC, kAsidOther, StreamWorld::EL1_EL0, iovaC, paC))
        << "precondition: Stream C (EL1_EL0, ASID=99) page must translate";

    // Issue TLBI_EL2_ASID with a DIFFERENT ASID (6).
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_EL2_ASID;
    cmd.asid = 6u;  // different from kAsidOther=99
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    issueSync(smmu);

    // Stream C's EL1_EL0 entry with ASID=99 must NOT be evicted (different ASID).
    uint64_t hitsBefore = smmu.getCacheHitCount();
    auto rC = smmu.translate(sidC, 0u, iovaC, AccessType::Read, SecurityState::NonSecure);
    uint64_t hitsAfter = smmu.getCacheHitCount();

    ASSERT_TRUE(rC.isOk())
        << "Regression: Stream C (EL1_EL0, ASID=99) page must still translate";

    EXPECT_GT(hitsAfter, hitsBefore)
        << "Regression: EL1_EL0 entry with a DIFFERENT ASID (99) must NOT be evicted "
           "by TLBI_EL2_ASID(ASID=6). "
           "TLB hit count: before=" << hitsBefore << " after=" << hitsAfter;
}

// ============================================================================
// BUG-NEW-E (TLBI_EL2_VA): must NOT invalidate EL1_EL0 entries (§4.4.2.8)
// ============================================================================
//
// ARM §4.4.2.8: TLBI_EL2_VA invalidates NS-EL2/EL2_E2H entries by VA+ASID.
// EL1_EL0 entries with the same VA must be preserved.
//
// Current code: calls tlbCache->invalidateByVAAndASID(iova, asid) which evicts
// ALL entries at that VA/ASID regardless of StreamWorld.
//
// Setup:
//   Stream A: STRW=EL2_E2H, ASID=7 — page at 0x1000 → PA 0x20000. TLB warmed.
//   Stream B: STRW=EL1_EL0, ASID=7 — page at 0x1000 → PA 0x30000. TLB warmed.
//
// Issue TLBI_EL2_VA targeting IOVA=0x1000, ASID=7.
//   Stream A → EVICTED (correct). Stream B → PRESERVED (bug: also evicted).
//
// BEFORE FIX: Stream B also evicted → TLB MISS → FAILS.
// AFTER FIX:  Stream B preserved → TLB HIT → PASSES.
TEST(BugNewE_TlbiEl2VaScope, El1El0_EntryPreserved_AfterTlbiEl2Va) {
    // §4.4.2.8: TLBI_EL2_VA must preserve EL1_EL0 entries with same VA+ASID.
    SMMU smmu;
    smmu.enableCaching(true);
    enableSMMUWithHyp(smmu);

    const uint32_t sidA = 60u;  // STRW=EL2_E2H
    const uint32_t sidB = 61u;  // STRW=EL1_EL0
    const uint16_t kAsid = 7u;
    const IOVA kIova = 0x1000u;

    ASSERT_TRUE(setupStreamAndWarm(smmu, sidA, kAsid, StreamWorld::EL2_E2H, kIova, 0x20000u))
        << "precondition: Stream A (EL2_E2H) page must translate";
    ASSERT_TRUE(setupStreamAndWarm(smmu, sidB, kAsid, StreamWorld::EL1_EL0, kIova, 0x30000u))
        << "precondition: Stream B (EL1_EL0) page must translate";

    // Issue TLBI_EL2_VA targeting the shared IOVA and ASID.
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_EL2_VA;
    cmd.startAddress = kIova;
    cmd.asid         = kAsid;
    cmd.ril          = false;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    issueSync(smmu);

    // Stream B's EL1_EL0 entry must still be in the TLB (TLB HIT).
    uint64_t hitsBefore = smmu.getCacheHitCount();
    auto rB = smmu.translate(sidB, 0u, kIova, AccessType::Read, SecurityState::NonSecure);
    uint64_t hitsAfter = smmu.getCacheHitCount();

    ASSERT_TRUE(rB.isOk())
        << "BUG-NEW-E (TLBI_EL2_VA): Stream B (EL1_EL0) page must still translate";

    EXPECT_GT(hitsAfter, hitsBefore)
        << "BUG-NEW-E: TLBI_EL2_VA must NOT invalidate EL1_EL0 entries "
           "(ARM §4.4.2.8: only NS-EL2/EL2_E2H entries should be evicted). "
           "Current code uses invalidateByVAAndASID() which evicts across StreamWorld. "
           "TLB hit count: before=" << hitsBefore << " after=" << hitsAfter;
}

// ============================================================================
// BUG-NEW-E (TLBI_EL2_VAA): must NOT invalidate EL1_EL0 entries (§4.4.2.9)
// ============================================================================
//
// ARM §4.4.2.9: TLBI_EL2_VAA invalidates NS-EL2/EL2_E2H entries by VA (any ASID).
// EL1_EL0 entries at the same VA must be preserved.
//
// Current code: calls tlbCache->invalidateByVA(iova) which evicts ALL entries
// at that VA regardless of StreamWorld.
//
// BEFORE FIX: EL1_EL0 entry at same VA evicted → TLB MISS → FAILS.
// AFTER FIX:  EL1_EL0 entry preserved → TLB HIT → PASSES.
TEST(BugNewE_TlbiEl2VaaScope, El1El0_EntryPreserved_AfterTlbiEl2Vaa) {
    // §4.4.2.9: TLBI_EL2_VAA must preserve EL1_EL0 entries with same VA.
    SMMU smmu;
    smmu.enableCaching(true);
    enableSMMUWithHyp(smmu);

    const uint32_t sidA = 70u;  // STRW=EL2_E2H
    const uint32_t sidB = 71u;  // STRW=EL1_EL0
    const uint16_t kAsidA = 10u;
    const uint16_t kAsidB = 11u;
    const IOVA kIova = 0x2000u;

    ASSERT_TRUE(setupStreamAndWarm(smmu, sidA, kAsidA, StreamWorld::EL2_E2H, kIova, 0x40000u))
        << "precondition: Stream A (EL2_E2H) page must translate";
    ASSERT_TRUE(setupStreamAndWarm(smmu, sidB, kAsidB, StreamWorld::EL1_EL0, kIova, 0x50000u))
        << "precondition: Stream B (EL1_EL0) page must translate";

    // Issue TLBI_EL2_VAA targeting the shared IOVA (no ASID operand — VAA = all ASIDs).
    CommandEntry cmd;
    cmd.type         = CommandType::TLBI_EL2_VAA;
    cmd.startAddress = kIova;
    cmd.ril          = false;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    issueSync(smmu);

    // Stream B's EL1_EL0 entry must still be in the TLB (TLB HIT).
    uint64_t hitsBefore = smmu.getCacheHitCount();
    auto rB = smmu.translate(sidB, 0u, kIova, AccessType::Read, SecurityState::NonSecure);
    uint64_t hitsAfter = smmu.getCacheHitCount();

    ASSERT_TRUE(rB.isOk())
        << "BUG-NEW-E (TLBI_EL2_VAA): Stream B (EL1_EL0) page must still translate";

    EXPECT_GT(hitsAfter, hitsBefore)
        << "BUG-NEW-E: TLBI_EL2_VAA must NOT invalidate EL1_EL0 entries "
           "(ARM §4.4.2.9: only NS-EL2/EL2_E2H entries should be evicted by VAA). "
           "Current code uses invalidateByVA() which evicts across StreamWorld. "
           "TLB hit count: before=" << hitsBefore << " after=" << hitsAfter;
}
