use std::num::NonZeroUsize;

use infra_net_libp2p::swarm::Reachability;
use membership::domain::{Endpoint, NetworkStatus};

use crate::test_peers::alice;
use crate::tui::{PeerLabels, StatusLine};

fn line(status: NetworkStatus) -> StatusLine {
    reachability_line(status, &Reachability::Unknown)
}

fn reachability_line(status: NetworkStatus, reachability: &Reachability) -> StatusLine {
    StatusLine::build(status, reachability, alice(), "peer-d75a9801", "broadcast")
}

fn connected() -> NetworkStatus {
    NetworkStatus::Connected(NonZeroUsize::new(3).expect("three"))
}

#[test]
fn the_three_network_states_all_read_differently() {
    // The canvas asks for exactly `Isolated | Joining | Connected(n)`.
    let isolated = line(NetworkStatus::Isolated).network;
    let joining = line(NetworkStatus::Joining).network;
    let connected = line(NetworkStatus::Connected(
        NonZeroUsize::new(3).expect("three"),
    ))
    .network;

    assert_eq!(isolated, "isolated");
    assert_eq!(joining, "joining");
    assert!(connected.contains('3'), "{connected}");
    assert_ne!(isolated, joining);
    assert_ne!(joining, connected);
}

#[test]
fn one_connected_peer_reads_in_the_singular() {
    let connected = line(NetworkStatus::Connected(NonZeroUsize::new(1).expect("one"))).network;

    assert_eq!(connected, "connected (1 peer)");
}

#[test]
fn isolation_is_stated_plainly_and_never_as_a_failure() {
    // `Isolated` is a normal state (canvas §2.2, S7): a fresh install on a
    // quiet network with no ticket is supposed to reach it.
    let isolated = line(NetworkStatus::Isolated);

    assert!(isolated.is_isolated());
    let text = isolated.text().to_lowercase();
    assert!(!text.contains("error"), "{text}");
    assert!(!text.contains("fail"), "{text}");
}

#[test]
fn the_line_names_this_peer_by_display_name_and_fingerprint() {
    let line = line(NetworkStatus::Isolated);

    assert!(line.identity.contains("peer-d75a9801"));
    assert!(line.identity.contains(&PeerLabels::short(alice())));
}

#[test]
fn the_line_names_the_conversation_on_screen() {
    let line = StatusLine::build(
        NetworkStatus::Isolated,
        &Reachability::Unknown,
        alice(),
        "peer-x",
        "direct 21fe 31df",
    );

    assert!(line.text().contains("direct 21fe 31df"));
}

#[test]
fn a_connected_status_is_not_isolated() {
    let connected = line(NetworkStatus::Connected(NonZeroUsize::new(2).expect("two")));

    assert!(!connected.is_isolated());
}

// ------------------------------------------------------ reachability (OP-2)

#[test]
fn an_unknown_reachability_renders_nothing_at_all() {
    // The single most important rule in OP-2 (canvas D6, S3). During normal
    // startup *every* peer is `Unknown`, so anything rendered here is rendered
    // by every instance on every launch — and anything that reads like a
    // verdict is the false negative the whole piece exists to prevent. So the
    // absence is asserted as an absence, not as "different from unreachable".
    let line = reachability_line(connected(), &Reachability::Unknown);

    assert_eq!(line.reachability, "");

    let text = line.text().to_lowercase();
    for forbidden in [
        "reach", "relay", "unknown", "checking", "probing", "pending", "…", "?",
    ] {
        assert!(
            !text.contains(forbidden),
            "an unknown verdict said {forbidden:?}: {text}"
        );
    }
    // And nothing is left behind where the verdict would have gone: the line
    // reads exactly as it did before this piece existed.
    assert_eq!(
        text,
        format!(
            "connected (3 peers) │ peer-d75a9801 · {} │ broadcast",
            PeerLabels::short(alice())
        )
        .to_lowercase()
    );
}

#[test]
fn a_reachable_verdict_names_the_address() {
    // "You are reachable" without saying where is not something a user can
    // check, paste, or compare against a ticket.
    let endpoint = Endpoint::direct("/ip4/203.0.113.7/tcp/4001").expect("a valid address");

    let line = reachability_line(connected(), &Reachability::Reachable(endpoint));

    assert_eq!(line.reachability, "reachable at /ip4/203.0.113.7/tcp/4001");
    assert!(
        line.text().contains("/ip4/203.0.113.7/tcp/4001"),
        "{}",
        line.text()
    );
}

#[test]
fn a_reachable_verdict_says_when_the_path_runs_through_a_relay() {
    // Reachable through someone else's bandwidth is a different fact from
    // reachable directly, and the address alone does not read as either.
    let endpoint = Endpoint::relayed("/ip4/203.0.113.7/tcp/4001/p2p-circuit").expect("valid");

    let line = reachability_line(connected(), &Reachability::Reachable(endpoint));

    assert_eq!(
        line.reachability,
        "reachable through a relay at /ip4/203.0.113.7/tcp/4001/p2p-circuit"
    );
}

#[test]
fn an_unreachable_verdict_states_the_consequence_and_blames_nobody() {
    // A corroborated failure says strangers' dials are not arriving. It does
    // not say why — a NAT, a firewall this user does not administer, or a
    // network that carries no inbound connections are indistinguishable from
    // here — so the wording names what follows and issues no instruction.
    let line = reachability_line(connected(), &Reachability::Unreachable);

    assert_eq!(
        line.reachability,
        "not reachable from outside — a relay will be needed"
    );

    let text = line.reachability.to_lowercase();
    for blame in [
        "you",
        "your",
        "misconfigur",
        "wrong",
        "invalid",
        "error",
        "fail",
    ] {
        assert!(!text.contains(blame), "the verdict blamed the user: {text}");
    }
    for instruction in [
        "router",
        "firewall",
        "port forward",
        "forward a port",
        "configure",
        "settings",
        "check ",
        "try ",
        "enable",
    ] {
        assert!(
            !text.contains(instruction),
            "the verdict gave an instruction the user may be unable to act on: {text}"
        );
    }
}

#[test]
fn the_three_reachability_states_all_read_differently() {
    // S3 in the renderer: `Unknown` and `Unreachable` are different facts, and
    // collapsing them anywhere reintroduces the false negative.
    let endpoint = Endpoint::direct("/ip4/203.0.113.7/tcp/4001").expect("a valid address");
    let unknown = reachability_line(connected(), &Reachability::Unknown).reachability;
    let reachable = reachability_line(connected(), &Reachability::Reachable(endpoint)).reachability;
    let unreachable = reachability_line(connected(), &Reachability::Unreachable).reachability;

    assert_ne!(unknown, unreachable);
    assert_ne!(unknown, reachable);
    assert_ne!(reachable, unreachable);
}

#[test]
fn the_verdict_is_shown_beside_the_peer_count_and_not_instead_of_it() {
    // D6: the status line is where a user already looks to answer "is this
    // thing working", and the peer count is half that answer.
    let line = reachability_line(connected(), &Reachability::Unreachable);

    assert_eq!(line.network, "connected (3 peers)");
    let text = line.text();
    let count = text.find("connected (3 peers)").expect("the peer count");
    let verdict = text.find("not reachable").expect("the verdict");
    assert!(count < verdict, "{text}");
}
