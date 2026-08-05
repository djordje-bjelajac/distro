//! The `distro` binary: read the arguments, assemble a node, start the engine,
//! join, and hand the terminal to the interface.
//!
//! # Everything hard is somewhere else
//!
//! This file is deliberately a sequence of five steps with no branches worth
//! testing. The startup *order* and its reasoning are on
//! [`Node::start`](app::composition::Node::start); the threading is on
//! [`Engine`](app::runtime::Engine); the interface loop is
//! [`tui::run`](app::tui::run). What is left here is exit codes and the
//! terminal's raw mode.
//!
//! # The two ways out
//!
//! * `--print-identity` reports this profile's identity and exits, having
//!   started no network and opened no terminal. It is the headless path: what a
//!   smoke check runs, and what tells an operator which `PeerId` a profile
//!   directory holds before they launch two instances against each other.
//! * Otherwise the interface runs until the user quits, and the terminal is
//!   restored on the way out — including when a panic unwinds through it,
//!   because a panic that leaves a terminal in raw mode leaves a user with a
//!   shell they cannot type into.

use std::process::ExitCode;
use std::sync::Arc;

use app::cli::{LaunchOptions, LaunchRequest, ProfileDirectory, ProfileEnvironment, Usage};
use app::composition::{Node, NodeSettings};
use app::runtime::{Engine, EngineCommand};
use app::tui;
use identity::ports::IdentityQueryPort;
use infra_net_libp2p::{JoinTicketCodec, NetworkConfig};
use infra_store_fs::{FileIdentityKeyStore, LocalStores};

#[cfg(test)]
mod main_test;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("distro: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = match LaunchOptions::parse(std::env::args_os().skip(1))
        .map_err(|error| error.to_string())?
    {
        LaunchRequest::Help => {
            println!("{}", Usage::text());
            return Ok(());
        }
        LaunchRequest::Version => {
            println!("{}", Usage::version());
            return Ok(());
        }
        LaunchRequest::Run(options) => *options,
    };

    let profile_directory = ProfileDirectory::resolve(
        options.profile_directory.as_deref(),
        &ProfileEnvironment::from_process(),
    )
    .map_err(|error| error.to_string())?;

    if options.print_identity {
        return print_identity(&profile_directory);
    }

    // A pasted or supplied ticket is decoded before anything is started, so a
    // typo is a message rather than a node that comes up and then fails to
    // join for a reason nobody sees.
    let ticket = match options.join_ticket.as_deref() {
        Some(text) => Some(
            JoinTicketCodec::decode(text)
                .map_err(|error| format!("the join ticket could not be read: {error}"))?,
        ),
        None => None,
    };

    let settings = NodeSettings {
        profile_directory,
        network: network_of(options),
    };

    let node = Arc::new(Node::start(&settings).map_err(|error| error.to_string())?);
    let (engine, commands) = Engine::channel();

    let worker = {
        let node = Arc::clone(&node);
        std::thread::Builder::new()
            .name("distro-engine".to_owned())
            .spawn(move || Engine::new(node).run(&commands))
            .map_err(|error| format!("the engine thread could not be started: {error}"))?
    };

    // The last startup step, and the first thing the user sees happening: the
    // ladder is walked on its own thread, the status line says `joining` while
    // it is, and the account of every rung tried lands in the notices whether
    // it succeeded or not (AC3 — a visible diagnostic, never a hang).
    engine.send(EngineCommand::Join(Box::new(ticket)));

    let mut terminal = ratatui::init();
    let interface = tui::run(&mut terminal, &node, &engine);
    ratatui::restore();

    engine.stop();
    let _ = worker.join();

    // Explicit, so the driver stops before the process starts tearing other
    // things down (`NetworkRuntime`'s contract, point 6). A surviving `Arc` is
    // unreachable once the engine thread has joined, and not worth a panic if
    // it ever is: `Drop` does the same thing on a timeout.
    if let Ok(node) = Arc::try_unwrap(node) {
        node.shutdown();
    }

    interface.map_err(|error| format!("the terminal failed: {error}"))
}

/// The swarm configuration a launch's options ask for.
///
/// The only place the two shapes meet, and pure translation: each option either
/// replaces a default or is passed straight through. Nothing here decides
/// anything and nothing here validates — a malformed multiaddress was already
/// refused when it was parsed, and whether an asserted address is one a
/// stranger could dial is the adapter's judgement, made against the predicate
/// it already owns.
///
/// A function of its own so that it can be asserted rather than read: this is
/// where an option silently failing to reach the network would look exactly
/// like an option that had no effect.
fn network_of(options: LaunchOptions) -> NetworkConfig {
    NetworkConfig {
        listen_addresses: if options.listen_addresses.is_empty() {
            NetworkConfig::default().listen_addresses
        } else {
            options.listen_addresses
        },
        // Straight through, in the order supplied. These are addresses this
        // peer is advertised *at*, never hosts it contacts (S1) — the field
        // they land in says the same thing, and there is nothing between here
        // and there that could turn one into the other.
        external_addresses: options.external_addresses,
        enable_lan_discovery: options.lan_discovery,
        broadcast_topic: options
            .broadcast_topic
            .unwrap_or_else(|| NetworkConfig::DEFAULT_TOPIC.to_owned()),
        ..NetworkConfig::default()
    }
}

/// Reports this profile's identity without starting anything.
///
/// Load-or-create, like every launch: running this against a fresh directory
/// creates the identity it then prints, which is the same thing the first real
/// launch would have done (AC1). Nothing is asked and nothing is configured.
fn print_identity(profile_directory: &std::path::Path) -> Result<(), String> {
    let stores = LocalStores::open(profile_directory).map_err(|error| error.to_string())?;
    let signer = stores
        .identity_keys()
        .load_or_create_signer()
        .map_err(|error| error.to_string())?;

    let context = identity::application::IdentityContext::new(
        stores.identity_keys(),
        stores.trust_records() as Arc<dyn identity::ports::TrustRecordStorePort + Send + Sync>,
    );
    identity::ports::IdentityCommandPort::initialize_local_identity(context.commands(), None)
        .map_err(|error| error.to_string())?;

    let summary = context
        .queries()
        .local_identity()
        .ok_or("the identity did not initialise")?;

    println!("profile     {}", stores.root().display());
    println!("display     {}", summary.display_name);
    println!("fingerprint {}", summary.fingerprint);

    // The key file name, so an operator can see which file to copy or delete —
    // never the key.
    println!("key file    {}", stores.identity_keys().path().display());
    debug_assert_eq!(signer.peer(), summary.peer);
    let _ = FileIdentityKeyStore::FILE_NAME;

    Ok(())
}
