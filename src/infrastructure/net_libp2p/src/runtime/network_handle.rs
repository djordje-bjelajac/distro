use std::sync::mpsc::sync_channel;
use std::time::Duration;

use shared_types::PeerId;
use tokio::sync::mpsc::UnboundedSender;

use crate::codec::{CodecDiagnostics, EnvelopeCodec};
use crate::swarm::network_command::{NetworkCommand, Reply};

/// The synchronous face of the swarm.
///
/// # The seam, stated plainly
///
/// Every port method in this crate does the same three things: build a command,
/// hand it to the driver over an **unbounded** `tokio` channel (so the send
/// never blocks and never awaits), and block on a **bounded** `std` reply
/// channel with a timeout. No `async fn`, no runtime handle, no `block_on`.
///
/// ## Which thread may call this
///
/// Any thread that is **not** the driver task. In practice that means: the
/// composition root's own thread, or any thread it spawns. The driver runs on
/// the runtime this crate owns and never calls back into a port, so the one
/// deadlock this design could have — the driver blocking on itself — is
/// structurally impossible rather than merely avoided.
///
/// Calling a port method from inside an `async` task *works* (unlike
/// `tokio::sync::oneshot::blocking_recv`, `std`'s `recv_timeout` does not
/// panic in a runtime context) but it parks a worker thread for the duration.
/// A root that wants that should use `spawn_blocking`. This is a documented
/// property, not a trap: the reply always arrives or times out.
///
/// ## What a timeout means
///
/// `Err` with the caller's chosen "unavailable" variant, never a hang (AC3).
/// A driver that has stopped, a swarm wedged on something, or a reply that was
/// simply slower than [`ResourceLimits::request_timeout`] all surface as the
/// same typed refusal, which the layer above turns into a delivery state or a
/// join diagnostic a user can read.
///
/// Cheap to clone: all three port adapters hold one, and they share the same
/// channel and the same counters.
///
/// [`ResourceLimits::request_timeout`]: crate::limits::ResourceLimits::request_timeout
#[derive(Clone)]
pub struct NetworkHandle {
    commands: UnboundedSender<NetworkCommand>,
    timeout: Duration,
    local: PeerId,
    codec: EnvelopeCodec,
    diagnostics: CodecDiagnostics,
}

impl NetworkHandle {
    pub(crate) fn new(
        commands: UnboundedSender<NetworkCommand>,
        timeout: Duration,
        local: PeerId,
        codec: EnvelopeCodec,
        diagnostics: CodecDiagnostics,
    ) -> Self {
        Self {
            commands,
            timeout,
            local,
            codec,
            diagnostics,
        }
    }

    /// This peer's identity.
    pub const fn local_peer(&self) -> PeerId {
        self.local
    }

    /// The counters every tolerated oddity and every refusal is recorded in
    /// (S2, S6).
    pub fn diagnostics(&self) -> &CodecDiagnostics {
        &self.diagnostics
    }

    pub(crate) fn codec(&self) -> &EnvelopeCodec {
        &self.codec
    }

    /// Sends one command and blocks for its answer.
    ///
    /// `on_unavailable` is returned when the driver is gone or does not answer
    /// in time. Each caller picks the variant that is honest for *its* port:
    /// a dial that never answered is `NoReachableEndpoint`, a ticket that never
    /// answered is `TicketUnreachable`, and so on — a single generic "the
    /// adapter failed" would erase the distinction the join diagnostic and the
    /// delivery state are built from.
    pub(crate) fn request<T, E: Copy>(
        &self,
        build: impl FnOnce(Reply<T, E>) -> NetworkCommand,
        on_unavailable: E,
    ) -> Result<T, E> {
        // Capacity one: exactly one answer is ever sent, and the driver must
        // never block handing it over — a caller that timed out and walked away
        // leaves a full channel behind, which `try_send` shrugs off.
        let (sender, receiver) = sync_channel(1);

        if self.commands.send(build(sender)).is_err() {
            return Err(on_unavailable);
        }

        receiver
            .recv_timeout(self.timeout)
            .unwrap_or(Err(on_unavailable))
    }

    /// Asks the driver to stop. Idempotent; a driver that has already stopped
    /// simply drops the message.
    pub(crate) fn shutdown(&self) {
        let _ = self.commands.send(NetworkCommand::Shutdown);
    }
}

impl std::fmt::Debug for NetworkHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkHandle")
            .field("local", &self.local)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}
