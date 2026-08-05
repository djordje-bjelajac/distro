use shared_types::PeerId;

use crate::domain::MessageBody;
use crate::ports::{MessagingCommandError, SendOutcome};

/// The **inbound** (driving) contract for composing messages (canvas §4,
/// inbound column).
///
/// # Two methods, because there are two paths
///
/// The canvas requires the direct and broadcast paths be kept separate end to
/// end, and this trait is where that separation starts. They are not one method
/// with a destination argument, because they are not one operation: a direct
/// message is addressed, acknowledged, and can fail visibly (D4, D10, AC11); a
/// broadcast has no recipient, no acknowledgement, and no failed state at all
/// (D3, AC10). Folding them together would force one of those lifecycles to
/// pretend to be the other.
///
/// # Addressed by `PeerId` alone
///
/// There is no endpoint, address, or reachability argument here or anywhere in
/// this crate. How a peer is reached is `membership`'s business; this context
/// knows a peer by identity (canvas §4).
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn SendMessagePort + Send + Sync>`.
pub trait SendMessagePort {
    /// Composes a 1:1 message to `to` and hands it to the transport (D4).
    ///
    /// Returns `Ok` even when the transport refuses: the message exists, and
    /// the outcome carries the `Failed(reason)` the user must be shown (AC11).
    /// An `Err` means no message was composed at all.
    fn send_direct(
        &self,
        to: PeerId,
        body: MessageBody,
    ) -> Result<SendOutcome, MessagingCommandError>;

    /// Composes a message for the network-wide channel and releases it to the
    /// gossip topic (D3).
    ///
    /// Unlike a direct send, a transport refusal here *is* an `Err`: a
    /// broadcast has no failed delivery state, so a message the topic never
    /// accepted must not be left behind claiming it was published.
    fn publish_broadcast(&self, body: MessageBody) -> Result<SendOutcome, MessagingCommandError>;
}
