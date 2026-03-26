// TDD failing tests for BUG-NEW-15, BUG-NEW-16, BUG-NEW-18, BUG-NEW-20,
// BUG-NEW-22, and BUG-NEW-23 (C++ implementation).
//
// Each test is written to FAIL with the current code and PASS only after the
// corresponding fix is applied.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-15 (C++ §4.1.6): CFGI commands missing SSec=1 check → CERROR_ILL
//
//   CMD_CFGI_STE, CMD_CFGI_ALL, CMD_CFGI_CD, CMD_CFGI_CD_ALL do NOT check
//   command.ssec before executing.  ARM §4.1.6: any NS-queue command with
//   SSec=1 must raise CERROR_ILL and GERROR.CMDQ_ERR.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-16 (C++ §4.4.2.7): IDR0.Hyp hardcoded=1 makes EL2_ALL guard dead
//
//   getIDR0() hardcodes bit 9 (Hyp) to 1, making the CMD_TLBI_EL2_ALL guard
//   permanently dead code.  Hyp must be configurable via setHypSupported().
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-18 (C++ §4.4.4.1): NSNH_ALL evicts Secure EL1_EL0 entries
//
//   invalidateNonHypEntries() filters on strw==EL1_EL0 but does NOT check
//   securityState==NonSecure.  Secure EL1_EL0 TLB entries must be preserved.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-20 (C++ §7.3.20): SSV set by pasid != 0 instead of s1cdMax > 0
//
//   ARM §7.3.20: SSV=1 when a SubstreamID was presented, meaning the stream
//   is substream-capable (s1cdMax > 0).  PASID=0 on a substream-capable stream
//   is a valid substream presentation; SSV must still be 1.
//
// ─────────────────────────────────────────────────────────────────────────────
// BUG-NEW-22 (C++ §3.13): Dirty-bit tracking coverage
//
//   The current isWrite check in updateAccessFlags() covers Write, ReadWrite,
//   WritePrivileged, ReadWritePrivileged.  The AccessType enum has no
//   WriteExecute/ReadWriteExecute variants in C++.  This test verifies all
//   existing write variants correctly set the dirty bit (regression guard +
//   complete coverage).
//
// ─────────────────────────────────────────────────────────────────────────────

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
#include <cstdint>
#include <memory>
#include <vector>

using namespace smmu;

// ============================================================================
// Helpers
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Returns true when GERROR.CMDQ_ERR is active (bit toggled relative to GERRORN).
static bool isGerrorCmdqErrActive(const SMMU& s) {
    return ((s.getGerror() ^ s.getGerrorN()) & GERROR_CMDQ_ERR) != 0u;
}

// Helper: submit a command and process it; returns the CMDQ_CONS.ERR value.
static uint32_t submitAndProcess(SMMU& s, const CommandEntry& cmd) {
    EXPECT_TRUE(s.submitCommand(cmd).isOk());
    s.processCommandQueue();
    return s.getCmdqConsErr();
}

// Build a minimal stage-1-only stream config for a given ASID.
static StreamConfig makeStage1Config(uint16_t asid,
                                     SecurityState ss = SecurityState::NonSecure,
                                     StreamWorld sw   = StreamWorld::EL1_EL0) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.securityState      = ss;
    cfg.asid               = asid;
    cfg.strw               = sw;
    cfg.t0sz               = 16u;
    cfg.aa64               = true;
    return cfg;
}

} // anonymous namespace

// ============================================================================
// BUG-NEW-15: CFGI_STE / CFGI_ALL / CFGI_CD / CFGI_CD_ALL with SSec=1
//             must raise CERROR_ILL (ARM §4.1.6)
// ============================================================================

// Test: CMD_CFGI_STE with ssec=true raises CERROR_ILL.
//
// BEFORE FIX: processCommand() silently calls executeInvalidationCommandLocked()
// without checking ssec, so no error is raised.
//
// AFTER FIX:  the ssec guard raises CERROR_ILL before execution.
TEST(BugNew15CfgiSsec, CfgiSteSsec1RaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_STE;
    cmd.streamID = 0xA0u;
    cmd.ssec     = true;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_ILL)
        << "BUG-NEW-15: CMD_CFGI_STE with ssec=1 must raise CERROR_ILL (ARM §4.1.6)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-15: CMD_CFGI_STE with ssec=1 must assert GERROR.CMDQ_ERR";
}

// Test: CMD_CFGI_ALL with ssec=true raises CERROR_ILL.
TEST(BugNew15CfgiSsec, CfgiAllSsec1RaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_ALL;
    cmd.ssec = true;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_ILL)
        << "BUG-NEW-15: CMD_CFGI_ALL with ssec=1 must raise CERROR_ILL (ARM §4.1.6)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-15: CMD_CFGI_ALL with ssec=1 must assert GERROR.CMDQ_ERR";
}

// Test: CMD_CFGI_CD with ssec=true raises CERROR_ILL.
TEST(BugNew15CfgiSsec, CfgiCdSsec1RaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD;
    cmd.streamID = 0xB0u;
    cmd.pasid    = 1u;
    cmd.ssec     = true;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_ILL)
        << "BUG-NEW-15: CMD_CFGI_CD with ssec=1 must raise CERROR_ILL (ARM §4.1.6)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-15: CMD_CFGI_CD with ssec=1 must assert GERROR.CMDQ_ERR";
}

// Test: CMD_CFGI_CD_ALL with ssec=true raises CERROR_ILL.
TEST(BugNew15CfgiSsec, CfgiCdAllSsec1RaisesCerrorIll) {
    SMMU smmu;
    enableSMMU(smmu);

    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_CD_ALL;
    cmd.streamID = 0xC0u;
    cmd.ssec     = true;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_ILL)
        << "BUG-NEW-15: CMD_CFGI_CD_ALL with ssec=1 must raise CERROR_ILL (ARM §4.1.6)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-15: CMD_CFGI_CD_ALL with ssec=1 must assert GERROR.CMDQ_ERR";
}

// Negative test: CMD_CFGI_STE with ssec=false executes normally (no error).
TEST(BugNew15CfgiSsec, CfgiSteSsec0NoError) {
    SMMU smmu;
    enableSMMU(smmu);

    // Configure and enable a stream so the CFGI_STE has a valid target.
    ASSERT_TRUE(smmu.configureStream(0xD0u, makeStage1Config(0x01u)).isOk());
    ASSERT_TRUE(smmu.enableStream(0xD0u).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(0xD0u, 0u).isOk());

    CommandEntry cmd;
    cmd.type     = CommandType::CFGI_STE;
    cmd.streamID = 0xD0u;
    cmd.ssec     = false;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_NONE)
        << "BUG-NEW-15 neg: CMD_CFGI_STE with ssec=0 must NOT raise CERROR_ILL";

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-15 neg: GERROR.CMDQ_ERR must NOT be asserted for ssec=0 CFGI_STE";
}

// ============================================================================
// BUG-NEW-16: IDR0.Hyp must be configurable so the EL2_ALL guard is testable
//             (ARM §4.4.2.7 / §6.3.1)
// ============================================================================

// Test: With default state (Hyp support enabled), CMD_TLBI_EL2_ALL succeeds.
//
// BEFORE FIX (dead code): Hyp is always 1 → guard never fires.
// AFTER FIX: setHypSupported(true) → guard does not fire → command succeeds.
TEST(BugNew16HypSupported, El2AllSucceedsWhenHypEnabled) {
    SMMU smmu;
    enableSMMU(smmu);

    // Default state: Hyp supported → IDR0 bit 9 = 1.
    EXPECT_NE(smmu.getIDR0() & (1u << 9u), 0u)
        << "BUG-NEW-16 precondition: IDR0.Hyp must be 1 by default";

    CommandEntry cmd;
    cmd.type = CommandType::TLBI_EL2_ALL;
    cmd.ssec = false;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_NONE)
        << "BUG-NEW-16: CMD_TLBI_EL2_ALL must succeed when IDR0.Hyp=1 (default)";

    EXPECT_FALSE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-16: GERROR.CMDQ_ERR must NOT be asserted when EL2_ALL succeeds";
}

// Test: After setHypSupported(false), IDR0.Hyp = 0, and CMD_TLBI_EL2_ALL
//       raises CERROR_ILL.
//
// BEFORE FIX: setHypSupported() does not exist; test does not compile.
//             OR: IDR0.Hyp is hardcoded to 1 so the guard never fires.
//
// AFTER FIX: setHypSupported(false) clears IDR0.Hyp → guard fires → CERROR_ILL.
TEST(BugNew16HypSupported, El2AllRaisesCerrorIllWhenHypDisabled) {
    SMMU smmu;
    enableSMMU(smmu);

    // BUG-NEW-16: setHypSupported() does not exist yet.
    // This call WILL NOT COMPILE until the method is added to SMMU — that is
    // the expected "red" state.
    smmu.setHypSupported(false);

    EXPECT_EQ(smmu.getIDR0() & (1u << 9u), 0u)
        << "BUG-NEW-16: IDR0.Hyp must be 0 after setHypSupported(false)";

    CommandEntry cmd;
    cmd.type = CommandType::TLBI_EL2_ALL;
    cmd.ssec = false;

    uint32_t err = submitAndProcess(smmu, cmd);

    EXPECT_EQ(err, CERROR_ILL)
        << "BUG-NEW-16: CMD_TLBI_EL2_ALL must raise CERROR_ILL when IDR0.Hyp=0 (§4.4.2.7)";

    EXPECT_TRUE(isGerrorCmdqErrActive(smmu))
        << "BUG-NEW-16: GERROR.CMDQ_ERR must be asserted when EL2_ALL fires on Hyp=0";
}

// ============================================================================
// BUG-NEW-18: CMD_TLBI_NSNH_ALL must preserve Secure EL1_EL0 TLB entries
//             (ARM §4.4.4.1)
// ============================================================================
//
// BEFORE FIX: invalidateNonHypEntries() checks only strw==EL1_EL0, evicting
//             both Secure and NonSecure EL1_EL0 entries.
//
// AFTER FIX: the filter adds securityState==NonSecure, so Secure EL1_EL0
//            entries are preserved.
TEST(BugNew18NsnhAllScope, SecureEl1El0EntrySurvivesNsnhAll) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID_NS_EL1 = 0x50u;
    static constexpr StreamID SID_SEC_EL1 = 0x51u;
    static constexpr IOVA     IOVA_NS  = 0x1000u;
    static constexpr IOVA     IOVA_SEC = 0x2000u;
    static constexpr PA       PA_NS    = 0x100000u;
    static constexpr PA       PA_SEC   = 0x200000u;

    // NonSecure EL1_EL0 stream.
    ASSERT_TRUE(smmu.configureStream(SID_NS_EL1, makeStage1Config(0x10u,
                                    SecurityState::NonSecure,
                                    StreamWorld::EL1_EL0)).isOk());
    ASSERT_TRUE(smmu.enableStream(SID_NS_EL1).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID_NS_EL1, 0u).isOk());
    ASSERT_TRUE(smmu.mapPage(SID_NS_EL1, 0u, IOVA_NS, PA_NS,
                             PagePermissions(true, false, false),
                             SecurityState::NonSecure).isOk());

    // Secure EL1_EL0 stream.
    ASSERT_TRUE(smmu.configureStream(SID_SEC_EL1, makeStage1Config(0x11u,
                                    SecurityState::Secure,
                                    StreamWorld::EL1_EL0)).isOk());
    ASSERT_TRUE(smmu.enableStream(SID_SEC_EL1).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID_SEC_EL1, 0u).isOk());
    ASSERT_TRUE(smmu.mapPage(SID_SEC_EL1, 0u, IOVA_SEC, PA_SEC,
                             PagePermissions(true, false, false),
                             SecurityState::Secure).isOk());

    // Warm up TLB for both streams.
    ASSERT_TRUE(smmu.translate(SID_NS_EL1, 0u, IOVA_NS, AccessType::Read,
                               SecurityState::NonSecure).isOk())
        << "BUG-NEW-18 setup: NS EL1_EL0 translate must succeed";
    ASSERT_TRUE(smmu.translate(SID_SEC_EL1, 0u, IOVA_SEC, AccessType::Read,
                               SecurityState::Secure).isOk())
        << "BUG-NEW-18 setup: Secure EL1_EL0 translate must succeed";

    // Unmap the NS page so that a post-invalidation translate walks the
    // page table; if the TLB was properly evicted the walk will fault.
    smmu.unmapPage(SID_NS_EL1, 0u, IOVA_NS);

    // Issue CMD_TLBI_NSNH_ALL — must evict NS EL1_EL0 entries ONLY.
    CommandEntry cmd;
    cmd.type = CommandType::TLBI_NSNH_ALL;
    cmd.ssec = false;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // NS EL1_EL0 entry must be evicted → walk faults on the now-unmapped page.
    TranslationResult rNS = smmu.translate(SID_NS_EL1, 0u, IOVA_NS, AccessType::Read,
                                           SecurityState::NonSecure);
    EXPECT_TRUE(rNS.isError())
        << "BUG-NEW-18: NSNH_ALL must evict the NonSecure EL1_EL0 TLB entry "
           "(translate on unmapped NS page must fault)";

    // Secure EL1_EL0 entry must survive — translate still returns the old PA.
    TranslationResult rSec = smmu.translate(SID_SEC_EL1, 0u, IOVA_SEC, AccessType::Read,
                                            SecurityState::Secure);
    EXPECT_TRUE(rSec.isOk())
        << "BUG-NEW-18: NSNH_ALL must NOT evict Secure EL1_EL0 TLB entries "
           "(ARM §4.4.4.1 — scope is NonSecure world only)";
}

// ============================================================================
// BUG-NEW-20: SSV=1 when s1cdMax > 0, NOT when pasid != 0 (ARM §7.3.20)
// ============================================================================
//
// ARM §7.3.20: SSV (SubstreamID Valid) is set when a SubstreamID was presented
// by the transaction.  A stream is substream-capable when s1cdMax > 0.
// PASID=0 on such a stream is a valid substream presentation; SSV must be 1.
//
// BEFORE FIX: generateEvent() sets ssv = (pasid != 0).
//             With pasid=0, ssv=false even on a substream-capable stream.
//
// AFTER FIX:  ssv = (config.s1cdMax > 0); PASID=0 on s1cdMax>0 stream → ssv=1.
TEST(BugNew20SsvS1cdMax, Pasid0OnSubstreamCapableStreamHasSsvTrue) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID = 0x60u;

    // Configure a substream-capable stage-1 stream (s1cdMax > 0).
    // Use t0sz=0 so ANY IOVA is valid (no F_TRANSLATION from T0SZ range check).
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.asid               = 0x20u;
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.t0sz               = 16u;
    cfg.aa64               = true;
    cfg.s1cdMax            = 4u;  // substream-capable (s1cdMax > 0)
    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());

    // Create PASID=0 context but do NOT map any page so the translation faults.
    // The fault event will carry the SSV flag that we want to observe.
    ASSERT_TRUE(smmu.createStreamPASID(SID, 0u).isOk());

    // Translate with PASID=0 to an unmapped address → generates a fault event.
    // We do not care about success or failure; we only inspect the event record.
    (void)smmu.translate(SID, 0u /*pasid=0*/, 0x5000u, AccessType::Read);

    // Retrieve the event queue and find a translation-related fault event.
    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-20 setup: a fault event must be generated for the unmapped address";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_TRANSLATION || e.type == EventType::F_PERMISSION ||
            e.type == EventType::F_ADDR_SIZE   || e.type == EventType::C_BAD_CD     ||
            e.type == EventType::C_BAD_STE) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "BUG-NEW-20 setup: a fault event (F_TRANSLATION or similar) must be present";

    // ARM §7.3.20: SSV must be 1 because the stream is substream-capable
    // (s1cdMax > 0), regardless of PASID value.
    EXPECT_TRUE(ev->ssv)
        << "BUG-NEW-20: SSV must be 1 for a substream-capable stream (s1cdMax > 0) "
           "even when PASID=0 (ARM §7.3.20)";
}

// Negative: on a non-substream-capable stream (s1cdMax==0), PASID=0 → ssv=false.
TEST(BugNew20SsvS1cdMax, Pasid0OnNonSubstreamStreamHasSsvFalse) {
    SMMU smmu;
    enableSMMU(smmu);

    static constexpr StreamID SID = 0x61u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.bypassEnabled      = false;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.asid               = 0x21u;
    cfg.strw               = StreamWorld::EL1_EL0;
    cfg.t0sz               = 16u;
    cfg.aa64               = true;
    cfg.s1cdMax            = 0u;  // NOT substream-capable
    ASSERT_TRUE(smmu.configureStream(SID, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(SID).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(SID, 0u).isOk());

    // Translate to an unmapped address → fault event with ssv=false expected.
    (void)smmu.translate(SID, 0u, 0x6000u, AccessType::Read);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "BUG-NEW-20 neg setup: a fault event must be generated";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_TRANSLATION || e.type == EventType::F_PERMISSION ||
            e.type == EventType::F_ADDR_SIZE   || e.type == EventType::C_BAD_CD     ||
            e.type == EventType::C_BAD_STE) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "BUG-NEW-20 neg setup: a fault event must be present";

    // s1cdMax==0 → stream is NOT substream-capable → SSV must be false.
    EXPECT_FALSE(ev->ssv)
        << "BUG-NEW-20 neg: SSV must be 0 when s1cdMax==0 (not substream-capable)";
}

// ============================================================================
// BUG-NEW-22: updateAccessFlags() dirty-bit coverage (ARM §3.13)
// ============================================================================
//
// ARM §3.13: dirty-bit tracking applies to any write access regardless of
// privilege level or combine-with-execute semantics.  The current isWrite
// check covers: Write, ReadWrite, WritePrivileged, ReadWritePrivileged.
// The AccessType enum does not define WriteExecute/ReadWriteExecute variants,
// so those are not applicable.  These tests verify complete coverage of the
// four existing write variants.

class DirtyBitCoverageTest : public ::testing::Test {
protected:
    std::unique_ptr<AddressSpace> as;
    static constexpr IOVA TEST_IOVA = 0x10000u;
    static constexpr PA   TEST_PA   = 0x80000000u;

    void SetUp() override {
        as = std::make_unique<AddressSpace>();
        as->mapPage(TEST_IOVA, TEST_PA, PagePermissions(true, true, false));
    }

    // Remap to a fresh (clean, AF=1) page before each sub-test.
    void remapFresh() {
        as->unmapPage(TEST_IOVA);
        as->mapPage(TEST_IOVA, TEST_PA, PagePermissions(true, true, false));
    }
};

// Baseline: Write must set dirty bit.
TEST_F(DirtyBitCoverageTest, Write_SetsDirtyBit) {
    bool changed = as->updateAccessFlags(TEST_IOVA, true, true, AccessType::Write);
    EXPECT_TRUE(changed)   << "BUG-NEW-22: Write must change dirty state";
    EXPECT_TRUE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22: Write must set dirty bit (ARM §3.13)";
}

// ReadWrite must set dirty bit.
TEST_F(DirtyBitCoverageTest, ReadWrite_SetsDirtyBit) {
    remapFresh();
    bool changed = as->updateAccessFlags(TEST_IOVA, true, true, AccessType::ReadWrite);
    EXPECT_TRUE(changed)   << "BUG-NEW-22: ReadWrite must change dirty state";
    EXPECT_TRUE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22: ReadWrite must set dirty bit (ARM §3.13)";
}

// WritePrivileged must set dirty bit.
TEST_F(DirtyBitCoverageTest, WritePrivileged_SetsDirtyBit) {
    remapFresh();
    bool changed = as->updateAccessFlags(TEST_IOVA, true, true,
                                         AccessType::WritePrivileged);
    EXPECT_TRUE(changed)
        << "BUG-NEW-22: WritePrivileged must change dirty state";
    EXPECT_TRUE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22: WritePrivileged must set dirty bit (ARM §3.13)";
}

// ReadWritePrivileged must set dirty bit.
TEST_F(DirtyBitCoverageTest, ReadWritePrivileged_SetsDirtyBit) {
    remapFresh();
    bool changed = as->updateAccessFlags(TEST_IOVA, true, true,
                                         AccessType::ReadWritePrivileged);
    EXPECT_TRUE(changed)
        << "BUG-NEW-22: ReadWritePrivileged must change dirty state";
    EXPECT_TRUE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22: ReadWritePrivileged must set dirty bit (ARM §3.13)";
}

// Read must NOT set dirty bit.
TEST_F(DirtyBitCoverageTest, Read_DoesNotSetDirtyBit) {
    remapFresh();
    as->updateAccessFlags(TEST_IOVA, true, true, AccessType::Read);
    EXPECT_FALSE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22 neg: Read must NOT set dirty bit";
}

// Execute must NOT set dirty bit.
TEST_F(DirtyBitCoverageTest, Execute_DoesNotSetDirtyBit) {
    remapFresh();
    as->updateAccessFlags(TEST_IOVA, true, true, AccessType::Execute);
    EXPECT_FALSE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22 neg: Execute must NOT set dirty bit";
}

// ReadExecute must NOT set dirty bit.
TEST_F(DirtyBitCoverageTest, ReadExecute_DoesNotSetDirtyBit) {
    remapFresh();
    as->updateAccessFlags(TEST_IOVA, true, true, AccessType::ReadExecute);
    EXPECT_FALSE(as->getPageDirty(TEST_IOVA))
        << "BUG-NEW-22 neg: ReadExecute must NOT set dirty bit";
}
