use shared_types::EnvelopeSignature;

use crate::composition::HeartbeatLedger;

fn signature(byte: u8) -> EnvelopeSignature {
    EnvelopeSignature::new([byte; EnvelopeSignature::LENGTH])
}

#[test]
fn a_signature_nobody_recorded_is_not_a_heartbeat() {
    // The default answer has to be "no": a signature this has never seen is a
    // message's, and treating it as a heartbeat would swallow a real delivery
    // report.
    let ledger = HeartbeatLedger::new();

    assert!(!ledger.is_heartbeat(&signature(1)));
}

#[test]
fn a_recorded_signature_is_recognised() {
    let ledger = HeartbeatLedger::new();

    ledger.record(signature(1));

    assert!(ledger.is_heartbeat(&signature(1)));
}

#[test]
fn recognising_a_heartbeat_does_not_consume_it() {
    // One round signs one envelope and sends it to every linked peer, so one
    // signature attracts one report per peer. A consuming lookup would
    // recognise the first peer's answer and let every other peer's fall through
    // to the message path, which is the notice S6 forbids.
    let ledger = HeartbeatLedger::new();
    ledger.record(signature(1));

    assert!(ledger.is_heartbeat(&signature(1)));
    assert!(ledger.is_heartbeat(&signature(1)));
    assert!(ledger.is_heartbeat(&signature(1)));
}

#[test]
fn recording_the_same_signature_twice_holds_one_entry() {
    // Every round after the first records the same signature, because a
    // heartbeat envelope does not vary and Ed25519 signing is deterministic.
    let ledger = HeartbeatLedger::new();

    ledger.record(signature(1));
    ledger.record(signature(1));
    ledger.record(signature(1));

    assert_eq!(ledger.held(), 1);
}

#[test]
fn the_ledger_is_bounded_and_evicts_the_oldest() {
    let ledger = HeartbeatLedger::with_capacity(2);

    ledger.record(signature(1));
    ledger.record(signature(2));
    ledger.record(signature(3));

    assert_eq!(ledger.held(), 2);
    assert!(!ledger.is_heartbeat(&signature(1)));
    assert!(ledger.is_heartbeat(&signature(2)));
    assert!(ledger.is_heartbeat(&signature(3)));
}

#[test]
fn a_repeat_of_the_oldest_signature_does_not_push_anything_out() {
    // The steady state: one signature recorded on every tick forever. If a
    // repeat counted as a new entry the cap would evict entries that are still
    // in use.
    let ledger = HeartbeatLedger::with_capacity(2);
    ledger.record(signature(1));
    ledger.record(signature(2));

    for _ in 0..100 {
        ledger.record(signature(1));
    }

    assert_eq!(ledger.held(), 2);
    assert!(ledger.is_heartbeat(&signature(1)));
    assert!(ledger.is_heartbeat(&signature(2)));
}
