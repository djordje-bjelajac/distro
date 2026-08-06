use std::sync::mpsc::SyncSender;

use membership::domain::{Endpoint, JoinTicket};
use membership::ports::{DiscoveredPeer, PeerDiscoveryError, PeerTransportError};
use messaging::ports::MessageTransportError;
use shared_types::{EnvelopeSignature, PeerId};

/// Where a command's answer is sent back to the blocked caller.
///
/// Deliberately `std::sync::mpsc` rather than `tokio::sync::oneshot`: the
/// caller is an ordinary synchronous thread executing a port method, and
/// `oneshot::Receiver::blocking_recv` **panics** when it happens to be called
/// from inside a runtime. A `std` receiver blocks anywhere, and
/// `recv_timeout` turns "the driver never answered" into a typed refusal
/// instead of a hang (AC3).
pub(crate) type Reply<T, E> = SyncSender<Result<T, E>>;

/// One thing the synchronous side asks the swarm to do.
///
/// # Why the ports are not simply async
///
/// `PeerTransportPort`, `PeerDiscoveryPort`, and `MessageTransportPort` take
/// `&self` and return values, by design: no `tokio`, socket, or async machinery
/// may appear in a port signature (canvas §4), and making them async would put
/// a runtime in every context's test. So the asynchrony stops at this enum. A
/// port method builds a command, hands it to the driver over a channel, and
/// blocks on the reply.
///
/// # Every variant carries its own error type
///
/// The three ports have three different error enums, and each one is shaped so
/// that every variant means something to the layer above — a delivery state a
/// user can read (AC11), a bootstrap-rung failure the join diagnostic can name
/// (AC3). Collapsing them into one adapter-level error here and mapping back
/// at the port would lose exactly the distinctions those enums exist to keep.
pub(crate) enum NetworkCommand {
    /// Start accepting inbound sessions and report the dialable endpoints.
    Listen {
        reply: Reply<Vec<Endpoint>, PeerTransportError>,
    },
    /// Dial `peer` at `endpoints`, in order, and report the one that answered.
    Dial {
        peer: PeerId,
        endpoints: Vec<Endpoint>,
        reply: Reply<Endpoint, PeerTransportError>,
    },
    /// Close every link to `peer`.
    ///
    /// Unambiguous by construction: the simultaneous-connect collapse is
    /// resolved inside the driver (see
    /// [`LinkRegistry`](crate::swarm::link_registry::LinkRegistry)), so by the
    /// time this arrives there is exactly one logical session to end.
    CloseSession {
        peer: PeerId,
        reply: Reply<(), PeerTransportError>,
    },
    /// Publish the local peer's endpoints so others can find it (S8).
    Announce {
        endpoints: Vec<Endpoint>,
        reply: Reply<(), PeerDiscoveryError>,
    },
    /// Report the peers discovery has seen recently.
    ///
    /// **Recently, not "since the last call".** Answering does not consume the
    /// sighting: a rung that empties its own input works exactly once, which is
    /// the defect canvas `0010` D12 names and A7 guards. Sightings leave the
    /// buffer by ageing out, never by being read — see
    /// [`SightingLedger`](crate::swarm::sighting_ledger::SightingLedger) for the
    /// retention rule and its bound.
    ObservePeers {
        reply: Reply<Vec<DiscoveredPeer>, PeerDiscoveryError>,
    },
    /// Dial a join ticket's endpoints and report the peer that answered (D1).
    RedeemTicket {
        ticket: Box<JoinTicket>,
        reply: Reply<DiscoveredPeer, PeerDiscoveryError>,
    },
    /// Send an already-encoded envelope to one peer (D4).
    SendDirect {
        to: PeerId,
        /// Carried so the delivery outcome can be correlated back to the
        /// message the user is looking at, without this layer decoding a
        /// payload it has no business reading.
        signature: EnvelopeSignature,
        frame: Vec<u8>,
        reply: Reply<(), MessageTransportError>,
    },
    /// Release an already-encoded envelope to the broadcast topic (D3).
    PublishBroadcast {
        frame: Vec<u8>,
        reply: Reply<(), MessageTransportError>,
    },
    /// Stop the driver. Sent once, by the runtime, on shutdown.
    Shutdown,
}
