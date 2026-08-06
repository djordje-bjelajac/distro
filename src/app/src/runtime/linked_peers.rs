use membership::ports::{KnownPeerView, NetworkView};
use shared_types::PeerId;

/// The peers a direct message can be sent to right now: those holding an
/// established session, read off one snapshot of the roster.
///
/// # Why the root needs this at all
///
/// The heartbeat beacon has to be told who to speak to (canvas `0010` D7), and
/// it must not grow a `membership` dependency to find out — the root already
/// holds the query port and drives the tick, so the set is assembled here and
/// passed in.
///
/// # Why it is a function and not four lines inside the engine
///
/// Because choosing the predicate is a decision, and it is the one canvas D4
/// singles out as the mirror of the observed defect. Three plausible readings
/// of "who should get a heartbeat" are wrong:
///
/// * **peers that are `Online`** — a peer whose evidence arrived over somebody
///   else's link has no session here, so the send would fail on every tick and
///   the counter would blame the network for a set this root chose;
/// * **peers holding a live session**, which includes `Connecting` — a dial in
///   flight can carry nothing yet, so the heartbeat is refused and the peer
///   looks unreachable at the exact moment it is being reached;
/// * **every known peer** — the roster holds peers learned from mDNS and the
///   DHT that have never been dialled, which is a per-tick send to an
///   attacker-supplied identity.
///
/// The predicate is therefore not written here. It is
/// [`PeerStanding::is_linked`](membership::domain::PeerStanding::is_linked),
/// the same classification the status line counts, so `Connected(n)` and the
/// set of peers that get a heartbeat are by construction the same peers — and a
/// future change to what "linked" means moves both together instead of leaving
/// them to drift, which is how the two stories diverged in the first place
/// (canvas D5).
///
/// # One snapshot
///
/// A [`NetworkView`] is one traversal of the roster at one clock reading. The
/// set is read off *that*, never re-queried per peer: a second read would let
/// the roster change mid-round and produce a heartbeat list that matched no
/// state the roster was ever in.
pub fn linked_peers(view: &NetworkView) -> Vec<PeerId> {
    view.peers()
        .iter()
        .filter(|peer| peer.standing().is_linked())
        .map(|peer: &KnownPeerView| peer.peer)
        .collect()
}
