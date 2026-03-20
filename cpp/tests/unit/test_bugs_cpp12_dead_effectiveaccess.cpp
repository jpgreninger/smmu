// ARM SMMU v3 — BUG-CPP-1 / BUG-CPP-2 Regression Guard Tests
// Copyright (c) 2024 John Greninger
//
// These tests guard against regressions introduced by the planned refactoring
// of the dead effectiveAccessType computations in:
//
//   BUG-CPP-1: performTwoStageTranslation() computes effectiveAccessType at the
//     top level but passes the raw accessType to the three sub-functions.
//     Each sub-function redundantly recomputes the same override chain.
//     Divergence between the two code paths would silently break behaviour.
//
//   BUG-CPP-2: performStage1OnlyTranslation() and performStage2OnlyTranslation()
//     each compute a local effectiveAccessType (STRW->INSTCFG->PRIVCFG) but then
//     pass the raw accessType to streamContext->translate().  The local computation
//     is entirely dead code.  Correctness is preserved only because
//     StreamContext::translateUnlocked() re-derives the same overrides internally.
//
// The tests intentionally exercise the override chains from the caller's perspective:
//   1. INSTCFG=2 (Force Data) single-stage: Execute->Read override must allow read-only page.
//   2. INSTCFG=2 (Force Data) two-stage:    Execute->Read override must allow read-only page.
//   3. PRIVCFG=3 (Force Privileged) single-stage: Read promoted->ReadPrivileged on priv-only page.
//   4. PRIVCFG=3 (Force Privileged) two-stage:    Read promoted->ReadPrivileged on priv-only page.
//   5. PRIVCFG=2 (Force Unprivileged) + STRW=EL2: ReadPrivileged demoted->Read; priv-only page must FAIL.
//   6. Fragility canary: INSTCFG=2 Execute->Data conversion must produce Data access result.
//
// All tests currently PASS (the existing code is functionally correct).
// They become FAILING regression guards if the refactoring accidentally breaks any override path.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants — use 0x50+ SIDs to avoid collisions with other test files.
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID BASE_SID  = 0x50u;
static constexpr PASID    PID       = 0;
static constexpr IOVA     BASE_IOVA = 0x2000u;
static constexpr PA       BASE_PA   = 0xD000u;

// ─── Stream setup helpers ────────────────────────────────────────────────────

// Stage-1-only stream.
void setupStage1Stream(SMMU& smmu, StreamID sid, const StreamConfig& cfg) {
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "configureStream must succeed for sid=" << sid;
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());
}

// Two-stage stream — also creates the S2 address space and maps one stage-2 page.
//   stage1IOVA -> stage1PA (==IPA): mapped in stage-1 with s1perms.
//   stage1PA (IPA) -> stage2PA: mapped in stage-2 with s2perms.
// Returns the stage-2 PA on success.
void setupTwoStageStream(SMMU& smmu, StreamID sid, const StreamConfig& cfg,
                         IOVA iova, IPA ipa, PA s2pa,
                         const PagePermissions& s1perms,
                         const PagePermissions& s2perms) {
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "configureStream must succeed for sid=" << sid;
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());

    // Stage-2 address space.
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());

    // Stage-1 mapping: IOVA -> IPA.
    ASSERT_TRUE(smmu.mapPage(sid, PID, iova, ipa, s1perms).isOk())
        << "stage-1 mapPage must succeed";

    // Stage-2 mapping: IPA -> PA.
    ASSERT_TRUE(s2as->mapPage(ipa, s2pa, s2perms).isOk())
        << "stage-2 mapPage must succeed";
}

} // anonymous namespace

// ═════════════════════════════════════════════════════════════════════════════
// Test fixture
// ═════════════════════════════════════════════════════════════════════════════

class BugsCpp12DeadEffectiveAccess : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// ═════════════════════════════════════════════════════════════════════════════
// Test 1: INSTCFG=2 (Force Data) — single-stage stream
//
// INSTCFG=2: Execute -> Read.  Page is read-only (execute NOT set).
// With the override applied correctly: Read permission is checked, access succeeds.
// If the override is accidentally lost: Execute permission is checked, access fails.
// ═════════════════════════════════════════════════════════════════════════════

TEST_F(BugsCpp12DeadEffectiveAccess, InstCfg_Data_SingleStage_ConvertedCorrectly) {
    static constexpr StreamID SID = BASE_SID;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.instCfg            = 2u;  // Force Data: Execute -> Read
    cfg.t0sz               = 0u;  // no VA range restriction
    setupStage1Stream(*smmu_, SID, cfg);

    // Map page read-only (read allowed, execute NOT allowed).
    PagePermissions perms;
    perms.read    = true;
    perms.write   = false;
    perms.execute = false;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Translate with Execute access: INSTCFG=2 must convert Execute -> Read.
    // Read is allowed on this page, so translation must succeed.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-1/2: INSTCFG=2 (Force Data) single-stage — Execute must be "
           "converted to Read before the permission check; read-only page must succeed";

    // No permission-fault event should be in the queue.
    auto ev = smmu_->getEventQueue();
    bool has_perm_fault = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_PERMISSION) {
            has_perm_fault = true;
        }
    }
    EXPECT_FALSE(has_perm_fault)
        << "BUG-CPP-1/2: INSTCFG=2 single-stage — no F_PERMISSION must be "
           "generated when Execute is correctly overridden to Read";
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 2: INSTCFG=2 (Force Data) — two-stage stream
//
// Same override test but exercising the two-stage path (performBothStagesTranslation).
// Stage-1 page is read-only; stage-2 page is also read-only.
// INSTCFG=2: Execute -> Read must be applied at both stages.
// ═════════════════════════════════════════════════════════════════════════════

TEST_F(BugsCpp12DeadEffectiveAccess, InstCfg_Data_TwoStage_ConvertedCorrectly) {
    static constexpr StreamID SID  = BASE_SID + 1u;
    static constexpr IPA      IPA_ = 0xE000u;   // IPA used as intermediate address
    static constexpr PA       PA_  = 0xF000u;   // final PA after stage-2 translation

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.instCfg            = 2u;  // Force Data: Execute -> Read
    cfg.t0sz               = 0u;
    cfg.s2t0sz             = 0u;
    cfg.s2ps               = 5u;  // 48-bit S2PS

    // Stage-1 mapping: read-only (execute NOT set).
    PagePermissions s1perms;
    s1perms.read    = true;
    s1perms.write   = false;
    s1perms.execute = false;

    // Stage-2 mapping: read-only.
    PagePermissions s2perms;
    s2perms.read    = true;
    s2perms.write   = false;
    s2perms.execute = false;

    setupTwoStageStream(*smmu_, SID, cfg,
                        BASE_IOVA, IPA_, PA_,
                        s1perms, s2perms);

    // Execute access: INSTCFG=2 must convert Execute -> Read at both stages.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-1/2: INSTCFG=2 (Force Data) two-stage — Execute must be "
           "converted to Read before both stage-1 and stage-2 permission checks; "
           "both read-only stages must succeed";

    auto ev = smmu_->getEventQueue();
    bool has_perm_fault = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_PERMISSION) {
            has_perm_fault = true;
        }
    }
    EXPECT_FALSE(has_perm_fault)
        << "BUG-CPP-1/2: INSTCFG=2 two-stage — no F_PERMISSION must be "
           "generated when Execute is correctly overridden to Read";
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 3: PRIVCFG=3 (Force Privileged) — single-stage stream
//
// PRIVCFG=3 promotes unprivileged access types to privileged variants.
// Page is read-allowed but privilegedOnly=true.
// Incoming Read -> promoted to ReadPrivileged -> privilegedOnly bypass -> success.
// If the override is lost: unprivileged Read on privilegedOnly page -> F_PERMISSION.
// ═════════════════════════════════════════════════════════════════════════════

TEST_F(BugsCpp12DeadEffectiveAccess, PrivCfg_ForcePrivileged_SingleStage) {
    static constexpr StreamID SID = BASE_SID + 2u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.privCfg            = 3u;  // Force Privileged
    cfg.t0sz               = 0u;
    setupStage1Stream(*smmu_, SID, cfg);

    // Map page: read allowed, privilegedOnly (unprivileged Read -> denied).
    PagePermissions perms;
    perms.read          = true;
    perms.write         = false;
    perms.execute       = false;
    perms.privilegedOnly = true;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Unprivileged Read: PRIVCFG=3 must promote to ReadPrivileged.
    // ReadPrivileged bypasses the privilegedOnly check -> success.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-1/2: PRIVCFG=3 (Force Privileged) single-stage — Read must be "
           "promoted to ReadPrivileged; privilegedOnly page must be accessible";

    auto ev = smmu_->getEventQueue();
    bool has_perm_fault = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_PERMISSION) {
            has_perm_fault = true;
        }
    }
    EXPECT_FALSE(has_perm_fault)
        << "BUG-CPP-1/2: PRIVCFG=3 single-stage — no F_PERMISSION must be generated "
           "when Read is correctly promoted to ReadPrivileged";
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 4: PRIVCFG=3 (Force Privileged) — two-stage stream
//
// Stage-1 page is privilegedOnly=true (unprivileged Read would be denied).
// Stage-2 page is NOT privilegedOnly (read freely accessible at stage-2).
// PRIVCFG=3 promotes unprivileged Read -> ReadPrivileged.
// ReadPrivileged bypasses the privilegedOnly check at stage-1 -> access succeeds.
//
// Note: The stage-2 PTW lookup always uses AccessType::Read for the table walk
// (ARM §3.3.2 — the access type of the transaction itself is applied at the
// permission-intersection step, not the PTW lookup).  Therefore stage-2 pages
// must NOT carry privilegedOnly=true for the PTW to succeed.  This test targets
// the PRIVCFG override as it affects stage-1 only, which is the path exercised
// by performBothStagesTranslation when it calls
//   stage1AddressSpace->translatePage(iova, effectiveAccessType, ...)
// with the post-override effectiveAccessType.
// ═════════════════════════════════════════════════════════════════════════════

TEST_F(BugsCpp12DeadEffectiveAccess, PrivCfg_ForcePrivileged_TwoStage) {
    static constexpr StreamID SID  = BASE_SID + 3u;
    static constexpr IPA      IPA_ = 0x10000u;
    static constexpr PA       PA_  = 0x11000u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.privCfg            = 3u;  // Force Privileged
    cfg.t0sz               = 0u;
    cfg.s2t0sz             = 0u;
    cfg.s2ps               = 5u;

    // Stage-1: read allowed, privilegedOnly=true.
    // Unprivileged Read would fail here; ReadPrivileged (post PRIVCFG=3) must pass.
    PagePermissions s1perms;
    s1perms.read          = true;
    s1perms.write         = false;
    s1perms.execute       = false;
    s1perms.privilegedOnly = true;

    // Stage-2: read allowed, privilegedOnly=false.
    // The stage-2 PTW lookup uses AccessType::Read (per ARM §3.3.2) so this page
    // must not be privilegedOnly to avoid a spurious PTW fault.
    PagePermissions s2perms;
    s2perms.read          = true;
    s2perms.write         = false;
    s2perms.execute       = false;
    s2perms.privilegedOnly = false;

    setupTwoStageStream(*smmu_, SID, cfg,
                        BASE_IOVA, IPA_, PA_,
                        s1perms, s2perms);

    // Unprivileged Read: PRIVCFG=3 must promote to ReadPrivileged before stage-1
    // permission check.  ReadPrivileged bypasses privilegedOnly at stage-1 -> succeed.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-1/2: PRIVCFG=3 (Force Privileged) two-stage — Read must be "
           "promoted to ReadPrivileged before the stage-1 permission check; "
           "privilegedOnly stage-1 page must be accessible after promotion";

    auto ev = smmu_->getEventQueue();
    bool has_perm_fault = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_PERMISSION) {
            has_perm_fault = true;
        }
    }
    EXPECT_FALSE(has_perm_fault)
        << "BUG-CPP-1/2: PRIVCFG=3 two-stage — no F_PERMISSION must be generated "
           "when Read is correctly promoted to ReadPrivileged at stage-1";
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 5: PRIVCFG=2 (Force Unprivileged) + STRW=EL2 — single-stage stream
//
// STRW=EL2 would normally promote all accesses to Privileged variants.
// PRIVCFG=2 then demotes Privileged -> Unprivileged.
// Net effect: access arrives as ReadPrivileged, STRW promotes (already priv),
// PRIVCFG=2 demotes back to Read.
// Page is privilegedOnly=true.
// Unprivileged Read on a privilegedOnly page -> F_PERMISSION.
//
// This validates the STRW->PRIVCFG chain interaction:
//   ReadPrivileged --[STRW=EL2: already priv, no-op]-->
//   ReadPrivileged --[PRIVCFG=2: demote]--> Read
//   Read on privilegedOnly=true -> DENY -> F_PERMISSION.
// ═════════════════════════════════════════════════════════════════════════════

TEST_F(BugsCpp12DeadEffectiveAccess, PrivCfg_ForceUnprivileged_SingleStage) {
    static constexpr StreamID SID = BASE_SID + 4u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.strw               = StreamWorld::EL2;  // STRW=EL2 → promote unprivileged
    cfg.privCfg            = 2u;               // Force Unprivileged — overrides STRW
    cfg.t0sz               = 0u;
    setupStage1Stream(*smmu_, SID, cfg);

    // Map page: read allowed, privilegedOnly (unprivileged access denied).
    PagePermissions perms;
    perms.read          = true;
    perms.write         = false;
    perms.execute       = false;
    perms.privilegedOnly = true;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Incoming ReadPrivileged:
    //   STRW=EL2 -> already privileged, no change.
    //   PRIVCFG=2 -> demote ReadPrivileged -> Read.
    //   Read on privilegedOnly=true -> DENY.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::ReadPrivileged);
    EXPECT_TRUE(r.isError())
        << "BUG-CPP-1/2: PRIVCFG=2 (Force Unprivileged) + STRW=EL2 single-stage — "
           "ReadPrivileged demoted to Read by PRIVCFG=2; privilegedOnly page must DENY access";

    // An F_PERMISSION event must be in the event queue.
    auto ev = smmu_->getEventQueue();
    bool has_perm_fault = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_PERMISSION) {
            has_perm_fault = true;
        }
    }
    EXPECT_TRUE(has_perm_fault)
        << "BUG-CPP-1/2: PRIVCFG=2 (Force Unprivileged) + STRW=EL2 single-stage — "
           "F_PERMISSION must be generated when demoted Read is denied on privilegedOnly page";
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 6: Fragility canary — INSTCFG=2 Execute->Data must produce correct result
//
// This test directly verifies the observable outcome of the INSTCFG=2 conversion:
//   - Page is mapped read-only (Read allowed, Execute NOT allowed).
//   - Incoming access is Execute.
//   - With INSTCFG=2 active, Execute becomes Read before the permission check.
//   - Translation must succeed (no fault, result is a valid PA).
//
// If any refactoring accidentally drops the override on the path that feeds the
// actual permission check, this Execute on a non-executable page will fail with
// F_PERMISSION instead of succeeding.
//
// This canary catches divergence between the "dead" local effectiveAccessType
// computation and the actual access type passed to streamContext->translate().
// ═════════════════════════════════════════════════════════════════════════════

TEST_F(BugsCpp12DeadEffectiveAccess, InstCfg_Data_FragilityCanary_ExecuteAsDataSucceeds) {
    static constexpr StreamID SID = BASE_SID + 5u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.instCfg            = 2u;  // Force Data: Execute -> Read
    cfg.t0sz               = 0u;
    setupStage1Stream(*smmu_, SID, cfg);

    // Map page as read-only (execute NOT allowed).
    PagePermissions perms;
    perms.read    = true;
    perms.write   = false;
    perms.execute = false;
    ASSERT_TRUE(smmu_->mapPage(SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Execute access with INSTCFG=2 must convert to Read and succeed.
    auto r = smmu_->translate(SID, PID, BASE_IOVA, AccessType::Execute);

    // --- Primary assertion: translation must succeed ---
    EXPECT_TRUE(r.isOk())
        << "BUG-CPP-1/2 Canary: INSTCFG=2 must convert Execute to Read BEFORE the "
           "permission check reaches streamContext->translate(); "
           "read-only page must succeed when the override is applied correctly";

    // --- Secondary assertion: no fault events at all ---
    auto ev = smmu_->getEventQueue();
    EXPECT_TRUE(ev.empty())
        << "BUG-CPP-1/2 Canary: no fault events must be generated when INSTCFG=2 "
           "correctly converts Execute to Read and the read-only page is accessible";

    // --- Physical address sanity check ---
    if (r.isOk()) {
        EXPECT_EQ(r.getValue().physicalAddress, BASE_PA)
            << "BUG-CPP-1/2 Canary: translated PA must equal the mapped PA";
    }
}
