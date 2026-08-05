//! Inbound and outbound port traits of the `messaging` context (canvas §4).
//!
//! Every trait is `Port`-suffixed, takes `&self`, and is object-safe, so a
//! composition root can hold one behind `Arc<dyn …>` and tests can substitute
//! an in-memory fake. Ports depend on `domain` and `shared_types` only — never
//! on `application` or any adapter type, and no `libp2p`, socket, or async
//! machinery appears in a signature.
//!
//! # The two directions
//!
//! **Outbound** (driven) ports are what this context calls:
//! [`MessageTransportPort`], [`MessageLogPort`], [`AuthorPolicyPort`],
//! [`SequenceCounterPort`], [`EnvelopeSignerPort`], [`EnvelopeVerifierPort`],
//! [`ClockPort`], [`EventPublisherPort`].
//!
//! **Inbound** (driving) ports are how this context is called:
//! [`SendMessagePort`] for composing, [`InboundEnvelopePort`] for everything
//! the network reports about messages, [`MessagingQueryPort`] for reads, and
//! [`PeerLifecyclePort`] for the peer-connected/disconnected news the
//! composition root fans in from `membership` (D10). Their arguments are
//! `domain` and `shared_types` types rather than the application's command
//! DTOs — a port may not name an application type, so the imperative commands
//! live in `application/commands/` and the services implementing these traits
//! build them from these arguments. The dependency keeps pointing inward.
//!
//! # Nothing here knows what an address is
//!
//! [`MessageTransportPort`] addresses peers by `PeerId` alone. There is no
//! endpoint, multiaddress, socket, or reachability concept in this module or
//! anywhere in this crate: how a peer is reached belongs to `membership`, and a
//! transport trait that learned about it would couple the two contexts through
//! their ports (canvas §4).
//!
//! # Traits another context also declares
//!
//! [`EnvelopeSignerPort`] and [`EnvelopeVerifierPort`] exist in `identity` too,
//! and these are deliberately **not** those. `shared_types` hosts no port
//! traits (canvas §2.4) and contexts never import each other (canvas §4), so
//! each context states its own need and the composition root wires both to one
//! underlying signer and one underlying verifier. The same reasoning makes
//! [`ClockPort`] a duplicate of `membership`'s, and gives this context its own
//! [`AuthorPolicyPort`] — one question about `identity`'s block list, asked in
//! this context's terms (invariant 11).
//!
//! [`SequenceCounterPort`] shares the keystore's *lifetime* but not its trait:
//! it is this context's counter, and D12 explains why it has to outlive the
//! process at all.
//!
//! # Types that are contracts, not ports
//!
//! [`UnsignedEnvelope`] and [`MessagePayload`] describe the seam where a
//! message meets the wire, and [`VerifiedAuthor`] describes what crossing that
//! seam inbound has established. They live here rather than in `domain` because
//! the domain has no idea anything is ever encoded or signed, and here rather
//! than in an adapter because they are what *every* adapter must agree on.

mod author_policy_port;
#[cfg(test)]
mod author_policy_port_test;
mod clock_port;
#[cfg(test)]
mod clock_port_test;
mod envelope_signer_port;
#[cfg(test)]
mod envelope_signer_port_test;
mod envelope_verifier_port;
#[cfg(test)]
mod envelope_verifier_port_test;
mod event_publisher_port;
#[cfg(test)]
mod event_publisher_port_test;
mod inbound_envelope_port;
mod inbound_verdict;
mod message_log_port;
#[cfg(test)]
mod message_log_port_test;
mod message_payload;
#[cfg(test)]
mod message_payload_test;
mod message_transport_port;
#[cfg(test)]
mod message_transport_port_test;
mod messaging_command_error;
#[cfg(test)]
mod messaging_command_error_test;
mod messaging_query_port;
mod peer_lifecycle_port;
#[cfg(test)]
pub(crate) mod port_fakes;
mod send_message_port;
mod send_outcome;
mod sequence_counter_port;
#[cfg(test)]
mod sequence_counter_port_test;
mod signature_verdict;
mod unsigned_envelope;
mod verified_author;
#[cfg(test)]
mod verified_author_test;

pub use author_policy_port::AuthorPolicyPort;
pub use clock_port::ClockPort;
pub use envelope_signer_port::{EnvelopeSignerError, EnvelopeSignerPort};
pub use envelope_verifier_port::{EnvelopeVerifierError, EnvelopeVerifierPort};
pub use event_publisher_port::{EventPublisherError, EventPublisherPort};
pub use inbound_envelope_port::InboundEnvelopePort;
pub use inbound_verdict::InboundVerdict;
pub use message_log_port::{MessageLogError, MessageLogPort};
pub use message_payload::{MessagePayload, MessagePayloadError};
pub use message_transport_port::{MessageTransportError, MessageTransportPort};
pub use messaging_command_error::MessagingCommandError;
pub use messaging_query_port::MessagingQueryPort;
pub use peer_lifecycle_port::PeerLifecyclePort;
pub use send_message_port::SendMessagePort;
pub use send_outcome::SendOutcome;
pub use sequence_counter_port::{SequenceCounterError, SequenceCounterPort};
pub use signature_verdict::SignatureVerdict;
pub use unsigned_envelope::UnsignedEnvelope;
pub use verified_author::VerifiedAuthor;
