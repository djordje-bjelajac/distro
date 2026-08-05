use std::sync::Arc;

use shared_types::Fingerprint;

use crate::application::IdentityContext;
use crate::domain::{DisplayName, VerificationState};
use crate::ports::port_fakes::{FakeKeyStore, InMemoryTrustRecordStore};
use crate::ports::{
    IdentityCommandPort, IdentityKeyStorePort, IdentityQueryPort, LocalIdentitySummary,
    PeerTrustState, TrustRecordStorePort,
};
use crate::test_peers;

fn context_over(
    key_store: &Arc<FakeKeyStore>,
    trust_records: &Arc<InMemoryTrustRecordStore>,
) -> IdentityContext {
    IdentityContext::new(
        Arc::clone(key_store) as Arc<dyn IdentityKeyStorePort + Send + Sync>,
        Arc::clone(trust_records) as Arc<dyn TrustRecordStorePort + Send + Sync>,
    )
}

fn fresh_context() -> (
    Arc<FakeKeyStore>,
    Arc<InMemoryTrustRecordStore>,
    IdentityContext,
) {
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));
    let trust_records = Arc::new(InMemoryTrustRecordStore::empty());
    let context = context_over(&key_store, &trust_records);
    (key_store, trust_records, context)
}

fn name(raw: &str) -> DisplayName {
    DisplayName::new(raw).expect("test fixture must be a valid display name")
}

#[test]
fn the_queries_return_what_the_commands_wrote() {
    let (_, _, context) = fresh_context();
    let commands: &dyn IdentityCommandPort = context.commands();
    let queries: &dyn IdentityQueryPort = context.queries();

    commands
        .initialize_local_identity(None)
        .expect("first launch (AC1)");
    commands.set_display_name("Ada").expect("rename");
    commands.verify_peer(test_peers::bob()).expect("verify");
    commands.block_peer(test_peers::carol()).expect("block");

    assert_eq!(
        queries.local_identity(),
        Some(LocalIdentitySummary {
            peer: test_peers::alice(),
            display_name: name("Ada"),
            fingerprint: Fingerprint::of(&test_peers::alice()),
        })
    );
    assert_eq!(
        queries.peer_trust_state(test_peers::bob()).expect("read"),
        PeerTrustState {
            peer: test_peers::bob(),
            verification: VerificationState::Verified,
            blocked: false,
            fingerprint: Fingerprint::of(&test_peers::bob()),
        }
    );
    assert_eq!(
        queries.blocked_peers().expect("read"),
        vec![test_peers::carol()]
    );
}

#[test]
fn unblocking_removes_the_peer_from_the_list_the_root_hands_to_messaging() {
    let (_, _, context) = fresh_context();
    let commands: &dyn IdentityCommandPort = context.commands();
    let queries: &dyn IdentityQueryPort = context.queries();

    commands.block_peer(test_peers::bob()).expect("block");
    commands.block_peer(test_peers::carol()).expect("block");
    commands.unblock_peer(test_peers::bob()).expect("unblock");

    assert_eq!(
        queries.blocked_peers().expect("read"),
        vec![test_peers::carol()],
        "invariant 11's block list is read through this port, never imported by messaging"
    );
}

#[test]
fn the_command_and_query_sides_share_one_local_identity() {
    let (_, _, context) = fresh_context();

    context
        .commands()
        .initialize_local_identity(Some(name("Ada")))
        .expect("initialize");
    let before = context.queries().local_identity().expect("assumed");
    context
        .commands()
        .set_display_name("Grace")
        .expect("rename");
    let after = context.queries().local_identity().expect("assumed");

    assert_eq!(before.display_name, name("Ada"));
    assert_eq!(after.display_name, name("Grace"));
    assert_eq!(
        before.peer, after.peer,
        "renaming never touches identity (invariant 8)"
    );
}

#[test]
fn queries_before_any_command_report_absence_rather_than_failing() {
    let (_, trust_records, context) = fresh_context();
    let queries: &dyn IdentityQueryPort = context.queries();

    assert_eq!(queries.local_identity(), None);
    assert_eq!(queries.blocked_peers().expect("read"), Vec::new());
    assert_eq!(
        queries
            .peer_trust_state(test_peers::bob())
            .expect("read")
            .verification,
        VerificationState::Unverified
    );
    assert_eq!(trust_records.saves(), 0, "no query wrote anything");
}

#[test]
fn the_query_side_writes_nothing_however_often_it_is_driven() {
    let (key_store, trust_records, context) = fresh_context();

    context
        .commands()
        .initialize_local_identity(None)
        .expect("initialize");
    context
        .commands()
        .block_peer(test_peers::bob())
        .expect("block");
    let writes_after_commands = trust_records.saves();

    for _ in 0..5 {
        context.queries().local_identity();
        context
            .queries()
            .peer_trust_state(test_peers::bob())
            .expect("read");
        context.queries().blocked_peers().expect("read");
    }

    assert_eq!(
        trust_records.saves(),
        writes_after_commands,
        "query handlers only read (AGENTS.md: command and query paths stay separate)"
    );
    assert_eq!(key_store.loads(), 1, "queries never reach for the keypair");
}

#[test]
fn a_restarted_context_over_the_same_stores_sees_the_same_identity_and_trust() {
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));
    let trust_records = Arc::new(InMemoryTrustRecordStore::empty());

    let first_run = context_over(&key_store, &trust_records);
    first_run
        .commands()
        .initialize_local_identity(None)
        .expect("first launch");
    first_run
        .commands()
        .verify_peer(test_peers::bob())
        .expect("verify");
    first_run
        .commands()
        .block_peer(test_peers::carol())
        .expect("block");
    let before = first_run.queries().local_identity().expect("assumed");

    let second_run = context_over(&key_store, &trust_records);
    second_run
        .commands()
        .initialize_local_identity(None)
        .expect("later launch");
    let after = second_run.queries().local_identity().expect("assumed");

    assert_eq!(
        before.peer, after.peer,
        "PeerId is stable across restarts (AC9)"
    );
    assert_eq!(key_store.creations(), 1);
    assert!(
        second_run
            .queries()
            .peer_trust_state(test_peers::bob())
            .expect("read")
            .verification
            .is_verified(),
        "a fingerprint comparison the user performed once is not repeated"
    );
    assert_eq!(
        second_run.queries().blocked_peers().expect("read"),
        vec![test_peers::carol()]
    );
}
