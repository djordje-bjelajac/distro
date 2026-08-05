use infra_net_libp2p::swarm::DirectMessageFailure;
use messaging::domain::DeliveryFailure;

use crate::runtime::{delivery_failure_of, transport_reason};

/// Every transport failure this build can observe.
///
/// Written out rather than taken from a constant on the enum, so that adding a
/// variant to `DirectMessageFailure` fails this list rather than quietly
/// shrinking the coverage of every test below it.
const ALL: [DirectMessageFailure; 4] = [
    DirectMessageFailure::PeerUnreachable,
    DirectMessageFailure::SessionClosed,
    DirectMessageFailure::NotAcknowledged,
    DirectMessageFailure::Refused,
];

#[test]
fn no_path_to_the_peer_is_reported_as_unreachable() {
    assert_eq!(
        delivery_failure_of(DirectMessageFailure::PeerUnreachable),
        DeliveryFailure::PeerUnreachable
    );
}

#[test]
fn a_link_that_died_is_reported_as_a_closed_session() {
    assert_eq!(
        delivery_failure_of(DirectMessageFailure::SessionClosed),
        DeliveryFailure::SessionClosed
    );
}

#[test]
fn a_timeout_is_reported_as_retries_exhausted() {
    // The same reason `MessageTransportError::NotAcknowledged` maps to, so the
    // synchronous and asynchronous halves of one condition read alike.
    assert_eq!(
        delivery_failure_of(DirectMessageFailure::NotAcknowledged),
        DeliveryFailure::RetriesExhausted
    );
}

#[test]
fn a_refusal_is_reported_as_retries_exhausted_because_nothing_truer_exists() {
    // `messaging` has no "refused by the recipient" outcome and the root may
    // not invent one. Of the five that exist, this is the only one that is
    // true of a refusal: the attempt ended without the message being taken in,
    // and the user may resend.
    assert_eq!(
        delivery_failure_of(DirectMessageFailure::Refused),
        DeliveryFailure::RetriesExhausted
    );
}

#[test]
fn a_refusal_never_claims_the_peer_was_unreachable_or_the_link_died() {
    // The two lies available: a path existed and the link was alive, which is
    // precisely what makes a refusal a refusal.
    let mapped = delivery_failure_of(DirectMessageFailure::Refused);

    assert_ne!(mapped, DeliveryFailure::PeerUnreachable);
    assert_ne!(mapped, DeliveryFailure::SessionClosed);
    assert_ne!(mapped, DeliveryFailure::TransportUnavailable);
}

#[test]
fn the_mapping_is_total() {
    // Every transport failure produces a delivery reason. A failure with no
    // reason would leave a message `Pending` for the life of the session,
    // which is the silent loss AC11 declares a non-state.
    for failure in ALL {
        let mapped = delivery_failure_of(failure);
        assert!(
            DeliveryFailure::ALL.contains(&mapped),
            "{failure:?} mapped outside the domain's reasons"
        );
    }
}

#[test]
fn nothing_maps_to_transport_unavailable() {
    // That reason means nothing was ever attempted locally, which cannot be
    // true of a failure the *network* reported: to report one, it had to have
    // tried.
    for failure in ALL {
        assert_ne!(
            delivery_failure_of(failure),
            DeliveryFailure::TransportUnavailable,
            "{failure:?}"
        );
    }
}

#[test]
fn every_transport_failure_has_a_sentence_of_its_own() {
    // The diagnosis the delivery state cannot carry. Two failures sharing a
    // sentence would erase the distinction the notice exists to preserve.
    let mut sentences: Vec<&str> = ALL.iter().copied().map(transport_reason).collect();
    sentences.sort_unstable();
    sentences.dedup();

    assert_eq!(sentences.len(), ALL.len());
    assert!(sentences.iter().all(|sentence| !sentence.is_empty()));
}

#[test]
fn the_two_failures_that_share_a_delivery_state_do_not_share_an_explanation() {
    // A refusal and a timeout are one outcome for the message and two
    // different faults on the network; the notice is where that survives.
    assert_eq!(
        delivery_failure_of(DirectMessageFailure::Refused),
        delivery_failure_of(DirectMessageFailure::NotAcknowledged)
    );
    assert_ne!(
        transport_reason(DirectMessageFailure::Refused),
        transport_reason(DirectMessageFailure::NotAcknowledged)
    );
}
