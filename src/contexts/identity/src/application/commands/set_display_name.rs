use std::sync::Arc;

use crate::application::LocalIdentityState;
use crate::domain::DisplayName;
use crate::domain::events::DisplayNameChanged;
use crate::ports::SetDisplayNameError;

/// Rename the local peer.
///
/// Carries the raw text a user typed rather than a validated
/// [`DisplayName`]: validation is this use case's job, so a caller cannot
/// bypass it and every rejection comes back as one typed error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDisplayName {
    /// The name as entered, before trimming or validation.
    pub requested: String,
}

impl SetDisplayName {
    pub fn new(requested: impl Into<String>) -> Self {
        Self {
            requested: requested.into(),
        }
    }
}

/// Handles [`SetDisplayName`]: validates through the value object, then asks
/// the aggregate to change.
///
/// Two "nothing happened" cases stay distinct. An unusable name is a typed
/// rejection ([`SetDisplayNameError::Invalid`]) because the user must be told
/// to pick another; setting the name the peer already has is `Ok(None)`,
/// because the post-condition holds and announcing a change that did not occur
/// would be a lie to every subscriber. Since [`DisplayName`] stores its text
/// trimmed, differently padded spellings of one name are the same name here.
///
/// Validation runs before the identity is consulted: whether text is a legal
/// display name is a property of the text alone, and answering it the same way
/// regardless of bootstrap order keeps the error a caller sees deterministic.
///
/// The name never touches the keypair or the `PeerId` (invariant 8), so this
/// command reaches no port at all — it is pure state plus the value object.
#[derive(Clone)]
pub struct SetDisplayNameHandler {
    state: Arc<LocalIdentityState>,
}

impl SetDisplayNameHandler {
    pub fn new(state: Arc<LocalIdentityState>) -> Self {
        Self { state }
    }

    pub fn handle(
        &self,
        command: SetDisplayName,
    ) -> Result<Option<DisplayNameChanged>, SetDisplayNameError> {
        let display_name = DisplayName::new(&command.requested)?;

        self.state
            .modify(|identity| identity.change_display_name(display_name))
            .ok_or(SetDisplayNameError::NotInitialized)
    }
}
