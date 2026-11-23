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
        Some("garita run --trigger push types --files packages/app/src/type-error.ts")
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
    assert_eq!(f.check, "types");
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

/// Each task's log as pnpm wraps it: its echo of the command, oxlint's JSON, its failure line.
fn writes_oxlint_logs() -> String {
    let oxlint = recording("oxlint@1.94.0");
    let turbo = recording(LINT_CASE);
    let task = |pkg: &str, case: &str, failed: bool| {
        let tail = if failed {
            "printf '[ELIFECYCLE] Command failed with exit code 1.\\n'"
        } else {
            "true"
        };
        format!(
            "{{ printf '$ oxlint src --format=json\\n'; cat '{o}/{case}/stdout'; {tail}; }} \
             > 'packages/{pkg}/{LOG}'\n",
            o = oxlint.display()
        )
    };
    format!(
        "mkdir -p .turbo/runs packages/app/.turbo packages/docs/.turbo packages/lib/.turbo\n\
         cp '{}/summary.json' .turbo/runs/LINT.json\n{}{}{}exit 1",
        turbo.display(),
        task("app", "no-files-matched-json", false),
        task("docs", "clean-json", false),
        task("lib", "one-error-in-package-json", true),
    )
}

#[test]
fn oxlint_through_turbo_gets_its_flag_after_the_separator_and_reports_file_and_line() {
    let repo = lint_repo(&writes_oxlint_logs());
    let out = repo.run();
    assert_eq!(
        repo.log("turbo"),
        vec!["lint:check --output-logs=errors-only --summarize --continue -- --format=json"]
    );
    let errors: Vec<_> = out
        .report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{:#?}", out.report.findings);
    assert_eq!(
        errors[0].file.as_deref(),
        Some("packages/lib/src/lint-error.ts")
    );
    assert_eq!(errors[0].line.map(|l| l.get()), Some(2));
    assert_eq!(errors[0].rule, "oxlint/eslint/no-debugger");
    assert_eq!(check(&out.report, "lint").status, CheckStatus::Fail);
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
    assert!(
        f.message.contains("unexpected argument '--bogus' found"),
        "{}",
        f.message
    );
    assert!(f.message.contains("Usage: turbo run"), "{}", f.message);
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
    assert!(
        errors[0].message.starts_with("b#test: exited with code 1:"),
        "{}",
        errors[0].message
    );
    assert!(errors[0].message.contains("assertion failed: 1 == 2"));
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
                .filter(|f| f.check == name && f.severity == Severity::Error)
                .map(|f| f.message.as_str())
                .collect();
            assert_eq!(messages.len(), 1, "{messages:#?}");
            assert!(messages[0].starts_with(own), "{}", messages[0]);
            assert!(!messages[0].contains(other), "{}", messages[0]);
        }
    }
}
