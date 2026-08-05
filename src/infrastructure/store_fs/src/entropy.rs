use std::fmt;

/// Fills `secret` with bytes from the operating system's cryptographic random
/// source.
///
/// # Why this is the one place entropy enters the crate
///
/// Exactly one value in this crate has to be unguessable: the Ed25519 secret
/// scalar seed that *is* a peer's identity (D5). Everything else on disk is
/// public or local bookkeeping. Keeping the acquisition behind one function
/// means there is a single place to audit, and a single place to change if a
/// platform ever needs a different source.
///
/// The source is the OS CSPRNG on every supported target — `getrandom(2)` /
/// `/dev/urandom` on Linux, `getentropy(2)` on macOS and the BSDs,
/// `BCryptGenRandom` on Windows. It is deliberately *not* read from
/// `/dev/urandom` by hand: a hand-rolled reader would silently do nothing
/// useful on a platform without that device, and a keypair generated from a
/// weak source is a forgeable identity that looks exactly like a strong one.
///
/// Any 32 bytes are a valid Ed25519 secret seed, so there is no rejection loop
/// and no bias to correct: the derived public key is always a valid
/// [`PeerId`](shared_types::PeerId) (invariant 1).
pub(crate) fn fill_secret(secret: &mut [u8; 32]) -> Result<(), EntropyUnavailable> {
    getrandom::getrandom(secret).map_err(|_| EntropyUnavailable)
}

/// The operating system refused to provide random bytes.
///
/// Carries no detail on purpose: the only sane response is to refuse to create
/// an identity, and the caller reports that as
/// [`IdentityKeyStoreError::CreationFailed`](identity::ports::IdentityKeyStoreError::CreationFailed).
/// Generating a key from a fallback source would produce an identity that is
/// indistinguishable from a sound one and forgeable by anyone who guesses the
/// fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntropyUnavailable;

impl fmt::Display for EntropyUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the operating system random source is unavailable")
    }
}

impl std::error::Error for EntropyUnavailable {}
