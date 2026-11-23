//! `garita doctor`: whether the gate will actually run here. It reads and never writes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use garita_core::adapter::Adapter;
use garita_core::argv::Arg;
use garita_core::config::{Check, CheckForm, Config};
use garita_core::glob;
use garita_core::report::{Examined, Finding, ScopeKind, Severity};

use crate::config_file;
use crate::engine::scope::base_unresolved;
use crate::engine::session::requires_error;
use crate::git;
use crate::hooks::{self, HOOK_FORMAT, HOOKS, Kind};
use crate::receipts;
use crate::verdict::{self, Next, Verdict, finding};

const COMMAND: &str = "doctor";
const INSTALL: &str = "garita hooks install";
const RECEIPTS_LINE: &str = ".garita/receipts/";

/// What `garita doctor` looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Where it starts; the repository root is found from here.
    pub dir: PathBuf,
    /// The running garita's version, checked against the config's `requires`.
    pub version: String,
    /// The `PATH` that tools are looked up in, as the hooks would see it.
    pub path: Option<OsString>,
}

pub fn doctor(request: &Request) -> Verdict {
    let root = match git::root(&request.dir) {
        Ok(root) => root,
        Err(e) => return not_run(e.to_string()),
    };
    let loaded = match config_file::load(&root) {
        Ok(loaded) => loaded,
        Err(e) => return not_run(e.to_string()),
    };
    let config = &loaded.config;
    if let Some(error) = requires_error(config, &request.version) {
        return not_run(error);
    }
    let path = request.path.as_deref();
    let mut findings = match hook_findings(&root) {
        Ok(findings) => findings,
        Err(e) => return not_run(e),
    };
    if find_program("garita", &root, path).is_none() {
        let message = "garita is not on PATH, and every hook calls it by name, so each would fail"
            .to_string();
        findings.push(finding(
            Severity::Error,
            "doctor/garita-not-on-path",
            message,
        ));
    }
    for check in &config.checks {
        findings.extend(check_findings(&root, check, path));
    }
    match repository_findings(&root, config) {
        Ok(more) => findings.extend(more),
        Err(e) => return not_run(e),
    }
    let examined = Examined::from([
        (
            "checks".to_string(),
            u64::try_from(config.checks.len()).unwrap_or(u64::MAX),
        ),
        (
            "hooks".to_string(),
            u64::try_from(HOOKS.len()).unwrap_or(u64::MAX),
        ),
    ]);
    let next = next_action(&findings);
    verdict::verdict(COMMAND, findings, examined, next)
}

/// A doctor that cannot look has nothing to suggest but the error itself.
fn not_run(error: String) -> Verdict {
    let mut v = verdict::not_run(COMMAND, error);
    v.report.next_action = None;
    v.report.next_action_reason = None;
    v.summary = v
        .summary
        .lines()
        .filter(|l| !l.starts_with("  next:"))
        .collect::<Vec<_>>()
        .join("\n");
    v
}

fn hook_findings(root: &Path) -> Result<Vec<Finding>, String> {
    let dir = git::hooks_dir(root).map_err(|e| e.to_string())?;
    let mut findings = Vec::new();
    for (name, _) in HOOKS {
        let path = dir.join(name);
        let shown = path.display();
        let file = hooks::examine(&path).map_err(|e| format!("could not read {shown}: {e}"))?;
        let (wants_exec, found) = match file.kind {
            Kind::Missing => (
                false,
                Some((
                    Severity::Warning,
                    "doctor/hook-missing",
                    format!("{shown} does not exist; run `{INSTALL}`"),
                )),
            ),
            Kind::Garita(format) if format < HOOK_FORMAT => (
                true,
                Some((
                    Severity::Warning,
                    "doctor/hook-outdated",
                    format!(
                        "{shown} is hook format {format}, older than {HOOK_FORMAT}; run `{INSTALL}`"
                    ),
                )),
            ),
            Kind::Garita(_) => (true, None),
            Kind::Foreign { calls_garita: true } => (
                true,
                Some((
                    Severity::Info,
                    "doctor/hook-calls-garita",
                    format!("{shown} is not garita's hook, but it runs `garita run`"),
                )),
            ),
            Kind::Foreign {
                calls_garita: false,
            } => (
                false,
                Some((
                    Severity::Warning,
                    "doctor/hook-foreign",
                    format!(
                        "{shown} is not garita's hook and does not run `garita run`, so the gate does not run there"
                    ),
                )),
            ),
        };
        findings.extend(found.map(|(s, rule, m)| finding(s, rule, m)));
        if wants_exec && !file.executable {
            let message = format!("{shown} is not executable, so git skips it; run `{INSTALL}`");
            findings.push(finding(
                Severity::Error,
                "doctor/hook-not-executable",
                message,
            ));
        }
    }
    Ok(findings)
}

fn check_findings(root: &Path, check: &Check, path: Option<&OsStr>) -> Vec<Finding> {
    let name = &check.name;
    let mut findings = Vec::new();
    let mut base = root.to_path_buf();
    if let Some(cwd) = &check.cwd {
        base = root.join(cwd);
        if !base.is_dir() {
            let message =
                format!("checks.{name}: the directory `{cwd}` given as `cwd` does not exist");
            findings.push(finding(Severity::Error, "doctor/cwd-missing", message));
        }
    }
    for pattern in check.partition.iter().flat_map(|p| &p.roots) {
        if !partition_root_exists(root, pattern) {
            let message =
                format!("checks.{name}: no directory matches the partition root `{pattern}`");
            findings.push(finding(
                Severity::Error,
                "doctor/partition-root-missing",
                message,
            ));
        }
    }
    match &check.form {
        CheckForm::Run {
            command, parser, ..
        } => {
            if let Err(e) = Adapter::from_parser_name(parser) {
                let message = format!("checks.{name}: {e}");
                findings.push(finding(Severity::Error, "doctor/unknown-parser", message));
            }
            if let Some(Arg::Literal(program)) = command.args().first() {
                if base.is_dir() && find_program(program, &base, path).is_none() {
                    let message = format!(
                        "checks.{name}: the program `{program}` cannot be found or is not executable"
                    );
                    findings.push(finding(Severity::Error, "doctor/tool-missing", message));
                }
            }
        }
        CheckForm::Native { .. } | CheckForm::Builtin { .. } => {
            let message = format!(
                "checks.{name}: this version of garita does not run a `native` or `builtin` check, so a run that selects it is not-run"
            );
            findings.push(finding(
                Severity::Warning,
                "doctor/form-unsupported",
                message,
            ));
        }
    }
    findings
}

fn partition_root_exists(root: &Path, pattern: &str) -> bool {
    if root.join(pattern).is_dir() {
        return true;
    }
    let Ok(files) = git::all_files(root) else {
        return false;
    };
    files.iter().any(|file| {
        let file = file.to_string_lossy().replace('\\', "/");
        file.match_indices('/')
            .filter_map(|(at, _)| file.get(..at))
            .any(|dir| glob::any_matches(&[pattern.to_string()], dir) && root.join(dir).is_dir())
    })
}

/// Resolves a program as `execvp` would: a name with a `/` against `base`, else along `PATH`.
pub fn find_program(program: &str, base: &Path, path: Option<&OsStr>) -> Option<PathBuf> {
    if program.contains('/') {
        let candidate = base.join(program);
        return hooks::is_executable(&candidate).then_some(candidate);
    }
    std::env::split_paths(path?)
        .map(|dir| {
            if dir.as_os_str().is_empty() {
                base.join(program)
            } else {
                base.join(dir).join(program)
            }
        })
        .find(|candidate| hooks::is_executable(candidate))
}

fn repository_findings(root: &Path, config: &Config) -> Result<Vec<Finding>, String> {
    let mut findings = Vec::new();
    if !receipts::ignored_by_git(root).map_err(|e| e.to_string())? {
        let message = format!(
            "git does not ignore .garita/receipts/, so receipts would show up as changes; add the line `{RECEIPTS_LINE}` to .gitignore"
        );
        findings.push(finding(
            Severity::Warning,
            "doctor/receipts-not-ignored",
            message,
        ));
    }
    if git::is_shallow(root).map_err(|e| e.to_string())? {
        let message = "this is a shallow clone, so the `changed` scope widens to all tracked files when its merge base is missing".to_string();
        findings.push(finding(Severity::Info, "doctor/shallow", message));
    }
    let mut base: Option<Option<String>> = None;
    for (_, trigger) in HOOKS {
        let t = trigger.as_str();
        if !config.checks.iter().any(|c| c.on.contains(&trigger)) {
            let message = format!("no check runs at the `{t}` trigger");
            findings.push(finding(
                Severity::Info,
                "doctor/trigger-without-checks",
                message,
            ));
            continue;
        }
        if !matches!(
            config.trigger(trigger).scope,
            ScopeKind::Changed | ScopeKind::Affected
        ) {
            continue;
        }
        let why = match &base {
            Some(why) => why.clone(),
            None => {
                let why = base_unresolved(root).map_err(|e| e.to_string())?;
                base = Some(why.clone());
                why
            }
        };
        if let Some(why) = why {
            let message = format!("the `{t}` trigger's scope has no base: {why}");
            findings.push(finding(Severity::Info, "doctor/base-unresolved", message));
        }
    }
    Ok(findings)
}

/// The command that fixes the first error, else the first warning, when one does.
fn next_action(findings: &[Finding]) -> Next {
    let first = findings
        .iter()
        .find(|f| f.severity == Severity::Error)
        .or_else(|| findings.iter().find(|f| f.severity == Severity::Warning))?;
    let action = match first.rule.as_str() {
        "doctor/hook-missing" | "doctor/hook-outdated" | "doctor/hook-not-executable" => {
            INSTALL.to_string()
        }
        "doctor/receipts-not-ignored" => format!("echo '{RECEIPTS_LINE}' >> .gitignore"),
        _ => return None,
    };
    Some((action, first.message.clone()))
}
