use std::fmt;

use ed25519_dalek::{Signer, SigningKey};
use shared_types::{EnvelopeSignature, Fingerprint, PeerId};

use crate::rng::SeededRng;

/// One simulated peer's real Ed25519 keypair.
///
/// # Real crypto, on purpose
///
/// The context crates' own test fakes sign with a keyed digest, which is right
/// for a unit test asserting that *some* signature was produced over *those*
/// bytes. It is not enough here. Every multi-peer claim in the canvas runs
/// through this harness (S5), and AC6 — "every displayed message is
/// signature-verified against the author's `PeerId`" — is only meaningfully
/// verified if the signature is one a real verifier could reject. A forged
/// envelope must fail for the reason a forged envelope fails on a real
/// network, not because a digest fake happened to notice.
///
/// So this signs with `ed25519-dalek`, over
/// [`Envelope::signable_bytes`](shared_types::Envelope::signable_bytes), and
/// the matching verifier checks it against the public key that *is* the
/// author's `PeerId`. Invariant 1 makes those the same bytes, so nothing has to
/// look a key up anywhere.
///
/// # Deterministic key material without a random source
///
/// The secret is filled from [`SeededRng`], so `alice` in a scenario seeded
/// with 7 has the same identity in every run on every machine (S5, AC13) — and
/// a different one from `bob`. No entropy source, no `rand`, no `getrandom`.
/// Any 32 bytes are a valid Ed25519 secret, and the public key derived from one
/// is always a valid [`PeerId`] (invariant 1).
pub struct SimKeypair {
    signing: SigningKey,
    peer: PeerId,
}

impl SimKeypair {
    /// Builds the keypair whose secret scalar seed is `secret`.
    pub fn from_secret_bytes(secret: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&secret);
        let peer = PeerId::from_public_key_bytes(signing.verifying_key().to_bytes())
            .expect("an Ed25519 verifying key is by construction a valid PeerId");

        Self { signing, peer }
    }

    /// The keypair a peer called `label` has in a run seeded with `seed`.
    ///
    /// Stable across runs and machines; distinct per label and per seed.
    pub fn derived(seed: u64, label: &str) -> Self {
        let mut rng = SeededRng::for_label(seed, label);
        let mut secret = [0u8; 32];
        rng.fill_bytes(&mut secret);

        Self::from_secret_bytes(secret)
    }

    /// This peer's identity: its public key (invariant 1).
    pub const fn peer(&self) -> PeerId {
        self.peer
    }

    /// Signs `message` — always the signable bytes of an envelope, never
    /// anything else.
    ///
    /// Takes the bytes rather than a draft because the two contexts hand over
    /// two different draft types, and the one thing they agree on is exactly
    /// this byte string.
    pub fn sign_bytes(&self, message: &[u8]) -> EnvelopeSignature {
        EnvelopeSignature::new(self.signing.sign(message).to_bytes())
    }
}

impl fmt::Debug for SimKeypair {
    /// Hand-written so no secret byte can reach a log, a panic message, or a
    /// test failure diff. The public fingerprint is the whole of what is
    /// printable about a keypair.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimKeypair")
            .field("peer", &Fingerprint::of(&self.peer).to_string())
            .finish_non_exhaustive()
    }
}
