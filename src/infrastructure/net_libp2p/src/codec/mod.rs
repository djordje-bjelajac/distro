//! The envelope wire encoding (D6) and the local counters S2 requires.
//!
//! The whole of the compatibility rule lives in `shared_types` as pure
//! functions — [`Compatibility::evaluate`](shared_types::Compatibility::evaluate)
//! and [`PayloadKind::from_code`](shared_types::PayloadKind::from_code) — and
//! this module *calls* them. It never restates them: a second copy of the rule
//! is how two builds come to disagree about the same envelope, which on a
//! network with no coordinated deploy is a permanent split rather than a bug to
//! fix next release.

mod codec_diagnostics;
mod envelope_codec;
mod envelope_codec_error;
#[cfg(test)]
mod envelope_codec_test;

pub use codec_diagnostics::CodecDiagnostics;
pub use envelope_codec::EnvelopeCodec;
pub use envelope_codec_error::EnvelopeCodecError;
