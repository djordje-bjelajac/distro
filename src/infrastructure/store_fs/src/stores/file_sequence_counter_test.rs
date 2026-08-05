use std::fs;
use std::path::PathBuf;

use messaging::domain::{ConversationId, SequenceNumber};
use messaging::ports::{SequenceCounterError, SequenceCounterPort};

use crate::format::hex_bytes;
use crate::stores::FileSequenceCounter;
use crate::test_dir::TestDir;
use crate::test_peers::{alice, bob};

fn counter(dir: &TestDir) -> FileSequenceCounter {
    FileSequenceCounter::at(dir.file(FileSequenceCounter::FILE_NAME))
}

fn plant(dir: &TestDir, contents: &str) -> PathBuf {
    let path = dir.file(FileSequenceCounter::FILE_NAME);
    fs::write(&path, contents).expect("the plant must land");
    path
}

fn issue(counter: &FileSequenceCounter, conversation: ConversationId) -> u64 {
    counter
        .issue_next(conversation)
        .expect("the counter must be healthy")
        .as_u64()
}

#[test]
fn a_fresh_counter_has_issued_nothing() {
    let dir = TestDir::new("counter-fresh");

    assert_eq!(
        counter(&dir).last_issued(ConversationId::Broadcast),
        Ok(None)
    );
}

#[test]
fn the_first_number_issued_is_one() {
    let dir = TestDir::new("counter-first");

    assert_eq!(
        counter(&dir).issue_next(ConversationId::Broadcast),
        Ok(SequenceNumber::FIRST)
    );
}

#[test]
fn numbers_are_strictly_monotonic_within_a_conversation() {
    let dir = TestDir::new("counter-monotonic");
    let counter = counter(&dir);

    let issued: Vec<u64> = (0..5)
        .map(|_| issue(&counter, ConversationId::Broadcast))
        .collect();

    assert_eq!(issued, vec![1, 2, 3, 4, 5]);
    assert_eq!(
        counter.last_issued(ConversationId::Broadcast),
        Ok(Some(
            SequenceNumber::new(5).expect("5 is a sequence number")
        ))
    );
}

#[test]
fn each_conversation_counts_independently() {
    let dir = TestDir::new("counter-independent");
    let counter = counter(&dir);

    issue(&counter, ConversationId::Broadcast);
    issue(&counter, ConversationId::Broadcast);

    assert_eq!(issue(&counter, ConversationId::Direct(alice())), 1);
    assert_eq!(issue(&counter, ConversationId::Direct(bob())), 1);
    assert_eq!(issue(&counter, ConversationId::Broadcast), 3);
}

#[test]
fn the_advance_is_on_disk_before_issue_next_returns() {
    let dir = TestDir::new("counter-persist-first");
    let counter = counter(&dir);

    let issued = issue(&counter, ConversationId::Broadcast);
    // Read the file directly, in the instant after the call returned: a number
    // handed out but not recorded is a number a crash would re-issue.
    let contents = fs::read_to_string(counter.path()).expect("the file exists");

    assert_eq!(issued, 1);
    assert_eq!(contents, "distro-sequence-counter 1\nbroadcast 1\n");
}

#[test]
fn a_reload_never_re_issues_a_number() {
    let dir = TestDir::new("counter-restart");

    let before: Vec<u64> = (0..3)
        .map(|_| issue(&counter(&dir), ConversationId::Broadcast))
        .collect();

    // AC16: the peer restarts — a new store over the same file, exactly as the
    // next launch builds it — and must not repeat itself, or every message it
    // sends is classified a duplicate by peers still online and it goes mute.
    let after: Vec<u64> = (0..3)
        .map(|_| issue(&counter(&dir), ConversationId::Broadcast))
        .collect();

    assert_eq!(before, vec![1, 2, 3]);
    assert_eq!(after, vec![4, 5, 6]);
}

#[test]
fn a_restart_resumes_every_conversation_independently() {
    let dir = TestDir::new("counter-restart-many");
    let first = counter(&dir);

    issue(&first, ConversationId::Broadcast);
    issue(&first, ConversationId::Broadcast);
    issue(&first, ConversationId::Direct(bob()));
    drop(first);

    let second = counter(&dir);

    assert_eq!(issue(&second, ConversationId::Broadcast), 3);
    assert_eq!(issue(&second, ConversationId::Direct(bob())), 2);
    assert_eq!(issue(&second, ConversationId::Direct(alice())), 1);
}

#[test]
fn a_counter_beside_a_deleted_key_starts_over() {
    let dir = TestDir::new("counter-key-gone");

    issue(&counter(&dir), ConversationId::Broadcast);
    // The counter's contract is the keypair's lifetime: discarding the identity
    // directory discards the counter with it, and starting at FIRST is then
    // correct rather than harmful (D12).
    fs::remove_file(dir.file(FileSequenceCounter::FILE_NAME)).expect("the removal must land");

    assert_eq!(
        counter(&dir).issue_next(ConversationId::Broadcast),
        Ok(SequenceNumber::FIRST)
    );
}

#[test]
fn the_file_lists_broadcast_first_then_directs_by_peer() {
    let dir = TestDir::new("counter-layout");
    let counter = counter(&dir);

    issue(&counter, ConversationId::Direct(bob()));
    issue(&counter, ConversationId::Direct(alice()));
    issue(&counter, ConversationId::Broadcast);

    let contents = fs::read_to_string(counter.path()).expect("the file exists");
    let mut peers = [alice(), bob()];
    peers.sort_unstable();

    assert_eq!(
        contents,
        format!(
            "distro-sequence-counter 1\nbroadcast 1\ndirect {} 1\ndirect {} 1\n",
            hex_bytes::encode(peers[0].as_bytes()),
            hex_bytes::encode(peers[1].as_bytes())
        )
    );
}

#[test]
fn a_corrupt_counter_file_is_unavailable_rather_than_a_panic() {
    let dir = TestDir::new("counter-corrupt");
    let path = plant(&dir, "distro-sequence-counter 1\nbroadcast not-a-number\n");

    // The port has no corruption variant; "cannot be reached" is the honest
    // reading, and it correctly stops the caller from sending.
    assert_eq!(
        FileSequenceCounter::at(&path).issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::Unavailable)
    );
}

#[test]
fn a_stored_zero_is_refused() {
    let dir = TestDir::new("counter-zero");
    let path = plant(&dir, "distro-sequence-counter 1\nbroadcast 0\n");

    // 0 means "no message yet", which an absent line already says; a written 0
    // is a file this build did not produce.
    assert_eq!(
        FileSequenceCounter::at(&path).last_issued(ConversationId::Broadcast),
        Err(SequenceCounterError::Unavailable)
    );
}

#[test]
fn a_duplicate_conversation_line_is_refused() {
    let dir = TestDir::new("counter-duplicate");
    let path = plant(
        &dir,
        "distro-sequence-counter 1\nbroadcast 4\nbroadcast 9\n",
    );

    // Picking either answer risks picking the lower one, which re-issues.
    assert_eq!(
        FileSequenceCounter::at(&path).issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::Unavailable)
    );
}

#[test]
fn an_exhausted_conversation_reports_exhausted_rather_than_wrapping() {
    let dir = TestDir::new("counter-exhausted");
    let path = plant(
        &dir,
        &format!("distro-sequence-counter 1\nbroadcast {}\n", u64::MAX),
    );

    assert_eq!(
        FileSequenceCounter::at(&path).issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::Exhausted)
    );
}

#[test]
fn an_unknown_schema_version_is_reported_and_the_counter_is_preserved() {
    let dir = TestDir::new("counter-future-version");
    let original = "distro-sequence-counter 9\nwhatever a later build writes\n";
    let path = plant(&dir, original);

    assert_eq!(
        FileSequenceCounter::at(&path).issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::UnsupportedSchemaVersion { found: 9 })
    );
    // Rewriting here would reset the counter, which is the mute-peer defect.
    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        original
    );
}

#[test]
fn a_write_that_cannot_land_issues_nothing() {
    let dir = TestDir::new("counter-write-fails");
    let counter = FileSequenceCounter::at(
        dir.file("never-created")
            .join(FileSequenceCounter::FILE_NAME),
    );

    // Reporting NotPersisted and sending nothing is strictly better than
    // sending something every peer will ignore.
    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::NotPersisted)
    );
}

#[cfg(unix)]
#[test]
fn a_number_is_not_issued_when_the_advance_cannot_be_recorded() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new("counter-readonly-dir");
    let counter = counter(&dir);

    assert_eq!(issue(&counter, ConversationId::Broadcast), 1);

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500))
        .expect("the chmod must land");

    // A process with privileges that ignore mode bits (root in a container)
    // cannot observe this failure; the assertion would be about the sandbox
    // rather than about the counter.
    if fs::write(dir.file("probe"), b"x").is_ok() {
        let _ = fs::remove_file(dir.file("probe"));
        return;
    }

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::NotPersisted)
    );

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))
        .expect("the chmod must land");

    // The mark on disk is still the one that was recorded, so nothing was
    // silently consumed and nothing will be re-issued.
    assert_eq!(
        counter.last_issued(ConversationId::Broadcast),
        Ok(Some(SequenceNumber::FIRST))
    );
    assert_eq!(issue(&counter, ConversationId::Broadcast), 2);
}
