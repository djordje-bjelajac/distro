use shared_types::PeerId;

use crate::domain::DisplayName;

/// The local peer's display name changed from `previous` to `current`.
///
/// Carries both sides so a subscriber can render the change without holding
/// prior state. Not emitted when a caller sets the name it already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayNameChanged {
    pub peer: PeerId,
    pub previous: DisplayName,
    pub current: DisplayName,
}
