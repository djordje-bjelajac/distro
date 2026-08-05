use std::fmt;

use shared_types::{Compatibility, PeerId, ProtocolVersion};

use crate::domain::{DurationMillis, Endpoint, Millis};

/// A self-describing bootstrap credential any member can mint and hand to a
/// newcomer out of band (canvas §2.2, D1).
///
/// This is the honest cost of serverless reach: internet-wide first contact
/// needs *some* first peer, and every automatic mechanism for supplying one —
/// hardcoded bootstrap hosts, public rendezvous, DNS seeds — is operator-run
/// infrastructure that S1 forbids. A ticket moves that first contact to a
/// human channel the participants already have, once per machine.
///
/// # What is and is not modelled here
///
/// The ticket's **validity** is a pure function of the ticket, a clock reading,
/// and the protocol version this build speaks — so it is domain logic and lives
/// here. Its **string encoding** for copy-paste is a transport concern and
/// lives in an adapter (OP-10/OP-12): the domain never parses ticket text, and
/// a ticket only exists once its parts have already been validated
/// individually.
///
/// A ticket is a *bootstrap hint*, not an authorisation: redeeming one proves
/// nothing about the issuer beyond the fact that someone published these
/// endpoints. Authentication happens at the session handshake, and trust is a
/// separate, local `identity` concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinTicket {
    issuer: PeerId,
    endpoints: Vec<Endpoint>,
    protocol: ProtocolVersion,
    expires_at: Millis,
}

impl JoinTicket {
    /// How long a freshly minted ticket stays valid unless the caller says
    /// otherwise.
    ///
    /// An engineering default per canvas §9. Twenty-four hours is long enough
    /// to survive the human round trip a ticket is made for — paste it into a
    /// chat, the recipient reads it tomorrow morning — and short enough that a
    /// ticket leaked into a public archive stops being a live pointer to the
    /// issuer's addresses within a day. Nothing is renewed automatically: the
    /// issuer simply mints another.
    pub const DEFAULT_LIFETIME: DurationMillis = DurationMillis::from_secs(24 * 60 * 60);

    /// Assembles a ticket that expires at an absolute instant.
    ///
    /// At least one endpoint is required — a ticket with nothing to dial is not
    /// a bootstrap credential.
    pub fn new(
        issuer: PeerId,
        endpoints: Vec<Endpoint>,
        protocol: ProtocolVersion,
        expires_at: Millis,
    ) -> Result<Self, JoinTicketError> {
        if endpoints.is_empty() {
            return Err(JoinTicketError::NoEndpoints);
        }

        Ok(Self {
            issuer,
            endpoints,
            protocol,
            expires_at,
        })
    }

    /// Assembles a ticket valid for `lifetime` from `issued_at`.
    pub fn expiring_after(
        issuer: PeerId,
        endpoints: Vec<Endpoint>,
        protocol: ProtocolVersion,
        issued_at: Millis,
        lifetime: DurationMillis,
    ) -> Result<Self, JoinTicketError> {
        Self::new(
            issuer,
            endpoints,
            protocol,
            issued_at.saturating_add(lifetime),
        )
    }

    pub const fn issuer(&self) -> PeerId {
        self.issuer
    }

    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }

    pub const fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    pub const fn expires_at(&self) -> Millis {
        self.expires_at
    }

    /// Whether the ticket has reached its expiry as of `now`.
    ///
    /// Validity is the half-open interval `[issued, expires_at)`: the expiry
    /// instant itself is already expired, so the boundary belongs to exactly
    /// one side and two peers reading the same instant never disagree.
    pub const fn is_expired(&self, now: Millis) -> bool {
        now.as_millis() >= self.expires_at.as_millis()
    }

    /// Decides whether this ticket may be redeemed.
    ///
    /// Expiry is checked before protocol compatibility, which pins the
    /// diagnostic a user sees for an old ticket from an old peer: "expired" is
    /// the actionable half of that answer — ask the issuer for a fresh one —
    /// whereas "incompatible" would send them chasing an upgrade they may not
    /// need.
    ///
    /// Compatibility follows the one S2 rule in `shared_types`: a different
    /// major version is rejected, a newer minor is tolerated (AC14).
    pub fn validate(&self, now: Millis, supported: ProtocolVersion) -> Result<(), JoinTicketError> {
        if self.is_expired(now) {
            return Err(JoinTicketError::Expired {
                expires_at: self.expires_at,
                now,
            });
        }

        match Compatibility::evaluate(self.protocol, supported) {
            Compatibility::Accept | Compatibility::Tolerate => Ok(()),
            Compatibility::Reject => Err(JoinTicketError::IncompatibleProtocol {
                ticket: self.protocol,
                supported,
            }),
        }
    }
}

/// Typed rejection of a [`JoinTicket`], at construction or at redemption time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinTicketError {
    /// The ticket carries no endpoint, so there is nothing to dial.
    NoEndpoints,
    /// The ticket reached its expiry; the issuer must mint a fresh one.
    Expired { expires_at: Millis, now: Millis },
    /// The ticket's major protocol version is not the one this build speaks.
    IncompatibleProtocol {
        ticket: ProtocolVersion,
        supported: ProtocolVersion,
    },
}

impl fmt::Display for JoinTicketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEndpoints => f.write_str("join ticket carries no endpoints to dial"),
            Self::Expired { expires_at, now } => {
                write!(f, "join ticket expired at {expires_at} and it is now {now}")
            }
            Self::IncompatibleProtocol { ticket, supported } => write!(
                f,
                "join ticket speaks protocol {}.{} and this build speaks {}.{}",
                ticket.major, ticket.minor, supported.major, supported.minor
            ),
        }
    }
}

impl std::error::Error for JoinTicketError {}
