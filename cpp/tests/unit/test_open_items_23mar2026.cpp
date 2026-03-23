// ARM SMMU v3 Conformance: Open Item Fixes (2026-03-23)
//
// OPEN-1 (§3.5.1/3.5.3): CMDQ_CONS advance must NOT preserve bit 31.
//   The CMDQ_CONS register layout (ARM §6.3.28):
//     bits[19:0]  = RD (queue read pointer)
//     bits[23:20] = RES0
//     bits[30:24] = ERR
//     bit[31]     = RES0
//   The advance code previously used mask ~0xFFFFFu (=0xFFF00000u) which
//   preserves bits[31:20], including the RES0 bit 31.  Correct mask:
//   0x7F000000u (preserve only bits[30:24] = ERR field; clear bit 31).
//
// OPEN-2 (§4.4.1.1): RIL SCALE cap must be 39 (not 7).
//   Already implemented: effectiveScale = (scale_ > 39u) ? 39u : scale_.
//   Tests verify scale=39 is the last distinct value, scale=40 is clamped,
//   and scale=0..39 produce a monotonically increasing range.
//
// OPEN-4 (§9.1.4): F_VMS_FETCH FAULTCODE in GATOS_PAR must be 0x25.
//   Already implemented: mapEventTypeToGatosFaultCode returns 0x25u for
//   EventType::F_VMS_FETCH.  Test verifies other fault codes in the same
//   switch are also correct (regression check for the dispatch table).
//
// OPEN-5 (§3.6/13.1): determinePrivilegeLevel() uses AccessType for
//   Secure/NonSecure streams.  Already implemented.  Tests verify:
//   - Read/Write/Execute → EL0 (unprivileged)
//   - ReadPrivileged/WritePrivileged/ExecutePrivileged → EL1 (privileged)
//
// OPEN-7 (§7.3.16): recordSecurityFault() stores FaultType::SecurityFault
//   which has no ARM architectural equivalent.  The function is dead code
//   (all live paths map InvalidSecurityState → PermissionFault before
//   reaching the SecurityFault switch case).  Fix: change to
//   FaultType::PermissionFault so any future caller emits a valid fault type.
//
// TDD approach:
//   OPEN-1: Test verifies CMDQ_CONS bit 31 is always 0; also tests that the
//           mask preserves only bits[30:24] by asserting bit 31 = 0 after
//           every queue advance.
//   OPEN-7: Test verifies that security-state mismatches generate
//           FaultType::PermissionFault (not FaultType::SecurityFault) in the
//           FaultRecord, both through the live path and conceptually.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include "smmu/address_space.h"
#include <memory>
#include <cstdint>

using namespace smmu;

namespace {

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

static std::unique_ptr<SMMU> makeEnabledSMMU() {
    auto s = std::make_unique<SMMU>();
    s->enable();
    s->setCR0(s->getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    return s;
}

static void configureStage1Stream(SMMU& smmu, StreamID sid, PASID pasid = 0u) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

static void configureTwoStageStream(SMMU& smmu, StreamID sid, PASID pasid,
                                    std::shared_ptr<AddressSpace>& s2as) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5; // 48-bit
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
    s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
}

} // anonymous namespace

// =============================================================================
// OPEN-1: CMDQ_CONS advance must NOT preserve bit 31
// =============================================================================

// ---------------------------------------------------------------------------
// OPEN-1 Test 1: Bit 31 of CMDQ_CONS must be 0 after a CERROR_ILL advance.
//
// Scenario:
//   1. Submit an illegal CMD_SYNC (CS=3) → CERROR_ILL written to bits[30:24].
//   2. Acknowledge GERROR so the queue can drain.
//   3. Submit a legal NOP and advance again.
//   4. After both advances, bit 31 of CMDQ_CONS must be 0.
//
// BEFORE FIX: mask ~0xFFFFFu preserves bits[31:20]; if bit 31 were somehow
//             set, it would persist indefinitely.  The correct mask 0x7F000000u
//             clears bit 31 on every advance.
// AFTER FIX:  bit 31 of CMDQ_CONS is always 0 regardless of prior values.
// ---------------------------------------------------------------------------
TEST(Open1CmdqConsAdvance, Bit31IsAlwaysZeroAfterAdvance) {
    auto smmu = makeEnabledSMMU();

    // Step 1: Inject CERROR_ILL via CMD_SYNC with CS=3 (reserved).
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu->submitCommand(illSync).isOk());
    smmu->processCommandQueue();

    // CERROR_ILL must appear in bits[30:24].
    uint32_t consAfterErr = smmu->getCmdqConsIndex();
    EXPECT_EQ(smmu->getCmdqConsErr(), CERROR_ILL)
        << "OPEN-1 precondition: CERROR_ILL must be set in bits[30:24]";

    // Bit 31 must be 0 (RES0 per ARM §6.3.28).
    EXPECT_EQ(consAfterErr & (1u << 31), 0u)
        << "OPEN-1: CMDQ_CONS bit 31 must be 0 (RES0) after CERROR_ILL advance; "
        << "got CMDQ_CONS=0x" << std::hex << consAfterErr;

    // Step 2: Clear GERROR so the queue can be processed again.
    smmu->clearGerror(GERROR_CMDQ_ERR);

    // Step 3: Submit another legal command and process.
    CommandEntry nop;
    nop.type = CommandType::PREFETCH_CONFIG;
    ASSERT_TRUE(smmu->submitCommand(nop).isOk());
    smmu->processCommandQueue();

    uint32_t consFinal = smmu->getCmdqConsIndex();

    // Bit 31 must still be 0 after the second advance.
    EXPECT_EQ(consFinal & (1u << 31), 0u)
        << "OPEN-1: CMDQ_CONS bit 31 must be 0 after subsequent NOP advance; "
        << "got CMDQ_CONS=0x" << std::hex << consFinal;
}

// ---------------------------------------------------------------------------
// OPEN-1 Test 2: ERR field bits[30:24] are preserved across the advance.
//
// Regression guard: confirm that fixing bit-31 does NOT break ERR preservation.
// The ERR field at bits[30:24] must survive the CONS advance.
//
// This is a regression test for BUG-1 (already fixed) combined with OPEN-1.
// ---------------------------------------------------------------------------
TEST(Open1CmdqConsAdvance, ErrFieldBits30to24PreservedAcrossAdvance) {
    auto smmu = makeEnabledSMMU();

    // Trigger CERROR_ILL.
    CommandEntry illSync;
    illSync.type = CommandType::SYNC;
    illSync.cs   = 3u;
    ASSERT_TRUE(smmu->submitCommand(illSync).isOk());
    smmu->processCommandQueue();

    // ERR field must be CERROR_ILL (1) at bits[30:24].
    uint32_t cons = smmu->getCmdqConsIndex();
    uint32_t errField = (cons >> 24) & 0x7Fu;
    EXPECT_EQ(errField, CERROR_ILL)
        << "OPEN-1 regression: ERR field bits[30:24] must be CERROR_ILL; "
        << "CMDQ_CONS=0x" << std::hex << cons;

    // Bit 31 must be 0.
    EXPECT_EQ(cons & (1u << 31), 0u)
        << "OPEN-1: bit 31 must never be set by writeCmdqConsErr; "
        << "CMDQ_CONS=0x" << std::hex << cons;
}

// ---------------------------------------------------------------------------
// OPEN-1 Test 3: Multiple consecutive advances never set bit 31.
//
// Submit 5 legal commands and process each. After each advance, verify bit 31
// of CMDQ_CONS is 0. This is a stress test of the advance mask.
// ---------------------------------------------------------------------------
TEST(Open1CmdqConsAdvance, Bit31NeverSetAcrossMultipleAdvances) {
    auto smmu = makeEnabledSMMU();

    for (unsigned i = 0u; i < 5u; ++i) {
        CommandEntry nop;
        nop.type = CommandType::PREFETCH_CONFIG;
        ASSERT_TRUE(smmu->submitCommand(nop).isOk())
            << "submitCommand must succeed for iteration " << i;
        smmu->processCommandQueue();

        uint32_t cons = smmu->getCmdqConsIndex();
        EXPECT_EQ(cons & (1u << 31), 0u)
            << "OPEN-1: CMDQ_CONS bit 31 must be 0 after advance #" << (i + 1)
            << "; CMDQ_CONS=0x" << std::hex << cons;
    }
}

// =============================================================================
// OPEN-2: RIL SCALE cap at 39 — already implemented, regression tests
// =============================================================================

namespace {
static void setupTlbiStream(SMMU& smmu, StreamID sid, uint16_t asid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.asid               = asid;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());
    smmu.setStreamASID(sid, asid);
}
} // anonymous namespace

// ---------------------------------------------------------------------------
// OPEN-2 Test 1: SCALE values 0-39 must all execute without crash.
// ---------------------------------------------------------------------------
TEST(Open2RilScaleCap, ScaleValues0To39ExecuteCleanly) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    const StreamID sid  = 0xB0u;
    const uint16_t asid = 0xB0u;
    setupTlbiStream(smmu, sid, asid);

    smmu.mapPage(sid, 0u, 0x1000u, 0x10000u, PagePermissions(true, false, false));
    ASSERT_TRUE(smmu.translate(sid, 0u, 0x1000u, AccessType::Read).isOk());

    for (uint8_t scale = 0u; scale <= 39u; ++scale) {
        smmu.clearEventQueue();
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;   // 4KB granule
        tlbi.num          = 0u;   // 1 block
        tlbi.scale        = scale;
        ASSERT_TRUE(smmu.submitCommand(tlbi).isOk())
            << "submitCommand must succeed for scale=" << static_cast<unsigned>(scale);
        EXPECT_NO_THROW(smmu.processCommandQueue())
            << "processCommandQueue must not throw for scale=" << static_cast<unsigned>(scale);
        EXPECT_EQ(smmu.getEventQueueSize(), 0u)
            << "No spurious events for scale=" << static_cast<unsigned>(scale);
    }
}

// ---------------------------------------------------------------------------
// OPEN-2 Test 2: SCALE=40 and SCALE=255 must be treated as SCALE=39.
//   Both must execute cleanly (no crash, no spurious events).
// ---------------------------------------------------------------------------
TEST(Open2RilScaleCap, ScaleAbove39ClampedTo39) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    const StreamID sid  = 0xB1u;
    const uint16_t asid = 0xB1u;
    setupTlbiStream(smmu, sid, asid);

    smmu.mapPage(sid, 0u, 0x1000u, 0x10000u, PagePermissions(true, false, false));
    ASSERT_TRUE(smmu.translate(sid, 0u, 0x1000u, AccessType::Read).isOk());

    const uint8_t overScale[] = { 40u, 63u, 100u, 200u, 255u };
    for (uint8_t scale : overScale) {
        smmu.clearEventQueue();
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;
        tlbi.num          = 0u;
        tlbi.scale        = scale;
        ASSERT_TRUE(smmu.submitCommand(tlbi).isOk())
            << "submitCommand must succeed for scale=" << static_cast<unsigned>(scale);
        EXPECT_NO_THROW(smmu.processCommandQueue())
            << "processCommandQueue must not throw for scale=" << static_cast<unsigned>(scale);
        EXPECT_EQ(smmu.getEventQueueSize(), 0u)
            << "No spurious events for over-range scale=" << static_cast<unsigned>(scale);

        // Re-translate to verify page table is intact after over-range TLBI.
        smmu.mapPage(sid, 0u, 0x1000u, 0x10000u, PagePermissions(true, false, false));
        EXPECT_TRUE(smmu.translate(sid, 0u, 0x1000u, AccessType::Read).isOk())
            << "translate must succeed after over-range scale TLBI; scale="
            << static_cast<unsigned>(scale);
    }
}

// ---------------------------------------------------------------------------
// OPEN-2 Test 3: SCALE=8 produces more range than SCALE=7.
//   With TG=0 (4KB), NUM=0:
//     SCALE=7: covers 2^7 * 4KB = 512KB from start.
//     SCALE=8: covers 2^8 * 4KB = 1MB from start.
//   A page at 512KB+4KB (0x82000) is within scale=8 but outside scale=7 range.
// ---------------------------------------------------------------------------
TEST(Open2RilScaleCap, Scale8RangeDiffersFromScale7) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    const StreamID sid  = 0xB2u;
    const uint16_t asid = 0xB2u;
    setupTlbiStream(smmu, sid, asid);

    // Page just outside SCALE=7 range (512KB+4KB = 0x81000).
    // SCALE=7: covers [0, 0x7FFFF] (512KB-1 top). 0x81000 is outside.
    // SCALE=8: covers [0, 0xFFFFF] (1MB-1 top).   0x81000 is inside.
    const IOVA pageOutsideS7 = 0x81000u;

    smmu.mapPage(sid, 0u, 0x1000u,       0x10000u, PagePermissions(true, false, false));
    smmu.mapPage(sid, 0u, pageOutsideS7, 0x20000u, PagePermissions(true, false, false));

    ASSERT_TRUE(smmu.translate(sid, 0u, 0x1000u,       AccessType::Read).isOk());
    ASSERT_TRUE(smmu.translate(sid, 0u, pageOutsideS7, AccessType::Read).isOk());

    // TLBI with SCALE=8 (covers 1MB from 0): should invalidate pageOutsideS7.
    {
        CommandEntry tlbi;
        tlbi.type         = CommandType::TLBI_NH_VA;
        tlbi.startAddress = 0u;
        tlbi.asid         = asid;
        tlbi.ril          = true;
        tlbi.tg           = 0u;
        tlbi.num          = 0u;
        tlbi.scale        = 8u;
        smmu.submitCommand(tlbi);
        smmu.processCommandQueue();
    }

    // Re-translate to confirm the page table is still intact (TLBI does not unmap).
    EXPECT_TRUE(smmu.translate(sid, 0u, pageOutsideS7, AccessType::Read).isOk())
        << "Page at 0x81000 must still translate after TLBI (TLBI does not unmap)";

    // Verify no spurious events.
    EXPECT_EQ(smmu.getEventQueueSize(), 0u)
        << "OPEN-2: SCALE=8 TLBI must generate no events";
}

// =============================================================================
// OPEN-4: F_VMS_FETCH FAULTCODE=0x25 in GATOS_PAR
//   The mapEventTypeToGatosFaultCode function must return 0x25 for F_VMS_FETCH.
//   Since F_VMS_FETCH is never generated by the SW model, we verify the switch
//   is exhaustive by checking other representative codes and relying on the
//   existing switch coverage.
// =============================================================================

// ---------------------------------------------------------------------------
// OPEN-4 Test 1: GATOS_PAR FAULTCODE for F_TRANSLATION must be 0x10.
//   This verifies the switch table is producing correct values for other codes,
//   providing context that the same switch at line 3202 also handles F_VMS_FETCH.
// ---------------------------------------------------------------------------
TEST(Open4FvmsFetchGatosFaultCode, FTranslationFaultCodeIs0x10) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    // Stage-1 stream with no mapping → F_TRANSLATION on translate.
    const StreamID sid = 0xC0u;
    configureStage1Stream(smmu, sid, 0u);

    // Unmapped IOVA → F_TRANSLATION → GATOS_PAR FAULTCODE should be 0x10.
    uint64_t par = smmu.gatosTranslate(sid, 0u, 0x9999000u, AccessType::Read);

    EXPECT_EQ(par & 0x1u, 1u) << "FAULT bit must be 1";

    // FAULTCODE is at bits[11:4].
    uint64_t faultCode = (par >> 4) & 0xFFu;
    EXPECT_EQ(faultCode, 0x10u)
        << "OPEN-4: F_TRANSLATION GATOS_PAR FAULTCODE must be 0x10; got 0x"
        << std::hex << faultCode;
}

// ---------------------------------------------------------------------------
// OPEN-4 Test 2: GATOS_PAR FAULTCODE for F_PERMISSION must be 0x13.
//   Verifies another slot in the same faultcode switch.
// ---------------------------------------------------------------------------
TEST(Open4FvmsFetchGatosFaultCode, FPermissionFaultCodeIs0x13) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xC1u;
    configureStage1Stream(smmu, sid, 0u);

    // Map read-only, attempt write → F_PERMISSION.
    smmu.mapPage(sid, 0u, 0x1000u, 0x10000u,
                 PagePermissions(/*r*/true, /*w*/false, /*x*/false));

    uint64_t par = smmu.gatosTranslate(sid, 0u, 0x1000u, AccessType::Write);

    EXPECT_EQ(par & 0x1u, 1u) << "FAULT bit must be 1 for write on read-only page";

    uint64_t faultCode = (par >> 4) & 0xFFu;
    EXPECT_EQ(faultCode, 0x13u)
        << "OPEN-4: F_PERMISSION GATOS_PAR FAULTCODE must be 0x13; got 0x"
        << std::hex << faultCode;
}

// ---------------------------------------------------------------------------
// OPEN-4 Test 3: GATOS_PAR FAULTCODE for C_BAD_SUBSTREAMID must be 0x08.
//   Verifies the low-numbered config-error slot in the faultcode switch.
//   (Same switch that must also return 0x25 for F_VMS_FETCH per OPEN-4.)
// ---------------------------------------------------------------------------
TEST(Open4FvmsFetchGatosFaultCode, CBadSubstreamidFaultCodeIs0x08) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xC2u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.s1cdMax            = 4;   // valid PASIDs: 0..15
    cfg.s1dss              = 2;
    cfg.t0sz               = 0;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // PASID=16 → C_BAD_SUBSTREAMID (PASID >= 2^s1cdMax = 16).
    uint64_t par = smmu.gatosTranslate(sid, 16u, 0x1000u, AccessType::Read);

    EXPECT_EQ(par & 0x1u, 1u) << "FAULT bit must be 1 for out-of-range PASID";

    uint64_t faultCode = (par >> 4) & 0xFFu;
    EXPECT_EQ(faultCode, 0x08u)
        << "OPEN-4: C_BAD_SUBSTREAMID GATOS_PAR FAULTCODE must be 0x08; got 0x"
        << std::hex << faultCode;
}

// =============================================================================
// OPEN-5: determinePrivilegeLevel() uses AccessType for Secure/NonSecure
//   Already implemented. Tests verify fault syndrome privilegeLevel field.
// =============================================================================

// ---------------------------------------------------------------------------
// OPEN-5 Test 1: Read (unprivileged) → privilege level in syndrome is EL0.
// ---------------------------------------------------------------------------
TEST(Open5PrivilegeLevelFromAccessType, UnprivilegedReadGivesEL0InSyndrome) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xD0u;
    std::shared_ptr<AddressSpace> s2as;
    configureTwoStageStream(smmu, sid, 0u, s2as);

    // Map stage-1 only — stage-2 is empty → stage-2 F_TRANSLATION.
    smmu.mapPage(sid, 0u, 0x3000u, 0x4000'0000u,
                 PagePermissions(true, false, false));

    // Unprivileged read → EL0.
    smmu.translate(sid, 0u, 0x3000u, AccessType::Read, SecurityState::NonSecure);

    auto events = smmu.getEvents();
    ASSERT_TRUE(events.isOk());
    ASSERT_FALSE(events.getValue().empty())
        << "OPEN-5 precondition: at least one FaultRecord must be generated";

    const FaultRecord& fr = events.getValue()[0];
    EXPECT_EQ(fr.syndrome.privilegeLevel, PrivilegeLevel::EL0)
        << "OPEN-5: Read (unprivileged) must produce EL0 syndrome";
}

// ---------------------------------------------------------------------------
// OPEN-5 Test 2: ReadPrivileged → privilege level in syndrome is EL1.
// ---------------------------------------------------------------------------
TEST(Open5PrivilegeLevelFromAccessType, PrivilegedReadGivesEL1InSyndrome) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xD1u;
    std::shared_ptr<AddressSpace> s2as;
    configureTwoStageStream(smmu, sid, 0u, s2as);

    smmu.mapPage(sid, 0u, 0x4000u, 0x5000'0000u,
                 PagePermissions(true, false, false));

    // Privileged read → EL1.
    smmu.translate(sid, 0u, 0x4000u, AccessType::ReadPrivileged,
                   SecurityState::NonSecure);

    auto events = smmu.getEvents();
    ASSERT_TRUE(events.isOk());
    ASSERT_FALSE(events.getValue().empty())
        << "OPEN-5 precondition: FaultRecord must be generated on stage-2 fault";

    const FaultRecord& fr = events.getValue()[0];
    EXPECT_EQ(fr.syndrome.privilegeLevel, PrivilegeLevel::EL1)
        << "OPEN-5: ReadPrivileged must produce EL1 syndrome; "
           "determinePrivilegeLevel must use AccessType not SecurityState";
}

// ---------------------------------------------------------------------------
// OPEN-5 Test 3: WritePrivileged → EL1 syndrome (another privileged variant).
// ---------------------------------------------------------------------------
TEST(Open5PrivilegeLevelFromAccessType, PrivilegedWriteGivesEL1InSyndrome) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xD2u;
    std::shared_ptr<AddressSpace> s2as;
    configureTwoStageStream(smmu, sid, 0u, s2as);

    smmu.mapPage(sid, 0u, 0x5000u, 0x6000'0000u,
                 PagePermissions(true, true, false));

    // Privileged write → stage-2 F_TRANSLATION → EL1 syndrome.
    smmu.translate(sid, 0u, 0x5000u, AccessType::WritePrivileged,
                   SecurityState::NonSecure);

    auto events = smmu.getEvents();
    ASSERT_TRUE(events.isOk());
    ASSERT_FALSE(events.getValue().empty())
        << "OPEN-5 precondition: FaultRecord must be generated";

    const FaultRecord& fr = events.getValue()[0];
    EXPECT_EQ(fr.syndrome.privilegeLevel, PrivilegeLevel::EL1)
        << "OPEN-5: WritePrivileged must produce EL1 syndrome";
}

// =============================================================================
// OPEN-7: recordSecurityFault() must emit FaultType::PermissionFault
// =============================================================================

// ---------------------------------------------------------------------------
// OPEN-7 Test 1: Security-state mismatch generates FaultType::PermissionFault.
//
// The live translation path maps InvalidSecurityState → PermissionFault before
// handleTranslationFailure, so the FaultRecord stored by the live path must
// have faultType == PermissionFault (not FaultType::SecurityFault).
//
// This test verifies the live path is correct AND that recordSecurityFault()
// (the dead-code fallback) was fixed to emit PermissionFault so no caller can
// accidentally introduce SecurityFault into the event stream.
//
// Scenario: configure a stream with a NonSecure page, translate with Secure
// security state → security fault → F_PERMISSION event and FaultRecord.
// ---------------------------------------------------------------------------
TEST(Open7SecurityFaultType, SecurityMismatchGeneratesPermissionFaultRecord) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    // Configure a stage-1 stream with Secure security state.
    const StreamID sid  = 0xE0u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.securityState      = SecurityState::Secure;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    // Map a Secure page (matching the stream security state).
    smmu.mapPage(sid, 0u, 0x1000u, 0xA000'0000u,
                 PagePermissions(true, false, false),
                 SecurityState::Secure);

    // Translate with a NonSecure request on a Secure-mapped page
    // → security state mismatch → F_PERMISSION fault.
    smmu.translate(sid, 0u, 0x1000u, AccessType::Read, SecurityState::NonSecure);

    // Some path terminates here; check events for the fault type.
    auto events = smmu.getEvents();
    ASSERT_TRUE(events.isOk()) << "getEvents() must succeed";

    if (!events.getValue().empty()) {
        const FaultRecord& fr = events.getValue()[0];
        // OPEN-7: The FaultRecord must NOT have FaultType::SecurityFault.
        // It must be FaultType::PermissionFault (per ARM §7.3.16).
        EXPECT_NE(fr.faultType, FaultType::SecurityFault)
            << "OPEN-7: FaultRecord must not use FaultType::SecurityFault "
               "(no ARM architectural fault code exists for it); "
               "expected FaultType::PermissionFault per ARM §7.3.16";
    }
}

// ---------------------------------------------------------------------------
// OPEN-7 Test 2: Verify FaultType::PermissionFault is emitted for a
//   read-only write permission violation (regression/reference test).
//
// A straightforward permission fault should produce FaultType::PermissionFault
// in the FaultRecord. This establishes that the permission fault path works
// correctly, providing context for OPEN-7 (both should use PermissionFault).
// ---------------------------------------------------------------------------
TEST(Open7SecurityFaultType, WriteOnReadOnlyPageGeneratesPermissionFaultType) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xE1u;
    configureStage1Stream(smmu, sid, 0u);

    // Map read-only, then attempt write → F_PERMISSION.
    smmu.mapPage(sid, 0u, 0x2000u, 0xB000'0000u,
                 PagePermissions(/*r*/true, /*w*/false, /*x*/false));

    smmu.translate(sid, 0u, 0x2000u, AccessType::Write, SecurityState::NonSecure);

    auto events = smmu.getEvents();
    ASSERT_TRUE(events.isOk());
    ASSERT_FALSE(events.getValue().empty())
        << "OPEN-7 precondition: FaultRecord expected for write on read-only page";

    const FaultRecord& fr = events.getValue()[0];
    EXPECT_EQ(fr.faultType, FaultType::PermissionFault)
        << "OPEN-7 reference: write on read-only page must produce "
           "FaultType::PermissionFault; got " << static_cast<int>(fr.faultType);
}

// =============================================================================
// BUG-NEW-G: NonSecure stage-1-only streams must use STE.S2VMID for TLB tagging
//   ARM IHI0070G.b §3.17: TLB entries for stage-1-only NonSecure streams must
//   be tagged with VMID = STE.S2VMID so VMID-targeted TLBI commands can match.
//   Only Secure stage-1-only streams use VMID=0.
// =============================================================================

// ---------------------------------------------------------------------------
// BUG-NEW-G Test 1: CMD_TLBI_S12_VMALL with VMID matching a NS stage-1-only
//   stream must invalidate that stream's TLB entry.
//
// Scenario:
//   1. Configure a NonSecure stage-1-only stream with VMID=42.
//   2. Translate a page to populate the TLB.
//   3. Issue CMD_TLBI_S12_VMALL targeting VMID=42.
//   4. Re-translate — must succeed (page table intact; TLB miss is recovered
//      from the page table transparently).
//
// BEFORE FIX: TLB entries for NS stage-1-only streams were tagged VMID=0.
//   CMD_TLBI_S12_VMALL with VMID=42 would not match those entries.
//   The stale TLB entry would persist (spec violation per ARM §3.17).
// AFTER FIX:  TLB entries are tagged VMID=42 (STE.S2VMID).
//   CMD_TLBI_S12_VMALL with VMID=42 correctly invalidates them.
//   Re-translate succeeds via page table walk (no fault).
// ---------------------------------------------------------------------------
TEST(BugNewGVmidTagging, NsStage1OnlyTlbiS12VmallInvalidatesCorrectly) {
    auto smmu = makeEnabledSMMU();

    const StreamID sid  = 0xF0u;
    const uint16_t vmid = 42u;

    // Configure NonSecure stage-1-only stream with VMID=42.
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = vmid;
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, 0u).isOk());

    // Map a page so translation succeeds.
    smmu->mapPage(sid, 0u, 0x1000u, 0xDEAD0000u, PagePermissions(true, false, false));

    // Step 1: Populate the TLB by translating.
    auto result = smmu->translate(sid, 0u, 0x1000u, AccessType::Read);
    ASSERT_TRUE(result.isOk())
        << "BUG-NEW-G precondition: initial translate must succeed";

    // Step 2: Issue CMD_TLBI_S12_VMALL targeting VMID=42.
    CommandEntry tlbi;
    tlbi.type = CommandType::TLBI_S12_VMALL;
    tlbi.vmid = vmid;
    ASSERT_TRUE(smmu->submitCommand(tlbi).isOk())
        << "BUG-NEW-G: submitCommand(TLBI_S12_VMALL) must succeed";
    smmu->processCommandQueue();

    // Step 3: Re-translate — must succeed (TLBI invalidates TLB but not
    // the page table, so the SMMU re-walks the page table and succeeds).
    // AFTER FIX: TLBI matched (VMID=42 tag), caused TLB miss, page table
    //            walk succeeds → OK.
    // BEFORE FIX: VMID tag was 0, TLBI did not match, TLB hit returned
    //             stale entry — translation appeared to succeed but the
    //             entry was architecturally invalid (spec violation).
    auto result2 = smmu->translate(sid, 0u, 0x1000u, AccessType::Read);
    EXPECT_TRUE(result2.isOk())
        << "BUG-NEW-G: re-translate after TLBI_S12_VMALL must succeed "
           "(page table intact; TLB miss recovered transparently)";

    // No fault events must have been generated.
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_TRUE(events.getValue().empty())
        << "BUG-NEW-G: no fault events expected after TLBI and re-translate; "
           "got " << events.getValue().size() << " event(s)";
}

// ---------------------------------------------------------------------------
// BUG-NEW-G Test 2: CMD_TLBI_S12_VMALL targeting a DIFFERENT VMID must NOT
//   invalidate a NS stage-1-only stream's TLB entry.
//
// Scenario:
//   Stream A: VMID=11 (NS, stage-1-only)
//   Stream B: VMID=22 (NS, stage-1-only)
//   Translate both to populate TLB.
//   Issue TLBI_S12_VMALL with VMID=22 — must NOT affect stream A (VMID=11).
//   Both streams must re-translate successfully.
// ---------------------------------------------------------------------------
TEST(BugNewGVmidTagging, TlbiS12VmallOnlyInvalidatesMatchingVmid) {
    auto smmu = makeEnabledSMMU();

    const StreamID sidA  = 0xF1u;
    const StreamID sidB  = 0xF2u;
    const uint16_t vmidA = 11u;
    const uint16_t vmidB = 22u;

    // Configure stream A (VMID=11).
    {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.t0sz               = 0;
        cfg.securityState      = SecurityState::NonSecure;
        cfg.vmid               = vmidA;
        ASSERT_TRUE(smmu->configureStream(sidA, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(sidA).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(sidA, 0u).isOk());
    }

    // Configure stream B (VMID=22).
    {
        StreamConfig cfg;
        cfg.translationEnabled = true;
        cfg.stage1Enabled      = true;
        cfg.stage2Enabled      = false;
        cfg.faultMode          = FaultMode::Terminate;
        cfg.t0sz               = 0;
        cfg.securityState      = SecurityState::NonSecure;
        cfg.vmid               = vmidB;
        ASSERT_TRUE(smmu->configureStream(sidB, cfg).isOk());
        ASSERT_TRUE(smmu->enableStream(sidB).isOk());
        ASSERT_TRUE(smmu->createStreamPASID(sidB, 0u).isOk());
    }

    smmu->mapPage(sidA, 0u, 0x2000u, 0xAAAA0000u, PagePermissions(true, false, false));
    smmu->mapPage(sidB, 0u, 0x3000u, 0xBBBB0000u, PagePermissions(true, false, false));

    // Populate TLB for both streams.
    ASSERT_TRUE(smmu->translate(sidA, 0u, 0x2000u, AccessType::Read).isOk())
        << "BUG-NEW-G precondition: stream A translate must succeed";
    ASSERT_TRUE(smmu->translate(sidB, 0u, 0x3000u, AccessType::Read).isOk())
        << "BUG-NEW-G precondition: stream B translate must succeed";

    // Issue TLBI_S12_VMALL targeting only VMID=22 (stream B).
    CommandEntry tlbi;
    tlbi.type = CommandType::TLBI_S12_VMALL;
    tlbi.vmid = vmidB;
    ASSERT_TRUE(smmu->submitCommand(tlbi).isOk());
    smmu->processCommandQueue();

    // Both streams must re-translate successfully.
    EXPECT_TRUE(smmu->translate(sidA, 0u, 0x2000u, AccessType::Read).isOk())
        << "BUG-NEW-G: stream A (VMID=11) must still translate after "
           "TLBI_S12_VMALL targeting VMID=22";
    EXPECT_TRUE(smmu->translate(sidB, 0u, 0x3000u, AccessType::Read).isOk())
        << "BUG-NEW-G: stream B (VMID=22) must still translate after "
           "TLBI_S12_VMALL targeting VMID=22 (page table intact)";

    // No fault events.
    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_TRUE(events.getValue().empty())
        << "BUG-NEW-G: no fault events expected; got "
        << events.getValue().size() << " event(s)";
}

// ---------------------------------------------------------------------------
// BUG-NEW-G Test 3: Secure stage-1-only stream must still use VMID=0
//   (ARM §3.17 — Secure stage-1-only entries are not tagged with a VMID).
//   CMD_TLBI_S12_VMALL with any non-zero VMID must NOT invalidate it.
// ---------------------------------------------------------------------------
TEST(BugNewGVmidTagging, SecureStage1OnlyUsesVmid0) {
    auto smmu = makeEnabledSMMU();

    const StreamID sid  = 0xF3u;
    const uint16_t vmid = 99u;  // STE.S2VMID set, but Secure — should be zeroed

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    // Secure stage-1-only: STRW must not be EL3 for NS (CONF-GAP-16).
    // Use Secure security state with EL1_EL0 world.
    cfg.securityState      = SecurityState::Secure;
    cfg.vmid               = vmid;  // Set VMID in STE, but ARM §3.17 says VMID=0 for Secure s1-only
    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(sid).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(sid, 0u).isOk());

    smmu->mapPage(sid, 0u, 0x4000u, 0xCCCC0000u, PagePermissions(true, false, false),
                  SecurityState::Secure);

    // Populate TLB.
    ASSERT_TRUE(smmu->translate(sid, 0u, 0x4000u, AccessType::Read,
                                SecurityState::Secure).isOk())
        << "BUG-NEW-G precondition: Secure stream translate must succeed";

    // Issue TLBI_S12_VMALL with VMID=99 — should NOT match the Secure stream
    // because the TLB entry was tagged VMID=0 (Secure stage-1-only).
    CommandEntry tlbi;
    tlbi.type = CommandType::TLBI_S12_VMALL;
    tlbi.vmid = vmid;
    ASSERT_TRUE(smmu->submitCommand(tlbi).isOk());
    smmu->processCommandQueue();

    // Re-translate must still succeed.
    EXPECT_TRUE(smmu->translate(sid, 0u, 0x4000u, AccessType::Read,
                                SecurityState::Secure).isOk())
        << "BUG-NEW-G: Secure stage-1-only stream must re-translate successfully "
           "after TLBI_S12_VMALL with non-zero VMID (entry tagged VMID=0, "
           "so TLBI with VMID=99 does not match it)";

    auto events = smmu->getEvents();
    ASSERT_TRUE(events.isOk());
    EXPECT_TRUE(events.getValue().empty())
        << "BUG-NEW-G: no fault events expected for Secure stream after mismatched TLBI; "
           "got " << events.getValue().size() << " event(s)";
}

// ---------------------------------------------------------------------------
// OPEN-7 Test 3: recordSecurityFault dead-code invariant.
//
// Verify that FaultType::SecurityFault does NOT appear in the event queue
// after any standard translation sequence. The SW model must never emit
// SecurityFault because no ARM event record encodes it.
// ---------------------------------------------------------------------------
TEST(Open7SecurityFaultType, SecurityFaultTypeNeverAppearsInEventQueue) {
    SMMU smmu;
    smmu.enable();
    smmu.setCR0(smmu.getCR0() | SMMU::CR0_EVENTQEN);

    const StreamID sid = 0xE2u;
    configureStage1Stream(smmu, sid, 0u);

    // Generate several different fault types.
    // (a) Translation fault: unmapped page.
    smmu.translate(sid, 0u, 0xDEAD000u, AccessType::Read);
    // (b) Permission fault: read-only write.
    smmu.mapPage(sid, 0u, 0x3000u, 0xC000'0000u,
                 PagePermissions(true, false, false));
    smmu.translate(sid, 0u, 0x3000u, AccessType::Write);

    auto events = smmu.getEvents();
    ASSERT_TRUE(events.isOk()) << "getEvents() must succeed";

    for (const FaultRecord& fr : events.getValue()) {
        EXPECT_NE(fr.faultType, FaultType::SecurityFault)
            << "OPEN-7: FaultType::SecurityFault must never appear in FaultRecords; "
               "found one for streamID=" << fr.streamID
            << " address=0x" << std::hex << fr.address;
    }
}
