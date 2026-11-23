//! Repository facts, read by running `git` with an explicit `-C <root>`.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use garita_core::ObjectId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("`git` is not installed or not on PATH")]
    GitMissing,
    #[error("could not run `git`: {0}")]
    Spawn(io::Error),
    #[error("{0} is not inside a git repository")]
    NotARepository(PathBuf),
    #[error("`{0}` does not name a commit in this repository")]
    RefNotFound(String),
    #[error("this is a shallow clone, so the merge base with `{0}` is not here")]
    Shallow(String),
    #[error("`{0}` shares no history with HEAD")]
    NoMergeBase(String),
    #[error("`git {args}` failed ({status}): {stderr}")]
    Failed {
        args: String,
        status: String,
        stderr: String,
    },
    #[error("`git {args}` printed something unexpected: {output}")]
    Unexpected { args: String, output: String },
    #[error("could not use the temporary index {path}: {source}")]
    TempIndex { path: PathBuf, source: io::Error },
}

/// A `git` command that ignores user settings that would change its output, and never prompts.
fn command(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args([
            "--no-pager",
            "--no-optional-locks",
            "-c",
            "core.quotepath=off",
            "-c",
            "color.ui=never",
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.relative=false",
            "-c",
            "status.relativePaths=false",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    cmd
}

/// Runs git, returning how to describe the call alongside its output.
fn output(mut cmd: Command) -> Result<(String, Output), GitError> {
    let desc = describe(&cmd);
    let out = cmd.output().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => GitError::GitMissing,
        _ => GitError::Spawn(e),
    })?;
    Ok((desc, out))
}

fn describe(cmd: &Command) -> String {
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let skip = args.iter().position(|a| a == "status.relativePaths=false");
    args.get(skip.map_or(0, |i| i + 1)..)
        .unwrap_or_default()
        .join(" ")
}

fn failed(desc: String, out: &Output) -> GitError {
    GitError::Failed {
        args: desc,
        status: out.status.to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    }
}

/// Runs git and returns stdout, failing on any non-zero exit.
fn run<I, S>(root: &Path, args: I) -> Result<Vec<u8>, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = command(root);
    cmd.args(args);
    run_cmd(cmd)
}

fn run_cmd(cmd: Command) -> Result<Vec<u8>, GitError> {
    let (desc, out) = output(cmd)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(failed(desc, &out))
    }
}

fn trimmed(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|b| *b != b'\n' && *b != b'\r')
        .map_or(0, |i| i + 1);
    bytes.get(..end).unwrap_or_default()
}

fn line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(trimmed(bytes)).into_owned()
}

fn path_line(bytes: &[u8]) -> PathBuf {
    path_from_bytes(trimmed(bytes))
}

fn object_id(args: &str, bytes: &[u8]) -> Result<ObjectId, GitError> {
    let text = line(bytes);
    text.parse().map_err(|_| GitError::Unexpected {
        args: args.to_string(),
        output: text,
    })
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Splits NUL-terminated output into paths, dropping duplicates while keeping order.
fn paths(bytes: &[u8]) -> Vec<PathBuf> {
    let mut seen = std::collections::BTreeSet::new();
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .filter(|p| seen.insert(p.to_vec()))
        .map(path_from_bytes)
        .collect()
}

fn absolute(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// The top of the working tree that contains `dir`.
pub fn root(dir: &Path) -> Result<PathBuf, GitError> {
    let mut cmd = command(dir);
    cmd.args(["rev-parse", "--show-toplevel"]);
    let (_, out) = output(cmd)?;
    if !out.status.success() {
        return Err(GitError::NotARepository(dir.to_path_buf()));
    }
    Ok(path_line(&out.stdout))
}

/// HEAD's commit, or `None` in a repository with no commits yet.
pub fn head_commit(root: &Path) -> Result<Option<ObjectId>, GitError> {
    let mut cmd = command(root);
    cmd.args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
    let (desc, out) = output(cmd)?;
    match out.status.code() {
        Some(0) => object_id("rev-parse HEAD", &out.stdout).map(Some),
        Some(1) => Ok(None),
        _ => Err(failed(desc, &out)),
    }
}

/// Files added, copied, modified or renamed in the index, by their new path.
pub fn staged_files(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    run(
        root,
        [
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--diff-filter=ACMR",
        ],
    )
    .map(|b| paths(&b))
}

/// Staged files whose working copy also differs from the index.
pub fn partly_staged(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    let staged = staged_files(root)?;
    let unstaged = paths(&run(
        root,
        ["diff", "--name-only", "-z", "--no-renames", "--no-ext-diff"],
    )?);
    Ok(staged
        .into_iter()
        .filter(|p| unstaged.contains(p))
        .collect())
}

/// Files that HEAD adds, copies, modifies or renames since its merge base with `base`.
pub fn changed_files(root: &Path, base: &str) -> Result<Vec<PathBuf>, GitError> {
    let spec = format!("{base}^{{commit}}");
    let mut verify = command(root);
    verify.args([
        "rev-parse",
        "--verify",
        "--quiet",
        "--end-of-options",
        &spec,
    ]);
    if !output(verify)?.1.status.success() {
        return Err(GitError::RefNotFound(base.to_string()));
    }
    let mut cmd = command(root);
    cmd.args(["merge-base", "HEAD", "--end-of-options", base]);
    let (desc, out) = output(cmd)?;
    if !out.status.success() {
        if is_shallow(root)? {
            return Err(GitError::Shallow(base.to_string()));
        }
        return match out.status.code() {
            Some(1) => Err(GitError::NoMergeBase(base.to_string())),
            _ => Err(failed(desc, &out)),
        };
    }
    let merge_base = object_id("merge-base", &out.stdout)?;
    run(
        root,
        [
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--diff-filter=ACMR",
            merge_base.as_str(),
            "HEAD",
            "--",
        ],
    )
    .map(|b| paths(&b))
}

/// Every tracked file in the index, once each.
/// The commit `rev` names, or `None` when it names none.
pub fn resolve_commit(root: &Path, rev: &str) -> Result<Option<ObjectId>, GitError> {
    let spec = format!("{rev}^{{commit}}");
    let mut cmd = command(root);
    cmd.args([
        "rev-parse",
        "--verify",
        "--quiet",
        "--end-of-options",
        &spec,
    ]);
    let (desc, out) = output(cmd)?;
    match out.status.code() {
        Some(0) => object_id("rev-parse", &out.stdout).map(Some),
        Some(1) => Ok(None),
        _ => Err(failed(desc, &out)),
    }
}

/// The short name of the checked-out branch, or `None` on a detached HEAD.
pub fn current_branch(root: &Path) -> Result<Option<String>, GitError> {
    let mut cmd = command(root);
    cmd.args(["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let (desc, out) = output(cmd)?;
    match out.status.code() {
        Some(0) => Ok(Some(line(&out.stdout))),
        Some(1) => Ok(None),
        _ => Err(failed(desc, &out)),
    }
}

pub fn all_files(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    run(root, ["ls-files", "-z", "--cached"]).map(|b| paths(&b))
}

pub fn is_shallow(root: &Path) -> Result<bool, GitError> {
    let out = run(root, ["rev-parse", "--is-shallow-repository"])?;
    match line(&out).as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(GitError::Unexpected {
            args: "rev-parse --is-shallow-repository".into(),
            output: other.into(),
        }),
    }
}

/// The tree of a commit-ish.
pub fn tree_of_commit(root: &Path, commit: &str) -> Result<ObjectId, GitError> {
    let spec = format!("{commit}^{{tree}}");
    let mut cmd = command(root);
    cmd.args([
        "rev-parse",
        "--verify",
        "--quiet",
        "--end-of-options",
        &spec,
    ]);
    let (_, out) = output(cmd)?;
    if !out.status.success() {
        return Err(GitError::RefNotFound(commit.to_string()));
    }
    object_id("rev-parse", &out.stdout)
}

/// A copy of the repository's index that is deleted when dropped.
struct TempIndex {
    path: PathBuf,
}

impl TempIndex {
    fn seeded(root: &Path) -> Result<Self, GitError> {
        let real = absolute(
            root,
            path_line(&run(root, ["rev-parse", "--git-path", "index"])?),
        );
        let name = format!("garita-index-{}", ulid::Ulid::generate());
        let path = absolute(
            root,
            path_line(&run(root, ["rev-parse", "--git-path", &name])?),
        );
        let temp = Self { path };
        let copied = match fs::copy(&real, &temp.path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(source),
        };
        copied.map_err(|source| GitError::TempIndex {
            path: temp.path.clone(),
            source,
        })?;
        Ok(temp)
    }

    fn command(&self, root: &Path) -> Command {
        let mut cmd = command(root);
        cmd.env("GIT_INDEX_FILE", &self.path);
        cmd
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let mut lock = self.path.clone().into_os_string();
        lock.push(".lock");
        let _ = fs::remove_file(lock);
    }
}

/// The index's tree. `git write-tree` can rewrite the index to store its cache tree, so it
/// runs against a copy and the repository's own index is never written.
pub fn tree_of_index(root: &Path) -> Result<ObjectId, GitError> {
    let temp = TempIndex::seeded(root)?;
    let mut cmd = temp.command(root);
    cmd.arg("write-tree");
    object_id("write-tree", &run_cmd(cmd)?)
}

/// A tree of the working files as they are: tracked files with their current content plus
/// untracked files that are not ignored. Neither the index nor the working tree is touched.
///
/// It does write to the object database: git stores a loose object for each changed or untracked
/// file it hashes, and runs that file's clean filter. Uncommitted changes inside a submodule are
/// not seen, since a submodule is recorded by its commit.
pub fn worktree_tree(root: &Path) -> Result<ObjectId, GitError> {
    let temp = TempIndex::seeded(root)?;
    let mut add = temp.command(root);
    add.args(["add", "-A", "--", "."]);
    // Naming an ignored path, even to exclude it, makes `git add` fail.
    if !is_ignored(root, Path::new(".garita/receipts/"))? {
        add.arg(":(exclude).garita/receipts");
    }
    run_cmd(add)?;
    let mut write = temp.command(root);
    write.arg("write-tree");
    object_id("write-tree", &run_cmd(write)?)
}

/// Whether the working files differ from `tree`.
pub fn dirty(root: &Path, tree: &ObjectId) -> Result<bool, GitError> {
    Ok(worktree_tree(root)? != *tree)
}

/// Where git looks for hooks. A set `core.hooksPath` wins: `~` is expanded as git does, and a
/// relative value is relative to the working tree's root. Unset, it is `--git-path hooks`,
/// which in a linked worktree is the main repository's shared hooks directory.
pub fn hooks_dir(root: &Path) -> Result<PathBuf, GitError> {
    let mut cmd = command(root);
    cmd.args(["config", "--path", "--get", "core.hooksPath"]);
    let (desc, out) = output(cmd)?;
    match out.status.code() {
        Some(0) => {
            let value = trimmed(&out.stdout);
            if !value.is_empty() {
                return Ok(absolute(root, path_from_bytes(value)));
            }
        }
        Some(1) => {}
        _ => return Err(failed(desc, &out)),
    }
    let hooks = run(root, ["rev-parse", "--git-path", "hooks"])?;
    Ok(absolute(root, path_line(&hooks)))
}

/// The worktree's own git directory and the repository's common one (the same outside a
/// linked worktree), both absolute.
pub fn git_dirs(root: &Path) -> Result<[PathBuf; 2], GitError> {
    let own = path_line(&run(root, ["rev-parse", "--absolute-git-dir"])?);
    let common = path_line(&run(root, ["rev-parse", "--git-common-dir"])?);
    let common = absolute(root, common);
    Ok([own, common.canonicalize().unwrap_or(common)])
}

/// Whether `core.hooksPath` is set to a non-empty value.
pub fn hooks_path_set(root: &Path) -> Result<bool, GitError> {
    let mut cmd = command(root);
    cmd.args(["config", "--get", "core.hooksPath"]);
    let (desc, out) = output(cmd)?;
    match out.status.code() {
        Some(0) => Ok(!trimmed(&out.stdout).is_empty()),
        Some(1) => Ok(false),
        _ => Err(failed(desc, &out)),
    }
}

/// The blob hash git gives a file's bytes, with no clean filters applied.
pub fn hash_object(root: &Path, file: &Path) -> Result<ObjectId, GitError> {
    let mut cmd = command(root);
    cmd.args(["hash-object", "--no-filters", "--"]).arg(file);
    object_id("hash-object", &run_cmd(cmd)?)
}

/// Whether git ignores `path` (relative to `root`).
pub fn is_ignored(root: &Path, path: &Path) -> Result<bool, GitError> {
    let mut cmd = command(root);
    cmd.args(["check-ignore", "-q", "--"]).arg(path);
    let (desc, out) = output(cmd)?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failed(desc, &out)),
    }
}
