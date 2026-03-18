// ARM SMMU v3 Conformance Gap Tests: EventEntry.nsipa field correctness
//
// ARM IHI0070G.b §7.3: NSIPA — "The IPA is Non-Secure." This field is valid
// when S2==1 (a stage-2 fault or a fault during a stage-2 table walk). NSIPA
// must be 1 when the IPA addresses Non-Secure PA space, i.e. when the stream
// is NonSecure (securityState == SecurityState::NonSecure) AND the fault
// involved stage-2 (isStage2 == true).
//
// Tests:
//   nsipa_stage2_nonsecure_stream_has_nsipa_true:
//     Two-stage NonSecure stream with no stage-2 mapping → stage-2 F_TRANSLATION
//     fault → EventEntry.nsipa must be true.
//
//   nsipa_stage1_only_fault_has_nsipa_false:
//     Stage-1-only NonSecure stream with no mapping → F_TRANSLATION fault with
//     s2=false → EventEntry.nsipa must be false.
//
//   nsipa_stage2_secure_stream_has_nsipa_false:
//     Two-stage Secure stream with no stage-2 mapping → stage-2 F_TRANSLATION
//     fault → EventEntry.nsipa must be false (IPA is in Secure PA space).
//
// pnu and UWXN are already correctly implemented in C++ via the
// AccessType::ReadPrivileged/ExecutePrivileged variants and the WXN check.
// Only nsipa is fixed here.

#include <gtest/gtest.h>
#include "smmu/smmu.h"
#include "smmu/types.h"
#include "smmu/address_space.h"
#include <memory>

using namespace smmu;

namespace {

static constexpr StreamID SID_NS2  = 0xD0u; // NonSecure two-stage
static constexpr StreamID SID_NS1  = 0xD1u; // NonSecure stage-1-only
static constexpr StreamID SID_SEC2 = 0xD2u; // Secure two-stage
static constexpr PASID    PID      = 0u;

void enableSMMU(SMMU& smmu) {
    smmu.setCR0(SMMU::CR0_SMMUEN | SMMU::CR0_EVENTQEN | SMMU::CR0_CMDQEN | SMMU::CR0_PRIQEN);
}

// Set up a stage-1-only NonSecure stream (default security state).
void setupStage1OnlyStream(SMMU& smmu, StreamID sid) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = false;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0; // no VA range restriction
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());
}

// Set up a NonSecure two-stage stream. Stage-2 is assigned an empty address
// space so that any IPA output from stage-1 will fail stage-2 lookup.
void setupNonSecureTwoStageStream(SMMU& smmu, StreamID sid,
                                  std::shared_ptr<AddressSpace>& s2as) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5; // 48-bit S2PS
    // securityState defaults to NonSecure in StreamConfig constructor
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());

    s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
}

// Set up a Secure two-stage stream. Stage-2 is assigned an empty address
// space so that any IPA output from stage-1 will fail stage-2 lookup.
void setupSecureTwoStageStream(SMMU& smmu, StreamID sid,
                               std::shared_ptr<AddressSpace>& s2as) {
    StreamConfig cfg;
    cfg.translationEnabled = true;
    cfg.stage1Enabled      = true;
    cfg.stage2Enabled      = true;
    cfg.faultMode          = FaultMode::Terminate;
    cfg.t0sz               = 0;
    cfg.s2t0sz             = 0;
    cfg.s2ps               = 5; // 48-bit S2PS
    cfg.securityState      = SecurityState::Secure;
    ASSERT_TRUE(smmu.configureStream(sid, cfg).isOk());
    ASSERT_TRUE(smmu.enableStream(sid).isOk());
    ASSERT_TRUE(smmu.createStreamPASID(sid, PID).isOk());

    s2as = std::make_shared<AddressSpace>();
    ASSERT_TRUE(smmu.setStreamStage2AddressSpace(sid, s2as).isOk());
}

// Map a stage-1 page (IOVA → PA used as IPA) in NonSecure space.
void mapNSPage(SMMU& smmu, StreamID sid, IOVA iova, PA pa) {
    PagePermissions perms;
    perms.read    = true;
    perms.write   = true;
    perms.execute = false;
    ASSERT_TRUE(smmu.mapPage(sid, PID, iova, pa, perms).isOk());
}

// Map a stage-1 page in Secure space.
void mapSecurePage(SMMU& smmu, StreamID sid, IOVA iova, PA pa) {
    PagePermissions perms;
    perms.read    = true;
    perms.write   = true;
    perms.execute = false;
    ASSERT_TRUE(smmu.mapPage(sid, PID, iova, pa, perms, SecurityState::Secure).isOk());
}

// Return the most-recent EventEntry of the given type from the event queue,
// or a default-constructed one if not found (nsipa=false by default).
EventEntry findLastEvent(const SMMU& smmu, EventType et) {
    const auto& q = smmu.getEventQueue();
    for (auto it = q.rbegin(); it != q.rend(); ++it) {
        if (it->type == et) {
            return *it;
        }
    }
    return EventEntry(); // default: nsipa=false
}

} // anonymous namespace

// ============================================================================
// Test 1: NonSecure two-stage stream → stage-2 fault → nsipa must be true
//
// §7.3: NSIPA=1 when S2=1 AND the IPA is in Non-Secure PA space.
// For a NonSecure stream, the IPA is always Non-Secure, so nsipa=true.
//
// BEFORE FIX: nsipa is hard-coded to false → test FAILS.
// AFTER FIX:  nsipa = (isStage2 && securityState == NonSecure) → test PASSES.
// ============================================================================
TEST(ConfGapNsipa, nsipa_stage2_nonsecure_stream_has_nsipa_true) {
    SMMU smmu;
    enableSMMU(smmu);

    // Build a NonSecure two-stage stream with no stage-2 mappings.
    std::shared_ptr<AddressSpace> s2as;
    setupNonSecureTwoStageStream(smmu, SID_NS2, s2as);

    // Map stage-1 IOVA → IPA; stage-2 has no IPA mapping → stage-2 F_TRANSLATION.
    static constexpr IOVA IOVA_VAL = 0x1000ULL;
    static constexpr PA   IPA_VAL  = 0x8000'0000ULL;
    mapNSPage(smmu, SID_NS2, IOVA_VAL, IPA_VAL);

    smmu.translate(SID_NS2, PID, IOVA_VAL, AccessType::Read);

    EventEntry ev = findLastEvent(smmu, EventType::F_TRANSLATION);
    EXPECT_TRUE(ev.s2)
        << "stage-2 fault must have s2=true in EventEntry";
    EXPECT_TRUE(ev.nsipa)
        << "NSIPA must be true for NonSecure two-stage fault (§7.3). "
           "Before fix, nsipa was always hard-coded to false.";
}

// ============================================================================
// Test 2: Stage-1-only NonSecure stream fault → nsipa must be false
//
// s2=false on a stage-1-only fault, so NSIPA must be 0 per §7.3.
// This is a regression guard: must pass before and after the fix.
// ============================================================================
TEST(ConfGapNsipa, nsipa_stage1_only_fault_has_nsipa_false) {
    SMMU smmu;
    enableSMMU(smmu);

    setupStage1OnlyStream(smmu, SID_NS1);
    // No mapping → F_TRANSLATION, s2=false → nsipa=false
    smmu.translate(SID_NS1, PID, 0x2000ULL, AccessType::Read);

    EventEntry ev = findLastEvent(smmu, EventType::F_TRANSLATION);
    EXPECT_FALSE(ev.s2)
        << "stage-1-only fault must have s2=false in EventEntry";
    EXPECT_FALSE(ev.nsipa)
        << "NSIPA must be false for stage-1-only fault (s2=false, §7.3)";
}

// ============================================================================
// Test 3: Secure two-stage stream → stage-2 fault → nsipa must be false
//
// §7.3: NSIPA refers specifically to Non-Secure PA space. For a Secure stream,
// the IPA is in Secure PA space, so nsipa=false even though s2=true.
//
// nsipa = (isStage2 && securityState == NonSecure)
//       = (true     && Secure == NonSecure) = false   → correct.
//
// BEFORE FIX: nsipa=false (accidentally correct for this case, but for the
//             wrong reason — hard-coded to false unconditionally).
// AFTER FIX:  nsipa=false (correct, because stream is Secure, not NonSecure).
// This test acts as a regression guard to ensure the fix does not
// incorrectly set nsipa=true for Secure streams.
// ============================================================================
TEST(ConfGapNsipa, nsipa_stage2_secure_stream_has_nsipa_false) {
    SMMU smmu;
    enableSMMU(smmu);

    // Build a Secure two-stage stream with no stage-2 mappings.
    std::shared_ptr<AddressSpace> s2as;
    setupSecureTwoStageStream(smmu, SID_SEC2, s2as);

    // Map stage-1 IOVA → IPA in Secure space; stage-2 has no IPA mapping.
    static constexpr IOVA IOVA_VAL = 0x3000ULL;
    static constexpr PA   IPA_VAL  = 0x9000'0000ULL;
    mapSecurePage(smmu, SID_SEC2, IOVA_VAL, IPA_VAL);

    smmu.translate(SID_SEC2, PID, IOVA_VAL, AccessType::Read, SecurityState::Secure);

    EventEntry ev = findLastEvent(smmu, EventType::F_TRANSLATION);
    EXPECT_TRUE(ev.s2)
        << "stage-2 fault must have s2=true in EventEntry";
    EXPECT_FALSE(ev.nsipa)
        << "NSIPA must be false for Secure two-stage fault (IPA is in Secure "
           "PA space, §7.3). A correct fix must not set nsipa=true here.";
}
