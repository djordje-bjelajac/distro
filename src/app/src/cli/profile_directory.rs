use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Where one instance keeps everything that outlives its process: the keypair
/// (D5), the trust records, the peer cache (D1 rung a), and the outbound
/// sequence counter (D12).
///
/// # The order, and why there are three ways to say it
///
/// 1. `--profile <DIR>` — the command-line flag wins over everything.
/// 2. `DISTRO_PROFILE_DIR` — the environment variable.
/// 3. `$XDG_DATA_HOME/distro`, or `$HOME/.local/share/distro` when
///    `XDG_DATA_HOME` is unset.
///
/// A default alone would be wrong for the one thing this project has to be able
/// to do before it can claim anything: run **two instances on one machine** and
/// have them talk to each other. Two instances sharing a profile directory
/// would share a `PeerId`, which is not two peers — and they would race on the
/// sequence counter, which is worse than sharing an identity because it is
/// silent. So an override exists, twice: a flag for a single run, an
/// environment variable for a shell that starts several. OP-13's manual
/// protocol needs exactly this.
///
/// # Why `$XDG_DATA_HOME` and not the macOS convention
///
/// One code path on every unix, including macOS, rather than a per-platform
/// lookup crate for a single directory name. The cost is that a mac user's
/// files land in `~/.local/share/distro` instead of `~/Library/Application
/// Support`; the benefit is that the resolution is a pure function of four
/// strings, so it is unit-tested here rather than asserted about a machine.
/// `data` rather than `config`: none of these files is something a user edits,
/// and losing them loses an identity (S4).
///
/// # This is not a secret store
///
/// The directory is created owner-only and the key file inside it `0600` — see
/// `LocalStores` — but a profile directory is still an ordinary directory on an
/// ordinary disk. Nothing here is encrypted at rest, and the canvas does not
/// claim it is.
pub struct ProfileDirectory;

impl ProfileDirectory {
    /// The environment variable that overrides the default location.
    pub const ENVIRONMENT_VARIABLE: &'static str = "DISTRO_PROFILE_DIR";

    /// The directory name appended to the platform data directory.
    const APPLICATION_DIRECTORY: &'static str = "distro";

    /// The XDG-relative fallback used when `XDG_DATA_HOME` is unset.
    const HOME_RELATIVE_DATA_DIRECTORY: &'static str = ".local/share";

    /// Applies the precedence above to one already-read environment.
    ///
    /// Pure: it reads nothing, creates nothing, and touches no disk, so the
    /// whole rule is exercised by unit tests rather than by a machine's actual
    /// `$HOME` (S5).
    pub fn resolve(
        flag: Option<&Path>,
        environment: &ProfileEnvironment,
    ) -> Result<PathBuf, ProfileDirectoryError> {
        if let Some(path) = flag {
            return non_empty(path.as_os_str()).ok_or(ProfileDirectoryError::EmptyFlag);
        }

        if let Some(path) = environment.override_path.as_deref().and_then(non_empty) {
            return Ok(path);
        }

        if let Some(data_home) = environment.xdg_data_home.as_deref().and_then(non_empty) {
            return Ok(data_home.join(Self::APPLICATION_DIRECTORY));
        }

        let home = environment
            .home
            .as_deref()
            .and_then(non_empty)
            .ok_or(ProfileDirectoryError::NoHomeDirectory)?;

        Ok(home
            .join(Self::HOME_RELATIVE_DATA_DIRECTORY)
            .join(Self::APPLICATION_DIRECTORY))
    }
}

/// The three environment values [`ProfileDirectory::resolve`] consults, read
/// once so the rule itself stays pure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileEnvironment {
    /// `DISTRO_PROFILE_DIR`.
    pub override_path: Option<OsString>,
    /// `XDG_DATA_HOME`.
    pub xdg_data_home: Option<OsString>,
    /// `HOME`.
    pub home: Option<OsString>,
}

impl ProfileEnvironment {
    /// Reads the three variables from this process's environment.
    pub fn from_process() -> Self {
        Self {
            override_path: std::env::var_os(ProfileDirectory::ENVIRONMENT_VARIABLE),
            xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
            home: std::env::var_os("HOME"),
        }
    }
}

/// An empty value is treated as unset.
///
/// `DISTRO_PROFILE_DIR=` in a shell script is an accident, not a request to put
/// a profile at the filesystem root, and resolving it to `""` would create the
/// stores relative to the working directory of whoever launched the process.
fn non_empty(value: &std::ffi::OsStr) -> Option<PathBuf> {
    (!value.is_empty()).then(|| PathBuf::from(value))
}

/// Why no profile directory could be named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDirectoryError {
    /// `--profile` was given with an empty value.
    EmptyFlag,
    /// Neither an override nor `XDG_DATA_HOME` nor `HOME` is set, so there is
    /// no per-user location to derive. Stating this beats guessing at the
    /// working directory and scattering identities across wherever the process
    /// happened to start.
    NoHomeDirectory,
}

impl fmt::Display for ProfileDirectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFlag => f.write_str("--profile was given an empty path"),
            Self::NoHomeDirectory => write!(
                f,
                "no profile directory could be determined: set {} or HOME",
                ProfileDirectory::ENVIRONMENT_VARIABLE
            ),
        }
    }
}

impl std::error::Error for ProfileDirectoryError {}
