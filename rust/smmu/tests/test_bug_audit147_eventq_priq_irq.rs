//! TDD tests for BUG-AUDIT-147.
//!
//! ARM §3.18.2: Event queue generates an interrupt on empty→non-empty transition
//! when IRQ_CTRL.EVENTQ_IRQEN is set; PRI queue generates an interrupt on
//! empty→non-empty transition when IRQ_CTRL.PRIQ_IRQEN is set.
#![allow(missing_docs)]
#![allow(clippy::doc_markdown)]

use smmu::SMMU;
use smmu::types::PRIEntry;
use smmu::types::AccessType;

fn make_smmu_eventq() -> SMMU {
    let smmu = SMMU::new();
    smmu.set_cr0(SMMU::CR0_CMDQEN | SMMU::CR0_EVENTQEN);
    smmu
}

fn make_smmu_priq() -> SMMU {
    let smmu = SMMU::new();
    smmu.enable().unwrap();
    smmu
}

const fn make_pri_entry() -> PRIEntry {
    PRIEntry::with_address(1, 0, 0x1000, AccessType::Read)
}

/// Enqueue an event via the public inject_ste_fetch_abort path (which calls enqueue_event).
fn inject_event(smmu: &SMMU) {
    smmu.inject_ste_fetch_abort(smmu::types::StreamID::new(1).unwrap());
}

// ── Test 1 ───────────────────────────────────────────────────────────────────

/// §3.18.2: With EVENTQ_IRQEN==0, enqueuing an event must NOT set eventq_irq_pending.
#[test]
fn eventq_irq_not_raised_when_irqen_disabled() {
    let smmu = make_smmu_eventq();
    smmu.set_irq_ctrl(0); // EVENTQ_IRQEN = 0

    inject_event(&smmu);

    assert!(
        !smmu.get_eventq_irq_pending(),
        "§3.18.2: eventq_irq_pending must be false when EVENTQ_IRQEN==0"
    );
}

// ── Test 2 ───────────────────────────────────────────────────────────────────

/// §3.18.2: With EVENTQ_IRQEN==1, enqueueing into an empty queue must set
/// eventq_irq_pending (empty→non-empty transition).
#[test]
fn eventq_irq_raised_on_empty_to_nonempty() {
    let smmu = make_smmu_eventq();
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_EVENTQ_IRQEN);

    inject_event(&smmu);

    assert!(
        smmu.get_eventq_irq_pending(),
        "§3.18.2: eventq_irq_pending must be true on empty→non-empty transition when EVENTQ_IRQEN==1"
    );
}

// ── Test 3 ───────────────────────────────────────────────────────────────────

/// §3.18.2: With EVENTQ_IRQEN==1, enqueueing a second event (non-empty→non-empty)
/// must NOT set eventq_irq_pending — only the empty→non-empty edge triggers the IRQ.
#[test]
fn eventq_irq_not_raised_on_nonempty_to_nonempty() {
    let smmu = make_smmu_eventq();
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_EVENTQ_IRQEN);

    // First enqueue: empty→non-empty (triggers IRQ).
    inject_event(&smmu);
    assert!(smmu.get_eventq_irq_pending(), "precondition: first enqueue must raise IRQ");

    // Clear the pending flag (models software ISR acknowledgment).
    smmu.clear_eventq_irq_pending();
    assert!(!smmu.get_eventq_irq_pending(), "precondition: flag must be clear after explicit clear");

    // Second enqueue: non-empty→non-empty (must NOT trigger IRQ).
    inject_event(&smmu);

    assert!(
        !smmu.get_eventq_irq_pending(),
        "§3.18.2: eventq_irq_pending must NOT be set on non-empty→non-empty transition"
    );
}

// ── Test 4 ───────────────────────────────────────────────────────────────────

/// §3.18.2: With PRIQ_IRQEN==1, submitting into an empty PRI queue must set
/// priq_irq_pending (empty→non-empty transition).
#[test]
fn priq_irq_raised_on_empty_to_nonempty() {
    let smmu = make_smmu_priq();
    smmu.set_irq_ctrl(SMMU::IRQ_CTRL_PRIQ_IRQEN);

    smmu.submit_page_request(make_pri_entry()).unwrap();

    assert!(
        smmu.get_priq_irq_pending(),
        "§3.18.2: priq_irq_pending must be true on empty→non-empty transition when PRIQ_IRQEN==1"
    );
}

// ── Test 5 ───────────────────────────────────────────────────────────────────

/// §3.18.2: With PRIQ_IRQEN==0, submitting a PRI request must NOT set priq_irq_pending.
#[test]
fn priq_irq_not_raised_when_priqen_disabled() {
    let smmu = make_smmu_priq();
    smmu.set_irq_ctrl(0); // PRIQ_IRQEN = 0

    smmu.submit_page_request(make_pri_entry()).unwrap();

    assert!(
        !smmu.get_priq_irq_pending(),
        "§3.18.2: priq_irq_pending must be false when PRIQ_IRQEN==0"
    );
}
