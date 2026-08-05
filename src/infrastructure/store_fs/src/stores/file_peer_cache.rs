use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use membership::domain::{Endpoint, Millis, Reachability};
use membership::ports::{CachedPeer, PeerCacheError, PeerCachePort};
use shared_types::PeerId;

use crate::format::{LocalFileError, SchemaHeader, hex_bytes, read_versioned, write_versioned};

/// The known-peer set that makes the next launch a warm start (D1, rung (a)).
///
/// # The file
///
/// ```text
/// distro-peer-cache 1
/// peer <64 hex characters: peer id> <last seen, millis> <endpoint count>
/// endpoint <direct|relayed> <address to the end of the line>
/// endpoint …
/// peer …
/// ```
///
/// Each `peer` line is followed by exactly the number of `endpoint` lines it
/// declares; anything else is corruption. The count is what makes the grouping
/// explicit rather than inferred from indentation or blank lines, both of which
/// a truncated file can imitate.
///
/// The address is the **last field and runs to the end of its line**, so it
/// needs no escaping: an [`Endpoint`] rejects control characters, and therefore
/// can never contain a newline, while it may perfectly well contain a space.
/// Every address is re-validated through [`Endpoint::new`] on the way in, so a
/// damaged file cannot inject an address the domain would have refused (S3 in
/// spirit — this file is local, but it is still input).
///
/// Order is preserved exactly as [`save`](PeerCachePort::save) received it:
/// the roster decides which peers are worth keeping and in what order to try
/// them, and a cache that re-sorted them would be quietly overriding the
/// bootstrap ladder.
///
/// # Replace, never merge
///
/// The port says so and the reason is that an append-only cache could never
/// forget a peer. Pruning is the roster's decision (it holds last-seen); this
/// store writes down whatever it is handed.
pub struct FilePeerCache {
    path: PathBuf,
    /// Serialises writes within one process.
    gate: Mutex<()>,
}

impl FilePeerCache {
    /// The conventional file name inside a store directory.
    pub const FILE_NAME: &'static str = "peers.cache";

    /// The header every version of this file carries.
    const HEADER: SchemaHeader = SchemaHeader::new("distro-peer-cache", 1);

    const PEER_TAG: &'static str = "peer";
    const ENDPOINT_TAG: &'static str = "endpoint";
    const DIRECT: &'static str = "direct";
    const RELAYED: &'static str = "relayed";

    /// A cache keeping its peers at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            gate: Mutex::new(()),
        }
    }

    /// Where this cache keeps its peers.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn parse(body: &[String]) -> Option<Vec<CachedPeer>> {
        let mut peers = Vec::new();
        let mut lines = body.iter();

        while let Some(line) = lines.next() {
            let mut fields = line.split(' ');

            if fields.next()? != Self::PEER_TAG {
                return None;
            }

            let peer = PeerId::from_public_key_bytes(hex_bytes::decode(fields.next()?)?).ok()?;
            let last_seen_at = Millis::from_millis(fields.next()?.parse().ok()?);
            let count: usize = fields.next()?.parse().ok()?;

            if fields.next().is_some() {
                return None;
            }

            // Grown rather than pre-sized: the count is a claim the file makes,
            // not a measurement, and reserving on it lets a two-line corrupt
            // file demand an allocation the process aborts on. Every iteration
            // has to produce a real line, so a dishonest count costs one failed
            // `next` (S6: caps hold before the data is trusted).
            let mut endpoints = Vec::new();

            for _ in 0..count {
                endpoints.push(Self::parse_endpoint(lines.next()?)?);
            }

            peers.push(CachedPeer {
                peer,
                endpoints,
                last_seen_at,
            });
        }

        Some(peers)
    }

    fn parse_endpoint(line: &str) -> Option<Endpoint> {
        let rest = line.strip_prefix(Self::ENDPOINT_TAG)?.strip_prefix(' ')?;
        let (class, address) = rest.split_once(' ')?;

        let reachability = match class {
            tag if tag == Self::DIRECT => Reachability::Direct,
            tag if tag == Self::RELAYED => Reachability::Relayed,
            _ => return None,
        };

        // Re-validated rather than trusted: a file is input too, and an
        // `Endpoint` that skipped its own constructor would be a domain value
        // nobody checked.
        Endpoint::new(address, reachability).ok()
    }

    fn render(peers: &[CachedPeer]) -> Vec<String> {
        let mut body = Vec::new();

        for cached in peers {
            body.push(format!(
                "{} {} {} {}",
                Self::PEER_TAG,
                hex_bytes::encode(cached.peer.as_bytes()),
                cached.last_seen_at.as_millis(),
                cached.endpoints.len()
            ));

            for endpoint in &cached.endpoints {
                let class = if endpoint.is_relayed() {
                    Self::RELAYED
                } else {
                    Self::DIRECT
                };

                body.push(format!(
                    "{} {class} {}",
                    Self::ENDPOINT_TAG,
                    endpoint.address()
                ));
            }
        }

        body
    }
}

impl PeerCachePort for FilePeerCache {
    fn load(&self) -> Result<Vec<CachedPeer>, PeerCacheError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        // An absent file is a cold start — exactly the case the rest of the
        // bootstrap ladder exists for — and not an error.
        let Some(body) = read_versioned(&self.path, &Self::HEADER).map_err(to_port_error)? else {
            return Ok(Vec::new());
        };

        Self::parse(&body).ok_or(PeerCacheError::Corrupt)
    }

    fn save(&self, peers: &[CachedPeer]) -> Result<(), PeerCacheError> {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        write_versioned(&self.path, &Self::HEADER, &Self::render(peers)).map_err(to_port_error)
    }
}

impl std::fmt::Debug for FilePeerCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePeerCache")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

const fn to_port_error(error: LocalFileError) -> PeerCacheError {
    match error {
        LocalFileError::Unreadable => PeerCacheError::Unreadable,
        LocalFileError::Corrupt => PeerCacheError::Corrupt,
        LocalFileError::UnsupportedSchemaVersion { found } => {
            PeerCacheError::UnsupportedSchemaVersion { found }
        }
        LocalFileError::WriteFailed => PeerCacheError::WriteFailed,
    }
}
