use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use garita_core::adapter::{Adapter, complete_turbo_argv};
use garita_core::argv::{Command, Placeholder};
use garita_core::config::{Check, CheckForm, Config};
use garita_core::glob;
use garita_core::report::{Finding, Severity, Trigger};

use super::finding::{finding, plural};
use super::scope::Resolved;
use crate::git;

/// One invocation of the tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub argv: Vec<String>,
    /// Repository-relative, empty for the root.
    pub cwd: String,
    pub files: u64,
}

#[derive(Debug, Clone)]
pub struct Plan<'a> {
    pub check: &'a Check,
    pub adapter: Adapter,
    pub units: Vec<Unit>,
    /// The check's input, repository-relative.
    pub files: Vec<String>,
    /// How many files the tool was given; `None` when the command has no `{files}`.
    pub given: Option<u64>,
    pub findings: Vec<Finding>,
    pub timeout: Duration,
    pub trigger: Trigger,
    pub turbo: bool,
    pub env: Vec<(OsString, OsString)>,
}

#[derive(Debug, Clone)]
pub enum Planned<'a> {
    Run(Plan<'a>),
    Skip {
        check: &'a Check,
        files: Vec<String>,
        findings: Vec<Finding>,
    },
    NotRun {
        check: &'a Check,
        reason: String,
    },
}

pub fn plan<'a>(
    root: &Path,
    config: &Config,
    trigger: Trigger,
    check: &'a Check,
    scope: &Resolved,
) -> Planned<'a> {
    match try_plan(root, config, trigger, check, scope) {
        Ok(planned) => planned,
        Err(reason) => Planned::NotRun {
            check,
            reason: format!("checks.{}: {reason}", check.name),
        },
    }
}

fn try_plan<'a>(
    root: &Path,
    config: &Config,
    trigger: Trigger,
    check: &'a Check,
    scope: &Resolved,
) -> Result<Planned<'a>, String> {
    let CheckForm::Run {
        command, parser, ..
    } = &check.form
    else {
        return Err("only a `run` check can be planned".to_string());
    };
    let adapter = Adapter::from_parser_name(parser).map_err(|e| e.to_string())?;
    let turbo = check.via.is_some();
    let files: Vec<String> = scope
        .files
        .iter()
        .filter(|f| check.globs.is_empty() || glob::any_matches(&check.globs, f))
        .filter(|f| !glob::any_matches(&check.exclude, f))
        .cloned()
        .collect();
    let wants_files = command.placeholders().contains(&Placeholder::Files);
    let (groups, dropped) = groups(root, check, &files, wants_files)?;
    let mut findings = Vec::new();
    if dropped > 0 {
        let message = format!(
            "{} in scope lie outside every directory this check runs in, so it does not see them",
            plural(dropped, "file")
        );
        findings.push(finding(
            &check.name,
            Severity::Info,
            "outside-root",
            message,
        ));
    }
    let given: usize = groups.values().map(Vec::len).sum();
    if wants_files && given == 0 {
        let message = if files.is_empty() {
            "no file in scope matches this check, so it did not run".to_string()
        } else {
            "no file in scope lies where this check runs, so it did not run".to_string()
        };
        findings.push(finding(&check.name, Severity::Info, "no-input", message));
        return Ok(Planned::Skip {
            check,
            files,
            findings,
        });
    }
    let affected = if scope.affected() {
        vec!["--affected".to_string()]
    } else {
        Vec::new()
    };
    let mut units = Vec::new();
    for (cwd, rel) in groups {
        units.push(unit(command, adapter, turbo, cwd, &rel, &affected)?);
    }
    let env = match (&scope.base, turbo && scope.affected()) {
        (Some(base), true) => vec![(OsString::from("TURBO_SCM_BASE"), OsString::from(base))],
        _ => Vec::new(),
    };
    Ok(Planned::Run(Plan {
        check,
        adapter,
        units,
        files,
        given: wants_files.then(|| u64::try_from(given).unwrap_or(u64::MAX)),
        findings,
        timeout: config.timeout(check, trigger),
        trigger,
        turbo,
        env,
    }))
}

fn unit(
    command: &Command,
    adapter: Adapter,
    turbo: bool,
    cwd: String,
    files: &[String],
    affected: &[String],
) -> Result<Unit, String> {
    let args: Vec<String> = files
        .iter()
        .map(|f| {
            if f.starts_with('-') {
                format!("./{f}")
            } else {
                f.clone()
            }
        })
        .collect();
    let mut argv = command.expand(&args, affected);
    adapter.complete_argv(&mut argv, turbo)?;
    if turbo {
        complete_turbo_argv(&mut argv);
    }
    Ok(Unit {
        argv,
        cwd,
        files: u64::try_from(files.len()).unwrap_or(u64::MAX),
    })
}

type Groups = BTreeMap<String, Vec<String>>;

/// The directories the tool runs in, each with its files relative to it, and how many files
/// fell outside all of them.
fn groups(
    root: &Path,
    check: &Check,
    files: &[String],
    wants_files: bool,
) -> Result<(Groups, usize), String> {
    if let Some(partition) = &check.partition {
        let (mut groups, dropped) = by_root(files, &partition.roots);
        if !wants_files {
            let tracked = git::all_files(root).map_err(|e| e.to_string())?;
            let tracked: Vec<String> = tracked
                .iter()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect();
            for dir in by_root(&tracked, &partition.roots).0.into_keys() {
                groups.entry(dir).or_default();
            }
        }
        return Ok((groups, dropped));
    }
    let cwd = check.cwd.clone().unwrap_or_default();
    if !root.join(&cwd).is_dir() {
        return Err(format!(
            "the directory `{cwd}` given as `cwd` does not exist"
        ));
    }
    if cwd.is_empty() {
        return Ok((BTreeMap::from([(cwd, files.to_vec())]), 0));
    }
    let prefix = format!("{cwd}/");
    let inside: Vec<String> = files
        .iter()
        .filter_map(|f| f.strip_prefix(&prefix).map(str::to_string))
        .collect();
    let dropped = files.len() - inside.len();
    Ok((BTreeMap::from([(cwd, inside)]), dropped))
}

/// Groups files under the shallowest directory that a root glob matches.
fn by_root(files: &[String], roots: &[String]) -> (Groups, usize) {
    let mut groups = Groups::new();
    let mut dropped = 0;
    for file in files {
        let found = file
            .match_indices('/')
            .map(|(at, _)| file.get(..at).unwrap_or_default())
            .find(|dir| glob::any_matches(roots, dir));
        match found {
            Some(dir) => {
                let rel = file.get(dir.len() + 1..).unwrap_or_default().to_string();
                groups.entry(dir.to_string()).or_default().push(rel);
            }
            None => dropped += 1,
        }
    }
    (groups, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn files_group_under_the_root_that_holds_them() {
        let files = strings(&[
            "packages/app/src/a.ts",
            "packages/lib/b.ts",
            "tools/c.ts",
            "packages/d.ts",
        ]);
        let (groups, dropped) = by_root(&files, &strings(&["packages/*"]));
        assert_eq!(dropped, 2);
        assert_eq!(groups.get("packages/app"), Some(&strings(&["src/a.ts"])));
        assert_eq!(groups.get("packages/lib"), Some(&strings(&["b.ts"])));
        assert_eq!(groups.len(), 2);
    }
}
