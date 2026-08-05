use std::collections::HashMap;

use libp2p::PeerId;

/// The per-peer inbound rate limit of S6, as a token bucket per remote peer.
///
/// # Why a bucket and not a counter
///
/// A fixed window lets a peer send two full windows' worth of traffic across a
/// window boundary, which is the burst the limit exists to stop. A token bucket
/// admits a genuine reconnect burst (a peer coming back with a heartbeat, an
/// acknowledgement, and a message in the same instant) and then holds it to the
/// sustained rate, which is exactly the shape of honest traffic.
///
/// # Time is an argument, not a call
///
/// Nothing here reads a clock (S5). The caller passes the monotonic instant it
/// already has, so this is a pure state machine a unit test drives by hand —
/// the same discipline the domain follows with `Millis`.
#[derive(Debug)]
pub struct InboundRateLimiter {
    capacity: u64,
    /// Tokens refilled per millisecond, scaled by [`Self::SCALE`] so the
    /// refill of a sub-millisecond rate is not silently rounded to zero.
    refill_per_milli_scaled: u64,
    buckets: HashMap<PeerId, Bucket>,
}

/// One peer's allowance.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    /// Tokens available, scaled by [`InboundRateLimiter::SCALE`].
    tokens_scaled: u64,
    last_refill_millis: u64,
}

impl InboundRateLimiter {
    /// Fixed-point scale for token arithmetic.
    ///
    /// Rates are per second but time arrives in milliseconds, so an integer
    /// "tokens per millisecond" would floor every rate below 1000/s to zero and
    /// silently refuse all traffic. Scaling by a million keeps a rate as low as
    /// one envelope per thousand seconds exact in `u64`.
    const SCALE: u64 = 1_000_000;

    /// Largest number of peers tracked at once.
    ///
    /// The map is itself an attack surface: a stranger who dials once from
    /// each of many identities would otherwise make this process hold an entry
    /// per identity forever. When the map is full, the least recently seen
    /// entry is evicted — which only ever *restores* a peer's full allowance,
    /// so eviction can never wrongly refuse traffic.
    const MAX_TRACKED_PEERS: usize = 4_096;

    /// A limiter admitting `burst` envelopes at once and `per_second`
    /// sustained.
    pub fn new(per_second: u32, burst: u32) -> Self {
        Self {
            capacity: u64::from(burst.max(1)) * Self::SCALE,
            refill_per_milli_scaled: (u64::from(per_second.max(1)) * Self::SCALE) / 1_000,
            buckets: HashMap::new(),
        }
    }

    /// Whether one more inbound envelope from `peer` is admitted at `now`.
    ///
    /// Spends a token on success and nothing on refusal, so a peer that is
    /// over its rate does not push its own recovery further away.
    pub fn admit(&mut self, peer: PeerId, now_millis: u64) -> bool {
        self.evict_if_full(now_millis);

        let capacity = self.capacity;
        let refill = self.refill_per_milli_scaled;
        let bucket = self.buckets.entry(peer).or_insert(Bucket {
            tokens_scaled: capacity,
            last_refill_millis: now_millis,
        });

        let elapsed = now_millis.saturating_sub(bucket.last_refill_millis);
        bucket.tokens_scaled = bucket
            .tokens_scaled
            .saturating_add(elapsed.saturating_mul(refill))
            .min(capacity);
        bucket.last_refill_millis = now_millis;

        if bucket.tokens_scaled >= Self::SCALE {
            bucket.tokens_scaled -= Self::SCALE;
            true
        } else {
            false
        }
    }

    /// Forgets `peer`'s allowance, restoring a full bucket on its next
    /// envelope. Called when the last link to a peer closes.
    pub fn forget(&mut self, peer: &PeerId) {
        self.buckets.remove(peer);
    }

    /// How many peers currently have an allowance recorded.
    pub fn tracked_peers(&self) -> usize {
        self.buckets.len()
    }

    /// Drops the least recently seen entry when the table is full.
    fn evict_if_full(&mut self, now_millis: u64) {
        if self.buckets.len() < Self::MAX_TRACKED_PEERS {
            return;
        }

        let stalest = self
            .buckets
            .iter()
            .filter(|(_, bucket)| bucket.last_refill_millis < now_millis)
            .min_by_key(|(peer, bucket)| (bucket.last_refill_millis, **peer))
            .map(|(peer, _)| *peer);

        if let Some(peer) = stalest {
            self.buckets.remove(&peer);
        }
    }
}
