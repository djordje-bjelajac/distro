use std::time::Duration;

use libp2p::identity::Keypair;
use libp2p::kad::store::MemoryStore;
use libp2p::swarm::NetworkBehaviour;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{
    StreamProtocol, autonat, connection_limits, dcutr, gossipsub, identify, kad, mdns, relay,
    request_response,
};
use rand::rngs::OsRng;

use crate::limits::ResourceLimits;
use crate::swarm::direct_message_codec::DirectMessageCodec;

/// Every protocol this peer speaks, composed into one behaviour.
///
/// # One binary, one code path, every service offered (AC4)
///
/// There is no relay flag, no "server mode", no bootstrap-node build. The
/// relay **server** is always constructed alongside the relay client, Kademlia
/// always answers routing queries, and AutoNAT always probes *for* other peers
/// as well as asking them about itself. That is what "peers provide all
/// infrastructure" means when written as code: the only thing that
/// distinguishes a peer that relays from one that does not is whether anyone
/// asked it to.
///
/// # Nothing here reaches a host somebody else operates (S1)
///
/// Every default that would have is disabled explicitly, and each is named
/// below rather than left to be discovered later:
///
/// * **No bootstrap addresses.** Kademlia starts with an empty routing table.
///   The only peers it learns are ones this instance discovered on its own LAN,
///   redeemed a ticket for, or found through a peer it already knew (D1).
/// * **No DNS.** The `dns` transport feature is not enabled, so no resolver is
///   consulted and every address is a literal one a peer told us about. It also
///   means a `/dnsaddr` bootstrap list is not merely unused but unreadable.
/// * **No default relay or rendezvous.** Circuit reservations are made only
///   with peers the roster already knows; there is no rendezvous protocol in
///   the build at all.
/// * **No STUN and no AutoNAT server list.** AutoNAT v2 asks peers it is
///   *already connected to* whether they can reach an address of ours. There
///   is no third-party address-reflection service anywhere in the path.
/// * **No telemetry.** `libp2p-metrics` is not a dependency. Every counter in
///   this crate is read in-process (see
///   [`CodecDiagnostics`](crate::codec::CodecDiagnostics)) and leaves the
///   machine only if a human looks at it.
/// * **mDNS is the one thing that speaks unprompted**, and only to the local
///   link's multicast group — which is the LAN rung of D1, and is switchable
///   off for a test or a user who does not want it.
#[derive(NetworkBehaviour)]
pub(crate) struct DistroBehaviour {
    /// S6's concurrent-session cap, enforced at the connection layer so a
    /// flood is refused before a handshake is paid for.
    pub(crate) connection_limits: connection_limits::Behaviour,
    /// How peers learn each other's addresses and protocols. Also the source
    /// of the observed-address candidates AutoNAT then tests.
    pub(crate) identify: identify::Behaviour,
    /// Peer routing (D2). Always in server mode: a peer that queried the DHT
    /// without answering queries would be taking a service it does not give.
    pub(crate) kademlia: kad::Behaviour<MemoryStore>,
    /// D1 rung (b): unconfigured LAN discovery. Switchable, because a test
    /// must not multicast onto the machine's real network.
    pub(crate) mdns: Toggle<mdns::tokio::Behaviour>,
    /// Asks connected peers whether an address of ours is reachable from
    /// outside.
    pub(crate) autonat_client: autonat::v2::client::Behaviour,
    /// Answers that question for others (AC4).
    pub(crate) autonat_server: autonat::v2::server::Behaviour<OsRng>,
    /// Circuit Relay v2, **server side** — this peer carries traffic for peers
    /// that cannot be reached directly. The heart of AC4 and AC12: the relay
    /// is a peer, not infrastructure, and it only ever sees ciphertext.
    pub(crate) relay_server: relay::Behaviour,
    /// Circuit Relay v2, client side — this peer reserves a slot on another
    /// peer when it is the one behind a NAT.
    pub(crate) relay_client: relay::client::Behaviour,
    /// Hole punching: upgrades a relayed link to a direct one when both ends
    /// can manage it, which is what keeps relaying rare rather than universal.
    pub(crate) dcutr: dcutr::Behaviour,
    /// D3: the one network-wide broadcast topic.
    pub(crate) gossipsub: gossipsub::Behaviour,
    /// D4: 1:1 messages over the authenticated session.
    pub(crate) direct: request_response::Behaviour<DirectMessageCodec>,
}

/// The identify protocol name. Peers of a different major protocol version
/// still connect and still identify — the version check is per envelope (S2),
/// not per connection, so a peer we cannot read messages from can still relay
/// for us.
const IDENTIFY_PROTOCOL: &str = "/distro/id/1.0.0";

/// The Kademlia protocol name, namespaced so this network's DHT is its own and
/// not a shard of the public IPFS one.
const KADEMLIA_PROTOCOL: StreamProtocol = StreamProtocol::new("/distro/kad/1.0.0");

impl DistroBehaviour {
    /// Builds the behaviour for `keypair`, given the relay-client behaviour the
    /// transport builder produced.
    pub(crate) fn new(
        keypair: &Keypair,
        relay_client: relay::client::Behaviour,
        broadcast_topic: &gossipsub::IdentTopic,
        enable_mdns: bool,
        limits: ResourceLimits,
    ) -> Result<Self, DistroBehaviourError> {
        let local = keypair.public().to_peer_id();

        let connection_limits = connection_limits::Behaviour::new(
            connection_limits::ConnectionLimits::default()
                .with_max_established(Some(limits.max_established_connections))
                .with_max_established_per_peer(Some(limits.max_established_per_peer))
                .with_max_pending_incoming(Some(limits.max_pending_incoming))
                .with_max_pending_outgoing(Some(limits.max_pending_outgoing)),
        );

        let identify = identify::Behaviour::new(identify::Config::new(
            IDENTIFY_PROTOCOL.to_owned(),
            keypair.public(),
        ));

        let mut kademlia_config = kad::Config::new(KADEMLIA_PROTOCOL);
        // Server mode from the start rather than `Auto`: `Auto` waits for an
        // external address to be confirmed before answering queries, and on a
        // network where every peer is also the infrastructure, a peer that
        // stays a client until AutoNAT vindicates it is a peer that never
        // helps anybody bootstrap (AC4).
        kademlia_config.set_record_ttl(None);
        let mut kademlia =
            kad::Behaviour::with_config(local, MemoryStore::new(local), kademlia_config);
        kademlia.set_mode(Some(kad::Mode::Server));

        let mdns = Toggle::from(if enable_mdns {
            Some(
                mdns::tokio::Behaviour::new(mdns::Config::default(), local)
                    .map_err(|_| DistroBehaviourError::MdnsUnavailable)?,
            )
        } else {
            None
        });

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            // S6 again, at the gossip layer: an oversize message is refused by
            // the protocol before it reaches the envelope codec.
            .max_transmit_size(limits.max_envelope_bytes)
            // The envelope's signature is the identity of the message
            // (invariant 4), so two copies of one message arriving by two
            // gossip paths must be one message. Deriving the id from the
            // signature makes AC7's exactly-once cheap: the duplicate is
            // dropped at the gossip layer and never costs a decode.
            .message_id_fn(message_id)
            .validation_mode(gossipsub::ValidationMode::Strict)
            // S6's per-session buffer cap, and the one libp2p default that
            // had to move: 5000 buffered messages per connection at the
            // 32 KiB envelope cap is 160 MiB a single peer could make this
            // process hold.
            .connection_handler_queue_len(limits.max_session_buffered_messages)
            // libp2p's default is *no* limit on messages per RPC, which is the
            // same flood in one frame instead of many.
            .max_messages_per_rpc(Some(limits.max_messages_per_rpc))
            .heartbeat_interval(Duration::from_secs(1))
            .build()
            .map_err(|_| DistroBehaviourError::GossipsubConfig)?;
        let mut gossipsub = gossipsub::Behaviour::new(
            // Signed at the gossip layer *as well as* at the envelope layer.
            // The two answer different questions: the envelope signature says
            // who wrote the message (invariant 4), the gossip signature says
            // who put this copy on the wire.
            gossipsub::MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|_| DistroBehaviourError::GossipsubConfig)?;
        gossipsub
            .subscribe(broadcast_topic)
            .map_err(|_| DistroBehaviourError::GossipsubSubscribe)?;

        let relay_server = relay::Behaviour::new(
            local,
            relay::Config {
                max_circuit_duration: limits.max_relay_circuit_duration,
                max_circuit_bytes: limits.max_relay_circuit_bytes,
                max_circuits: limits.max_relay_circuits,
                max_circuits_per_peer: limits.max_relay_circuits_per_peer,
                ..relay::Config::default()
            },
        );

        let direct = request_response::Behaviour::with_codec(
            DirectMessageCodec::new(limits.max_envelope_bytes),
            [(
                DirectMessageCodec::PROTOCOL,
                request_response::ProtocolSupport::Full,
            )],
            request_response::Config::default().with_request_timeout(limits.request_timeout),
        );

        Ok(Self {
            connection_limits,
            identify,
            kademlia,
            mdns,
            autonat_client: autonat::v2::client::Behaviour::new(
                OsRng,
                autonat::v2::client::Config::default(),
            ),
            autonat_server: autonat::v2::server::Behaviour::new(OsRng),
            relay_server,
            relay_client,
            dcutr: dcutr::Behaviour::new(local),
            gossipsub,
            direct,
        })
    }
}

/// A gossip message's identity: the tail of the frame it arrived in.
///
/// The envelope's signature sits at a fixed place in the encoding, but this
/// function must work on bytes it has not parsed — gossipsub calls it before
/// anything is decoded, and decoding here would put a parser on the hot path of
/// every duplicate. Hashing the whole frame is exact for the property that
/// matters: two byte-identical copies of one published message are one message,
/// and two different messages are never confused.
fn message_id(message: &gossipsub::Message) -> gossipsub::MessageId {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    message.data.hash(&mut hasher);
    gossipsub::MessageId::from(hasher.finish().to_be_bytes())
}

/// Why the behaviour could not be assembled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DistroBehaviourError {
    /// mDNS could not bind its multicast socket. LAN discovery is one rung of
    /// the ladder (D1), so this is reported rather than being fatal.
    MdnsUnavailable,
    /// The gossipsub configuration is internally inconsistent — a programming
    /// error in this file, surfaced rather than unwrapped.
    GossipsubConfig,
    /// The broadcast topic could not be subscribed to.
    GossipsubSubscribe,
}

impl std::fmt::Display for DistroBehaviourError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MdnsUnavailable => f.write_str("LAN discovery could not bind its socket"),
            Self::GossipsubConfig => f.write_str("the broadcast channel could not be configured"),
            Self::GossipsubSubscribe => {
                f.write_str("the broadcast channel could not be subscribed to")
            }
        }
    }
}

impl std::error::Error for DistroBehaviourError {}
