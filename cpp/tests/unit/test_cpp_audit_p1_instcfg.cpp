// TDD failing test for BUG-AUDIT-133-CPP: INSTCFG encoding inverted
//
// ARM §5.2 bits[115:114] INSTCFG encoding:
//   0b00 (0u) = Use incoming access type — no change
//   0b01 (1u) = Reserved/passthrough — no change (NOT Force Instruction)
//   0b10 (2u) = Force Data — Execute -> Read  (CORRECT in code)
//   0b11 (3u) = Force Instruction — Read -> Execute  (BUG: code uses 1u)
//
// The bug: all Force-Instruction branches use == 1u instead of == 3u.
// Fix: change every `instCfg == 1u` Force-Instruction test to `instCfg == 3u`.
//
// These tests MUST FAIL before the fix is applied.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

static constexpr StreamID SID_INSTCFG = 0xC0u;

// Helper: build a basic stage-1 only stream config
static StreamConfig makeStage1Config(uint8_t instCfgVal)
{
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.aa64               = true;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.instCfg            = instCfgVal;
    return cfg;
}

// ── BUG-AUDIT-133-CPP Test 1 ────────────────────────────────────────────────
// instCfg=3 (Force Instruction per spec): Read access should be promoted to
// Execute.  A page mapped Execute-only (execute=true, read=false) must be
// accessible via a Read access when instCfg=3 is in effect.
//
// Before the fix, instCfg==3 falls through the dead else-if branches and the
// access type remains Read, causing a PermissionFault because the page has
// execute=true, read=false.
//
// After the fix, instCfg==3 promotes Read→Execute and the Execute permission
// passes.
TEST(BugAuditP1InstcfgTest, InstCfg3_ForceInstruction_ReadPromotedToExecute)
{
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg = makeStage1Config(3u);  // INSTCFG=3 = Force Instruction
    VoidResult cfgResult = smmu.configureStream(SID_INSTCFG, cfg);
    ASSERT_TRUE(cfgResult.isOk()) << "configureStream failed: "
        << static_cast<int>(cfgResult.getError());
    ASSERT_TRUE(smmu.enableStream(SID_INSTCFG).isOk()) << "enableStream failed";
    ASSERT_TRUE(smmu.createStreamPASID(SID_INSTCFG, 0u).isOk()) << "createStreamPASID failed";

    // Map IOVA 0x1000 -> PA 0x1000 with Execute-only permission (no Read)
    PagePermissions execOnly;
    execOnly.read    = false;
    execOnly.write   = false;
    execOnly.execute = true;
    VoidResult mapResult = smmu.mapPage(SID_INSTCFG, 0u, 0x1000u, 0x1000u, execOnly);
    ASSERT_TRUE(mapResult.isOk()) << "mapPage failed";

    // With instCfg=3 (Force Instruction), Read should be promoted to Execute,
    // and Execute permission is granted -> translation succeeds.
    TranslationResult result = smmu.translate(SID_INSTCFG, 0u, 0x1000u,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-133-CPP: instCfg=3 (Force Instruction) should promote "
           "Read->Execute, but translation faulted.  Before fix: instCfg==1u "
           "condition is used instead of instCfg==3u, so instCfg=3 is a no-op.";
}

// ── BUG-AUDIT-133-CPP Test 2 ────────────────────────────────────────────────
// instCfg=1 (Reserved/passthrough per spec): Read access must NOT be promoted
// to Execute.  A page mapped Execute-only must fault on a Read access.
//
// Before the fix, instCfg==1 incorrectly triggers Force-Instruction behavior,
// making this test PASS (but for the wrong reason — the wrong encoding fires).
// After the fix, instCfg==1 is a no-op and the Read faults on Execute-only page.
//
// This test checks the COMPLEMENTARY case: instCfg=1 must be passthrough.
TEST(BugAuditP1InstcfgTest, InstCfg1_Reserved_ReadNotPromotedToExecute)
{
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg = makeStage1Config(1u);  // INSTCFG=1 = Reserved (passthrough)
    VoidResult cfgResult = smmu.configureStream(SID_INSTCFG + 1u, cfg);
    ASSERT_TRUE(cfgResult.isOk()) << "configureStream failed";
    ASSERT_TRUE(smmu.enableStream(SID_INSTCFG + 1u).isOk()) << "enableStream failed";
    ASSERT_TRUE(smmu.createStreamPASID(SID_INSTCFG + 1u, 0u).isOk()) << "createStreamPASID failed";

    // Map with Execute-only permission
    PagePermissions execOnly;
    execOnly.read    = false;
    execOnly.write   = false;
    execOnly.execute = true;
    VoidResult mapResult = smmu.mapPage(SID_INSTCFG + 1u, 0u, 0x2000u, 0x2000u, execOnly);
    ASSERT_TRUE(mapResult.isOk()) << "mapPage failed";

    // With instCfg=1 (Reserved/passthrough), Read stays Read.
    // Execute-only page -> PermissionFault expected.
    TranslationResult result = smmu.translate(SID_INSTCFG + 1u, 0u, 0x2000u,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "BUG-AUDIT-133-CPP: instCfg=1 (Reserved) must NOT promote Read->Execute. "
           "Before fix: instCfg==1u triggers Force-Instruction, promoting Read->Execute "
           "and incorrectly succeeding on Execute-only page.";
}

// ── BUG-AUDIT-133-CPP Test 3 ────────────────────────────────────────────────
// instCfg=3 (Force Instruction): ReadPrivileged should be promoted to
// ExecutePrivileged.  A page mapped with Read+Write+Execute (all perms) should
// be accessible via a ReadPrivileged access promoted to ExecutePrivileged.
// More specifically: a page with execute=true but read=false must be accessible.
TEST(BugAuditP1InstcfgTest, InstCfg3_ForceInstruction_ExecuteOnlyAccessViaRead)
{
    SMMU smmu;
    smmu.enable();

    StreamConfig cfg = makeStage1Config(3u);  // INSTCFG=3 = Force Instruction
    VoidResult cfgResult = smmu.configureStream(SID_INSTCFG + 2u, cfg);
    ASSERT_TRUE(cfgResult.isOk()) << "configureStream failed";
    ASSERT_TRUE(smmu.enableStream(SID_INSTCFG + 2u).isOk()) << "enableStream failed";
    ASSERT_TRUE(smmu.createStreamPASID(SID_INSTCFG + 2u, 0u).isOk()) << "createStreamPASID failed";

    // Map with Execute-only (no read, no write)
    PagePermissions execOnly;
    execOnly.read    = false;
    execOnly.write   = false;
    execOnly.execute = true;
    VoidResult mapResult = smmu.mapPage(SID_INSTCFG + 2u, 0u, 0x3000u, 0x3000u, execOnly);
    ASSERT_TRUE(mapResult.isOk()) << "mapPage failed";

    // instCfg=0 (no override): Read on Execute-only page must fault
    StreamConfig cfgNoOverride = makeStage1Config(0u);
    SMMU smmu2;
    smmu2.enable();
    smmu2.configureStream(SID_INSTCFG + 2u, cfgNoOverride);
    smmu2.enableStream(SID_INSTCFG + 2u);
    smmu2.createStreamPASID(SID_INSTCFG + 2u, 0u);
    smmu2.mapPage(SID_INSTCFG + 2u, 0u, 0x3000u, 0x3000u, execOnly);
    TranslationResult noOverrideResult = smmu2.translate(SID_INSTCFG + 2u, 0u, 0x3000u,
                                                         AccessType::Read,
                                                         SecurityState::NonSecure);
    EXPECT_TRUE(noOverrideResult.isError())
        << "Sanity check: instCfg=0 must NOT promote Read->Execute; Read on "
           "Execute-only page must fault.";

    // instCfg=3 (Force Instruction): Read should be promoted to Execute -> success
    TranslationResult result = smmu.translate(SID_INSTCFG + 2u, 0u, 0x3000u,
                                              AccessType::Read,
                                              SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "BUG-AUDIT-133-CPP: instCfg=3 must promote Read->Execute and succeed "
           "on Execute-only page.  Before fix: instCfg==1u check is used, so "
           "instCfg=3 is dead code and Read stays Read, causing PermissionFault.";
}

}  // namespace test
}  // namespace smmu
