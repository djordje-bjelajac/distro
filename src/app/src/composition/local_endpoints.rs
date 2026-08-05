use std::sync::Mutex;

use membership::domain::Endpoint;

/// Where this peer believes it can be reached, accumulated from what the
/// network reports.
///
/// # Why the root keeps this
///
/// Two things need it and neither can derive it:
///
/// * **Re-announcing.** `NetworkEvent::ExternalAddressConfirmed` is the first
///   moment a NAT-ed peer has a truthful address to publish, and its own
///   documentation says the root should re-`announce` on it. Announcing only
///   the newly confirmed address would replace the listening ones, so the whole
///   set has to be held somewhere.
/// * **Minting a join ticket** (D1). A ticket carries the issuer's endpoints,
///   and `JoinTicket::new` refuses one with none. `MembershipQueryPort` reports
///   what this peer knows about *others*; nothing in either context reports
///   where this peer itself is reachable, because that is a fact the transport
///   discovers and the roster has no entry for the local peer (invariant 2).
///
/// # Confirmed addresses come first
///
/// A relayed or NAT-confirmed address is the one a stranger can actually dial;
/// a listening address may be `0.0.0.0` or a private range that means nothing
/// outside this machine. Both are kept — a LAN peer redeeming a ticket wants
/// the private one — but the confirmed ones are listed first so a dialer tries
/// what is most likely to work, and so a truncated ticket keeps the useful
/// half.
#[derive(Debug, Default)]
pub struct LocalEndpoints {
    listening: Mutex<Vec<Endpoint>>,
    confirmed: Mutex<Vec<Endpoint>>,
}

impl LocalEndpoints {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a local listener. Returns whether it was new.
    pub fn record_listening(&self, endpoint: Endpoint) -> bool {
        push_new(&self.listening, endpoint)
    }

    /// Records an address another peer's probe confirmed reachable from
    /// outside. Returns whether it was new.
    pub fn record_confirmed(&self, endpoint: Endpoint) -> bool {
        push_new(&self.confirmed, endpoint)
    }

    /// Every endpoint, externally confirmed ones first.
    pub fn all(&self) -> Vec<Endpoint> {
        let mut endpoints = lock(&self.confirmed).clone();

        for endpoint in lock(&self.listening).iter() {
            if !endpoints.contains(endpoint) {
                endpoints.push(endpoint.clone());
            }
        }

        endpoints
    }

    /// Whether anything is known yet — a ticket cannot be minted before it is.
    pub fn is_empty(&self) -> bool {
        lock(&self.confirmed).is_empty() && lock(&self.listening).is_empty()
    }
}

fn push_new(cell: &Mutex<Vec<Endpoint>>, endpoint: Endpoint) -> bool {
    let mut endpoints = lock(cell);

    if endpoints.contains(&endpoint) {
        return false;
    }

    endpoints.push(endpoint);
    true
}

/// A poisoned lock means a previous holder panicked. A list of addresses has no
/// invariant a panic could have broken, so recovering is correct.
fn lock(cell: &Mutex<Vec<Endpoint>>) -> std::sync::MutexGuard<'_, Vec<Endpoint>> {
    cell.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
