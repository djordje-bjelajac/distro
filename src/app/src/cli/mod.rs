//! What a launch is told before anything is wired: the arguments, the profile
//! directory, and the usage text that carries S7's and S8's disclosures.
//!
//! Nothing here reads a store, opens a socket, or knows a port exists. The
//! whole module is pure apart from one function that reads three environment
//! variables, which is what lets the precedence rules be unit-tested rather
//! than asserted about a machine.

mod launch_options;
#[cfg(test)]
mod launch_options_test;
mod profile_directory;
#[cfg(test)]
mod profile_directory_test;
mod usage;
#[cfg(test)]
mod usage_test;

pub use launch_options::{ArgumentError, LaunchOptions, LaunchRequest};
pub use profile_directory::{ProfileDirectory, ProfileDirectoryError, ProfileEnvironment};
pub use usage::Usage;
