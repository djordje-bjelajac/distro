use std::fs;

use identity::ports::{IdentityKeyStoreError, IdentityKeyStorePort};

use crate::format::hex_bytes;
use crate::stores::FileIdentityKeyStore;
use crate::test_dir::TestDir;
use crate::test_peers::{ALICE_SECRET_KEY, alice};

fn plant(dir: &TestDir, contents: &str) -> std::path::PathBuf {
    let path = dir.file(FileIdentityKeyStore::FILE_NAME);
    fs::write(&path, contents).expect("the plant must land");
    path
}

#[test]
fn first_launch_creates_an_identity_with_nobody_asked_anything() {
    let dir = TestDir::new("keystore-first-launch");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    // AC1: no config, no args, no prior state, no prompt.
    let peer = store
        .load_or_create_local_peer()
        .expect("a fresh directory must yield an identity");

    assert!(store.path().exists(), "the keypair must be persisted");
    assert_eq!(store.load_or_create_local_peer(), Ok(peer));
}

#[test]
fn a_restart_returns_the_same_peer_id() {
    let dir = TestDir::new("keystore-restart");
    let path = dir.file(FileIdentityKeyStore::FILE_NAME);

    let first = FileIdentityKeyStore::at(&path)
        .load_or_create_local_peer()
        .expect("first launch must yield an identity");

    // A second store over the same file is exactly what the next launch does.
    let second = FileIdentityKeyStore::at(&path)
        .load_or_create_local_peer()
        .expect("a restart must load the identity");

    assert_eq!(first, second, "AC9: the PeerId is stable across restarts");
}

#[test]
fn two_directories_hold_two_identities() {
    let one = TestDir::new("keystore-distinct-one");
    let other = TestDir::new("keystore-distinct-other");

    let first = FileIdentityKeyStore::at(one.file(FileIdentityKeyStore::FILE_NAME))
        .load_or_create_local_peer()
        .expect("an identity must be created");
    let second = FileIdentityKeyStore::at(other.file(FileIdentityKeyStore::FILE_NAME))
        .load_or_create_local_peer()
        .expect("an identity must be created");

    // If this ever fails, the random source is not one.
    assert_ne!(first, second);
}

#[test]
fn the_file_holds_the_seed_and_only_the_seed() {
    let dir = TestDir::new("keystore-layout");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    let peer = store
        .load_or_create_local_peer()
        .expect("an identity must be created");
    let contents = fs::read_to_string(store.path()).expect("the file exists");
    let lines: Vec<&str> = contents.lines().collect();

    assert_eq!(lines.len(), 2, "header plus seed, nothing else");
    assert_eq!(lines[0], "distro-identity-key 1");
    assert!(lines[1].starts_with("ed25519-seed "));
    // The public half is derived, never stored: no second field can disagree
    // with the first (invariant 1).
    assert!(
        !contents.contains(&hex_bytes::encode(peer.as_bytes())),
        "the public key must not be written to the file"
    );
}

#[test]
fn a_stored_seed_derives_the_public_key_the_test_vector_states() {
    let dir = TestDir::new("keystore-derivation");
    let path = plant(
        &dir,
        &format!(
            "distro-identity-key 1\ned25519-seed {}\n",
            hex_bytes::encode(&ALICE_SECRET_KEY)
        ),
    );

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Ok(alice()),
        "RFC 8032 §7.1 TEST 1 pins the seed-to-PeerId derivation"
    );
}

#[cfg(unix)]
#[test]
fn the_key_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new("keystore-mode");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    store
        .load_or_create_local_peer()
        .expect("an identity must be created");

    let mode = fs::metadata(store.path())
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(mode, 0o600);
}

#[test]
fn a_truncated_key_file_is_corrupt_rather_than_a_new_identity() {
    let dir = TestDir::new("keystore-truncated");
    let path = plant(&dir, "distro-identity-key 1\ned25519-seed 9d61b1");
    let original = fs::read(&path).expect("the file exists");

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Err(IdentityKeyStoreError::Corrupt)
    );
    // Silently minting a replacement identity is the one thing that must not
    // happen here: the old one may still be recoverable.
    assert_eq!(fs::read(&path).expect("the file exists"), original);
}

#[test]
fn a_header_only_key_file_is_corrupt() {
    let dir = TestDir::new("keystore-header-only");
    let path = plant(&dir, "distro-identity-key 1\n");

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Err(IdentityKeyStoreError::Corrupt)
    );
}

#[test]
fn an_extra_line_is_corrupt() {
    let dir = TestDir::new("keystore-extra-line");
    let path = plant(
        &dir,
        &format!(
            "distro-identity-key 1\ned25519-seed {}\ned25519-seed {}\n",
            hex_bytes::encode(&ALICE_SECRET_KEY),
            hex_bytes::encode(&ALICE_SECRET_KEY)
        ),
    );

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Err(IdentityKeyStoreError::Corrupt)
    );
}

#[test]
fn an_unknown_tag_on_the_seed_line_is_corrupt() {
    let dir = TestDir::new("keystore-unknown-tag");
    let path = plant(
        &dir,
        &format!(
            "distro-identity-key 1\nsecp256k1-seed {}\n",
            hex_bytes::encode(&ALICE_SECRET_KEY)
        ),
    );

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Err(IdentityKeyStoreError::Corrupt)
    );
}

#[test]
fn an_unknown_schema_version_is_reported_and_the_key_is_preserved() {
    let dir = TestDir::new("keystore-future-version");
    let original = "distro-identity-key 2\nsomething a later build understands\n";
    let path = plant(&dir, original);

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Err(IdentityKeyStoreError::UnsupportedSchemaVersion { found: 2 })
    );
    // S4: refused, never rewritten — running the newer build again must find
    // its identity exactly as it left it.
    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        original
    );
}

#[test]
fn a_missing_directory_reports_creation_failed_rather_than_panicking() {
    let dir = TestDir::new("keystore-missing-directory");
    let store = FileIdentityKeyStore::at(
        dir.file("never-created")
            .join(FileIdentityKeyStore::FILE_NAME),
    );

    assert_eq!(
        store.load_or_create_local_peer(),
        Err(IdentityKeyStoreError::CreationFailed)
    );
}

#[test]
fn a_directory_where_the_key_belongs_is_unreadable() {
    let dir = TestDir::new("keystore-directory-in-the-way");
    let path = dir.file(FileIdentityKeyStore::FILE_NAME);
    fs::create_dir(&path).expect("the plant must land");

    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_local_peer(),
        Err(IdentityKeyStoreError::Unreadable)
    );
}

// ------------------------------------------------- the transport key seam (S3a)

#[test]
fn the_transport_secret_is_the_seed_in_the_file() {
    // S3a's one crossing: the libp2p handshake needs the raw secret, so the
    // composition root reads it from the concrete store — never from the port,
    // which still returns a `PeerId` and nothing else.
    let dir = TestDir::new("keystore-transport-secret");
    let path = plant(
        &dir,
        &format!(
            "distro-identity-key 1\ned25519-seed {}\n",
            hex_bytes::encode(&ALICE_SECRET_KEY)
        ),
    );
    let store = FileIdentityKeyStore::at(&path);

    let mut secret = [0u8; 32];
    let peer = store
        .load_or_create_transport_secret_key(&mut secret)
        .expect("the planted key must load");

    assert_eq!(secret, ALICE_SECRET_KEY);
    assert_eq!(peer, alice());
}

#[test]
fn the_transport_secret_is_the_same_identity_the_port_reports() {
    // Both entry points come through one load-or-create path, so the swarm
    // authenticates as exactly the peer the rest of the application is.
    let dir = TestDir::new("keystore-transport-agreement");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    let mut secret = [0u8; 32];
    let transport_peer = store
        .load_or_create_transport_secret_key(&mut secret)
        .expect("a fresh directory must yield an identity");

    assert_eq!(store.load_or_create_local_peer(), Ok(transport_peer));
    assert_eq!(
        store
            .load_or_create_signer()
            .expect("the same key must load")
            .peer(),
        transport_peer
    );
}

#[test]
fn asking_for_the_transport_secret_first_creates_the_identity() {
    // AC1: the startup order is identity-then-network, but either call may be
    // the one that creates the file, and neither may fail because it was.
    let dir = TestDir::new("keystore-transport-creates");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    let mut secret = [0u8; 32];
    store
        .load_or_create_transport_secret_key(&mut secret)
        .expect("a fresh directory must yield an identity");

    assert!(store.path().exists());
    assert_ne!(secret, [0u8; 32]);
}

#[test]
fn zeroize_overwrites_the_buffer() {
    // The caller's error path. The success path hands the buffer to
    // `NetworkIdentity::from_ed25519_secret_key`, which clears it.
    let mut secret = ALICE_SECRET_KEY;

    FileIdentityKeyStore::zeroize(&mut secret);

    assert_eq!(secret, [0u8; 32]);
}

#[test]
fn an_unreadable_key_file_refuses_the_transport_secret_too() {
    let dir = TestDir::new("keystore-transport-corrupt");
    let path = plant(&dir, "distro-identity-key 1\ned25519-seed not-hex\n");

    let mut secret = [0u8; 32];
    assert_eq!(
        FileIdentityKeyStore::at(&path).load_or_create_transport_secret_key(&mut secret),
        Err(IdentityKeyStoreError::Corrupt)
    );
    assert_eq!(secret, [0u8; 32], "nothing may be written on a refusal");
}
