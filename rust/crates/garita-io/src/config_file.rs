//! Reads `.garita/config.yaml`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use garita_core::ObjectId;
use garita_core::config::{Config, ConfigErrors};
use thiserror::Error;

use crate::git::{self, GitError};

pub const PATH: &str = ".garita/config.yaml";

#[derive(Debug, Error)]
pub enum ConfigFileError {
    #[error("{0} does not exist")]
    Missing(PathBuf),
    #[error("could not read {path}: {source}")]
    Unreadable { path: PathBuf, source: io::Error },
    #[error("{path} is not a valid config:\n{errors}")]
    Invalid { path: PathBuf, errors: ConfigErrors },
    #[error("could not hash {path}: {source}")]
    Hash { path: PathBuf, source: GitError },
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
        io::ErrorKind::NotFound => ConfigFileError::Missing(path.clone()),
        _ => ConfigFileError::Unreadable {
            path: path.clone(),
            source,
        },
    })?;
    let config = Config::parse(&text).map_err(|errors| ConfigFileError::Invalid {
        path: path.clone(),
        errors,
    })?;
    let hash =
        git::hash_object(root, &path).map_err(|source| ConfigFileError::Hash { path, source })?;
    Ok(Loaded { config, hash })
}
