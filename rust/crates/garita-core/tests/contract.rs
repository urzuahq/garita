use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use garita_core::config::Config;
use garita_core::describe::{CONTRACT_VERSION, describe};
use garita_core::receipt::{RECEIPT_VERSION, Receipt, ReceiptCheck, ResultWord, Tool};
use garita_core::report::{
    CheckEntry, CheckKind, CheckStatus, Finding, Report, RequiredCheck, Risk, RunContext, RunError,
    Scope, ScopeKind, Severity, Trigger,
};
use garita_core::risk::Touched;

const COMMIT: &str = "1b29de5ea3f04c7d9a6b2e8f5c1d0a7b3e9f4c62";
const TREE: &str = "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80";
const WORKTREE: &str = "4e7a1c9b2d5f8036a1b4c7e0d3f6a9c2b5e8d1f4";
const RUN_ID: &str = "01M7XM3Q4T8Z6R2K9V5B1N0HCW";
const STARTED: &str = "2026-11-21T03:15:02Z";
const FINISHED: &str = "2026-11-21T03:15:04Z";
const RECEIPT_PATH: &str = ".garita/receipts/20261121T031502Z-push-01M7XM3Q4T8Z6R2K9V5B1N0HCW.json";

fn scope() -> Scope {
    Scope {
        kind: ScopeKind::Affected,
        files: Some(vec!["src/app.ts".into(), "src/util.ts".into()]),
        packages: Some(vec!["frontend".into()]),
    }
}

fn context(receipt: bool) -> RunContext {
    RunContext {
        trigger: Some(Trigger::Push),
        commit: Some(COMMIT.into()),
        tree: Some(TREE.into()),
        scope: Some(scope()),
        receipt: receipt.then(|| RECEIPT_PATH.into()),
        ..RunContext::default()
    }
}

fn entry(name: &str, kind: CheckKind, status: CheckStatus, files: u64) -> CheckEntry {
    CheckEntry {
        duration_ms: 412,
        examined: [("files".to_string(), files)].into(),
        rerun: Some(format!("garita run {name}")),
        output: Some(format!(".garita/receipts/{RUN_ID}/{name}.log")),
        ..CheckEntry::new(name, kind, status)
    }
}

fn finding(check: &str, severity: Severity, file: Option<&str>, line: Option<u64>) -> Finding {
    Finding {
        check: Some(check.into()),
        severity,
        rule: format!("{check}/rule"),
        file: file.map(Into::into),
        line: line.and_then(std::num::NonZeroU64::new),
        column: line.and_then(|_| std::num::NonZeroU64::new(5)),
        message: "Unexpected `debugger` statement".into(),
        fix: None,
        fix_argv: None,
        fingerprint: Some("5d1c0e9a".into()),
    }
}

fn pass_report() -> Report {
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Pass, 38),
        entry("format", CheckKind::Format, CheckStatus::Pass, 38),
    ];
    Report::from_run(context(true), checks, Vec::new(), None)
}

fn warn_report() -> Report {
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Warn, 38),
        entry("format", CheckKind::Format, CheckStatus::Pass, 38),
    ];
    let findings = vec![finding(
        "lint",
        Severity::Warning,
        Some("src/app.ts"),
        Some(12),
    )];
    Report::from_run(context(true), checks, findings, None)
}

fn fail_report() -> Report {
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Fail, 38),
        entry("types", CheckKind::Types, CheckStatus::Pass, 38),
    ];
    let findings = vec![
        finding("lint", Severity::Error, Some("src/app.ts"), Some(1302)),
        finding("lint", Severity::Error, Some("src/util.ts"), Some(7)),
    ];
    Report::from_run(context(true), checks, findings, None)
}

const RISK_POLICY: &str = r#"
schema_version: 1
risk:
  classes:
    - { name: routine, approvals: 0 }
    - { name: core }
    - { name: guarded, author_counts: false }
  default: core
  match:
    - { paths: ["docs/**"], class: routine }
    - { paths: ["src/**"], class: guarded }
"#;

fn required(report: &Report) -> Vec<RequiredCheck> {
    report
        .checks
        .iter()
        .map(|c| RequiredCheck {
            check: c.name.clone(),
            system: 0,
            status: Some(c.status),
        })
        .collect()
}

/// The pass report with the class its files match.
fn risk_matched_report() -> Report {
    let policy = Config::parse(RISK_POLICY).unwrap().risk.unwrap();
    let touched = [
        Touched::path("src/app.ts"),
        Touched::renamed("src/util.ts", "lib/util.ts"),
        Touched::path("docs/a.md"),
    ];
    let mut report = pass_report();
    let risk = policy.report(policy.match_change(&touched));
    report.risk = Some(Some(risk.with_required(required(&report))));
    report
}

fn risk_none_report() -> Report {
    let mut report = pass_report();
    report.risk = Some(None);
    report
}

/// The fail report of a change whose merge base's policy is invalid.
fn risk_base_unreadable_report() -> Report {
    let mut report = fail_report();
    report.findings.insert(
        0,
        Finding {
            check: None,
            rule: "risk-unreadable".into(),
            message: "the change has no risk class, so a person must approve it: .garita/config.yaml at the merge base 1b29de5ea3f0 is not a valid config: risk.default: `high` is not a class in risk.classes; the classes are `routine`, `core`".into(),
            ..finding("lint", Severity::Info, None, None)
        },
    );
    report.risk = Some(Some(
        Risk::base_unreadable().with_required(required(&report)),
    ));
    report
}

fn not_run_report() -> Report {
    let context = RunContext {
        trigger: Some(Trigger::Commit),
        ..RunContext::default()
    };
    Report::not_run(context, RunError::environment("not a git repository"))
}

fn mixed_not_run_report() -> Report {
    let mut missing = entry("types", CheckKind::Types, CheckStatus::NotRun, 0);
    missing.rerun = None;
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Fail, 38),
        missing,
    ];
    let findings = vec![finding(
        "lint",
        Severity::Error,
        Some("src/app.ts"),
        Some(3),
    )];
    Report::from_run(context(false), checks, findings, None)
}

fn about_the_run_report() -> Report {
    let checks = vec![entry("lint", CheckKind::Lint, CheckStatus::Pass, 38)];
    let findings = vec![Finding {
        check: None,
        rule: "scope-widened".into(),
        message: "no base ref was found, so the scope is every file".into(),
        ..finding("lint", Severity::Info, None, None)
    }];
    Report::from_run(context(true), checks, findings, None)
}

fn no_location_report() -> Report {
    let checks = vec![entry("records", CheckKind::Records, CheckStatus::Fail, 4)];
    let findings = vec![finding("records", Severity::Error, None, None)];
    Report::from_run(context(true), checks, findings, None)
}

/// A failing exit-code check with its output on the check, and an unformatted file with a fix.
fn fail_output_report() -> Report {
    let records = CheckEntry {
        output_tail: Some("checked 3 records\nerror: 0004-x.md has no status\n".into()),
        output_notable: vec!["error: 0004-x.md has no status".into()],
        ..entry("records", CheckKind::Records, CheckStatus::Fail, 3)
    };
    let format = CheckEntry {
        output_tail: Some("src/[id]/page.tsx\n".into()),
        ..entry("format", CheckKind::Format, CheckStatus::Fail, 38)
    };
    let failed = garita_core::adapter::ToolFinding {
        severity: Severity::Error,
        rule: "exit-code/failed".into(),
        file: None,
        line: None,
        column: None,
        message: "exited with code 1".into(),
        fix: None,
    };
    let unformatted = garita_core::adapter::ToolFinding {
        rule: "oxfmt/unformatted".into(),
        file: Some("src/[id]/page.tsx".into()),
        message: "File is not formatted".into(),
        fix: Some(vec!["oxfmt".into(), "src/[id]/page.tsx".into()]),
        ..failed.clone()
    };
    let findings = vec![
        failed.into_finding("records"),
        unformatted.into_finding("format"),
    ];
    Report::from_run(context(true), vec![records, format], findings, None)
}

fn receipt_check(
    name: &str,
    kind: CheckKind,
    status: CheckStatus,
    code: Option<i32>,
) -> ReceiptCheck {
    ReceiptCheck {
        name: name.into(),
        kind,
        command: format!("{name} src/app.ts src/util.ts"),
        scope: scope(),
        tool_version: Some("1.2.3".into()),
        started_at: STARTED.into(),
        finished_at: FINISHED.into(),
        duration_ms: 412,
        exit_code: code,
        status,
        examined: [("files".to_string(), 38)].into(),
        output_tail: "2 errors".into(),
        output: "src/app.ts:1302:5 error\nsrc/util.ts:7:5 error\n2 errors".into(),
    }
}

fn receipt(result: ResultWord, checks: Vec<ReceiptCheck>, report: Report) -> Receipt {
    Receipt {
        receipt_version: RECEIPT_VERSION,
        run_id: RUN_ID.into(),
        started_at: STARTED.into(),
        finished_at: FINISHED.into(),
        trigger: Trigger::Push,
        commit: Some(COMMIT.parse().unwrap()),
        tree: TREE.parse().unwrap(),
        worktree: WORKTREE.parse().unwrap(),
        dirty: false,
        config_hash: "sha256:0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"
            .into(),
        machine: "build-host".into(),
        tool: Tool {
            name: "garita".into(),
            version: "0.1.0".into(),
        },
        result,
        checks,
        report,
    }
}

fn json<T: serde::Serialize>(value: T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

fn cases() -> Vec<(&'static str, serde_json::Value)> {
    let fail = fail_report();
    let fail_receipt = receipt(
        ResultWord::from(fail.status),
        vec![
            receipt_check("lint", CheckKind::Lint, CheckStatus::Fail, Some(1)),
            receipt_check("types", CheckKind::Types, CheckStatus::Pass, Some(0)),
        ],
        fail,
    );
    let mixed = mixed_not_run_report();
    let mixed_receipt = receipt(
        ResultWord::from(mixed.status),
        vec![
            receipt_check("lint", CheckKind::Lint, CheckStatus::Fail, Some(1)),
            receipt_check("types", CheckKind::Types, CheckStatus::NotRun, None),
        ],
        mixed,
    );
    let not_run = not_run_report();
    let not_run_receipt = receipt(ResultWord::from(not_run.status), Vec::new(), not_run);
    let risk = risk_matched_report();
    let risk_receipt = receipt(
        ResultWord::from(risk.status),
        vec![
            receipt_check("lint", CheckKind::Lint, CheckStatus::Pass, Some(0)),
            receipt_check("format", CheckKind::Format, CheckStatus::Pass, Some(0)),
        ],
        risk,
    );
    vec![
        ("run-report.risk-matched", json(risk_matched_report())),
        ("run-report.risk-none", json(risk_none_report())),
        (
            "run-report.risk-base-unreadable",
            json(risk_base_unreadable_report()),
        ),
        ("receipt.risk", json(risk_receipt)),
        ("run-report.pass", json(pass_report())),
        ("run-report.warn", json(warn_report())),
        ("run-report.fail", json(fail_report())),
        (
            "run-report.finding-about-the-run",
            json(about_the_run_report()),
        ),
        ("run-report.not-run-no-start", json(not_run_report())),
        ("run-report.not-run-mixed", json(mixed_not_run_report())),
        ("run-report.finding-no-location", json(no_location_report())),
        ("run-report.fail-output", json(fail_output_report())),
        ("describe.garita", json(describe("0.0.0"))),
        ("receipt.fail", json(fail_receipt)),
        ("receipt.not-run-mixed", json(mixed_receipt)),
        ("receipt.not-run", json(not_run_receipt)),
    ]
}

/// Goldens that the garita-cli tests write and compare, from the real command output.
const WRITTEN_BY_THE_CLI: &[&str] = &[
    "run-report.doctor-not-run.json",
    "run-report.doctor-pass.json",
    "run-report.hooks-install.json",
    "run-report.hooks-uninstall.json",
    "run-report.hook-missing-garita.json",
    "run-report.usage-error.json",
];

#[test]
fn examples_match_what_the_types_serialize() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/contract/examples");
    // GARITA_UPDATE_GOLDEN=1 rewrites the goldens instead of comparing, so a contract change is reviewed as a diff.
    let update = std::env::var("GARITA_UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    if update {
        fs::create_dir_all(&dir).unwrap();
    }
    let mut produced = BTreeSet::new();
    for (name, value) in cases() {
        let file = format!("{name}.json");
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
        let path = dir.join(&file);
        if update {
            fs::write(&path, &text).unwrap();
        } else {
            let golden = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("golden {file} is missing: {e}"));
            assert_eq!(golden, text, "golden {file} differs");
        }
        produced.insert(file);
    }
    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| !WRITTEN_BY_THE_CLI.contains(&name.as_str()))
        .collect();
    if update {
        for stale in on_disk.difference(&produced) {
            fs::remove_file(dir.join(stale)).unwrap();
        }
    } else {
        assert_eq!(
            on_disk, produced,
            "examples/ holds files the test does not produce"
        );
    }
}

#[test]
fn a_receipt_and_its_report_agree_on_every_check() {
    let statuses = |checks: &serde_json::Value| -> Vec<(String, String)> {
        checks
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["name"].to_string(), c["status"].to_string()))
            .collect()
    };
    for (name, value) in cases() {
        if name.starts_with("receipt.") {
            assert_eq!(
                statuses(&value["checks"]),
                statuses(&value["report"]["checks"]),
                "{name}"
            );
        }
    }
}

#[test]
fn a_finding_names_a_check_that_the_report_lists() {
    let mut seen = 0;
    for (name, value) in cases() {
        let report = if name.starts_with("receipt.") {
            &value["report"]
        } else if name.starts_with("run-report.") {
            &value
        } else {
            continue;
        };
        let names: Vec<&serde_json::Value> = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| &c["name"])
            .collect();
        for f in report["findings"].as_array().unwrap() {
            if let Some(check) = f.get("check") {
                seen += 1;
                assert!(names.contains(&check), "{name}: {f}");
            }
        }
    }
    assert!(seen > 0);
}

/// Whether garita wrote a finding's message itself, rather than passing on a tool's diagnostic.
fn garitas_own(rule: &str) -> bool {
    !rule.contains('/')
        || rule.starts_with("doctor/")
        || ["/unrecognized-output", "/failed", "/task-failed"]
            .iter()
            .any(|s| rule.ends_with(s))
}

#[test]
fn no_golden_error_or_garita_message_has_a_newline() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/contract/examples");
    let mut seen = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let report = value.get("report").unwrap_or(&value);
        if let Some(error) = report.get("error").and_then(|e| e.as_str()) {
            seen += 1;
            assert!(!error.contains('\n'), "{}: {error}", path.display());
        }
        for f in report["findings"].as_array().into_iter().flatten() {
            let rule = f["rule"].as_str().unwrap_or_default();
            let message = f["message"].as_str().unwrap_or_default();
            assert!(
                !garitas_own(rule) || !message.contains('\n'),
                "{}: {rule}: {message}",
                path.display()
            );
        }
    }
    assert!(seen > 0);
}

#[test]
fn describe_declares_the_vendored_contract_version() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/contract/VERSION");
    let text = fs::read_to_string(path).unwrap();
    let tag = text.split_whitespace().next().unwrap();
    let release = tag.trim_start_matches('v');
    let mut parts = release.split('.');
    let major_minor = format!("{}.{}", parts.next().unwrap(), parts.next().unwrap());
    assert_eq!(CONTRACT_VERSION, major_minor);
}
