//! `infra-sim-net`: the deterministic in-process network, virtual clock, and
//! multi-peer harness every behavioural claim in this project is verified
//! through (canvas OP-8, safeguard S5).
//!
//! # What this crate is for
//!
//! > *"The sim-net harness is the required vehicle for every multi-peer
//! > behaviour claim."* — canvas §7/S5
//!
//! Domain and application tests prove one context's rules against port fakes.
//! They cannot prove that two peers agree — that a message sent by one is
//! displayed by the other in the author's order, that a departure is noticed,
//! that a restarted peer is still heard. Those claims need N peers, a network
//! between them, and a way to make the network misbehave on purpose. This crate
//! is that, and OP-9's scenarios are written entirely against it.
//!
//! # The determinism contract (AC13)
//!
//! Three properties, each self-tested:
//!
//! 1. **The clock never advances on its own.** There is no `std::time` anywhere
//!    in this crate. [`VirtualClock`] moves only when a scenario moves it.
//! 2. **The fabric delivers nothing without an explicit pump.** A frame handed
//!    to a transport is queued with a due instant and stays there.
//! 3. **The same seed and the same script produce a byte-identical trace.**
//!    Delivery order is `(due_at, enqueue_id)` — both integers — every
//!    collection iterated to build a delivery set is a `BTree*`, and the only
//!    chance in the simulation is [`SeededRng`], seeded once per network.
//!
//! `rand`, `tokio`, threads, and real sockets are all deliberately absent: each
//! would put an outcome outside the scenario's control.
//!
//! # Real crypto, not a digest fake
//!
//! Signing and verification are genuine Ed25519 over
//! [`Envelope::signable_bytes`](shared_types::Envelope::signable_bytes), and a
//! `PeerId` **is** the verifying key (invariant 1), so no lookup is involved.
//! AC6 is about forged envelopes being refused before the read model; verifying
//! that against a stand-in would be a claim about the stand-in.
//!
//! # It is not linked into the product
//!
//! `app` (OP-12) must never depend on this crate, and no context crate may
//! depend on it in either direction. It is composition-layer test
//! infrastructure: it may depend on all three contexts — the rule that contexts
//! never import each other binds the context crates, not the root that wires
//! them — and nothing may depend on it but `tests/integration`.
//!
//! # Starting point
//!
//! [`SimNetwork`] is the entry point; [`SimPeer`] is one peer's three
//! contexts; [`SimFabric`] is the network underneath them.

mod clock;
mod crypto;
mod fabric;
mod harness;
mod rng;
mod stores;
mod trace;

pub use clock::VirtualClock;
pub use crypto::{SimKeyStore, SimKeypair, SimSigner, SimVerifier};
pub use fabric::{
    DialFault, DropCause, FrameLabel, LinkPolicy, QueuedFrame, SimFabric, SimFrame,
    SimMessageTransport, SimPeerDiscovery, SimPeerTransport,
};
pub use harness::{DurablePeerState, SimNetwork, SimNetworkBuilder, SimPeer, SimSettings};
pub use rng::SeededRng;
pub use stores::{
    InMemoryMessageLog, InMemoryPeerCache, InMemoryTrustRecords, PersistentSequenceCounter,
    TrustRecordAuthorPolicy,
};
pub use trace::{
    EventTrace, MembershipEventRecorder, MessagingEventRecorder, PeerLifecycle, TraceEntry,
    TraceEvent,
};
