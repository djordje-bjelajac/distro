use crate::domain::Millis;

/// The local peer deliberately left the network (canvas §2.2).
///
/// Emitted by the `LeaveNetwork` command (OP-6) — a local decision, not an
/// observation. Losing the last session to a network failure is *not* this
/// event: that is a `PeerDisconnected` and a return to `Isolated`, and
/// conflating the two would let the UI report a departure the user never
/// requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkLeft {
    pub at: Millis,
}
