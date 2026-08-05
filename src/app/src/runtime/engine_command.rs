use membership::domain::JoinTicket;
use messaging::domain::MessageBody;
use shared_types::PeerId;

/// What the interface asks the engine to do.
///
/// # Why only these five
///
/// Everything a user can do falls into one of two kinds, and only one of them
/// needs a channel:
///
/// * **Local and immediate** — verify a peer, block one, mint a join ticket,
///   read a conversation, switch panes. These touch a small local file or a
///   map in memory and answer in microseconds, so the interface performs them
///   on its own thread and redraws. Routing them through a queue would add a
///   frame of latency and a second place for them to fail.
///
/// * **Blocking on the network** — the five below. Every one of them ends in a
///   synchronous call to `infra-net-libp2p`, which blocks for up to
///   `ResourceLimits::request_timeout` (ten seconds) if the driver is wedged.
///   Doing that on the interface thread would freeze the screen — including
///   the status line that is supposed to be saying what is going on.
///
/// A join is slower still: the D1 ladder walks up to three rungs and each dial
/// is bounded but not fast. The engine therefore runs a join on a thread of its
/// own rather than in its loop, so the network's events keep being drained
/// while the ladder is walked (AC3: a diagnostic, never a hang).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineCommand {
    /// Walk the D1 bootstrap ladder: cached peers, then the local network,
    /// then the ticket if one was supplied.
    Join(Box<Option<JoinTicket>>),
    /// Close every session, save the peer cache, announce the departure.
    Leave,
    /// Dial a peer the roster already knows.
    ConnectTo(PeerId),
    /// Compose and publish a message on the network-wide channel (D3).
    PublishBroadcast(MessageBody),
    /// Compose and send a 1:1 message (D4).
    SendDirect { to: PeerId, body: MessageBody },
    /// Stop the loop. The engine leaves the network first.
    Stop,
}
