//! `garita hooks install` and `uninstall`: the `pre-commit` and `pre-push` hooks (RFC-1 §7).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use garita_core::report::{Examined, Finding, Severity, Trigger};

use crate::config_file;
use crate::git;
use crate::verdict::{self, Verdict, finding};

/// Raised when the script changes in a way an installed hook must be rewritten for.
pub const HOOK_FORMAT: u32 = 1;
const MARKER: &str = "# garita hook format ";

/// The hooks garita installs, with the trigger each runs.
pub const HOOKS: [(&str, Trigger); 2] =
    [("pre-commit", Trigger::Commit), ("pre-push", Trigger::Push)];

/// The script for one hook. It fails closed when `garita` is missing and leaves the verdict's
/// exit code to `exec`.
pub fn script(name: &str, trigger: Trigger) -> String {
    let t = trigger.as_str();
    format!(
        "#!/bin/sh\n\
         {MARKER}{HOOK_FORMAT}: written by `garita hooks install`, which rewrites this file\n\
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
    /// Written by garita, in this hook format.
    Garita(u32),
    /// Someone else's; `calls_garita` when a line of it runs `garita run`.
    Foreign {
        calls_garita: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookFile {
    pub path: PathBuf,
    pub kind: Kind,
    pub symlink: bool,
    pub executable: bool,
    pub text: Option<String>,
}

pub fn examine(path: &Path) -> io::Result<HookFile> {
    let symlink = match fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type().is_symlink(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    let (text, executable) = match fs::read(path) {
        Ok(bytes) => (
            Some(String::from_utf8_lossy(&bytes).into_owned()),
            is_executable(path),
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (None, false),
        Err(e) => return Err(e),
    };
    let kind = match &text {
        None => Kind::Missing,
        Some(text) => match format_of(text) {
            Some(format) => Kind::Garita(format),
            None => Kind::Foreign {
                calls_garita: calls_garita(text),
            },
        },
    };
    Ok(HookFile {
        path: path.to_path_buf(),
        kind,
        symlink,
        executable,
        text,
    })
}

fn format_of(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(MARKER)?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    })
}

fn calls_garita(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        !line.starts_with('#') && line.contains("garita run")
    })
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
    const COMMAND: &str = "hooks install";
    let (root, hooks) = match locate(dir) {
        Ok(found) => found,
        Err(e) => return verdict::not_run(COMMAND, e.to_string()),
    };
    let mut findings = Vec::new();
    let mut refused = false;
    match inside_repository(&root, &hooks) {
        Ok(true) => {}
        Ok(false) => {
            let shown = hooks.display();
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
                        "{shown} (core.hooksPath) lies outside this repository and may be shared with others; pass --replace to write there anyway"
                    ),
                )
            };
            findings.push(finding(severity, "hooks-dir-shared", message));
        }
        Err(e) => return verdict::not_run(COMMAND, e.to_string()),
    }
    let mut steps = Vec::new();
    for (name, trigger) in HOOKS {
        let path = hooks.join(name);
        let file = match examine(&path) {
            Ok(file) => file,
            Err(e) => {
                return verdict::not_run(
                    COMMAND,
                    format!("could not read {}: {e}", path.display()),
                );
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
        return verdict::verdict(COMMAND, findings, examined_hooks(), None);
    }
    for step in steps {
        match step {
            Step::Keep(f) => findings.push(f),
            Step::Write {
                name,
                trigger,
                path,
                done,
            } => {
                if let Err(e) = write_hook(&hooks, &path, &script(name, trigger)) {
                    let error = format!("could not write {}: {e}", path.display());
                    return verdict::not_run(COMMAND, error);
                }
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
    verdict::verdict(COMMAND, findings, examined_hooks(), None)
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
        Kind::Garita(_) => {
            if file.executable && file.text.as_deref() == Some(script(name, trigger).as_str()) {
                Step::Keep(finding(
                    Severity::Info,
                    "hook-unchanged",
                    format!("{shown} is up to date"),
                ))
            } else {
                write("hook-updated", format!("updated {shown}"))
            }
        }
        Kind::Foreign { .. } if replace => write(
            "hook-replaced",
            format!("{shown} was not garita's hook; --replace overwrote it"),
        ),
        Kind::Foreign { .. } => Step::Keep(finding(
            Severity::Error,
            "hook-foreign",
            format!(
                "{shown} is not garita's hook, so nothing was written; pass --replace to overwrite it, or add the line `garita run --trigger {}` to it by hand",
                trigger.as_str()
            ),
        )),
    }
}

/// Writes beside the hook and renames over it, so a symlink is replaced, never followed.
fn write_hook(hooks: &Path, path: &Path, text: &str) -> io::Result<()> {
    fs::create_dir_all(hooks)?;
    let temp = hooks.join(format!(".garita-hook-{}", ulid::Ulid::generate()));
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        set_mode(&temp)?;
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

#[cfg(unix)]
fn set_mode(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_mode(_: &Path) -> io::Result<()> {
    Ok(())
}

/// Removes the hooks garita wrote; any other file is left where it is.
pub fn uninstall(dir: &Path) -> Verdict {
    const COMMAND: &str = "hooks uninstall";
    let (_, hooks) = match locate(dir) {
        Ok(found) => found,
        Err(e) => return verdict::not_run(COMMAND, e.to_string()),
    };
    let mut findings = Vec::new();
    for (name, _) in HOOKS {
        let path = hooks.join(name);
        let shown = path.display();
        let file = match examine(&path) {
            Ok(file) => file,
            Err(e) => return verdict::not_run(COMMAND, format!("could not read {shown}: {e}")),
        };
        let (rule, message) = match file.kind {
            Kind::Missing if !file.symlink => ("hook-absent", format!("{shown} does not exist")),
            Kind::Garita(_) => {
                if let Err(e) = fs::remove_file(&path) {
                    return verdict::not_run(COMMAND, format!("could not remove {shown}: {e}"));
                }
                ("hook-removed", format!("removed {shown}"))
            }
            Kind::Missing | Kind::Foreign { .. } => (
                "hook-foreign",
                format!("{shown} is not garita's hook, so it was left in place"),
            ),
        };
        findings.push(finding(Severity::Info, rule, message));
    }
    verdict::verdict(COMMAND, findings, examined_hooks(), None)
}
