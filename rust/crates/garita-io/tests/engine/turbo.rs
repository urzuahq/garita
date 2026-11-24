use garita_core::report::{CheckStatus, Severity, Trigger};

use crate::support::{Repo, check, finding, recording};

const CASE: &str = "turbo@2.5.6/type-check-summarize";

fn turbo_repo(body: &str) -> Repo {
    let repo = Repo::new();
    let log = repo.log_path("turbo");
    let tool = repo.tool(
        "turbo",
        &format!(
            "printf '%s|%s|%s\\n' \"$PWD\" \"$*\" \"${{TURBO_SCM_BASE:-}}\" >> '{}'\n{body}",
            log.display()
        ),
    );
    repo.config(&format!(
        "schema_version: 1\ntriggers:\n  push: {{ scope: affected }}\nchecks:\n  types:\n    \
         run: {tool} run type-check {{affected}}\n    parser: tsc\n    via: turbo\n"
    ));
    repo.write(
        "packages/app/src/type-error.ts",
        "export const n: number = 'x';\n",
    );
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.write("packages/app/src/more.ts", "x\n");
    repo.commit_all("change");
    repo
}

fn writes_recorded_summary() -> String {
    let d = recording(CASE);
    format!(
        "mkdir -p .turbo/runs packages/app/.turbo packages/lib/.turbo\n\
         cp '{d}/summary.json' .turbo/runs/2ZRUNID.json\n\
         cp '{d}/logs/packages/app/turbo-type-check.log' packages/app/.turbo/\n\
         cp '{d}/logs/packages/lib/turbo-type-check.log' packages/lib/.turbo/\n\
         exit 2",
        d = d.display()
    )
}

#[test]
fn turbo_findings_come_from_each_tasks_log_rebased_from_its_package() {
    let repo = turbo_repo(&writes_recorded_summary());
    let out = repo.run_at(Trigger::Push);
    let errors: Vec<_> = out
        .report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{:#?}", out.report.findings);
    assert_eq!(
        errors[0].file.as_deref(),
        Some("packages/app/src/type-error.ts")
    );
    assert_eq!(errors[0].line.map(|l| l.get()), Some(1));
    assert!(errors[0].rule.contains("TS2322"), "{}", errors[0].rule);
    assert_eq!(check(&out.report, "types").status, CheckStatus::Fail);
    assert_eq!(
        out.report.next_action.as_deref(),
        Some("garita run --trigger push types")
    );
    let log = repo.log("turbo");
    assert_eq!(log.len(), 1);
    assert!(
        log[0].ends_with("|run type-check --affected --summarize --continue|main"),
        "{}",
        log[0]
    );
}

#[test]
fn an_older_turbo_summary_is_not_read_as_this_runs() {
    let repo = turbo_repo("exit 0");
    repo.write(
        ".turbo/runs/OLD.json",
        &std::fs::read_to_string(recording(CASE).join("summary.json")).unwrap(),
    );
    let out = repo.run_at(Trigger::Push);
    let f = finding(&out.report, "turbo/unrecognized-output");
    assert!(f.message.contains("no run summary"), "{}", f.message);
}

#[test]
fn a_missing_turbo_summary_is_an_unrecognized_output_error() {
    let repo = turbo_repo("exit 2");
    let out = repo.run_at(Trigger::Push);
    let f = finding(&out.report, "turbo/unrecognized-output");
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.check.as_deref(), Some("types"));
    assert_eq!(out.exit_code(), 1);
}

#[test]
fn a_missing_task_log_is_an_unrecognized_output_error() {
    let d = recording(CASE);
    let repo = turbo_repo(&format!(
        "mkdir -p .turbo/runs\ncp '{}/summary.json' .turbo/runs/X.json\nexit 2",
        d.display()
    ));
    let out = repo.run_at(Trigger::Push);
    let f = finding(&out.report, "turbo/unrecognized-output");
    assert!(f.message.contains("cannot be read"), "{}", f.message);
}

#[test]
fn affected_expands_to_nothing_outside_the_affected_scope() {
    let repo = turbo_repo(&writes_recorded_summary());
    repo.run_at(Trigger::Ci);
    let log = repo.log("turbo");
    assert!(
        log[0].ends_with("|run type-check --summarize --continue|"),
        "{}",
        log[0]
    );
}

const LINT_CASE: &str = "turbo@2.5.6/lint-check-summarize";
const LOG: &str = ".turbo/turbo-lint$colon$check.log";

fn lint_repo(body: &str) -> Repo {
    let repo = Repo::new();
    let log = repo.log_path("turbo");
    let tool = repo.tool(
        "turbo",
        &format!("printf '%s\\n' \"$*\" >> '{}'\n{body}", log.display()),
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    \
         run: {tool} lint:check --output-logs=errors-only\n    parser: oxlint\n    via: turbo\n"
    ));
    repo.write("packages/lib/src/lint-error.ts", "debugger;\n");
    repo.stage(&["packages/lib/src/lint-error.ts"]);
    repo
}

/// Each task's log as pnpm wraps it: its echo of the command, the script's own output, and its
/// failure line. `app` runs oxlint cleanly, `lib` finds an error, and `docs` is another tool.
fn writes_lint_logs(docs_fails: bool) -> String {
    let oxlint = recording("oxlint@1.94.0");
    let turbo = recording(LINT_CASE);
    let failed = "printf '[ELIFECYCLE] Command failed with exit code 1.\\n'";
    let task = |pkg: &str, source: String, failed: Option<&str>| {
        format!(
            "{{ printf '$ lint\\n'; {source}; {}; }} > 'packages/{pkg}/{LOG}'\n",
            failed.unwrap_or("true")
        )
    };
    let recorded = |case: &str| format!("cat '{}/{case}/stdout'", oxlint.display());
    let docs_exit = if docs_fails {
        // The third exit code in the recorded summary, after the run's and app's, is docs'.
        "awk '/\"exitCode\"/ { n++; if (n == 3) sub(/0/, \"1\") } { print }' .turbo/runs/LINT.json \
         > .turbo/runs/LINT.tmp && mv .turbo/runs/LINT.tmp .turbo/runs/LINT.json\n"
    } else {
        ""
    };
    format!(
        "mkdir -p .turbo/runs packages/app/.turbo packages/docs/.turbo packages/lib/.turbo\n\
         cp '{}/summary.json' .turbo/runs/LINT.json\n{docs_exit}{}{}{}exit 1",
        turbo.display(),
        task("app", recorded("clean-default"), None),
        task(
            "docs",
            "printf 'markdownlint: checked 12 files\\nsee {{ docs }} for details\\n'".to_string(),
            docs_fails.then_some("printf 'markdownlint: MD013 line too long\\n'; false"),
        ),
        task(
            "lib",
            recorded("one-error-in-package-default"),
            Some(failed)
        ),
    )
}

fn errors(report: &garita_core::report::Report) -> Vec<&garita_core::report::Finding> {
    report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

#[test]
fn oxlint_through_turbo_reads_each_scripts_own_output_and_injects_nothing() {
    let repo = lint_repo(&writes_lint_logs(false));
    let out = repo.run();
    assert_eq!(
        repo.log("turbo"),
        vec!["lint:check --output-logs=errors-only --summarize --continue"]
    );
    let errors = errors(&out.report);
    assert_eq!(errors.len(), 1, "{:#?}", out.report.findings);
    assert_eq!(
        errors[0].file.as_deref(),
        Some("packages/lib/src/lint-error.ts")
    );
    assert_eq!(errors[0].line.map(|l| l.get()), Some(2));
    assert_eq!(errors[0].rule, "oxlint/eslint/no-debugger");
    assert_eq!(check(&out.report, "lint").status, CheckStatus::Fail);
    assert_eq!(out.exit_code(), 1);
}

#[test]
fn another_tool_failing_through_turbo_is_named_for_its_task() {
    let repo = lint_repo(&writes_lint_logs(true));
    let out = repo.run();
    let errors = errors(&out.report);
    assert_eq!(errors.len(), 2, "{:#?}", out.report.findings);
    let docs = errors
        .iter()
        .find(|f| f.rule == "oxlint/unrecognized-output")
        .unwrap_or_else(|| panic!("{:#?}", out.report.findings));
    assert!(
        docs.message.starts_with("@fixture/docs#lint:check: "),
        "{}",
        docs.message
    );
    let notable = &check(&out.report, "lint").output_notable;
    assert!(
        notable
            .iter()
            .any(|l| l.starts_with("@fixture/docs#lint:check: ")
                && l.contains("MD013 line too long")),
        "{notable:?}"
    );
    assert_eq!(out.exit_code(), 1);
}

#[test]
fn a_turbo_usage_error_says_what_turbo_printed() {
    let repo = lint_repo(
        "echo ' ERROR  unexpected argument '\"'\"'--bogus'\"'\"' found' >&2\n\
         echo 'Usage: turbo run [OPTIONS] [TASKS]...' >&2\nexit 1",
    );
    let out = repo.run();
    let f = finding(&out.report, "turbo/unrecognized-output");
    assert!(f.message.contains("no run summary"), "{}", f.message);
    assert!(!f.message.contains('\n'), "{}", f.message);
    let lint = check(&out.report, "lint");
    let tail = lint.output_tail.as_deref().unwrap_or_default();
    assert!(
        tail.contains("unexpected argument '--bogus' found"),
        "{tail}"
    );
    assert!(tail.contains("Usage: turbo run"), "{tail}");
    assert_eq!(
        lint.output_notable,
        ["ERROR  unexpected argument '--bogus' found"]
    );
}

/// A turbo summary with one task per `(package, exit)` pair, and each task's log.
fn writes_tasks(task: &str, run_id: &str, tasks: &[(&str, i32, &str)]) -> String {
    let entries: Vec<String> = tasks
        .iter()
        .map(|(pkg, code, _)| {
            format!(
                "{{\"taskId\":\"{pkg}#{task}\",\"task\":\"{task}\",\"package\":\"{pkg}\",\
                 \"directory\":\"packages/{pkg}\",\"logFile\":\"packages/{pkg}/.turbo/{task}.log\",\
                 \"cache\":{{\"status\":\"MISS\"}},\"execution\":{{\"exitCode\":{code}}}}}"
            )
        })
        .collect();
    let mut body = String::from("mkdir -p .turbo/runs\n");
    for (pkg, _, log) in tasks {
        body.push_str(&format!(
            "mkdir -p packages/{pkg}/.turbo\nprintf '%s\\n' '{log}' > packages/{pkg}/.turbo/{task}.log\n"
        ));
    }
    body.push_str(&format!(
        "printf '%s' '{{\"tasks\":[{}]}}' > .turbo/runs/{run_id}.json\n",
        entries.join(",")
    ));
    body
}

#[test]
fn exit_code_through_turbo_names_each_failing_task() {
    let repo = Repo::new();
    let body = writes_tasks(
        "test",
        "T",
        &[
            ("a", 0, "all good"),
            ("b", 1, "assertion failed: 1 == 2"),
            ("c", 0, "fine"),
        ],
    );
    let tool = repo.tool("turbo", &format!("{body}exit 1"));
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  tests:\n    run: {tool} run test\n    via: turbo\n"
    ));
    let out = repo.run_at(Trigger::Ci);
    let errors: Vec<_> = out
        .report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{:#?}", out.report.findings);
    assert_eq!(errors[0].file, None);
    assert_eq!(errors[0].message, "b#test: exited with code 1");
    assert_eq!(
        check(&out.report, "tests").output_notable,
        ["b#test: assertion failed: 1 == 2"]
    );
    assert_eq!(check(&out.report, "tests").status, CheckStatus::Fail);
}

/// `lint` writes its summary first and exits last, so a newest-summary lookup would hand it
/// the one `types` wrote meanwhile.
#[test]
fn two_turbo_checks_in_one_directory_each_read_only_their_own_summary() {
    let repo = Repo::new();
    for dir in ["bin/lint", "bin/types"] {
        std::fs::create_dir_all(repo.base().join(dir)).unwrap();
    }
    let lint = repo.tool(
        "lint/turbo",
        &format!(
            "{}sleep 0.3\nexit 1",
            writes_tasks("lint", "L", &[("app", 1, "lint error: no-debugger")])
        ),
    );
    let types = repo.tool(
        "types/turbo",
        &format!(
            "sleep 0.1\n{}exit 1",
            writes_tasks("types", "T", &[("lib", 1, "type error: TS2322")])
        ),
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {lint} run lint\n    via: turbo\n  \
         types:\n    run: {types} run types\n    via: turbo\n"
    ));
    for _ in 0..3 {
        let _ = std::fs::remove_dir_all(repo.root().join(".turbo"));
        let out = repo.run_at(Trigger::Ci);
        for (name, own, other) in [
            ("lint", "app#lint", "lib#types"),
            ("types", "lib#types", "app#lint"),
        ] {
            let messages: Vec<&str> = out
                .report
                .findings
                .iter()
                .filter(|f| f.check.as_deref() == Some(name) && f.severity == Severity::Error)
                .map(|f| f.message.as_str())
                .collect();
            assert_eq!(messages.len(), 1, "{messages:#?}");
            assert!(messages[0].starts_with(own), "{}", messages[0]);
            assert!(!messages[0].contains(other), "{}", messages[0]);
        }
    }
}

/// `<name> <sleep before> <exit code> [sleep after writing the summary]`: a task runner with no
/// `turbo` in its name, so any new summary in its directory reads as its own.
const RUNNER: &str = r#"sleep "$2"
mkdir -p .turbo/runs "packages/$1/.turbo"
echo "$1 says exit $3" > "packages/$1/.turbo/t.log"
printf '{"tasks":[{"taskId":"%s#t","task":"t","package":"%s","directory":"packages/%s","logFile":"packages/%s/.turbo/t.log","cache":{"status":"MISS"},"execution":{"exitCode":%s}}]}' "$1" "$1" "$1" "$1" "$3" > ".turbo/runs/$1-$$.json"
sleep "${4:-0}"
exit "$3""#;

#[test]
fn via_checks_run_one_at_a_time_whatever_their_directories() {
    let repo = Repo::new();
    let pk = repo.tool("pk", RUNNER);
    let pk2 = repo.tool("pk2", RUNNER);
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  \
         a: {{ run: {pk} a 0.1 0, via: turbo, partition: {{ by: dir, roots: [p] }} }}\n  \
         b: {{ run: {pk2} b 0.7 1, via: turbo, cwd: q }}\n  \
         c: {{ run: {pk2} c 0.05 0 0.4, via: turbo, partition: {{ by: dir, roots: [p, q] }} }}\n"
    ));
    repo.write("p/x.ts", "x\n");
    repo.write("q/y.ts", "y\n");
    repo.stage(&["p/x.ts", "q/y.ts"]);
    for _ in 0..3 {
        let out = repo.run();
        let failed: Vec<&str> = out
            .report
            .checks
            .iter()
            .filter(|c| c.status != CheckStatus::Pass)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(failed, ["b"], "{:#?}", out.report.findings);
        assert_eq!(check(&out.report, "c").status, CheckStatus::Pass);
    }
}

/// A summary whose tasks are not cached, so turbo names no log file for any of them.
fn uncached_turbo(b_exit: i32) -> Repo {
    let repo = Repo::new();
    let task = |pkg: &str, code: i32| {
        format!(
            "{{\"taskId\":\"{pkg}#check\",\"task\":\"check\",\"package\":\"{pkg}\",\
             \"directory\":\"packages/{pkg}\",\"logFile\":null,\"cache\":{{\"status\":\"MISS\"}},\
             \"execution\":{{\"exitCode\":{code}}}}}"
        )
    };
    let summary = format!("{{\"tasks\":[{},{}]}}", task("a", 0), task("b", b_exit));
    let say = if b_exit == 0 {
        "echo 'b:check: all fine'".to_string()
    } else {
        "echo 'b:check: FATAL: schema 7 is missing' >&2".to_string()
    };
    let tool = repo.tool(
        "turbo",
        &format!(
            "mkdir -p .turbo/runs\nprintf '%s' '{summary}' > .turbo/runs/U.json\n{say}\nexit {b_exit}"
        ),
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  check:\n    run: {tool} run check\n    via: turbo\n"
    ));
    repo
}

#[test]
fn tasks_without_log_files_that_succeed_pass() {
    let out = uncached_turbo(0).run_at(Trigger::Ci);
    assert_eq!(check(&out.report, "check").status, CheckStatus::Pass);
    assert_eq!(out.exit_code(), 0, "{:#?}", out.report.findings);
}

#[test]
fn a_failing_task_without_a_log_file_shows_turbos_own_output() {
    let out = uncached_turbo(1).run_at(Trigger::Ci);
    let errors = errors(&out.report);
    assert_eq!(errors.len(), 1, "{:#?}", out.report.findings);
    assert_eq!(errors[0].rule, "turbo/task-failed");
    assert_eq!(errors[0].message, "b#check: exited with code 1");
    let tail = check(&out.report, "check")
        .output_tail
        .clone()
        .unwrap_or_default();
    assert!(tail.contains("FATAL: schema 7 is missing"), "{tail}");
    assert_eq!(out.exit_code(), 1);
}

#[test]
fn a_task_that_examined_nothing_and_failed_is_an_error_naming_it() {
    let repo = Repo::new();
    let body = writes_tasks(
        "lint",
        "N",
        &[(
            "app",
            1,
            "No files found to lint. Please check your paths and ignore patterns.",
        )],
    );
    let tool = repo.tool("turbo", &format!("{body}exit 1"));
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} run lint\n    parser: oxlint\n    via: turbo\n"
    ));
    let out = repo.run_at(Trigger::Ci);
    let errors = errors(&out.report);
    assert_eq!(errors.len(), 1, "{:#?}", out.report.findings);
    assert_eq!(errors[0].rule, "no-op");
    assert_eq!(
        errors[0].message,
        "app#lint: lint exited with code 1 and examined nothing in packages/app"
    );
    assert_eq!(
        check(&out.report, "lint").output_notable,
        ["app#lint: No files found to lint. Please check your paths and ignore patterns."]
    );
    assert_eq!(out.exit_code(), 1);
}

fn turbo_writing(summary: &str, exit: i32) -> Repo {
    let repo = Repo::new();
    let tool = repo.tool(
        "turbo",
        &format!("mkdir -p .turbo/runs\nprintf '%s' '{summary}' > .turbo/runs/S.json\nexit {exit}"),
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  check:\n    run: {tool} run check\n    via: turbo\n"
    ));
    repo
}

#[test]
fn a_summary_without_this_checks_tasks_says_so_and_fails() {
    let other = "{\"tasks\":[{\"taskId\":\"a#build\",\"task\":\"build\",\"package\":\"a\",\
                 \"directory\":\"packages/a\",\"cache\":{\"status\":\"MISS\"},\
                 \"execution\":{\"exitCode\":0}}]}";
    for summary in ["{\"tasks\":[]}", other] {
        let out = turbo_writing(summary, 0).run_at(Trigger::Ci);
        let f = finding(&out.report, "turbo/unrecognized-output");
        assert!(
            f.message
                .starts_with("turbo's run summary holds no task for check"),
            "{}",
            f.message
        );
        assert_eq!(out.exit_code(), 1);
    }
}

#[test]
fn a_task_with_no_execution_record_is_a_no_op_warning() {
    let summary = "{\"tasks\":[{\"taskId\":\"a#check\",\"task\":\"check\",\"package\":\"a\",\
                   \"directory\":\"packages/a\",\"logFile\":null,\"cache\":{\"status\":\"MISS\"}}]}";
    let out = turbo_writing(summary, 0).run_at(Trigger::Ci);
    let f = finding(&out.report, "no-op");
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.message, "a#check has no execution record");
    assert_eq!(out.exit_code(), 0);
    assert_eq!(check(&out.report, "check").status, CheckStatus::Warn);
}
