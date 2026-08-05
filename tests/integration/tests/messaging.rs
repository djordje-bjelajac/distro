//! Sending, ordering, and delivery state across peers: the claims that need
//! more than one instance to be true or false (canvas AC4, AC7, AC8, AC10,
//! AC11, AC15, AC16, invariants 5 and 6).
//!
//! Arrival order, duplication, and loss are all scripted here rather than
//! hoped for. Nothing sleeps, nothing polls, and every instant is one the
//! scenario put on the clock (AC13, safeguard S5).

use std::sync::Arc;

use infra_sim_net::{FrameLabel, SimNetwork, SimSigner, TraceEvent};
use messaging::domain::events::{
    GapCloseCause, MessageDeliveryStateChanged, MessagingEvent, RejectionReason,
};
use messaging::domain::{
    ConversationId, DeliveryFailure, DeliveryState, MessageBody, Millis, SequenceNumber,
};
use messaging::ports::{EnvelopeSignerPort, MessagePayload, UnsignedEnvelope};
use shared_types::{PayloadKind, PeerId};

/// One seed for the whole file, written down rather than picked implicitly
/// (AC13).
const SEED: u64 = 90_002;

fn pair() -> SimNetwork {
    SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
        .build()
}

/// A network whose 1:1 messages stay `Pending`: recipients still receive them,
/// only the acknowledgement is suppressed.
///
/// The state every asynchronous-failure scenario has to start from — a message
/// the transport has taken and not yet answered for (D10, AC11).
fn holding_directs_pending<'a>(labels: impl IntoIterator<Item = &'a str>) -> SimNetwork {
    SimNetwork::seeded(SEED)
        .with_peers(labels)
        .acknowledging_directs(false)
        .build()
}

/// The gap-tolerance window every peer in `net` was assembled with (rule R,
/// S6).
fn gap_tolerance(net: &SimNetwork) -> u64 {
    net.settings().gap_tolerance.as_millis()
}

// ---------------------------------------------------------------------------
// AC4, AC11 — a 1:1 message, end to end
// ---------------------------------------------------------------------------

#[test]
fn a_direct_message_round_trips_and_moves_from_pending_to_delivered() {
    // AC4: one binary, one code path — the same two instances that discovered
    // each other unconfigured also carry a 1:1 conversation.
    // AC11: the message carries visible delivery state throughout.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let outcome = net
        .peer(alice)
        .send_direct(bob, "hello over here")
        .expect("the session is up");

    // Pending is a state the sender can show, and it is the state before the
    // recipient has answered for it.
    assert_eq!(outcome.delivery, DeliveryState::Pending);
    assert_eq!(
        net.peer(alice).delivery_state(outcome.sent.id),
        Some(DeliveryState::Pending)
    );
    assert!(
        net.peer(bob).direct_history(alice).is_empty(),
        "nothing arrives without a delivery"
    );

    net.settle();

    // The recipient has it, in the conversation the recipient names by the
    // sender.
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["hello over here"]
    );
    let received = &net.peer(bob).direct_history(alice)[0];
    assert_eq!(received.author(), alice);
    assert_eq!(received.sequence(), SequenceNumber::FIRST);

    // And the sender's copy has moved on, without anything being polled.
    assert_eq!(
        net.peer(alice).delivery_state(outcome.sent.id),
        Some(DeliveryState::Delivered)
    );
    assert!(
        net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageReceived(_))),
        "the arrival was never recorded"
    );
}

// ---------------------------------------------------------------------------
// AC10 — the broadcast channel
// ---------------------------------------------------------------------------

#[test]
fn a_broadcast_reaches_every_online_peer_however_gossip_scrambles_it() {
    // AC10: broadcast messages reach every online subscribed peer. The delivery
    // order is deliberately scrambled — per message and per recipient — so that
    // "everyone sees everything, in the author's order" is proved against a
    // network that agrees on nothing.
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob", "carol", "dave", "erin"])
        .build();
    let alice = net.peer_id("alice");
    let listeners: Vec<PeerId> = net
        .peer_ids()
        .into_iter()
        .filter(|peer| *peer != alice)
        .collect();

    net.boot_all();
    assert_eq!(listeners.len(), 4);

    // Three messages, four recipients, twelve distinct latencies drawn from the
    // seeded stream: reproducible, and in no particular relation to send order.
    let mut delays: Vec<u64> = (1..=12).map(|step| step * 10).collect();
    net.shuffle(&mut delays);
    net.script_delays(delays);

    let said = ["first", "second", "third"];
    for text in said {
        net.peer(alice)
            .publish_broadcast(text)
            .expect("gossip accepts it");
    }
    net.settle();

    for listener in &listeners {
        assert_eq!(
            net.peer(*listener).transcript(ConversationId::Broadcast),
            said,
            "{} did not see the channel as its author wrote it",
            net.label_of(*listener)
        );
    }

    // A guard against the scramble having quietly become a no-op: at least one
    // recipient must have been handed the messages out of the author's order,
    // or this scenario proves only that an ordered network stays ordered.
    let scrambled = listeners.iter().any(|listener| {
        let arrivals = message_arrivals(&net, *listener);
        arrivals.windows(2).any(|pair| pair[0] > pair[1])
    });
    assert!(scrambled, "no recipient received the messages out of order");
}

#[test]
fn a_late_joiner_sees_everything_the_author_sends_after_it_arrives() {
    // AC10's affirmative half, and the exact clause the pre-rework code failed:
    // no history is replayed to a late joiner, *but* every message the author
    // sends after it joins must be displayed, within one gap-tolerance window
    // of first contact (rule R).
    let mut net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
        .build();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let before = ["old one", "old two", "old three", "old four", "old five"];
    for text in before {
        net.peer(alice)
            .publish_broadcast(text)
            .expect("gossip accepts it");
    }
    net.settle();
    assert_eq!(net.peer(bob).transcript(ConversationId::Broadcast), before);

    // Carol's process starts now: it was not in the network for any of that.
    let carol = net.add_peer("carol");
    net.boot(carol);
    assert!(net.peer(carol).is_connected_to(alice) || net.peer(carol).is_connected_to(bob));

    let after = ["new one", "new two", "new three"];
    for text in after {
        net.peer(alice)
            .publish_broadcast(text)
            .expect("gossip accepts it");
    }
    net.settle();
    let first_contact = net.now();

    // Nothing is displayed yet, and that is correct: carol's first sighting of
    // alice is sequence 6, and a gap means "not yet received" until the window
    // says otherwise. Displaying it immediately would be guessing.
    assert!(
        net.peer(carol).broadcast_history().is_empty(),
        "an unresolved gap must not be displayed through"
    );

    net.advance(gap_tolerance(&net));
    net.tick();

    // The window elapsed, the run behind the abandoned range became visible,
    // and it is exactly what alice said after carol arrived — in her order.
    assert_eq!(net.peer(carol).transcript(ConversationId::Broadcast), after);
    let waited = net.now() - first_contact;
    assert!(
        waited <= gap_tolerance(&net),
        "the late joiner waited {waited} ms, past one gap-tolerance window"
    );

    // No history replay in v1: what was said before carol arrived stays unsaid
    // for her, and the range given up on is named rather than forgotten.
    for text in before {
        assert!(
            !net.peer(carol)
                .transcript(ConversationId::Broadcast)
                .contains(&text.to_owned()),
            "history was replayed to a late joiner: {text}"
        );
    }
    let closed = gap_closures(&net, carol);
    assert_eq!(closed.len(), 1, "expected exactly one abandoned range");
    assert_eq!(closed[0].conversation, ConversationId::Broadcast);
    assert_eq!(closed[0].author, alice);
    assert_eq!(closed[0].from.as_u64(), 1);
    assert_eq!(closed[0].to.as_u64(), before.len() as u64);
    assert_eq!(closed[0].cause, GapCloseCause::ToleranceElapsed);
}

// ---------------------------------------------------------------------------
// AC7, AC8 — at-least-once delivery, exactly-once application, one order
// ---------------------------------------------------------------------------

#[test]
fn a_redelivered_message_is_applied_exactly_once() {
    // AC7: redelivery of the same message changes nothing user-visible.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.duplicate_next(1);
    let outcome = net
        .peer(alice)
        .send_direct(bob, "said once")
        .expect("the session is up");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["said once"],
        "the redelivery was applied a second time"
    );
    assert!(
        net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageDuplicateIgnored(_))),
        "the duplicate was not counted in local diagnostics"
    );

    // Nothing user-visible changed on the sender's side either: the second
    // arrival's acknowledgement did not disturb a state that had already moved.
    assert_eq!(
        net.peer(alice).delivery_state(outcome.sent.id),
        Some(DeliveryState::Delivered)
    );
    assert_eq!(net.peer(alice).direct_history(bob).len(), 1);
}

#[test]
fn messages_display_in_the_authors_send_order_however_they_arrive() {
    // AC8: one author's messages display in that author's send order regardless
    // of arrival order, within the gap-tolerance window.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    // Written down rather than randomised: these three latencies make the
    // messages arrive second, third, first.
    net.script_delays([90, 30, 60]);
    for text in ["first", "second", "third"] {
        net.peer(alice)
            .send_direct(bob, text)
            .expect("the session is up");
    }
    net.settle();

    assert_eq!(
        message_arrivals(&net, bob),
        vec![2, 3, 1],
        "the scenario's scripted arrival order stopped holding"
    );
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["first", "second", "third"]
    );

    // Ordinary reordering inside the window is not an incident: it produces no
    // abandoned range and no rejection.
    assert!(gap_closures(&net, bob).is_empty());
    assert!(
        !net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageRejected(_))),
        "a reorder that resolved inside the window raised a false diagnostic"
    );
}

// ---------------------------------------------------------------------------
// AC15 — a gap that never closes, and what arrives after it is abandoned
// ---------------------------------------------------------------------------

#[test]
fn a_permanent_gap_is_abandoned_by_name_and_the_run_behind_it_becomes_visible() {
    // AC15 and rule R: the abandoned range is named in a `MessageGapClosed`,
    // the messages stuck behind it become visible, and a message that arrives
    // after its gap has closed is reported rather than silently discarded.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    // The first message is delayed far past the window; the next two are not.
    const LOST_FOR_MILLIS: u64 = 10_000;
    net.script_delays([LOST_FOR_MILLIS, 0, 0]);

    let first = net
        .peer(alice)
        .send_direct(bob, "one")
        .expect("the session is up");
    for text in ["two", "three"] {
        net.peer(alice)
            .send_direct(bob, text)
            .expect("the session is up");
    }

    // `pump` rather than `settle`: the clock must stay where the scenario put
    // it, so the delayed message is still in flight.
    net.pump();
    assert!(
        net.peer(bob).direct_history(alice).is_empty(),
        "messages behind an open gap must not be displayed"
    );
    assert_eq!(
        net.pending_frames(),
        1,
        "the first message is still in flight"
    );

    net.advance(gap_tolerance(&net));
    net.tick();

    // The range is named, its cause is stated, and the run behind it is now
    // readable in the author's order.
    let closed = gap_closures(&net, bob);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].conversation, ConversationId::Direct(alice));
    assert_eq!(closed[0].author, alice);
    assert_eq!(closed[0].from, SequenceNumber::FIRST);
    assert_eq!(closed[0].to, SequenceNumber::FIRST);
    assert_eq!(closed[0].cause, GapCloseCause::ToleranceElapsed);
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["two", "three"]
    );

    // The abandoned message finally arrives. It is reported, not applied, and
    // not miscounted as a duplicate: it is loss, and calling it a duplicate
    // would hide it.
    net.advance(LOST_FOR_MILLIS - gap_tolerance(&net));
    net.pump();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["two", "three"],
        "a message that arrived after its gap closed was displayed out of order"
    );
    assert!(
        net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(
                event,
                MessagingEvent::MessageRejected(rejected)
                    if rejected.reason == RejectionReason::ArrivedAfterGapClosed
            )),
        "the late arrival was discarded without a word"
    );
    assert!(
        !net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageDuplicateIgnored(_))),
        "lost content was reported as content already seen"
    );

    // The sender was never told, and its own view says so plainly: the message
    // is still awaiting an acknowledgement that will not come. Pending is a
    // visible state; silence is not (AC11).
    assert_eq!(
        net.peer(alice).delivery_state(first.sent.id),
        Some(DeliveryState::Pending)
    );
}

// ---------------------------------------------------------------------------
// AC11, D10 — failure is a state, never silence
// ---------------------------------------------------------------------------

#[test]
fn a_direct_send_whose_transport_fails_is_visibly_failed() {
    // AC11: silent loss is not a state. With the only path severed and no third
    // peer to relay, the send fails at the transport and the message says so.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.sever_link(alice, bob);

    let outcome = net
        .peer(alice)
        .send_direct(bob, "into the dark")
        .expect("the message is composed whether or not it can travel");

    assert_eq!(
        outcome.delivery,
        DeliveryState::Failed(DeliveryFailure::NoRelayAvailable)
    );
    assert_eq!(
        outcome.failure_reason(),
        Some(DeliveryFailure::NoRelayAvailable)
    );
    assert_eq!(
        net.peer(alice).delivery_state(outcome.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::NoRelayAvailable))
    );

    // The message exists in the sender's own conversation carrying its failure:
    // it can be seen, and resent, rather than having quietly not happened.
    let own = net.peer(alice).direct_history(bob);
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].body().to_string(), "into the dark");
    assert_eq!(
        own[0].delivery_state(),
        DeliveryState::Failed(DeliveryFailure::NoRelayAvailable)
    );

    assert_eq!(net.pending_frames(), 0, "nothing reached the wire");
    net.settle();
    assert!(net.peer(bob).direct_history(alice).is_empty());
    assert!(
        net.trace()
            .messaging_events_of(alice)
            .iter()
            .any(|event| matches!(
                event,
                MessagingEvent::MessageDeliveryStateChanged(changed)
                    if changed.to == DeliveryState::Failed(DeliveryFailure::NoRelayAvailable)
            )),
        "the failure was not recorded"
    );
}

#[test]
fn a_peer_disconnecting_fails_that_peers_pending_directs() {
    // D10: one bounded attempt while the session lives, and a stated failure
    // when it does not. Acknowledgement is switched off so the messages are
    // genuinely still in flight when the session ends.
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob", "carol"])
        .acknowledging_directs(false)
        .build();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );
    net.boot_all();
    if !net.peer(alice).is_connected_to(carol) {
        net.peer(alice)
            .connect_to(carol)
            .expect("carol answers a dial");
        net.settle();
    }
    assert!(net.peer(alice).is_connected_to(bob));
    assert!(net.peer(alice).is_connected_to(carol));

    let to_bob: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|text| {
            net.peer(alice)
                .send_direct(bob, text)
                .expect("the session is up")
        })
        .collect();
    let to_carol = net
        .peer(alice)
        .send_direct(carol, "unrelated")
        .expect("the session is up");
    net.settle();

    for outcome in &to_bob {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Pending)
        );
    }

    // Bob leaves: the session closes through the real port, `membership`
    // publishes `PeerDisconnected`, and the root fans it into `messaging`.
    net.peer(bob).leave().expect("the publisher is healthy");
    net.settle();

    for outcome in &to_bob {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Failed(DeliveryFailure::SessionClosed)),
            "a pending direct outlived the session it depended on"
        );
    }

    // Only that peer's conversation is affected: a disconnect is not a reason
    // to give up on anyone else.
    assert_eq!(
        net.peer(alice).delivery_state(to_carol.sent.id),
        Some(DeliveryState::Pending)
    );
    assert!(
        net.trace()
            .messaging_events_of(alice)
            .iter()
            .any(|event| matches!(
                event,
                MessagingEvent::MessageDeliveryStateChanged(changed)
                    if changed.to == DeliveryState::Failed(DeliveryFailure::SessionClosed)
            )),
        "the failures were not recorded"
    );
}

// ---------------------------------------------------------------------------
// AC11, D10 — the ending `send_direct` cannot report
// ---------------------------------------------------------------------------

/// How long after a send the transport's refusal comes back.
///
/// Any positive number would do: what matters is that the report lands *after*
/// the send returned, and that a scenario put that instant on the clock rather
/// than a real one (AC13, S5).
const REFUSAL_ARRIVES_AFTER_MILLIS: u64 = 750;

#[test]
fn a_direct_refused_after_the_send_returned_is_visibly_failed_while_its_session_stays_up() {
    // AC11's asynchronous half. `send_direct` answers `Ok` as soon as the
    // transport has queued the request; a refusal or a timeout comes back
    // later, as news about one message, with the session still healthy. Nothing
    // else can move that message then — an acknowledgement runs the other way,
    // and a disconnect is both too much and unavailable while the link is up —
    // so before this path existed it sat `Pending` for the life of the session,
    // which is precisely the silent loss AC11 declares a non-state.
    //
    // Acknowledgement is switched off, so all three messages are genuinely
    // still awaiting an answer when the refusal arrives.
    let net = holding_directs_pending(["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let sent: Vec<_> = ["one", "two", "three"]
        .into_iter()
        .map(|text| {
            net.peer(alice)
                .send_direct(bob, text)
                .expect("the session is up")
        })
        .collect();
    net.settle();
    for outcome in &sent {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Pending),
            "the scenario needs all three still awaiting an answer"
        );
    }

    // The transport gives up on the second one, long after that send returned.
    net.advance(REFUSAL_ARRIVES_AFTER_MILLIS);
    let refused_at = net.now();
    net.peer(alice)
        .message_delivery_failed(sent[1].sent.id, DeliveryFailure::RetriesExhausted)
        .expect("a pending direct may fail");

    // It reached a terminal state, and the conversation the user reads shows
    // it — carrying the reason the transport gave rather than a default.
    assert_eq!(
        net.peer(alice).delivery_state(sent[1].sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::RetriesExhausted))
    );
    let own = net.peer(alice).direct_history(bob);
    assert_eq!(own.len(), 3);
    assert_eq!(own[1].body().to_string(), "two");
    assert_eq!(
        own[1].delivery_state(),
        DeliveryState::Failed(DeliveryFailure::RetriesExhausted),
        "the failure is not readable where the message is"
    );

    // Its two siblings are untouched: one refusal is news about one message,
    // not about the peer.
    for outcome in [&sent[0], &sent[2]] {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Pending),
            "a single refusal reached a message it did not name"
        );
    }

    // The session it travelled on is still up on both sides, and still carries
    // traffic — which is the condition that made this ending unreachable.
    assert!(net.peer(alice).is_connected_to(bob));
    assert!(net.peer(bob).is_connected_to(alice));
    assert_eq!(net.peer(alice).online_peers(), vec![bob]);

    net.peer(bob)
        .send_direct(alice, "still here")
        .expect("the session is up");
    net.settle();
    assert!(
        net.peer(alice)
            .direct_history(bob)
            .iter()
            .any(|message| message.author() == bob),
        "the session stopped carrying traffic"
    );

    // Announced once, at the instant the scenario put the refusal on the clock:
    // after the send, not with it.
    let announced = delivery_changes(&net, alice);
    assert_eq!(
        announced.len(),
        1,
        "one refusal must produce one announcement"
    );
    assert_eq!(announced[0].0, refused_at);
    assert_eq!(announced[0].1.id, sent[1].sent.id);
    assert_eq!(announced[0].1.from, DeliveryState::Pending);
    assert_eq!(
        announced[0].1.to,
        DeliveryState::Failed(DeliveryFailure::RetriesExhausted)
    );
}

#[test]
fn a_later_disconnect_fails_what_an_asynchronous_refusal_left_pending() {
    // D10 and the asynchronous path compose rather than compete: a refusal ends
    // one message while the link lives, and the disconnect that follows ends
    // everything still waiting, without re-deciding what the refusal already
    // settled. Between them no direct message is left in a state the user
    // cannot read (AC11).
    let net = holding_directs_pending(["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let sent: Vec<_> = ["one", "two", "three"]
        .into_iter()
        .map(|text| {
            net.peer(alice)
                .send_direct(bob, text)
                .expect("the session is up")
        })
        .collect();
    net.settle();

    net.advance(REFUSAL_ARRIVES_AFTER_MILLIS);
    net.peer(alice)
        .message_delivery_failed(sent[0].sent.id, DeliveryFailure::PeerUnreachable)
        .expect("a pending direct may fail");
    assert_eq!(
        net.peer(alice).delivery_state(sent[0].sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::PeerUnreachable))
    );
    for outcome in [&sent[1], &sent[2]] {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Pending),
            "the disconnect below needs something left to fail"
        );
    }

    // Bob leaves: the session closes through the real port, `membership`
    // publishes `PeerDisconnected`, and the root fans it into `messaging`.
    net.peer(bob).leave().expect("the publisher is healthy");
    net.settle();
    assert!(!net.peer(alice).is_connected_to(bob));

    for outcome in [&sent[1], &sent[2]] {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Failed(DeliveryFailure::SessionClosed)),
            "a pending direct outlived the session it depended on"
        );
    }

    // What the refusal settled stays settled: the disconnect neither overwrote
    // the reason the user was already shown nor announced it a second time.
    assert_eq!(
        net.peer(alice).delivery_state(sent[0].sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::PeerUnreachable)),
        "a disconnect overwrote an ending the user had already been given"
    );

    let announced = delivery_changes(&net, alice);
    assert_eq!(
        announced.len(),
        3,
        "expected one refusal and the two endings the disconnect cost"
    );
    assert_eq!(announced[0].1.id, sent[0].sent.id);
    assert_eq!(
        announced[0].1.to,
        DeliveryState::Failed(DeliveryFailure::PeerUnreachable)
    );
    for (announcement, outcome) in announced[1..].iter().zip([&sent[1], &sent[2]]) {
        assert_eq!(announcement.1.id, outcome.sent.id);
        assert_eq!(announcement.1.from, DeliveryState::Pending);
        assert_eq!(
            announcement.1.to,
            DeliveryState::Failed(DeliveryFailure::SessionClosed)
        );
    }

    // Nothing is left in the state AC11 calls silent loss.
    assert!(
        net.peer(alice)
            .direct_history(bob)
            .iter()
            .all(|message| message.delivery_state().is_terminal()),
        "a direct message ended in neither delivery nor a stated failure"
    );
}

#[test]
fn the_reason_the_transport_reported_is_the_reason_the_read_model_shows() {
    // AC11 asks for a cause the user can act on, so the reason travels from the
    // layer that observed it to the read model unchanged — never defaulted,
    // never collapsed onto its neighbours. Every reason the domain admits is
    // exercised, so one added later is covered by this scenario rather than
    // forgotten by it.
    let net = holding_directs_pending(["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let sent: Vec<_> = DeliveryFailure::ALL
        .iter()
        .map(|reason| {
            net.peer(alice)
                .send_direct(bob, &format!("ends as {reason}"))
                .expect("the session is up")
        })
        .collect();
    net.settle();

    net.advance(REFUSAL_ARRIVES_AFTER_MILLIS);
    for (outcome, reason) in sent.iter().zip(DeliveryFailure::ALL) {
        net.peer(alice)
            .message_delivery_failed(outcome.sent.id, reason)
            .expect("a pending direct may fail");
    }

    for (outcome, reason) in sent.iter().zip(DeliveryFailure::ALL) {
        assert_eq!(
            net.peer(alice).delivery_state(outcome.sent.id),
            Some(DeliveryState::Failed(reason)),
            "the read model shows a reason the transport never gave"
        );
    }

    // And the same reason is what the rest of the system was told, message by
    // message, in the order the reports came in.
    let announced = delivery_changes(&net, alice);
    assert_eq!(announced.len(), DeliveryFailure::ALL.len());
    for ((announcement, outcome), reason) in announced.iter().zip(&sent).zip(DeliveryFailure::ALL) {
        assert_eq!(announcement.1.id, outcome.sent.id);
        assert_eq!(announcement.1.to.failure_reason(), Some(reason));
    }
}

// ---------------------------------------------------------------------------
// AC16, D12 — a restarted peer is still heard
// ---------------------------------------------------------------------------

#[test]
fn a_restarted_peer_is_still_heard_by_a_peer_that_stayed_online() {
    // AC16: a peer that restarts continues to be heard by peers already online,
    // because its outbound sequence does not reset (D12). Before that decision,
    // the message after the restart carried sequence 1, every listener
    // classified it a duplicate, and the peer went permanently mute while
    // appearing — to itself — to work.
    let mut net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    for text in ["one", "two"] {
        net.peer(alice)
            .publish_broadcast(text)
            .expect("gossip accepts it");
    }
    net.settle();
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["one", "two"]
    );

    net.restart(alice);

    // History died with the process (D7); the identity and its counter did not
    // (D12, AC9).
    assert!(net.peer(alice).broadcast_history().is_empty());
    assert_eq!(
        net.peer(alice).local_identity().expect("assumed").peer,
        alice
    );
    assert_eq!(
        net.peer(alice)
            .durable()
            .counter()
            .mark(ConversationId::Broadcast)
            .map(SequenceNumber::as_u64),
        Some(2)
    );

    net.peer(alice).join().expect("the publisher is healthy");
    net.settle();

    let resumed = net
        .peer(alice)
        .publish_broadcast("three")
        .expect("gossip accepts it");
    net.settle();

    assert_eq!(resumed.sent.id.sequence().as_u64(), 3);
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["one", "two", "three"],
        "the restarted peer was not heard"
    );

    // The counterfactual, staged rather than asserted in prose: this is exactly
    // the envelope a peer whose counter had reset would have put on the wire —
    // same author, same key, same channel, sequence 1 — and it reaches no read
    // model. A peer that resumed at 1 would be mute, and this is why.
    let as_if_reset = signed_broadcast(&net, alice, SequenceNumber::FIRST, "three");
    let verdict = net
        .peer(bob)
        .accept_envelope(as_if_reset)
        .expect("the boundary judges it");

    assert!(
        verdict.is_duplicate(),
        "a counter reset would not have been silently ignored: {verdict:?}"
    );
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["one", "two", "three"]
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Every abandoned range one peer reported, in the order it reported them.
fn gap_closures(
    net: &SimNetwork,
    peer: PeerId,
) -> Vec<messaging::domain::events::MessageGapClosed> {
    net.trace()
        .messaging_events_of(peer)
        .into_iter()
        .filter_map(|event| match event {
            MessagingEvent::MessageGapClosed(closed) => Some(closed),
            _ => None,
        })
        .collect()
}

/// Every delivery-state change one peer announced, paired with the instant it
/// announced it at.
///
/// The instant is what makes "after the send returned" a claim rather than a
/// figure of speech: a refusal is stamped at the moment the scenario put on the
/// clock, and nothing in this crate can have put it anywhere else (S5).
fn delivery_changes(net: &SimNetwork, peer: PeerId) -> Vec<(u64, MessageDeliveryStateChanged)> {
    net.trace()
        .entries()
        .into_iter()
        .filter_map(|entry| match entry.event {
            TraceEvent::Messaging {
                peer: publisher,
                event: MessagingEvent::MessageDeliveryStateChanged(changed),
            } if publisher == peer => Some((entry.at, changed)),
            _ => None,
        })
        .collect()
}

/// The sequence numbers of the message frames handed to `peer`, in the order
/// the network handed them over.
///
/// Arrival order, as distinct from display order. A scenario that scripts one
/// asserts on the other, and this is how it checks its own script still says
/// what it meant.
fn message_arrivals(net: &SimNetwork, peer: PeerId) -> Vec<u64> {
    net.trace()
        .entries()
        .into_iter()
        .filter_map(|entry| match entry.event {
            TraceEvent::FrameDelivered {
                to,
                frame: FrameLabel::Direct(sequence) | FrameLabel::Broadcast(sequence),
                ..
            } if to == peer => Some(sequence),
            _ => None,
        })
        .collect()
}

/// One broadcast envelope, genuinely signed by `author`'s key, carrying
/// `sequence`.
///
/// The way to put on the wire something no honest peer would send — here, the
/// message a peer whose sequence counter had reset would produce.
fn signed_broadcast(
    net: &SimNetwork,
    author: PeerId,
    sequence: SequenceNumber,
    text: &str,
) -> shared_types::Envelope {
    let signer = SimSigner::new(Arc::clone(net.peer(author).durable().keypair()));
    let payload = MessagePayload::new(
        sequence,
        Millis::from_millis(net.now()),
        MessageBody::new(text).expect("an admissible body"),
    );

    EnvelopeSignerPort::seal(
        &signer,
        UnsignedEnvelope::draft(
            author,
            net.settings().protocol,
            PayloadKind::BroadcastMessage,
            payload.encode(),
        ),
    )
    .expect("the signer holds this author's key")
}
