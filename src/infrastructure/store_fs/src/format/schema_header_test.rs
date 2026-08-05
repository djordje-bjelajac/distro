use crate::format::{LocalFileError, SchemaHeader};

const HEADER: SchemaHeader = SchemaHeader::new("distro-example", 1);

#[test]
fn renders_the_magic_and_the_version() {
    assert_eq!(HEADER.line(), "distro-example 1");
}

#[test]
fn accepts_the_line_it_renders() {
    assert_eq!(HEADER.accept(&HEADER.line()), Ok(()));
}

#[test]
fn rejects_another_stores_file_as_corrupt() {
    assert_eq!(
        HEADER.accept("distro-something-else 1"),
        Err(LocalFileError::Corrupt)
    );
}

#[test]
fn reports_a_newer_version_with_the_number_found() {
    assert_eq!(
        HEADER.accept("distro-example 2"),
        Err(LocalFileError::UnsupportedSchemaVersion { found: 2 })
    );
}

#[test]
fn reports_an_older_version_with_the_number_found() {
    // There is no v0, so this is a damaged or forged file rather than a
    // downgrade — but it is still a *stated* version, and naming it is more
    // useful than calling it corrupt.
    assert_eq!(
        HEADER.accept("distro-example 0"),
        Err(LocalFileError::UnsupportedSchemaVersion { found: 0 })
    );
}

#[test]
fn rejects_a_version_that_is_not_a_number() {
    // Not `UnsupportedSchemaVersion`: naming a version nobody wrote would be
    // worse than admitting the line makes no sense.
    assert_eq!(
        HEADER.accept("distro-example latest"),
        Err(LocalFileError::Corrupt)
    );
}

#[test]
fn rejects_a_line_with_no_version_at_all() {
    assert_eq!(
        HEADER.accept("distro-example"),
        Err(LocalFileError::Corrupt)
    );
    assert_eq!(HEADER.accept(""), Err(LocalFileError::Corrupt));
}
