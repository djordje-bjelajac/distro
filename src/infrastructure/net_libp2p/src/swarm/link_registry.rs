use std::collections::{BTreeMap, HashMap};
use std::fmt;

use libp2p::Multiaddr;
use libp2p::swarm::ConnectionId;
use membership::domain::{SessionCollapse, SessionCollapseError, SessionDirection};
use shared_types::PeerId;

/// Which libp2p connection carries the one logical session with each peer.
///
/// # The problem this type exists to solve
///
/// libp2p is happy to hold several connections to the same peer and multiplex
/// streams over any of them; the domain models exactly one [`Session`] per peer
/// and has a rule (invariant 3) for what to do when two appear at once. Those
/// two views have to be reconciled somewhere, and the only place that can see
/// both is here — the domain has no connection handles, and libp2p has no
/// opinion about which link is "the" session.
///
/// # Why the collapse is resolved *below* the port and not above it
///
/// `PeerTransportPort::close_session` closes **by peer**. During a simultaneous
/// connect this peer holds two links to that same peer, so a close-by-peer
/// cannot name one of them — which is the port-granularity gap OP-6 flagged.
/// The resolution is not to widen the port but to remove the need: this
/// registry applies the domain's own [`SessionCollapse::resolve`] the instant a
/// second connection appears, closes the superseded link itself, and reports
/// only the survivor upward. The application therefore never holds two sessions
/// for one peer, `SessionOutcome::superseded` is never populated, and
/// `close_session(peer)` carries exactly one unambiguous meaning: *this peer's
/// session is over, close everything*.
///
/// The rule itself is **not** re-derived. `SessionCollapse::resolve` is a pure
/// domain function over the two identities; both peers evaluate the same one
/// and reach the same answer without exchanging a message. Re-implementing it
/// here is how a running network splits into halves that disagree about which
/// link to keep.
///
/// # Determinism
///
/// Connections are held in a `BTreeMap` keyed by [`ConnectionId`], so "the
/// oldest remaining link" and "the survivor among same-direction duplicates"
/// are decided by an ordering rather than by hash iteration order.
///
/// [`Session`]: membership::domain::Session
#[derive(Debug)]
pub(crate) struct LinkRegistry {
    local: PeerId,
    peers: HashMap<libp2p::PeerId, PeerLinks>,
}

#[derive(Debug)]
struct PeerLinks {
    primary: ConnectionId,
    links: BTreeMap<ConnectionId, LinkRecord>,
}

#[derive(Debug, Clone)]
struct LinkRecord {
    direction: SessionDirection,
    address: Multiaddr,
}

/// What a newly established connection changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EstablishedOutcome {
    /// The connection that now carries the logical session.
    pub(crate) primary: ConnectionId,
    /// The address the primary connection is on — direct or through a relay
    /// circuit, which is what the reachability class is read from (AC12).
    pub(crate) primary_address: Multiaddr,
    /// Connections to close: the link the collapse rule discarded, or a
    /// redundant duplicate dial. Empty in the ordinary case.
    pub(crate) close: Vec<ConnectionId>,
    /// Whether the peer had no session at all before this connection. The one
    /// condition under which a session is announced upward.
    pub(crate) newly_connected: bool,
    /// The collapse decision, when a simultaneous connect was resolved. Kept
    /// for diagnostics: the decision is already carried out by the time this
    /// is returned.
    pub(crate) collapse: Option<SessionCollapse>,
}

/// What a closed connection changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClosedOutcome {
    /// Whether the connection that closed was the session-bearing one.
    pub(crate) was_primary: bool,
    /// The connection that took over, when another link remains.
    pub(crate) new_primary: Option<ConnectionId>,
    /// Whether the peer has no link left. The one condition under which a
    /// session close is announced upward.
    pub(crate) peer_gone: bool,
}

impl LinkRegistry {
    pub(crate) fn new(local: PeerId) -> Self {
        Self {
            local,
            peers: HashMap::new(),
        }
    }

    /// Records a newly established connection and decides which link now
    /// carries the session.
    ///
    /// `remote_identity` is the same peer as `remote`, already mapped into the
    /// domain's terms — the caller has it, and re-deriving it here would put a
    /// fallible mapping inside a bookkeeping method.
    pub(crate) fn record_established(
        &mut self,
        remote_identity: PeerId,
        remote: libp2p::PeerId,
        connection: ConnectionId,
        direction: SessionDirection,
        address: Multiaddr,
    ) -> Result<EstablishedOutcome, LinkRegistryError> {
        if remote_identity == self.local {
            return Err(LinkRegistryError::SelfConnection);
        }

        let record = LinkRecord { direction, address };

        let Some(existing) = self.peers.get_mut(&remote) else {
            self.peers.insert(
                remote,
                PeerLinks {
                    primary: connection,
                    links: BTreeMap::from([(connection, record.clone())]),
                },
            );

            return Ok(EstablishedOutcome {
                primary: connection,
                primary_address: record.address,
                close: Vec::new(),
                newly_connected: true,
                collapse: None,
            });
        };

        existing.links.insert(connection, record);

        // Two links to one peer. If they came from opposite directions this is
        // the simultaneous connect invariant 3 describes, and the domain picks
        // the survivor. If they came from the same direction it is a redundant
        // dial, not a collapse, and the oldest link keeps the session.
        let directions: Vec<SessionDirection> =
            existing.links.values().map(|link| link.direction).collect();
        let simultaneous = directions.contains(&SessionDirection::Outbound)
            && directions.contains(&SessionDirection::Inbound);

        let collapse = if simultaneous {
            Some(SessionCollapse::resolve(self.local, remote_identity)?)
        } else {
            None
        };

        let survivor = match collapse {
            Some(decision) => existing
                .links
                .iter()
                .find(|(_, link)| link.direction == decision.survivor())
                .map(|(id, _)| *id),
            None => existing.links.keys().next().copied(),
        }
        .unwrap_or(connection);

        existing.primary = survivor;
        let close = existing
            .links
            .keys()
            .filter(|id| **id != survivor)
            .copied()
            .collect();
        let primary_address = existing
            .links
            .get(&survivor)
            .map(|link| link.address.clone())
            // Unreachable: `survivor` is a key of `links`, chosen from it just
            // above. Falling back to the connection's own address keeps the
            // impossible branch honest instead of panicking in an adapter.
            .unwrap_or_else(|| address_of(existing, connection));

        Ok(EstablishedOutcome {
            primary: survivor,
            primary_address,
            close,
            // The peer already had a session; swapping which link carries it
            // is invisible above this line, and announcing it would make the
            // roster see a second open for a peer it already holds.
            newly_connected: false,
            collapse,
        })
    }

    /// Records a connection closing, whichever side closed it.
    pub(crate) fn record_closed(
        &mut self,
        remote: libp2p::PeerId,
        connection: ConnectionId,
    ) -> Option<ClosedOutcome> {
        let links = self.peers.get_mut(&remote)?;
        links.links.remove(&connection)?;

        let was_primary = links.primary == connection;

        let Some(new_primary) = links.links.keys().next().copied() else {
            self.peers.remove(&remote);
            return Some(ClosedOutcome {
                was_primary,
                new_primary: None,
                peer_gone: true,
            });
        };

        if was_primary {
            links.primary = new_primary;
        }

        Some(ClosedOutcome {
            was_primary,
            new_primary: Some(new_primary),
            peer_gone: false,
        })
    }

    /// Forgets every link to `remote`, for use after closing them all.
    pub(crate) fn forget(&mut self, remote: &libp2p::PeerId) {
        self.peers.remove(remote);
    }

    /// Every live connection to `remote`, oldest first.
    pub(crate) fn connections_of(&self, remote: &libp2p::PeerId) -> Vec<ConnectionId> {
        self.peers
            .get(remote)
            .map(|links| links.links.keys().copied().collect())
            .unwrap_or_default()
    }

    /// The address of the session-bearing link to `remote`, when there is one.
    pub(crate) fn primary_address(&self, remote: &libp2p::PeerId) -> Option<Multiaddr> {
        let links = self.peers.get(remote)?;
        links
            .links
            .get(&links.primary)
            .map(|link| link.address.clone())
    }

    /// Whether a session with `remote` exists at all.
    pub(crate) fn holds_session(&self, remote: &libp2p::PeerId) -> bool {
        self.peers.contains_key(remote)
    }

    /// How many peers currently hold a session.
    #[cfg(test)]
    pub(crate) fn session_count(&self) -> usize {
        self.peers.len()
    }
}

/// Why a connection could not be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkRegistryError {
    /// The connection claims this peer's own identity (invariant 2).
    SelfConnection,
}

impl From<SessionCollapseError> for LinkRegistryError {
    fn from(_: SessionCollapseError) -> Self {
        // `resolve` has exactly one failure — the two identities are the same
        // key — and that is this peer connecting to itself.
        Self::SelfConnection
    }
}

impl fmt::Display for LinkRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfConnection => {
                f.write_str("a connection claiming this peer's own identity was refused")
            }
        }
    }
}

impl std::error::Error for LinkRegistryError {}

/// The address recorded for `connection`, or the empty multiaddress when the
/// connection is not in `links` — which the caller has already established
/// cannot happen.
fn address_of(links: &PeerLinks, connection: ConnectionId) -> Multiaddr {
    links
        .links
        .get(&connection)
        .map(|link| link.address.clone())
        .unwrap_or_else(Multiaddr::empty)
}
