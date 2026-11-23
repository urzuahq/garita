use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use garita_core::adapter::turbo::{self, CacheStatus};
use garita_core::adapter::{
    Adapter, FILES, Outcome, ParseContext, Parsed, ToolOutput, count_project_files,
};

const OXLINT: &str = "oxlint@1.94.0";
const OXFMT: &str = "oxfmt@0.79.0";
const TSC: &str = "tsc@5.8.3";
const TSGO: &str = "tsgo@7.0.0-dev.20260707.2";
const TURBO: &str = "turbo@2.5.6";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/recordings")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

struct Recording {
    cwd: String,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

fn recording(tool: &str, case: &str) -> Recording {
    let dir = root().join(tool).join(case);
    let cmd = read(&dir.join("cmd"));
    let cwd = cmd.lines().next().unwrap_or(".").trim();
    Recording {
        cwd: if cwd == "." {
            String::new()
        } else {
            cwd.to_string()
        },
        stdout: read(&dir.join("stdout")),
        stderr: read(&dir.join("stderr")),
        exit_code: read(&dir.join("exit_code")).trim().parse().ok(),
    }
}

fn cases(tool: &str) -> BTreeSet<String> {
    let entries = fs::read_dir(root().join(tool)).unwrap();
    entries
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().unwrap().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn tools() -> BTreeSet<String> {
    fs::read_dir(root())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn every_recorded_tool_version_is_tested() {
    let known: BTreeSet<String> = [OXLINT, OXFMT, TSC, TSGO, TURBO].map(String::from).into();
    assert_eq!(tools(), known, "a recorded tool version has no test table");
}

/// One recording's expected parse: outcome, finding count, `examined.files`, and the first
/// finding's file, line, rule and a piece of its message.
struct Expect {
    case: &'static str,
    outcome: &'static str,
    findings: usize,
    files: Option<u64>,
    first: Option<(
        Option<&'static str>,
        Option<u64>,
        &'static str,
        &'static str,
    )>,
}

const fn e(
    case: &'static str,
    outcome: &'static str,
    findings: usize,
    files: Option<u64>,
) -> Expect {
    Expect {
        case,
        outcome,
        findings,
        files,
        first: None,
    }
}

const UNFORMATTED: &str = "File is not formatted";

/// A non-zero exit whose output the parser does not recognise: one blocking finding.
fn unrecognized(case: &'static str, tool: &'static str, exit: i32) -> Expect {
    let rule = match tool {
        "oxlint" => "oxlint/unrecognized-output",
        _ => "oxfmt/unrecognized-output",
    };
    let said = match exit {
        1 => "exited with code 1",
        _ => "exited with code 2",
    };
    e(case, "findings", 1, None).first(None, None, rule, said)
}

impl Expect {
    const fn first(
        mut self,
        file: Option<&'static str>,
        line: Option<u64>,
        rule: &'static str,
        message: &'static str,
    ) -> Self {
        self.first = Some((file, line, rule, message));
        self
    }
}

fn check_table(tool: &str, adapter: Adapter, table: &[Expect]) {
    let expected: BTreeSet<String> = table.iter().map(|x| x.case.to_string()).collect();
    assert_eq!(
        expected.len(),
        table.len(),
        "{tool}: a case is listed twice"
    );
    let recorded = cases(tool);
    let untested: Vec<_> = recorded.difference(&expected).collect();
    let stale: Vec<_> = expected.difference(&recorded).collect();
    assert!(
        untested.is_empty(),
        "{tool}: recordings without expectations: {untested:?}"
    );
    assert!(
        stale.is_empty(),
        "{tool}: expectations without recordings: {stale:?}"
    );
    for x in table {
        let r = recording(tool, x.case);
        let output = ToolOutput {
            stdout: &r.stdout,
            stderr: &r.stderr,
            exit_code: r.exit_code,
        };
        let context = ParseContext {
            cwd: &r.cwd,
            files: None,
        };
        let parsed = adapter.parse(&output, &context);
        assert_case(tool, x, &parsed);
    }
}

fn assert_case(tool: &str, x: &Expect, parsed: &Parsed) {
    let at = format!("{tool}/{}", x.case);
    assert_eq!(
        parsed.outcome.name(),
        x.outcome,
        "{at}: {:?}",
        parsed.outcome
    );
    assert_eq!(parsed.findings.len(), x.findings, "{at}");
    assert_eq!(parsed.examined.get(FILES).copied(), x.files, "{at}");
    if let Some((file, line, rule, message)) = x.first {
        let first = &parsed.findings[0];
        assert_eq!(first.file.as_deref(), file, "{at}");
        assert_eq!(first.line.map(|l| l.get()), line, "{at}");
        assert_eq!(first.rule, rule, "{at}");
        assert!(first.message.contains(message), "{at}: {:?}", first.message);
    }
    for finding in &parsed.findings {
        let fingerprint = finding.fingerprint();
        assert_eq!(fingerprint.len(), 16, "{at}");
        if let Some(file) = &finding.file {
            assert!(
                !file.starts_with("./") && !file.contains('\\'),
                "{at}: {file}"
            );
        }
    }
}

const DEBUGGER: &str = "`debugger` statement is not allowed (help: Remove the debugger statement)";
const LINT_ERROR: Option<&str> = Some("packages/app/src/lint-error.ts");

#[test]
fn oxlint_recordings() {
    let rule = "oxlint/eslint/no-debugger";
    let table = [
        e("clean-default", "crashed", 0, None),
        e("clean-json", "clean", 0, Some(3)),
        unrecognized("errors-two-files-default", "oxlint", 1),
        e("errors-two-files-json", "findings", 2, Some(5)).first(
            Some("packages/app/src/lint-error-2.ts"),
            Some(2),
            rule,
            DEBUGGER,
        ),
        unrecognized("invalid-config-default", "oxlint", 1),
        unrecognized("invalid-config-json", "oxlint", 1),
        e("no-files-matched-allowed-default", "crashed", 0, None),
        e("no-files-matched-allowed-json", "nothing", 0, Some(0)),
        unrecognized("no-files-matched-default", "oxlint", 1),
        e("no-files-matched-json", "nothing", 0, Some(0)),
        unrecognized("nonexistent-path-default", "oxlint", 1),
        e("nonexistent-path-json", "nothing", 0, Some(0)),
        unrecognized("one-error-default", "oxlint", 1),
        unrecognized("one-error-default-agent-env", "oxlint", 1),
        unrecognized("one-error-in-package-default", "oxlint", 1),
        e("one-error-in-package-json", "findings", 1, Some(4)).first(
            LINT_ERROR,
            Some(2),
            rule,
            DEBUGGER,
        ),
        e("one-error-json", "findings", 1, Some(4)).first(LINT_ERROR, Some(2), rule, DEBUGGER),
        e("one-error-json-agent-env", "findings", 1, Some(4)).first(
            LINT_ERROR,
            Some(2),
            rule,
            DEBUGGER,
        ),
        e("version", "crashed", 0, None),
        e("warning-only-default", "crashed", 0, None),
        e("warning-only-json", "findings", 1, Some(4)).first(
            Some("packages/app/src/lint-warn.ts"),
            Some(2),
            "oxlint/eslint/eqeqeq",
            "Expected === and instead saw == (help: Prefer === operator)",
        ),
    ];
    check_table(OXLINT, Adapter::Oxlint, &table);
}

#[test]
fn oxlint_recorded_finding_in_full() {
    let r = recording(OXLINT, "one-error-in-package-json");
    let output = ToolOutput {
        stdout: &r.stdout,
        stderr: &r.stderr,
        exit_code: r.exit_code,
    };
    let context = ParseContext {
        cwd: &r.cwd,
        files: None,
    };
    let finding = &Adapter::Oxlint.parse(&output, &context).findings[0];
    assert_eq!(finding.column.map(|c| c.get()), Some(3));
    assert_eq!(finding.severity, garita_core::report::Severity::Error);
    assert_eq!(finding.message, DEBUGGER);
    assert_eq!(
        finding.fix.as_deref(),
        Some("oxlint --fix packages/app/src/lint-error.ts")
    );
    let invalid = recording(OXLINT, "invalid-config-json");
    let output = ToolOutput {
        stdout: &invalid.stdout,
        stderr: &invalid.stderr,
        exit_code: invalid.exit_code,
    };
    let parsed = Adapter::Oxlint.parse(&output, &context);
    assert_eq!(parsed.outcome, Outcome::Findings, "a bad config must block");
    let message = &parsed.findings[0].message;
    assert!(
        message.starts_with("oxlint exited with code 1")
            && message.contains("Failed to parse oxlint configuration file.")
            && message.contains("EOF while parsing a value"),
        "{message}"
    );
}

#[test]
fn oxfmt_recordings() {
    let rule = "oxfmt/unformatted";
    let bad = Some("packages/app/src/format-bad.ts");
    let bad2 = Some("packages/app/src/format-bad-2.ts");
    let table = [
        e("check-formatted", "clean", 0, Some(3)),
        e("check-formatted-in-package", "clean", 0, Some(3)),
        e("check-no-files-matched", "nothing", 0, Some(0)),
        e("check-no-files-matched-allowed", "nothing", 0, Some(0)),
        e("check-nonexistent-path", "nothing", 0, Some(0)),
        e("check-one-unformatted", "findings", 1, Some(4)).first(bad, None, rule, UNFORMATTED),
        e("check-one-unformatted-in-package", "findings", 1, Some(4)).first(
            bad,
            None,
            rule,
            UNFORMATTED,
        ),
        unrecognized("check-syntax-error", "oxfmt", 2),
        e("check-two-unformatted", "findings", 2, Some(5)).first(bad2, None, rule, UNFORMATTED),
        e("check-unknown-file-type-explicit", "nothing", 0, Some(0)),
        e("check-unknown-file-type-in-dir", "clean", 0, Some(3)),
        e("list-different-formatted", "clean", 0, None),
        e("list-different-formatted-in-package", "clean", 0, None),
        e("list-different-no-files-matched", "nothing", 0, Some(0)),
        e(
            "list-different-no-files-matched-allowed",
            "nothing",
            0,
            Some(0),
        ),
        e("list-different-nonexistent-path", "nothing", 0, Some(0)),
        e("list-different-one-unformatted", "findings", 1, None).first(
            bad,
            None,
            rule,
            UNFORMATTED,
        ),
        e(
            "list-different-one-unformatted-in-package",
            "findings",
            1,
            None,
        )
        .first(bad, None, rule, UNFORMATTED),
        unrecognized("list-different-syntax-error", "oxfmt", 2),
        e("list-different-two-unformatted", "findings", 2, None).first(
            bad2,
            None,
            rule,
            UNFORMATTED,
        ),
        e(
            "list-different-unknown-file-type-explicit",
            "nothing",
            0,
            Some(0),
        ),
        e("list-different-unknown-file-type-in-dir", "clean", 0, None),
        e("version", "crashed", 0, None),
    ];
    check_table(OXFMT, Adapter::Oxfmt, &table);
}

#[test]
fn oxfmt_list_mode_takes_the_passed_count() {
    let r = recording(OXFMT, "list-different-one-unformatted");
    let output = ToolOutput {
        stdout: &r.stdout,
        stderr: &r.stderr,
        exit_code: r.exit_code,
    };
    let context = ParseContext {
        cwd: &r.cwd,
        files: Some(7),
    };
    let parsed = Adapter::Oxfmt.parse(&output, &context);
    assert_eq!(parsed.examined.get(FILES), Some(&7));
    assert_eq!(
        parsed.findings[0].fix.as_deref(),
        Some("oxfmt packages/app/src/format-bad.ts")
    );
}

fn tsc_table(nonexistent: &'static str) -> Vec<Expect> {
    let ts2322 = "tsc/TS2322";
    let assign = "Type 'string' is not assignable to type 'number'.";
    let one = Some("packages/app/src/type-error.ts");
    let two = Some("packages/app/src/type-error-2.ts");
    let boolean = "Type 'number' is not assignable to type 'boolean'.";
    vec![
        e("clean", "clean", 0, None),
        e("clean-in-package", "clean", 0, None),
        e("errors-two-files", "findings", 2, None).first(two, Some(1), ts2322, boolean),
        e("extended-diagnostics", "clean", 0, None),
        e("list-files", "clean", 0, None),
        e("list-files-only", "clean", 0, None),
        e("nonexistent-project", "findings", 1, None).first(None, None, "tsc/TS5058", nonexistent),
        e("solution-build", "clean", 0, None),
        e("solution-build-with-errors", "findings", 2, None).first(two, Some(1), ts2322, boolean),
        e("solution-plain", "clean", 0, None),
        e("syntax-error", "findings", 1, None).first(
            Some("packages/app/src/syntax-error.ts"),
            Some(1),
            "tsc/TS1134",
            "Variable declaration expected.",
        ),
        e("type-error", "findings", 1, None).first(one, Some(1), ts2322, assign),
        e("type-error-in-package", "findings", 1, None).first(one, Some(1), ts2322, assign),
        e("type-error-multiline", "findings", 1, None).first(
            Some("packages/app/src/type-error-multiline.ts"),
            Some(5),
            ts2322,
            "to type 'Point'.\n  The types of 'label.text' are incompatible between these types.\n    Type 'number' is not assignable to type 'string'.",
        ),
        e("type-error-second-run", "findings", 1, None).first(one, Some(1), ts2322, assign),
        e("version", "clean", 0, None),
    ]
}

#[test]
fn tsc_recordings() {
    let table = tsc_table("The specified path does not exist: 'packages/missing'.");
    check_table(TSC, Adapter::Tsc, &table);
}

#[test]
fn tsgo_recordings() {
    let table = tsc_table("The specified path does not exist: '<WORKSPACE>/packages/missing'.");
    check_table(TSGO, Adapter::Tsc, &table);
}

#[test]
fn both_compilers_list_the_same_project_files() {
    for tool in [TSC, TSGO] {
        for case in ["list-files-only", "list-files"] {
            let r = recording(tool, case);
            assert_eq!(count_project_files(&r.stdout), 6, "{tool}/{case}");
        }
    }
}

#[test]
fn tsc_with_a_zero_count_ran_on_nothing() {
    let r = recording(TSC, "clean");
    let output = ToolOutput {
        stdout: &r.stdout,
        stderr: &r.stderr,
        exit_code: r.exit_code,
    };
    let context = ParseContext {
        cwd: &r.cwd,
        files: Some(0),
    };
    assert_eq!(
        Adapter::Tsc.parse(&output, &context).outcome,
        Outcome::Nothing
    );
}

/// What each turbo recording holds besides terminal output.
enum TurboExpect {
    /// `summary.json`: task ids with their exit codes, in the summary's order.
    Summary(&'static [(&'static str, i32)]),
    /// A `--dry=json` plan on stdout: task ids, no exit codes.
    Dry(&'static [&'static str]),
    /// Terminal output and logs only.
    NoSummary,
}

fn turbo_table() -> Vec<(&'static str, TurboExpect)> {
    use TurboExpect::{Dry, NoSummary, Summary};
    vec![
        ("affected-git-unchanged", NoSummary),
        ("affected-no-git", NoSummary),
        ("lint-check-all-cached", NoSummary),
        ("lint-check-all-passing", NoSummary),
        ("lint-check-continue", NoSummary),
        ("lint-check-default", NoSummary),
        (
            "lint-check-dry-json",
            Dry(&[
                "@fixture/app#lint:check",
                "@fixture/docs#lint:check",
                "@fixture/lib#lint:check",
            ]),
        ),
        ("lint-check-errors-only", NoSummary),
        ("lint-check-log-order-grouped", NoSummary),
        ("lint-check-log-order-stream", NoSummary),
        ("lint-check-log-prefix-none", NoSummary),
        (
            "lint-check-summarize",
            Summary(&[
                ("@fixture/app#lint:check", 0),
                ("@fixture/docs#lint:check", 0),
                ("@fixture/lib#lint:check", 1),
            ]),
        ),
        ("type-check-all-cached", NoSummary),
        ("type-check-all-passing", NoSummary),
        ("type-check-and-lint-check-all-passing", NoSummary),
        ("type-check-and-lint-check-two-failing", NoSummary),
        ("type-check-and-lint-check-two-failing-continue", NoSummary),
        ("type-check-continue", NoSummary),
        ("type-check-default", NoSummary),
        (
            "type-check-dry-json",
            Dry(&[
                "@fixture/app#type-check",
                "@fixture/docs#type-check",
                "@fixture/lib#type-check",
            ]),
        ),
        ("type-check-errors-only", NoSummary),
        ("type-check-log-order-grouped", NoSummary),
        ("type-check-log-order-stream", NoSummary),
        ("type-check-log-prefix-none", NoSummary),
        (
            "type-check-summarize",
            Summary(&[
                ("@fixture/app#type-check", 2),
                ("@fixture/lib#type-check", 0),
            ]),
        ),
        ("version", NoSummary),
    ]
}

#[test]
fn turbo_recordings() {
    let table = turbo_table();
    let expected: BTreeSet<String> = table.iter().map(|(c, _)| c.to_string()).collect();
    assert_eq!(expected.len(), table.len());
    assert_eq!(
        cases(TURBO),
        expected,
        "turbo cases and expectations differ"
    );
    for (case, expect) in &table {
        let dir = root().join(TURBO).join(case);
        let has_summary = dir.join("summary.json").exists();
        match expect {
            TurboExpect::Summary(tasks) => {
                let summary = turbo::parse_summary(&read(&dir.join("summary.json"))).unwrap();
                let got: Vec<_> = summary
                    .tasks
                    .iter()
                    .map(|t| (t.task_id.as_str(), t.exit_code.unwrap()))
                    .collect();
                assert_eq!(&got, tasks, "{case}");
                assert!(
                    summary
                        .tasks
                        .iter()
                        .all(|t| t.ran_fresh() && t.cache == CacheStatus::Miss)
                );
                assert!(
                    summary
                        .tasks
                        .iter()
                        .all(|t| t.start_time <= t.end_time && t.start_time.is_some())
                );
                assert_eq!(summary.attempted, Some(3), "{case}");
            }
            TurboExpect::Dry(tasks) => {
                assert!(!has_summary, "{case}");
                let summary = turbo::parse_summary(&read(&dir.join("stdout"))).unwrap();
                let got: Vec<_> = summary.tasks.iter().map(|t| t.task_id.as_str()).collect();
                assert_eq!(&got, tasks, "{case}");
                assert!(
                    summary
                        .tasks
                        .iter()
                        .all(|t| t.exit_code.is_none() && !t.ran_fresh())
                );
                assert_eq!(summary.exit_code, None, "{case}");
            }
            TurboExpect::NoSummary => assert!(!has_summary, "{case}"),
        }
        check_logs(&dir.join("logs"));
    }
}

/// Every recorded log loses only the package manager's lines.
fn check_logs(dir: &Path) {
    let Ok(packages) = fs::read_dir(dir.join("packages")) else {
        return;
    };
    for package in packages {
        for log in fs::read_dir(package.unwrap().path()).unwrap() {
            let path = log.unwrap().path();
            let raw = read(&path);
            let clean = turbo::clean_log(&raw);
            assert!(
                raw.starts_with("$ ") && !clean.starts_with("$ "),
                "{}",
                path.display()
            );
            assert!(!clean.contains("ELIFECYCLE"), "{}", path.display());
            let kept: Vec<&str> = raw
                .lines()
                .skip(1)
                .filter(|l| !l.starts_with("[ELIFECYCLE]"))
                .collect();
            assert_eq!(
                clean.lines().collect::<Vec<_>>(),
                kept,
                "{}",
                path.display()
            );
        }
    }
}

fn parse_task_log(case: &str, task: &turbo::Task, adapter: Adapter) -> Parsed {
    let log_file = task.log_file.as_deref().unwrap();
    let name = Path::new(log_file).file_name().unwrap();
    let path = root()
        .join(TURBO)
        .join(case)
        .join("logs")
        .join(&task.directory)
        .join(name);
    let raw = read(&path);
    let output = ToolOutput {
        stdout: turbo::clean_log(&raw),
        stderr: "",
        exit_code: task.exit_code,
    };
    let context = ParseContext {
        cwd: &task.directory,
        files: None,
    };
    adapter.parse(&output, &context)
}

#[test]
fn turbo_type_check_tasks_parse_per_package() {
    let case = "type-check-summarize";
    let summary =
        turbo::parse_summary(&read(&root().join(TURBO).join(case).join("summary.json"))).unwrap();
    let outcomes: Vec<_> = summary
        .tasks
        .iter()
        .map(|t| (t.directory.as_str(), parse_task_log(case, t, Adapter::Tsc)))
        .collect();
    let (dir, app) = &outcomes[0];
    assert_eq!(*dir, "packages/app");
    assert_eq!(app.outcome, Outcome::Findings);
    assert_eq!(
        app.findings[0].file.as_deref(),
        Some("packages/app/src/type-error.ts")
    );
    assert_eq!(app.findings[0].rule, "tsc/TS2322");
    assert_eq!(outcomes[1].1.outcome, Outcome::Clean);
}

#[test]
fn turbo_lint_logs_in_the_default_format_never_pass() {
    let case = "lint-check-summarize";
    let summary =
        turbo::parse_summary(&read(&root().join(TURBO).join(case).join("summary.json"))).unwrap();
    for task in &summary.tasks {
        let parsed = parse_task_log(case, task, Adapter::Oxlint);
        let expected = match task.exit_code {
            Some(0) => "crashed",
            _ => "findings",
        };
        assert_eq!(parsed.outcome.name(), expected, "{}", task.task_id);
        if expected == "findings" {
            assert_eq!(parsed.findings[0].rule, "oxlint/unrecognized-output");
        }
    }
}
