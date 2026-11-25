//! The verdict garita prints (RFC-1 §5).

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

use crate::argv::join;

pub const SCHEMA_VERSION: u32 = 1;
pub const DOCTOR: &str = "garita doctor";

/// [`DOCTOR`] as its arguments.
pub fn doctor_argv() -> Vec<String> {
    vec!["garita".to_string(), "doctor".to_string()]
}

/// The next action after a usage error: `describe` lists the arguments each command takes.
pub fn describe_argv() -> Vec<String> {
    vec!["garita".to_string(), "describe".to_string()]
}

/// Why a command could not run (RFC-4 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorKind {
    /// The arguments were wrong.
    Usage,
    /// The configuration could not be loaded or is invalid.
    Config,
    /// Something outside garita is missing: a repository, a program, a directory.
    Environment,
    /// garita itself failed.
    Internal,
}

/// The `error` and `error_kinds` of a `not-run` report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunError {
    /// Never empty; in declaration order, each kind once.
    pub kinds: Vec<ErrorKind>,
    pub message: String,
}

impl RunError {
    /// `error` is one line (RFC-4 §4), so a message of several lines is joined with `; `.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        let message: String = message.into();
        let lines: Vec<&str> = message
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        Self {
            kinds: vec![kind],
            message: lines.join("; "),
        }
    }

    /// Keeps the kinds of `self` and replaces its message.
    pub fn with_message(&self, message: impl Into<String>) -> Self {
        let mut error = Self::new(self.kinds[0], message);
        error.kinds.clone_from(&self.kinds);
        error
    }

    /// One error for several reasons: every kind among them, the messages of all of them in
    /// order; `None` when there is no reason.
    pub fn combine(reasons: &[&Self]) -> Option<Self> {
        let first = reasons.first()?;
        let messages: Vec<&str> = reasons.iter().map(|r| r.message.as_str()).collect();
        let mut error = first.with_message(messages.join("; "));
        error.kinds = reasons
            .iter()
            .flat_map(|r| r.kinds.iter().copied())
            .collect();
        error.kinds.sort_unstable();
        error.kinds.dedup();
        Some(error)
    }

    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kinds.contains(&kind)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, message)
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Config, message)
    }

    pub fn environment(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Environment, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Commit,
    Push,
    Ci,
    Merge,
}

impl Trigger {
    pub const ALL: [Trigger; 4] = [Trigger::Commit, Trigger::Push, Trigger::Ci, Trigger::Merge];

    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Commit => "commit",
            Trigger::Push => "push",
            Trigger::Ci => "ci",
            Trigger::Merge => "merge",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Pass,
    Warn,
    Fail,
    NotRun,
}

impl RunStatus {
    pub fn exit_code(self) -> u8 {
        match self {
            RunStatus::Pass | RunStatus::Warn => 0,
            RunStatus::Fail => 1,
            RunStatus::NotRun => 2,
        }
    }

    pub fn blocking(self) -> bool {
        self.exit_code() == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skipped,
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckKind {
    Lint,
    Format,
    Types,
    Test,
    Build,
    Records,
    Other,
}

impl CheckKind {
    pub const ALL: [CheckKind; 7] = [
        CheckKind::Lint,
        CheckKind::Format,
        CheckKind::Types,
        CheckKind::Test,
        CheckKind::Build,
        CheckKind::Records,
        CheckKind::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CheckKind::Lint => "lint",
            CheckKind::Format => "format",
            CheckKind::Types => "types",
            CheckKind::Test => "test",
            CheckKind::Build => "build",
            CheckKind::Records => "records",
            CheckKind::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    Staged,
    Changed,
    Affected,
    All,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 4] = [
        ScopeKind::Staged,
        ScopeKind::Changed,
        ScopeKind::Affected,
        ScopeKind::All,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Staged => "staged",
            ScopeKind::Changed => "changed",
            ScopeKind::Affected => "affected",
            ScopeKind::All => "all",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub kind: ScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages: Option<Vec<String>>,
}

/// One count per unit (`files`, `tests`, `packages`), in key order.
pub type Examined = BTreeMap<String, u64>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The check that produced it; absent on a finding about the run itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    pub severity: Severity,
    pub rule: String,
    pub file: Option<String>,
    pub line: Option<NonZeroU64>,
    pub column: Option<NonZeroU64>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// `fix` as its arguments; present exactly when `fix` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckEntry {
    pub name: String,
    pub kind: CheckKind,
    pub status: CheckStatus,
    pub duration_ms: u64,
    pub examined: Examined,
    pub rerun: Option<String>,
    /// The last characters of the check's output, as its receipt check holds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tail: Option<String>,
    /// The repository-relative path of the check's full output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Lines picked out of a failing output as the likely reason; garita's own key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_notable: Vec<String>,
    /// Tool findings below the check's `min_severity`, counted by severity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suppressed: BTreeMap<Severity, u64>,
}

impl CheckEntry {
    /// An entry with no output, rerun or suppressed findings.
    pub fn new(name: &str, kind: CheckKind, status: CheckStatus) -> Self {
        Self {
            name: name.to_string(),
            kind,
            status,
            duration_ms: 0,
            examined: Examined::new(),
            rerun: None,
            output_tail: None,
            output: None,
            output_notable: Vec::new(),
            suppressed: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextActionInput {
    pub placeholder: String,
    pub flag: String,
    pub means: String,
}

/// Where a run happened; every field may be unknown when the run could not start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunContext {
    pub trigger: Option<Trigger>,
    pub commit: Option<String>,
    pub tree: Option<String>,
    pub scope: Option<Scope>,
    pub receipt: Option<String>,
    /// Checks whose command takes no `{files}`, so `--files` cannot narrow their re-run.
    pub without_files: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub status: RunStatus,
    pub blocking: bool,
    pub trigger: Option<Trigger>,
    pub commit: Option<String>,
    pub tree: Option<String>,
    pub scope: Option<Scope>,
    pub checks: Vec<CheckEntry>,
    pub findings: Vec<Finding>,
    pub examined: Examined,
    pub next_action: Option<String>,
    pub next_action_reason: Option<String>,
    /// `next_action` as its arguments; null exactly when it is.
    #[serde(default)]
    pub next_action_argv: Option<Vec<String>>,
    pub next_action_inputs: Vec<NextActionInput>,
    pub receipt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kinds: Option<Vec<ErrorKind>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextAction {
    pub action: Option<String>,
    pub argv: Option<Vec<String>>,
    pub reason: Option<String>,
}

impl NextAction {
    pub const NONE: NextAction = NextAction {
        action: None,
        argv: None,
        reason: None,
    };

    /// The command `argv`, printed from it so the two forms agree.
    pub fn run(argv: Vec<String>, reason: String) -> Self {
        Self {
            action: Some(join(&argv)),
            argv: Some(argv),
            reason: Some(reason),
        }
    }
}

impl Report {
    /// Builds the report for a run; an `error` makes it `not-run` whatever its checks say.
    pub fn from_run(
        context: RunContext,
        checks: Vec<CheckEntry>,
        findings: Vec<Finding>,
        error: Option<RunError>,
    ) -> Self {
        let status = run_status(&checks, &findings, error.is_some());
        let error = match (status, error) {
            (RunStatus::NotRun, None) => Some(RunError::environment(not_run_error(&checks))),
            (_, error) => error,
        };
        let next = if error.as_ref().is_some_and(|e| e.is(ErrorKind::Usage)) {
            NextAction::run(
                describe_argv(),
                "the arguments were wrong; describe lists the ones each command takes".to_string(),
            )
        } else {
            next_action_for(
                context.trigger,
                status,
                &checks,
                &findings,
                &context.without_files,
            )
        };
        Self {
            schema_version: SCHEMA_VERSION,
            status,
            blocking: status.blocking(),
            trigger: context.trigger,
            commit: context.commit,
            tree: context.tree,
            scope: context.scope,
            examined: total_examined(&checks),
            checks,
            findings,
            next_action: next.action,
            next_action_reason: next.reason,
            next_action_argv: next.argv,
            next_action_inputs: Vec::new(),
            receipt: context.receipt,
            error_kinds: error.as_ref().map(|e| e.kinds.clone()),
            error: error.map(|e| e.message),
        }
    }

    /// A report for a run that could not start: no checks, no findings.
    pub fn not_run(context: RunContext, error: RunError) -> Self {
        Self::from_run(context, Vec::new(), Vec::new(), Some(error))
    }

    /// Replaces the next action, keeping its string and argv forms in step.
    pub fn set_next(&mut self, next: NextAction) {
        self.next_action = next.action;
        self.next_action_argv = next.argv;
        self.next_action_reason = next.reason;
    }

    pub fn exit_code(&self) -> u8 {
        self.status.exit_code()
    }
}

/// `not-run` wins when outcomes mix; then any error fails and any warning warns.
pub fn run_status(checks: &[CheckEntry], findings: &[Finding], could_not_start: bool) -> RunStatus {
    if could_not_start || checks.iter().any(|c| c.status == CheckStatus::NotRun) {
        RunStatus::NotRun
    } else if any_status(checks, CheckStatus::Fail) || any_severity(findings, Severity::Error) {
        RunStatus::Fail
    } else if any_status(checks, CheckStatus::Warn) || any_severity(findings, Severity::Warning) {
        RunStatus::Warn
    } else {
        RunStatus::Pass
    }
}

fn any_status(checks: &[CheckEntry], status: CheckStatus) -> bool {
    checks.iter().any(|c| c.status == status)
}

fn any_severity(findings: &[Finding], severity: Severity) -> bool {
    findings.iter().any(|f| f.severity == severity)
}

pub fn total_examined(checks: &[CheckEntry]) -> Examined {
    let mut total = Examined::new();
    for check in checks {
        for (unit, count) in &check.examined {
            let sum = total.entry(unit.clone()).or_insert(0);
            *sum = sum.saturating_add(*count);
        }
    }
    total
}

fn not_run_error(checks: &[CheckEntry]) -> String {
    let names: Vec<&str> = checks
        .iter()
        .filter(|c| c.status == CheckStatus::NotRun)
        .map(|c| c.name.as_str())
        .collect();
    format!("could not run: {}", names.join(", "))
}

pub fn next_action(
    trigger: Option<Trigger>,
    status: RunStatus,
    checks: &[CheckEntry],
    findings: &[Finding],
) -> NextAction {
    next_action_for(trigger, status, checks, findings, &BTreeSet::new())
}

/// [`next_action`] knowing which checks take no `{files}`.
pub fn next_action_for(
    trigger: Option<Trigger>,
    status: RunStatus,
    checks: &[CheckEntry],
    findings: &[Finding],
    without_files: &BTreeSet<String>,
) -> NextAction {
    match status {
        RunStatus::Pass | RunStatus::Warn => NextAction::NONE,
        RunStatus::NotRun => NextAction::run(doctor_argv(), not_run_reason(checks)),
        RunStatus::Fail => match failing_rerun(trigger, checks, findings, without_files) {
            Some(argv) => NextAction::run(argv, fail_reason(checks, findings)),
            None => NextAction::NONE,
        },
    }
}

fn not_run_reason(checks: &[CheckEntry]) -> String {
    let count = checks
        .iter()
        .filter(|c| c.status == CheckStatus::NotRun)
        .count();
    if count == 0 {
        "the run could not start".to_string()
    } else {
        format!("{} could not run", plural(count, "check"))
    }
}

/// `N errors in M checks`, with `, K for the run` when some belong to no listed check.
fn fail_reason(checks: &[CheckEntry], findings: &[Finding]) -> String {
    let mut names: Vec<&str> = Vec::new();
    let mut errors = 0;
    let mut run_level = 0;
    for f in findings.iter().filter(|f| f.severity == Severity::Error) {
        errors += 1;
        match f
            .check
            .as_deref()
            .filter(|n| checks.iter().any(|c| c.name == *n))
        {
            None => run_level += 1,
            Some(name) if !names.contains(&name) => names.push(name),
            Some(_) => {}
        }
    }
    if errors == 0 {
        let failed = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .count();
        return format!("{} failed", plural(failed, "check"));
    }
    match (names.len(), run_level) {
        (_, 0) => format!(
            "{} in {}",
            plural(errors, "error"),
            plural(names.len(), "check")
        ),
        (0, _) => format!("{} for the run", plural(errors, "error")),
        (n, k) => format!(
            "{}: {} in {}, {k} for the run",
            plural(errors, "error"),
            errors - k,
            plural(n, "check")
        ),
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// The narrowest re-run of what failed: each listed check that failed or has an error, then any
/// unlisted check with an error, with their error files only when every one of them has some.
fn failing_rerun(
    trigger: Option<Trigger>,
    checks: &[CheckEntry],
    findings: &[Finding],
    without_files: &BTreeSet<String>,
) -> Option<Vec<String>> {
    let has_error = |name: &str| {
        findings
            .iter()
            .any(|f| f.check.as_deref() == Some(name) && f.severity == Severity::Error)
    };
    let names: Vec<&str> = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail || has_error(&c.name))
        .map(|c| c.name.as_str())
        .collect();
    let run_level = findings.iter().any(|f| {
        f.severity == Severity::Error
            && !f
                .check
                .as_deref()
                .is_some_and(|n| checks.iter().any(|c| c.name == n))
    });
    // An error that belongs to no listed check (a trigger's budget) can only be re-run by
    // running the trigger again, so it widens the re-run to the whole trigger.
    if run_level {
        return Some(rerun_argv(trigger, &[], &[]));
    }
    let first = *names.first()?;
    if names.iter().any(|n| without_files.contains(*n)) {
        return Some(rerun_argv(trigger, &names, &[]));
    }
    if names.len() == 1 {
        return Some(rerun_argv(trigger, &[first], &error_files(first, findings)));
    }
    let mut files: Vec<&str> = Vec::new();
    for name in &names {
        let own = error_files(name, findings);
        if own.is_empty() {
            return Some(rerun_argv(trigger, &names, &[]));
        }
        for file in own {
            if !files.contains(&file) {
                files.push(file);
            }
        }
    }
    Some(rerun_argv(trigger, &names, &files))
}

fn error_files<'a>(check: &str, findings: &'a [Finding]) -> Vec<&'a str> {
    let mut files: Vec<&str> = Vec::new();
    for f in findings
        .iter()
        .filter(|f| f.check.as_deref() == Some(check) && f.severity == Severity::Error)
    {
        if let Some(file) = f.file.as_deref() {
            if !files.contains(&file) {
                files.push(file);
            }
        }
    }
    files
}

/// `garita run [--trigger <t>] <check> --files ...`; `--trigger` is left out at `commit`, the
/// default, and a file beginning with `-` puts the files after `--`.
pub fn rerun_command(trigger: Option<Trigger>, check: &str, files: &[&str]) -> String {
    rerun_checks(trigger, &[check], files)
}

/// [`rerun_command`] for several checks at once.
pub fn rerun_checks(trigger: Option<Trigger>, checks: &[&str], files: &[&str]) -> String {
    join(&rerun_argv(trigger, checks, files))
}

/// [`rerun_checks`] as its arguments.
pub fn rerun_argv(trigger: Option<Trigger>, checks: &[&str], files: &[&str]) -> Vec<String> {
    let mut argv = vec!["garita".to_string(), "run".to_string()];
    if let Some(t) = trigger.filter(|t| *t != Trigger::Commit) {
        argv.extend(["--trigger".to_string(), t.as_str().to_string()]);
    }
    argv.extend(checks.iter().map(|c| (*c).to_string()));
    if !files.is_empty() {
        argv.push("--files".to_string());
        if files.iter().any(|f| f.starts_with('-')) {
            argv.push("--".to_string());
        }
        argv.extend(files.iter().map(|f| (*f).to_string()));
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_error_kinds_are_each_kind_once_in_order_for_every_pair() {
        let kinds = [
            ErrorKind::Usage,
            ErrorKind::Config,
            ErrorKind::Environment,
            ErrorKind::Internal,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                let (first, second) = (RunError::new(*a, "a"), RunError::new(*b, "b"));
                let mut want = vec![kinds[i.min(j)], kinds[i.max(j)]];
                want.dedup();
                for reasons in [[&first, &second], [&second, &first]] {
                    let got = RunError::combine(&reasons).unwrap();
                    assert_eq!(got.kinds, want, "{a:?} with {b:?}");
                }
                assert_eq!(
                    RunError::combine(&[&first, &second]).unwrap().message,
                    "a; b"
                );
            }
        }
        assert_eq!(RunError::combine(&[]), None);
    }

    const RFC_EXAMPLE: &str = r#"{
      "schema_version": 1,
      "status": "fail",
      "blocking": true,
      "trigger": "push",
      "commit": "1b29de5ea3f04c7d9a6b2e8f5c1d0a7b3e9f4c62",
      "tree": "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80",
      "scope": { "kind": "affected", "packages": ["frontend"] },
      "checks": [
        {
          "name": "lint",
          "kind": "lint",
          "status": "fail",
          "duration_ms": 412,
          "examined": { "files": 38 },
          "rerun": "garita run --trigger push lint --files ts/frontend/src/App.tsx"
        }
      ],
      "findings": [
        {
          "check": "lint",
          "severity": "error",
          "rule": "oxlint/no-debugger",
          "file": "ts/frontend/src/App.tsx",
          "line": 1302,
          "column": 5,
          "message": "Unexpected `debugger` statement",
          "fix": "oxlint --fix ts/frontend/src/App.tsx",
          "fix_argv": ["oxlint", "--fix", "ts/frontend/src/App.tsx"],
          "fingerprint": "5d1c0e9a"
        }
      ],
      "examined": { "files": 38 },
      "next_action": "garita run --trigger push lint --files ts/frontend/src/App.tsx",
      "next_action_reason": "1 error in 1 check",
      "next_action_argv": ["garita", "run", "--trigger", "push", "lint", "--files", "ts/frontend/src/App.tsx"],
      "next_action_inputs": [],
      "receipt": ".garita/receipts/20261121T031502Z-push-01M7XM3Q4T8Z6R2K9V5B1N0HCW.json"
    }"#;

    fn check(name: &str, status: CheckStatus, files: u64) -> CheckEntry {
        CheckEntry {
            duration_ms: 1,
            examined: [("files".to_string(), files)].into(),
            ..CheckEntry::new(name, CheckKind::Lint, status)
        }
    }

    fn finding(check: &str, severity: Severity, file: Option<&str>) -> Finding {
        Finding {
            check: Some(check.into()),
            severity,
            rule: "r".into(),
            file: file.map(Into::into),
            line: None,
            column: None,
            message: "m".into(),
            fix: None,
            fix_argv: None,
            fingerprint: None,
        }
    }

    #[test]
    fn rfc_example_round_trips() {
        let value: serde_json::Value = serde_json::from_str(RFC_EXAMPLE).unwrap();
        let report: Report = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), value);
    }

    #[test]
    fn rfc_example_is_what_from_run_builds() {
        let expected: Report = serde_json::from_str(RFC_EXAMPLE).unwrap();
        let context = RunContext {
            trigger: expected.trigger,
            commit: expected.commit.clone(),
            tree: expected.tree.clone(),
            scope: expected.scope.clone(),
            receipt: expected.receipt.clone(),
            without_files: BTreeSet::new(),
        };
        let built = Report::from_run(
            context,
            expected.checks.clone(),
            expected.findings.clone(),
            None,
        );
        assert_eq!(built, expected);
    }

    #[test]
    fn exit_codes_and_blocking() {
        assert_eq!(
            (RunStatus::Pass.exit_code(), RunStatus::Pass.blocking()),
            (0, false)
        );
        assert_eq!(
            (RunStatus::Warn.exit_code(), RunStatus::Warn.blocking()),
            (0, false)
        );
        assert_eq!(
            (RunStatus::Fail.exit_code(), RunStatus::Fail.blocking()),
            (1, true)
        );
        assert_eq!(
            (RunStatus::NotRun.exit_code(), RunStatus::NotRun.blocking()),
            (2, false)
        );
    }

    #[test]
    fn no_findings_pass() {
        assert_eq!(
            run_status(&[check("a", CheckStatus::Pass, 1)], &[], false),
            RunStatus::Pass
        );
        assert_eq!(run_status(&[], &[], false), RunStatus::Pass);
    }

    #[test]
    fn info_findings_still_pass() {
        let f = [finding("a", Severity::Info, None)];
        assert_eq!(run_status(&[], &f, false), RunStatus::Pass);
    }

    #[test]
    fn a_warning_warns() {
        let f = [
            finding("a", Severity::Info, None),
            finding("a", Severity::Warning, None),
        ];
        assert_eq!(run_status(&[], &f, false), RunStatus::Warn);
    }

    #[test]
    fn an_error_fails_over_warnings() {
        let f = [
            finding("a", Severity::Warning, None),
            finding("b", Severity::Error, None),
        ];
        assert_eq!(run_status(&[], &f, false), RunStatus::Fail);
    }

    #[test]
    fn not_run_wins_over_errors() {
        let checks = [
            check("a", CheckStatus::Fail, 1),
            check("b", CheckStatus::NotRun, 0),
        ];
        let f = [finding("a", Severity::Error, None)];
        assert_eq!(run_status(&checks, &f, false), RunStatus::NotRun);
        assert_eq!(run_status(&[], &f, true), RunStatus::NotRun);
    }

    #[test]
    fn examined_sums_per_unit_in_key_order() {
        let mut b = check("b", CheckStatus::Pass, 2);
        b.examined.insert("tests".into(), 7);
        b.examined.insert("packages".into(), 1);
        let total = total_examined(&[check("a", CheckStatus::Pass, 3), b]);
        assert_eq!(
            serde_json::to_string(&total).unwrap(),
            r#"{"files":5,"packages":1,"tests":7}"#
        );
        assert!(total_examined(&[]).is_empty());
    }

    #[test]
    fn next_action_is_null_on_pass_and_warn() {
        for status in [RunStatus::Pass, RunStatus::Warn] {
            let next = next_action(
                None,
                status,
                &[],
                &[finding("a", Severity::Warning, Some("x"))],
            );
            assert_eq!(next, NextAction::NONE);
        }
    }

    #[test]
    fn next_action_is_doctor_on_not_run() {
        let next = next_action(None, RunStatus::NotRun, &[], &[]);
        assert_eq!(next.action.as_deref(), Some(DOCTOR));
        assert_eq!(next.reason.as_deref(), Some("the run could not start"));
        let checks = [
            check("a", CheckStatus::NotRun, 0),
            check("b", CheckStatus::NotRun, 0),
        ];
        let next = next_action(None, RunStatus::NotRun, &checks, &[]);
        assert_eq!(next.reason.as_deref(), Some("2 checks could not run"));
    }

    #[test]
    fn fail_reruns_every_failing_check_on_their_error_files() {
        let checks = [
            check("format", CheckStatus::Pass, 1),
            check("lint", CheckStatus::Fail, 1),
            check("types", CheckStatus::Fail, 1),
        ];
        let f = [
            finding("types", Severity::Error, Some("t.ts")),
            finding("lint", Severity::Warning, Some("w.ts")),
            finding("lint", Severity::Error, Some("b.ts")),
            finding("lint", Severity::Error, Some("a b.ts")),
            finding("lint", Severity::Error, Some("b.ts")),
            finding("lint", Severity::Error, None),
        ];
        let next = next_action(None, RunStatus::Fail, &checks, &f);
        assert_eq!(
            next.action.as_deref(),
            Some("garita run lint types --files b.ts 'a b.ts' t.ts")
        );
        assert_eq!(next.reason.as_deref(), Some("5 errors in 2 checks"));
    }

    #[test]
    fn two_failing_checks_rerun_together_without_files_when_one_has_none() {
        let checks = [
            check("lint", CheckStatus::Fail, 1),
            check("types", CheckStatus::Fail, 1),
        ];
        let f = [
            finding("lint", Severity::Error, Some("a.ts")),
            finding("types", Severity::Error, None),
        ];
        let next = next_action(Some(Trigger::Push), RunStatus::Fail, &checks, &f);
        assert_eq!(
            next.action.as_deref(),
            Some("garita run --trigger push lint types")
        );
    }

    #[test]
    fn files_are_left_out_when_a_failing_check_takes_none() {
        let checks = [
            check("lint", CheckStatus::Fail, 1),
            check("lint-all", CheckStatus::Fail, 1),
        ];
        let f = [
            finding("lint", Severity::Error, Some("a.ts")),
            finding("lint-all", Severity::Error, Some("b.ts")),
        ];
        let none = BTreeSet::from(["lint-all".to_string()]);
        let next = next_action_for(Some(Trigger::Push), RunStatus::Fail, &checks, &f, &none);
        assert_eq!(
            next.action.as_deref(),
            Some("garita run --trigger push lint lint-all")
        );
        let next = next_action_for(None, RunStatus::Fail, &checks[1..], &f[1..], &none);
        assert_eq!(next.action.as_deref(), Some("garita run lint-all"));
        let next = next_action_for(None, RunStatus::Fail, &checks, &f, &BTreeSet::new());
        assert_eq!(
            next.action.as_deref(),
            Some("garita run lint lint-all --files a.ts b.ts")
        );
    }

    #[test]
    fn a_run_level_error_is_not_counted_as_a_check() {
        let checks = [check("a", CheckStatus::Fail, 1)];
        let f = [
            finding("a", Severity::Error, None),
            finding("garita", Severity::Error, None),
        ];
        let next = next_action(None, RunStatus::Fail, &checks, &f);
        assert_eq!(next.action.as_deref(), Some("garita run"));
        assert_eq!(
            next.reason.as_deref(),
            Some("2 errors: 1 in 1 check, 1 for the run")
        );
        let only = [finding("garita", Severity::Error, None)];
        let next = next_action(None, RunStatus::Fail, &checks, &only);
        assert_eq!(next.reason.as_deref(), Some("1 error for the run"));
    }

    #[test]
    fn an_error_outside_every_check_reruns_the_whole_trigger() {
        let checks = [
            check("lint", CheckStatus::Fail, 1),
            check("types", CheckStatus::Fail, 1),
        ];
        let f = [
            finding("lint", Severity::Error, Some("a.ts")),
            finding("types", Severity::Error, None),
            finding("budget", Severity::Error, None),
        ];
        let next = next_action(Some(Trigger::Push), RunStatus::Fail, &checks, &f);
        assert_eq!(next.action.as_deref(), Some("garita run --trigger push"));
        assert_eq!(
            next.reason.as_deref(),
            Some("3 errors: 2 in 2 checks, 1 for the run")
        );
    }

    #[test]
    fn fail_without_files_reruns_the_whole_check() {
        let checks = [check("types", CheckStatus::Fail, 1)];
        let f = [finding("types", Severity::Error, None)];
        let next = next_action(None, RunStatus::Fail, &checks, &f);
        assert_eq!(next.action.as_deref(), Some("garita run types"));
        assert_eq!(next.reason.as_deref(), Some("1 error in 1 check"));
    }

    #[test]
    fn fail_from_a_finding_whose_check_is_not_listed() {
        let f = [finding("budget", Severity::Error, None)];
        let next = next_action(None, RunStatus::Fail, &[], &f);
        assert_eq!(next.action.as_deref(), Some("garita run"));
    }

    #[test]
    fn fail_without_any_error_finding_has_no_action() {
        let next = next_action(
            None,
            RunStatus::Fail,
            &[],
            &[finding("a", Severity::Warning, None)],
        );
        assert_eq!(next, NextAction::NONE);
    }

    fn verdict(checks: Vec<CheckEntry>, findings: Vec<Finding>) -> Report {
        Report::from_run(RunContext::default(), checks, findings, None)
    }

    #[test]
    fn a_failed_check_with_no_findings_fails() {
        let r = verdict(vec![check("types", CheckStatus::Fail, 1)], Vec::new());
        assert_eq!(
            (r.status, r.blocking, r.exit_code()),
            (RunStatus::Fail, true, 1)
        );
        assert_eq!(r.next_action.as_deref(), Some("garita run types"));
        assert_eq!(r.next_action_reason.as_deref(), Some("1 check failed"));
    }

    #[test]
    fn a_warned_check_with_no_findings_warns() {
        let r = verdict(vec![check("a", CheckStatus::Warn, 1)], Vec::new());
        assert_eq!(
            (r.status, r.blocking, r.exit_code()),
            (RunStatus::Warn, false, 0)
        );
        assert_eq!((r.next_action, r.next_action_reason), (None, None));
    }

    #[test]
    fn a_failed_check_with_only_warnings_fails_and_reruns_it() {
        let checks = vec![
            check("a", CheckStatus::Pass, 1),
            check("b", CheckStatus::Fail, 1),
            check("c", CheckStatus::Fail, 1),
        ];
        let r = verdict(checks, vec![finding("b", Severity::Warning, Some("w.ts"))]);
        assert_eq!(r.status, RunStatus::Fail);
        assert_eq!(r.next_action.as_deref(), Some("garita run b c"));
        assert_eq!(r.next_action_reason.as_deref(), Some("2 checks failed"));
    }

    #[test]
    fn a_failed_check_without_files_drops_files_from_the_joint_rerun() {
        let checks = vec![
            check("a", CheckStatus::Fail, 1),
            check("b", CheckStatus::Fail, 1),
        ];
        let r = verdict(checks, vec![finding("b", Severity::Error, Some("x.ts"))]);
        assert_eq!(r.next_action.as_deref(), Some("garita run a b"));
        assert_eq!(r.next_action_reason.as_deref(), Some("1 error in 1 check"));
    }

    #[test]
    fn skipped_checks_only_pass() {
        let checks = vec![
            check("a", CheckStatus::Skipped, 0),
            check("b", CheckStatus::Skipped, 0),
        ];
        let r = verdict(checks, vec![finding("a", Severity::Info, None)]);
        assert_eq!((r.status, r.exit_code()), (RunStatus::Pass, 0));
        assert_eq!((r.next_action, r.next_action_reason), (None, None));
    }

    #[test]
    fn info_findings_only_pass() {
        let checks = vec![check("a", CheckStatus::Pass, 1)];
        let r = verdict(checks, vec![finding("a", Severity::Info, Some("x"))]);
        assert_eq!(
            (r.status, r.blocking, r.exit_code()),
            (RunStatus::Pass, false, 0)
        );
        assert_eq!((r.next_action, r.next_action_reason), (None, None));
    }

    #[test]
    fn finding_line_and_column_reject_zero() {
        let base = serde_json::json!({
            "check": "a", "severity": "error", "rule": "r", "file": null,
            "line": 1, "column": 1, "message": "m"
        });
        assert!(serde_json::from_value::<Finding>(base.clone()).is_ok());
        for key in ["line", "column"] {
            let mut bad = base.clone();
            bad[key] = serde_json::json!(0);
            assert!(serde_json::from_value::<Finding>(bad).is_err(), "{key}");
        }
    }

    #[test]
    fn not_run_report_always_carries_the_gate_keys() {
        let report = Report::not_run(
            RunContext::default(),
            RunError::config("no .garita/config.yaml"),
        );
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["status"], "not-run");
        assert_eq!(value["blocking"], false);
        assert_eq!(value["findings"], serde_json::json!([]));
        assert_eq!(value["examined"], serde_json::json!({}));
        assert_eq!(value["error"], "no .garita/config.yaml");
        assert_eq!(value["error_kinds"], serde_json::json!(["config"]));
        assert_eq!(value["next_action"], DOCTOR);
        assert_eq!(
            value["next_action_argv"],
            serde_json::json!(["garita", "doctor"])
        );
        assert_eq!(report.exit_code(), 2);
    }

    #[test]
    fn a_check_that_could_not_start_makes_a_not_run_report_with_an_error() {
        let checks = vec![
            check("a", CheckStatus::Pass, 4),
            check("b", CheckStatus::NotRun, 0),
        ];
        let report = Report::from_run(RunContext::default(), checks, Vec::new(), None);
        assert_eq!(report.status, RunStatus::NotRun);
        assert!(!report.blocking);
        assert_eq!(report.error.as_deref(), Some("could not run: b"));
        assert_eq!(report.error_kinds, Some(vec![ErrorKind::Environment]));
        assert_eq!(report.examined, Examined::from([("files".to_string(), 4)]));
    }

    #[test]
    fn an_error_forces_not_run_and_is_kept() {
        let report = Report::from_run(
            RunContext::default(),
            vec![check("a", CheckStatus::Fail, 1)],
            vec![finding("a", Severity::Error, None)],
            Some(RunError::environment("tool missing")),
        );
        assert_eq!(report.status, RunStatus::NotRun);
        assert!(!report.blocking);
        assert_eq!(report.error.as_deref(), Some("tool missing"));
    }

    #[test]
    fn a_report_that_ran_has_no_error_key() {
        let report = Report::from_run(RunContext::default(), Vec::new(), Vec::new(), None);
        let value = serde_json::to_value(&report).unwrap();
        assert!(value.get("error").is_none());
        assert!(value.get("error_kinds").is_none());
        assert_eq!(value["next_action_argv"], serde_json::Value::Null);
        assert_eq!(value["next_action"], serde_json::Value::Null);
        assert_eq!(value["next_action_reason"], serde_json::Value::Null);
        assert_eq!(value["commit"], serde_json::Value::Null);
    }

    #[test]
    fn enums_serialize_with_contract_spellings() {
        let j = |v: serde_json::Value| v.as_str().unwrap_or_default().to_string();
        assert_eq!(
            j(serde_json::to_value(RunStatus::NotRun).unwrap()),
            "not-run"
        );
        assert_eq!(
            j(serde_json::to_value(CheckStatus::Skipped).unwrap()),
            "skipped"
        );
        assert_eq!(
            j(serde_json::to_value(CheckStatus::NotRun).unwrap()),
            "not-run"
        );
        assert_eq!(
            j(serde_json::to_value(Severity::Warning).unwrap()),
            "warning"
        );
        assert_eq!(j(serde_json::to_value(Trigger::Ci).unwrap()), "ci");
        for kind in CheckKind::ALL {
            assert_eq!(j(serde_json::to_value(kind).unwrap()), kind.as_str());
        }
        for kind in ScopeKind::ALL {
            assert_eq!(j(serde_json::to_value(kind).unwrap()), kind.as_str());
        }
    }

    #[test]
    fn finding_omits_absent_fix_and_fingerprint() {
        let value = serde_json::to_value(finding("a", Severity::Info, None)).unwrap();
        assert!(value.get("fix").is_none());
        assert!(value.get("fingerprint").is_none());
        assert_eq!(value["file"], serde_json::Value::Null);
    }

    #[test]
    fn rerun_command_quotes_what_needs_it() {
        assert_eq!(rerun_command(None, "lint", &[]), "garita run lint");
        assert_eq!(
            rerun_command(None, "lint", &["a.ts", "it's.ts"]),
            r"garita run lint --files a.ts 'it'\''s.ts'"
        );
    }

    #[test]
    fn rerun_command_names_every_trigger_but_commit() {
        assert_eq!(
            rerun_command(Some(Trigger::Commit), "lint", &[]),
            "garita run lint"
        );
        assert_eq!(
            rerun_command(Some(Trigger::Push), "lint", &["a.ts"]),
            "garita run --trigger push lint --files a.ts"
        );
        assert_eq!(
            rerun_command(Some(Trigger::Merge), "x", &[]),
            "garita run --trigger merge x"
        );
    }

    #[test]
    fn the_next_action_string_is_its_argv_quoted() {
        let checks = [check("lint", CheckStatus::Fail, 1)];
        let f = [finding("lint", Severity::Error, Some("src/[id]/a b.ts"))];
        let next = next_action(Some(Trigger::Push), RunStatus::Fail, &checks, &f);
        let argv = next.argv.unwrap();
        assert_eq!(
            argv,
            [
                "garita",
                "run",
                "--trigger",
                "push",
                "lint",
                "--files",
                "src/[id]/a b.ts"
            ]
        );
        assert_eq!(
            next.action.as_deref(),
            Some("garita run --trigger push lint --files 'src/[id]/a b.ts'")
        );
        assert_eq!(
            crate::argv::split(next.action.as_deref().unwrap()).unwrap(),
            argv
        );
    }

    #[test]
    fn a_usage_error_points_at_describe_and_others_at_doctor() {
        let usage = Report::not_run(RunContext::default(), RunError::usage("bad"));
        assert_eq!(usage.next_action.as_deref(), Some("garita describe"));
        assert_eq!(usage.next_action_argv, Some(describe_argv()));
        for error in [
            RunError::config("c"),
            RunError::environment("e"),
            RunError::internal("i"),
        ] {
            let r = Report::not_run(RunContext::default(), error);
            assert_eq!(r.next_action_argv, Some(doctor_argv()));
        }
    }

    #[test]
    fn an_error_is_one_line() {
        let e = RunError::config("a is not valid:\n  x: bad\n\n  y: worse\n");
        assert_eq!(e.message, "a is not valid:; x: bad; y: worse");
    }

    #[test]
    fn rerun_command_ends_options_before_a_file_that_looks_like_one() {
        assert_eq!(
            rerun_command(None, "lint", &["a.ts", "-b.ts"]),
            "garita run lint --files -- a.ts -b.ts"
        );
    }
}
