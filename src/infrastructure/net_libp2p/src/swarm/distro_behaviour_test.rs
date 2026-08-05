use libp2p::core::Endpoint;
use libp2p::core::transport::PortUse;
use libp2p::identity::Keypair;
use libp2p::swarm::handler::UpgradeInfoSend;
use libp2p::swarm::{ConnectionHandler, ConnectionId, NetworkBehaviour};
use libp2p::{Multiaddr, autonat, dcutr, gossipsub, identify, kad, relay};
use rand::rngs::OsRng;

use crate::limits::ResourceLimits;
use crate::mapping::PeerIdMapping;
use crate::swarm::direct_message_codec::DirectMessageCodec;
use crate::swarm::distro_behaviour::DistroBehaviour;
use crate::test_peers::{ALICE_SECRET_KEY, BOB_SECRET_KEY};

/// The topic these tests subscribe to. Not the production one: a test that
/// asserted against `NetworkConfig::DEFAULT_TOPIC` would pass for the wrong
/// reason if somebody changed the default.
const TOPIC: &str = "/distro/broadcast/behaviour-test";

fn keypair(mut secret: [u8; 32]) -> Keypair {
    Keypair::ed25519_from_bytes(&mut secret).expect("RFC 8032 vector is a valid secret key")
}

/// A behaviour built the way `NetworkRuntime` builds it, minus LAN discovery.
///
/// `enable_mdns = false` because constructing the mDNS behaviour binds a
/// multicast socket, and a test must not multicast onto a developer's real
/// network. The toggle is asserted separately, in the one test that does.
fn behaviour() -> DistroBehaviour {
    behaviour_with_lan_discovery(false).expect("the behaviour assembles without LAN discovery")
}

fn behaviour_with_lan_discovery(
    enable_mdns: bool,
) -> Result<DistroBehaviour, crate::swarm::distro_behaviour::DistroBehaviourError> {
    let keypair = keypair(ALICE_SECRET_KEY);
    let (_transport, relay_client) = relay::client::new(keypair.public().to_peer_id());

    DistroBehaviour::new(
        &keypair,
        relay_client,
        &gossipsub::IdentTopic::new(TOPIC),
        enable_mdns,
        ResourceLimits::DEFAULT,
    )
}

/// Every protocol name this behaviour would answer on a connection a stranger
/// opened to us over `local_address`.
///
/// This is the question AC4 actually asks — *what does this peer offer other
/// peers* — and it is asked of the assembled behaviour rather than of the
/// source, so a service that was constructed but disabled would not answer.
/// Reading it means walking libp2p's own path: the behaviour is asked for the
/// handler it would give that connection, and the handler is asked which
/// protocols it listens for.
fn protocols_offered_to_a_caller(local_address: &str) -> Vec<String> {
    let mut behaviour = behaviour();

    names(
        behaviour
            .handle_established_inbound_connection(
                ConnectionId::new_unchecked(1),
                stranger(),
                &address(local_address),
                &address(REMOTE_ADDRESS),
            )
            .expect("an inbound connection from a stranger is accepted"),
    )
}

/// The same, for a connection this peer opened.
///
/// Needed because connection roles are not symmetric: DCUtR's specification
/// puts the direct-connection upgrade in the hands of the side that *listened*
/// on the circuit, so the dialling side is the one that answers `/libp2p/dcutr`
/// and the listening side deliberately denies it.
fn protocols_offered_to_a_peer_we_dialled(address_dialled: &str) -> Vec<String> {
    let mut behaviour = behaviour();

    names(
        behaviour
            .handle_established_outbound_connection(
                ConnectionId::new_unchecked(2),
                stranger(),
                &address(address_dialled),
                Endpoint::Dialer,
                PortUse::New,
            )
            .expect("an outbound connection to a stranger is accepted"),
    )
}

fn names(handler: impl ConnectionHandler) -> Vec<String> {
    // `AsRef::<str>::as_ref` spelled out: the protocol names arrive as nested
    // `Either`s, which have an inherent `as_ref` of their own that would be
    // picked instead.
    let mut protocols: Vec<String> = handler
        .listen_protocol()
        .upgrade()
        .protocol_info()
        .map(|protocol| AsRef::<str>::as_ref(&protocol).to_owned())
        .collect();
    protocols.sort();
    protocols
}

fn stranger() -> libp2p::PeerId {
    PeerIdMapping::to_libp2p(crate::test_peers::bob())
        .expect("an RFC 8032 vector maps to a libp2p peer id")
}

fn address(text: &str) -> Multiaddr {
    text.parse().expect("a valid multiaddress")
}

/// A plain address: what a peer that can be dialled directly connects over.
const DIRECT_ADDRESS: &str = "/ip4/127.0.0.1/tcp/40001";

/// A circuit address: what a peer behind a NAT connects over, through a third
/// peer acting as relay (AC12).
const RELAYED_ADDRESS: &str = "/ip4/127.0.0.1/tcp/40001/p2p-circuit";

/// Where the other end is. Irrelevant to every assertion here, so it is a
/// constant rather than a parameter.
const REMOTE_ADDRESS: &str = "/ip4/127.0.0.1/tcp/40002";

#[test]
fn every_instance_is_also_infrastructure() {
    // AC4, as a check rather than as a reading of the source. Each field is
    // bound to its concrete type first: if a future edit puts any of these
    // behind a `Toggle`, an `Option`, or a config flag, this stops compiling —
    // which is the point. A red build is the only thing that stops a role flag
    // from arriving quietly.
    let behaviour = behaviour();

    let _relay_server: &relay::Behaviour = &behaviour.relay_server;
    let _relay_client: &relay::client::Behaviour = &behaviour.relay_client;
    let _autonat_server: &autonat::v2::server::Behaviour<OsRng> = &behaviour.autonat_server;
    let _autonat_client: &autonat::v2::client::Behaviour = &behaviour.autonat_client;
    let _dcutr: &dcutr::Behaviour = &behaviour.dcutr;
    let _gossipsub: &gossipsub::Behaviour = &behaviour.gossipsub;
    let _identify: &identify::Behaviour = &behaviour.identify;
    let _kademlia: &kad::Behaviour<kad::store::MemoryStore> = &behaviour.kademlia;
    let _direct: &libp2p::request_response::Behaviour<DirectMessageCodec> = &behaviour.direct;
}

#[test]
fn a_peer_offers_relaying_and_routing_to_every_stranger() {
    // The same claim, made where it is observable: these are the protocols a
    // peer that has never met us can start with us. "Offers relay" means the
    // hop protocol is answered, not that a `relay::Behaviour` was constructed.
    let offered = protocols_offered_to_a_caller(DIRECT_ADDRESS);
    let offers = |protocol: &str| offered.iter().any(|name| name == protocol);

    assert!(
        offers("/libp2p/circuit/relay/0.2.0/hop"),
        "the relay *server* must answer to strangers — a peer that only ran the \
         relay client would take a service it does not give (AC4, AC12). Offered: {offered:?}"
    );
    assert!(
        offers("/libp2p/circuit/relay/0.2.0/stop"),
        "the relay client is how this peer is reached when it is the one behind \
         a NAT (AC12). Offered: {offered:?}"
    );
    assert!(
        offers("/libp2p/autonat/2/dial-request"),
        "the AutoNAT server must answer other peers' reachability questions, not \
         only ask its own (AC4). Offered: {offered:?}"
    );
    assert!(
        offers("/distro/kad/1.0.0"),
        "Kademlia must answer routing queries: `kad::Mode::Client` advertises no \
         inbound protocol at all, so its absence here is a peer that queries the \
         DHT without serving it (AC4, D2). Offered: {offered:?}"
    );
    assert!(
        // `/ipfs/id/1.0.0` and not `/distro/id/1.0.0`: the string this crate
        // passes to `identify::Config::new` is the `protocolVersion` *field* of
        // the identify payload, which is a network label. The stream itself is
        // libp2p's, and it has to be — a peer of a different major version must
        // still be able to identify us in order to relay for us.
        offers("/ipfs/id/1.0.0"),
        "identify is how peers learn each other's addresses (D1). Offered: {offered:?}"
    );
    assert!(
        offers("/meshsub/1.1.0"),
        "gossipsub carries the broadcast channel (D3). Offered: {offered:?}"
    );
    assert!(
        offers(DirectMessageCodec::PROTOCOL.as_ref()),
        "1:1 messages ride their own protocol (D4). Offered: {offered:?}"
    );
}

#[test]
fn a_relayed_link_is_offered_the_hole_punch_that_would_replace_it() {
    // The other half of AC4. DCUtR runs only over a circuit, so asking a direct
    // connection about it would prove nothing either way, and it runs from the
    // side that dialled — hence the outbound role.
    let offered = protocols_offered_to_a_peer_we_dialled(RELAYED_ADDRESS);

    assert!(
        offered.iter().any(|name| name == "/libp2p/dcutr"),
        "hole-punch coordination is what keeps relaying rare rather than \
         universal (AC4, AC12). Offered: {offered:?}"
    );
}

#[test]
fn kademlia_serves_from_the_first_moment_rather_than_waiting_to_be_vindicated() {
    // `Mode::Auto` — libp2p's default — stays a client until AutoNAT confirms an
    // external address. On a network where the peers *are* the infrastructure,
    // a peer that waits is a peer that never helps anybody bootstrap (AC4).
    assert_eq!(behaviour().kademlia.mode(), kad::Mode::Server);
}

#[test]
fn the_broadcast_topic_is_subscribed_before_the_swarm_is_ever_polled() {
    // A peer that subscribed lazily would miss every message published between
    // its first connection and its first send (D3, AC10).
    let behaviour = behaviour();
    let topic_hash = gossipsub::IdentTopic::new(TOPIC).hash();

    assert!(
        behaviour
            .gossipsub
            .topics()
            .any(|topic| *topic == topic_hash),
        "the one broadcast topic must already be subscribed"
    );
}

#[test]
fn lan_discovery_is_off_only_when_it_is_switched_off() {
    // mDNS is the one thing in this build that speaks unprompted, so it is the
    // one service that is switchable (D1 rung b). The switch must be the *only*
    // thing that turns it off.
    assert!(
        !behaviour().mdns.is_enabled(),
        "`enable_mdns = false` must actually disable it"
    );

    match behaviour_with_lan_discovery(true) {
        Ok(behaviour) => assert!(
            behaviour.mdns.is_enabled(),
            "`enable_mdns = true` must actually enable it"
        ),
        // Binding the multicast socket is the one part of this file that needs
        // the machine's cooperation, so it gets the same treatment as the
        // loopback tests: skipped where a socket is impossible, failed where
        // the environment promised one.
        Err(error) => crate::required_network::skip(&error),
    }
}

#[test]
fn nothing_here_is_built_per_peer_or_per_role() {
    // Two peers, two keypairs, one construction path. If a role ever appeared,
    // the shape most likely to carry it is a parameter — so the assertion is
    // that the only parameters are identity, topic, LAN switch and caps, and
    // that two different identities produce the same set of services.
    let alice = behaviour();

    let bob_keypair = keypair(BOB_SECRET_KEY);
    let (_transport, relay_client) = relay::client::new(bob_keypair.public().to_peer_id());
    let bob = DistroBehaviour::new(
        &bob_keypair,
        relay_client,
        &gossipsub::IdentTopic::new(TOPIC),
        false,
        ResourceLimits::DEFAULT,
    )
    .expect("the second peer assembles the same way");

    assert_eq!(alice.kademlia.mode(), bob.kademlia.mode());
    assert_eq!(alice.mdns.is_enabled(), bob.mdns.is_enabled());
    assert_eq!(
        alice.gossipsub.topics().count(),
        bob.gossipsub.topics().count()
    );
}
