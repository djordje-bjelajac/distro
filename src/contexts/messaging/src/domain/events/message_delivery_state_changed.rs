use crate::domain::{DeliveryState, MessageId};

/// A message moved to a new delivery state (AC11, D10).
///
/// Both ends are carried so a consumer never has to remember the previous one,
/// and so a diagnostic can show the whole transition. Since every terminal
/// state is terminal, at most one of these is ever raised per message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageDeliveryStateChanged {
    pub id: MessageId,
    pub from: DeliveryState,
    pub to: DeliveryState,
}
