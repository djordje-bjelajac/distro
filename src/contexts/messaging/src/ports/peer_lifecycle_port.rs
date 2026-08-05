use shared_types::{PeerConnected, PeerDisconnected};

use crate::domain::events::MessageDeliveryStateChanged;
use crate::ports::MessagingCommandError;

/// The **inbound** (driving) contract through which peer lifecycle news
/// reaches this context (canvas §4, inbound column; D10).
///
/// # The one seam between two contexts, and its shape
///
/// `membership` owns sessions; `messaging` owns messages. Neither imports the
/// other (canvas §4), so the composition root subscribes to `membership`'s
/// `PeerConnected` / `PeerDisconnected` — published through `shared_types`,
/// carrying a `PeerId` and nothing else — and fans them into this port. That is
/// the whole of the coupling: this context never learns what an `Endpoint`, a
/// session, or a reachability class is.
///
/// # Why a disconnect is not just a notification
///
/// D10 gives delivery a bounded, visible lifecycle. A direct message handed to
/// a transport whose session has died will not arrive, and AC11 makes silent
/// loss a non-state — so the disconnect is what turns those pending messages
/// into stated failures the user can act on. Without a caller on this port they
/// stay `Pending` forever, which is silent loss wearing a spinner.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn PeerLifecyclePort + Send + Sync>`.
pub trait PeerLifecyclePort {
    /// A session with `event.peer` became established.
    ///
    /// Opens this peer's direct conversation if it is not open yet, rehydrating
    /// the local outbound sequence from the counter (D12, AC16) so the first
    /// message sent after a restart continues the run its listeners already
    /// hold instead of re-using numbers they would classify as duplicates.
    ///
    /// Doing it on connect rather than on first send means the counter read —
    /// which may touch a store — happens while the user is not waiting on it.
    fn peer_connected(&self, event: PeerConnected) -> Result<(), MessagingCommandError>;

    /// The session with `event.peer` ended.
    ///
    /// Fails every 1:1 message to that peer still awaiting acknowledgement,
    /// with [`DeliveryFailure::SessionClosed`](crate::domain::DeliveryFailure::SessionClosed),
    /// and reports each transition (D10, AC11). Broadcast messages are
    /// untouched — they are `Published`, and gossip has no session to lose.
    ///
    /// Messages to *other* peers are untouched too: a disconnect is news about
    /// one link, not about the network.
    fn peer_disconnected(
        &self,
        event: PeerDisconnected,
    ) -> Result<Vec<MessageDeliveryStateChanged>, MessagingCommandError>;
}
