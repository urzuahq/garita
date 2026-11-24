//! `garita hooks install` and `uninstall`: the `pre-commit` and `pre-push` hooks (RFC-1 §7).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use garita_core::report::{Examined, Finding, RunError, Severity, Trigger};

use crate::config_file;
use crate::git;
use crate::verdict::{self, Verdict, finding};

/// Raised when the script changes in a way an installed hook must be rewritten for.
pub const HOOK_FORMAT: u32 = 2;
const MARKER: &str = "# garita hook format ";

/// The hooks garita installs, with the trigger each runs.
pub const HOOKS: [(&str, Trigger); 2] =
    [("pre-commit", Trigger::Commit), ("pre-push", Trigger::Push)];

/// Writes the exact script of one hook format, for a hook name and its trigger.
pub type Script = fn(&str, Trigger) -> String;

/// Every hook format garita has written, oldest first; a file is garita's only when it is
/// byte for byte one of these.
pub const FORMATS: &[(u32, Script)] = &[(1, script_v1), (2, script_v2)];

/// The script for one hook in the current format.
pub fn script(name: &str, trigger: Trigger) -> String {
    script_v2(name, trigger)
}

/// The `not-run` report a hook prints when `garita` is not on PATH (RFC-4 §5).
pub fn missing_garita_report(name: &str, trigger: Trigger) -> String {
    let t = trigger.as_str();
    format!(
        "{{\"status\":\"not-run\",\
         \"error\":\"garita is not on PATH, so the {name} hook cannot run it and blocks this {t}\",\
         \"error_kind\":\"environment\",\"next_action\":null,\"next_action_reason\":null,\
         \"next_action_argv\":null,\"next_action_inputs\":[]}}"
    )
}

/// Prints [`missing_garita_report`] on stdout, where `garita run` prints its own, and exits 2.
fn script_v2(name: &str, trigger: Trigger) -> String {
    let t = trigger.as_str();
    let report = missing_garita_report(name, trigger);
    format!(
        "#!/bin/sh\n\
         {MARKER}2: written by `garita hooks install`, which rewrites this file\n\
         if ! command -v garita >/dev/null 2>&1; then\n  \
         printf '%s\\n' '{report}'\n  \
         exit 2\n\
         fi\n\
         exec garita run --trigger {t}\n"
    )
}

/// Fails closed when `garita` is missing and leaves the verdict's exit code to `exec`.
fn script_v1(name: &str, trigger: Trigger) -> String {
    let t = trigger.as_str();
    format!(
        "#!/bin/sh\n\
         {MARKER}1: written by `garita hooks install`, which rewrites this file\n\
         if ! command -v garita >/dev/null 2>&1; then\n  \
         echo \"garita: the {name} hook needs garita, which is not on PATH; this {t} is blocked\" >&2\n  \
         exit 2\n\
         fi\n\
         exec garita run --trigger {t}\n"
    )
}

/// What a hook file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Missing,
    /// Exactly the script garita writes, in this hook format.
    Garita(u32),
    /// Carries garita's marker but has been edited; someone else's now.
    Modified {
        calls_garita: bool,
    },
    /// Someone else's; `calls_garita` when it runs `garita run` at this hook's trigger.
    Foreign {
        calls_garita: bool,
    },
}

impl Kind {
    pub fn is_garita(self) -> bool {
        matches!(self, Kind::Garita(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookFile {
    pub path: PathBuf,
    pub kind: Kind,
    pub symlink: bool,
    pub executable: bool,
    pub text: Option<String>,
}

pub fn examine(path: &Path, name: &str, trigger: Trigger) -> io::Result<HookFile> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => Some(meta),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let symlink = meta.as_ref().is_some_and(|m| m.file_type().is_symlink());
    if meta.as_ref().is_some_and(fs::Metadata::is_dir) {
        return Ok(HookFile {
            path: path.to_path_buf(),
            kind: Kind::Foreign {
                calls_garita: false,
            },
            symlink,
            executable: false,
            text: None,
        });
    }
    let (text, executable) = match fs::read(path) {
        Ok(bytes) => (
            Some(String::from_utf8_lossy(&bytes).into_owned()),
            is_executable(path),
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (None, false),
        Err(e) => return Err(e),
    };
    let kind = text
        .as_deref()
        .map_or(Kind::Missing, |t| classify(t, name, trigger, FORMATS));
    Ok(HookFile {
        path: path.to_path_buf(),
        kind,
        symlink,
        executable,
        text,
    })
}

/// Garita's hook in some format of `formats`, an edited copy of one, or someone else's.
pub fn classify(text: &str, name: &str, trigger: Trigger, formats: &[(u32, Script)]) -> Kind {
    if let Some((format, _)) = formats.iter().find(|(_, s)| s(name, trigger) == text) {
        return Kind::Garita(*format);
    }
    let calls_garita = calls_garita(text, trigger);
    if text.lines().any(|l| l.starts_with(MARKER)) {
        Kind::Modified { calls_garita }
    } else {
        Kind::Foreign { calls_garita }
    }
}

/// Whether a line runs `garita run` as a command at `trigger`; `commit` is its default trigger.
/// A line that joins it to another command (`|| true`, `&`, `;`) does not count, since the hook
/// may then not act on the verdict.
fn calls_garita(text: &str, trigger: Trigger) -> bool {
    let joined = text.replace("\\\n", " ");
    joined.lines().any(|line| {
        let line = line.trim();
        let line = line.strip_prefix("exec ").map_or(line, str::trim_start);
        let Some(rest) = line.strip_prefix("garita run") else {
            return false;
        };
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return false;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        if words
            .iter()
            .any(|w| !is_fd_redirect(w) && w.contains(['|', '&', ';']))
        {
            return false;
        }
        let mut named = None;
        for (i, word) in words.iter().enumerate() {
            if *word == "--trigger" {
                named = words.get(i + 1).copied();
            } else if let Some(t) = word.strip_prefix("--trigger=") {
                named = Some(t);
            }
        }
        match named {
            Some(t) => t == trigger.as_str(),
            None => trigger == Trigger::Commit,
        }
    })
}

/// `2>&1` and the like: a redirection between file descriptors, which keeps the verdict.
fn is_fd_redirect(word: &str) -> bool {
    let rest = word.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.strip_prefix(">&")
        .is_some_and(|fd| !fd.is_empty() && fd.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The repository root and the directory git reads hooks from.
fn locate(dir: &Path) -> Result<(PathBuf, PathBuf), git::GitError> {
    let root = git::root(dir)?;
    let hooks = git::hooks_dir(&root)?;
    Ok((root, hooks))
}

/// Whether `hooks` lies inside the working tree or the repository's git directories.
fn inside_repository(root: &Path, hooks: &Path) -> Result<bool, git::GitError> {
    let hooks = resolved(hooks);
    let [own, common] = git::git_dirs(root)?;
    Ok([root.to_path_buf(), own, common]
        .iter()
        .any(|dir| hooks.starts_with(resolved(dir))))
}

/// `path` with every existing ancestor's symlinks resolved, for a path that may not exist yet.
fn resolved(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(real) = fs::canonicalize(cur) {
            return rest.iter().rev().fold(real, |p, c| p.join(c));
        }
        match (cur.file_name(), cur.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Every way a hook command fails to run lies outside garita: the repository or its files.
fn cannot(error: impl Into<String>) -> Verdict {
    verdict::not_run(RunError::environment(error))
}

fn examined_hooks() -> Examined {
    Examined::from([("hooks".to_string(), HOOKS.len() as u64)])
}

enum Step {
    Write {
        name: &'static str,
        trigger: Trigger,
        path: PathBuf,
        done: Finding,
    },
    Keep(Finding),
}

/// Writes both hooks, or neither when either would overwrite something that is not garita's.
pub fn install(dir: &Path, replace: bool) -> Verdict {
    let (root, hooks) = match locate(dir) {
        Ok(found) => found,
        Err(e) => return cannot(e.to_string()),
    };
    let mut findings = Vec::new();
    let mut refused = false;
    match inside_repository(&root, &hooks) {
        Ok(true) => {}
        Ok(false) => {
            let shown = match git::hooks_path_set(&root) {
                Ok(true) => format!("{} (core.hooksPath)", hooks.display()),
                Ok(false) => format!("{}, where .git/hooks resolves to,", hooks.display()),
                Err(e) => return cannot(e.to_string()),
            };
            let (severity, message) = if replace {
                (
                    Severity::Info,
                    format!(
                        "{shown} lies outside this repository and may be shared with others; --replace writes there anyway"
                    ),
                )
            } else {
                refused = true;
                (
                    Severity::Error,
                    format!(
                        "{shown} lies outside this repository and may be shared with others; pass --replace to write there anyway"
                    ),
                )
            };
            findings.push(finding(severity, "hooks-dir-shared", message));
        }
        Err(e) => return cannot(e.to_string()),
    }
    let mut steps = Vec::new();
    for (name, trigger) in HOOKS {
        let path = hooks.join(name);
        let file = match examine(&path, name, trigger) {
            Ok(file) => file,
            Err(e) => {
                return cannot(format!("could not read {}: {e}", path.display()));
            }
        };
        let step = plan(&file, name, trigger, replace);
        if matches!(&step, Step::Keep(f) if f.severity == Severity::Error) {
            refused = true;
        }
        steps.push(step);
    }
    if refused {
        findings.extend(steps.into_iter().filter_map(|s| match s {
            Step::Keep(f) if f.severity == Severity::Error => Some(f),
            _ => None,
        }));
        return verdict::verdict(findings, examined_hooks(), None);
    }
    let mut written: Vec<(PathBuf, Prior)> = Vec::new();
    for step in steps {
        match step {
            Step::Keep(f) => findings.push(f),
            Step::Write {
                name,
                trigger,
                path,
                done,
            } => {
                let prior = Prior::read(&path);
                if let Err(e) = write_hook(&hooks, &path, script(name, trigger).as_bytes(), 0o755) {
                    let mut error = format!("could not write {}: {e}", path.display());
                    for (path, prior) in written.iter().rev() {
                        if let Err(e) = prior.restore(&hooks, path) {
                            error.push_str(&format!("; could not restore {}: {e}", path.display()));
                        }
                    }
                    return cannot(error);
                }
                written.push((path, prior));
                findings.push(done);
            }
        }
    }
    if !root.join(config_file::PATH).exists() {
        let message = format!(
            "{} does not exist, so these hooks block every commit and push until one is added",
            config_file::PATH
        );
        findings.push(finding(Severity::Warning, "config-missing", message));
    }
    verdict::verdict(findings, examined_hooks(), None)
}

fn plan(file: &HookFile, name: &'static str, trigger: Trigger, replace: bool) -> Step {
    let shown = file.path.display().to_string();
    let write = |rule: &str, message: String| Step::Write {
        name,
        trigger,
        path: file.path.clone(),
        done: finding(Severity::Info, rule, message),
    };
    if file.symlink {
        return if replace {
            write(
                "hook-replaced",
                format!(
                    "{shown} was a symlink; the link itself is replaced by garita's {name} hook"
                ),
            )
        } else {
            Step::Keep(finding(
                Severity::Error,
                "hook-symlink",
                format!(
                    "{shown} is a symlink, which garita does not write through; pass --replace to replace the link with garita's hook"
                ),
            ))
        };
    }
    match file.kind {
        Kind::Missing => write("hook-installed", format!("installed {shown}")),
        Kind::Garita(format) => {
            if file.executable && format == HOOK_FORMAT {
                Step::Keep(finding(
                    Severity::Info,
                    "hook-unchanged",
                    format!("{shown} is up to date"),
                ))
            } else {
                write("hook-updated", format!("updated {shown}"))
            }
        }
        Kind::Foreign { .. } | Kind::Modified { .. } if replace => write(
            "hook-replaced",
            format!("{shown} was not garita's hook; --replace overwrote it"),
        ),
        Kind::Foreign { .. } | Kind::Modified { .. } => {
            let what = if matches!(file.kind, Kind::Modified { .. }) {
                "was written by garita and has been edited"
            } else {
                "is not garita's hook"
            };
            Step::Keep(finding(
                Severity::Error,
                "hook-foreign",
                format!(
                    "{shown} {what}, so nothing was written; pass --replace to overwrite it, or add the line `garita run --trigger {}` to it by hand",
                    trigger.as_str()
                ),
            ))
        }
    }
}

/// What stood at a hook's path before install wrote it, to put back when a later write fails.
enum Prior {
    Absent,
    File(Vec<u8>, u32),
    Link(PathBuf),
}

impl Prior {
    fn read(path: &Path) -> Self {
        if let Ok(target) = fs::read_link(path) {
            return Prior::Link(target);
        }
        match (fs::read(path), fs::metadata(path)) {
            (Ok(bytes), Ok(meta)) => Prior::File(bytes, mode_of(&meta)),
            _ => Prior::Absent,
        }
    }

    fn restore(&self, hooks: &Path, path: &Path) -> io::Result<()> {
        match self {
            Prior::Absent => fs::remove_file(path),
            Prior::File(bytes, mode) => write_hook(hooks, path, bytes, *mode),
            Prior::Link(target) => {
                fs::remove_file(path)?;
                make_link(target, path)
            }
        }
    }
}

/// Writes beside the hook and renames over it, so a symlink is replaced, never followed.
fn write_hook(hooks: &Path, path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    fs::create_dir_all(hooks)?;
    let temp = hooks.join(format!(".garita-hook-{}", ulid::Ulid::generate()));
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
        set_mode(&temp, mode)?;
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(_: &fs::Metadata) -> u32 {
    0o755
}

#[cfg(unix)]
fn make_link(target: &Path, path: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(not(unix))]
fn make_link(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::other("symlinks are not supported here"))
}

/// Removes the hooks garita wrote; any other file is left where it is.
pub fn uninstall(dir: &Path) -> Verdict {
    let (_, hooks) = match locate(dir) {
        Ok(found) => found,
        Err(e) => return cannot(e.to_string()),
    };
    let mut findings = Vec::new();
    for (name, trigger) in HOOKS {
        let path = hooks.join(name);
        let shown = path.display();
        let file = match examine(&path, name, trigger) {
            Ok(file) => file,
            Err(e) => return cannot(format!("could not read {shown}: {e}")),
        };
        let (rule, message) = match file.kind {
            Kind::Missing if !file.symlink => ("hook-absent", format!("{shown} does not exist")),
            Kind::Garita(_) => {
                if let Err(e) = fs::remove_file(&path) {
                    return cannot(format!("could not remove {shown}: {e}"));
                }
                ("hook-removed", format!("removed {shown}"))
            }
            Kind::Missing | Kind::Foreign { .. } | Kind::Modified { .. } => (
                "hook-foreign",
                format!("{shown} is not garita's hook, so it was left in place"),
            ),
        };
        findings.push(finding(Severity::Info, rule, message));
    }
    verdict::verdict(findings, examined_hooks(), None)
}
