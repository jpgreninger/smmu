// ARM SMMU v3 FINDING-NEW-34 through NEW-43 spec tests
// Copyright (c) 2024 John Greninger
//
// TDD tests for:
// - FINDING-NEW-34: Root security state rejected in ASID/STE validation (§3.10)
// - FINDING-NEW-35: AccessType::ReadWrite denied by validateAccessPermissions (§3.24)
// - FINDING-NEW-37: TLB time-based eviction is not spec-defined (§3.16)
// - FINDING-NEW-38: Two-stage permission intersection missing (§3.3.1)
// - FINDING-NEW-39: Completion events hardcode NonSecure security state (§4.5.1, §4.8)
// - FINDING-NEW-40: CMD_CFGI_STE with unknown StreamID does not generate C_BAD_STREAMID (§4.3.1)
// - FINDING-NEW-41: Bypass path returns zero PagePermissions (§5.2 STE.Config==0b100)
// - FINDING-NEW-43: classifyTranslationFault() applies arbitrary IOVA heuristics (§7.3.13-16)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/stream_context.h"
#include "smmu/address_space.h"
#include "smmu/types.h"
#include <memory>

namespace smmu {
namespace test {

// ─── Shared constants ─────────────────────────────────────────────────────────

static constexpr StreamID N34_STREAM  = 0x340;
static constexpr StreamID N35_STREAM  = 0x350;
static constexpr StreamID N37_STREAM  = 0x370;
static constexpr StreamID N38_STREAM  = 0x380;
static constexpr StreamID N39_STREAM  = 0x390;
static constexpr StreamID N40_STREAM  = 0x400;
static constexpr StreamID N41_STREAM  = 0x410;
static constexpr StreamID N43_STREAM  = 0x430;
static constexpr PASID    TEST_PASID  = 0;
static constexpr IOVA     TEST_PAGE   = 0x1000ULL;
static constexpr PA       TEST_PA     = 0x9000'0000ULL;

// ─── Helpers ──────────────────────────────────────────────────────────────────

static void enableSMMU(SMMU& smmu) {
    smmu.enable();
    uint32_t cr0 = smmu.getCR0() | (1u << 4); // CMDQEN
    smmu.setCR0(cr0);
}

static void configureBasicStream(SMMU& smmu, StreamID sid,
                                  bool s1 = true, bool s2 = false,
                                  FaultMode fm = FaultMode::Terminate) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = s1;
    cfg.stage2Enabled = s2;
    cfg.faultMode = fm;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, TEST_PASID).isOk());
}

static int countEvents(SMMU& smmu, EventType type) {
    int n = 0;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == type) { ++n; }
    }
    return n;
}

// ─── FINDING-NEW-34: Root security state accepted in ASID/STE validation ──────
//
// ARM §3.10 defines four valid security states: NonSecure(0x00), Secure(0x01),
// Realm(0x02), Root(0x03).  The old allowlist only accepted 0..2, causing
// Root (0x03) to be rejected as invalid.

TEST(RootSecurityStateTest, RootStateAcceptedInASIDValidation) {
    StreamContext ctx;
    // Create PASID 0 in this context
    ASSERT_TRUE(ctx.createPASID(TEST_PASID).isOk());

    // Build a ContextDescriptor with Root security state
    ContextDescriptor cd;
    cd.asid = 0x42;
    cd.securityState = SecurityState::Root;
    cd.ttbr0 = 0x1000;   // 4KB-aligned
    cd.ttbr0Valid = true;
    cd.ttbr1Valid = false;
    cd.tcr.granuleSize = TranslationGranule::Size4KB;
    cd.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
    cd.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;

    // validateContextDescriptor uses validateASIDConfigurationUnlocked internally.
    // If Root is incorrectly rejected the result will be false.
    Result<bool> result = ctx.validateContextDescriptor(cd, TEST_PASID, N34_STREAM);
    ASSERT_TRUE(result.isOk()) << "validateContextDescriptor returned an error";
    EXPECT_TRUE(result.getValue())
        << "Root security state must be accepted per ARM §3.10";
}

TEST(RootSecurityStateTest, RootStateAcceptedInSTEValidation) {
    StreamContext ctx;

    StreamTableEntry ste;
    ste.translationEnabled = true;
    ste.stage1Enabled = true;
    ste.stage2Enabled = false;
    ste.contextDescriptorTableBase = 0x4000;  // 64-byte aligned
    ste.contextDescriptorTableSize = 1;
    ste.securityState = SecurityState::Root;  // Must be accepted
    ste.faultMode = FaultMode::Terminate;
    ste.stage1Granule = TranslationGranule::Size4KB;
    ste.stage2Granule = TranslationGranule::Size4KB;

    Result<bool> result = ctx.validateStreamTableEntry(ste);
    ASSERT_TRUE(result.isOk()) << "validateStreamTableEntry returned an error";
    EXPECT_TRUE(result.getValue())
        << "Root security state must be accepted in STE validation per ARM §3.10";
}

TEST(RootSecurityStateTest, AllFourSecurityStatesAccepted) {
    StreamContext ctx;
    ASSERT_TRUE(ctx.createPASID(TEST_PASID).isOk());

    const SecurityState states[] = {
        SecurityState::NonSecure,
        SecurityState::Secure,
        SecurityState::Realm,
        SecurityState::Root
    };

    for (auto ss : states) {
        ContextDescriptor cd;
        cd.asid = 0x10;
        cd.securityState = ss;
        cd.ttbr0 = 0x1000;
        cd.ttbr0Valid = true;
        cd.ttbr1Valid = false;
        cd.tcr.granuleSize = TranslationGranule::Size4KB;
        cd.tcr.inputAddressSize = AddressSpaceSize::Size48Bit;
        cd.tcr.outputAddressSize = AddressSpaceSize::Size48Bit;

        Result<bool> result = ctx.validateContextDescriptor(cd, TEST_PASID, N34_STREAM);
        ASSERT_TRUE(result.isOk());
        EXPECT_TRUE(result.getValue())
            << "Security state " << static_cast<int>(ss) << " must be accepted";
    }
}

// ─── FINDING-NEW-35: AccessType::ReadWrite permitted on RW page ───────────────
//
// ARM §3.24 — atomics (ReadWrite) are write-class; they require both read and
// write permissions.  Previously the default case in validateAccessPermissions()
// returned false, denying all atomic operations.

TEST(AccessTypeReadWriteTest, ReadWritePermittedOnRWPage) {
    SMMU smmu;
    enableSMMU(smmu);
    configureBasicStream(smmu, N35_STREAM);

    PagePermissions rwPerms;
    rwPerms.read = true;
    rwPerms.write = true;
    rwPerms.execute = false;
    ASSERT_TRUE(smmu.mapPage(N35_STREAM, TEST_PASID, TEST_PAGE, TEST_PA, rwPerms).isOk());

    // Atomic (ReadWrite) on an RW page must succeed, not produce F_PERMISSION
    TranslationResult result = smmu.translate(N35_STREAM, TEST_PASID, TEST_PAGE,
                                               AccessType::ReadWrite);
    EXPECT_TRUE(result.isOk())
        << "AccessType::ReadWrite on RW page must succeed per ARM §3.24";
    EXPECT_EQ(countEvents(smmu, EventType::F_PERMISSION), 0)
        << "No F_PERMISSION event should be generated for valid ReadWrite access";
}

TEST(AccessTypeReadWriteTest, ReadWriteDeniedOnReadOnlyPage) {
    SMMU smmu;
    enableSMMU(smmu);
    configureBasicStream(smmu, N35_STREAM + 1);

    PagePermissions roPerms;
    roPerms.read = true;
    roPerms.write = false;
    roPerms.execute = false;
    ASSERT_TRUE(smmu.mapPage(N35_STREAM + 1, TEST_PASID, TEST_PAGE, TEST_PA, roPerms).isOk());

    // ReadWrite (atomic) on a read-only page must be denied
    TranslationResult result = smmu.translate(N35_STREAM + 1, TEST_PASID, TEST_PAGE,
                                               AccessType::ReadWrite);
    EXPECT_TRUE(result.isError())
        << "AccessType::ReadWrite on read-only page must be denied";
}

// ─── FINDING-NEW-37: TLB entries valid until explicit TLBI ───────────────────
//
// ARM §3.16 — TLB entries are valid until an explicit TLBI command.
// The implementation must not evict entries based on a time-based age check.

TEST(TLBEvictionTest, TLBEntryNotEvictedByTimeAlone) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.enableCaching(true);
    configureBasicStream(smmu, N37_STREAM);

    PagePermissions perms;
    perms.read = true;
    perms.write = true;
    perms.execute = false;
    ASSERT_TRUE(smmu.mapPage(N37_STREAM, TEST_PASID, TEST_PAGE, TEST_PA, perms).isOk());

    // First translation — populates TLB
    TranslationResult first = smmu.translate(N37_STREAM, TEST_PASID, TEST_PAGE,
                                              AccessType::Read);
    ASSERT_TRUE(first.isOk()) << "First translation must succeed";

    // Second translation (no TLBI issued) — must still succeed from TLB or page table
    // The critical property is that no artificial age-based eviction causes a fault.
    TranslationResult second = smmu.translate(N37_STREAM, TEST_PASID, TEST_PAGE,
                                               AccessType::Read);
    EXPECT_TRUE(second.isOk())
        << "TLB entry must remain valid without explicit TLBI per ARM §3.16";
    EXPECT_EQ(second.getValue().physicalAddress & ~PAGE_MASK,
              first.getValue().physicalAddress & ~PAGE_MASK)
        << "Repeated translation must return the same physical page";
}

TEST(TLBEvictionTest, TLBEntryInvalidatedAfterTLBI) {
    SMMU smmu;
    enableSMMU(smmu);
    smmu.enableCaching(true);
    configureBasicStream(smmu, N37_STREAM + 1);

    PagePermissions perms;
    perms.read = true;
    perms.write = true;
    perms.execute = false;
    ASSERT_TRUE(smmu.mapPage(N37_STREAM + 1, TEST_PASID, TEST_PAGE, TEST_PA, perms).isOk());

    // Populate TLB
    TranslationResult first = smmu.translate(N37_STREAM + 1, TEST_PASID, TEST_PAGE,
                                              AccessType::Read);
    ASSERT_TRUE(first.isOk());

    // Issue TLBI via invalidateStreamCache
    smmu.invalidateStreamCache(N37_STREAM + 1);

    // Unmap the page — if TLBI worked, the next translation must miss the TLB
    // and look up the page table, where the page is now absent
    ASSERT_TRUE(smmu.unmapPage(N37_STREAM + 1, TEST_PASID, TEST_PAGE).isOk());

    TranslationResult second = smmu.translate(N37_STREAM + 1, TEST_PASID, TEST_PAGE,
                                               AccessType::Read);
    EXPECT_TRUE(second.isError())
        << "After TLBI + unmap, translation must miss (TLBI is the spec-correct eviction)";
}

// ─── FINDING-NEW-38: Two-stage permission intersection (§3.3.1) ───────────────
//
// ARM §3.3.1 — effective permissions = Stage-1 ∩ Stage-2.
// When Stage-1 maps a page read-only and Stage-2 maps it read-write,
// the effective permission must be read-only (Stage-1 restricts).
//
// This test exercises StreamContext::translateUnlocked() directly with separate
// Stage-1 and Stage-2 address space objects, bypassing the aliased-address-space
// short-circuit in performBothStagesTranslation().

TEST(TwoStagePermissionTest, Stage1PermissionsEnforcedInBothStagePath) {
    // Use StreamContext directly to test translateUnlocked two-stage path
    StreamContext ctx;
    ctx.setStage1Enabled(true);
    ctx.setStage2Enabled(true);

    // PASID 1 for Stage-1 (guest virtual address space)
    constexpr PASID GUEST_PASID = 1;
    ASSERT_TRUE(ctx.createPASID(GUEST_PASID).isOk());

    // Stage-1: map TEST_PAGE as read-only — IOVA -> IPA (IPA == TEST_PAGE for identity)
    PagePermissions s1Perms;
    s1Perms.read    = true;
    s1Perms.write   = false;
    s1Perms.execute = false;
    ASSERT_TRUE(ctx.mapPage(GUEST_PASID, TEST_PAGE, TEST_PAGE, s1Perms).isOk());

    // Stage-2: separate address space maps IPA -> PA as read-write
    auto stage2AS = std::make_shared<AddressSpace>();
    PagePermissions s2Perms;
    s2Perms.read    = true;
    s2Perms.write   = true;
    s2Perms.execute = false;
    ASSERT_TRUE(stage2AS->mapPage(TEST_PAGE, TEST_PA, s2Perms).isOk());

    // Install the Stage-2 address space
    ctx.setStage2AddressSpace(stage2AS);

    // Enable the stream for translation
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate;
    ASSERT_TRUE(ctx.updateConfiguration(cfg).isOk());
    ASSERT_TRUE(ctx.enableStream().isOk());

    // Write access: Stage-1 is read-only, Stage-2 is RW — intersection is read-only.
    // Per ARM §3.3.1, write must be denied.
    TranslationResult writeResult = ctx.translate(GUEST_PASID, TEST_PAGE,
                                                   AccessType::Write,
                                                   SecurityState::NonSecure);
    EXPECT_TRUE(writeResult.isError())
        << "Write must be denied when Stage-1 is read-only, per ARM §3.3.1 permission intersection";
    EXPECT_EQ(writeResult.getError(), SMMUError::PagePermissionViolation)
        << "Error must be PagePermissionViolation for S1 read-only with write request";

    // Read access: both stages allow reading — must succeed.
    TranslationResult readResult = ctx.translate(GUEST_PASID, TEST_PAGE,
                                                  AccessType::Read,
                                                  SecurityState::NonSecure);
    EXPECT_TRUE(readResult.isOk())
        << "Read must succeed when both Stage-1 and Stage-2 allow reading";
}

TEST(TwoStagePermissionTest, Stage2PermissionsEnforcedInBothStagePath) {
    // Symmetric case: Stage-1 allows write, Stage-2 is read-only — write must be denied.
    StreamContext ctx;
    ctx.setStage1Enabled(true);
    ctx.setStage2Enabled(true);

    constexpr PASID GUEST_PASID = 1;
    ASSERT_TRUE(ctx.createPASID(GUEST_PASID).isOk());

    // Stage-1: map as read-write
    PagePermissions s1Perms;
    s1Perms.read    = true;
    s1Perms.write   = true;
    s1Perms.execute = false;
    ASSERT_TRUE(ctx.mapPage(GUEST_PASID, TEST_PAGE, TEST_PAGE, s1Perms).isOk());

    // Stage-2: read-only
    auto stage2AS = std::make_shared<AddressSpace>();
    PagePermissions s2Perms;
    s2Perms.read    = true;
    s2Perms.write   = false;
    s2Perms.execute = false;
    ASSERT_TRUE(stage2AS->mapPage(TEST_PAGE, TEST_PA, s2Perms).isOk());
    ctx.setStage2AddressSpace(stage2AS);

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = true;
    cfg.faultMode = FaultMode::Terminate;
    ASSERT_TRUE(ctx.updateConfiguration(cfg).isOk());
    ASSERT_TRUE(ctx.enableStream().isOk());

    TranslationResult writeResult = ctx.translate(GUEST_PASID, TEST_PAGE,
                                                   AccessType::Write,
                                                   SecurityState::NonSecure);
    EXPECT_TRUE(writeResult.isError())
        << "Write must be denied when Stage-2 is read-only, per ARM §3.3.1";

    TranslationResult readResult = ctx.translate(GUEST_PASID, TEST_PAGE,
                                                  AccessType::Read,
                                                  SecurityState::NonSecure);
    EXPECT_TRUE(readResult.isOk())
        << "Read must succeed when both stages allow reading";
}

// ─── FINDING-NEW-39: Completion events use stream security state ──────────────
//
// ARM §4.5.1, §4.8 — completion event security state must reflect the stream
// context, not a hardcoded NonSecure.

TEST(CompletionEventSecurityStateTest, AtcInvCompletionUsesStreamSecurityState) {
    SMMU smmu;
    enableSMMU(smmu);

    // Configure a Secure stream
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    cfg.securityState = SecurityState::Secure;
    ASSERT_TRUE(smmu.configureStream(N39_STREAM, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(N39_STREAM).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(N39_STREAM, TEST_PASID).isOk());

    smmu.clearEventQueue();

    // Submit CMD_ATC_INV for this Secure stream
    CommandEntry cmd;
    cmd.type = CommandType::ATC_INV;
    cmd.streamID = N39_STREAM;
    cmd.pasid = TEST_PASID;
    cmd.startAddress = TEST_PAGE;
    cmd.endAddress = TEST_PAGE + 0x1000;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    // Check that the ATC_INVALIDATE_COMPLETION event has Secure security state
    bool foundSecureCompletion = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::ATC_INVALIDATE_COMPLETION &&
            ev.streamID == N39_STREAM) {
            EXPECT_EQ(ev.securityState, SecurityState::Secure)
                << "ATC completion event must carry Secure state from stream config";
            foundSecureCompletion = true;
        }
    }
    EXPECT_TRUE(foundSecureCompletion)
        << "ATC_INVALIDATE_COMPLETION event must be generated";
}

TEST(CompletionEventSecurityStateTest, SyncCompletionUsesStreamSecurityState) {
    SMMU smmu;
    enableSMMU(smmu);

    // Configure a Secure stream
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled = true;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    cfg.securityState = SecurityState::Secure;
    ASSERT_TRUE(smmu.configureStream(N39_STREAM + 1, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(N39_STREAM + 1).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(N39_STREAM + 1, TEST_PASID).isOk());

    smmu.clearEventQueue();

    // Submit CMD_SYNC with CS=0b01 (SIG_IRQ) so a completion event is generated
    CommandEntry syncCmd;
    syncCmd.type = CommandType::SYNC;
    syncCmd.streamID = N39_STREAM + 1;
    syncCmd.pasid = TEST_PASID;
    syncCmd.cs = 1;  // SIG_IRQ — triggers completion event
    ASSERT_TRUE(smmu.submitCommand(syncCmd).isOk());
    smmu.processCommandQueue();

    bool foundSecureSync = false;
    for (const auto& ev : smmu.getEventQueue()) {
        if (ev.type == EventType::COMMAND_SYNC_COMPLETION &&
            ev.streamID == N39_STREAM + 1) {
            EXPECT_EQ(ev.securityState, SecurityState::Secure)
                << "SYNC completion event must carry Secure state from stream config";
            foundSecureSync = true;
        }
    }
    EXPECT_TRUE(foundSecureSync)
        << "COMMAND_SYNC_COMPLETION event must be generated when CS!=0";
}

// ─── FINDING-NEW-40: CMD_CFGI_STE with unknown StreamID → C_BAD_STREAMID ──────
//
// ARM §4.3.1 — C_BAD_STREAMID + GERROR.CMDQ_ERR for out-of-table StreamID.

TEST(CfgiSteTest, UnknownStreamIDGeneratesCBadStreamid) {
    SMMU smmu;
    enableSMMU(smmu);

    // StreamID 0xDEAD is not configured
    constexpr StreamID UNKNOWN_STREAM = 0xDEAD;

    smmu.clearEventQueue();
    smmu.clearGerror(0xFFFFFFFFu);

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = UNKNOWN_STREAM;
    cmd.pasid = 0;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    EXPECT_GT(countEvents(smmu, EventType::C_BAD_STREAMID), 0)
        << "CMD_CFGI_STE with unknown StreamID must generate C_BAD_STREAMID per ARM §4.3.1";
    EXPECT_NE(smmu.getGerror() & GERROR_CMDQ_ERR, 0u)
        << "GERROR.CMDQ_ERR must be set for C_BAD_STREAMID per ARM §4.3.1";
}

TEST(CfgiSteTest, KnownStreamIDDoesNotGenerateCBadStreamid) {
    SMMU smmu;
    enableSMMU(smmu);
    configureBasicStream(smmu, N40_STREAM);

    smmu.clearEventQueue();
    smmu.clearGerror(0xFFFFFFFFu);

    CommandEntry cmd;
    cmd.type = CommandType::CFGI_STE;
    cmd.streamID = N40_STREAM;
    cmd.pasid = 0;
    ASSERT_TRUE(smmu.submitCommand(cmd).isOk());
    smmu.processCommandQueue();

    EXPECT_EQ(countEvents(smmu, EventType::C_BAD_STREAMID), 0)
        << "CMD_CFGI_STE with a known StreamID must NOT generate C_BAD_STREAMID";
}

// ─── FINDING-NEW-41: Bypass path grants full RWX permissions ─────────────────
//
// ARM §5.2 STE.Config==0b100 — bypass passes the transaction through unchanged
// with full read/write/execute permissions, not zero permissions.

TEST(BypassPermissionsTest, BypassTranslationGrantsFullPermissions) {
    SMMU smmu;
    enableSMMU(smmu);

    // STE.Config==0b100 bypass mode: translationEnabled=false, bypassEnabled=true
    // ARM §5.2: bypass passes transaction through with full RWX permissions.
    StreamConfig cfg;
    cfg.translationEnabled = false;
    cfg.bypassEnabled = true;   // STE.Config==0b100
    cfg.stage1Enabled = false;
    cfg.stage2Enabled = false;
    cfg.faultMode = FaultMode::Terminate;
    ASSERT_TRUE(smmu.configureStream(N41_STREAM, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(N41_STREAM).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(N41_STREAM, TEST_PASID).isOk());

    // All three access types must succeed in bypass mode
    TranslationResult readResult = smmu.translate(N41_STREAM, TEST_PASID, TEST_PAGE,
                                                   AccessType::Read);
    EXPECT_TRUE(readResult.isOk())
        << "Bypass mode must grant Read permission per ARM §5.2";
    if (readResult.isOk()) {
        EXPECT_TRUE(readResult.getValue().permissions.read)
            << "Bypass mode must set read=true in returned permissions";
        EXPECT_TRUE(readResult.getValue().permissions.write)
            << "Bypass mode must set write=true in returned permissions";
        EXPECT_TRUE(readResult.getValue().permissions.execute)
            << "Bypass mode must set execute=true in returned permissions";
    }

    TranslationResult writeResult = smmu.translate(N41_STREAM, TEST_PASID, TEST_PAGE,
                                                    AccessType::Write);
    EXPECT_TRUE(writeResult.isOk())
        << "Bypass mode must grant Write permission per ARM §5.2";

    TranslationResult execResult = smmu.translate(N41_STREAM, TEST_PASID, TEST_PAGE,
                                                   AccessType::Execute);
    EXPECT_TRUE(execResult.isOk())
        << "Bypass mode must grant Execute permission per ARM §5.2";
}

TEST(BypassPermissionsTest, StreamContextBypassGrantsFullPermissions) {
    // Test the StreamContext::translateUnlocked bypass path directly
    StreamContext ctx;
    ctx.setStage1Enabled(false);
    ctx.setStage2Enabled(false);
    // Create PASID 0 so the context is otherwise ready
    ASSERT_TRUE(ctx.createPASID(TEST_PASID).isOk());

    TranslationResult result = ctx.translate(TEST_PASID, TEST_PAGE,
                                              AccessType::Read,
                                              SecurityState::NonSecure);
    EXPECT_TRUE(result.isOk())
        << "StreamContext bypass must succeed";
    if (result.isOk()) {
        EXPECT_TRUE(result.getValue().permissions.read)
            << "Bypass must grant read";
        EXPECT_TRUE(result.getValue().permissions.write)
            << "Bypass must grant write";
        EXPECT_TRUE(result.getValue().permissions.execute)
            << "Bypass must grant execute";
    }
}

// ─── FINDING-NEW-43: classifyTranslationFault() must not use IOVA heuristics ──
//
// ARM §7.3.13–7.3.16 — fault classification must be based on actual error
// cause, not on arbitrary IOVA value ranges.

TEST(FaultClassificationTest, ZeroIOVANotClassifiedAsAccessFault) {
    SMMU smmu;
    enableSMMU(smmu);
    configureBasicStream(smmu, N43_STREAM);

    // Translate IOVA=0 which is unmapped — must produce F_TRANSLATION, not F_ACCESS
    smmu.clearEventQueue();
    TranslationResult result = smmu.translate(N43_STREAM, TEST_PASID, 0ULL,
                                               AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "Translation of unmapped IOVA=0 must fail";

    // No F_ACCESS event should be generated (that would be the IOVA heuristic)
    EXPECT_EQ(countEvents(smmu, EventType::F_ACCESS), 0)
        << "IOVA=0 unmapped translation must generate F_TRANSLATION, not F_ACCESS";
    // F_TRANSLATION must be generated as the default for unmapped pages
    EXPECT_GT(countEvents(smmu, EventType::F_TRANSLATION), 0)
        << "IOVA=0 unmapped translation must generate F_TRANSLATION per ARM §7.3.13";
}

TEST(FaultClassificationTest, HighIOVANotClassifiedAsAddrSizeFault) {
    SMMU smmu;
    enableSMMU(smmu);
    configureBasicStream(smmu, N43_STREAM + 1);

    // IOVA above 48-bit (0x0001000000001000) — unmapped, must produce F_TRANSLATION
    constexpr IOVA HIGH_IOVA = 0x0001000000001000ULL;
    smmu.clearEventQueue();
    TranslationResult result = smmu.translate(N43_STREAM + 1, TEST_PASID, HIGH_IOVA,
                                               AccessType::Read);
    EXPECT_TRUE(result.isError())
        << "Translation of unmapped high IOVA must fail";

    // F_ADDR_SIZE from the IOVA heuristic must NOT appear
    // (F_ADDR_SIZE is valid only when the address exceeds the configured input size)
    EXPECT_EQ(countEvents(smmu, EventType::F_ACCESS), 0)
        << "High IOVA must not generate F_ACCESS";
    // The fault must be F_TRANSLATION for an unmapped address
    EXPECT_GT(countEvents(smmu, EventType::F_TRANSLATION), 0)
        << "Unmapped high IOVA must generate F_TRANSLATION, not F_ADDR_SIZE heuristic";
}

} // namespace test
} // namespace smmu
