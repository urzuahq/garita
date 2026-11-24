//! Reads `.garita/config.yaml`.

use std::fs;
use std::io;
use std::path::Path;

use garita_core::ObjectId;
use garita_core::config::{Config, ConfigErrors};
use thiserror::Error;

use crate::git::{self, GitError};

pub const PATH: &str = ".garita/config.yaml";

#[derive(Debug, Error)]
pub enum ConfigFileError {
    #[error("{PATH} does not exist")]
    Missing,
    #[error("could not read {PATH}: {source}")]
    Unreadable { source: io::Error },
    #[error("{PATH} is not a valid config: {errors}")]
    Invalid { errors: ConfigErrors },
    #[error("could not hash {PATH}: {source}")]
    Hash { source: GitError },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub config: Config,
    /// The file's git blob hash.
    pub hash: ObjectId,
}

pub fn load(root: &Path) -> Result<Loaded, ConfigFileError> {
    let path = root.join(PATH);
    let text = fs::read_to_string(&path).map_err(|source| match source.kind() {
        io::ErrorKind::NotFound => ConfigFileError::Missing,
        _ => ConfigFileError::Unreadable { source },
    })?;
    let config = Config::parse(&text).map_err(|errors| ConfigFileError::Invalid { errors })?;
    let hash = git::hash_object(root, &path).map_err(|source| ConfigFileError::Hash { source })?;
    Ok(Loaded { config, hash })
}
