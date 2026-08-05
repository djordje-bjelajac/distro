use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::sync_channel;

use libp2p::gossipsub::IdentTopic;
use libp2p::swarm::Swarm;
use libp2p::{Multiaddr, SwarmBuilder, noise, tcp, yamux};
use shared_types::PeerId;

use crate::adapters::{Libp2pMessageTransport, Libp2pPeerDiscovery, Libp2pPeerTransport};
use crate::codec::{CodecDiagnostics, EnvelopeCodec};
use crate::runtime::{NetworkConfig, NetworkEvents, NetworkHandle, NetworkIdentity};
use crate::swarm::distro_behaviour::DistroBehaviour;
use crate::swarm::network_driver::NetworkDriver;

/// The running network: one tokio runtime, one driver task, and the
/// synchronous handles the rest of the application uses.
///
/// # The runtime/threading contract a composition root must honour
///
/// 1. **This type owns its runtime.** `tokio` does not appear in the root's
///    dependency list, in its `main`, or in any signature it writes. That is
///    canvas D2's containment rule taken to its conclusion: the async world
///    begins and ends inside this crate.
/// 2. **Exactly one driver task exists**, spawned by [`start`](Self::start),
///    and it owns the swarm outright. Nothing else can reach it.
/// 3. **Port calls block, and must not run on the driver's threads.** Call
///    them from the root's own thread or one it spawned. They never deadlock:
///    the driver never calls a port, and every call has a timeout.
/// 4. **Events must be drained.** [`events`](Self::events) is a bounded queue;
///    a root that stops draining loses events and the loss is counted. Drain
///    on a loop, from one thread, and fan each event out to the inbound ports
///    named on [`NetworkEvent`](crate::swarm::NetworkEvent).
/// 5. **Keep this value alive for as long as the network is wanted.** Dropping
///    it stops the driver and closes every connection; the three port adapters
///    hold only a [`NetworkHandle`], and after the runtime is gone every call
///    on them returns its port's "unavailable" refusal rather than hanging.
/// 6. **Shut down explicitly** with [`shutdown`](Self::shutdown) where the
///    root can, so the driver stops before the process starts tearing other
///    things down. `Drop` does the same thing, on a timeout.
pub struct NetworkRuntime {
    /// `Option` so `shutdown` can take it by value while `Drop` still works.
    runtime: Option<tokio::runtime::Runtime>,
    handle: NetworkHandle,
    events: Arc<NetworkEvents>,
    local: PeerId,
    diagnostics: CodecDiagnostics,
}

impl NetworkRuntime {
    /// Builds the swarm and starts driving it.
    ///
    /// Nothing is listening yet: that is `PeerTransportPort::listen`, which the
    /// membership context calls when it decides to join. Starting a swarm and
    /// joining a network are two different decisions, and only the second one
    /// is the user's.
    pub fn start(
        identity: &NetworkIdentity,
        config: &NetworkConfig,
    ) -> Result<Self, NetworkStartError> {
        let listen_addresses = parse_listen_addresses(&config.listen_addresses)?;
        // Parsed before anything is built, so a typo costs nothing but the
        // refusal: this is the option a user reaches for when nothing else
        // worked, and it must never fail quietly (S4).
        let external_addresses = parse_external_addresses(&config.external_addresses)?;
        let topic = IdentTopic::new(&config.broadcast_topic);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            // One worker is enough: the driver is a single task and the
            // transports' IO is driven by the same reactor. More threads would
            // buy nothing and cost context switches.
            .worker_threads(1)
            .enable_all()
            .thread_name("distro-net")
            .build()
            .map_err(|_| NetworkStartError::RuntimeUnavailable)?;

        // The transports register with the reactor as they are built, so the
        // swarm must be constructed inside the runtime's context.
        let swarm = {
            let _guard = runtime.enter();
            build_swarm(identity, config, &topic)?
        };

        let diagnostics = CodecDiagnostics::new();
        let codec = EnvelopeCodec::new(config.protocol_version, config.limits, diagnostics.clone());

        let (commands_tx, commands_rx) = tokio::sync::mpsc::unbounded_channel();
        let (events_tx, events_rx) = sync_channel(config.limits.event_queue_capacity);

        let mut driver = NetworkDriver::new(
            swarm,
            identity.peer_id(),
            topic,
            listen_addresses,
            config.limits,
            codec.clone(),
            diagnostics.clone(),
            commands_rx,
            events_tx,
        );

        // Before the driver is spawned, so the confirmations are already in the
        // queue the root drains and a ticket minted after the first `listen`
        // carries them. Refusing here rather than warning is deliberate: this
        // process clears the screen for a terminal interface moments later, and
        // a warning printed into that is a warning nobody sees (S3).
        for address in external_addresses {
            driver.assert_external_address(address)?;
        }

        runtime.spawn(driver.run());

        Ok(Self {
            runtime: Some(runtime),
            handle: NetworkHandle::new(
                commands_tx,
                config.limits.request_timeout,
                identity.peer_id(),
                codec,
                diagnostics.clone(),
            ),
            events: Arc::new(NetworkEvents::new(events_rx)),
            local: identity.peer_id(),
            diagnostics,
        })
    }

    /// This peer's identity.
    pub const fn local_peer(&self) -> PeerId {
        self.local
    }

    /// A cloneable synchronous handle to the swarm.
    pub fn handle(&self) -> NetworkHandle {
        self.handle.clone()
    }

    /// The bounded queue of everything the network reports. Drain it.
    pub fn events(&self) -> Arc<NetworkEvents> {
        Arc::clone(&self.events)
    }

    /// The S2/S6 counters.
    pub fn diagnostics(&self) -> CodecDiagnostics {
        self.diagnostics.clone()
    }

    /// `membership`'s `PeerTransportPort`.
    pub fn peer_transport(&self) -> Libp2pPeerTransport {
        Libp2pPeerTransport::new(self.handle())
    }

    /// `membership`'s `PeerDiscoveryPort`.
    pub fn peer_discovery(&self) -> Libp2pPeerDiscovery {
        Libp2pPeerDiscovery::new(self.handle())
    }

    /// `messaging`'s `MessageTransportPort`.
    pub fn message_transport(&self) -> Libp2pMessageTransport {
        Libp2pMessageTransport::new(self.handle())
    }

    /// Stops the driver and waits for the runtime to wind down.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.handle.shutdown();
        if let Some(runtime) = self.runtime.take() {
            // A bounded wait: a transport stuck closing a connection must not
            // stop the application from exiting.
            runtime.shutdown_timeout(SHUTDOWN_GRACE);
        }
    }
}

impl Drop for NetworkRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

impl fmt::Debug for NetworkRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkRuntime")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// How long the runtime is given to wind down before the process moves on.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Assembles the transport stack and the behaviour.
///
/// # Transports, and where Noise actually runs
///
/// Two transports, both authenticated, both encrypted:
///
/// * **QUIC** (`/udp/…/quic-v1`) is the preferred one. Its handshake is TLS 1.3
///   with libp2p peer certificates rather than Noise — the same proof of key
///   possession by a different construction, and the one that traverses NATs
///   best. Canvas D2 says "QUIC + Noise"; this is the honest reading of it,
///   because libp2p's QUIC does not offer a Noise variant.
/// * **TCP + Noise + Yamux** is the fallback, and it is what a relayed circuit
///   runs over — so Noise is exactly where AC12's "relayed bytes are ciphertext
///   to the relay" is enforced.
pub(crate) fn build_swarm(
    identity: &NetworkIdentity,
    config: &NetworkConfig,
    topic: &IdentTopic,
) -> Result<Swarm<DistroBehaviour>, NetworkStartError> {
    let enable_mdns = config.enable_lan_discovery;
    let limits = config.limits;
    let topic = topic.clone();

    SwarmBuilder::with_existing_identity(identity.keypair().clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|_| NetworkStartError::TransportUnavailable)?
        .with_quic()
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .map_err(|_| NetworkStartError::TransportUnavailable)?
        .with_behaviour(|keypair, relay_client| {
            DistroBehaviour::new(keypair, relay_client, &topic, enable_mdns, limits)
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
        })
        .map_err(|_| NetworkStartError::BehaviourUnavailable)
        .map(|builder| {
            builder
                .with_swarm_config(|swarm| {
                    swarm.with_idle_connection_timeout(config.idle_connection_timeout)
                })
                .build()
        })
}

fn parse_listen_addresses(addresses: &[String]) -> Result<Vec<Multiaddr>, NetworkStartError> {
    if addresses.is_empty() {
        return Err(NetworkStartError::NoListenAddress);
    }

    addresses
        .iter()
        .map(|address| {
            address
                .parse()
                .map_err(|_| NetworkStartError::MalformedListenAddress)
        })
        .collect()
}

/// The asserted external addresses, parsed and in the order they were supplied.
///
/// An empty configuration is the ordinary case and is not an error — unlike the
/// listen addresses above, where having none means the peer can do nothing.
/// Whether each address is one a stranger could dial is *not* decided here: that
/// filter lives with the predicate it shares with piece 1's ledger and is
/// applied by
/// [`NetworkDriver::assert_external_address`](crate::swarm::network_driver::NetworkDriver::assert_external_address),
/// so there is one such filter in this crate rather than two that drift.
fn parse_external_addresses(addresses: &[String]) -> Result<Vec<Multiaddr>, NetworkStartError> {
    addresses
        .iter()
        .map(|address| {
            address
                .trim()
                .parse()
                .map_err(|_| NetworkStartError::MalformedExternalAddress)
        })
        .collect()
}

/// Why the network could not be started.
///
/// All of them are configuration or environment problems visible at startup,
/// not things a peer on the network can cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStartError {
    /// The configuration names no address to listen on.
    NoListenAddress,
    /// One of the listen addresses is not a multiaddress.
    MalformedListenAddress,
    /// One of the asserted external addresses is not a multiaddress.
    MalformedExternalAddress,
    /// One of the asserted external addresses is not one a stranger on the open
    /// internet could dial.
    NonGlobalExternalAddress,
    /// An async runtime could not be created.
    RuntimeUnavailable,
    /// The transport stack could not be assembled.
    TransportUnavailable,
    /// The protocol behaviours could not be assembled — LAN discovery could
    /// not bind, or the broadcast topic was refused.
    BehaviourUnavailable,
}

impl fmt::Display for NetworkStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoListenAddress => f.write_str("no listen address is configured"),
            Self::MalformedListenAddress => {
                f.write_str("a configured listen address is not a multiaddress")
            }
            Self::MalformedExternalAddress => {
                f.write_str("a configured external address is not a multiaddress")
            }
            // The refusal is the useful half here. Somebody who typed their LAN
            // address was trying to be reachable from another machine, and the
            // answer they need is not "rejected" — it is that this build
            // already finds peers on the local link without being told
            // anything, so the option they reached for is one they do not need.
            Self::NonGlobalExternalAddress => f.write_str(
                "a configured external address is not reachable from outside \
                 this network — only an address a stranger on the internet \
                 could dial can be advertised. mDNS already covers the local \
                 network, so peers on the same LAN find each other without \
                 this option",
            ),
            Self::RuntimeUnavailable => f.write_str("the network runtime could not be created"),
            Self::TransportUnavailable => f.write_str("the transport stack could not be built"),
            Self::BehaviourUnavailable => {
                f.write_str("the network protocols could not be assembled")
            }
        }
    }
}

impl std::error::Error for NetworkStartError {}
