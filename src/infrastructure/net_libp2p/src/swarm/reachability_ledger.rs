use std::collections::{HashMap, HashSet};

use libp2p::PeerId as Libp2pPeerId;
use membership::domain::Endpoint;

use crate::swarm::external_address_ledger::CORROBORATION_THRESHOLD;

/// Whether strangers can dial this peer (canvas D3).
///
/// # Three states, and never a boolean
///
/// "Not probed yet" and "probed and refused" are different facts about the
/// world, and a `bool` would force startup to claim one of them — the alarming
/// one, for every peer, for the first seconds of every launch. `Option<bool>`
/// carries the same information without names and reads as a nullable flag at
/// every call site. So: three named states, and [`Unknown`](Self::Unknown) is
/// the default (invariant 3).
///
/// **[`Unknown`](Self::Unknown) must stay distinguishable from
/// [`Unreachable`](Self::Unreachable) everywhere** — in this type, in the
/// router, and in the renderer (S3). Collapsing them anywhere reintroduces the
/// false negative the whole piece exists to avoid.
///
/// # What this is not
///
/// * Not [`membership::domain::Reachability`], which is a property of one
///   *address* — direct or through a relay — and is carried inside the
///   [`Endpoint`] below. This type is a property of *this process's* position
///   on the network. They share a name and nothing else; a call site holding
///   both should alias one of them.
/// * Not `membership::domain::Presence`, which is derived evidence about a
///   *remote* peer's liveness (D5). Structurally similar, semantically
///   unrelated, and conflating them would be a modelling error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Reachability {
    /// Nothing conclusive yet. The honest state at startup, and the state a
    /// single failed probe leaves this peer in.
    #[default]
    Unknown,
    /// A probe dialled this peer back at this address and got through.
    Reachable(Endpoint),
    /// Enough distinct servers failed to dial this peer back that the failure
    /// is no longer one peer's word.
    Unreachable,
}

/// What one AutoNAT probe reported, in this crate's vocabulary.
///
/// The libp2p `Result<(), autonat::v2::client::Error>` is translated to this at
/// the driver's match arm and never enters the ledger. Two reasons: the ledger
/// makes the same decision whatever the error says — a failure is a failure, and
/// the distinctions inside that error are about *why the probe could not run*,
/// not about this peer's position on the network — and `client::Error` has a
/// private field, so a test could not construct one to drive the failure path
/// with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeResult {
    /// The dial-back arrived. Proof.
    Succeeded,
    /// The dial-back did not arrive. Evidence, and only evidence.
    Failed,
}

/// The ledger's verdict on one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    /// The probe changed nothing a user would be shown. The steady state of a
    /// healthy peer, which AutoNAT re-probes on a timer.
    Unchanged,
    /// The derived state moved, and this is where it moved to.
    Changed(Reachability),
}

/// Per-address failure evidence, the address currently proven reachable, and
/// the verdict the two produce.
///
/// # Why this is a separate type rather than three fields on the driver
///
/// The rule it holds is a *safety* property (S2): a false negative sends a user
/// to change router settings that were never wrong, which is worse than saying
/// nothing. Sitting inside the swarm's event loop that rule would be testable
/// only by standing up a real NAT — which no test can do (S4) — so it lives
/// here, as a pure function of its own state plus one probe result, where every
/// clause of it has a test that runs in microseconds and cannot flake.
///
/// # Purity
///
/// No swarm, no socket, no clock, no random source. The same sequence of probe
/// results produces the same sequence of verdicts on every machine and every
/// run. Nothing here iterates a `HashMap` or a `HashSet`, so hash ordering
/// cannot leak into a decision either.
///
/// # Evidence is asymmetric, deliberately (D2, S2)
///
/// One success sets [`Reachable`](Reachability::Reachable). A dial-back that
/// arrived is proof: a server cannot fabricate a connection this peer's own
/// transport accepted, and no attacker gains anything by convincing us we are
/// reachable when we are.
///
/// A failure never concludes [`Unreachable`](Reachability::Unreachable) on its
/// own. It is one server's word, and that server may be broken, overloaded, or
/// hostile. [`CORROBORATION_THRESHOLD`] distinct servers must agree — the same
/// constant piece 1 uses for observed addresses, imported rather than redefined
/// so there is one story about not trusting a single peer. **This is not a
/// tuning knob**: lowering it to one lets any peer condemn any other, and
/// making the evidence symmetric "for consistency" produces confident false
/// negatives.
///
/// # What it deliberately does not do (D4, S5)
///
/// Nothing here changes a dial, a relay reservation, or an address selection.
/// libp2p already prefers a confirmed direct address and falls back to a
/// circuit; acting on this derived state would duplicate that logic with worse
/// information. The verdict reports, and that is all it does.
pub(crate) struct ReachabilityLedger {
    /// Invariant 6's cap on how many addresses failure evidence is held for.
    max_failing_addresses: usize,
    /// The address most recently proven reachable, if any.
    reachable: Option<Endpoint>,
    /// Which distinct servers have failed to dial each address back.
    ///
    /// An address stops accumulating the moment it is condemned, so each set
    /// holds at most [`CORROBORATION_THRESHOLD`] entries and one bound on the
    /// number of keys bounds the whole structure.
    failures: HashMap<Endpoint, HashSet<Libp2pPeerId>>,
    /// Addresses whose failure evidence has reached the threshold. A subset of
    /// `failures`' keys, kept so that "is any address corroborated as failing"
    /// is an O(1) question with no iteration in it.
    condemned: HashSet<Endpoint>,
    /// The verdict last returned, so a repeat is [`ProbeOutcome::Unchanged`]
    /// rather than a fresh event (invariant 5).
    current: Reachability,
}

impl ReachabilityLedger {
    /// A ledger that has been told nothing, which is [`Reachability::Unknown`].
    pub(crate) fn new(max_failing_addresses: usize) -> Self {
        Self {
            max_failing_addresses,
            reachable: None,
            failures: HashMap::new(),
            condemned: HashSet::new(),
            current: Reachability::Unknown,
        }
    }

    /// Records what one server reported about one address, and says whether the
    /// answer a user would be shown has moved.
    ///
    /// `server` is the peer that ran the probe. It is what makes corroboration
    /// mean "distinct servers agreed" rather than "we asked twice", so there is
    /// no way to call this without one.
    pub(crate) fn record(
        &mut self,
        server: Libp2pPeerId,
        address: Endpoint,
        result: ProbeResult,
    ) -> ProbeOutcome {
        match result {
            ProbeResult::Succeeded => self.succeeded(address),
            ProbeResult::Failed => self.failed(server, address),
        }

        self.settle()
    }

    /// A dial-back arrived at `address`.
    ///
    /// Every failure record is dropped, not just this address's. Reachability
    /// is a property of this peer, and proof that one address works is proof
    /// that strangers can dial it; letting stale failures for other addresses
    /// survive would let the very next failed probe flip a peer that has just
    /// been *proven* reachable into `Unreachable` (invariant 4, S2). Clearing
    /// also keeps the bound below from being reached by a peer whose network
    /// keeps changing.
    fn succeeded(&mut self, address: Endpoint) {
        self.failures.clear();
        self.condemned.clear();
        self.reachable = Some(address);
    }

    /// A dial-back did not arrive at `address`, according to `server`.
    ///
    /// The whole decision, in order, because the order is load-bearing:
    ///
    /// 1. **Is this address already condemned?** Then there is nothing left to
    ///    learn about it, and refusing here is what keeps the per-address
    ///    server set capped by the threshold rather than by the size of the
    ///    network.
    /// 2. **Does the bound admit a new address?** Probe results are produced by
    ///    servers this peer did not choose, about addresses fed in by whatever
    ///    the swarm saw, so the ledger refuses rather than grows (S6).
    /// 3. **Have enough distinct servers now failed the same address?** Only
    ///    then is the address condemned — and only then does it displace a
    ///    proof of reachability, and only its own.
    fn failed(&mut self, server: Libp2pPeerId, address: Endpoint) {
        if self.condemned.contains(&address) {
            return;
        }

        if !self.failures.contains_key(&address)
            && self.failures.len() >= self.max_failing_addresses
        {
            return;
        }

        let blaming = self.failures.entry(address.clone()).or_default();
        blaming.insert(server);
        if blaming.len() < CORROBORATION_THRESHOLD {
            return;
        }

        // Corroborated. The proof it overturns is the proof about *this*
        // address and no other: a multi-homed peer whose IPv4 path is blocked
        // and whose IPv6 path works is reachable, and saying otherwise would be
        // exactly the false negative S2 forbids.
        self.condemned.insert(address.clone());
        if self.reachable.as_ref() == Some(&address) {
            self.reachable = None;
        }
    }

    /// Derives the verdict from the evidence and reports whether it moved.
    ///
    /// Proof outranks evidence: an address known to work means strangers can
    /// dial this peer, whatever else has failed. Only with no working address
    /// does corroborated failure become a verdict, and with neither the honest
    /// answer is `Unknown` (invariant 3).
    fn settle(&mut self) -> ProbeOutcome {
        let verdict = match &self.reachable {
            Some(address) => Reachability::Reachable(address.clone()),
            None if !self.condemned.is_empty() => Reachability::Unreachable,
            None => Reachability::Unknown,
        };

        if verdict == self.current {
            return ProbeOutcome::Unchanged;
        }

        self.current = verdict.clone();
        ProbeOutcome::Changed(verdict)
    }

    /// The verdict as it stands.
    #[cfg(test)]
    pub(crate) const fn reachability(&self) -> &Reachability {
        &self.current
    }

    /// How many addresses failure evidence is held for.
    #[cfg(test)]
    pub(crate) fn failing_addresses(&self) -> usize {
        self.failures.len()
    }

    /// How many distinct servers have failed to dial `address` back.
    #[cfg(test)]
    pub(crate) fn servers_blaming(&self, address: &Endpoint) -> usize {
        self.failures.get(address).map_or(0, HashSet::len)
    }
}
