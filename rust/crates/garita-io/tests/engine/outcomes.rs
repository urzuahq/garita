use garita_core::report::{CheckStatus, RunStatus, Severity, Trigger};

use crate::support::{Repo, check, finding, rules};

const LINT_FILE: &str = "packages/app/src/lint-error.ts";

/// A fake oxlint that insists on JSON output, then replays a recorded run.
fn oxlint(repo: &Repo, case: &str) -> String {
    let replay = crate::support::replay_body(case);
    repo.tool(
        "oxlint",
        &format!(
            "case \" $* \" in *' --format=json '*) ;; *) echo 'no --format=json' >&2; exit 9;; esac\n{replay}"
        ),
    )
}

fn lint_repo(case: &str, extra: &str) -> Repo {
    let repo = Repo::new();
    let tool = oxlint(&repo, case);
    repo.write(LINT_FILE, "debugger;\n");
    repo.stage(&[LINT_FILE]);
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n    parser: oxlint\n{extra}"
    ));
    repo
}

#[test]
fn a_failing_lint_yields_findings_with_file_line_rule_and_message() {
    let repo = lint_repo("oxlint@1.94.0/one-error-json", "");
    let out = repo.run();
    let r = &out.report;
    assert_eq!(r.status, RunStatus::Fail);
    assert_eq!(out.exit_code(), 1);
    assert!(r.blocking);
    let f = r.findings.iter().find(|f| f.check == "lint").unwrap();
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.file.as_deref(), Some(LINT_FILE));
    assert_eq!(f.line.map(|l| l.get()), Some(2));
    assert!(f.rule.contains("no-debugger"), "{}", f.rule);
    assert!(f.message.contains("debugger"), "{}", f.message);
    assert!(!f.message.contains("exited with"), "{}", f.message);
    assert!(f.fingerprint.is_some());
    assert_eq!(
        r.next_action.as_deref(),
        Some(&*format!("garita run lint --files {LINT_FILE}"))
    );
}

#[test]
fn a_check_that_ran_on_nothing_is_a_no_op_warning_never_a_pass() {
    let repo = lint_repo("oxlint@1.94.0/no-files-matched-json", "");
    let out = repo.run();
    let r = &out.report;
    assert_eq!(r.status, RunStatus::Warn);
    assert_eq!(out.exit_code(), 0);
    assert_eq!(check(r, "lint").status, CheckStatus::Warn);
    let f = finding(r, "no-op");
    assert_eq!(f.severity, Severity::Warning);
    assert!(
        f.message.contains("was given 1 file and examined none"),
        "{}",
        f.message
    );
}

#[test]
fn no_config_is_not_run_exit_2_without_a_receipt() {
    let repo = Repo::new();
    let out = repo.run();
    assert_eq!(out.report.status, RunStatus::NotRun);
    assert_eq!(out.exit_code(), 2);
    assert!(!out.report.blocking);
    let error = out.report.error.as_deref().unwrap();
    assert!(
        error.contains(".garita/config.yaml does not exist"),
        "{error}"
    );
    assert!(out.report.tree.is_some());
    assert!(repo.receipts().is_empty());
}

#[test]
fn a_broken_config_is_not_run_and_lists_every_error() {
    let repo = Repo::new();
    repo.config("schema_version: 1\nchecks:\n  a:\n    run: x\n    colour: red\n  b:\n    run: y\n    on: [later]\n");
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    let error = out.report.error.as_deref().unwrap();
    assert!(error.contains("checks.a.colour"), "{error}");
    assert!(error.contains("checks.b.on[0]"), "{error}");
}

#[test]
fn a_missing_tool_binary_is_not_run_never_a_pass() {
    let repo = Repo::new();
    repo.write("a.ts", "x\n");
    repo.stage(&["a.ts"]);
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: /nonexistent/garita-tool {files}\n");
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    assert_eq!(check(&out.report, "lint").status, CheckStatus::NotRun);
    let error = out.report.error.as_deref().unwrap();
    assert!(
        error.contains("checks.lint") && error.contains("could not start"),
        "{error}"
    );
    assert_eq!(out.report.next_action.as_deref(), Some("garita doctor"));
    assert_eq!(repo.receipts().len(), 1);
}

#[test]
fn a_hook_style_multi_step_config_runs_every_step() {
    let repo = Repo::new();
    let marks = repo.base().join("marks");
    std::fs::create_dir_all(&marks).unwrap();
    let mut yaml = "schema_version: 1\nchecks:\n".to_string();
    for (i, code) in [0, 1, 0].iter().enumerate() {
        let tool = repo.tool(
            &format!("step{i}"),
            &format!("touch '{}/step{i}'\nexit {code}", marks.display()),
        );
        yaml.push_str(&format!("  step{i}:\n    run: {tool}\n"));
    }
    repo.config(&yaml);
    let out = repo.run();
    for i in 0..3 {
        assert!(
            marks.join(format!("step{i}")).exists(),
            "step{i} did not run"
        );
    }
    let statuses: Vec<_> = out.report.checks.iter().map(|c| c.status).collect();
    assert_eq!(
        statuses,
        [CheckStatus::Pass, CheckStatus::Fail, CheckStatus::Pass]
    );
    assert_eq!(out.exit_code(), 1);
    assert_eq!(out.report.next_action.as_deref(), Some("garita run step1"));
}

fn exit_tool_repo(codes: &[(&str, &str)]) -> Repo {
    let repo = Repo::new();
    let mut yaml = "schema_version: 1\nchecks:\n".to_string();
    for (name, body) in codes {
        let tool = repo.tool(name, body);
        yaml.push_str(&format!("  {name}:\n    run: {tool}\n"));
    }
    repo.config(&yaml);
    repo
}

#[test]
fn a_clean_run_passes_with_exit_0() {
    let out = exit_tool_repo(&[("ok", "exit 0")]).run();
    assert_eq!(
        (out.report.status, out.exit_code(), out.report.blocking),
        (RunStatus::Pass, 0, false)
    );
    assert_eq!(out.report.next_action, None);
}

#[test]
fn a_warning_run_warns_with_exit_0() {
    let repo = lint_repo("oxlint@1.94.0/warning-only-json", "");
    let out = repo.run();
    assert_eq!(
        (out.report.status, out.exit_code(), out.report.blocking),
        (RunStatus::Warn, 0, false)
    );
}

#[test]
fn a_failing_run_fails_with_exit_1() {
    let out = exit_tool_repo(&[("bad", "echo broken >&2; exit 3")]).run();
    assert_eq!(
        (out.report.status, out.exit_code(), out.report.blocking),
        (RunStatus::Fail, 1, true)
    );
    assert!(
        finding(&out.report, "exit-code/failed")
            .message
            .contains("broken")
    );
}

#[test]
fn mixed_outcomes_not_run_wins() {
    let repo = exit_tool_repo(&[("bad", "exit 1")]);
    let config = std::fs::read_to_string(repo.root().join(".garita/config.yaml")).unwrap();
    repo.config(&format!("{config}  gone:\n    run: /nonexistent/tool\n"));
    let out = repo.run();
    assert_eq!(check(&out.report, "bad").status, CheckStatus::Fail);
    assert_eq!(
        (out.report.status, out.exit_code(), out.report.blocking),
        (RunStatus::NotRun, 2, false)
    );
}

#[test]
fn advisory_caps_a_checks_findings_at_warning() {
    let repo = lint_repo("oxlint@1.94.0/one-error-json", "    advisory: true\n");
    let out = repo.run();
    assert!(
        out.report
            .findings
            .iter()
            .all(|f| f.severity != Severity::Error)
    );
    assert_eq!(check(&out.report, "lint").status, CheckStatus::Warn);
    assert_eq!(out.exit_code(), 0);
}

#[test]
fn native_and_builtin_checks_are_not_run_by_this_version() {
    for body in ["    native: urzua\n", "    builtin: clean-tree\n"] {
        let repo = Repo::new();
        repo.config(&format!("schema_version: 1\nchecks:\n  c:\n{body}"));
        let out = repo.run();
        assert_eq!(out.exit_code(), 2, "{body}");
        let error = out.report.error.as_deref().unwrap();
        assert!(
            error.contains("checks.c") && error.contains("not supported by this version"),
            "{error}"
        );
    }
}

#[test]
fn an_unknown_named_check_is_not_run() {
    let repo = exit_tool_repo(&[("ok", "exit 0")]);
    let out = repo.run_named(Trigger::Commit, &["ok", "nope"]);
    assert_eq!(check(&out.report, "ok").status, CheckStatus::Pass);
    assert_eq!(check(&out.report, "nope").status, CheckStatus::NotRun);
    assert!(
        out.report
            .error
            .as_deref()
            .unwrap()
            .contains("`nope` is not a check")
    );
    assert_eq!(out.exit_code(), 2);
}

#[test]
fn an_unsatisfied_requires_is_not_run_with_a_receipt() {
    let repo = Repo::new();
    repo.config("schema_version: 1\nrequires: '>=9.0.0'\nchecks:\n  a:\n    run: x\n");
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    let error = out.report.error.as_deref().unwrap();
    assert!(
        error.contains("requires") && error.contains(">=9.0.0") && error.contains("0.1.0"),
        "{error}"
    );
    assert_eq!(repo.receipts().len(), 1);
}

#[test]
fn an_unknown_parser_is_not_run() {
    let repo = Repo::new();
    repo.config("schema_version: 1\nchecks:\n  a:\n    run: x\n    parser: eslint\n");
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    assert!(
        out.report
            .error
            .as_deref()
            .unwrap()
            .contains("unknown parser `eslint`")
    );
}

#[test]
fn oxlint_with_another_format_is_not_run() {
    let repo = Repo::new();
    repo.config(
        "schema_version: 1\nchecks:\n  a:\n    run: oxlint --format stylish\n    parser: oxlint\n",
    );
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    assert!(
        out.report
            .error
            .as_deref()
            .unwrap()
            .contains("--format stylish")
    );
}

#[test]
fn affected_without_turbo_is_a_config_error() {
    let repo = Repo::new();
    repo.config("schema_version: 1\nchecks:\n  a:\n    run: tool {affected}\n");
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    assert!(
        out.report
            .error
            .as_deref()
            .unwrap()
            .contains("checks.a.run")
    );
}

#[test]
fn a_timeout_is_a_crash_warning() {
    let repo = exit_tool_repo(&[("slow", "exec sleep 5")]);
    let config = std::fs::read_to_string(repo.root().join(".garita/config.yaml")).unwrap();
    repo.config(&format!(
        "{config}    budget: {{ duration: 100ms }}\ntimeout: {{ factor: 3 }}\n"
    ));
    let out = repo.run();
    let f = finding(&out.report, "crash");
    assert_eq!(f.severity, Severity::Warning);
    assert!(f.message.contains("timed out"), "{}", f.message);
    assert_eq!(out.exit_code(), 0);
    let receipt = repo.receipt_of(&out.report);
    assert_eq!(receipt.checks[0].exit_code, None);
}

#[test]
fn a_timeout_with_on_crash_fail_is_an_error_and_exit_1() {
    let repo = exit_tool_repo(&[("slow", "exec sleep 5")]);
    let config = std::fs::read_to_string(repo.root().join(".garita/config.yaml")).unwrap();
    repo.config(&format!(
        "{config}    budget: {{ duration: 100ms }}\ntimeout: {{ factor: 3 }}\non_crash: fail\n"
    ));
    let out = repo.run();
    assert_eq!(finding(&out.report, "crash").severity, Severity::Error);
    assert_eq!(out.exit_code(), 1);
}

#[test]
fn a_tool_killed_by_a_signal_is_a_crash() {
    let out = exit_tool_repo(&[("dies", "kill -9 $$")]).run();
    let f = finding(&out.report, "crash");
    assert!(f.message.contains("signal 9"), "{}", f.message);
    assert_eq!(f.severity, Severity::Warning);
}

#[test]
fn a_check_without_a_placeholder_runs_even_with_an_empty_scope() {
    let repo = Repo::new();
    let tool = repo.recorder("always");
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  always:\n    run: {tool} --all\n"
    ));
    let out = repo.run();
    assert_eq!(check(&out.report, "always").status, CheckStatus::Pass);
    assert_eq!(repo.log("always").len(), 1);
}

#[test]
fn a_files_check_with_no_matching_file_is_skipped_with_no_input() {
    let repo = Repo::new();
    let tool = repo.recorder("lint");
    repo.write("a.md", "x\n");
    repo.stage(&["a.md"]);
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n    globs: ['**/*.ts']\n"
    ));
    let out = repo.run();
    assert_eq!(check(&out.report, "lint").status, CheckStatus::Skipped);
    assert_eq!(finding(&out.report, "no-input").severity, Severity::Info);
    assert!(repo.log("lint").is_empty());
    assert_eq!(rules(&out.report, "lint"), ["no-input"]);
    assert_eq!(out.exit_code(), 0);
}

#[test]
fn a_run_where_no_check_applies_to_the_trigger_warns_with_no_checks() {
    for checks in ["checks:\n  late:\n    run: x\n    on: [ci]\n", ""] {
        let repo = Repo::new();
        repo.config(&format!("schema_version: 1\n{checks}"));
        let out = repo.run();
        let f = finding(&out.report, "no-checks");
        assert_eq!(
            (f.check.as_str(), f.severity),
            ("garita", Severity::Warning)
        );
        assert!(f.message.contains("`commit` trigger"), "{}", f.message);
        assert_eq!((out.report.status, out.exit_code()), (RunStatus::Warn, 0));
    }
}

#[test]
fn selected_but_skipped_or_named_checks_get_no_no_checks_finding() {
    let repo = Repo::new();
    let tool = repo.recorder("lint");
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n    on: [ci]\n"
    ));
    assert!(!rules(&repo.run_at(Trigger::Ci).report, "garita").contains(&"no-checks"));
    assert!(
        !rules(&repo.run_named(Trigger::Commit, &["lint"]).report, "garita").contains(&"no-checks")
    );
}

#[test]
fn many_files_are_split_into_batches_and_one_failing_batch_still_fails_the_run() {
    let repo = Repo::new();
    let calls = repo.log_path("calls");
    let tool = repo.tool(
        "lint",
        &format!(
            "echo x >> '{}'\nfor f; do case \"$f\" in *target*) echo \"bad $f\" >&2; exit 1;; esac; done\nexit 0",
            calls.display()
        ),
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n"
    ));
    let dir = "d".repeat(180);
    let total = 1800;
    for i in 0..total {
        let name = if i == 1000 {
            format!("{dir}/target.ts")
        } else {
            format!("{dir}/f{i:05}.ts")
        };
        repo.write(&name, "");
    }
    repo.stage(&[&dir]);
    let out = repo.run();
    assert!(repo.log("calls").len() >= 3, "{}", repo.log("calls").len());
    assert_eq!(out.exit_code(), 1);
    assert!(
        finding(&out.report, "exit-code/failed")
            .message
            .contains("target.ts")
    );
    assert_eq!(
        check(&out.report, "lint").examined.get("files"),
        Some(&(total as u64))
    );
}
