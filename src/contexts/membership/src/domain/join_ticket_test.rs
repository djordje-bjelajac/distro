use shared_types::ProtocolVersion;

use crate::domain::{DurationMillis, Endpoint, JoinTicket, JoinTicketError, Millis};
use crate::test_peers;

const EXPIRES_AT: Millis = Millis::from_millis(100_000);

fn endpoints() -> Vec<Endpoint> {
    vec![Endpoint::direct("/ip4/198.51.100.7/udp/4001/quic-v1").unwrap()]
}

fn ticket_expiring_at(expires_at: Millis) -> JoinTicket {
    JoinTicket::new(
        test_peers::alice(),
        endpoints(),
        ProtocolVersion::CURRENT,
        expires_at,
    )
    .expect("a ticket with one endpoint is well formed")
}

#[test]
fn carries_the_issuer_its_endpoints_the_protocol_version_and_an_expiry() {
    let ticket = ticket_expiring_at(EXPIRES_AT);

    assert_eq!(ticket.issuer(), test_peers::alice());
    assert_eq!(ticket.endpoints(), endpoints().as_slice());
    assert_eq!(ticket.protocol(), ProtocolVersion::CURRENT);
    assert_eq!(ticket.expires_at(), EXPIRES_AT);
}

#[test]
fn rejects_a_ticket_with_no_endpoints() {
    // A ticket exists to be dialled (D1); with nothing to dial it is not a
    // bootstrap credential at all.
    let ticket = JoinTicket::new(
        test_peers::alice(),
        Vec::new(),
        ProtocolVersion::CURRENT,
        EXPIRES_AT,
    );

    assert_eq!(ticket, Err(JoinTicketError::NoEndpoints));
}

#[test]
fn is_valid_strictly_before_its_expiry() {
    let ticket = ticket_expiring_at(EXPIRES_AT);

    assert_eq!(
        ticket.validate(
            Millis::from_millis(EXPIRES_AT.as_millis() - 1),
            ProtocolVersion::CURRENT
        ),
        Ok(())
    );
}

#[test]
fn is_expired_at_exactly_its_expiry_instant() {
    // Validity is the half-open interval [issued, expires_at): the boundary
    // instant belongs to the expired side, so two peers whose clocks agree
    // never disagree about a ticket at its own expiry.
    let ticket = ticket_expiring_at(EXPIRES_AT);

    assert_eq!(
        ticket.validate(EXPIRES_AT, ProtocolVersion::CURRENT),
        Err(JoinTicketError::Expired {
            expires_at: EXPIRES_AT,
            now: EXPIRES_AT,
        })
    );
    assert!(ticket.is_expired(EXPIRES_AT));
}

#[test]
fn is_expired_after_its_expiry_instant() {
    let ticket = ticket_expiring_at(EXPIRES_AT);
    let now = Millis::from_millis(EXPIRES_AT.as_millis() + 1);

    assert_eq!(
        ticket.validate(now, ProtocolVersion::CURRENT),
        Err(JoinTicketError::Expired {
            expires_at: EXPIRES_AT,
            now,
        })
    );
}

#[test]
fn rejects_a_ticket_from_an_incompatible_major_version() {
    let ticket = JoinTicket::new(
        test_peers::alice(),
        endpoints(),
        ProtocolVersion::new(2, 0),
        EXPIRES_AT,
    )
    .expect("well formed but from a future protocol");

    assert_eq!(
        ticket.validate(Millis::ZERO, ProtocolVersion::CURRENT),
        Err(JoinTicketError::IncompatibleProtocol {
            ticket: ProtocolVersion::new(2, 0),
            supported: ProtocolVersion::CURRENT,
        })
    );
}

#[test]
fn accepts_a_ticket_whose_minor_version_is_newer_than_ours() {
    // Same major: S2 says tolerate, so a ticket minted by a slightly newer
    // peer still bootstraps us (AC14).
    let newer_minor = ProtocolVersion::new(ProtocolVersion::CURRENT.major, 9);
    let ticket = JoinTicket::new(test_peers::alice(), endpoints(), newer_minor, EXPIRES_AT)
        .expect("well formed");

    assert_eq!(
        ticket.validate(Millis::ZERO, ProtocolVersion::CURRENT),
        Ok(())
    );
}

#[test]
fn expiry_is_reported_before_protocol_incompatibility() {
    // Pins the diagnostic a user sees for a stale ticket from an old peer:
    // "expired" is the actionable one — ask for a fresh ticket.
    let ticket = JoinTicket::new(
        test_peers::alice(),
        endpoints(),
        ProtocolVersion::new(2, 0),
        EXPIRES_AT,
    )
    .expect("well formed");

    assert_eq!(
        ticket.validate(EXPIRES_AT, ProtocolVersion::CURRENT),
        Err(JoinTicketError::Expired {
            expires_at: EXPIRES_AT,
            now: EXPIRES_AT,
        })
    );
}

#[test]
fn expiring_after_derives_the_expiry_from_the_issuing_instant() {
    let issued_at = Millis::from_millis(5_000);

    let ticket = JoinTicket::expiring_after(
        test_peers::alice(),
        endpoints(),
        ProtocolVersion::CURRENT,
        issued_at,
        JoinTicket::DEFAULT_LIFETIME,
    )
    .expect("well formed");

    assert_eq!(
        ticket.expires_at(),
        issued_at.saturating_add(JoinTicket::DEFAULT_LIFETIME)
    );
}

#[test]
fn the_default_lifetime_is_bounded_but_long_enough_to_share_out_of_band() {
    assert!(JoinTicket::DEFAULT_LIFETIME >= DurationMillis::from_secs(60 * 60));
    assert!(JoinTicket::DEFAULT_LIFETIME <= DurationMillis::from_secs(7 * 24 * 60 * 60));
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            JoinTicketError::NoEndpoints,
            "join ticket carries no endpoints to dial",
        ),
        (
            JoinTicketError::Expired {
                expires_at: Millis::from_millis(10),
                now: Millis::from_millis(25),
            },
            "join ticket expired at 10ms and it is now 25ms",
        ),
        (
            JoinTicketError::IncompatibleProtocol {
                ticket: ProtocolVersion::new(2, 1),
                supported: ProtocolVersion::new(1, 0),
            },
            "join ticket speaks protocol 2.1 and this build speaks 1.0",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
