use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use identity::application::IdentityContext;
use identity::ports::{IdentityCommandPort, IdentityKeyStoreError, TrustRecordStorePort};
use infra_net_libp2p::{
    CodecDiagnostics, NetworkConfig, NetworkEvents, NetworkIdentity, NetworkIdentityError,
    NetworkRuntime, NetworkStartError,
};
use infra_store_fs::{LocalStores, LocalStoresError};
use membership::application::{MembershipContext, MembershipSettings};
use membership::domain::{JoinTicket, JoinTicketError};
use membership::ports::{
    ClockPort as MembershipClockPort, EventPublisherPort as MembershipPublisherPort, PeerCachePort,
    PeerDiscoveryPort, PeerTransportPort,
};
use messaging::application::{MessagingContext, MessagingPorts, MessagingSettings};
use messaging::ports::{
    AuthorPolicyPort, ClockPort as MessagingClockPort, EnvelopeSignerPort, EnvelopeVerifierPort,
    EventPublisherPort as MessagingPublisherPort, MessageLogPort, MessageTransportPort,
    SequenceCounterPort,
};
use shared_types::{PeerId, ProtocolVersion};

use crate::composition::{
    CorrelatingTransport, DeliveryIndex, Diagnostics, GapLedger, HeartbeatBeacon, HeartbeatLedger,
    LocalEndpoints, MembershipEventRelay, MessagingEventSink, NoticeFeed, SystemClock,
    TrustDirectory,
};

/// One running instance: three contexts, five stores, one swarm, and the
/// adapters that join them.
///
/// # The startup order, and why it is that order
///
/// [`start`](Self::start) does five things and the sequence is load-bearing:
///
/// 1. **Open the stores.** One directory, five stores, created owner-only.
///    Nothing is read yet.
/// 2. **Assume the identity** (AC1, AC9). The keystore's load-or-create runs
///    before anything else exists, because every other piece needs the
///    `PeerId`: `MembershipSettings`, `MessagingSettings`, and the swarm's own
///    handshake key. `initialize_local_identity(None)` asks the user nothing
///    and derives a display name from the fingerprint — first launch has no
///    registration step to skip.
/// 3. **Take the transport secret** (S3a). The one crossing the canvas records:
///    the libp2p handshake cannot be delegated behind a port, so the raw secret
///    is read from the *concrete* `FileIdentityKeyStore` and passed straight
///    into `NetworkIdentity::from_ed25519_secret_key`, which zeroes the buffer
///    as it consumes it. The buffer is zeroed on every error path too.
/// 4. **Start the network.** Nothing is listening yet: that is
///    `PeerTransportPort::listen`, which `membership` calls when it decides to
///    join. Starting a swarm and joining a network are two different decisions
///    and only the second one is the user's. Any address the operator asserted
///    with `--external-address` is advertised here, and a malformed or
///    non-global one refuses the launch rather than being quietly dropped —
///    the whole option exists for the peer that has nobody to ask, so it must
///    not fail silently.
/// 5. **Build the contexts** over the swarm's three port adapters, the five
///    stores, the one clock, and the root's own adapters.
///
/// Joining is *not* here. It is a decision, it takes seconds, and doing it
/// during construction would mean a UI that cannot draw until the network
/// answers — so the caller starts the engine, shows `joining`, and asks for the
/// ladder to be walked (AC3: a visible diagnostic, never a hang).
///
/// # What this type is not
///
/// It holds no state of its own beyond what it assembled, and it makes no
/// decisions. Every method below either hands out a collaborator or forwards a
/// call. The one exception is [`mint_join_ticket`](Self::mint_join_ticket),
/// which assembles a `JoinTicket` out of facts the root already holds — the
/// domain constructor does the deciding.
pub struct Node {
    profile_directory: PathBuf,
    local_peer: PeerId,
    protocol: ProtocolVersion,

    clock: Arc<SystemClock>,
    identity: Arc<IdentityContext>,
    membership: Arc<MembershipContext>,
    messaging: Arc<MessagingContext>,

    trust: Arc<TrustDirectory>,
    membership_events: Arc<MembershipEventRelay>,
    endpoints: Arc<LocalEndpoints>,
    deliveries: Arc<DeliveryIndex>,
    heartbeats: Arc<HeartbeatLedger>,
    gaps: Arc<GapLedger>,
    notices: Arc<NoticeFeed>,
    diagnostics: Arc<Diagnostics>,
    beacon: Arc<HeartbeatBeacon>,
    discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,

    /// The swarm. Held to the end of the process on purpose: dropping it stops
    /// the driver and closes every connection, and the three port adapters
    /// would then return their "unavailable" refusals rather than hanging.
    network: NetworkRuntime,
}

impl Node {
    /// Assembles and starts everything, in the order documented above.
    pub fn start(settings: &NodeSettings) -> Result<Self, StartError> {
        let stores = LocalStores::open(&settings.profile_directory)?;

        // ---------------------------------------------------------- identity
        let key_store = stores.identity_keys();
        let signer = Arc::new(key_store.load_or_create_signer()?);
        let local_peer = signer.peer();

        let identity = Arc::new(IdentityContext::new(
            key_store.clone(),
            stores.trust_records() as Arc<dyn TrustRecordStorePort + Send + Sync>,
        ));
        // AC1: zero interaction, and a display name derived from this peer's own
        // fingerprint rather than asked for.
        identity.commands().initialize_local_identity(None)?;

        // ------------------------------------------- transport identity (S3a)
        let mut secret = [0u8; 32];
        let network_identity = match key_store.load_or_create_transport_secret_key(&mut secret) {
            Ok(_) => {
                // Consumes and zeroes the buffer.
                NetworkIdentity::from_ed25519_secret_key(&mut secret)
            }
            Err(error) => {
                infra_store_fs::FileIdentityKeyStore::zeroize(&mut secret);
                return Err(StartError::KeyStore(error));
            }
        };
        let network_identity = match network_identity {
            Ok(identity) => identity,
            Err(error) => {
                infra_store_fs::FileIdentityKeyStore::zeroize(&mut secret);
                return Err(StartError::TransportIdentity(error));
            }
        };
        // Belt and braces: `from_ed25519_secret_key` clears the slice, and this
        // is what makes that a property of this function rather than of a
        // dependency's implementation.
        infra_store_fs::FileIdentityKeyStore::zeroize(&mut secret);

        debug_assert_eq!(
            network_identity.peer_id(),
            local_peer,
            "the swarm must authenticate as the peer the application is"
        );

        // ----------------------------------------------------------- network
        let network = NetworkRuntime::start(&network_identity, &settings.network)?;
        drop(network_identity);

        // ------------------------------------------------- shared collaborators
        let clock = Arc::new(SystemClock::now());
        let trust = Arc::new(TrustDirectory::new(
            stores.trust_records() as Arc<dyn TrustRecordStorePort + Send + Sync>
        ));
        let membership_events = Arc::new(MembershipEventRelay::new());
        let endpoints = Arc::new(LocalEndpoints::new());
        let deliveries = Arc::new(DeliveryIndex::new());
        let heartbeats = Arc::new(HeartbeatLedger::new());
        let gaps = Arc::new(GapLedger::new());
        let notices = Arc::new(NoticeFeed::new());
        let diagnostics = Arc::new(Diagnostics::default());
        // What this launch was *asked* to advertise (external-address canvas
        // D6). Whether any of it took hold arrives later and separately, as
        // `NetworkEvent::ExternalAddressConfirmed` reaches the event router —
        // two facts from two sources, because the failure this option is
        // reached for looks identical to success unless they are kept apart.
        diagnostics.record_supplied_external_addresses(&settings.network.external_addresses);
        let discovery =
            Arc::new(network.peer_discovery()) as Arc<dyn PeerDiscoveryPort + Send + Sync>;

        // -------------------------------------------------------- membership
        let membership = Arc::new(MembershipContext::new(
            MembershipSettings::for_local_peer(local_peer)
                .with_protocol(settings.network.protocol_version),
            Arc::clone(&clock) as Arc<dyn MembershipClockPort + Send + Sync>,
            Arc::new(network.peer_transport()) as Arc<dyn PeerTransportPort + Send + Sync>,
            Arc::clone(&discovery),
            stores.peer_cache() as Arc<dyn PeerCachePort + Send + Sync>,
            Arc::clone(&membership_events) as Arc<dyn MembershipPublisherPort + Send + Sync>,
        ));

        // --------------------------------------------------------- messaging
        let wire =
            Arc::new(network.message_transport()) as Arc<dyn MessageTransportPort + Send + Sync>;
        // The transport is wrapped so the root can recognise the
        // acknowledgement that arrives later, by signature (AC11).
        let transport = Arc::new(CorrelatingTransport::new(
            Arc::clone(&wire),
            Arc::clone(&deliveries),
        )) as Arc<dyn MessageTransportPort + Send + Sync>;

        let messaging = Arc::new(MessagingContext::new(
            MessagingSettings::for_local_peer(local_peer)
                .speaking(settings.network.protocol_version),
            MessagingPorts {
                clock: Arc::clone(&clock) as Arc<dyn MessagingClockPort + Send + Sync>,
                counter: stores.sequence_counter() as Arc<dyn SequenceCounterPort + Send + Sync>,
                // One signer over one key, behind all four crypto ports
                // (canvas §4): `identity`'s and `messaging`'s signer and
                // verifier are the same object.
                signer: signer.clone() as Arc<dyn EnvelopeSignerPort + Send + Sync>,
                verifier: signer.clone() as Arc<dyn EnvelopeVerifierPort + Send + Sync>,
                // Invariant 11: `messaging` asks its own question, `identity`
                // holds the answer, and neither imports the other.
                policy: Arc::clone(&trust) as Arc<dyn AuthorPolicyPort + Send + Sync>,
                transport: Arc::clone(&transport),
                log: stores.message_log() as Arc<dyn MessageLogPort + Send + Sync>,
                publisher: Arc::new(MessagingEventSink::new(
                    Arc::clone(&gaps),
                    Arc::clone(&diagnostics),
                )) as Arc<dyn MessagingPublisherPort + Send + Sync>,
            },
        ));

        // The beacon is given the **unwrapped** transport, and that is the
        // structural half of S6: a heartbeat cannot enter the delivery index
        // because it never passes through the thing that writes to it. The
        // wrapper would in fact decline to index one — a heartbeat's payload is
        // empty and does not decode into a `MessagePayload` — but that is a
        // property of what a heartbeat happens to carry, not a rule, and the
        // rule is worth having in the wiring where it can be read.
        //
        // What the beacon writes instead is `heartbeats`, which the event
        // router consults first on both delivery events.
        let beacon = Arc::new(HeartbeatBeacon::new(
            local_peer,
            settings.network.protocol_version,
            signer as Arc<dyn EnvelopeSignerPort + Send + Sync>,
            wire,
            Arc::clone(&heartbeats),
        ));

        Ok(Self {
            profile_directory: stores.root().to_path_buf(),
            local_peer,
            protocol: settings.network.protocol_version,
            clock,
            identity,
            membership,
            messaging,
            trust,
            membership_events,
            endpoints,
            deliveries,
            heartbeats,
            gaps,
            notices,
            diagnostics,
            beacon,
            discovery,
            network,
        })
    }

    // ------------------------------------------------------------- accessors

    /// This instance's stable identity (AC9).
    pub const fn local_peer(&self) -> PeerId {
        self.local_peer
    }

    /// The wire protocol this build speaks (S2).
    pub const fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// Where this instance's persistent state lives.
    pub fn profile_directory(&self) -> &Path {
        &self.profile_directory
    }

    pub fn clock(&self) -> &Arc<SystemClock> {
        &self.clock
    }

    pub fn identity(&self) -> &Arc<IdentityContext> {
        &self.identity
    }

    pub fn membership(&self) -> &Arc<MembershipContext> {
        &self.membership
    }

    pub fn messaging(&self) -> &Arc<MessagingContext> {
        &self.messaging
    }

    pub fn trust(&self) -> &Arc<TrustDirectory> {
        &self.trust
    }

    pub fn membership_events(&self) -> &Arc<MembershipEventRelay> {
        &self.membership_events
    }

    pub fn endpoints(&self) -> &Arc<LocalEndpoints> {
        &self.endpoints
    }

    pub fn deliveries(&self) -> &Arc<DeliveryIndex> {
        &self.deliveries
    }

    /// The signatures the beacon released, so a delivery report about a
    /// heartbeat is never read as one about a message (canvas `0010` S6).
    pub fn heartbeats(&self) -> &Arc<HeartbeatLedger> {
        &self.heartbeats
    }

    pub fn gaps(&self) -> &Arc<GapLedger> {
        &self.gaps
    }

    pub fn notices(&self) -> &Arc<NoticeFeed> {
        &self.notices
    }

    pub fn diagnostics(&self) -> &Arc<Diagnostics> {
        &self.diagnostics
    }

    pub fn beacon(&self) -> &Arc<HeartbeatBeacon> {
        &self.beacon
    }

    pub fn discovery(&self) -> &Arc<dyn PeerDiscoveryPort + Send + Sync> {
        &self.discovery
    }

    /// The bounded queue of everything the network reports. It must be drained
    /// (see `NetworkRuntime`'s threading contract).
    pub fn network_events(&self) -> Arc<NetworkEvents> {
        self.network.events()
    }

    /// The adapter's own counters: tolerated oddities, refusals, dropped
    /// events (S2, S6).
    pub fn codec_diagnostics(&self) -> CodecDiagnostics {
        self.network.diagnostics()
    }

    // -------------------------------------------------------------- actions

    /// Assembles a join ticket naming this peer and where it can be reached
    /// (D1).
    ///
    /// Everything decided here was decided elsewhere: the endpoints are what
    /// the transport reported, the protocol is this build's, the lifetime is
    /// the domain's `DEFAULT_LIFETIME`, and the expiry arithmetic is
    /// `JoinTicket::expiring_after`'s. The root supplies the facts.
    ///
    /// Fails while this peer has no endpoint yet — which is the truth at the
    /// instant after startup, before the transport has said where it is
    /// listening. A ticket with nothing to dial is not a bootstrap credential.
    pub fn mint_join_ticket(&self) -> Result<JoinTicket, JoinTicketError> {
        JoinTicket::expiring_after(
            self.local_peer,
            self.endpoints.all(),
            self.protocol,
            MembershipClockPort::now(self.clock.as_ref()),
            JoinTicket::DEFAULT_LIFETIME,
        )
    }

    /// Stops the swarm and waits for it to wind down.
    ///
    /// Explicit rather than left to `Drop` so the driver stops before the
    /// process starts tearing other things down, exactly as `NetworkRuntime`
    /// asks.
    pub fn shutdown(self) {
        self.network.shutdown();
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("local_peer", &self.local_peer)
            .field("profile_directory", &self.profile_directory)
            .finish_non_exhaustive()
    }
}

/// What a [`Node`] needs before it can be assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSettings {
    /// The directory holding this instance's identity, trust records, peer
    /// cache and sequence counter. Two instances on one machine need two.
    pub profile_directory: PathBuf,
    /// Everything about the swarm — listen addresses, LAN discovery, topic,
    /// protocol version, and the S6 caps. There is no bootstrap list in it and
    /// there never will be (S1).
    pub network: NetworkConfig,
}

/// Why an instance could not start.
///
/// Each variant is a condition visible at startup rather than something a peer
/// on the network can cause, and each names the step that refused — a user
/// whose key file is from a newer build needs a different answer from one whose
/// UDP port is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    /// The profile directory could not be created or made owner-only.
    Stores(LocalStoresError),
    /// The keypair could not be loaded or created (S4).
    KeyStore(IdentityKeyStoreError),
    /// The stored key is not one the transport can authenticate with.
    TransportIdentity(NetworkIdentityError),
    /// The swarm could not be built or started.
    Network(NetworkStartError),
}

impl From<LocalStoresError> for StartError {
    fn from(error: LocalStoresError) -> Self {
        Self::Stores(error)
    }
}

impl From<IdentityKeyStoreError> for StartError {
    fn from(error: IdentityKeyStoreError) -> Self {
        Self::KeyStore(error)
    }
}

impl From<NetworkStartError> for StartError {
    fn from(error: NetworkStartError) -> Self {
        Self::Network(error)
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stores(error) => write!(f, "{error}"),
            Self::KeyStore(error) => write!(f, "{error}"),
            Self::TransportIdentity(error) => write!(f, "{error}"),
            Self::Network(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StartError {}
