use crate::format::LocalFileError;

/// The first line of every file this crate writes: `<magic> <version>`.
///
/// # What each half is for
///
/// The **magic** names the store. It is not decoration: the four files live in
/// one directory, and a reader that accepted any well-formed body would happily
/// parse a peer cache as trust records the day someone renames a file. A
/// mismatch is [`Corrupt`](LocalFileError::Corrupt) — this reader cannot say
/// what the bytes are, only that they are not its own.
///
/// The **version** is S4's migration discipline in one number. It is `1` for
/// every store today and moves when a layout changes. A file carrying anything
/// else yields
/// [`UnsupportedSchemaVersion { found }`](LocalFileError::UnsupportedSchemaVersion)
/// and is left byte-for-byte alone, so downgrading a build never costs a peer
/// its identity, its trust decisions, or its warm start. When a v2 arrives it
/// arrives as an explicit v1→v2 upgrade path here, next to this comment;
/// silently rewriting a file whose meaning this build does not know is the one
/// thing S4 forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SchemaHeader {
    magic: &'static str,
    version: u32,
}

impl SchemaHeader {
    /// The header a store writes and demands.
    pub(crate) const fn new(magic: &'static str, version: u32) -> Self {
        Self { magic, version }
    }

    /// The line to write at the top of the file.
    pub(crate) fn line(&self) -> String {
        format!("{} {}", self.magic, self.version)
    }

    /// Checks the first line of a file against this header.
    pub(crate) fn accept(&self, line: &str) -> Result<(), LocalFileError> {
        let (magic, version) = line.split_once(' ').ok_or(LocalFileError::Corrupt)?;

        if magic != self.magic {
            return Err(LocalFileError::Corrupt);
        }

        // A version that is not a number at all says nothing about which
        // version it is, so it is malformed rather than unsupported: reporting
        // `UnsupportedSchemaVersion { found: 0 }` would name a version nobody
        // ever wrote.
        let found: u32 = version.parse().map_err(|_| LocalFileError::Corrupt)?;

        if found == self.version {
            Ok(())
        } else {
            Err(LocalFileError::UnsupportedSchemaVersion { found })
        }
    }
}
