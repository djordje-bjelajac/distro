use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use infra_net_libp2p::mapping::EndpointMapping;
use infra_net_libp2p::swarm::Reachability;

/// The local counters AC6, AC14 and AC15 ask for, plus the one non-counted fact
/// the interface reports the same way — whether strangers can dial this peer.
///
/// # Why counters and not log lines
///
/// Three acceptance criteria require that something be *counted in local
/// diagnostics*: envelopes that failed signature verification (AC6), envelopes
/// carrying an unknown minor addition (AC14), and abandoned gaps (AC15). A log
/// line satisfies none of them on a terminal application whose whole screen is
/// the conversation — the user would have to leave the program to see it. These
/// are numbers a pane can show.
///
/// Every counter is a plain `AtomicU64` with relaxed ordering: they are
/// reported, never compared against each other, and no decision anywhere reads
/// one. `infra-net-libp2p` keeps its own (`CodecDiagnostics`) for what only it
/// can see — oversize frames, rate-limited peers, dropped events — and the UI
/// shows both.
///
/// # The things here that are not numbers
///
/// [`Reachability`] is a state, not a tally, and it lives here because it is
/// reported exactly like one: the root receives it, keeps the latest, and shows
/// it. It is *held and nothing more* — no dial, no relay reservation and no
/// address selection changes on the strength of it (reachability canvas D4,
/// S5) — so this stays a place where facts are recorded rather than acted on.
///
/// The two external-address lists are the same kind of thing, and they are two
/// lists rather than one on purpose (external-address canvas D6, S4).
/// `--external-address` is the option somebody reaches for when nothing else
/// has worked, so *"I set the flag"* and *"the flag took effect"* have to be
/// separable without a debugger — and they come from two different places: the
/// first from the launch options, the second from the confirmations the network
/// reports back. Recording only one of them would leave the interesting failure
/// — supplied, and not in effect — looking exactly like success.
#[derive(Debug, Default)]
pub struct Diagnostics {
    envelopes_accepted: AtomicU64,
    envelopes_refused: AtomicU64,
    envelopes_ignored: AtomicU64,
    duplicates_ignored: AtomicU64,
    gaps_abandoned: AtomicU64,
    messages_never_received: AtomicU64,
    heartbeats_sent: AtomicU64,
    heartbeats_failed: AtomicU64,
    direct_delivery_failures: AtomicU64,
    uncorrelated_reports: AtomicU64,
    port_refusals: AtomicU64,
    /// The last answer the network gave to "can strangers dial this peer".
    ///
    /// No `Option` wrapper: [`Reachability::Unknown`] *is* the honest default,
    /// and `Option<Reachability>` would give the same fact two spellings — one
    /// of which every call site would have to remember not to render as
    /// alarming.
    reachability: Mutex<Reachability>,
    /// The addresses this launch was told to advertise with
    /// `--external-address`, in the order they were supplied. Empty — the
    /// ordinary case — means nothing was asserted.
    ///
    /// Written once at startup and never added to: an override is a
    /// launch-time claim, not something a running peer learns.
    external_addresses_supplied: Mutex<Vec<String>>,
    /// Those of the supplied addresses the network has since confirmed and is
    /// advertising.
    ///
    /// Filled from `NetworkEvent::ExternalAddressConfirmed`, which is a
    /// different source from the field above and is why both exist (D6). It is
    /// *not* a count of confirmations: observation and probing confirm
    /// addresses too, and reporting one of those as an override would tell a
    /// user their flag took effect on a launch that never carried one.
    external_addresses_in_effect: Mutex<Vec<String>>,
}

macro_rules! counter {
    ($count:ident, $read:ident) => {
        pub fn $count(&self) {
            self.$read.fetch_add(1, Ordering::Relaxed);
        }

        pub fn $read(&self) -> u64 {
            self.$read.load(Ordering::Relaxed)
        }
    };
}

impl Diagnostics {
    counter!(count_envelope_accepted, envelopes_accepted);
    counter!(count_envelope_refused, envelopes_refused);
    counter!(count_envelope_ignored, envelopes_ignored);
    counter!(count_duplicate_ignored, duplicates_ignored);
    counter!(count_heartbeat_sent, heartbeats_sent);
    counter!(count_heartbeat_failed, heartbeats_failed);
    counter!(count_direct_delivery_failure, direct_delivery_failures);
    counter!(count_uncorrelated_report, uncorrelated_reports);
    counter!(count_port_refusal, port_refusals);

    /// Records one abandoned gap and how many of an author's messages it wrote
    /// off (AC15).
    ///
    /// Two numbers because they answer different questions: how often the
    /// network is losing runs, and how much was lost. One gap of forty is a
    /// different fault from forty gaps of one.
    pub fn count_gap_abandoned(&self, messages: u64) {
        self.gaps_abandoned.fetch_add(1, Ordering::Relaxed);
        self.messages_never_received
            .fetch_add(messages, Ordering::Relaxed);
    }

    pub fn gaps_abandoned(&self) -> u64 {
        self.gaps_abandoned.load(Ordering::Relaxed)
    }

    pub fn messages_never_received(&self) -> u64 {
        self.messages_never_received.load(Ordering::Relaxed)
    }

    /// Records the verdict the network just reported.
    ///
    /// Replaces rather than accumulates, and needs no debouncing:
    /// `NetworkEvent::ReachabilityChanged` is emitted only on an actual
    /// transition, so whatever last arrived is what holds now — including the
    /// return to [`Reachability::Reachable`] after a failure, which the adapter
    /// deliberately does not latch.
    pub fn record_reachability(&self, reachability: Reachability) {
        *self
            .reachability
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = reachability;
    }

    /// The verdict as it stands.
    ///
    /// [`Reachability::Unknown`] until a probe concludes — which is a different
    /// fact from [`Reachability::Unreachable`] and must stay one everywhere it
    /// is read (reachability canvas S3).
    pub fn reachability(&self) -> Reachability {
        self.reachability
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records what this launch was asked to advertise (D6).
    ///
    /// Judges nothing. A value that is not a multiaddress was already refused
    /// when it was typed, and one that is not globally routable refuses the
    /// launch outright — by the time anything reaches here the question is only
    /// what the user asked for, not whether they may have it.
    ///
    /// The values are put through the same mapping the adapter puts an
    /// advertised address through, so that a confirmation can be recognised as
    /// answering one of them by equality rather than by guesswork. Anything
    /// that somehow does not survive that is kept as it was given rather than
    /// dropped: an unmatchable entry still shows up as *supplied*, which is the
    /// half of D6 that matters when something has gone wrong.
    pub fn record_supplied_external_addresses(&self, addresses: &[String]) {
        *lock(&self.external_addresses_supplied) = addresses
            .iter()
            .map(|address| advertised(address))
            .collect();
    }

    /// Notes that the network confirmed an address, when it is one of the
    /// supplied ones.
    ///
    /// Confirmations reach the root from three sources — a corroborated
    /// observation, an AutoNAT probe, and this option — and only the third is
    /// an override taking effect. Counting all three here would make "the flag
    /// took effect" unreadable, which is the confusion D6 exists to remove.
    ///
    /// Recording a confirmation is all this does. Nothing about an address
    /// being in effect makes it *work*: the probe that would contradict it runs
    /// regardless and its verdict is the one the status line shows (S2).
    pub fn record_confirmed_external_address(&self, address: &str) {
        let was_supplied = lock(&self.external_addresses_supplied)
            .iter()
            .any(|supplied| supplied == address);

        if !was_supplied {
            return;
        }

        let mut in_effect = lock(&self.external_addresses_in_effect);
        if !in_effect.iter().any(|known| known == address) {
            in_effect.push(address.to_owned());
        }
    }

    /// What the operator asked this peer to advertise (D6).
    pub fn external_addresses_supplied(&self) -> Vec<String> {
        lock(&self.external_addresses_supplied).clone()
    }

    /// Which of those the network has confirmed and is advertising (D6).
    pub fn external_addresses_in_effect(&self) -> Vec<String> {
        lock(&self.external_addresses_in_effect).clone()
    }
}

/// The text the adapter will advertise for a supplied value.
///
/// `NetworkEvent::ExternalAddressConfirmed` carries the multiaddress as libp2p
/// renders it, which is not always character-for-character what somebody typed
/// — `/ip6/2001:db8:0:0:0:0:0:1/…` comes back as `/ip6/2001:db8::1/…`. Both
/// sides go through the same mapping so that the two can be compared at all.
fn advertised(address: &str) -> String {
    EndpointMapping::parse(address).map_or_else(
        |_| address.trim().to_owned(),
        |endpoint| endpoint.address().to_owned(),
    )
}

/// A poisoned lock means a previous holder panicked. A list of addresses has no
/// invariant a panic could have broken, so recovering is correct.
fn lock(cell: &Mutex<Vec<String>>) -> MutexGuard<'_, Vec<String>> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}
