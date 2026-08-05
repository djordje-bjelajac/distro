use std::sync::Arc;

use crate::application::{ConversationRegistry, MessageRecorder};
use crate::domain::events::MessageDeliveryStateChanged;
use crate::domain::{ConversationError, DeliveryFailure, MessageId};
use crate::ports::MessagingCommandError;

/// Record that one 1:1 message will not arrive, and why (D10, AC11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkMessageFailed {
    pub id: MessageId,
    /// What the transport reported. Stated by the layer that observed it and
    /// recorded by the aggregate; nothing in between reinterprets it.
    pub reason: DeliveryFailure,
}

/// Handles [`MarkMessageFailed`]: `pending → failed(reason)`, announced.
///
/// # The ending `send_direct` cannot report
///
/// [`SendDirectMessage`](crate::application::commands::SendDirectMessage)
/// already fails a message its transport refuses — but only when the refusal is
/// *synchronous*. A real transport answers `Ok` as soon as it has queued the
/// request, and the refusal or timeout comes back afterwards as a network
/// event. At that moment the send has long returned, and while the session is
/// still up nothing else in this context can move that one message: an
/// acknowledgement runs the other way, and
/// [`peer_disconnected`](crate::ports::PeerLifecyclePort::peer_disconnected)
/// fails *every* pending direct to the peer, which is wrong for one refused
/// message and unavailable while the link is healthy. Without this command the
/// message stays `Pending` for the life of the session — silent loss wearing a
/// spinner, which is exactly what AC11 and D10 forbid.
///
/// # One message, named by identifier
///
/// The counterpart to
/// [`FailPendingDirects`](crate::application::commands::FailPendingDirects),
/// and deliberately not a special case of it. A disconnect is news about a
/// link, so the aggregate decides which messages it costs; a refusal is news
/// about one message, so the caller names it and the aggregate decides only
/// whether that move is legal.
///
/// # Why an unknown message is an error rather than a shrug
///
/// A refusal naming a message this peer does not hold means the transport and
/// the conversation disagree about what was sent — a correlation bug, a replayed
/// report, or a peer answering for traffic it never carried. Swallowing it would
/// hide all three. The conversation is not opened to find out: a stray report
/// must not be able to populate the conversation list, so a conversation this
/// process has never touched simply has no such message.
///
/// # Terminal states stay terminal
///
/// A message already delivered, or already failed, is refused by the aggregate
/// rather than silently overwritten — so a late or repeated refusal can neither
/// overturn an acknowledgement the user has seen nor announce the same failure
/// twice. Broadcast messages are `Published` and refuse this too: gossip has no
/// recipient set, so it has no delivery to lose (D3). Every one of those is the
/// domain's ruling, surfaced as
/// [`ConversationError`](crate::domain::ConversationError) rather than
/// re-decided here.
#[derive(Clone)]
pub struct MarkMessageFailedHandler {
    registry: Arc<ConversationRegistry>,
    recorder: MessageRecorder,
}

impl MarkMessageFailedHandler {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        recorder: MessageRecorder,
    ) -> Self {
        Self { registry, recorder }
    }

    pub fn handle(
        &self,
        command: MarkMessageFailed,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        let change = self
            .registry
            .modify_open(command.id.conversation(), |open| {
                open.mark_failed(&command.id, command.reason)
            })
            .ok_or(ConversationError::UnknownMessage)??;

        self.recorder.announce(change)?;

        Ok(change)
    }
}
