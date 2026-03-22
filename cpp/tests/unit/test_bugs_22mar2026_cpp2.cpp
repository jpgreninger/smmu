// Tests for BUG-CPP-2 confirmed on 2026-03-22.
// TDD: all tests are written to FAIL before the fix is applied.
//
// BUG-CPP-2 (generateEvent() InputAddr/SubstreamID zeroing):
//   In generateEvent(), event.address and event.pasid are assigned unconditionally
//   for ALL event types from the function parameters.  Four event types have no
//   InputAddr or SubstreamID fields in their ARM §7.3 wire format — bits that
//   carry those values are RES0 per §7.3 catch-all:
//
//   - C_BAD_STREAMID (0x02): §7.3.3 — no InputAddr; bits [191:64] RES0 except
//     StreamID[63:32].
//   - C_BAD_STE (0x04): §7.3.5 — no InputAddr; bits [191:64] RES0 except
//     StreamID[63:32].
//   - F_STREAM_DISABLED (0x06): §7.3.7 — no InputAddr, no SubstreamID, no SSV;
//     bits [191:32] ALL RES0 except StreamID[63:32].
//   - C_BAD_CD (0x0A): §7.3.11 — no InputAddr; bits [191:64] RES0 except
//     StreamID[63:32].  (SubstreamID and SSV ARE defined at [31:11].)
//
//   Note: C_BAD_SUBSTREAMID (0x08) DOES define InputAddr per §7.3.9 — do NOT zero.
//
//   The bug exists in BOTH the normal path (~line 4884) and the stall-pending
//   path (~line 4736) of generateEvent() in cpp/src/smmu/smmu.cpp.
//
// Fix (applied after tests are confirmed failing):
//   After the unconditional pasid/address assignments in both paths, add an
//   override block that zeros address and pasid for the four affected types.
//   C_BAD_SUBSTREAMID is explicitly excluded from the override.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/configuration.h"
#include <cstdint>
#include <vector>

using namespace smmu;

// ============================================================================
// Helper utilities
// ============================================================================

namespace {

static void enableSMMU(SMMU& s) {
    s.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Find the first event of the given type in the queue, or nullptr.
static const EventEntry* findEvent(const std::vector<EventEntry>& events, EventType target) {
    for (const auto& ev : events) {
        if (ev.type == target) {
            return &ev;
        }
    }
    return nullptr;
}

} // namespace

// ============================================================================
// TEST 1: C_BAD_STREAMID — event.address must be 0.
//
// ARM IHI0070G.b §7.3.3: bits [191:64] are RES0 except StreamID[63:32].
// InputAddr lives in bits [191:128] — it is entirely within the RES0 region.
//
// Setup: configure SMMU with LOG2SIZE=2 (4 streams max), enable RECINVSID so
//        C_BAD_STREAMID events are recorded.  Translate with a StreamID that is
//        outside the table (>= 2^LOG2SIZE) and a non-zero IOVA.
//        Assert event.address == 0.
//
// BEFORE FIX: event.address = iova (non-zero IOVA leaks into the RES0 field).
// AFTER FIX:  override block zeros address; assert passes.
// ============================================================================
TEST(BugCpp22Mar2026Cpp2, CBadStreamIdAddressIsZero) {
    SMMU smmu;
    enableSMMU(smmu);

    // Enable RECINVSID so C_BAD_STREAMID events are written to the event queue.
    smmu.setCR2(SMMU::CR2_RECINVSID);

    // Configure SMMU with LOG2SIZE=2 (table covers StreamIDs 0..3).
    smmu.setStrtabLog2Size(2u);

    // StreamID 0x10 is outside the 4-entry table — produces C_BAD_STREAMID.
    const StreamID sid = 0x10u;
    const uint64_t nonZeroIova = 0xDEAD000000u;

    TranslationResult result = smmu.translate(
        sid, 0, nonZeroIova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "Out-of-range StreamID must produce a translation error";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::C_BAD_STREAMID);
    ASSERT_NE(ev, nullptr)
        << "C_BAD_STREAMID event must be present (RECINVSID=1, out-of-range StreamID)";

    // ARM §7.3.3: InputAddr is RES0; event.address must be 0.
    EXPECT_EQ(ev->address, 0u)
        << "BUG-CPP-2: C_BAD_STREAMID.address must be 0 (RES0 per §7.3.3);"
           " got 0x" << std::hex << ev->address
        << " (leaked IOVA 0x" << nonZeroIova << ")";
}

// ============================================================================
// TEST 2: C_BAD_STE — event.address must be 0.
//
// ARM IHI0070G.b §7.3.5: bits [191:64] are RES0 except StreamID[63:32].
// InputAddr is entirely in the RES0 region.
//
// Setup: do NOT call configureStream for a given StreamID.  Translating that
//        StreamID finds no entry in streamMap (equivalent to STE.V=0) and
//        emits C_BAD_STE.  Use a non-zero IOVA.
//        Assert event.address == 0.
//
// BEFORE FIX: event.address = iova (non-zero IOVA leaks into the RES0 field).
// AFTER FIX:  override block zeros address; assert passes.
// ============================================================================
TEST(BugCpp22Mar2026Cpp2, CBadSteAddressIsZero) {
    SMMU smmu;
    enableSMMU(smmu);

    // StreamID 5 is within the default table range but has no configured stream
    // (STE.V=0 equivalent) → C_BAD_STE.
    const StreamID sid = 5u;
    const uint64_t nonZeroIova = 0xBEEF000000u;

    TranslationResult result = smmu.translate(
        sid, 0, nonZeroIova, AccessType::Write, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "Unconfigured StreamID must produce a translation error (C_BAD_STE)";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::C_BAD_STE);
    ASSERT_NE(ev, nullptr)
        << "C_BAD_STE event must be present for unconfigured stream (STE.V=0)";

    // ARM §7.3.5: InputAddr is RES0; event.address must be 0.
    EXPECT_EQ(ev->address, 0u)
        << "BUG-CPP-2: C_BAD_STE.address must be 0 (RES0 per §7.3.5);"
           " got 0x" << std::hex << ev->address
        << " (leaked IOVA 0x" << nonZeroIova << ")";
}

// ============================================================================
// TEST 3: C_BAD_CD — event.address must be 0.
//
// ARM IHI0070G.b §7.3.11: bits [191:64] are RES0 except StreamID[63:32].
// InputAddr is entirely within the RES0 region.
//
// Setup: configure a stream with aa64=false (AArch32 LPAE mode).  The SMMU
//        models only AArch64; aa64=false causes C_BAD_CD immediately.
//        Use a non-zero IOVA and assert event.address == 0.
//
// BEFORE FIX: event.address = iova (non-zero IOVA leaks into the RES0 field).
// AFTER FIX:  override block zeros address; assert passes.
// ============================================================================
TEST(BugCpp22Mar2026Cpp2, CBadCdAddressIsZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 7u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 16u;
    cfg.aa64               = false; // AArch32 LPAE — not supported → C_BAD_CD
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);

    const uint64_t nonZeroIova = 0xCAFE000000u;

    TranslationResult result = smmu.translate(
        sid, 0, nonZeroIova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "AArch32 (aa64=false) CD must produce C_BAD_CD";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::C_BAD_CD);
    ASSERT_NE(ev, nullptr)
        << "C_BAD_CD event must be present (aa64=false → AArch32 unsupported)";

    // ARM §7.3.11: InputAddr is RES0; event.address must be 0.
    EXPECT_EQ(ev->address, 0u)
        << "BUG-CPP-2: C_BAD_CD.address must be 0 (RES0 per §7.3.11);"
           " got 0x" << std::hex << ev->address
        << " (leaked IOVA 0x" << nonZeroIova << ")";
}

// ============================================================================
// TEST 4: F_STREAM_DISABLED — event.address must be 0, event.pasid must be 0.
//
// ARM IHI0070G.b §7.3.7: bits [191:32] are ALL RES0 except StreamID[63:32].
// This means both InputAddr (bits [191:128]) and SubstreamID/SSV (bits [31:11])
// are RES0.
//
// Setup: configure a substream-capable stream (s1cdMax > 0) with S1DSS=0x00
//        (abort non-substream transactions).  Translate with pasid=0 (no PASID
//        presented) and a non-zero IOVA.
//        Assert event.address == 0 and event.pasid == 0.
//
// BEFORE FIX: event.address = iova (non-zero) and event.pasid may be non-zero.
// AFTER FIX:  override block zeros both; asserts pass.
// ============================================================================
TEST(BugCpp22Mar2026Cpp2, FStreamDisabledAddressAndPasidAreZero) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 9u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 16u;
    cfg.s1cdMax            = 4u;   // substream-capable (S1CDMax > 0)
    cfg.s1dss              = 0x00u; // S1DSS=0b00 → abort PASID=0 with F_STREAM_DISABLED
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);

    const uint64_t nonZeroIova = 0xF00D000000u;
    // PASID=0 on a substream-capable stream with S1DSS=0 → F_STREAM_DISABLED.
    TranslationResult result = smmu.translate(
        sid, 0, nonZeroIova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "PASID=0 on S1DSS=0 substream-capable stream must produce F_STREAM_DISABLED";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::F_STREAM_DISABLED);
    ASSERT_NE(ev, nullptr)
        << "F_STREAM_DISABLED event must be present (s1cdMax>0, s1dss=0, pasid=0)";

    // ARM §7.3.7: all bits [191:32] are RES0 except StreamID[63:32].
    // InputAddr and SubstreamID are both in the RES0 region.
    EXPECT_EQ(ev->address, 0u)
        << "BUG-CPP-2: F_STREAM_DISABLED.address must be 0 (RES0 per §7.3.7);"
           " got 0x" << std::hex << ev->address
        << " (leaked IOVA 0x" << nonZeroIova << ")";
    EXPECT_EQ(ev->pasid, 0u)
        << "BUG-CPP-2: F_STREAM_DISABLED.pasid must be 0 (RES0 per §7.3.7)";
}

// ============================================================================
// TEST 5: C_BAD_SUBSTREAMID — event.address must NOT be zeroed (regression guard).
//
// ARM IHI0070G.b §7.3.9: InputAddr IS defined for C_BAD_SUBSTREAMID.
// The address field must carry the IOVA of the failing transaction.
//
// Setup: configure a stream with s1cdMax=2 (supports PASID 0..3).  Translate
//        with PASID=4 — this exceeds the SubstreamID capacity (>= 2^s1cdMax)
//        and generates C_BAD_SUBSTREAMID.  Assert event.address == nonZeroIova
//        (i.e. the address was NOT incorrectly zeroed by the BUG-CPP-2 fix).
//
// This test must pass BOTH before and after the BUG-CPP-2 fix, confirming
// that the zeroing override does not accidentally touch C_BAD_SUBSTREAMID.
// ============================================================================
TEST(BugCpp22Mar2026Cpp2, CBadSubstreamIdAddressPreserved) {
    SMMU smmu;
    enableSMMU(smmu);

    const StreamID sid = 11u;
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 16u;
    cfg.s1cdMax            = 2u; // supports PASID 0..3; PASID>=4 → C_BAD_SUBSTREAMID
    smmu.configureStream(sid, cfg);
    smmu.enableStream(sid);
    smmu.createStreamPASID(sid, 0);

    const uint64_t nonZeroIova = 0xABCD000000u;
    const PASID    nonZeroPasid = 4u; // >= 2^s1cdMax (2^2=4) → C_BAD_SUBSTREAMID

    TranslationResult result = smmu.translate(
        sid, nonZeroPasid, nonZeroIova, AccessType::Read, SecurityState::NonSecure);
    EXPECT_TRUE(result.isError())
        << "PASID >= 2^s1cdMax must produce C_BAD_SUBSTREAMID";

    std::vector<EventEntry> events = smmu.getEventQueue();
    const EventEntry* ev = findEvent(events, EventType::C_BAD_SUBSTREAMID);
    ASSERT_NE(ev, nullptr)
        << "C_BAD_SUBSTREAMID event must be present (PASID=4 >= 2^s1cdMax=4)";

    // ARM §7.3.9: InputAddr IS defined; address must carry the IOVA.
    EXPECT_EQ(ev->address, nonZeroIova)
        << "Regression guard: C_BAD_SUBSTREAMID.address must carry the IOVA (§7.3.9);"
           " got 0x" << std::hex << ev->address
        << " expected 0x" << nonZeroIova;
}
