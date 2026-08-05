use shared_types::PeerId;

/// Whether this peer refuses another peer's content (invariant 11, canvas §4).
///
/// Invariant 11 says a blocked peer's envelopes are dropped at the application
/// boundary of every context — but this context had nowhere to ask, so the rule
/// had no enforcement site here at all. This is that site: the
/// `AcceptInboundMessage` handler consults it before any aggregate is touched,
/// and refuses the envelope with
/// [`RejectionReason::AuthorBlocked`](crate::domain::events::RejectionReason::AuthorBlocked).
///
/// # Messaging's own trait, on purpose
///
/// The block list itself is `identity`'s (`TrustRecord`'s orthogonal `Blocked`
/// flag). Importing that would be a cross-context import, and `shared_types`
/// hosts no port traits (canvas §2.4, §4) — so this context states only the
/// question it needs answered, and the composition root wires it to the one
/// underlying list. The same reasoning gives this context its own
/// [`ClockPort`](crate::ports::ClockPort),
/// [`EnvelopeSignerPort`](crate::ports::EnvelopeSignerPort), and
/// [`EnvelopeVerifierPort`](crate::ports::EnvelopeVerifierPort).
///
/// # Blocking is local, and says nothing
///
/// Nothing is announced to the blocked peer or to anyone else (invariant 11).
/// A blocked peer keeps sending; this peer stops listening. There is no appeal,
/// no reputation, and no state anywhere but here.
///
/// # No error type
///
/// The decision is a lookup in a list this process already holds — there is no
/// I/O in the call and nothing to fail. An implementation that would have to
/// read a file or a network to answer must load the list ahead of time instead:
/// a failure here has no safe default, since blocking everyone silences the
/// network and blocking no one ignores the user's decision.
pub trait AuthorPolicyPort {
    /// Whether content authored by `peer` is refused by this peer.
    fn is_blocked(&self, peer: PeerId) -> bool;
}
