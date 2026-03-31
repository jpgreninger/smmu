// TDD failing tests for BUG-AUDIT-58, BUG-AUDIT-59, BUG-AUDIT-60, BUG-AUDIT-61,
// and BUG-AUDIT-62.
//
// BUG-AUDIT-58 (Both §5.2 SteIllegal): S2TG not validated against IDR5 granule support.
//   Spec §5.2 SteIllegal() line 8362: if !GranuleSupported(s2tg) then return TRUE.
//   IDR5 advertises only GRAN4K=1 (GRAN16K=0, GRAN64K=0).
//   s2tg=0 → 4KB (valid); s2tg=1 → 64KB (invalid); s2tg=2 → 16KB (invalid);
//   s2tg=3 → Reserved (always invalid).
//   Current code stores any s2tg without validation → no C_BAD_STE emitted.
//   BEFORE FIX: configureStream() accepts s2tg=1/2/3 silently → tests FAIL.
//   AFTER FIX:  configureStream() emits C_BAD_STE for s2tg != 0 → tests PASS.
//
// BUG-AUDIT-59 (Rust-only §6.3.1/§6.3.6): Relaxed ordering in IDR register reads.
//   get_idr0() and get_idr5() use Ordering::Relaxed for stall_model_.load().
//   The store in set_stall_model() uses Ordering::Release; reads should use
//   Ordering::Acquire. This file includes a C++ functional consistency test.
//   NOTE: On x86 this is functionally correct today; the fix is a code quality
//   requirement. The C++ test here verifies consistent functional behavior.
//
// BUG-AUDIT-60 (Both §6.3.12): CR2 write must be ignored when SMMUEN=1.
//   Spec §6.3.12 lines 12578-12591: CR2 is effectively RO when SMMUEN=1.
//   Current code applies the write even when SMMUEN=1.
//   BEFORE FIX: getCR2() returns non-zero after write with SMMUEN=1 → tests FAIL.
//   AFTER FIX:  getCR2() returns 0 (write silently ignored) → tests PASS.
//
// BUG-AUDIT-61 (Both §6.3.11): CR1 write must be ignored when SMMUEN=1 or queues enabled.
//   Spec §6.3.11: TABLE_* fields are RO when SMMUEN=1; QUEUE_* fields RO when queue enabled.
//   Current code applies the write regardless.
//   BEFORE FIX: getCR1() returns non-zero after write with SMMUEN=1 → tests FAIL.
//   AFTER FIX:  getCR1() returns 0 (write silently ignored) → tests PASS.
//
// BUG-AUDIT-62 (C++ only §9.1): gatosTranslate() uses shadow smmuen_ instead of cr0_.
//   This is a code consistency/maintenance bug; shadow bool is always kept in sync
//   so functional tests pass. These tests document the requirement.

#include <gtest/gtest.h>
#include <type_traits>
#include <memory>
#include "smmu/smmu.h"
#include "smmu/address_space.h"

using namespace smmu;

// Helper: enable all queues and the SMMU global enable.
static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
    s.setS1PSupported(true);
    s.setS2PSupported(true);
}

// Helper: check whether the event queue contains at least one event of given type.
static bool hasEvent(const SMMU& s, EventType type) {
    const auto& events = s.getEventQueue();
    for (const auto& ev : events) {
        if (ev.type == type) {
            return true;
        }
    }
    return false;
}

// Helper: build a minimal legal stage-2-only stream config.
// s2tg=0 (4KB) is valid; tests override s2tg to inject the bug.
static StreamConfig stage2Config(uint8_t s2tg = 0u) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = false;
    cfg.stage2Enabled      = true;
    cfg.s2aa64             = true;
    cfg.s2t0sz             = 16u;  // legal minimum
    cfg.s2ps               = 5u;   // 48-bit PA
    cfg.s2tg               = s2tg;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.vmid               = 10u;
    return cfg;
}

// ─── BUG-AUDIT-58 tests — S2TG not validated against IDR5 granule support ────

// BUG-AUDIT-58 test 1: s2tg=1 (64KB) must emit C_BAD_STE — not in IDR5.
//
// ARM §5.2 SteIllegal() line 8362: GranuleSupported(s2tg) checks IDR5.
// IDR5 advertises only GRAN4K (bit[4]=1). 64KB (s2tg=1) is not supported.
//
// BEFORE FIX: configureStream() returns Ok, no C_BAD_STE event → FAILS.
// AFTER FIX:  C_BAD_STE event in event queue → PASSES.
TEST(BugAudit58, Stage2_s2tg64k_Emits_CBadSte) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config(1u);  // s2tg=1 → 64KB (not in IDR5)
    smmu.configureStream(600u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-AUDIT-58: s2tg=1 (64KB) must emit C_BAD_STE because IDR5 only "
           "advertises GRAN4K. ARM §5.2 SteIllegal() line 8362: "
           "if !GranuleSupported(s2tg) then return TRUE. "
           "Current code stores s2tg without validation.";
}

// BUG-AUDIT-58 test 2: s2tg=2 (16KB) must emit C_BAD_STE — not in IDR5.
//
// ARM §5.2 SteIllegal() line 8362: GranuleSupported(s2tg) checks IDR5.
// IDR5.GRAN16K (bit[5]) is 0 in this model. 16KB (s2tg=2) is not supported.
//
// BEFORE FIX: configureStream() returns Ok, no C_BAD_STE event → FAILS.
// AFTER FIX:  C_BAD_STE event in event queue → PASSES.
TEST(BugAudit58, Stage2_s2tg16k_Emits_CBadSte) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config(2u);  // s2tg=2 → 16KB (not in IDR5)
    smmu.configureStream(601u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-AUDIT-58: s2tg=2 (16KB) must emit C_BAD_STE because IDR5 only "
           "advertises GRAN4K. ARM §5.2 SteIllegal() line 8362. "
           "Current code stores s2tg without validation.";
}

// BUG-AUDIT-58 test 3: s2tg=3 (Reserved) must emit C_BAD_STE — always invalid.
//
// ARM §5.2 SteIllegal() line 8362: s2tg=0b11 is Reserved and therefore never
// supported regardless of IDR5 granule configuration.
//
// BEFORE FIX: configureStream() returns Ok, no C_BAD_STE event → FAILS.
// AFTER FIX:  C_BAD_STE event in event queue → PASSES.
TEST(BugAudit58, Stage2_s2tgReserved_Emits_CBadSte) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config(3u);  // s2tg=3 → Reserved (always illegal)
    smmu.configureStream(602u, cfg);

    EXPECT_TRUE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-AUDIT-58: s2tg=3 (Reserved) must always emit C_BAD_STE. "
           "ARM §5.2 SteIllegal() line 8362: Reserved s2tg is always unsupported. "
           "Current code stores s2tg without validation.";
}

// BUG-AUDIT-58 test 4 (regression): s2tg=0 (4KB) must NOT emit C_BAD_STE.
//
// IDR5.GRAN4K is set; s2tg=0 (4KB) is the only supported granule.
// This must pass both before and after the fix (regression guard).
//
// BEFORE / AFTER FIX: s2tg=0 is valid → no C_BAD_STE → PASSES.
TEST(BugAudit58, Stage2_s2tg4k_Valid_NoEvent) {
    SMMU smmu;
    enableSMMU(smmu);

    StreamConfig cfg = stage2Config(0u);  // s2tg=0 → 4KB (valid per IDR5)
    EXPECT_TRUE(smmu.configureStream(603u, cfg).isOk())
        << "BUG-AUDIT-58 regression: s2tg=0 (4KB) must be accepted without error.";

    EXPECT_FALSE(hasEvent(smmu, EventType::C_BAD_STE))
        << "BUG-AUDIT-58 regression: s2tg=0 (4KB, supported in IDR5) must NOT emit "
           "C_BAD_STE. ARM §5.2 SteIllegal(): GranuleSupported(0) is TRUE.";
}

// ─── BUG-AUDIT-59 — IDR0 STALL_MODEL functional consistency (C++ counterpart) ──
//
// BUG-AUDIT-59 is Rust-only (memory ordering). This test verifies the equivalent
// functional correctness in C++: after setStallModel(0x01), both getIDR0() and
// getIDR5() must reflect the new stall_model value.
//
// This test validates that IDR register read/write consistency is correct in C++
// and documents the requirement for the Rust fix (Relaxed → Acquire in load calls).
//
// BEFORE / AFTER FIX: Always passes in C++ (atomics are sequentially consistent
// on x86). This test documents the behavioral requirement; if ever ported to
// a weakly-ordered architecture, the ordering fix becomes mandatory.

// BUG-AUDIT-59 C++ test 1: After setStallModel(0x01), getIDR0() STALL_MODEL field
// bits[25:24] must reflect 0x01.
//
// NOTE: This test PASSES on the current code because C++ uses the same Relaxed
// ordering. Included as a documentation/regression test.
TEST(BugAudit59, GetIdr0_ReflectsStallModel_AfterSet) {
    SMMU smmu;

    smmu.setStallModel(0x01u);
    uint32_t idr0 = smmu.getIDR0();
    uint32_t stall_model_bits = (idr0 >> 24u) & 0x3u;

    EXPECT_EQ(stall_model_bits, 0x01u)
        << "BUG-AUDIT-59: After setStallModel(0x01), IDR0 bits[25:24] (STALL_MODEL) "
           "must equal 0x01. The Rust equivalent uses Ordering::Relaxed for "
           "stall_model_.load() in get_idr0() — should use Ordering::Acquire to "
           "pair correctly with the Ordering::Release store in set_stall_model(). "
           "ARM §6.3.1. idr0=0x" << std::hex << idr0;
}

// BUG-AUDIT-59 C++ test 2: After setStallModel(0x01), getIDR5() STALL_MAX field
// bits[31:16] must be 0 (no stall entries when STALL_MODEL==0x01).
//
// ARM §6.3.6: STALL_MAX is 0 when STALL_MODEL==0x01 (terminate-only).
// This test documents the behavioral requirement.
TEST(BugAudit59, GetIdr5_StallMax_Zero_WhenTerminateOnly) {
    SMMU smmu;

    smmu.setStallModel(0x01u);  // terminate-only → STALL_MAX=0
    uint32_t idr5 = smmu.getIDR5();
    uint32_t stall_max = (idr5 >> 16u) & 0xFFFFu;

    EXPECT_EQ(stall_max, 0u)
        << "BUG-AUDIT-59: When stall_model==0x01 (terminate-only), IDR5 STALL_MAX "
           "bits[31:16] must be 0. The Rust fix changes Ordering::Relaxed to "
           "Ordering::Acquire in stall_model_.load() calls in get_idr0()/get_idr5(). "
           "ARM §6.3.6. idr5=0x" << std::hex << idr5;
}

// ─── BUG-AUDIT-60 tests — CR2 write must be ignored when SMMUEN=1 ─────────────

// BUG-AUDIT-60 test 1: setCR2() must be silently ignored when SMMUEN=1.
//
// ARM §6.3.12 lines 12578-12591: CR2 is effectively read-only when SMMUEN=1.
// Writes must be silently discarded.
//
// BEFORE FIX: getCR2() returns CR2_RECINVSID (write applied) → FAILS.
// AFTER FIX:  getCR2() returns 0 (write ignored) → PASSES.
TEST(BugAudit60, SetCR2_Ignored_When_SMMUEN1) {
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1

    // Attempt to write CR2_RECINVSID when SMMUEN=1. Must be ignored.
    smmu.setCR2(SMMU::CR2_RECINVSID);

    uint32_t cr2 = smmu.getCR2();
    EXPECT_EQ(cr2 & SMMU::CR2_RECINVSID, 0u)
        << "BUG-AUDIT-60: setCR2(CR2_RECINVSID) must be silently ignored when "
           "SMMUEN=1. ARM §6.3.12 lines 12578-12591: CR2 is RO when SMMUEN=1. "
           "Current code applies the write. getCR2()=0x" << std::hex << cr2;
}

// BUG-AUDIT-60 test 2: setCR2() with CR2_PTM must be silently ignored when SMMUEN=1.
//
// ARM §6.3.12: CR2.PTM (bit 2) write must be discarded when SMMUEN=1.
//
// BEFORE FIX: getCR2() returns CR2_PTM (write applied) → FAILS.
// AFTER FIX:  getCR2() returns 0 (write ignored) → PASSES.
TEST(BugAudit60, SetCR2_PTM_Ignored_When_SMMUEN1) {
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1

    smmu.setCR2(SMMU::CR2_PTM);

    uint32_t cr2 = smmu.getCR2();
    EXPECT_EQ(cr2 & SMMU::CR2_PTM, 0u)
        << "BUG-AUDIT-60: setCR2(CR2_PTM) must be silently ignored when "
           "SMMUEN=1. ARM §6.3.12. Current code applies the write. "
           "getCR2()=0x" << std::hex << cr2;
}

// BUG-AUDIT-60 test 3 (regression): setCR2() CAN be written when SMMUEN=0.
//
// CR2 is writable during SMMU configuration (before enabling).
// This must pass both before and after the fix.
//
// BEFORE / AFTER FIX: SMMUEN=0 → write is applied → getCR2() reflects write → PASSES.
TEST(BugAudit60, SetCR2_Accepted_When_SMMUEN0) {
    SMMU smmu;
    // Do NOT call enableSMMU(). SMMUEN=0 is the reset state.

    smmu.setCR2(SMMU::CR2_RECINVSID);
    uint32_t cr2 = smmu.getCR2();

    EXPECT_EQ(cr2 & SMMU::CR2_RECINVSID, SMMU::CR2_RECINVSID)
        << "BUG-AUDIT-60 regression: setCR2(CR2_RECINVSID) must succeed when SMMUEN=0. "
           "CR2 is only read-only when SMMUEN=1 (ARM §6.3.12). "
           "getCR2()=0x" << std::hex << cr2;
}

// BUG-AUDIT-60 test 4: After enabling the SMMU, a previous valid CR2 write is preserved,
// but subsequent writes are ignored.
//
// Flow: write CR2 → enable SMMU → write CR2 again → second write ignored.
//
// BEFORE FIX: second write updates CR2 → FAILS.
// AFTER FIX:  second write ignored → CR2 retains first value → PASSES.
TEST(BugAudit60, SetCR2_AfterEnable_Preserved_Then_Ignored) {
    SMMU smmu;

    // Step 1: write CR2 before enable (should succeed).
    smmu.setCR2(SMMU::CR2_RECINVSID);
    EXPECT_EQ(smmu.getCR2() & SMMU::CR2_RECINVSID, SMMU::CR2_RECINVSID)
        << "BUG-AUDIT-60 regression: pre-enable CR2 write must succeed.";

    // Step 2: enable SMMU.
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);

    // Step 3: attempt to write CR2 again with a different value (should be ignored).
    smmu.setCR2(SMMU::CR2_PTM);

    uint32_t cr2 = smmu.getCR2();
    EXPECT_EQ(cr2 & SMMU::CR2_PTM, 0u)
        << "BUG-AUDIT-60: After SMMUEN=1, setCR2(CR2_PTM) must be silently ignored. "
           "ARM §6.3.12. Current code applies the write. getCR2()=0x" << std::hex << cr2;
}

// ─── BUG-AUDIT-61 tests — CR1 write must be ignored when SMMUEN=1 ─────────────

// BUG-AUDIT-61 test 1: setCR1() must be silently ignored when SMMUEN=1.
//
// ARM §6.3.11: TABLE_* fields (bits[11:0]) are RO when SMMUEN=1.
// QUEUE_* fields (bits[5:0]) are RO when any queue is enabled.
// Writes must be silently discarded.
//
// BEFORE FIX: getCR1() returns the written value → FAILS.
// AFTER FIX:  getCR1() returns 0 (write ignored) → PASSES.
TEST(BugAudit61, SetCR1_Ignored_When_SMMUEN1) {
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1, CMDQEN=1, EVENTQEN=1

    // Write all table shareability/cache bits — must be ignored.
    const uint32_t cr1_all_table_bits = SMMU::CR1_TABLE_SH | SMMU::CR1_TABLE_OC | SMMU::CR1_TABLE_IC;
    smmu.setCR1(cr1_all_table_bits);

    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & cr1_all_table_bits, 0u)
        << "BUG-AUDIT-61: setCR1() TABLE_* bits must be silently ignored when SMMUEN=1. "
           "ARM §6.3.11: TABLE_* fields are RO when SMMUEN=1. "
           "Current code applies the write. getCR1()=0x" << std::hex << cr1;
}

// BUG-AUDIT-61 test 2: setCR1() with value 0x3F must be silently ignored when SMMUEN=1.
//
// 0x3F covers bits[5:0] (queue cache/shareability bits) which are also RO when
// any queue is enabled (CMDQEN=1 or EVENTQEN=1).
//
// BEFORE FIX: getCR1() returns 0x3F (or subset) → FAILS.
// AFTER FIX:  getCR1() returns 0 → PASSES.
TEST(BugAudit61, SetCR1_0x3F_Ignored_When_SMMUEN1) {
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1

    smmu.setCR1(0x3Fu);

    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1, 0u)
        << "BUG-AUDIT-61: setCR1(0x3F) must be silently ignored when SMMUEN=1. "
           "ARM §6.3.11. Current code applies the write. getCR1()=0x" << std::hex << cr1;
}

// BUG-AUDIT-61 test 3 (regression): setCR1() CAN be written when SMMUEN=0 and
// queues are disabled.
//
// CR1 is writable during SMMU configuration (before enabling).
// This must pass both before and after the fix.
//
// BEFORE / AFTER FIX: SMMUEN=0 + queues disabled → write applied → PASSES.
TEST(BugAudit61, SetCR1_Accepted_When_SMMUEN0_QueuesDisabled) {
    SMMU smmu;
    // Do NOT call enableSMMU(). SMMUEN=0, no queues enabled — fully writable.

    const uint32_t cr1_val = SMMU::CR1_TABLE_SH | SMMU::CR1_TABLE_OC;
    smmu.setCR1(cr1_val);

    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & cr1_val, cr1_val)
        << "BUG-AUDIT-61 regression: setCR1() must succeed when SMMUEN=0 and no queues "
           "are enabled. ARM §6.3.11. getCR1()=0x" << std::hex << cr1;
}

// BUG-AUDIT-61 test 4: Queue-only bits in CR1 must be ignored when queue is enabled
// even if SMMUEN=0.
//
// ARM §6.3.11: QUEUE_* fields (bits[5:0]) are RO when any queue is enabled (CMDQEN or
// EVENTQEN). This tests the queue-enabled guard independent of SMMUEN.
//
// BEFORE FIX: getCR1() reflects the written queue bits → FAILS.
// AFTER FIX:  getCR1() returns 0 for queue bits when queue is enabled → PASSES.
TEST(BugAudit61, SetCR1_QueueBits_Ignored_When_QueueEnabled) {
    SMMU smmu;
    // Enable queues only — SMMUEN remains 0.
    smmu.setCR0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);

    // QUEUE_* bits are bits[5:0]: CR1_QUEUE_SH(bits[5:4]) | CR1_QUEUE_OC(bits[3:2]) |
    // CR1_QUEUE_IC(bits[1:0]).
    const uint32_t cr1_queue_bits = SMMU::CR1_QUEUE_SH | SMMU::CR1_QUEUE_OC | SMMU::CR1_QUEUE_IC;
    smmu.setCR1(cr1_queue_bits);

    uint32_t cr1 = smmu.getCR1();
    EXPECT_EQ(cr1 & cr1_queue_bits, 0u)
        << "BUG-AUDIT-61: CR1 QUEUE_* bits must be silently ignored when any queue is "
           "enabled (ARM §6.3.11), even if SMMUEN=0. Current code applies the write. "
           "getCR1()=0x" << std::hex << cr1;
}

// ─── BUG-AUDIT-62 tests — gatosTranslate() uses smmuen_ shadow vs cr0_ ────────
//
// BUG-AUDIT-62 is a code consistency/maintenance bug. The shadow bool smmuen_ is
// always kept in sync with cr0_ by setCR0(). These tests verify the required
// functional behavior (the code path that uses smmuen_ must behave identically to
// what cr0_ & CR0_SMMUEN would produce). They pass today because smmuen_ is correct.
//
// The actual fix is to change the read in gatosTranslate() from smmuen_.load() to
// (cr0_.load() & CR0_SMMUEN) for consistency with translate(). See smmu.cpp ~line 3624.

// BUG-AUDIT-62 test 1: gatosTranslate() must return FAULT=1+FAULTCODE=0xFD when SMMUEN=0.
//
// ARM §9.1 + BUG-AUDIT-39: If SMMUEN=0, gatosTranslate() must return FAULT=1 with
// FAULTCODE=0xFD (INTERNAL_ERR). This behavior is gated on smmuen_ in current code.
//
// NOTE: This test PASSES currently (smmuen_ is correctly maintained).
// It documents the BUG-AUDIT-62 code path consistency requirement.
TEST(BugAudit62, GatosTranslate_SMMUEN0_ReturnsFault) {
    SMMU smmu;
    // SMMUEN=0 is the reset state. Do not call enableSMMU().

    // Even without a configured stream, SMMUEN=0 must cause FAULT immediately.
    uint64_t par = smmu.gatosTranslate(999u, 0u, 0x1000u,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(par & 1u, 1u)
        << "BUG-AUDIT-62: gatosTranslate() must return FAULT=1 when SMMUEN=0. "
           "ARM §9.1 + BUG-AUDIT-39. The guard uses smmuen_ shadow bool rather than "
           "cr0_ & CR0_SMMUEN for consistency with translate(). par=0x"
           << std::hex << par;

    // FAULTCODE must be 0xFD (INTERNAL_ERR) per BUG-AUDIT-39 / §9.1.
    uint64_t faultcode = (par >> 4u) & 0xFFu;
    EXPECT_EQ(faultcode, 0xFDu)
        << "BUG-AUDIT-62: When SMMUEN=0, gatosTranslate() FAULTCODE must be 0xFD "
           "(INTERNAL_ERR). ARM §9.1. par=0x" << std::hex << par;
}

// BUG-AUDIT-62 test 2: gatosTranslate() must return FAULT=0 for a valid translation
// when SMMUEN=1.
//
// ARM §9.1: When SMMUEN=1 and the translation succeeds, FAULT bit must be 0.
// This verifies the smmuen_ guard enables the translation path.
//
// NOTE: This test PASSES currently. Included to document the BUG-AUDIT-62 code path.
TEST(BugAudit62, GatosTranslate_SMMUEN1_ReturnsValid) {
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1

    const StreamID sid  = 700u;
    const IOVA     iova = 0x1000u;
    const PA       pa   = 0xA000u;

    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.securityState      = SecurityState::NonSecure;
    cfg.t0sz               = 0u;  // sentinel
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, 0u).isOk());

    PagePermissions ro(true, false, false);
    ASSERT_TRUE(smmu.mapPage(sid, 0u, iova, pa, ro, SecurityState::NonSecure).isOk());

    uint64_t par = smmu.gatosTranslate(sid, 0u, iova,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(par & 1u, 0u)
        << "BUG-AUDIT-62: gatosTranslate() must return FAULT=0 for a valid translation "
           "when SMMUEN=1. ARM §9.1. The smmuen_ shadow bool used in the guard must "
           "match the actual cr0_ & CR0_SMMUEN value for code consistency. "
           "par=0x" << std::hex << par;
}

// BUG-AUDIT-62 test 3: After setCR0() disables SMMUEN, gatosTranslate() must revert
// to FAULT=1+FAULTCODE=0xFD (verifying smmuen_ is kept in sync with cr0_).
//
// NOTE: This test PASSES currently (shadow bool is correctly cleared on disable).
// It documents the synchronization requirement that justifies the code consistency fix.
TEST(BugAudit62, GatosTranslate_AfterDisable_ReturnsFault) {
    SMMU smmu;
    enableSMMU(smmu);  // SMMUEN=1

    // Disable SMMU (SMMUEN=0).
    smmu.setCR0(0u);

    uint64_t par = smmu.gatosTranslate(999u, 0u, 0x2000u,
                                        AccessType::Read,
                                        SecurityState::NonSecure);

    EXPECT_EQ(par & 1u, 1u)
        << "BUG-AUDIT-62: After setCR0(0) (SMMUEN=0), gatosTranslate() must return "
           "FAULT=1. ARM §9.1. smmuen_ shadow bool must track cr0_ & CR0_SMMUEN for "
           "consistency. par=0x" << std::hex << par;
}
