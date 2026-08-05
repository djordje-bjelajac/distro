use std::path::PathBuf;

use crate::cli::{ArgumentError, LaunchOptions, LaunchRequest};

fn run(arguments: &[&str]) -> LaunchOptions {
    match LaunchOptions::parse(arguments.iter().copied()) {
        Ok(LaunchRequest::Run(options)) => *options,
        other => panic!("expected a run request, got {other:?}"),
    }
}

#[test]
fn no_arguments_is_a_complete_launch() {
    // AC1: first launch takes no config and no args.
    assert_eq!(run(&[]), LaunchOptions::default());
}

#[test]
fn lan_discovery_is_on_unless_it_is_switched_off() {
    assert!(run(&[]).lan_discovery);
    assert!(!run(&["--no-lan"]).lan_discovery);
}

#[test]
fn the_profile_directory_is_read_as_a_path() {
    assert_eq!(
        run(&["--profile", "/tmp/peer-a"]).profile_directory,
        Some(PathBuf::from("/tmp/peer-a"))
    );
}

#[test]
fn a_join_ticket_can_be_supplied_at_startup() {
    assert_eq!(
        run(&["--ticket", "distro-join-1.abc"]).join_ticket,
        Some("distro-join-1.abc".to_owned())
    );
}

#[test]
fn the_broadcast_topic_can_be_named() {
    assert_eq!(
        run(&["--topic", "/distro/broadcast/test"]).broadcast_topic,
        Some("/distro/broadcast/test".to_owned())
    );
}

#[test]
fn options_combine() {
    let options = run(&["--profile", "/tmp/b", "--no-lan", "--print-identity"]);

    assert_eq!(options.profile_directory, Some(PathBuf::from("/tmp/b")));
    assert!(!options.lan_discovery);
    assert!(options.print_identity);
}

#[test]
fn help_and_version_are_requests_of_their_own() {
    assert_eq!(LaunchOptions::parse(["--help"]), Ok(LaunchRequest::Help));
    assert_eq!(LaunchOptions::parse(["-h"]), Ok(LaunchRequest::Help));
    assert_eq!(
        LaunchOptions::parse(["--version"]),
        Ok(LaunchRequest::Version)
    );
    assert_eq!(LaunchOptions::parse(["-V"]), Ok(LaunchRequest::Version));
}

#[test]
fn help_wins_over_anything_after_it() {
    // Asking for help must never start a network as a side effect.
    assert_eq!(
        LaunchOptions::parse(["--help", "--profile", "/tmp/a"]),
        Ok(LaunchRequest::Help)
    );
}

#[test]
fn an_unknown_argument_is_refused_rather_than_ignored() {
    assert_eq!(
        LaunchOptions::parse(["--bootstrap", "1.2.3.4"]),
        Err(ArgumentError::Unknown("--bootstrap".to_owned()))
    );
}

#[test]
fn there_is_no_role_flag_and_no_way_to_add_one_quietly() {
    // AC4 is "one binary, one code path, no role flags", and the command line is
    // where a role flag would first appear — a `--relay` or a `--bootstrap-node`
    // is a smaller edit than a second behaviour, and it reads like a
    // convenience. Every name below must stay *unknown*: the moment one parses,
    // this instance is no longer the same program as every other instance, and
    // "peers provide all infrastructure" has quietly become "some peers do".
    //
    // S1 rides along: `--bootstrap`/`--bootstrap-node`/`--relay-address` would
    // each be a hardcoded host by another route (D1 exists precisely because
    // there is no such list). `--listen` is not on this list and must not be —
    // it names addresses this peer *binds*, never hosts it contacts.
    for role_flag in [
        "--server",
        "--client",
        "--relay",
        "--no-relay",
        "--relay-server",
        "--relay-only",
        "--relay-address",
        "--bootstrap",
        "--bootstrap-node",
        "--bootstrap-peer",
        "--rendezvous",
        "--stun",
        "--role",
        "--mode",
        "--metrics",
        "--telemetry",
    ] {
        assert_eq!(
            LaunchOptions::parse([role_flag]),
            Err(ArgumentError::Unknown(role_flag.to_owned())),
            "`{role_flag}` parses, so this build has a role or an operator-run \
             host in it (AC4, S1). Adding one needs `$spdd-prompt-update`, not \
             a line removed from this list."
        );
    }
}

#[test]
fn the_whole_option_set_is_six_options_and_two_requests() {
    // The list above only catches names somebody thought of. This catches the
    // rest: if an option is added, this fails until whoever added it has looked
    // at the AC4 list and decided the new one is not a role. Kept as names
    // rather than a count so the failure says what changed.
    let every_option = [
        "--profile",
        "--ticket",
        "--topic",
        "--listen",
        "--no-lan",
        "--print-identity",
    ];

    for option in every_option {
        assert!(
            !matches!(
                LaunchOptions::parse([option]),
                Err(ArgumentError::Unknown(_))
            ),
            "`{option}` is documented in `--help` but no longer parses"
        );
    }

    // And nothing that is not on the list and not a request.
    assert_eq!(
        run(&[]),
        LaunchOptions {
            profile_directory: None,
            join_ticket: None,
            broadcast_topic: None,
            listen_addresses: Vec::new(),
            lan_discovery: true,
            print_identity: false,
        },
        "a launch with no arguments must still be fully described by these six \
         fields — a seventh means a new option, which needs the AC4 check above"
    );
}

#[test]
fn an_option_missing_its_value_is_refused() {
    assert_eq!(
        LaunchOptions::parse(["--profile"]),
        Err(ArgumentError::MissingValue("--profile"))
    );
    assert_eq!(
        LaunchOptions::parse(["--ticket"]),
        Err(ArgumentError::MissingValue("--ticket"))
    );
}

#[test]
fn listen_addresses_accumulate() {
    // A peer binds several transports, so the option repeats rather than
    // taking a delimited list nobody can escape correctly.
    let options = run(&[
        "--listen",
        "/ip4/0.0.0.0/udp/4001/quic-v1",
        "--listen",
        "/ip4/0.0.0.0/tcp/4001",
    ]);

    assert_eq!(
        options.listen_addresses,
        vec![
            "/ip4/0.0.0.0/udp/4001/quic-v1".to_owned(),
            "/ip4/0.0.0.0/tcp/4001".to_owned(),
        ]
    );
}

#[test]
fn no_listen_address_means_the_default() {
    // Every interface, on a port the OS picks — which is what lets two
    // instances share a machine without colliding.
    assert!(run(&[]).listen_addresses.is_empty());
}

#[test]
fn a_listen_option_missing_its_value_is_refused() {
    assert_eq!(
        LaunchOptions::parse(["--listen"]),
        Err(ArgumentError::MissingValue("--listen"))
    );
}
