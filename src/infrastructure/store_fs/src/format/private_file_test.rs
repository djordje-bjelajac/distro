use std::fs;

use crate::format::private_file;
use crate::test_dir::TestDir;

#[test]
fn replace_writes_the_whole_payload() {
    let dir = TestDir::new("private-replace");
    let path = dir.file("target");

    private_file::replace_atomically(&path, b"first").expect("the write must land");
    private_file::replace_atomically(&path, b"second payload").expect("the write must land");

    assert_eq!(fs::read(&path).expect("the file exists"), b"second payload");
}

#[test]
fn replace_leaves_no_temp_file_behind() {
    let dir = TestDir::new("private-no-temp");
    let path = dir.file("target");

    private_file::replace_atomically(&path, b"payload").expect("the write must land");

    let leftovers: Vec<String> = fs::read_dir(dir.path())
        .expect("the directory exists")
        .map(|entry| {
            entry
                .expect("a readable entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != "target")
        .collect();

    assert_eq!(leftovers, Vec::<String>::new());
}

#[test]
fn a_leftover_temp_file_is_never_the_live_file() {
    let dir = TestDir::new("private-stale-temp");
    let path = dir.file("target");

    private_file::replace_atomically(&path, b"the good payload").expect("the write must land");

    // Exactly what a crash between "temp written" and "renamed" leaves: a
    // half-written file whose name marks it as not live.
    fs::write(dir.file("target.tmp-999-999"), b"half a pay").expect("the plant must land");

    assert_eq!(
        fs::read(&path).expect("the file exists"),
        b"the good payload"
    );

    // And it does not stop the next write from succeeding.
    private_file::replace_atomically(&path, b"the next payload").expect("the write must land");
    assert_eq!(
        fs::read(&path).expect("the file exists"),
        b"the next payload"
    );
}

#[test]
fn replace_reports_a_missing_directory_rather_than_panicking() {
    let dir = TestDir::new("private-missing-dir");
    let path = dir.file("absent").join("target");

    assert!(private_file::replace_atomically(&path, b"payload").is_err());
}

#[test]
fn create_exclusively_refuses_to_clobber() {
    let dir = TestDir::new("private-exclusive");
    let path = dir.file("target");

    private_file::create_exclusively(&path, b"the first identity").expect("the write must land");

    let second = private_file::create_exclusively(&path, b"a second identity")
        .expect_err("the second create must lose");

    assert_eq!(second.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read(&path).expect("the file exists"),
        b"the first identity"
    );
}

#[cfg(unix)]
#[test]
fn every_written_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new("private-modes");
    let replaced = dir.file("replaced");
    let created = dir.file("created");

    private_file::replace_atomically(&replaced, b"payload").expect("the write must land");
    private_file::create_exclusively(&created, b"payload").expect("the write must land");

    for path in [&replaced, &created] {
        let mode = fs::metadata(path)
            .expect("the file exists")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600, "{} must be owner-only", path.display());
    }
}

#[cfg(unix)]
#[test]
fn replace_hardens_a_file_that_was_left_world_readable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new("private-harden");
    let path = dir.file("target");

    fs::write(&path, b"loose").expect("the plant must land");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("the chmod must land");

    private_file::replace_atomically(&path, b"tight").expect("the write must land");

    let mode = fs::metadata(&path)
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn the_store_directory_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDir::new("private-dir-mode");
    let nested = dir.file("nested");

    private_file::create_owner_only_directory(&nested).expect("the directory must be created");

    let mode = fs::metadata(&nested)
        .expect("the directory exists")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(mode, 0o700);
}
