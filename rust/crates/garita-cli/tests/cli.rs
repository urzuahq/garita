use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use garita_core::report::{Report, RunStatus};

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
fn a_failing_run_prints_one_json_report_and_a_summary_and_exits_1() {
    let t = Temp::new();
    lint_repo(&t);
    t.write("a.ts", "x\n");
    git(&t.repo(), &["add", "a.ts"]);
    let out = garita(&t.repo(), &["run"]);
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::Fail);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("garita: fail at commit"), "{err}");
    assert!(err.contains("lint: fail"), "{err}");
    assert!(err.contains("next: garita run lint"), "{err}");
    assert!(report.receipt.is_some());
}

#[test]
fn outside_a_repository_is_not_run_and_exits_2() {
    let t = Temp::new();
    let out = garita(&t.repo(), &["run", "--trigger", "push"]);
    let report = report_of(&out);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        report
            .error
            .unwrap()
            .contains("not inside a git repository")
    );
    assert!(stderr(&out).contains("garita: not-run"));
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
    assert!(report.error.unwrap().contains("lunch"));
}

#[test]
fn version_exits_zero() {
    let t = Temp::new();
    assert_eq!(garita(&t.repo(), &["--version"]).status.code(), Some(0));
}

#[test]
fn unimplemented_commands_exit_one_with_empty_stdout() {
    let t = Temp::new();
    for args in [&["doctor"][..], &["hooks", "install"]] {
        let out = garita(&t.repo(), args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(stderr(&out).contains("is not implemented"), "{args:?}");
    }
}
