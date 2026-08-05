use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use messaging::domain::{ConversationId, SequenceNumber};
use messaging::ports::{SequenceCounterError, SequenceCounterPort};
use shared_types::PeerId;

use crate::format::{LocalFileError, SchemaHeader, hex_bytes, read_versioned, write_versioned};

/// The local peer's outbound sequence counter, one number per conversation,
/// beside the keypair (D12, AC16).
///
/// # The file
///
/// ```text
/// distro-sequence-counter 1
/// broadcast <last issued number>
/// direct <64 hex characters: peer id> <last issued number>
/// direct …
/// ```
///
/// One line per conversation this peer has spoken in, broadcast first and
/// directs after it in peer-id order — the same order
/// [`ConversationId`] itself sorts in. A number of `0` is corruption: 0 is
/// reserved for "no message yet" and an absent line already says that.
///
/// # Why it is on disk at all
///
/// With in-memory-only history (D7) a restarted peer used to resume at
/// [`SequenceNumber::FIRST`] while every peer still online held its high-water
/// mark at N. Each message it sent was then, correctly by the receiver's rules,
/// classified a duplicate and ignored: the peer went permanently mute while
/// appearing, to itself, to work. The counter's domain of validity is the
/// identity, not the process — so it lives in the same directory as the key and
/// shares its lifetime exactly. Delete the key and the counter goes with it,
/// which is right: that is a different identity, and it starts at `FIRST`.
///
/// # Persisted before it is returned
///
/// [`issue_next`](SequenceCounterPort::issue_next) reads the file, computes the
/// next number, writes the whole file back atomically, and only then returns.
/// A crash at any point leaves either the old number (so the message was never
/// sent and the number will be issued to someone else's message — harmless) or
/// the new one (so the number is spent and the next call moves past it —
/// harmless). What cannot happen is a number handed to a caller and forgotten,
/// which is the silent-duplicate defect this port exists to prevent. A write
/// that fails is reported as
/// [`NotPersisted`](SequenceCounterError::NotPersisted) and **no number is
/// returned**: sending nothing beats sending something every peer will ignore.
///
/// Skipping a number is therefore possible and deliberately harmless — receivers
/// tolerate gaps for the whole gap-tolerance window and then close them
/// explicitly (rule R). Re-issuing one is not possible, and that asymmetry is
/// the entire design.
///
/// # State lives in the file, not in this struct
///
/// Every operation re-reads. It costs one read of a file measured in hundreds
/// of bytes, and it buys the property that the disk is the only authority:
/// there is no cached mark that could survive a failed write, and two stores
/// opened on one directory cannot disagree.
///
/// # One coarseness worth knowing
///
/// [`SequenceCounterError`] has no `Corrupt` variant, so a damaged counter file
/// is reported as [`Unavailable`](SequenceCounterError::Unavailable) — the
/// counter cannot be reached, which is true and which correctly stops the
/// caller from sending. The distinction the other ports draw is not available
/// here, and inventing one would mean changing a domain contract from an
/// adapter.
pub struct FileSequenceCounter {
    path: PathBuf,
    /// Serialises read-modify-write within one process. Two threads issuing
    /// into the same conversation concurrently must not read the same mark and
    /// hand out the same number.
    gate: Mutex<()>,
}

impl FileSequenceCounter {
    /// The conventional file name inside a store directory.
    pub const FILE_NAME: &'static str = "sequence.counter";

    /// The header every version of this file carries.
    const HEADER: SchemaHeader = SchemaHeader::new("distro-sequence-counter", 1);

    const BROADCAST_TAG: &'static str = "broadcast";
    const DIRECT_TAG: &'static str = "direct";

    /// A counter keeping its marks at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            gate: Mutex::new(()),
        }
    }

    /// Where this counter keeps its marks.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<BTreeMap<ConversationId, SequenceNumber>, SequenceCounterError> {
        let Some(body) = read_versioned(&self.path, &Self::HEADER).map_err(to_port_error)? else {
            return Ok(BTreeMap::new());
        };

        let mut marks = BTreeMap::new();

        for line in &body {
            let (conversation, mark) =
                Self::parse_mark(line).ok_or(SequenceCounterError::Unavailable)?;

            if marks.insert(conversation, mark).is_some() {
                return Err(SequenceCounterError::Unavailable);
            }
        }

        Ok(marks)
    }

    fn write(
        &self,
        marks: &BTreeMap<ConversationId, SequenceNumber>,
    ) -> Result<(), SequenceCounterError> {
        let body: Vec<String> = marks
            .iter()
            .map(|(conversation, mark)| Self::render_mark(*conversation, *mark))
            .collect();

        write_versioned(&self.path, &Self::HEADER, &body).map_err(to_port_error)
    }

    fn render_mark(conversation: ConversationId, mark: SequenceNumber) -> String {
        match conversation {
            ConversationId::Broadcast => format!("{} {}", Self::BROADCAST_TAG, mark.as_u64()),
            ConversationId::Direct(peer) => format!(
                "{} {} {}",
                Self::DIRECT_TAG,
                hex_bytes::encode(peer.as_bytes()),
                mark.as_u64()
            ),
        }
    }

    fn parse_mark(line: &str) -> Option<(ConversationId, SequenceNumber)> {
        let mut fields = line.split(' ');

        let conversation = match fields.next()? {
            tag if tag == Self::BROADCAST_TAG => ConversationId::Broadcast,
            tag if tag == Self::DIRECT_TAG => {
                let peer =
                    PeerId::from_public_key_bytes(hex_bytes::decode(fields.next()?)?).ok()?;
                ConversationId::Direct(peer)
            }
            _ => return None,
        };

        // `SequenceNumber::new` is what rejects 0: an absent line already says
        // "no message yet", so a stored 0 is a file this build did not write.
        let mark = SequenceNumber::new(fields.next()?.parse().ok()?).ok()?;

        if fields.next().is_some() {
            return None;
        }

        Some((conversation, mark))
    }
}

impl SequenceCounterPort for FileSequenceCounter {
    fn issue_next(
        &self,
        conversation: ConversationId,
    ) -> Result<SequenceNumber, SequenceCounterError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        let mut marks = self.read()?;
        let next = SequenceNumber::following(marks.get(&conversation).copied())
            .map_err(|_| SequenceCounterError::Exhausted)?;

        marks.insert(conversation, next);

        // The advance reaches the disk before the caller sees the number.
        // Everything about this port is this line's ordering.
        self.write(&marks)?;

        Ok(next)
    }

    fn last_issued(
        &self,
        conversation: ConversationId,
    ) -> Result<Option<SequenceNumber>, SequenceCounterError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        Ok(self.read()?.get(&conversation).copied())
    }
}

impl std::fmt::Debug for FileSequenceCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSequenceCounter")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Translates a file failure into the port's vocabulary.
///
/// Both `Unreadable` and `Corrupt` become `Unavailable`: the port has no
/// corruption variant, and "cannot be reached" is the honest reading of a file
/// whose contents this build cannot make sense of. Either way the caller must
/// not send, which is the decision the variant drives.
const fn to_port_error(error: LocalFileError) -> SequenceCounterError {
    match error {
        LocalFileError::Unreadable | LocalFileError::Corrupt => SequenceCounterError::Unavailable,
        LocalFileError::UnsupportedSchemaVersion { found } => {
            SequenceCounterError::UnsupportedSchemaVersion { found }
        }
        LocalFileError::WriteFailed => SequenceCounterError::NotPersisted,
    }
}
