//! S1 as a check rather than as a choice.
//!
//! # The difference this file makes
//!
//! Safeguard S1 — no operator-run host in any code path — currently holds
//! because of the feature list in `Cargo.toml`: `dns` is off, `metrics` is off,
//! nothing enables a rendezvous or a STUN client. That is a *choice*, and a
//! choice is one careless `--features` away from being unmade. `Cargo.lock`
//! still resolves `libp2p-dns`, `hickory-resolver`, `libp2p-metrics`,
//! `prometheus-client` and `hyper`, because a lockfile records every optional
//! dependency the graph *could* pull in.
//!
//! So the lockfile cannot answer the question. What can is the resolved
//! dependency graph of a package as it is actually compiled, which is what
//! `cargo tree -e normal -p <package>` prints. This test reads that graph for
//! the two packages that matter — this crate, where the libp2p features are
//! chosen, and `app`, the binary a user runs — and fails if any forbidden crate
//! appears in either.
//!
//! The difference is between "we chose not to enable DNS" and "DNS cannot be
//! enabled without a red build". Only the second one survives a maintainer who
//! has not read the canvas.
//!
//! # Why `hickory-proto` is not on the list
//!
//! It *is* in the graph, via `libp2p-mdns`, and it belongs there: mDNS speaks
//! DNS message format to a link-local multicast group. That is not a resolver
//! and it contacts no nameserver. `hickory-resolver` — the crate that reads
//! `/etc/resolv.conf` and queries whatever host is named in it — is the one
//! that would breach S1, and it is the one named below.
//!
//! # When cargo is not there
//!
//! Spawning `cargo` can fail — a stripped container, a sandbox with no process
//! spawning. That is a fact about the machine, so the check reports itself
//! skipped rather than failing. A `cargo` that runs and then *errors* is a
//! different thing: the guard could not do its job and says so loudly, because
//! a guard that reports success when it did not run is worse than no guard.

use std::io::Write;
use std::process::Command;

/// Crates that must never enter the compiled dependency graph, each with the
/// reason it is forbidden — a failure message that only named the crate would
/// send the next maintainer back to this file to find out why.
const FORBIDDEN: [(&str, &str); 5] = [
    (
        "libp2p-dns",
        "a DNS transport resolves names against whatever nameserver the host is \
         configured with, and makes a `/dnsaddr` bootstrap list readable — both \
         are infrastructure somebody else operates (S1, D1)",
    ),
    (
        "hickory-resolver",
        "a stub resolver queries a nameserver this project does not run; mDNS \
         needs `hickory-proto` for the message format and nothing more (S1)",
    ),
    (
        "libp2p-metrics",
        "telemetry. Every counter in this workspace is read in-process and \
         leaves the machine only if a human looks at it (S1, S8)",
    ),
    (
        "prometheus-client",
        "the encoding half of the same telemetry, and the thing a scrape \
         endpoint would be built on (S1, S8)",
    ),
    (
        "hyper",
        "an HTTP client or server has no honest use here: there is no API to \
         call, no endpoint to report to, and no web frontend (S1, D8)",
    ),
];

/// The packages whose graphs are guarded.
///
/// This crate because it is where the libp2p feature set is chosen, and `app`
/// because it is the binary a user actually runs — a forbidden crate could
/// arrive through any of its other dependencies, and only the binary's own
/// graph would show it.
const GUARDED: [&str; 2] = ["infra-net-libp2p", "app"];

#[test]
fn no_operator_run_infrastructure_can_reach_the_compiled_graph() {
    for package in GUARDED {
        let Some(graph) = dependency_graph(package) else {
            return;
        };

        let found: Vec<&(&str, &str)> = FORBIDDEN
            .iter()
            .filter(|(crate_name, _)| graph.iter().any(|present| present == crate_name))
            .collect();

        assert!(
            found.is_empty(),
            "S1 breach: {package} now compiles against {count} forbidden \
             {noun}.\n{details}\n\nS1 is non-negotiable (canvas §7): no \
             operator-run host may enter any code path. If this is deliberate \
             it needs `$spdd-prompt-update`, not an edit to this test.",
            count = found.len(),
            noun = if found.len() == 1 { "crate" } else { "crates" },
            details = found
                .iter()
                .map(|(crate_name, reason)| format!("  * `{crate_name}` — {reason}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

#[test]
fn the_guard_is_reading_a_graph_and_not_an_empty_list() {
    // A check that silently read nothing would pass forever. This is the
    // control: crates that must be present, in the package whose job they are.
    let Some(graph) = dependency_graph("infra-net-libp2p") else {
        return;
    };

    for expected in ["libp2p", "libp2p-relay", "libp2p-mdns", "tokio"] {
        assert!(
            graph.iter().any(|present| present == expected),
            "`{expected}` is missing from the graph this guard read, so the \
             guard is not reading the graph. Present: {graph:?}"
        );
    }
}

/// Every crate in `package`'s normal (non-dev, non-build) dependency graph.
///
/// `None` means cargo could not be spawned at all, and the caller should skip.
fn dependency_graph(package: &str) -> Option<Vec<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    let output = match Command::new(&cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--package",
            package,
            // Dev-dependencies and build-dependencies do not ship, and a test
            // fixture that pulled in an HTTP client would be a different
            // (smaller) problem than a binary that did.
            "--edges",
            "normal",
            // One package per line, no tree drawing to parse around.
            "--prefix",
            "none",
            "--format",
            "{p}",
            // The check itself must not reach the network. By the time a test
            // runs, everything it names is already in the local registry.
            "--offline",
        ])
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "distro: skipping the S1 dependency guard — `{cargo} tree` could \
                 not be run ({error})."
            );
            return None;
        }
    };

    assert!(
        output.status.success(),
        "the S1 dependency guard could not read {package}'s graph: `cargo tree` \
         exited with {status}.\n{stderr}",
        status = output.status,
        stderr = String::from_utf8_lossy(&output.stderr),
    );

    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            // A line is `name version (source)`; the version and source are
            // noise for this question, and `(*)` marks a subtree cargo already
            // printed.
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}
