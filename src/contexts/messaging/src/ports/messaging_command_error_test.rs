use crate::domain::events::{MessageRejected, RejectionReason};
use crate::domain::{ConversationError, ConversationId, MessagePlacement, SequenceNumber};
use crate::ports::{
    EnvelopeSignerError, EnvelopeVerifierError, EventPublisherError, InboundVerdict,
    MessageLogError, MessageTransportError, MessagingCommandError, SequenceCounterError,
};
use crate::test_peers;

#[test]
fn every_collaborator_failure_converts_into_the_command_error() {
    assert_eq!(
        MessagingCommandError::from(ConversationError::UnknownMessage),
        MessagingCommandError::Conversation(ConversationError::UnknownMessage)
    );
    assert_eq!(
        MessagingCommandError::from(SequenceCounterError::NotPersisted),
        MessagingCommandError::Sequence(SequenceCounterError::NotPersisted)
    );
    assert_eq!(
        MessagingCommandError::from(EnvelopeSignerError::KeyUnavailable),
        MessagingCommandError::Signer(EnvelopeSignerError::KeyUnavailable)
    );
    assert_eq!(
        MessagingCommandError::from(EnvelopeVerifierError::VerifierUnavailable),
        MessagingCommandError::Verifier(EnvelopeVerifierError::VerifierUnavailable)
    );
    assert_eq!(
        MessagingCommandError::from(MessageTransportError::NoRelayAvailable),
        MessagingCommandError::Transport(MessageTransportError::NoRelayAvailable)
    );
    assert_eq!(
        MessagingCommandError::from(MessageLogError::CapacityExhausted),
        MessagingCommandError::Log(MessageLogError::CapacityExhausted)
    );
    assert_eq!(
        MessagingCommandError::from(EventPublisherError::Unavailable),
        MessagingCommandError::Publisher(EventPublisherError::Unavailable)
    );
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        MessagingCommandError::Transport(MessageTransportError::PeerUnreachable).to_string(),
        "the recipient could not be reached"
    );
    assert_eq!(
        MessagingCommandError::SequenceDiverged {
            issued: SequenceNumber::new(9).expect("valid"),
            recorded: SequenceNumber::FIRST,
        }
        .to_string(),
        "the counter issued sequence #9 but the conversation would record #1"
    );
}

fn rejected(reason: RejectionReason) -> MessageRejected {
    MessageRejected {
        conversation: ConversationId::Broadcast,
        claimed_author: test_peers::bob(),
        sequence: None,
        reason,
    }
}

#[test]
fn a_boundary_refusal_reports_its_reason_and_counts_as_refused() {
    let verdict = InboundVerdict::RefusedAtBoundary(rejected(RejectionReason::SignatureInvalid));

    assert!(verdict.is_refused());
    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::SignatureInvalid)
    );
    assert!(!verdict.is_applied());
    assert!(!verdict.is_duplicate());
}

#[test]
fn a_conversation_rejection_reports_its_reason_too() {
    // AC15's inbound mirror: a message that arrived after its gap closed is
    // *reported*, and a caller counting diagnostics must see it exactly as it
    // sees a boundary refusal.
    let verdict = InboundVerdict::Judged(MessagePlacement::Rejected(rejected(
        RejectionReason::ArrivedAfterGapClosed,
    )));

    assert!(verdict.is_refused());
    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::ArrivedAfterGapClosed)
    );
}

#[test]
fn a_tolerated_payload_kind_is_neither_refused_nor_applied() {
    // S2/AC14: an unknown kind is counted, not treated as an error.
    let verdict = InboundVerdict::Ignored(shared_types::PayloadKind::Unknown(77));

    assert!(!verdict.is_refused());
    assert!(!verdict.is_applied());
    assert_eq!(verdict.rejection_reason(), None);
}
