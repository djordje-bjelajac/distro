use infra_net_libp2p::swarm::DirectMessageFailure;
use messaging::domain::DeliveryFailure;

/// Translates what the network observed into what the user is told (AC11).
///
/// # Why the root owns this translation
///
/// The two enums belong to two crates that may not know about each other.
/// `infra-net-libp2p` names transport outcomes — a dial that found no path, a
/// stream that died, a request that timed out, a frame the recipient refused —
/// and `messaging` names *delivery* outcomes, which are what a user reads next
/// to a message. `infra-*` crates depend on the context ports they implement
/// and never on each other, so the only place both vocabularies exist is here.
///
/// The precedent is `MessageTransportError::as_delivery_failure`, which does
/// exactly this for the synchronous half of the same story: a send the
/// transport refuses immediately already becomes a `DeliveryFailure` that way.
/// This is the asynchronous half — the refusal that arrives after `send_direct`
/// has already returned `Ok` — and it must produce the same vocabulary, or the
/// same underlying condition would read differently depending on how quickly
/// the network noticed it.
///
/// # Total, and deliberately not `From`
///
/// A `match` with no wildcard: adding a variant to either enum has to be
/// considered here rather than silently defaulting to whichever reason looked
/// closest. Not a `From` impl because neither type is this crate's, and a free
/// function keeps the translation searchable by name.
///
/// # One mapping is lossy, and it is the honest choice available
///
/// [`DirectMessageFailure::Refused`] means the message *arrived* and the
/// recipient would not take it in — it was over its inbound rate limit (S6), or
/// the frame was not one it could read. `messaging` has no "refused by the
/// recipient" outcome, and inventing one would be writing a domain rule in the
/// composition root.
///
/// Of the five reasons that exist, [`RetriesExhausted`](DeliveryFailure::RetriesExhausted)
/// is the only one that is *true* of a refusal: the bounded attempt ended
/// without the message being taken in, and the user may resend. Every other
/// candidate asserts something false — `PeerUnreachable` claims no path
/// existed, `SessionClosed` claims the link died, `TransportUnavailable` claims
/// nothing was attempted. It is also the same reason a timeout maps to, which
/// is right: a negative acknowledgement and no acknowledgement are one outcome
/// as far as the message is concerned.
///
/// What is lost is the *diagnosis*, not the state — so the caller states the
/// transport's own reason in the notice beside the message, which is where a
/// user looking for "why" will read it.
pub const fn delivery_failure_of(failure: DirectMessageFailure) -> DeliveryFailure {
    match failure {
        DirectMessageFailure::PeerUnreachable => DeliveryFailure::PeerUnreachable,
        DirectMessageFailure::SessionClosed => DeliveryFailure::SessionClosed,
        DirectMessageFailure::NotAcknowledged | DirectMessageFailure::Refused => {
            DeliveryFailure::RetriesExhausted
        }
    }
}

/// The transport's own account of a failure, for the notice beside the message.
///
/// Kept apart from [`delivery_failure_of`] on purpose: the delivery *state* is
/// what the domain records and what the conversation pane draws, while this is
/// the sentence that explains it. Folding them together would either coarsen
/// the explanation to fit the state or push transport vocabulary into the
/// domain.
pub const fn transport_reason(failure: DirectMessageFailure) -> &'static str {
    match failure {
        DirectMessageFailure::PeerUnreachable => "no path to that peer could be opened",
        DirectMessageFailure::SessionClosed => "the link died with the message on it",
        DirectMessageFailure::NotAcknowledged => "nothing came back inside the timeout",
        DirectMessageFailure::Refused => {
            "it arrived and was refused — the recipient was over its rate limit, or could not read the frame"
        }
    }
}
