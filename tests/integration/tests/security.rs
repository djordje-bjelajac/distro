//! What a peer refuses, and what it tolerates: signatures, block lists, and
//! wire versions (canvas AC6, AC14, invariants 10 and 11, safeguard S2).
//!
//! The forgery here is a real one — genuine Ed25519 over the envelope's
//! signable bytes, corrupted in flight — because a claim about a stand-in
//! signature would be a claim about the stand-in.

use std::sync::Arc;

use identity::domain::VerificationState;
use infra_sim_net::{SimNetwork, SimSigner};
use messaging::domain::events::{MessagingEvent, RejectionReason};
use messaging::domain::{ConversationId, DeliveryState, MessageBody, Millis, SequenceNumber};
use messaging::ports::{EnvelopeSignerPort, InboundVerdict, MessagePayload, UnsignedEnvelope};
use shared_types::{Envelope, PayloadKind, PeerId, ProtocolVersion};

/// One seed for the whole file, written down rather than picked implicitly
/// (AC13).
const SEED: u64 = 90_003;

fn network() -> SimNetwork {
    SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob", "carol"])
        .build()
}

/// The gap-tolerance window every peer in `net` was assembled with (rule R).
fn gap_tolerance(net: &SimNetwork) -> u64 {
    net.settings().gap_tolerance.as_millis()
}

/// Whether `peer` recorded a refusal with this reason.
fn refused_for(net: &SimNetwork, peer: PeerId, reason: RejectionReason) -> bool {
    net.trace().messaging_events_of(peer).iter().any(|event| {
        matches!(
            event,
            MessagingEvent::MessageRejected(rejected) if rejected.reason == reason
        )
    })
}

// ---------------------------------------------------------------------------
// AC6, invariant 10 — nothing unverified reaches a read model
// ---------------------------------------------------------------------------

#[test]
fn a_forged_signature_never_reaches_any_read_model() {
    // AC6: every displayed message is signature-verified against the author's
    // `PeerId`; invalid envelopes are rejected before the read model and
    // counted in local diagnostics. The envelope leaves alice genuine and
    // arrives with a flipped signature bit, so the refusal happens exactly at
    // the boundary invariant 10 names.
    let net = network();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );
    net.boot_all();
    net.clear_trace();

    // A 1:1 message is one frame, so "the next message frame" is exactly this
    // message and nothing else.
    net.corrupt_next_signatures(1);
    let forged = net
        .peer(alice)
        .send_direct(bob, "tampered in flight")
        .expect("the session is up");
    net.settle();

    assert!(
        net.peer(bob).direct_history(alice).is_empty(),
        "a forged envelope reached the conversation"
    );
    assert!(
        net.peer(bob).broadcast_history().is_empty(),
        "a forged envelope reached some other read model"
    );
    assert!(
        refused_for(&net, bob, RejectionReason::SignatureInvalid),
        "the refusal was not counted in local diagnostics"
    );

    // The sender is not told it landed: acknowledging content that reached no
    // read model would make a forgery look like a delivery.
    assert_eq!(
        net.peer(alice).delivery_state(forged.sent.id),
        Some(DeliveryState::Pending)
    );

    // The boundary refused an envelope, not a peer: bob is still listening, and
    // untampered traffic is displayed as usual.
    net.peer(carol)
        .publish_broadcast("genuine")
        .expect("gossip accepts it");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["genuine"]
    );
}

// ---------------------------------------------------------------------------
// Invariant 11 — blocking drops traffic, and only here
// ---------------------------------------------------------------------------

#[test]
fn a_blocked_peers_messages_are_dropped_and_the_block_is_purely_local() {
    // Invariant 11: a blocked peer's envelopes are dropped at the application
    // boundary of every context, and blocking is local — no other peer's view
    // changes, and nothing about it travels.
    let net = network();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );
    net.boot_all();

    net.peer(bob)
        .block(alice)
        .expect("alice is not blocked yet");
    let state = net.peer(bob).trust_state(alice).expect("a healthy store");
    assert!(state.blocked);
    assert_eq!(state.verification, VerificationState::Unverified);

    net.peer(alice)
        .publish_broadcast("unwelcome")
        .expect("gossip accepts it");
    net.settle();

    assert!(
        net.peer(bob).broadcast_history().is_empty(),
        "a blocked author's message was displayed"
    );
    assert!(refused_for(&net, bob, RejectionReason::AuthorBlocked));

    // Purely local: carol has the same message, and knows nothing of the block.
    assert_eq!(
        net.peer(carol).transcript(ConversationId::Broadcast),
        ["unwelcome"]
    );
    assert_eq!(
        net.peer(bob).blocked_peers().expect("a healthy store"),
        vec![alice]
    );
    assert!(
        net.peer(carol)
            .blocked_peers()
            .expect("a healthy store")
            .is_empty()
    );

    // And reversible. What was dropped stays dropped — it is loss, and rule R
    // treats it as loss — so the next thing alice says becomes visible one
    // gap-tolerance window after it arrives, rather than never.
    net.peer(bob).unblock(alice).expect("alice is blocked");
    net.peer(alice)
        .publish_broadcast("welcome back")
        .expect("gossip accepts it");
    net.settle();
    net.advance(gap_tolerance(&net));
    net.tick();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["welcome back"]
    );
}

// ---------------------------------------------------------------------------
// AC14, S2 — the wire compatibility rule, both halves
// ---------------------------------------------------------------------------

#[test]
fn an_envelope_with_an_unsupported_major_version_is_rejected_with_a_stated_reason() {
    // AC14: an unsupported major version is rejected with a logged reason.
    // Peers upgrade independently and there is no coordinated deploy, so this
    // is an ordinary event on an open network, not an attack.
    let net = network();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();
    net.clear_trace();

    let next_major = ProtocolVersion::new(ProtocolVersion::CURRENT.major + 1, 0);
    let from_the_future = signed(
        &net,
        alice,
        next_major,
        PayloadKind::BroadcastMessage,
        SequenceNumber::FIRST,
        "sent by a build nobody here speaks",
    );

    let verdict = net
        .peer(bob)
        .accept_envelope(from_the_future)
        .expect("the boundary judges it rather than failing");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::UnsupportedProtocolVersion)
    );
    assert!(matches!(verdict, InboundVerdict::RefusedAtBoundary(_)));
    assert!(
        net.peer(bob).broadcast_history().is_empty(),
        "an envelope from an unsupported major version was displayed"
    );
    assert!(
        refused_for(&net, bob, RejectionReason::UnsupportedProtocolVersion),
        "the rejection was not logged with its reason"
    );

    // The control: the same author, key, channel, sequence, and body at the
    // supported version is taken. The version is what was refused.
    let speakable = signed(
        &net,
        alice,
        net.settings().protocol,
        PayloadKind::BroadcastMessage,
        SequenceNumber::FIRST,
        "sent by a build nobody here speaks",
    );

    assert!(
        net.peer(bob)
            .accept_envelope(speakable)
            .expect("the boundary judges it")
            .is_applied()
    );
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["sent by a build nobody here speaks"]
    );
}

#[test]
fn an_envelope_with_a_newer_minor_version_is_tolerated_and_displayed() {
    // S2's other half: within one major version, a newer peer's additions are
    // tolerated rather than refused. Rejecting them would make every
    // uncoordinated upgrade a partition.
    let net = network();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let newer_minor = ProtocolVersion::new(
        ProtocolVersion::CURRENT.major,
        ProtocolVersion::CURRENT.minor + 1,
    );
    let envelope = signed(
        &net,
        alice,
        newer_minor,
        PayloadKind::BroadcastMessage,
        SequenceNumber::FIRST,
        "from a slightly newer build",
    );

    assert!(
        net.peer(bob)
            .accept_envelope(envelope)
            .expect("the boundary judges it")
            .is_applied()
    );
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["from a slightly newer build"]
    );
}

#[test]
fn an_unknown_payload_kind_is_ignored_rather_than_refused() {
    // S2 again: an unknown payload kind is a newer peer speaking, not a fault.
    // It is counted and dropped, and it is not a rejection — a peer that
    // treated it as one would report an incident every time a neighbour
    // upgraded.
    let net = network();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();
    net.clear_trace();

    let unknown = PayloadKind::Unknown(9_999);
    let envelope = signed(
        &net,
        alice,
        net.settings().protocol,
        unknown,
        SequenceNumber::FIRST,
        "a kind this build has never heard of",
    );

    let verdict = net
        .peer(bob)
        .accept_envelope(envelope)
        .expect("the boundary judges it");

    assert_eq!(verdict, InboundVerdict::Ignored(unknown));
    assert!(net.peer(bob).broadcast_history().is_empty());
    assert!(
        !net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageRejected(_))),
        "tolerating an unknown kind must not raise a rejection"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// One envelope genuinely signed by `author`'s own key.
///
/// The way to stage what no honest peer in this network would send — a version
/// nobody speaks, a payload kind nobody knows — without weakening the signature
/// that carries it.
fn signed(
    net: &SimNetwork,
    author: PeerId,
    version: ProtocolVersion,
    kind: PayloadKind,
    sequence: SequenceNumber,
    text: &str,
) -> Envelope {
    let signer = SimSigner::new(Arc::clone(net.peer(author).durable().keypair()));
    let payload = MessagePayload::new(
        sequence,
        Millis::from_millis(net.now()),
        MessageBody::new(text).expect("an admissible body"),
    );

    EnvelopeSignerPort::seal(
        &signer,
        UnsignedEnvelope::draft(author, version, kind, payload.encode()),
    )
    .expect("the signer holds this author's key")
}
