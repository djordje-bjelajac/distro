use std::collections::BTreeMap;
use std::sync::Mutex;

use membership::domain::events::MembershipEvent;
use messaging::domain::events::MessagingEvent;
use shared_types::PeerId;

use crate::stores::guard;
use crate::trace::{TraceEntry, TraceEvent};

/// The ordered record of everything a simulated run did (S5, AC13).
///
/// # This type is the determinism claim
///
/// The canvas requires that the same seed and the same script produce a
/// byte-identical trace across two runs. [`render`](Self::render) is what
/// "byte-identical" is measured on: one line per entry, every field either an
/// integer, a fixed token, or a scenario-supplied label. Nothing in a rendered
/// line comes from a pointer, a hash iteration order, a real clock, or an
/// environment variable, so two runs can only differ if the *simulation*
/// differed — which is exactly what the self-test is looking for.
///
/// # Shared by the whole network, not by one peer
///
/// Every peer's publishers and the fabric itself write here, so the interleaving
/// across peers is part of what is pinned. A per-peer trace would let two peers
/// swap the order of their events between runs with nothing to notice.
///
/// # Labels
///
/// Peers are registered with the names a scenario gave them, so a trace reads
/// `alice -> bob direct#1` rather than sixty-four hex characters. An
/// unregistered peer renders as a short digest of its key, which is still
/// deterministic — a missing label degrades readability, never reproducibility.
#[derive(Debug, Default)]
pub struct EventTrace {
    state: Mutex<TraceState>,
}

#[derive(Debug, Default)]
struct TraceState {
    labels: BTreeMap<PeerId, String>,
    entries: Vec<TraceEntry>,
}

impl EventTrace {
    /// An empty trace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the name a scenario knows `peer` by.
    pub fn label(&self, peer: PeerId, label: &str) {
        guard(&self.state).labels.insert(peer, label.to_owned());
    }

    /// The name `peer` renders as.
    pub fn label_of(&self, peer: PeerId) -> String {
        crate::trace::label_of(&guard(&self.state).labels, &peer)
    }

    /// Appends one entry, stamped with the instant it happened at.
    pub fn record(&self, at: u64, event: TraceEvent) {
        guard(&self.state).entries.push(TraceEntry { at, event });
    }

    /// Every entry, in the order it happened.
    pub fn entries(&self) -> Vec<TraceEntry> {
        guard(&self.state).entries.clone()
    }

    /// How many entries the trace holds.
    pub fn len(&self) -> usize {
        guard(&self.state).entries.len()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Discards every entry, keeping the labels.
    ///
    /// The usual reason is to ignore a scenario's setup and pin only the part
    /// under test.
    pub fn clear(&self) {
        guard(&self.state).entries.clear();
    }

    /// The whole trace as text: one line per entry, newline-terminated.
    ///
    /// This is what two runs are compared on.
    pub fn render(&self) -> String {
        let state = guard(&self.state);
        let mut rendered = String::new();

        for entry in &state.entries {
            rendered.push_str(&format!(
                "{:>10} {}\n",
                entry.at,
                entry.event.render(&state.labels)
            ));
        }

        rendered
    }

    /// Every `membership` event, paired with the peer that published it.
    pub fn membership_events(&self) -> Vec<(PeerId, MembershipEvent)> {
        guard(&self.state)
            .entries
            .iter()
            .filter_map(|entry| match &entry.event {
                TraceEvent::Membership { peer, event } => Some((*peer, *event)),
                _ => None,
            })
            .collect()
    }

    /// Every `messaging` event, paired with the peer that published it.
    pub fn messaging_events(&self) -> Vec<(PeerId, MessagingEvent)> {
        guard(&self.state)
            .entries
            .iter()
            .filter_map(|entry| match &entry.event {
                TraceEvent::Messaging { peer, event } => Some((*peer, *event)),
                _ => None,
            })
            .collect()
    }

    /// Everything one peer's `membership` context published, in order.
    pub fn membership_events_of(&self, peer: PeerId) -> Vec<MembershipEvent> {
        self.membership_events()
            .into_iter()
            .filter_map(|(publisher, event)| (publisher == peer).then_some(event))
            .collect()
    }

    /// Everything one peer's `messaging` context published, in order.
    pub fn messaging_events_of(&self, peer: PeerId) -> Vec<MessagingEvent> {
        self.messaging_events()
            .into_iter()
            .filter_map(|(publisher, event)| (publisher == peer).then_some(event))
            .collect()
    }
}
