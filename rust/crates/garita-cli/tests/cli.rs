use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use garita_core::argv::split;
use garita_core::report::{ErrorKind, Report, RequiredCheck, RiskSource, RunStatus};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("garita-cli-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("repo")).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.repo().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn tool(&self, name: &str, body: &str) -> String {
        let path = self.0.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn garita(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_garita"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

/// Whether garita wrote a finding's message itself, rather than passing on a tool's diagnostic.
fn garitas_own(rule: &str) -> bool {
    !rule.contains('/')
        || rule.starts_with("doctor/")
        || ["/unrecognized-output", "/failed", "/task-failed"]
            .iter()
            .any(|s| rule.ends_with(s))
}

/// Whether `text` names a path inside a test sandbox's `repo` directory.
fn names_sandbox_repo(text: &str) -> bool {
    text.match_indices("garita-cli-").any(|(at, _)| {
        let rest = text.get(at..).unwrap_or_default();
        rest.split('/')
            .nth(1)
            .and_then(|segment| segment.strip_prefix("repo"))
            .is_some_and(|after| !after.starts_with(|c: char| c.is_alphanumeric() || c == '-'))
    })
}

fn assert_no_sandbox_path(report: &Report) {
    let error = report.error.as_deref().unwrap_or_default();
    assert!(!names_sandbox_repo(error), "{error}");
    for f in report.findings.iter().filter(|f| garitas_own(&f.rule)) {
        assert!(!names_sandbox_repo(&f.message), "{}", f.message);
    }
}

/// stdout is exactly one JSON object and a newline; it is a report that keeps the contract.
fn report_of(out: &Output) -> Report {
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(stdout.ends_with('\n'), "{stdout:?}");
    assert_eq!(stdout.matches('\n').count(), 1, "{stdout:?}");
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(value.is_object());
    let report: Report = serde_json::from_value(value).unwrap();
    assert_eq!(i32::from(report.exit_code()), out.status.code().unwrap());
    assert_eq!(report.blocking, report.exit_code() == 1);
    assert_eq!(
        report.next_action.is_none(),
        report.next_action_reason.is_none()
    );
    if report.status == RunStatus::NotRun {
        assert!(report.error.is_some());
        assert!(report.error_kinds.is_some());
    }
    match (&report.next_action, &report.next_action_argv) {
        (None, None) => {}
        (Some(action), Some(argv)) => assert_eq!(&split(action).unwrap(), argv),
        other => panic!("next_action and next_action_argv disagree: {other:?}"),
    }
    assert!(
        !report.error.as_deref().unwrap_or_default().contains('\n'),
        "{:?}",
        report.error
    );
    for f in &report.findings {
        assert!(
            !garitas_own(&f.rule) || !f.message.contains('\n'),
            "{}",
            f.message
        );
    }
    assert_no_sandbox_path(&report);
    assert_eq!(stderr(out), "", "stderr carries nothing but a diagnostic");
    for f in &report.findings {
        if let Some(check) = &f.check {
            assert!(report.checks.iter().any(|c| &c.name == check), "{check}");
        }
    }
    report
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A repository with a lint check whose fake tool reports an error in every file it is given.
fn lint_repo(t: &Temp) {
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    let tool = t.tool(
        "lint",
        r#"for f in "$@"; do case "$f" in -*) ;; *) printf '%s:1:1 bad\n' "$f";; esac; done; exit 1"#,
    );
    t.write(".gitignore", ".garita/receipts/\n");
    t.write(
        ".garita/config.yaml",
        &format!("schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n"),
    );
}

#[test]
fn a_failing_run_prints_one_json_report_and_exits_1() {
    let t = Temp::new();
    lint_repo(&t);
    t.write("a.ts", "x\n");
    git(&t.repo(), &["add", "a.ts"]);
    let out = garita(&t.repo(), &["run"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::Fail);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(report.next_action.as_deref(), Some("garita run lint"));
    assert!(report.receipt.is_some());
}

#[test]
fn outside_a_repository_is_not_run_and_exits_2() {
    let t = Temp::new();
    let out = garita(&t.repo(), &["run", "--trigger", "push"]);
    let report = report_of(&out);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        report.error.as_deref(),
        Some("the current directory is not inside a git repository")
    );
}

#[test]
fn a_bad_config_is_named_relative_to_the_repository() {
    let t = Temp::new();
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    t.write(".garita/config.yaml", "schema_version: 1\nchecks: 3\n");
    let out = garita(&t.repo(), &["run"]);
    let report = report_of(&out);
    assert_eq!(report.error_kinds, Some(vec![ErrorKind::Config]));
    let error = report.error.unwrap();
    assert!(
        error.starts_with(".garita/config.yaml is not a valid config: "),
        "{error}"
    );
}

#[test]
fn a_missing_config_is_named_relative_to_the_repository() {
    let t = Temp::new();
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    let out = garita(&t.repo(), &["run"]);
    assert_eq!(
        report_of(&out).error.as_deref(),
        Some(".garita/config.yaml does not exist")
    );
}

#[test]
fn a_name_that_is_not_a_check_has_no_entry_and_no_output_file() {
    let t = Temp::new();
    lint_repo(&t);
    t.write("a.ts", "x\n");
    git(&t.repo(), &["add", "a.ts"]);
    let out = garita(&t.repo(), &["run", "nosuch", "alsonot"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::NotRun);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(report.error_kinds, Some(vec![ErrorKind::Usage]));
    assert_eq!(
        report.error.as_deref(),
        Some("`nosuch`, `alsonot` are not checks in the config")
    );
    assert_eq!(report.next_action.as_deref(), Some("garita describe"));
    assert!(report.checks.is_empty(), "{:?}", report.checks);
    let runs: Vec<_> = fs::read_dir(t.repo().join(".garita/receipts"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    for run in runs {
        assert_eq!(fs::read_dir(run.path()).unwrap().count(), 0);
    }
}

#[test]
fn files_after_a_double_dash_may_begin_with_a_dash() {
    let t = Temp::new();
    lint_repo(&t);
    t.write("-a.ts", "x\n");
    t.write("b.ts", "x\n");
    let out = garita(
        &t.repo(),
        &["run", "lint", "--files", "b.ts", "--", "-a.ts"],
    );
    let report = report_of(&out);
    let scope = report.scope.unwrap().files.unwrap();
    assert_eq!(scope, ["b.ts", "-a.ts"]);
    assert_eq!(report.status, RunStatus::Fail);
    let out = garita(&t.repo(), &["run", "lint", "--files", "--", "-a.ts"]);
    let report = report_of(&out);
    assert!(report.scope.is_some(), "{report:?}");
    assert_eq!(report.scope.unwrap().files.unwrap(), ["-a.ts"]);
}

#[test]
fn next_action_puts_a_dash_file_after_a_double_dash() {
    let t = Temp::new();
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    let tool = t.tool(
        "oxlint",
        r#"printf '{"diagnostics":[{"message":"m","code":"eslint(x)","severity":"error","filename":"-a.ts","labels":[{"span":{"offset":0,"length":1,"line":1,"column":1}}]}],"number_of_files":1,"number_of_rules":1}\n'; exit 1"#,
    );
    t.write(".gitignore", ".garita/receipts/\n");
    t.write(
        ".garita/config.yaml",
        &format!(
            "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n    parser: oxlint\n"
        ),
    );
    t.write("-a.ts", "x\n");
    let out = garita(&t.repo(), &["run", "--files", "--", "-a.ts"]);
    let report = report_of(&out);
    assert_eq!(
        report.next_action.as_deref(),
        Some("garita run lint --files -- -a.ts")
    );
    let again = garita(&t.repo(), &["run", "lint", "--files", "--", "-a.ts"]);
    assert_eq!(report_of(&again).findings, report.findings);
}

#[test]
fn a_usage_error_under_run_is_still_one_not_run_report() {
    let t = Temp::new();
    let out = garita(&t.repo(), &["run", "--trigger", "lunch"]);
    let report = report_of(&out);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(report.error_kinds, Some(vec![ErrorKind::Usage]));
    let error = report.error.unwrap();
    assert!(error.contains("lunch"), "{error}");
    assert!(!error.starts_with("error:"), "{error}");
    assert!(!error.contains("Usage") && !error.contains('\n'), "{error}");
}

#[test]
fn a_top_level_usage_error_is_one_not_run_report_naming_the_argument() {
    let t = Temp::new();
    for (args, named) in [
        (&[][..], "no command was given"),
        (&["lunch"], "lunch"),
        (&["--bogus"], "--bogus"),
        (&["hooks"], "no command was given"),
    ] {
        let out = garita(&t.repo(), args);
        let report = report_of(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(report.error_kinds, Some(vec![ErrorKind::Usage]), "{args:?}");
        let error = report.error.unwrap();
        assert!(error.contains(named), "{args:?}: {error}");
        assert!(!error.contains("Usage") && !error.contains('\n'), "{error}");
    }
}

#[test]
fn a_usage_error_report_matches_its_golden() {
    let t = Temp::new();
    let out = garita(&t.repo(), &["--bogus"]);
    report_of(&out);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../garita-core/tests/contract/examples/run-report.usage-error.json");
    if std::env::var("GARITA_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        fs::write(&path, &text).unwrap();
    } else {
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn a_failing_check_carries_its_output_on_the_check_and_one_sentence_in_the_finding() {
    let t = Temp::new();
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    let tool = t.tool(
        "records",
        "echo 'checked 3 records'; echo 'error: 0004-x.md has no status' >&2; exit 1",
    );
    t.write(".gitignore", ".garita/receipts/\n");
    t.write(
        ".garita/config.yaml",
        &format!(
            "schema_version: 1\nchecks:\n  records:\n    run: {tool}\n    parser: exit-code\n"
        ),
    );
    let out = garita(&t.repo(), &["run"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::Fail);
    let check = &report.checks[0];
    let tail = check.output_tail.as_deref().unwrap();
    assert!(
        tail.contains("checked 3 records") && tail.contains("0004-x.md"),
        "{tail}"
    );
    assert_eq!(check.output_notable, ["error: 0004-x.md has no status"]);
    let output = check.output.as_deref().unwrap();
    assert!(t.repo().join(output).is_file(), "{output}");
    assert_eq!(report.findings[0].message, "exited with code 1");
}

#[test]
fn version_exits_zero() {
    let t = Temp::new();
    assert_eq!(garita(&t.repo(), &["--version"]).status.code(), Some(0));
}

#[test]
fn a_usage_error_under_any_subcommand_is_one_report_with_the_error_said_once() {
    let t = Temp::new();
    for args in [&["doctor", "--bogus"][..], &["hooks", "install", "--bogus"]] {
        let out = garita(&t.repo(), args);
        let report = report_of(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(report.error.unwrap().contains("--bogus"), "{args:?}");
    }
}

#[test]
fn a_passing_run_prints_one_json_report_and_nothing_else() {
    let t = Temp::new();
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    let tool = t.tool("lint", "exit 0");
    t.write(".gitignore", ".garita/receipts/\n");
    t.write(
        ".garita/config.yaml",
        &format!("schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n"),
    );
    t.write("a.ts", "x\n");
    git(&t.repo(), &["add", "a.ts"]);
    let out = garita(&t.repo(), &["run"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::Pass);
    assert_eq!(out.status.code(), Some(0));
}

/// The version is the crate's, which a release changes; the golden holds a fixed one.
fn without_version(mut value: serde_json::Value) -> serde_json::Value {
    value["version"] = "0.0.0".into();
    value
}

#[test]
fn describe_prints_the_golden_and_nothing_else() {
    let t = Temp::new();
    let out = garita(&t.repo(), &["describe"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stderr(&out), "");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.matches('\n').count(), 1, "{stdout:?}");
    let printed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(printed["version"], env!("CARGO_PKG_VERSION"));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../garita-core/tests/contract/examples/describe.garita.json");
    let golden: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(without_version(printed), golden);
}

fn risk_repo(t: &Temp, policy: &str) {
    let repo = t.repo();
    git(&repo, &["init", "-q", "-b", "main"]);
    let tool = t.tool("lint", "exit 1");
    t.write(
        ".garita/config.yaml",
        &format!(
            "schema_version: 1\n{policy}checks:\n  lint:\n    run: {tool}\n    on: [push, ci]\n  local:\n    run: {tool}\n    on: [commit]\n"
        ),
    );
    t.write("a.txt", "a\n");
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    git(&repo, &["switch", "-q", "-c", "feature"]);
    t.write("crates/core/a.rs", "x\n");
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "change",
        ],
    );
}

const RISK_POLICY: &str = "risk:\n  classes: [{ name: routine, approvals: 0 }, { name: core }]\n  default: routine\n  match: [{ paths: [\"crates/core/**\"], class: core }]\n";

#[test]
fn risk_prints_the_class_and_runs_no_check() {
    let t = Temp::new();
    risk_repo(&t, RISK_POLICY);
    let out = garita(&t.repo(), &["risk"]);
    let report = report_of(&out);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(report.status, RunStatus::Pass);
    assert!(report.checks.is_empty());
    assert_eq!((report.trigger, report.receipt.as_deref()), (None, None));
    assert!(report.commit.is_some());
    let risk = report.risk.unwrap().unwrap();
    assert_eq!(risk.class.as_deref(), Some("core"));
    assert_eq!(risk.source, RiskSource::Matched);
    assert!(risk.needs_person);
    assert_eq!((risk.reached, risk.met), (None, None));
    assert_eq!(
        risk.required,
        [RequiredCheck {
            check: "lint".into(),
            system: 0,
            status: None,
        }]
    );
    assert!(!t.repo().join(".garita/receipts").exists());
}

#[test]
fn risk_takes_a_base() {
    let t = Temp::new();
    risk_repo(&t, RISK_POLICY);
    let out = garita(&t.repo(), &["risk", "--base", "feature"]);
    let risk = report_of(&out).risk.unwrap().unwrap();
    assert_eq!(risk.class.as_deref(), Some("routine"));
    assert_eq!(risk.source, RiskSource::Default);
    let out = garita(&t.repo(), &["risk", "--base", "nope"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::Pass);
    let risk = report.risk.unwrap().unwrap();
    assert_eq!(risk.source, RiskSource::BaseUnreadable);
    assert!(
        report.findings[0].message.contains("`nope`"),
        "{:?}",
        report.findings
    );
}

#[test]
fn risk_is_null_and_passes_without_a_policy() {
    let t = Temp::new();
    risk_repo(&t, "");
    let out = garita(&t.repo(), &["risk"]);
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(stdout.contains("\"risk\":null"), "{stdout}");
    assert_eq!(report_of(&out).status, RunStatus::Pass);
}

#[test]
fn risk_is_not_run_outside_a_repository_or_without_a_config() {
    let t = Temp::new();
    let report = report_of(&garita(&t.repo(), &["risk"]));
    assert_eq!(report.error_kinds, Some(vec![ErrorKind::Environment]));
    assert_eq!(report.risk, None);
    git(&t.repo(), &["init", "-q", "-b", "main"]);
    let report = report_of(&garita(&t.repo(), &["risk"]));
    assert_eq!(report.error_kinds, Some(vec![ErrorKind::Config]));
    assert_eq!(
        report.error.as_deref(),
        Some(".garita/config.yaml does not exist")
    );
    let report = report_of(&garita(&t.repo(), &["risk", "--bogus"]));
    assert_eq!(report.error_kinds, Some(vec![ErrorKind::Usage]));
}

#[test]
fn a_git_failure_does_not_name_the_repository_path() {
    let t = Temp::new();
    lint_repo(&t);
    fs::write(t.repo().join(".git/index"), "junk\n").unwrap();
    let out = garita(&t.repo(), &["run"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::NotRun);
    assert!(report.error.unwrap().contains(" .git/garita-index-"));
}
