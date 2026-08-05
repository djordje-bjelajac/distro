use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::format::{SchemaHeader, private_file};

/// Reads a versioned store file, returning its body lines.
///
/// `Ok(None)` is an absent file, which every store here treats as a valid
/// starting state rather than a failure: a fresh install has no keypair, no
/// trust records, no cached peers and has spoken in no conversation.
///
/// **This function never writes.** That is what makes S4's "preserve the
/// original" guarantee structural rather than a promise each store has to keep
/// separately: there is no path from an unreadable, corrupt, or
/// future-versioned file to a modification of it.
///
/// Empty body lines are refused. Nothing here writes one, so a blank line means
/// the file has been truncated or edited, and skipping past it would let a
/// half-written file read as a shorter valid one.
pub(crate) fn read_versioned(
    path: &Path,
    header: &SchemaHeader,
) -> Result<Option<Vec<String>>, LocalFileError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(LocalFileError::Unreadable),
    };

    let text = String::from_utf8(bytes).map_err(|_| LocalFileError::Corrupt)?;
    let mut lines = text.lines();

    header.accept(lines.next().ok_or(LocalFileError::Corrupt)?)?;

    let body: Vec<String> = lines.map(str::to_owned).collect();

    if body.iter().any(|line| line.trim().is_empty()) {
        return Err(LocalFileError::Corrupt);
    }

    Ok(Some(body))
}

/// Replaces a versioned store file with `header` and `body`, atomically.
///
/// The header is written by this function rather than by callers, so a body
/// can never reach disk without the version that explains it.
pub(crate) fn write_versioned(
    path: &Path,
    header: &SchemaHeader,
    body: &[String],
) -> Result<(), LocalFileError> {
    let mut text = header.line();

    for line in body {
        text.push('\n');
        text.push_str(line);
    }

    // A trailing newline so the file ends on a line boundary: `str::lines`
    // treats the two forms identically, and the conventional shape is the one
    // an operator inspecting the directory expects.
    text.push('\n');

    private_file::replace_atomically(path, text.as_bytes()).map_err(|_| LocalFileError::WriteFailed)
}

/// What can go wrong with one local file, before it is translated into the
/// typed error of whichever port asked.
///
/// Kept internal on purpose: the ports each have their own vocabulary
/// (`IdentityKeyStoreError::CreationFailed` and
/// `PeerCacheError::WriteFailed` are the same event seen from two contexts),
/// and translating at the boundary is what keeps a filesystem concept out of
/// the contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalFileError {
    /// The file exists but could not be read.
    Unreadable,
    /// The file was read but does not parse.
    Corrupt,
    /// The file carries a schema version this build does not understand. The
    /// original is untouched (S4).
    UnsupportedSchemaVersion { found: u32 },
    /// The file could not be written; the caller must assume the change did not
    /// survive.
    WriteFailed,
}

impl fmt::Display for LocalFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("the store file could not be read"),
            Self::Corrupt => f.write_str("the store file does not parse"),
            Self::UnsupportedSchemaVersion { found } => {
                write!(f, "the store file has unsupported schema version {found}")
            }
            Self::WriteFailed => f.write_str("the store file could not be written"),
        }
    }
}

impl std::error::Error for LocalFileError {}
