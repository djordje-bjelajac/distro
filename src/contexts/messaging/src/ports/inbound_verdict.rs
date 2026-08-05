use shared_types::PayloadKind;

use crate::domain::MessagePlacement;
use crate::domain::events::{MessageRejected, RejectionReason};

/// What the inbound boundary did with one envelope (S3).
///
/// Three outcomes, because the boundary can end an envelope's journey in three
/// genuinely different ways and a caller counting diagnostics must be able to
/// tell them apart (AC6, AC14, AC15):
///
/// * [`RefusedAtBoundary`](Self::RefusedAtBoundary) — the envelope failed a
///   check *before* any aggregate was touched: version, signature, block list,
///   or payload. Nothing reached the read model, which is what invariant 10
///   requires.
/// * [`Ignored`](Self::Ignored) — a well-formed envelope carrying a payload
///   kind this build does not act on. S2's tolerance rule: counted locally,
///   never treated as an error, because peers upgrade independently and a
///   newer peer's new kind must not look like an attack (AC14).
/// * [`Judged`](Self::Judged) — it reached the conversation, and the
///   [`MessagePlacement`] says what happened there.
///
/// None of the three is "dropped". That is the point: AC11 and AC15 make
/// silent loss a non-state in both directions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundVerdict {
    /// Refused at the boundary, with the reason a diagnostic will report.
    RefusedAtBoundary(MessageRejected),
    /// A payload kind this build does not handle; tolerated and counted (S2).
    Ignored(PayloadKind),
    /// The conversation judged it (invariant 5 rule R, invariant 6).
    Judged(MessagePlacement),
}

impl InboundVerdict {
    /// Whether the message became part of a conversation.
    pub const fn is_applied(&self) -> bool {
        matches!(self, Self::Judged(MessagePlacement::Applied(_)))
    }

    /// Whether it is being held until a gap closes. Held messages are visible
    /// to nothing — not to a query, not to the log.
    pub const fn is_buffered(&self) -> bool {
        matches!(self, Self::Judged(MessagePlacement::Buffered { .. }))
    }

    /// Whether it changed nothing because it had already been seen
    /// (invariant 6, AC7).
    pub const fn is_duplicate(&self) -> bool {
        matches!(self, Self::Judged(MessagePlacement::DuplicateIgnored(_)))
    }

    /// Whether it was refused anywhere — at the boundary or by the
    /// conversation.
    pub const fn is_refused(&self) -> bool {
        matches!(
            self,
            Self::RefusedAtBoundary(_) | Self::Judged(MessagePlacement::Rejected(_))
        )
    }

    /// Why it was refused, for the local diagnostic counters AC6 and AC15 ask
    /// for; `None` when it was not refused.
    pub const fn rejection_reason(&self) -> Option<RejectionReason> {
        match self {
            Self::RefusedAtBoundary(rejected)
            | Self::Judged(MessagePlacement::Rejected(rejected)) => Some(rejected.reason),
            _ => None,
        }
    }
}
