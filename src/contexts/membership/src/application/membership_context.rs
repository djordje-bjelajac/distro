use std::sync::Arc;

use crate::application::commands::{InboundSessionService, JoinNetworkService};
use crate::application::queries::MembershipQueryService;
use crate::application::{MembershipSettings, MembershipState};
use crate::ports::{
    ClockPort, EventPublisherPort, PeerCachePort, PeerDiscoveryPort, PeerTransportPort,
};

/// The assembled `membership` context: its three inbound ports, wired over the
/// outbound ports a composition root supplies.
///
/// # Why all three are built together
///
/// CQRS separates the command and query *paths*, not the state they describe.
/// The two command services and the query service must see one
/// [`MembershipState`], or a session the network just reported would be
/// invisible to the roster pane — a defect that surfaces only at runtime, in
/// the UI, as a peer that never appears. Constructing them here makes that
/// mistake unrepresentable at the root: there is no way to hand them different
/// rosters.
///
/// # What OP-12 wires
///
/// The root supplies `infra-net-libp2p`'s [`PeerTransportPort`] and
/// [`PeerDiscoveryPort`], `infra-store-fs`'s [`PeerCachePort`], one
/// [`ClockPort`], and an [`EventPublisherPort`] that fans this context's
/// cross-context events out to `messaging` — then drives the context through
/// [`join`](Self::join) as `&dyn JoinNetworkPort` (startup, the UI's join and
/// leave), [`sessions`](Self::sessions) as `&dyn InboundSessionPort` (the
/// network pump and the liveness tick), and [`queries`](Self::queries) as
/// `&dyn MembershipQueryPort` (every redraw).
///
/// Nothing here starts a task, opens a socket, or reads a clock: the context is
/// inert until a command arrives. In particular, **no timer is started** — the
/// presence sweep is driven from outside through `InboundSessionPort`, which is
/// what keeps every test in this crate free of real time (AC13).
pub struct MembershipContext {
    join: JoinNetworkService,
    sessions: InboundSessionService,
    queries: MembershipQueryService,
}

impl MembershipContext {
    /// Assembles all three inbound ports over the given outbound ports.
    pub fn new(
        settings: MembershipSettings,
        clock: Arc<dyn ClockPort + Send + Sync>,
        transport: Arc<dyn PeerTransportPort + Send + Sync>,
        discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,
        cache: Arc<dyn PeerCachePort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        let state = Arc::new(MembershipState::for_local_peer(settings.local_peer));

        Self {
            join: JoinNetworkService::new(
                settings,
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&transport),
                discovery,
                cache,
                Arc::clone(&publisher),
            ),
            sessions: InboundSessionService::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                transport,
                publisher,
                settings.liveness_windows,
            ),
            queries: MembershipQueryService::new(state, clock, settings.liveness_windows),
        }
    }

    /// The inbound port for decisions: join, leave, connect to a peer.
    pub const fn join(&self) -> &JoinNetworkService {
        &self.join
    }

    /// The inbound port for reports: discovery, session lifecycle, heartbeats,
    /// and the presence sweep.
    pub const fn sessions(&self) -> &InboundSessionService {
        &self.sessions
    }

    /// The inbound port for reads. Nothing behind it writes.
    pub const fn queries(&self) -> &MembershipQueryService {
        &self.queries
    }

    /// Splits the context so a root can hand each side to a different owner —
    /// the network pump, the UI task, the liveness ticker — while all three
    /// keep the shared roster this constructor established.
    pub fn into_parts(
        self,
    ) -> (
        JoinNetworkService,
        InboundSessionService,
        MembershipQueryService,
    ) {
        (self.join, self.sessions, self.queries)
    }
}
