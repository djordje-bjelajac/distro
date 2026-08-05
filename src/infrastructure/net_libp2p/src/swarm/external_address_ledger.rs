use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId as Libp2pPeerId};

/// How many **distinct** peers must report the same address before this peer
/// will advertise it (canvas D2).
///
/// Two, and the reasoning is worth keeping next to the number. An observed
/// address is a remote peer's claim *about us*. Advertising on one peer's word
/// lets a single hostile peer — identities being free — put an attacker-chosen
/// address into our join tickets and DHT records, which is a cheap misdirection
/// vector and free to attempt. Two distinct observers reporting an identical
/// address is the smallest rule that is not "trust anyone".
///
/// It is not a tuning knob (S2). Lowering it to one restores the vector in
/// full; raising it to five means a four-peer network never confirms anything
/// and the peer stays unreachable forever, which fails the requirement in the
/// common case.
///
/// It is also explicitly **interim** (D3). AutoNAT v2 confirms an address by
/// having another peer dial it back, which is proof rather than corroboration;
/// when that lands its verdict is authoritative and this heuristic becomes the
/// fallback for peers with no AutoNAT server available.
pub(crate) const CORROBORATION_THRESHOLD: usize = 2;

/// Which addresses other peers say they see us at, and which of those this peer
/// has decided to advertise.
///
/// # Why this is a separate type rather than three fields on the driver
///
/// "Should this address be advertised" is a decision with a security property
/// attached to it (S2), a filter that must not be bypassable (S3), and two
/// bounds on untrusted input (S5). Sitting inside the swarm's event loop it
/// would be testable only by standing up two swarms and hoping the right
/// packets arrived; here it is a pure function of its own state plus one
/// observation, and every rule above has a test that runs in microseconds and
/// cannot flake.
///
/// # Purity (invariant 5)
///
/// No swarm, no socket, no clock, no random source. The same sequence of
/// observations produces the same sequence of verdicts on every machine and
/// every run. Nothing here iterates a `HashMap`, so hash ordering cannot leak
/// into a decision either.
///
/// # What it deliberately does not do
///
/// Nothing expires. A confirmed address stays confirmed for the life of the
/// process even if it stops working — `SwarmEvent::ExternalAddrExpired` is
/// unhandled and `LocalEndpoints` has no removal path. That is a real gap,
/// recorded as a follow-up in canvas §9 rather than half-fixed here.
pub(crate) struct ExternalAddressLedger {
    /// This peer's own libp2p identity, so an observation attributed to
    /// ourselves can be refused rather than counted.
    local: Libp2pPeerId,
    /// Distinct observers required to promote. [`CORROBORATION_THRESHOLD`] in
    /// every build; a field only so the per-address observer cap can be proven
    /// to be enforced (see [`with_threshold`](Self::with_threshold)).
    threshold: usize,
    /// S5's cap on how many addresses are tracked at once, promoted included.
    max_addresses: usize,
    /// S5's cap on how many observers are counted for one address.
    max_observers_per_address: usize,
    /// Addresses awaiting corroboration, each with the distinct peers that have
    /// reported it.
    candidates: HashMap<Multiaddr, HashSet<Libp2pPeerId>>,
    /// Addresses already promoted. Kept so a repeat observation is refused
    /// rather than promoted a second time (invariant 1), and so a promoted
    /// address still counts against `max_addresses`.
    promoted: HashSet<Multiaddr>,
}

/// The ledger's verdict on one observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Promotion {
    /// The observation changed nothing, for the stated reason.
    Ignored(CandidateRejection),
    /// The observation was counted; the address is not yet advertised.
    Recorded {
        /// How many distinct peers have now reported this address.
        corroborations: usize,
    },
    /// The address has reached the threshold and should be advertised.
    Promote(Multiaddr),
}

/// Why an observation was ignored.
///
/// Hand-written rather than derived from an error crate: these are verdicts a
/// healthy peer produces constantly on an ordinary network — a LAN neighbour
/// reporting a private address is `NotGlobal` several times a minute — and none
/// of them is a failure anybody needs a backtrace for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateRejection {
    /// Not an address a stranger could dial: loopback, private, link-local,
    /// carrier-NAT, unspecified, multicast, or a relay circuit.
    NotGlobal,
    /// Already advertised. Promoting twice would re-enter the confirmation
    /// path for no new information (invariant 1).
    AlreadyPromoted,
    /// A bound was reached, so nothing was recorded (invariant 4).
    LedgerFull,
    /// The observation was attributed to this peer itself, which corroborates
    /// nothing.
    SelfObservation,
}

impl ExternalAddressLedger {
    /// A ledger for `local` with the shipped corroboration threshold.
    pub(crate) fn new(
        local: Libp2pPeerId,
        max_addresses: usize,
        max_observers_per_address: usize,
    ) -> Self {
        Self {
            local,
            threshold: CORROBORATION_THRESHOLD,
            max_addresses,
            max_observers_per_address,
            candidates: HashMap::new(),
            promoted: HashSet::new(),
        }
    }

    /// A ledger with a corroboration threshold other than the shipped one.
    ///
    /// Tests only, and for exactly one reason: at a threshold of two an address
    /// is promoted the instant a second observer reports it, so the per-address
    /// observer cap can never bind and a test of it would assert nothing.
    /// Raising the threshold is how that bound is shown to be enforced.
    /// Production calls [`new`](Self::new) and has no way to change it (S2).
    #[cfg(test)]
    pub(crate) fn with_threshold(
        local: Libp2pPeerId,
        threshold: usize,
        max_addresses: usize,
        max_observers_per_address: usize,
    ) -> Self {
        Self {
            threshold,
            ..Self::new(local, max_addresses, max_observers_per_address)
        }
    }

    /// Records one peer's claim about where it sees us, and decides what to do
    /// about it.
    ///
    /// The whole decision, in order, because the order is load-bearing:
    ///
    /// 1. **Is the observer somebody else?** An observation attributed to this
    ///    peer corroborates nothing.
    /// 2. **Is the address one a stranger could dial?** The filter sits here
    ///    rather than at the call site so a second call site cannot bypass it
    ///    (D5, S3) — and it sits *before* the counting, because two peers on
    ///    one LAN both observing `192.168.x.x` would otherwise meet the
    ///    threshold immediately.
    /// 3. **Is it already advertised?** Then there is nothing left to decide.
    /// 4. **Do the bounds admit it?** Candidate addresses arrive from untrusted
    ///    peers, so both caps refuse rather than grow (S5).
    /// 5. **Have enough distinct peers now said the same thing?**
    ///
    /// `observer` is the peer whose identify exchange produced this address.
    /// Passing an unattributed or guessed observer would make the threshold
    /// meaningless (S4), which is why there is no way to call this without one.
    pub(crate) fn observe(&mut self, observer: Libp2pPeerId, address: Multiaddr) -> Promotion {
        if observer == self.local {
            return Promotion::Ignored(CandidateRejection::SelfObservation);
        }

        if !is_globally_dialable(&address) {
            return Promotion::Ignored(CandidateRejection::NotGlobal);
        }

        if self.promoted.contains(&address) {
            return Promotion::Ignored(CandidateRejection::AlreadyPromoted);
        }

        let existing = self.candidates.get(&address);
        let already_counted = existing.is_some_and(|observers| observers.contains(&observer));

        // A peer already counted is not asking for a slot, so neither bound
        // applies to it.
        if !already_counted {
            if existing.is_none()
                && self.candidates.len() + self.promoted.len() >= self.max_addresses
            {
                return Promotion::Ignored(CandidateRejection::LedgerFull);
            }
            if existing.map_or(0, HashSet::len) >= self.max_observers_per_address {
                return Promotion::Ignored(CandidateRejection::LedgerFull);
            }
        }

        let observers = self.candidates.entry(address.clone()).or_default();
        observers.insert(observer);
        let corroborations = observers.len();

        if corroborations < self.threshold {
            return Promotion::Recorded { corroborations };
        }

        self.candidates.remove(&address);
        self.promoted.insert(address.clone());
        Promotion::Promote(address)
    }

    /// Whether `address` has been advertised.
    #[cfg(test)]
    pub(crate) fn is_promoted(&self, address: &Multiaddr) -> bool {
        self.promoted.contains(address)
    }

    /// How many addresses are awaiting corroboration.
    #[cfg(test)]
    pub(crate) fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// How many distinct peers have reported `address`.
    #[cfg(test)]
    pub(crate) fn corroborations(&self, address: &Multiaddr) -> usize {
        self.candidates.get(address).map_or(0, HashSet::len)
    }
}

/// Whether `address` is one a stranger on the open internet could dial.
///
/// # Why this is `pub(crate)` rather than private to the ledger
///
/// It has a second caller: an external address the operator *asserts* at
/// launch (`--external-address`, canvas `0008` D3) has to clear exactly the bar
/// an observed one does. Advertising `192.168.x.x` globally is no more useful
/// because a human typed it than because two peers agreed on it.
///
/// Reused rather than reimplemented, and reused *as a function* rather than as
/// a rule copied into the startup path: two predicates would agree on the day
/// they were written and drift on the first RFC anybody remembered on only one
/// side. [`NON_GLOBAL`] is the single table both callers are tested against for
/// the same reason.
///
/// It is still not a filter a call site can skip. Each caller applies it as its
/// *first* act — the ledger before it counts anything (below), the driver before
/// it advertises anything
/// ([`assert_external_address`](crate::swarm::network_driver::NetworkDriver::assert_external_address))
/// — so there is no path in this crate that reaches an advertisement without
/// passing through here.
///
/// # Why the answer is hand-written
///
/// `Ipv4Addr::is_global` and `Ipv6Addr::is_global` are unstable in `std`, so
/// the classes are spelled out. That is not a loss: the list below is the list
/// canvas D5 names, each entry is a class a peer would genuinely observe us at,
/// and a reader can check it against the RFCs without leaving the file.
///
/// # Why a relay circuit is refused
///
/// `/p2p-circuit` means "reachable *through* that peer", and the address before
/// the circuit belongs to the relay, not to us. It is a perfectly good address
/// — it is how a NAT-ed peer is reached (AC12) — but it is not evidence that
/// *this* peer is directly reachable, which is the only thing this piece
/// establishes. The scan therefore looks at the whole address rather than
/// stopping at the first IP.
///
/// # Why an address with no IP literal is refused
///
/// identify reports the socket address a connection arrived from, so a
/// candidate always carries an IP. Anything else — a `/dns4/…` name, a
/// `/memory/…` address — cannot be judged against the classes below, and
/// "cannot be judged" has to mean "not advertised".
///
/// Documentation ranges (RFC 5737, RFC 3849) are deliberately **not** refused:
/// nobody's real address falls in them, so refusing them buys no safety, and
/// they are the correct fixture for a test that must not name a real host.
pub(crate) fn is_globally_dialable(address: &Multiaddr) -> bool {
    let mut ip = None;

    for protocol in address.iter() {
        match protocol {
            Protocol::P2pCircuit => return false,
            Protocol::Ip4(v4) if ip.is_none() => ip = Some(IpAddr::V4(v4)),
            Protocol::Ip6(v6) if ip.is_none() => ip = Some(IpAddr::V6(v6)),
            _ => {}
        }
    }

    match ip {
        Some(IpAddr::V4(v4)) => is_global_v4(v4),
        Some(IpAddr::V6(v6)) => is_global_v6(v6),
        None => false,
    }
}

fn is_global_v4(address: Ipv4Addr) -> bool {
    !(address.is_unspecified()
        || address.is_loopback()
        // RFC 1918. The address two peers on one LAN observe each other at.
        || address.is_private()
        // 169.254/16 — a host that got no DHCP lease.
        || address.is_link_local()
        // Neither of these is a unicast address anything could dial back.
        || address.is_multicast()
        || address.is_broadcast()
        || is_carrier_grade_nat(address))
}

/// 100.64.0.0/10 — RFC 6598 carrier-grade NAT space.
///
/// A peer behind a carrier NAT is observed here by any peer inside the same
/// carrier network, so it corroborates as easily as a LAN address does, and it
/// is just as undialable from outside.
fn is_carrier_grade_nat(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    first == 100 && (64..=127).contains(&second)
}

fn is_global_v6(address: Ipv6Addr) -> bool {
    // `::ffff:a.b.c.d` is an IPv4 address wearing an IPv6 encoding, and the
    // IPv4 rules are the ones that describe it.
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_global_v4(mapped);
    }

    let leading = address.segments()[0];

    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        // fc00::/7 — RFC 4193 unique local addresses, the IPv6 equivalent of
        // RFC 1918.
        || (leading & 0xfe00) == 0xfc00
        // fe80::/10 — link-local unicast.
        || (leading & 0xffc0) == 0xfe80)
}

/// Every shape [`is_globally_dialable`] refuses, with the reason it is refused.
///
/// Deliberately written out rather than generated: each row is a class of
/// address a peer on the same LAN, behind the same carrier NAT, or on the same
/// host would observe us at — or that an operator might reasonably type — and
/// advertising any of them globally would publish an address a stranger cannot
/// dial.
///
/// It lives beside the predicate rather than inside one test file because it is
/// the predicate's specification and **both** of its callers are asserted
/// against it: the ledger, in `external_address_ledger_test.rs` (P1-5), and the
/// asserted-address path, in `network_driver_test.rs` (P3-8). One table, so
/// the two can never come to refuse different sets.
///
/// A `/p2p-circuit` address is refused too and is not a row here: it is built
/// from a public relay address rather than being a literal, so each test
/// constructs one from a peer identity it already has.
#[cfg(test)]
pub(crate) const NON_GLOBAL: [(&str, &str); 16] = [
    ("/ip4/127.0.0.1/tcp/4001", "IPv4 loopback"),
    ("/ip4/10.0.0.4/tcp/4001", "RFC 1918 private, 10/8"),
    ("/ip4/172.16.3.9/tcp/4001", "RFC 1918 private, 172.16/12"),
    ("/ip4/192.168.1.20/tcp/4001", "RFC 1918 private, 192.168/16"),
    ("/ip4/169.254.7.7/tcp/4001", "IPv4 link-local"),
    ("/ip4/100.64.0.1/tcp/4001", "CGNAT, low edge of 100.64/10"),
    (
        "/ip4/100.127.255.254/tcp/4001",
        "CGNAT, high edge of 100.64/10",
    ),
    ("/ip4/0.0.0.0/tcp/4001", "IPv4 unspecified"),
    ("/ip4/224.0.0.1/tcp/4001", "IPv4 multicast"),
    ("/ip4/255.255.255.255/tcp/4001", "IPv4 broadcast"),
    ("/ip6/::1/tcp/4001", "IPv6 loopback"),
    ("/ip6/::/tcp/4001", "IPv6 unspecified"),
    ("/ip6/fd00::1/tcp/4001", "IPv6 unique local, fc00::/7"),
    ("/ip6/fe80::1/tcp/4001", "IPv6 link-local, fe80::/10"),
    (
        "/ip6/::ffff:192.168.0.4/tcp/4001",
        "IPv4-mapped IPv6 carrying a private address",
    ),
    (
        "/dns4/example.com/tcp/4001",
        "no IP literal at all, so nothing can be judged global",
    ),
];
