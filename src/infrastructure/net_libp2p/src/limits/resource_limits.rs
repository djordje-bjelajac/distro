use std::time::Duration;

/// Every hostile-input cap this adapter enforces, in one place (canvas §7/S6).
///
/// # Why the caps live here and nowhere else
///
/// A symmetric open network has no gatekeeper — no load balancer, no API
/// gateway, no operator who can add a rule later. The only place between a
/// stranger's bytes and this process's memory is this crate, so every bound the
/// canvas names is enforced at this boundary, and the ones that can be checked
/// *before* deserialization are (invariant 12): the wire framing refuses an
/// oversize frame from its length prefix alone, without ever allocating for it.
///
/// # Why a struct of fields rather than bare `const`s
///
/// The values below are the shipped defaults, each with the reasoning that
/// produced it. Keeping them as fields lets a test drive a limit down to
/// something a unit test can actually reach without waiting for 32 KiB of
/// traffic, while production reads [`ResourceLimits::DEFAULT`] and changes
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Largest encoded envelope accepted from the wire, in bytes.
    pub max_envelope_bytes: usize,
    /// Largest join-ticket string accepted for decoding, in bytes.
    pub max_ticket_bytes: usize,
    /// Largest number of simultaneously established connections.
    pub max_established_connections: u32,
    /// Largest number of established connections to one remote peer.
    pub max_established_per_peer: u32,
    /// Largest number of inbound connections still negotiating.
    pub max_pending_incoming: u32,
    /// Largest number of outbound connections still negotiating.
    pub max_pending_outgoing: u32,
    /// Messages buffered for one session before the connection stops taking
    /// more — S6's per-session buffer cap.
    pub max_session_buffered_messages: usize,
    /// Messages this peer will process out of a single gossip RPC.
    pub max_messages_per_rpc: usize,
    /// Sustained inbound envelopes accepted from one peer, per second.
    pub inbound_envelopes_per_second: u32,
    /// Inbound envelopes one peer may send in a burst before the sustained
    /// rate applies.
    pub inbound_envelope_burst: u32,
    /// Bytes this peer will carry for one relayed circuit before closing it.
    pub max_relay_circuit_bytes: u64,
    /// How long one relayed circuit may stay open.
    pub max_relay_circuit_duration: Duration,
    /// Simultaneous relayed circuits this peer will carry for one remote.
    pub max_relay_circuits_per_peer: usize,
    /// Simultaneous relayed circuits this peer will carry in total.
    pub max_relay_circuits: usize,
    /// Observed external addresses tracked at once, promoted ones included.
    pub max_candidate_addresses: usize,
    /// Distinct observers counted for one candidate external address.
    pub max_observers_per_address: usize,
    /// Addresses failed AutoNAT probes are remembered for.
    pub max_failing_addresses: usize,
    /// Undelivered network events buffered for the composition root.
    pub event_queue_capacity: usize,
    /// How long a synchronous port call waits for the driver's reply.
    pub request_timeout: Duration,
}

impl ResourceLimits {
    /// The shipped values. Each rationale is the canvas's or an engineering
    /// default per canvas §9 — none is user-visible policy.
    pub const DEFAULT: Self = Self {
        // S6, stated: 32 KiB. The domain caps a message body at 16 KiB, and an
        // envelope is a body plus a 32-byte key, a 64-byte signature, four
        // small integers, and CBOR framing — well under 17 KiB. The remaining
        // headroom is for additive minor evolution inside `payload` (S2,
        // architect Note 4) and nothing more.
        max_envelope_bytes: 32 * 1024,

        // A ticket carries one key, a handful of multiaddresses capped at
        // `Endpoint::MAX_ADDRESS_BYTES` (256) each, and two integers. 4 KiB
        // admits roughly a dozen endpoints, far more than any peer has, while
        // keeping a pasted string from becoming an allocation lever.
        max_ticket_bytes: 4 * 1024,

        // S6's "max concurrent sessions". A text-messaging peer talks to a
        // human-scale network; 256 established connections is generous for
        // that and still bounds the per-connection buffers, task handles, and
        // file descriptors a stranger can make this process hold.
        max_established_connections: 256,

        // Two, deliberately — not one. A simultaneous connect is the *normal*
        // case in a symmetric network (invariant 3), so both links must be
        // admissible long enough for the collapse rule to pick one. Capping at
        // one would have the transport reject the very event the domain has a
        // rule for.
        max_established_per_peer: 2,

        // Half-open inbound connections are the cheapest thing to flood, since
        // an attacker pays only a handshake. 64 in flight keeps the accept
        // path responsive while the established cap does the real work.
        max_pending_incoming: 64,

        // Outbound pending connections are self-inflicted: one per dial the
        // bootstrap ladder or the roster asked for. 64 covers a wide fan-out
        // without letting a retry loop spend the whole connection budget.
        max_pending_outgoing: 64,

        // S6's "per-session buffer cap", and the one libp2p default that has
        // to move: gossipsub buffers 5000 messages per connection out of the
        // box, which at the 32 KiB envelope cap is 160 MiB one peer could make
        // this process hold. 256 is 8 MiB worst case, and far more than a text
        // conversation ever queues.
        max_session_buffered_messages: 256,

        // A single RPC carrying an unbounded number of messages is the same
        // attack in one frame instead of many. Sixteen covers a legitimate
        // gossip burst; libp2p's default here is no limit at all.
        max_messages_per_rpc: 16,

        // S6's "per-peer inbound rate limit". A human types a few messages a
        // minute; 32 envelopes per second per peer is three orders of
        // magnitude above real use and still turns a flood into a counted
        // refusal rather than an unbounded read-model write.
        inbound_envelopes_per_second: 32,

        // A reconnecting peer legitimately arrives with a short burst — a
        // heartbeat, a backlog it is re-sending, an acknowledgement — so the
        // bucket starts full at two seconds' worth rather than empty.
        inbound_envelope_burst: 64,

        // S6's "max relay-service bandwidth per peer". Relaying is a service
        // this peer volunteers to *strangers* (AC4), which makes it the one
        // place a peer spends its bandwidth on traffic it cannot read. 8 MiB
        // per circuit carries a long text conversation and stops a circuit
        // from being used as a free file-transfer tunnel — a v1 exclusion.
        max_relay_circuit_bytes: 8 * 1024 * 1024,

        // Ten minutes: long enough that a relayed conversation is not
        // interrupted mid-sentence, short enough that an abandoned circuit
        // stops costing this peer anything. DCUtR is expected to upgrade a
        // relayed link to a direct one well inside this window.
        max_relay_circuit_duration: Duration::from_secs(600),

        // One remote peer may hold a few circuits (it may be relaying to
        // several destinations through us) but not an unbounded number.
        max_relay_circuits_per_peer: 4,

        // The total relay budget: 32 concurrent circuits at 8 MiB each bounds
        // what this peer can be made to carry for others.
        max_relay_circuits: 32,

        // An observed address is a *claim by a remote peer about us*, so the
        // ledger that holds them is fed entirely by untrusted input: a peer
        // that reports a fresh address on every identify exchange would grow
        // it without limit. A real peer has a handful of external addresses —
        // one per family per transport, so four or so — and 16 leaves room for
        // a multi-homed host and a genuine address change without letting a
        // hostile peer turn candidate tracking into an allocation lever.
        max_candidate_addresses: 16,

        // Corroboration needs two distinct observers, so at the shipped
        // threshold this cap can never bind — which is exactly why it is here.
        // It is the structural guarantee that a Sybil crowd agreeing on one
        // address costs a fixed eight peer identities' worth of memory however
        // many of them show up, and it keeps holding if a later piece raises
        // the threshold.
        max_observers_per_address: 8,

        // The failure half of reachability, and the mirror of the cap above.
        // An AutoNAT probe result is a *remote server's report about us*, for
        // an address libp2p picked out of the candidate pool that any peer can
        // add to, so the ledger holding those reports is fed by untrusted input
        // exactly as the candidate ledger is. Sixteen is the same number for
        // the same reason: a real peer has a handful of external addresses, and
        // a hostile one must not be able to turn failure evidence into an
        // allocation lever.
        //
        // One number is enough here where the candidate ledger needed two,
        // because an address is condemned the moment it reaches the
        // corroboration threshold and then stops taking evidence — so the
        // servers held per address are capped by the threshold itself, and
        // capping the addresses caps the whole structure.
        max_failing_addresses: 16,

        // The composition root drains events on its own loop. A queue of 1024
        // absorbs a burst without letting a stalled root grow this process's
        // memory without bound; overflow is counted, never silent.
        event_queue_capacity: 1024,

        // A synchronous port call must never hang (AC3). Ten seconds is above
        // any QUIC or relayed handshake that is going to succeed and below the
        // point where a user concludes the application is dead.
        request_timeout: Duration::from_secs(10),
    };
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
