// TDD failing tests for AUDIT-NEW-02 and AUDIT-NEW-05 regression.
//
// Each test is written to FAIL with the current code (red) and PASS only after
// the corresponding fix is applied (green), EXCEPT where explicitly marked as a
// regression/baseline test (which passes both before and after).
//
// ─────────────────────────────────────────────────────────────────────────────
// NOTE on AUDIT-NEW-01 (Rust-only): No C++ test — finding is Rust-only.
//
// NOTE on AUDIT-NEW-03 (C++ IDR0.PRI not gated on ATS==1):
//   Since IDR0.ATS is hardcoded to 1 in this C++ model (bit 10 always set),
//   gating IDR0.PRI on ats_bit_set would never change observable behaviour.
//   Therefore no failing test is written for AUDIT-NEW-03.
//
// NOTE on AUDIT-NEW-04 (Rust-only / already correct in C++): No C++ test —
//   the C++ CFGI_CD / CFGI_CD_ALL stage-1 guard was fixed in commit 3e02bbb
//   and is already covered by test_bugs_new16.cpp.
// ─────────────────────────────────────────────────────────────────────────────
//
// ─────────────────────────────────────────────────────────────────────────────
// AUDIT-NEW-02 (C++ §6.3.1): IDR0.NS1ATS (bit 11) not gated on S2P
//
//   ARM IHI0070G.b §6.3.1 SMMU_IDR0, field NS1ATS (bit 11):
//     "RES0 when SMMU_IDR0.S2P==0."
//
//   Current C++ getIDR0() at smmu.cpp line 3260:
//     | (ns1atsSupported_.load(std::memory_order_acquire) ? (1u << 11) : 0u)
//
//   Bug: bit 11 is set unconditionally based on ns1atsSupported_ without
//   checking s2pSupported_.  When S2P==0, NS1ATS MUST be RES0 (zero).
//
//   Fix required: gate NS1ATS on s2pSupported_ as well:
//     | ((ns1atsSupported_.load(...) && s2pSupported_.load(...)) ? (1u << 11) : 0u)
//
//   Two tests:
//     Test 1 (regression): NS1ATS=1, S2P=1 (default) → bit 11 IS set.
//                          PASSES both before and after fix.
//     Test 2 (failing):    NS1ATS=1, S2P=0 → bit 11 MUST be zero.
//                          FAILS before fix (bit 11 still set); PASSES after fix.
//
// ─────────────────────────────────────────────────────────────────────────────
// AUDIT-NEW-05 (C++ §7.3.12 regression): injectWalkEabt() extended signature
//
//   ARM IHI0070G.b §7.3.12 line 27078: F_WALK_EABT can arise in three contexts:
//     1. Single-stage walk:         isStage2=false, eventClass=1 (CLASS=TT).
//     2. Two-stage TT descriptor:   isStage2=true,  eventClass=1 (CLASS=TT).
//     3. Two-stage IPA input:       isStage2=true,  eventClass=2 (CLASS=IN).
//
//   The extended API was added in commit 3e02bbb.  This test file adds a
//   regression test (Test 3) confirming that the existing call with
//   isStage2=false and eventClass=1 still works correctly after any future
//   modifications.
//
//   This test always passes (baseline).
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

} // anonymous namespace

// ============================================================================
// AUDIT-NEW-02: IDR0.NS1ATS must be RES0 when IDR0.S2P==0 (ARM §6.3.1)
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 (regression/baseline): NS1ATS=1 with S2P=1 (default) → bit 11 SET
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1: NS1ATS (bit 11) is set when both NS1ATS is advertised and S2P=1.
// S2P defaults to true, so calling setNS1ATSSupported(true) alone should be
// sufficient to expose bit 11.
//
// BEFORE FIX: bit 11 is set — PASSES (no regression introduced by fix).
// AFTER FIX:  bit 11 is still set (S2P=1 is the default) — PASSES.
TEST(AuditNew02Idr0Ns1ats, NS1ATS_Visible_When_Both_Enabled) {
    // §6.3.1: NS1ATS=1, S2P=1 (default) → IDR0 bit 11 must be set.
    // This is the regression guard: the fix must NOT break the case where
    // both NS1ATS and S2P are enabled.
    SMMU smmu;
    enableSMMU(smmu);

    // S2P is true by default — do NOT call setS2PSupported() here.
    smmu.setNS1ATSSupported(true);

    const uint32_t idr0    = smmu.getIDR0();
    const uint32_t ns1ats  = (idr0 >> 11u) & 1u;

    EXPECT_EQ(ns1ats, 1u)
        << "AUDIT-NEW-02 baseline: IDR0.NS1ATS (bit 11) must be 1 when "
           "setNS1ATSSupported(true) is called AND S2P is enabled (default). "
           "ARM §6.3.1: NS1ATS is RES0 only when S2P==0; S2P is currently true. "
           "IDR0=0x" << std::hex << idr0;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 (failing): NS1ATS=1 with S2P=0 → bit 11 MUST be zero (RES0)
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §6.3.1 SMMU_IDR0 bit 11 (NS1ATS):
//   "RES0 when SMMU_IDR0.S2P==0."
//
// When S2P is disabled via setS2PSupported(false), NS1ATS MUST read as zero
// regardless of what setNS1ATSSupported() was called with.
//
// BEFORE FIX: current getIDR0() line 3260 checks only ns1atsSupported_, so
//             bit 11 is still set even though S2P=0 → FAILS.
// AFTER FIX:  NS1ATS gated on s2pSupported_ as well → bit 11 is zero → PASSES.
TEST(AuditNew02Idr0Ns1ats, NS1ATS_RES0_When_S2P_Disabled) {
    // §6.3.1: NS1ATS (bit 11) must be RES0 when S2P==0.
    // Calling setS2PSupported(false) disables stage-2 support; NS1ATS must
    // track it since NS1ATS is only meaningful when stage-2 is present.
    SMMU smmu;
    enableSMMU(smmu);

    // Enable NS1ATS feature in the model's internal state.
    smmu.setNS1ATSSupported(true);

    // Now disable S2P.  Per ARM §6.3.1, NS1ATS becomes RES0.
    smmu.setS2PSupported(false);

    const uint32_t idr0    = smmu.getIDR0();
    const uint32_t ns1ats  = (idr0 >> 11u) & 1u;
    const uint32_t s2p_bit = (idr0 >> 0u)  & 1u;

    // Verify S2P is indeed 0 (sanity check that setS2PSupported(false) worked).
    ASSERT_EQ(s2p_bit, 0u)
        << "AUDIT-NEW-02 prerequisite: IDR0.S2P (bit 0) must be 0 after "
           "setS2PSupported(false). IDR0=0x" << std::hex << idr0;

    EXPECT_EQ(ns1ats, 0u)
        << "AUDIT-NEW-02: IDR0.NS1ATS (bit 11) must be RES0 (zero) when "
           "IDR0.S2P==0, per ARM §6.3.1: "
           "'RES0 when SMMU_IDR0.S2P==0.' "
           "Current behavior (WRONG): getIDR0() at smmu.cpp line 3260 sets "
           "bit 11 based solely on ns1atsSupported_ without checking "
           "s2pSupported_, so bit 11 remains 1 even when S2P=0. "
           "Expected: IDR0 bit 11 == 0.  "
           "Got IDR0=0x" << std::hex << idr0
        << " (NS1ATS bit=" << std::dec << ns1ats << ").";
}

// ============================================================================
// AUDIT-NEW-05 regression: injectWalkEabt() with isStage2=false, eventClass=1
// (ARM §7.3.12 line 27078) — always passes; guards against future regression
// ============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 (regression/baseline): single-stage walk abort → s2=false, eventClass=1
// ─────────────────────────────────────────────────────────────────────────────
//
// ARM §7.3.12: A single-stage page-table walk abort produces F_WALK_EABT with
// CLASS=TT (eventClass=1) and S2=0.
//
// The extended API signature:
//   injectWalkEabt(streamID, pasid, iova, isStage2=false, eventClass=1,
//                  SecurityState::NonSecure)
// was introduced in commit 3e02bbb.  This regression test confirms that the
// baseline (isStage2=false, eventClass=1) path continues to work correctly.
//
// BEFORE FIX: already passes (existing behaviour preserved by default params).
// AFTER FIX:  must still pass (regression guard).
TEST(AuditNew05WalkEabtRegression, SingleStageWalk_IsStage2False_EventClass1) {
    // §7.3.12: single-stage walk abort baseline — isStage2=false, eventClass=1.
    // Verifies the extended API still produces the correct event fields.
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid   = 0x80u;
    const PASID    pasid = 0u;
    const IOVA     iova  = 0x4000u;

    // Call the extended API with the original single-stage parameters.
    smmu.injectWalkEabt(sid, pasid, iova,
                        /*isStage2=*/false, /*eventClass=*/1u,
                        SecurityState::NonSecure);

    std::vector<EventEntry> events = smmu.getEventQueue();
    ASSERT_FALSE(events.empty())
        << "AUDIT-NEW-05 regression: at least one event must be enqueued after "
           "injectWalkEabt(isStage2=false, eventClass=1, NonSecure)";

    const EventEntry* ev = nullptr;
    for (const auto& e : events) {
        if (e.type == EventType::F_WALK_EABT) {
            ev = &e;
            break;
        }
    }
    ASSERT_NE(ev, nullptr)
        << "AUDIT-NEW-05 regression: F_WALK_EABT event not found in queue after "
           "injectWalkEabt(isStage2=false, eventClass=1)";

    EXPECT_FALSE(ev->s2)
        << "AUDIT-NEW-05 regression: single-stage walk abort must have s2=false "
           "(ARM §7.3.12: S2 flag is false for single-stage walk faults). "
           "Got s2=" << ev->s2;

    EXPECT_EQ(ev->eventClass, 1u)
        << "AUDIT-NEW-05 regression: single-stage walk abort must have "
           "eventClass=1 (CLASS=TT per ARM §7.3.12). "
           "Got eventClass=" << static_cast<unsigned>(ev->eventClass);
}
