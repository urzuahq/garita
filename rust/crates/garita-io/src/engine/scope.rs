use std::path::{Path, PathBuf};

use garita_core::adapter::rebase;
use garita_core::config::Config;
use garita_core::report::{Finding, Scope, ScopeKind, Severity};

use super::Request;
use super::finding::{GARITA, file_finding, finding};
use crate::git::{self, GitError};

const BASE_CANDIDATES: [&str; 5] = [
    "origin/HEAD",
    "origin/main",
    "origin/master",
    "main",
    "master",
];

/// The files a run covers, after any widening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub kind: ScopeKind,
    pub files: Vec<String>,
    /// The base ref the change was measured against, when there is one.
    pub base: Option<String>,
    /// The files came from `--files`, not from git.
    pub explicit: bool,
    pub findings: Vec<Finding>,
}

impl Resolved {
    pub fn report_scope(&self) -> Scope {
        Scope {
            kind: self.kind,
            files: Some(self.files.clone()),
            packages: None,
        }
    }

    /// Whether `{affected}` expands to `--affected`.
    pub fn affected(&self) -> bool {
        self.kind == ScopeKind::Affected && !self.explicit
    }
}

pub fn resolve(root: &Path, config: &Config, request: &Request) -> Result<Resolved, String> {
    let kind = config.trigger(request.trigger).scope;
    let mut resolved = match &request.files {
        Some(files) => Resolved {
            kind,
            files: explicit_files(files)?,
            base: None,
            explicit: true,
            findings: Vec::new(),
        },
        None => from_git(root, kind, request.base.as_deref()).map_err(|e| e.to_string())?,
    };
    if resolved.kind == ScopeKind::Staged && !resolved.explicit {
        let partly = git::partly_staged(root).map_err(|e| e.to_string())?;
        resolved
            .findings
            .extend(partly.iter().map(|p| partly_staged(p)));
    }
    Ok(resolved)
}

fn explicit_files(files: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for file in files {
        let normal = rebase("", file);
        let outside = normal.starts_with('/')
            || normal == "."
            || normal.split('/').any(|s| s == "..")
            || Path::new(file).is_absolute();
        if outside {
            return Err(format!(
                "--files {file}: name a file by its path inside the repository"
            ));
        }
        if !out.contains(&normal) {
            out.push(normal);
        }
    }
    Ok(out)
}

fn from_git(root: &Path, kind: ScopeKind, base: Option<&str>) -> Result<Resolved, GitError> {
    let resolved = |kind, files: Vec<PathBuf>, base| Resolved {
        kind,
        files: existing(root, files),
        base,
        explicit: false,
        findings: Vec::new(),
    };
    match kind {
        ScopeKind::Staged => Ok(resolved(kind, git::staged_files(root)?, None)),
        ScopeKind::All => Ok(resolved(kind, git::all_files(root)?, None)),
        ScopeKind::Changed | ScopeKind::Affected => match changed(root, base)? {
            Ok((files, base)) => Ok(resolved(kind, files, Some(base))),
            Err(why) => {
                let mut wide = resolved(ScopeKind::All, git::all_files(root)?, None);
                let message = format!(
                    "the {} scope cannot be computed ({why}), so this run covers all tracked files",
                    kind.as_str()
                );
                wide.findings
                    .push(finding(GARITA, Severity::Info, "scope-widened", message));
                Ok(wide)
            }
        },
    }
}

/// The changed files and the base they were measured against, or why there are none.
fn changed(
    root: &Path,
    base: Option<&str>,
) -> Result<Result<(Vec<PathBuf>, String), String>, GitError> {
    let candidates: Vec<&str> = match base {
        Some(base) => vec![base],
        None => BASE_CANDIDATES.to_vec(),
    };
    for candidate in &candidates {
        match git::changed_files(root, candidate) {
            Ok(files) => return Ok(Ok((files, (*candidate).to_string()))),
            Err(GitError::RefNotFound(_)) if base.is_none() => {}
            Err(
                e @ (GitError::RefNotFound(_) | GitError::Shallow(_) | GitError::NoMergeBase(_)),
            ) => return Ok(Err(e.to_string())),
            Err(e) => return Err(e),
        }
    }
    Ok(Err(format!(
        "no base ref was found; tried {}; pass --base",
        candidates.join(", ")
    )))
}

/// Files present in the working tree; a staged or committed file deleted since is not checked.
fn existing(root: &Path, files: Vec<PathBuf>) -> Vec<String> {
    files
        .into_iter()
        .filter(|f| root.join(f).is_file())
        .map(|f| f.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn partly_staged(path: &Path) -> Finding {
    let file = path.to_string_lossy().replace('\\', "/");
    let message = format!(
        "{file} also has unstaged changes; garita checks the working file, so those changes are included"
    );
    file_finding(
        GARITA,
        Severity::Warning,
        "partly-staged",
        Some(file),
        message,
    )
}
