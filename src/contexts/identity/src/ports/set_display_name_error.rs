use std::fmt;

use crate::domain::DisplayNameError;

/// Typed failure of
/// [`IdentityCommandPort::set_display_name`](crate::ports::IdentityCommandPort::set_display_name).
///
/// Setting the name the peer already has is **not** an error: it is a no-op
/// that emits nothing (see `SetDisplayNameHandler`), so it is reported as
/// `Ok(None)` rather than a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetDisplayNameError {
    /// The command arrived before `InitializeLocalIdentity`; there is no peer
    /// to rename yet.
    NotInitialized,
    /// The requested text is not a valid
    /// [`DisplayName`](crate::domain::DisplayName).
    Invalid(DisplayNameError),
}

impl From<DisplayNameError> for SetDisplayNameError {
    fn from(error: DisplayNameError) -> Self {
        Self::Invalid(error)
    }
}

impl fmt::Display for SetDisplayNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => f.write_str("local identity has not been initialized yet"),
            Self::Invalid(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SetDisplayNameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotInitialized => None,
            Self::Invalid(error) => Some(error),
        }
    }
}
