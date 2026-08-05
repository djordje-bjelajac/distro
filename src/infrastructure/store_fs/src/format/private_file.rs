//! Creating and replacing files only the owner can read.
//!
//! # Owner-only, from the moment the file exists
//!
//! On unix every file this crate writes is created with mode `0600` through
//! [`OpenOptionsExt::mode`](std::os::unix::fs::OpenOptionsExt::mode), so the
//! secret seed is never briefly world-readable between `open` and a later
//! `chmod`, and the mode is re-asserted afterwards in case a pre-existing file
//! carried looser bits. The store directory is `0700` for the same reason.
//!
//! **On non-unix targets there is no equivalent and this crate does not
//! pretend otherwise.** `std::fs` exposes no portable ACL API, so a file
//! created on Windows inherits the directory's ACL, which for a per-user
//! profile directory is normally owner-plus-administrators. That is weaker than
//! `0600` and it is stated here rather than silently assumed: a Windows port
//! must set a DACL explicitly (`CreateFileW` with a `SECURITY_ATTRIBUTES`, or
//! the `windows-acl` route) before the keystore can claim the same protection
//! it claims on unix. Nothing else in the crate changes.
//!
//! # Two ways to put bytes on disk, and they are not interchangeable
//!
//! * [`replace_atomically`] — temp file in the same directory, fsync, rename.
//!   Used by every store whose file is *rewritten*: trust records, peer cache,
//!   sequence counter. A crash leaves either the whole old file or the whole
//!   new one.
//! * [`create_exclusively`] — `O_EXCL` create at the final path. Used by the
//!   keystore alone, because a rename would **clobber**: two processes racing
//!   on one profile directory would each publish their own keypair and the
//!   loser would silently change identity. An exclusive create makes the race
//!   decidable — the loser sees `AlreadyExists`, re-reads, and adopts the
//!   identity that won (AC9). The residual risk is a crash inside the single
//!   `write_all` of a ~100-byte file, which surfaces as a `Corrupt` keystore on
//!   the next launch: losing an identity loudly is better than minting a second
//!   one silently.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

/// Mode of every file this crate writes on unix: owner read/write, nothing
/// else.
#[cfg(unix)]
pub(crate) const OWNER_ONLY_FILE: u32 = 0o600;

/// Mode of the store directory on unix: owner traverse/read/write, nothing
/// else. A directory others can list leaks the set of peers this machine has
/// talked to even when the files themselves are unreadable.
#[cfg(unix)]
pub(crate) const OWNER_ONLY_DIRECTORY: u32 = 0o700;

/// Distinguishes the temp files of concurrent writers within one process.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Creates `path` with `contents`, failing if anything is already there.
///
/// The exclusive create is the point: see the module docs on why the keystore
/// must not be published by rename.
pub(crate) fn create_exclusively(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    options.mode(OWNER_ONLY_FILE);

    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);

    harden(path)?;
    sync_parent_directory(path);
    Ok(())
}

/// Replaces `path` with `contents`, atomically.
///
/// The temp file lives in the same directory so the rename stays within one
/// filesystem — a cross-device rename is not atomic and, on most platforms, not
/// even permitted. It is fsynced before the rename, so the rename can never
/// publish a file whose contents have not reached the disk.
pub(crate) fn replace_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temp = temp_path(path)?;

    if let Err(error) = write_temp(&temp, contents) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temp, path) {
        // The old file is still the live one and the temp is worthless; leaving
        // it behind would accumulate rubbish in a directory the user never
        // looks at.
        let _ = fs::remove_file(&temp);
        return Err(error);
    }

    sync_parent_directory(path);
    Ok(())
}

/// Creates the store directory if it is not there, owner-only on unix.
pub(crate) fn create_owner_only_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY_DIRECTORY))?;

    Ok(())
}

fn write_temp(temp: &Path, contents: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    options.mode(OWNER_ONLY_FILE);

    // `create` rather than `create_new`: a temp left behind by a process that
    // crashed mid-write is not live data and clobbering it is safe, while
    // refusing to write because of one would break every later save.
    let mut file = options.open(temp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);

    harden(temp)
}

/// Re-asserts owner-only permissions on a file that already exists.
#[cfg(unix)]
fn harden(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY_FILE))
}

/// See the module docs: no portable equivalent exists off unix.
#[cfg(not(unix))]
fn harden(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Pushes the directory entry itself to disk, best effort.
///
/// Deliberately not an error path. The contents are already durable — the file
/// was fsynced before the rename — and what this adds is that the *name* points
/// at them after a power loss. Some filesystems and some platforms refuse to
/// sync a directory handle at all, and turning that refusal into a failed save
/// would report "not persisted" for a write that did land, which for the
/// sequence counter is the more expensive lie: a skipped number costs nothing,
/// a re-issued one silences a peer (D12).
fn sync_parent_directory(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
}

fn temp_path(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a store file needs a parent directory to write its temp file in",
        )
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a store path needs a name"))?
        .to_string_lossy()
        .into_owned();

    // Process id and a per-process counter, so two writers never collide and a
    // temp file is always recognisable as one.
    let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!("{name}.tmp-{}-{unique}", std::process::id())))
}
