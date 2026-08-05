use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

use infra_net_libp2p::mapping::EndpointMapping;

/// Everything a launch can be told, parsed from `argv`.
///
/// # Hand-written rather than a parser dependency
///
/// Seven options, no subcommands, no completions. A parsing crate would be the
/// largest dependency in this crate after the terminal library, and it would
/// buy nothing this file does not already do — while making the `--help` text
/// something generated rather than something written, which matters here
/// because S7 and S8 require specific sentences to be in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    /// `--profile <DIR>`: overrides where this instance keeps its identity and
    /// caches, so two instances can run on one machine (OP-13).
    pub profile_directory: Option<PathBuf>,
    /// `--ticket <STRING>`: a join ticket to fall back on if the cache and the
    /// LAN produce nothing — D1's third rung, supplied at startup instead of
    /// pasted into the UI.
    pub join_ticket: Option<String>,
    /// `--topic <NAME>`: the broadcast topic, which is a network identifier as
    /// much as a topic name — two instances with different topics are on two
    /// different networks even if they connect (D3).
    pub broadcast_topic: Option<String>,
    /// `--listen <MULTIADDR>`, repeatable: the addresses to bind. Empty means
    /// the default — every interface, on a port the OS picks.
    ///
    /// **Not a bootstrap list** (S1): these are addresses *this* peer binds,
    /// not hosts it contacts. The distinction is the whole of S1, so the option
    /// is named for what it does.
    ///
    /// It exists because the default port is `0`, which is right for a machine
    /// running two instances and wrong for one that wants to be found again
    /// after a restart: a peer that changes port is at a different address, and
    /// the addresses other peers cached for it stop working. Pinning a port is
    /// what makes the warm-start rung (D1 rung a) and a forwarded port through
    /// a NAT possible.
    pub listen_addresses: Vec<String>,
    /// `--external-address <MULTIADDR>`, repeatable: addresses the world
    /// reaches this peer at, asserted by whoever forwarded the port. Empty —
    /// the ordinary case — means nothing is asserted.
    ///
    /// **This peer's own address, not a host to contact** (S1). It is
    /// advertised so other peers can reach *this* one, and nothing dials it.
    /// That makes it the same shape as the bootstrap list this project does
    /// not have, and unrelated to it — the distinction `listen_addresses`
    /// draws, drawn again for the same reason.
    ///
    /// It exists for the peer that has nobody to ask. An address otherwise
    /// becomes advertised only after two peers have observed it or a probe has
    /// confirmed it, and the first instance on a network with a freshly
    /// forwarded port has neither: it would wait for a peer that does not
    /// exist yet.
    ///
    /// Asserted, never proven. Supplying one does not suppress observation or
    /// probing, and a later verdict that the address does not work still
    /// stands (S2) — this is the weakest of the three sources of an advertised
    /// address, not the strongest.
    pub external_addresses: Vec<String>,
    /// `--no-lan`: switches off mDNS. The one thing this build does
    /// unprompted is multicast on the local link; a user on a network where
    /// that is unwelcome can say so.
    pub lan_discovery: bool,
    /// `--print-identity`: report this profile's identity and exit without
    /// starting a network or a terminal. The headless path — it is what a
    /// smoke check runs, and what tells an operator which `PeerId` a profile
    /// directory holds before launching anything.
    pub print_identity: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            profile_directory: None,
            join_ticket: None,
            broadcast_topic: None,
            listen_addresses: Vec::new(),
            external_addresses: Vec::new(),
            lan_discovery: true,
            print_identity: false,
        }
    }
}

/// What the arguments asked for.
///
/// `--help` and `--version` are not options on a run; they are two other things
/// the program can do, and modelling them as a third state keeps `main` from
/// having to remember to check a flag before starting a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchRequest {
    /// Run, with these options.
    Run(Box<LaunchOptions>),
    /// Print the usage text and exit successfully.
    Help,
    /// Print the version and exit successfully.
    Version,
}

impl LaunchOptions {
    /// Parses arguments **excluding** the program name.
    ///
    /// Long options only. A single-letter alias saves four characters and costs
    /// a second syntax to explain, and there is no option here anyone types
    /// often enough to care.
    pub fn parse<I, S>(arguments: I) -> Result<LaunchRequest, ArgumentError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter().map(Into::into).peekable();

        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--help" | "-h") => return Ok(LaunchRequest::Help),
                Some("--version" | "-V") => return Ok(LaunchRequest::Version),
                Some("--no-lan") => options.lan_discovery = false,
                Some("--print-identity") => options.print_identity = true,
                Some("--profile") => {
                    options.profile_directory =
                        Some(PathBuf::from(value_of("--profile", arguments.next())?));
                }
                Some("--ticket") => {
                    options.join_ticket = Some(text_of("--ticket", arguments.next())?);
                }
                Some("--topic") => {
                    options.broadcast_topic = Some(text_of("--topic", arguments.next())?);
                }
                Some("--listen") => {
                    options
                        .listen_addresses
                        .push(text_of("--listen", arguments.next())?);
                }
                Some("--external-address") => {
                    let value = text_of("--external-address", arguments.next())?;
                    options
                        .external_addresses
                        .push(external_address("--external-address", &value)?);
                }
                _ => return Err(ArgumentError::Unknown(lossy(&argument))),
            }
        }

        Ok(LaunchRequest::Run(Box::new(options)))
    }
}

fn value_of(flag: &'static str, value: Option<OsString>) -> Result<OsString, ArgumentError> {
    value.ok_or(ArgumentError::MissingValue(flag))
}

fn text_of(flag: &'static str, value: Option<OsString>) -> Result<String, ArgumentError> {
    value_of(flag, value)?
        .into_string()
        .map_err(|_| ArgumentError::NotText(flag))
}

fn lossy(argument: &OsStr) -> String {
    argument.to_string_lossy().into_owned()
}

/// Checks that a value is a well-formed multiaddress and returns the text that
/// will be advertised.
///
/// # Syntax only
///
/// Whether the address is *global* is the adapter's judgement — it owns the
/// tested predicate, and a second copy here would drift from it. Whether the
/// address actually works from outside is nobody's judgement: it is the
/// operator's claim, and it is contradicted by evidence rather than checked.
///
/// # Why here, when `--listen` is not checked here
///
/// A malformed listen address already fails loudly: nothing binds and the
/// launch stops. A malformed external address would not — the value is only
/// ever advertised, so the launch would succeed and the peer would be
/// unreachable in exactly the way the flag was typed to fix. This is the option
/// someone reaches for when nothing else has worked, so it refuses at the point
/// it is typed and repeats the value back (P3-3, S4).
///
/// The trimmed value is what is kept, because the trimmed value is what was
/// checked; keeping the untrimmed one would validate one string and advertise
/// another.
fn external_address(flag: &'static str, value: &str) -> Result<String, ArgumentError> {
    let address = value.trim();

    EndpointMapping::parse(address).map_err(|_| ArgumentError::MalformedAddress {
        flag,
        value: value.to_owned(),
    })?;

    Ok(address.to_owned())
}

/// Why the arguments could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentError {
    /// An option this build does not know. Not tolerated the way an unknown
    /// *wire* field is (S2): a peer's unknown field is another peer's newer
    /// build, while an unknown flag is this user's typo, and silently ignoring
    /// it would start a node that is not the one they asked for.
    Unknown(String),
    /// An option that takes a value was last on the line.
    MissingValue(&'static str),
    /// An option's value is not valid text.
    NotText(&'static str),
    /// An option's value had to be a multiaddress and is not one.
    ///
    /// Carries the value rather than just the flag, because the point of the
    /// refusal is to show the user the typo (P3-3, S4).
    MalformedAddress {
        /// The option that was given the value.
        flag: &'static str,
        /// The value, as it was typed.
        value: String,
    },
}

impl fmt::Display for ArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(argument) => write!(f, "unknown argument {argument:?}"),
            Self::MissingValue(flag) => write!(f, "{flag} needs a value"),
            Self::NotText(flag) => write!(f, "the value of {flag} is not valid text"),
            Self::MalformedAddress { flag, value } => write!(
                f,
                "the value of {flag} is not a well-formed multiaddress: {value:?}"
            ),
        }
    }
}

impl std::error::Error for ArgumentError {}
