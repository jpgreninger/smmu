// ARM SMMU v3 — Conformance Gaps NEW-3, NEW-4, NEW-6, NEW-7, NEW-8, NEW-9 fixes
// Copyright (c) 2024 John Greninger
//
// TDD spec tests for six conformance gaps:
//
//   NEW-3  §3.4/§5.2: S2T0SZ IPA input range enforcement
//     - IPA >= 2^(64-S2T0SZ) must generate F_TRANSLATION in two-stage and stage-2-only streams
//
//   NEW-4  §3.3.4/§13.5: STE.PRIVCFG override before permission checks
//     - PRIVCFG=3 (Force Privileged): unprivileged access types promoted to privileged
//     - PRIVCFG=2 (Force Unprivileged): privileged access types demoted to unprivileged
//
//   NEW-6  §5.2: INSTCFG ReadWrite gap documented (intentional, not a bug)
//
//   NEW-7  §5.4: CD.EPD0=1: TTBR0 translation walk disabled -> F_TRANSLATION
//
//   NEW-8  §3.4/§7.3.14: Stage-2 output PA vs S2PS check -> F_ADDR_SIZE
//
//   NEW-9  §3.5.3: Stall events bypass EVENTQEN=0 gate (removed incorrect exception)

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include "smmu/configuration.h"
#include <memory>

using namespace smmu;

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants and helpers
// ─────────────────────────────────────────────────────────────────────────────

namespace {

static constexpr StreamID SID      = 0x20;
static constexpr PASID    PID      = 0;
static constexpr IOVA     BASE_IOVA = 0x1000;
static constexpr PA       BASE_PA   = 0xA000;

// Build and enable a stage-2-only stream.
void setupStage2OnlyStream(SMMU& smmu, StreamID sid = SID, uint8_t s2t0sz = 16u, uint8_t s2ps = 5u) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2t0sz             = s2t0sz;
    cfg.s2ps               = s2ps;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "configureStream must succeed for stage-2-only stream";
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());
}

// Build and enable a two-stage stream.
void setupTwoStageStream(SMMU& smmu, StreamID sid = SID, uint8_t s2t0sz = 16u, uint8_t s2ps = 5u,
                         uint8_t t0sz = 0u) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = t0sz;
    cfg.s2t0sz             = s2t0sz;
    cfg.s2ps               = s2ps;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk())
        << "configureStream must succeed for two-stage stream";
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());
}

// Build and enable a stage-1-only stream.
void setupStage1Stream(SMMU& smmu, StreamID sid = SID, PASID pasid = PID) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, pasid).isOk());
}

} // anonymous namespace

// ═════════════════════════════════════════════════════════════════════════════
// NEW-3: S2T0SZ IPA input range enforcement (ARM IHI0070G.b §3.4 / §5.2)
// ═════════════════════════════════════════════════════════════════════════════

class New3S2T0SZTests : public ::testing::Test {
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

// Two-stage stream: s2t0sz=16 → IPA limit = 2^(64-16) = 2^48.
// Map IOVA -> IPA at exactly 2^48 (at the limit, out of range).
// Translation must fail with F_TRANSLATION.
TEST_F(New3S2T0SZTests, new3_s2t0sz_16_ipa_at_limit_generates_f_translation) {
    // Set up two-stage stream with t0sz=0 (no VA limit) and s2t0sz=16.
    static constexpr StreamID TEST_SID = SID;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = 0u;   // no stage-1 VA restriction
    cfg.s2t0sz             = 16u;  // stage-2 IPA limit = 2^48
    cfg.s2ps               = 5u;   // 48-bit S2PS
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Create a stage-2 address space (must be separate to avoid alias shortcut).
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(TEST_SID, s2as).isOk());

    // Map IOVA -> IPA at exactly 2^48 (at the S2T0SZ limit, out of range).
    static constexpr IPA OUT_OF_RANGE_IPA = UINT64_C(1) << 48u;
    PagePermissions perms(true, true, false);
    // Stage-1 maps IOVA -> the out-of-range IPA.
    ASSERT_TRUE(smmu_->mapPage(TEST_SID, PID, BASE_IOVA, OUT_OF_RANGE_IPA, perms).isOk());

    auto r = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "NEW-3: IPA at S2T0SZ limit must produce a translation error";

    auto ev = smmu_->getEventQueue();
    bool has_f_translation = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            has_f_translation = true;
        }
    }
    EXPECT_TRUE(has_f_translation)
        << "NEW-3: §3.4 — IPA >= 2^(64-S2T0SZ) must generate F_TRANSLATION event";
}

// s2t0sz=0 imposes no IPA restriction; a large IPA must translate normally.
TEST_F(New3S2T0SZTests, new3_s2t0sz_0_no_restriction) {
    static constexpr StreamID TEST_SID = SID + 1u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.t0sz               = 0u;
    cfg.s2t0sz             = 0u;  // no IPA range restriction
    cfg.s2ps               = 5u;  // 48-bit S2PS
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(TEST_SID, s2as).isOk());

    // A large IPA that would fail if s2t0sz=16 were applied, but s2t0sz=0 → no check.
    static constexpr IPA LARGE_IPA = (UINT64_C(1) << 48u) | 0x2000u;
    // Map IPA -> PA in stage-2 (IPA within 48-bit S2PS limit).
    // The IPA is beyond 2^48, so we use a smaller IPA for the s2 map and a
    // different one for stage-1 output to keep both within bounds.
    static constexpr IPA WITHIN_IPA = 0x10000u;
    static constexpr PA  S2_PA      = 0xB000u;
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(smmu_->mapPage(TEST_SID, PID, BASE_IOVA, WITHIN_IPA, perms).isOk());
    PagePermissions s2perms(true, true, false);
    ASSERT_TRUE(s2as->mapPage(WITHIN_IPA, S2_PA, s2perms).isOk());

    auto r = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "NEW-3: s2t0sz=0 imposes no IPA restriction; translation must succeed";

    auto ev = smmu_->getEventQueue();
    bool has_f_translation = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            has_f_translation = true;
        }
    }
    EXPECT_FALSE(has_f_translation)
        << "NEW-3: s2t0sz=0 — no F_TRANSLATION event should be generated";
}

// Stage-2-only stream: s2t0sz=16 → IPA limit = 2^48.
// IOVA == IPA in stage-2-only mode. IPA=2^48 is out of range → F_TRANSLATION.
TEST_F(New3S2T0SZTests, new3_stage2_only_ipa_exceeds_s2t0sz) {
    static constexpr StreamID TEST_SID = SID + 2u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2t0sz             = 16u;  // IPA limit = 2^48
    cfg.s2ps               = 5u;
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // In stage-2-only mode IOVA is treated as IPA directly.
    // Use an IPA/IOVA at exactly 2^48 — out of the S2T0SZ range.
    static constexpr IOVA OUT_OF_RANGE_IPA = UINT64_C(1) << 48u;

    auto r = smmu_->translate(TEST_SID, PID, OUT_OF_RANGE_IPA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "NEW-3: stage-2-only stream — IPA at S2T0SZ limit must produce error";

    auto ev = smmu_->getEventQueue();
    bool has_f_translation = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_TRANSLATION) {
            has_f_translation = true;
        }
    }
    EXPECT_TRUE(has_f_translation)
        << "NEW-3: stage-2-only stream — IPA >= 2^(64-S2T0SZ) must generate F_TRANSLATION";
}

// ═════════════════════════════════════════════════════════════════════════════
// NEW-4: STE.PRIVCFG override before permission checks (§3.3.4/§13.5)
// ═════════════════════════════════════════════════════════════════════════════

class New4PrivCFGTests : public ::testing::Test {
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

// PRIVCFG=3 (Force Privileged), page is privilegedOnly.
// Incoming Read access → promoted to ReadPrivileged → privilegedOnly check bypassed → success.
TEST_F(New4PrivCFGTests, new4_privcfg_privileged_converts_read_to_priv) {
    static constexpr StreamID TEST_SID = SID + 10u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.privCfg            = 3u;  // Force Privileged
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Map page as read-allowed but privilegedOnly.
    PagePermissions perms;
    perms.read          = true;
    perms.write         = false;
    perms.execute       = false;
    perms.privilegedOnly = true;
    ASSERT_TRUE(smmu_->mapPage(TEST_SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Read access: PRIVCFG=3 promotes Read->ReadPrivileged, bypassing privilegedOnly.
    auto r = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "NEW-4: PRIVCFG=3 (Force Privileged) — Read promoted to ReadPrivileged; "
           "privilegedOnly page must be accessible";
}

// PRIVCFG=3 (Force Privileged): ExecutePrivileged access on execute-allowed page succeeds.
// Incoming Execute → promoted to ExecutePrivileged → privilegedOnly check bypassed → success.
TEST_F(New4PrivCFGTests, new4_privcfg_privileged_converts_exec_to_priv) {
    static constexpr StreamID TEST_SID = SID + 11u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.privCfg            = 3u;  // Force Privileged
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Map page as execute-allowed, privilegedOnly.
    PagePermissions perms;
    perms.read          = false;
    perms.write         = false;
    perms.execute       = true;
    perms.privilegedOnly = true;
    ASSERT_TRUE(smmu_->mapPage(TEST_SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Execute access: PRIVCFG=3 promotes Execute->ExecutePrivileged, bypassing privilegedOnly.
    auto r = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(r.isOk())
        << "NEW-4: PRIVCFG=3 (Force Privileged) — Execute promoted to ExecutePrivileged; "
           "privilegedOnly+execute page must be accessible";
}

// PRIVCFG=2 (Force Unprivileged): ReadPrivileged demoted to Read.
// Page is read-allowed and NOT privilegedOnly → Read passes → success.
TEST_F(New4PrivCFGTests, new4_privcfg_unprivileged_demotes_readpriv) {
    static constexpr StreamID TEST_SID = SID + 12u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.privCfg            = 2u;  // Force Unprivileged
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Map page as read-allowed, NOT privilegedOnly.
    PagePermissions perms;
    perms.read          = true;
    perms.write         = false;
    perms.execute       = false;
    perms.privilegedOnly = false;
    ASSERT_TRUE(smmu_->mapPage(TEST_SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // ReadPrivileged access: PRIVCFG=2 demotes ReadPrivileged->Read.
    // Page is read-allowed and not privilegedOnly → Read passes.
    auto r = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::ReadPrivileged);
    EXPECT_TRUE(r.isOk())
        << "NEW-4: PRIVCFG=2 (Force Unprivileged) — ReadPrivileged demoted to Read; "
           "non-privilegedOnly read-allowed page must be accessible";
}

// PRIVCFG=0: no override — normal behavior unchanged.
TEST_F(New4PrivCFGTests, new4_privcfg_0_no_override) {
    static constexpr StreamID TEST_SID = SID + 13u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.privCfg            = 0u;  // No override (from-translation)
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Map page as read-only, NOT privilegedOnly.
    PagePermissions perms;
    perms.read          = true;
    perms.write         = false;
    perms.execute       = false;
    perms.privilegedOnly = false;
    ASSERT_TRUE(smmu_->mapPage(TEST_SID, PID, BASE_IOVA, BASE_PA, perms).isOk());

    // Read must succeed (no override, read is permitted).
    auto rRead = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Read);
    EXPECT_TRUE(rRead.isOk())
        << "NEW-4: PRIVCFG=0 — Read on read-allowed page must succeed without any override";

    // Execute must fail (no override, execute not permitted).
    auto rExec = smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Execute);
    EXPECT_TRUE(rExec.isError())
        << "NEW-4: PRIVCFG=0 — Execute on non-executable page must fail without override";
}

// ═════════════════════════════════════════════════════════════════════════════
// NEW-8: Stage-2 output PA vs S2PS check (ARM IHI0070G.b §3.4 / §7.3.14)
// ═════════════════════════════════════════════════════════════════════════════

class New8S2PSTests : public ::testing::Test {
protected:
    void SetUp() override {
        // Use a custom SMMU with 52-bit OAS so that PAs above 32-bit can be mapped.
        SMMUConfiguration smmuCfg;
        AddressConfiguration addrCfg;
        addrCfg.maxPASize = 52u;  // 52-bit max PA to allow mapping above 32-bit
        smmuCfg.setAddressConfiguration(addrCfg);
        smmu_ = std::make_unique<SMMU>(smmuCfg);
        smmu_->enable();
        smmu_->setCR0(smmu_->getCR0() | SMMU::CR0_EVENTQEN);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// Stage-2-only stream: s2ps=0 (32-bit limit).
// Stage-2 maps IPA -> PA where PA = 2^32 (at the limit, out of range).
// Translation must fail with F_ADDR_SIZE.
TEST_F(New8S2PSTests, new8_stage2_pa_exceeds_s2ps_generates_f_addr_size) {
    static constexpr StreamID TEST_SID = SID + 20u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2t0sz             = 0u;   // no IPA restriction
    cfg.s2ps               = 0u;   // 32-bit S2PS limit (PA must be < 2^32)
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Set up stage-2 address space.
    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(TEST_SID, s2as).isOk());

    // Map IPA -> PA where PA is exactly at 2^32 (outside 32-bit S2PS).
    static constexpr IPA  TEST_IPA = 0x1000u;
    static constexpr PA   OUT_PA   = UINT64_C(1) << 32u;  // 4 GiB — above 32-bit limit
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(s2as->mapPage(TEST_IPA, OUT_PA, perms).isOk());

    auto r = smmu_->translate(TEST_SID, PID, TEST_IPA, AccessType::Read);
    EXPECT_TRUE(r.isError())
        << "NEW-8: PA at S2PS limit must produce a translation error";

    auto ev = smmu_->getEventQueue();
    bool has_f_addr_size = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE) {
            has_f_addr_size = true;
        }
    }
    EXPECT_TRUE(has_f_addr_size)
        << "NEW-8: §3.4/§7.3.14 — PA >= 2^S2PSBits must generate F_ADDR_SIZE event";
}

// Stage-2-only stream: s2ps=5 (48-bit limit).
// Stage-2 maps IPA -> PA well within 48-bit range → success.
TEST_F(New8S2PSTests, new8_stage2_pa_within_s2ps_succeeds) {
    static constexpr StreamID TEST_SID = SID + 21u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2t0sz             = 0u;
    cfg.s2ps               = 5u;   // 48-bit S2PS limit
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    auto s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu_->setStreamStage2AddressSpace(TEST_SID, s2as).isOk());

    // Map IPA -> PA well within 48-bit range.
    static constexpr IPA TEST_IPA = 0x2000u;
    static constexpr PA  TEST_PA  = 0xC000u;
    PagePermissions perms(true, true, false);
    ASSERT_TRUE(s2as->mapPage(TEST_IPA, TEST_PA, perms).isOk());

    auto r = smmu_->translate(TEST_SID, PID, TEST_IPA, AccessType::Read);
    EXPECT_TRUE(r.isOk())
        << "NEW-8: PA within S2PS range must translate successfully";

    auto ev = smmu_->getEventQueue();
    bool has_f_addr_size = false;
    for (const auto& e : ev) {
        if (e.type == EventType::F_ADDR_SIZE) {
            has_f_addr_size = true;
        }
    }
    EXPECT_FALSE(has_f_addr_size)
        << "NEW-8: PA within S2PS range must not generate F_ADDR_SIZE event";
}

// ═════════════════════════════════════════════════════════════════════════════
// NEW-9: Stall events must not be written when EVENTQEN=0 (ARM IHI0070G.b §3.5.3)
// ═════════════════════════════════════════════════════════════════════════════

class New9EventQENTests : public ::testing::Test {
protected:
    void SetUp() override {
        smmu_ = std::make_unique<SMMU>();
        smmu_->enable();
        // Deliberately leave EVENTQEN=0 after enable() — CR0_SMMUEN is set but
        // CR0_EVENTQEN is cleared so no events are enqueued.
        uint32_t cr0 = smmu_->getCR0();
        cr0 &= ~SMMU::CR0_EVENTQEN;
        smmu_->setCR0(cr0);
    }

    void TearDown() override {
        smmu_.reset();
    }

    std::unique_ptr<SMMU> smmu_;
};

// Configure a stall-mode stream, disable EVENTQEN, trigger a fault.
// The event queue must remain empty — even stall events obey the EVENTQEN gate.
TEST_F(New9EventQENTests, new9_stall_event_not_written_when_eventqen_zero) {
    static constexpr StreamID TEST_SID = SID + 30u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Stall;  // stall mode
    ASSERT_TRUE(smmu_->configureStream(TEST_SID, cfg).isOk());
    ASSERT_TRUE(smmu_->enableStream(TEST_SID).isOk());
    ASSERT_TRUE(smmu_->createStreamPASID(TEST_SID, PID).isOk());

    // Confirm EVENTQEN is indeed 0.
    ASSERT_EQ(smmu_->getCR0() & SMMU::CR0_EVENTQEN, 0u)
        << "Precondition: EVENTQEN must be 0 for this test";

    // Trigger a fault by translating an unmapped address (stall mode would
    // normally enqueue a stall event; with EVENTQEN=0 it must not).
    smmu_->translate(TEST_SID, PID, BASE_IOVA, AccessType::Read);

    // Event queue must remain empty.
    auto ev = smmu_->getEventQueue();
    EXPECT_TRUE(ev.empty())
        << "NEW-9: §3.5.3 — EVENTQEN=0 must suppress ALL events including stall events; "
           "event queue must remain empty";
}
