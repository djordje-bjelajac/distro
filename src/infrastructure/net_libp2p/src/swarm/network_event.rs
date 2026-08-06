use membership::domain::Endpoint;
use membership::ports::DiscoveredPeer;
use shared_types::{Envelope, EnvelopeSignature, PeerId};

use crate::swarm::reachability_ledger::Reachability;

/// Everything the network reports upward, in this crate's own vocabulary.
///
/// # Why this is an enum and not a set of port calls
///
/// The driver could hold `InboundSessionPort` and `InboundEnvelopePort` and
/// call them directly. It deliberately does not: those are *application*
/// traits, and an infrastructure crate that calls into two contexts' handlers
/// from inside an async task would decide, on their behalf, which thread their
/// aggregates are mutated on. Emitting plain data instead leaves that to the
/// composition root, which is where the wiring belongs (canvas §4:
/// `infra-*` crates never depend on `application`).
///
/// Every variant maps onto exactly one inbound port call — that correspondence
/// is the contract OP-12 implements, and it is listed on each variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    /// A local listener came up. → nothing; the endpoints a peer announces are
    /// what `PeerTransportPort::listen` returned.
    ListeningOn(Endpoint),

    /// An address of this peer was confirmed reachable from outside by another
    /// peer's AutoNAT probe. → re-`announce`, since this is the first moment a
    /// NAT-ed peer has a truthful address to publish.
    ExternalAddressConfirmed(Endpoint),

    /// The answer to "can strangers dial me" moved. → **no port call at all**;
    /// the composition root holds the latest value and shows it (canvas OP-2).
    ///
    /// The one variant that maps onto no inbound port, and deliberately: this
    /// is a fact about *this process's* network position, not about any peer,
    /// any message, or any session, so no context owns it (D5). It is also
    /// report-only — nothing downstream may change a dial, a relay
    /// reservation, or an address selection on the strength of it (D4, S5).
    /// libp2p already prefers a confirmed direct address and falls back to a
    /// circuit; second-guessing that from here would duplicate the logic with
    /// worse information.
    ///
    /// Emitted only on an actual transition, so a root is not woken by the
    /// steady state of a healthy peer being re-probed on a timer.
    /// [`Reachability::Unknown`] is the value before any probe has concluded
    /// and **is not** [`Reachability::Unreachable`]; a renderer that treated
    /// them alike would show every peer an alarming, false claim during every
    /// startup (S3).
    ReachabilityChanged(Reachability),

    /// A discovery mechanism saw a peer. → `InboundSessionPort::peer_observed`.
    ///
    /// Also delivered through `PeerDiscoveryPort::observe_peers`, which reads
    /// the same buffer without emptying it (canvas `0010` D12). A root may use
    /// either or both: what either one records is a peer's *address*, which is
    /// idempotent, and since discovery is not evidence of life a repeat
    /// sighting cannot make a peer look alive.
    PeerDiscovered(DiscoveredPeer),

    /// A remote peer dialled this one and the handshake completed. →
    /// `InboundSessionPort::session_opened(peer, vec![endpoint])` followed by
    /// `session_established(peer)`.
    ///
    /// **Inbound only.** An outbound dial produces no event: it is a decision
    /// the application made, and `PeerTransportPort::dial` returning `Ok` is
    /// already the answer — `connect_to_peer` opens *and* establishes the
    /// session on the strength of it. Emitting one here as well would have the
    /// roster see a second open for a peer it already holds.
    ///
    /// Emitted **once per peer**, for the link the collapse rule kept. A
    /// simultaneous connect produces one of these, not two, because the
    /// superseded link is closed below this line and never announced.
    SessionEstablished {
        peer: PeerId,
        /// Where the surviving link runs — `Relayed` when a third peer is
        /// carrying it (AC12).
        endpoint: Endpoint,
    },

    /// A peer's last link went away. → `InboundSessionPort::session_closed`.
    ///
    /// Not emitted when a superseded link closes: the session lives on the
    /// survivor, and reporting a close would have the roster forget a peer it
    /// is still talking to.
    SessionClosed { peer: PeerId },

    /// A signed envelope arrived. → `InboundEnvelopePort::accept_envelope`.
    ///
    /// `from` is the peer that *handed it over* — the requester for a direct
    /// message, the propagating peer for a broadcast. It is **not** the author:
    /// the author is whoever's signature verifies (invariant 4), which is
    /// decided above this line and never here. `from` is evidence of life for
    /// presence (→ `InboundSessionPort::peer_heartbeat`), nothing more.
    EnvelopeReceived { from: PeerId, envelope: Envelope },

    /// A direct message this peer sent was taken in by its recipient. →
    /// `InboundEnvelopePort::message_delivered`.
    ///
    /// Correlated by envelope signature rather than by `MessageId`: a
    /// `MessageId` lives inside the payload, and this layer carries payloads
    /// unread. The root, which signed the envelope, knows which message the
    /// signature belongs to.
    DirectMessageDelivered {
        peer: PeerId,
        signature: EnvelopeSignature,
    },

    /// A direct message this peer sent did not get there. → the same failure
    /// path `MessageTransportPort` errors take, so the message reaches
    /// `Failed(reason)` and never sits `Pending` forever (AC11, D10).
    DirectMessageFailed {
        peer: PeerId,
        signature: EnvelopeSignature,
        reason: DirectMessageFailure,
    },
}

/// Why a direct message did not arrive.
///
/// Each variant is a sentence a user can act on, which is the whole of AC11:
/// silent loss is not a state, and "it failed" is not a diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectMessageFailure {
    /// No path to the peer could be opened at all.
    PeerUnreachable,
    /// The link died with the message on it.
    SessionClosed,
    /// It went out and nothing came back inside the timeout.
    NotAcknowledged,
    /// It arrived and the recipient refused it — over its rate limit, or not
    /// a frame it could read.
    Refused,
}
