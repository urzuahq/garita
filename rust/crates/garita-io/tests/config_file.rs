#![cfg(unix)]

mod common;

use std::fs;

use common::{TempDir, git, init, write};
use garita_io::config_file::{self, ConfigFileError};

#[test]
fn loads_and_hashes_the_config() {
    let tmp = TempDir::new();
    let r = tmp.path();
    init(r);
    write(r, config_file::PATH, "schema_version: 1\n");
    let loaded = config_file::load(r).unwrap();
    assert!(loaded.config.checks.is_empty());
    assert_eq!(
        loaded.hash.as_str(),
        git(r, &["hash-object", config_file::PATH]).trim()
    );
}

#[test]
fn a_missing_config_is_its_own_error() {
    let tmp = TempDir::new();
    init(tmp.path());
    assert!(matches!(
        config_file::load(tmp.path()),
        Err(ConfigFileError::Missing)
    ));
}

#[test]
fn an_unreadable_config_is_its_own_error() {
    let tmp = TempDir::new();
    let r = tmp.path();
    init(r);
    fs::create_dir_all(r.join(config_file::PATH)).unwrap();
    assert!(matches!(
        config_file::load(r),
        Err(ConfigFileError::Unreadable { .. })
    ));
    fs::remove_dir(r.join(config_file::PATH)).unwrap();
    fs::write(r.join(config_file::PATH), b"\xff\xfe").unwrap();
    assert!(matches!(
        config_file::load(r),
        Err(ConfigFileError::Unreadable { .. })
    ));
}

#[test]
fn an_invalid_config_carries_every_error() {
    let tmp = TempDir::new();
    let r = tmp.path();
    init(r);
    write(
        r,
        config_file::PATH,
        "schema_version: 1\nbogus: 1\non_crash: maybe\n",
    );
    match config_file::load(r) {
        Err(ConfigFileError::Invalid { errors, .. }) => assert_eq!(errors.0.len(), 2, "{errors}"),
        other => panic!("{other:?}"),
    }
}
