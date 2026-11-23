//! The receipt store under `<root>/.garita/receipts/` (RFC-1 §8).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use garita_core::receipt::{Receipt, ReceiptNameError, file_name};
use garita_core::report::{CheckStatus, Trigger};
use thiserror::Error;

use crate::git::{self, GitError};

pub const DIR: &str = ".garita/receipts";

#[derive(Debug, Error)]
pub enum ReceiptsError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Name(#[from] ReceiptNameError),
    #[error("could not serialize the receipt: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("{0} resolves outside the repository; garita will not write or delete there")]
    OutsideRepository(PathBuf),
    #[error("`{0}` cannot name a check's output file")]
    BadCheckName(String),
    #[error(transparent)]
    Git(#[from] GitError),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> ReceiptsError + '_ {
    move |source| ReceiptsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A receipt read from the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub path: PathBuf,
    pub run_id: String,
    pub receipt: Receipt,
}

/// A file in the store that is not a readable receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: String,
}

/// Receipts newest first, and the files that could not be read as one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
    pub receipts: Vec<Stored>,
    pub skipped: Vec<Skipped>,
}

impl Listing {
    /// The newest receipt in which `check` ran (any status but `skipped`) at `trigger`.
    pub fn last_run_of(&self, check: &str, trigger: Trigger) -> Option<&Stored> {
        self.receipts
            .iter()
            .find(|s| ran(&s.receipt, check, trigger))
    }
}

fn ran(receipt: &Receipt, check: &str, trigger: Trigger) -> bool {
    receipt.trigger == trigger
        && receipt
            .checks
            .iter()
            .any(|c| c.name == check && c.status != CheckStatus::Skipped)
}

/// The store's directory, created if missing and checked to lie inside the repository.
fn store(root: &Path, create: bool) -> Result<Option<PathBuf>, ReceiptsError> {
    let dir = root.join(DIR);
    if create {
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    } else if !dir.exists() {
        return Ok(None);
    }
    let real_root = fs::canonicalize(root).map_err(io_err(root))?;
    let real = fs::canonicalize(&dir).map_err(io_err(&dir))?;
    if !real.starts_with(&real_root) {
        return Err(ReceiptsError::OutsideRepository(dir));
    }
    Ok(Some(real))
}

/// Writes the receipt atomically and returns its path relative to `root`.
pub fn write(root: &Path, receipt: &Receipt) -> Result<String, ReceiptsError> {
    let name = file_name(&receipt.started_at, receipt.trigger, &receipt.run_id)?;
    let dir = store(root, true)?.ok_or_else(|| ReceiptsError::OutsideRepository(root.join(DIR)))?;
    let mut json = serde_json::to_vec_pretty(receipt)?;
    json.push(b'\n');
    let temp = dir.join(format!(".{name}.{}.tmp", ulid::Ulid::generate()));
    let target = dir.join(&name);
    let written = write_then_rename(&temp, &target, &json);
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written?;
    Ok(format!("{DIR}/{name}"))
}

fn write_then_rename(temp: &Path, target: &Path, bytes: &[u8]) -> Result<(), ReceiptsError> {
    let mut file = fs::File::create_new(temp).map_err(io_err(temp))?;
    file.write_all(bytes).map_err(io_err(temp))?;
    file.sync_all().map_err(io_err(temp))?;
    fs::rename(temp, target).map_err(io_err(target))
}

/// Where a check's full output goes, `<receipts>/<run id>/<check>.log`; creates the directory.
pub fn output_path(root: &Path, run_id: &str, check: &str) -> Result<PathBuf, ReceiptsError> {
    let plain = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    if !plain(check) {
        return Err(ReceiptsError::BadCheckName(check.to_string()));
    }
    file_name("1970-01-01T00:00:00Z", Trigger::Ci, run_id)?;
    let dir = store(root, true)?.ok_or_else(|| ReceiptsError::OutsideRepository(root.join(DIR)))?;
    let runs = dir.join(run_id);
    fs::create_dir_all(&runs).map_err(io_err(&runs))?;
    Ok(runs.join(format!("{check}.log")))
}

/// The run id in a receipt's file name, if the name has the receipt's shape.
fn run_id_of(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(".json")?;
    let (rest, run_id) = stem.rsplit_once('-')?;
    let (stamp, trigger) = rest.split_once('-')?;
    let trigger_ok = ["commit", "push", "ci", "merge"].contains(&trigger);
    let stamp_ok = stamp.len() == 16 && stamp.is_ascii();
    let probe = file_name("1970-01-01T00:00:00Z", Trigger::Ci, run_id);
    (trigger_ok && stamp_ok && probe.is_ok()).then_some(run_id)
}

/// Every receipt, newest first by the time and run id in its name.
pub fn list(root: &Path) -> Result<Listing, ReceiptsError> {
    let Some(dir) = store(root, false)? else {
        return Ok(Listing::default());
    };
    let mut names = Vec::new();
    let mut listing = Listing::default();
    for entry in fs::read_dir(&dir).map_err(io_err(&dir))? {
        let entry = entry.map_err(io_err(&dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        if !is_file || name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        match run_id_of(&name) {
            Some(_) => names.push(name),
            None => listing.skipped.push(Skipped {
                path,
                reason: "the name is not <UTC timestamp>-<trigger>-<run id>.json".into(),
            }),
        }
    }
    names.sort_by(|a, b| b.cmp(a));
    for name in names {
        let path = dir.join(&name);
        let run_id = run_id_of(&name).unwrap_or_default().to_string();
        let parsed = fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<Receipt>(&b).map_err(|e| e.to_string()));
        match parsed {
            Ok(receipt) => listing.receipts.push(Stored {
                path,
                run_id,
                receipt,
            }),
            Err(reason) => listing.skipped.push(Skipped { path, reason }),
        }
    }
    Ok(listing)
}

/// The newest receipt in which `check` ran at `trigger`.
pub fn last_run_of(
    root: &Path,
    check: &str,
    trigger: Trigger,
) -> Result<Option<Stored>, ReceiptsError> {
    Ok(list(root)?.last_run_of(check, trigger).cloned())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pruned {
    pub deleted: Vec<PathBuf>,
    /// Older than `keep` but kept as the last run of some check at some trigger.
    pub kept_for_demotion: Vec<PathBuf>,
    pub skipped: Vec<Skipped>,
}

/// Keeps the newest `keep` receipts, and the newest receipt in which each check ran at each
/// trigger; deletes the rest with their output directories. Unreadable files are left alone.
pub fn prune(root: &Path, keep: usize) -> Result<Pruned, ReceiptsError> {
    let listing = list(root)?;
    let Some(dir) = store(root, false)? else {
        return Ok(Pruned::default());
    };
    let mut protected = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for stored in &listing.receipts {
        for check in &stored.receipt.checks {
            let key = (check.name.clone(), stored.receipt.trigger);
            if check.status != CheckStatus::Skipped && seen.insert(key) {
                protected.insert(stored.path.clone());
            }
        }
    }
    let mut pruned = Pruned {
        skipped: listing.skipped,
        ..Pruned::default()
    };
    for stored in listing.receipts.iter().skip(keep) {
        if protected.contains(&stored.path) {
            pruned.kept_for_demotion.push(stored.path.clone());
            continue;
        }
        remove_inside(&dir, &stored.path)?;
        remove_inside(&dir, &dir.join(&stored.run_id))?;
        pruned.deleted.push(stored.path.clone());
    }
    Ok(pruned)
}

/// Removes `path` only if it sits directly in `dir`; a symlink is unlinked, never followed.
fn remove_inside(dir: &Path, path: &Path) -> Result<(), ReceiptsError> {
    let parent_ok = path
        .parent()
        .and_then(|p| fs::canonicalize(p).ok())
        .is_some_and(|p| p == dir);
    if !parent_ok {
        return Err(ReceiptsError::OutsideRepository(path.to_path_buf()));
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(path)(e)),
    };
    let removed = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(io_err(path))
}

/// Whether git ignores the receipts directory. This reports; it never edits `.gitignore`.
pub fn ignored_by_git(root: &Path) -> Result<bool, ReceiptsError> {
    Ok(git::is_ignored(root, Path::new(".garita/receipts/"))?)
}
