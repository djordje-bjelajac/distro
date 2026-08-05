use std::sync::Arc;

use crate::application::test_context::{
    TestContext, TestContextBuilder, broadcast_from, direct_from,
};
use crate::domain::{ConversationId, Millis};
use crate::ports::port_fakes::{FailingTransport, UnavailableMessageLog};
use crate::ports::{
    MessageLogError, MessageLogPort, MessageTransportError, MessageTransportPort,
    MessagingQueryPort,
};
use crate::test_peers;

const CLAIMED_AT: Millis = Millis::from_millis(7);

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice()).build()
}

fn listed(context: &TestContext) -> Vec<ConversationId> {
    context
        .context
        .queries()
        .conversations()
        .expect("the log answers")
}

#[test]
fn an_instance_that_has_said_and_heard_nothing_lists_nothing() {
    assert_eq!(listed(&alice()), Vec::new());
}

#[test]
fn a_conversation_appears_once_something_has_been_said_in_it() {
    let context = alice();

    context
        .send_direct(test_peers::bob(), "hello")
        .expect("sent");

    assert_eq!(
        listed(&context),
        vec![ConversationId::Direct(test_peers::bob())]
    );
}

#[test]
fn a_conversation_appears_once_something_has_been_heard_in_it() {
    let context = alice();

    context
        .accept(direct_from(test_peers::carol(), 1, "hello", CLAIMED_AT))
        .expect("applied");

    assert_eq!(
        listed(&context),
        vec![ConversationId::Direct(test_peers::carol())]
    );
}

#[test]
fn the_listing_is_deterministic_broadcast_first_then_peers_by_identity() {
    // S5/AC13: a listing whose order depended on insertion history would make
    // two runs of the same scenario differ.
    let context = alice();

    context
        .send_direct(test_peers::carol(), "to carol")
        .expect("sent");
    context
        .send_direct(test_peers::bob(), "to bob")
        .expect("sent");
    context.publish_broadcast("to all").expect("published");

    let mut expected = vec![
        ConversationId::Broadcast,
        ConversationId::Direct(test_peers::bob()),
        ConversationId::Direct(test_peers::carol()),
    ];
    expected.sort_unstable();

    assert_eq!(listed(&context), expected);
    assert_eq!(listed(&context)[0], ConversationId::Broadcast);
}

#[test]
fn a_conversation_with_only_a_buffered_arrival_is_not_listed() {
    // Nothing has been said in it that this peer can show, so it is not a
    // conversation yet (invariant 5).
    let context = alice();

    context
        .accept(broadcast_from(test_peers::bob(), 9, "early", CLAIMED_AT))
        .expect("buffered");

    assert_eq!(listed(&context), Vec::new());
}

#[test]
fn a_refused_broadcast_leaves_no_conversation_behind() {
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(
            Arc::new(FailingTransport(MessageTransportError::Unavailable))
                as Arc<dyn MessageTransportPort + Send + Sync>,
        )
        .build();

    let _ = context.publish_broadcast("never left");

    assert_eq!(listed(&context), Vec::new());
}

#[test]
fn a_log_that_cannot_be_reached_reports_its_error_rather_than_an_empty_list() {
    // An empty list and an unreachable log mean different things to a user
    // interface, and collapsing them would show "no conversations" for a
    // machine that simply cannot read its own history.
    struct Fixture;
    impl Fixture {
        fn build() -> crate::application::queries::ListConversationsHandler {
            crate::application::queries::ListConversationsHandler::new(Arc::new(
                UnavailableMessageLog,
            )
                as Arc<dyn MessageLogPort + Send + Sync>)
        }
    }

    let outcome = Fixture::build().handle(crate::application::queries::ListConversations);

    assert_eq!(outcome, Err(MessageLogError::Unavailable));
}
