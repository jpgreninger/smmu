// ARM SMMU v3 Bug-fix Tests: newBugs18Mar2026_8am.md
//
// Bugs covered:
//   Bug A:    WXN/UWXN isExec gate misses ReadExecute in stream_context.cpp and smmu.cpp
//   Bug B:    PRIVCFG=3/STRW promotes ReadExecute->ExecutePrivileged, dropping the read
//             requirement; needs ReadExecutePrivileged access type
//   Bug F:    INSTCFG=2 (Force Data) does not demote ReadExecute->Read
//   NEW-1:    generateFaultSyndrome() sets InD=0 for ReadExecute faults (should be 1)
//   NEW-2:    Stage-2-only path missing AccessFlagFaultError case (falls through to
//             AccessFault instead of AccessFlagFault)
//   NEW-3:    applyConfigurationChanges() does not sync s1Stalld_ mirror
//   RnW-GAP:  EventEntry.rnw bit is inverted vs ARM §7.3 spec
//             (spec: RnW=1=Read, RnW=0=Write; code had: rnw=true for Write)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/stream_context.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>

using namespace smmu;

namespace {

// ---------------------------------------------------------------------------
// Common helpers
// ---------------------------------------------------------------------------

void enableSMMU(SMMU& smmu) {
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Build a stage-1-only stream with the given extra config fields.
// Returns stream config template; caller may alter fields before calling
// configureStream.
StreamConfig stage1Cfg(FaultMode fm = FaultMode::Terminate) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = fm;
    cfg.t0sz               = 0; // no VA range restriction
    return cfg;
}

// Build a stage-2-only stream config.
StreamConfig stage2OnlyCfg(FaultMode fm = FaultMode::Terminate) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = fm;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5;
    return cfg;
}

static const PagePermissions RW   { true,  true,  false }; // read+write, no execute
static const PagePermissions RWX  { true,  true,  true  }; // read+write+execute
static const PagePermissions RX   { true,  false, true  }; // read+execute, no write
static const PagePermissions XOnly{ false, false, true  }; // execute-only

} // namespace

// ============================================================================
// Bug A — WXN misses ReadExecute
// A ReadExecute access to a writable (RW) page with WXN=1 must produce
// F_PERMISSION.  Before the fix, isExec only checked Execute/ExecutePrivileged
// so ReadExecute silently bypassed the WXN check.
// ============================================================================

TEST(BugA, ReadExecuteToWritablePageWithWXN_GivesFPermission) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xA0u;
    const PASID    pid = 0u;

    StreamConfig cfg = stage1Cfg();
    cfg.wxn = true;                         // WXN=1: writable pages are execute-never
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pid).isOk());

    // Map a RW (writable) page.  With WXN=1 this page must be execute-never.
    PagePermissions rw{true, true, false};
    ASSERT_TRUE(smmu.mapPage(sid, pid, 0x1000u, 0x1000u, rw).isOk());

    // ReadExecute to this page must fail with F_PERMISSION.
    auto result = smmu.translate(sid, pid, 0x1000u, AccessType::ReadExecute);
    EXPECT_TRUE(result.isError())
        << "ReadExecute on writable page with WXN=1 must produce a fault";

    auto events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.streamID == sid && ev.type == EventType::F_PERMISSION) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found) << "Expected F_PERMISSION event for Bug A (WXN + ReadExecute)";
}

// ============================================================================
// Bug B — ReadExecute in all-privileged (STRW=EL2) stream to execute-only page
// When STRW=EL2 maps ReadExecute→ExecutePrivileged (Bug B: drops the read bit),
// a page with execute-only permissions should be accepted for ExecutePrivileged
// but must be rejected for ReadExecutePrivileged (needs read too).
// The bug: STRW promotion converts ReadExecute→ExecutePrivileged, so a page that
// has execute but no read passes.  With ReadExecutePrivileged it correctly requires
// both bits.
// ============================================================================

TEST(BugB, ReadExecuteInAllPrivStreamToExecuteOnlyPage_GivesFPermission) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xB0u;
    const PASID    pid = 0u;

    StreamConfig cfg = stage1Cfg();
    cfg.strw = StreamWorld::EL2;            // all-privileged stream
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pid).isOk());

    // Map execute-only page: no read bit.
    PagePermissions execOnly{false, false, true};
    ASSERT_TRUE(smmu.mapPage(sid, pid, 0x2000u, 0x2000u, execOnly).isOk());

    // ReadExecute needs both read and execute.  The execute-only page lacks read.
    // After Bug B fix, ReadExecute in EL2 stream → ReadExecutePrivileged →
    // permission check requires read AND execute → fails (no read bit).
    auto result = smmu.translate(sid, pid, 0x2000u, AccessType::ReadExecute);
    EXPECT_TRUE(result.isError())
        << "ReadExecute in EL2 stream to execute-only page must fault (no read bit)";

    auto events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.streamID == sid && ev.type == EventType::F_PERMISSION) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "Expected F_PERMISSION event for Bug B (STRW=EL2 + ReadExecute + exec-only page)";
}

// PRIVCFG=3 variant of Bug B.
TEST(BugB, ReadExecuteWithPrivCfg3ToExecuteOnlyPage_GivesFPermission) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xB1u;
    const PASID    pid = 0u;

    StreamConfig cfg = stage1Cfg();
    cfg.privCfg = 3u;                       // PRIVCFG=3: force all accesses privileged
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pid).isOk());

    // Map execute-only page.
    PagePermissions execOnly{false, false, true};
    ASSERT_TRUE(smmu.mapPage(sid, pid, 0x3000u, 0x3000u, execOnly).isOk());

    // ReadExecute → with PRIVCFG=3 and Bug B fix → ReadExecutePrivileged.
    // ReadExecutePrivileged requires both read AND execute.  Fails: no read.
    auto result = smmu.translate(sid, pid, 0x3000u, AccessType::ReadExecute);
    EXPECT_TRUE(result.isError())
        << "ReadExecute with PRIVCFG=3 to execute-only page must fault";

    auto events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.streamID == sid && ev.type == EventType::F_PERMISSION) {
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found)
        << "Expected F_PERMISSION for Bug B (PRIVCFG=3 + ReadExecute + exec-only page)";
}

// Verify that ReadExecute in an EL2 stream to a page with BOTH read+execute
// succeeds (i.e. the fix does not break the happy path).
TEST(BugB, ReadExecuteInAllPrivStreamToReadExecutePage_Succeeds) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xB2u;
    const PASID    pid = 0u;

    StreamConfig cfg = stage1Cfg();
    cfg.strw = StreamWorld::EL2;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pid).isOk());

    // Map read+execute page.
    PagePermissions rx{true, false, true};
    ASSERT_TRUE(smmu.mapPage(sid, pid, 0x4000u, 0x4000u, rx).isOk());

    auto result = smmu.translate(sid, pid, 0x4000u, AccessType::ReadExecute);
    EXPECT_TRUE(result.isOk())
        << "ReadExecute in EL2 stream to read+execute page must succeed";
}

// ============================================================================
// Bug F — INSTCFG=2 must demote ReadExecute → Read
// ============================================================================

TEST(BugF, ReadExecuteWithInstCfg2_TreatedAsDataRead) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xF0u;
    const PASID    pid = 0u;

    StreamConfig cfg = stage1Cfg();
    cfg.instCfg = 2u;                       // Force Data: execute → read
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pid).isOk());

    // Map a read-only page (no execute permission).
    PagePermissions ro{true, false, false};
    ASSERT_TRUE(smmu.mapPage(sid, pid, 0x5000u, 0x5000u, ro).isOk());

    // Without Bug F fix: ReadExecute is NOT demoted by INSTCFG=2, so
    // the execute component fails (page has no execute).
    // With Bug F fix: ReadExecute → Read (data access) → succeeds (page has read).
    auto result = smmu.translate(sid, pid, 0x5000u, AccessType::ReadExecute);
    EXPECT_TRUE(result.isOk())
        << "ReadExecute with INSTCFG=2 should be treated as a data Read and succeed "
           "on a read-only page (Bug F)";
}

// ============================================================================
// Bug NEW-1 — generateFaultSyndrome() InD must be 1 for ReadExecute faults
// The FaultSyndrome.instructionFetch field (InD) must be true for ReadExecute.
// We verify indirectly via classifyAccess: if InD is correct, the AccessClass
// in the syndrome will be InstructionFetch.
// A simpler proxy: translate a ReadExecute through an unmapped region, then
// check the event's ind field.
// ============================================================================

TEST(BugNEW1, ReadExecuteFaultProducesIndTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 0xE1u;
    const PASID    pid = 0u;

    StreamConfig cfg = stage1Cfg();
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pid).isOk());

    // No page mapped — will produce F_TRANSLATION.
    smmu.translate(sid, pid, 0x6000u, AccessType::ReadExecute);

    auto events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.streamID == sid) {
            EXPECT_TRUE(ev.ind)
                << "ReadExecute fault must set ind=true (InD=1) in EventEntry (Bug NEW-1)";
            // RnW-GAP fix: ReadExecute is a read (no write component); RnW=1=Read per ARM §7.3.
            EXPECT_TRUE(ev.rnw)
                << "ReadExecute fault: rnw must be true (ARM §7.3 RnW=1=Read, no write component)";
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found) << "Expected a fault event for Bug NEW-1";
}

// ============================================================================
// Bug NEW-2 — Stage-2-only path missing AccessFlagFaultError case
// A stage-2-only stream with an AF=0 page (and ha=false, affd=false) must
// produce F_ACCESS.  Before the fix, the missing case fell through to the
// default (AccessFault / generic AccessFault), so eventType would not be F_ACCESS.
// ============================================================================

TEST(BugNEW2, Stage2OnlyAFZeroPage_GivesFAccess) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid  = 0xE2u;
    const PASID    pid  = 0u;
    const IOVA     iova = 0x7000u;
    const PA       pa   = 0x7000u;

    StreamConfig cfg = stage2OnlyCfg();
    cfg.ha    = false;  // hardware AF management disabled
    cfg.affd  = false;  // AF-fault-disabled = false (so AF=0 triggers fault)
    cfg.s2ha  = false;
    cfg.s2affd = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());

    // Map with AF=false (unaccessed) in the stage-2 address space.
    auto s2as = std::make_shared<AddressSpace>();
    PagePermissions rwx{true, true, true};
    // mapPage with accessFlag=false creates AF=0 entry for AF fault testing.
    s2as->mapPage(iova, pa, rwx, SecurityState::NonSecure, /*accessFlag=*/false);
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());

    // Stage-2-only: PASID 0 is implicit; no stage-1 PASID setup needed.

    auto result = smmu.translate(sid, pid, iova, AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "AF=0 page in stage-2-only stream must produce a fault (Bug NEW-2)";

    auto events = smmu.getEventQueue();
    bool found = false;
    for (const auto& ev : events) {
        if (ev.streamID == sid) {
            EXPECT_EQ(ev.type, EventType::F_ACCESS)
                << "Stage-2-only AF=0 fault must produce F_ACCESS event (Bug NEW-2)";
            found = true;
            break;
        }
    }
    EXPECT_TRUE(found) << "Expected a fault event for Bug NEW-2";
}

// ============================================================================
// Bug NEW-3 — applyConfigurationChanges() does not sync s1Stalld_ mirror
// After calling applyConfigurationChanges() with s1Stalld=true, the accessor
// isS1StallDisabled() must reflect the new value.  Before the fix, only
// updateConfiguration() synced s1Stalld_.
// ============================================================================

TEST(BugNEW3, ApplyConfigurationChangesSyncsS1Stalld) {
    // Directly exercise StreamContext::applyConfigurationChanges() which is
    // the incremental update path.  The bug: applyConfigurationChanges() syncs
    // stage1Enabled, stage2Enabled, faultMode but NOT s1Stalld_.
    StreamContext sc;

    // Initial config: s1Stalld=false.
    StreamConfig cfg1;
    cfg1.translationEnabled = true;
    cfg1.stage1Enabled      = true;
    cfg1.stage2Enabled      = false;
    cfg1.faultMode          = FaultMode::Stall;
    cfg1.s1Stalld           = false;
    ASSERT_TRUE(sc.updateConfiguration(cfg1).isOk());
    EXPECT_FALSE(sc.isS1StallDisabled())
        << "s1Stalld should start as false after initial updateConfiguration";

    // Incremental update: change s1Stalld to true via applyConfigurationChanges.
    StreamConfig cfg2 = cfg1;
    cfg2.s1Stalld = true;
    ASSERT_TRUE(sc.applyConfigurationChanges(cfg2).isOk());

    // After applyConfigurationChanges, isS1StallDisabled() must return true.
    EXPECT_TRUE(sc.isS1StallDisabled())
        << "applyConfigurationChanges() must sync s1Stalld_ mirror (Bug NEW-3)";
}

// ============================================================================
// RnW GAP — EventEntry.rnw bit inverted vs ARM §7.3 spec
// ARM spec: RnW=0 means Write, RnW=1 means Read.
// Bug: code had rnw=true for Write, rnw=false for Read.
// After fix: Write → rnw=false, Read → rnw=true.
// ============================================================================

// Helper: trigger a translation fault for a given access type and capture
// the first event generated.
static EventEntry getEventRnW(AccessType at) {
    SMMU smmu;
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    const StreamID sid = 0xC0u;
    const PASID    pid = 0u;
    StreamConfig cfg = stage1Cfg();
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, pid);
    // No page mapped → F_TRANSLATION event captures rnw.
    smmu.translate(sid, pid, 0x9000u, at);
    auto q = smmu.getEventQueue();
    return q.empty() ? EventEntry{} : q.front();
}

TEST(RnWGap, WriteFault_RnwIsFalse) {
    // ARM §7.3: RnW=0 for Write transactions.
    EventEntry ev = getEventRnW(AccessType::Write);
    EXPECT_FALSE(ev.rnw) << "Write fault: rnw must be false (ARM §7.3 RnW=0=Write) [RnW-GAP]";
}

TEST(RnWGap, ReadFault_RnwIsTrue) {
    // ARM §7.3: RnW=1 for Read transactions.
    EventEntry ev = getEventRnW(AccessType::Read);
    EXPECT_TRUE(ev.rnw) << "Read fault: rnw must be true (ARM §7.3 RnW=1=Read) [RnW-GAP]";
}

TEST(RnWGap, ExecuteFault_RnwIsTrue) {
    // ARM §7.3: Execute is a read (instruction fetch); RnW=1.
    EventEntry ev = getEventRnW(AccessType::Execute);
    EXPECT_TRUE(ev.rnw) << "Execute fault: rnw must be true (ARM §7.3) [RnW-GAP]";
}

TEST(RnWGap, ReadWriteFault_RnwIsFalse) {
    // ARM §3.24 / §7.3: atomic RMW is write-class; RnW=0.
    EventEntry ev = getEventRnW(AccessType::ReadWrite);
    EXPECT_FALSE(ev.rnw) << "ReadWrite fault: rnw must be false (write-class per ARM §3.24) [RnW-GAP]";
}

TEST(RnWGap, ReadExecuteFault_RnwIsTrue) {
    // ReadExecute has a read component and no write; RnW=1.
    EventEntry ev = getEventRnW(AccessType::ReadExecute);
    EXPECT_TRUE(ev.rnw) << "ReadExecute fault: rnw must be true (read component, ARM §7.3) [RnW-GAP]";
}

TEST(RnWGap, ReadPrivilegedFault_RnwIsTrue) {
    EventEntry ev = getEventRnW(AccessType::ReadPrivileged);
    EXPECT_TRUE(ev.rnw) << "ReadPrivileged fault: rnw must be true [RnW-GAP]";
}

TEST(RnWGap, WritePrivilegedFault_RnwIsFalse) {
    EventEntry ev = getEventRnW(AccessType::WritePrivileged);
    EXPECT_FALSE(ev.rnw) << "WritePrivileged fault: rnw must be false [RnW-GAP]";
}

TEST(RnWGap, ExecutePrivilegedFault_RnwIsTrue) {
    EventEntry ev = getEventRnW(AccessType::ExecutePrivileged);
    EXPECT_TRUE(ev.rnw) << "ExecutePrivileged fault: rnw must be true [RnW-GAP]";
}

TEST(RnWGap, ReadWritePrivilegedFault_RnwIsFalse) {
    EventEntry ev = getEventRnW(AccessType::ReadWritePrivileged);
    EXPECT_FALSE(ev.rnw) << "ReadWritePrivileged fault: rnw must be false (write-class) [RnW-GAP]";
}
