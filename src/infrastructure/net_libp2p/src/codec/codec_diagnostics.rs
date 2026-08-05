use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The local counters S2 requires: every tolerated oddity and every refusal is
/// counted where it happened.
///
/// # Why counting is not optional
///
/// S2's tolerance rule says unknown fields and unknown payload kinds are
/// "ignored with a local diagnostic counter". Ignoring without counting would
/// make an entire class of wire drift invisible — a peer shipping a field
/// nobody reads looks exactly like a peer shipping nothing — and AC14 is
/// asserted against these numbers. The rejections are counted for the mirror
/// reason: an operator-free network has no server log to consult, so the only
/// place a "why did that peer go quiet" answer can live is this process.
///
/// Cheap to clone: every clone shares one set of atomics, so an adapter, the
/// swarm driver, and a diagnostics pane all read the same numbers.
#[derive(Debug, Clone, Default)]
pub struct CodecDiagnostics {
    counters: Arc<Counters>,
}

#[derive(Debug, Default)]
struct Counters {
    tolerated_minor: AtomicU64,
    unknown_fields: AtomicU64,
    unknown_payload_kinds: AtomicU64,
    rejected_major: AtomicU64,
    oversize_frames: AtomicU64,
    malformed_frames: AtomicU64,
    rate_limited: AtomicU64,
    dropped_events: AtomicU64,
    external_candidates_seen: AtomicU64,
    external_candidates_recorded: AtomicU64,
    external_addresses_promoted: AtomicU64,
}

impl CodecDiagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Envelopes accepted from a same-major peer whose minor version is newer
    /// than this build's (`Compatibility::Tolerate`).
    pub fn tolerated_minor(&self) -> u64 {
        self.counters.tolerated_minor.load(Ordering::Relaxed)
    }

    /// Fields present on the wire that this build has no name for. Counted per
    /// field, not per envelope, so a peer adding three fields reads as three.
    pub fn unknown_fields(&self) -> u64 {
        self.counters.unknown_fields.load(Ordering::Relaxed)
    }

    /// Envelopes carrying a [`PayloadKind`](shared_types::PayloadKind) code
    /// this build does not know.
    pub fn unknown_payload_kinds(&self) -> u64 {
        self.counters.unknown_payload_kinds.load(Ordering::Relaxed)
    }

    /// Envelopes refused because their major version is not this build's.
    pub fn rejected_major(&self) -> u64 {
        self.counters.rejected_major.load(Ordering::Relaxed)
    }

    /// Frames refused on size before anything was deserialized (S6).
    pub fn oversize_frames(&self) -> u64 {
        self.counters.oversize_frames.load(Ordering::Relaxed)
    }

    /// Frames that were not a decodable envelope for any other reason.
    pub fn malformed_frames(&self) -> u64 {
        self.counters.malformed_frames.load(Ordering::Relaxed)
    }

    /// Inbound envelopes refused because the sending peer was over its rate
    /// limit (S6).
    pub fn rate_limited(&self) -> u64 {
        self.counters.rate_limited.load(Ordering::Relaxed)
    }

    /// Network events dropped because the composition root was not draining
    /// them fast enough. Never silent — that is what this counter is for.
    pub fn dropped_events(&self) -> u64 {
        self.counters.dropped_events.load(Ordering::Relaxed)
    }

    /// Addresses a remote peer reported seeing this peer at, counted as they
    /// arrive and before any of them is judged.
    ///
    /// # Why external-address discovery is counted at all
    ///
    /// Its failure mode is silence: a peer that never learns its public
    /// address simply stays unreachable, and looks from the inside exactly
    /// like a peer nobody happens to be messaging. These three numbers are the
    /// only way to tell the difference. Read together they say where the
    /// process stopped — nothing seen means no peer is identifying us, seen
    /// without recorded means every observation was a LAN or loopback address,
    /// and recorded without promoted means no second peer has corroborated one
    /// yet.
    pub fn external_candidates_seen(&self) -> u64 {
        self.counters
            .external_candidates_seen
            .load(Ordering::Relaxed)
    }

    /// Observed addresses that were attributed to a peer, passed the
    /// global-address filter, and were counted toward corroboration.
    pub fn external_candidates_recorded(&self) -> u64 {
        self.counters
            .external_candidates_recorded
            .load(Ordering::Relaxed)
    }

    /// Addresses corroborated by enough distinct peers to be advertised.
    pub fn external_addresses_promoted(&self) -> u64 {
        self.counters
            .external_addresses_promoted
            .load(Ordering::Relaxed)
    }

    pub(crate) fn count_tolerated_minor(&self) {
        self.counters
            .tolerated_minor
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_unknown_fields(&self, fields: u64) {
        self.counters
            .unknown_fields
            .fetch_add(fields, Ordering::Relaxed);
    }

    pub(crate) fn count_unknown_payload_kind(&self) {
        self.counters
            .unknown_payload_kinds
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_rejected_major(&self) {
        self.counters.rejected_major.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_oversize_frame(&self) {
        self.counters
            .oversize_frames
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_malformed_frame(&self) {
        self.counters
            .malformed_frames
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_rate_limited(&self) {
        self.counters.rate_limited.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_dropped_event(&self) {
        self.counters.dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_external_candidate_seen(&self) {
        self.counters
            .external_candidates_seen
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_external_candidate_recorded(&self) {
        self.counters
            .external_candidates_recorded
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_external_address_promoted(&self) {
        self.counters
            .external_addresses_promoted
            .fetch_add(1, Ordering::Relaxed);
    }
}
