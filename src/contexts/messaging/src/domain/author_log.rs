use std::collections::BTreeMap;

use shared_types::PeerId;

use crate::domain::{Message, Millis, SequenceNumber};

/// One author's slice of a conversation: what has been applied, how far it
/// runs, what it has committed to, and what is waiting for a gap to close
/// (invariant 5, rule R).
///
/// Sequence numbers are counted per `(author, conversation)`, so every author
/// gets one of these and they never interact. That is what makes AC8 decidable
/// without a clock or any agreement between peers: an author's own order is
/// the only order that has to hold.
///
/// # Applied is ordered, but not always unbroken
///
/// [`messages`](Self::messages) is always in ascending sequence order and never
/// shows a message before the ones it follows. It is *contiguous* only while no
/// gap has been abandoned: when one is (rule R), the run continues past the
/// numbers this log gave up on and
/// [`MessageGapClosed`](crate::domain::events::MessageGapClosed) says which
/// those were.
///
/// That is exactly why [`is_applied`](Self::is_applied) tests membership rather
/// than comparing against [`high_water_mark`](Self::high_water_mark) — after a
/// skip the two differ, and conflating them would report content that was
/// *lost* as content that was already *seen* (invariant 6, as tightened).
///
/// # Two marks, not one
///
/// [`origin`](Self::origin) is the lowest sequence this log has committed to
/// and `high_water` the highest it has applied. The pair is what distinguishes
/// "not yet received" from "never will be": a message below the origin, or
/// inside a gap the log has closed behind, is
/// [out of reach](Self::is_out_of_reach) — not a duplicate.
///
/// # The stream starts where this peer first heard it (D10)
///
/// A log with no origin has heard nothing from this author, so it has no reason
/// to believe the author ever sent it anything: AC10 gives a late joiner no
/// history replay, and this peer's own mark does not survive its process (D7)
/// while the sender's counter does (D12). The first sequence that becomes
/// applicable therefore *establishes* the origin, and the run below it is not a
/// gap — it never existed here. Only a run between two sequences this log
/// actually observed is loss, and only that is reported as
/// [`MessageGapClosed`](crate::domain::events::MessageGapClosed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorLog {
    author: PeerId,
    applied: Vec<Message>,
    origin: Option<SequenceNumber>,
    high_water: Option<SequenceNumber>,
    buffered: BTreeMap<SequenceNumber, BufferedMessage>,
}

/// What [`AuthorLog::close_gap`] came to.
///
/// The distinction this type exists to force is D10's: an absence below the
/// first sequence this peer ever saw from an author is **not** loss — those
/// messages were never in flight here — while an absence between two sequences
/// it did see is. Returning a bare range conflated the two, and the interface
/// then told a user that messages which never existed for it "were never
/// received".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GapClose {
    /// Nothing was held, so there was no wait to end and nothing moved.
    Nothing,
    /// First sight of this author: the lowest held sequence became the origin
    /// and the stream starts there (AC10, D10). Nothing was abandoned.
    OriginEstablished { origin: SequenceNumber },
    /// A run between two sequences this log observed was given up on,
    /// inclusive of both ends. This is loss, and it is reported (AC15).
    Abandoned {
        from: SequenceNumber,
        to: SequenceNumber,
    },
}

/// A message held until its gap closes, with the **local** instant it arrived.
///
/// This log's private storage record — nothing here reaches a read view. The
/// instant is the one this peer observed, never the author's claimed send time:
/// ageing a gap by an author's own claim would let that author hold a gap open
/// forever by backdating.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BufferedMessage {
    message: Message,
    received_at: Millis,
}

impl AuthorLog {
    /// How many out-of-order messages one author may keep this peer holding.
    ///
    /// A gap means *not yet received* (invariant 5), so the buffer must exist;
    /// it must also be bounded, because on an open network the peer creating
    /// the gap may be doing it on purpose and there is no gatekeeper to add a
    /// limit later (S6). At 64 messages of at most
    /// [`MessageBody::MAX_BYTES`](crate::domain::MessageBody::MAX_BYTES) each,
    /// one author can pin at most ~1 MiB per conversation — bounded, and
    /// generous next to any plausible reordering window on a real path.
    ///
    /// Reaching it evicts nothing and refuses nothing: it is the second trigger
    /// for the same close the tolerance window performs (rule R). The oldest
    /// gap is abandoned, everything held becomes visible, and the arrival is
    /// judged again against that state. Refusing the arrival instead would let
    /// a flooding peer decide which of an honest peer's messages this one never
    /// sees.
    pub const MAX_BUFFERED_MESSAGES: usize = 64;

    pub(super) const fn empty(author: PeerId) -> Self {
        Self {
            author,
            applied: Vec::new(),
            origin: None,
            high_water: None,
            buffered: BTreeMap::new(),
        }
    }

    /// A log that has *issued* numbers up to `high_water` but holds none of the
    /// messages (D7, D12).
    ///
    /// This is what a restarted peer's own log looks like: history died with
    /// the process, the counter did not. The origin is pinned to the mark so
    /// nothing below it is ever taken for content this log holds — which
    /// [`is_applied`](Self::is_applied) would have claimed, wrongly, under a
    /// high-water comparison.
    pub(super) const fn rehydrated(author: PeerId, high_water: SequenceNumber) -> Self {
        Self {
            author,
            applied: Vec::new(),
            origin: Some(high_water),
            high_water: Some(high_water),
            buffered: BTreeMap::new(),
        }
    }

    pub const fn author(&self) -> PeerId {
        self.author
    }

    /// Applied messages, in the author's send order.
    pub fn messages(&self) -> &[Message] {
        &self.applied
    }

    /// The lowest sequence number this log has committed to; `None` before it
    /// has committed to anything.
    ///
    /// The first sequence this log applies commits it: genesis commits it to
    /// [`SequenceNumber::FIRST`], and first contact part-way through an
    /// author's run commits it to whatever that first applicable sequence was
    /// (D10). Abandoning a gap afterwards moves it to the last sequence given
    /// up on, because from that moment nothing at or below is ever admissible
    /// again.
    pub const fn origin(&self) -> Option<SequenceNumber> {
        self.origin
    }

    /// The highest sequence number applied so far; `None` before the first.
    pub const fn high_water_mark(&self) -> Option<SequenceNumber> {
        self.high_water
    }

    /// How many messages are held waiting for a gap to close. Diagnostics
    /// only — none of them is visible in the read view.
    pub fn buffered_count(&self) -> usize {
        self.buffered.len()
    }

    /// Whether this author has content waiting on a missing message.
    pub fn has_gap(&self) -> bool {
        !self.buffered.is_empty()
    }

    /// Whether the buffer can hold nothing more.
    pub fn is_buffer_full(&self) -> bool {
        self.buffered.len() >= Self::MAX_BUFFERED_MESSAGES
    }

    /// The local instant of the message that has waited longest; `None` when
    /// nothing is held.
    ///
    /// The *oldest* arrival decides when a gap is abandoned: it is the one that
    /// has been unreadable the longest, and a later arrival must not extend the
    /// wait for it (rule R).
    pub fn oldest_buffered_at(&self) -> Option<Millis> {
        self.buffered.values().map(|held| held.received_at).min()
    }

    /// The lowest sequence number held; `None` when nothing is held.
    pub fn lowest_buffered(&self) -> Option<SequenceNumber> {
        self.buffered.keys().copied().next()
    }

    /// An applied message by its sequence number. Buffered messages are
    /// deliberately not reachable: they are not part of the conversation yet.
    pub fn applied(&self, sequence: SequenceNumber) -> Option<&Message> {
        self.index_of(sequence).map(|index| &self.applied[index])
    }

    /// Whether this log actually holds `sequence`.
    ///
    /// Membership, never a comparison against the mark. The two agree only
    /// while nothing has been skipped, and where they disagree the comparison
    /// is wrong in the one direction that matters: it would call lost content a
    /// duplicate (invariant 6).
    pub fn is_applied(&self, sequence: SequenceNumber) -> bool {
        self.index_of(sequence).is_some()
    }

    /// Whether `sequence` is already held, waiting for its gap to close.
    pub fn is_buffered(&self, sequence: SequenceNumber) -> bool {
        self.buffered.contains_key(&sequence)
    }

    /// Whether `sequence` names content this log can never take: it is below
    /// the origin it committed to, or inside a gap that has closed behind it
    /// (rule R, AC15).
    ///
    /// The two clauses overlap in every state this log can reach — the origin
    /// never sits above the mark — and both are stated because they are the
    /// same fact seen from each end: the floor this log committed to, and the
    /// run it has moved past. Neither is a duplicate.
    pub fn is_out_of_reach(&self, sequence: SequenceNumber) -> bool {
        let below_origin = self.origin.is_some_and(|origin| sequence < origin);
        let inside_closed_gap =
            self.high_water.is_some_and(|mark| sequence <= mark) && !self.is_applied(sequence);

        below_origin || inside_closed_gap
    }

    pub(super) fn applied_mut(&mut self, sequence: SequenceNumber) -> Option<&mut Message> {
        self.index_of(sequence)
            .map(|index| &mut self.applied[index])
    }

    /// Every applied message, mutably, in send order.
    pub(super) fn applied_messages_mut(&mut self) -> impl Iterator<Item = &mut Message> {
        self.applied.iter_mut()
    }

    /// Appends a message the caller has established is contiguous.
    pub(super) fn apply(&mut self, message: Message) {
        let sequence = message.sequence();
        if self.origin.is_none() {
            self.origin = Some(sequence);
        }
        self.high_water = Some(sequence);
        self.applied.push(message);
    }

    /// Holds an out-of-order message until its gap closes, stamping it with the
    /// local instant it arrived; `false` when the buffer is full and nothing
    /// was stored.
    pub(super) fn buffer(&mut self, message: Message, received_at: Millis) -> bool {
        if self.is_buffer_full() {
            return false;
        }

        self.buffered.insert(
            message.sequence(),
            BufferedMessage {
                message,
                received_at,
            },
        );
        true
    }

    /// Stops waiting on the gap below the lowest held message, applying
    /// everything that makes contiguous, and says what that came to.
    ///
    /// Two very different things end the same wait, which is why the answer is
    /// [`GapClose`] rather than a range:
    ///
    /// * This log had committed to nothing, so the lowest held message
    ///   *establishes* the origin. Nothing below it was ever in flight to this
    ///   peer (AC10, D10) — there is no gap, and nothing is lost.
    /// * This log had a mark, so the run between it and the lowest held message
    ///   was genuinely in flight and did not arrive. That is loss, and the
    ///   range is named.
    pub(super) fn close_gap(&mut self) -> GapClose {
        let Some(lowest) = self.lowest_buffered() else {
            return GapClose::Nothing;
        };

        // First sight of this author: no origin committed and nothing applied.
        // The two are set together everywhere, so this is exactly "this log has
        // never held anything from this author".
        if self.origin.is_none() && self.high_water.is_none() {
            let held = self
                .buffered
                .remove(&lowest)
                .expect("the lowest held sequence is held");
            // `apply` is what commits the origin, so the stream starts here.
            self.apply(held.message);
            self.drain_contiguous();

            return GapClose::OriginEstablished { origin: lowest };
        }

        // Both hold in every reachable state: a buffered message always sits
        // above the mark, so the mark has a successor and the message has a
        // predecessor.
        let (Ok(from), Some(to)) = (
            SequenceNumber::following(self.high_water),
            lowest.predecessor(),
        ) else {
            return GapClose::Nothing;
        };

        self.high_water = Some(to);
        self.drain_contiguous();

        GapClose::Abandoned { from, to }
    }

    /// Applies every buffered message that has just become contiguous, in
    /// order.
    ///
    /// Reports nothing: the caller took the applied length before appending,
    /// so the tail of [`messages`](Self::messages) past that point is exactly
    /// what became visible — no message has to be cloned out to say so.
    pub(super) fn drain_contiguous(&mut self) {
        while let Ok(next) = SequenceNumber::following(self.high_water) {
            let Some(held) = self.buffered.remove(&next) else {
                break;
            };
            self.apply(held.message);
        }
    }

    fn index_of(&self, sequence: SequenceNumber) -> Option<usize> {
        self.applied
            .binary_search_by(|message| message.sequence().cmp(&sequence))
            .ok()
    }
}
