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
