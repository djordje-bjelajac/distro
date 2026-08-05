use membership::domain::LivenessWindows;
use messaging::domain::DurationMillis;
use shared_types::ProtocolVersion;

use crate::clock::VirtualClock;
use crate::harness::{SimNetwork, SimSettings};

/// Builds a [`SimNetwork`], stating everything a scenario depends on before the
/// first peer exists.
///
/// # Why a builder rather than a constructor
///
/// Seed, epoch, protocol version, liveness windows, and gap tolerance are all
/// fixed for the life of a network, and every one of them changes what a
/// scenario means. A positional constructor taking five of them would let two
/// be transposed without the compiler noticing; a builder makes each an
/// explicit, named decision and lets a scenario override only the one it is
/// interrogating.
///
/// The seed is not optional and has no default. A scenario's determinism is
/// stated by its seed, and one picked implicitly is one nobody wrote down.
#[derive(Debug, Clone)]
pub struct SimNetworkBuilder {
    seed: u64,
    epoch: u64,
    settings: SimSettings,
    peers: Vec<String>,
}

impl SimNetworkBuilder {
    /// A network seeded with `seed`, at the default epoch, with the settings a
    /// real launch uses.
    pub const fn seeded(seed: u64) -> Self {
        Self {
            seed,
            epoch: VirtualClock::EPOCH_MILLIS,
            settings: SimSettings {
                protocol: ProtocolVersion::CURRENT,
                liveness_windows: LivenessWindows::DEFAULT,
                gap_tolerance: messaging::domain::Conversation::GAP_TOLERANCE,
                message_log_capacity: crate::stores::InMemoryMessageLog::DEFAULT_CAPACITY,
                acknowledge_directs: true,
            },
            peers: Vec::new(),
        }
    }

    /// Where the virtual clock starts.
    pub const fn starting_at(mut self, millis: u64) -> Self {
        self.epoch = millis;
        self
    }

    /// The wire protocol every peer speaks.
    pub const fn speaking(mut self, protocol: ProtocolVersion) -> Self {
        self.settings.protocol = protocol;
        self
    }

    /// The evidence-age thresholds presence is derived against.
    pub const fn with_liveness_windows(mut self, windows: LivenessWindows) -> Self {
        self.settings.liveness_windows = windows;
        self
    }

    /// How long a gap may stay open before the sweep abandons it (rule R).
    pub const fn with_gap_tolerance(mut self, tolerance: DurationMillis) -> Self {
        self.settings.gap_tolerance = tolerance;
        self
    }

    /// How many messages one peer's in-memory log holds (D7, S6).
    pub const fn with_message_log_capacity(mut self, capacity: usize) -> Self {
        self.settings.message_log_capacity = capacity;
        self
    }

    /// Whether a delivered 1:1 message acknowledges itself back to its sender.
    ///
    /// Turning it off holds every direct message at `Pending`, which is how a
    /// scenario watches a disconnect turn them into stated failures (D10,
    /// AC11).
    pub const fn acknowledging_directs(mut self, acknowledge: bool) -> Self {
        self.settings.acknowledge_directs = acknowledge;
        self
    }

    /// Replaces the whole settings block.
    pub const fn with_settings(mut self, settings: SimSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Adds peers with these names, in this order.
    ///
    /// They are constructed but have assumed nothing and joined nothing — see
    /// [`SimNetwork::boot_all`].
    pub fn with_peers<'a>(mut self, labels: impl IntoIterator<Item = &'a str>) -> Self {
        self.peers
            .extend(labels.into_iter().map(std::borrow::ToOwned::to_owned));
        self
    }

    /// Builds the network and its peers.
    pub fn build(self) -> SimNetwork {
        let mut network = SimNetwork::assemble(self.seed, self.epoch, self.settings);

        for label in &self.peers {
            network.add_peer(label);
        }

        network
    }
}
