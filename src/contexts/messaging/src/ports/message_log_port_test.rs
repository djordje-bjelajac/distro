use crate::domain::{ConversationId, Message, MessageBody, MessageId, Millis, SequenceNumber};
use crate::ports::port_fakes::{InMemoryMessageLog, UnavailableMessageLog};
use crate::ports::{MessageLogError, MessageLogPort};
use crate::test_peers;

const SENT_AT: Millis = Millis::from_millis(5);

fn message(conversation: ConversationId, sequence: u64, text: &str) -> Message {
    Message::received(
        MessageId::new(
            test_peers::bob(),
            conversation,
            SequenceNumber::new(sequence).expect("positive"),
        ),
        MessageBody::new(text).expect("valid body"),
        SENT_AT,
    )
}

#[test]
fn the_port_is_object_safe_so_one_log_can_be_shared() {
    let log = InMemoryMessageLog::default();
    let port: &dyn MessageLogPort = &log;

    assert!(port.load(ConversationId::Broadcast).is_ok());
}

#[test]
fn an_unknown_conversation_loads_as_empty_rather_than_failing() {
    // A conversation nobody has spoken in yet is not an error; it is a
    // conversation with nothing in it.
    let log = InMemoryMessageLog::default();

    assert_eq!(log.load(ConversationId::Broadcast), Ok(Vec::new()));
    assert_eq!(log.conversations(), Ok(Vec::new()));
}

#[test]
fn appended_messages_load_back_in_append_order() {
    let log = InMemoryMessageLog::default();
    let first = message(ConversationId::Broadcast, 1, "first");
    let second = message(ConversationId::Broadcast, 2, "second");

    log.append(&first).expect("room");
    log.append(&second).expect("room");

    assert_eq!(log.load(ConversationId::Broadcast), Ok(vec![first, second]));
}

#[test]
fn each_conversation_is_stored_separately() {
    let log = InMemoryMessageLog::default();
    let direct = ConversationId::Direct(test_peers::bob());
    log.append(&message(ConversationId::Broadcast, 1, "to everyone"))
        .expect("room");
    log.append(&message(direct, 1, "just to you"))
        .expect("room");

    assert_eq!(log.load(ConversationId::Broadcast).map(|m| m.len()), Ok(1));
    assert_eq!(log.load(direct).map(|m| m.len()), Ok(1));
    assert_eq!(
        log.conversations(),
        Ok(vec![ConversationId::Broadcast, direct]),
        "listed in a deterministic order"
    );
}

#[test]
fn an_unavailable_log_reports_a_typed_error_on_every_operation() {
    let log = UnavailableMessageLog;

    assert_eq!(
        log.append(&message(ConversationId::Broadcast, 1, "x")),
        Err(MessageLogError::Unavailable)
    );
    assert_eq!(
        log.load(ConversationId::Broadcast),
        Err(MessageLogError::Unavailable)
    );
    assert_eq!(log.conversations(), Err(MessageLogError::Unavailable));
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        MessageLogError::Unavailable.to_string(),
        "the message log is not available"
    );
    assert_eq!(
        MessageLogError::CapacityExhausted.to_string(),
        "the message log has no room for more messages"
    );
}
