// ARM SMMU v3 FINDING-NEW-09: SMMUEN global enable — disabled-by-default (C++)
//
// TDD spec tests for SMMU_CR0.SMMUEN (ARM IHI0070G.b §6.3.9).
//
// Spec: ARM IHI0070G.b §6.3.9 (SMMU_CR0.SMMUEN),
//       §3.11 (SMMU_GBPA — global bypass/abort control),
//       §13.2 (software reset requirements)
//
// Requirements:
// - SMMU must start DISABLED (SMMUEN=0) after default construction.
// - SMMU must start DISABLED (SMMUEN=0) after custom-config construction.
// - When disabled with GBPA.ABORT=0: all transactions bypass (PA == IOVA),
//   full permissions, no fault events.
// - When disabled with GBPA.ABORT=1: all transactions abort with
//   SMMUError::GbpaAbort regardless of stream configuration.
// - enable() sets SMMUEN=1 — translations go through the normal path.
// - disable() sets SMMUEN=0 — translations bypass/abort again.
// - reset() returns SMMUEN=0 and GBPA.ABORT=0.
// - isEnabled() accurately reflects the current SMMUEN bit.
// - isGbpaAbort() accurately reflects the current GBPA.ABORT bit.
// - Bypass returns identity PA (PA == IOVA) with read+write+execute permissions.
// - With SMMU enabled and an unconfigured stream, a fault event is generated.
//   With SMMU disabled, no fault events are generated (bypass).

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ── Constants ──────────────────────────────────────────────────────────────

static constexpr StreamID SMMUEN_STREAM_A  = 0xA0;
static constexpr StreamID SMMUEN_STREAM_B  = 0xB0;
static constexpr PASID    SMMUEN_PASID_0   = 0;
static constexpr PASID    SMMUEN_PASID_1   = 1;
static constexpr IOVA     SMMUEN_IOVA      = 0x0001'0000ULL;
static constexpr PA       SMMUEN_PA        = 0x4000'0000ULL;

// ── Helpers ────────────────────────────────────────────────────────────────

// Configure a Stage-1 stream with PASID and a page mapping, then enable it.
static void configureStreamWithMapping(SMMU* smmu, StreamID sid, PASID pasid,
                                       IOVA iova, PA pa) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;

    ASSERT_TRUE(smmu->configureStream(sid, cfg).isOk())
        << "configureStream failed for sid " << sid;
    ASSERT_TRUE(smmu->enableStream(sid).isOk())
        << "enableStream failed for sid " << sid;
    ASSERT_TRUE(smmu->createStreamPASID(sid, pasid).isOk())
        << "createStreamPASID failed for pasid " << pasid;

    PagePermissions perms;
    perms.read    = true;
    perms.write   = true;
    perms.execute = false;

    ASSERT_TRUE(smmu->mapPage(sid, pasid, iova, pa, perms).isOk())
        << "mapPage failed";
}

// ── §6.3.9: Post-construction state ────────────────────────────────────────

/// §6.3.9: Default constructor — SMMU must start disabled (SMMUEN=0).
TEST(SMMUenSpec, DefaultConstructionStartsDisabled) {
    auto smmu = std::make_unique<SMMU>();

    EXPECT_FALSE(smmu->isEnabled())
        << "SMMU must start with SMMUEN=0 after default construction (ARM §6.3.9)";
}

/// §6.3.9: Custom-config constructor — SMMU must start disabled (SMMUEN=0).
TEST(SMMUenSpec, CustomConfigConstructionStartsDisabled) {
    SMMUConfiguration cfg;
    CacheConfiguration ccfg;
    ccfg.enableCaching  = true;
    ccfg.tlbCacheSize   = 512;
    cfg.setCacheConfiguration(ccfg);

    auto smmu = std::make_unique<SMMU>(cfg);

    EXPECT_FALSE(smmu->isEnabled())
        << "SMMU must start with SMMUEN=0 after custom-config construction (ARM §6.3.9)";
}

// ── §6.3.9: Bypass behaviour when disabled (GBPA.ABORT=0) ─────────────────

/// §6.3.9 / §3.11: When disabled and GBPA.ABORT=0, translate() returns bypass
/// (PA == IOVA), not an error.
TEST(SMMUenSpec, DisabledSMMU_BypassReturnSuccess) {
    auto smmu = std::make_unique<SMMU>();
    // SMMU is disabled by default; GBPA.ABORT=0 by default.

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "Disabled SMMU with GBPA.ABORT=0 must bypass (not error)";
}

/// §3.11: Bypass maps PA == IOVA (identity mapping).
TEST(SMMUenSpec, DisabledSMMU_BypassIdentityMapping) {
    auto smmu = std::make_unique<SMMU>();
    // SMMU disabled, GBPA.ABORT=0.

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isOk());
    EXPECT_EQ(result.getValue().physicalAddress,
              static_cast<PA>(SMMUEN_IOVA))
        << "Bypass PA must equal IOVA (identity mapping, ARM §3.11)";
}

/// §3.11: Bypass grants full read+write+execute permissions.
TEST(SMMUenSpec, DisabledSMMU_BypassFullPermissions) {
    auto smmu = std::make_unique<SMMU>();

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Write, SecurityState::NonSecure);

    ASSERT_TRUE(result.isOk());
    const PagePermissions& perms = result.getValue().permissions;
    EXPECT_TRUE(perms.read)    << "Bypass must grant read permission";
    EXPECT_TRUE(perms.write)   << "Bypass must grant write permission";
    EXPECT_TRUE(perms.execute) << "Bypass must grant execute permission";
}

// ── §6.3.9 / §3.11: GBPA.ABORT=1 — abort instead of bypass ───────────────

/// §3.11: When disabled and GBPA.ABORT=1, translate() must return GbpaAbort.
TEST(SMMUenSpec, DisabledSMMU_GbpaAbortReturnsError) {
    auto smmu = std::make_unique<SMMU>();
    smmu->setGbpaAbort(true);

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "Disabled SMMU with GBPA.ABORT=1 must abort";
    EXPECT_EQ(result.getError(), SMMUError::GbpaAbort)
        << "Abort error must be SMMUError::GbpaAbort (ARM §3.11)";
}

/// §3.11: When disabled and GBPA.ABORT=0 (explicit), translate() must bypass.
TEST(SMMUenSpec, DisabledSMMU_GbpaAbortFalse_Bypass) {
    auto smmu = std::make_unique<SMMU>();
    smmu->setGbpaAbort(false);  // explicit no-abort

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "Disabled SMMU with explicit GBPA.ABORT=0 must bypass";
}

// ── §6.3.9: enable() / disable() transitions ──────────────────────────────

/// §6.3.9: isEnabled() must return false initially, true after enable().
TEST(SMMUenSpec, IsEnabledReflectsState) {
    auto smmu = std::make_unique<SMMU>();

    EXPECT_FALSE(smmu->isEnabled()) << "initially false";

    smmu->enable();
    EXPECT_TRUE(smmu->isEnabled())  << "true after enable()";

    smmu->disable();
    EXPECT_FALSE(smmu->isEnabled()) << "false after disable()";
}

/// §6.3.9: After enable(), normal translation path is taken — unconfigured
/// stream returns StreamNotConfigured, not bypass.
TEST(SMMUenSpec, EnabledSMMU_UnconfiguredStream_ReturnsStreamNotConfigured) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isError())
        << "Enabled SMMU with unconfigured stream must return error";
    // BUG-CPP-DBGR-12 fix: §7.3.3 — the streamMap miss path now returns
    // SMMUError::InvalidStreamID (matching C_BAD_STREAMID) instead of StreamNotConfigured.
    EXPECT_EQ(result.getError(), SMMUError::InvalidStreamID)
        << "Error must be InvalidStreamID (§7.3.3 C_BAD_STREAMID) when SMMU enabled and stream unknown";
}

/// §6.3.9: After enable() + stream configured + page mapped, normal translation
/// succeeds and returns the correct PA.
TEST(SMMUenSpec, EnabledSMMU_ConfiguredStream_ReturnsCorrectPA) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    configureStreamWithMapping(smmu.get(),
        SMMUEN_STREAM_A, SMMUEN_PASID_1, SMMUEN_IOVA, SMMUEN_PA);

    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_1, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    ASSERT_TRUE(result.isOk()) << "Enabled SMMU with mapped page must succeed";
    EXPECT_EQ(result.getValue().physicalAddress, SMMUEN_PA)
        << "Translated PA must equal the mapped PA";
}

/// §6.3.9: disable() returns SMMU to bypass — even a mapped stream bypasses.
TEST(SMMUenSpec, AfterDisable_MappedStream_Bypasses) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    configureStreamWithMapping(smmu.get(),
        SMMUEN_STREAM_A, SMMUEN_PASID_1, SMMUEN_IOVA, SMMUEN_PA);

    // Verify normal translation works while enabled.
    TranslationResult r1 = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_1, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r1.isOk());
    EXPECT_EQ(r1.getValue().physicalAddress, SMMUEN_PA);

    // Disable the SMMU globally.
    smmu->disable();

    // Same stream — now bypasses (PA == IOVA).
    TranslationResult r2 = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_1, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);
    ASSERT_TRUE(r2.isOk()) << "Disabled SMMU must bypass";
    EXPECT_EQ(r2.getValue().physicalAddress, static_cast<PA>(SMMUEN_IOVA))
        << "Bypass PA must equal IOVA after disable()";
}

/// §6.3.9: Toggle enable/disable multiple times — bypass resumes each disable().
TEST(SMMUenSpec, MultipleToggle_EnableDisable_ConsistentBehaviour) {
    auto smmu = std::make_unique<SMMU>();

    for (int cycle = 0; cycle < 3; ++cycle) {
        // Disabled state: bypass expected.
        {
            TranslationResult r = smmu->translate(
                SMMUEN_STREAM_B, SMMUEN_PASID_0, SMMUEN_IOVA,
                AccessType::Read, SecurityState::NonSecure);
            EXPECT_TRUE(r.isOk())  << "cycle " << cycle << ": disabled must bypass";
            if (r.isOk()) {
                EXPECT_EQ(r.getValue().physicalAddress, static_cast<PA>(SMMUEN_IOVA))
                    << "cycle " << cycle << ": bypass PA must equal IOVA";
            }
        }

        // Enable — unconfigured stream must now error.
        smmu->enable();
        {
            TranslationResult r = smmu->translate(
                SMMUEN_STREAM_B, SMMUEN_PASID_0, SMMUEN_IOVA,
                AccessType::Read, SecurityState::NonSecure);
            EXPECT_TRUE(r.isError())
                << "cycle " << cycle << ": enabled with no stream must error";
        }

        // Disable again for next cycle.
        smmu->disable();
    }
}

// ── §13.2: reset() returns SMMU to disabled state ─────────────────────────

/// §13.2 / §6.3.9: reset() must set SMMUEN=0.
TEST(SMMUenSpec, ResetReturnsSMMUToDisabledState) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    ASSERT_TRUE(smmu->isEnabled());

    smmu->reset();

    EXPECT_FALSE(smmu->isEnabled())
        << "reset() must clear SMMUEN to 0 (ARM §13.2)";
}

/// §13.2 / §3.11: reset() must also clear GBPA.ABORT to 0 (bypass, not abort).
TEST(SMMUenSpec, ResetClearsGbpaAbort) {
    auto smmu = std::make_unique<SMMU>();
    smmu->setGbpaAbort(true);
    ASSERT_TRUE(smmu->isGbpaAbort());

    smmu->reset();

    EXPECT_FALSE(smmu->isGbpaAbort())
        << "reset() must clear GBPA.ABORT to 0 (ARM §13.2 / §3.11)";
}

/// §13.2: After reset(), translate() bypasses (not aborts), confirming
/// both SMMUEN=0 and GBPA.ABORT=0 were restored.
TEST(SMMUenSpec, AfterReset_BypassNotAbort) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    smmu->setGbpaAbort(true);

    smmu->reset();

    // With SMMUEN=0 and GBPA.ABORT=0 post-reset, bypass expected.
    TranslationResult result = smmu->translate(
        SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
        AccessType::Read, SecurityState::NonSecure);

    EXPECT_TRUE(result.isOk())
        << "Post-reset translate() must bypass (SMMUEN=0, GBPA.ABORT=0)";
    if (result.isOk()) {
        EXPECT_EQ(result.getValue().physicalAddress, static_cast<PA>(SMMUEN_IOVA))
            << "Post-reset bypass PA must equal IOVA";
    }
}

// ── §6.3.9: isGbpaAbort() accuracy ────────────────────────────────────────

/// §3.11: isGbpaAbort() must accurately reflect GBPA.ABORT state.
TEST(SMMUenSpec, IsGbpaAbortReflectsState) {
    auto smmu = std::make_unique<SMMU>();

    EXPECT_FALSE(smmu->isGbpaAbort()) << "GBPA.ABORT must be 0 initially";

    smmu->setGbpaAbort(true);
    EXPECT_TRUE(smmu->isGbpaAbort())  << "isGbpaAbort() must be true after setGbpaAbort(true)";

    smmu->setGbpaAbort(false);
    EXPECT_FALSE(smmu->isGbpaAbort()) << "isGbpaAbort() must be false after setGbpaAbort(false)";
}

// ── FINDING-NEW-15: STE.Config==0b000 disabled stream generates no event ──

/// §7.3.7 / §5.2: When STE.Config==0b000 (stream disabled), incoming traffic
/// is terminated WITHOUT recording an event to the event queue.
TEST(SMMUenSpec, StreamDisabled_TranslationSilentlyAborts_NoEvent) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    // Configure a stage-1 stream then immediately disable it — simulates
    // STE.Config==0b000 (administratively disabled).
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu->configureStream(SMMUEN_STREAM_A, cfg).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(SMMUEN_STREAM_A, SMMUEN_PASID_0).isOk());
    // Do NOT call enableStream — leaves the stream disabled (STE.Config==0b000).

    auto result = smmu->translate(SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
                                   AccessType::Read, SecurityState::NonSecure);

    // Translation must fail (stream is disabled)
    EXPECT_TRUE(result.isError())
        << "Disabled stream must not produce a successful translation";

    // §7.3.7: NO event must be enqueued for STE.Config==0b000.
    auto events = smmu->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "STE.Config==0b000 (disabled stream) must not record an event (ARM §7.3.7)";
}

/// §7.3.7 contrast: Enabled stream DOES record a FaultRecord on fault —
/// confirms the disabled-stream path (not the general fault path) is silent.
TEST(SMMUenSpec, StreamEnabled_TranslationFault_RecordsFaultRecord) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    ASSERT_TRUE(smmu->configureStream(SMMUEN_STREAM_A, cfg).isOk());
    ASSERT_TRUE(smmu->enableStream(SMMUEN_STREAM_A).isOk());
    ASSERT_TRUE(smmu->createStreamPASID(SMMUEN_STREAM_A, SMMUEN_PASID_0).isOk());
    // No page mapped — causes a translation fault with the stream ENABLED.

    smmu->translate(SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    // For an enabled stream fault, getEvents() (FaultRecords) is populated;
    // this contrasts with the disabled-stream case which is fully silent.
    auto faults = smmu->getEvents();
    EXPECT_FALSE(faults.isError())
        << "getEvents() must not error";
    if (faults.isOk()) {
        EXPECT_FALSE(faults.getValue().empty())
            << "Enabled stream fault must record a FaultRecord (ARM §7.3.7 contrast)";
    }
}

// ── §6.3.9: bypass generates no fault events ──────────────────────────────

/// §6.3.9: When SMMU is disabled (bypass), no fault events are generated.
TEST(SMMUenSpec, Disabled_BypassGeneratesNoFaultEvents) {
    auto smmu = std::make_unique<SMMU>();
    // Disabled by default, GBPA.ABORT=0 => bypass.

    smmu->translate(SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    EXPECT_TRUE(events.empty())
        << "Bypass mode must not generate any fault events (ARM §6.3.9)";
}

/// §6.3.9: When SMMU is enabled and stream is unconfigured, a fault event
/// is generated — confirming bypass suppresses events while enabled path does not.
TEST(SMMUenSpec, Enabled_UnknownStream_GeneratesFaultEvent) {
    auto smmu = std::make_unique<SMMU>();
    smmu->enable();
    // §6.3.12 CR2.RECINVSID=1: enable C_BAD_STREAMID event recording.
    smmu->setCR2(SMMU::CR2_RECINVSID);

    smmu->translate(SMMUEN_STREAM_A, SMMUEN_PASID_0, SMMUEN_IOVA,
                    AccessType::Read, SecurityState::NonSecure);

    auto events = smmu->getEventQueue();
    EXPECT_FALSE(events.empty())
        << "Enabled SMMU with unconfigured stream must generate a fault event";
}

}  // namespace test
}  // namespace smmu
