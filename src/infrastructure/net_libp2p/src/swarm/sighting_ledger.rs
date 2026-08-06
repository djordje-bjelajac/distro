use std::collections::BTreeMap;
use std::time::Duration;

use membership::domain::{Endpoint, KnownPeer};
use membership::ports::DiscoveredPeer;
use shared_types::PeerId;

/// How long a sighting stays worth reporting after it was last refreshed.
///
/// # The retention rule, stated once
///
/// **A sighting is kept for [`SIGHTING_RETENTION`] from the last time it was
/// seen, and reading never removes it.** Nothing else expires it; nothing else
/// keeps it.
///
/// # Why reading does not remove it (canvas `0010` D12)
///
/// This buffer answers a *pull* — the join ladder's LAN rung — and a rung that
/// empties its own input works exactly once. The observed failure is precisely
/// that: the same unmoved instance joined over `local network` on its first
/// attempt and reported `local network: nothing to try` on its second, with a
/// live neighbour on the same link the whole time. A read is a question, and a
/// question must not consume its own answer.
///
/// # Why an age and not merely a count
///
/// The alternative to draining is accumulating, and both halves of that are
/// wrong. Sightings arrive from mDNS, which any host on the link can answer,
/// and from Kademlia routing updates, which any peer that can place a record in
/// the DHT can cause — so an unbounded buffer is an allocation lever fed by
/// strangers (canvas §7/S6). And an address a peer announced an hour ago is not
/// a candidate worth reporting even when memory is free: it is a dial that will
/// time out and delay the rung that would have worked.
///
/// # Why fifteen minutes
///
/// Three of libp2p's default mDNS query rounds — `mdns::Config::default()` asks
/// every five minutes and its records carry a six-minute TTL — so a peer that
/// is genuinely still on the link refreshes its sighting several times inside
/// the window and can never age out while it is there. A sighting that does
/// reach the end of it belongs to a peer that has missed three consecutive
/// announcement rounds and whose own mDNS record expired nine minutes earlier.
///
/// It is an engineering default (system canvas §9), not user-visible policy.
pub(crate) const SIGHTING_RETENTION: Duration = Duration::from_secs(15 * 60);

/// The peers discovery has seen recently, and what they said they answer at.
///
/// # Why this is a separate type rather than a field on the driver
///
/// The same reason [`ExternalAddressLedger`] and [`ReachabilityLedger`] are:
/// the rules it holds — a bound on untrusted input, an eviction order, and an
/// age — are decisions with consequences, and inside the swarm's event loop
/// they would be testable only by standing up real peers and waiting real
/// minutes. Here they are a pure function of the ledger's own state, one
/// sighting, and one instant, so every rule below has a test that runs in
/// microseconds and cannot flake.
///
/// # What is evicted when it is full
///
/// The least recently seen sighting, ties broken in [`PeerId`] order so a flood
/// evicts deterministically. Freshness is the only thing this type knows: it
/// holds *claims*, not evidence — an mDNS record is answerable by any host on
/// the link and a DHT record by anyone who can write one — so there is no
/// proven entry here to protect, and the peer that announced itself most
/// recently is the one most likely to still answer a dial. Weighing evidence,
/// and never evicting a peer that produced some, is the roster's rule (canvas
/// `0010` D9); this is a buffer in front of the roster, not a second copy of it.
///
/// # Purity
///
/// No swarm, no socket, no clock. Time arrives as an argument, so the same
/// sequence of sightings produces the same contents on every machine and every
/// run, and the iteration order is [`PeerId`] order rather than a hash order.
///
/// [`ExternalAddressLedger`]: crate::swarm::external_address_ledger::ExternalAddressLedger
/// [`ReachabilityLedger`]: crate::swarm::reachability_ledger::ReachabilityLedger
pub(crate) struct SightingLedger {
    /// How many peers may be held at once — canvas §7/S6's bound on this
    /// buffer, supplied from [`ResourceLimits`](crate::limits::ResourceLimits).
    max_peers: usize,
    /// [`SIGHTING_RETENTION`] in milliseconds, to match the driver's monotonic
    /// counter.
    retention_millis: u64,
    /// One entry per peer. A `BTreeMap` rather than a `HashMap` so what
    /// `observed` reports is ordered by identity and not by hash seed.
    sightings: BTreeMap<PeerId, Sighting>,
}

/// One peer, the endpoints it has claimed, and when it was last heard from.
struct Sighting {
    endpoints: Vec<Endpoint>,
    last_seen_millis: u64,
}

impl SightingLedger {
    /// An empty ledger holding at most `max_peers` sightings for `retention`.
    pub(crate) fn new(max_peers: usize, retention: Duration) -> Self {
        Self {
            max_peers,
            retention_millis: u64::try_from(retention.as_millis()).unwrap_or(u64::MAX),
            sightings: BTreeMap::new(),
        }
    }

    /// Records that `peer` was seen at `endpoints`, now.
    ///
    /// A peer already held is refreshed rather than duplicated: its endpoints
    /// are merged and its retention window starts again. A peer not held is
    /// admitted, evicting the stalest sighting first if the ledger is full.
    pub(crate) fn record(&mut self, peer: PeerId, endpoints: &[Endpoint], now_millis: u64) {
        self.expire(now_millis);

        if let Some(existing) = self.sightings.get_mut(&peer) {
            merge(&mut existing.endpoints, endpoints);
            existing.last_seen_millis = now_millis;
            return;
        }

        self.make_room();

        // Nothing left to evict and still no room: the sighting is refused
        // rather than admitted over the cap. Only reachable at a capacity of
        // zero — a configuration mistake, not a state a shipped build is in —
        // but a bound that overflows in its degenerate case is not a bound.
        if self.sightings.len() >= self.max_peers {
            return;
        }

        let mut kept = Vec::new();
        merge(&mut kept, endpoints);
        self.sightings.insert(
            peer,
            Sighting {
                endpoints: kept,
                last_seen_millis: now_millis,
            },
        );
    }

    /// Every sighting still inside its retention window, in [`PeerId`] order.
    ///
    /// **Non-destructive.** Calling this twice in a row answers the same thing
    /// twice, which is the whole of acceptance criterion A7: a second join has
    /// the same LAN rung available to it that the first one had. It takes
    /// `&mut self` only to drop what has aged out, so a ledger nobody reads and
    /// nobody feeds still does not grow.
    pub(crate) fn observed(&mut self, now_millis: u64) -> Vec<DiscoveredPeer> {
        self.expire(now_millis);
        self.sightings
            .iter()
            .map(|(peer, sighting)| DiscoveredPeer {
                peer: *peer,
                endpoints: sighting.endpoints.clone(),
            })
            .collect()
    }

    /// How many sightings are held, aged-out ones included.
    ///
    /// Test-only: the bound is asserted against this rather than against the
    /// length of what `observed` returns, so a ledger that quietly kept
    /// everything and filtered on the way out would still fail.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.sightings.len()
    }

    /// Drops every sighting older than the retention window.
    ///
    /// Applied on both paths — read and write — so neither a process that
    /// stops discovering nor one that stops joining can hold a sighting past
    /// its window.
    fn expire(&mut self, now_millis: u64) {
        let retention = self.retention_millis;
        self.sightings
            .retain(|_, sighting| now_millis.saturating_sub(sighting.last_seen_millis) < retention);
    }

    /// Evicts until one more sighting fits, stalest first (see the type's
    /// eviction rule).
    fn make_room(&mut self) {
        while self.sightings.len() >= self.max_peers {
            let Some(stalest) = self
                .sightings
                .iter()
                .min_by_key(|(peer, sighting)| (sighting.last_seen_millis, **peer))
                .map(|(peer, _)| *peer)
            else {
                return;
            };
            self.sightings.remove(&stalest);
        }
    }
}

/// Adds the endpoints not already present, then keeps only the newest
/// [`KnownPeer::MAX_ENDPOINTS`].
///
/// The cap is the domain's own, referenced rather than restated: the roster
/// applies it to everything this adapter reports, so buffering more than that
/// would hold addresses that are discarded the moment they are used — which is
/// exactly the free growth a peer announcing thousands of addresses would be
/// paying nothing for. The oldest go first, the same end `KnownPeer` drops, so
/// the addresses a peer most recently claimed are the ones kept.
fn merge(into: &mut Vec<Endpoint>, incoming: &[Endpoint]) {
    for endpoint in incoming {
        if !into.contains(endpoint) {
            into.push(endpoint.clone());
        }
    }

    let excess = into.len().saturating_sub(KnownPeer::MAX_ENDPOINTS);
    into.drain(..excess);
}
