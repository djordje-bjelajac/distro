use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use shared_types::PeerId;

use crate::application::JoinPhase;
use crate::domain::{NetworkStatus, PeerRoster};

/// The one [`PeerRoster`] this process reasons about, plus the single bit the
/// roster deliberately cannot hold: whether a join is in flight.
///
/// # Why the join bit lives here and not in the domain
///
/// [`NetworkStatus::Joining`] describes an *operation*, not a state of the
/// roster — `NetworkStatus::from_connected_peers` cannot derive it, and says
/// so, because a count of zero sessions looks identical whether a ladder is
/// running or was never started. The application is the layer that knows an
/// operation is running, so the bit belongs to the application. The roster
/// stays a pure aggregate.
///
/// # Interior mutability, and one rule about it
///
/// A composition root drives this context from more than one task — a network
/// pump reporting inbound sessions, a UI redrawing the roster, a ticker
/// sweeping presence — so the cell must be `Sync`; `Mutex` is the std answer
/// and this crate takes no async runtime dependency. A poisoned lock is
/// recovered rather than propagated, so one failed assertion cannot turn every
/// later read into a panic.
///
/// **No caller may hold the roster lock across a call into a port.** The
/// bootstrap ladder calls discovery, cache, and transport ports while a join
/// phase is live, and any of those may legitimately ask this context for the
/// current status; a lock held across that boundary turns a redraw into a
/// deadlock. Every accessor below therefore takes the lock, runs a closure of
/// pure domain work, and releases it.
pub struct MembershipState {
    roster: Mutex<PeerRoster>,
    joining: AtomicBool,
}

impl MembershipState {
    /// An empty state belonging to `local`.
    pub fn for_local_peer(local: PeerId) -> Self {
        Self {
            roster: Mutex::new(PeerRoster::for_local_peer(local)),
            joining: AtomicBool::new(false),
        }
    }

    /// The peer this instance is.
    pub fn local_peer(&self) -> PeerId {
        self.lock().local_peer()
    }

    /// Reads the roster under the lock.
    ///
    /// `view` returns an owned value: nothing borrowed from the roster may
    /// outlive the guard.
    pub(crate) fn read<R>(&self, view: impl FnOnce(&PeerRoster) -> R) -> R {
        view(&self.lock())
    }

    /// Changes the roster under the lock.
    ///
    /// `change` is pure domain work and must not call a port — see the rule on
    /// this type. Handlers therefore return the events a transition produced
    /// and publish them after the lock is released.
    pub(crate) fn modify<R>(&self, change: impl FnOnce(&mut PeerRoster) -> R) -> R {
        change(&mut self.lock())
    }

    /// Marks a bootstrap ladder as in flight until the returned guard drops.
    ///
    /// The guard, rather than a matching `end_join`, is what makes AC3's "never
    /// a hang" hold for the *status line* as well as for the ladder: a handler
    /// that abandons the join with `?` still releases the phase, so the UI can
    /// never latch on `Joining` for a join that is no longer running.
    pub(crate) fn begin_join(&self) -> JoinPhase<'_> {
        self.joining.store(true, Ordering::Release);
        JoinPhase::over(self)
    }

    /// Ends the join phase. Called by [`JoinPhase`]'s `Drop`, never directly.
    pub(crate) fn end_join(&self) {
        self.joining.store(false, Ordering::Release);
    }

    /// How connected this instance currently is.
    ///
    /// `Joining` outranks the session count: a re-join over live sessions is
    /// still a join, and the in-flight operation is what the caller is waiting
    /// on. Once the phase ends the count decides — `Connected(n)` or, for zero,
    /// `Isolated`, which is a normal state and not a failure.
    pub fn network_status(&self) -> NetworkStatus {
        if self.joining.load(Ordering::Acquire) {
            return NetworkStatus::Joining;
        }

        NetworkStatus::from_connected_peers(self.lock().established_session_count())
    }

    fn lock(&self) -> MutexGuard<'_, PeerRoster> {
        self.roster.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
