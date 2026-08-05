use std::sync::Arc;

use identity::domain::{TrustRecord, VerificationState};
use identity::ports::{IdentityKeyStorePort, TrustRecordStorePort};
use membership::ports::PeerCachePort;
use messaging::domain::ConversationId;
use messaging::ports::{MessageLogPort, SequenceCounterPort};

use crate::stores::{
    FileIdentityKeyStore, FilePeerCache, FileSequenceCounter, FileTrustRecordStore,
};
use crate::test_dir::TestDir;
use crate::test_peers::alice;
use crate::{LocalStores, LocalStoresError};

#[test]
fn open_creates_the_directory() {
    let dir = TestDir::new("stores-open");
    let root = dir.file("profile").join("distro");

    let stores = LocalStores::open(&root).expect("the directory must be created");

    assert!(root.is_dir());
    assert_eq!(stores.root(), root);
}

#[test]
fn opening_touches_no_file() {
    let dir = TestDir::new("stores-lazy");
    let stores = LocalStores::open(dir.path()).expect("the directory must open");

    // First launch and a wiped directory must behave identically (AC1), so
    // opening reads nothing and writes nothing.
    let entries = std::fs::read_dir(stores.root())
        .expect("the directory exists")
        .count();

    assert_eq!(entries, 0);
}

#[test]
fn every_store_lands_in_the_same_directory() {
    let dir = TestDir::new("stores-layout");
    let stores = LocalStores::open(dir.path()).expect("the directory must open");

    stores
        .identity_keys()
        .load_or_create_local_peer()
        .expect("an identity must be created");
    stores
        .trust_records()
        .save_trust_record(&TrustRecord::unverified(alice()))
        .expect("the save must land");
    stores.peer_cache().save(&[]).expect("the save must land");
    stores
        .sequence_counter()
        .issue_next(ConversationId::Broadcast)
        .expect("the counter must be healthy");

    for name in [
        FileIdentityKeyStore::FILE_NAME,
        FileTrustRecordStore::FILE_NAME,
        FilePeerCache::FILE_NAME,
        FileSequenceCounter::FILE_NAME,
    ] {
        assert!(
            dir.file(name).is_file(),
            "{name} must live in the store directory"
        );
    }
}

#[test]
fn a_second_open_finds_what_the_first_left() {
    let dir = TestDir::new("stores-restart");

    let first = LocalStores::open(dir.path()).expect("the directory must open");
    let peer = first
        .identity_keys()
        .load_or_create_local_peer()
        .expect("an identity must be created");
    first
        .trust_records()
        .save_trust_record(&TrustRecord::rehydrate(
            alice(),
            VerificationState::Verified,
            true,
        ))
        .expect("the save must land");
    first
        .sequence_counter()
        .issue_next(ConversationId::Broadcast)
        .expect("the counter must be healthy");
    drop(first);

    let second = LocalStores::open(dir.path()).expect("the directory must open");

    assert_eq!(
        second.identity_keys().load_or_create_local_peer(),
        Ok(peer),
        "AC9"
    );
    assert_eq!(
        second.trust_records().list_blocked_peers(),
        Ok(vec![alice()])
    );
    assert_eq!(
        second
            .sequence_counter()
            .issue_next(ConversationId::Broadcast)
            .map(|number| number.as_u64()),
        Ok(2),
        "AC16: the counter shares the keypair's directory and its lifetime"
    );
    // …while history did not survive, which is D7 and the reason the counter
    // has to.
    assert!(second.message_log().is_empty());
}

#[test]
fn every_store_coerces_to_its_port_object() {
    let dir = TestDir::new("stores-ports");
    let stores = LocalStores::open(dir.path()).expect("the directory must open");

    // What the composition root does with them (OP-12).
    let keys: Arc<dyn IdentityKeyStorePort + Send + Sync> = stores.identity_keys();
    let trust: Arc<dyn TrustRecordStorePort + Send + Sync> = stores.trust_records();
    let cache: Arc<dyn PeerCachePort + Send + Sync> = stores.peer_cache();
    let counter: Arc<dyn SequenceCounterPort + Send + Sync> = stores.sequence_counter();
    let log: Arc<dyn MessageLogPort + Send + Sync> = stores.message_log();

    assert!(keys.load_or_create_local_peer().is_ok());
    assert_eq!(trust.list_blocked_peers(), Ok(Vec::new()));
    assert_eq!(cache.load(), Ok(Vec::new()));
    assert!(counter.issue_next(ConversationId::Broadcast).is_ok());
    assert_eq!(log.conversations(), Ok(Vec::new()));
}

#[test]
fn a_clone_shares_the_same_stores() {
    let dir = TestDir::new("stores-clone");
    let stores = LocalStores::open(dir.path()).expect("the directory must open");
    let clone = stores.clone();

    stores
        .message_log()
        .append(&messaging::domain::Message::received(
            messaging::domain::MessageId::new(
                alice(),
                ConversationId::Broadcast,
                messaging::domain::SequenceNumber::FIRST,
            ),
            messaging::domain::MessageBody::new("hello").expect("a valid fixture body"),
            messaging::domain::Millis::ZERO,
        ))
        .expect("the append must land");

    // The in-memory log is shared rather than copied, or two contexts wired
    // from one root would see two different histories.
    assert_eq!(clone.message_log().len(), 1);
}

#[test]
fn a_root_that_cannot_be_created_is_reported() {
    let dir = TestDir::new("stores-blocked-root");
    let path = dir.file("a-file");
    std::fs::write(&path, b"in the way").expect("the plant must land");

    assert_eq!(
        LocalStores::open(path.join("distro")).err(),
        Some(LocalStoresError::DirectoryUnavailable)
    );
}

#[cfg(unix)]
#[test]
fn the_store_directory_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new("stores-mode");
    let root = dir.file("distro");

    LocalStores::open(&root).expect("the directory must be created");

    let mode = std::fs::metadata(&root)
        .expect("the directory exists")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(mode, 0o700);
}
