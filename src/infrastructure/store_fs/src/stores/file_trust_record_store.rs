use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use identity::domain::{TrustRecord, VerificationState};
use identity::ports::{TrustRecordStoreError, TrustRecordStorePort};
use shared_types::PeerId;

use crate::format::{LocalFileError, SchemaHeader, hex_bytes, read_versioned, write_versioned};

/// What this peer locally believes about every remote peer it has verified or
/// blocked, in one file (canvas §2.1, invariant 11).
///
/// # The file
///
/// ```text
/// distro-trust-records 1
/// record <64 hex characters: peer id> <unverified|verified> <open|blocked>
/// record …
/// ```
///
/// One line per peer, sorted by peer id so the file is byte-identical for a
/// given set of records regardless of the order they were saved in — a
/// diffable file is one a user can eyeball, and a deterministic one does not
/// churn on every save. Both trust axes are written out in full because they
/// are orthogonal: blocking a verified peer keeps the verification, and
/// unblocking restores exactly the state that was there before.
///
/// A duplicate peer line is corruption rather than a last-one-wins merge.
/// Nothing here can write one, so a second line for a peer means the file was
/// edited or damaged, and quietly picking one of two answers about whether
/// someone is blocked is not a thing this store should do.
///
/// # Read-modify-write, whole file
///
/// [`save_trust_record`](TrustRecordStorePort::save_trust_record) is a
/// whole-record upsert, so it reads the file, replaces one entry, and writes
/// the file back atomically. At the scale this holds — the peers one human has
/// verified or blocked — that is cheaper than any incremental format would be
/// to get right, and it means a partially written file can never exist (see
/// [`crate::format`]).
pub struct FileTrustRecordStore {
    path: PathBuf,
    /// Serialises read-modify-write within one process; two threads blocking
    /// two different peers concurrently must not lose one of the decisions.
    gate: Mutex<()>,
}

impl FileTrustRecordStore {
    /// The conventional file name inside a store directory.
    pub const FILE_NAME: &'static str = "trust.records";

    /// The header every version of this file carries.
    const HEADER: SchemaHeader = SchemaHeader::new("distro-trust-records", 1);

    const RECORD_TAG: &'static str = "record";
    const UNVERIFIED: &'static str = "unverified";
    const VERIFIED: &'static str = "verified";
    const OPEN: &'static str = "open";
    const BLOCKED: &'static str = "blocked";

    /// A store keeping its records at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            gate: Mutex::new(()),
        }
    }

    /// Where this store keeps its records.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<BTreeMap<PeerId, TrustRecord>, TrustRecordStoreError> {
        let Some(body) = read_versioned(&self.path, &Self::HEADER).map_err(to_port_error)? else {
            return Ok(BTreeMap::new());
        };

        let mut records = BTreeMap::new();

        for line in &body {
            let record = Self::parse_record(line).ok_or(TrustRecordStoreError::Corrupt)?;

            if records.insert(record.peer(), record).is_some() {
                return Err(TrustRecordStoreError::Corrupt);
            }
        }

        Ok(records)
    }

    fn write(&self, records: &BTreeMap<PeerId, TrustRecord>) -> Result<(), TrustRecordStoreError> {
        let body: Vec<String> = records.values().map(Self::render_record).collect();

        write_versioned(&self.path, &Self::HEADER, &body).map_err(to_port_error)
    }

    fn render_record(record: &TrustRecord) -> String {
        let verification = if record.is_verified() {
            Self::VERIFIED
        } else {
            Self::UNVERIFIED
        };
        let blocked = if record.is_blocked() {
            Self::BLOCKED
        } else {
            Self::OPEN
        };

        format!(
            "{} {} {verification} {blocked}",
            Self::RECORD_TAG,
            hex_bytes::encode(record.peer().as_bytes())
        )
    }

    fn parse_record(line: &str) -> Option<TrustRecord> {
        let mut fields = line.split(' ');

        if fields.next()? != Self::RECORD_TAG {
            return None;
        }

        let peer = PeerId::from_public_key_bytes(hex_bytes::decode(fields.next()?)?).ok()?;

        let verification = match fields.next()? {
            tag if tag == Self::VERIFIED => VerificationState::Verified,
            tag if tag == Self::UNVERIFIED => VerificationState::Unverified,
            _ => return None,
        };
        let blocked = match fields.next()? {
            tag if tag == Self::BLOCKED => true,
            tag if tag == Self::OPEN => false,
            _ => return None,
        };

        // Trailing rubbish is corruption, not something to ignore: this format
        // has exactly four fields per line and a fifth means the writer was not
        // this build.
        if fields.next().is_some() {
            return None;
        }

        Some(TrustRecord::rehydrate(peer, verification, blocked))
    }
}

impl TrustRecordStorePort for FileTrustRecordStore {
    fn load_trust_record(
        &self,
        peer: PeerId,
    ) -> Result<Option<TrustRecord>, TrustRecordStoreError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        // An absent record is the trust-on-first-use starting point, not a
        // failure — and so is an absent file.
        Ok(self.read()?.get(&peer).cloned())
    }

    fn save_trust_record(&self, record: &TrustRecord) -> Result<(), TrustRecordStoreError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        let mut records = self.read()?;
        records.insert(record.peer(), record.clone());

        self.write(&records)
    }

    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        Ok(self
            .read()?
            .values()
            .filter(|record| record.is_blocked())
            .map(TrustRecord::peer)
            .collect())
    }
}

impl std::fmt::Debug for FileTrustRecordStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTrustRecordStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

const fn to_port_error(error: LocalFileError) -> TrustRecordStoreError {
    match error {
        LocalFileError::Unreadable => TrustRecordStoreError::Unreadable,
        LocalFileError::Corrupt => TrustRecordStoreError::Corrupt,
        LocalFileError::UnsupportedSchemaVersion { found } => {
            TrustRecordStoreError::UnsupportedSchemaVersion { found }
        }
        LocalFileError::WriteFailed => TrustRecordStoreError::WriteFailed,
    }
}
