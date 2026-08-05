use membership::ports::ClockPort as MembershipClock;
use messaging::ports::ClockPort as MessagingClock;

use crate::composition::SystemClock;

#[test]
fn readings_start_at_the_anchor() {
    let clock = SystemClock::anchored_to(1_700_000_000_000);

    let reading = clock.epoch_millis();

    assert!(reading >= 1_700_000_000_000);
    // Construction and one read cannot plausibly take a second.
    assert!(reading < 1_700_000_001_000);
}

#[test]
fn readings_never_decrease() {
    // `membership::ports::ClockPort`'s stated contract, which is what makes
    // every presence age meaningful.
    let clock = SystemClock::anchored_to(0);

    let mut previous = clock.epoch_millis();
    for _ in 0..1_000 {
        let reading = clock.epoch_millis();
        assert!(reading >= previous, "{reading} < {previous}");
        previous = reading;
    }
}

#[test]
fn both_contexts_read_the_same_instant() {
    // A roster ageing presence and a conversation ageing a gap must never
    // disagree about what time it is (canvas §4: one implementation behind
    // both ports).
    let clock = SystemClock::anchored_to(5_000);

    let membership = MembershipClock::now(&clock).as_millis();
    let messaging = MessagingClock::now(&clock).as_millis();

    assert!(messaging >= membership);
    assert!(messaging - membership < 50);
}

#[test]
fn the_origin_is_the_unix_epoch_so_two_peers_can_compare_a_ticket() {
    // The whole reason the origin is not left unspecified: a ticket minted on
    // one machine is validated against another machine's clock.
    let clock = SystemClock::now();

    let reading = clock.epoch_millis();

    // Somewhere after 2020 and before 2100 — enough to prove the reading is an
    // epoch reading and not a process-relative counter.
    assert!(reading > 1_577_836_800_000, "{reading} predates 2020");
    assert!(reading < 4_102_444_800_000, "{reading} is past 2100");
}

#[test]
fn a_ticket_minted_now_has_not_expired_yet() {
    use membership::domain::{Endpoint, JoinTicket};
    use shared_types::ProtocolVersion;

    let clock = SystemClock::anchored_to(1_700_000_000_000);
    let issuer = crate::test_peers::alice();
    let now = MembershipClock::now(&clock);

    let ticket = JoinTicket::expiring_after(
        issuer,
        vec![Endpoint::direct("/ip4/10.0.0.1/udp/1/quic-v1").expect("an address")],
        ProtocolVersion::CURRENT,
        now,
        JoinTicket::DEFAULT_LIFETIME,
    )
    .expect("one endpoint");

    assert!(!ticket.is_expired(MembershipClock::now(&clock)));
}
