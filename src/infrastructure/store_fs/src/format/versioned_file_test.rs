use std::fs;

use crate::format::{LocalFileError, SchemaHeader, read_versioned, write_versioned};
use crate::test_dir::TestDir;

const HEADER: SchemaHeader = SchemaHeader::new("distro-example", 1);

fn body(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn an_absent_file_reads_as_absent_rather_than_failing() {
    let dir = TestDir::new("versioned-absent");

    assert_eq!(read_versioned(&dir.file("nothing"), &HEADER), Ok(None));
}

#[test]
fn round_trips_a_body() {
    let dir = TestDir::new("versioned-round-trip");
    let path = dir.file("store");

    write_versioned(&path, &HEADER, &body(&["one", "two three"])).expect("the write must land");

    assert_eq!(
        read_versioned(&path, &HEADER),
        Ok(Some(body(&["one", "two three"])))
    );
}

#[test]
fn writes_the_header_and_a_trailing_newline() {
    let dir = TestDir::new("versioned-layout");
    let path = dir.file("store");

    write_versioned(&path, &HEADER, &body(&["one"])).expect("the write must land");

    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        "distro-example 1\none\n"
    );
}

#[test]
fn an_empty_body_is_a_header_only_file() {
    let dir = TestDir::new("versioned-empty");
    let path = dir.file("store");

    write_versioned(&path, &HEADER, &[]).expect("the write must land");

    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        "distro-example 1\n"
    );
    assert_eq!(read_versioned(&path, &HEADER), Ok(Some(Vec::new())));
}

#[test]
fn a_truncated_file_is_corrupt_not_a_panic() {
    let dir = TestDir::new("versioned-truncated");
    let path = dir.file("store");

    fs::write(&path, "distro-ex").expect("the plant must land");

    assert_eq!(read_versioned(&path, &HEADER), Err(LocalFileError::Corrupt));
}

#[test]
fn a_blank_body_line_is_corrupt() {
    let dir = TestDir::new("versioned-blank");
    let path = dir.file("store");

    fs::write(&path, "distro-example 1\none\n\ntwo\n").expect("the plant must land");

    assert_eq!(read_versioned(&path, &HEADER), Err(LocalFileError::Corrupt));
}

#[test]
fn non_utf8_bytes_are_corrupt_not_a_panic() {
    let dir = TestDir::new("versioned-non-utf8");
    let path = dir.file("store");

    fs::write(&path, [0xffu8, 0xfe, 0xfd]).expect("the plant must land");

    assert_eq!(read_versioned(&path, &HEADER), Err(LocalFileError::Corrupt));
}

#[test]
fn an_unknown_version_is_reported_and_the_file_is_left_untouched() {
    let dir = TestDir::new("versioned-future");
    let path = dir.file("store");
    let original = "distro-example 2\nsomething this build has never heard of\n";

    fs::write(&path, original).expect("the plant must land");

    assert_eq!(
        read_versioned(&path, &HEADER),
        Err(LocalFileError::UnsupportedSchemaVersion { found: 2 })
    );
    // S4: refused, never rewritten. A downgraded build must be able to hand the
    // file back to the newer one unharmed.
    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        original
    );
}

#[test]
fn a_directory_where_a_file_belongs_is_unreadable_not_a_panic() {
    let dir = TestDir::new("versioned-directory");
    let path = dir.file("store");

    fs::create_dir(&path).expect("the plant must land");

    assert_eq!(
        read_versioned(&path, &HEADER),
        Err(LocalFileError::Unreadable)
    );
}

#[test]
fn a_write_that_cannot_land_is_reported() {
    let dir = TestDir::new("versioned-write-fails");
    let path = dir.file("absent").join("store");

    assert_eq!(
        write_versioned(&path, &HEADER, &body(&["one"])),
        Err(LocalFileError::WriteFailed)
    );
}
