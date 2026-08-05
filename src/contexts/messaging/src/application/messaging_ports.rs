use std::sync::Arc;

use crate::ports::{
    AuthorPolicyPort, ClockPort, EnvelopeSignerPort, EnvelopeVerifierPort, EventPublisherPort,
    MessageLogPort, MessageTransportPort, SequenceCounterPort,
};

/// The eight outbound ports one `messaging` context runs on (canvas §4).
///
/// Named fields rather than eight positional arguments: a composition root
/// wiring this by position has to keep two signers, two verifiers, and two
/// clocks straight across three contexts, and the compiler only catches a
/// transposition when the two traits happen to differ. This is also the list
/// OP-12 must satisfy, in one place, which is easier to check against the
/// canvas than a constructor signature.
///
/// # Two of these are cross-context wirings, and neither is an import
///
/// [`verifier`](Self::verifier) and [`signer`](Self::signer) are wired to the
/// same underlying implementation `identity` uses, and [`policy`](Self::policy)
/// to `identity`'s block list — each through *this* context's own trait, stated
/// in this context's terms. `shared_types` hosts no port traits and contexts
/// never import each other (canvas §2.4, §4), so the root is the only place the
/// two meet.
///
/// # Nothing here knows what an address is
///
/// [`transport`](Self::transport) addresses peers by `PeerId` alone. How a peer
/// is reached belongs entirely to `membership`.
pub struct MessagingPorts {
    /// The one source of time (D11, S5): the instant stamped on outbound
    /// messages, and the arrival instant that ages a gap.
    pub clock: Arc<dyn ClockPort + Send + Sync>,
    /// The local peer's outbound sequence counter. Its contract is the
    /// keypair's lifetime, exactly (D12, AC16).
    pub counter: Arc<dyn SequenceCounterPort + Send + Sync>,
    /// Produces this peer's signature over envelopes it sends — the outbound
    /// half of invariant 4.
    pub signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
    /// Checks an inbound envelope against its claimed author — the inbound half
    /// of invariant 4, and the only thing that establishes an author at all.
    pub verifier: Arc<dyn EnvelopeVerifierPort + Send + Sync>,
    /// Whether this peer refuses another's content (invariant 11).
    pub policy: Arc<dyn AuthorPolicyPort + Send + Sync>,
    /// How a signed envelope leaves this peer, by `PeerId` (D3, D4).
    pub transport: Arc<dyn MessageTransportPort + Send + Sync>,
    /// Where applied messages are mirrored (D7).
    pub log: Arc<dyn MessageLogPort + Send + Sync>,
    /// Where this context's events go.
    pub publisher: Arc<dyn EventPublisherPort + Send + Sync>,
}
