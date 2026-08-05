//! What a test does when the machine will not give it a socket — and how it
//! says so out loud.
//!
//! # The problem this file exists to remove
//!
//! Six loopback tests in this crate return early and green when a socket cannot
//! be bound. That is defensible in a sandbox with no network namespace: it is a
//! fact about the machine, not a failure of the code, and the pure tests cover
//! the logic. It is *not* defensible on a machine that has networking, because
//! a skip there is a test that proved nothing while reporting success.
//!
//! Two things fix that, and both are here:
//!
//! * **The skip is visible.** The message is written to the process's own
//!   stderr handle rather than through `eprintln!`, because the test harness
//!   captures the print macros and shows them only for tests that failed — so a
//!   skip announced with `eprintln!` is announced to nobody.
//! * **The skip is refusable.** Setting [`REQUIRE_NETWORK_TESTS`] turns it into
//!   a panic. A machine that is supposed to have networking cannot quietly pass
//!   a suite it never ran.
//!
//! [`Verdict`] is a separate, pure decision so that the escalation itself is
//! checked rather than assumed — a guard against silent skips that was only
//! ever exercised when a skip happened would be the same problem one level up.

use std::fmt;
use std::io::Write;

/// Set this to anything but `0` or the empty string and a test that cannot bind
/// a socket fails instead of skipping.
///
/// Intended for CI and for any developer machine where networking is expected:
/// `DISTRO_REQUIRE_NETWORK_TESTS=1 cargo test --workspace`.
pub(crate) const REQUIRE_NETWORK_TESTS: &str = "DISTRO_REQUIRE_NETWORK_TESTS";

/// What to do about a test that cannot get a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The machine cannot provide one and nobody claimed otherwise.
    Skip,
    /// The environment says this machine has networking, so a missing socket is
    /// a failure.
    Fail,
}

impl Verdict {
    /// Reads the verdict from the environment variable's value, `None` meaning
    /// unset.
    ///
    /// Trimmed, and both `0` and the empty string mean "not required", so a
    /// shell that exports the variable empty does not silently turn every
    /// sandbox red.
    pub(crate) fn of(setting: Option<&str>) -> Self {
        match setting.map(str::trim) {
            None | Some("" | "0") => Self::Skip,
            Some(_) => Self::Fail,
        }
    }
}

/// Reports that a test could not get a local socket.
///
/// Panics — failing the test — when [`REQUIRE_NETWORK_TESTS`] says this machine
/// has networking. Otherwise writes one visible line and lets the caller
/// return.
pub(crate) fn skip(reason: &dyn fmt::Display) {
    let setting = std::env::var(REQUIRE_NETWORK_TESTS).ok();

    assert!(
        Verdict::of(setting.as_deref()) == Verdict::Skip,
        "a test that needs a local socket could not get one ({reason}), and \
         {REQUIRE_NETWORK_TESTS} says this machine has networking. Unset it to \
         allow the skip, or fix the environment."
    );

    // Deliberately not `eprintln!`: the harness captures that for a passing
    // test, which is how a skip becomes silent in the first place.
    let _ = writeln!(
        std::io::stderr(),
        "distro: skipping a test that needs a local socket — {reason}. \
         Set {REQUIRE_NETWORK_TESTS}=1 to make this a failure instead."
    );
}
