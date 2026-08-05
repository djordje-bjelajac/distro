use libp2p::PeerId;

use crate::limits::InboundRateLimiter;

fn peer() -> PeerId {
    PeerId::random()
}

#[test]
fn admits_a_full_burst_at_one_instant() {
    let mut limiter = InboundRateLimiter::new(4, 8);
    let peer = peer();

    for index in 0..8 {
        assert!(
            limiter.admit(peer, 0),
            "envelope {index} should be admitted"
        );
    }
}

#[test]
fn refuses_once_the_burst_is_spent() {
    let mut limiter = InboundRateLimiter::new(4, 8);
    let peer = peer();

    for _ in 0..8 {
        assert!(limiter.admit(peer, 0));
    }

    assert!(!limiter.admit(peer, 0));
}

#[test]
fn refusal_does_not_spend_a_token() {
    let mut limiter = InboundRateLimiter::new(1_000, 1);
    let peer = peer();

    assert!(limiter.admit(peer, 0));
    assert!(!limiter.admit(peer, 0));

    // One millisecond refills exactly one token at 1000/s; had the refusal
    // spent a token, this would still be refused.
    assert!(limiter.admit(peer, 1));
}

#[test]
fn refills_at_the_sustained_rate() {
    let mut limiter = InboundRateLimiter::new(10, 1);
    let peer = peer();

    assert!(limiter.admit(peer, 0));
    assert!(!limiter.admit(peer, 50), "50 ms is half a token at 10/s");
    assert!(limiter.admit(peer, 100), "100 ms is one token at 10/s");
}

#[test]
fn refill_is_capped_at_the_burst_size() {
    let mut limiter = InboundRateLimiter::new(10, 2);
    let peer = peer();

    assert!(limiter.admit(peer, 0));
    assert!(limiter.admit(peer, 0));

    // An hour of silence must not bank an hour of tokens.
    assert!(limiter.admit(peer, 3_600_000));
    assert!(limiter.admit(peer, 3_600_000));
    assert!(!limiter.admit(peer, 3_600_000));
}

#[test]
fn one_peers_flood_does_not_touch_another_peers_allowance() {
    let mut limiter = InboundRateLimiter::new(1, 2);
    let flooder = peer();
    let quiet = peer();

    assert!(limiter.admit(flooder, 0));
    assert!(limiter.admit(flooder, 0));
    assert!(!limiter.admit(flooder, 0));

    assert!(limiter.admit(quiet, 0));
    assert!(limiter.admit(quiet, 0));
}

#[test]
fn forgetting_a_peer_restores_its_full_allowance() {
    let mut limiter = InboundRateLimiter::new(1, 1);
    let peer = peer();

    assert!(limiter.admit(peer, 0));
    assert!(!limiter.admit(peer, 0));

    limiter.forget(&peer);

    assert!(limiter.admit(peer, 0));
    assert_eq!(limiter.tracked_peers(), 1);
}

#[test]
fn a_clock_that_ran_backwards_does_not_grant_tokens() {
    let mut limiter = InboundRateLimiter::new(1_000, 1);
    let peer = peer();

    assert!(limiter.admit(peer, 10_000));
    assert!(
        !limiter.admit(peer, 0),
        "a smaller reading must not refill the bucket"
    );
}

#[test]
fn a_sub_millisecond_rate_still_refills() {
    // One envelope per two seconds: an integer tokens-per-millisecond would
    // floor this to zero and refuse the peer forever.
    let mut limiter = InboundRateLimiter::new(1, 1);
    let peer = peer();

    assert!(limiter.admit(peer, 0));
    assert!(!limiter.admit(peer, 500));
    assert!(limiter.admit(peer, 1_000));
}
