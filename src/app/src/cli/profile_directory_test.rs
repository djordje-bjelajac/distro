use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::{ProfileDirectory, ProfileDirectoryError, ProfileEnvironment};

fn environment(
    override_path: Option<&str>,
    xdg_data_home: Option<&str>,
    home: Option<&str>,
) -> ProfileEnvironment {
    ProfileEnvironment {
        override_path: override_path.map(OsString::from),
        xdg_data_home: xdg_data_home.map(OsString::from),
        home: home.map(OsString::from),
    }
}

#[test]
fn the_flag_wins_over_every_environment_value() {
    let resolved = ProfileDirectory::resolve(
        Some(Path::new("/tmp/one")),
        &environment(Some("/tmp/two"), Some("/tmp/three"), Some("/home/user")),
    );

    assert_eq!(resolved, Ok(PathBuf::from("/tmp/one")));
}

#[test]
fn the_environment_override_wins_over_the_platform_default() {
    let resolved = ProfileDirectory::resolve(
        None,
        &environment(Some("/tmp/two"), Some("/tmp/three"), Some("/home/user")),
    );

    assert_eq!(resolved, Ok(PathBuf::from("/tmp/two")));
}

#[test]
fn xdg_data_home_gains_the_application_directory() {
    let resolved =
        ProfileDirectory::resolve(None, &environment(None, Some("/data"), Some("/home/user")));

    assert_eq!(resolved, Ok(PathBuf::from("/data/distro")));
}

#[test]
fn without_xdg_data_home_the_default_is_relative_to_home() {
    let resolved = ProfileDirectory::resolve(None, &environment(None, None, Some("/home/user")));

    assert_eq!(
        resolved,
        Ok(PathBuf::from("/home/user/.local/share/distro"))
    );
}

#[test]
fn an_empty_environment_value_is_treated_as_unset() {
    // `DISTRO_PROFILE_DIR=` in a shell script is an accident, and resolving it
    // to "" would put an identity wherever the process happened to start.
    let resolved = ProfileDirectory::resolve(None, &environment(Some(""), Some(""), Some("/home")));

    assert_eq!(resolved, Ok(PathBuf::from("/home/.local/share/distro")));
}

#[test]
fn an_empty_flag_is_refused_rather_than_ignored() {
    // Unlike an empty variable, an empty flag was typed on purpose and means
    // the caller believes it said something.
    let resolved = ProfileDirectory::resolve(Some(Path::new("")), &environment(None, None, None));

    assert_eq!(resolved, Err(ProfileDirectoryError::EmptyFlag));
}

#[test]
fn with_nothing_set_at_all_no_directory_is_invented() {
    let resolved = ProfileDirectory::resolve(None, &environment(None, None, None));

    assert_eq!(resolved, Err(ProfileDirectoryError::NoHomeDirectory));
}

#[test]
fn two_instances_can_be_given_two_directories() {
    // The property OP-13's manual protocol depends on: two profile
    // directories, therefore two keypairs, therefore two peers on one machine.
    let first = ProfileDirectory::resolve(
        Some(Path::new("/tmp/peer-a")),
        &environment(None, None, None),
    );
    let second = ProfileDirectory::resolve(
        Some(Path::new("/tmp/peer-b")),
        &environment(None, None, None),
    );

    assert_ne!(first, second);
}
